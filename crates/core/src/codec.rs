//! Varint byte codec shared by the snapshot and timeline formats.

use std::fmt;

/// Error when decoding a snapshot or a timeline point.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    /// The data does not start with the expected magic bytes.
    BadMagic,
    /// The format version is not supported.
    Version(u16),
    /// The data ends before the format says it should.
    Truncated,
    /// The data is malformed.
    Invalid(&'static str),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::BadMagic => f.write_str("not a diskmap file"),
            Error::Version(v) => write!(f, "unsupported format version {v}"),
            Error::Truncated => f.write_str("data is truncated"),
            Error::Invalid(what) => write!(f, "invalid data: {what}"),
        }
    }
}

impl std::error::Error for Error {}

pub(crate) fn put_u64(out: &mut Vec<u8>, mut v: u64) {
    while v >= 0x80 {
        out.push(v as u8 | 0x80);
        v >>= 7;
    }
    out.push(v as u8);
}

pub(crate) fn put_i64(out: &mut Vec<u8>, v: i64) {
    put_u64(out, ((v << 1) ^ (v >> 63)) as u64);
}

pub(crate) fn put_str(out: &mut Vec<u8>, s: &str) {
    put_u64(out, s.len() as u64);
    out.extend_from_slice(s.as_bytes());
}

/// Writes the magic and the version.
pub(crate) fn put_header(out: &mut Vec<u8>, magic: &[u8; 4], version: u16) {
    out.extend_from_slice(magic);
    out.extend_from_slice(&version.to_le_bytes());
}

/// Cursor over an encoded buffer.
pub(crate) struct Reader<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    /// Checks the magic and the version and returns a reader positioned after them.
    pub(crate) fn open(buf: &'a [u8], magic: &[u8; 4], version: u16) -> Result<Self, Error> {
        let mut r = Reader { buf, pos: 0 };
        if r.bytes(4).map_err(|_| Error::BadMagic)? != magic {
            return Err(Error::BadMagic);
        }
        let b = r.bytes(2)?;
        let v = u16::from_le_bytes([b[0], b[1]]);
        if v != version {
            return Err(Error::Version(v));
        }
        Ok(r)
    }

    pub(crate) fn is_at_end(&self) -> bool {
        self.pos == self.buf.len()
    }

    pub(crate) fn bytes(&mut self, n: usize) -> Result<&'a [u8], Error> {
        let end = self.pos.checked_add(n).ok_or(Error::Truncated)?;
        let s = self.buf.get(self.pos..end).ok_or(Error::Truncated)?;
        self.pos = end;
        Ok(s)
    }

    pub(crate) fn u8(&mut self) -> Result<u8, Error> {
        Ok(self.bytes(1)?[0])
    }

    pub(crate) fn u64(&mut self) -> Result<u64, Error> {
        let mut v = 0u64;
        for shift in (0..64).step_by(7) {
            let b = self.u8()?;
            v |= u64::from(b & 0x7f) << shift;
            if b & 0x80 == 0 {
                return Ok(v);
            }
        }
        Err(Error::Invalid("varint too long"))
    }

    pub(crate) fn u32(&mut self) -> Result<u32, Error> {
        u32::try_from(self.u64()?).map_err(|_| Error::Invalid("value out of range"))
    }

    pub(crate) fn i64(&mut self) -> Result<i64, Error> {
        let v = self.u64()?;
        Ok((v >> 1) as i64 ^ -((v & 1) as i64))
    }

    pub(crate) fn str(&mut self) -> Result<&'a str, Error> {
        let len = self.u64()?;
        let len = usize::try_from(len).map_err(|_| Error::Truncated)?;
        std::str::from_utf8(self.bytes(len)?).map_err(|_| Error::Invalid("text is not UTF-8"))
    }
}

/// Lowercase key for case-insensitive name matching, consistent with `Tree::find_child`.
pub(crate) fn fold(name: &str) -> String {
    if name.is_ascii() {
        name.to_ascii_lowercase()
    } else {
        name.to_lowercase()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn varints_round_trip() {
        let mut out = Vec::new();
        put_header(&mut out, b"TEST", 3);
        let us = [0, 1, 127, 128, 300, u32::MAX as u64, u64::MAX];
        let is = [0, -1, 1, i64::MIN, i64::MAX, -123_456_789];
        for &u in &us {
            put_u64(&mut out, u);
        }
        for &i in &is {
            put_i64(&mut out, i);
        }
        put_str(&mut out, "h\u{e9}llo");
        let mut r = Reader::open(&out, b"TEST", 3).unwrap();
        for &u in &us {
            assert_eq!(r.u64().unwrap(), u);
        }
        for &i in &is {
            assert_eq!(r.i64().unwrap(), i);
        }
        assert_eq!(r.str().unwrap(), "h\u{e9}llo");
        assert!(r.is_at_end());
        assert_eq!(r.u8(), Err(Error::Truncated));
        assert!(matches!(
            Reader::open(&out, b"TEST", 4),
            Err(Error::Version(3))
        ));
        assert!(matches!(
            Reader::open(&out, b"NOPE", 3),
            Err(Error::BadMagic)
        ));
        assert!(matches!(
            Reader::open(b"TE", b"TEST", 3),
            Err(Error::BadMagic)
        ));
    }
}
