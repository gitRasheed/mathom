//! Windows volume I/O: open the raw volume, locate the $MFT, and stream it
//! in sector-aligned buffers. The only module that touches the disk — all
//! `unsafe` in the crate lives here (aligned allocation + Win32 calls),
//! wrapped so the rest speaks safe types.

use std::alloc::{Layout, alloc_zeroed, dealloc};
use std::path::Path;

use windows::Win32::Foundation::ERROR_MORE_DATA;
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_FLAG_NO_BUFFERING, FILE_FLAG_SEQUENTIAL_SCAN, FILE_READ_ATTRIBUTES,
    FILE_READ_DATA, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, GetVolumeInformationW,
    GetVolumeNameForVolumeMountPointW, GetVolumePathNameW, OPEN_EXISTING, ReadFile, SYNCHRONIZE,
};
use windows::Win32::System::IO::{DeviceIoControl, OVERLAPPED};
use windows::Win32::System::Ioctl::{
    FSCTL_GET_RETRIEVAL_POINTERS, RETRIEVAL_POINTERS_BUFFER, STARTING_VCN_INPUT_BUFFER,
};
use windows::core::{Owned, PCWSTR};

use crate::boot::{Geometry, parse_boot_sector};
use crate::record::parse_record0;
use crate::runs::Extent;

/// A 4 KiB-aligned heap buffer, as `FILE_FLAG_NO_BUFFERING` demands.
pub struct AlignedBuf {
    ptr: *mut u8,
    len: usize,
}

// Exclusive ownership of a raw allocation; nothing thread-affine about it.
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

/// Where a scan root lives: its volume (as a mount-point path like `C:\` or
/// a folder mount) and the path components below the volume root.
pub struct VolumeLocation {
    pub mount: String,
    pub components: Vec<String>,
}

/// Resolves a scan root to its volume mount point + relative components.
/// Symlinks/junctions in the path are resolved first so the MFT tree walk
/// sees real names.
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

/// True when the mounted filesystem is NTFS.
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

/// An open raw-volume handle (needs elevation).
pub struct Volume {
    handle: Owned<windows::Win32::Foundation::HANDLE>,
}

// The handle is used from the reader thread only after construction.
unsafe impl Send for Volume {}

impl Volume {
    /// Opens the volume backing `mount` (e.g. `C:\` → `\\?\Volume{…}`).
    /// Fails with access-denied when not elevated — the probe signal.
    pub fn open(mount: &str) -> Result<Volume, String> {
        let mount_wide = to_wide(mount.as_ref());
        let mut guid = [0u16; 64];
        // SAFETY: valid NUL-terminated mount path and output buffer.
        unsafe { GetVolumeNameForVolumeMountPointW(PCWSTR(mount_wide.as_ptr()), &mut guid) }
            .map_err(|e| format!("volume name for {mount}: {e}"))?;
        // CreateFileW wants the volume path *without* the trailing slash.
        let mut device = from_wide(&guid);
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
                FILE_FLAG_NO_BUFFERING | FILE_FLAG_SEQUENTIAL_SCAN,
                None,
            )
        }
        .map_err(|e| format!("open volume {device}: {e}"))?;
        // SAFETY: we own the fresh handle; Owned closes it on drop.
        Ok(Volume {
            handle: unsafe { Owned::new(handle) },
        })
    }

    /// Positioned synchronous read. Offset, length, and the buffer address
    /// all honor the no-buffering alignment rules by construction.
    pub fn read_at(&self, offset: u64, buf: &mut [u8]) -> Result<(), String> {
        let mut overlapped = OVERLAPPED::default();
        overlapped.Anonymous.Anonymous.Offset = offset as u32;
        overlapped.Anonymous.Anonymous.OffsetHigh = (offset >> 32) as u32;
        let mut read = 0u32;
        // SAFETY: buffer and overlapped outlive the synchronous call.
        unsafe {
            ReadFile(
                *self.handle,
                Some(buf),
                Some(&mut read),
                Some(&mut overlapped),
            )
        }
        .map_err(|e| format!("read {} bytes at {offset}: {e}", buf.len()))?;
        if read as usize != buf.len() {
            return Err(format!(
                "short volume read at {offset}: {read} of {} bytes",
                buf.len()
            ));
        }
        Ok(())
    }

    /// The $MFT's extents via `FSCTL_GET_RETRIEVAL_POINTERS` on the given
    /// handle-able path — the fallback when record 0's run list is
    /// incomplete, and the debug cross-check for the parsed one.
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
        let mut out = vec![0u8; 64 * 1024];
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
                    out.len() as u32,
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

        // SAFETY: the kernel filled a RETRIEVAL_POINTERS_BUFFER header.
        let header = unsafe { &*(out.as_ptr() as *const RETRIEVAL_POINTERS_BUFFER) };
        let count = header.ExtentCount as usize;
        let mut extents = Vec::with_capacity(count);
        let mut vcn = header.StartingVcn;
        for i in 0..count {
            // SAFETY: Extents is a flexible array; the kernel wrote `count`
            // entries, each 16 bytes, within `written` bytes.
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

/// Everything needed to read the whole $MFT.
pub struct MftMap {
    pub geometry: Geometry,
    pub extents: Vec<Extent>,
    /// Total record bytes (the $MFT $DATA real size).
    pub mft_bytes: u64,
}

/// Reads the boot sector + record 0 and produces the read plan. `mount` is
/// used for the FSCTL fallback when record 0's runs are incomplete.
pub fn map_mft(volume: &Volume, mount: &str) -> Result<MftMap, String> {
    let mut boot = AlignedBuf::new(4096);
    volume.read_at(0, boot.as_mut_slice())?;
    let geometry = parse_boot_sector(boot.as_mut_slice()).map_err(|e| e.to_string())?;
    if !(geometry.cluster_size as u64).is_multiple_of(geometry.record_size as u64) {
        // Extents would not be record-aligned (sub-1KiB clusters). Rare
        // enough that falling back to the walker beats complicating reads.
        return Err("unsupported geometry: cluster smaller than FILE record".into());
    }

    let mut rec0 = AlignedBuf::new(4096);
    volume.read_at(geometry.mft_byte_offset(), rec0.as_mut_slice())?;
    let rec0_slice = &mut rec0.as_mut_slice()[..geometry.record_size as usize];
    let (extents, mft_bytes) = match parse_record0(rec0_slice) {
        Ok(m) => {
            #[cfg(debug_assertions)]
            if let Ok(fsctl) = Volume::mft_extents_via_fsctl(mount) {
                debug_assert_eq!(m.extents, fsctl, "record-0 runs vs FSCTL disagree");
            }
            (m.extents, m.data_size)
        }
        Err(crate::ParseError("$MFT runs incomplete in record 0")) => {
            // Heavily fragmented $MFT: let the filesystem enumerate it.
            let extents = Volume::mft_extents_via_fsctl(mount)?;
            let covered: u64 = extents.iter().map(|e| e.clusters).sum();
            (extents, covered * geometry.cluster_size as u64)
        }
        Err(e) => return Err(e.to_string()),
    };

    let covered: u64 = extents.iter().map(|e| e.clusters).sum();
    if covered * geometry.cluster_size as u64 == 0 {
        return Err("$MFT extent map is empty".into());
    }
    let mft_bytes = mft_bytes.min(covered * geometry.cluster_size as u64);
    Ok(MftMap {
        geometry,
        extents,
        mft_bytes,
    })
}

fn to_wide(s: &std::ffi::OsStr) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    s.encode_wide().chain(std::iter::once(0)).collect()
}

fn from_wide(buf: &[u16]) -> String {
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..end])
}
