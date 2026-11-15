//! Tauri commands called by the front end. Commands that act on a scan take the id of its tab.

use crate::appearance;
use crate::args::{Args, Flags};
use crate::compare::under;
use crate::config::{ClassDef, Config, FileColors, Schedule, class_defs, hex, same_path};
use crate::elevate;
use crate::fmt;
use crate::log;
use crate::paint::{self, FREE_ID, PaintCtx, UNKNOWN_ID, deleted_folder};
use crate::paths;
use crate::scheduler;
use crate::session::{Session, find_by_names};
use crate::shell;
use crate::state::{
    AppState, Inner, Status, TabId, node_id, start_rescan, start_scan, start_view_rescan, status,
};
use crate::timeline;
use crate::update::{self, UpdateInfo};
use crate::watch;
use diskmap_core::age::age_bucket;
use diskmap_core::classes::{FileClasses, OTHER_COLOR};
use diskmap_core::diff::{DiffEntry, Status as DeltaStatus};
use diskmap_core::export::{self, Format, Row, Source};
use diskmap_core::log::Level;
use diskmap_core::tags::Tag;
use diskmap_core::tree::{NodeId, NodeKind};
use diskmap_scan::volume::{DriveKind, list_drives};
use serde::Serialize;
use std::io::Write;
use tauri::{AppHandle, State, WebviewWindow};

/// Result of a command.
pub type Res<T> = Result<T, CmdError>;

/// Error of a command, sent to the front end as its text. Creating one logs it.
#[derive(Debug)]
pub struct CmdError(String);

impl Serialize for CmdError {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.0)
    }
}

impl From<String> for CmdError {
    fn from(message: String) -> Self {
        log::error("command", message.clone());
        CmdError(message)
    }
}

impl From<&str> for CmdError {
    fn from(message: &str) -> Self {
        message.to_string().into()
    }
}

/// An error that is expected in normal use and not logged, such as a call racing a closed tab.
pub struct Quiet(&'static str);

impl From<Quiet> for CmdError {
    fn from(q: Quiet) -> Self {
        CmdError(q.0.to_string())
    }
}

/// Error of a command on a tab without a scan.
pub const NO_SESSION: Quiet = Quiet("No scan");

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
    Ok(window.show().map_err(|e| e.to_string())?)
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
    let mut g = state.lock();
    let Inner {
        sessions,
        classes,
        cfg,
        ..
    } = &mut *g;
    let s = sessions.get_mut(&tab).ok_or(NO_SESSION)?;
    s.refresh_compare(false, classes);
    let cx = PaintCtx {
        classes,
        min_tile: cfg.min_tile,
        text_scale: cfg.text_scale(),
        width,
        height,
        dpr,
        highlight: cfg.highlight_changes,
        now: fmt::filetime_now(),
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

/// Sets how the file tiles of a tab are coloured.
#[tauri::command]
pub async fn set_file_colors(
    state: State<'_, AppState>,
    tab: TabId,
    mode: FileColors,
) -> Res<Status> {
    with_session(&state, tab, |s| s.view.file_colors = mode)
}

/// Flips one of the view toggles of a tab: `free` or `unknown`.
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
        "free" => s.view.show_free = on,
        "unknown" => s.view.show_unknown = on,
        _ => return Err(format!("Unknown toggle {kind}").into()),
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
    /// Age bucket of a file, most recent 0.
    age: Option<u8>,
    tag: u8,
    pinned: bool,
    /// Compare mode: size in the snapshot, empty when new.
    old: String,
    /// Compare mode: size change.
    delta: String,
    /// Compare mode: `New`, `Grown`, `Shrunk`, `Same` or `Deleted`.
    change: &'static str,
}

fn status_name(st: DeltaStatus) -> &'static str {
    match st {
        DeltaStatus::New => "New",
        DeltaStatus::Deleted => "Deleted",
        DeltaStatus::Grown => "Grown",
        DeltaStatus::Shrunk => "Shrunk",
        DeltaStatus::Same => "Same",
    }
}

/// Old size, change and status of a node in compare mode.
fn add_delta(s: &Session, id: NodeId, d: &mut Details) {
    let Some(c) = s.compare.as_ref() else { return };
    let tree = s.scan.tree();
    d.old = c.map.old_size(id).map_or_else(String::new, fmt::size);
    d.delta = fmt::delta(c.map.delta(tree, id));
    d.change = status_name(c.map.status(tree, id));
}

fn deleted_details(s: &Session, node: i32, folder: NodeId) -> Res<Details> {
    let tree = s.scan.tree();
    let c = s.compare.as_ref().ok_or("Not comparing")?;
    if folder.0 as usize >= tree.len() {
        return Err("Unknown node".into());
    }
    let bytes = c.map.deleted(folder);
    Ok(Details {
        id: node,
        name: "Deleted".into(),
        path: tree.full_path(folder),
        kind: "deleted",
        size: fmt::size(bytes),
        disk: fmt::size(bytes),
        items: Some(c.map.deleted_items(folder)),
        old: fmt::size(bytes),
        delta: fmt::delta(-(bytes.min(i64::MAX as u64) as i64)),
        change: "Deleted",
        ..Details::default()
    })
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
    if let Some(folder) = deleted_folder(node) {
        return deleted_details(s, node, folder);
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
    let path = tree.full_path(id);
    let mut d = Details {
        id: node,
        name: tree.name(id).to_string(),
        pinned: !is_file && g.cfg.is_pinned(&path),
        path,
        kind: if is_file { "file" } else { "folder" },
        size: fmt::size(logical),
        disk: fmt::size(size),
        share: fmt::percent(size, s.size_of(s.root)),
        modified: fmt::date_time(times.mtime),
        created: fmt::date_time(times.ctime),
        accessed: fmt::date_time(times.atime),
        items: (!is_file).then(|| n.child_count()),
        class,
        age: is_file.then(|| age_bucket(times.mtime, fmt::filetime_now())),
        tag: s.tags.raw(id),
        ..Details::default()
    };
    add_delta(s, id, &mut d);
    Ok(d)
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
    /// Bytes per age bucket, most recent first.
    ages: Vec<AgeRow>,
    /// Compare mode: largest changes under the view root.
    changes: Option<Changes>,
}

/// One row of the changes lists.
#[derive(Serialize)]
pub struct ChangeRow {
    /// The node, or its nearest existing folder when deleted.
    id: Option<u32>,
    name: String,
    parent: String,
    delta: String,
    change: &'static str,
}

/// Top growers and shrinkers.
#[derive(Serialize)]
pub struct Changes {
    growers: Vec<ChangeRow>,
    shrinkers: Vec<ChangeRow>,
}

const CHANGES_N: usize = 10;

fn change_row(tree: &diskmap_core::tree::Tree, e: &DiffEntry) -> ChangeRow {
    let (parent, name) = e.path.rsplit_once('\\').unwrap_or(("", &e.path));
    let id = tree.find_path(&e.path).or_else(|| tree.find_path(parent));
    ChangeRow {
        id: id.map(|n| n.0),
        name: name.to_string(),
        parent: parent.to_string(),
        delta: fmt::delta(e.delta()),
        change: status_name(e.status),
    }
}

/// Largest changes under the view root, when comparing.
fn changes(s: &mut Session) -> Option<Changes> {
    let root = s.rel_path(s.root);
    let ver = s.scan.version;
    let tree = s.scan.tree();
    let m = s.compare.as_mut()?.movers(tree, ver);
    let rows = |list: &[usize]| {
        list.iter()
            .map(|&i| &m.entries[i])
            .filter(|e| under(&e.path, &root))
            .take(CHANGES_N)
            .map(|e| change_row(tree, e))
            .collect()
    };
    Some(Changes {
        growers: rows(&m.growers),
        shrinkers: rows(&m.shrinkers),
    })
}

/// Bytes of one age bucket.
#[derive(Serialize)]
pub struct AgeRow {
    size: String,
    share: f32,
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
    let ages = st
        .ages
        .iter()
        .map(|&bytes| AgeRow {
            size: fmt::size(bytes),
            share: bytes as f32 / total as f32,
        })
        .collect();
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
        ages,
        changes: changes(s),
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

/// Opens a terminal in a folder, or in the folder holding a file.
#[tauri::command]
pub async fn terminal_node(state: State<'_, AppState>, tab: TabId, node: i32) -> Res<()> {
    let dir = {
        let g = state.lock();
        let s = g.sessions.get(&tab).ok_or(NO_SESSION)?;
        let id = node_id(s, node).ok_or("Unknown node")?;
        let tree = s.scan.tree();
        let dir = match tree.get(id).kind {
            NodeKind::File => tree.parent(id).unwrap_or(id),
            _ => id,
        };
        tree.full_path(dir)
    };
    Ok(shell::terminal(&dir)?)
}

/// Shows the Explorer Properties dialog of a node.
#[tauri::command]
pub async fn properties_node(
    app: AppHandle,
    state: State<'_, AppState>,
    tab: TabId,
    node: i32,
) -> Res<()> {
    let path = path_of(&state, tab, node)?;
    let (tx, rx) = std::sync::mpsc::channel();
    app.run_on_main_thread(move || {
        let _ = tx.send(shell::properties(&path));
    })
    .map_err(|e| e.to_string())?;
    Ok(rx.recv().map_err(|e| e.to_string())??)
}

/// Moves a node to the Recycle Bin and drops it from the tree.
#[tauri::command]
pub async fn recycle_node(state: State<'_, AppState>, tab: TabId, node: i32) -> Res<Status> {
    let (path, rel) = {
        let g = state.lock();
        let s = g.sessions.get(&tab).ok_or(NO_SESSION)?;
        let id = node_id(s, node).ok_or("Unknown node")?;
        if id == s.scan.tree().root() {
            return Err("Scan root".into());
        }
        (s.scan.tree().full_path(id), s.rel_path(id))
    };
    tauri::async_runtime::spawn_blocking(move || shell::recycle(&path))
        .await
        .map_err(|e| e.to_string())??;
    let mut g = state.lock();
    let Inner {
        sessions, classes, ..
    } = &mut *g;
    if let Some(s) = sessions.get_mut(&tab) {
        s.forget_path(&rel, classes);
    }
    Ok(status(&g, tab))
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

/// A recent scan or a pinned folder for the start screen.
#[derive(Serialize)]
pub struct RecentRow {
    path: String,
    size: String,
    pinned: bool,
}

/// Start screen content.
#[derive(Serialize)]
pub struct StartInfo {
    drives: Vec<DriveRow>,
    pins: Vec<RecentRow>,
    recent: Vec<RecentRow>,
}

fn path_row(cfg: &Config, path: &str, size: u64, pinned: bool) -> RecentRow {
    RecentRow {
        path: path.to_string(),
        size: if size > 0 {
            fmt::size(size)
        } else {
            String::new()
        },
        pinned: pinned || cfg.is_pinned(path),
    }
}

pub fn kind_name(kind: DriveKind) -> &'static str {
    match kind {
        DriveKind::Fixed => "Fixed",
        DriveKind::Removable => "Removable",
        DriveKind::Network => "Network",
        DriveKind::Other => "Other",
    }
}

/// Drives, pinned folders and recent scans.
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
    let g = state.lock();
    let cfg = &g.cfg;
    let pins = cfg
        .pins
        .iter()
        .map(|p| path_row(cfg, p, cfg.known_size(p), true))
        .collect();
    let recent = cfg
        .recent
        .iter()
        .map(|r| path_row(cfg, &r.path, r.size, false))
        .collect();
    Ok(StartInfo {
        drives,
        pins,
        recent,
    })
}

/// Pins or unpins a folder; returns whether it is pinned now.
#[tauri::command]
pub async fn set_pin(state: State<'_, AppState>, path: String, on: bool) -> Res<bool> {
    let mut g = state.lock();
    if g.cfg.set_pin(&path, on) {
        g.cfg.save();
    }
    Ok(g.cfg.is_pinned(&path))
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

/// Rows of an export scope: `view`, `selection` (or `node` when given), `files` or `folders`.
fn export_rows(
    s: &mut Session,
    classes: &FileClasses,
    scope: &str,
    depth: Option<u32>,
    node: Option<i32>,
) -> Res<Vec<Row>> {
    let flat = match scope {
        "files" => Some(s.stats(classes).files.clone()),
        "folders" => Some(s.stats(classes).folders.clone()),
        _ => None,
    };
    let src = Source {
        tree: s.scan.tree(),
        tags: Some(&s.tags),
        filtered: s.filter.filtered.as_ref(),
    };
    if let Some(list) = flat {
        let ids: Vec<NodeId> = list.into_iter().map(|(id, _)| id).collect();
        return Ok(src.list(&ids));
    }
    let root = match scope {
        "view" => s.root,
        "selection" => node
            .and_then(|n| node_id(s, n))
            .or(s.selection)
            .ok_or("No selection")?,
        _ => return Err(format!("Unknown scope {scope}").into()),
    };
    Ok(src.subtree(root, depth))
}

/// Default file name: `diskmap-<root>-<date>.<ext>`.
fn export_name(s: &Session, format: Format) -> String {
    let name: String = s
        .scan
        .tree()
        .name(s.root)
        .chars()
        .map(|c| {
            if c.is_control() || "\\/:*?\"<>|".contains(c) {
                '_'
            } else {
                c
            }
        })
        .collect();
    let name = name.trim_matches(['_', ' ', '.']);
    let root = if name.is_empty() { "scan" } else { name };
    let date = export::iso_utc(fmt::filetime_now());
    let date = date.get(..10).unwrap_or_default();
    format!("diskmap-{root}-{date}.{}", format.ext())
}

/// Asks for a file to save to.
async fn save_dialog(window: WebviewWindow, name: String, format: Format) -> Res<Option<String>> {
    let picked = tauri::async_runtime::spawn_blocking(move || {
        let ext = format.ext();
        rfd::FileDialog::new()
            .set_parent(&window)
            .set_file_name(name)
            .add_filter(ext.to_uppercase(), &[ext])
            .save_file()
    })
    .await
    .map_err(|e| e.to_string())?;
    Ok(picked.map(|p| p.to_string_lossy().into_owned()))
}

/// Exports a scope of a tab (`view`, `selection`, `files`, `folders`) as `csv` or `json`, down to `depth`
/// levels (no limit when absent), honouring the active filter. Without `path` a save dialog asks.
/// Returns the written path, `None` when the dialog was cancelled.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn export(
    window: WebviewWindow,
    state: State<'_, AppState>,
    tab: TabId,
    scope: String,
    format: String,
    depth: Option<u32>,
    node: Option<i32>,
    path: Option<String>,
) -> Res<Option<String>> {
    let format = Format::parse(&format).ok_or_else(|| format!("Unknown format {format}"))?;
    let path = match path {
        Some(p) => p,
        None => {
            let name = {
                let g = state.lock();
                export_name(g.sessions.get(&tab).ok_or(NO_SESSION)?, format)
            };
            match save_dialog(window, name, format).await? {
                Some(p) => p,
                None => return Ok(None),
            }
        }
    };
    let mut bytes = Vec::new();
    {
        let mut g = state.lock();
        let classes = g.classes.clone();
        let s = g.sessions.get_mut(&tab).ok_or(NO_SESSION)?;
        let rows = export_rows(s, &classes, &scope, depth, node)?;
        export::write(&mut bytes, format, &rows).map_err(|e| e.to_string())?;
    }
    std::fs::write(&path, bytes).map_err(|e| e.to_string())?;
    Ok(Some(path))
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

/// Log entries held, oldest first.
#[tauri::command]
pub async fn log_entries(state: State<'_, AppState>) -> Res<Vec<log::Entry>> {
    Ok(state.log.entries())
}

/// Forgets the log entries held.
#[tauri::command]
pub async fn log_clear(state: State<'_, AppState>) -> Res<()> {
    state.log.clear();
    Ok(())
}

/// Logs an entry from the front end, e.g. an uncaught script error.
#[tauri::command]
pub async fn log_js(state: State<'_, AppState>, level: String, message: String) -> Res<()> {
    state.log.push(Level::parse(&level), "ui", message);
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
    portable: bool,
}

/// Version, license, configuration path and build of this binary.
#[tauri::command]
pub async fn about() -> Res<About> {
    Ok(About {
        name: env!("CARGO_PKG_NAME"),
        version: env!("CARGO_PKG_VERSION"),
        license: env!("CARGO_PKG_LICENSE"),
        config: paths::config_path()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default(),
        target: format!("{}-{}", std::env::consts::ARCH, std::env::consts::OS),
        profile: if cfg!(debug_assertions) {
            "debug"
        } else {
            "release"
        },
        portable: paths::is_portable(),
    })
}

/// Opens a project page in the default browser: `repo`, `issues` or `release` (the latest release).
#[tauri::command]
pub async fn open_link(link: String) -> Res<()> {
    let repo = env!("CARGO_PKG_REPOSITORY");
    let url = match link.as_str() {
        "repo" => repo.to_string(),
        "issues" => format!("{repo}/issues"),
        "release" => format!("{repo}/releases/latest"),
        _ => return Err(format!("Unknown link: {link}").into()),
    };
    shell::open(&url);
    Ok(())
}

/// Shows the configuration file in Explorer, or its folder when the file does not exist yet.
#[tauri::command]
pub async fn reveal_config() -> Res<()> {
    let path = paths::config_path().ok_or("No config path")?;
    if path.exists() {
        shell::reveal(&path.to_string_lossy());
    } else if let Some(dir) = path.parent() {
        shell::open(&dir.to_string_lossy());
    }
    Ok(())
}

/// Checks the release endpoint for a newer version; a failure is a `failed` state, not an error.
#[tauri::command]
pub async fn update_check(app: AppHandle) -> Res<UpdateInfo> {
    Ok(update::check(&app).await)
}

/// Downloads and installs the available update, then restarts. Installed builds only.
#[tauri::command]
pub async fn update_install(app: AppHandle) -> Res<()> {
    Ok(update::install(&app).await?)
}

/// The schedule settings, whether the task exists, and the folders it can snapshot.
#[derive(Serialize)]
pub struct ScheduleInfo {
    config: Schedule,
    /// The task is registered in Task Scheduler.
    exists: bool,
    /// Fixed drives and pinned folders.
    choices: Vec<String>,
    /// This process runs as administrator.
    admin: bool,
}

fn schedule_info(config: Schedule, pins: Vec<String>, exists: bool) -> ScheduleInfo {
    let mut choices: Vec<String> = list_drives()
        .into_iter()
        .filter(|d| d.kind == DriveKind::Fixed)
        .map(|d| d.root)
        .collect();
    for r in pins.into_iter().chain(config.roots.iter().cloned()) {
        if !choices.iter().any(|c| same_path(c, &r)) {
            choices.push(r);
        }
    }
    ScheduleInfo {
        config,
        exists,
        choices,
        admin: diskmap_scan::is_elevated(),
    }
}

async fn task_exists() -> Res<bool> {
    Ok(tauri::async_runtime::spawn_blocking(scheduler::exists)
        .await
        .map_err(|e| e.to_string())?)
}

/// Schedule settings and the state of the task.
#[tauri::command]
pub async fn schedule_get(state: State<'_, AppState>) -> Res<ScheduleInfo> {
    let (config, pins) = {
        let g = state.lock();
        (g.cfg.schedule.clone(), g.cfg.pins.clone())
    };
    Ok(schedule_info(config, pins, task_exists().await?))
}

/// Creates, replaces or deletes the task, then stores the settings. Nothing is stored on failure.
#[tauri::command]
pub async fn schedule_set(state: State<'_, AppState>, schedule: Schedule) -> Res<ScheduleInfo> {
    let mut new = schedule;
    new.sanitize();
    let (old, pins) = {
        let g = state.lock();
        (g.cfg.schedule.clone(), g.cfg.pins.clone())
    };
    let task = new.clone();
    tauri::async_runtime::spawn_blocking(move || scheduler::apply(&old, &task))
        .await
        .map_err(|e| e.to_string())??;
    {
        let mut g = state.lock();
        g.cfg.schedule = new.clone();
        g.cfg.save();
    }
    Ok(schedule_info(new, pins, task_exists().await?))
}

/// Totals of one timeline point.
#[derive(Serialize)]
pub struct TimelinePoint {
    /// Unix seconds, UTC.
    pub time: i64,
    pub allocated: u64,
    pub logical: u64,
    pub files: u64,
}

/// Root totals of every timeline point of a root, oldest first; unreadable points are skipped.
pub fn timeline_points(root: &str) -> Vec<TimelinePoint> {
    timeline::list(root)
        .into_iter()
        .filter_map(|p| {
            let r = timeline::load_root(&p.path)
                .map_err(|e| log::warn("timeline", e))
                .ok()?;
            Some(TimelinePoint {
                time: p.time,
                allocated: r.allocated,
                logical: r.logical,
                files: r.files,
            })
        })
        .collect()
}

/// The timeline points of a root, oldest first.
#[tauri::command]
pub async fn timeline(root: String) -> Res<Vec<TimelinePoint>> {
    Ok(
        tauri::async_runtime::spawn_blocking(move || timeline_points(&root))
            .await
            .map_err(|e| e.to_string())?,
    )
}

/// The most recent report lines, newest first (default 50).
#[tauri::command]
pub async fn reports(limit: Option<usize>) -> Res<Vec<timeline::ReportLine>> {
    Ok(timeline::reports(limit.unwrap_or(50)))
}
