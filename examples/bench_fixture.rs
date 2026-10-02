//! `cargo run --release --example bench_fixture [-- 10000 100000 500000]`: writes
//! fixtures/bench-<N>, the trees `examples/bench.rs` and docs/benchmarks.md use.

use std::path::PathBuf;
use std::time::Instant;

fn main() {
    let mut sizes: Vec<usize> = std::env::args().skip(1).filter_map(|a| a.parse().ok()).collect();
    if sizes.is_empty() {
        sizes = vec![10_000, 100_000, 500_000];
    }
    for n in sizes {
        let label = if n >= 1000 { format!("{}k", n / 1000) } else { n.to_string() };
        let root = PathBuf::from(format!("fixtures/bench-{label}"));
        let t0 = Instant::now();
        let made = lupasta::fixture::build_bench(&root, n).expect("could not write the bench fixture");
        println!("{}: {made} entries in {:.1} s", root.display(), t0.elapsed().as_secs_f64());
    }
}
