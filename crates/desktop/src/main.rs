//! diskmap: disk-usage treemap.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod appearance;
mod args;
mod commands;
mod config;
mod elevate;
mod fmt;
mod log;
mod marks;
mod paint;
mod paths;
mod scan;
mod session;
mod shell;
mod state;
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
    Args::from_env().apply(&mut cfg);
    let startup = cfg.clone();
    tauri::Builder::default()
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
            commands::details,
            commands::stats,
            commands::find_node,
            commands::open_node,
            commands::reveal_node,
            commands::start_info,
            commands::set_pin,
            commands::browse,
            commands::elevate,
            commands::log_perf,
            commands::log_entries,
            commands::log_clear,
            commands::log_js,
            commands::about,
            commands::open_link,
            commands::reveal_config,
        ])
        .run(tauri::generate_context!())
        .expect("failed to run diskmap");
}
