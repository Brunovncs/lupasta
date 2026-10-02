//! Tree model: a lazily populated mirror of the directories the core has listed.
//! Pure data — no layout, no rendering, no IO.

use crate::filesystem::{Entry, FLAG_DIR, FLAG_HIDDEN};
use std::collections::{HashMap, HashSet};

/// One folder's entries, in display order. `path` is root-relative ("" is the root).
#[derive(Debug, Clone)]
pub struct Listing {
    pub path: String,
    pub entries: Vec<Entry>,
}

#[derive(Debug, Clone)]
pub struct Node {
    /// Root-relative path ("" is the root).
    pub id: String,
    pub parent_id: Option<String>,
    pub name: String,
    pub is_dir: bool,
    pub is_hidden: bool,
    pub mtime: f64,
    pub size: u64,
    pub loaded: bool,
    pub children: Option<Vec<String>>,
}

pub fn join_path(parent: &str, name: &str) -> String {
    if parent.is_empty() { name.to_string() } else { format!("{parent}/{name}") }
}

pub fn parent_of(path: &str) -> &str {
    path.rfind('/').map_or("", |i| &path[..i])
}

#[derive(Debug, Default)]
pub struct TreeModel {
    nodes: HashMap<String, Node>,
    pub version: u64,
}

impl TreeModel {
    pub fn new(root_name: &str) -> TreeModel {
        let mut nodes = HashMap::new();
        nodes.insert(
            String::new(),
            Node {
                id: String::new(),
                parent_id: None,
                name: root_name.to_string(),
                is_dir: true,
                is_hidden: false,
                mtime: 0.0,
                size: 0,
                loaded: false,
                children: None,
            },
        );
        TreeModel { nodes, version: 0 }
    }

    pub fn get(&self, id: &str) -> Option<&Node> {
        self.nodes.get(id)
    }

    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    pub fn nodes(&self) -> impl Iterator<Item = &Node> {
        self.nodes.values()
    }

    pub fn children(&self, id: &str) -> Option<Vec<&Node>> {
        let ids = self.nodes.get(id)?.children.as_ref()?;
        Some(ids.iter().filter_map(|c| self.nodes.get(c)).collect())
    }

    /// Replaces the children of `listing.path`. Removed entries drop their whole subtree.
    pub fn ingest(&mut self, listing: &Listing) -> bool {
        let Some(parent) = self.nodes.get(&listing.path) else { return false };
        let parent_id = parent.id.clone();
        let mut changed = !parent.loaded;
        let old_children = parent.children.clone().unwrap_or_default();
        let mut next = Vec::with_capacity(listing.entries.len());
        let mut seen = HashSet::with_capacity(listing.entries.len());
        for Entry(name, flags, mtime, size) in &listing.entries {
            let id = join_path(&parent_id, name);
            seen.insert(id.clone());
            next.push(id.clone());
            let is_dir = flags & FLAG_DIR != 0;
            let is_hidden = flags & FLAG_HIDDEN != 0;
            match self.nodes.get_mut(&id) {
                Some(old) if old.is_dir == is_dir => {
                    if old.mtime != *mtime || old.size != *size || old.is_hidden != is_hidden {
                        old.mtime = *mtime;
                        old.size = *size;
                        old.is_hidden = is_hidden;
                        changed = true;
                    }
                    continue;
                }
                Some(_) => self.remove_subtree(&id),
                None => {}
            }
            changed = true;
            self.nodes.insert(
                id.clone(),
                Node {
                    id,
                    parent_id: Some(parent_id.clone()),
                    name: name.clone(),
                    is_dir,
                    is_hidden,
                    mtime: *mtime,
                    size: *size,
                    loaded: !is_dir,
                    children: if is_dir { None } else { Some(Vec::new()) },
                },
            );
        }
        for old in &old_children {
            if !seen.contains(old) {
                self.remove_subtree(old);
                changed = true;
            }
        }
        let parent = self.nodes.get_mut(&parent_id).unwrap();
        if !changed && parent.children.as_ref().is_some_and(|c| *c != next) {
            changed = true;
        }
        parent.children = Some(next);
        parent.loaded = true;
        if changed {
            self.version += 1;
        }
        changed
    }

    /// A directory the OS refused to list (permissions, dangling junction): treat as empty so it
    /// is not requested again on every selection change.
    pub fn mark_unreadable(&mut self, id: &str) {
        let Some(n) = self.nodes.get_mut(id) else { return };
        if n.loaded {
            return;
        }
        n.loaded = true;
        n.children = Some(Vec::new());
        self.version += 1;
    }

    fn remove_subtree(&mut self, id: &str) {
        let Some(n) = self.nodes.remove(id) else { return };
        for c in n.children.unwrap_or_default() {
            self.remove_subtree(&c);
        }
    }

    /// Ids from the top-level ancestor down to `id` (root excluded).
    pub fn path_to(&self, id: &str) -> Vec<String> {
        let mut out = Vec::new();
        let mut cur = self.nodes.get(id);
        while let Some(n) = cur {
            let Some(p) = &n.parent_id else { break };
            out.push(n.id.clone());
            cur = self.nodes.get(p);
        }
        out.reverse();
        out
    }

    pub fn siblings(&self, id: &str) -> Vec<&Node> {
        match self.nodes.get(id).and_then(|n| n.parent_id.as_deref()) {
            Some(p) => self.children(p).unwrap_or_default(),
            None => Vec::new(),
        }
    }

    /// Directories whose listing the preview columns need but that are not loaded yet.
    pub fn missing_for_preview(&self, selected: &str) -> Vec<String> {
        let Some(sel) = self.nodes.get(selected) else { return Vec::new() };
        let mut want: Vec<&Node> = self.siblings(selected).into_iter().filter(|s| s.is_dir).collect();
        if sel.is_dir && sel.loaded {
            want.extend(self.children(&sel.id).unwrap_or_default().into_iter().filter(|c| c.is_dir));
        }
        want.into_iter().filter(|n| !n.loaded).map(|n| n.id.clone()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn e(name: &str, dir: bool) -> Entry {
        Entry(name.into(), if dir { FLAG_DIR } else { 0 }, 1.0, 0)
    }

    #[test]
    fn ingest_paths_and_removal() {
        let mut m = TreeModel::new("root");
        assert!(m.ingest(&Listing { path: "".into(), entries: vec![e("a", true), e("b.txt", false)] }));
        assert!(m.ingest(&Listing { path: "a".into(), entries: vec![e("c", true)] }));
        assert_eq!(m.path_to("a/c"), vec!["a", "a/c"]);
        assert_eq!(m.siblings("b.txt").len(), 2);
        assert_eq!(m.missing_for_preview("a"), vec!["a/c"]);
        assert!(!m.ingest(&Listing { path: "".into(), entries: vec![e("a", true), e("b.txt", false)] }));
        assert!(m.ingest(&Listing { path: "".into(), entries: vec![e("b.txt", false)] }));
        assert!(m.get("a/c").is_none());
    }
}
