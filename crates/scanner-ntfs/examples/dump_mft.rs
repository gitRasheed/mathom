//! Dumps a volume's raw $MFT to a file (elevation required) — the input
//! for the oracle real-volume parity test:
//!
//! ```text
//! cargo run --release -p mathom-scanner-ntfs --features mft-backend --example dump_mft -- C:\ %TEMP%\mft-c.bin
//! set MATHOM_MFT_DUMP=%TEMP%\mft-c.bin
//! cargo test -p mathom-scanner-ntfs --release --features oracle-tests --test oracle -- --nocapture
//! ```

#[cfg(all(windows, feature = "mft-backend"))]
fn main() {
    use std::io::Write;

    use mathom_scanner_ntfs::volume::{AlignedBuf, Volume, map_mft};

    let args: Vec<String> = std::env::args().skip(1).collect();
    let (Some(mount_arg), Some(out_path)) = (args.first(), args.get(1)) else {
        eprintln!("usage: dump_mft <volume, e.g. C:\\> <output file>");
        std::process::exit(2);
    };
    let mut mount = mount_arg.clone();
    if !mount.ends_with('\\') {
        mount.push('\\');
    }

    let volume = Volume::open(&mount).unwrap_or_else(|e| {
        eprintln!("{e}\n(hint: this needs an elevated terminal)");
        std::process::exit(1);
    });
    let map = map_mft(&volume, &mount).expect("mapping the $MFT");
    let record = map.geometry.record_size as u64;
    let mut remaining = (map.mft_bytes / record) * record;
    println!(
        "dumping {} MiB of $MFT ({} records, {} extents) to {out_path}…",
        remaining >> 20,
        remaining / record,
        map.extents.len()
    );

    let mut out = std::io::BufWriter::new(std::fs::File::create(out_path).expect("output file"));
    let mut buf = AlignedBuf::new(16 * 1024 * 1024);
    for extent in &map.extents {
        let mut off = extent.lcn * map.geometry.cluster_size as u64;
        let mut left = extent.clusters * map.geometry.cluster_size as u64;
        while left > 0 && remaining > 0 {
            let want = (buf.as_mut_slice().len() as u64).min(left).min(remaining) as usize;
            volume
                .read_at(off, &mut buf.as_mut_slice()[..want])
                .expect("volume read");
            out.write_all(&buf.as_mut_slice()[..want]).expect("write");
            off += want as u64;
            left -= want as u64;
            remaining -= want as u64;
        }
    }
    out.flush().expect("flush");
    println!("done.");
}

#[cfg(not(all(windows, feature = "mft-backend")))]
fn main() {
    eprintln!("dump_mft needs Windows and --features mft-backend");
}
