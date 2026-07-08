//! Parse + assemble throughput over a synthetic in-memory $MFT — the
//! portable half of the M5 benchmark story (the disk half is
//! `examples/mft_scan.rs`, elevated). Run:
//! `cargo bench -p mathom-scanner-ntfs`

use criterion::{BatchSize, Criterion, Throughput, criterion_group, criterion_main};

use mathom_scanner_ntfs::assemble::assemble;
use mathom_scanner_ntfs::fixture::{RecordBuilder, image, root_dir};
use mathom_scanner_ntfs::pipeline::{Sweep, Table};

const RECORD_SIZE: usize = 1024;
const RECORDS: usize = 100_000;

/// A volume shaped roughly like real life: ~1/8 directories, a mix of
/// resident/non-resident/compressed files, deterministic.
fn build_image() -> Vec<u8> {
    let mut records: Vec<(u64, RecordBuilder)> = vec![
        (
            0,
            RecordBuilder::file()
                .std_info(0x6, 130_000_000_000_000_000)
                .name(5, 5, 3, "$MFT")
                .data_nonresident(
                    (RECORDS * RECORD_SIZE) as u64,
                    (RECORDS * RECORD_SIZE) as u64,
                ),
        ),
        (5, root_dir()),
    ];
    let mut dirs: Vec<u64> = vec![5];
    for no in 16..RECORDS as u64 {
        let parent = dirs[no as usize % dirs.len()];
        let pseq = if parent == 5 { 5 } else { 1 };
        let rec = match no % 8 {
            0 => {
                dirs.push(no);
                RecordBuilder::dir()
                    .std_info(0, 131_000_000_000_000_000 + no)
                    .name(parent, pseq, 1, &format!("directory-{no}"))
            }
            1 => RecordBuilder::file()
                .std_info(0, 131_000_000_000_000_000 + no)
                .name(parent, pseq, 1, &format!("resident-{no}.ini"))
                .data_resident_bytes(&[0x42; 200]),
            2 => RecordBuilder::file()
                .std_info(0, 131_000_000_000_000_000 + no)
                .name(parent, pseq, 1, &format!("compressed-{no}.log"))
                .data_nonresident_compressed(no * 4096, no * 4096, no * 1024),
            _ => RecordBuilder::file()
                .std_info(0, 131_000_000_000_000_000 + no)
                .name(parent, pseq, 1, &format!("some longer file name {no}.bin"))
                .data_nonresident(no * 1337, (no * 1337).next_multiple_of(4096)),
        };
        records.push((no, rec));
    }
    image(RECORD_SIZE, records)
}

fn swept(img: &[u8]) -> Table {
    let mut owned = img.to_vec();
    let mut sweep = Sweep::new(
        (owned.len() / RECORD_SIZE) as u32,
        RECORD_SIZE,
        mathom_scanner_ntfs::fixture::CLUSTER as u32,
    )
    .unwrap();
    sweep.consume(0, &mut owned);
    sweep.finish()
}

fn bench_mft(c: &mut Criterion) {
    let img = build_image();

    let mut group = c.benchmark_group("mft");
    group.throughput(Throughput::Bytes(img.len() as u64));
    // Fixups mutate the buffer, so each iteration parses a fresh copy
    // (the copy happens in setup, outside the measurement).
    group.bench_function("sweep_100k_records", |b| {
        b.iter_batched(
            || img.clone(),
            |mut fresh| {
                let mut sweep = Sweep::new(
                    (fresh.len() / RECORD_SIZE) as u32,
                    RECORD_SIZE,
                    mathom_scanner_ntfs::fixture::CLUSTER as u32,
                )
                .unwrap();
                sweep.consume(0, &mut fresh);
                sweep.finish()
            },
            BatchSize::LargeInput,
        )
    });

    let table = swept(&img);
    group.throughput(Throughput::Elements(RECORDS as u64));
    group.bench_function("assemble_100k_records", |b| {
        b.iter(|| {
            let mut entries = 0usize;
            assemble(&table, &[], "C:\\", 4096, |batch| {
                entries += batch.len();
                true
            })
            .unwrap();
            entries
        })
    });
    group.finish();
}

criterion_group!(benches, bench_mft);
criterion_main!(benches);
