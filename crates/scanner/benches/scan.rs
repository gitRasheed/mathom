//! Milestone-1 throughput benches:
//! 1. `treebuilder`: pure in-memory aggregation throughput (entries/sec) —
//!    the ceiling any scanner backend can feed into.
//! 2. `scan_disk`: full scan + build of a generated on-disk fixture
//!    (reused across runs at %TEMP%\mathom-bench-fixture-v1).

use std::fs;
use std::path::PathBuf;

use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use mathom_core::{EntryBatch, EntryFlags, FileEntry, TreeBuilder};
use mathom_scanner::{GenericScanner, ScanEvent, ScanOptions, Scanner};

const DIRS: u32 = 5_000;
const FILES_PER_DIR: u32 = 200;

fn synth_batches() -> (Vec<EntryBatch>, u64) {
    let entry = |id: u32, parent: u32, flags: EntryFlags, size: u64| FileEntry {
        path_id: id,
        parent_id: parent,
        name_off: 0,
        name_len: 0,
        flags,
        size,
        allocated_size: size,
        mtime: 1_700_000_000,
    };

    let mut batches = Vec::new();
    let mut root = EntryBatch::default();
    root.push("root", entry(0, 0, EntryFlags::DIR, 0));
    batches.push(root);

    // Dir ids 1..=DIRS, then files. Names repeat heavily (realistic interning).
    let mut dir_batch = EntryBatch::default();
    for d in 0..DIRS {
        dir_batch.push(
            &format!("dir_{}", d % 500),
            entry(d + 1, 0, EntryFlags::DIR, 0),
        );
        if dir_batch.len() == 4096 {
            batches.push(std::mem::take(&mut dir_batch));
        }
    }
    if !dir_batch.is_empty() {
        batches.push(dir_batch);
    }

    let mut next_id = DIRS + 1;
    for d in 0..DIRS {
        let mut b = EntryBatch::default();
        for f in 0..FILES_PER_DIR {
            b.push(
                &format!("file_{}.bin", f % 200),
                entry(next_id, d + 1, EntryFlags(0), u64::from(f) * 37 + 1),
            );
            next_id += 1;
        }
        batches.push(b);
    }

    let total = u64::from(DIRS) * u64::from(FILES_PER_DIR) + u64::from(DIRS) + 1;
    (batches, total)
}

fn bench_treebuilder(c: &mut Criterion) {
    let (batches, total_entries) = synth_batches();
    let mut group = c.benchmark_group("treebuilder");
    group.throughput(Throughput::Elements(total_entries));
    group.sample_size(20);
    group.bench_function("aggregate_1m_entries", |b| {
        b.iter(|| {
            let mut builder = TreeBuilder::new();
            for batch in &batches {
                builder.add_batch(batch);
            }
            builder.finish()
        });
    });
    group.finish();
}

/// ~1.9k dirs, ~15k empty files; built once, reused across bench runs.
fn disk_fixture() -> (PathBuf, u64) {
    let root = std::env::temp_dir().join("mathom-bench-fixture-v1");
    let marker = root.join(".complete");
    let mut entries = 0u64;

    let mut build = |count_only: bool| {
        entries = 0;
        let mut stack = vec![(root.clone(), 0u32)];
        while let Some((dir, depth)) = stack.pop() {
            entries += 1;
            if !count_only {
                fs::create_dir_all(&dir).unwrap();
            }
            for f in 0..8 {
                entries += 1;
                if !count_only {
                    fs::write(dir.join(format!("f{f}.dat")), b"").unwrap();
                }
            }
            if depth < 3 {
                for d in 0..12 {
                    stack.push((dir.join(format!("d{d}")), depth + 1));
                }
            }
        }
    };

    if marker.exists() {
        build(true);
    } else {
        build(false);
        fs::write(&marker, b"").unwrap();
    }
    (root, entries)
}

fn bench_scan_disk(c: &mut Criterion) {
    let (root, entries) = disk_fixture();
    let mut group = c.benchmark_group("scan_disk");
    group.throughput(Throughput::Elements(entries));
    group.sample_size(10);
    group.bench_function("scan_and_build_fixture", |b| {
        b.iter(|| {
            let handle = GenericScanner.scan(ScanOptions::new(&root));
            let mut builder = TreeBuilder::new();
            for event in handle.events().iter() {
                match event {
                    ScanEvent::Batch(batch) => builder.add_batch(&batch),
                    ScanEvent::Done(stats) => {
                        assert_eq!(stats.errors, 0);
                        break;
                    }
                    _ => {}
                }
            }
            builder.finish()
        });
    });
    group.finish();
}

criterion_group!(benches, bench_treebuilder, bench_scan_disk);
criterion_main!(benches);
