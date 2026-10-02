//! Backend benchmark: `cargo run --release --example bench -- --root fixtures/bench-100k`
//! Measures listing, index build, index load (startup), search latency, incremental updates
//! and process memory. Prints a markdown table; numbers are whatever this machine produces.

use lupasta::filesystem::{self, Scope};
use lupasta::index::{self, Index, ScanOptions};
use lupasta::search::{self, Corpus};
use lupasta::watcher;
use std::path::PathBuf;
use std::sync::{mpsc, RwLock};
use std::time::{Duration, Instant};

#[cfg(windows)]
fn working_set_mb() -> f64 {
    use windows_sys::Win32::System::ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS};
    use windows_sys::Win32::System::Threading::GetCurrentProcess;
    unsafe {
        let mut c: PROCESS_MEMORY_COUNTERS = std::mem::zeroed();
        c.cb = std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32;
        GetProcessMemoryInfo(GetCurrentProcess(), &mut c, c.cb);
        c.WorkingSetSize as f64 / 1048576.0
    }
}

#[cfg(not(windows))]
fn working_set_mb() -> f64 {
    std::fs::read_to_string("/proc/self/statm")
        .ok()
        .and_then(|s| s.split_whitespace().nth(1).and_then(|v| v.parse::<f64>().ok()))
        .map(|pages| pages * 4096.0 / 1048576.0)
        .unwrap_or(0.0)
}

fn ms(d: Duration) -> String {
    format!("{:.2} ms", d.as_secs_f64() * 1000.0)
}

fn percentile(v: &mut [Duration], p: f64) -> Duration {
    v.sort();
    v[((v.len() as f64 - 1.0) * p).round() as usize]
}

fn main() {
    let mut root = None;
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        if a == "--root" {
            root = it.next().map(PathBuf::from);
        }
    }
    let root = root.expect("--root <dir>");
    let scope = Scope::new(&root).expect("root");
    let data = std::env::temp_dir().join(format!("lupasta-bench-{}", std::process::id()));
    std::fs::create_dir_all(&data).unwrap();
    let db = data.join("index.sqlite");
    let mut rows: Vec<(String, String)> = Vec::new();
    let mem0 = working_set_mb();

    // Directory listing (what an expansion costs on the Rust side).
    let t = Instant::now();
    let top = filesystem::list_dir(scope.root()).unwrap();
    rows.push(("list_dir(root) cold".into(), format!("{} ({} entries)", ms(t.elapsed()), top.len())));
    let first_dir = top.iter().find(|e| e.is_dir()).map(|e| scope.root().join(e.name())).unwrap();
    let t = Instant::now();
    let l = filesystem::list_dir(&first_dir).unwrap();
    rows.push(("list_dir(first child dir)".into(), format!("{} ({} entries)", ms(t.elapsed()), l.len())));
    let mut samples: Vec<Duration> = (0..50)
        .map(|_| {
            let t = Instant::now();
            filesystem::list_dir(&first_dir).unwrap();
            t.elapsed()
        })
        .collect();
    rows.push(("list_dir warm p50".into(), ms(percentile(&mut samples, 0.5))));

    // Walk alone (the floor for any scan).
    let t = Instant::now();
    let walked = std::sync::atomic::AtomicU64::new(0);
    ignore::WalkBuilder::new(scope.root()).hidden(false).ignore(false).git_ignore(false).git_exclude(false).git_global(false).build_parallel().run(|| {
        let walked = &walked;
        Box::new(move |e| {
            if let Ok(e) = e {
                let _ = e.metadata();
                walked.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            }
            ignore::WalkState::Continue
        })
    });
    rows.push(("walk only (parallel, with metadata)".into(), format!("{} ({} entries)", ms(t.elapsed()), walked.into_inner())));

    // Full index build from scratch.
    let opts = ScanOptions { respect_gitignore: false, ..ScanOptions::default() };
    let index = Index::open(&db).unwrap();
    let corpus = RwLock::new(Corpus::default());
    let stats = index::scan(&scope, scope.root(), &index, &corpus, &opts, true, &|_| {}).unwrap();
    let db_mb = std::fs::metadata(&db).map(|m| m.len()).unwrap_or(0) as f64 / 1048576.0;
    rows.push(("index build (walk + SQLite + FTS5)".into(), format!("{} ms, {} entries, db {:.1} MB", stats.elapsed_ms, stats.entries, db_mb)));
    let t = Instant::now();
    let restats = index::scan(&scope, scope.root(), &index, &corpus, &opts, true, &|_| {}).unwrap();
    rows.push(("resync scan (nothing changed)".into(), format!("{} ({} entries)", ms(t.elapsed()), restats.entries)));
    let mem_built = working_set_mb();
    drop(corpus);
    drop(index);

    // Startup path: open existing index and load the in-memory corpus.
    let t = Instant::now();
    let index = Index::open(&db).unwrap();
    let corpus = RwLock::new(Corpus::default());
    {
        let mut c = corpus.write().unwrap();
        index.load_all(|id, p, f| c.upsert(id, &p, f)).unwrap();
    }
    rows.push(("startup: open index + load corpus".into(), format!("{} ({} items)", ms(t.elapsed()), corpus.read().unwrap().len())));
    let mem_loaded = working_set_mb();

    // Search latency.
    for q in ["router", "rtr", "file browser", "readme", "a", "zzqxj", "mordor roster"] {
        let first = search::search(&corpus, Some(&index), q, 40);
        let mut s: Vec<Duration> = (0..30)
            .map(|_| {
                let t = Instant::now();
                search::search(&corpus, Some(&index), q, 40);
                t.elapsed()
            })
            .collect();
        rows.push((
            format!("search \"{q}\" warm p50 / p95"),
            format!("{} / {} ({}, {} hits, {} candidates; top: {})", ms(percentile(&mut s, 0.5)), ms(percentile(&mut s, 0.95)), first.strategy, first.hits.len(), first.candidates, first.hits.iter().take(3).map(|h| h.name.as_str()).collect::<Vec<_>>().join(", ")),
        ));
    }

    // Incremental update through the same code path the watcher uses.
    let probe = first_dir.join("bench-probe-file.txt");
    std::fs::write(&probe, "x").unwrap();
    let t = Instant::now();
    index::apply_paths(&scope, &index, &corpus, &opts, std::slice::from_ref(&probe)).unwrap();
    rows.push(("incremental: create -> index+corpus".into(), ms(t.elapsed())));
    std::fs::remove_file(&probe).unwrap();
    let t = Instant::now();
    index::apply_paths(&scope, &index, &corpus, &opts, std::slice::from_ref(&probe)).unwrap();
    rows.push(("incremental: delete -> index+corpus".into(), ms(t.elapsed())));

    // End-to-end watcher latency (includes the 120 ms quiet-window debounce).
    let (tx, rx) = mpsc::channel();
    let canon = std::fs::canonicalize(&root).unwrap();
    let h = watcher::watch(&canon, None, move |b| {
        let _ = tx.send((Instant::now(), b));
    })
    .unwrap();
    std::thread::sleep(Duration::from_millis(200));
    let t = Instant::now();
    std::fs::write(&probe, "x").unwrap();
    if let Ok((at, _)) = rx.recv_timeout(Duration::from_secs(5)) {
        rows.push(("watcher: write -> coalesced batch".into(), ms(at - t)));
    }
    std::fs::remove_file(&probe).unwrap();
    drop(h);

    rows.push(("memory: baseline / after build / after reload".into(), format!("{mem0:.1} / {mem_built:.1} / {mem_loaded:.1} MB")));
    println!("| metric | result |\n|---|---|");
    for (k, v) in rows {
        println!("| {k} | {v} |");
    }
    let _ = std::fs::remove_dir_all(&data);
}
