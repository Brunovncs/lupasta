//! `cargo run --example fixture [-- <dir>]`: writes the synthetic fixture (default
//! fixtures/visual/Users) used by the smoke run and for trying the app.

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

fn main() {
    let root = std::env::args().nth(1).map(PathBuf::from).unwrap_or_else(|| PathBuf::from("fixtures/visual/Users"));
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs_f64() * 1000.0;
    lupasta::fixture::build(&root, now).expect("could not write the fixture");
    println!("fixture written to {}", root.display());
}
