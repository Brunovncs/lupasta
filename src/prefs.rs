//! Settings → runtime state, and the settings panel as a flat list of rows in the same
//! one-line-per-entry language as the tree. Pure: the panel renders these rows and routes keys
//! and clicks back through them.

use crate::filesystem::SortKey;
use crate::session::{IndexStatus, RootInfo};
use crate::settings::Settings;
use std::rc::Rc;

pub const AGE_PRESETS: &[(u32, &str)] = &[(7, "1 week"), (30, "1 month"), (182, "6 months"), (365, "1 year"), (730, "2 years"), (1825, "5 years")];
pub const ZOOM_PRESETS: &[f64] = &[0.8, 0.9, 1.0, 1.1, 1.25, 1.5, 1.75, 2.0];
pub const ANIMATION_PRESETS: &[(u32, &str)] = &[(0, "off"), (120, "fast"), (240, "normal"), (400, "slow")];
pub const TRUNCATE_PRESETS: &[u32] = &[16, 25, 40, 60];
/// Wheel delta per row: a smaller step scrolls faster.
pub const WHEEL_PRESETS: &[(u32, &str)] = &[(80, "slow"), (40, "normal"), (20, "fast")];

/// Monospace faces. Each is sized so that its advance is exactly one 10 px cell, so the layout
/// geometry never changes with the font.
pub const FONTS: &[(&str, &str, &str)] = &[
    ("iosevka", "Iosevka", "Iosevka Medium"),
    ("jetbrains", "JetBrains Mono", "JetBrains Mono Medium"),
    ("plex", "IBM Plex Mono", "IBM Plex Mono Medium"),
    ("system", "system", SYSTEM_MONO),
];

#[cfg(windows)]
const SYSTEM_MONO: &str = "Consolas";
#[cfg(target_os = "macos")]
const SYSTEM_MONO: &str = "Menlo";
#[cfg(all(unix, not(target_os = "macos")))]
const SYSTEM_MONO: &str = "DejaVu Sans Mono";

/// The font family to load for a settings id (unknown ids fall back to Iosevka).
pub fn font_family(id: &str) -> &'static str {
    FONTS.iter().find(|f| f.0 == id).unwrap_or(&FONTS[0]).2
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Changes {
    /// Listings must be fetched again (hidden files, sort order).
    pub listing: bool,
    /// The layout must be recomputed (truncation, colours).
    pub layout: bool,
    pub font: bool,
    pub zoom: bool,
    pub motion: bool,
}

pub fn diff(prev: &Settings, next: &Settings) -> Changes {
    Changes {
        listing: prev.show_hidden != next.show_hidden || prev.sort != next.sort || prev.folders_first != next.folders_first,
        layout: prev.max_chars != next.max_chars || prev.color_mode != next.color_mode || prev.age_max_days != next.age_max_days,
        font: prev.font != next.font,
        zoom: prev.zoom != next.zoom,
        motion: prev.animation_ms != next.animation_ms,
    }
}

/// Next/previous zoom step from the presets (Ctrl+= / Ctrl+-).
pub fn step_zoom(zoom: f64, dir: i32) -> f64 {
    let n = ZOOM_PRESETS.len();
    let at = ZOOM_PRESETS.iter().position(|z| *z >= zoom - 1e-6).unwrap_or(n - 1);
    let exact = (ZOOM_PRESETS[at] - zoom).abs() < 1e-6;
    let j = if dir > 0 { if exact { at as i64 + 1 } else { at as i64 } } else { at as i64 - 1 };
    ZOOM_PRESETS[j.clamp(0, n as i64 - 1) as usize]
}

/// "3 days ago", "just now", ... for the status line.
pub fn relative_age(mtime_ms: f64, now_ms: f64) -> String {
    let s = ((now_ms - mtime_ms) / 1000.0).max(0.0);
    let units = [(365.0 * 86400.0, "year"), (30.0 * 86400.0, "month"), (7.0 * 86400.0, "week"), (86400.0, "day"), (3600.0, "hour"), (60.0, "minute")];
    for (secs, name) in units {
        let n = (s / secs).floor() as u64;
        if n >= 1 {
            return format!("{n} {name}{} ago", if n > 1 { "s" } else { "" });
        }
    }
    "just now".into()
}

pub fn format_size(bytes: u64) -> String {
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    let units = ["KB", "MB", "GB", "TB"];
    let mut v = bytes as f64 / 1024.0;
    let mut i = 0;
    while v >= 1024.0 && i < units.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    if v < 10.0 { format!("{v:.1} {}", units[i]) } else { format!("{} {}", v.round(), units[i]) }
}

fn group_thousands(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

pub fn status_text(st: Option<&IndexStatus>) -> String {
    let Some(st) = st else { return String::new() };
    if !st.enabled {
        return "off (--no-index)".into();
    }
    let n = group_thousands(if st.indexed > 0 { st.indexed } else { st.corpus as u64 });
    let t = if st.state == "ready" && st.last_scan_ms > 0 { format!(", last scan {:.1} s", st.last_scan_ms as f64 / 1000.0) } else { String::new() };
    format!("{}, {n} entries{t}", st.state)
}

#[derive(Debug, Clone, PartialEq)]
pub struct UpdateInfo {
    pub current: String,
    pub latest: Option<String>,
    pub newer: bool,
    pub url: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum UpdateState {
    Unknown,
    Checking,
    Failed(String),
    Known(UpdateInfo),
}

fn update_text(u: &UpdateState) -> String {
    match u {
        UpdateState::Unknown => String::new(),
        UpdateState::Checking => "checking…".into(),
        UpdateState::Failed(e) => e.clone(),
        UpdateState::Known(i) => match &i.latest {
            None => "no releases published yet".into(),
            Some(l) if i.newer => format!("{l} available"),
            Some(l) => format!("up to date ({l})"),
        },
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    PickRoot,
    UpRoot,
    OpenRoot(String),
    Reindex,
    ClearIndex,
    CheckUpdate,
    OpenRelease(String),
    OpenData,
}

pub type Apply = Rc<dyn Fn(&Settings) -> Settings>;

#[derive(Clone)]
pub struct Opt {
    pub label: String,
    pub apply: Apply,
}

#[derive(Clone)]
pub enum Row {
    Head(&'static str),
    Choice { label: &'static str, options: Vec<Opt>, index: usize },
    Action { label: &'static str, detail: String, action: Action },
    Edit { label: &'static str, value: String },
    Info { label: String, detail: String },
}

impl Row {
    pub fn focusable(&self) -> bool {
        matches!(self, Row::Choice { .. } | Row::Action { .. } | Row::Edit { .. })
    }

    pub fn label(&self) -> &str {
        match self {
            Row::Head(l) => l,
            Row::Choice { label, .. } | Row::Action { label, .. } | Row::Edit { label, .. } => label,
            Row::Info { label, .. } => label,
        }
    }
}

/// Rows whose options would not fit next to the label show only the current one: ‹ value ›.
pub const WIDE: usize = 34;

pub fn compact(options: &[Opt]) -> bool {
    options.iter().map(|o| o.label.chars().count() + 2).sum::<usize>().saturating_sub(2) > WIDE
}

pub struct Context<'a> {
    pub settings: &'a Settings,
    pub root: Option<&'a RootInfo>,
    pub version: &'a str,
    pub os: &'a str,
    pub data_dir: &'a str,
    pub status: Option<&'a IndexStatus>,
    pub update: &'a UpdateState,
}

fn choice<T: PartialEq + Clone + 'static>(label: &'static str, current: &T, values: Vec<(T, String)>, set: fn(&mut Settings, T)) -> Row {
    let index = values.iter().position(|(v, _)| v == current).unwrap_or(0);
    let options = values
        .into_iter()
        .map(|(v, l)| Opt {
            label: l,
            apply: Rc::new(move |s: &Settings| {
                let mut next = s.clone();
                set(&mut next, v.clone());
                next
            }),
        })
        .collect();
    Row::Choice { label, options, index }
}

fn yes_no() -> Vec<(bool, String)> {
    vec![(true, "on".into()), (false, "off".into())]
}

fn nearest_u32(values: &[u32], v: u32) -> u32 {
    *values.iter().min_by_key(|x| (**x as i64 - v as i64).abs()).unwrap()
}

fn nearest_f64(values: &[f64], v: f64) -> f64 {
    values.iter().copied().fold(values[0], |a, b| if (b - v).abs() < (a - v).abs() { b } else { a })
}

pub const KEYS: &[(&str, &str)] = &[
    ("↑ ↓  wheel", "move"),
    ("→  space", "enter folder"),
    ("←", "parent (at the top: go up a folder)"),
    ("enter  double-click", "open with the default app"),
    ("/  ctrl+k", "search"),
    ("ctrl+c", "copy the full path"),
    ("ctrl+e", "show in the file manager"),
    ("ctrl+h", "show / hide hidden files"),
    ("ctrl+o", "open another folder"),
    ("ctrl+=  ctrl+-  ctrl+0", "zoom"),
    ("ctrl+,", "settings"),
];

pub fn build_rows(c: &Context) -> Vec<Row> {
    let s = c.settings;
    let mut rows = vec![Row::Head("appearance")];
    rows.push(choice("colour", &s.color_mode, vec![("age".to_string(), "by age".into()), ("kind".to_string(), "by kind".into())], |x, v| x.color_mode = v));
    let ages: Vec<u32> = AGE_PRESETS.iter().map(|a| a.0).collect();
    rows.push(choice("oldest colour at", &nearest_u32(&ages, s.age_max_days), AGE_PRESETS.iter().map(|(d, l)| (*d, l.to_string())).collect(), |x, v| x.age_max_days = v));
    rows.push(choice("font", &s.font, FONTS.iter().map(|f| (f.0.to_string(), f.1.to_string())).collect(), |x, v| x.font = v));
    let zoom = nearest_f64(ZOOM_PRESETS, s.zoom);
    rows.push(choice("zoom", &zoom, ZOOM_PRESETS.iter().map(|z| (*z, format!("{}%", (z * 100.0).round()))).collect(), |x, v| x.zoom = v));
    rows.push(choice("animation", &s.animation_ms, ANIMATION_PRESETS.iter().map(|(v, l)| (*v, l.to_string())).collect(), |x, v| x.animation_ms = v));
    rows.push(choice("cut names at", &s.max_chars, TRUNCATE_PRESETS.iter().map(|n| (*n, n.to_string())).collect(), |x, v| x.max_chars = v));
    rows.push(choice("status line", &s.status_line, yes_no(), |x, v| x.status_line = v));

    rows.push(Row::Head("listing"));
    rows.push(choice("hidden files", &s.show_hidden, vec![(true, "show".into()), (false, "hide".into())], |x, v| x.show_hidden = v));
    rows.push(choice(
        "sort by",
        &s.sort,
        vec![(SortKey::Name, "name".into()), (SortKey::Modified, "newest".into()), (SortKey::Size, "largest".into())],
        |x, v| x.sort = v,
    ));
    rows.push(choice("folders first", &s.folders_first, yes_no(), |x, v| x.folders_first = v));

    rows.push(Row::Head("navigation"));
    rows.push(choice("wheel speed", &s.wheel_step, WHEEL_PRESETS.iter().map(|(v, l)| (*v, l.to_string())).collect(), |x, v| x.wheel_step = v));
    rows.push(choice("reopen last place", &s.restore_last, yes_no(), |x, v| x.restore_last = v));

    rows.push(Row::Head("folder"));
    rows.push(Row::Action { label: "open another…", detail: c.root.map(|r| r.abs.clone()).unwrap_or_default(), action: Action::PickRoot });
    if let Some(parent) = c.root.and_then(|r| r.parent.clone()) {
        rows.push(Row::Action { label: "go up", detail: parent, action: Action::UpRoot });
    }
    for r in s.recent_roots.iter().filter(|r| Some(r.as_str()) != c.root.map(|x| x.abs.as_str())) {
        rows.push(Row::Action { label: "recent", detail: r.clone(), action: Action::OpenRoot(r.clone()) });
    }

    rows.push(Row::Head("search index"));
    rows.push(Row::Info { label: "status".into(), detail: status_text(c.status) });
    if let Some(e) = c.status.and_then(|st| st.watcher_error.clone()) {
        rows.push(Row::Info { label: "live updates".into(), detail: format!("unavailable: {e}") });
    }
    rows.push(choice("respect .gitignore", &s.respect_gitignore, yes_no(), |x, v| x.respect_gitignore = v));
    rows.push(Row::Edit { label: "skip folders", value: s.excludes.join(" ") });
    rows.push(Row::Action { label: "rescan now", detail: String::new(), action: Action::Reindex });
    rows.push(Row::Action { label: "rebuild from scratch", detail: String::new(), action: Action::ClearIndex });

    rows.push(Row::Head("about"));
    rows.push(Row::Info { label: "version".into(), detail: format!("{} ({})", c.version, c.os) });
    match c.update {
        UpdateState::Known(UpdateInfo { newer: true, url: Some(url), .. }) => {
            rows.push(Row::Action { label: "download update", detail: update_text(c.update), action: Action::OpenRelease(url.clone()) })
        }
        _ => rows.push(Row::Action { label: "check for updates", detail: update_text(c.update), action: Action::CheckUpdate }),
    }
    rows.push(Row::Action { label: "settings & index", detail: c.data_dir.to_string(), action: Action::OpenData });

    rows.push(Row::Head("keys"));
    for (k, what) in KEYS {
        rows.push(Row::Info { label: k.to_string(), detail: what.to_string() });
    }
    rows
}

/// The "skip folders" edit: whitespace-separated folder names.
pub fn commit_excludes(s: &Settings, text: &str) -> Settings {
    let mut next = s.clone();
    next.excludes = text.split_whitespace().map(str::to_string).collect();
    next
}

/// Index of the next focusable row from `i` in direction `dir` (stays put at the ends).
pub fn next_focus(rows: &[Row], i: i64, dir: i64) -> i64 {
    let mut j = i + dir;
    while j >= 0 && (j as usize) < rows.len() {
        if rows[j as usize].focusable() {
            return j;
        }
        j += dir;
    }
    i
}

/// The settings after moving a choice row's selection by `dir`, wrapping around.
pub fn cycle(s: &Settings, row: &Row, dir: i64) -> Settings {
    let Row::Choice { options, index, .. } = row else { return s.clone() };
    let n = options.len() as i64;
    (options[((*index as i64 + dir + n) % n) as usize].apply)(s)
}

/// Search hit display: the folder part (with its trailing '/') and the name.
pub fn split_hit(path: &str) -> (&str, &str) {
    match path.rfind('/') {
        Some(i) => (&path[..=i], &path[i + 1..]),
        None => ("", path),
    }
}

/// Keeps the tail of long parent paths, which is the informative part.
pub fn clip_dir(dir: &str, max: usize) -> String {
    let chars: Vec<char> = dir.chars().collect();
    if chars.len() <= max {
        return dir.to_string();
    }
    let tail: String = chars[chars.len() - max + 1..].iter().collect();
    format!("…{tail}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zoom_steps_through_presets() {
        assert_eq!(step_zoom(1.0, 1), 1.1);
        assert_eq!(step_zoom(1.0, -1), 0.9);
        assert_eq!(step_zoom(1.05, 1), 1.1);
        assert_eq!(step_zoom(2.0, 1), 2.0);
        assert_eq!(step_zoom(0.8, -1), 0.8);
    }

    #[test]
    fn sizes_and_ages() {
        assert_eq!(format_size(512), "512 B");
        assert_eq!(format_size(1536), "1.5 KB");
        assert_eq!(format_size(50 * 1024 * 1024), "50 MB");
        assert_eq!(relative_age(0.0, 3.0 * 86400.0 * 1000.0), "3 days ago");
        assert_eq!(relative_age(0.0, 1000.0), "just now");
    }

    #[test]
    fn rows_cycle_and_focus() {
        let s = Settings::default();
        let st = UpdateState::Unknown;
        let ctx = Context { settings: &s, root: None, version: "0.1.0", os: "windows", data_dir: "", status: None, update: &st };
        let rows = build_rows(&ctx);
        let first = next_focus(&rows, -1, 1) as usize;
        assert_eq!(rows[first].label(), "colour");
        assert_eq!(cycle(&s, &rows[first], 1).color_mode, "kind");
        assert_eq!(cycle(&s, &rows[first], -1).color_mode, "kind");
        let font = rows.iter().find(|r| r.label() == "font").unwrap();
        assert!(matches!(font, Row::Choice { options, .. } if compact(options)));
    }

    #[test]
    fn hits_split_and_clip() {
        assert_eq!(split_hit("a/b/c.txt"), ("a/b/", "c.txt"));
        assert_eq!(clip_dir("abcdef", 4), "…def");
        assert_eq!(group_thousands(1234567), "1,234,567");
    }
}
