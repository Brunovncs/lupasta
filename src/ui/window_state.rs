//! Window size and position across launches (what the window-state plugin did in the Tauri
//! build). Kept apart from settings.json: it changes on every move and is not a preference.

use gpui::{App, Bounds, WindowBounds, point, px, size};
use serde::{Deserialize, Serialize};
use std::path::Path;

const FILE_NAME: &str = "window.json";

#[derive(Serialize, Deserialize)]
struct Saved {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    maximized: bool,
}

/// The saved bounds, if they still land on one of the current displays.
pub fn load(data_dir: &Path, cx: &App) -> Option<WindowBounds> {
    let text = std::fs::read_to_string(data_dir.join(FILE_NAME)).ok()?;
    let s: Saved = serde_json::from_str(&text).ok()?;
    if s.w < 200.0 || s.h < 150.0 {
        return None;
    }
    let b = Bounds::new(point(px(s.x), px(s.y)), size(px(s.w), px(s.h)));
    let visible = cx.displays().iter().any(|d| d.bounds().intersects(&b));
    visible.then_some(if s.maximized { WindowBounds::Maximized(b) } else { WindowBounds::Windowed(b) })
}

pub fn save(data_dir: &Path, bounds: WindowBounds) {
    let (b, maximized) = match bounds {
        WindowBounds::Windowed(b) => (b, false),
        WindowBounds::Maximized(b) | WindowBounds::Fullscreen(b) => (b, true),
    };
    let s = Saved { x: b.origin.x.into(), y: b.origin.y.into(), w: b.size.width.into(), h: b.size.height.into(), maximized };
    if let Ok(text) = serde_json::to_string(&s) {
        let _ = std::fs::write(data_dir.join(FILE_NAME), text);
    }
}
