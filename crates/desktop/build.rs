const COMMANDS: &[&str] = &[
    "boot",
    "show_window",
    "set_config",
    "get_status",
    "scan_path",
    "rescan",
    "rescan_view",
    "stop_scan",
    "close_tab",
    "layout",
    "select",
    "zoom",
    "nav_back",
    "nav_forward",
    "nav_up",
    "nav_home",
    "tag",
    "clear_tags",
    "set_filter",
    "set_detail",
    "set_toggle",
    "set_file_colors",
    "details",
    "stats",
    "find_node",
    "open_node",
    "reveal_node",
    "start_info",
    "browse",
    "elevate",
    "log_perf",
];

fn main() {
    let manifest = tauri_build::AppManifest::new().commands(COMMANDS);
    tauri_build::try_build(tauri_build::Attributes::new().app_manifest(manifest))
        .expect("failed to run tauri-build");
}
