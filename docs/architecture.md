# Architecture

## Crates

| Crate | Role |
| --- | --- |
| `crates/core` (`diskmap-core`) | Arena `Tree`, squarified layout, filter grammar and evaluation, colour tags, file classes, snapshot and timeline formats, diff (`docs/snapshots.md`). No I/O. |
| `crates/scan` (`diskmap-scan`) | Walk and MFT scan backends, `TreeBuilder`, `ReadDirectoryChangesW` watcher, `LiveUpdater`, volume and drive info. Windows only. |
| `crates/bench` (`diskmap-bench`) | Command line tool: `scan`, `compare`, `watch`. |
| `crates/desktop` (`diskmap`) | Tauri 2 app: sessions, config, paint list encoder, commands. Front end in `ui/`. |

## Data flow

```
scanner threads (walk: rayon pool, MFT: reader + parsers)
   | ScanEvent::Batch / Error / Done      crossbeam channel
   v
scan worker thread (state.rs)             one per scan
   | TreeBuilder::apply_batch             under the state mutex
   v
Tree (core) + Session (filter, tags, selection, history)
   | emit "scan" Status snapshot          every 100 ms and when done
   v
front end (ui/js)  -- invoke layout(tab, w, h, dpr) -->  paint::build
   |                                                       layout() + encode
   | <-- binary paint list -------------------------------+
   v
PaintList decode -> Painter (offscreen canvas) -> MapView canvas + hover and highlight overlay
```

- Scanners send `DirBatch` events: a directory token and its entries. The root is token 0.
- The worker applies events in slices of at most 8 ms per lock hold, refreshes the filter, and emits a `Status` snapshot at most every 100 ms.
- `layout` runs `diskmap_core::layout::layout` for the view root of the tab in a device-pixel area, then encodes the items. The front end requests a layout when a snapshot's `ver` changed, on resize and after navigation.
- Directory sizes in the tree are subtree totals; layout uses allocated sizes, or the per-node sizes of the active filter.

## Paint list

`crates/desktop/src/paint.rs` writes it, `ui/js/paintlist.js` reads it. All values are little-endian `i32`.

- Header, 4 values: item count, label blob bytes, view root node id, layout time in microseconds.
- Items, 8 values each: `x, y, w, h, node, meta, parent index, highlight`.
  - `meta = depth | class << 8 | flags << 16`; class 255 means none. With the age flag the class byte is an age bucket (`diskmap_core::age`, 0 is under a week); with the delta flag it is a delta class (`diskmap_core::compare`: 0 same, 1 new, 2 shrunk, 3 to 6 grown by relative growth, 7 deleted).
  - Flags: file 1, dir 2, drilled 4, free 8, unknown 16, selected 32, bold 64, label 128, age 2048 (bit 11), delta 4096 (bit 12, compare mode), deleted 8192 (bit 13, ghost item of deleted bytes). The tag (0 to 4) is stored in the flag bits from bit 8.
  - `highlight = weight << 16 | warning << 15 | age in ms`; 0 when there is none.
- Label blob: UTF-8 labels joined by `\n`, one per item flagged label, in item order. A label is `name`, byte 0x01, size text.
- Item 0 is the view root and lies outside the drawn area. Node ids -1 and -2 are the free and unknown space items; `-3 - f` is the deleted item of folder `f` in compare mode.
- Items are in paint order; hit testing walks them backwards, so the deepest item wins.

## Sessions and tabs

- The front end owns the tab list. A tab is a start screen (`mode: 'start'`) or a map. Tab ids come from the front end.
- The backend holds `AppState`, an `Arc<Mutex<Inner>>` with the config, the file classes and one `Session` per scanned tab.
- A `Session` has the `Scan` (tree, backend, volume, errors), the view root, the selection, the history stacks, tags, the filter state, recent change marks and the optional watcher.
- Each scan gets an increasing epoch. `Status.ver` is the sum of the scan, filter and tag versions. The front end drops a snapshot older than the one it holds.
- Rescan starts a new session in the same tab and carries the view options, filter, view root and selection (as names below the scan root), restoring them when the scan ends. Rescan view scans the view root alone and replaces that subtree.
- Closing a tab drops its session, which cancels the scan and stops the watcher.

## Filter evaluation

`Filter::parse` turns the text into selectors, excludes, size, age and attribute conditions and folder masks. `Filter::apply` runs once over the tree:

1. Forward pass over nodes (parents come before children): inherited tag mask and folder-mask bits, then the match test for each file.
2. Reverse pass: matching sizes are added into their parents.

The result is per-node allocated and logical sizes, the match count and the folders whose own name matches a positive mask (drawn bold). The session re-applies it when the tree, tags (if the filter uses them) or file classes change, throttled to 500 ms while a scan runs. When a filter is active the layout, statistics and the volume's free and unknown space use the filtered sizes. Grammar: [filters.md](filters.md).

## History and selection

- Click selects the item under the pointer and clears nothing in history. The layout gets the selection as `focus`: the selected node and its ancestors are drilled even when the detail level would stop earlier, so a click digs in place.
- Double click zooms to the first-clicked folder (the parent for a file). `zoom_to` pushes the previous view root on the back stack and clears the forward stack.
- History holds view roots only. Back pops the stack and selects the folder that was left (or the restored root when it is not an ancestor); Forward re-applies a popped root. Up and Root are zooms and are recorded.
- Selection changes never enter history. After live updates, the view root, selection and history entries that were removed move to their nearest surviving ancestor.

## Live updates

- When Watch is on and a scan has finished, `Watcher` (`ReadDirectoryChangesW`) watches the scan root and sends `WatchEvent`s.
- A worker batches events for 250 ms (up to 8192) and applies them in chunks of 512 per lock hold through `LiveUpdater`, which re-stats each distinct path and edits the tree (add, remove, resize; new folders are walked).
- A kernel buffer overflow marks the session stale; the status bar shows a Stale button that rescans.
- Change highlight (`marks.rs`): changed nodes are remembered for 2 s. Painted items get weight 255 for the changed node and 110 for painted ancestors; shrinks and removals set the warning bit; a removed node is marked on its nearest surviving ancestor. The front end fades the highlight over 1.5 s. It is only encoded when `highlight_changes` is on.

## Config file

`diskmap.toml` at the location below, or the path in `DISKMAP_CONFIG`. It is loaded at start, written when settings change and after scans (recent list). Missing or invalid files give the defaults; values are clamped on load.

| Key | Default | Meaning |
| --- | --- | --- |
| `theme` | `graphite` | `graphite`, `paper`, `dusk`, `ember`, `violet` |
| `detail` | 3 | Nesting levels for new tabs, 1 to 12 |
| `file_colors` | `accent` | File tile colours for new tabs: `accent`, `class` or `age`; reads the former `class_colors` flag |
| `show_free`, `show_unknown` | true, true | Defaults for new tabs |
| `show_side` | false | Side panel |
| `ui_scale` | 100 | Webview zoom in percent, 80 to 200 |
| `map_text` | `medium` | `small`, `medium`, `large` |
| `animation` | true | Zoom animation |
| `min_tile` | 3 | Smallest tile in CSS pixels, 2 to 16 |
| `threads` | 0 | Scan threads, 0 for automatic, at most 64 |
| `backend` | `auto` | `auto`, `mft`, `walk` |
| `watch` | false | Live updates |
| `highlight_changes` | false | Change highlight |
| `check_updates` | true | Check for a new release at startup |
| `auto_snapshot` | true | Save a full snapshot after every completed full scan |
| `keep_snapshots` | 5 | Automatic snapshots kept per root (1 to 100); manual ones are never pruned |
| `recent` | empty | Last 8 scanned paths with sizes |
| `classes` | six defaults | File classes: name, `#rrggbb`, extensions |

The environment variable `DISKMAP_PERF` names a file that receives one timing line per layout.

## Storage paths

`paths.rs` picks the mode once at startup:

| Mode | When | Config | Data (snapshots, timeline, logs) |
| --- | --- | --- | --- |
| Portable | a `portable` file or a `diskmap.toml` next to the exe | `<exe dir>\diskmap.toml` | `<exe dir>\data\` |
| Installed | otherwise | `%APPDATA%\diskmap\diskmap.toml` | `%LOCALAPPDATA%\diskmap\data\` |

The release zip ships the `portable` marker. The installer puts the exe in `%LOCALAPPDATA%\diskmap\`, so installed data sits in its `data\` subfolder, like portable. An existing `diskmap.toml` next to the exe keeps 0.1.0 setups portable. `DISKMAP_CONFIG` overrides the config path in both modes; it does not change the data folder. Folders are created on first write.
