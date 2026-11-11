# Changelog

## 0.1.0

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
