//! IPC surface. Every command takes root-relative paths and re-validates them via `Scope`.
//! Filesystem and search work runs on the blocking pool so the webview never waits on it.

use crate::filesystem::{self, Entry, Scope};
use crate::index::{self, Index, ScanOptions, Status};
use crate::search::{self, Corpus, SearchResponse};
use serde::Serialize;
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::{Arc, RwLock};
use tauri::{AppHandle, Emitter, State};

pub struct AppState {
    pub scope: Scope,
    pub index: Option<Arc<Index>>,
    pub corpus: Arc<RwLock<Corpus>>,
    pub status: Arc<Status>,
    pub scan_opts: ScanOptions,
    pub initial: Option<String>,
    pub data_dir: PathBuf,
}

type Res<T> = Result<T, String>;

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> Res<T> + Send + 'static) -> Res<T> {
    tauri::async_runtime::spawn_blocking(f).await.map_err(|e| e.to_string())?
}

#[derive(Serialize)]
pub struct RootInfo {
    name: String,
    abs: String,
    initial: Option<String>,
}

#[derive(Serialize)]
pub struct Listing {
    path: String,
    entries: Vec<Entry>,
}

fn list(scope: &Scope, rel: &str) -> Res<Listing> {
    let path = Scope::normalize(rel).map_err(|e| e.to_string())?;
    let abs = scope.resolve(&path).map_err(|e| e.to_string())?;
    let entries = filesystem::list_dir(&abs).map_err(|e| e.to_string())?;
    Ok(Listing { path, entries })
}

#[tauri::command]
pub fn get_root(state: State<'_, AppState>) -> RootInfo {
    RootInfo { name: state.scope.root_name(), abs: state.scope.root().to_string_lossy().into_owned(), initial: state.initial.clone() }
}

#[tauri::command]
pub async fn list_directory(state: State<'_, AppState>, path: String) -> Res<Listing> {
    let scope = state.scope.clone();
    blocking(move || list(&scope, &path)).await
}

/// Batched listing used for preview columns (one IPC round-trip for N sibling folders).
/// Unreadable folders are skipped instead of failing the whole batch.
#[tauri::command]
pub async fn list_directories(state: State<'_, AppState>, paths: Vec<String>) -> Res<Vec<Listing>> {
    let scope = state.scope.clone();
    blocking(move || Ok(paths.iter().filter_map(|p| list(&scope, p).ok()).collect())).await
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
    let scope = state.scope.clone();
    let idx = state.index.clone();
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
    let corpus = state.corpus.clone();
    let idx = state.index.clone();
    blocking(move || Ok(search::search(&corpus, idx.as_deref(), &query, limit.unwrap_or(40).min(200)))).await
}

#[derive(Serialize, Clone)]
pub struct IndexStatus {
    state: &'static str,
    indexed: u64,
    corpus: usize,
    last_scan_ms: u64,
}

pub fn status_of(status: &Status, corpus: &RwLock<Corpus>) -> IndexStatus {
    IndexStatus {
        state: match status.state.load(Ordering::Relaxed) {
            index::STATE_LOADING => "loading",
            index::STATE_INDEXING => "indexing",
            index::STATE_READY => "ready",
            _ => "idle",
        },
        indexed: status.indexed.load(Ordering::Relaxed),
        corpus: corpus.read().map(|c| c.len()).unwrap_or(0),
        last_scan_ms: status.last_scan_ms.load(Ordering::Relaxed),
    }
}

#[tauri::command]
pub fn index_status(state: State<'_, AppState>) -> IndexStatus {
    status_of(&state.status, &state.corpus)
}

#[tauri::command]
pub fn start_indexing(app: AppHandle, state: State<'_, AppState>) -> bool {
    crate::app::spawn_scan(app, &state)
}

/// Everything the renderer needs to reveal a deep path in one round-trip: the listing of
/// every ancestor (root included) plus the target itself when it is a directory.
#[tauri::command]
pub async fn reveal_path(state: State<'_, AppState>, path: String) -> Res<Vec<Listing>> {
    let scope = state.scope.clone();
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
        chain.iter().map(|p| list(&scope, p)).collect()
    })
    .await
}

#[tauri::command]
pub async fn open_path(state: State<'_, AppState>, path: String) -> Res<()> {
    let scope = state.scope.clone();
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

pub fn emit_changed(app: &AppHandle, dirs: Vec<String>) {
    let _ = app.emit("filesystem_changed", serde_json::json!({ "dirs": dirs }));
}
