//! In-memory search corpus + fuzzy ranking.
//!
//! Retrieval is two-stage: FTS5 trigram gives cheap substring candidates for "normal" queries;
//! when that is not enough (short queries, subsequence queries like `rtr`) the whole corpus is
//! scanned with nucleo-matcher across threads. Ranking always goes through nucleo.

use crate::filesystem::FLAG_DIR;
use crate::index::Index;
use nucleo_matcher::pattern::{AtomKind, CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher, Utf32Str};
use serde::Serialize;
use std::collections::HashMap;
use std::sync::RwLock;

/// Below this many items a single-threaded scan beats spawning workers.
const PARALLEL_MIN: usize = 32_768;

struct Item {
    id: i64,
    path: Box<str>,
    name_at: u32,
    flags: u8,
}

#[derive(Default)]
pub struct Corpus {
    items: Vec<Item>,
    slots: HashMap<i64, u32>,
}

impl Corpus {
    pub fn upsert(&mut self, id: i64, path: &str, flags: u8) {
        let name_at = path.rfind('/').map(|i| i + 1).unwrap_or(0) as u32;
        let item = Item { id, path: path.into(), name_at, flags };
        match self.slots.get(&id) {
            Some(&slot) => self.items[slot as usize] = item,
            None => {
                self.slots.insert(id, self.items.len() as u32);
                self.items.push(item);
            }
        }
    }

    pub fn remove(&mut self, id: i64) {
        if let Some(slot) = self.slots.remove(&id) {
            let slot = slot as usize;
            self.items.swap_remove(slot);
            if slot < self.items.len() {
                self.slots.insert(self.items[slot].id, slot as u32);
            }
        }
    }

    pub fn contains(&self, id: i64) -> bool {
        self.slots.contains_key(&id)
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct SearchHit {
    pub path: String,
    pub name: String,
    pub kind: &'static str,
    pub dir: bool,
    pub score: u32,
}

#[derive(Debug, Serialize)]
pub struct SearchResponse {
    pub query: String,
    pub hits: Vec<SearchHit>,
    pub candidates: usize,
    pub strategy: &'static str,
    pub micros: u64,
}

/// Trims, collapses whitespace and turns path separators into spaces so `file browser`,
/// `file-browser/` and `  FILE   browser ` all behave the same.
pub fn normalize_query(q: &str) -> String {
    q.split(|c: char| c.is_whitespace() || c == '/' || c == '\\').filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" ")
}

fn pattern(q: &str) -> Pattern {
    Pattern::new(q, CaseMatching::Ignore, Normalization::Smart, AtomKind::Fuzzy)
}

/// Name matches dominate; the full path acts as a tie-breaker and enables `dir file` queries.
fn score_item(item: &Item, pat: &Pattern, matcher: &mut Matcher, buf: &mut Vec<char>) -> Option<u32> {
    let path_score = pat.score(Utf32Str::new(&item.path, buf), matcher)?;
    let name = &item.path[item.name_at as usize..];
    let name_score = pat.score(Utf32Str::new(name, buf), matcher).unwrap_or(0);
    // Tie-breakers: tighter names (more of the name is the match) and shallower paths.
    let depth = item.path.matches('/').count() as u32;
    let slack = name.chars().count() as u32;
    Some((path_score + name_score * 2).saturating_sub(depth + slack))
}

fn hit(item: &Item, score: u32) -> SearchHit {
    let name = item.path[item.name_at as usize..].to_string();
    let dir = item.flags & FLAG_DIR != 0;
    SearchHit { kind: crate::index::classify(&name, dir, name.starts_with('.')), path: item.path.to_string(), name, dir, score }
}

const WORD_END_BONUS: u32 = 24;

/// Abbreviation heuristic: `rtr` → `RouteR`. A name match whose last matched char ends a word
/// (end of name, before punctuation, or before a camelCase hump) reads as an abbreviation.
fn word_end_bonus(item: &Item, pat: &Pattern, matcher: &mut Matcher, buf: &mut Vec<char>, idx: &mut Vec<u32>) -> u32 {
    let name = &item.path[item.name_at as usize..];
    idx.clear();
    if pat.indices(Utf32Str::new(name, buf), matcher, idx).is_none() {
        return 0;
    }
    let Some(&last) = idx.iter().max() else { return 0 };
    let chars: Vec<char> = name.chars().collect();
    let (cur, next) = (chars[last as usize], chars.get(last as usize + 1));
    match next {
        None => WORD_END_BONUS,
        Some(n) if !n.is_alphanumeric() => WORD_END_BONUS,
        Some(n) if cur.is_lowercase() && n.is_uppercase() => WORD_END_BONUS,
        _ => 0,
    }
}

fn top_k(mut scored: Vec<(u32, usize)>, items: &[Item], pat: &Pattern, limit: usize) -> Vec<SearchHit> {
    let key = |&(s, i): &(u32, usize)| (std::cmp::Reverse(s), items[i].path.len(), i);
    // Shortlist on the base score, then re-rank the shortlist with the costlier heuristics.
    let shortlist = limit * 3;
    if scored.len() > shortlist {
        scored.select_nth_unstable_by_key(shortlist, key);
        scored.truncate(shortlist);
    }
    let mut matcher = Matcher::new(Config::DEFAULT.match_paths());
    let (mut buf, mut idx) = (Vec::new(), Vec::new());
    for (s, i) in scored.iter_mut() {
        *s += word_end_bonus(&items[*i], pat, &mut matcher, &mut buf, &mut idx);
    }
    scored.sort_unstable_by_key(key);
    scored.truncate(limit);
    scored.into_iter().map(|(s, i)| hit(&items[i], s)).collect()
}

fn scan_chunk(part: &[Item], offset: usize, pat: &Pattern, keep: usize) -> Vec<(u32, usize)> {
    let mut matcher = Matcher::new(Config::DEFAULT.match_paths());
    let mut buf = Vec::new();
    let mut local: Vec<(u32, usize)> = Vec::new();
    for (j, item) in part.iter().enumerate() {
        if let Some(sc) = score_item(item, pat, &mut matcher, &mut buf) {
            local.push((sc, offset + j));
        }
    }
    if local.len() > keep {
        local.select_nth_unstable_by_key(keep, |&(s, _)| std::cmp::Reverse(s));
        local.truncate(keep);
    }
    local
}

pub fn search(corpus: &RwLock<Corpus>, index: Option<&Index>, raw: &str, limit: usize) -> SearchResponse {
    let t0 = std::time::Instant::now();
    let query = normalize_query(raw);
    let mut out = SearchResponse { query: query.clone(), hits: Vec::new(), candidates: 0, strategy: "empty", micros: 0 };
    if query.is_empty() {
        return out;
    }
    let pat = pattern(&query);
    let corpus = corpus.read().unwrap();

    // Stage 1: FTS candidates.
    if let Some(index) = index
        && let Ok(Some(ids)) = index.fts_candidates(&query, 20_000) {
            let mut matcher = Matcher::new(Config::DEFAULT.match_paths());
            let mut buf = Vec::new();
            let scored: Vec<(u32, usize)> = ids
                .iter()
                .filter_map(|id| corpus.slots.get(id).map(|&s| s as usize))
                .filter_map(|i| score_item(&corpus.items[i], &pat, &mut matcher, &mut buf).map(|s| (s, i)))
                .collect();
            if scored.len() >= limit {
                out.candidates = ids.len();
                out.strategy = "fts";
                out.hits = top_k(scored, &corpus.items, &pat, limit);
                out.micros = t0.elapsed().as_micros() as u64;
                return out;
            }
        }

    // Stage 2: parallel full fuzzy scan.
    let items = &corpus.items;
    let keep = limit * 4;
    let scored: Vec<(u32, usize)> = if items.len() < PARALLEL_MIN {
        scan_chunk(items, 0, &pat, keep)
    } else {
        let threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4).min(16);
        let chunk = items.len().div_ceil(threads);
        std::thread::scope(|s| {
            let pat = &pat;
            let handles: Vec<_> = items.chunks(chunk).enumerate().map(|(ci, part)| s.spawn(move || scan_chunk(part, ci * chunk, pat, keep))).collect();
            handles.into_iter().flat_map(|h| h.join().unwrap()).collect()
        })
    };
    out.candidates = items.len();
    out.strategy = "scan";
    out.hits = top_k(scored, items, &pat, limit);
    out.micros = t0.elapsed().as_micros() as u64;
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn corpus(paths: &[&str]) -> RwLock<Corpus> {
        let mut c = Corpus::default();
        for (i, p) in paths.iter().enumerate() {
            let dir = !p.contains('.');
            c.upsert(i as i64 + 1, p, if dir { FLAG_DIR } else { 0 });
        }
        RwLock::new(c)
    }

    const PATHS: &[&str] = &[
        "drcode/file-browser",
        "drcode/file-browser/Sources",
        "drcode/file-browser/Sources/App.swift",
        "drcode/file-browser/Sources/OrthogonalRouter.swift",
        "drcode/file-browser/Sources/ScreenGeometry.swift",
        "drcode/file-browser/Tests/OrthogonalRouterTests.swift",
        "drcode/file-browser/Tests/RouterResponsivenessTests.swift",
        "drcode/file-browser/test-folders/05 Mordor/Orc shift roster.csv",
        "drcode/uruk/main.swift",
        "drcode/Downloads/export.xlsx",
    ];

    #[test]
    fn normalizes_queries() {
        assert_eq!(normalize_query("  FILE   browser "), "FILE browser");
        assert_eq!(normalize_query("file-browser/"), "file-browser");
        assert_eq!(normalize_query("a\\b"), "a b");
        assert_eq!(normalize_query("   "), "");
    }

    #[test]
    fn router_finds_orthogonal_router_in_top_three() {
        let c = corpus(PATHS);
        let r = search(&c, None, "router", 5);
        assert!(r.hits.iter().take(3).any(|h| h.name == "OrthogonalRouter.swift"), "{:?}", r.hits);
        assert!(r.hits.iter().all(|h| h.name.to_lowercase().contains("router")));
    }

    #[test]
    fn subsequence_rtr_finds_router() {
        let c = corpus(PATHS);
        let r = search(&c, None, "rtr", 5);
        assert!(r.hits.iter().take(3).any(|h| h.name == "OrthogonalRouter.swift"), "{:?}", r.hits);
        assert_eq!(r.strategy, "scan");
    }

    #[test]
    fn rtr_prefers_the_word_it_abbreviates() {
        let mut paths = PATHS.to_vec();
        paths.extend([
            "drcode/file-browser/test-folders/05 Mordor/Mount Doom/One Ring Returns Desk",
            "drcode/file-browser/test-folders/05 Mordor/Mount Doom/One Ring Returns Desk/Returned rings.csv",
            "drcode/file-browser/test-folders/07 Isengard/Saruman's Help Desk/Ticket 10 - tower on fire.txt",
            "drcode/file-browser/test-folders/04 Gondor/Minas Tirith",
        ]);
        let c = corpus(&paths);
        let r = search(&c, None, "rtr", 5);
        assert!(r.hits.iter().take(5).any(|h| h.name == "OrthogonalRouter.swift"), "{:#?}", r.hits);
    }

    #[test]
    fn case_insensitive_and_multi_word_path_queries() {
        let c = corpus(PATHS);
        let r = search(&c, None, "FILE browser", 3);
        assert_eq!(r.hits[0].path, "drcode/file-browser");
        let r = search(&c, None, "mordor roster", 3);
        assert_eq!(r.hits[0].name, "Orc shift roster.csv");
    }

    #[test]
    fn no_match_and_limits() {
        let c = corpus(PATHS);
        assert!(search(&c, None, "zzzqqq", 5).hits.is_empty());
        assert_eq!(search(&c, None, "s", 3).hits.len(), 3);
        assert!(search(&c, None, "", 3).hits.is_empty());
    }

    #[test]
    fn corpus_remove_keeps_slots_consistent() {
        let c = corpus(PATHS);
        let mut w = c.write().unwrap();
        w.remove(1);
        w.remove(5);
        assert_eq!(w.len(), PATHS.len() - 2);
        for (id, &slot) in &w.slots {
            assert_eq!(w.items[slot as usize].id, *id);
        }
        w.upsert(3, "renamed/App.swift", 0);
        assert_eq!(w.len(), PATHS.len() - 2);
    }
}
