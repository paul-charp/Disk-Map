//! diskmap: disk-usage treemap.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod appearance;
mod args;
mod commands;
mod compare;
mod config;
mod dashboard;
mod elevate;
mod fmt;
mod headless;
mod log;
mod marks;
mod paint;
mod paths;
mod scan;
mod scheduler;
mod session;
mod shell;
mod snapcmds;
mod snapstore;
mod state;
mod timeline;
mod toast;
mod update;
mod watch;

use args::Args;
use config::Config;
use state::AppState;
use std::time::Duration;
use tauri::{DragDropEvent, Emitter, Manager, WindowEvent};

/// The window is shown by the front end after its first frame; this shows it anyway if that never happens.
const SHOW_FALLBACK: Duration = Duration::from_secs(4);

fn on_window_event(window: &tauri::Window, event: &WindowEvent) {
    if let WindowEvent::DragDrop(DragDropEvent::Drop { paths, .. }) = event {
        let dropped: Vec<String> = paths
            .iter()
            .map(|p| p.to_string_lossy().into_owned())
            .collect();
        let _ = window.emit("dropped", dropped);
    }
}

fn main() {
    log::info(
        "app",
        format!("diskmap {} started", env!("CARGO_PKG_VERSION")),
    );
    let mut cfg = Config::load();
    let args = Args::from_env();
    args.apply(&mut cfg);
    if !args.headless.roots.is_empty() {
        std::process::exit(headless::run(&args.headless, &cfg));
    }
    let startup = cfg.clone();
    tauri::Builder::default()
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(AppState::new(cfg))
        .on_window_event(on_window_event)
        .setup(move |app| {
            log::get().attach(app.handle().clone());
            if let Some(w) = app.get_webview_window("main") {
                appearance::apply(&w, &startup);
                std::thread::spawn(move || {
                    std::thread::sleep(SHOW_FALLBACK);
                    let _ = w.show();
                });
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::boot,
            commands::show_window,
            commands::set_config,
            commands::get_status,
            commands::scan_path,
            commands::rescan,
            commands::rescan_view,
            commands::stop_scan,
            commands::close_tab,
            commands::layout,
            commands::select,
            commands::zoom,
            commands::nav_back,
            commands::nav_forward,
            commands::nav_up,
            commands::nav_home,
            commands::tag,
            commands::clear_tags,
            commands::set_filter,
            commands::set_detail,
            commands::set_toggle,
            commands::set_file_colors,
            commands::details,
            commands::stats,
            commands::find_node,
            commands::open_node,
            commands::reveal_node,
            commands::terminal_node,
            commands::properties_node,
            commands::recycle_node,
            commands::start_info,
            commands::set_pin,
            commands::browse,
            commands::export,
            commands::elevate,
            commands::log_perf,
            commands::log_entries,
            commands::log_clear,
            commands::log_js,
            commands::about,
            commands::open_link,
            commands::reveal_config,
            commands::update_check,
            commands::update_install,
            commands::schedule_get,
            commands::schedule_set,
            commands::timeline,
            commands::reports,
            dashboard::dashboard,
            snapcmds::snapshots,
            snapcmds::save_snapshot,
            snapcmds::delete_snapshot,
            snapcmds::compare,
        ])
        .run(tauri::generate_context!())
        .expect("failed to run diskmap");
}
