//! User settings, persisted as `settings.json` in the app data directory.
//!
//! Several (hidden files, sort order, index exclusions, the root) change what the core does;
//! the others only change how the scene is drawn.
//! Unknown or missing fields fall back to defaults, so older files keep loading.

use crate::filesystem::{ListOptions, SortKey};
use crate::index::{ScanOptions, DEFAULT_EXCLUDES};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

pub const FILE_NAME: &str = "settings.json";
pub const MAX_RECENT: usize = 8;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    // Appearance (drawing only)
    pub color_mode: String,
    pub age_max_days: u32,
    pub font: String,
    pub zoom: f64,
    pub animation_ms: u32,
    pub max_chars: u32,
    pub status_line: bool,
    // Listing
    pub show_hidden: bool,
    pub sort: SortKey,
    pub folders_first: bool,
    // Navigation
    pub wheel_step: u32,
    pub restore_last: bool,
    /// Where the app was left; reopened when `restore_last` is on and no `--root` is given.
    pub last: Option<Location>,
    pub recent_roots: Vec<String>,
    // Index
    pub respect_gitignore: bool,
    pub excludes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Location {
    /// Absolute root.
    pub root: String,
    /// Root-relative selection.
    pub select: Option<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            color_mode: "age".into(),
            age_max_days: 730,
            font: "iosevka".into(),
            zoom: 1.0,
            animation_ms: 240,
            max_chars: 25,
            status_line: false,
            show_hidden: true,
            sort: SortKey::Name,
            folders_first: false,
            wheel_step: 40,
            restore_last: true,
            last: None,
            recent_roots: Vec::new(),
            respect_gitignore: true,
            excludes: DEFAULT_EXCLUDES.iter().map(|s| s.to_string()).collect(),
        }
    }
}

impl Settings {
    pub fn path(data_dir: &Path) -> PathBuf {
        data_dir.join(FILE_NAME)
    }

    /// A missing file yields defaults; a corrupt one too (it is overwritten on the next save).
    pub fn load(data_dir: &Path) -> Settings {
        fs::read_to_string(Self::path(data_dir))
            .ok()
            .and_then(|s| serde_json::from_str::<Settings>(&s).ok())
            .map(Settings::sanitized)
            .unwrap_or_default()
    }

    /// Writes atomically (temp file + rename) so a crash never leaves half a file.
    pub fn save(&self, data_dir: &Path) -> io::Result<()> {
        fs::create_dir_all(data_dir)?;
        let tmp = data_dir.join(format!("{FILE_NAME}.tmp"));
        fs::write(&tmp, serde_json::to_vec_pretty(self)?)?;
        fs::rename(tmp, Self::path(data_dir))
    }

    /// Clamps values the UI relies on to sane ranges.
    pub fn sanitized(mut self) -> Settings {
        if !matches!(self.color_mode.as_str(), "age" | "kind") {
            self.color_mode = "age".into();
        }
        self.age_max_days = self.age_max_days.clamp(1, 3650);
        self.zoom = if self.zoom.is_finite() { self.zoom.clamp(0.5, 3.0) } else { 1.0 };
        self.animation_ms = self.animation_ms.min(2000);
        self.max_chars = self.max_chars.clamp(8, 200);
        self.wheel_step = self.wheel_step.clamp(5, 400);
        self.excludes.retain(|e| !e.trim().is_empty());
        self.recent_roots.truncate(MAX_RECENT);
        self
    }

    pub fn list_options(&self) -> ListOptions {
        ListOptions { show_hidden: self.show_hidden, sort: self.sort, folders_first: self.folders_first }
    }

    pub fn scan_options(&self) -> ScanOptions {
        ScanOptions { respect_gitignore: self.respect_gitignore, excludes: self.excludes.clone(), cancel: None }
    }

    /// Moves `root` to the front of the recent list.
    pub fn remember_root(&mut self, root: &str) {
        self.recent_roots.retain(|r| r != root);
        self.recent_roots.insert(0, root.to_string());
        self.recent_roots.truncate(MAX_RECENT);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_or_corrupt_file_gives_defaults() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(Settings::load(dir.path()), Settings::default());
        fs::write(Settings::path(dir.path()), "{not json").unwrap();
        assert_eq!(Settings::load(dir.path()), Settings::default());
    }

    #[test]
    fn round_trips_and_fills_missing_fields() {
        let dir = tempfile::tempdir().unwrap();
        let s = Settings { show_hidden: false, sort: SortKey::Modified, zoom: 1.25, ..Settings::default() };
        s.save(dir.path()).unwrap();
        assert_eq!(Settings::load(dir.path()), s);
        fs::write(Settings::path(dir.path()), r#"{"font":"jetbrains","sort":"size","unknown":1}"#).unwrap();
        let s = Settings::load(dir.path());
        assert_eq!((s.font.as_str(), s.sort, s.max_chars), ("jetbrains", SortKey::Size, 25));
    }

    #[test]
    fn sanitizes_out_of_range_values() {
        let s = Settings { zoom: f64::NAN, max_chars: 1, color_mode: "neon".into(), excludes: vec![" ".into(), ".git".into()], ..Settings::default() }.sanitized();
        assert_eq!((s.zoom, s.max_chars, s.color_mode.as_str()), (1.0, 8, "age"));
        assert_eq!(s.excludes, [".git"]);
    }

    #[test]
    fn recent_roots_dedupe_and_cap() {
        let mut s = Settings::default();
        for i in 0..12 {
            s.remember_root(&format!("r{i}"));
        }
        s.remember_root("r5");
        assert_eq!(s.recent_roots.len(), MAX_RECENT);
        assert_eq!(s.recent_roots[0], "r5");
        assert_eq!(s.recent_roots.iter().filter(|r| *r == "r5").count(), 1);
    }
}
