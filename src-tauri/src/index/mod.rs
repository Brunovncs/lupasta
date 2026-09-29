//! Persistent search index (SQLite + FTS5 trigram) and the background scanner.
//!
//! Row ids are a stable 63-bit hash of the root-relative path, so a parent's id is known
//! without a lookup. That lets the parallel walker emit rows in any order and lets the
//! watcher address rows directly by path.

use crate::filesystem::{self, Scope, FLAG_DIR, FLAG_HIDDEN};
use crate::search::Corpus;
use rusqlite::{params, Connection, OptionalExtension};
use std::collections::{BTreeSet, HashMap};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, AtomicU8};
use std::sync::{Mutex, OnceLock, RwLock};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

/// Directory names never indexed (navigation still shows them).
pub const DEFAULT_EXCLUDES: &[&str] = &[".git", "node_modules", "target", "AppData", "$Recycle.Bin", "System Volume Information"];

const BATCH: usize = 4096;

pub fn path_id(rel: &str) -> i64 {
    if rel.is_empty() {
        return 0;
    }
    // FNV-1a 64, masked to a positive i64 (SQLite rowid).
    let mut h: u64 = 0xcbf29ce484222325;
    for b in rel.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    (h & 0x7fff_ffff_ffff_ffff) as i64
}

fn kinds() -> &'static (Vec<(String, String)>, Vec<String>) {
    static KINDS: OnceLock<(Vec<(String, String)>, Vec<String>)> = OnceLock::new();
    KINDS.get_or_init(|| {
        let v: serde_json::Value = serde_json::from_str(include_str!("../../../src/styles/file-kinds.json")).expect("file-kinds.json");
        let mut by_ext = Vec::new();
        for (kind, exts) in v["kinds"].as_object().unwrap() {
            for e in exts.as_array().unwrap() {
                by_ext.push((e.as_str().unwrap().to_string(), kind.clone()));
            }
        }
        let special = v["special"].as_array().unwrap().iter().map(|s| s.as_str().unwrap().to_string()).collect();
        (by_ext, special)
    })
}

pub fn extension_of(name: &str) -> String {
    match name.rfind('.') {
        Some(i) if i > 0 => name[i + 1..].to_lowercase(),
        _ => String::new(),
    }
}

/// Same rules as `classify` in src/styles/palette.ts, driven by the same JSON file.
pub fn classify(name: &str, is_dir: bool, is_hidden: bool) -> &'static str {
    let (by_ext, special) = kinds();
    if special.iter().any(|s| s == name) {
        return "special";
    }
    if is_dir {
        return if is_hidden { "hidden" } else { "directory" };
    }
    if is_hidden {
        return "hidden";
    }
    let ext = extension_of(name);
    for (e, k) in by_ext {
        if *e == ext {
            return match k.as_str() {
                "code" => "code",
                "text" => "text",
                "image" => "image",
                "document" => "document",
                _ => "binary",
            };
        }
    }
    "binary"
}

#[derive(Debug, Clone)]
pub struct Record {
    pub id: i64,
    pub parent_id: i64,
    pub path: String,
    pub name: String,
    pub extension: String,
    pub kind: &'static str,
    pub size: u64,
    pub mtime: i64,
    pub flags: u8,
    pub file_id: Option<i64>,
}

impl Record {
    pub fn new(rel: String, meta: &fs::Metadata, is_symlink: bool) -> Record {
        let name = filesystem::name_of(&rel).to_string();
        let flags = filesystem::flags_for(&name, meta, is_symlink);
        Record {
            id: path_id(&rel),
            parent_id: path_id(filesystem::parent_rel(&rel)),
            extension: if meta.is_dir() { String::new() } else { extension_of(&name) },
            kind: classify(&name, flags & FLAG_DIR != 0, flags & FLAG_HIDDEN != 0),
            size: if meta.is_dir() { 0 } else { meta.len() },
            mtime: filesystem::mtime_ms(meta) as i64,
            flags,
            file_id: filesystem::file_id(meta),
            name,
            path: rel,
        }
    }
}

pub struct Index {
    writer: Mutex<Connection>,
    reader: Mutex<Connection>,
}

const SCHEMA: &str = r#"
PRAGMA journal_mode = WAL;
PRAGMA synchronous = NORMAL;
CREATE TABLE IF NOT EXISTS files (
  id            INTEGER PRIMARY KEY,
  parent_id     INTEGER NOT NULL,
  path          TEXT    NOT NULL,
  name          TEXT    NOT NULL,
  extension     TEXT    NOT NULL,
  kind          TEXT    NOT NULL,
  size          INTEGER NOT NULL,
  modified_time INTEGER NOT NULL,
  is_directory  INTEGER NOT NULL,
  is_hidden     INTEGER NOT NULL,
  file_id       INTEGER
);
CREATE INDEX IF NOT EXISTS files_parent ON files(parent_id);
-- Names only: paths are long (one trigram per character) and are already matched by the
-- in-memory fuzzy scan, which is the fallback whenever name candidates are too few.
CREATE VIRTUAL TABLE IF NOT EXISTS files_fts USING fts5(name, content='files', content_rowid='id', tokenize='trigram');
CREATE TABLE IF NOT EXISTS meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
"#;

/// Keeps files_fts in sync with files. Dropped during a bulk build (then rebuilt in one pass).
const TRIGGERS: &str = r#"
CREATE TRIGGER IF NOT EXISTS files_ai AFTER INSERT ON files BEGIN
  INSERT INTO files_fts(rowid, name) VALUES (new.id, new.name);
END;
CREATE TRIGGER IF NOT EXISTS files_ad AFTER DELETE ON files BEGIN
  INSERT INTO files_fts(files_fts, rowid, name) VALUES ('delete', old.id, old.name);
END;
CREATE TRIGGER IF NOT EXISTS files_au AFTER UPDATE OF name ON files BEGIN
  INSERT INTO files_fts(files_fts, rowid, name) VALUES ('delete', old.id, old.name);
  INSERT INTO files_fts(rowid, name) VALUES (new.id, new.name);
END;
"#;

/// Bump to rebuild existing index files (v3: FTS on names only).
const SCHEMA_VERSION: i64 = 3;

impl Index {
    pub fn open(path: &Path) -> rusqlite::Result<Index> {
        let writer = Connection::open(path)?;
        let version: i64 = writer.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        if version != SCHEMA_VERSION {
            writer.execute_batch("DROP TABLE IF EXISTS files_fts; DROP TABLE IF EXISTS files; DROP TABLE IF EXISTS meta;")?;
            writer.execute_batch(&format!("PRAGMA user_version = {SCHEMA_VERSION};"))?;
        }
        writer.execute_batch(SCHEMA)?;
        writer.execute_batch(TRIGGERS)?;
        let reader = Connection::open(path)?;
        reader.execute_batch("PRAGMA query_only = ON;")?;
        Ok(Index { writer: Mutex::new(writer), reader: Mutex::new(reader) })
    }

    pub fn upsert(&self, records: &[Record]) -> rusqlite::Result<()> {
        let mut conn = self.writer.lock().unwrap();
        let tx = conn.transaction()?;
        {
            let mut stmt = tx.prepare_cached(
                "INSERT INTO files (id, parent_id, path, name, extension, kind, size, modified_time, is_directory, is_hidden, file_id)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
                 ON CONFLICT(id) DO UPDATE SET size = excluded.size, modified_time = excluded.modified_time,
                   is_directory = excluded.is_directory, is_hidden = excluded.is_hidden, kind = excluded.kind,
                   file_id = excluded.file_id",
            )?;
            for r in records {
                stmt.execute(params![
                    r.id,
                    r.parent_id,
                    r.path,
                    r.name,
                    r.extension,
                    r.kind,
                    r.size as i64,
                    r.mtime,
                    (r.flags & FLAG_DIR != 0) as i64,
                    (r.flags & FLAG_HIDDEN != 0) as i64,
                    r.file_id
                ])?;
            }
        }
        tx.commit()
    }

    /// Deletes a path and everything below it. Returns the removed ids.
    pub fn delete_subtree(&self, rel: &str) -> rusqlite::Result<Vec<i64>> {
        let mut conn = self.writer.lock().unwrap();
        let tx = conn.transaction()?;
        let ids: Vec<i64> = {
            let mut stmt = tx.prepare_cached(
                "WITH RECURSIVE sub(id) AS (SELECT id FROM files WHERE id = ?1
                   UNION ALL SELECT f.id FROM files f JOIN sub ON f.parent_id = sub.id)
                 SELECT id FROM sub",
            )?;
            let rows = stmt.query_map([path_id(rel)], |r| r.get(0))?;
            rows.collect::<Result<_, _>>()?
        };
        {
            let mut del = tx.prepare_cached("DELETE FROM files WHERE id = ?1")?;
            for id in &ids {
                del.execute([id])?;
            }
        }
        tx.commit()?;
        Ok(ids)
    }

    pub fn delete_ids(&self, ids: &[i64]) -> rusqlite::Result<()> {
        let mut conn = self.writer.lock().unwrap();
        let tx = conn.transaction()?;
        {
            let mut stmt = tx.prepare_cached("DELETE FROM files WHERE id = ?1")?;
            for id in ids {
                stmt.execute([id])?;
            }
        }
        tx.commit()
    }

    /// What the index believes about every row: id → (mtime, size, dir/hidden flags).
    pub fn signatures(&self) -> rusqlite::Result<HashMap<i64, Signature>> {
        let conn = self.reader.lock().unwrap();
        let mut stmt = conn.prepare("SELECT id, modified_time, size, is_directory, is_hidden FROM files")?;
        let mut rows = stmt.query([])?;
        let mut out = HashMap::new();
        while let Some(r) = rows.next()? {
            let flags = (r.get::<_, i64>(3)? as u8 * FLAG_DIR) | (r.get::<_, i64>(4)? as u8 * FLAG_HIDDEN);
            out.insert(r.get(0)?, (r.get(1)?, r.get(2)?, flags));
        }
        Ok(out)
    }

    /// Bulk build: drop the per-row FTS triggers and relax durability; `bulk_end` rebuilds the
    /// full-text index in one pass (far cheaper than row-by-row trigram inserts).
    pub fn bulk_begin(&self) -> rusqlite::Result<()> {
        self.writer.lock().unwrap().execute_batch(
            "DROP TRIGGER IF EXISTS files_ai; DROP TRIGGER IF EXISTS files_ad; DROP TRIGGER IF EXISTS files_au; PRAGMA synchronous = OFF;",
        )
    }

    pub fn bulk_end(&self) -> rusqlite::Result<()> {
        let conn = self.writer.lock().unwrap();
        conn.execute_batch("INSERT INTO files_fts(files_fts) VALUES ('rebuild'); PRAGMA synchronous = NORMAL;")?;
        conn.execute_batch(TRIGGERS)
    }

    /// FTS5 trigram candidates for queries whose tokens are all >= 3 chars.
    pub fn fts_candidates(&self, query: &str, limit: usize) -> rusqlite::Result<Option<Vec<i64>>> {
        let tokens: Vec<&str> = query.split_whitespace().collect();
        if tokens.is_empty() || tokens.iter().any(|t| t.chars().count() < 3) {
            return Ok(None);
        }
        let expr = tokens.iter().map(|t| format!("\"{}\"", t.replace('"', "\"\""))).collect::<Vec<_>>().join(" AND ");
        let conn = self.reader.lock().unwrap();
        let mut stmt = conn.prepare_cached("SELECT rowid FROM files_fts WHERE files_fts MATCH ?1 LIMIT ?2")?;
        let rows = stmt.query_map(params![expr, limit as i64], |r| r.get(0))?;
        Ok(Some(rows.collect::<Result<_, _>>()?))
    }

    pub fn load_all(&self, mut f: impl FnMut(i64, String, u8)) -> rusqlite::Result<()> {
        let conn = self.reader.lock().unwrap();
        let mut stmt = conn.prepare("SELECT id, path, is_directory, is_hidden FROM files")?;
        let mut rows = stmt.query([])?;
        while let Some(r) = rows.next()? {
            let flags = (r.get::<_, i64>(2)? as u8 * FLAG_DIR) | (r.get::<_, i64>(3)? as u8 * FLAG_HIDDEN);
            f(r.get(0)?, r.get(1)?, flags);
        }
        Ok(())
    }

    pub fn count(&self) -> i64 {
        let conn = self.reader.lock().unwrap();
        conn.query_row("SELECT count(*) FROM files", [], |r| r.get(0)).unwrap_or(0)
    }

    pub fn get(&self, rel: &str) -> rusqlite::Result<Option<(String, i64, i64)>> {
        let conn = self.reader.lock().unwrap();
        conn.query_row("SELECT path, size, modified_time FROM files WHERE id = ?1", [path_id(rel)], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .optional()
    }
}

pub type Signature = (i64, i64, u8);

pub fn now_ms() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

pub const STATE_IDLE: u8 = 0;
pub const STATE_LOADING: u8 = 1;
pub const STATE_INDEXING: u8 = 2;
pub const STATE_READY: u8 = 3;

#[derive(Default)]
pub struct Status {
    pub state: AtomicU8,
    pub indexed: AtomicU64,
    pub last_scan_ms: AtomicU64,
}

#[derive(Clone)]
pub struct ScanOptions {
    pub respect_gitignore: bool,
    pub excludes: Vec<String>,
}

impl Default for ScanOptions {
    fn default() -> Self {
        ScanOptions { respect_gitignore: true, excludes: DEFAULT_EXCLUDES.iter().map(|s| s.to_string()).collect() }
    }
}

pub struct ScanStats {
    pub entries: u64,
    pub removed: u64,
    pub elapsed_ms: u64,
}

/// Walks `start` (a path under the scope root) in parallel and upserts everything into the
/// index and the in-memory corpus. With `purge`, rows not seen by this scan are deleted.
pub fn scan(
    scope: &Scope,
    start: &Path,
    index: &Index,
    corpus: &RwLock<Corpus>,
    opts: &ScanOptions,
    purge: bool,
    progress: &dyn Fn(u64),
) -> rusqlite::Result<ScanStats> {
    let t0 = Instant::now();
    let (tx, rx) = crossbeam_channel::bounded::<Record>(BATCH * 4);
    let excludes = opts.excludes.clone();
    let walker = ignore::WalkBuilder::new(start)
        .hidden(false)
        .parents(false)
        .ignore(opts.respect_gitignore)
        .git_ignore(opts.respect_gitignore)
        .git_exclude(opts.respect_gitignore)
        .git_global(false)
        .require_git(false)
        .follow_links(false)
        .filter_entry(move |e| e.depth() == 0 || !excludes.iter().any(|x| e.file_name() == x.as_str()))
        .build_parallel();
    let scope_c = scope.clone();
    let walk = std::thread::spawn(move || {
        walker.run(|| {
            let tx = tx.clone();
            let scope = scope_c.clone();
            Box::new(move |res| {
                let Ok(entry) = res else { return ignore::WalkState::Continue };
                if entry.depth() == 0 {
                    return ignore::WalkState::Continue;
                }
                let Some(rel) = scope.to_rel(entry.path()) else { return ignore::WalkState::Continue };
                let is_symlink = entry.path_is_symlink();
                if let Ok(meta) = entry.metadata() {
                    if tx.send(Record::new(rel, &meta, is_symlink)).is_err() {
                        return ignore::WalkState::Quit;
                    }
                }
                ignore::WalkState::Continue
            })
        });
    });

    // A full resync diffs against what the index already knows and only writes changes; rows
    // left in `known` afterwards were not seen on disk and are deleted. An empty index is
    // built in bulk mode.
    let mut known = if purge { index.signatures()? } else { HashMap::new() };
    let bulk = purge && known.is_empty();
    if bulk {
        index.bulk_begin()?;
    }
    let mut batch = Vec::with_capacity(BATCH);
    let mut total = 0u64;
    let flush = |batch: &mut Vec<Record>| -> rusqlite::Result<()> {
        index.upsert(batch)?;
        let mut c = corpus.write().unwrap();
        for r in batch.iter() {
            c.upsert(r.id, &r.path, r.flags);
        }
        batch.clear();
        Ok(())
    };
    for rec in rx {
        total += 1;
        let sig = (rec.mtime, rec.size as i64, rec.flags & (FLAG_DIR | FLAG_HIDDEN));
        if known.remove(&rec.id) == Some(sig) {
            if total % BATCH as u64 == 0 {
                progress(total);
            }
            continue;
        }
        batch.push(rec);
        if batch.len() >= BATCH {
            flush(&mut batch)?;
            progress(total);
        }
    }
    flush(&mut batch)?;
    let _ = walk.join();
    if bulk {
        index.bulk_end()?;
    }
    progress(total);

    let stale: Vec<i64> = known.into_keys().collect();
    if !stale.is_empty() {
        index.delete_ids(&stale)?;
        let mut c = corpus.write().unwrap();
        for id in &stale {
            c.remove(*id);
        }
    }
    Ok(ScanStats { entries: total, removed: stale.len() as u64, elapsed_ms: t0.elapsed().as_millis() as u64 })
}

/// Applies a coalesced set of changed absolute paths (from the watcher) to index + corpus.
/// Returns the root-relative parent directories whose listings changed.
pub fn apply_paths(
    scope: &Scope,
    index: &Index,
    corpus: &RwLock<Corpus>,
    opts: &ScanOptions,
    paths: &[PathBuf],
) -> rusqlite::Result<BTreeSet<String>> {
    let mut dirs = BTreeSet::new();
    let mut upserts = Vec::new();
    for abs in paths {
        let Some(rel) = scope.to_rel(abs) else { continue };
        if rel.is_empty() || rel.split('/').any(|p| opts.excludes.iter().any(|x| x == p)) {
            continue;
        }
        dirs.insert(filesystem::parent_rel(&rel).to_string());
        match fs::symlink_metadata(abs) {
            Ok(lmeta) => {
                let is_symlink = lmeta.file_type().is_symlink();
                let meta = if is_symlink { fs::metadata(abs).unwrap_or(lmeta) } else { lmeta };
                let known = corpus.read().unwrap().contains(path_id(&rel));
                if meta.is_dir() && !known {
                    // A directory appeared (created or moved in): index its whole subtree.
                    scan(scope, abs, index, corpus, opts, false, &|_| {})?;
                }
                dirs.insert(rel.clone());
                upserts.push(Record::new(rel, &meta, is_symlink));
            }
            Err(_) => {
                let ids = index.delete_subtree(&rel)?;
                let mut c = corpus.write().unwrap();
                c.remove(path_id(&rel));
                for id in ids {
                    c.remove(id);
                }
            }
        }
    }
    if !upserts.is_empty() {
        index.upsert(&upserts)?;
        let mut c = corpus.write().unwrap();
        for r in &upserts {
            c.upsert(r.id, &r.path, r.flags);
        }
    }
    Ok(dirs)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() -> (tempfile::TempDir, tempfile::TempDir, Scope, Index, RwLock<Corpus>) {
        let root = tempfile::tempdir().unwrap();
        let data = tempfile::tempdir().unwrap();
        fs::create_dir_all(root.path().join("file-browser/Sources")).unwrap();
        fs::create_dir_all(root.path().join("file-browser/.git/objects")).unwrap();
        fs::write(root.path().join("file-browser/Sources/OrthogonalRouter.swift"), "x").unwrap();
        fs::write(root.path().join("file-browser/Sources/App.swift"), "x").unwrap();
        fs::write(root.path().join("file-browser/.git/HEAD"), "x").unwrap();
        fs::write(root.path().join("file-browser/README.md"), "x").unwrap();
        let scope = Scope::new(root.path()).unwrap();
        let index = Index::open(&data.path().join("i.sqlite")).unwrap();
        (root, data, scope, index, RwLock::new(Corpus::default()))
    }

    #[test]
    fn path_ids_are_stable_and_parent_linked() {
        assert_eq!(path_id(""), 0);
        assert_eq!(path_id("a/b"), path_id("a/b"));
        assert_ne!(path_id("a/b"), path_id("a/c"));
        assert!(path_id("x") > 0);
    }

    #[test]
    fn classify_uses_shared_config() {
        assert_eq!(classify("OrthogonalRouter.swift", false, false), "code");
        assert_eq!(classify("notes.TXT", false, false), "text");
        assert_eq!(classify("a.png", false, false), "image");
        assert_eq!(classify("memo.m4a", false, false), "binary");
        assert_eq!(classify("plan.xlsx", false, false), "document");
        assert_eq!(classify("Sources", true, false), "directory");
        assert_eq!(classify(".eye.log", false, true), "hidden");
        assert_eq!(classify("README.md", false, false), "special");
    }

    #[test]
    fn scan_inserts_updates_and_purges() {
        let (root, _d, scope, index, corpus) = setup();
        let stats = scan(&scope, scope.root(), &index, &corpus, &ScanOptions::default(), true, &|_| {}).unwrap();
        // file-browser, Sources, 2 swift files, README.md (.git excluded)
        assert_eq!(stats.entries, 5);
        assert_eq!(index.count(), 5);
        assert!(index.get("file-browser/.git/HEAD").unwrap().is_none());
        let (_, parent_of_router) = {
            let conn = index.reader.lock().unwrap();
            let pid: i64 = conn.query_row("SELECT parent_id FROM files WHERE name = 'OrthogonalRouter.swift'", [], |r| r.get(0)).unwrap();
            ((), pid)
        };
        assert_eq!(parent_of_router, path_id("file-browser/Sources"));

        fs::remove_file(root.path().join("file-browser/README.md")).unwrap();
        fs::write(root.path().join("file-browser/Sources/App.swift"), "longer").unwrap();
        let stats = scan(&scope, scope.root(), &index, &corpus, &ScanOptions::default(), true, &|_| {}).unwrap();
        assert_eq!(stats.removed, 1);
        assert_eq!(index.count(), 4);
        assert_eq!(index.get("file-browser/Sources/App.swift").unwrap().unwrap().1, 6);
        assert_eq!(corpus.read().unwrap().len(), 4);
    }

    #[test]
    fn fts_finds_substrings_and_skips_short_tokens() {
        let (_r, _d, scope, index, corpus) = setup();
        scan(&scope, scope.root(), &index, &corpus, &ScanOptions::default(), true, &|_| {}).unwrap();
        let hits = index.fts_candidates("router", 10).unwrap().unwrap();
        assert_eq!(hits, vec![path_id("file-browser/Sources/OrthogonalRouter.swift")]);
        // Names only: a directory word does not match its children (the fuzzy scan covers paths).
        let hits = index.fts_candidates("sources", 10).unwrap().unwrap();
        assert_eq!(hits, vec![path_id("file-browser/Sources")]);
        assert!(index.fts_candidates("file sources", 10).unwrap().unwrap().is_empty());
        assert!(index.fts_candidates("rt", 10).unwrap().is_none());
    }

    #[test]
    fn path_queries_still_resolve_through_the_fuzzy_scan() {
        let (_r, _d, scope, index, corpus) = setup();
        scan(&scope, scope.root(), &index, &corpus, &ScanOptions::default(), true, &|_| {}).unwrap();
        let r = crate::search::search(&corpus, Some(&index), "sources app", 5);
        assert_eq!(r.strategy, "scan");
        assert_eq!(r.hits[0].path, "file-browser/Sources/App.swift");
    }

    #[test]
    fn apply_paths_handles_create_modify_delete() {
        let (root, _d, scope, index, corpus) = setup();
        scan(&scope, scope.root(), &index, &corpus, &ScanOptions::default(), true, &|_| {}).unwrap();
        let opts = ScanOptions::default();

        let newdir = root.path().join("file-browser/Tests");
        fs::create_dir_all(newdir.join("deep")).unwrap();
        fs::write(newdir.join("deep/RouterTests.swift"), "x").unwrap();
        let dirs = apply_paths(&scope, &index, &corpus, &opts, &[newdir.clone()]).unwrap();
        assert!(dirs.contains("file-browser"));
        assert!(index.get("file-browser/Tests/deep/RouterTests.swift").unwrap().is_some());

        fs::write(root.path().join("file-browser/Sources/App.swift"), "123456789").unwrap();
        apply_paths(&scope, &index, &corpus, &opts, &[root.path().join("file-browser/Sources/App.swift")]).unwrap();
        assert_eq!(index.get("file-browser/Sources/App.swift").unwrap().unwrap().1, 9);

        fs::remove_dir_all(&newdir).unwrap();
        apply_paths(&scope, &index, &corpus, &opts, &[newdir]).unwrap();
        assert!(index.get("file-browser/Tests/deep/RouterTests.swift").unwrap().is_none());
        assert!(index.get("file-browser/Tests").unwrap().is_none());
        assert!(!corpus.read().unwrap().contains(path_id("file-browser/Tests/deep")));
        assert_eq!(index.fts_candidates("RouterTests", 10).unwrap().unwrap().len(), 0);
    }
}
