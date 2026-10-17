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
    /// Batches the watcher produced while a scan was running. The scan applies them when it ends
    /// (see `drain_watch_queue`), so its flushes can never resurrect a path the watcher deleted.
    gate: Mutex<WatchQueue>,
}

/// See `Session::gate`.
#[derive(Default)]
struct WatchQueue {
    paths: Vec<PathBuf>,
    /// A scan was requested while one was already running (a settings change, a rescan after an
    /// event overflow, the Reindex button): run another scan once this one finishes.
    rescan: bool,
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
            // Non-blocking on purpose: this is called from the UI thread, and the loader holds
            // the corpus write lock for a whole load. Falling back to the scan count keeps a
            // frame from stalling behind it.
            corpus: self.corpus.try_read().map(|c| c.len()).unwrap_or_else(|_| st.indexed.load(Ordering::Relaxed) as usize),
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

/// Opens the index file, discarding and recreating it once when it is unreadable (corrupt
/// database or WAL). The index is a rebuildable cache, so a bad file must never stop its root
/// from opening.
fn open_or_rebuild(file: &Path) -> Result<Index, String> {
    match Index::open(file) {
        Ok(idx) => Ok(idx),
        Err(e) => {
            eprintln!("index unreadable, rebuilding it from scratch: {e}");
            for suffix in ["", "-wal", "-shm"] {
                let mut stale = file.as_os_str().to_os_string();
                stale.push(suffix);
                let _ = std::fs::remove_file(PathBuf::from(stale));
            }
            Index::open(file).map_err(|e| e.to_string())
        }
    }
}

/// Builds (but does not start) the session for `root`: scope, index file, corpus.
pub fn build_session(root: &Path, initial: Option<String>, data_dir: &Path, no_index: bool, settings: &Settings, generation: u64) -> Result<Arc<Session>, String> {
    let scope = Scope::new(root).map_err(|e| format!("{}: {e}", root.display()))?;
    let index = if no_index {
        None
    } else {
        let file = data_dir.join(format!("index-{}.sqlite", root_hash(scope.root())));
        Some(Arc::new(open_or_rebuild(&file)?))
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
        gate: Mutex::new(WatchQueue::default()),
    }))
}

/// Starts a background full scan unless the session is already loading or indexing, in which
/// case the request is remembered and restarts the scan when that one ends.
pub fn spawn_scan(s: Arc<Session>, tx: EventTx) -> bool {
    if s.index.is_none() {
        return false;
    }
    {
        let mut gate = s.gate.lock().unwrap();
        if matches!(s.status.state.load(Ordering::SeqCst), index::STATE_LOADING | index::STATE_INDEXING) {
            gate.rescan = true;
            return false;
        }
        // Requests that arrived before this claim are covered by the scan starting now.
        gate.rescan = false;
        s.status.state.store(index::STATE_INDEXING, Ordering::SeqCst);
    }
    let run = s.clone();
    match std::thread::Builder::new().name("indexer".into()).spawn(move || run_scan(run, tx)) {
        Ok(_) => true,
        Err(e) => {
            eprintln!("scan thread failed to start: {e}");
            s.status.state.store(index::STATE_IDLE, Ordering::SeqCst);
            false
        }
    }
}

/// One full scan, then the post-scan duties: apply the batches the watcher queued while it ran,
/// publish `READY`, report, and start another scan when one was requested meanwhile.
fn run_scan(s: Arc<Session>, tx: EventTx) {
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
    let restart = drain_watch_queue(&s, &tx);
    let _ = tx.send_blocking(CoreEvent::Index { generation });
    if restart && !s.cancel.load(Ordering::Relaxed) {
        spawn_scan(s.clone(), tx);
    }
}

/// Applies every batch the watcher queued while the scan ran, then publishes `READY`. The final
/// emptiness check and the `READY` store share one critical section, so a batch that arrives
/// after this returns is applied by the watcher directly instead of being queued behind a scan
/// that has already finished. Returns whether another scan was requested meanwhile.
fn drain_watch_queue(s: &Session, tx: &EventTx) -> bool {
    loop {
        let paths = {
            let mut gate = s.gate.lock().unwrap();
            if gate.paths.is_empty() {
                s.status.state.store(index::STATE_READY, Ordering::SeqCst);
                let restart = gate.rescan;
                gate.rescan = false;
                return restart;
            }
            std::mem::take(&mut gate.paths)
        };
        if let Some(dirs) = apply_batch(s, &paths) {
            let _ = tx.send_blocking(CoreEvent::Changed { generation: s.generation, dirs });
        }
    }
}

/// Applies one coalesced batch of changed paths (from the watcher) to the index and corpus and
/// returns the root-relative dirs whose listings changed, when there are any.
fn apply_batch(s: &Session, paths: &[PathBuf]) -> Option<Vec<String>> {
    let idx = s.index.as_deref()?;
    let opts = s.scan_opts.read().unwrap().clone();
    match index::apply_paths(&s.scope, idx, &s.corpus, &opts, paths) {
        Ok(dirs) if !dirs.is_empty() => Some(dirs.into_iter().collect()),
        Ok(_) => None,
        Err(e) => {
            eprintln!("watch update failed: {e}");
            None
        }
    }
}

/// Starts the watcher and the index lifecycle (load the persisted index, then resync) for the
/// session that is now current. Never blocks the caller: the `LOADING` claim is the only
/// synchronous step, everything else runs on a background thread.
pub fn start_session(s: Arc<Session>, data_dir: &Path, tx: EventTx) {
    if s.index.is_none() {
        return;
    }
    // Claim before returning, so a scan request that lands before the thread below runs is
    // deferred to the post-load scan instead of racing the load.
    s.status.state.store(index::STATE_LOADING, Ordering::SeqCst);
    let data_canon = std::fs::canonicalize(data_dir).ok();
    std::thread::Builder::new()
        .name("index-load".into())
        .spawn(move || {
            let idx = s.index.clone().unwrap();
            // Installed here, off the UI thread: on Linux registering the recursive inotify
            // watches can block for a while on a large tree. The callback holds a weak
            // reference: the session owns the watcher, so dropping the session stops it.
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
                // Ordering against a running scan: while one (or the initial load) is in
                // flight the batch is queued and applied by `drain_watch_queue` afterwards,
                // so the scan's flush can never overwrite it. The state check and the enqueue
                // run under the scan's gate lock, so a batch can never slip past the scan's
                // final drain and its `READY` store.
                let dirs = {
                    let mut gate = s.gate.lock().unwrap();
                    if matches!(s.status.state.load(Ordering::SeqCst), index::STATE_LOADING | index::STATE_INDEXING) {
                        gate.paths.extend(batch.paths);
                        None
                    } else {
                        apply_batch(&s, &batch.paths)
                    }
                };
                if let Some(dirs) = dirs {
                    let _ = watch_tx.send_blocking(CoreEvent::Changed { generation: s.generation, dirs });
                }
            });
            match watched {
                Ok(h) => *s.watcher.lock().unwrap() = Some(h),
                Err(e) => {
                    // Typically the inotify watch limit on Linux; browsing still works, live
                    // updates do not.
                    eprintln!("watcher unavailable: {e}");
                    *s.watch_error.lock().unwrap() = Some(e.to_string());
                }
            }
            {
                let mut c = s.corpus.write().unwrap();
                if let Err(e) = idx.load_all(|id, path, flags| c.upsert(id, &path, flags)) {
                    eprintln!("index load failed: {e}");
                }
            }
            // Only leave `LOADING` if nothing moved on meanwhile (a scan claim would have).
            let _ = s.status.state.compare_exchange(index::STATE_LOADING, index::STATE_IDLE, Ordering::SeqCst, Ordering::SeqCst);
            if !s.cancel.load(Ordering::Relaxed) {
                let _ = tx.send_blocking(CoreEvent::Index { generation: s.generation });
                spawn_scan(s, tx);
            }
        })
        .expect("spawn index loader");
}

/// Empties the index and rebuilds it from scratch (waits for the initial load or a running scan
/// to stop first). Blocking: run it off the UI thread.
pub fn clear_index(s: Arc<Session>, tx: EventTx) -> Result<bool, String> {
    let Some(idx) = s.index.clone() else { return Ok(false) };
    s.cancel.store(true, Ordering::SeqCst);
    for _ in 0..600 {
        let st = s.status.state.load(Ordering::SeqCst);
        if st == index::STATE_IDLE || st == index::STATE_READY {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    // Reset `cancel` before propagating a failure: leaving it set would silently disable
    // every later scan.
    let cleared = idx.clear();
    s.cancel.store(false, Ordering::SeqCst);
    cleared.map_err(|e| e.to_string())?;
    *s.corpus.write().unwrap() = Corpus::default();
    s.status.indexed.store(0, Ordering::Relaxed);
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::Location;

    #[test]
    fn open_or_rebuild_discards_a_corrupt_index() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("index.sqlite");
        std::fs::write(&file, b"this is definitely not a sqlite database").unwrap();
        let idx = open_or_rebuild(&file).unwrap();
        assert_eq!(idx.count(), 0, "a fresh index is created once the corrupt one is discarded");
    }

    #[test]
    fn open_or_rebuild_keeps_a_healthy_index() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("index.sqlite");
        {
            let idx = open_or_rebuild(&file).unwrap();
            let rec = index::Record::new("a.txt".into(), &std::fs::metadata(&file).unwrap(), false);
            idx.upsert(&[rec]).unwrap();
            assert_eq!(idx.count(), 1);
        }
        let idx = open_or_rebuild(&file).unwrap();
        assert_eq!(idx.count(), 1, "a readable index is not touched");
    }

    #[test]
    fn starting_point_prefers_the_explicit_root() {
        let real = tempfile::tempdir().unwrap();
        // Even with a live last location, `--root` wins and is not remembered.
        let settings = Settings {
            last: Some(Location { root: real.path().to_string_lossy().into_owned(), select: Some("sub".into()) }),
            ..Settings::default()
        };
        let (root, sel, persist) = starting_point(Some(PathBuf::from("/explicit")), Some("here".into()), &settings, Some(PathBuf::from("/home/u")));
        assert_eq!(root, PathBuf::from("/explicit"));
        assert_eq!(sel.as_deref(), Some("here"));
        assert!(!persist);
    }

    #[test]
    fn starting_point_restores_a_live_last_location() {
        let real = tempfile::tempdir().unwrap();
        let settings = Settings {
            last: Some(Location { root: real.path().to_string_lossy().into_owned(), select: Some("sub".into()) }),
            ..Settings::default()
        };
        let (root, sel, persist) = starting_point(None, None, &settings, Some(PathBuf::from("/home/u")));
        assert_eq!(root, PathBuf::from(real.path().to_string_lossy().into_owned()));
        assert_eq!(sel.as_deref(), Some("sub"));
        assert!(persist);
        // An explicit selection beats the remembered one.
        let (_, sel, _) = starting_point(None, Some("here".into()), &settings, None);
        assert_eq!(sel.as_deref(), Some("here"));
    }

    #[test]
    fn starting_point_falls_back_to_home_when_last_is_stale_or_disabled() {
        let home = tempfile::tempdir().unwrap();
        for restore in [true, false] {
            let settings = Settings {
                restore_last: restore,
                last: Some(Location { root: "/definitely/missing".into(), select: Some("x".into()) }),
                ..Settings::default()
            };
            let (root, sel, persist) = starting_point(None, None, &settings, Some(home.path().to_path_buf()));
            assert_eq!(root, home.path().parent().unwrap().to_path_buf());
            assert_eq!(sel.as_deref(), home.path().file_name().unwrap().to_str());
            assert!(persist);
        }
        // No home at all: the current directory.
        let (root, _, persist) = starting_point(None, None, &Settings::default(), None);
        assert_eq!(root, PathBuf::from("."));
        assert!(persist);
    }

    #[test]
    fn reveal_chain_lists_every_ancestor_of_a_path() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("a/b")).unwrap();
        std::fs::write(dir.path().join("a/b/c.txt"), "x").unwrap();
        let scope = Scope::new(dir.path()).unwrap();
        let opts = ListOptions::default();

        // A file: its ancestors are listed, the file itself is not.
        let chain = reveal_chain(&scope, "a/b/c.txt", &opts).unwrap();
        let paths: Vec<&str> = chain.iter().map(|l| l.path.as_str()).collect();
        assert_eq!(paths, ["", "a", "a/b"]);
        assert!(chain[0].entries.iter().any(|e| e.name() == "a"));
        assert!(chain[2].entries.iter().any(|e| e.name() == "c.txt"));
        // A folder is listed as well, so its contents can be shown.
        let paths: Vec<String> = reveal_chain(&scope, "a/b", &opts).unwrap().into_iter().map(|l| l.path).collect();
        assert_eq!(paths, ["", "a", "a/b"]);
        assert!(reveal_chain(&scope, "a/missing", &opts).is_err());
    }

    #[test]
    fn list_many_skips_folders_that_fail() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("a")).unwrap();
        std::fs::create_dir_all(dir.path().join("b")).unwrap();
        let scope = Scope::new(dir.path()).unwrap();
        let opts = ListOptions::default();
        let out = list_many(&scope, &["a".into(), "missing".into(), "b".into()], &opts);
        let paths: Vec<&str> = out.iter().map(|l| l.path.as_str()).collect();
        assert_eq!(paths, ["a", "b"]);
        // Single listings reject traversal instead of reaching the disk.
        assert!(list(&scope, "a/../../x", &opts).is_err());
    }
}
