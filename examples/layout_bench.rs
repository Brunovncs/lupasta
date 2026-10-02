//! `cargo run --release --example layout_bench`: layout + routing cost per keystroke in a folder
//! with 20,000 entries (the README's responsiveness number).

use lupasta::filesystem::{Entry, FLAG_DIR};
use lupasta::layout::compute_layout;
use lupasta::metrics::Metrics;
use lupasta::palette::Colors;
use lupasta::router::{Box, route_layout};
use lupasta::tree::{Listing, TreeModel};
use std::time::Instant;

fn main() {
    let n = 20_000;
    let mut model = TreeModel::new("root");
    model.ingest(&Listing { path: String::new(), entries: vec![Entry("big".into(), FLAG_DIR, 1.7e12, 0)] });
    let entries = (0..n).map(|i| Entry(format!("entry-{i:05}-with-a-reasonably-long-name.txt"), if i % 50 == 0 { FLAG_DIR } else { 0 }, 1.7e12 - i as f64 * 1e6, 100)).collect();
    model.ingest(&Listing { path: "big".into(), entries });
    for i in (0..n).step_by(50) {
        let dir = format!("big/entry-{i:05}-with-a-reasonably-long-name.txt");
        model.ingest(&Listing { path: dir, entries: (0..20).map(|j| Entry(format!("child-{j}"), 0, 1.7e12, 1)).collect() });
    }
    let (m, colors) = (Metrics::default(), Colors::default());
    let mut samples = Vec::new();
    for k in 0..200 {
        let sel = format!("big/entry-{:05}-with-a-reasonably-long-name.txt", n / 2 + k);
        let t = Instant::now();
        let l = compute_layout(&model, &sel, &m, &colors, 1.7e12);
        let routes = route_layout(&l, &|id| l.nodes.get(id).map(|n| Box { x: n.x, y: n.y, w: n.w }), &m, None);
        samples.push(t.elapsed().as_secs_f64() * 1000.0);
        std::hint::black_box((l, routes));
    }
    samples.sort_by(f64::total_cmp);
    println!("{n} entries: layout + routing per keystroke, median {:.2} ms, p95 {:.2} ms", samples[100], samples[190]);
}
