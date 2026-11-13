//! Volume information for a path.

use std::io;
use std::path::Path;
use windows::Win32::Storage::FileSystem::{
    GetDiskFreeSpaceExW, GetDiskFreeSpaceW, GetDriveTypeW, GetLogicalDrives, GetVolumeInformationW,
    GetVolumePathNameW,
};
use windows::core::PCWSTR;

/// Capacity and geometry of the volume holding a path.
#[derive(Clone, Debug)]
pub struct VolumeInfo {
    /// Volume root, e.g. `C:\`.
    pub root: String,
    pub total_bytes: u64,
    pub free_bytes: u64,
    pub cluster_size: u64,
    /// True when the queried path is the volume root itself.
    pub is_volume_root: bool,
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn os_error(e: windows::core::Error) -> io::Error {
    io::Error::from_raw_os_error(e.code().0 & 0xFFFF)
}

/// Queries the volume that holds `path`.
pub fn volume_info(path: &Path) -> io::Result<VolumeInfo> {
    let abs = std::path::absolute(path)?;
    let abs_str = abs.to_string_lossy().into_owned();
    let wpath = wide(&abs_str);

    let mut buf = [0u16; 1024];
    // SAFETY: wpath is NUL-terminated and buf is a valid writable slice.
    unsafe { GetVolumePathNameW(PCWSTR(wpath.as_ptr()), &mut buf) }.map_err(os_error)?;
    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    let root = String::from_utf16_lossy(&buf[..len]);
    let wroot = wide(&root);

    let (mut total, mut free) = (0u64, 0u64);
    // SAFETY: wroot is NUL-terminated and the out pointers reference live locals.
    unsafe {
        GetDiskFreeSpaceExW(
            PCWSTR(wroot.as_ptr()),
            None,
            Some(&mut total),
            Some(&mut free),
        )
    }
    .map_err(os_error)?;

    let (mut sectors, mut bytes, mut _free_clusters, mut _total_clusters) =
        (0u32, 0u32, 0u32, 0u32);
    // SAFETY: wroot is NUL-terminated and the out pointers reference live locals.
    unsafe {
        GetDiskFreeSpaceW(
            PCWSTR(wroot.as_ptr()),
            Some(&mut sectors),
            Some(&mut bytes),
            Some(&mut _free_clusters),
            Some(&mut _total_clusters),
        )
    }
    .map_err(os_error)?;

    let trim = |s: &str| s.trim_end_matches(['\\', '/']).to_ascii_lowercase();
    Ok(VolumeInfo {
        is_volume_root: trim(&abs_str) == trim(&root),
        root,
        total_bytes: total,
        free_bytes: free,
        cluster_size: u64::from(sectors) * u64::from(bytes),
    })
}

/// Kind of a drive.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DriveKind {
    Fixed,
    Removable,
    Network,
    Other,
}

/// A mounted drive with its capacity.
#[derive(Clone, Debug)]
pub struct DriveInfo {
    /// Drive root, e.g. `C:\`.
    pub root: String,
    pub label: String,
    /// File system name, e.g. `NTFS`; empty when unknown.
    pub file_system: String,
    pub kind: DriveKind,
    pub total_bytes: u64,
    pub free_bytes: u64,
}

fn drive_kind(code: u32) -> DriveKind {
    match code {
        3 => DriveKind::Fixed,
        2 => DriveKind::Removable,
        4 => DriveKind::Network,
        _ => DriveKind::Other,
    }
}

fn from_wide(buf: &[u16]) -> String {
    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..len])
}

/// Volume label and file system name, empty when the volume cannot be queried.
fn volume_names(wroot: &[u16]) -> (String, String) {
    let mut label = [0u16; 261];
    let mut fs = [0u16; 64];
    // SAFETY: wroot is NUL-terminated and both buffers are valid writable slices.
    let ok = unsafe {
        GetVolumeInformationW(
            PCWSTR(wroot.as_ptr()),
            Some(&mut label),
            None,
            None,
            None,
            Some(&mut fs),
        )
    };
    if ok.is_err() {
        return (String::new(), String::new());
    }
    (from_wide(&label), from_wide(&fs))
}

fn drive_info(letter: char) -> Option<DriveInfo> {
    let root = format!("{letter}:\\");
    let wroot = wide(&root);
    // SAFETY: wroot is NUL-terminated.
    let kind = drive_kind(unsafe { GetDriveTypeW(PCWSTR(wroot.as_ptr())) });
    let (mut total, mut free) = (0u64, 0u64);
    // SAFETY: wroot is NUL-terminated and the out pointers reference live locals.
    unsafe {
        GetDiskFreeSpaceExW(
            PCWSTR(wroot.as_ptr()),
            None,
            Some(&mut total),
            Some(&mut free),
        )
    }
    .ok()?;
    let (label, file_system) = volume_names(&wroot);
    Some(DriveInfo {
        label,
        file_system,
        root,
        kind,
        total_bytes: total,
        free_bytes: free,
    })
}

impl DriveInfo {
    /// Whether the MFT backend can read this drive when elevated: a local fixed or removable NTFS volume.
    pub fn mft_capable(&self) -> bool {
        matches!(self.kind, DriveKind::Fixed | DriveKind::Removable)
            && self.file_system.eq_ignore_ascii_case("NTFS")
    }
}

/// Lists the mounted drives; drives that cannot be queried are skipped.
pub fn list_drives() -> Vec<DriveInfo> {
    // SAFETY: no arguments.
    let mask = unsafe { GetLogicalDrives() };
    ('A'..='Z')
        .enumerate()
        .filter(|(i, _)| mask & (1 << i) != 0)
        .filter_map(|(_, letter)| drive_info(letter))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_system_drive() {
        let drives = list_drives();
        assert!(
            drives
                .iter()
                .any(|d| d.kind == DriveKind::Fixed && d.total_bytes > 0)
        );
    }
}
