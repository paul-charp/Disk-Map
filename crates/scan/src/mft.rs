//! Raw NTFS scan backend: reads the Master File Table straight from the volume.
//!
//! Needs administrator rights (to open `\\.\X:`) and an NTFS volume; see [`mft_available`].
//! Differences from the walk backend, by design:
//! * NTFS metadata files (records below 16 other than the root, and everything under `$Extend`) are skipped.
//! * A file with several hard links appears once, under the parent of its first Win32/POSIX name.
//! * Reparse points keep their attribute bit with zero size and are never descended into, so a
//!   junction's target files belong to their real parent only.
//! * Named `$DATA` streams are ignored.

mod record;

use crate::event::{DirBatch, DirToken, Entry, ScanEvent, Scanner};
use crate::walk::{absolute_root, default_threads};
use crossbeam_channel::{Sender, bounded};
use rayon::ThreadPoolBuilder;
use record::{RecordError, parse_record};
use std::collections::VecDeque;
use std::ffi::c_void;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use windows::Win32::Foundation::{CloseHandle, GENERIC_READ, HANDLE};
use windows::Win32::Security::{GetTokenInformation, TOKEN_ELEVATION, TOKEN_QUERY, TokenElevation};
use windows::Win32::Storage::FileSystem::{
    BY_HANDLE_FILE_INFORMATION, CreateFileW, FILE_BEGIN, FILE_FLAG_BACKUP_SEMANTICS,
    FILE_FLAG_SEQUENTIAL_SCAN, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, GetDriveTypeW,
    GetFileInformationByHandle, GetVolumeInformationW, GetVolumeNameForVolumeMountPointW,
    GetVolumePathNameW, OPEN_EXISTING, ReadFile, SetFilePointerEx,
};
use windows::Win32::System::IO::DeviceIoControl;
use windows::Win32::System::Ioctl::{FSCTL_GET_NTFS_VOLUME_DATA, NTFS_VOLUME_DATA_BUFFER};
use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
use windows::core::PCWSTR;

const CHUNK_BYTES: usize = 4 << 20;
const FIRST_USER_RECORD: usize = 16;
const ATTR_DIRECTORY: u32 = 0x10;
const ATTR_NORMAL: u32 = 0x80;
const ATTR_REPARSE: u32 = 0x400;
const ERROR_INVALID_DATA: u32 = 13;

const F_USED: u8 = 1;
const F_DIR: u8 = 2;
const F_STD: u8 = 4;
const F_NAME: u8 = 8;
const F_DATA: u8 = 16;

/// Whether the MFT backend can scan a path.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MftSupport {
    Available,
    NotElevated,
    NotNtfs,
    NotLocal,
}

impl MftSupport {
    /// Short human-readable reason, empty when available.
    pub fn reason(self) -> &'static str {
        match self {
            MftSupport::Available => "",
            MftSupport::NotElevated => "administrator rights are required",
            MftSupport::NotNtfs => "the volume is not NTFS",
            MftSupport::NotLocal => "the volume is not a local fixed or removable drive",
        }
    }
}

/// Scan backend in use.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Backend {
    Mft,
    Walk,
}

impl Backend {
    /// Display name: `MFT` or `Walk`.
    pub fn name(self) -> &'static str {
        match self {
            Backend::Mft => "MFT",
            Backend::Walk => "Walk",
        }
    }
}

impl fmt::Display for Backend {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// Picks the MFT backend when it is available for `path`, the walk otherwise.
pub fn auto_scanner(path: &Path, threads: Option<usize>) -> (Box<dyn Scanner>, Backend) {
    if mft_available(path) == MftSupport::Available {
        (Box::new(MftScanner::new(threads)), Backend::Mft)
    } else {
        (
            Box::new(crate::walk::WalkScanner::new(threads)),
            Backend::Walk,
        )
    }
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn from_wide(buf: &[u16]) -> String {
    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..len])
}

fn volume_root(path: &Path) -> Option<String> {
    let abs = absolute_root(path);
    let wpath = wide(&abs);
    let mut buf = [0u16; 1024];
    // SAFETY: wpath is NUL-terminated and buf is a valid writable slice.
    unsafe { GetVolumePathNameW(PCWSTR(wpath.as_ptr()), &mut buf) }.ok()?;
    Some(from_wide(&buf))
}

/// True when the process token is elevated.
pub fn is_elevated() -> bool {
    let mut token = HANDLE::default();
    // SAFETY: token is a live out parameter.
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) }.is_err() {
        return false;
    }
    let mut elevation = TOKEN_ELEVATION::default();
    let mut returned = 0u32;
    // SAFETY: the buffer is a live TOKEN_ELEVATION and its size is passed.
    let ok = unsafe {
        GetTokenInformation(
            token,
            TokenElevation,
            Some((&mut elevation as *mut TOKEN_ELEVATION).cast::<c_void>()),
            std::mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut returned,
        )
    }
    .is_ok();
    // SAFETY: token was opened above and is closed once.
    let _ = unsafe { CloseHandle(token) };
    ok && elevation.TokenIsElevated != 0
}

/// Checks that the volume holding `path` is local NTFS and that the process is elevated.
pub fn mft_available(path: &Path) -> MftSupport {
    let Some(root) = volume_root(path) else {
        return MftSupport::NotLocal;
    };
    let wroot = wide(&root);
    // SAFETY: wroot is NUL-terminated.
    let kind = unsafe { GetDriveTypeW(PCWSTR(wroot.as_ptr())) };
    if kind != 3 && kind != 2 {
        return MftSupport::NotLocal;
    }
    let mut fs = [0u16; 64];
    // SAFETY: wroot is NUL-terminated and fs is a valid writable slice.
    let ok = unsafe {
        GetVolumeInformationW(
            PCWSTR(wroot.as_ptr()),
            None,
            None,
            None,
            None,
            Some(&mut fs),
        )
    };
    if ok.is_err() || !from_wide(&fs).eq_ignore_ascii_case("NTFS") {
        return MftSupport::NotNtfs;
    }
    if !is_elevated() {
        return MftSupport::NotElevated;
    }
    MftSupport::Available
}

/// Counters filled while a scan runs; share them before boxing the scanner.
#[derive(Debug, Default)]
pub struct MftStats {
    /// File records examined (in use or not).
    pub records_read: AtomicU64,
    /// File records in use.
    pub records_in_use: AtomicU64,
    /// Records rejected (failed fixups, malformed).
    pub records_bad: AtomicU64,
    /// Bytes read from the volume.
    pub bytes_read: AtomicU64,
}

/// MFT scan backend.
pub struct MftScanner {
    threads: usize,
    stats: Arc<MftStats>,
}

impl MftScanner {
    /// Creates a scanner parsing with `threads` workers, or the default when `None`.
    pub fn new(threads: Option<usize>) -> Self {
        MftScanner {
            threads: threads.filter(|&n| n > 0).unwrap_or_else(default_threads),
            stats: Arc::new(MftStats::default()),
        }
    }

    /// Counters of this scanner's run.
    pub fn stats(&self) -> Arc<MftStats> {
        self.stats.clone()
    }
}

impl Default for MftScanner {
    fn default() -> Self {
        Self::new(None)
    }
}

#[derive(Debug)]
enum MftError {
    Os { what: &'static str, code: u32 },
    Format(String),
}

impl MftError {
    fn code(&self) -> u32 {
        match self {
            MftError::Os { code, .. } => *code,
            MftError::Format(_) => ERROR_INVALID_DATA,
        }
    }

    fn os(what: &'static str) -> impl FnOnce(windows::core::Error) -> MftError {
        move |e| {
            let hr = e.code().0 as u32;
            MftError::Os {
                what,
                code: if hr >> 16 == 0x8007 { hr & 0xFFFF } else { hr },
            }
        }
    }
}

impl fmt::Display for MftError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MftError::Os { what, code } => write!(f, "{what} failed (error {code})"),
            MftError::Format(msg) => f.write_str(msg),
        }
    }
}

/// Compact per-record data; extension records are merged into their base record.
#[derive(Clone, Copy, Debug, Default)]
struct Slot {
    logical: u64,
    allocated: u64,
    ctime: i64,
    mtime: i64,
    atime: i64,
    parent: u32,
    name_off: u32,
    attrs: u32,
    name_len: u16,
    flags: u8,
}

impl Slot {
    fn used(&self) -> bool {
        self.flags & F_USED != 0
    }

    fn is_dir(&self) -> bool {
        self.flags & F_DIR != 0
    }
}

#[derive(Default)]
struct ChunkOut {
    slots: Vec<Slot>,
    names: Vec<u16>,
    ext: Vec<(u32, Slot)>,
    in_use: u64,
    bad: u64,
}

#[derive(Default)]
struct Table {
    slots: Vec<Slot>,
    names: Vec<u16>,
    ext: Vec<(u32, Slot)>,
}

fn slot_of(p: &record::Parsed, rec: &[u8], names: &mut Vec<u16>) -> Slot {
    let mut s = Slot {
        flags: F_USED,
        ..Slot::default()
    };
    if p.is_dir() {
        s.flags |= F_DIR;
    }
    if let Some(std) = &p.std {
        s.flags |= F_STD;
        s.ctime = std.ctime;
        s.mtime = std.mtime;
        s.atime = std.atime;
        s.attrs = std.attrs;
    }
    if let Some(n) = &p.name {
        s.flags |= F_NAME;
        s.parent = u32::try_from(n.parent).unwrap_or(u32::MAX);
        s.name_off = names.len() as u32;
        s.name_len = (n.name.len() / 2) as u16;
        names.extend(
            rec[n.name.clone()]
                .as_chunks::<2>()
                .0
                .iter()
                .map(|c| u16::from_le_bytes(*c)),
        );
    }
    if let Some(d) = &p.data {
        s.flags |= F_DATA;
        s.logical = d.logical;
        s.allocated = d.allocated;
    }
    s
}

/// Parses consecutive raw records of size `rs`; chunk-local name offsets.
fn parse_chunk(buf: &mut [u8], rs: usize) -> ChunkOut {
    let mut out = ChunkOut {
        slots: Vec::with_capacity(buf.len() / rs),
        ..ChunkOut::default()
    };
    for rec in buf.chunks_exact_mut(rs) {
        match parse_record(rec) {
            Ok(p) if p.in_use() => {
                out.in_use += 1;
                let slot = slot_of(&p, rec, &mut out.names);
                if p.is_extension() {
                    out.ext
                        .push((u32::try_from(p.base).unwrap_or(u32::MAX), slot));
                    out.slots.push(Slot::default());
                } else {
                    out.slots.push(slot);
                }
            }
            Ok(_) | Err(RecordError::BadSignature) => out.slots.push(Slot::default()),
            Err(_) => {
                out.bad += 1;
                out.slots.push(Slot::default());
            }
        }
    }
    out
}

impl Table {
    fn with_records(n: usize) -> Self {
        Table {
            slots: vec![Slot::default(); n],
            ..Table::default()
        }
    }

    fn insert_chunk(&mut self, first: usize, mut out: ChunkOut) {
        let base = self.names.len() as u32;
        for s in out
            .slots
            .iter_mut()
            .chain(out.ext.iter_mut().map(|(_, s)| s))
        {
            if s.flags & F_NAME != 0 {
                s.name_off += base;
            }
        }
        let end = (first + out.slots.len()).min(self.slots.len());
        self.slots[first..end].copy_from_slice(&out.slots[..end - first]);
        self.names.append(&mut out.names);
        self.ext.append(&mut out.ext);
    }

    /// Merges extension records into their base records.
    fn merge_extensions(&mut self) {
        for (base, ext) in std::mem::take(&mut self.ext) {
            let Some(b) = self.slots.get_mut(base as usize).filter(|b| b.used()) else {
                continue;
            };
            if b.flags & F_STD == 0 && ext.flags & F_STD != 0 {
                (b.ctime, b.mtime, b.atime, b.attrs) = (ext.ctime, ext.mtime, ext.atime, ext.attrs);
                b.flags |= F_STD;
            }
            if b.flags & F_NAME == 0 && ext.flags & F_NAME != 0 {
                (b.parent, b.name_off, b.name_len) = (ext.parent, ext.name_off, ext.name_len);
                b.flags |= F_NAME;
            }
            if b.flags & F_DATA == 0 && ext.flags & F_DATA != 0 {
                (b.logical, b.allocated) = (ext.logical, ext.allocated);
                b.flags |= F_DATA;
            }
        }
    }

    fn name(&self, s: &Slot) -> &[u16] {
        &self.names[s.name_off as usize..s.name_off as usize + s.name_len as usize]
    }

    /// Parent index when the record is a user entry hanging under a live directory.
    fn parent_of(&self, rec: usize) -> Option<usize> {
        let s = &self.slots[rec];
        if rec < FIRST_USER_RECORD || !s.used() || s.flags & F_NAME == 0 {
            return None;
        }
        let p = s.parent as usize;
        let parent = self.slots.get(p)?;
        (p != rec && parent.used() && parent.is_dir()).then_some(p)
    }
}

fn entry_attrs(s: &Slot) -> u32 {
    let mut a = s.attrs & !0x3000_0000;
    if s.is_dir() {
        a |= ATTR_DIRECTORY;
    }
    if a == 0 {
        a = ATTR_NORMAL;
    }
    a
}

/// Emits one batch per directory below `root_rec`, breadth first.
fn assemble(
    table: &Table,
    root_rec: usize,
    cancel: &AtomicBool,
    mut emit: impl FnMut(DirBatch),
) -> Result<(), MftError> {
    let n = table.slots.len();
    match table.slots.get(root_rec) {
        Some(s) if s.used() && s.is_dir() => {}
        _ => {
            return Err(MftError::Format(
                "the scan root is not a directory record in the MFT".into(),
            ));
        }
    }
    let mut start = vec![0u32; n + 1];
    for rec in FIRST_USER_RECORD..n {
        if let Some(p) = table.parent_of(rec) {
            start[p + 1] += 1;
        }
    }
    for i in 0..n {
        start[i + 1] += start[i];
    }
    let mut fill = start.clone();
    let mut list = vec![0u32; start[n] as usize];
    for rec in FIRST_USER_RECORD..n {
        if let Some(p) = table.parent_of(rec) {
            list[fill[p] as usize] = rec as u32;
            fill[p] += 1;
        }
    }
    drop(fill);

    let mut queue = VecDeque::from([(root_rec, DirToken::ROOT)]);
    let mut next_token = 1u32;
    while let Some((dir, token)) = queue.pop_front() {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        let kids = &list[start[dir] as usize..start[dir + 1] as usize];
        if kids.is_empty() {
            continue;
        }
        let mut names = String::new();
        let mut entries = Vec::with_capacity(kids.len());
        for &kid in kids {
            let s = &table.slots[kid as usize];
            let from = names.len() as u32;
            names.extend(
                char::decode_utf16(table.name(s).iter().copied()).map(|r| r.unwrap_or('\u{FFFD}')),
            );
            let attrs = entry_attrs(s);
            let reparse = attrs & ATTR_REPARSE != 0;
            let is_dir = s.is_dir();
            let (logical, allocated) = if reparse || is_dir {
                (0, 0)
            } else {
                (s.logical, s.allocated)
            };
            let dir = (is_dir && !reparse).then(|| {
                let t = DirToken(next_token);
                next_token += 1;
                queue.push_back((kid as usize, t));
                t
            });
            entries.push(Entry {
                name: from..names.len() as u32,
                logical,
                allocated,
                ctime: s.ctime,
                mtime: s.mtime,
                atime: s.atime,
                attrs,
                dir,
            });
        }
        emit(DirBatch {
            parent: token,
            names,
            entries,
        });
    }
    Ok(())
}

/// Closes the handle on drop.
struct Handle(HANDLE);

// SAFETY: a file handle may be used from any thread; reads come from one thread at a time.
unsafe impl Send for Handle {}
unsafe impl Sync for Handle {}

impl Handle {
    fn open(
        path: &str,
        access: u32,
        flags: windows::Win32::Storage::FileSystem::FILE_FLAGS_AND_ATTRIBUTES,
    ) -> Result<Self, MftError> {
        let w = wide(path);
        // SAFETY: w is NUL-terminated and outlives the call.
        let h = unsafe {
            CreateFileW(
                PCWSTR(w.as_ptr()),
                access,
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                None,
                OPEN_EXISTING,
                flags,
                None,
            )
        }
        .map_err(MftError::os("opening the volume or root"))?;
        Ok(Handle(h))
    }

    fn read_at(&self, offset: u64, buf: &mut [u8]) -> Result<(), MftError> {
        // SAFETY: the handle is open and the out pointer is null.
        unsafe { SetFilePointerEx(self.0, offset as i64, None, FILE_BEGIN) }
            .map_err(MftError::os("seeking in the volume"))?;
        let mut done = 0usize;
        while done < buf.len() {
            let mut got = 0u32;
            // SAFETY: the slice is valid for the whole call.
            unsafe { ReadFile(self.0, Some(&mut buf[done..]), Some(&mut got), None) }
                .map_err(MftError::os("reading the volume"))?;
            if got == 0 {
                return Err(MftError::Format("unexpected end of the volume".into()));
            }
            done += got as usize;
        }
        Ok(())
    }
}

impl Drop for Handle {
    fn drop(&mut self) {
        // SAFETY: the handle came from CreateFileW and is closed once.
        let _ = unsafe { CloseHandle(self.0) };
    }
}

fn volume_device(root: &str) -> Result<String, MftError> {
    let b = root.as_bytes();
    if b.len() == 3 && b[0].is_ascii_alphabetic() && &root[1..] == ":\\" {
        return Ok(format!(r"\\.\{}:", &root[..1]));
    }
    let w = wide(root);
    let mut buf = [0u16; 128];
    // SAFETY: w is NUL-terminated and buf is a valid writable slice.
    unsafe { GetVolumeNameForVolumeMountPointW(PCWSTR(w.as_ptr()), &mut buf) }
        .map_err(MftError::os("resolving the volume name"))?;
    Ok(from_wide(&buf).trim_end_matches('\\').to_string())
}

fn root_record_number(path: &Path) -> Result<u64, MftError> {
    let h = Handle::open(&absolute_root(path), 0, FILE_FLAG_BACKUP_SEMANTICS)?;
    let mut info = BY_HANDLE_FILE_INFORMATION::default();
    // SAFETY: info is a live out parameter.
    unsafe { GetFileInformationByHandle(h.0, &mut info) }
        .map_err(MftError::os("querying the scan root"))?;
    Ok(
        (u64::from(info.nFileIndexHigh) << 32 | u64::from(info.nFileIndexLow))
            & 0x0000_FFFF_FFFF_FFFF,
    )
}

struct Geometry {
    cluster: u64,
    sector: u64,
    record: usize,
    mft_lcn: u64,
    valid_len: u64,
}

fn volume_geometry(vol: &Handle) -> Result<Geometry, MftError> {
    let mut data = NTFS_VOLUME_DATA_BUFFER::default();
    let mut returned = 0u32;
    // SAFETY: the output buffer is a live NTFS_VOLUME_DATA_BUFFER and its size is passed.
    unsafe {
        DeviceIoControl(
            vol.0,
            FSCTL_GET_NTFS_VOLUME_DATA,
            None,
            0,
            Some((&mut data as *mut NTFS_VOLUME_DATA_BUFFER).cast::<c_void>()),
            std::mem::size_of::<NTFS_VOLUME_DATA_BUFFER>() as u32,
            Some(&mut returned),
            None,
        )
    }
    .map_err(MftError::os("FSCTL_GET_NTFS_VOLUME_DATA"))?;
    let record = data.BytesPerFileRecordSegment as usize;
    if !(512..=65536).contains(&record) || !record.is_power_of_two() {
        return Err(MftError::Format(format!(
            "unexpected file record size {record}"
        )));
    }
    Ok(Geometry {
        cluster: u64::from(data.BytesPerCluster),
        sector: u64::from(data.BytesPerSector).max(512),
        record,
        mft_lcn: data.MftStartLcn.max(0) as u64,
        valid_len: data.MftValidDataLength.max(0) as u64,
    })
}

/// Reads record 0 (`$MFT`) and returns the extents of the whole MFT and its size.
fn mft_extents(vol: &Handle, g: &Geometry) -> Result<(Vec<record::Run>, u64), MftError> {
    let mut buf = vec![0u8; (g.record as u64).next_multiple_of(g.sector) as usize];
    vol.read_at(g.mft_lcn * g.cluster, &mut buf)?;
    let rec = &mut buf[..g.record];
    let parsed = parse_record(rec)
        .map_err(|e| MftError::Format(format!("record 0 ($MFT) is unreadable: {e:?}")))?;
    let data = parsed
        .data
        .filter(|d| !d.resident)
        .ok_or_else(|| MftError::Format("$MFT has no non-resident $DATA".into()))?;
    let runs = record::decode_runs(&rec[data.runs.clone()])
        .map_err(|e| MftError::Format(format!("$MFT run list is unreadable: {e:?}")))?;
    let size = if g.valid_len > 0 {
        g.valid_len.min(data.logical)
    } else {
        data.logical
    };
    let covered: u64 = runs.iter().map(|r| r.len * g.cluster).sum();
    if covered < size {
        return Err(MftError::Format(
            "the MFT run list is split across extension records (not supported)".into(),
        ));
    }
    Ok((runs, size))
}

struct Layout<'a> {
    runs: &'a [record::Run],
    cluster: u64,
    record: usize,
    records: usize,
}

type ReadAt<'a> = &'a (dyn Fn(u64, &mut [u8]) -> Result<(), MftError> + Sync);

/// Reads the MFT extents in `chunk`-sized pieces and parses them in parallel.
/// Returns `None` when cancelled.
fn read_table(
    read: ReadAt,
    layout: &Layout,
    chunk: usize,
    threads: usize,
    cancel: &AtomicBool,
    stats: &MftStats,
) -> Result<Option<Table>, MftError> {
    let (rs, records, cluster) = (layout.record, layout.records, layout.cluster);
    let table = Mutex::new(Table::with_records(records));
    let pool = ThreadPoolBuilder::new()
        .num_threads(threads + 1)
        .build()
        .map_err(|e| MftError::Format(format!("cannot start the thread pool: {e}")))?;
    let (permit_tx, permit_rx) = bounded::<()>(threads * 2);
    for _ in 0..threads * 2 {
        let _ = permit_tx.send(());
    }
    let read_result = pool.scope(|scope| -> Result<(), MftError> {
        let mut pos = 0u64;
        let mut carry: Vec<u8> = Vec::new();
        for r in layout.runs {
            let run_bytes = r.len * cluster;
            let Some(lcn) = r.lcn else {
                pos += run_bytes;
                carry.clear();
                continue;
            };
            let mut done = 0u64;
            while done < run_bytes {
                let first = ((pos - carry.len() as u64) / rs as u64) as usize;
                if cancel.load(Ordering::Relaxed) || first >= records {
                    return Ok(());
                }
                let _ = permit_rx.recv();
                let piece = (chunk as u64).min(run_bytes - done) as usize;
                let mut buf = vec![0u8; carry.len() + piece];
                buf[..carry.len()].copy_from_slice(&carry);
                read(lcn * cluster + done, &mut buf[carry.len()..])?;
                stats.bytes_read.fetch_add(piece as u64, Ordering::Relaxed);
                pos += piece as u64;
                done += piece as u64;
                let usable = buf.len() / rs * rs;
                carry = buf[usable..].to_vec();
                let count = (usable / rs).min(records - first);
                buf.truncate(count * rs);
                if count == 0 {
                    let _ = permit_tx.send(());
                    continue;
                }
                let (table, permit_tx) = (&table, &permit_tx);
                scope.spawn(move |_| {
                    let out = parse_chunk(&mut buf, rs);
                    stats
                        .records_read
                        .fetch_add(count as u64, Ordering::Relaxed);
                    stats
                        .records_in_use
                        .fetch_add(out.in_use, Ordering::Relaxed);
                    stats.records_bad.fetch_add(out.bad, Ordering::Relaxed);
                    table.lock().unwrap().insert_chunk(first, out);
                    let _ = permit_tx.send(());
                });
            }
        }
        Ok(())
    });
    read_result?;
    if cancel.load(Ordering::Relaxed) {
        return Ok(None);
    }
    Ok(Some(table.into_inner().unwrap()))
}

fn run(
    root: &Path,
    threads: usize,
    cancel: &AtomicBool,
    stats: &MftStats,
    tx: &Sender<ScanEvent>,
) -> Result<(), MftError> {
    let volume_root = volume_root(root)
        .ok_or_else(|| MftError::Format("cannot resolve the volume of the scan root".into()))?;
    let root_rec = root_record_number(root)?;
    let vol = Handle::open(
        &volume_device(&volume_root)?,
        GENERIC_READ.0,
        FILE_FLAG_SEQUENTIAL_SCAN,
    )?;
    let g = volume_geometry(&vol)?;
    let (runs, mft_size) = mft_extents(&vol, &g)?;
    let rs = g.record;
    let records = (mft_size / rs as u64) as usize;
    if records >= u32::MAX as usize || root_rec as usize >= records {
        return Err(MftError::Format(
            "the MFT is too large or the scan root is outside it".into(),
        ));
    }

    let layout = Layout {
        runs: &runs,
        cluster: g.cluster,
        record: rs,
        records,
    };
    let read = |offset: u64, buf: &mut [u8]| vol.read_at(offset, buf);
    let Some(mut table) = read_table(&read, &layout, CHUNK_BYTES, threads, cancel, stats)? else {
        return Ok(());
    };
    table.merge_extensions();
    assemble(&table, root_rec as usize, cancel, |batch| {
        if tx.send(ScanEvent::Batch(batch)).is_err() {
            cancel.store(true, Ordering::Relaxed);
        }
    })
}

impl Scanner for MftScanner {
    fn start(self: Box<Self>, root: PathBuf, tx: Sender<ScanEvent>, cancel: Arc<AtomicBool>) {
        std::thread::spawn(move || {
            if let Err(e) = run(&root, self.threads, &cancel, &self.stats, &tx) {
                let _ = tx.send(ScanEvent::Error {
                    path: format!("{}: MFT scan: {e}", absolute_root(&root)),
                    code: e.code(),
                });
            }
            let _ = tx.send(ScanEvent::Done {
                cancelled: cancel.load(Ordering::Relaxed),
            });
        });
    }
}

#[cfg(test)]
mod tests {
    use super::record::testutil::*;
    use super::*;
    use crate::apply::TreeBuilder;
    use diskmap_core::tree::{NodeKind, Tree};

    const RUNS: [u8; 4] = [0x11, 0x04, 0x10, 0x00];

    fn table_of(recs: Vec<(usize, Rec)>) -> Table {
        let n = recs.iter().map(|(i, _)| i + 1).max().unwrap();
        let mut buf = vec![0u8; n * RS];
        for (i, r) in recs {
            buf[i * RS..(i + 1) * RS].copy_from_slice(&r.build());
        }
        let mut table = Table::with_records(n);
        table.insert_chunk(0, parse_chunk(&mut buf, RS));
        table.merge_extensions();
        table
    }

    fn named(parent: u64, name: &str, attrs: u32) -> Vec<Vec<u8>> {
        vec![std_info(10, 20, 30, attrs), file_name(parent, name, 1)]
    }

    fn file(parent: u64, name: &str, size: Option<(u64, u64)>) -> Rec {
        let mut a = named(parent, name, 0x20);
        a.push(match size {
            Some((l, al)) => data_nonresident(0, al, l, None, &RUNS),
            None => data_resident(100),
        });
        Rec::file(a)
    }

    fn dir(parent: u64, name: &str) -> Rec {
        Rec::dir(named(parent, name, 0))
    }

    fn build(table: &Table, root: usize) -> Tree {
        let mut b = TreeBuilder::new("X:\\");
        let cancel = AtomicBool::new(false);
        assemble(table, root, &cancel, |batch| b.apply_batch(&batch)).unwrap();
        b.into_tree()
    }

    fn sample() -> Table {
        let mut hard = named(16, "x.bin", 0x20);
        hard.push(file_name(5, "y.bin", 1));
        hard.push(data_nonresident(0, 8192, 5000, None, &RUNS));
        let mut split = named(16, "split.dat", 0x20);
        split.push(attr_list());
        table_of(vec![
            (5, dir(5, ".")),
            (3, file(5, "$Bitmap", Some((9000, 12288)))),
            (11, dir(5, "$Extend")),
            (25, file(11, "$UsnJrnl", Some((7, 4096)))),
            (16, dir(5, "dirA")),
            (17, file(16, "a.txt", Some((10000, 12288)))),
            (18, file(5, "b.txt", None)),
            (19, Rec::file(hard)),
            (20, Rec::file(split)),
            (
                21,
                Rec::extension(20, vec![data_nonresident(0, 4096, 4000, None, &RUNS)]),
            ),
            (
                22,
                Rec::file(vec![
                    file_name(16, "DOSNAM~1.TXT", 2),
                    file_name(16, "Dos Name.txt", 1),
                    data_nonresident(0, 4096, 1, None, &RUNS),
                ]),
            ),
            (23, file(99, "orphan", Some((1, 4096)))),
            (24, file(12, "stale", Some((1, 4096)))),
            (26, Rec::dir(named(5, "junction", 0x410))),
            (27, file(26, "behind.bin", Some((100, 4096)))),
            (28, dir(16, "sub")),
            (29, file(28, "deep.bin", Some((3, 4096)))),
            (
                30,
                Rec::file(vec![
                    file_name(5, "packed", 1),
                    data_nonresident(0, 65536, 60000, Some(8192), &RUNS),
                ]),
            ),
            (
                31,
                Rec::file(vec![std_info(0, 0, 0, 0), file_name(5, "plain", 1)]),
            ),
        ])
    }

    #[test]
    fn assembles_tree_from_fake_records() {
        let t = build(&sample(), 5);
        let r = t.root();
        let sizes = |p: &str| {
            let n = t.get(t.find_path(p).unwrap_or_else(|| panic!("missing {p}")));
            (n.logical, n.allocated)
        };
        assert_eq!(sizes("dirA\\a.txt"), (10000, 12288));
        assert_eq!(sizes("b.txt"), (100, 0));
        assert_eq!(sizes("dirA\\x.bin"), (5000, 8192));
        assert!(t.find_path("y.bin").is_none(), "hard link counted once");
        assert_eq!(sizes("dirA\\split.dat"), (4000, 4096));
        assert_eq!(sizes("dirA\\Dos Name.txt"), (1, 4096));
        assert!(t.find_path("dirA\\DOSNAM~1.TXT").is_none());
        assert_eq!(sizes("dirA\\sub\\deep.bin"), (3, 4096));
        assert_eq!(sizes("packed"), (60000, 8192));
        assert_eq!(sizes("plain"), (0, 0));
        for gone in ["$Bitmap", "$Extend", "orphan", "stale"] {
            assert!(t.find_path(gone).is_none(), "{gone}");
        }
        let junction = t.find_path("junction").unwrap();
        assert_eq!(t.get(junction).kind, NodeKind::Dir, "junction is a leaf");
        assert_eq!(t.get(junction).child_count(), 0);
        assert_eq!(t.get(junction).attrs & 0x400, 0x400);
        assert!(t.find_path("junction\\behind.bin").is_none());
        assert_eq!(t.get(t.find_path("plain").unwrap()).attrs, 0x80);
        assert_eq!(t.get(t.find_path("dirA").unwrap()).attrs & 0x10, 0x10);
        assert_eq!(t.times(t.find_path("b.txt").unwrap()).mtime, 20);

        let total_logical = 10000 + 100 + 5000 + 4000 + 1 + 3 + 60000;
        assert_eq!(t.get(r).logical, total_logical);
        assert_eq!(t.get(r).allocated, 12288 + 8192 + 4096 + 4096 + 4096 + 8192);
        assert_eq!(t.file_count(), 8);
        assert_eq!(t.dir_count(), 3);
    }

    #[test]
    fn subtree_root_scans_only_its_descendants() {
        let t = build(&sample(), 16);
        assert!(t.find_path("a.txt").is_some());
        assert!(t.find_path("sub\\deep.bin").is_some());
        assert!(t.find_path("b.txt").is_none());
        assert_eq!(t.file_count(), 5);
    }

    #[test]
    fn root_must_be_a_directory() {
        let table = sample();
        let cancel = AtomicBool::new(false);
        assert!(assemble(&table, 17, &cancel, |_| {}).is_err());
        assert!(assemble(&table, 500, &cancel, |_| {}).is_err());
    }

    #[test]
    fn cancel_stops_emission() {
        let cancel = AtomicBool::new(true);
        let mut batches = 0;
        assemble(&sample(), 5, &cancel, |_| batches += 1).unwrap();
        assert_eq!(batches, 0);
    }

    #[test]
    fn extension_without_live_base_is_dropped() {
        let table = table_of(vec![
            (5, dir(5, ".")),
            (
                16,
                Rec::extension(17, vec![data_nonresident(0, 4096, 4000, None, &RUNS)]),
            ),
        ]);
        assert_eq!(build(&table, 5).file_count(), 0);
    }

    #[test]
    fn chunks_merge_with_name_offsets() {
        let mut buf = vec![0u8; 4 * RS];
        for (i, r) in [
            (0, dir(5, ".")),
            (1, dir(5, "alpha")),
            (2, file(1, "beta.txt", None)),
            (3, file(1, "gamma.txt", None)),
        ] {
            buf[i * RS..(i + 1) * RS].copy_from_slice(&r.build());
        }
        let (a, b) = buf.split_at_mut(2 * RS);
        let mut table = Table::with_records(4);
        table.insert_chunk(2, parse_chunk(b, RS));
        table.insert_chunk(0, parse_chunk(a, RS));
        let names: Vec<String> = (0..4)
            .map(|i| String::from_utf16_lossy(table.name(&table.slots[i])))
            .collect();
        assert_eq!(names, [".", "alpha", "beta.txt", "gamma.txt"]);
    }

    #[test]
    fn bad_records_are_counted_not_fatal() {
        let mut buf = vec![0u8; 3 * RS];
        buf[..RS].copy_from_slice(&dir(5, ".").build());
        let mut torn = file(5, "torn", None).build();
        torn[RS - 1] ^= 0xFF;
        buf[RS..2 * RS].copy_from_slice(&torn);
        let out = parse_chunk(&mut buf, RS);
        assert_eq!((out.in_use, out.bad), (1, 1));
        assert_eq!(out.slots.len(), 3);
    }

    #[test]
    fn reads_fragmented_unaligned_extents() {
        // 512-byte clusters, 1024-byte records: record 1 straddles the first two extents,
        // a sparse gap sits in the middle, and 1536-byte chunks exercise the carry logic.
        let n = 24;
        let cluster = 512usize;
        let mut stream = vec![0u8; n * RS];
        let mut recs = vec![
            (1usize, file(5, "straddle", None)),
            (5, dir(5, ".")),
            (16, dir(5, "alpha")),
        ];
        for i in 17..24 {
            recs.push((
                i,
                file(16, &format!("f{i}.bin"), Some((i as u64 * 100, 4096))),
            ));
        }
        for (i, r) in &recs {
            stream[i * RS..(i + 1) * RS].copy_from_slice(&r.build());
        }
        let mut disk = vec![0u8; 200 * cluster];
        let mut runs = Vec::new();
        for (from, len, lcn) in [
            (0usize, 3usize, Some(40u64)),
            (3, 13, Some(100)),
            (16, 4, None),
            (20, 28, Some(7)),
        ] {
            if let Some(l) = lcn {
                let src = &stream[from * cluster..(from + len) * cluster];
                disk[l as usize * cluster..][..src.len()].copy_from_slice(src);
            }
            runs.push(record::Run {
                lcn,
                len: len as u64,
            });
        }
        let read = |off: u64, buf: &mut [u8]| {
            buf.copy_from_slice(&disk[off as usize..off as usize + buf.len()]);
            Ok(())
        };
        let layout = Layout {
            runs: &runs,
            cluster: cluster as u64,
            record: RS,
            records: n,
        };
        let stats = MftStats::default();
        let cancel = AtomicBool::new(false);
        let mut table = read_table(&read, &layout, 1536, 3, &cancel, &stats)
            .unwrap()
            .unwrap();
        table.merge_extensions();
        assert_eq!(
            stats.records_read.load(Ordering::Relaxed),
            22,
            "the sparse gap holds two records"
        );
        assert_eq!(stats.records_in_use.load(Ordering::Relaxed), 10);
        assert_eq!(
            stats.bytes_read.load(Ordering::Relaxed),
            44 * cluster as u64
        );
        assert_eq!(
            String::from_utf16_lossy(table.name(&table.slots[1])),
            "straddle"
        );
        let t = build(&table, 5);
        assert_eq!(t.file_count(), 7);
        assert_eq!(
            t.get(t.find_path("alpha").unwrap()).logical,
            (17..24).map(|i| i * 100).sum::<u64>()
        );

        cancel.store(true, Ordering::Relaxed);
        assert!(
            read_table(&read, &layout, 1536, 3, &cancel, &stats)
                .unwrap()
                .is_none()
        );
        let failing = |_: u64, _: &mut [u8]| Err(MftError::Format("boom".into()));
        cancel.store(false, Ordering::Relaxed);
        assert!(read_table(&failing, &layout, 1536, 3, &cancel, &stats).is_err());
    }

    #[test]
    fn support_checks_do_not_panic() {
        let _ = mft_available(Path::new("."));
        let _ = is_elevated();
        assert_eq!(Backend::Mft.name(), "MFT");
        assert_eq!(Backend::Walk.to_string(), "Walk");
    }

    /// Needs an administrator shell: scans a real folder with both backends and compares.
    /// Set `DISKMAP_MFT_TEST_PATH` to choose the folder (default `C:\Windows\System32\drivers`).
    #[test]
    #[ignore = "requires administrator rights and a real NTFS volume"]
    fn matches_walk_on_real_volume() {
        use crate::walk::WalkScanner;
        let path = PathBuf::from(
            std::env::var("DISKMAP_MFT_TEST_PATH")
                .unwrap_or_else(|_| r"C:\Windows\System32\drivers".into()),
        );
        assert_eq!(
            mft_available(&path),
            MftSupport::Available,
            "run from an elevated shell on NTFS"
        );
        let scan = |scanner: Box<dyn Scanner>| {
            let (tx, rx) = crossbeam_channel::unbounded();
            let mut b = TreeBuilder::new(&absolute_root(&path));
            scanner.start(path.clone(), tx, Arc::new(AtomicBool::new(false)));
            let result = b.drain(&rx);
            (b.into_tree(), result)
        };
        let (walk, _) = scan(Box::new(WalkScanner::new(None)));
        let (mft, result) = scan(Box::new(MftScanner::new(None)));
        assert!(result.errors.is_empty(), "{:?}", result.errors);
        let close = |a: u64, b: u64| a.abs_diff(b) * 100 <= a.max(b).max(1);
        let (w, m) = (walk.get(walk.root()), mft.get(mft.root()));
        assert!(
            close(walk.file_count(), mft.file_count()),
            "files {} vs {}",
            walk.file_count(),
            mft.file_count()
        );
        assert!(
            close(walk.dir_count(), mft.dir_count()),
            "dirs {} vs {}",
            walk.dir_count(),
            mft.dir_count()
        );
        assert!(
            close(w.logical, m.logical),
            "logical {} vs {}",
            w.logical,
            m.logical
        );
        assert!(
            close(w.allocated, m.allocated),
            "allocated {} vs {}",
            w.allocated,
            m.allocated
        );
    }
}
