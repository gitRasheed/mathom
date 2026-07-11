//! Windows volume I/O and sector-aligned $MFT reads — the only module that
//! touches the disk; all `unsafe` in the crate lives here.

use std::alloc::{Layout, alloc_zeroed, dealloc};
use std::path::Path;

use windows::Win32::Foundation::{ERROR_IO_PENDING, ERROR_MORE_DATA, HANDLE, WAIT_OBJECT_0};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_FLAG_NO_BUFFERING, FILE_FLAG_OVERLAPPED, FILE_FLAG_SEQUENTIAL_SCAN,
    FILE_READ_ATTRIBUTES, FILE_READ_DATA, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
    GetVolumeInformationW, GetVolumeNameForVolumeMountPointW, GetVolumePathNameW, OPEN_EXISTING,
    ReadFile, SYNCHRONIZE,
};
use windows::Win32::System::IO::{CancelIoEx, DeviceIoControl, GetOverlappedResult, OVERLAPPED};
use windows::Win32::System::Ioctl::{
    FSCTL_GET_RETRIEVAL_POINTERS, GET_LENGTH_INFORMATION, IOCTL_DISK_GET_LENGTH_INFO,
    RETRIEVAL_POINTERS_BUFFER, RETRIEVAL_POINTERS_BUFFER_0, STARTING_VCN_INPUT_BUFFER,
};
use windows::Win32::System::Threading::{
    CreateEventW, INFINITE, ResetEvent, WaitForMultipleObjects,
};
use windows::core::{Owned, PCWSTR};

use crate::boot::{Geometry, geometry_fits_device, parse_boot_sector, plan_mft_read};
use crate::record::parse_record0;
use crate::runs::Extent;

/// A 4 KiB-aligned heap buffer, as `FILE_FLAG_NO_BUFFERING` demands.
pub struct AlignedBuf {
    ptr: *mut u8,
    len: usize,
}

// SAFETY: exclusive ownership of a raw allocation; nothing thread-affine.
unsafe impl Send for AlignedBuf {}

impl AlignedBuf {
    pub fn new(len: usize) -> Self {
        assert!(len > 0 && len.is_multiple_of(4096));
        let layout = Layout::from_size_align(len, 4096).expect("valid buffer layout");
        // SAFETY: non-zero size, valid layout; null checked below.
        let ptr = unsafe { alloc_zeroed(layout) };
        assert!(!ptr.is_null(), "aligned buffer allocation failed");
        AlignedBuf { ptr, len }
    }

    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        // SAFETY: we own `ptr` for `len` bytes, exclusively borrowed here.
        unsafe { std::slice::from_raw_parts_mut(self.ptr, self.len) }
    }
}

impl Drop for AlignedBuf {
    fn drop(&mut self) {
        let layout = Layout::from_size_align(self.len, 4096).expect("valid buffer layout");
        // SAFETY: allocated with exactly this layout in `new`.
        unsafe { dealloc(self.ptr, layout) };
    }
}

pub struct VolumeLocation {
    pub mount: String,
    pub components: Vec<String>,
}

pub fn locate(root: &Path) -> Result<VolumeLocation, String> {
    let canonical = std::fs::canonicalize(root).map_err(|e| format!("{}: {e}", root.display()))?;
    let wide = to_wide(canonical.as_os_str());
    let mut buf = vec![0u16; wide.len() + 64];
    // SAFETY: both buffers are valid for the call; wide is NUL-terminated.
    unsafe { GetVolumePathNameW(PCWSTR(wide.as_ptr()), &mut buf) }
        .map_err(|e| format!("volume of {}: {e}", canonical.display()))?;
    let mount = from_wide(&buf);

    let rel = canonical
        .strip_prefix(&mount)
        .map_err(|_| "scan root escapes its own volume".to_string())?;
    let components = rel
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect();
    Ok(VolumeLocation { mount, components })
}

pub fn is_ntfs(mount: &str) -> bool {
    let wide = to_wide(mount.as_ref());
    let mut fs_name = [0u16; 64];
    // SAFETY: valid NUL-terminated root path and output buffer.
    unsafe {
        GetVolumeInformationW(
            PCWSTR(wide.as_ptr()),
            None,
            None,
            None,
            None,
            Some(&mut fs_name),
        )
    }
    .is_ok()
        && from_wide(&fs_name) == "NTFS"
}

pub struct Volume {
    handle: Owned<windows::Win32::Foundation::HANDLE>,
}

// SAFETY: the handle is used from the reader thread only after construction.
unsafe impl Send for Volume {}

impl Volume {
    /// Opens the raw volume backing `mount`. Fails with access-denied when
    /// not elevated — that failure is the probe's fallback signal.
    pub fn open(mount: &str) -> Result<Volume, String> {
        let mount_wide = to_wide(mount.as_ref());
        let mut guid = [0u16; 64];
        // SAFETY: valid NUL-terminated mount path and output buffer.
        unsafe { GetVolumeNameForVolumeMountPointW(PCWSTR(mount_wide.as_ptr()), &mut guid) }
            .map_err(|e| format!("volume name for {mount}: {e}"))?;
        let mut device = from_wide(&guid);
        // CreateFileW wants the volume path without the trailing slash.
        while device.ends_with('\\') {
            device.pop();
        }
        let device_wide = to_wide(device.as_ref());

        // SAFETY: all pointers are valid for the duration of the call.
        let handle = unsafe {
            CreateFileW(
                PCWSTR(device_wide.as_ptr()),
                (FILE_READ_DATA | FILE_READ_ATTRIBUTES | SYNCHRONIZE).0,
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                None,
                OPEN_EXISTING,
                FILE_FLAG_NO_BUFFERING | FILE_FLAG_SEQUENTIAL_SCAN | FILE_FLAG_OVERLAPPED,
                None,
            )
        }
        .map_err(|e| format!("open volume {device}: {e}"))?;
        // SAFETY: we own the fresh handle; Owned closes it on drop.
        Ok(Volume {
            handle: unsafe { Owned::new(handle) },
        })
    }

    /// Synchronous-style read on the (overlapped) handle: submit, then wait
    /// on a private event. Setup reads only — the sweep uses [`ReadRing`].
    pub fn read_at(&self, offset: u64, buf: &mut [u8]) -> Result<(), String> {
        let len = buf.len();
        let fail = |e: windows::core::Error| format!("read {len} bytes at {offset}: {e}");
        // SAFETY: fresh event, owned; buffer and overlapped outlive the
        // bWait=true completion below.
        unsafe {
            let event = Owned::new(CreateEventW(None, true, false, None).map_err(fail)?);
            let mut overlapped = OVERLAPPED {
                hEvent: *event,
                ..Default::default()
            };
            overlapped.Anonymous.Anonymous.Offset = offset as u32;
            overlapped.Anonymous.Anonymous.OffsetHigh = (offset >> 32) as u32;
            match ReadFile(*self.handle, Some(buf), None, Some(&mut overlapped)) {
                Ok(()) => {}
                Err(e) if e.code() == ERROR_IO_PENDING.to_hresult() => {}
                Err(e) => return Err(fail(e)),
            }
            let mut read = 0u32;
            GetOverlappedResult(*self.handle, &overlapped, &mut read, true).map_err(fail)?;
            if read as usize != len {
                return Err(format!(
                    "short volume read at {offset}: {read} of {len} bytes"
                ));
            }
        }
        Ok(())
    }

    /// The volume's real byte length from the driver — the ground truth
    /// every disk-supplied size is validated against.
    pub fn length(&self) -> Result<u64, String> {
        let mut info = GET_LENGTH_INFORMATION::default();
        let mut written = 0u32;
        let fail = |e: windows::core::Error| format!("volume length: {e}");
        // SAFETY: out-pointer and OVERLAPPED are valid through the waited
        // completion. The overlapped handle makes the OVERLAPPED mandatory
        // even for this normally-synchronous ioctl.
        unsafe {
            let event = Owned::new(CreateEventW(None, true, false, None).map_err(fail)?);
            let mut overlapped = OVERLAPPED {
                hEvent: *event,
                ..Default::default()
            };
            match DeviceIoControl(
                *self.handle,
                IOCTL_DISK_GET_LENGTH_INFO,
                None,
                0,
                Some(&mut info as *mut _ as *mut _),
                size_of::<GET_LENGTH_INFORMATION>() as u32,
                Some(&mut written),
                Some(&mut overlapped),
            ) {
                Ok(()) => {}
                Err(e) if e.code() == ERROR_IO_PENDING.to_hresult() => {
                    let mut got = 0u32;
                    GetOverlappedResult(*self.handle, &overlapped, &mut got, true).map_err(fail)?;
                }
                Err(e) => return Err(fail(e)),
            }
        }
        u64::try_from(info.Length).map_err(|_| "volume reports a negative length".into())
    }

    /// $MFT extents via `FSCTL_GET_RETRIEVAL_POINTERS` — the fallback when
    /// record 0's run list is incomplete, and the debug cross-check for it.
    pub fn mft_extents_via_fsctl(mount: &str) -> Result<Vec<Extent>, String> {
        let path = format!("{mount}$MFT::$DATA");
        let wide = to_wide(path.as_ref());
        // SAFETY: valid path; FILE_READ_ATTRIBUTES needs no data access.
        let handle = unsafe {
            CreateFileW(
                PCWSTR(wide.as_ptr()),
                FILE_READ_ATTRIBUTES.0,
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                None,
                OPEN_EXISTING,
                Default::default(),
                None,
            )
        }
        .map_err(|e| format!("open {path}: {e}"))?;
        // SAFETY: fresh handle, closed on drop.
        let handle = unsafe { Owned::new(handle) };

        let input = STARTING_VCN_INPUT_BUFFER::default();
        // u64-backed so the RETRIEVAL_POINTERS_BUFFER view is aligned.
        let mut out = vec![0u64; 8 * 1024];
        let mut written = 0u32;
        loop {
            // SAFETY: in/out buffers valid and sized for the call.
            let res = unsafe {
                DeviceIoControl(
                    *handle,
                    FSCTL_GET_RETRIEVAL_POINTERS,
                    Some(&input as *const _ as *const _),
                    size_of::<STARTING_VCN_INPUT_BUFFER>() as u32,
                    Some(out.as_mut_ptr() as *mut _),
                    (out.len() * size_of::<u64>()) as u32,
                    Some(&mut written),
                    None,
                )
            };
            match res {
                Ok(()) => break,
                Err(e) if e.code() == ERROR_MORE_DATA.into() => out.resize(out.len() * 2, 0),
                Err(e) => return Err(format!("retrieval pointers for {path}: {e}")),
            }
        }

        // SAFETY: the buffer is 8-aligned (u64-backed), zero-initialized,
        // and at least header-sized; the field reads below are bounds-checked
        // against `written` before anything past the header is trusted.
        let header = unsafe { &*(out.as_ptr() as *const RETRIEVAL_POINTERS_BUFFER) };
        let count = header.ExtentCount as usize;
        let extents_off = std::mem::offset_of!(RETRIEVAL_POINTERS_BUFFER, Extents);
        let needed = count
            .checked_mul(size_of::<RETRIEVAL_POINTERS_BUFFER_0>())
            .and_then(|n| n.checked_add(extents_off));
        if needed.is_none_or(|n| n > written as usize) {
            return Err(format!("retrieval pointers for {path}: truncated reply"));
        }
        let mut extents = Vec::with_capacity(count);
        let mut vcn = header.StartingVcn;
        for i in 0..count {
            // SAFETY: `count` entries proven to lie within the kernel-written
            // `written` bytes above.
            let e = unsafe { &*header.Extents.as_ptr().add(i) };
            let next = e.NextVcn;
            let lcn = e.Lcn;
            if lcn < 0 || next <= vcn {
                return Err("unexpected virtual extent in $MFT".into());
            }
            extents.push(Extent {
                lcn: lcn as u64,
                clusters: (next - vcn) as u64,
            });
            vcn = next;
        }
        Ok(extents)
    }
}

pub struct MftMap {
    pub geometry: Geometry,
    pub extents: Vec<Extent>,
    pub mft_bytes: u64,
    pub total_records: u32,
}

pub fn map_mft(volume: &Volume, mount: &str) -> Result<MftMap, String> {
    let device_bytes = volume.length()?;
    let mut boot = AlignedBuf::new(4096);
    volume.read_at(0, boot.as_mut_slice())?;
    let geometry = parse_boot_sector(boot.as_mut_slice()).map_err(|e| e.to_string())?;
    // Nothing may be sized or offset from the boot sector's totals until
    // they're proven to fit the device (record 0's offset included).
    geometry_fits_device(&geometry, device_bytes).map_err(|e| e.to_string())?;
    if !(geometry.cluster_size as u64).is_multiple_of(geometry.record_size as u64) {
        return Err("unsupported geometry: cluster smaller than FILE record".into());
    }

    let mut rec0 = AlignedBuf::new(4096);
    volume.read_at(geometry.mft_byte_offset(), rec0.as_mut_slice())?;
    let rec0_slice = &mut rec0.as_mut_slice()[..geometry.record_size as usize];
    let (extents, data_size) = match parse_record0(rec0_slice) {
        Ok(m) => {
            #[cfg(debug_assertions)]
            if let Ok(fsctl) = Volume::mft_extents_via_fsctl(mount) {
                debug_assert_eq!(m.extents, fsctl, "record-0 runs vs FSCTL disagree");
            }
            (m.extents, m.data_size)
        }
        Err(crate::ParseError("$MFT runs incomplete in record 0")) => {
            // No real $DATA size known: u64::MAX = read all covered clusters.
            (Volume::mft_extents_via_fsctl(mount)?, u64::MAX)
        }
        Err(e) => return Err(e.to_string()),
    };

    let plan =
        plan_mft_read(&geometry, &extents, data_size, device_bytes).map_err(|e| e.to_string())?;
    Ok(MftMap {
        geometry,
        extents,
        mft_bytes: plan.mft_bytes,
        total_records: plan.total_records,
    })
}

/// A fixed set of in-flight overlapped volume reads (MFT speed round 2:
/// QD>1 keeps the NVMe queue full; see BENCHMARKS.md 2026-07-11 for the
/// matrix that picked the depth). Buffers move in at `submit` and come
/// back only from `wait_any` once the kernel is done writing them; drop
/// cancels and waits out stragglers so no buffer is ever freed under an
/// active read.
pub struct ReadRing<'v> {
    volume: &'v Volume,
    events: Vec<Owned<HANDLE>>,
    ovs: Vec<OVERLAPPED>, // allocated once — stable addresses while in flight
    slots: Vec<Option<(AlignedBuf, usize)>>,
}

impl<'v> ReadRing<'v> {
    pub fn new(volume: &'v Volume, depth: usize) -> Result<ReadRing<'v>, String> {
        assert!(
            (1..=64).contains(&depth),
            "depth bounded by WaitForMultipleObjects"
        );
        let mut events = Vec::with_capacity(depth);
        for _ in 0..depth {
            // SAFETY: fresh manual-reset event; Owned closes it on drop.
            unsafe {
                let e = CreateEventW(None, true, false, None)
                    .map_err(|e| format!("create read event: {e}"))?;
                events.push(Owned::new(e));
            }
        }
        let mut ovs = vec![OVERLAPPED::default(); depth];
        for (ov, event) in ovs.iter_mut().zip(&events) {
            ov.hEvent = **event;
        }
        Ok(ReadRing {
            volume,
            events,
            ovs,
            slots: (0..depth).map(|_| None).collect(),
        })
    }

    /// Starts a read of `len` bytes at `offset` into `buf`, which the ring
    /// owns until [`Self::wait_any`] hands it back completed.
    pub fn submit(
        &mut self,
        slot: usize,
        offset: u64,
        len: usize,
        mut buf: AlignedBuf,
    ) -> Result<(), String> {
        assert!(self.slots[slot].is_none(), "slot already in flight");
        assert!(len <= buf.as_mut_slice().len());
        let ov = &mut self.ovs[slot];
        ov.Anonymous.Anonymous.Offset = offset as u32;
        ov.Anonymous.Anonymous.OffsetHigh = (offset >> 32) as u32;
        // SAFETY: event is reset before reuse; buffer and OVERLAPPED live in
        // per-slot storage until the completion is consumed (or Drop waits).
        unsafe {
            ResetEvent(*self.events[slot]).map_err(|e| format!("reset read event: {e}"))?;
            match ReadFile(
                *self.volume.handle,
                Some(&mut buf.as_mut_slice()[..len]),
                None,
                Some(ov),
            ) {
                Ok(()) => {} // completed synchronously; the event is set
                Err(e) if e.code() == ERROR_IO_PENDING.to_hresult() => {}
                Err(e) => return Err(format!("read {len} bytes at {offset}: {e}")),
            }
        }
        self.slots[slot] = Some((buf, len));
        Ok(())
    }

    /// Blocks until any in-flight read completes; returns its slot and the
    /// filled buffer. At least one read must be in flight.
    pub fn wait_any(&mut self) -> Result<(usize, AlignedBuf), String> {
        let busy: Vec<usize> = (0..self.slots.len())
            .filter(|&i| self.slots[i].is_some())
            .collect();
        assert!(!busy.is_empty(), "wait_any with nothing in flight");
        let handles: Vec<HANDLE> = busy.iter().map(|&i| *self.events[i]).collect();
        // SAFETY: live event handles owned by self.
        let wait = unsafe { WaitForMultipleObjects(&handles, false, INFINITE) };
        let idx = wait.0.wrapping_sub(WAIT_OBJECT_0.0) as usize;
        if idx >= busy.len() {
            return Err(format!("wait on volume reads failed: {wait:?}"));
        }
        let slot = busy[idx];
        // The completion has signalled, so the kernel is done with the
        // buffer either way — reclaim it before checking the result.
        let (buf, want) = self.slots[slot].take().expect("busy slot holds a buffer");
        let mut got = 0u32;
        // SAFETY: this slot's op has completed; its OVERLAPPED is live.
        unsafe { GetOverlappedResult(*self.volume.handle, &self.ovs[slot], &mut got, false) }
            .map_err(|e| format!("volume read failed: {e}"))?;
        if got as usize != want {
            return Err(format!("short volume read: {got} of {want} bytes"));
        }
        Ok((slot, buf))
    }
}

impl Drop for ReadRing<'_> {
    fn drop(&mut self) {
        if self.slots.iter().all(Option::is_none) {
            return;
        }
        // SAFETY: cancelling this handle's IO, then waiting each in-flight
        // slot's completion — after this no kernel write can touch the
        // buffers we're about to free.
        unsafe {
            let _ = CancelIoEx(*self.volume.handle, None);
            for (i, slot) in self.slots.iter().enumerate() {
                if slot.is_some() {
                    let mut got = 0u32;
                    let _ = GetOverlappedResult(*self.volume.handle, &self.ovs[i], &mut got, true);
                }
            }
        }
    }
}

fn to_wide(s: &std::ffi::OsStr) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    s.encode_wide().chain(std::iter::once(0)).collect()
}

fn from_wide(buf: &[u16]) -> String {
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..end])
}
