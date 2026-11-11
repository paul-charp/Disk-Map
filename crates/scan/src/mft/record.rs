//! Pure parsing of NTFS file records: fixups, attributes and run lists.

use std::ops::Range;

const SECTOR: usize = 512;
const FLAG_IN_USE: u16 = 0x0001;
const FLAG_DIR: u16 = 0x0002;
const ATTR_STANDARD_INFORMATION: u32 = 0x10;
const ATTR_ATTRIBUTE_LIST: u32 = 0x20;
const ATTR_FILE_NAME: u32 = 0x30;
const ATTR_DATA: u32 = 0x80;
const ATTR_END: u32 = 0xFFFF_FFFF;
const NS_DOS: u8 = 2;
const ATTR_FLAG_COMPRESSED: u16 = 0x0001;
const ATTR_FLAG_SPARSE: u16 = 0x8000;
const REF_MASK: u64 = 0x0000_FFFF_FFFF_FFFF;

/// Why a record was rejected.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecordError {
    /// Not a `FILE` record (unused slots are zeroed, damaged ones read `BAAD`).
    BadSignature,
    /// The update sequence check failed (torn or damaged record).
    BadFixup,
    /// Header or attribute fields point outside the record.
    Malformed,
}

/// `$STANDARD_INFORMATION` fields.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StdInfo {
    pub ctime: i64,
    pub mtime: i64,
    pub atime: i64,
    pub attrs: u32,
}

/// The chosen `$FILE_NAME`: first non-DOS name of the record.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileName {
    /// Record number of the parent directory.
    pub parent: u64,
    /// Byte range of the UTF-16LE name inside the record.
    pub name: Range<usize>,
}

/// The unnamed `$DATA` attribute (the part that starts at VCN 0).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DataInfo {
    pub logical: u64,
    pub allocated: u64,
    pub resident: bool,
    /// Byte range of the run list inside the record (empty when resident).
    pub runs: Range<usize>,
}

/// What a record contributes to its file.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Parsed {
    pub flags: u16,
    /// Record number of the base record; 0 for a base record.
    pub base: u64,
    pub std: Option<StdInfo>,
    pub name: Option<FileName>,
    pub data: Option<DataInfo>,
    /// The file has an `$ATTRIBUTE_LIST`: some attributes live in extension records.
    pub has_attr_list: bool,
}

impl Parsed {
    pub fn in_use(&self) -> bool {
        self.flags & FLAG_IN_USE != 0
    }

    pub fn is_dir(&self) -> bool {
        self.flags & FLAG_DIR != 0
    }

    pub fn is_extension(&self) -> bool {
        self.base != 0
    }
}

fn u16_at(b: &[u8], o: usize) -> u16 {
    u16::from_le_bytes([b[o], b[o + 1]])
}

fn u32_at(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes(b[o..o + 4].try_into().unwrap())
}

fn u64_at(b: &[u8], o: usize) -> u64 {
    u64::from_le_bytes(b[o..o + 8].try_into().unwrap())
}

fn i64_at(b: &[u8], o: usize) -> i64 {
    i64::from_le_bytes(b[o..o + 8].try_into().unwrap())
}

/// Checks the update sequence array and restores the real last two bytes of every sector.
pub fn apply_fixups(rec: &mut [u8]) -> Result<(), RecordError> {
    if rec.len() < 48 || &rec[..4] != b"FILE" {
        return Err(RecordError::BadSignature);
    }
    let usa_off = u16_at(rec, 4) as usize;
    let usa_count = u16_at(rec, 6) as usize;
    if usa_count < 2 || usa_off + usa_count * 2 > rec.len() || (usa_count - 1) * SECTOR > rec.len()
    {
        return Err(RecordError::Malformed);
    }
    let usn = [rec[usa_off], rec[usa_off + 1]];
    for i in 1..usa_count {
        let end = i * SECTOR;
        if rec[end - 2..end] != usn {
            return Err(RecordError::BadFixup);
        }
        let saved = [rec[usa_off + i * 2], rec[usa_off + i * 2 + 1]];
        rec[end - 2..end].copy_from_slice(&saved);
    }
    Ok(())
}

/// Parses one raw file record in place (fixups are applied to `rec`).
pub fn parse_record(rec: &mut [u8]) -> Result<Parsed, RecordError> {
    apply_fixups(rec)?;
    let mut out = Parsed {
        flags: u16_at(rec, 0x16),
        base: u64_at(rec, 0x20) & REF_MASK,
        ..Parsed::default()
    };
    if !out.in_use() {
        return Ok(out);
    }
    let used = (u32_at(rec, 0x18) as usize).min(rec.len());
    let mut off = u16_at(rec, 0x14) as usize;
    while off + 8 <= used {
        let ty = u32_at(rec, off);
        if ty == ATTR_END {
            break;
        }
        let len = u32_at(rec, off + 4) as usize;
        if len < 16 || !len.is_multiple_of(8) || off + len > used {
            return Err(RecordError::Malformed);
        }
        parse_attribute(&rec[off..off + len], off, ty, &mut out)?;
        off += len;
    }
    Ok(out)
}

/// `attr` is one whole attribute; `base` is its offset inside the record.
fn parse_attribute(attr: &[u8], base: usize, ty: u32, out: &mut Parsed) -> Result<(), RecordError> {
    let non_resident = attr[8] != 0;
    let name_len = attr[9];
    let flags = u16_at(attr, 0x0C);
    match ty {
        ATTR_ATTRIBUTE_LIST => out.has_attr_list = true,
        ATTR_STANDARD_INFORMATION if !non_resident => {
            let v = resident_value(attr)?;
            if v.len() >= 0x24 {
                out.std = Some(StdInfo {
                    ctime: i64_at(v, 0x00),
                    mtime: i64_at(v, 0x08),
                    atime: i64_at(v, 0x18),
                    attrs: u32_at(v, 0x20),
                });
            }
        }
        ATTR_FILE_NAME if !non_resident => {
            let range = resident_range(attr)?;
            let v = &attr[range.clone()];
            if v.len() < 0x42 {
                return Ok(());
            }
            let chars = v[0x40] as usize;
            let namespace = v[0x41];
            if namespace == NS_DOS || out.name.is_some() {
                return Ok(());
            }
            if 0x42 + chars * 2 > v.len() {
                return Err(RecordError::Malformed);
            }
            let start = base + range.start + 0x42;
            out.name = Some(FileName {
                parent: u64_at(v, 0) & REF_MASK,
                name: start..start + chars * 2,
            });
        }
        ATTR_DATA if name_len == 0 => {
            if !non_resident {
                let v = resident_value(attr)?;
                out.data = Some(DataInfo {
                    logical: v.len() as u64,
                    allocated: 0,
                    resident: true,
                    runs: 0..0,
                });
            } else if attr.len() >= 0x40 && u64_at(attr, 0x10) == 0 {
                let run_off = u16_at(attr, 0x20) as usize;
                if run_off > attr.len() {
                    return Err(RecordError::Malformed);
                }
                let unit = u16_at(attr, 0x22);
                let mut allocated = u64_at(attr, 0x28);
                if (flags & (ATTR_FLAG_COMPRESSED | ATTR_FLAG_SPARSE) != 0 || unit != 0)
                    && attr.len() >= 0x48
                {
                    allocated = u64_at(attr, 0x40);
                }
                out.data = Some(DataInfo {
                    logical: u64_at(attr, 0x30),
                    allocated,
                    resident: false,
                    runs: base + run_off..base + attr.len(),
                });
            }
        }
        _ => {}
    }
    Ok(())
}

fn resident_range(attr: &[u8]) -> Result<Range<usize>, RecordError> {
    if attr.len() < 0x18 {
        return Err(RecordError::Malformed);
    }
    let len = u32_at(attr, 0x10) as usize;
    let off = u16_at(attr, 0x14) as usize;
    if off + len > attr.len() {
        return Err(RecordError::Malformed);
    }
    Ok(off..off + len)
}

fn resident_value(attr: &[u8]) -> Result<&[u8], RecordError> {
    Ok(&attr[resident_range(attr)?])
}

/// One extent of a non-resident attribute.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Run {
    /// Starting cluster, `None` for a sparse run.
    pub lcn: Option<u64>,
    /// Length in clusters.
    pub len: u64,
}

/// Decodes a run list; stops at the terminating zero byte or the end of the slice.
pub fn decode_runs(bytes: &[u8]) -> Result<Vec<Run>, RecordError> {
    let mut runs = Vec::new();
    let mut pos = 0usize;
    let mut lcn = 0i64;
    while pos < bytes.len() && bytes[pos] != 0 {
        let header = bytes[pos];
        let len_bytes = (header & 0x0F) as usize;
        let off_bytes = (header >> 4) as usize;
        pos += 1;
        if len_bytes == 0
            || len_bytes > 8
            || off_bytes > 8
            || pos + len_bytes + off_bytes > bytes.len()
        {
            return Err(RecordError::Malformed);
        }
        let mut len = 0u64;
        for (i, &b) in bytes[pos..pos + len_bytes].iter().enumerate() {
            len |= u64::from(b) << (8 * i);
        }
        pos += len_bytes;
        if off_bytes == 0 {
            runs.push(Run { lcn: None, len });
            continue;
        }
        let mut delta = 0i64;
        for (i, &b) in bytes[pos..pos + off_bytes].iter().enumerate() {
            delta |= i64::from(b) << (8 * i);
        }
        if bytes[pos + off_bytes - 1] & 0x80 != 0 && off_bytes < 8 {
            delta |= -1i64 << (8 * off_bytes);
        }
        pos += off_bytes;
        lcn = lcn.checked_add(delta).ok_or(RecordError::Malformed)?;
        if lcn < 0 {
            return Err(RecordError::Malformed);
        }
        runs.push(Run {
            lcn: Some(lcn as u64),
            len,
        });
    }
    Ok(runs)
}

#[cfg(test)]
pub(crate) mod testutil {
    //! Synthetic record builder shared by the tests of this module and of `mft`.

    use super::*;

    pub const RS: usize = 1024;

    pub fn align8(v: &mut Vec<u8>) {
        while !v.len().is_multiple_of(8) {
            v.push(0);
        }
    }

    fn resident(ty: u32, value: &[u8]) -> Vec<u8> {
        let mut a = Vec::new();
        a.extend_from_slice(&ty.to_le_bytes());
        a.extend_from_slice(&0u32.to_le_bytes());
        a.extend_from_slice(&[0, 0, 0x18, 0, 0, 0, 0, 0]);
        a.extend_from_slice(&(value.len() as u32).to_le_bytes());
        a.extend_from_slice(&0x18u16.to_le_bytes());
        a.extend_from_slice(&[0, 0]);
        a.extend_from_slice(value);
        align8(&mut a);
        let len = a.len() as u32;
        a[4..8].copy_from_slice(&len.to_le_bytes());
        a
    }

    pub fn std_info(ctime: i64, mtime: i64, atime: i64, attrs: u32) -> Vec<u8> {
        let mut v = vec![0u8; 0x30];
        v[0..8].copy_from_slice(&ctime.to_le_bytes());
        v[8..16].copy_from_slice(&mtime.to_le_bytes());
        v[0x18..0x20].copy_from_slice(&atime.to_le_bytes());
        v[0x20..0x24].copy_from_slice(&attrs.to_le_bytes());
        resident(ATTR_STANDARD_INFORMATION, &v)
    }

    pub fn file_name(parent: u64, name: &str, ns: u8) -> Vec<u8> {
        let units: Vec<u16> = name.encode_utf16().collect();
        let mut v = vec![0u8; 0x42];
        v[0..8].copy_from_slice(&(parent | (1 << 48)).to_le_bytes());
        v[0x40] = units.len() as u8;
        v[0x41] = ns;
        for u in units {
            v.extend_from_slice(&u.to_le_bytes());
        }
        resident(ATTR_FILE_NAME, &v)
    }

    pub fn data_resident(len: usize) -> Vec<u8> {
        resident(ATTR_DATA, &vec![7u8; len])
    }

    pub fn attr_list() -> Vec<u8> {
        resident(ATTR_ATTRIBUTE_LIST, &[0u8; 32])
    }

    pub fn data_nonresident(
        start_vcn: u64,
        alloc: u64,
        real: u64,
        compressed: Option<u64>,
        runs: &[u8],
    ) -> Vec<u8> {
        let mut a = vec![0u8; 0x48];
        a[0..4].copy_from_slice(&ATTR_DATA.to_le_bytes());
        a[8] = 1;
        a[0x0A..0x0C].copy_from_slice(&0x40u16.to_le_bytes());
        a[0x10..0x18].copy_from_slice(&start_vcn.to_le_bytes());
        a[0x20..0x22].copy_from_slice(&0x48u16.to_le_bytes());
        if let Some(c) = compressed {
            a[0x0C..0x0E].copy_from_slice(&ATTR_FLAG_COMPRESSED.to_le_bytes());
            a[0x22..0x24].copy_from_slice(&4u16.to_le_bytes());
            a[0x40..0x48].copy_from_slice(&c.to_le_bytes());
        }
        a[0x28..0x30].copy_from_slice(&alloc.to_le_bytes());
        a[0x30..0x38].copy_from_slice(&real.to_le_bytes());
        a[0x38..0x40].copy_from_slice(&real.to_le_bytes());
        a.extend_from_slice(runs);
        align8(&mut a);
        let len = a.len() as u32;
        a[4..8].copy_from_slice(&len.to_le_bytes());
        a
    }

    pub struct Rec {
        pub flags: u16,
        pub base: u64,
        pub attrs: Vec<Vec<u8>>,
    }

    impl Rec {
        pub fn file(attrs: Vec<Vec<u8>>) -> Self {
            Rec {
                flags: FLAG_IN_USE,
                base: 0,
                attrs,
            }
        }

        pub fn dir(attrs: Vec<Vec<u8>>) -> Self {
            Rec {
                flags: FLAG_IN_USE | FLAG_DIR,
                base: 0,
                attrs,
            }
        }

        pub fn extension(base: u64, attrs: Vec<Vec<u8>>) -> Self {
            Rec {
                flags: FLAG_IN_USE,
                base: base | (1 << 48),
                attrs,
            }
        }

        /// Raw record with the update sequence array applied.
        pub fn build(&self) -> Vec<u8> {
            let mut r = vec![0u8; RS];
            r[..4].copy_from_slice(b"FILE");
            r[4..6].copy_from_slice(&0x30u16.to_le_bytes());
            let usa_count = (RS / SECTOR + 1) as u16;
            r[6..8].copy_from_slice(&usa_count.to_le_bytes());
            r[0x14..0x16].copy_from_slice(&0x38u16.to_le_bytes());
            r[0x16..0x18].copy_from_slice(&self.flags.to_le_bytes());
            r[0x20..0x28].copy_from_slice(&self.base.to_le_bytes());
            let mut off = 0x38;
            for a in &self.attrs {
                r[off..off + a.len()].copy_from_slice(a);
                off += a.len();
            }
            r[off..off + 4].copy_from_slice(&ATTR_END.to_le_bytes());
            off += 8;
            r[0x18..0x1C].copy_from_slice(&(off as u32).to_le_bytes());
            r[0x1C..0x20].copy_from_slice(&(RS as u32).to_le_bytes());
            let usn = [0x34u8, 0x12];
            r[0x30..0x32].copy_from_slice(&usn);
            for i in 1..usa_count as usize {
                let end = i * SECTOR;
                let saved = [r[end - 2], r[end - 1]];
                r[0x30 + i * 2..0x32 + i * 2].copy_from_slice(&saved);
                r[end - 2..end].copy_from_slice(&usn);
            }
            r
        }
    }
}

#[cfg(test)]
mod tests {
    use super::testutil::*;
    use super::*;

    fn name_of(rec: &[u8], p: &Parsed) -> String {
        let units: Vec<u16> = rec[p.name.as_ref().unwrap().name.clone()]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|c| u16::from_le_bytes(*c))
            .collect();
        String::from_utf16(&units).unwrap()
    }

    #[test]
    fn fixups_good_and_corrupted() {
        let raw = Rec::file(vec![
            std_info(1, 2, 3, 0x20),
            file_name(5, "a.txt", 1),
            data_resident(10),
        ])
        .build();
        assert_eq!(&raw[510..512], &[0x34, 0x12]);
        let mut good = raw.clone();
        apply_fixups(&mut good).unwrap();
        assert_eq!(&good[510..512], &raw[0x32..0x34]);
        assert_eq!(&good[1022..1024], &raw[0x34..0x36]);

        let mut torn = raw.clone();
        torn[1023] ^= 0xFF;
        assert_eq!(parse_record(&mut torn), Err(RecordError::BadFixup));
        let mut sig = raw.clone();
        sig[..4].copy_from_slice(b"BAAD");
        assert_eq!(parse_record(&mut sig), Err(RecordError::BadSignature));
        assert_eq!(
            parse_record(&mut vec![0u8; RS]),
            Err(RecordError::BadSignature)
        );
    }

    #[test]
    fn fixed_up_bytes_reach_attributes() {
        // a name long enough to straddle the first sector end (offset 510)
        let long = "x".repeat(200);
        let mut raw = Rec::file(vec![
            std_info(1, 2, 3, 0x20),
            file_name(5, &long, 1),
            data_resident(0),
        ])
        .build();
        let p = parse_record(&mut raw).unwrap();
        assert_eq!(name_of(&raw, &p), long);
    }

    #[test]
    fn standard_information_and_name() {
        let mut raw = Rec::file(vec![
            std_info(11, 22, 33, 0x21),
            file_name(42, "hello.txt", 1),
            data_resident(300),
        ])
        .build();
        let p = parse_record(&mut raw).unwrap();
        assert!(p.in_use() && !p.is_dir() && !p.is_extension());
        assert_eq!(
            p.std,
            Some(StdInfo {
                ctime: 11,
                mtime: 22,
                atime: 33,
                attrs: 0x21
            })
        );
        assert_eq!(p.name.as_ref().unwrap().parent, 42);
        assert_eq!(name_of(&raw, &p), "hello.txt");
        assert_eq!(
            p.data,
            Some(DataInfo {
                logical: 300,
                allocated: 0,
                resident: true,
                runs: 0..0
            })
        );
    }

    #[test]
    fn not_in_use_and_directory_flags() {
        let mut unused = Rec::file(vec![file_name(5, "gone", 1)]);
        unused.flags = 0;
        let p = parse_record(&mut unused.build()).unwrap();
        assert!(!p.in_use() && p.name.is_none());
        let p = parse_record(&mut Rec::dir(vec![file_name(5, "d", 1)]).build()).unwrap();
        assert!(p.is_dir());
    }

    #[test]
    fn non_resident_data_sizes() {
        let runs = [0x21, 0x04, 0x00, 0x10, 0x00];
        let mut raw = Rec::file(vec![
            file_name(5, "big", 1),
            data_nonresident(0, 16384, 10000, None, &runs),
        ])
        .build();
        let d = parse_record(&mut raw).unwrap().data.unwrap();
        assert_eq!((d.logical, d.allocated, d.resident), (10000, 16384, false));
        assert_eq!(&raw[d.runs.clone()][..5], &runs);

        let mut raw = Rec::file(vec![
            file_name(5, "z", 1),
            data_nonresident(0, 65536, 50000, Some(8192), &runs),
        ])
        .build();
        let d = parse_record(&mut raw).unwrap().data.unwrap();
        assert_eq!((d.logical, d.allocated), (50000, 8192));
    }

    #[test]
    fn data_extent_with_nonzero_vcn_is_ignored() {
        let mut raw = Rec::extension(
            100,
            vec![data_nonresident(16, 0, 0, None, &[0x11, 1, 5, 0])],
        )
        .build();
        let p = parse_record(&mut raw).unwrap();
        assert!(p.is_extension() && p.data.is_none());
        assert_eq!(p.base, 100);
    }

    #[test]
    fn multiple_names_pick_first_non_dos() {
        let mut raw = Rec::file(vec![
            file_name(5, "LONGNA~1.TXT", 2),
            file_name(7, "Long name.txt", 1),
            file_name(9, "Link.txt", 1),
        ])
        .build();
        let p = parse_record(&mut raw).unwrap();
        assert_eq!(p.name.as_ref().unwrap().parent, 7);
        assert_eq!(name_of(&raw, &p), "Long name.txt");

        let mut dos_only = Rec::file(vec![file_name(5, "ABC~1", 2)]).build();
        assert!(parse_record(&mut dos_only).unwrap().name.is_none());

        let mut combined = Rec::file(vec![file_name(5, "ABC", 3)]).build();
        let p = parse_record(&mut combined).unwrap();
        assert_eq!(name_of(&combined, &p), "ABC");
    }

    #[test]
    fn attribute_list_flag() {
        let p =
            parse_record(&mut Rec::file(vec![std_info(0, 0, 0, 0), attr_list()]).build()).unwrap();
        assert!(p.has_attr_list);
    }

    #[test]
    fn malformed_attribute_length() {
        let mut bad = file_name(5, "a", 1);
        bad[4..8].copy_from_slice(&0x7000u32.to_le_bytes());
        assert_eq!(
            parse_record(&mut Rec::file(vec![bad]).build()),
            Err(RecordError::Malformed)
        );
    }

    #[test]
    fn run_lists() {
        let runs = decode_runs(&[0x21, 0x18, 0x34, 0x12, 0x00]).unwrap();
        assert_eq!(
            runs,
            vec![Run {
                lcn: Some(0x1234),
                len: 0x18
            }]
        );

        // second run goes backwards (negative offset), third is sparse
        let bytes = [
            0x11, 0x10, 0x50, 0x11, 0x04, 0xF0, 0x01, 0x08, 0x11, 0x02, 0x08, 0x00,
        ];
        let runs = decode_runs(&bytes).unwrap();
        assert_eq!(
            runs,
            vec![
                Run {
                    lcn: Some(0x50),
                    len: 0x10
                },
                Run {
                    lcn: Some(0x40),
                    len: 0x04
                },
                Run { lcn: None, len: 8 },
                Run {
                    lcn: Some(0x48),
                    len: 2
                },
            ]
        );

        // multi-byte length and a 3-byte offset
        let runs = decode_runs(&[0x32, 0x00, 0x01, 0x00, 0x00, 0x01, 0x00]).unwrap();
        assert_eq!(
            runs,
            vec![Run {
                lcn: Some(0x10000),
                len: 0x100
            }]
        );

        assert!(decode_runs(&[0x21, 0x01]).is_err());
        assert!(decode_runs(&[0x01, 0x05, 0x00]).is_ok());
        assert!(
            decode_runs(&[0x11, 0x01, 0x80]).is_err(),
            "negative start LCN"
        );
        assert!(decode_runs(&[0x00]).unwrap().is_empty());
    }
}
