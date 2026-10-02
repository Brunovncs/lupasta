//! Layout: TreeModel + selection → positioned boxes. Pure and deterministic.
//!
//! Shape (from the reference): a horizontal "focus" path. Column i lists the siblings of the
//! i-th node on the selection path, shifted vertically so that path node sits on the focus row
//! (y = 0). After the selection column come preview columns:
//!   level 1 — children of every directory among the selection's siblings,
//!   level 2 — children of the selected directory's own sub-directories.
//! Each preview group is centred on its parent row; groups are then pushed apart (one blank
//! row between them) outward from the group closest to the focus row, which stays centred.

use crate::metrics::Metrics;
use crate::palette::{Colors, Hue, Rgb};
use crate::router::lane_overflow;
use crate::tree::{Node, TreeModel};
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct LayoutNode {
    pub id: String,
    pub parent_id: String,
    pub label: String,
    pub x: f32,
    pub y: f32,
    /// Text width (connectors attach here).
    pub w: f32,
    /// Layout width: text + one trailing cell (not for an overflowing selected name).
    pub box_w: f32,
    pub cut: bool,
    pub col: usize,
    pub hue: Hue,
    pub color: Rgb,
    pub is_dir: bool,
    pub selected: bool,
    pub on_path: bool,
}

#[derive(Debug, Clone)]
pub struct Group {
    pub parent_id: String,
    pub col: usize,
    /// Row top of the first child.
    pub top: f32,
    pub rows: usize,
    pub first_id: String,
    pub last_id: String,
}

#[derive(Debug, Clone, Default)]
pub struct Column {
    pub x: f32,
    pub w: f32,
    pub ids: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct Layout {
    pub selected_id: String,
    pub nodes: HashMap<String, LayoutNode>,
    pub columns: Vec<Column>,
    pub groups: Vec<Group>,
    pub path_edges: Vec<(String, String)>,
    pub min_x: f32,
    pub max_x: f32,
}

pub const ELLIPSIS: char = '…';

/// Terminal-style cell width of a code point. East Asian wide characters and emoji fall back to
/// fonts that are ~2 cells; combining marks take 0. "…" is one cell: Iosevka draws it two cells
/// wide, so the renderer squeezes it into one.
pub fn cells_of(c: char) -> usize {
    let cp = c as u32;
    if (0x300..=0x36f).contains(&cp) || cp == 0x200d || (0xfe00..=0xfe0f).contains(&cp) {
        return 0;
    }
    let wide = [
        (0x1100, 0x115f),
        (0x2e80, 0xa4cf),
        (0xac00, 0xd7a3),
        (0xf900, 0xfaff),
        (0xfe30, 0xfe4f),
        (0xff00, 0xff60),
        (0xffe0, 0xffe6),
        (0x1f300, 0x1faff),
        (0x20000, 0x3fffd),
    ];
    if wide.iter().any(|&(a, b)| cp >= a && cp <= b) { 2 } else { 1 }
}

pub fn cell_width(text: &str) -> usize {
    text.chars().map(cells_of).sum()
}

/// Cuts `name` to at most `max` cells, ending in "…".
pub fn truncate(name: &str, max: usize) -> String {
    if cell_width(name) <= max {
        return name.to_string();
    }
    let budget = max.saturating_sub(1);
    let mut out = String::new();
    let mut used = 0;
    for ch in name.chars() {
        let c = cells_of(ch);
        if used + c > budget {
            break;
        }
        out.push(ch);
        used += c;
    }
    out.push(ELLIPSIS);
    out
}

/// Widths of a name and of its truncated label, without building the label: most names in a
/// large folder are measured for the column width but never placed.
struct Measure {
    cells: usize,
    label_cells: usize,
}

fn measure(n: &Node, max: usize) -> Measure {
    let cells = cell_width(&n.name);
    if cells <= max {
        return Measure { cells, label_cells: cells };
    }
    let budget = max.saturating_sub(1);
    let mut used = 0;
    for ch in n.name.chars() {
        let c = cells_of(ch);
        if used + c > budget {
            break;
        }
        used += c;
    }
    Measure { cells, label_cells: used + 1 }
}

pub struct Desired {
    pub top: f32,
    pub rows: usize,
    pub parent_y: f32,
}

/// Places groups (desired top per group, in order) so none overlap, keeping the anchor group
/// (closest parent to the focus row) where it wants to be.
pub fn stack_groups(desired: &[Desired], row_h: f32, gap_rows: f32) -> Vec<f32> {
    let mut tops: Vec<f32> = desired.iter().map(|d| d.top).collect();
    if desired.is_empty() {
        return tops;
    }
    let mut anchor = 0;
    for (i, d) in desired.iter().enumerate() {
        if d.parent_y.abs() < desired[anchor].parent_y.abs() {
            anchor = i;
        }
    }
    let gap = gap_rows * row_h;
    for i in anchor + 1..tops.len() {
        let prev_bottom = tops[i - 1] + desired[i - 1].rows as f32 * row_h;
        tops[i] = tops[i].max(prev_bottom + gap);
    }
    for i in (0..anchor).rev() {
        let max_bottom = tops[i + 1] - gap;
        tops[i] = tops[i].min(max_bottom - desired[i].rows as f32 * row_h);
    }
    tops
}

/// The child that ArrowRight lands on: the one sitting on the parent's row in the preview.
pub fn entry_child_index(count: usize) -> usize {
    count / 2
}

pub fn compute_layout(model: &TreeModel, selected_id: &str, m: &Metrics, colors: &Colors, now_ms: f64) -> Layout {
    let mut out = Layout { selected_id: selected_id.to_string(), ..Default::default() };
    let path = model.path_to(selected_id);
    if path.is_empty() {
        return out;
    }

    // Only rows near the focus row are materialized (the camera never leaves it vertically);
    // column widths still account for every name.
    let reach = m.window_rows * m.row_h;
    let box_cells = |n: &Node, ms: &Measure| {
        if n.id != selected_id {
            return ms.label_cells + m.cell_pad;
        }
        if ms.cells > m.max_chars { ms.cells } else { ms.cells + m.cell_pad }
    };
    let place = |nodes: &mut HashMap<String, LayoutNode>, n: &Node, ms: Measure, x: f32, y: f32, col: usize, hue: Hue, on_path: bool| {
        let selected = n.id == selected_id;
        let box_w = box_cells(n, &ms) as f32 * m.char_w;
        let (label, w) = if selected || ms.cells <= m.max_chars { (n.name.clone(), ms.cells) } else { (truncate(&n.name, m.max_chars), ms.label_cells) };
        let t = colors.age_t(n.mtime, now_ms);
        nodes.insert(
            n.id.clone(),
            LayoutNode {
                id: n.id.clone(),
                parent_id: n.parent_id.clone().unwrap_or_default(),
                cut: label != n.name,
                label,
                x,
                y,
                w: w as f32 * m.char_w,
                box_w,
                col,
                hue,
                color: colors.color_for(&n.name, n.is_dir, n.is_hidden, t, hue),
                is_dir: n.is_dir,
                selected,
                on_path,
            },
        );
    };

    // Selection path columns. The selection column is measured without its truncated (long)
    // names: those overhang into the preview column and only push the groups beside them.
    let mut x = 0.0;
    for (col, pid) in path.iter().enumerate() {
        let sibs = model.siblings(pid);
        let at = sibs.iter().position(|s| s.id == *pid).unwrap_or(0);
        let last = col == path.len() - 1;
        let mut ids = Vec::new();
        let mut w: f32 = 0.0;
        let mut compact: f32 = 0.0;
        for (j, s) in sibs.iter().enumerate() {
            let ms = measure(s, m.max_chars);
            let bw = box_cells(s, &ms) as f32 * m.char_w;
            w = w.max(bw);
            if s.id == selected_id || ms.cells <= m.max_chars {
                compact = compact.max(bw);
            }
            let y = (j as f32 - at as f32) * m.row_h;
            if y.abs() > reach && s.id != *pid {
                continue;
            }
            place(&mut out.nodes, s, ms, x, y, col, Hue::Path, s.id == *pid);
            ids.push(s.id.clone());
        }
        let width = if last && compact > 0.0 { compact } else { w };
        out.columns.push(Column { x, w: width, ids });
        if col > 0 {
            out.path_edges.push((path[col - 1].clone(), pid.clone()));
        }
        x += width + if last { m.preview_gap } else { m.path_gap };
    }

    // Preview columns.
    let Some(sel) = model.get(selected_id) else { return out };
    let level1: Vec<String> = out.columns.last().unwrap().ids.iter().filter(|id| model.get(id).is_some_and(|n| n.is_dir)).cloned().collect();
    let level2: Vec<String> = if sel.is_dir {
        model.children(selected_id).unwrap_or_default().into_iter().filter(|c| c.is_dir).map(|c| c.id.clone()).collect()
    } else {
        Vec::new()
    };
    for parents in [level1, level2] {
        let col = out.columns.len();
        let gap = if col == path.len() { m.preview_gap } else { m.path_gap };
        let with_kids: Vec<(String, Vec<&Node>, f32)> = parents
            .iter()
            .filter_map(|pid| {
                let kids = model.children(pid).unwrap_or_default();
                let parent = out.nodes.get(pid)?;
                (!kids.is_empty()).then(|| (pid.clone(), kids, parent.y))
            })
            .collect();
        if with_kids.is_empty() {
            break;
        }
        let desired: Vec<Desired> = with_kids
            .iter()
            .map(|(_, kids, py)| Desired { top: py - (kids.len() as f32 - 1.0) * m.row_h / 2.0, rows: kids.len(), parent_y: *py })
            .collect();
        let tops = stack_groups(&desired, m.row_h, m.group_gap_rows);
        // Names of the previous column that overhang this column's left edge.
        let overhang: Vec<(String, f32, f32)> = out.columns[col - 1]
            .ids
            .iter()
            .filter_map(|id| out.nodes.get(id))
            .filter(|n| n.x + n.box_w + gap > x)
            .map(|n| (n.id.clone(), n.y, n.x + n.box_w + gap))
            .collect();
        let mut ids = Vec::new();
        let mut w: f32 = 0.0;
        for (gi, (pid, kids, _)) in with_kids.iter().enumerate() {
            let top = tops[gi];
            let bottom = top + kids.len() as f32 * m.row_h;
            let mut gx = x;
            for (oid, oy, right) in &overhang {
                if (oy + m.row_h > top && *oy < bottom) || oid == pid {
                    gx = gx.max(*right);
                }
            }
            out.groups.push(Group {
                parent_id: pid.clone(),
                col,
                top,
                rows: kids.len(),
                first_id: kids[0].id.clone(),
                last_id: kids[kids.len() - 1].id.clone(),
            });
            for (j, k) in kids.iter().enumerate() {
                let ms = measure(k, m.max_chars);
                if gx == x {
                    w = w.max(box_cells(k, &ms) as f32 * m.char_w);
                }
                let y = top + j as f32 * m.row_h;
                if y.abs() > reach && j != 0 && j != kids.len() - 1 {
                    continue;
                }
                place(&mut out.nodes, k, ms, gx, y, col, Hue::Preview, false);
                ids.push(k.id.clone());
            }
        }
        out.columns.push(Column { x, w, ids });
        x += w + m.path_gap;
    }

    // Widen preview gaps that are too narrow for their connector lanes.
    let over = lane_overflow(&out, m);
    if !over.is_empty() {
        let shift_for = |col: usize| over.iter().filter(|(c, _)| **c <= col).map(|(_, px)| *px).sum::<f32>();
        for n in out.nodes.values_mut() {
            n.x += shift_for(n.col);
        }
        for (i, c) in out.columns.iter_mut().enumerate() {
            c.x += shift_for(i);
        }
    }
    out.min_x = 0.0;
    out.max_x = out.nodes.values().map(|n| n.x + n.box_w).fold(0.0, f32::max);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncation_counts_cells() {
        assert_eq!(truncate("short", 25), "short");
        let long = "a".repeat(30);
        let t = truncate(&long, 25);
        assert_eq!(cell_width(&t), 25);
        assert!(t.ends_with(ELLIPSIS));
        assert_eq!(cell_width("日本"), 4);
    }

    #[test]
    fn stacking_keeps_anchor() {
        let d = vec![
            Desired { top: -16.0, rows: 3, parent_y: -16.0 },
            Desired { top: 0.0, rows: 3, parent_y: 0.0 },
            Desired { top: 16.0, rows: 3, parent_y: 16.0 },
        ];
        let tops = stack_groups(&d, 16.0, 1.0);
        assert_eq!(tops[1], 0.0);
        assert_eq!(tops[2], 64.0);
        assert_eq!(tops[0], -64.0);
    }
}
