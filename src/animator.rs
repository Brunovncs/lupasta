//! Interpolates between successive layouts. Existing nodes slide from wherever they are (even
//! mid-flight) to their new slot; new nodes emerge from their parent; removed nodes retreat into
//! their parent and fade. The camera eases with the same clock.

use crate::layout::{Layout, LayoutNode};
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Vis {
    pub x: f32,
    pub y: f32,
    pub o: f32,
}

#[derive(Debug, Clone)]
pub struct Track {
    pub node: LayoutNode,
    pub from: Vis,
    pub to: Vis,
    pub cur: Vis,
    pub exiting: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Cam {
    pub x: f32,
    pub y: f32,
}

pub fn ease_out_cubic(t: f32) -> f32 {
    1.0 - (1.0 - t).powi(3)
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

#[derive(Debug, Default)]
pub struct Animator {
    pub tracks: HashMap<String, Track>,
    pub cam: Cam,
    cam_from: Cam,
    cam_to: Cam,
    /// Clock origin of the current transition, in ms.
    start: f64,
    started: bool,
    pub running: bool,
    pub duration: f32,
}

impl Animator {
    pub fn new(duration: f32) -> Animator {
        Animator { duration, ..Default::default() }
    }

    pub fn set_target(&mut self, layout: &Layout, cam: Cam, now: f64, instant: bool) {
        let instant = instant || !self.started || self.duration <= 0.0;
        self.started = true;
        for ln in layout.nodes.values() {
            let to = Vis { x: ln.x, y: ln.y, o: 1.0 };
            if let Some(t) = self.tracks.get_mut(&ln.id) {
                t.node = ln.clone();
                t.from = t.cur;
                t.to = to;
                t.exiting = false;
            } else {
                let from = match self.tracks.get(&ln.parent_id) {
                    Some(p) if !p.exiting => Vis { x: p.cur.x, y: p.cur.y, o: 0.0 },
                    _ => Vis { o: 0.0, ..to },
                };
                self.tracks.insert(ln.id.clone(), Track { node: ln.clone(), from, to, cur: from, exiting: false });
            }
        }
        for (id, t) in self.tracks.iter_mut() {
            if layout.nodes.contains_key(id) {
                continue;
            }
            t.exiting = true;
            t.from = t.cur;
            t.to = match layout.nodes.get(&t.node.parent_id) {
                Some(p) => Vis { x: p.x, y: p.y, o: 0.0 },
                None => Vis { o: 0.0, ..t.cur },
            };
        }
        self.cam_from = self.cam;
        self.cam_to = cam;
        self.start = now;
        self.running = true;
        if instant {
            self.start = now - self.duration.max(1.0) as f64;
            self.tick(now);
        }
    }

    /// Moves the camera target without starting a transition (window resize).
    pub fn snap_camera(&mut self, cam: Cam) {
        self.cam = cam;
        self.cam_to = cam;
        self.cam_from = cam;
    }

    fn raw(&self, now: f64) -> f32 {
        if self.duration <= 0.0 { 1.0 } else { (((now - self.start) / self.duration as f64) as f32).clamp(0.0, 1.0) }
    }

    /// Advances to `now` (ms). Returns true while still animating.
    pub fn tick(&mut self, now: f64) -> bool {
        let raw = self.raw(now);
        let e = ease_out_cubic(raw);
        self.tracks.retain(|_, t| {
            t.cur.x = lerp(t.from.x, t.to.x, e);
            t.cur.y = lerp(t.from.y, t.to.y, e);
            t.cur.o = lerp(t.from.o, t.to.o, e);
            !(raw >= 1.0 && t.exiting)
        });
        self.cam.x = lerp(self.cam_from.x, self.cam_to.x, e);
        self.cam.y = lerp(self.cam_from.y, self.cam_to.y, e);
        self.running = raw < 1.0;
        self.running
    }

    pub fn progress(&self, now: f64) -> f32 {
        ease_out_cubic(self.raw(now))
    }
}
