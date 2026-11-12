# Snapshots and timeline

Core modules in `crates/core`, working on byte buffers only:

| Module | Role |
| --- | --- |
| `snapshot` | Full tree to bytes and back: `encode(&Tree, &SnapshotMeta)`, `decode`, `read_info` (header only). |
| `summary` | Timeline point: `Summary::from_tree`, `encode`, `decode`. |
| `diff` | `diff_trees`, `diff_summaries`, `top_growers`, `top_shrinkers`. |

Plan and retention: `docs/roadmap.md`.

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
