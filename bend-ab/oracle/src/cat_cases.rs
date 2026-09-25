use std::fmt::Write as _;
use std::fs;

use mathom_core::category::{categorize, extension_key};

use crate::rng::Rng;

const EXTS: &[&str] = &[
    "mp4",
    "mkv",
    "mpeg",
    "m2ts",
    "flac",
    "aiff",
    "jpg",
    "jpeg",
    "heic",
    "psd",
    "7z",
    "zst",
    "wim",
    "pdf",
    "docx",
    "epub",
    "csv",
    "rs",
    "c",
    "ts",
    "tsx",
    "yaml",
    "lock",
    "ps1",
    "exe",
    "msix",
    "jar",
    "dll",
    "winmd",
    "efi",
    "db",
    "sqlite3",
    "qcow2",
    "vhdx",
    "bak",
    "xyz",
    "abcdefgh",
    "abcdefghi",
    "",
    "tar",
    "gz",
    "h",
    "md",
];

const ODD: &[&str] = &[
    "é", "日", "😀", "İ", "K", "ß", "\u{0}", "\"", "\\", " ", "\t", ".", "..", "Ω", "ﬁ",
];

fn pick<'a>(r: &mut Rng, xs: &'a [&'a str]) -> &'a str {
    xs[r.below(xs.len() as u64) as usize]
}

fn mangle_case(r: &mut Rng, s: &str) -> String {
    s.chars()
        .map(|c| {
            if r.chance(40) {
                c.to_ascii_uppercase()
            } else {
                c
            }
        })
        .collect()
}

fn gen_name(r: &mut Rng) -> String {
    let mut s = String::new();
    for _ in 0..r.below(4) {
        if r.chance(30) {
            s.push_str(pick(r, ODD));
        } else {
            s.push_str(["file", "a", "photo", "backup.tar", "x.y"][r.below(5) as usize]);
        }
    }
    if r.chance(85) {
        s.push('.');
        let ext = pick(r, EXTS).to_string();
        let mut ext = mangle_case(r, &ext);
        if r.chance(20) {
            let at = r.below(ext.len() as u64 + 1) as usize;
            if ext.is_char_boundary(at) {
                ext.insert_str(at, pick(r, ODD));
            }
        }
        s.push_str(&ext);
    }
    s
}

/// A Bend string literal.
pub fn bend_str(s: &str) -> String {
    let mut o = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            ' '..='~' => o.push(c),
            _ => {
                let _ = write!(o, "\\u{{{:x}}}", c as u32);
            }
        }
    }
    o.push('"');
    o
}

fn line(name: &str) -> String {
    let ext = match extension_key(name) {
        Some(k) => k.as_str().to_string(),
        None => "-".to_string(),
    };
    format!("{} {}", categorize(name, false) as u8, ext)
}

pub fn emit(n: usize, out: &str) {
    let mut r = Rng(0x6361_7465);
    let mut names: Vec<String> = [
        "movie.mkv",
        "MOVIE.MKV",
        "main.ts",
        "README",
        "weird.xyz123",
        "trailing.",
        ".",
        "",
        "a.b.c.pdf",
        ".bashrc",
        "x.日本",
        "x.日本語",
        "x.é",
        "x.ÉXE",
        "x.EXE",
        "x.K",
        "x.aB€",
        "x.\u{1F600}\u{1F600}",
        "x.\u{1F600}\u{1F600}\u{1F600}",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    while names.len() < n {
        names.push(gen_name(&mut r));
    }
    let expect: String = names.iter().map(|c| line(c) + "\n").collect();
    fs::create_dir_all(out).unwrap();
    fs::write(format!("{out}/expected.txt"), expect).unwrap();
    let items: Vec<String> = names.iter().map(|n| bend_str(n)).collect();
    crate::write_bend_list(&format!("{out}/cases.bend"), "names", "String", &items);
}
