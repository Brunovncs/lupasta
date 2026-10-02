//! Everything drawn over the scene, in the same monospace cells as the tree: the top bar, the
//! search prompt, the settings panel and the bottom line. Each painter also records where its
//! clickable parts are, so mouse handling reads the same geometry that was drawn.

use super::frame::{Frame, hsla};
use lupasta::layout::cell_width;
use lupasta::line_edit::LineEdit;
use lupasta::palette::{BACKGROUND, CONNECTOR, DIM, PATH_CONNECTOR, RULE, SELECTED, TEXT};
use lupasta::prefs::{self, Row};
use lupasta::search::SearchHit;
use lupasta::session::{IndexStatus, RootInfo};

/// Chrome geometry, in layout px (multiplied by the zoom when drawn).
pub const PAD_X: f32 = 20.0;
pub const BAR_H: f32 = 28.0;
const BAR_PAD_Y: f32 = 6.0;
const BAR_GAP_CELLS: usize = 3;
const SEARCH_TOP: f32 = 16.0;
const SEARCH_INPUT_CELLS: usize = 60;
const PANEL_PAD_Y: f32 = 16.0;
const LABEL_CELLS: usize = 22;
const INDENT_CELLS: usize = 2;
const BOTTOM: f32 = 16.0;
pub const MAX_HITS: usize = 12;

#[derive(Debug, Clone, PartialEq)]
pub enum Target {
    BarPath,
    BarUp,
    BarSearch,
    BarSettings,
    BarUpdate,
    SearchHit(usize),
    /// Anywhere on an overlay that should swallow the click.
    Overlay,
    Row(usize),
    RowOption(usize, usize),
    RowCycle(usize, i64),
}

pub struct Hit {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub target: Target,
}

#[derive(Default)]
pub struct Hits(pub Vec<Hit>);

impl Hits {
    fn add(&mut self, x: f32, y: f32, w: f32, h: f32, target: Target) {
        self.0.push(Hit { x, y, w, h, target });
    }

    /// The topmost target under a window-px point.
    pub fn at(&self, x: f32, y: f32) -> Option<&Target> {
        self.0.iter().rev().find(|h| x >= h.x && x < h.x + h.w && y >= h.y && y < h.y + h.h).map(|h| &h.target)
    }
}

/// Draws text cell by cell from (x, y) in layout px; returns the x after it.
struct Pen<'a> {
    frame: &'a mut Frame,
    z: f32,
    cell: f32,
    row_h: f32,
}

impl Pen<'_> {
    fn text(&mut self, x: f32, y: f32, s: &str, color: gpui::Hsla) -> f32 {
        self.frame.text(x * self.z, y * self.z, s, color);
        x + self.width(s)
    }

    fn width(&self, s: &str) -> f32 {
        cell_width(s) as f32 * self.cell
    }

    fn rect(&mut self, x: f32, y: f32, w: f32, h: f32, color: gpui::Hsla) {
        self.frame.rect(x * self.z, y * self.z, w * self.z, h * self.z, color);
    }

    /// The red block behind a first character: the tree's caret idiom.
    fn caret_block(&mut self, x: f32, y: f32) {
        let (w, h) = (self.cell, self.row_h);
        self.rect(x, y, w, h, hsla(SELECTED, 1.0));
    }
}

fn hit(hits: &mut Hits, z: f32, x: f32, y: f32, w: f32, h: f32, target: Target) {
    hits.add(x * z, y * z, w * z, h * z, target);
}

/// Keeps the end of a long path visible by cutting its start.
fn clip_left(s: &str, max_cells: usize) -> String {
    if cell_width(s) <= max_cells {
        return s.to_string();
    }
    let chars: Vec<char> = s.chars().collect();
    let mut out = Vec::new();
    let mut used = 1;
    for c in chars.iter().rev() {
        let w = lupasta::layout::cells_of(*c);
        if used + w > max_cells {
            break;
        }
        out.push(*c);
        used += w;
    }
    out.reverse();
    format!("…{}", out.into_iter().collect::<String>())
}

/// Cuts a row to the panel width, ending in "…".
fn clip_right(s: &str, max_cells: usize) -> String {
    lupasta::layout::truncate(s, max_cells)
}

pub struct Bar<'a> {
    pub root: Option<&'a RootInfo>,
    pub update: Option<String>,
    pub settings_open: bool,
    /// 0 = hidden, 1 = shown (slides down while it eases in).
    pub shown: f32,
    pub mouse: (f32, f32),
}

pub fn paint_bar(frame: &mut Frame, hits: &mut Hits, bar: &Bar, vw: f32, z: f32, cell: f32, row_h: f32) {
    if bar.shown <= 0.001 {
        return;
    }
    let a = bar.shown;
    let top = -BAR_H * (1.0 - a);
    let y = top + BAR_PAD_Y;
    let mut pen = Pen { frame, z, cell, row_h };
    pen.rect(0.0, top, vw, BAR_H, hsla(BACKGROUND, a));
    pen.rect(0.0, top + BAR_H, vw, 1.0, hsla(RULE, a));
    hit(hits, z, 0.0, top, vw, BAR_H, Target::Overlay);

    let gap = BAR_GAP_CELLS as f32 * cell;
    let hover = |x: f32, w: f32| {
        let (mx, my) = (bar.mouse.0 / z, bar.mouse.1 / z);
        mx >= x && mx < x + w && my >= top && my < top + BAR_H
    };
    // Right-hand items, laid out from the right edge.
    let mut right: Vec<(String, Target, bool)> = Vec::new();
    if let Some(u) = &bar.update {
        right.push((u.clone(), Target::BarUpdate, false));
    }
    if bar.root.is_some_and(|r| r.parent.is_some()) {
        right.push(("up".into(), Target::BarUp, false));
    }
    right.push(("search".into(), Target::BarSearch, false));
    right.push(("settings".into(), Target::BarSettings, bar.settings_open));
    let mut x = vw - PAD_X;
    let mut placed = Vec::new();
    for (label, target, on) in right.into_iter().rev() {
        x -= pen.width(&label);
        placed.push((x, label, target, on));
        x -= gap;
    }
    let right_edge = x;

    let name_end = pen.text(PAD_X, y, "lupasta", hsla(PATH_CONNECTOR, a));
    let path_x = name_end + gap;
    let max_cells = ((right_edge - path_x) / cell).floor().max(0.0) as usize;
    let path = clip_left(bar.root.map_or("", |r| r.abs.as_str()), max_cells);
    let pw = pen.width(&path);
    let color = if hover(path_x, pw) { TEXT } else { DIM };
    pen.text(path_x, y, &path, hsla(color, a));
    hit(hits, z, path_x, top, pw, BAR_H, Target::BarPath);

    for (x, label, target, on) in placed {
        let w = pen.width(&label);
        let base = if target == Target::BarUpdate { CONNECTOR } else { DIM };
        let color = if on || hover(x, w) { TEXT } else { base };
        pen.text(x, y, &label, hsla(color, a));
        hit(hits, z, x, top, w, BAR_H, target);
    }
}

pub struct SearchView<'a> {
    pub edit: &'a LineEdit,
    pub hits: &'a [SearchHit],
    pub active: usize,
    pub status: Option<&'a IndexStatus>,
}

pub fn paint_search(frame: &mut Frame, hits: &mut Hits, s: &SearchView, z: f32, cell: f32, row_h: f32) {
    let mut rows: Vec<String> = Vec::new();
    let pending = s.status.filter(|st| st.state != "ready" && st.enabled);
    if let Some(st) = pending {
        rows.push(format!("{} {}", st.state, if st.indexed > 0 { st.indexed } else { st.corpus as u64 }));
    }
    let lines: Vec<(&str, String)> = s.hits.iter().map(|h| {
        let (dir, name) = prefs::split_hit(&h.path);
        (name, prefs::clip_dir(dir, 48))
    }).collect();
    let widest = lines.iter().map(|(n, d)| cell_width(n) + 1 + cell_width(d)).chain(rows.iter().map(|r| cell_width(r))).max().unwrap_or(0).max(SEARCH_INPUT_CELLS + 1);
    let total_rows = 1 + rows.len() + lines.len();
    let mut pen = Pen { frame, z, cell, row_h };
    pen.rect(PAD_X, SEARCH_TOP, widest as f32 * cell, total_rows as f32 * row_h, hsla(BACKGROUND, 1.0));
    hit(hits, z, PAD_X, SEARCH_TOP, widest as f32 * cell, total_rows as f32 * row_h, Target::Overlay);

    let x = pen.text(PAD_X, SEARCH_TOP, "/", hsla(CONNECTOR, 1.0));
    pen.text(x, SEARCH_TOP, &s.edit.text, hsla(TEXT, 1.0));
    let before: String = s.edit.text.chars().take(s.edit.caret).collect();
    let cx = x + pen.width(&before);
    pen.rect(cx, SEARCH_TOP + 1.0, 1.5, row_h - 2.0, hsla(SELECTED, 1.0));

    let mut y = SEARCH_TOP + row_h;
    for r in &rows {
        pen.text(PAD_X, y, r, hsla(DIM, 1.0));
        y += row_h;
    }
    for (i, (name, dir)) in lines.iter().enumerate() {
        if i == s.active {
            pen.caret_block(PAD_X, y);
        }
        let after = pen.text(PAD_X, y, name, hsla(TEXT, 1.0));
        pen.text(after + cell, y, dir, hsla(DIM, 1.0));
        hit(hits, z, PAD_X, y, widest as f32 * cell, row_h, Target::SearchHit(i));
        y += row_h;
    }
}

pub struct PanelView<'a> {
    pub rows: &'a [Row],
    pub active: usize,
    pub editing: Option<&'a LineEdit>,
    pub scroll: f32,
    pub mouse: (f32, f32),
}

/// Row tops (layout px, before scrolling) and the total content height.
pub fn panel_geometry(rows: &[Row], row_h: f32) -> (Vec<f32>, f32) {
    let mut tops = Vec::with_capacity(rows.len());
    let mut y = 0.0;
    for (i, r) in rows.iter().enumerate() {
        if matches!(r, Row::Head(_)) && i > 0 {
            y += row_h;
        }
        tops.push(y);
        y += row_h;
    }
    (tops, y)
}

pub fn panel_viewport(vh: f32) -> f32 {
    (vh - BAR_H - 2.0 * PANEL_PAD_Y).max(0.0)
}

#[allow(clippy::too_many_arguments)]
pub fn paint_panel(frame: &mut Frame, hits: &mut Hits, p: &PanelView, vw: f32, vh: f32, z: f32, cell: f32, row_h: f32) {
    let mut pen = Pen { frame, z, cell, row_h };
    pen.rect(0.0, BAR_H, vw, vh - BAR_H, gpui::Hsla { a: 0.9, ..hsla(BACKGROUND, 1.0) });
    hit(hits, z, 0.0, BAR_H, vw, vh - BAR_H, Target::Overlay);
    let (tops, _) = panel_geometry(p.rows, row_h);
    let origin = BAR_H + PANEL_PAD_Y - p.scroll;
    let clip_top = BAR_H;
    let max_cells = (((vw - PAD_X * 2.0) / cell).floor() as usize).max(4);
    let hover = |x: f32, y: f32, w: f32| {
        let (mx, my) = (p.mouse.0 / z, p.mouse.1 / z);
        mx >= x && mx < x + w && my >= y && my < y + row_h
    };
    for (i, row) in p.rows.iter().enumerate() {
        let y = origin + tops[i];
        if y + row_h <= clip_top || y >= vh {
            continue;
        }
        if let Row::Head(label) = row {
            pen.text(PAD_X, y, label, hsla(CONNECTOR, 1.0));
            continue;
        }
        let x0 = PAD_X + INDENT_CELLS as f32 * cell;
        hit(hits, z, PAD_X, y, vw - 2.0 * PAD_X, row_h, Target::Row(i));
        if i == p.active && row.focusable() {
            pen.caret_block(x0, y);
        }
        let label = row.label();
        pen.text(x0, y, label, hsla(TEXT, 1.0));
        // A label that fills its column still keeps one blank cell before the value.
        let x1 = x0 + LABEL_CELLS.max(cell_width(label) + 1) as f32 * cell;
        let room = max_cells.saturating_sub(INDENT_CELLS + LABEL_CELLS);
        match row {
            Row::Choice { options, index, .. } if prefs::compact(options) => {
                let mut x = x1;
                let lt = "‹";
                let w = pen.width(lt);
                pen.text(x, y, lt, hsla(if hover(x, y, w) { TEXT } else { DIM }, 1.0));
                hit(hits, z, x, y, w, row_h, Target::RowCycle(i, -1));
                x += w + cell;
                x = pen.text(x, y, &options[*index].label, hsla(TEXT, 1.0)) + cell;
                let gt = "›";
                let w = pen.width(gt);
                pen.text(x, y, gt, hsla(if hover(x, y, w) { TEXT } else { DIM }, 1.0));
                hit(hits, z, x, y, w, row_h, Target::RowCycle(i, 1));
            }
            Row::Choice { options, index, .. } => {
                let mut x = x1;
                for (j, o) in options.iter().enumerate() {
                    let w = pen.width(&o.label);
                    let on = j == *index || hover(x, y, w);
                    pen.text(x, y, &o.label, hsla(if on { TEXT } else { DIM }, 1.0));
                    hit(hits, z, x, y, w, row_h, Target::RowOption(i, j));
                    x += w + 2.0 * cell;
                }
            }
            Row::Edit { value, .. } => match p.editing.filter(|_| i == p.active) {
                Some(edit) => {
                    pen.text(x1, y, &edit.text, hsla(TEXT, 1.0));
                    let before: String = edit.text.chars().take(edit.caret).collect();
                    let cx = x1 + pen.width(&before);
                    pen.rect(cx, y + 1.0, 1.5, row_h - 2.0, hsla(SELECTED, 1.0));
                }
                None => {
                    let shown = if value.is_empty() { "—".to_string() } else { clip_right(value, room) };
                    pen.text(x1, y, &shown, hsla(DIM, 1.0));
                }
            },
            Row::Action { detail, .. } | Row::Info { detail, .. } => {
                pen.text(x1, y, &clip_right(detail, room), hsla(DIM, 1.0));
            }
            Row::Head(_) => {}
        }
    }
    // The bar's band stays clear when the content scrolls under it.
    pen.rect(0.0, 0.0, vw, BAR_H, hsla(BACKGROUND, 1.0));
}

/// The bottom-left line: a notice, an error, or the status of the selection.
pub fn paint_bottom(frame: &mut Frame, text: &str, vh: f32, z: f32, cell: f32, row_h: f32) {
    let mut pen = Pen { frame, z, cell, row_h };
    pen.text(PAD_X, vh - BOTTOM - row_h, text, hsla(DIM, 1.0));
}

/// What the status line says about a node: its size or item count, and its age.
pub fn status_text(is_dir: bool, children: Option<usize>, size: u64, mtime: f64, now_ms: f64) -> String {
    let what = if is_dir { format!("{} items", children.map_or("…".to_string(), |n| n.to_string())) } else { prefs::format_size(size) };
    let when = if mtime > 0.0 { format!("modified {}", prefs::relative_age(mtime, now_ms)) } else { String::new() };
    [what, when].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join("   ")
}
