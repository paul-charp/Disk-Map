//! Window frame, background and zoom, kept in step with the theme and the UI scale.

use crate::config::Config;
use tauri::window::Color;
use tauri::{Theme, WebviewWindow};

/// Page background of a chrome theme; the same values as `--bg` in `style.css`.
fn background(theme: &str) -> Color {
    match theme {
        "paper" => Color(0xf6, 0xf6, 0xf4, 255),
        "dusk" => Color(0x1a, 0x20, 0x29, 255),
        "ember" => Color(0x1f, 0x1d, 0x1a, 255),
        "violet" => Color(0x1a, 0x1a, 0x1e, 255),
        _ => Color(0x1b, 0x1c, 0x1f, 255),
    }
}

/// Sets the title bar theme, the webview background and the zoom from the configuration.
pub fn apply(window: &WebviewWindow, cfg: &Config) {
    let frame = if cfg.theme == "paper" {
        Theme::Light
    } else {
        Theme::Dark
    };
    let _ = window.set_theme(Some(frame));
    let _ = window.set_background_color(Some(background(&cfg.theme)));
    let _ = window.set_zoom(f64::from(cfg.ui_scale) / 100.0);
}
