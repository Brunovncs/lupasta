//! Filesystem authority: path scoping/normalization and directory listing.
//!
//! The UI only ever speaks in root-relative paths with `/` separators. Every path it
//! passes in is rebuilt component by component here and verified (after resolving symlinks) to
//! stay inside the root, so crafted input like `../..`, `C:\`, `\\server\x` or ADS names
//! (`a:stream`) is rejected before touching the disk.

use serde::{Deserialize, Serialize};
use std::fmt;
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};
use std::time::UNIX_EPOCH;

pub const FLAG_DIR: u8 = 1;
pub const FLAG_HIDDEN: u8 = 2;
pub const FLAG_SYMLINK: u8 = 4;

#[derive(Debug)]
pub enum FsError {
    InvalidPath(String),
    OutsideRoot,
    Io(io::Error),
}

impl fmt::Display for FsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FsError::InvalidPath(p) => write!(f, "invalid path: {p}"),
            FsError::OutsideRoot => write!(f, "path escapes the root"),
            FsError::Io(e) => write!(f, "{e}"),
        }
    }
}

impl From<io::Error> for FsError {
    fn from(e: io::Error) -> Self {
        FsError::Io(e)
    }
}

/// One directory entry: `(name, flags, mtimeMs, size)`.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Entry(pub String, pub u8, pub f64, pub u64);

impl Entry {
    pub fn name(&self) -> &str {
        &self.0
    }
    pub fn is_dir(&self) -> bool {
        self.1 & FLAG_DIR != 0
    }
}

#[derive(Debug, Clone)]
pub struct Scope {
    root: PathBuf,
}

impl Scope {
    pub fn new(root: &Path) -> io::Result<Self> {
        let root = fs::canonicalize(root)?;
        if !root.is_dir() {
            return Err(io::Error::new(io::ErrorKind::NotADirectory, "root is not a directory"));
        }
        Ok(Scope { root })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn root_name(&self) -> String {
        self.root
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| self.root.to_string_lossy().into_owned())
    }

    /// Normalizes a UI-supplied relative path into its canonical `a/b/c` form.
    pub fn normalize(rel: &str) -> Result<String, FsError> {
        if rel.contains('\0') {
            return Err(FsError::InvalidPath(rel.into()));
        }
        let mut parts: Vec<&str> = Vec::new();
        for part in rel.split(['/', '\\']) {
            match part {
                "" | "." => continue,
                ".." => return Err(FsError::OutsideRoot),
                p if p.contains(':') => return Err(FsError::InvalidPath(rel.into())),
                p => parts.push(p),
            }
        }
        Ok(parts.join("/"))
    }

    /// Resolves a relative path to an absolute one that is guaranteed to live under the root.
    pub fn resolve(&self, rel: &str) -> Result<PathBuf, FsError> {
        let norm = Self::normalize(rel)?;
        let mut abs = self.root.clone();
        for part in norm.split('/').filter(|p| !p.is_empty()) {
            let comp = Path::new(part).components().next();
            if !matches!(comp, Some(Component::Normal(_))) {
                return Err(FsError::InvalidPath(rel.into()));
            }
            abs.push(part);
        }
        // Resolve symlinks/junctions and make sure the real target is still inside the root.
        let real = fs::canonicalize(&abs)?;
        if !real.starts_with(&self.root) {
            return Err(FsError::OutsideRoot);
        }
        Ok(abs)
    }

    /// Converts an absolute path (from the walker/watcher) back to a root-relative one.
    pub fn to_rel(&self, abs: &Path) -> Option<String> {
        // canonicalize() yields `\\?\C:\...` on Windows while notify/callers may use `C:\...`.
        let plain = self.root.to_str().and_then(|s| s.strip_prefix(r"\\?\")).map(Path::new);
        let rel = abs.strip_prefix(&self.root).ok().or_else(|| plain.and_then(|p| abs.strip_prefix(p).ok()))?;
        let mut out = String::new();
        for c in rel.components() {
            if let Component::Normal(s) = c {
                if !out.is_empty() {
                    out.push('/');
                }
                out.push_str(&s.to_string_lossy());
            } else {
                return None;
            }
        }
        Some(out)
    }
}

/// The form of an absolute path a person would type: `canonicalize()` returns verbatim paths on
/// Windows (`\\?\C:\x`, `\\?\UNC\server\share`), which Explorer and most programs reject.
pub fn display_path(p: &Path) -> String {
    let s = p.to_string_lossy();
    if let Some(unc) = s.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{unc}")
    } else if let Some(plain) = s.strip_prefix(r"\\?\") {
        plain.to_string()
    } else {
        s.into_owned()
    }
}

pub fn parent_rel(rel: &str) -> &str {
    rel.rfind('/').map(|i| &rel[..i]).unwrap_or("")
}

pub fn name_of(rel: &str) -> &str {
    rel.rfind('/').map(|i| &rel[i + 1..]).unwrap_or(rel)
}

pub fn mtime_ms(meta: &fs::Metadata) -> f64 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as f64)
        .unwrap_or(0.0)
}

#[cfg(windows)]
fn os_hidden(meta: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    meta.file_attributes() & 0x2 != 0
}

/// `chflags hidden` (UF_HIDDEN), which is what hides `~/Library` in Finder.
#[cfg(target_os = "macos")]
fn os_hidden(meta: &fs::Metadata) -> bool {
    use std::os::macos::fs::MetadataExt;
    meta.st_flags() & 0x8000 != 0
}

#[cfg(not(any(windows, target_os = "macos")))]
fn os_hidden(_meta: &fs::Metadata) -> bool {
    false
}

#[cfg(unix)]
pub fn file_id(meta: &fs::Metadata) -> Option<i64> {
    use std::os::unix::fs::MetadataExt;
    Some(meta.ino() as i64)
}

#[cfg(not(unix))]
pub fn file_id(_meta: &fs::Metadata) -> Option<i64> {
    None // needs an open handle on Windows; too costly per entry for the MVP
}

pub fn flags_for(name: &str, meta: &fs::Metadata, is_symlink: bool) -> u8 {
    let mut flags = 0;
    if meta.is_dir() {
        flags |= FLAG_DIR;
    }
    if name.starts_with('.') || os_hidden(meta) {
        flags |= FLAG_HIDDEN;
    }
    if is_symlink {
        flags |= FLAG_SYMLINK;
    }
    flags
}

/// Stable, case-insensitive ordering used everywhere (matches the reference: `.git`,
/// `.gitignore`, `build`, `Info.plist`, `README.md`, ...).
pub fn sort_entries(entries: &mut [Entry]) {
    entries.sort_by_cached_key(|e| (e.0.to_lowercase(), e.0.clone()));
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SortKey {
    #[default]
    Name,
    /// Newest first.
    Modified,
    /// Largest first; folders (size 0) keep name order among themselves.
    Size,
}

/// How a listing is presented: which entries are kept and in what order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ListOptions {
    pub show_hidden: bool,
    pub sort: SortKey,
    pub folders_first: bool,
}

impl Default for ListOptions {
    fn default() -> Self {
        ListOptions { show_hidden: true, sort: SortKey::Name, folders_first: false }
    }
}

/// Filters and orders a name-sorted listing. Ties always fall back to name order.
pub fn present(mut entries: Vec<Entry>, opts: &ListOptions) -> Vec<Entry> {
    if !opts.show_hidden {
        entries.retain(|e| e.1 & FLAG_HIDDEN == 0);
    }
    match opts.sort {
        SortKey::Name => {}
        SortKey::Modified => entries.sort_by(|a, b| b.2.total_cmp(&a.2)),
        SortKey::Size => entries.sort_by_key(|e| std::cmp::Reverse(e.3)),
    }
    if opts.folders_first {
        entries.sort_by_key(|e| !e.is_dir());
    }
    entries
}

pub fn entry_for(path: &Path, name: String) -> io::Result<Entry> {
    let lmeta = fs::symlink_metadata(path)?;
    let is_symlink = lmeta.file_type().is_symlink();
    let meta = if is_symlink { fs::metadata(path).unwrap_or(lmeta) } else { lmeta };
    let flags = flags_for(&name, &meta, is_symlink);
    Ok(Entry(name, flags, mtime_ms(&meta), if meta.is_dir() { 0 } else { meta.len() }))
}

/// Lists a single directory (never recursive).
pub fn list_dir(abs: &Path) -> io::Result<Vec<Entry>> {
    let mut out = Vec::new();
    for item in fs::read_dir(abs)? {
        let Ok(item) = item else { continue };
        let name = item.file_name().to_string_lossy().into_owned();
        let Ok(ft) = item.file_type() else { continue };
        let entry = if ft.is_symlink() {
            entry_for(&item.path(), name)
        } else {
            // On Windows DirEntry::metadata comes straight from FindNextFile: no extra syscall.
            item.metadata().map(|m| Entry(name.clone(), flags_for(&name, &m, false), mtime_ms(&m), if m.is_dir() { 0 } else { m.len() }))
        };
        if let Ok(e) = entry {
            out.push(e);
        }
    }
    sort_entries(&mut out);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_rejects_traversal_and_drive_prefixes() {
        assert_eq!(Scope::normalize("a/b/../c").unwrap_err().to_string(), "path escapes the root");
        assert!(Scope::normalize("..").is_err());
        assert!(Scope::normalize("C:\\Windows").is_err());
        assert!(Scope::normalize("file.txt:stream").is_err());
        assert!(Scope::normalize("a\0b").is_err());
        assert_eq!(Scope::normalize("/a//b/./c/").unwrap(), "a/b/c");
        assert_eq!(Scope::normalize("a\\b").unwrap(), "a/b");
        assert_eq!(Scope::normalize("").unwrap(), "");
    }

    #[test]
    fn resolve_stays_inside_root() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("inner/deep")).unwrap();
        let scope = Scope::new(dir.path()).unwrap();
        let p = scope.resolve("inner/deep").unwrap();
        assert!(p.ends_with("deep"));
        assert!(scope.resolve("inner/../..").is_err());
        assert!(scope.resolve("missing").is_err());
        assert_eq!(scope.to_rel(&scope.root().join("inner").join("deep")).unwrap(), "inner/deep");
    }

    #[test]
    fn list_dir_sorts_case_insensitively_and_flags_entries() {
        let dir = tempfile::tempdir().unwrap();
        for f in ["README.md", "build.sh", ".gitignore", "Info.plist"] {
            fs::write(dir.path().join(f), "x").unwrap();
        }
        fs::create_dir(dir.path().join("scripts")).unwrap();
        let entries = list_dir(dir.path()).unwrap();
        let names: Vec<_> = entries.iter().map(|e| e.name()).collect();
        assert_eq!(names, [".gitignore", "build.sh", "Info.plist", "README.md", "scripts"]);
        assert!(entries[0].1 & FLAG_HIDDEN != 0);
        assert!(entries[4].is_dir());
        assert_eq!(entries[1].3, 1);
    }

    #[test]
    fn present_filters_hidden_and_sorts_stably() {
        let e = |n: &str, flags: u8, mtime: f64, size: u64| Entry(n.into(), flags, mtime, size);
        let listing = vec![
            e(".env", FLAG_HIDDEN, 5.0, 10),
            e("a.txt", 0, 1.0, 300),
            e("b", FLAG_DIR, 3.0, 0),
            e("c.txt", 0, 3.0, 300),
            e("d", FLAG_DIR, 9.0, 0),
        ];
        let names = |v: Vec<Entry>| v.into_iter().map(|e| e.0).collect::<Vec<_>>();
        assert_eq!(names(present(listing.clone(), &ListOptions::default())), [".env", "a.txt", "b", "c.txt", "d"]);
        let hide = ListOptions { show_hidden: false, ..ListOptions::default() };
        assert_eq!(names(present(listing.clone(), &hide)), ["a.txt", "b", "c.txt", "d"]);
        let newest = ListOptions { sort: SortKey::Modified, ..ListOptions::default() };
        assert_eq!(names(present(listing.clone(), &newest)), ["d", ".env", "b", "c.txt", "a.txt"]);
        let largest = ListOptions { sort: SortKey::Size, folders_first: true, ..ListOptions::default() };
        assert_eq!(names(present(listing, &largest)), ["b", "d", "a.txt", "c.txt", ".env"]);
    }

    #[test]
    fn display_path_strips_verbatim_prefixes() {
        assert_eq!(display_path(Path::new(r"\\?\C:\Users\x")), r"C:\Users\x");
        assert_eq!(display_path(Path::new(r"\\?\UNC\srv\share\a")), r"\\srv\share\a");
        assert_eq!(display_path(Path::new("/home/x")), "/home/x");
    }

    #[test]
    fn rel_helpers() {
        assert_eq!(parent_rel("a/b/c"), "a/b");
        assert_eq!(parent_rel("a"), "");
        assert_eq!(name_of("a/b/c.txt"), "c.txt");
    }
}
