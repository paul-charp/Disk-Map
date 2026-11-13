# Roadmap

Planned work after 0.1.0. Each point is one `feature/<name>` branch cut from `develop` (see `branching.md`).
Points in wave 2 depend on wave 1 branches and start after those merge.

## Wave 1

| Branch | Point |
| --- | --- |
| `feature/storage-paths` | Installed vs portable data locations |
| `feature/snapshot-core` | Snapshot format, timeline summary, diff (core) |
| `feature/context-menu` | New context menu items |
| `feature/pins` | Pinned folders |
| `feature/exports` | CSV and JSON exports |
| `feature/age-colour` | Colour tiles by file age |
| `feature/about` | About section, GitHub link |
| `feature/log-console` | In-app log of errors |

## Wave 2

| Branch | Point | Needs |
| --- | --- | --- |
| `feature/compare-mode` | Snapshot compare on the map | snapshot-core |
| `feature/periodic-scans` | Headless snapshot, scheduler, report | snapshot-core, storage-paths |
| `feature/dashboard` | Home page dashboard | pins, periodic-scans |
| `feature/installer` | NSIS installer next to the portable build | storage-paths |
| `feature/updater` | Auto updater | installer, about |
| `feature/winget` | Winget manifest in the release workflow | installer |

## Storage paths

- Installed: the installer records its folder in `HKCU\Software\diskmap\InstallDir`; the exe in that folder keeps settings in `%APPDATA%\diskmap\diskmap.toml`; snapshots, timeline and logs in `%LOCALAPPDATA%\diskmap\data\`.
- Portable: any other copy of the exe. Everything stays next to the exe.
- `DISKMAP_CONFIG` still overrides the config path.

## Snapshots and timeline

Measured on a synthetic tree of 1M nodes (`feature/snapshot-core`, no compression): a full snapshot costs 28.6 bytes per node
with mtime only, 38.6 with all three times. A system drive of 1.5M nodes is about 45 MB, so daily full snapshots still cost
over 15 GB per year. A timeline point is about 33 bytes per folder kept. There are two tiers:

- Full snapshot: the whole tree, for detailed diffs and compare mode. Keep the last few per root plus manual ones.
- Timeline point: folders above a size threshold or depth with size and file count. 1k to 10k entries, 30 to 330 KB.
  Daily for a year is 10 to 120 MB. Thresholds to tune on a real drive. Retention: daily for 90 days, then weekly, then monthly.

Core modules (no I/O beyond byte buffers): snapshot format, tree to timeline point, diff (grown, shrunk, new, deleted, rolled up to folders).

## Compare mode

- Tile colour by size delta. Deleted items as ghost tiles in a virtual Deleted node, like Free and Unknown.
- Tooltip shows old and new size. Side panel lists top growers and shrinkers.
- Filters `is:new`, `is:deleted`, `delta:>1GB`.

## Periodic scans

- Headless mode: `diskmap.exe --snapshot C:\ --quiet` scans, writes a timeline point (and optionally a full snapshot), exits.
- Scheduled through Windows Task Scheduler, set up from Settings, daily or weekly, optionally elevated for MFT.
- Report: a toast with the top movers, linking to the dashboard.

## Dashboard

Home page: drive cards with usage, file system, MFT badge, sparkline and 7 or 30 day delta; top movers opening compare mode;
pinned, recent with date and delta, snapshots; trend chart per drive on a canvas; slots for update notice, elevation and scheduler status.

## Pins

`pins` list in the config. Shown on the home page, toggled from the context menu, optionally included in periodic scans.

## Context menu

Keep the in-app menu. Items: Open, Open in terminal, Reveal, Copy path, Properties (native dialog), Recycle, Scan in new tab, Pin, tags, Zoom.

## Exports

CSV and JSON of the current view, the selection subtree with a depth limit, the top lists, and later diff results.
The headless mode reuses the same code with `--export`.

## Age colour

A colour mode next to class colours, by `mtime`: a ramp from recent to old taken from theme data, and a highlight for untouched files.
The `age` filter already exists.

## About

Settings section with version, GitHub link, license, data folder (with Open), and later Check for updates. An info icon button opens it.

## Log console

- Backend ring buffer of about 1000 entries (level, time, source, message), fed by command errors, scan and watcher failures.
- Mirrored to `diskmap.log` in the data folder, size capped.
- Front end errors (`onerror`, `unhandledrejection`) forwarded to the backend.
- Status bar badge with the error count opens a bottom Output panel with level filter, Copy and Clear.

## Install, update, distribution

- Installer: Tauri bundler, NSIS, per-user without admin, Start menu shortcut, optional desktop shortcut, uninstaller that can keep data.
  Releases ship `diskmap-<tag>-windows-x64-setup.exe` and `diskmap-<tag>-windows-x64-portable.exe`.
- Updater: `tauri-plugin-updater`, signed `latest.json` on GitHub Releases (key in a repository secret).
  Installed builds install and restart; portable builds only check and link to the release.
  Done; setup in `docs/distribution.md`.
- Winget: `wingetcreate` in `release.yml` after the first installer release (user scope, nullsoft).
  Done: `PaulCharp.Diskmap`, templates in `winget/manifests/`; first submission by hand (`docs/distribution.md`).
- Code signing: none at first; consider SignPath for open source later.
