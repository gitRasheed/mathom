use std::fmt::Write as _;
use std::fs;

use mathom_scanner_ntfs::runs::{backed_clusters, decode_runs};

use crate::rng::Rng;

fn gen_structured(r: &mut Rng) -> Vec<u8> {
    let mut v = Vec::new();
    for _ in 0..r.below(6) {
        let lw = if r.chance(5) {
            r.below(16)
        } else {
            1 + r.below(8)
        };
        let ow = if r.chance(15) {
            0
        } else if r.chance(5) {
            r.below(16)
        } else {
            1 + r.below(8)
        };
        v.push(((ow << 4) | lw) as u8);
        for _ in 0..(lw + ow).min(16) {
            v.push(r.edge_byte());
        }
    }
    if !r.chance(10) {
        v.push(0);
    }
    if r.chance(10) && !v.is_empty() {
        let cut = r.below(v.len() as u64) as usize;
        v.truncate(cut);
    }
    v
}

fn gen_random(r: &mut Rng) -> Vec<u8> {
    (0..r.below(24)).map(|_| r.edge_byte()).collect()
}

/// Hand-built cases aimed at 64-bit edges.
fn fixed() -> Vec<Vec<u8>> {
    let mut out = vec![
        vec![],
        vec![0],
        vec![0x21, 0x18, 0x34, 0x56, 0x00],
        vec![0x21, 0x30, 0x20, 0x01, 0x11, 0x10, 0xFF, 0x00],
        vec![0x01, 0x10, 0x00],
        vec![0x11, 0x10, 0xFE, 0x00],
        vec![0x04, 0x89, 0xF7, 0x17, 0x06, 0x00],
        vec![0xF0, 0x01],
    ];
    // i64::MAX LCN, then +1: signed overflow.
    let mut a = vec![0x81, 0x01];
    a.extend_from_slice(&i64::MAX.to_le_bytes());
    a.extend_from_slice(&[0x11, 0x01, 0x01, 0x00]);
    out.push(a);
    // i64::MAX LCN, then -1: fine.
    let mut a = vec![0x81, 0x01];
    a.extend_from_slice(&i64::MAX.to_le_bytes());
    a.extend_from_slice(&[0x11, 0x01, 0xFF, 0x00]);
    out.push(a);
    // 8-byte lengths of u64::MAX: backed overflow on the second.
    let mut a = vec![0x18];
    a.extend_from_slice(&u64::MAX.to_le_bytes());
    a.push(0x01);
    a.push(0x18);
    a.extend_from_slice(&u64::MAX.to_le_bytes());
    a.push(0x01);
    a.push(0x00);
    out.push(a);
    // i64::MIN delta straight away.
    let mut a = vec![0x81, 0x01];
    a.extend_from_slice(&i64::MIN.to_le_bytes());
    a.push(0x00);
    out.push(a);
    // 7-byte negative offset (sign-extends from bit 55).
    out.push(vec![0x71, 0x05, 0, 0, 0, 0, 0, 0, 0x80, 0x00]);
    // Lengths straddling the U32 boundary.
    out.push(vec![0x15, 0xFF, 0xFF, 0xFF, 0xFF, 0x00, 0x01, 0x00]);
    out.push(vec![0x15, 0x00, 0x00, 0x00, 0x00, 0x01, 0x01, 0x00]);
    out
}

fn hex(v: u64) -> String {
    format!("{v:016x}")
}

fn line(data: &[u8]) -> String {
    let mut s = String::new();
    match decode_runs(data) {
        Ok(exts) => {
            s.push_str("OK");
            for e in exts {
                let _ = write!(s, " {}:{}", hex(e.lcn), hex(e.clusters));
            }
        }
        Err(e) => {
            let _ = write!(s, "ERR {}", e.0);
        }
    }
    s.push_str(" | ");
    match backed_clusters(data) {
        Ok(t) => {
            let _ = write!(s, "OK {}", hex(t));
        }
        Err(e) => {
            let _ = write!(s, "ERR {}", e.0);
        }
    }
    s
}

pub fn emit(n: usize, out: &str) {
    let mut r = Rng(0x6d61_7468_6f6d);
    let mut cases = fixed();
    while cases.len() < n {
        cases.push(if r.chance(70) {
            gen_structured(&mut r)
        } else {
            gen_random(&mut r)
        });
    }
    let expect: String = cases.iter().map(|c| line(c) + "\n").collect();
    fs::create_dir_all(out).unwrap();
    fs::write(format!("{out}/expected.txt"), expect).unwrap();
    crate::write_bend_bytes(&format!("{out}/cases.bend"), "cases", &cases);
}
