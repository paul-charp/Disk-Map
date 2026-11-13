# diskmap

Windows disk-usage treemap.

![diskmap](docs/screenshot.png)

Navigation inspired by SpaceSniffer.

## Features

- Squarified treemap of a folder or drive, drawn on a canvas with nested folders.
- Parallel directory walk, or a raw NTFS MFT scan when run as administrator.
- Live updates from a file-system watcher, with an optional highlight of recent changes.
- Filter language: name masks, size, age, attributes, folders, file classes and colour tags.
- Colour tags (4), file classes with editable colours and extensions.
- Tabs, back and forward over zooms, breadcrumb, free and unknown space of the volume.
- Side panel with the largest files and folders and bytes per class.
- Five themes, UI scale, detail level 1 to 12.
- `diskmap-bench` command line tool to time and compare the scan backends.

## Install

Requires Windows 10 or 11 and the WebView2 runtime (part of Windows 11 and current Windows 10). Each [release](https://github.com/paul-charp/Disk-Map/releases) has two downloads; the installer is also on winget once the package is accepted (`winget install PaulCharp.Diskmap`):

| Asset | Kind | Settings | Data (snapshots, log) |
| --- | --- | --- | --- |
| `diskmap-vX.Y.Z-windows-x64-setup.exe` | Installer, per user, no admin | `%APPDATA%\diskmap\diskmap.toml` | `%LOCALAPPDATA%\diskmap\data\` |
| `diskmap-vX.Y.Z-windows-x64-portable.exe` | Portable | `diskmap.toml` next to the exe | `data\` next to the exe |

- Installer: installs to `%LOCALAPPDATA%\diskmap` by default (data in its `data` subfolder), adds a Start menu shortcut and, from a checkbox on the last page, a desktop shortcut. It downloads WebView2 only if it is missing. Uninstall from Windows Settings, Apps; settings and data are kept unless "Delete the application data" is ticked.
- Portable: put the exe in any folder and run it; settings and data stay in that folder. Any copy of the exe the installer did not put in place runs portable.
- `DISKMAP_CONFIG` overrides the settings path in both modes.
- Updates: installed builds check at startup (Settings, About, Auto check) and show an `Update x.y.z` chip in the status bar; Install downloads the signed installer, runs it in passive mode and restarts. Portable builds only check and link to the release page. Updates stay off in builds without an updater public key; see [docs/distribution.md](docs/distribution.md).

Release downloads contain `diskmap.exe` only; `diskmap-bench` is a development tool built from source.

## Build

Requires Rust 1.90 or newer.

```powershell
cargo build --release -p diskmap
```

The executable is `target\release\diskmap.exe`. The installer needs the Tauri CLI (it fetches NSIS itself):

```powershell
cargo install tauri-cli --version "^2" --locked
cd crates\desktop
cargo tauri build --bundles nsis --no-sign
```

`--no-sign` skips the updater signature; without it the build needs `TAURI_SIGNING_PRIVATE_KEY` once a public key is configured (see [docs/distribution.md](docs/distribution.md)).

The installer is written to `target\release\bundle\nsis\diskmap_<version>_x64-setup.exe`.

## Usage

- Start screen: type a path (several separated by `;`), pick a drive or a recent scan, or browse. A dashboard shows drive usage with history, a trend chart, the latest movers and the schedule state. Folders dropped on the window open in tabs. Paths on the command line open one tab each.
- Click a folder or file to select it; the map digs in place to show its contents.
- Double click a folder to zoom to it. Back and Forward step through zooms only; Up and Root zoom out.
- Right click for Open, Reveal, Copy path, tags and Zoom.
- Tabs: one scan per tab, Ctrl+N for a new tab, middle click to close.
- Live updates: the Watch toggle (Ctrl+L) follows changes after a scan. Highlight changes is in Settings, Scan.
- MFT scan: start `diskmap.exe` as administrator, or use the Elevate button in the status bar, which relaunches with the open tabs. The backend (Settings, Scan) is Auto, MFT or Walk; the status bar shows which one ran.

## Keyboard shortcuts

| Key | Action |
| --- | --- |
| Ctrl+N | New tab |
| Ctrl+W | Close tab |
| Ctrl+Tab / Ctrl+Shift+Tab | Next / previous tab |
| Ctrl+, | Settings |
| F5 | Rescan / stop |
| Backspace | Back |
| Shift+Backspace | Forward |
| Ctrl+Up | Up |
| Ctrl+Home | Root |
| Ctrl++ / Ctrl+- | Detail |
| Ctrl+T | Class colours |
| Ctrl+F | Free space |
| Ctrl+U | Unknown space |
| Ctrl+L | Watch |
| Ctrl+B | Side panel |
| Ctrl+1 to Ctrl+4 | Tag hovered or selected item |
| Ctrl+0 | Clear tags |

Mouse buttons 4 and 5 act as Back and Forward.

## Filter syntax

Conditions are separated by `;`. A leading `|` negates a condition. See [docs/filters.md](docs/filters.md).

| Example | Matches |
| --- | --- |
| `*.jpg;*.png` | Files with either name mask |
| `\node_modules` | Files inside a folder with that name |
| `size>100mb` | Files larger than 100 MB |
| `modify<7d` | Files modified less than 7 days ago |
| `access>1year` | Files not accessed for more than a year |
| `:attr:hidden+system` | Hidden and system files |
| `:class:Video;size>1gb` | Videos larger than 1 GB |
| `:tag:red,blue` | Files tagged red or blue |
| `\src;*.rs;\|\target` | Rust files inside `src`, outside `target` |

## Build from source and test

```powershell
cargo build --workspace
cargo test --workspace
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
```

The front end in `crates/desktop/ui` is plain HTML, CSS and ES modules; there is no Node build step. See [docs/architecture.md](docs/architecture.md) and [docs/scanning.md](docs/scanning.md).

## Contributing

Work happens on `develop` and `feature/*` branches; `main` holds releases. See [docs/branching.md](docs/branching.md); release assets, updater keys and winget in [docs/distribution.md](docs/distribution.md).

## Licence

MIT, see [LICENSE](LICENSE).
