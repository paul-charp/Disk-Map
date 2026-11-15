# Roadmap

Each point is one `feature/<name>` branch cut from `develop` (see `branching.md`).

## Shipped in 0.2.0

Two waves of branches after 0.1.0, all merged:

| Branch | Point | Doc |
| --- | --- | --- |
| `feature/storage-paths`, `feature/portable-detection` | Installed and portable data locations | [architecture.md](architecture.md#storage-paths) |
| `feature/snapshot-core` | Snapshot format, timeline point, diff | [snapshots.md](snapshots.md) |
| `feature/context-menu` | Open in terminal, Properties, Recycle, Scan in new tab | README |
| `feature/pins` | Pinned folders | README |
| `feature/exports` | CSV and JSON exports | [architecture.md](architecture.md#export) |
| `feature/age-colour` | File colours by age | [architecture.md](architecture.md#paint-list) |
| `feature/about` | About section, GitHub link | README |
| `feature/log-console` | Output panel and `diskmap.log` | [architecture.md](architecture.md#log) |
| `feature/compare-mode`, `feature/snapshot-manager` | Snapshots, compare mode, snapshot manager | [snapshots.md](snapshots.md) |
| `feature/periodic-scans` | Headless snapshot, Task Scheduler, reports | [snapshots.md](snapshots.md) |
| `feature/dashboard` | Dashboard start screen | [architecture.md](architecture.md#start-screen) |
| `feature/installer` | NSIS installer next to the portable exe | [distribution.md](distribution.md) |
| `feature/updater` | Auto updater with signed `latest.json` | [distribution.md](distribution.md#updater) |
| `feature/winget` | Winget package `PaulCharp.Diskmap` | [distribution.md](distribution.md#winget) |

## Open

| Point | Notes |
| --- | --- |
| Timeline thresholds | Tune `min_fraction` and `max_depth` of timeline points on a real drive (sizes in [snapshots.md](snapshots.md)). |
| Headless full snapshot | `--snapshot` writes a timeline point only; an option to also save a full snapshot for compare. |
| Toast action | Clicking the report toast opens the dashboard. |
| Dashboard snapshots | List of stored snapshots on the start screen; today they are in Settings, Snapshots. |
| Headless export | `--export` reusing the export code; export of diff results. |
| Untouched files | Highlight of files not accessed for a long time in the Age colour mode. |
| Code signing | Authenticode for the exe and installer, e.g. SignPath for open source ([distribution.md](distribution.md#code-signing)). |
