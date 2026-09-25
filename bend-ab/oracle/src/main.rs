//! A/B oracle: runs mathom's real Rust code over generated inputs and writes
//! (a) the inputs as a Bend module and (b) the expected output lines.

use std::fmt::Write as _;
use std::fs;

mod cat_cases;
mod rng;
mod runs_cases;
mod tree_cases;
mod treemap_cases;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let (which, n, out) = (&args[1], args[2].parse::<usize>().unwrap(), &args[3]);
    match which.as_str() {
        "runs" => runs_cases::emit(n, out),
        "cat" => cat_cases::emit(n, out),
        "treemap" => treemap_cases::emit(n, args.get(4).map_or(8, |d| d.parse().unwrap()), out),
        "tree" => tree_cases::emit(n, args.get(4).map_or(12, |d| d.parse().unwrap()), true, out),
        _ => panic!("unknown suite"),
    }
}

/// Chunked so no single literal nests deeper than chunk() elements.
fn chunk() -> usize {
    std::env::var("BEND_CHUNK")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(250)
}

pub fn write_bend_list(path: &str, name: &str, ty: &str, items: &[String]) {
    let mut s = String::from("import Base\n\n");
    let chunks: Vec<&[String]> = items.chunks(chunk()).collect();
    for (k, ch) in chunks.iter().enumerate() {
        let _ = writeln!(s, "def {name}.c{k}() -> List<&2, {ty}>:");
        let _ = writeln!(s, "  [{}]\n", ch.join(",\n   "));
    }
    let _ = writeln!(s, "def {name}() -> List<&2, List<&2, {ty}>>:");
    let calls: Vec<String> = (0..chunks.len())
        .map(|k| format!("{name}.c{k}()"))
        .collect();
    let _ = writeln!(s, "  [{}]", calls.join(", "));
    fs::write(path, s).unwrap();
}

pub fn write_bend_bytes(path: &str, name: &str, cases: &[Vec<u8>]) {
    let items: Vec<String> = cases
        .iter()
        .map(|c| {
            let bs: Vec<String> = c.iter().map(|b| b.to_string()).collect();
            format!("[{}]", bs.join(", "))
        })
        .collect();
    write_bend_list(path, name, "List<&2, U32>", &items);
}
