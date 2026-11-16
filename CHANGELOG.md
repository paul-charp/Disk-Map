# Changelog

## 0.3.0

- Explorer menu: Open in Diskmap on folders, folder backgrounds and drives (HKCU, written by the installer, toggle in Settings, About).
- Single instance: a second launch opens its paths as tabs in the running window.
- Update channels Stable, Beta and Dev (Settings, About, Channel); switching back offers the older stable version. Dev builds of `develop` and a `channels` release with `beta.json` and `dev.json`.

## 0.2.0

- Window title, shortcuts, uninstall entry and toasts named Diskmap; the exe stays `diskmap.exe`.
- About and Settings buttons on the start screen.
- Filter bar Presets (built-in filters, saved filters stored in `filters` in the config, Save, Clear) and Syntax menu that inserts tokens.
- Tab reordering by drag, Ctrl+Shift+PageUp / PageDown.
- CI uploads the installer artifact unzipped.
- Portable and installed storage: the installer records its folder in `HKCU\Software\diskmap\InstallDir`; the exe in that folder keeps settings in `%APPDATA%\diskmap` and data in `%LOCALAPPDATA%\diskmap\data`, any other copy keeps everything next to itself. Releases ship a bare `diskmap-<tag>-windows-x64-portable.exe` instead of a zip.
- Core snapshot format, timeline points and tree diff with top movers (`docs/snapshots.md`).
- Output panel (Ctrl+`) with the application log: command, scan, watcher, config and script errors, level filter, Copy, Clear; error and warning counts in the status bar; mirrored to `diskmap.log` in the data folder (1 MB, rotated to `.1`); test switch `--log`.
- About section in Settings (version, license, GitHub and Issues links, config path with Reveal, build), opened from an info button in the toolbar.
- Pinned folders: Pinned section on the start screen, pin buttons on recent rows, Pin and Unpin in the map context menu.
- Context menu: Open in terminal, Properties, Scan in new tab and Recycle (with confirm).
- Export of the view, the selection, top files and top folders as CSV or JSON (toolbar Export menu, context menu), honouring the filter and the detail level.
- Periodic scans: a timeline point after every full scan; headless `--snapshot <root> [--quiet] [--no-toast]` that scans, stores a point, prunes by retention (daily 90 days, weekly to a year, then monthly), appends to `reports.jsonl` and shows a toast with the top mover; Schedule section in Settings that registers a daily or weekly task `diskmap\Snapshot` in Task Scheduler, optionally elevated for the MFT backend; commands `schedule_get`, `schedule_set`, `timeline` and `reports`.
- File colour mode Accent, Class or Age: age colours files by modification time with a ramp per theme, an Age legend in the side panel and the `--colour` test switch. Replaces the class colour toggle; `class_colors` in old configs is still read.
- Snapshots and compare mode: a full snapshot after every scan (Settings, Scan: Auto snapshot, Keep snapshots) and Save now; the toolbar Compare menu colours tiles by change (new, grown, shrunk, same), shows deleted bytes as hatched Deleted tiles, adds old size and change to the tooltip and a Changes list to the side panel; filters `is:new`, `is:grown`, `is:shrunk`, `is:deleted`, `delta:>1GB`; status bar chip; test switch `--compare`.
- NSIS installer `diskmap-<tag>-windows-x64-setup.exe` next to the portable exe: per-user install without admin, Start menu shortcut, optional desktop shortcut, uninstaller that keeps settings and data unless asked to delete them. CI builds it on `release/*` branches and on manual runs.
- Updater (`tauri-plugin-updater`): startup check (`check_updates`, Settings, About, Auto check), Check now button, `Update x.y.z` status bar chip; installed builds install in passive mode and restart, portable builds link to the release. Releases ship a signed `latest.json` and `.sig` when the signing secrets are set; updates are off while `plugins.updater.pubkey` is empty (`docs/distribution.md`).
- Snapshot manager in Settings, Snapshots (also Compare menu, Manage): all stored snapshots by root with totals; Keep or Unkeep, Compare, Reveal, Delete per snapshot; Compare latest and Delete unkept per root; Open folder.
- Dashboard start screen: drive cards with file system, MFT badge, used bar, 7 and 30 day change and a sparkline; Trend chart of used space per drive with time axis and hover readout; Movers from the latest reports, expandable to the top growers and shrinkers, opening a tab or Compare against the latest snapshot; last scan time and change on Pinned and Recent; status slots for update, schedule and elevation; command `dashboard`.
- Winget package `PaulCharp.Diskmap`: manifest templates in `winget/manifests/`, a Release job that fills them (artifact `winget-manifests`) and submits stable versions with `wingetcreate` when `WINGET_TOKEN` is set; setup in `docs/distribution.md`.

## 0.1.0

- Repository renamed to Disk-Map; branch model in `docs/branching.md`.
- Local working files move to `.local/`.
- Workspace crates: `diskmap-core`, `diskmap-scan`, `diskmap-bench`, `diskmap` (Tauri 2 desktop app).
- Treemap view with squarified layout, nested folders, detail levels 1 to 12, free and unknown space of the volume.
- Click to select and dig in place, double click to zoom, Back, Forward, Up and Root, breadcrumb.
- Tabs with a start screen (drives, recent scans, folder picker, drag and drop).
- Parallel directory walk scan backend.
- NTFS MFT scan backend (administrator), backend setting Auto, MFT or Walk, Elevate button.
- Live updates from a `ReadDirectoryChangesW` watcher, change highlight, stale indicator.
- Filter language: name and folder masks, size, age, attributes, tags, file classes, negation.
- Four colour tags, file classes with editable colours and extensions.
- Side panel with selection details, largest files and folders, bytes per class.
- Rescan and rescan view.
- Settings: five themes, UI scale, map text size, animation, minimum tile, threads, classes, shortcut list.
- Test switches: `--theme`, `--scale`, `--tabs`, `--side`, `--detail`, `--zoom`, `--select`, `--hover`, `--tooltip`, `--settings`.
- `diskmap-bench` with `scan`, `compare` and `watch`.
- CI and release workflows for GitHub Actions.
