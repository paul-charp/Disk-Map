# Snapshots and timeline

Core modules in `crates/core`, working on byte buffers only:

| Module | Role |
| --- | --- |
| `snapshot` | Full tree to bytes and back: `encode(&Tree, &SnapshotMeta)`, `decode`, `read_info` (header only). |
| `summary` | Timeline point: `Summary::from_tree`, `encode`, `decode`. |
| `diff` | `diff_trees`, `diff_summaries`, `top_growers`, `top_shrinkers`. |
| `retention` | UTC calendar helpers, point file names, `keep` (which points retention keeps). |
| `report` | `report` (totals and top movers between two points), `previous` (point to compare with). |
| `key` | `root_key`: folder name for a scan root. |
| `schedule` | Task Scheduler XML and `schtasks` arguments for the snapshot task. |

Plan: `docs/roadmap.md`. Storage, retention, reports and the scheduler: below.

## Store and compare mode

- Files: `<data>/snapshots/<root_key(root)>/<YYYYMMDD-HHMMSS>.dmsn` (scan time, UTC; `crates/desktop/src/snapstore.rs`). Manual snapshots end in `-keep` and are never pruned. Writes go to a `.tmp` file, then rename.
- After every completed full scan (not view rescans, not stopped scans) a snapshot is saved in the background when `auto_snapshot` is on, then the root keeps its newest `keep_snapshots` automatic ones.
- Compare mode (`crates/desktop/src/compare.rs`, `diskmap_core::compare`): every node of the current tree is matched by path to the snapshot; tiles get a delta class, deleted bytes become a Deleted ghost item per folder. Live changes recompute the match at most every 3 s. A rescan keeps comparing against the same snapshot.
- Snapshot manager (Settings, Snapshots; also Compare menu, Manage): every stored snapshot grouped by root, with totals. Per snapshot: Keep (renames to or from `-keep`), Compare (in the active tab when it shows that root, otherwise in a new tab after its scan), Reveal, Delete; per root: Compare latest and Delete unkept.
- Commands: `snapshots`, `save_snapshot`, `delete_snapshot`, `snapshot_store`, `keep_snapshot`, `reveal_snapshot`, `compare`. Test switches `--compare latest|<path>`, `--settings Snapshots`.

## Encoding

- Little-endian. `varint` is unsigned LEB128, `zigzag` a signed varint, `str` a varint byte length then UTF-8.
- Both formats start with 4 magic bytes and a `u16` version (1). A decoder rejects other versions.
- No compression: varints, prefix-shared names and predicted sizes already give about 29 bytes per node, most of it names.

## Full snapshot (`DMSN`)

Header after magic and version:

| Field | Type |
| --- | --- |
| flags (1 all times, 2 times in seconds) | varint |
| scan time (FILETIME ticks) | zigzag |
| cluster size | varint |
| root path | str |
| nodes (root included), files, dirs | varint ×3 |
| root logical, root allocated | varint ×2 |
| root attributes | varint |
| root mtime, then ctime and atime as deltas from it when flag 1 | zigzag |
| root child count | varint |

Then every reachable node in pre-order, siblings in insertion order. The parent is implicit: a folder record holds its child count and its children follow.

| Field | Present when |
| --- | --- |
| tag byte | always |
| child count, varint | folder |
| attributes, varint | attributes differ from the previous node of the same kind |
| logical size, varint | file, or folder with own size |
| allocated size, varint | allocation mode 0 |
| mtime delta from the previous node, zigzag | mtime differs |
| ctime and atime deltas from this mtime, zigzag | flag 1 |
| shared name prefix with the previous sibling, varint | prefix bit |
| rest of the name, str | always |

Tag bits: 0 folder, 1 same attributes, 2-3 allocation mode, 4 same mtime, 5 prefix, 6 folder has own size, 7 reserved (must be 0).
Allocation modes: 0 explicit, 1 zero, 2 logical rounded up to the cluster size, 3 equal to logical.
Folder sizes are not stored: decoding rebuilds them as totals. A folder's own size is its total minus its children's.
With flag 2 times are whole seconds (lossy, a few bytes less per node).

## Timeline point (`DMTP`)

Folders kept: depth at most `max_depth` (default 2), or allocated size at least `min_fraction` of the root (default 0.1%). Kept folders are ancestor-closed.

Header: scan time (zigzag), `min_fraction` in millionths, `max_depth`, root path, entry count. Then entries in pre-order, the root first:
parent distance (`index - parent index`, varint, absent for the root), name (str, absent for the root), allocated, logical, files below, folders below (varint each).

## Diff

- `diff_trees` walks both trees by child name (case-insensitive). Folder entries only by default; `files` adds files and `unchanged` adds entries without change. An unchanged folder stays when something below it is listed. New and deleted folders are single entries with their totals.
- Status by allocated size: New, Deleted, Grown, Shrunk, Same (change of at most `same_within` bytes).
- A file replaced by a folder of the same name, or the reverse, is a Deleted and a New entry.
- `diff_summaries` matches entries by path. A folder missing from one point is New or Deleted when that point would have kept it by depth; otherwise it fell below the size threshold, its missing size counts as 0 and the entry has `approx` set.
- `top_growers` and `top_shrinkers` sort by allocated delta. With `leaf_most`, an entry with a listed descendant moving the same way is skipped, so no change is counted twice.

## Sizes

Synthetic tree, 1M nodes (909k files, 91k folders, 17.8 name bytes per node), `cargo test -p diskmap-core --release --test snapshot_size -- --ignored --nocapture`:

| Format | Bytes | Per node |
| --- | --- | --- |
| Snapshot, mtime | 28.6 MB | 28.6 |
| Snapshot, mtime in seconds | 25.1 MB | 25.1 |
| Snapshot, all times | 38.6 MB | 38.6 |
| Snapshot, all times in seconds | 29.1 MB | 29.1 |
| Timeline point, defaults (962 folders) | 31.6 KB | 32.9 per entry |

Encode takes about 0.35 s, decode 0.13 s and a diff of two such trees 0.6 s (release build).

## Timeline store

`crates/desktop/src/timeline.rs`. Points live in the data folder (`docs/architecture.md`, Storage paths):

```
<data>\timeline\<root_key(root)>\<YYYYMMDD-HHMMSS>.dmtl   UTC scan time, DMTP bytes
<data>\reports.jsonl                                     one report per headless run
```

- A point is written to `<name>.dmtl.tmp` and renamed. Other files in the folder are ignored.
- The app writes a point after every full scan that ran to its end: not after a view rescan, a stopped scan, or a scan that read nothing because it failed (an MFT error, an unreadable root). The file is written on a background thread.
- Retention runs after every write (`retention::keep`, by UTC calendar day of the point against now):
  - younger than 90 days: the last point of each day;
  - 90 to 365 days: the first point of each ISO week;
  - older: the first point of each month.
  Kept points stay kept, so the selection is stable.

## Headless snapshot

`diskmap.exe --snapshot C:\ [--snapshot D:\ ...] [--quiet] [--no-toast]` runs before the window is built and exits with 0, or 1 when a root failed.
For each root: scan with the configured backend (`Auto` uses the MFT when elevated, the walk otherwise), write a point, apply retention,
compare with the previous point and append a report line. Results and failures go to `diskmap.log`. `--quiet` drops console output.

The previous point is the newest from an earlier UTC day, else the newest. A report line (times in Unix seconds, sizes in allocated bytes;
`prev` and `old` are null for a first point; up to 5 movers each way, leaf-most so no change counts twice):

```json
{"time":1762916400,"root":"C:\\","prev":1762830000,"old":412000000000,"new":416500000000,
 "growers":[{"path":"Users\\me\\Downloads","delta":3300000000}],"shrinkers":[{"path":"Windows\\Temp","delta":-200000000}]}
```

### Toast

Unless `--no-toast`, one toast follows the run: the title lists each root's change (`C: +4.2 GB · D: -1 GB`, the total for a first point),
the body the largest single move (`C:\Users\me\Downloads +3.1 GB`). It uses `Windows.UI.Notifications` with the AppUserModelID
`dev.diskmap.app`, registered under `HKCU\Software\Classes\AppUserModelId\dev.diskmap.app` (DisplayName) before each toast so an
unpackaged exe can show it without a Start menu shortcut. A failure is logged as a warning and nothing else happens. Clicking the toast
has no action yet; the installer's Start menu shortcut, carrying the same AUMID, is the natural way to make it open the app.

## Schedule

Settings, Schedule: Snapshots on or off, Daily or Weekly with a day, a local time, the folders (fixed drives, pins) and Elevated.
Changes apply with Apply. `schedule_set` writes the task first and stores the settings only when that worked.

- The task `diskmap\Snapshot` is registered with `schtasks /Create /TN diskmap\Snapshot /XML <file> /F` from an XML definition
  (`schedule::task_xml`, written UTF-16 with BOM to `<data>\schedule.xml` and deleted afterwards). XML instead of `/TR` so the task also runs
  on battery, starts late after a missed time (`StartWhenAvailable`), never runs twice at once and stops after 2 hours.
- Action: `"<current exe>" "--snapshot" "C:\\" ... "--quiet"`. Moving the exe breaks the task; Apply again from the new location.
- Elevated sets `RunLevel` HighestAvailable so the run can use the MFT. Registering such a task needs an elevated `schtasks`: when the app
  is not elevated it runs `schtasks.exe` through `ShellExecuteExW` with `runas` (one UAC prompt) and waits for its exit code. Replacing or
  deleting an elevated task from a normal process falls back to the same.
- Off deletes the task (`/Delete /F`). `schedule_get` reports whether `/Query /TN diskmap\Snapshot` finds it.

## Commands

| Command | Returns |
| --- | --- |
| `timeline(root)` | Points of a root, oldest first: `time` (Unix seconds), `allocated`, `logical`, `files`. |
| `reports(limit)` | The last report lines, newest first (default 50). |
| `schedule_get` | `config` (the schedule settings), `exists`, `choices` (fixed drives, pins), `admin`. |
| `schedule_set(schedule)` | Same as `schedule_get` after the change. |
| `dashboard` | Start screen data: drives with their timeline points, pins, recent, the last 10 reports, schedule, `admin` (`docs/architecture.md`, Start screen). |
