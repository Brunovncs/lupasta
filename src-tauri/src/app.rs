use crate::commands::{self, AppState};
use crate::index::{self, Index, ScanOptions, Status};
use crate::search::Corpus;
use crate::{filesystem, watcher};
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::{Arc, RwLock};
use tauri::{AppHandle, Emitter, Manager};

struct Args {
    root: Option<PathBuf>,
    select: Option<String>,
    data_dir: Option<PathBuf>,
    no_index: bool,
    no_gitignore: bool,
    capture: bool,
}

fn parse_args() -> Args {
    let mut args = Args { root: None, select: None, data_dir: None, no_index: false, no_gitignore: false, capture: false };
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        match a.as_str() {
            "--root" => args.root = it.next().map(PathBuf::from),
            "--select" => args.select = it.next(),
            "--data-dir" => args.data_dir = it.next().map(PathBuf::from),
            "--no-index" => args.no_index = true,
            "--no-gitignore" => args.no_gitignore = true,
            "--capture" => args.capture = true,
            _ => {}
        }
    }
    args
}

/// Starts a background full scan unless one is already running.
pub fn spawn_scan(app: AppHandle, state: &AppState) -> bool {
    let Some(idx) = state.index.clone() else { return false };
    let status = state.status.clone();
    if status.state.swap(index::STATE_INDEXING, Ordering::SeqCst) == index::STATE_INDEXING {
        return false;
    }
    let scope = state.scope.clone();
    let corpus = state.corpus.clone();
    let opts = state.scan_opts.clone();
    std::thread::Builder::new()
        .name("indexer".into())
        .spawn(move || {
            let emit_progress = |n: u64| {
                status.indexed.store(n, Ordering::Relaxed);
                if n % 16384 < 4096 {
                    let _ = app.emit("index_progress", commands::status_of(&status, &corpus));
                }
            };
            match index::scan(&scope, scope.root(), &idx, &corpus, &opts, true, &emit_progress) {
                Ok(stats) => {
                    status.indexed.store(stats.entries, Ordering::Relaxed);
                    status.last_scan_ms.store(stats.elapsed_ms, Ordering::Relaxed);
                }
                Err(e) => eprintln!("scan failed: {e}"),
            }
            status.state.store(index::STATE_READY, Ordering::SeqCst);
            let _ = app.emit("index_ready", commands::status_of(&status, &corpus));
        })
        .is_ok()
}

fn root_hash(root: &std::path::Path) -> String {
    format!("{:016x}", index::path_id(&root.to_string_lossy()))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let args = parse_args();
    tauri::Builder::default()
        .setup(move |app| {
            let home = app.path().home_dir().ok();
            // Default: the folder that contains the home directory, with the home selected —
            // the same starting point as the reference (/Users → drcode).
            let (root, initial) = match (&args.root, &home) {
                (Some(r), _) => (r.clone(), args.select.clone()),
                (None, Some(h)) => {
                    let parent = h.parent().map(|p| p.to_path_buf()).unwrap_or_else(|| h.clone());
                    let sel = h.file_name().map(|n| n.to_string_lossy().into_owned());
                    (parent, args.select.clone().or(sel))
                }
                (None, None) => (PathBuf::from("."), args.select.clone()),
            };
            if args.capture {
                // Visual-regression mode: exactly 1812×1344 device px at an effective DPR of 2
                // (a 906×672 CSS viewport), whatever the monitor scaling is.
                if let Some(win) = app.get_webview_window("main") {
                    let scale = win.scale_factor().unwrap_or(1.0);
                    win.set_decorations(false)?;
                    win.set_size(tauri::PhysicalSize::new(1812u32, 1344u32))?;
                    win.set_position(tauri::PhysicalPosition::new(0i32, 0i32))?;
                    win.set_zoom(2.0 / scale)?;
                }
            }
            let scope = filesystem::Scope::new(&root)?;
            let data_dir = match &args.data_dir {
                Some(d) => d.clone(),
                None => app.path().app_data_dir()?,
            };
            std::fs::create_dir_all(&data_dir)?;
            let index = if args.no_index {
                None
            } else {
                Some(Arc::new(Index::open(&data_dir.join(format!("index-{}.sqlite", root_hash(scope.root()))))?))
            };
            let corpus = Arc::new(RwLock::new(Corpus::default()));
            let status = Arc::new(Status::default());
            let scan_opts = ScanOptions { respect_gitignore: !args.no_gitignore, ..ScanOptions::default() };

            // Watcher: index + corpus update, then tell the renderer which listings changed.
            if let Some(idx) = index.clone() {
                let handle = app.handle().clone();
                let (scope_w, corpus_w, opts_w) = (scope.clone(), corpus.clone(), scan_opts.clone());
                let data_canon = std::fs::canonicalize(&data_dir).ok();
                match watcher::watch(scope.root(), data_canon, move |batch| {
                    if batch.rescan {
                        let state = handle.state::<AppState>();
                        spawn_scan(handle.clone(), &state);
                    }
                    match index::apply_paths(&scope_w, &idx, &corpus_w, &opts_w, &batch.paths) {
                        Ok(dirs) if !dirs.is_empty() => commands::emit_changed(&handle, dirs.into_iter().collect()),
                        Ok(_) => {}
                        Err(e) => eprintln!("watch update failed: {e}"),
                    }
                }) {
                    Ok(h) => {
                        app.manage(h);
                    }
                    Err(e) => eprintln!("watcher unavailable: {e}"),
                }
            }

            app.manage(AppState { scope, index: index.clone(), corpus: corpus.clone(), status: status.clone(), scan_opts, initial, data_dir });

            // Index lifecycle, never blocking the window: load the persisted index, then resync.
            if let Some(idx) = index {
                let handle = app.handle().clone();
                std::thread::spawn(move || {
                    status.state.store(index::STATE_LOADING, Ordering::SeqCst);
                    {
                        let mut c = corpus.write().unwrap();
                        let _ = idx.load_all(|id, path, flags| c.upsert(id, &path, flags));
                    }
                    status.state.store(index::STATE_IDLE, Ordering::SeqCst);
                    let _ = handle.emit("index_progress", commands::status_of(&status, &corpus));
                    let state = handle.state::<AppState>();
                    spawn_scan(handle.clone(), &state);
                });
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_root,
            commands::list_directory,
            commands::list_directories,
            commands::get_children,
            commands::expand_directory,
            commands::get_file_metadata,
            commands::search_files,
            commands::index_status,
            commands::start_indexing,
            commands::reveal_path,
            commands::open_path,
        ])
        .run(tauri::generate_context!())
        .expect("error while running lupasta");
}
