//! Hand-built NTFS byte fixtures for tests and benches.

#[derive(Clone, Debug)]
pub struct RecordBuilder {
    seq: u16,
    base_ref: u64,
    in_use: bool,
    is_dir: bool,
    name_count: u16,
    attrs: Vec<Vec<u8>>,
}

const ALIGN: usize = 8;

fn align8(n: usize) -> usize {
    n.div_ceil(ALIGN) * ALIGN
}

impl RecordBuilder {
    pub fn file() -> Self {
        RecordBuilder {
            seq: 1,
            base_ref: 0,
            in_use: true,
            is_dir: false,
            name_count: 0,
            attrs: Vec::new(),
        }
    }

    pub fn dir() -> Self {
        RecordBuilder {
            is_dir: true,
            ..Self::file()
        }
    }

    pub fn free() -> Self {
        RecordBuilder {
            in_use: false,
            ..Self::file()
        }
    }

    pub fn seq(mut self, seq: u16) -> Self {
        self.seq = seq;
        self
    }

    pub fn extension_of(mut self, base_no: u64, base_seq: u16) -> Self {
        self.base_ref = base_no | (base_seq as u64) << 48;
        self
    }

    pub fn std_info(mut self, dos_attrs: u32, mtime_filetime: u64) -> Self {
        let mut v = vec![0u8; 0x48];
        for time_off in [0x00, 0x08, 0x10, 0x18] {
            v[time_off..time_off + 8].copy_from_slice(&mtime_filetime.to_le_bytes());
        }
        v[0x20..0x24].copy_from_slice(&dos_attrs.to_le_bytes());
        self.attrs.push(resident_attr(0x10, 0, "", &v));
        self
    }

    /// $FILE_NAME. Namespaces: 0 POSIX, 1 Win32, 2 DOS, 3 Win32&DOS.
    pub fn name(self, parent_no: u64, parent_seq: u16, namespace: u8, name: &str) -> Self {
        let units: Vec<u16> = name.encode_utf16().collect();
        self.name_utf16(parent_no, parent_seq, namespace, &units)
    }

    pub fn name_utf16(
        mut self,
        parent_no: u64,
        parent_seq: u16,
        namespace: u8,
        units: &[u16],
    ) -> Self {
        assert!(
            units.len() <= 255,
            "NTFS names are at most 255 UTF-16 units"
        );
        let mut v = vec![0u8; 0x42 + 2 * units.len()];
        let parent_ref = parent_no | (parent_seq as u64) << 48;
        v[0..8].copy_from_slice(&parent_ref.to_le_bytes());
        // The duplicated sizes here are stale on real volumes; poison them
        // so a parser trusting them fails tests.
        v[0x28..0x30].copy_from_slice(&0x2222u64.to_le_bytes());
        v[0x30..0x38].copy_from_slice(&0x1111u64.to_le_bytes());
        v[0x40] = units.len() as u8;
        v[0x41] = namespace;
        for (i, u) in units.iter().enumerate() {
            v[0x42 + 2 * i..0x44 + 2 * i].copy_from_slice(&u.to_le_bytes());
        }
        self.name_count += 1;
        self.attrs.push(resident_attr(0x30, 0, "", &v));
        self
    }

    pub fn data_resident_bytes(mut self, content: &[u8]) -> Self {
        self.attrs.push(resident_attr(0x80, 0, "", content));
        self
    }

    pub fn data_nonresident(mut self, real: u64, alloc: u64) -> Self {
        self.attrs
            .push(nonres_attr(0x80, 0, "", 0, alloc, real, 0, None));
        self
    }

    /// NTFS-compressed $DATA: `alloc` is the VCN-span allocation,
    /// `total_alloc` the truly backed bytes (what a parser must report).
    pub fn data_nonresident_compressed(mut self, real: u64, alloc: u64, total_alloc: u64) -> Self {
        self.attrs.push(nonres_attr(
            0x80,
            0x0001,
            "",
            0,
            alloc,
            real,
            4,
            Some(total_alloc),
        ));
        self
    }

    pub fn data_nonresident_sparse(mut self, real: u64, backed: u64) -> Self {
        self.attrs.push(nonres_attr(
            0x80,
            0x8000,
            "",
            0,
            real,
            real,
            4,
            Some(backed),
        ));
        self
    }

    pub fn data_nonresident_with_runs(mut self, real: u64, highest_vcn: u64, runs: &[u8]) -> Self {
        let mut a = nonres_attr(0x80, 0, "", 0, real, real, 0, None);
        a[24..32].copy_from_slice(&highest_vcn.to_le_bytes());
        let run_off = u16::from_le_bytes([a[32], a[33]]) as usize;
        a.truncate(run_off);
        a.extend_from_slice(runs);
        let total = align8(a.len());
        a.resize(total, 0);
        a[4..8].copy_from_slice(&(total as u32).to_le_bytes());
        self.attrs.push(a);
        self
    }

    pub fn named_data_nonresident(mut self, stream: &str, real: u64, alloc: u64) -> Self {
        self.attrs
            .push(nonres_attr(0x80, 0, stream, 0, alloc, real, 0, None));
        self
    }

    /// Continuation fragment (lowest VCN > 0); its sizes are poison values —
    /// only the VCN-0 fragment's sizes are real.
    pub fn data_continuation(mut self, lowest_vcn: u64) -> Self {
        self.attrs.push(nonres_attr(
            0x80, 0, "", lowest_vcn, 0xD1E5, 0xD1E5, 0, None,
        ));
        self
    }

    pub fn reparse(mut self, tag: u32) -> Self {
        let mut v = vec![0u8; 8];
        v[0..4].copy_from_slice(&tag.to_le_bytes());
        self.attrs.push(resident_attr(0xC0, 0, "", &v));
        self
    }

    pub fn attribute_list_stub(mut self) -> Self {
        let mut v = vec![0u8; 32];
        v[0..4].copy_from_slice(&0x10u32.to_le_bytes());
        v[4..6].copy_from_slice(&32u16.to_le_bytes()); // entry length
        v[7] = 32; // name offset (unused, past the entry)
        v[16..24].copy_from_slice(&30u64.to_le_bytes()); // base file reference
        self.attrs.push(resident_attr(0x20, 0, "", &v));
        self
    }

    pub fn build(&self, record_no: u64, record_size: usize) -> Vec<u8> {
        assert!(record_size >= 512 && record_size.is_power_of_two());
        let mut rec = vec![0u8; record_size];
        let sectors = record_size / 512;
        let usa_off = 0x30usize;
        let usa_count = (sectors + 1) as u16;
        let first_attr = align8(usa_off + 2 * usa_count as usize);

        rec[0..4].copy_from_slice(b"FILE");
        rec[4..6].copy_from_slice(&(usa_off as u16).to_le_bytes());
        rec[6..8].copy_from_slice(&usa_count.to_le_bytes());
        rec[0x10..0x12].copy_from_slice(&self.seq.to_le_bytes());
        rec[0x12..0x14].copy_from_slice(&self.name_count.to_le_bytes());
        rec[0x14..0x16].copy_from_slice(&(first_attr as u16).to_le_bytes());
        let flags = u16::from(self.in_use) | u16::from(self.is_dir) << 1;
        rec[0x16..0x18].copy_from_slice(&flags.to_le_bytes());
        rec[0x1C..0x20].copy_from_slice(&(record_size as u32).to_le_bytes());
        rec[0x20..0x28].copy_from_slice(&self.base_ref.to_le_bytes());
        rec[0x28..0x2A].copy_from_slice(&(self.attrs.len() as u16 + 1).to_le_bytes());
        rec[0x2A..0x2C].copy_from_slice(&((record_no >> 32) as u16).to_le_bytes());
        rec[0x2C..0x30].copy_from_slice(&(record_no as u32).to_le_bytes());

        let mut pos = first_attr;
        for (id, attr) in self.attrs.iter().enumerate() {
            assert!(
                pos + attr.len() + 8 <= record_size,
                "fixture record overflows {record_size} bytes"
            );
            rec[pos..pos + attr.len()].copy_from_slice(attr);
            rec[pos + 14..pos + 16].copy_from_slice(&(id as u16).to_le_bytes());
            pos += attr.len();
        }
        rec[pos..pos + 4].copy_from_slice(&0xFFFF_FFFFu32.to_le_bytes());
        pos += 4;
        rec[0x18..0x1C].copy_from_slice(&(pos as u32).to_le_bytes());

        let usn = (self.seq | 0x4B00).to_le_bytes();
        rec[usa_off..usa_off + 2].copy_from_slice(&usn);
        for i in 1..usa_count as usize {
            let end = i * 512 - 2;
            let (a, b) = (rec[end], rec[end + 1]);
            rec[usa_off + 2 * i] = a;
            rec[usa_off + 2 * i + 1] = b;
            rec[end..end + 2].copy_from_slice(&usn);
        }
        rec
    }
}

pub fn image(record_size: usize, records: Vec<(u64, RecordBuilder)>) -> Vec<u8> {
    let max = records.iter().map(|(n, _)| *n).max().unwrap_or(0);
    let mut img = vec![0u8; (max as usize + 1) * record_size];
    for (no, b) in records {
        let off = no as usize * record_size;
        img[off..off + record_size].copy_from_slice(&b.build(no, record_size));
    }
    img
}

pub fn root_dir() -> RecordBuilder {
    RecordBuilder::dir()
        .seq(5)
        .name(5, 5, 3, ".")
        .std_info(0x6, 0)
}

fn resident_attr(ty: u32, attr_flags: u16, name: &str, value: &[u8]) -> Vec<u8> {
    let units: Vec<u16> = name.encode_utf16().collect();
    let name_off = 24usize;
    let val_off = align8(name_off + 2 * units.len());
    let total = align8(val_off + value.len());
    let mut a = vec![0u8; total];
    write_common_header(&mut a, ty, total, false, &units, name_off, attr_flags);
    a[16..20].copy_from_slice(&(value.len() as u32).to_le_bytes());
    a[20..22].copy_from_slice(&(val_off as u16).to_le_bytes());
    a[val_off..val_off + value.len()].copy_from_slice(value);
    a
}

#[allow(clippy::too_many_arguments)]
fn nonres_attr(
    ty: u32,
    attr_flags: u16,
    name: &str,
    lowest_vcn: u64,
    alloc: u64,
    real: u64,
    compression_unit: u16,
    total_alloc: Option<u64>,
) -> Vec<u8> {
    assert_eq!(
        total_alloc.is_some(),
        compression_unit != 0,
        "total_allocated exists exactly when a compression unit is set"
    );
    let units: Vec<u16> = name.encode_utf16().collect();
    let name_off = if total_alloc.is_some() { 0x48 } else { 0x40 };
    let run_off = align8(name_off + 2 * units.len());
    // A plausible little run list; the record parser never reads it, the
    // oracle may. One cluster at LCN 1 (or a fragment for continuations).
    let runs: &[u8] = &[0x11, 0x01, 0x01, 0x00];
    let total = align8(run_off + runs.len());
    let mut a = vec![0u8; total];
    write_common_header(&mut a, ty, total, true, &units, name_off, attr_flags);
    a[16..24].copy_from_slice(&lowest_vcn.to_le_bytes());
    a[24..32].copy_from_slice(&lowest_vcn.to_le_bytes()); // highest = lowest (1 cluster)
    a[32..34].copy_from_slice(&(run_off as u16).to_le_bytes());
    a[34..36].copy_from_slice(&compression_unit.to_le_bytes());
    a[40..48].copy_from_slice(&alloc.to_le_bytes());
    a[48..56].copy_from_slice(&real.to_le_bytes());
    a[56..64].copy_from_slice(&real.to_le_bytes()); // initialized = real
    if let Some(t) = total_alloc {
        a[64..72].copy_from_slice(&t.to_le_bytes());
    }
    a[run_off..run_off + runs.len()].copy_from_slice(runs);
    a
}

fn write_common_header(
    a: &mut [u8],
    ty: u32,
    total: usize,
    non_resident: bool,
    name_units: &[u16],
    name_off: usize,
    attr_flags: u16,
) {
    a[0..4].copy_from_slice(&ty.to_le_bytes());
    a[4..8].copy_from_slice(&(total as u32).to_le_bytes());
    a[8] = non_resident as u8;
    a[9] = name_units.len() as u8;
    a[10..12].copy_from_slice(&(name_off as u16).to_le_bytes());
    a[12..14].copy_from_slice(&attr_flags.to_le_bytes());
    for (i, u) in name_units.iter().enumerate() {
        a[name_off + 2 * i..name_off + 2 * i + 2].copy_from_slice(&u.to_le_bytes());
    }
}
