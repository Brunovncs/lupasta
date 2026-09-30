use crate::commands::{self, AppState, Session};
use crate::index::{self, Index, ScanOptions, Status};
use crate::search::Corpus;
use crate::settings::Settings;
use crate::{filesystem, watcher};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};

struct Args {
    root: Option<PathBuf>,
    select: Option<String>,
    data_dir: Option<PathBuf>,
    no_index: bool,
    no_gitignore: bool,
    capture: bool,
    smoke: bool,
}

fn parse_args() -> Args {
    let mut args = Args { root: None, select: None, data_dir: None, no_index: false, no_gitignore: false, capture: false, smoke: false };
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        match a.as_str() {
            "--root" => args.root = it.next().map(PathBuf::from),
            "--select" => args.select = it.next(),
            "--data-dir" => args.data_dir = it.next().map(PathBuf::from),
            "--no-index" => args.no_index = true,
            "--no-gitignore" => args.no_gitignore = true,
            "--capture" => args.capture = true,
            "--smoke" => args.smoke = true,
            _ => {}
        }
    }
    args
}

/// How long `--smoke` waits for the renderer before failing.
const SMOKE_TIMEOUT: Duration = Duration::from_secs(90);

pub fn smoke_log(data_dir: &Path, line: &str) {
    println!("{line}");
    let _ = std::fs::write(data_dir.join("smoke.log"), format!("{line}\n"));
}

/// Starts a background full scan unless one is already running.
pub fn spawn_scan(app: AppHandle, s: Arc<Session>) -> bool {
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
            let current = || app.state::<AppState>().is_current(s.generation);
            let emit_progress = |n: u64| {
                s.status.indexed.store(n, Ordering::Relaxed);
                if n % 16384 < 4096 && current() {
                    let _ = app.emit("index_progress", commands::status_of(&s));
                }
            };
            match index::scan(&s.scope, s.scope.root(), &idx, &s.corpus, &opts, true, &emit_progress) {
                Ok(stats) => {
                    s.status.indexed.store(stats.entries, Ordering::Relaxed);
                    s.status.last_scan_ms.store(stats.elapsed_ms, Ordering::Relaxed);
                }
                Err(e) => eprintln!("scan failed: {e}"),
            }
            s.status.state.store(index::STATE_READY, Ordering::SeqCst);
            if current() {
                let _ = app.emit("index_ready", commands::status_of(&s));
            }
        })
        .is_ok()
}

fn root_hash(root: &Path) -> String {
    format!("{:016x}", index::path_id(&root.to_string_lossy()))
}

/// Builds (but does not start) the session for `root`: scope, index file, corpus.
pub fn open_session(app: &AppHandle, root: &Path, initial: Option<String>) -> Result<Arc<Session>, String> {
    let state = app.state::<AppState>();
    let settings = state.settings.read().unwrap().clone();
    build_session(root, initial, &state.data_dir, state.no_index, &settings, state.next_generation())
}

fn build_session(root: &Path, initial: Option<String>, data_dir: &Path, no_index: bool, settings: &Settings, generation: u64) -> Result<Arc<Session>, String> {
    let scope = filesystem::Scope::new(root).map_err(|e| format!("{}: {e}", root.display()))?;
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

/// Starts the watcher and the index lifecycle (load the persisted index, then resync) for the
/// session that is now current. Never blocks the window.
pub fn start_session(app: &AppHandle, s: Arc<Session>) {
    let Some(_) = s.index.clone() else { return };
    let data_canon = std::fs::canonicalize(&app.state::<AppState>().data_dir).ok();
    let handle = app.clone();
    let generation = s.generation;
    // The callback looks the session up instead of capturing it, so dropping the session
    // (which owns the watcher) is enough to stop everything.
    let watched = watcher::watch(s.scope.root(), data_canon, move |batch| {
        let state = handle.state::<AppState>();
        if !state.is_current(generation) {
            return;
        }
        let s = state.session();
        if batch.rescan {
            spawn_scan(handle.clone(), s.clone());
        }
        let Some(idx) = s.index.as_deref() else { return };
        let opts = s.scan_opts.read().unwrap().clone();
        match index::apply_paths(&s.scope, idx, &s.corpus, &opts, &batch.paths) {
            Ok(dirs) if !dirs.is_empty() => commands::emit_changed(&handle, dirs.into_iter().collect()),
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

    let handle = app.clone();
    std::thread::spawn(move || {
        let idx = s.index.clone().unwrap();
        s.status.state.store(index::STATE_LOADING, Ordering::SeqCst);
        {
            let mut c = s.corpus.write().unwrap();
            let _ = idx.load_all(|id, path, flags| c.upsert(id, &path, flags));
        }
        s.status.state.store(index::STATE_IDLE, Ordering::SeqCst);
        if handle.state::<AppState>().is_current(s.generation) {
            let _ = handle.emit("index_progress", commands::status_of(&s));
            spawn_scan(handle.clone(), s);
        }
    });
}

/// Picks the starting root and selection: `--root` wins, then the last location (when
/// `restore_last` is on and it still exists), then the folder that contains the home directory
/// with the home selected — the same starting point as the reference (/Users → drcode).
fn starting_point(args: &Args, settings: &Settings, home: Option<PathBuf>) -> (PathBuf, Option<String>, bool) {
    if let Some(r) = &args.root {
        return (r.clone(), args.select.clone(), false);
    }
    if settings.restore_last {
        if let Some(last) = settings.last.as_ref().filter(|l| Path::new(&l.root).is_dir()) {
            return (PathBuf::from(&last.root), args.select.clone().or(last.select.clone()), true);
        }
    }
    match home {
        Some(h) => {
            let parent = h.parent().map(|p| p.to_path_buf()).unwrap_or_else(|| h.clone());
            let sel = h.file_name().map(|n| n.to_string_lossy().into_owned());
            (parent, args.select.clone().or(sel), true)
        }
        None => (PathBuf::from("."), args.select.clone(), true),
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let args = parse_args();
    let mut builder = tauri::Builder::default().plugin(tauri_plugin_dialog::init()).plugin(tauri_plugin_clipboard_manager::init());
    // Visual regression and smoke runs need a deterministic window, not the one saved last time.
    if !args.capture && !args.smoke {
        builder = builder.plugin(tauri_plugin_window_state::Builder::default().build());
    }
    builder
        .setup(move |app| {
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
            let data_dir = match &args.data_dir {
                Some(d) => d.clone(),
                None => app.path().app_data_dir()?,
            };
            std::fs::create_dir_all(&data_dir)?;
            let mut settings = Settings::load(&data_dir);
            if args.no_gitignore {
                settings.respect_gitignore = false;
            }
            let (root, initial, persist) = starting_point(&args, &settings, app.path().home_dir().ok());
            let session = build_session(&root, initial, &data_dir, args.no_index, &settings, 1)?;
            app.manage(AppState::new(session.clone(), settings, data_dir.clone(), args.no_index, args.capture, args.smoke, persist));
            start_session(app.handle(), session);

            if args.smoke {
                let handle = app.handle().clone();
                std::thread::spawn(move || {
                    std::thread::sleep(SMOKE_TIMEOUT);
                    smoke_log(&data_dir, "smoke FAILED: timed out waiting for the renderer");
                    handle.exit(2);
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
            commands::clear_index,
            commands::reveal_path,
            commands::open_path,
            commands::reveal_in_os,
            commands::abs_path,
            commands::get_settings,
            commands::set_settings,
            commands::remember_selection,
            commands::set_root,
            commands::app_info,
            commands::open_data_dir,
            commands::check_update,
            commands::open_release,
            commands::smoke_report,
        ])
        .run(tauri::generate_context!())
        .expect("error while running lupasta");
}
