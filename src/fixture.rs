//! The synthetic filesystem used by the tests, the smoke run and screenshots: the names seen in
//! the reference video, each with an "age" t ∈ [0,1] (0 = just modified, 1 = oldest). Mtimes
//! come from the same curve the palette uses, so the colours match the reference.

use crate::palette::Colors;
use serde_json::Value;
use std::fs;
use std::io;
use std::path::Path;

pub const TREE: &str = include_str!("../tests/fixture.json");

/// Writes the fixture under `root` (replacing whatever is there), ages relative to `now_ms`.
pub fn build(root: &Path, now_ms: f64) -> io::Result<()> {
    if root.exists() {
        fs::remove_dir_all(root)?;
    }
    fs::create_dir_all(root)?;
    let tree: Value = serde_json::from_str(TREE).map_err(io::Error::other)?;
    let colors = Colors::default();
    let stamp = |p: &Path, t: f64| {
        let when = (now_ms - colors.age_for_t(t)) / 1000.0;
        let ft = filetime::FileTime::from_unix_time(when.floor() as i64, ((when.fract()) * 1e9) as u32);
        filetime::set_file_mtime(p, ft)
    };
    fn walk(dir: &Path, node: &Value, stamp: &dyn Fn(&Path, f64) -> io::Result<()>) -> io::Result<()> {
        let Some(map) = node.as_object() else { return Ok(()) };
        for (name, spec) in map {
            let p = dir.join(name);
            match spec {
                Value::Number(t) => {
                    fs::write(&p, format!("{name}\n"))?;
                    stamp(&p, t.as_f64().unwrap_or(0.0))?;
                }
                Value::Array(pair) => {
                    fs::create_dir(&p)?;
                    walk(&p, &pair[1], stamp)?;
                    // After the children: creating entries bumps the directory mtime.
                    stamp(&p, pair[0].as_f64().unwrap_or(0.0))?;
                }
                _ => {}
            }
        }
        Ok(())
    }
    walk(root, &tree, &stamp)
}

const WORDS: &[&str] = &[
    "router", "orthogonal", "connector", "screen", "geometry", "browser", "model", "view", "editor", "tree", "index", "search", "watcher", "layout",
    "camera", "palette", "fixture", "render", "scene", "motion", "column", "preview", "lane", "caret",
];
const EXTS: &[&str] = &["swift", "rs", "ts", "md", "txt", "png", "csv", "pdf", "json", "log"];

/// The generator of the old `scripts/make-bench-fixture.ts`, step for step: JavaScript computed
/// `(seed * 1103515245 + 12345) & 0x7fffffff` in doubles, so the product is rounded to 53 bits
/// before the integer mask. Reproducing that keeps the bench trees (and their names) identical.
struct Lcg(u32);

impl Lcg {
    fn next(&mut self) -> f64 {
        let v = self.0 as f64 * 1103515245.0 + 12345.0;
        self.0 = (v.rem_euclid(4294967296.0) as u64 as u32) & 0x7fff_ffff;
        self.0 as f64 / 0x7fff_ffff as f64
    }

    fn word(&mut self) -> &'static str {
        WORDS[(self.next() * WORDS.len() as f64) as usize]
    }
}

/// Writes a deterministic tree of `total` entries under `root` (fan-out 8 folders / 40 empty
/// files per folder) plus `wide/`, one flat folder with total/10 (at most 20,000) files.
/// Returns how many entries were made.
pub fn build_bench(root: &Path, total: usize) -> io::Result<usize> {
    if root.exists() {
        fs::remove_dir_all(root)?;
    }
    fs::create_dir_all(root)?;
    let mut rng = Lcg(42);
    let mut count = 0;
    let wide_n = (total / 10).min(20_000);
    let wide = root.join("wide");
    fs::create_dir(&wide)?;
    count += 1;
    for i in 0..wide_n {
        let name = format!("{}-{}-{i}.{}", rng.word(), rng.word(), EXTS[i % EXTS.len()]);
        fs::write(wide.join(name), "")?;
        count += 1;
    }
    let mut queue = std::collections::VecDeque::from([root.to_path_buf()]);
    while count < total {
        let Some(dir) = queue.pop_front() else { break };
        let mut d = 0;
        while d < 8 && count < total {
            let p = dir.join(format!("{}-{d}", rng.word()));
            fs::create_dir(&p)?;
            queue.push_back(p);
            d += 1;
            count += 1;
        }
        let mut f = 0;
        while f < 40 && count < total {
            // Three draws: a word, the capitalised first letter of a second, the rest of a third.
            let (a, b, c) = (rng.word(), rng.word(), rng.word());
            fs::write(dir.join(format!("{a}{}{}-{f}.{}", b[..1].to_uppercase(), &c[1..], EXTS[f % EXTS.len()])), "")?;
            f += 1;
            count += 1;
        }
    }
    Ok(count)
}

#[cfg(test)]
mod tests {
    #[test]
    fn bench_tree_matches_the_old_typescript_generator() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("bench");
        assert_eq!(super::build_bench(&root, 3000).unwrap(), 3000);
        // Names the TypeScript generator produced for the same size.
        for p in ["wide/layout-watcher-0.swift", "router-0", "previewTatcher-0.swift"] {
            assert!(root.join(p).exists(), "{p}");
        }
        assert_eq!(std::fs::read_dir(root.join("wide")).unwrap().count(), 300);
    }
}
