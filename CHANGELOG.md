# Changelog

## Unreleased

- Portable and installed storage: a `portable` marker (shipped in the zip) or an existing `diskmap.toml` keeps everything next to the exe; otherwise settings go to `%APPDATA%\diskmap` and data to `%LOCALAPPDATA%\diskmap\data`.
- Core snapshot format, timeline points and tree diff with top movers (`docs/snapshots.md`).
- Output panel (Ctrl+`) with the application log: command, scan, watcher, config and script errors, level filter, Copy, Clear; error and warning counts in the status bar; mirrored to `diskmap.log` next to the config file (1 MB, rotated to `.1`); test switch `--log`.
- About section in Settings (version, license, GitHub and Issues links, config path with Reveal, build), opened from an info button in the toolbar.
- Pinned folders: Pinned section on the start screen, pin buttons on recent rows, Pin and Unpin in the map context menu.
- Context menu: Open in terminal, Properties, Scan in new tab and Recycle (with confirm).
- Export of the view, the selection, top files and top folders as CSV or JSON (toolbar Export menu, context menu), honouring the filter and the detail level.
- File colour mode Accent, Class or Age: age colours files by modification time with a ramp per theme, an Age legend in the side panel and the `--colour` test switch. Replaces the class colour toggle; `class_colors` in old configs is still read.
- NSIS installer `diskmap-<tag>-windows-x64-setup.exe` next to the portable zip: per-user install without admin, Start menu shortcut, optional desktop shortcut, uninstaller that keeps settings and data unless asked to delete them. CI builds it on `release/*` branches and on manual runs.
- Updater (`tauri-plugin-updater`): startup check (`check_updates`, Settings, About, Auto check), Check now button, `Update x.y.z` status bar chip; installed builds install in passive mode and restart, portable builds link to the release. Releases ship a signed `latest.json` and `.sig` when the signing secrets are set; updates are off while `plugins.updater.pubkey` is empty (`docs/distribution.md`).

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
