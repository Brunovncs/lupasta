//! Loads the whole visual fixture and, for every possible selection:
//! - checks the layout/routing invariants (no overlapping names, orthogonal connectors, no
//!   crossings, no connector through text);
//! - compares every position and connector with tests/golden/layout.json, which the original
//!   TypeScript implementation produced for the same fixture (scripts/golden.ts).

use lupasta::filesystem;
use lupasta::layout::{Layout, compute_layout};
use lupasta::metrics::Metrics;
use lupasta::palette::Colors;
use lupasta::router::{self, Box, crossings, is_orthogonal, route_layout, text_hits};
use lupasta::tree::{Listing, TreeModel};
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::Path;

fn load_all(root: &Path) -> TreeModel {
    let mut model = TreeModel::new("Users");
    fn walk(model: &mut TreeModel, root: &Path, rel: &str) {
        let abs = if rel.is_empty() { root.to_path_buf() } else { root.join(rel) };
        let entries = filesystem::list_dir(&abs).unwrap();
        let dirs: Vec<String> = entries.iter().filter(|e| e.is_dir()).map(|e| e.0.clone()).collect();
        model.ingest(&Listing { path: rel.to_string(), entries });
        for d in dirs {
            walk(model, root, &if rel.is_empty() { d } else { format!("{rel}/{d}") });
        }
    }
    walk(&mut model, root, "");
    model
}

fn fixture() -> (tempfile::TempDir, TreeModel) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("Users");
    lupasta::fixture::build(&root, 1.8e12).unwrap();
    let model = load_all(&root);
    (dir, model)
}

fn boxes(l: &Layout) -> impl Fn(&str) -> Option<Box> + '_ {
    |id| l.nodes.get(id).map(|n| Box { x: n.x, y: n.y, w: n.w })
}

fn selections(model: &TreeModel) -> Vec<String> {
    let mut ids: Vec<String> = model.nodes().map(|n| n.id.clone()).filter(|id| !id.is_empty()).collect();
    ids.sort();
    ids
}

#[test]
fn every_selection_lays_out_without_overlaps_crossings_or_wires_through_text() {
    let (_dir, model) = fixture();
    let m = Metrics::default();
    let colors = Colors::default();
    let mut problems = Vec::new();
    let ids = selections(&model);
    assert!(ids.len() > 100, "fixture has {} entries", ids.len());
    for sel in &ids {
        let l = compute_layout(&model, sel, &m, &colors, 1.8e12);
        let mut rows: BTreeMap<i64, Vec<(f32, f32, &str)>> = BTreeMap::new();
        for n in l.nodes.values() {
            let row = rows.entry((n.y * 100.0) as i64).or_default();
            for (x, w, id) in row.iter() {
                if n.x < x + w && *x < n.x + n.w {
                    problems.push(format!("{sel}: \"{}\" overlaps \"{id}\"", n.id));
                }
            }
            row.push((n.x, n.w, &n.id));
        }
        let routes = route_layout(&l, &boxes(&l), &m, None);
        let all: Vec<Box> = l.nodes.values().map(|n| Box { x: n.x, y: n.y, w: n.w }).collect();
        for r in &routes {
            if !is_orthogonal(&r.points) {
                problems.push(format!("{sel}: {} not orthogonal", r.id));
            }
            if text_hits(&r.points, &all, &m) {
                problems.push(format!("{sel}: {} runs through text", r.id));
            }
        }
        for (a, b) in crossings(&routes) {
            problems.push(format!("{sel}: {a} crosses {b}"));
        }
    }
    assert!(problems.is_empty(), "{:#?}", &problems[..problems.len().min(20)]);
}

#[test]
fn layouts_and_routes_match_the_typescript_original() {
    let (_dir, model) = fixture();
    let golden: Value = serde_json::from_str(include_str!("golden/layout.json")).unwrap();
    let golden = golden.as_object().unwrap();
    let m = Metrics::default();
    let colors = Colors::default();
    let close = |a: f32, b: &Value| (a - b.as_f64().unwrap() as f32).abs() < 0.02;
    let mut problems = Vec::new();
    let ids = selections(&model);
    assert_eq!(ids.len(), golden.len(), "same fixture");
    for sel in &ids {
        let want = &golden[sel];
        let l = compute_layout(&model, sel, &m, &colors, 1.8e12);
        if !close(l.max_x, &want["maxX"]) {
            problems.push(format!("{sel}: maxX {} vs {}", l.max_x, want["maxX"]));
        }
        let want_nodes = want["nodes"].as_array().unwrap();
        if want_nodes.len() != l.nodes.len() {
            problems.push(format!("{sel}: {} nodes vs {}", l.nodes.len(), want_nodes.len()));
        }
        for wn in want_nodes {
            let id = wn[0].as_str().unwrap();
            let Some(n) = l.nodes.get(id) else {
                problems.push(format!("{sel}: missing {id}"));
                continue;
            };
            if !(close(n.x, &wn[1]) && close(n.y, &wn[2]) && close(n.w, &wn[3]) && close(n.box_w, &wn[4]) && n.label == wn[5].as_str().unwrap()) {
                problems.push(format!("{sel}: {id} at ({}, {}) w {} box {} {:?}, want {wn}", n.x, n.y, n.w, n.box_w, n.label));
            }
        }
        let mut routes = route_layout(&l, &boxes(&l), &m, None);
        routes.sort_by(|a, b| a.id.cmp(&b.id));
        let want_routes = want["routes"].as_array().unwrap();
        if routes.len() != want_routes.len() {
            problems.push(format!("{sel}: {} routes vs {}", routes.len(), want_routes.len()));
            continue;
        }
        for (r, wr) in routes.iter().zip(want_routes) {
            let pts = wr[1].as_array().unwrap();
            let same = r.id == wr[0].as_str().unwrap()
                && r.points.len() == pts.len()
                && r.points.iter().zip(pts).all(|(p, q)| close(p.0, &q[0]) && close(p.1, &q[1]));
            if !same {
                problems.push(format!("{sel}: route {} {:?}, want {wr}", r.id, r.points));
            }
        }
    }
    assert!(problems.is_empty(), "{} mismatches:\n{:#?}", problems.len(), &problems[..problems.len().min(15)]);
}

#[test]
fn rounded_paths_keep_their_endpoints() {
    let pts = [(0.0, 0.0), (20.0, 0.0), (20.0, 30.0), (40.0, 30.0)];
    let (start, steps) = router::rounded(&pts, 5.0);
    assert_eq!(start, pts[0]);
    assert_eq!(steps.last(), Some(&router::Step::Line(pts[3])));
}
