//! Orthogonal connector routing. Input is plain geometry (so it can run on interpolated positions
//! every animation frame); output is polylines made of H/V segments only.
//!
//! Preview connectors attach to the child row nearest the parent (straight when the group spans
//! the parent row). The vertical run hugs the parent when nothing is in the way, otherwise it
//! runs next to the child column in a lane: lanes stack outward so edges that travel further sit
//! further left, which keeps nested edges from crossing.

use crate::layout::Layout;
use crate::metrics::Metrics;
use std::collections::{BTreeMap, HashMap};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Box {
    pub x: f32,
    /// Row top.
    pub y: f32,
    pub w: f32,
}

#[derive(Debug, Clone)]
pub struct GroupGeom {
    pub id: String,
    pub level: usize,
    pub parent: Box,
    pub first: Box,
    pub last: Box,
    /// Other names in the parent's column.
    pub siblings: Vec<Box>,
    /// Every name in the children's column.
    pub targets: Vec<Box>,
    /// The column's left edge (a group beside an overhanging name sits right of it).
    pub base_x: f32,
}

pub type Point = (f32, f32);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RouteKind {
    Path,
    Preview,
}

#[derive(Debug, Clone)]
pub struct Route {
    pub id: String,
    pub kind: RouteKind,
    pub points: Vec<Point>,
    pub span_top: f32,
    pub span_bottom: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Mode {
    Straight,
    Near,
    Far,
}

#[derive(Debug, Clone, Copy)]
struct Detour {
    y: f32,
    xe: f32,
}

#[derive(Debug, Clone)]
struct Pending {
    g: usize,
    xs: f32,
    xe: f32,
    yp: f32,
    yt: f32,
    mode: Mode,
    xv: f32,
    lane: usize,
    /// Groups pushed right by an overhanging name: run in the column's lanes, then detour along
    /// the blank row next to the group (always free: groups are stacked one row apart).
    detour: Option<Detour>,
    /// Lanes blocked by an overhanging name: drop down beside it and land on the group's edge.
    landing: Option<Vec<Point>>,
}

impl Pending {
    fn span(&self) -> (f32, f32) {
        (self.yp.min(self.yt), self.yp.max(self.yt))
    }

    fn build(&self) -> Vec<Point> {
        if self.mode == Mode::Straight {
            return vec![(self.xs, self.yp), (self.xe, self.yp)];
        }
        if let Some(l) = &self.landing {
            return l.clone();
        }
        if let Some(d) = self.detour {
            return vec![(self.xs, self.yp), (self.xv, self.yp), (self.xv, d.y), (d.xe, d.y)];
        }
        vec![(self.xs, self.yp), (self.xv, self.yp), (self.xv, self.yt), (self.xe, self.yt)]
    }
}

const EPS: f32 = 0.5;

pub fn route_path(id: String, from: Box, to: Box, m: &Metrics) -> Route {
    let y = from.y + m.connector_y;
    let ty = to.y + m.connector_y;
    let xs = from.x + from.w + m.conn_gap_l;
    let xe = to.x - m.conn_gap_r;
    let points = if (ty - y).abs() < EPS {
        vec![(xs, y), (xe, y)]
    } else {
        let mid = (xs + xe) / 2.0;
        vec![(xs, y), (mid, y), (mid, ty), (xe, ty)]
    };
    Route { id, kind: RouteKind::Path, points, span_top: to.y, span_bottom: to.y + m.row_h }
}

/// The furthest right a vertical may sit left of: past every sibling whose glyph band it touches.
fn sibling_limit(p: &Pending, g: &GroupGeom, m: &Metrics) -> f32 {
    let (lo, hi) = p.span();
    let mut limit = p.xs + m.min_run;
    for o in &g.siblings {
        if o.y + 1.0 < hi && o.y + m.row_h - 1.0 > lo {
            limit = limit.max(o.x + o.w + m.conn_gap_l);
        }
    }
    limit
}

#[allow(clippy::needless_range_loop)] // passes mutate one entry while reading the others
pub fn route_groups(groups: &[GroupGeom], m: &Metrics, mut overflow: Option<&mut BTreeMap<usize, f32>>) -> Vec<Route> {
    let mut by_level: BTreeMap<usize, Vec<Pending>> = BTreeMap::new();
    for (gi, g) in groups.iter().enumerate() {
        let yp = g.parent.y + m.connector_y;
        let y_first = g.first.y + m.connector_y;
        let y_last = g.last.y + m.connector_y;
        let xs = g.parent.x + g.parent.w + m.conn_gap_l;
        let xe = g.first.x - m.conn_gap_r;
        let mut p = Pending { g: gi, xs, xe, yp, yt: yp, mode: Mode::Straight, xv: xe, lane: 0, detour: None, landing: None };
        // Detour only when the group was pushed by someone else's name (an overhanging parent
        // just routes normally toward its own, shifted, group).
        let shifted = g.first.x > g.base_x + EPS && xs < g.base_x - m.conn_gap_r - m.near_child;
        let out_of_span = yp < y_first - EPS || yp > y_last + EPS;
        if out_of_span && shifted {
            let from_above = yp < y_first;
            p.xe = g.base_x - m.conn_gap_r;
            // Run along the group's outer edge (bottom of the blank row next to it) into its first name.
            p.yt = if from_above { g.first.y - m.land_gap } else { g.last.y + m.row_h + m.land_gap };
            p.detour = Some(Detour { y: p.yt, xe });
            p.mode = Mode::Far;
        } else if out_of_span {
            p.yt = if yp < y_first { y_first - m.enter_offset } else { y_last + m.enter_offset };
            p.xv = xs + m.near_parent;
            let (lo, hi) = p.span();
            // Any name whose glyph band the vertical would touch, and that reaches past it.
            let blocked = g.siblings.iter().any(|o| o.y + 1.0 < hi && o.y + m.row_h - 1.0 > lo && o.x + o.w + m.conn_gap_l > p.xv - EPS);
            p.mode = if blocked { Mode::Far } else { Mode::Near };
        }
        by_level.entry(g.level).or_default().push(p);
    }

    let mut out = Vec::new();
    for (level, mut list) in by_level {
        for _pass in 0..8 {
            let mut changed = false;
            // A near run is invalid if another edge's first horizontal passes through it.
            for i in 0..list.len() {
                if list[i].mode != Mode::Near {
                    continue;
                }
                let (lo, hi) = list[i].span();
                let xv = list[i].xv;
                let hit = list.iter().enumerate().any(|(j, q)| {
                    j != i && q.yp > lo + EPS && q.yp < hi - EPS && (if q.mode == Mode::Near { q.xv } else { q.xe }) > xv - EPS
                });
                if hit {
                    list[i].mode = Mode::Far;
                    changed = true;
                }
            }
            let mut up: Vec<usize> = (0..list.len()).filter(|&i| list[i].mode == Mode::Far && list[i].yt < list[i].yp).collect();
            up.sort_by(|&a, &b| list[b].yp.total_cmp(&list[a].yp));
            assign_lanes(&mut list, &up);
            let mut down: Vec<usize> = (0..list.len()).filter(|&i| list[i].mode == Mode::Far && list[i].yt > list[i].yp).collect();
            down.sort_by(|&a, &b| list[a].yp.total_cmp(&list[b].yp));
            assign_lanes(&mut list, &down);
            for p in list.iter_mut() {
                if p.mode == Mode::Far {
                    p.xv = p.xe - m.near_child - p.lane as f32 * m.lane_gap;
                }
            }
            // When the parent is the column's widest name, "next to the parent" lands inside the
            // lane zone; a near run to the right of an overlapping lane would cut through it.
            for i in 0..list.len() {
                if list[i].mode != Mode::Near {
                    continue;
                }
                let (lo, hi) = list[i].span();
                let xv = list[i].xv;
                let hit = list.iter().any(|q| {
                    let (qlo, qhi) = q.span();
                    q.mode == Mode::Far && q.xv < xv + EPS && qlo < hi - EPS && lo < qhi - EPS
                });
                if hit {
                    list[i].mode = Mode::Far;
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }
        // Lanes that do not fit between the names they pass and the children. First choice (as
        // in the reference): run down in the free space right of the overhanging name and land on
        // the group's top/bottom edge. Otherwise report how much wider the gap must be (the
        // layout widens it) and clamp meanwhile.
        let mut needs: HashMap<usize, f32> = HashMap::new();
        for i in 0..list.len() {
            let p = &list[i];
            if p.mode != Mode::Far || p.detour.is_some() {
                continue;
            }
            let limit = sibling_limit(p, &groups[p.g], m);
            let need = limit - p.xv;
            if need <= 0.0 {
                continue;
            }
            match landing_route(p, &groups[p.g], limit, m) {
                Some(land) => list[i].landing = Some(land),
                None => {
                    needs.insert(i, need);
                }
            }
        }
        // A landing that crosses another connector falls back to widening.
        for i in 0..list.len() {
            let Some(land) = list[i].landing.clone() else { continue };
            let mut routes = vec![Route { id: "a".into(), kind: RouteKind::Preview, points: land, span_top: 0.0, span_bottom: 0.0 }];
            for (j, q) in list.iter().enumerate() {
                if j != i {
                    routes.push(Route { id: "b".into(), kind: RouteKind::Preview, points: q.build(), span_top: 0.0, span_bottom: 0.0 });
                }
            }
            if !crossings(&routes).is_empty() {
                list[i].landing = None;
                let limit = sibling_limit(&list[i], &groups[list[i].g], m);
                needs.insert(i, limit - list[i].xv);
            }
        }
        for (i, need) in needs {
            if let Some(o) = overflow.as_deref_mut() {
                let e = o.entry(level).or_insert(0.0);
                *e = e.max(need);
            }
            list[i].xv += need;
        }
        for p in &list {
            let g = &groups[p.g];
            out.push(Route { id: g.id.clone(), kind: RouteKind::Preview, points: p.build(), span_top: g.first.y, span_bottom: g.last.y + m.row_h });
        }
    }
    out
}

/// Vertical just right of the names in the way, ending with a short hook on the group's edge.
fn landing_route(p: &Pending, g: &GroupGeom, limit: f32, m: &Metrics) -> Option<Vec<Point>> {
    let x = limit + m.near_parent - m.conn_gap_l;
    let down = p.yt > p.yp;
    let end = g
        .targets
        .iter()
        .filter(|b| b.y > g.first.y - EPS && b.y < g.last.y + EPS)
        .map(|b| b.x + b.w)
        .fold(f32::NEG_INFINITY, f32::max);
    if !(x > g.first.x + m.char_w && x < end - m.char_w) {
        return None;
    }
    let y_edge = if down { g.first.y - m.land_gap } else { g.last.y + m.row_h + m.land_gap };
    let pts = vec![(p.xs, p.yp), (x, p.yp), (x, y_edge), (x - m.hook, y_edge)];
    let boxes: Vec<Box> = g.siblings.iter().chain(&g.targets).copied().collect();
    if text_hits(&pts, &boxes, m) { None } else { Some(pts) }
}

fn assign_lanes(list: &mut [Pending], ordered: &[usize]) {
    let mut done: Vec<usize> = Vec::new();
    for &i in ordered {
        let (lo, hi) = list[i].span();
        let mut lane = 0;
        for &j in &done {
            let (qlo, qhi) = list[j].span();
            if qlo < hi + EPS && lo < qhi + EPS {
                lane = lane.max(list[j].lane + 1);
            }
        }
        list[i].lane = lane;
        done.push(i);
    }
}

/// One step of a rounded-elbow polyline: a straight line to `to`, or a quadratic curve to `to`
/// with control point `ctrl`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Step {
    Line(Point),
    Quad { ctrl: Point, to: Point },
}

/// Polyline with rounded elbows: the start point, then the steps to draw.
pub fn rounded(points: &[Point], radius: f32) -> (Point, Vec<Step>) {
    let mut steps = Vec::new();
    if points.len() < 2 {
        return (points.first().copied().unwrap_or_default(), steps);
    }
    for i in 1..points.len() - 1 {
        let (px, py) = points[i - 1];
        let (cx, cy) = points[i];
        let (nx, ny) = points[i + 1];
        let in_len = (cx - px).hypot(cy - py);
        let out_len = (nx - cx).hypot(ny - cy);
        let r = radius.min(in_len / 2.0).min(out_len / 2.0);
        if r < 0.01 {
            steps.push(Step::Line((cx, cy)));
            continue;
        }
        let a = (cx - (cx - px) / in_len * r, cy - (cy - py) / in_len * r);
        let b = (cx + (nx - cx) / out_len * r, cy + (ny - cy) / out_len * r);
        steps.push(Step::Line(a));
        steps.push(Step::Quad { ctrl: (cx, cy), to: b });
    }
    steps.push(Step::Line(points[points.len() - 1]));
    (points[0], steps)
}

/// True when every segment is axis-aligned (used by tests).
pub fn is_orthogonal(points: &[Point]) -> bool {
    points.windows(2).all(|w| (w[1].0 - w[0].0).abs() <= 1e-4 || (w[1].1 - w[0].1).abs() <= 1e-4)
}

/// Routes every connector of a layout, reading (possibly interpolated) boxes through `bx`.
pub fn route_layout(layout: &Layout, bx: &dyn Fn(&str) -> Option<Box>, m: &Metrics, overflow: Option<&mut BTreeMap<usize, f32>>) -> Vec<Route> {
    let mut routes = Vec::new();
    for (from, to) in &layout.path_edges {
        if let (Some(a), Some(b)) = (bx(from), bx(to)) {
            routes.push(route_path(format!("p:{from}>{to}"), a, b, m));
        }
    }
    let first_preview_col = layout.groups.first().map_or(0, |g| g.col);
    let mut column_boxes: HashMap<usize, Vec<(&str, Box)>> = HashMap::new();
    let mut col_boxes = |col: usize| -> Vec<(&str, Box)> {
        column_boxes
            .entry(col)
            .or_insert_with(|| layout.columns.get(col).map_or(Vec::new(), |c| c.ids.iter().filter_map(|id| bx(id).map(|b| (id.as_str(), b))).collect()))
            .clone()
    };
    let mut geoms = Vec::new();
    for g in &layout.groups {
        let (Some(parent), Some(first), Some(last), Some(pn)) = (bx(&g.parent_id), bx(&g.first_id), bx(&g.last_id), layout.nodes.get(&g.parent_id)) else {
            continue;
        };
        let siblings = col_boxes(pn.col).into_iter().filter(|(id, _)| *id != g.parent_id).map(|(_, b)| b).collect();
        let base_x = match (layout.nodes.get(&g.first_id), layout.columns.get(g.col)) {
            (Some(t), Some(c)) => first.x - (t.x - c.x),
            _ => first.x,
        };
        let targets = col_boxes(g.col).into_iter().map(|(_, b)| b).collect();
        geoms.push(GroupGeom {
            id: format!("g:{}", g.parent_id),
            level: g.col - first_preview_col,
            parent,
            first,
            last,
            siblings,
            targets,
            base_x,
        });
    }
    routes.extend(route_groups(&geoms, m, overflow));
    routes
}

/// Extra horizontal room each preview column needs so its connector lanes fit (by column).
pub fn lane_overflow(layout: &Layout, m: &Metrics) -> BTreeMap<usize, f32> {
    let mut by_level = BTreeMap::new();
    route_layout(layout, &|id| layout.nodes.get(id).map(|n| Box { x: n.x, y: n.y, w: n.w }), m, Some(&mut by_level));
    let first = layout.groups.first().map_or(0, |g| g.col);
    by_level.into_iter().map(|(level, px)| (first + level, px.ceil())).collect()
}

/// Pairs of routes whose segments cross (a horizontal strictly through a vertical) or run on top
/// of each other (collinear overlap).
pub fn crossings(routes: &[Route]) -> Vec<(String, String)> {
    struct Seg<'a> {
        id: &'a str,
        h: bool,
        a: f32,
        b: f32,
        c: f32,
    }
    let mut segs = Vec::new();
    for r in routes {
        for w in r.points.windows(2) {
            let ((x1, y1), (x2, y2)) = (w[0], w[1]);
            if y1 == y2 {
                segs.push(Seg { id: &r.id, h: true, a: x1.min(x2), b: x1.max(x2), c: y1 });
            } else {
                segs.push(Seg { id: &r.id, h: false, a: y1.min(y2), b: y1.max(y2), c: x1 });
            }
        }
    }
    let mut out = Vec::new();
    for s in &segs {
        for t in &segs {
            if s.id == t.id {
                continue;
            }
            if s.h && !t.h && t.c > s.a + 0.5 && t.c < s.b - 0.5 && s.c > t.a + 0.5 && s.c < t.b - 0.5 {
                out.push((s.id.to_string(), t.id.to_string()));
            }
            if s.h == t.h && s.id < t.id && (s.c - t.c).abs() < 1.0 && s.b.min(t.b) - s.a.max(t.a) > 1.0 {
                out.push((s.id.to_string(), t.id.to_string()));
            }
        }
    }
    out
}

/// True when a segment of `points` runs through a name's glyphs.
pub fn text_hits(points: &[Point], boxes: &[Box], m: &Metrics) -> bool {
    points.windows(2).any(|w| {
        let ((x1, y1), (x2, y2)) = (w[0], w[1]);
        let (ax, bx) = (x1.min(x2), x1.max(x2));
        let (ay, by) = (y1.min(y2), y1.max(y2));
        boxes.iter().any(|b| ax < b.x + b.w - 1.0 && bx > b.x + 1.0 && ay < b.y + m.row_h - 1.0 && by > b.y + 1.0)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_route_is_z_shaped() {
        let m = Metrics::default();
        let r = route_path("p".into(), Box { x: 0.0, y: 0.0, w: 50.0 }, Box { x: 80.0, y: 32.0, w: 30.0 }, &m);
        assert_eq!(r.points.len(), 4);
        assert!(is_orthogonal(&r.points));
    }

    #[test]
    fn rounded_elbows_end_on_the_last_point() {
        let (start, steps) = rounded(&[(0.0, 0.0), (10.0, 0.0), (10.0, 20.0)], 5.0);
        assert_eq!(start, (0.0, 0.0));
        assert_eq!(steps.last(), Some(&Step::Line((10.0, 20.0))));
        assert!(matches!(steps[1], Step::Quad { .. }));
    }
}
