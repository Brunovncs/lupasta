//! The window: owns the session, the controller and the animator, turns input into navigation and
//! runs every listing, search and index call off the UI thread.

use super::chrome::{self, Bar, Hits, MAX_HITS, PanelView, SearchView, Target};
use super::frame::{self, Frame, Style};
use super::scene::{SceneView, camera_for};
use super::text::{self, FontFit};
use gpui::{
    Bounds, ClipboardItem, Context, ElementInputHandler, EntityInputHandler, FocusHandle, InteractiveElement, IntoElement, KeyDownEvent,
    MouseButton, MouseDownEvent, MouseMoveEvent, ParentElement, PathPromptOptions, Pixels, Render, ScrollDelta, ScrollWheelEvent, Styled,
    Task, UTF16Selection, Window, canvas, div,
};
use lupasta::animator::{Animator, ease_out_cubic};
use lupasta::controller::{Controller, Need};
use lupasta::filesystem::{self, ListOptions};
use lupasta::layout::Layout;
use lupasta::line_edit::LineEdit;
use lupasta::metrics::Metrics;
use lupasta::palette::{ColorMode, Colors, DAY_MS};
use lupasta::prefs::{self, Action, Context as RowContext, Row, UpdateInfo, UpdateState};
use lupasta::search::{self, SearchHit};
use lupasta::session::{self, CoreEvent, EventTx, IndexStatus, RootInfo, Session};
use lupasta::settings::{Location, Settings};
use lupasta::tree::Listing;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub const REPO: &str = "Brunovncs/lupasta";
const BAR_EASE_MS: f64 = 160.0;
/// The bar appears when the pointer reaches the top edge and leaves once it is well below it.
const BAR_REVEAL_Y: f32 = 28.0;
const BAR_HIDE_Y: f32 = 44.0;
const BAR_HIDE_DELAY: Duration = Duration::from_millis(350);
const NOTICE_TIME: Duration = Duration::from_millis(2200);
const REMEMBER_DELAY: Duration = Duration::from_millis(800);
/// Pixels per wheel line, so a notch scrolls like a browser (3 lines ≈ 100 px).
const LINE_PX: f32 = 100.0 / 3.0;
/// The scene steps back while search or settings is open, so they read on top of it.
const RECEDED: f32 = 0.25;

pub struct Args {
    pub root: Option<PathBuf>,
    pub select: Option<String>,
    pub data_dir: Option<PathBuf>,
    pub no_index: bool,
    pub no_gitignore: bool,
    pub smoke: bool,
}

struct SearchState {
    edit: LineEdit,
    hits: Vec<SearchHit>,
    active: usize,
    seq: u64,
    /// The character that opened the prompt, still on its way as text input.
    swallow: Option<String>,
}

struct PanelState {
    active: usize,
    editing: Option<LineEdit>,
    scroll: f32,
}

pub struct Lupasta {
    focus: FocusHandle,
    data_dir: PathBuf,
    no_index: bool,
    smoke: bool,
    /// Off while the root came from `--root` (tests, fixtures), so it is not remembered.
    persist_location: bool,
    settings: Settings,
    session: Arc<Session>,
    next_generation: u64,
    tx: EventTx,
    root: RootInfo,
    ctl: Controller,
    anim: Animator,
    /// The layout being left, whose connectors fade out during the transition.
    prev_layout: Option<Layout>,
    m: Metrics,
    colors: Colors,
    fit: FontFit,
    zoom: f32,
    clock: Instant,
    /// Window size in window px.
    viewport: (f32, f32),
    dpr: f32,
    search: Option<SearchState>,
    panel: Option<PanelState>,
    bar_hover: bool,
    bar_from: f32,
    bar_since: f64,
    bar_hide: Option<Task<()>>,
    notice: Option<String>,
    notice_task: Option<Task<()>>,
    error: Option<String>,
    status: Option<IndexStatus>,
    update: UpdateState,
    mouse: (f32, f32),
    hits: Hits,
    remember: Option<Task<()>>,
    search_task: Option<Task<()>>,
}

fn wall_ms() -> f64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs_f64() * 1000.0).unwrap_or(0.0)
}

pub fn default_data_dir() -> PathBuf {
    dirs::data_dir().unwrap_or_else(|| PathBuf::from(".")).join("io.github.brunovncs.lupasta")
}

fn smoke_log(data_dir: &Path, line: &str) {
    println!("{line}");
    let _ = std::fs::write(data_dir.join("smoke.log"), format!("{line}\n"));
}

impl Lupasta {
    pub fn new(args: Args, window: &mut Window, cx: &mut Context<Self>) -> Result<Lupasta, String> {
        let data_dir = args.data_dir.clone().unwrap_or_else(default_data_dir);
        std::fs::create_dir_all(&data_dir).map_err(|e| format!("{}: {e}", data_dir.display()))?;
        let mut settings = Settings::load(&data_dir);
        if args.no_gitignore {
            settings.respect_gitignore = false;
        }
        let (root, initial, persist) = session::starting_point(args.root.clone(), args.select.clone(), &settings, dirs::home_dir());
        let session = session::build_session(&root, initial, &data_dir, args.no_index, &settings, 1)?;
        let (tx, rx) = async_channel::unbounded();
        session::start_session(session.clone(), &data_dir, tx.clone());
        cx.spawn(async move |this, cx| {
            while let Ok(ev) = rx.recv().await {
                if this.update(cx, |v, cx| v.on_core_event(ev, cx)).is_err() {
                    break;
                }
            }
        })
        .detach();

        let m = Metrics::default();
        let fit = text::fit(cx, &settings.font, m.char_w, m.row_h);
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        let vp = window.viewport_size();
        let mut app = Lupasta {
            focus,
            data_dir,
            no_index: args.no_index,
            smoke: args.smoke,
            persist_location: persist,
            root: session.root_info(),
            session,
            next_generation: 2,
            tx,
            ctl: Controller::new(),
            anim: Animator::new(m.duration_ms),
            prev_layout: None,
            colors: Colors::default(),
            fit,
            zoom: 1.0,
            m,
            clock: Instant::now(),
            viewport: (f32::from(vp.width), f32::from(vp.height)),
            dpr: window.scale_factor(),
            search: None,
            panel: None,
            bar_hover: false,
            bar_from: 0.0,
            bar_since: -BAR_EASE_MS,
            bar_hide: None,
            notice: None,
            notice_task: None,
            error: None,
            status: None,
            update: UpdateState::Unknown,
            mouse: (-1.0, -1.0),
            hits: Hits::default(),
            remember: None,
            search_task: None,
            settings,
        };
        app.apply(None, cx);
        app.load_root(true, cx);
        app.status = Some(app.session.status());
        if app.smoke {
            app.run_smoke(cx);
        }
        Ok(app)
    }

    fn now(&self) -> f64 {
        self.clock.elapsed().as_secs_f64() * 1000.0
    }

    fn list_options(&self) -> ListOptions {
        self.settings.list_options()
    }

    /// Viewport in layout px.
    fn layout_viewport(&self) -> (f32, f32) {
        (self.viewport.0 / self.zoom, self.viewport.1 / self.zoom)
    }

    fn flash(&mut self, text: impl Into<String>, cx: &mut Context<Self>) {
        let text = text.into();
        self.notice = Some(text.clone());
        self.notice_task = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(NOTICE_TIME).await;
            let _ = this.update(cx, |v, cx| {
                if v.notice.as_deref() == Some(text.as_str()) {
                    v.notice = None;
                    cx.notify();
                }
            });
        }));
        cx.notify();
    }

    // ── layout and animation ────────────────────────────────────────────────────────────────

    fn relayout(&mut self, instant: bool, cx: &mut Context<Self>) {
        let old = self.ctl.layout.clone();
        let (m, colors) = (self.m.clone(), self.colors);
        let Some(layout) = self.ctl.relayout(&m, &colors, wall_ms()) else { return };
        let (vw, vh) = (self.viewport.0 / self.zoom, self.viewport.1 / self.zoom);
        let cam = camera_for(layout, vw, vh, &self.m, self.dpr);
        if std::env::var_os("LUPASTA_DEBUG_LAYOUT").is_some() {
            eprintln!("layout {}: viewport {vw}x{vh} max_x {} cam {:?} nodes {}", layout.selected_id, layout.max_x, cam, layout.nodes.len());
        }
        let now = self.clock.elapsed().as_secs_f64() * 1000.0;
        self.anim.set_target(layout, cam, now, instant);
        self.prev_layout = old;
        cx.notify();
    }

    /// After the selection moved: relayout, remember it, load what the previews need.
    fn selection_changed(&mut self, instant: bool, cx: &mut Context<Self>) {
        self.relayout(instant, cx);
        self.schedule_remember(cx);
        self.ensure_preview(cx);
    }

    fn schedule_remember(&mut self, cx: &mut Context<Self>) {
        let id = self.ctl.selected_id.clone();
        self.remember = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(REMEMBER_DELAY).await;
            let _ = this.update(cx, |v, _| v.remember_selection(&id));
        }));
    }

    fn remember_selection(&mut self, id: &str) {
        if !self.persist_location {
            return;
        }
        let loc = Some(Location { root: self.root.abs.clone(), select: Some(id.to_string()).filter(|p| !p.is_empty()) });
        if self.settings.last == loc {
            return;
        }
        self.settings.last = loc;
        let _ = self.settings.save(&self.data_dir);
    }

    /// Loads whatever the preview columns need for the current selection, then relayouts. Two
    /// rounds: the selected folder's sub-folders are only known once it is loaded.
    fn ensure_preview(&mut self, cx: &mut Context<Self>) {
        let epoch = self.ctl.epoch;
        cx.spawn(async move |this, cx| {
            for _round in 0..2 {
                let Ok(Some((missing, scope, opts))) = this.update(cx, |v, _| {
                    let missing = v.ctl.take_missing_previews();
                    (!missing.is_empty() && v.ctl.epoch == epoch).then(|| (missing, v.session.scope.clone(), v.list_options()))
                }) else {
                    return;
                };
                let req = missing.clone();
                let listings = cx.background_executor().spawn(async move { session::list_many(&scope, &req, &opts) }).await;
                let go_on = this
                    .update(cx, |v, cx| {
                        if v.ctl.epoch != epoch {
                            return false;
                        }
                        if v.ctl.previews_loaded(&missing, &listings) {
                            v.relayout(false, cx);
                        }
                        true
                    })
                    .unwrap_or(false);
                if !go_on {
                    return;
                }
            }
        })
        .detach();
    }

    /// Lists the root and selects the initial entry (or the middle one).
    fn load_root(&mut self, instant: bool, cx: &mut Context<Self>) {
        let opts = self.list_options();
        let s = self.session.clone();
        let top = session::list(&s.scope, "", &opts).unwrap_or(Listing { path: String::new(), entries: Vec::new() });
        self.ctl.reset(&self.root.name, &top);
        self.prev_layout = None;
        let revealed = s
            .initial
            .as_deref()
            .and_then(|initial| session::reveal_chain(&s.scope, initial, &opts).ok().map(|chain| self.ctl.revealed(initial, &chain)))
            .unwrap_or(false);
        if revealed || self.ctl.select_default() {
            self.selection_changed(instant, cx);
        }
        cx.notify();
    }

    /// Reveals a deep path (search pick): lists the chain off the UI thread, then selects it.
    fn reveal(&mut self, path: String, cx: &mut Context<Self>) {
        let (scope, opts, epoch) = (self.session.scope.clone(), self.list_options(), self.ctl.epoch);
        cx.spawn(async move |this, cx| {
            let p = path.clone();
            let chain = cx.background_executor().spawn(async move { session::reveal_chain(&scope, &p, &opts) }).await;
            let _ = this.update(cx, |v, cx| match chain {
                Ok(chain) if v.ctl.epoch == epoch => {
                    if v.ctl.revealed(&path, &chain) {
                        v.selection_changed(false, cx);
                    }
                }
                Ok(_) => {}
                Err(e) => v.flash(e, cx),
            });
        })
        .detach();
    }

    /// Lists `dirs` again (watcher news, or a listing setting changed) and relayouts.
    fn refresh(&mut self, dirs: Vec<String>, cx: &mut Context<Self>) {
        if dirs.is_empty() {
            return;
        }
        let (scope, opts, epoch) = (self.session.scope.clone(), self.list_options(), self.ctl.epoch);
        cx.spawn(async move |this, cx| {
            let listings = cx.background_executor().spawn(async move { session::list_many(&scope, &dirs, &opts) }).await;
            let _ = this.update(cx, |v, cx| {
                if v.ctl.epoch == epoch && v.ctl.refreshed(&listings) {
                    v.relayout(false, cx);
                    v.ensure_preview(cx);
                }
            });
        })
        .detach();
    }

    fn on_core_event(&mut self, ev: CoreEvent, cx: &mut Context<Self>) {
        match ev {
            CoreEvent::Index { generation } if generation == self.session.generation => {
                self.status = Some(self.session.status());
                cx.notify();
            }
            CoreEvent::Changed { generation, dirs } if generation == self.session.generation => {
                let loaded = self.ctl.loaded_among(&dirs);
                self.refresh(loaded, cx);
            }
            _ => {}
        }
    }

    // ── navigation ──────────────────────────────────────────────────────────────────────────

    fn handle_need(&mut self, need: Need, cx: &mut Context<Self>) {
        match need {
            Need::Nothing => {}
            Need::Listing(dir) => {
                let (scope, opts, epoch) = (self.session.scope.clone(), self.list_options(), self.ctl.epoch);
                cx.spawn(async move |this, cx| {
                    let d = dir.clone();
                    let listing = cx.background_executor().spawn(async move { session::list(&scope, &d, &opts) }).await;
                    let _ = this.update(cx, |v, cx| {
                        if v.ctl.epoch != epoch {
                            return;
                        }
                        match listing {
                            Ok(l) => {
                                v.ctl.ingest(&[l]);
                                if v.ctl.entered(&dir) {
                                    v.selection_changed(false, cx);
                                } else {
                                    v.relayout(false, cx);
                                }
                            }
                            Err(_) => v.ctl.listing_failed(&dir),
                        }
                    });
                })
                .detach();
            }
            Need::Open(id) => match self.session.scope.resolve(&id) {
                Ok(abs) => cx.open_with_system(&abs),
                Err(e) => self.flash(e.to_string(), cx),
            },
            Need::LeaveRoot => self.up_root(cx),
        }
    }

    /// Runs a navigation step; relayouts when the selection moved.
    fn nav(&mut self, f: impl FnOnce(&mut Controller) -> bool, cx: &mut Context<Self>) {
        let before = self.ctl.selected_id.clone();
        if f(&mut self.ctl) || self.ctl.selected_id != before {
            self.selection_changed(false, cx);
        }
    }

    fn tree_key(&mut self, key: &str, cx: &mut Context<Self>) -> bool {
        match key {
            "up" => self.nav(|c| c.move_by(-1), cx),
            "down" => self.nav(|c| c.move_by(1), cx),
            "pageup" => self.nav(|c| c.move_by(-10), cx),
            "pagedown" => self.nav(|c| c.move_by(10), cx),
            "home" => self.nav(|c| c.move_by(i64::MIN / 2), cx),
            "end" => self.nav(|c| c.move_by(i64::MAX / 2), cx),
            "right" | "space" => self.step(Controller::enter, cx),
            "left" => self.step(Controller::leave, cx),
            "enter" => self.step(Controller::activate, cx),
            _ => return false,
        }
        true
    }

    fn step(&mut self, f: fn(&mut Controller) -> Need, cx: &mut Context<Self>) {
        let before = self.ctl.selected_id.clone();
        let need = f(&mut self.ctl);
        if self.ctl.selected_id != before {
            self.selection_changed(false, cx);
        }
        self.handle_need(need, cx);
    }

    // ── roots ───────────────────────────────────────────────────────────────────────────────

    fn set_root(&mut self, path: &Path, select: Option<String>, cx: &mut Context<Self>) {
        if !path.is_dir() {
            self.flash(format!("not a folder: {}", path.display()), cx);
            return;
        }
        let generation = self.next_generation;
        self.next_generation += 1;
        let next = match session::build_session(path, select, &self.data_dir, self.no_index, &self.settings, generation) {
            Ok(s) => s,
            Err(e) => return self.flash(e, cx),
        };
        self.session.retire();
        self.session = next.clone();
        session::start_session(next.clone(), &self.data_dir, self.tx.clone());
        self.root = next.root_info();
        self.persist_location = true;
        self.settings.remember_root(&self.root.abs);
        self.settings.last = Some(Location { root: self.root.abs.clone(), select: next.initial.clone() });
        if let Err(e) = self.settings.save(&self.data_dir) {
            self.flash(format!("could not save settings: {e}"), cx);
        }
        self.search = None;
        self.load_root(false, cx);
        self.status = Some(self.session.status());
    }

    fn up_root(&mut self, cx: &mut Context<Self>) {
        if let Some(parent) = self.root.parent.clone() {
            let name = self.root.name.clone();
            self.set_root(Path::new(&parent), Some(name), cx);
        }
    }

    fn pick_root(&mut self, cx: &mut Context<Self>) {
        let rx = cx.prompt_for_paths(PathPromptOptions { files: false, directories: true, multiple: false, prompt: Some("Open folder in lupasta".into()) });
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(paths))) = rx.await
                && let Some(p) = paths.into_iter().next()
            {
                let _ = this.update(cx, |v, cx| v.set_root(&p, None, cx));
            }
        })
        .detach();
    }

    // ── settings ────────────────────────────────────────────────────────────────────────────

    /// Applies settings to the running scene; `prev` = what is currently applied.
    fn apply(&mut self, prev: Option<&Settings>, cx: &mut Context<Self>) {
        let s = self.settings.clone();
        let ch = prev.map(|p| prefs::diff(p, &s)).unwrap_or(prefs::Changes { listing: false, layout: true, font: true, zoom: true, motion: true });
        self.m.max_chars = s.max_chars as usize;
        // The system's "reduce motion" preference turns animation off, like prefers-reduced-motion.
        self.m.duration_ms = if cx.reduce_motion() { 0.0 } else { s.animation_ms as f32 };
        self.anim.duration = self.m.duration_ms;
        self.colors = Colors::new(ColorMode::parse(&s.color_mode), s.age_max_days as f64 * DAY_MS);
        self.ctl.wheel_step = s.wheel_step as f32;
        if ch.font {
            self.fit = text::fit(cx, &s.font, self.m.char_w, self.m.row_h);
            self.m.font_size = self.fit.font_size;
            self.m.text_y = self.fit.text_y;
        }
        self.zoom = s.zoom as f32;
        if ch.listing {
            let all = self.ctl.all_loaded();
            self.refresh(all, cx);
        } else if ch.layout || ch.font || ch.zoom {
            self.relayout(ch.font || ch.zoom, cx);
        }
        cx.notify();
    }

    fn change(&mut self, next: Settings, cx: &mut Context<Self>) {
        let prev = self.settings.clone();
        let mut next = next.sanitized();
        // Bookkeeping fields are owned here; a row's copy may be stale.
        next.last = prev.last.clone();
        next.recent_roots = prev.recent_roots.clone();
        let rescan = prev.respect_gitignore != next.respect_gitignore || prev.excludes != next.excludes;
        if let Err(e) = next.save(&self.data_dir) {
            self.flash(format!("could not save settings: {e}"), cx);
        }
        self.settings = next;
        if rescan {
            session::rescan_with(self.session.clone(), &self.settings, self.tx.clone());
        }
        self.apply(Some(&prev), cx);
    }

    fn rows(&self) -> Vec<Row> {
        let os = std::env::consts::OS;
        let data_dir = filesystem::display_path(&self.data_dir);
        prefs::build_rows(&RowContext {
            settings: &self.settings,
            root: Some(&self.root),
            version: env!("CARGO_PKG_VERSION"),
            os,
            data_dir: &data_dir,
            status: self.status.as_ref(),
            update: &self.update,
        })
    }

    fn run_action(&mut self, a: Action, cx: &mut Context<Self>) {
        match a {
            Action::PickRoot => self.pick_root(cx),
            Action::UpRoot => self.up_root(cx),
            Action::OpenRoot(p) => self.set_root(Path::new(&p), None, cx),
            Action::Reindex => {
                session::spawn_scan(self.session.clone(), self.tx.clone());
                self.status = Some(self.session.status());
            }
            Action::ClearIndex => {
                let (s, tx) = (self.session.clone(), self.tx.clone());
                cx.background_executor().spawn(async move { session::clear_index(s, tx) }).detach();
            }
            Action::CheckUpdate => self.check_update(cx),
            Action::OpenRelease(url) => {
                if url.starts_with(&format!("https://github.com/{REPO}/")) {
                    cx.open_url(&url);
                }
            }
            Action::OpenData => cx.open_with_system(&self.data_dir),
        }
        cx.notify();
    }

    /// Asks GitHub for the latest published release. No auto-install: that needs signed builds.
    fn check_update(&mut self, cx: &mut Context<Self>) {
        self.update = UpdateState::Checking;
        cx.spawn(async move |this, cx| {
            let result = cx.background_executor().spawn(async { fetch_update() }).await;
            let _ = this.update(cx, |v, cx| {
                v.update = match result {
                    Ok(info) => UpdateState::Known(info),
                    Err(e) => UpdateState::Failed(e),
                };
                cx.notify();
            });
        })
        .detach();
    }

    fn open_panel(&mut self, cx: &mut Context<Self>) {
        let rows = self.rows();
        let active = prefs::next_focus(&rows, -1, 1).max(0) as usize;
        self.panel = Some(PanelState { active, editing: None, scroll: 0.0 });
        self.search = None;
        cx.notify();
    }

    fn panel_key(&mut self, key: &str, ctrl: bool, cx: &mut Context<Self>) {
        let rows = self.rows();
        let Some(p) = self.panel.as_mut() else { return };
        if let Some(edit) = p.editing.as_mut() {
            match key {
                "enter" => {
                    let text = edit.text.clone();
                    p.editing = None;
                    let next = prefs::commit_excludes(&self.settings, &text);
                    self.change(next, cx);
                }
                "escape" => p.editing = None,
                "v" if ctrl => {
                    if let Some(t) = cx.read_from_clipboard().and_then(|c| c.text()) {
                        edit.insert(&t);
                    }
                }
                k => {
                    edit.key(k, ctrl);
                }
            }
            cx.notify();
            return;
        }
        let row = rows.get(p.active).cloned();
        match key {
            "escape" => self.panel = None,
            "," if ctrl => self.panel = None,
            "down" => p.active = prefs::next_focus(&rows, p.active as i64, 1) as usize,
            "up" => p.active = prefs::next_focus(&rows, p.active as i64, -1) as usize,
            "home" => p.active = prefs::next_focus(&rows, -1, 1) as usize,
            "end" => p.active = prefs::next_focus(&rows, rows.len() as i64, -1) as usize,
            "right" | "left" => {
                if let Some(r @ Row::Choice { .. }) = row {
                    let next = prefs::cycle(&self.settings, &r, if key == "right" { 1 } else { -1 });
                    self.change(next, cx);
                }
            }
            "enter" | "space" => {
                if let Some(r) = row {
                    self.activate_row(&r, cx);
                }
            }
            _ => {}
        }
        self.keep_active_visible();
        cx.notify();
    }

    fn activate_row(&mut self, row: &Row, cx: &mut Context<Self>) {
        match row {
            Row::Choice { .. } => {
                let next = prefs::cycle(&self.settings, row, 1);
                self.change(next, cx);
            }
            Row::Action { action, .. } => self.run_action(action.clone(), cx),
            Row::Edit { value, .. } => {
                if let Some(p) = self.panel.as_mut() {
                    p.editing = Some(LineEdit::new(value));
                }
            }
            _ => {}
        }
    }

    fn keep_active_visible(&mut self) {
        let rows = self.rows();
        let (_, vh) = self.layout_viewport();
        let row_h = self.m.row_h;
        let Some(p) = self.panel.as_mut() else { return };
        let (tops, total) = chrome::panel_geometry(&rows, row_h);
        let view = chrome::panel_viewport(vh);
        let Some(&top) = tops.get(p.active) else { return };
        if top < p.scroll {
            p.scroll = top;
        } else if top + row_h > p.scroll + view {
            p.scroll = top + row_h - view;
        }
        p.scroll = p.scroll.clamp(0.0, (total - view).max(0.0));
    }

    // ── search ──────────────────────────────────────────────────────────────────────────────

    fn open_search(&mut self, swallow: Option<String>, cx: &mut Context<Self>) {
        self.panel = None;
        self.search = Some(SearchState { edit: LineEdit::default(), hits: Vec::new(), active: 0, seq: 0, swallow });
        cx.notify();
    }

    fn run_search(&mut self, cx: &mut Context<Self>) {
        let Some(st) = self.search.as_mut() else { return };
        st.seq += 1;
        let seq = st.seq;
        let norm = search::normalize_query(&st.edit.text);
        if norm.is_empty() {
            st.hits.clear();
            cx.notify();
            return;
        }
        let (corpus, index) = (self.session.corpus.clone(), self.session.index.clone());
        self.search_task = Some(cx.spawn(async move |this, cx| {
            let res = cx.background_executor().spawn(async move { search::search(&corpus, index.as_deref(), &norm, MAX_HITS) }).await;
            let _ = this.update(cx, |v, cx| {
                if let Some(st) = v.search.as_mut()
                    && st.seq == seq
                {
                    st.hits = res.hits;
                    st.active = 0;
                    cx.notify();
                }
            });
        }));
    }

    fn search_key(&mut self, key: &str, ctrl: bool, cx: &mut Context<Self>) -> bool {
        let Some(st) = self.search.as_mut() else { return false };
        st.swallow = None;
        match key {
            "escape" => self.search = None,
            "down" => st.active = (st.active + 1).min(st.hits.len().saturating_sub(1)),
            "up" => st.active = st.active.saturating_sub(1),
            "enter" => {
                if let Some(h) = st.hits.get(st.active) {
                    let path = h.path.clone();
                    self.search = None;
                    self.reveal(path, cx);
                }
            }
            "v" if ctrl => {
                if let Some(t) = cx.read_from_clipboard().and_then(|c| c.text()) {
                    st.edit.insert(&t);
                    self.run_search(cx);
                }
            }
            k => {
                if !st.edit.key(k, ctrl) {
                    return false;
                }
                self.run_search(cx);
            }
        }
        cx.notify();
        true
    }

    fn editing(&self) -> bool {
        self.search.is_some() || self.panel.as_ref().is_some_and(|p| p.editing.is_some())
    }

    fn edit_mut(&mut self) -> Option<&mut LineEdit> {
        if let Some(s) = self.search.as_mut() {
            return Some(&mut s.edit);
        }
        self.panel.as_mut().and_then(|p| p.editing.as_mut())
    }

    // ── shell shortcuts ─────────────────────────────────────────────────────────────────────

    fn copy_path(&mut self, cx: &mut Context<Self>) {
        let Some(n) = self.ctl.selected() else { return };
        match self.session.scope.resolve(&n.id) {
            Ok(abs) => {
                let shown = filesystem::display_path(&abs);
                cx.write_to_clipboard(ClipboardItem::new_string(shown.clone()));
                self.flash(format!("copied {shown}"), cx);
            }
            Err(e) => self.flash(e.to_string(), cx),
        }
    }

    fn reveal_in_os(&mut self, cx: &mut Context<Self>) {
        let Some(n) = self.ctl.selected() else { return };
        if let Ok(abs) = self.session.scope.resolve(&n.id) {
            cx.reveal_path(Path::new(&filesystem::display_path(&abs)));
        }
    }

    /// Shell-level shortcuts; returns true when handled. `typed` is the character the key
    /// produces: "/" is AltGr+Q on a Brazilian keyboard, which arrives as ctrl+alt+q.
    fn shortcut(&mut self, key: &str, typed: Option<&str>, ctrl: bool, cx: &mut Context<Self>) -> bool {
        if ctrl && key == "," {
            if self.panel.is_some() {
                self.panel = None;
            } else {
                self.open_panel(cx);
            }
            cx.notify();
            return true;
        }
        if self.panel.is_some() {
            return false;
        }
        if typed == Some("/") || (key == "/" && !ctrl) {
            self.open_search(Some("/".into()), cx);
            return true;
        }
        if !ctrl {
            return false;
        }
        match key {
            "k" => self.open_search(None, cx),
            "c" => self.copy_path(cx),
            "e" => self.reveal_in_os(cx),
            "h" => {
                let mut next = self.settings.clone();
                next.show_hidden = !next.show_hidden;
                let msg = if next.show_hidden { "hidden files shown" } else { "hidden files hidden" };
                self.change(next, cx);
                self.flash(msg, cx);
            }
            "o" => self.pick_root(cx),
            "=" | "+" => {
                let mut next = self.settings.clone();
                next.zoom = prefs::step_zoom(next.zoom, 1);
                self.change(next, cx);
            }
            "-" => {
                let mut next = self.settings.clone();
                next.zoom = prefs::step_zoom(next.zoom, -1);
                self.change(next, cx);
            }
            "0" => {
                let mut next = self.settings.clone();
                next.zoom = 1.0;
                self.change(next, cx);
            }
            _ => return false,
        }
        true
    }

    // ── input ───────────────────────────────────────────────────────────────────────────────

    fn on_key_down(&mut self, ev: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let ks = &ev.keystroke;
        // AltGr is reported as ctrl+alt; it types characters, it is not a shortcut modifier.
        let ctrl = (ks.modifiers.control && !ks.modifiers.alt) || ks.modifiers.platform;
        let key = ks.key.as_str();
        let typed = ks.key_char.as_deref();
        let handled = if self.search.is_some() {
            self.search_key(key, ctrl, cx)
        } else if self.panel.as_ref().is_some_and(|p| p.editing.is_some()) {
            // Printable keys arrive as text input; only editing keys are handled here.
            let editing_key = matches!(key, "enter" | "escape" | "backspace" | "delete" | "left" | "right" | "home" | "end") || (ctrl && key == "v");
            if editing_key {
                self.panel_key(key, ctrl, cx);
            }
            editing_key
        } else if self.shortcut(key, typed, ctrl, cx) {
            true
        } else if self.panel.is_some() {
            self.panel_key(key, ctrl, cx);
            true
        } else {
            key != "escape" && self.tree_key(key, cx)
        };
        if handled {
            cx.stop_propagation();
        }
    }

    fn on_mouse_down(&mut self, ev: &MouseDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let (x, y) = (f32::from(ev.position.x), f32::from(ev.position.y));
        self.mouse = (x, y);
        let target = self.hits.at(x, y).cloned();
        if self.search.is_some() {
            if let Some(Target::SearchHit(i)) = target {
                if let Some(path) = self.search.as_ref().and_then(|s| s.hits.get(i)).map(|h| h.path.clone()) {
                    self.search = None;
                    self.reveal(path, cx);
                }
                return;
            }
            // A click anywhere else closes the prompt, then acts as usual.
            self.search = None;
            cx.notify();
            if target == Some(Target::Overlay) {
                return;
            }
        }
        match target {
            Some(Target::BarPath) => return self.pick_root(cx),
            Some(Target::BarUp) => return self.up_root(cx),
            Some(Target::BarSearch) => return self.open_search(None, cx),
            Some(Target::BarSettings) => {
                if self.panel.is_some() {
                    self.panel = None;
                } else {
                    self.open_panel(cx);
                }
                return cx.notify();
            }
            Some(Target::BarUpdate) => {
                if let UpdateState::Known(UpdateInfo { url: Some(url), .. }) = &self.update {
                    let url = url.clone();
                    self.run_action(Action::OpenRelease(url), cx);
                }
                return;
            }
            Some(Target::Row(i)) => {
                let rows = self.rows();
                if let (Some(p), Some(row)) = (self.panel.as_mut(), rows.get(i))
                    && row.focusable() {
                        p.active = i;
                        p.editing = None;
                        let row = row.clone();
                        self.activate_row(&row, cx);
                    }
                return cx.notify();
            }
            Some(Target::RowOption(i, j)) => {
                let rows = self.rows();
                if let Some(Row::Choice { options, .. }) = rows.get(i) {
                    if let Some(p) = self.panel.as_mut() {
                        p.active = i;
                    }
                    let next = (options[j].apply)(&self.settings);
                    self.change(next, cx);
                }
                return;
            }
            Some(Target::RowCycle(i, dir)) => {
                let rows = self.rows();
                if let Some(row) = rows.get(i) {
                    if let Some(p) = self.panel.as_mut() {
                        p.active = i;
                    }
                    let next = prefs::cycle(&self.settings, row, dir);
                    self.change(next, cx);
                }
                return;
            }
            Some(Target::Overlay) | Some(Target::SearchHit(_)) => return,
            None => {}
        }
        if self.panel.is_some() {
            return;
        }
        let hit = self.scene_view().hit(x, y);
        if let Some(id) = hit {
            if self.ctl.select(&id) {
                self.selection_changed(false, cx);
            }
            if ev.click_count >= 2 && self.ctl.model.get(&id).is_some_and(|n| !n.is_dir) {
                self.handle_need(Need::Open(id), cx);
            }
        }
    }

    fn on_mouse_move(&mut self, ev: &MouseMoveEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let (x, y) = (f32::from(ev.position.x), f32::from(ev.position.y));
        self.mouse = (x, y);
        let yl = y / self.zoom;
        if yl <= BAR_REVEAL_Y {
            self.bar_hide = None;
            self.set_bar_hover(true);
        } else if self.bar_hover && yl > BAR_HIDE_Y && self.bar_hide.is_none() {
            self.bar_hide = Some(cx.spawn(async move |this, cx| {
                cx.background_executor().timer(BAR_HIDE_DELAY).await;
                let _ = this.update(cx, |v, cx| {
                    v.bar_hide = None;
                    v.set_bar_hover(false);
                    cx.notify();
                });
            }));
        }
        if self.bar_hover || self.panel.is_some() {
            cx.notify();
        }
    }

    fn bar_target(&self) -> bool {
        self.bar_hover || self.panel.is_some()
    }

    fn bar_shown(&self) -> f32 {
        let t = ((self.now() - self.bar_since) / BAR_EASE_MS).clamp(0.0, 1.0) as f32;
        let to = if self.bar_target() { 1.0 } else { 0.0 };
        self.bar_from + (to - self.bar_from) * ease_out_cubic(t)
    }

    fn set_bar_hover(&mut self, on: bool) {
        if self.bar_hover == on {
            return;
        }
        self.bar_from = self.bar_shown();
        self.bar_since = self.now();
        self.bar_hover = on;
    }

    fn on_scroll(&mut self, ev: &ScrollWheelEvent, _window: &mut Window, cx: &mut Context<Self>) {
        // Positive = towards the user (content moves up), as in a browser's deltaY.
        let dy = match ev.delta {
            ScrollDelta::Pixels(p) => -f32::from(p.y),
            ScrollDelta::Lines(p) => -p.y * LINE_PX,
        };
        if self.search.is_some() {
            return;
        }
        if self.panel.is_some() {
            let rows = self.rows();
            let (_, vh) = self.layout_viewport();
            let (_, total) = chrome::panel_geometry(&rows, self.m.row_h);
            let max = (total - chrome::panel_viewport(vh)).max(0.0);
            if let Some(p) = self.panel.as_mut() {
                p.scroll = (p.scroll + dy / self.zoom).clamp(0.0, max);
            }
            cx.notify();
            return;
        }
        if self.ctl.wheel(dy) {
            self.selection_changed(false, cx);
        }
    }

    fn scene_view(&self) -> SceneView<'_> {
        let (vw, vh) = self.layout_viewport();
        let receded = self.search.is_some() || self.panel.is_some();
        SceneView {
            anim: &self.anim,
            layout: self.ctl.layout.as_ref(),
            prev: self.prev_layout.as_ref(),
            progress: self.anim.progress(self.now()),
            m: &self.m,
            zoom: self.zoom,
            vw,
            vh,
            alpha: if receded { RECEDED } else { 1.0 },
        }
    }

    // ── smoke test ──────────────────────────────────────────────────────────────────────────

    /// `--smoke`: proves the real app lays out, indexes, searches and watches on this platform,
    /// then exits 0 (or 1 with the reason).
    fn run_smoke(&mut self, cx: &mut Context<Self>) {
        let data_dir = self.data_dir.clone();
        cx.spawn(async move |this, cx| {
            let bg = cx.background_executor().clone();
            let timer = |ms| bg.timer(Duration::from_millis(ms));
            for _ in 0..100 {
                let missing = this.update(cx, |v, _| !v.ctl.model.missing_for_preview(&v.ctl.selected_id).is_empty()).unwrap_or(false);
                if !missing {
                    break;
                }
                timer(50).await;
            }
            for _ in 0..600 {
                if this.update(cx, |v, _| v.session.status().state == "ready").unwrap_or(true) {
                    break;
                }
                timer(100).await;
            }
            let result = this.update(cx, |v, _| -> Result<String, String> {
                let nodes = v.ctl.layout.as_ref().map_or(0, |l| l.nodes.len());
                if nodes == 0 {
                    return Err("no layout after init".into());
                }
                let first = v.ctl.model.children("").and_then(|k| k.first().map(|n| (n.id.clone(), n.name.clone()))).ok_or("root listing is empty")?;
                let res = search::search(&v.session.corpus, v.session.index.as_deref(), &first.1, 5);
                if !res.hits.iter().any(|h| h.path == first.0) {
                    return Err(format!("search for \"{}\" did not find it ({})", first.1, res.strategy));
                }
                if let Some(e) = v.session.watch_error.lock().unwrap().clone() {
                    return Err(format!("watcher: {e}"));
                }
                Ok(format!("{nodes} nodes laid out, search found {}", first.0))
            });
            let (line, code) = match result {
                Ok(Ok(detail)) => (format!("smoke ok: {detail}"), 0),
                Ok(Err(e)) => (format!("smoke FAILED: {e}"), 1),
                Err(e) => (format!("smoke FAILED: {e}"), 1),
            };
            smoke_log(&data_dir, &line);
            std::process::exit(code);
        })
        .detach();
    }
}

fn fetch_update() -> Result<UpdateInfo, String> {
    let current = env!("CARGO_PKG_VERSION").to_string();
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
    let newer = tag.as_deref().is_some_and(|t| lupasta::update::is_newer(t, &current));
    Ok(UpdateInfo { current, latest: tag, newer, url: page })
}

impl Render for Lupasta {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let vp = window.viewport_size();
        let size = (f32::from(vp.width), f32::from(vp.height));
        self.dpr = window.scale_factor();
        if size != self.viewport {
            // A resize re-applies the layout instantly.
            self.viewport = size;
            if let Some(layout) = self.ctl.layout.as_ref() {
                let (vw, vh) = self.layout_viewport();
                let cam = camera_for(layout, vw, vh, &self.m, self.dpr);
                let now = self.now();
                self.anim.set_target(layout, cam, now, true);
            }
        }
        let now = self.now();
        if self.anim.running {
            self.anim.tick(now);
            if !self.anim.running {
                self.prev_layout = None;
            }
        }
        let bar_shown = self.bar_shown();
        let bar_moving = (bar_shown - if self.bar_target() { 1.0 } else { 0.0 }).abs() > 0.001;

        let (z, cell, row_h) = (self.zoom, self.m.char_w, self.m.row_h);
        let (vw, vh) = self.layout_viewport();
        let mut frame = Frame::default();
        let mut hits = Hits::default();
        self.scene_view().paint(&mut frame);

        let update_label = match &self.update {
            UpdateState::Known(UpdateInfo { newer: true, latest: Some(l), .. }) => Some(format!("{l} available")),
            _ => None,
        };
        if let Some(p) = &self.panel {
            let rows = self.rows();
            let view = PanelView { rows: &rows, active: p.active, editing: p.editing.as_ref(), scroll: p.scroll, mouse: self.mouse };
            chrome::paint_panel(&mut frame, &mut hits, &view, vw, vh, z, cell, row_h);
        }
        let bar = Bar { root: Some(&self.root), update: update_label, settings_open: self.panel.is_some(), shown: bar_shown, mouse: self.mouse };
        chrome::paint_bar(&mut frame, &mut hits, &bar, vw, z, cell, row_h);
        if let Some(s) = &self.search {
            let view = SearchView { edit: &s.edit, hits: &s.hits, active: s.active, status: self.status.as_ref() };
            chrome::paint_search(&mut frame, &mut hits, &view, z, cell, row_h);
        }
        let bottom = match (&self.error, &self.notice) {
            (Some(e), _) => Some(e.clone()),
            (None, Some(n)) => Some(n.clone()),
            _ if self.settings.status_line && self.search.is_none() && self.panel.is_none() => self.ctl.selected().map(|n| {
                let kids = n.children.as_ref().filter(|_| n.loaded).map(|c| c.len());
                chrome::status_text(n.is_dir, kids, n.size, n.mtime, wall_ms())
            }),
            _ => None,
        };
        if let Some(text) = bottom {
            chrome::paint_bottom(&mut frame, &text, vh, z, cell, row_h);
        }
        self.hits = hits;
        if self.anim.running || bar_moving {
            window.request_animation_frame();
        }

        let style = Style { fit: self.fit.clone(), zoom: z, cell, row_h };
        let focus = self.focus.clone();
        let input = self.editing().then(|| cx.entity());
        div()
            .id("lupasta")
            .track_focus(&self.focus)
            .size_full()
            .bg(gpui::rgb(0x000000))
            .on_key_down(cx.listener(Self::on_key_down))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .on_scroll_wheel(cx.listener(Self::on_scroll))
            .child(
                canvas(
                    |_, _, _| {},
                    move |bounds: Bounds<Pixels>, _, window, cx| {
                        if let Some(entity) = input {
                            window.handle_input(&focus, ElementInputHandler::new(bounds, entity), cx);
                        }
                        frame::paint(frame, &style, window, cx);
                    },
                )
                .size_full(),
            )
    }
}

/// Text input (typed characters, dead keys, IME commits) for the search prompt and the panel's
/// edit row. The caret is the whole selection; there is no marked-text display.
impl EntityInputHandler for Lupasta {
    fn text_for_range(&mut self, range: Range<usize>, adjusted: &mut Option<Range<usize>>, _: &mut Window, _: &mut Context<Self>) -> Option<String> {
        let edit = self.edit_mut()?;
        let utf16: Vec<u16> = edit.text.encode_utf16().collect();
        let r = range.start.min(utf16.len())..range.end.min(utf16.len());
        *adjusted = Some(r.clone());
        String::from_utf16(&utf16[r]).ok()
    }

    fn selected_text_range(&mut self, _: bool, _: &mut Window, _: &mut Context<Self>) -> Option<UTF16Selection> {
        let edit = self.edit_mut()?;
        let at: usize = edit.text.chars().take(edit.caret).map(char::len_utf16).sum();
        Some(UTF16Selection { range: at..at, reversed: false })
    }

    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        None
    }

    fn unmark_text(&mut self, _: &mut Window, _: &mut Context<Self>) {}

    fn replace_text_in_range(&mut self, _range: Option<Range<usize>>, text: &str, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(s) = self.search.as_mut() {
            if s.swallow.take().as_deref() == Some(text) {
                return;
            }
            s.edit.insert(text);
            self.run_search(cx);
        } else if let Some(edit) = self.panel.as_mut().and_then(|p| p.editing.as_mut()) {
            edit.insert(text);
        }
        cx.notify();
    }

    fn replace_and_mark_text_in_range(&mut self, _: Option<Range<usize>>, _: &str, _: Option<Range<usize>>, _: &mut Window, _: &mut Context<Self>) {}

    fn bounds_for_range(&mut self, _: Range<usize>, bounds: Bounds<Pixels>, _: &mut Window, _: &mut Context<Self>) -> Option<Bounds<Pixels>> {
        Some(bounds)
    }

    fn character_index_for_point(&mut self, _: gpui::Point<Pixels>, _: &mut Window, _: &mut Context<Self>) -> Option<usize> {
        None
    }
}
