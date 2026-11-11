# Scanning

`diskmap-scan` has two backends behind the `Scanner` trait. Both send `ScanEvent`s (`Batch`, `Error`, `Done`) and the same `TreeBuilder` turns them into a tree.

## Walk

- `WalkScanner` runs one rayon task per directory on a pool of `threads` workers (default: available parallelism clamped to 2 to 16).
- Each directory is opened with `CreateFileW` and listed with `GetFileInformationByHandleEx` (`FileIdBothDirectoryInfo`, 64 KiB buffer), giving name, logical size (`EndOfFile`), allocated size (`AllocationSize`), timestamps and attributes in one call.
- Batches of at most 4096 entries are sent; sub-directories are queued after their parent batch is sent.
- Reparse points (junctions, symlinks) keep their attributes with size 0 and are not entered.
- A directory that cannot be opened or listed sends an `Error` with the Win32 code; the scan continues.
- Works without elevation on any volume. Hard links count once per directory entry.

## MFT

Source: `crates/scan/src/mft.rs`, `mft/record.rs`.

- Requires an elevated process and a local fixed or removable NTFS volume (`mft_available`).
- Opens `\\.\X:` (or the volume GUID path for a mount point), reads `FSCTL_GET_NTFS_VOLUME_DATA`, then reads record 0 (`$MFT`) to get the extents of the whole MFT from its non-resident `$DATA` run list.
- Reads the MFT extents in 4 MiB chunks and parses them in parallel. Each record gets its update sequence fixups checked and applied.
- Attributes used per record:
  - `$STANDARD_INFORMATION`: creation, modification and access times, file attributes.
  - `$FILE_NAME`: parent record and name; the first non-DOS name of the record is used.
  - unnamed `$DATA`: resident size, or non-resident logical and allocated size (compressed size for compressed and sparse streams).
  - `$ATTRIBUTE_LIST` is noted; records that extend another file record are merged into their base record.
- Skipped:
  - NTFS metadata files (records below 16, other than the root, and everything under `$Extend`).
  - Named `$DATA` streams.
  - Records that are not in use, that fail fixups or are malformed (counted as bad), and entries whose parent chain does not reach a live directory (orphans).
- Hard links: a file appears once, under the parent of its first non-DOS `$FILE_NAME`.
- Reparse points keep the attribute with size 0 and are not entered. Directory entries carry size 0; directory totals come from their files.
- The scan root can be any folder of the volume: its record number is read from the folder and the tree is assembled from that record.
- Resident `$DATA` has logical size equal to its length and allocated size 0.
- Failures (no permission, not NTFS, unexpected record size, an MFT run list split across extension records, a scan root outside the MFT) end the scan with an `Error` event naming the cause. There is no automatic switch to the walk after a scan started on the MFT.

## Backend selection and fallbacks

The `backend` setting is `auto`, `mft` or `walk`. `auto` and `mft` call `auto_scanner`: the MFT backend when `mft_available` says so, the walk otherwise. The app does not show an error for that fallback: the status bar shows `Walk`, its tooltip gives the reason, and an Elevate button appears when elevation alone would enable the MFT. Elevate relaunches `diskmap.exe` with `runas` and the paths of the open tabs.

## Watcher and live updater

- `Watcher` opens the scan root with `FILE_FLAG_OVERLAPPED` and loops on `ReadDirectoryChangesW` (recursive; file and folder names, size, last write, creation) with a 64 KiB buffer. Events carry a path relative to the root and `Added`, `Removed`, `Modified` or `Renamed { from }`.
- Overflow, an empty read or a failure to open or arm the watch sends one `Overflow` event. The app then shows Stale and a rescan brings the tree back in line.
- `LiveUpdater::apply` takes a batch of events, removes duplicate paths, sorts them by depth and re-stats each path (`GetFileAttributesExW`).
  - Gone: the node is removed (`Removed`).
  - Present and known: file size and times are updated (`Grew` or `Shrank` when the size changed); a file that became a folder or the opposite is replaced.
  - Present and new: the node is added; a new folder is walked with the walk backend (2 threads) and grafted in.
- Allocated size of a changed or new file is its logical size rounded up to the cluster size of the volume, or `GetCompressedFileSizeW` for compressed and sparse files.

## Differences between the backends

From the code:

- NTFS metadata files (records below 16 and `$Extend`) are never in the MFT tree. Whether the walk lists any of them is not confirmed here.
- Hard links: the MFT backend counts a file once; the walk counts every directory entry, so allocated totals can be larger.
- Named data streams are ignored by the MFT backend; the walk takes the sizes the directory listing reports.
- Access-denied directories raise errors in the walk and have no equivalent in the MFT backend, which reads the volume directly.
- Reparse points are never entered by either backend.
- Resident small files: allocated size 0 in the MFT backend.
- Live updates use a cluster-rounded estimate for allocated size, which can differ from the MFT value for a file that changes.

Not confirmed from the code alone: the exact `AllocationSize` the walk reports for resident files and for files on compressed volumes. Use the comparison below on real data.

## Validating with `diskmap-bench compare`

Open a terminal as administrator, then:

```powershell
cargo build --release -p diskmap-bench
target\release\diskmap-bench.exe compare C:\
target\release\diskmap-bench.exe scan C:\ --backend mft --threads 8
target\release\diskmap-bench.exe watch C:\Temp --for 30
```

- `compare <path> [--threads N]` scans with the walk and then the MFT and prints elapsed time, file and directory counts, logical and allocated bytes, error counts, the MFT record counters, and the ten top-level entries with the largest allocated difference. It exits with 1 when the MFT backend is unavailable.
- `scan <path> [--backend walk|mft|auto] [--threads N]` prints one scan: counts, bytes, throughput, errors, tree memory, and for a volume root the difference between used space and allocated bytes.
- `watch <path> [--for SECONDS]` scans, then prints live changes as they are applied.
- Expect small count and size differences from the list above. Large differences in one top-level entry point to a junction, hard links or an access-denied folder.
