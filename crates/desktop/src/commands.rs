//! Tauri commands called by the front end. Commands that act on a scan take the id of its tab.

use crate::appearance;
use crate::args::{Args, Flags};
use crate::config::{ClassDef, Config, class_defs, config_path, hex};
use crate::elevate;
use crate::fmt;
use crate::paint::{self, FREE_ID, PaintCtx, UNKNOWN_ID};
use crate::session::{Session, find_by_names};
use crate::shell;
use crate::state::{
    AppState, Status, TabId, node_id, start_rescan, start_scan, start_view_rescan, status,
};
use crate::watch;
use diskmap_core::classes::{FileClasses, OTHER_COLOR};
use diskmap_core::tags::Tag;
use diskmap_core::tree::{NodeId, NodeKind};
use diskmap_scan::volume::{DriveKind, list_drives};
use serde::Serialize;
use std::io::Write;
use tauri::{AppHandle, State, WebviewWindow};

type Res<T> = Result<T, String>;

const NO_SESSION: &str = "No scan";

/// Name and colour of a file class.
#[derive(Serialize)]
pub struct ClassInfo {
    name: String,
    color: String,
}

/// Data the front end needs at startup.
#[derive(Serialize)]
pub struct Boot {
    config: Config,
    classes: Vec<ClassInfo>,
    default_classes: Vec<ClassDef>,
    /// Paths to open, one per tab; an empty entry is a start tab.
    tabs: Vec<String>,
    flags: Flags,
    debug: bool,
}

/// Names and colours of the classes, followed by the colour of files without a class.
fn class_infos(classes: &FileClasses) -> Vec<ClassInfo> {
    let mut out: Vec<ClassInfo> = classes
        .classes()
        .iter()
        .map(|c| ClassInfo {
            name: c.name.clone(),
            color: hex(c.color),
        })
        .collect();
    out.push(ClassInfo {
        name: "Other".into(),
        color: hex(OTHER_COLOR),
    });
    out
}

/// Startup data: configuration, class colours and the paths given on the command line.
#[tauri::command]
pub async fn boot(state: State<'_, AppState>) -> Res<Boot> {
    let g = state.lock();
    let args = Args::from_env();
    Ok(Boot {
        config: g.cfg.clone(),
        classes: class_infos(&g.classes),
        default_classes: class_defs(&FileClasses::default()),
        tabs: args.tabs,
        flags: args.flags,
        debug: cfg!(debug_assertions),
    })
}

/// Shows the main window; it starts hidden so the first frame is already themed.
#[tauri::command]
pub async fn show_window(window: WebviewWindow) -> Res<()> {
    window.show().map_err(|e| e.to_string())
}

/// Stores new settings, applies their side effects and returns the class list.
#[tauri::command]
pub async fn set_config(
    app: AppHandle,
    window: WebviewWindow,
    state: State<'_, AppState>,
    config: Config,
) -> Res<Vec<ClassInfo>> {
    let mut g = state.lock();
    let before = g.cfg.clone();
    g.cfg.replace(config);
    g.cfg.save();
    if g.cfg.theme != before.theme || g.cfg.ui_scale != before.ui_scale {
        appearance::apply(&window, &g.cfg);
    }
    if g.cfg.classes != before.classes {
        let classes = g.cfg.file_classes();
        for s in g.sessions.values_mut() {
            s.classes_changed(&classes);
        }
        g.classes = classes;
    }
    let watch_changed = g.cfg.watch != before.watch;
    let infos = class_infos(&g.classes);
    drop(g);
    if watch_changed {
        watch::sync(&app, &state);
    }
    Ok(infos)
}

/// Current status snapshot of a tab.
#[tauri::command]
pub async fn get_status(state: State<'_, AppState>, tab: TabId) -> Res<Status> {
    Ok(status(&state.lock(), tab))
}

/// Starts scanning a folder in a tab.
#[tauri::command]
pub async fn scan_path(
    app: AppHandle,
    state: State<'_, AppState>,
    tab: TabId,
    path: String,
) -> Res<Status> {
    start_scan(&app, &state, tab, &path, None)?;
    Ok(status(&state.lock(), tab))
}

/// Scans the root of a tab again.
#[tauri::command]
pub async fn rescan(app: AppHandle, state: State<'_, AppState>, tab: TabId) -> Res<Status> {
    start_rescan(&app, &state, tab)?;
    Ok(status(&state.lock(), tab))
}

/// Rescans only the view root and replaces that subtree.
#[tauri::command]
pub async fn rescan_view(app: AppHandle, state: State<'_, AppState>, tab: TabId) -> Res<Status> {
    start_view_rescan(&app, &state, tab)?;
    Ok(status(&state.lock(), tab))
}

/// Stops a running scan, keeping what was found.
#[tauri::command]
pub async fn stop_scan(state: State<'_, AppState>, tab: TabId) -> Res<Status> {
    let g = state.lock();
    if let Some(s) = g.sessions.get(&tab) {
        s.scan.stop();
    }
    Ok(status(&g, tab))
}

/// Closes a tab: cancels its scan and frees its tree.
#[tauri::command]
pub async fn close_tab(state: State<'_, AppState>, tab: TabId) -> Res<()> {
    let session = state.lock().sessions.remove(&tab);
    drop(session);
    Ok(())
}

/// Lays out the view root of a tab in a device-pixel area and returns the encoded paint list.
#[tauri::command]
pub async fn layout(
    state: State<'_, AppState>,
    tab: TabId,
    width: u32,
    height: u32,
    dpr: f32,
) -> Res<tauri::ipc::Response> {
    let g = state.lock();
    let s = g.sessions.get(&tab).ok_or(NO_SESSION)?;
    let cx = PaintCtx {
        classes: &g.classes,
        min_tile: g.cfg.min_tile,
        text_scale: g.cfg.text_scale(),
        width,
        height,
        dpr,
        highlight: g.cfg.highlight_changes,
    };
    Ok(tauri::ipc::Response::new(paint::build(s, &cx)))
}

fn with_session(
    state: &State<'_, AppState>,
    tab: TabId,
    f: impl FnOnce(&mut Session),
) -> Res<Status> {
    let mut g = state.lock();
    f(g.sessions.get_mut(&tab).ok_or(NO_SESSION)?);
    Ok(status(&g, tab))
}

/// Selects a node, or clears the selection.
#[tauri::command]
pub async fn select(state: State<'_, AppState>, tab: TabId, node: Option<i32>) -> Res<Status> {
    with_session(&state, tab, |s| {
        let id = node.and_then(|n| node_id(s, n));
        s.select(id);
    })
}

/// Zooms to a folder.
#[tauri::command]
pub async fn zoom(state: State<'_, AppState>, tab: TabId, node: i32) -> Res<Status> {
    with_session(&state, tab, |s| {
        if let Some(id) = node_id(s, node) {
            s.zoom_to(id);
        }
    })
}

/// Back in the zoom history.
#[tauri::command]
pub async fn nav_back(state: State<'_, AppState>, tab: TabId) -> Res<Status> {
    with_session(&state, tab, Session::go_back)
}

/// Forward in the zoom history.
#[tauri::command]
pub async fn nav_forward(state: State<'_, AppState>, tab: TabId) -> Res<Status> {
    with_session(&state, tab, Session::go_forward)
}

/// Zooms out one level.
#[tauri::command]
pub async fn nav_up(state: State<'_, AppState>, tab: TabId) -> Res<Status> {
    with_session(&state, tab, Session::go_up)
}

/// Zooms out to the scan root.
#[tauri::command]
pub async fn nav_home(state: State<'_, AppState>, tab: TabId) -> Res<Status> {
    with_session(&state, tab, Session::go_home)
}

/// Toggles a colour tag on a node.
#[tauri::command]
pub async fn tag(state: State<'_, AppState>, tab: TabId, node: i32, tag: u8) -> Res<Status> {
    with_session(&state, tab, |s| {
        if let (Some(id), Some(tag)) = (node_id(s, node), Tag::from_u8(tag)) {
            s.toggle_tag(id, tag);
        }
    })
}

/// Clears the tags under the view root.
#[tauri::command]
pub async fn clear_tags(state: State<'_, AppState>, tab: TabId) -> Res<Status> {
    with_session(&state, tab, Session::clear_tags)
}

/// Result of applying a filter text.
#[derive(Serialize)]
pub struct FilterInfo {
    error: Option<String>,
    matched: u64,
    total: u64,
}

/// Applies filter text; a parse error leaves the previous filter active.
#[tauri::command]
pub async fn set_filter(state: State<'_, AppState>, tab: TabId, text: String) -> Res<FilterInfo> {
    let mut g = state.lock();
    let classes = g.classes.clone();
    let s = g.sessions.get_mut(&tab).ok_or(NO_SESSION)?;
    let error = s.set_filter(&text, &classes).err();
    Ok(FilterInfo {
        error,
        matched: s.matched_files(),
        total: s.scan.tree().file_count(),
    })
}

/// Sets the detail level of a tab.
#[tauri::command]
pub async fn set_detail(state: State<'_, AppState>, tab: TabId, detail: u32) -> Res<Status> {
    with_session(&state, tab, |s| s.view.detail = detail.clamp(1, 12))
}

/// Flips one of the view toggles of a tab: `classes`, `free` or `unknown`.
#[tauri::command]
pub async fn set_toggle(
    state: State<'_, AppState>,
    tab: TabId,
    kind: String,
    on: bool,
) -> Res<Status> {
    let mut g = state.lock();
    let s = g.sessions.get_mut(&tab).ok_or(NO_SESSION)?;
    match kind.as_str() {
        "classes" => s.view.class_colors = on,
        "free" => s.view.show_free = on,
        "unknown" => s.view.show_unknown = on,
        _ => return Err(format!("Unknown toggle {kind}")),
    }
    Ok(status(&g, tab))
}

/// Details of one node for the tooltip, the status bar and the side panel.
#[derive(Serialize, Default)]
pub struct Details {
    id: i32,
    name: String,
    path: String,
    kind: &'static str,
    size: String,
    disk: String,
    share: String,
    modified: String,
    created: String,
    accessed: String,
    items: Option<u32>,
    class: String,
    tag: u8,
}

fn special_details(s: &Session, id: i32) -> Details {
    let (free, unknown) = s.free_unknown();
    let (name, kind, bytes) = if id == FREE_ID {
        ("Free space", "free", free)
    } else {
        ("Unknown space", "unknown", unknown)
    };
    Details {
        id,
        name: name.into(),
        path: s.scan.root_path.clone(),
        kind,
        size: fmt::size(bytes),
        disk: fmt::size(bytes),
        ..Details::default()
    }
}

/// Details of a node, or of the free and unknown space items.
#[tauri::command]
pub async fn details(state: State<'_, AppState>, tab: TabId, node: i32) -> Res<Details> {
    let g = state.lock();
    let s = g.sessions.get(&tab).ok_or(NO_SESSION)?;
    if node == FREE_ID || node == UNKNOWN_ID {
        return Ok(special_details(s, node));
    }
    let id = node_id(s, node).ok_or("Unknown node")?;
    let tree = s.scan.tree();
    let n = tree.get(id);
    let is_file = n.kind == NodeKind::File;
    let size = s.size_of(id);
    let logical = s.filter.filtered.as_ref().map_or(n.logical, |f| {
        f.logical.get(id.0 as usize).copied().unwrap_or(0)
    });
    let times = tree.times(id);
    let class = if is_file {
        g.classes
            .class_of(tree.name(id))
            .map_or("Other".to_string(), |i| g.classes.classes()[i].name.clone())
    } else {
        String::new()
    };
    Ok(Details {
        id: node,
        name: tree.name(id).to_string(),
        path: tree.full_path(id),
        kind: if is_file { "file" } else { "folder" },
        size: fmt::size(logical),
        disk: fmt::size(size),
        share: fmt::percent(size, s.size_of(s.root)),
        modified: fmt::date_time(times.mtime),
        created: fmt::date_time(times.ctime),
        accessed: fmt::date_time(times.atime),
        items: (!is_file).then(|| n.child_count()),
        class,
        tag: s.tags.raw(id),
    })
}

/// One row of the largest-items lists.
#[derive(Serialize)]
pub struct Top {
    id: u32,
    name: String,
    parent: String,
    size: String,
}

/// Bytes of one file class.
#[derive(Serialize)]
pub struct ClassRow {
    name: String,
    color: String,
    size: String,
    share: f32,
}

/// Side panel statistics.
#[derive(Serialize)]
pub struct StatsOut {
    files: Vec<Top>,
    folders: Vec<Top>,
    classes: Vec<ClassRow>,
}

fn tops(s: &Session, list: &[(NodeId, u64)]) -> Vec<Top> {
    let tree = s.scan.tree();
    list.iter()
        .map(|&(id, size)| Top {
            id: id.0,
            name: tree.name(id).to_string(),
            parent: tree
                .parent(id)
                .map_or_else(String::new, |p| tree.full_path(p)),
            size: fmt::size(size),
        })
        .collect()
}

/// Largest files and folders and bytes per class under the view root.
#[tauri::command]
pub async fn stats(state: State<'_, AppState>, tab: TabId) -> Res<StatsOut> {
    let mut g = state.lock();
    let classes = g.classes.clone();
    let s = g.sessions.get_mut(&tab).ok_or(NO_SESSION)?;
    let st = s.stats(&classes);
    let (files, folders, per_class) = (st.files.clone(), st.folders.clone(), st.classes.clone());
    let total: u64 = per_class.iter().sum::<u64>().max(1);
    let mut rows: Vec<ClassRow> = per_class
        .iter()
        .enumerate()
        .map(|(i, &bytes)| {
            let (name, color) = classes
                .classes()
                .get(i)
                .map_or(("Other".to_string(), OTHER_COLOR), |c| {
                    (c.name.clone(), c.color)
                });
            ClassRow {
                name,
                color: hex(color),
                size: fmt::size(bytes),
                share: bytes as f32 / total as f32,
            }
        })
        .collect();
    rows.sort_by(|a, b| b.share.total_cmp(&a.share));
    Ok(StatsOut {
        files: tops(s, &files),
        folders: tops(s, &folders),
        classes: rows,
    })
}

fn path_of(state: &State<'_, AppState>, tab: TabId, node: i32) -> Res<String> {
    let g = state.lock();
    let s = g.sessions.get(&tab).ok_or(NO_SESSION)?;
    let id = node_id(s, node).ok_or("Unknown node")?;
    Ok(s.scan.tree().full_path(id))
}

/// Finds a node by its path below the scan root.
#[tauri::command]
pub async fn find_node(state: State<'_, AppState>, tab: TabId, path: String) -> Res<Option<i32>> {
    let g = state.lock();
    let s = g.sessions.get(&tab).ok_or(NO_SESSION)?;
    let names: Vec<&str> = path.split(['\\', '/']).filter(|p| !p.is_empty()).collect();
    Ok(find_by_names(s.scan.tree(), &names).map(|id| id.0 as i32))
}

/// Opens a node with its default handler.
#[tauri::command]
pub async fn open_node(state: State<'_, AppState>, tab: TabId, node: i32) -> Res<()> {
    shell::open(&path_of(&state, tab, node)?);
    Ok(())
}

/// Shows a node in Explorer.
#[tauri::command]
pub async fn reveal_node(state: State<'_, AppState>, tab: TabId, node: i32) -> Res<()> {
    shell::reveal(&path_of(&state, tab, node)?);
    Ok(())
}

/// A mounted drive for the start screen.
#[derive(Serialize)]
pub struct DriveRow {
    root: String,
    label: String,
    kind: &'static str,
    usage: String,
    share: f32,
}

/// A recent scan for the start screen.
#[derive(Serialize)]
pub struct RecentRow {
    path: String,
    size: String,
}

/// Start screen content.
#[derive(Serialize)]
pub struct StartInfo {
    drives: Vec<DriveRow>,
    recent: Vec<RecentRow>,
}

fn kind_name(kind: DriveKind) -> &'static str {
    match kind {
        DriveKind::Fixed => "Fixed",
        DriveKind::Removable => "Removable",
        DriveKind::Network => "Network",
        DriveKind::Other => "Other",
    }
}

/// Drives and recent scans.
#[tauri::command]
pub async fn start_info(state: State<'_, AppState>) -> Res<StartInfo> {
    let drives = list_drives()
        .into_iter()
        .map(|d| {
            let used = d.total_bytes.saturating_sub(d.free_bytes);
            DriveRow {
                root: d.root,
                label: d.label,
                kind: kind_name(d.kind),
                usage: format!("{} / {}", fmt::size(used), fmt::size(d.total_bytes)),
                share: used as f32 / d.total_bytes.max(1) as f32,
            }
        })
        .collect();
    let recent = state
        .lock()
        .cfg
        .recent
        .iter()
        .map(|r| RecentRow {
            path: r.path.clone(),
            size: if r.size > 0 {
                fmt::size(r.size)
            } else {
                String::new()
            },
        })
        .collect();
    Ok(StartInfo { drives, recent })
}

/// Relaunches elevated with the paths of the open tabs and quits when that worked; false when it was cancelled.
#[tauri::command]
pub async fn elevate(app: AppHandle, state: State<'_, AppState>, tab: TabId) -> Res<bool> {
    let paths = state.lock().tab_paths(tab);
    let relaunched = tauri::async_runtime::spawn_blocking(move || elevate::relaunch(&paths))
        .await
        .map_err(|e| e.to_string())?;
    if relaunched {
        app.exit(0);
    }
    Ok(relaunched)
}

/// Opens a folder picker and returns the chosen path.
#[tauri::command]
pub async fn browse(window: WebviewWindow) -> Res<Option<String>> {
    let picked = tauri::async_runtime::spawn_blocking(move || {
        rfd::FileDialog::new().set_parent(&window).pick_folder()
    })
    .await
    .map_err(|e| e.to_string())?;
    Ok(picked.map(|p| p.to_string_lossy().into_owned()))
}

/// Appends a timing line to the file named by `DISKMAP_PERF`, when set.
#[tauri::command]
pub async fn log_perf(line: String) -> Res<()> {
    if let Ok(path) = std::env::var("DISKMAP_PERF")
        && let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
    {
        let _ = writeln!(f, "{line}");
    }
    Ok(())
}

/// Application facts for the About section.
#[derive(Serialize)]
pub struct About {
    name: &'static str,
    version: &'static str,
    license: &'static str,
    config: String,
    target: String,
    profile: &'static str,
}

/// Version, license, configuration path and build of this binary.
#[tauri::command]
pub async fn about() -> Res<About> {
    Ok(About {
        name: env!("CARGO_PKG_NAME"),
        version: env!("CARGO_PKG_VERSION"),
        license: env!("CARGO_PKG_LICENSE"),
        config: config_path()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default(),
        target: format!("{}-{}", std::env::consts::ARCH, std::env::consts::OS),
        profile: if cfg!(debug_assertions) {
            "debug"
        } else {
            "release"
        },
    })
}

/// Opens a project page in the default browser: `repo` or `issues`.
#[tauri::command]
pub async fn open_link(link: String) -> Res<()> {
    let repo = env!("CARGO_PKG_REPOSITORY");
    let url = match link.as_str() {
        "repo" => repo.to_string(),
        "issues" => format!("{repo}/issues"),
        _ => return Err(format!("Unknown link: {link}")),
    };
    shell::open(&url);
    Ok(())
}

/// Shows the configuration file in Explorer, or its folder when the file does not exist yet.
#[tauri::command]
pub async fn reveal_config() -> Res<()> {
    let path = config_path().ok_or("No config path")?;
    if path.exists() {
        shell::reveal(&path.to_string_lossy());
    } else if let Some(dir) = path.parent() {
        shell::open(&dir.to_string_lossy());
    }
    Ok(())
}
