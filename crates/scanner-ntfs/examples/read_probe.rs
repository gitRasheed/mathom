//! Drive-ceiling probe for MFT speed round 2: sequential unbuffered volume
//! reads at queue depth × block size. Each combo reads its own fresh span so
//! nothing is measured twice. The matrix answers "how far below the drive's
//! ceiling is the scanner's synchronous QD1 reader?" before committing to an
//! overlapped rewrite.
//!
//! Run from an elevated terminal:
//! `cargo run --release -p mathom-scanner-ntfs --features mft-backend --example read_probe -- C:`

#[cfg(all(windows, feature = "mft-backend"))]
fn main() {
    if let Err(e) = probe::run() {
        eprintln!("{e}");
        std::process::exit(1);
    }
}

#[cfg(all(windows, feature = "mft-backend"))]
mod probe {
    use std::io::Write as _;
    use std::time::Instant;

    use mathom_scanner_ntfs::volume::AlignedBuf;
    use windows::Win32::Foundation::{ERROR_IO_PENDING, HANDLE, WAIT_OBJECT_0};
    use windows::Win32::Storage::FileSystem::{
        CreateFileW, FILE_FLAG_NO_BUFFERING, FILE_FLAG_OVERLAPPED, FILE_READ_ATTRIBUTES,
        FILE_READ_DATA, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
        ReadFile, SYNCHRONIZE,
    };
    use windows::Win32::System::IO::{GetOverlappedResult, OVERLAPPED};
    use windows::Win32::System::Threading::{
        CreateEventW, INFINITE, ResetEvent, WaitForMultipleObjects,
    };
    use windows::core::{Owned, PCWSTR};

    const QDS: [usize; 5] = [1, 2, 4, 8, 16];
    const BLOCKS: [usize; 3] = [256 * 1024, 1024 * 1024, 4 * 1024 * 1024];
    const SPAN: u64 = 2 << 30; // bytes per combo, capped by TIME_CAP
    const START: u64 = 1 << 30; // skip the volume's first GiB
    const TIME_CAP: f64 = 1.5; // seconds per combo

    pub fn run() -> Result<(), String> {
        let root = std::env::args().nth(1).unwrap_or_else(|| "C:".into());
        let device = format!(r"\\.\{}", root.trim_end_matches(['\\', '/']));
        let handle = open_overlapped(&device)?;

        let mut bufs: Vec<AlignedBuf> = (0..QDS[QDS.len() - 1])
            .map(|_| AlignedBuf::new(BLOCKS[BLOCKS.len() - 1]))
            .collect();

        println!(
            "{device}: sequential read GB/s, {} GiB or {TIME_CAP}s per cell",
            SPAN >> 30
        );
        print!("{:>8}", "block\\qd");
        for qd in QDS {
            print!("{qd:>8}");
        }
        println!();

        let mut combo = 0u64;
        for block in BLOCKS {
            print!("{:>8}", format!("{}K", block / 1024));
            for qd in QDS {
                let start = START + combo * SPAN;
                combo += 1;
                let gbps = run_combo(*handle, start, block, qd, &mut bufs)?;
                print!("{gbps:>8.2}");
                let _ = std::io::stdout().flush();
            }
            println!();
        }
        println!("\nscanner baseline 2026-07-08: 0.90 GB/s (synchronous reads, QD1)");
        Ok(())
    }

    fn open_overlapped(device: &str) -> Result<Owned<HANDLE>, String> {
        let wide: Vec<u16> = device.encode_utf16().chain(Some(0)).collect();
        // SAFETY: NUL-terminated path; handle ownership passes to Owned.
        unsafe {
            let handle = CreateFileW(
                PCWSTR(wide.as_ptr()),
                (FILE_READ_DATA | FILE_READ_ATTRIBUTES | SYNCHRONIZE).0,
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                None,
                OPEN_EXISTING,
                FILE_FLAG_NO_BUFFERING | FILE_FLAG_OVERLAPPED,
                None,
            )
            .map_err(|e| format!("open {device}: {e} — run from an elevated terminal"))?;
            Ok(Owned::new(handle))
        }
    }

    fn run_combo(
        handle: HANDLE,
        start: u64,
        block: usize,
        qd: usize,
        bufs: &mut [AlignedBuf],
    ) -> Result<f64, String> {
        let mut events: Vec<Owned<HANDLE>> = Vec::with_capacity(qd);
        for _ in 0..qd {
            // SAFETY: fresh event handle, owned for the combo's duration.
            unsafe {
                let e = CreateEventW(None, true, false, None)
                    .map_err(|e| format!("create event: {e}"))?;
                events.push(Owned::new(e));
            }
        }
        let mut ovs = vec![OVERLAPPED::default(); qd];
        for (ov, event) in ovs.iter_mut().zip(&events) {
            ov.hEvent = **event;
        }

        let end = start + SPAN;
        let mut next = start;
        let mut total = 0u64;
        let started = Instant::now();

        for slot in 0..qd {
            submit(
                handle,
                &mut bufs[slot].as_mut_slice()[..block],
                &mut ovs[slot],
                next,
            )?;
            next += block as u64;
        }

        let mut active: Vec<usize> = (0..qd).collect();
        while !active.is_empty() {
            let handles: Vec<HANDLE> = active.iter().map(|&s| *events[s]).collect();
            // SAFETY: all waited handles are live events owned above.
            let w = unsafe { WaitForMultipleObjects(&handles, false, INFINITE) };
            let i = (w.0.wrapping_sub(WAIT_OBJECT_0.0)) as usize;
            if i >= active.len() {
                return Err(format!("wait failed: {w:?}"));
            }
            let slot = active[i];

            let mut bytes = 0u32;
            // SAFETY: the overlapped op for this slot has signalled completion.
            unsafe { GetOverlappedResult(handle, &ovs[slot], &mut bytes, false) }
                .map_err(|e| format!("read result: {e}"))?;
            if bytes as usize != block {
                return Err(format!("short read: {bytes} of {block} bytes"));
            }
            total += u64::from(bytes);

            if next < end && started.elapsed().as_secs_f64() < TIME_CAP {
                // SAFETY: resetting an event this combo owns.
                unsafe { ResetEvent(*events[slot]) }.map_err(|e| format!("reset event: {e}"))?;
                submit(
                    handle,
                    &mut bufs[slot].as_mut_slice()[..block],
                    &mut ovs[slot],
                    next,
                )?;
                next += block as u64;
            } else {
                active.remove(i);
            }
        }

        Ok(total as f64 / started.elapsed().as_secs_f64() / 1e9)
    }

    fn submit(
        handle: HANDLE,
        buf: &mut [u8],
        ov: &mut OVERLAPPED,
        offset: u64,
    ) -> Result<(), String> {
        ov.Anonymous.Anonymous.Offset = offset as u32;
        ov.Anonymous.Anonymous.OffsetHigh = (offset >> 32) as u32;
        // SAFETY: buffer and OVERLAPPED outlive the in-flight op — both live in
        // per-slot storage that survives until the completion is consumed.
        match unsafe { ReadFile(handle, Some(buf), None, Some(ov)) } {
            Ok(()) => Ok(()), // completed synchronously; the event is set
            Err(e) if e.code() == ERROR_IO_PENDING.to_hresult() => Ok(()),
            Err(e) => Err(format!("read at {offset}: {e}")),
        }
    }
}

#[cfg(not(all(windows, feature = "mft-backend")))]
fn main() {
    eprintln!("read_probe needs Windows and --features mft-backend");
}
