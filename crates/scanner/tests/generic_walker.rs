//! Integration tests: GenericScanner against real on-disk fixture trees.

use std::fs;
use std::path::Path;

use mathom_core::{EntryFlags, NodeId, Tree, TreeBuilder};
use mathom_scanner::{GenericScanner, ScanEvent, ScanOptions, ScanStats, Scanner};

fn scan_to_tree(root: &Path) -> (Tree, ScanStats) {
    let handle = GenericScanner.scan(ScanOptions::new(root));
    let mut builder = TreeBuilder::new();
    let mut stats = None;
    for event in handle.events().iter() {
        match event {
            ScanEvent::Batch(batch) => builder.add_batch(&batch),
            ScanEvent::DirError { id, .. } => builder.mark_error(id),
            ScanEvent::Progress(_) => {}
            ScanEvent::Done(s) => {
                stats = Some(s);
                break;
            }
        }
    }
    (builder.finish(), stats.expect("scan ended without Done"))
}

fn child_by_name(tree: &Tree, parent: NodeId, name: &str) -> NodeId {
    tree.children(parent)
        .find(|&c| tree.name(c) == name)
        .unwrap_or_else(|| panic!("no child named {name}"))
}

fn write_file(path: &Path, len: usize) {
    fs::write(path, vec![0u8; len]).unwrap();
}

#[test]
fn known_fixture_aggregates_exactly() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    fs::create_dir_all(root.join("a/sub")).unwrap();
    fs::create_dir(root.join("b")).unwrap();
    write_file(&root.join("a/f1"), 100);
    write_file(&root.join("a/sub/f2"), 7);
    write_file(&root.join("f3"), 1);

    let (tree, stats) = scan_to_tree(root);

    assert_eq!(stats.files, 3);
    assert_eq!(stats.dirs, 4, "root, a, sub, b");
    assert_eq!(stats.bytes, 108);
    assert_eq!(stats.errors, 0);
    assert!(!stats.cancelled);

    let root_node = tree.node(Tree::ROOT);
    assert_eq!(root_node.size, 108);
    assert_eq!(root_node.items, 6, "3 files + 3 dirs below root");
    assert!(root_node.is_dir());

    let a = child_by_name(&tree, Tree::ROOT, "a");
    assert_eq!(tree.node(a).size, 107);
    assert_eq!(tree.node(a).items, 3);

    let sub = child_by_name(&tree, a, "sub");
    assert_eq!(tree.node(sub).size, 7);
    assert_eq!(tree.node(sub).items, 1);

    let b = child_by_name(&tree, Tree::ROOT, "b");
    assert_eq!(tree.node(b).size, 0);
    assert_eq!(tree.node(b).items, 0);

    let f1 = child_by_name(&tree, a, "f1");
    assert_eq!(tree.node(f1).size, 100);
    assert!(!tree.node(f1).is_dir());
}

/// Deterministic generated fixture; totals recorded during generation must
/// match the scanned tree exactly.
#[test]
fn generated_fixture_totals_match() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();

    let mut rng: u64 = 0x5DEECE66D;
    let mut next = move || {
        rng = rng
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (rng >> 33) as usize
    };

    let mut expected_files = 0u64;
    let mut expected_dirs = 1u64; // root
    let mut expected_bytes = 0u64;

    let mut stack = vec![(root.to_path_buf(), 0u32)];
    while let Some((dir_path, depth)) = stack.pop() {
        let n_files = 3 + next() % 8;
        for i in 0..n_files {
            let len = next() % 5000;
            write_file(&dir_path.join(format!("file_{i}.bin")), len);
            expected_files += 1;
            expected_bytes += len as u64;
        }
        if depth < 4 {
            let n_dirs = 2 + next() % 3;
            for i in 0..n_dirs {
                let sub = dir_path.join(format!("dir_{i}"));
                fs::create_dir(&sub).unwrap();
                expected_dirs += 1;
                stack.push((sub, depth + 1));
            }
        }
    }

    let (tree, stats) = scan_to_tree(root);

    assert_eq!(stats.files, expected_files);
    assert_eq!(stats.dirs, expected_dirs);
    assert_eq!(stats.bytes, expected_bytes);
    assert_eq!(stats.errors, 0);

    let root_node = tree.node(Tree::ROOT);
    assert_eq!(root_node.size, expected_bytes);
    assert_eq!(
        u64::from(root_node.items),
        expected_files + expected_dirs - 1
    );
    assert_eq!(tree.len() as u64, expected_files + expected_dirs);
}

/// With more directories than the channel holds and no draining, workers
/// block; cancel must wind the scan down and still deliver Done.
#[test]
fn cancel_terminates_with_done() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    for i in 0..700 {
        let sub = root.join(format!("d{i}"));
        fs::create_dir(&sub).unwrap();
        write_file(&sub.join("f"), 10);
    }

    let handle = GenericScanner.scan(ScanOptions::new(root));
    handle.cancel();

    let mut got_done = false;
    for event in handle.events().iter() {
        if let ScanEvent::Done(stats) = event {
            assert!(stats.cancelled);
            got_done = true;
            break;
        }
    }
    assert!(got_done);
}

#[test]
fn nonexistent_root_reports_error_and_done() {
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("does-not-exist");

    let handle = GenericScanner.scan(ScanOptions::new(&missing));
    let mut saw_root_error = false;
    let mut stats = None;
    for event in handle.events().iter() {
        match event {
            ScanEvent::DirError { id: 0, .. } => saw_root_error = true,
            ScanEvent::Done(s) => {
                stats = Some(s);
                break;
            }
            _ => {}
        }
    }
    assert!(saw_root_error);
    assert_eq!(stats.unwrap().errors, 1);
}

/// Junctions (Windows) / symlinks (unix) must be marked and never descended.
#[cfg(windows)]
#[test]
fn junction_is_marked_and_not_descended() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    fs::create_dir(root.join("real")).unwrap();
    write_file(&root.join("real/payload"), 1000);

    let status = std::process::Command::new("cmd")
        .args(["/C", "mklink", "/J"])
        .arg(root.join("jump"))
        .arg(root.join("real"))
        .status()
        .unwrap();
    assert!(status.success(), "mklink /J failed");

    let (tree, stats) = scan_to_tree(root);

    let jump = child_by_name(&tree, Tree::ROOT, "jump");
    assert!(tree.node(jump).flags.contains(EntryFlags::REPARSE));
    assert!(!tree.node(jump).is_dir());
    assert_eq!(tree.node(jump).size, 0);
    assert_eq!(tree.children(jump).count(), 0);
    // payload counted exactly once (through "real", not through the junction)
    assert_eq!(stats.bytes, 1000);
    assert_eq!(tree.node(Tree::ROOT).size, 1000);
}

/// Sparse files (and by the same attribute-driven path, compressed files
/// and cloud placeholders) must report true on-disk allocation, not their
/// logical size — a dehydrated OneDrive file is the worst case: huge
/// logical size, near-zero allocation.
#[cfg(windows)]
#[test]
fn sparse_file_reports_true_allocation() {
    use std::io::Write;

    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let path = root.join("holey.bin");

    let mut f = fs::File::create(&path).unwrap();
    let status = std::process::Command::new("fsutil")
        .args(["sparse", "setflag"])
        .arg(&path)
        .status()
        .unwrap();
    assert!(
        status.success(),
        "fsutil sparse setflag failed (non-NTFS temp dir?)"
    );
    f.write_all(&[0xAB; 4096]).unwrap(); // 4 KiB of real data
    f.set_len(8 * 1024 * 1024).unwrap(); // the rest is a hole
    drop(f);

    let (tree, stats) = scan_to_tree(root);

    let holey = child_by_name(&tree, Tree::ROOT, "holey.bin");
    let node = tree.node(holey);
    assert!(
        node.flags.contains(EntryFlags::SPARSE),
        "sparse flag missing"
    );
    assert_eq!(
        node.size,
        8 * 1024 * 1024,
        "logical size is the full extent"
    );
    assert!(
        node.allocated < 1024 * 1024,
        "allocated must reflect the hole, got {}",
        node.allocated
    );
    assert!(
        node.allocated >= 4096,
        "the written 4 KiB is really allocated"
    );
    assert_eq!(
        stats.bytes,
        8 * 1024 * 1024,
        "progress counts logical bytes"
    );
}

#[cfg(unix)]
#[test]
fn symlink_is_marked_and_not_descended() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    fs::create_dir(root.join("real")).unwrap();
    write_file(&root.join("real/payload"), 1000);
    std::os::unix::fs::symlink(root.join("real"), root.join("jump")).unwrap();

    let (tree, stats) = scan_to_tree(root);

    let jump = child_by_name(&tree, Tree::ROOT, "jump");
    assert!(tree.node(jump).flags.contains(EntryFlags::REPARSE));
    assert!(!tree.node(jump).is_dir());
    assert_eq!(tree.children(jump).count(), 0);
    assert_eq!(stats.bytes, 1000);
    assert_eq!(tree.node(Tree::ROOT).size, 1000);
}
