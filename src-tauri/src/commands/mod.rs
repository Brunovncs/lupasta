//! IPC surface. Every command takes root-relative paths and re-validates them via `Scope`.
//! Filesystem and search work runs on the blocking pool so the webview never waits on it.

use crate::filesystem::{self, Entry, ListOptions, Scope};
use crate::index::{self, Index, ScanOptions, Status};
use crate::search::{self, Corpus, SearchResponse};
use crate::settings::{Location, Settings};
use crate::watcher::WatchHandle;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use tauri::{AppHandle, Emitter, Manager, State};

/// Everything tied to one root. Replaced wholesale when the root changes; background threads
/// hold their own `Arc` and check `AppState::is_current` before emitting anything.
pub struct Session {
    pub generation: u64,
    pub scope: Scope,
    pub index: Option<Arc<Index>>,
    pub corpus: Arc<RwLock<Corpus>>,
    pub status: Arc<Status>,
    pub scan_opts: RwLock<ScanOptions>,
    pub cancel: Arc<AtomicBool>,
    pub initial: Option<String>,
    pub watcher: Mutex<Option<WatchHandle>>,
    pub watch_error: Mutex<Option<String>>,
}

pub struct AppState {
    session: RwLock<Arc<Session>>,
    next_generation: AtomicU64,
    pub settings: RwLock<Settings>,
    pub data_dir: PathBuf,
    pub no_index: bool,
    pub capture: bool,
    pub smoke: bool,
    /// Off while the root came from `--root` (tests, `dev:fixture`), so it is not remembered.
    pub persist_location: AtomicBool,
}

impl AppState {
    pub fn new(session: Arc<Session>, settings: Settings, data_dir: PathBuf, no_index: bool, capture: bool, smoke: bool, persist: bool) -> Self {
        AppState {
            next_generation: AtomicU64::new(session.generation + 1),
            session: RwLock::new(session),
            settings: RwLock::new(settings),
            data_dir,
            no_index,
            capture,
            smoke,
            persist_location: AtomicBool::new(persist),
        }
    }

    pub fn session(&self) -> Arc<Session> {
        self.session.read().unwrap().clone()
    }

    pub fn is_current(&self, generation: u64) -> bool {
        self.session.read().unwrap().generation == generation
    }

    pub fn next_generation(&self) -> u64 {
        self.next_generation.fetch_add(1, Ordering::SeqCst)
    }

    /// Swaps in a new session; the old one stops watching and cancels its scan.
    pub fn replace(&self, next: Arc<Session>) {
        let old = std::mem::replace(&mut *self.session.write().unwrap(), next);
        old.cancel.store(true, Ordering::SeqCst);
        old.watcher.lock().unwrap().take();
    }

    pub fn list_options(&self) -> ListOptions {
        self.settings.read().unwrap().list_options()
    }

    fn save_settings(&self, s: &Settings) -> Res<()> {
        s.save(&self.data_dir).map_err(|e| format!("could not save settings: {e}"))
    }
}

type Res<T> = Result<T, String>;

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> Res<T> + Send + 'static) -> Res<T> {
    tauri::async_runtime::spawn_blocking(f).await.map_err(|e| e.to_string())?
}

#[derive(Serialize)]
pub struct RootInfo {
    name: String,
    abs: String,
    /// The folder above the root, when there is one to go up to.
    parent: Option<String>,
    initial: Option<String>,
    generation: u64,
}

fn root_info(s: &Session) -> RootInfo {
    let root = s.scope.root();
    RootInfo {
        name: s.scope.root_name(),
        abs: filesystem::display_path(root),
        parent: root.parent().map(filesystem::display_path),
        initial: s.initial.clone(),
        generation: s.generation,
    }
}

#[derive(Serialize)]
pub struct Listing {
    path: String,
    entries: Vec<Entry>,
}

fn list(scope: &Scope, rel: &str, opts: &ListOptions) -> Res<Listing> {
    let path = Scope::normalize(rel).map_err(|e| e.to_string())?;
    let abs = scope.resolve(&path).map_err(|e| e.to_string())?;
    let entries = filesystem::list_dir(&abs).map_err(|e| e.to_string())?;
    Ok(Listing { path, entries: filesystem::present(entries, opts) })
}

#[tauri::command]
pub fn get_root(state: State<'_, AppState>) -> RootInfo {
    root_info(&state.session())
}

#[tauri::command]
pub async fn list_directory(state: State<'_, AppState>, path: String) -> Res<Listing> {
    let (scope, opts) = (state.session().scope.clone(), state.list_options());
    blocking(move || list(&scope, &path, &opts)).await
}

/// Batched listing used for preview columns (one IPC round-trip for N sibling folders).
/// Unreadable folders are skipped instead of failing the whole batch.
#[tauri::command]
pub async fn list_directories(state: State<'_, AppState>, paths: Vec<String>) -> Res<Vec<Listing>> {
    let (scope, opts) = (state.session().scope.clone(), state.list_options());
    blocking(move || Ok(paths.iter().filter_map(|p| list(&scope, p, &opts).ok()).collect())).await
}

#[tauri::command]
pub async fn get_children(state: State<'_, AppState>, path: String) -> Res<Listing> {
    list_directory(state, path).await
}

#[tauri::command]
pub async fn expand_directory(state: State<'_, AppState>, path: String) -> Res<Listing> {
    list_directory(state, path).await
}

#[derive(Serialize)]
pub struct Metadata {
    path: String,
    entry: Entry,
    kind: &'static str,
    extension: String,
    indexed: bool,
}

#[tauri::command]
pub async fn get_file_metadata(state: State<'_, AppState>, path: String) -> Res<Metadata> {
    let s = state.session();
    let (scope, idx) = (s.scope.clone(), s.index.clone());
    blocking(move || {
        let path = Scope::normalize(&path).map_err(|e| e.to_string())?;
        let abs = scope.resolve(&path).map_err(|e| e.to_string())?;
        let name = filesystem::name_of(&path).to_string();
        let entry = filesystem::entry_for(&abs, name.clone()).map_err(|e| e.to_string())?;
        let kind = index::classify(&name, entry.is_dir(), entry.1 & filesystem::FLAG_HIDDEN != 0);
        let indexed = idx.map(|i| i.get(&path).ok().flatten().is_some()).unwrap_or(false);
        Ok(Metadata { extension: index::extension_of(&name), path, entry, kind, indexed })
    })
    .await
}

#[tauri::command]
pub async fn search_files(state: State<'_, AppState>, query: String, limit: Option<usize>) -> Res<SearchResponse> {
    let s = state.session();
    let (corpus, idx) = (s.corpus.clone(), s.index.clone());
    blocking(move || Ok(search::search(&corpus, idx.as_deref(), &query, limit.unwrap_or(40).min(200)))).await
}

#[derive(Serialize, Clone)]
pub struct IndexStatus {
    state: &'static str,
    indexed: u64,
    corpus: usize,
    last_scan_ms: u64,
    enabled: bool,
    watcher_error: Option<String>,
}

pub fn status_of(s: &Session) -> IndexStatus {
    let status = &s.status;
    IndexStatus {
        state: match status.state.load(Ordering::Relaxed) {
            index::STATE_LOADING => "loading",
            index::STATE_INDEXING => "indexing",
            index::STATE_READY => "ready",
            _ => "idle",
        },
        indexed: status.indexed.load(Ordering::Relaxed),
        corpus: s.corpus.read().map(|c| c.len()).unwrap_or(0),
        last_scan_ms: status.last_scan_ms.load(Ordering::Relaxed),
        enabled: s.index.is_some(),
        watcher_error: s.watch_error.lock().unwrap().clone(),
    }
}

#[tauri::command]
pub fn index_status(state: State<'_, AppState>) -> IndexStatus {
    status_of(&state.session())
}

#[tauri::command]
pub fn start_indexing(app: AppHandle, state: State<'_, AppState>) -> bool {
    crate::app::spawn_scan(app, state.session())
}

/// Empties the index and rebuilds it from scratch (waits for a running scan to stop first).
#[tauri::command]
pub async fn clear_index(app: AppHandle, state: State<'_, AppState>) -> Res<bool> {
    let s = state.session();
    blocking(move || {
        let Some(idx) = s.index.clone() else { return Ok(false) };
        s.cancel.store(true, Ordering::SeqCst);
        for _ in 0..600 {
            if s.status.state.load(Ordering::SeqCst) != index::STATE_INDEXING {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        idx.clear().map_err(|e| e.to_string())?;
        *s.corpus.write().unwrap() = Corpus::default();
        s.status.indexed.store(0, Ordering::Relaxed);
        s.cancel.store(false, Ordering::SeqCst);
        Ok(crate::app::spawn_scan(app, s))
    })
    .await
}

/// Everything the renderer needs to reveal a deep path in one round-trip: the listing of
/// every ancestor (root included) plus the target itself when it is a directory.
#[tauri::command]
pub async fn reveal_path(state: State<'_, AppState>, path: String) -> Res<Vec<Listing>> {
    let (scope, opts) = (state.session().scope.clone(), state.list_options());
    blocking(move || {
        let path = Scope::normalize(&path).map_err(|e| e.to_string())?;
        let abs = scope.resolve(&path).map_err(|e| e.to_string())?;
        let mut chain = vec![String::new()];
        let mut acc = String::new();
        let parts: Vec<&str> = path.split('/').filter(|p| !p.is_empty()).collect();
        for (i, part) in parts.iter().enumerate() {
            if !acc.is_empty() {
                acc.push('/');
            }
            acc.push_str(part);
            if i + 1 < parts.len() || abs.is_dir() {
                chain.push(acc.clone());
            }
        }
        chain.iter().map(|p| list(&scope, p, &opts)).collect()
    })
    .await
}

#[tauri::command]
pub async fn open_path(state: State<'_, AppState>, path: String) -> Res<()> {
    let scope = state.session().scope.clone();
    blocking(move || {
        let abs = scope.resolve(&path).map_err(|e| e.to_string())?;
        #[cfg(windows)]
        let cmd = std::process::Command::new("explorer").arg(&abs).spawn();
        #[cfg(target_os = "macos")]
        let cmd = std::process::Command::new("open").arg(&abs).spawn();
        #[cfg(all(unix, not(target_os = "macos")))]
        let cmd = std::process::Command::new("xdg-open").arg(&abs).spawn();
        cmd.map(|_| ()).map_err(|e| e.to_string())
    })
    .await
}

/// Shows the item selected in the system file manager (Explorer, Finder, or whatever
/// implements org.freedesktop.FileManager1 on Linux; falls back to opening the parent).
#[tauri::command]
pub async fn reveal_in_os(state: State<'_, AppState>, path: String) -> Res<()> {
    let scope = state.session().scope.clone();
    blocking(move || {
        let abs = scope.resolve(&path).map_err(|e| e.to_string())?;
        let shown = filesystem::display_path(&abs);
        reveal(&shown).map_err(|e| e.to_string())
    })
    .await
}

#[cfg(windows)]
fn reveal(p: &str) -> std::io::Result<()> {
    use std::os::windows::process::CommandExt;
    // explorer parses its own command line: the path must be quoted inside the /select, token.
    std::process::Command::new("explorer").raw_arg(format!("/select,\"{p}\"")).spawn().map(|_| ())
}

#[cfg(target_os = "macos")]
fn reveal(p: &str) -> std::io::Result<()> {
    std::process::Command::new("open").arg("-R").arg(p).spawn().map(|_| ())
}

#[cfg(all(unix, not(target_os = "macos")))]
fn reveal(p: &str) -> std::io::Result<()> {
    let uri = format!("file://{}", percent_encode(p));
    let ok = std::process::Command::new("dbus-send")
        .args(["--session", "--dest=org.freedesktop.FileManager1", "--type=method_call", "/org/freedesktop/FileManager1", "org.freedesktop.FileManager1.ShowItems"])
        .arg(format!("array:string:{uri}"))
        .arg("string:")
        .status()
        .is_ok_and(|s| s.success());
    if ok {
        return Ok(());
    }
    let parent = Path::new(p).parent().unwrap_or(Path::new(p));
    std::process::Command::new("xdg-open").arg(parent).spawn().map(|_| ())
}

#[cfg(all(unix, not(target_os = "macos")))]
fn percent_encode(p: &str) -> String {
    p.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'/' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// Absolute, human-readable path of a root-relative one (for "copy path").
#[tauri::command]
pub fn abs_path(state: State<'_, AppState>, path: String) -> Res<String> {
    let abs = state.session().scope.resolve(&path).map_err(|e| e.to_string())?;
    Ok(filesystem::display_path(&abs))
}

#[tauri::command]
pub fn get_settings(state: State<'_, AppState>) -> Settings {
    state.settings.read().unwrap().clone()
}

/// Stores new settings. Index-affecting changes (gitignore, exclusions) trigger a resync,
/// which also purges rows that are now excluded. The caller reloads listings itself.
#[tauri::command]
pub fn set_settings(app: AppHandle, state: State<'_, AppState>, settings: Settings) -> Res<Settings> {
    let mut next = settings.sanitized();
    let rescan = {
        let cur = state.settings.read().unwrap();
        // Bookkeeping fields are owned by the backend; the renderer's copy may be stale.
        next.last = cur.last.clone();
        next.recent_roots = cur.recent_roots.clone();
        cur.respect_gitignore != next.respect_gitignore || cur.excludes != next.excludes
    };
    state.save_settings(&next)?;
    *state.settings.write().unwrap() = next.clone();
    if rescan {
        let s = state.session();
        *s.scan_opts.write().unwrap() = ScanOptions { cancel: Some(s.cancel.clone()), ..next.scan_options() };
        crate::app::spawn_scan(app, s);
    }
    Ok(next)
}

/// Remembers the selection so the next launch reopens it (debounced by the renderer).
#[tauri::command]
pub fn remember_selection(state: State<'_, AppState>, path: String) -> Res<()> {
    if !state.persist_location.load(Ordering::Relaxed) {
        return Ok(());
    }
    let root = filesystem::display_path(state.session().scope.root());
    let mut s = state.settings.write().unwrap();
    let loc = Some(Location { root, select: Some(path).filter(|p| !p.is_empty()) });
    if s.last == loc {
        return Ok(());
    }
    s.last = loc;
    state.save_settings(&s)
}

/// Re-roots the browser at an absolute folder. `select` is relative to the new root.
#[tauri::command]
pub async fn set_root(app: AppHandle, path: String, select: Option<String>) -> Res<RootInfo> {
    blocking(move || {
        let state = app.state::<AppState>();
        let target = Path::new(&path);
        if !target.is_dir() {
            return Err(format!("not a folder: {path}"));
        }
        let session = crate::app::open_session(&app, target, select)?;
        let info = root_info(&session);
        state.replace(session.clone());
        crate::app::start_session(&app, session);
        state.persist_location.store(true, Ordering::Relaxed);
        let mut s = state.settings.write().unwrap();
        s.remember_root(&info.abs);
        s.last = Some(Location { root: info.abs.clone(), select: info.initial.clone() });
        state.save_settings(&s)?;
        Ok(info)
    })
    .await
}

#[derive(Serialize)]
pub struct AppInfo {
    version: &'static str,
    os: &'static str,
    data_dir: String,
    capture: bool,
    smoke: bool,
}

#[tauri::command]
pub fn app_info(state: State<'_, AppState>) -> AppInfo {
    AppInfo {
        version: env!("CARGO_PKG_VERSION"),
        os: std::env::consts::OS,
        data_dir: filesystem::display_path(&state.data_dir),
        capture: state.capture,
        smoke: state.smoke,
    }
}

#[tauri::command]
pub async fn open_data_dir(state: State<'_, AppState>) -> Res<()> {
    let dir = state.data_dir.clone();
    blocking(move || {
        #[cfg(windows)]
        let cmd = std::process::Command::new("explorer").arg(&dir).spawn();
        #[cfg(target_os = "macos")]
        let cmd = std::process::Command::new("open").arg(&dir).spawn();
        #[cfg(all(unix, not(target_os = "macos")))]
        let cmd = std::process::Command::new("xdg-open").arg(&dir).spawn();
        cmd.map(|_| ()).map_err(|e| e.to_string())
    })
    .await
}

const REPO: &str = "Brunovncs/lupasta";

#[derive(Serialize)]
pub struct UpdateInfo {
    current: &'static str,
    latest: Option<String>,
    newer: bool,
    url: Option<String>,
}

/// Asks GitHub for the latest published release. No auto-install: that needs signed builds.
#[tauri::command]
pub async fn check_update() -> Res<UpdateInfo> {
    blocking(|| {
        let current = env!("CARGO_PKG_VERSION");
        let url = format!("https://api.github.com/repos/{REPO}/releases/latest");
        let resp = ureq::get(&url)
            .header("User-Agent", concat!("lupasta/", env!("CARGO_PKG_VERSION")))
            .header("Accept", "application/vnd.github+json")
            .call();
        let mut resp = match resp {
            Ok(r) => r,
            Err(ureq::Error::StatusCode(404)) => return Ok(UpdateInfo { current, latest: None, newer: false, url: None }),
            Err(e) => return Err(format!("update check failed: {e}")),
        };
        let v: serde_json::Value = resp.body_mut().read_json().map_err(|e| e.to_string())?;
        let tag = v["tag_name"].as_str().map(str::to_string);
        let page = v["html_url"].as_str().map(str::to_string);
        let newer = tag.as_deref().is_some_and(|t| crate::update::is_newer(t, current));
        Ok(UpdateInfo { current, latest: tag, newer, url: page })
    })
    .await
}

/// Opens a release page of this project in the browser; any other URL is refused.
#[tauri::command]
pub async fn open_release(url: String) -> Res<()> {
    if !url.starts_with(&format!("https://github.com/{REPO}/")) {
        return Err("refused: not a lupasta release URL".into());
    }
    blocking(move || {
        #[cfg(windows)]
        let cmd = std::process::Command::new("explorer").arg(&url).spawn();
        #[cfg(target_os = "macos")]
        let cmd = std::process::Command::new("open").arg(&url).spawn();
        #[cfg(all(unix, not(target_os = "macos")))]
        let cmd = std::process::Command::new("xdg-open").arg(&url).spawn();
        cmd.map(|_| ()).map_err(|e| e.to_string())
    })
    .await
}

/// `--smoke`: the renderer reports whether it got a layout and a search hit; the process exits
/// with 0 on success so CI can run the real app on every platform.
#[tauri::command]
pub fn smoke_report(app: AppHandle, state: State<'_, AppState>, ok: bool, detail: String) {
    if !state.smoke {
        return;
    }
    let watcher_error = state.session().watch_error.lock().unwrap().clone();
    let ok = ok && watcher_error.is_none();
    let line = format!("smoke {}: {detail}{}", if ok { "ok" } else { "FAILED" }, watcher_error.map(|e| format!(" (watcher: {e})")).unwrap_or_default());
    crate::app::smoke_log(&state.data_dir, &line);
    app.exit(if ok { 0 } else { 1 });
}

pub fn emit_changed(app: &AppHandle, dirs: Vec<String>) {
    let _ = app.emit("filesystem_changed", serde_json::json!({ "dirs": dirs }));
}
