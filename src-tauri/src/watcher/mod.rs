//! Recursive filesystem watcher with burst coalescing.
//!
//! Raw notify events are collected until the stream has been quiet for `QUIET` (or `MAX_WAIT`
//! has passed since the first event), then delivered as one deduplicated batch of paths.

use notify::{Event, EventKind, RecursiveMode, Watcher};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, Instant};

pub const QUIET: Duration = Duration::from_millis(120);
pub const MAX_WAIT: Duration = Duration::from_millis(800);

#[derive(Debug, Default, PartialEq)]
pub struct Batch {
    pub paths: Vec<PathBuf>,
    /// The OS dropped events (buffer overflow): the caller should rescan.
    pub rescan: bool,
}

/// Folds raw events into a batch. Pure, so it is unit-testable without touching the disk.
pub fn coalesce(events: impl IntoIterator<Item = Event>, ignore_prefix: Option<&Path>) -> Batch {
    let mut set = BTreeSet::new();
    let mut rescan = false;
    for ev in events {
        if ev.need_rescan() {
            rescan = true;
        }
        if matches!(ev.kind, EventKind::Access(_)) {
            continue;
        }
        for p in ev.paths {
            if ignore_prefix.is_some_and(|pre| p.starts_with(pre)) {
                continue;
            }
            set.insert(p);
        }
    }
    Batch { paths: set.into_iter().collect(), rescan }
}

pub struct WatchHandle {
    _watcher: notify::RecommendedWatcher,
}

/// Starts watching `root` recursively. `on_batch` runs on a dedicated thread.
pub fn watch(root: &Path, ignore_prefix: Option<PathBuf>, on_batch: impl Fn(Batch) + Send + 'static) -> notify::Result<WatchHandle> {
    let (tx, rx) = mpsc::channel::<notify::Result<Event>>();
    let mut watcher = notify::recommended_watcher(tx)?;
    watcher.watch(root, RecursiveMode::Recursive)?;
    std::thread::Builder::new()
        .name("fs-coalescer".into())
        .spawn(move || {
            while let Ok(first) = rx.recv() {
                let started = Instant::now();
                let mut events = Vec::new();
                events.extend(first.ok());
                loop {
                    let left = MAX_WAIT.saturating_sub(started.elapsed());
                    if left.is_zero() {
                        break;
                    }
                    match rx.recv_timeout(QUIET.min(left)) {
                        Ok(ev) => events.extend(ev.ok()),
                        Err(mpsc::RecvTimeoutError::Timeout) => break,
                        Err(mpsc::RecvTimeoutError::Disconnected) => return,
                    }
                }
                let batch = coalesce(events, ignore_prefix.as_deref());
                if !batch.paths.is_empty() || batch.rescan {
                    on_batch(batch);
                }
            }
        })
        .expect("spawn coalescer");
    Ok(WatchHandle { _watcher: watcher })
}

#[cfg(test)]
mod tests {
    use super::*;
    use notify::event::{CreateKind, ModifyKind, RemoveKind, RenameMode};

    fn ev(kind: EventKind, paths: &[&str]) -> Event {
        let mut e = Event::new(kind);
        for p in paths {
            e = e.add_path(PathBuf::from(p));
        }
        e
    }

    #[test]
    fn coalesces_and_dedups_bursts() {
        let events = vec![
            ev(EventKind::Create(CreateKind::File), &["/r/a.txt"]),
            ev(EventKind::Modify(ModifyKind::Any), &["/r/a.txt"]),
            ev(EventKind::Modify(ModifyKind::Any), &["/r/a.txt"]),
            ev(EventKind::Modify(ModifyKind::Name(RenameMode::Both)), &["/r/b.txt", "/r/c.txt"]),
            ev(EventKind::Remove(RemoveKind::File), &["/r/d.txt"]),
            ev(EventKind::Access(notify::event::AccessKind::Any), &["/r/e.txt"]),
            ev(EventKind::Create(CreateKind::File), &["/data/index.sqlite-wal"]),
        ];
        let b = coalesce(events, Some(Path::new("/data")));
        let names: Vec<_> = b.paths.iter().map(|p| p.to_string_lossy().replace('\\', "/")).collect();
        assert_eq!(names, ["/r/a.txt", "/r/b.txt", "/r/c.txt", "/r/d.txt"]);
        assert!(!b.rescan);
    }

    #[test]
    fn flags_rescan() {
        let e = Event::new(EventKind::Other).set_flag(notify::event::Flag::Rescan);
        assert!(coalesce(vec![e], None).rescan);
    }

    #[test]
    fn real_watcher_delivers_one_batch_for_a_burst() {
        let dir = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(dir.path()).unwrap();
        let (tx, rx) = mpsc::channel();
        let _h = watch(&root, None, move |b| {
            let _ = tx.send(b);
        })
        .unwrap();
        std::thread::sleep(Duration::from_millis(100));
        for i in 0..20 {
            std::fs::write(root.join(format!("f{i}.txt")), "x").unwrap();
        }
        let batch = rx.recv_timeout(Duration::from_secs(5)).expect("batch");
        let mut seen: BTreeSet<PathBuf> = batch.paths.into_iter().collect();
        while let Ok(more) = rx.recv_timeout(Duration::from_millis(400)) {
            seen.extend(more.paths);
        }
        assert!((0..20).all(|i| seen.contains(&root.join(format!("f{i}.txt")))), "{seen:?}");
    }
}
