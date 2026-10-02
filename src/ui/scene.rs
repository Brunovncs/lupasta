//! The scene: names and connectors at their interpolated positions. The canvas is redrawn every
//! animation frame; routing runs on the interpolated boxes, so moving the selection animates the
//! whole scene instead of re-rendering it.

use super::frame::{Frame, Item, hsla};
use lupasta::animator::{Animator, Cam};
use lupasta::layout::Layout;
use lupasta::metrics::Metrics;
use lupasta::palette::{CONNECTOR, PATH_CONNECTOR, SELECTED};
use lupasta::router::{Box, Route, RouteKind, rounded, route_layout};
use std::collections::HashSet;

/// Camera for a layout in a viewport of `vw`×`vh` layout px: the bounding box is centred
/// (biased right like the reference) and the selection row sits on the focus line.
pub fn camera_for(layout: &Layout, vw: f32, vh: f32, m: &Metrics, dpr: f32) -> Cam {
    let snap = |v: f32| (v * dpr).round() / dpr;
    Cam { x: snap(vw / 2.0 + m.camera_bias_x - (layout.min_x + layout.max_x) / 2.0), y: snap(vh * m.focus_y) }
}

fn routes_for(layout: &Layout, anim: &Animator, m: &Metrics) -> Vec<Route> {
    route_layout(layout, &|id| anim.tracks.get(id).map(|t| Box { x: t.cur.x, y: t.cur.y, w: t.node.w }), m, None)
}

pub struct SceneView<'a> {
    pub anim: &'a Animator,
    pub layout: Option<&'a Layout>,
    /// The layout being left, whose connectors fade out while the animation runs.
    pub prev: Option<&'a Layout>,
    pub progress: f32,
    pub m: &'a Metrics,
    pub zoom: f32,
    /// Viewport in layout px (window px / zoom).
    pub vw: f32,
    pub vh: f32,
    /// Multiplies every alpha (the scene recedes behind search and settings).
    pub alpha: f32,
}

impl SceneView<'_> {
    fn visible(&self, x: f32, y: f32, w: f32) -> bool {
        let (cam, o) = (self.anim.cam, self.m.overscan);
        let (sx, sy) = (x + cam.x, y + cam.y);
        sx + w > -o && sx < self.vw + o && sy + self.m.row_h > -o && sy < self.vh + o
    }

    pub fn paint(&self, frame: &mut Frame) {
        let (m, z, cam) = (self.m, self.zoom, self.anim.cam);
        let sx = |x: f32| (x + cam.x) * z;
        let sy = |y: f32| (y + cam.y) * z;

        if let Some(layout) = self.layout {
            let current = routes_for(layout, self.anim, m);
            let ids: HashSet<&str> = current.iter().map(|r| r.id.as_str()).collect();
            let fading = match self.prev {
                Some(p) if self.anim.running => routes_for(p, self.anim, m).into_iter().filter(|r| !ids.contains(r.id.as_str())).collect(),
                _ => Vec::new(),
            };
            for (r, opacity) in current.iter().map(|r| (r, 1.0)).chain(fading.iter().map(|r| (r, 1.0 - self.progress))) {
                // Like the reference: a connector whose target lies beyond the screen collapses to a dot.
                let end_y = r.points.last().map_or(0.0, |p| p.1) + cam.y;
                let offscreen = r.kind == RouteKind::Preview && (end_y < -m.row_h || end_y > self.vh + m.row_h);
                if offscreen {
                    let (x, y) = r.points[0];
                    frame.items.push(Item::Dot { x: sx(x + m.dot_r), y: sy(y), r: m.dot_r * z, color: hsla(CONNECTOR, opacity * self.alpha) });
                    continue;
                }
                let screen: Vec<(f32, f32)> = r.points.iter().map(|&(x, y)| (sx(x), sy(y))).collect();
                let (start, steps) = rounded(&screen, m.radius * z);
                let (color, width) = match r.kind {
                    RouteKind::Path => (PATH_CONNECTOR, m.path_stroke_w),
                    RouteKind::Preview => (CONNECTOR, m.stroke_w),
                };
                frame.items.push(Item::Wire { start, steps, width: width * z, color: hsla(color, opacity * self.alpha) });
            }

            if let Some(sel) = self.anim.tracks.get(&layout.selected_id) {
                frame.rect(sx(sel.cur.x + m.caret_x), sy(sel.cur.y), m.caret_w * z, m.row_h * z, hsla(SELECTED, self.alpha));
            }
        }

        for t in self.anim.tracks.values() {
            if t.cur.o <= 0.001 || !self.visible(t.cur.x, t.cur.y, t.node.w) {
                continue;
            }
            frame.text(sx(t.cur.x), sy(t.cur.y), t.node.label.clone(), hsla(t.node.color, t.cur.o * self.alpha));
        }
    }

    /// The name under a window-px point, if any.
    pub fn hit(&self, x: f32, y: f32) -> Option<String> {
        let (m, z, cam) = (self.m, self.zoom, self.anim.cam);
        let (lx, ly) = (x / z - cam.x, y / z - cam.y);
        self.anim
            .tracks
            .values()
            .filter(|t| !t.exiting && t.cur.o > 0.5)
            .find(|t| lx >= t.cur.x && lx < t.cur.x + t.node.w.max(m.char_w) && ly >= t.cur.y && ly < t.cur.y + m.row_h)
            .map(|t| t.node.id.clone())
    }
}
