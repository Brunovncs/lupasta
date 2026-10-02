//! Everything tied to one root, and the background work around it: the index lifecycle and the
//! filesystem watcher. Replaced wholesale when the root changes; background threads hold their
//! own `Arc` and tag what they report with the session's generation, so the UI can drop news
//! about a root it already left.

use crate::filesystem::{self, ListOptions, Scope};
use crate::index::{self, Index, ScanOptions, Status};
use crate::search::Corpus;
use crate::settings::Settings;
use crate::tree::Listing;
use crate::watcher::{self, WatchHandle};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock, Weak};

/// News from background threads, for the UI thread.
#[derive(Debug, Clone)]
pub enum CoreEvent {
    /// The index state or progress changed.
    Index { generation: u64 },
    /// These root-relative folders changed on disk.
    Changed { generation: u64, dirs: Vec<String> },
}

pub type EventTx = async_channel::Sender<CoreEvent>;

pub struct Session {
    pub generation: u64,
    pub scope: Scope,
    pub index: Option<Arc<Index>>,
    pub corpus: Arc<RwLock<Corpus>>,
    pub status: Arc<Status>,
    pub scan_opts: RwLock<ScanOptions>,
    pub cancel: Arc<AtomicBool>,
    /// Root-relative selection to reveal first.
    pub initial: Option<String>,
    pub watcher: Mutex<Option<WatchHandle>>,
    pub watch_error: Mutex<Option<String>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RootInfo {
    pub name: String,
    pub abs: String,
    /// The folder above the root, when there is one to go up to.
    pub parent: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct IndexStatus {
    pub state: &'static str,
    pub indexed: u64,
    pub corpus: usize,
    pub last_scan_ms: u64,
    pub enabled: bool,
    pub watcher_error: Option<String>,
}

impl Session {
    pub fn root_info(&self) -> RootInfo {
        let root = self.scope.root();
        RootInfo { name: self.scope.root_name(), abs: filesystem::display_path(root), parent: root.parent().map(filesystem::display_path) }
    }

    pub fn status(&self) -> IndexStatus {
        let st = &self.status;
        IndexStatus {
            state: match st.state.load(Ordering::Relaxed) {
                index::STATE_LOADING => "loading",
                index::STATE_INDEXING => "indexing",
                index::STATE_READY => "ready",
                _ => "idle",
            },
            indexed: st.indexed.load(Ordering::Relaxed),
            corpus: self.corpus.read().map(|c| c.len()).unwrap_or(0),
            last_scan_ms: st.last_scan_ms.load(Ordering::Relaxed),
            enabled: self.index.is_some(),
            watcher_error: self.watch_error.lock().unwrap().clone(),
        }
    }

    /// Stops watching and cancels a running scan (the root is being replaced).
    pub fn retire(&self) {
        self.cancel.store(true, Ordering::SeqCst);
        self.watcher.lock().unwrap().take();
    }
}

fn root_hash(root: &Path) -> String {
    format!("{:016x}", index::path_id(&root.to_string_lossy()))
}

/// Builds (but does not start) the session for `root`: scope, index file, corpus.
pub fn build_session(root: &Path, initial: Option<String>, data_dir: &Path, no_index: bool, settings: &Settings, generation: u64) -> Result<Arc<Session>, String> {
    let scope = Scope::new(root).map_err(|e| format!("{}: {e}", root.display()))?;
    let index = if no_index {
        None
    } else {
        let file = data_dir.join(format!("index-{}.sqlite", root_hash(scope.root())));
        Some(Arc::new(Index::open(&file).map_err(|e| e.to_string())?))
    };
    let cancel = Arc::new(AtomicBool::new(false));
    let scan_opts = ScanOptions { cancel: Some(cancel.clone()), ..settings.scan_options() };
    Ok(Arc::new(Session {
        generation,
        scope,
        index,
        corpus: Arc::new(RwLock::new(Corpus::default())),
        status: Arc::new(Status::default()),
        scan_opts: RwLock::new(scan_opts),
        cancel,
        initial,
        watcher: Mutex::new(None),
        watch_error: Mutex::new(None),
    }))
}

/// Starts a background full scan unless one is already running.
pub fn spawn_scan(s: Arc<Session>, tx: EventTx) -> bool {
    if s.index.is_none() {
        return false;
    }
    if s.status.state.swap(index::STATE_INDEXING, Ordering::SeqCst) == index::STATE_INDEXING {
        return false;
    }
    std::thread::Builder::new()
        .name("indexer".into())
        .spawn(move || {
            let idx = s.index.clone().unwrap();
            let opts = s.scan_opts.read().unwrap().clone();
            let generation = s.generation;
            let progress = |n: u64| {
                s.status.indexed.store(n, Ordering::Relaxed);
                if n % 16384 < 4096 {
                    let _ = tx.try_send(CoreEvent::Index { generation });
                }
            };
            match index::scan(&s.scope, s.scope.root(), &idx, &s.corpus, &opts, true, &progress) {
                Ok(stats) => {
                    s.status.indexed.store(stats.entries, Ordering::Relaxed);
                    s.status.last_scan_ms.store(stats.elapsed_ms, Ordering::Relaxed);
                }
                Err(e) => eprintln!("scan failed: {e}"),
            }
            s.status.state.store(index::STATE_READY, Ordering::SeqCst);
            let _ = tx.send_blocking(CoreEvent::Index { generation });
        })
        .is_ok()
}

/// Starts the watcher and the index lifecycle (load the persisted index, then resync) for the
/// session that is now current. Never blocks the caller.
pub fn start_session(s: Arc<Session>, data_dir: &Path, tx: EventTx) {
    if s.index.is_none() {
        return;
    }
    let data_canon = std::fs::canonicalize(data_dir).ok();
    // The callback holds a weak reference: the session owns the watcher, so dropping the session
    // is enough to stop everything.
    let weak: Weak<Session> = Arc::downgrade(&s);
    let watch_tx = tx.clone();
    let watched = watcher::watch(s.scope.root(), data_canon, move |batch| {
        let Some(s) = weak.upgrade() else { return };
        if s.cancel.load(Ordering::Relaxed) {
            return;
        }
        if batch.rescan {
            spawn_scan(s.clone(), watch_tx.clone());
        }
        let Some(idx) = s.index.as_deref() else { return };
        let opts = s.scan_opts.read().unwrap().clone();
        match index::apply_paths(&s.scope, idx, &s.corpus, &opts, &batch.paths) {
            Ok(dirs) if !dirs.is_empty() => {
                let _ = watch_tx.send_blocking(CoreEvent::Changed { generation: s.generation, dirs: dirs.into_iter().collect() });
            }
            Ok(_) => {}
            Err(e) => eprintln!("watch update failed: {e}"),
        }
    });
    match watched {
        Ok(h) => *s.watcher.lock().unwrap() = Some(h),
        Err(e) => {
            // Typically the inotify watch limit on Linux; browsing still works, live updates do not.
            eprintln!("watcher unavailable: {e}");
            *s.watch_error.lock().unwrap() = Some(e.to_string());
        }
    }

    std::thread::spawn(move || {
        let idx = s.index.clone().unwrap();
        s.status.state.store(index::STATE_LOADING, Ordering::SeqCst);
        {
            let mut c = s.corpus.write().unwrap();
            let _ = idx.load_all(|id, path, flags| c.upsert(id, &path, flags));
        }
        s.status.state.store(index::STATE_IDLE, Ordering::SeqCst);
        if !s.cancel.load(Ordering::Relaxed) {
            let _ = tx.send_blocking(CoreEvent::Index { generation: s.generation });
            spawn_scan(s, tx);
        }
    });
}

/// Empties the index and rebuilds it from scratch (waits for a running scan to stop first).
/// Blocking: run it off the UI thread.
pub fn clear_index(s: Arc<Session>, tx: EventTx) -> Result<bool, String> {
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
    Ok(spawn_scan(s, tx))
}

/// Applies new index settings to a session and resyncs when they affect what is indexed.
pub fn rescan_with(s: Arc<Session>, settings: &Settings, tx: EventTx) {
    *s.scan_opts.write().unwrap() = ScanOptions { cancel: Some(s.cancel.clone()), ..settings.scan_options() };
    spawn_scan(s, tx);
}

/// Lists one folder. Paths are root-relative and re-validated against the scope.
pub fn list(scope: &Scope, rel: &str, opts: &ListOptions) -> Result<Listing, String> {
    let path = Scope::normalize(rel).map_err(|e| e.to_string())?;
    let abs = scope.resolve(&path).map_err(|e| e.to_string())?;
    let entries = filesystem::list_dir(&abs).map_err(|e| e.to_string())?;
    Ok(Listing { path, entries: filesystem::present(entries, opts) })
}

/// Batched listing for preview columns. Unreadable folders are skipped instead of failing the
/// whole batch.
pub fn list_many(scope: &Scope, paths: &[String], opts: &ListOptions) -> Vec<Listing> {
    paths.iter().filter_map(|p| list(scope, p, opts).ok()).collect()
}

/// Everything needed to reveal a deep path at once: the listing of every ancestor (root
/// included) plus the target itself when it is a directory.
pub fn reveal_chain(scope: &Scope, rel: &str, opts: &ListOptions) -> Result<Vec<Listing>, String> {
    let path = Scope::normalize(rel).map_err(|e| e.to_string())?;
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
    chain.iter().map(|p| list(scope, p, opts)).collect()
}

/// Picks the starting root and selection: `--root` wins, then the last location (when
/// `restore_last` is on and it still exists), then the folder that contains the home directory
/// with the home selected. The bool is whether the location may be remembered.
pub fn starting_point(root: Option<PathBuf>, select: Option<String>, settings: &Settings, home: Option<PathBuf>) -> (PathBuf, Option<String>, bool) {
    if let Some(r) = root {
        return (r, select, false);
    }
    if settings.restore_last
        && let Some(last) = settings.last.as_ref().filter(|l| Path::new(&l.root).is_dir()) {
            return (PathBuf::from(&last.root), select.or(last.select.clone()), true);
        }
    match home {
        Some(h) => {
            let parent = h.parent().map(|p| p.to_path_buf()).unwrap_or_else(|| h.clone());
            let sel = h.file_name().map(|n| n.to_string_lossy().into_owned());
            (parent, select.or(sel), true)
        }
        None => (PathBuf::from("."), select, true),
    }
}
