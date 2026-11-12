# Changelog

## Unreleased

- Portable and installed storage: a `portable` marker (shipped in the zip) or an existing `diskmap.toml` keeps everything next to the exe; otherwise settings go to `%APPDATA%\diskmap` and data to `%LOCALAPPDATA%\diskmap`.
- Core snapshot format, timeline points and tree diff with top movers (`docs/snapshots.md`).
- Output panel (Ctrl+`) with the application log: command, scan, watcher, config and script errors, level filter, Copy, Clear; error and warning counts in the status bar; mirrored to `diskmap.log` next to the config file (1 MB, rotated to `.1`); test switch `--log`.
- About section in Settings (version, license, GitHub and Issues links, config path with Reveal, build), opened from an info button in the toolbar.
- Pinned folders: Pinned section on the start screen, pin buttons on recent rows, Pin and Unpin in the map context menu.
- Context menu: Open in terminal, Properties, Scan in new tab and Recycle (with confirm).

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
