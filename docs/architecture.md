# Architecture

## Crates

| Crate | Role |
| --- | --- |
| `crates/core` (`diskmap-core`) | Arena `Tree`, squarified layout, filter grammar and evaluation, colour tags, file classes, age buckets, snapshot and timeline formats, diff, compare, retention, reports, task XML (`docs/snapshots.md`), CSV and JSON export, log ring. No I/O. |
| `crates/scan` (`diskmap-scan`) | Walk and MFT scan backends, `TreeBuilder`, `ReadDirectoryChangesW` watcher, `LiveUpdater`, volume and drive info. Windows only. |
| `crates/bench` (`diskmap-bench`) | Command line tool: `scan`, `compare`, `watch`. |
| `crates/desktop` (`diskmap`) | Tauri 2 app: sessions, config, paint list encoder, commands. Front end in `ui/`. |

### Desktop modules

| Module | Role |
| --- | --- |
| `main.rs`, `args.rs` | Startup, command line paths and test switches, headless `--snapshot` dispatch |
| `commands.rs` | Tauri commands; `build.rs` lists them, `capabilities/main.json` allows them |
| `state.rs`, `scan.rs`, `session.rs` | Shared state, scan worker, per-tab session (navigation, selection, tags, filter, statistics) |
| `paint.rs`, `marks.rs` | Paint list encoder, change highlight |
| `watch.rs` | Live updates |
| `config.rs`, `paths.rs`, `appearance.rs` | Settings file, installed or portable locations, window frame and zoom |
| `snapstore.rs`, `snapcmds.rs`, `compare.rs` | Full snapshots on disk, their commands, compare mode |
| `timeline.rs`, `headless.rs`, `scheduler.rs`, `toast.rs` | Timeline points and reports, headless runs, Task Scheduler, toast |
| `dashboard.rs` | The `dashboard` command |
| `update.rs` | Updater ([distribution.md](distribution.md)) |
| `channel.rs` | Update channels: manifests and accepted versions |
| `log.rs` | Application log and `diskmap.log` |
| `explorer.rs` | Open in Diskmap entry of the Explorer menu (HKCU) |
| `elevate.rs`, `shell.rs`, `fmt.rs` | Relaunch as administrator, Explorer integration, size and date text |

The front end (`ui/js`) has one module per part: `app.js` (tab state and actions), `api.js` (command wrappers), `map.js` and `paintlist.js` (map canvas), `toolbar.js`, `statusbar.js`, `side.js`, `start.js` and `chart.js` (dashboard), `settings.js` and its sections, `output.js`, `menu.js`, `flags.js` (test switches).

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
- `app.afterScan(tab, fn)` runs `fn` once when the next scan of that tab ends (scan events with `active` and not `running`). The dashboard uses it for Compare.

## Start screen

The Home tab, fixed first in the strip and never closed, is a dashboard (`ui/js/start.js`, drawings in `ui/js/chart.js`), filled by one `dashboard` command (`crates/desktop/src/dashboard.rs`):

- Drives: label, file system, MFT capability (local fixed or removable NTFS), total and free bytes, and the root totals of every timeline point. Points are read with `Summary::read_root` from the first 4 KB of each file.
- Pins and recent scans with size, last scan time (the recent entry's `time` or the newest point) and the change between the last two points.
- Changes: one card per root with history (drives, scheduled roots, pins), largest change first: totals and up to 5 growers and shrinkers between the newest point and the window start (`report::window_start`: newest point at least 7 or 30 days old, or the oldest for All and for a shorter history), and whether the root has a snapshot. `dashboard` takes `days` (0 for all).
- The schedule settings and whether the process is elevated. Whether the task exists comes from a background `schedule_get`.

Drive cards show a used bar, the 7 and 30 day change (newest point against the newest one at least that old) and an SVG sparkline; the Trend chart is a canvas scaled by `devicePixelRatio` and redrawn on resize and theme change; its gridlines share one unit and the decimals the step needs, and a change too small for two decimals labels the lowest line absolute and the others as offsets. Scans started from Home open new tabs; `+` and Ctrl+N go to Home. Change rows open a new tab on a root or a mover; Compare scans the root, compares with the newest snapshot not taken by that scan and zooms to the mover. Status slots: update available, next scheduled run (or Schedule off, Task missing), Elevate or Admin. Up and down colours are the `--up` and `--down` theme variables.

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

## Export

`export` writes rows of a tab as CSV or JSON (`diskmap_core::export`): the view root's subtree, the selection's subtree, or the top files or folders of the side panel. Rows carry path, kind, allocated and logical size, file count for folders, modification time and tag, and honour the active filter. CSV has a header line, CRLF line ends and a UTF-8 BOM. A save dialog proposes `diskmap-<root>-<date>.<ext>`.

## Log

`log.rs` keeps a ring of the last 1000 entries (level, time, source, message) fed by command errors, scan and watcher failures, config errors and front end errors (`onerror`, `unhandledrejection`). Entries are sent to the front end (Output panel, status bar counts) and appended to `diskmap.log` in the data folder, rotated to `diskmap.log.1` at 1 MB.

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
| `recent` | empty | Last 8 scanned paths with sizes and scan times |
| `pins` | empty | Pinned folders |
| `filters` | empty | Saved filters, at most 20 |
| `schedule` | off | Scheduled snapshots: `enabled`, `every` (`daily`, `weekly`), `day`, `time` (`HH:MM`), `roots`, `elevated` ([snapshots.md](snapshots.md)) |
| `classes` | six defaults | File classes: name, `#rrggbb`, extensions |

The environment variable `DISKMAP_PERF` names a file that receives one timing line per layout.

## Storage paths

`paths.rs` picks the mode once at startup:

| Mode | When | Config | Data (snapshots, timeline, logs) |
| --- | --- | --- | --- |
| Installed | the exe folder is `HKCU\Software\diskmap\InstallDir` | `%APPDATA%\diskmap\diskmap.toml` | `%LOCALAPPDATA%\diskmap\data\` |
| Portable | otherwise | `<exe dir>\diskmap.toml` | `<exe dir>\data\` |

The installer writes `InstallDir` on every install and update (`windows/hooks.nsh`); the uninstaller deletes it. Folders are compared after canonicalization, ignoring case. The portable exe, a dev build, a 0.1.0 unzip or a copy of the installed exe elsewhere is portable. The installer puts the exe in `%LOCALAPPDATA%\diskmap\` by default, so installed data sits in its `data\` subfolder. `DISKMAP_CONFIG` overrides the config path in both modes; it does not change the data folder. Folders are created on first write.
