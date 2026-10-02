//! Navigation state: selection, lazy loading and keyboard/mouse intent. Owns the TreeModel and
//! produces layouts. No IO: whatever must be listed is returned to the caller, which lists it
//! off the UI thread and hands the listings back.

use crate::layout::{Layout, compute_layout, entry_child_index};
use crate::metrics::Metrics;
use crate::palette::Colors;
use crate::tree::{Listing, Node, TreeModel, parent_of};
use std::collections::{HashMap, HashSet};

/// What a navigation step needs before it can finish.
#[derive(Debug, Clone, PartialEq)]
pub enum Need {
    Nothing,
    /// List this folder, then call `entered`.
    Listing(String),
    /// Open this file with the default application.
    Open(String),
    /// Left arrow on a top-level entry: re-root one folder up.
    LeaveRoot,
}

#[derive(Debug, Default)]
pub struct Controller {
    pub model: TreeModel,
    pub selected_id: String,
    pub layout: Option<Layout>,
    last_child: HashMap<String, String>,
    inflight: HashSet<String>,
    wheel_acc: f32,
    pub wheel_step: f32,
    /// Bumped by `reset`: answers to requests made for a previous root are dropped.
    pub epoch: u64,
}

impl Controller {
    pub fn new() -> Controller {
        Controller { wheel_step: 40.0, ..Default::default() }
    }

    /// Starts over on a new root with its top-level listing.
    pub fn reset(&mut self, root_name: &str, top: &Listing) {
        self.epoch += 1;
        self.model = TreeModel::new(root_name);
        self.selected_id.clear();
        self.layout = None;
        self.last_child.clear();
        self.inflight.clear();
        self.model.ingest(top);
    }

    /// Selects the default top-level entry (the middle one, as ArrowRight would).
    pub fn select_default(&mut self) -> bool {
        let kids = self.model.children("").unwrap_or_default();
        if kids.is_empty() {
            return false;
        }
        let id = kids[entry_child_index(kids.len())].id.clone();
        self.select(&id)
    }

    pub fn ingest(&mut self, listings: &[Listing]) -> bool {
        let mut changed = false;
        for l in listings {
            changed |= self.model.ingest(l);
        }
        changed
    }

    pub fn relayout(&mut self, m: &Metrics, colors: &Colors, now_ms: f64) -> Option<&Layout> {
        if self.selected_id.is_empty() {
            return None;
        }
        self.layout = Some(compute_layout(&self.model, &self.selected_id, m, colors, now_ms));
        self.layout.as_ref()
    }

    /// Changes the selection; the caller relayouts. False when nothing changed.
    pub fn select(&mut self, id: &str) -> bool {
        if id.is_empty() || self.model.get(id).is_none() {
            return false;
        }
        self.selected_id = id.to_string();
        self.last_child.insert(parent_of(id).to_string(), id.to_string());
        true
    }

    pub fn selected(&self) -> Option<&Node> {
        self.model.get(&self.selected_id)
    }

    /// Folders the preview columns need that are neither loaded nor being loaded; marks them in
    /// flight. Hand the result of listing them to `previews_loaded`.
    pub fn take_missing_previews(&mut self) -> Vec<String> {
        let missing: Vec<String> = self.model.missing_for_preview(&self.selected_id).into_iter().filter(|p| !self.inflight.contains(p)).collect();
        self.inflight.extend(missing.iter().cloned());
        missing
    }

    /// Ingests preview listings; folders that came back missing are unreadable. True when the
    /// layout must be recomputed.
    pub fn previews_loaded(&mut self, requested: &[String], listings: &[Listing]) -> bool {
        for p in requested {
            self.inflight.remove(p);
        }
        let got: HashSet<&str> = listings.iter().map(|l| l.path.as_str()).collect();
        for p in requested {
            if !got.contains(p.as_str()) {
                self.model.mark_unreadable(p);
            }
        }
        self.ingest(listings)
    }

    /// Ingests the chain returned for a reveal and selects `path`.
    pub fn revealed(&mut self, path: &str, chain: &[Listing]) -> bool {
        self.ingest(chain);
        self.select(path)
    }

    pub fn move_by(&mut self, delta: i64) -> bool {
        let sibs = self.model.siblings(&self.selected_id);
        let Some(i) = sibs.iter().position(|s| s.id == self.selected_id) else { return false };
        let j = (i as i64).saturating_add(delta).clamp(0, sibs.len() as i64 - 1) as usize;
        if j == i {
            return false;
        }
        let id = sibs[j].id.clone();
        self.select(&id)
    }

    /// ArrowRight / Space. The selection moves only when the folder is already loaded.
    pub fn enter(&mut self) -> Need {
        let Some(node) = self.model.get(&self.selected_id) else { return Need::Nothing };
        if !node.is_dir {
            return Need::Nothing;
        }
        if !node.loaded {
            return Need::Listing(node.id.clone());
        }
        let id = node.id.clone();
        self.entered(&id);
        Need::Nothing
    }

    /// Finishes `enter` once `dir` is loaded (or failed to load). True when the selection moved.
    pub fn entered(&mut self, dir: &str) -> bool {
        if self.selected_id != dir {
            return false;
        }
        let kids = self.model.children(dir).unwrap_or_default();
        if kids.is_empty() {
            return false;
        }
        let target = match self.last_child.get(dir) {
            Some(r) if self.model.get(r).is_some() => r.clone(),
            _ => kids[entry_child_index(kids.len())].id.clone(),
        };
        self.select(&target)
    }

    pub fn listing_failed(&mut self, dir: &str) {
        self.model.mark_unreadable(dir);
    }

    /// ArrowLeft.
    pub fn leave(&mut self) -> Need {
        let parent = parent_of(&self.selected_id).to_string();
        if !parent.is_empty() {
            self.select(&parent);
            return Need::Nothing;
        }
        if self.selected_id.is_empty() { Need::Nothing } else { Need::LeaveRoot }
    }

    /// Enter: a folder is entered, a file is opened.
    pub fn activate(&mut self) -> Need {
        match self.model.get(&self.selected_id) {
            Some(n) if n.is_dir => self.enter(),
            Some(n) => Need::Open(n.id.clone()),
            None => Need::Nothing,
        }
    }

    /// Wheel delta in px; returns true when the selection moved.
    pub fn wheel(&mut self, delta_y: f32) -> bool {
        self.wheel_acc += delta_y;
        let step = self.wheel_step.max(1.0);
        let mut moved = false;
        while self.wheel_acc.abs() >= step {
            let dir = self.wheel_acc.signum();
            self.wheel_acc -= dir * step;
            moved |= self.move_by(dir as i64);
        }
        moved
    }

    /// Watcher notification: the loaded folders among `dirs`, to be listed again.
    pub fn loaded_among(&self, dirs: &[String]) -> Vec<String> {
        let mut out: Vec<String> = dirs.iter().filter(|d| self.model.get(d).is_some_and(|n| n.loaded)).cloned().collect();
        out.sort();
        out.dedup();
        out
    }

    /// Every loaded folder (after the hidden-files or sort setting changed).
    pub fn all_loaded(&self) -> Vec<String> {
        let mut out: Vec<String> = self.model.nodes().filter(|n| n.is_dir && n.loaded).map(|n| n.id.clone()).collect();
        out.sort();
        out
    }

    /// Ingests refreshed listings and repairs the selection if it vanished. True when the layout
    /// must be recomputed.
    pub fn refreshed(&mut self, listings: &[Listing]) -> bool {
        if !self.ingest(listings) {
            return false;
        }
        let mut sel = self.selected_id.clone();
        while !sel.is_empty() && self.model.get(&sel).is_none() {
            sel = parent_of(&sel).to_string();
        }
        if sel.is_empty() {
            sel = self.model.children("").and_then(|k| k.first().map(|n| n.id.clone())).unwrap_or_default();
        }
        self.selected_id = sel;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::filesystem::{Entry, FLAG_DIR};

    fn listing(path: &str, names: &[(&str, bool)]) -> Listing {
        Listing { path: path.into(), entries: names.iter().map(|(n, d)| Entry(n.to_string(), if *d { FLAG_DIR } else { 0 }, 0.0, 0)).collect() }
    }

    #[test]
    fn navigation_round_trip() {
        let mut c = Controller::new();
        c.reset("root", &listing("", &[("a", true), ("b", true), ("c", false)]));
        assert!(c.select_default());
        assert_eq!(c.selected_id, "b");
        assert_eq!(c.enter(), Need::Listing("b".into()));
        c.ingest(&[listing("b", &[("x", false), ("y", false), ("z", false)])]);
        assert!(c.entered("b"));
        assert_eq!(c.selected_id, "b/y");
        assert!(c.move_by(1));
        assert_eq!(c.selected_id, "b/z");
        assert_eq!(c.leave(), Need::Nothing);
        assert_eq!(c.selected_id, "b");
        c.enter();
        assert_eq!(c.selected_id, "b/z", "remembers the last child");
        c.leave();
        assert_eq!(c.leave(), Need::LeaveRoot);
        assert!(c.move_by(i64::MAX));
        assert_eq!(c.activate(), Need::Open("c".into()));
    }

    #[test]
    fn wheel_accumulates() {
        let mut c = Controller::new();
        c.reset("root", &listing("", &[("a", false), ("b", false), ("c", false), ("d", false)]));
        c.select("a");
        assert!(!c.wheel(30.0));
        assert!(c.wheel(30.0));
        assert_eq!(c.selected_id, "b");
    }
}
