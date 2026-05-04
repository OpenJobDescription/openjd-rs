// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

use crate::hash::HashAlgorithm;
use crate::path_util::{is_absolute_path, normalize_path};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::marker::PhantomData;

// --- Helper for serde skip_serializing_if ---

fn is_false(v: &bool) -> bool {
    !v
}

// --- Path helpers for common_root ---

/// Returns the parent directory of a (normalized) path, using `/` separators.
///
/// - `/foo/bar/baz` → `/foo/bar`
/// - `/foo` → `/` (immediate child of absolute root)
/// - `/` → `/` (root's parent is itself; defensive — shouldn't arise in a file path)
/// - `C:/foo/bar` → `C:/foo`
/// - `C:/foo` → `C:/` (immediate child of drive root — preserve the slash so
///   the caller can distinguish the drive root from a bare drive specifier)
/// - `foo/bar` → `foo` (relative)
/// - `foo` → `"."` (immediate child of the relative-path root)
/// - `"."` → `"."` (relative root's parent is itself; defensive)
/// - `""` → `""` (defensive)
fn parent_dir(path: &str) -> &str {
    if path.is_empty() || path == "/" || path == "." {
        return path;
    }
    match path.rfind('/') {
        None => ".",    // relative, no slash: parent is the relative root
        Some(0) => "/", // absolute, single slash at start: parent is root
        Some(2) if is_drive_letter_prefix(path) => &path[..3], // `C:/foo` → `C:/`
        Some(i) => &path[..i],
    }
}

/// Returns true if `path` starts with a Windows drive-letter prefix like
/// `C:/`. Used to recognize drive roots so their trailing `/` is preserved
/// (to distinguish the drive root `C:/` from the bare drive `C:`).
fn is_drive_letter_prefix(path: &str) -> bool {
    let bytes = path.as_bytes();
    bytes.len() >= 3 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' && bytes[2] == b'/'
}

/// Returns true if no entry in `live_paths` is a strict descendant of `dir`.
///
/// A path `p` is a strict descendant of `dir` when `p != dir` and one of:
/// - `dir == "/"` (absolute root contains every absolute path)
/// - `dir == "."` (relative root contains every relative path — i.e., every
///   path that is not itself absolute or another copy of `.`)
/// - `p` starts with `dir` followed by `/`
fn dir_is_empty(dir: &str, live_paths: &HashSet<&str>) -> bool {
    if dir == "/" {
        // Absolute root: any other absolute path (starting with `/`) is a descendant.
        !live_paths.iter().any(|p| *p != dir && p.starts_with('/'))
    } else if dir == "." {
        // Relative root: any other *relative* path is a descendant. A path is
        // relative if it does not start with `/` and is not a drive-letter
        // absolute path like `C:/...`.
        !live_paths
            .iter()
            .any(|p| *p != dir && !p.starts_with('/') && !is_drive_letter_prefix(p))
    } else {
        let prefix = format!("{dir}/");
        !live_paths
            .iter()
            .any(|p| *p != dir && p.starts_with(&prefix))
    }
}

/// Returns the longest directory prefix that is an ancestor (or equal to)
/// every path in `paths`. `paths` must be non-empty.
///
/// The result has no trailing slash, except when the result is a root path
/// (POSIX `/` or a Windows drive root like `C:/`). If the computed boundary
/// is an empty byte range (no shared directory prefix among relative paths),
/// returns `"."` — the relative root — so the result is always a valid path.
fn longest_common_dir_prefix(paths: &[&str]) -> String {
    debug_assert!(!paths.is_empty());

    // Step 1: find the longest common byte prefix `L` across all paths.
    let first = paths[0];
    let mut common_len = first.len();
    for &p in &paths[1..] {
        let a = first.as_bytes();
        let b = p.as_bytes();
        let upto = common_len.min(b.len());
        let mut i = 0;
        while i < upto && a[i] == b[i] {
            i += 1;
        }
        common_len = i;
        if common_len == 0 {
            break;
        }
    }

    // Step 2: determine whether byte `common_len` sits at a directory boundary.
    //
    // The common byte prefix is a valid directory iff every path that is
    // strictly longer than `common_len` has `/` at position `common_len`.
    // (If all paths have exactly length `common_len`, that's also a valid
    // boundary — they're all equal, so the common path IS the result.)
    //
    // Otherwise, the common byte prefix ends mid-component (e.g. `/app/ba`
    // from `["/app/bar", "/app/baz"]`, or `/a` from `["/a", "/ab"]`) and we
    // must walk back to the last `/`.
    let at_boundary = paths
        .iter()
        .filter(|p| p.len() > common_len)
        .all(|p| p.as_bytes()[common_len] == b'/');

    let boundary = if at_boundary {
        common_len
    } else {
        // Walk back to the last `/` at or before common_len. If there is no
        // slash at all, truncate to zero.
        first[..common_len]
            .rfind('/')
            .map(|i| i + 1) // include the slash itself
            .unwrap_or(0)
    };

    // Step 3: strip trailing `/` unless the result *is* a root path.
    // Root paths keep their trailing `/`:
    //   - "/" is the POSIX absolute root
    //   - "C:/" (for any drive letter) is the root of a Windows drive
    //
    // For relative inputs that don't share any common directory prefix, the
    // boundary is 0 and the result would be an empty string. Return "." in
    // that case so the result is always a valid path that can be passed to
    // operations like [`subtree_snapshot`] as the subtree root.
    let result = &first[..boundary];
    if is_root_path(result) {
        result.to_string()
    } else if result.is_empty() {
        ".".to_string()
    } else if let Some(stripped) = result.strip_suffix('/') {
        stripped.to_string()
    } else {
        result.to_string()
    }
}

/// Returns true if the path is a root path whose trailing `/` must be kept:
/// either absolute root `/` or a Windows drive root like `C:/`.
fn is_root_path(path: &str) -> bool {
    path == "/" || (path.len() == 3 && is_drive_letter_prefix(path))
}

// --- Entry types ---

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileEntry {
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hash: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mtime: Option<u64>,
    #[serde(
        rename = "chunkhashes",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub chunk_hashes: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub symlink_target: Option<String>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub runnable: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub deleted: bool,
}

impl FileEntry {
    pub fn new(path: impl Into<String>) -> Self {
        Self {
            path: normalize_path(&path.into()),
            hash: None,
            size: None,
            mtime: None,
            chunk_hashes: None,
            symlink_target: None,
            runnable: false,
            deleted: false,
        }
    }

    pub fn file(path: impl Into<String>, size: u64, mtime: u64) -> Self {
        Self {
            size: Some(size),
            mtime: Some(mtime),
            ..Self::new(path)
        }
    }

    pub fn symlink(path: impl Into<String>, target: impl Into<String>) -> Self {
        Self {
            symlink_target: Some(normalize_path(&target.into())),
            ..Self::new(path)
        }
    }

    pub fn deleted(path: impl Into<String>) -> Self {
        Self {
            deleted: true,
            ..Self::new(path)
        }
    }
}

impl std::fmt::Display for FileEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.deleted {
            write!(f, "{} (deleted)", self.path)
        } else if let Some(ref target) = self.symlink_target {
            write!(f, "{} -> {}", self.path, target)
        } else {
            write!(f, "{} ({}B)", self.path, self.size.unwrap_or(0))
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DirEntry {
    pub path: String,
    #[serde(default, skip_serializing_if = "is_false")]
    pub deleted: bool,
}

impl DirEntry {
    pub fn new(path: impl Into<String>) -> Self {
        Self {
            path: normalize_path(&path.into()),
            deleted: false,
        }
    }

    pub fn deleted(path: impl Into<String>) -> Self {
        Self {
            path: normalize_path(&path.into()),
            deleted: true,
        }
    }
}

impl std::fmt::Display for DirEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.deleted {
            write!(f, "{} (deleted)", self.path)
        } else {
            write!(f, "{}", self.path)
        }
    }
}

// --- Marker types ---

#[derive(Clone, Debug)]
pub struct Abs;
#[derive(Clone, Debug)]
pub struct Rel;
#[derive(Clone, Debug)]
pub struct Full;
#[derive(Clone, Debug)]
pub struct Diff;

// --- Manifest ---

/// A content-addressed file tree manifest parameterized by path style and kind.
///
/// The phantom type parameters `P` and `K` encode constraints at the type level:
/// - `P`: `Abs` (absolute paths) or `Rel` (relative paths)
/// - `K`: `Full` (no deleted entries) or `Diff` (deleted entries allowed)
///
/// **Important:** These phantom types are `#[serde(skip)]`, so deserializing JSON
/// directly via `serde_json::from_str::<Manifest<P, K>>()` will succeed regardless
/// of whether the paths actually match `P` or the entries match `K`. Always use
/// the [`decode_v2023`](crate::decode_v2023) / [`decode_v2025`](crate::decode_v2025)
/// functions to deserialize manifests — they select the correct type based on the
/// spec version field. If you must deserialize directly, call [`validate()`](Manifest::validate)
/// on the result to enforce the phantom type constraints at runtime.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Manifest<P, K> {
    pub hash_alg: HashAlgorithm,
    pub files: Vec<FileEntry>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dirs: Vec<DirEntry>,
    pub total_size: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_manifest_hash: Option<String>,
    pub file_chunk_size_bytes: i64,
    #[serde(skip)]
    _phantom: PhantomData<(P, K)>,
}

pub type AbsSnapshot = Manifest<Abs, Full>;
pub type AbsSnapshotDiff = Manifest<Abs, Diff>;
pub type Snapshot = Manifest<Rel, Full>;
pub type SnapshotDiff = Manifest<Rel, Diff>;

// --- Validation traits ---

pub trait ValidatePaths {
    fn validate_path(path: &str) -> crate::Result<()>;
}

impl ValidatePaths for Abs {
    fn validate_path(path: &str) -> crate::Result<()> {
        if path.is_empty() {
            return Err(crate::SnapshotError::Validation(
                "path must not be empty".into(),
            ));
        }
        if !is_absolute_path(path) {
            return Err(crate::SnapshotError::Validation(format!(
                "expected absolute path, got: {path}"
            )));
        }
        Ok(())
    }
}

impl ValidatePaths for Rel {
    fn validate_path(path: &str) -> crate::Result<()> {
        if path.is_empty() {
            return Err(crate::SnapshotError::Validation(
                "path must not be empty".into(),
            ));
        }
        if is_absolute_path(path) {
            return Err(crate::SnapshotError::Validation(format!(
                "expected relative path, got: {path}"
            )));
        }
        Ok(())
    }
}

pub trait ValidateKind {
    fn validate_deleted(deleted: bool) -> crate::Result<()>;
}

impl ValidateKind for Full {
    fn validate_deleted(deleted: bool) -> crate::Result<()> {
        if deleted {
            return Err(crate::SnapshotError::Validation(
                "full manifest must not contain deleted entries".into(),
            ));
        }
        Ok(())
    }
}

impl ValidateKind for Diff {
    fn validate_deleted(_deleted: bool) -> crate::Result<()> {
        Ok(())
    }
}

// --- Manifest impl ---

impl<P, K> Manifest<P, K> {
    pub fn new(hash_alg: HashAlgorithm, file_chunk_size_bytes: i64) -> Self {
        Self {
            hash_alg,
            files: Vec::new(),
            dirs: Vec::new(),
            total_size: 0,
            parent_manifest_hash: None,
            file_chunk_size_bytes,
            _phantom: PhantomData,
        }
    }

    pub fn with_files(mut self, files: Vec<FileEntry>) -> Self {
        self.files = files;
        self.recompute_total_size();
        self
    }

    pub fn with_dirs(mut self, dirs: Vec<DirEntry>) -> Self {
        self.dirs = dirs;
        self
    }

    pub fn with_parent_hash(mut self, hash: Option<String>) -> Self {
        self.parent_manifest_hash = hash;
        self
    }

    pub fn clear_hashes(&mut self) {
        for f in &mut self.files {
            if f.symlink_target.is_none() && !f.deleted {
                f.hash = None;
                f.chunk_hashes = None;
            }
        }
    }

    pub fn recompute_total_size(&mut self) {
        self.total_size = self
            .files
            .iter()
            .filter(|f| !f.deleted && f.symlink_target.is_none())
            .filter_map(|f| f.size)
            .sum();
    }

    /// Returns the longest directory prefix that is an ancestor (or equal to)
    /// every meaningful non-deleted path in this manifest, or `None` if the
    /// manifest contains no such paths.
    ///
    /// The rules for which paths contribute to the common prefix are:
    ///
    /// - **Files** always contribute their **parent directory**.
    /// - **Empty directories** — dirs with no strict descendants declared
    ///   elsewhere in the manifest — contribute **themselves**. This is
    ///   because an empty dir can only be preserved by a [`subtree_snapshot`]
    ///   operation whose root is at or above that dir.
    /// - **Non-empty directories** — dirs that have at least one other file
    ///   or dir entry under them in the manifest — are **ignored**. They are
    ///   already implied by their descendants, and including them would
    ///   collapse the result toward the filesystem root whenever a caller
    ///   happens to list a dir like `/` that contains other entries.
    /// - **Deleted entries** (file or dir) are ignored.
    ///
    /// The returned path has no trailing slash, except when the result is a
    /// root path (absolute `/` or a Windows drive root like `C:/`). For
    /// relative manifests, the relative root is spelled `"."` — a file at
    /// `baz.txt` (no path separators) yields `"."`, and relative entries with
    /// no shared directory prefix also yield `"."`.
    ///
    /// [`subtree_snapshot`]: crate::subtree_snapshot
    ///
    /// # Examples
    ///
    /// | Manifest                                              | Common root |
    /// |-------------------------------------------------------|-------------|
    /// | `[file /foo/bar/baz.txt]`                             | `/foo/bar`  |
    /// | `[dir /foo/bar]`                                      | `/foo/bar`  |
    /// | `[file /a/b.txt, file /a/c.txt]`                      | `/a`        |
    /// | `[file /a/b.txt, dir /a/b]` (dir is non-empty)        | `/a`        |
    /// | `[file /a/b.txt, dir /c]` (dir is empty)              | `/`         |
    /// | `[file foo/bar/baz.txt]` (relative)                   | `foo/bar`   |
    /// | `[file baz.txt]` (relative, at manifest root)         | `.`         |
    /// | `[file foo/a.txt, file bar/b.txt]` (no shared subdir) | `.`         |
    /// | `[]`                                                  | `None`      |
    ///
    /// # Composition
    ///
    /// ```no_run
    /// # use openjd_snapshots::*;
    /// # fn example(abs: &AbsSnapshot) -> Result<Snapshot> {
    /// let root = abs.common_root().expect("manifest is non-empty");
    /// subtree_snapshot(abs, &root, SymlinkPolicy::CollapseEscaping)
    /// # }
    /// ```
    pub fn common_root(&self) -> Option<String> {
        // Build the set of all non-deleted paths in the manifest.
        // Used to determine whether each dir entry is "empty" (has no
        // declared descendants).
        let live_paths: HashSet<&str> = self
            .files
            .iter()
            .filter(|f| !f.deleted)
            .map(|f| f.path.as_str())
            .chain(
                self.dirs
                    .iter()
                    .filter(|d| !d.deleted)
                    .map(|d| d.path.as_str()),
            )
            .collect();

        // Collect contributor paths. Files contribute their parent directory;
        // empty dirs contribute themselves.
        //
        // We collect into owned strings so borrows across the two sources
        // (file parents, which are computed strings; and dir paths, which
        // borrow from `self.dirs`) have a uniform type.
        let mut contributors: Vec<String> = Vec::new();
        for f in &self.files {
            if f.deleted {
                continue;
            }
            contributors.push(parent_dir(&f.path).to_string());
        }
        for d in &self.dirs {
            if d.deleted {
                continue;
            }
            if dir_is_empty(&d.path, &live_paths) {
                contributors.push(d.path.clone());
            }
        }

        if contributors.is_empty() {
            return None;
        }
        let refs: Vec<&str> = contributors.iter().map(String::as_str).collect();
        Some(longest_common_dir_prefix(&refs))
    }
}

impl<P: ValidatePaths, K: ValidateKind> Manifest<P, K> {
    pub fn validate(&self) -> crate::Result<()> {
        for f in &self.files {
            P::validate_path(&f.path)?;
            K::validate_deleted(f.deleted)?;
            if f.deleted {
                if f.size.is_some()
                    || f.mtime.is_some()
                    || f.hash.is_some()
                    || f.chunk_hashes.is_some()
                    || f.symlink_target.is_some()
                {
                    return Err(crate::SnapshotError::Validation(format!(
                        "deleted entry must have no data fields: {}",
                        f.path
                    )));
                }
            } else if let Some(ref target) = f.symlink_target {
                P::validate_path(target).map_err(|_| {
                    crate::SnapshotError::Validation(format!(
                        "symlink_target path style mismatch for {}: {}",
                        f.path, target
                    ))
                })?;
                if f.hash.is_some() || f.chunk_hashes.is_some() {
                    return Err(crate::SnapshotError::Validation(format!(
                        "symlink must not have hash or chunk_hashes: {}",
                        f.path
                    )));
                }
            } else {
                // Regular file: at most one of hash, chunk_hashes, symlink_target
                let count = [f.hash.is_some(), f.chunk_hashes.is_some()]
                    .iter()
                    .filter(|&&v| v)
                    .count();
                if count > 1 {
                    return Err(crate::SnapshotError::Validation(format!(
                        "regular file must have at most one of hash/chunk_hashes: {}",
                        f.path
                    )));
                }
                if f.size.is_none() || f.mtime.is_none() {
                    return Err(crate::SnapshotError::Validation(format!(
                        "regular file must have size and mtime: {}",
                        f.path
                    )));
                }
            }
        }
        for d in &self.dirs {
            P::validate_path(&d.path)?;
            K::validate_deleted(d.deleted)?;
        }

        // Validate chunkhashes
        for f in &self.files {
            if let Some(ref ch) = f.chunk_hashes {
                let size = f.size.ok_or_else(|| {
                    crate::SnapshotError::Validation(format!(
                        "file with chunkhashes must have size: {}",
                        f.path
                    ))
                })?;
                if self.file_chunk_size_bytes == crate::hash::WHOLE_FILE_CHUNK_SIZE {
                    return Err(crate::SnapshotError::Validation(format!(
                        "file '{}' has chunkhashes but manifest has no chunking (fileChunkSizeBytes={})",
                        f.path, self.file_chunk_size_bytes
                    )));
                }
                let chunk_size = self.file_chunk_size_bytes as u64;
                if size <= chunk_size {
                    return Err(crate::SnapshotError::Validation(format!(
                        "file '{}' with chunkhashes must have size > {} (chunk size), got {}",
                        f.path, chunk_size, size
                    )));
                }
                let expected = ((size as f64) / (chunk_size as f64)).ceil() as usize;
                if ch.len() != expected {
                    return Err(crate::SnapshotError::Validation(format!(
                        "file '{}' with size {} should have {} chunks (chunk_size={}), got {}",
                        f.path,
                        size,
                        expected,
                        chunk_size,
                        ch.len()
                    )));
                }
            }
        }

        // Validate no duplicate paths across files and dirs
        let mut seen = HashSet::with_capacity(self.files.len() + self.dirs.len());
        for f in &self.files {
            if !seen.insert(f.path.as_str()) {
                return Err(crate::SnapshotError::Validation(format!(
                    "duplicate path: {}",
                    f.path
                )));
            }
        }
        for d in &self.dirs {
            if !seen.insert(d.path.as_str()) {
                return Err(crate::SnapshotError::Validation(format!(
                    "duplicate path: {}",
                    d.path
                )));
            }
        }

        Ok(())
    }
}

// --- SymlinkPolicy ---

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SymlinkPolicy {
    CollapseEscaping,
    CollapseAll,
    ExcludeEscaping,
    ExcludeAll,
    Preserve,
    TransitiveIncludeTargets,
}

impl std::fmt::Display for SymlinkPolicy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::CollapseEscaping => f.write_str("collapse_escaping"),
            Self::CollapseAll => f.write_str("collapse_all"),
            Self::ExcludeEscaping => f.write_str("exclude_escaping"),
            Self::ExcludeAll => f.write_str("exclude_all"),
            Self::Preserve => f.write_str("preserve"),
            Self::TransitiveIncludeTargets => f.write_str("transitive_include_targets"),
        }
    }
}

// --- ManifestRef trait ---

/// Trait for accessing manifest data regardless of path type (absolute or relative).
pub trait ManifestRef {
    fn files(&self) -> &[FileEntry];
    fn hash_alg(&self) -> HashAlgorithm;
    fn file_chunk_size_bytes(&self) -> i64;
}

// --- Enum wrappers ---

#[derive(Debug)]
pub enum AbsManifest {
    Snapshot(AbsSnapshot),
    Diff(AbsSnapshotDiff),
}

#[derive(Debug)]
pub enum RelManifest {
    Snapshot(Snapshot),
    Diff(SnapshotDiff),
}

macro_rules! impl_manifest_accessors {
    ($name:ident, $snap:ty, $diff:ty) => {
        impl $name {
            pub fn files(&self) -> &[FileEntry] {
                match self {
                    Self::Snapshot(m) => &m.files,
                    Self::Diff(m) => &m.files,
                }
            }
            pub fn dirs(&self) -> &[DirEntry] {
                match self {
                    Self::Snapshot(m) => &m.dirs,
                    Self::Diff(m) => &m.dirs,
                }
            }
            pub fn hash_alg(&self) -> HashAlgorithm {
                match self {
                    Self::Snapshot(m) => m.hash_alg,
                    Self::Diff(m) => m.hash_alg,
                }
            }
            pub fn file_chunk_size_bytes(&self) -> i64 {
                match self {
                    Self::Snapshot(m) => m.file_chunk_size_bytes,
                    Self::Diff(m) => m.file_chunk_size_bytes,
                }
            }
            pub fn total_size(&self) -> u64 {
                match self {
                    Self::Snapshot(m) => m.total_size,
                    Self::Diff(m) => m.total_size,
                }
            }
            pub fn parent_manifest_hash(&self) -> Option<&str> {
                match self {
                    Self::Snapshot(m) => m.parent_manifest_hash.as_deref(),
                    Self::Diff(m) => m.parent_manifest_hash.as_deref(),
                }
            }
        }

        impl ManifestRef for $name {
            fn files(&self) -> &[FileEntry] {
                self.files()
            }
            fn hash_alg(&self) -> HashAlgorithm {
                self.hash_alg()
            }
            fn file_chunk_size_bytes(&self) -> i64 {
                self.file_chunk_size_bytes()
            }
        }
    };
}

impl_manifest_accessors!(AbsManifest, AbsSnapshot, AbsSnapshotDiff);
impl_manifest_accessors!(RelManifest, Snapshot, SnapshotDiff);

// --- ManifestEntry enum for filter operations ---

pub enum ManifestEntry<'a> {
    File(&'a FileEntry),
    Dir(&'a DirEntry),
}

impl<'a> ManifestEntry<'a> {
    pub fn path(&self) -> &str {
        match self {
            Self::File(f) => &f.path,
            Self::Dir(d) => &d.path,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hash::HashAlgorithm;
    use crate::DEFAULT_FILE_CHUNK_SIZE;

    fn make_abs_snapshot(files: Vec<FileEntry>) -> AbsSnapshot {
        Manifest::new(HashAlgorithm::Xxh128, DEFAULT_FILE_CHUNK_SIZE).with_files(files)
    }

    fn make_rel_snapshot(files: Vec<FileEntry>) -> Snapshot {
        Manifest::new(HashAlgorithm::Xxh128, DEFAULT_FILE_CHUNK_SIZE).with_files(files)
    }

    #[test]
    fn abs_snapshot_valid() {
        let m = make_abs_snapshot(vec![FileEntry::file("/tmp/a.txt", 100, 1000)]);
        assert!(m.validate().is_ok());
    }

    #[test]
    fn rel_snapshot_valid() {
        let m = make_rel_snapshot(vec![FileEntry::file("src/main.rs", 200, 2000)]);
        assert!(m.validate().is_ok());
    }

    #[test]
    fn abs_snapshot_rejects_relative_path() {
        let m = make_abs_snapshot(vec![FileEntry::file("relative/path.txt", 10, 1)]);
        assert!(m.validate().is_err());
    }

    #[test]
    fn rel_snapshot_rejects_absolute_path() {
        let m = make_rel_snapshot(vec![FileEntry::file("/absolute/path.txt", 10, 1)]);
        assert!(m.validate().is_err());
    }

    #[test]
    fn rejects_empty_path_in_rel_manifest() {
        let m = make_rel_snapshot(vec![FileEntry::file("", 10, 1)]);
        let err = m.validate().unwrap_err();
        assert!(err.to_string().contains("must not be empty"), "{err}");
    }

    #[test]
    fn rejects_empty_path_in_abs_manifest() {
        let m = make_abs_snapshot(vec![FileEntry::file("", 10, 1)]);
        let err = m.validate().unwrap_err();
        assert!(err.to_string().contains("must not be empty"), "{err}");
    }

    #[test]
    fn full_manifest_rejects_deleted() {
        let m = make_abs_snapshot(vec![FileEntry::deleted("/tmp/gone.txt")]);
        assert!(m.validate().is_err());
    }

    #[test]
    fn diff_manifest_allows_deleted() {
        let m: AbsSnapshotDiff = Manifest::new(HashAlgorithm::Xxh128, DEFAULT_FILE_CHUNK_SIZE)
            .with_files(vec![FileEntry::deleted("/tmp/gone.txt")]);
        assert!(m.validate().is_ok());
    }

    #[test]
    fn clear_hashes_works() {
        let mut m = make_abs_snapshot(vec![
            {
                let mut f = FileEntry::file("/tmp/a.txt", 100, 1);
                f.hash = Some("abc".into());
                f
            },
            FileEntry::symlink("/tmp/link", "/tmp/target"),
        ]);
        m.clear_hashes();
        assert!(m.files[0].hash.is_none());
        // Symlink hash fields untouched (they were already None)
        assert!(m.files[1].symlink_target.is_some());
    }

    #[test]
    fn recompute_total_size_works() {
        let mut m: AbsSnapshotDiff = Manifest::new(HashAlgorithm::Xxh128, DEFAULT_FILE_CHUNK_SIZE)
            .with_files(vec![
                FileEntry::file("/tmp/a.txt", 100, 1),
                FileEntry::file("/tmp/b.txt", 200, 2),
                FileEntry::deleted("/tmp/c.txt"),
                FileEntry::symlink("/tmp/link", "/tmp/target"),
            ]);
        m.recompute_total_size();
        assert_eq!(m.total_size, 300);
    }

    #[test]
    fn file_entry_constructors_normalize_paths() {
        let f = FileEntry::new("a//b/../c");
        assert_eq!(f.path, "a/c");

        let f = FileEntry::file("x/./y", 10, 1);
        assert_eq!(f.path, "x/y");

        #[cfg(windows)]
        {
            let f = FileEntry::symlink("a\\b", "c\\d");
            assert_eq!(f.path, "a/b");
            assert_eq!(f.symlink_target.unwrap(), "c/d");
        }
        #[cfg(not(windows))]
        {
            let f = FileEntry::symlink("a\\b", "c\\d");
            assert_eq!(f.path, "a\\b");
            assert_eq!(f.symlink_target.unwrap(), "c\\d");
        }

        let f = FileEntry::deleted("/tmp/./gone");
        assert_eq!(f.path, "/tmp/gone");
    }

    #[test]
    fn serde_round_trip() {
        let m = make_rel_snapshot(vec![
            {
                let mut f = FileEntry::file("src/main.rs", 500, 12345);
                f.hash = Some("deadbeef".into());
                f.runnable = true;
                f
            },
            FileEntry::symlink("link", "target"),
        ]);

        let json = serde_json::to_string(&m).unwrap();
        let deserialized: Snapshot = serde_json::from_str(&json).unwrap();

        assert_eq!(m.files, deserialized.files);
        assert_eq!(m.dirs, deserialized.dirs);
        assert_eq!(m.hash_alg, deserialized.hash_alg);
        assert_eq!(m.total_size, deserialized.total_size);
        assert_eq!(m.file_chunk_size_bytes, deserialized.file_chunk_size_bytes);
        assert_eq!(m.parent_manifest_hash, deserialized.parent_manifest_hash);
    }

    #[test]
    fn serde_chunk_hashes_field_name() {
        let mut f = FileEntry::new("test.bin");
        f.chunk_hashes = Some(vec!["aaa".into(), "bbb".into()]);
        let json = serde_json::to_string(&f).unwrap();
        assert!(json.contains("\"chunkhashes\""));
        assert!(!json.contains("\"chunkHashes\""));
        assert!(!json.contains("\"chunk_hashes\""));
    }

    #[test]
    fn serde_skips_false_bools_and_empty_optional() {
        let f = FileEntry::new("test.txt");
        let json = serde_json::to_string(&f).unwrap();
        assert!(!json.contains("deleted"));
        assert!(!json.contains("runnable"));
        assert!(!json.contains("hash"));
    }

    #[test]
    fn validate_chunkhashes_correct_count() {
        let chunk_size = 256i64;
        let mut f = FileEntry::file("/tmp/big.bin", 1024, 1);
        f.chunk_hashes = Some(vec!["a".into(), "b".into(), "c".into(), "d".into()]);
        let m: AbsSnapshot = Manifest::new(HashAlgorithm::Xxh128, chunk_size).with_files(vec![f]);
        assert!(m.validate().is_ok());
    }

    #[test]
    fn validate_chunkhashes_wrong_count() {
        let chunk_size = 256i64;
        let mut f = FileEntry::file("/tmp/big.bin", 1024, 1);
        f.chunk_hashes = Some(vec!["a".into(), "b".into()]);
        let m: AbsSnapshot = Manifest::new(HashAlgorithm::Xxh128, chunk_size).with_files(vec![f]);
        assert!(m.validate().is_err());
    }

    #[test]
    fn validate_chunkhashes_with_whole_file_chunk_size() {
        use crate::hash::WHOLE_FILE_CHUNK_SIZE;
        let mut f = FileEntry::file("/tmp/big.bin", 1024, 1);
        f.chunk_hashes = Some(vec!["a".into()]);
        let m: AbsSnapshot =
            Manifest::new(HashAlgorithm::Xxh128, WHOLE_FILE_CHUNK_SIZE).with_files(vec![f]);
        assert!(m.validate().is_err());
    }

    #[test]
    fn validate_chunkhashes_size_not_larger_than_chunk() {
        let chunk_size = 1024i64;
        let mut f = FileEntry::file("/tmp/small.bin", 512, 1);
        f.chunk_hashes = Some(vec!["a".into()]);
        let m: AbsSnapshot = Manifest::new(HashAlgorithm::Xxh128, chunk_size).with_files(vec![f]);
        assert!(m.validate().is_err());
    }

    #[test]
    fn validate_chunkhashes_missing_size() {
        let chunk_size = 256i64;
        let mut f = FileEntry::new("/tmp/big.bin");
        f.mtime = Some(1);
        f.chunk_hashes = Some(vec!["a".into()]);
        let m: AbsSnapshot = Manifest::new(HashAlgorithm::Xxh128, chunk_size).with_files(vec![f]);
        assert!(m.validate().is_err());
    }

    #[test]
    fn manifest_ref_trait_works_for_abs_and_rel() {
        let abs = AbsManifest::Snapshot(
            Manifest::new(HashAlgorithm::Xxh128, DEFAULT_FILE_CHUNK_SIZE)
                .with_files(vec![FileEntry::file("/tmp/a.txt", 100, 1)]),
        );
        let rel = RelManifest::Snapshot(
            Manifest::new(HashAlgorithm::Xxh128, DEFAULT_FILE_CHUNK_SIZE)
                .with_files(vec![FileEntry::file("b.txt", 200, 2)]),
        );

        fn check(m: &dyn ManifestRef) {
            let _ = m.files();
            let _ = m.hash_alg();
            let _ = m.file_chunk_size_bytes();
        }

        check(&abs);
        check(&rel);

        assert_eq!(abs.files().len(), 1);
        assert_eq!(rel.files().len(), 1);
        assert_eq!(abs.hash_alg(), HashAlgorithm::Xxh128);
        assert_eq!(abs.file_chunk_size_bytes(), DEFAULT_FILE_CHUNK_SIZE);
    }

    #[test]
    fn abs_snapshot_rejects_relative_symlink_target() {
        let m = make_abs_snapshot(vec![FileEntry::symlink("/tmp/link", "relative/target")]);
        let err = m.validate().unwrap_err();
        assert!(err.to_string().contains("symlink_target"));
    }

    #[test]
    fn rel_snapshot_rejects_absolute_symlink_target() {
        let m = make_rel_snapshot(vec![FileEntry::symlink("link", "/absolute/target")]);
        let err = m.validate().unwrap_err();
        assert!(err.to_string().contains("symlink_target"));
    }

    #[test]
    fn abs_snapshot_accepts_absolute_symlink_target() {
        let m = make_abs_snapshot(vec![FileEntry::symlink("/tmp/link", "/tmp/target")]);
        assert!(m.validate().is_ok());
    }

    #[test]
    fn rel_snapshot_accepts_relative_symlink_target() {
        let m = make_rel_snapshot(vec![FileEntry::symlink("link", "target")]);
        assert!(m.validate().is_ok());
    }

    #[test]
    fn validate_chunkhashes_ceil_division() {
        // 1000 / 256 = 3.90625, ceil = 4 chunks
        let chunk_size = 256i64;
        let mut f = FileEntry::file("/tmp/big.bin", 1000, 1);
        f.chunk_hashes = Some(vec!["a".into(), "b".into(), "c".into(), "d".into()]);
        let m: AbsSnapshot = Manifest::new(HashAlgorithm::Xxh128, chunk_size).with_files(vec![f]);
        assert!(m.validate().is_ok());
    }

    // ----- common_root -----

    fn abs_manifest(files: Vec<FileEntry>, dirs: Vec<DirEntry>) -> AbsSnapshot {
        Manifest::new(HashAlgorithm::Xxh128, DEFAULT_FILE_CHUNK_SIZE)
            .with_files(files)
            .with_dirs(dirs)
    }

    fn rel_manifest(files: Vec<FileEntry>, dirs: Vec<DirEntry>) -> Snapshot {
        Manifest::new(HashAlgorithm::Xxh128, DEFAULT_FILE_CHUNK_SIZE)
            .with_files(files)
            .with_dirs(dirs)
    }

    fn abs_diff_manifest(files: Vec<FileEntry>, dirs: Vec<DirEntry>) -> AbsSnapshotDiff {
        Manifest::new(HashAlgorithm::Xxh128, DEFAULT_FILE_CHUNK_SIZE)
            .with_files(files)
            .with_dirs(dirs)
    }

    #[test]
    fn common_root_empty_manifest() {
        let m = abs_manifest(vec![], vec![]);
        assert_eq!(m.common_root(), None);
    }

    #[test]
    fn common_root_single_file_returns_parent_dir() {
        let m = abs_manifest(vec![FileEntry::file("/foo/bar/baz.txt", 10, 1)], vec![]);
        assert_eq!(m.common_root(), Some("/foo/bar".into()));
    }

    #[test]
    fn common_root_single_empty_dir_returns_the_dir_itself() {
        let m = abs_manifest(vec![], vec![DirEntry::new("/foo/bar")]);
        assert_eq!(m.common_root(), Some("/foo/bar".into()));
    }

    #[test]
    fn common_root_file_at_absolute_root_returns_root() {
        let m = abs_manifest(vec![FileEntry::file("/baz.txt", 10, 1)], vec![]);
        assert_eq!(m.common_root(), Some("/".into()));
    }

    #[test]
    fn common_root_relative_single_file_returns_relative_parent() {
        let m = rel_manifest(vec![FileEntry::file("foo/bar/baz.txt", 10, 1)], vec![]);
        assert_eq!(m.common_root(), Some("foo/bar".into()));
    }

    #[test]
    fn common_root_relative_single_file_at_root_returns_dot() {
        // A file with no path separators sits at the manifest's virtual root;
        // the canonical spelling of "the relative current directory" is ".".
        let m = rel_manifest(vec![FileEntry::file("baz.txt", 10, 1)], vec![]);
        assert_eq!(m.common_root(), Some(".".into()));
    }

    #[test]
    fn common_root_relative_multiple_files_at_root_returns_dot() {
        let m = rel_manifest(
            vec![
                FileEntry::file("a.txt", 10, 1),
                FileEntry::file("b.txt", 20, 1),
            ],
            vec![],
        );
        assert_eq!(m.common_root(), Some(".".into()));
    }

    #[test]
    fn common_root_relative_no_shared_subdir_returns_dot() {
        // Parents: foo and bar. No shared byte prefix, so the result walks
        // back to 0 bytes, which for relative inputs means `.`.
        let m = rel_manifest(
            vec![
                FileEntry::file("foo/a.txt", 10, 1),
                FileEntry::file("bar/b.txt", 20, 1),
            ],
            vec![],
        );
        assert_eq!(m.common_root(), Some(".".into()));
    }

    #[test]
    fn common_root_relative_single_dot_dir() {
        // A single DirEntry at "." means the manifest root itself is tracked
        // as an empty dir. It contributes itself.
        let m = rel_manifest(vec![], vec![DirEntry::new(".")]);
        assert_eq!(m.common_root(), Some(".".into()));
    }

    #[test]
    fn common_root_relative_dot_dir_with_nested_file_is_not_empty() {
        // dir "." is non-empty because the file "foo/x.txt" is a descendant
        // (every relative non-"." path is a descendant of "."). So dir "."
        // is ignored; only the file's parent ("foo") contributes.
        let m = rel_manifest(
            vec![FileEntry::file("foo/x.txt", 10, 1)],
            vec![DirEntry::new(".")],
        );
        assert_eq!(m.common_root(), Some("foo".into()));
    }

    #[test]
    fn common_root_relative_dot_dir_with_root_file_is_not_empty() {
        // dir "." plus a file directly at the root ("bar.txt"). The file's
        // parent is ".", and dir "." is non-empty (bar.txt is a descendant).
        // So dir "." is ignored; the file parent "." is the only contributor.
        let m = rel_manifest(
            vec![FileEntry::file("bar.txt", 10, 1)],
            vec![DirEntry::new(".")],
        );
        assert_eq!(m.common_root(), Some(".".into()));
    }

    #[test]
    fn common_root_relative_dir_empty_leaf() {
        // dir "foo/bar" is empty (no descendants). Common of {foo/bar} = foo/bar.
        let m = rel_manifest(vec![], vec![DirEntry::new("foo/bar")]);
        assert_eq!(m.common_root(), Some("foo/bar".into()));
    }

    #[test]
    fn common_root_relative_composes_with_subtree_snapshot() {
        // Integration check: a relative manifest with a single file `baz.txt`
        // at the root. common_root() returns ".", which subtree_snapshot
        // accepts as "the whole manifest" and returns the same files back.
        use crate::ops::subtree_rel_snapshot;
        let m = rel_manifest(vec![FileEntry::file("baz.txt", 10, 1)], vec![]);
        let root = m.common_root().expect("non-empty");
        assert_eq!(root, ".");
        let result = subtree_rel_snapshot(&m, &root, SymlinkPolicy::CollapseEscaping).unwrap();
        assert_eq!(result.files.len(), 1);
        assert_eq!(result.files[0].path, "baz.txt");
    }

    #[test]
    fn common_root_multiple_files_sharing_prefix() {
        let m = abs_manifest(
            vec![
                FileEntry::file("/a/b/c.txt", 10, 1),
                FileEntry::file("/a/b/d.txt", 20, 1),
            ],
            vec![],
        );
        assert_eq!(m.common_root(), Some("/a/b".into()));
    }

    #[test]
    fn common_root_multiple_files_sharing_deeper_prefix() {
        let m = abs_manifest(
            vec![
                FileEntry::file("/a/b/c/x.txt", 10, 1),
                FileEntry::file("/a/b/d/y.txt", 20, 1),
            ],
            vec![],
        );
        // Parents are /a/b/c and /a/b/d; common dir prefix is /a/b.
        assert_eq!(m.common_root(), Some("/a/b".into()));
    }

    #[test]
    fn common_root_multiple_files_no_shared_subdir_returns_root() {
        let m = abs_manifest(
            vec![
                FileEntry::file("/a/b.txt", 10, 1),
                FileEntry::file("/x/y.txt", 20, 1),
            ],
            vec![],
        );
        // Parents are /a and /x; common dir is /.
        assert_eq!(m.common_root(), Some("/".into()));
    }

    #[test]
    fn common_root_non_empty_dir_ignored() {
        // dir /a/b is non-empty because file /a/b/c.txt is under it.
        // Only the file's parent (/a/b) contributes; result is /a/b.
        let m = abs_manifest(
            vec![FileEntry::file("/a/b/c.txt", 10, 1)],
            vec![DirEntry::new("/a/b")],
        );
        assert_eq!(m.common_root(), Some("/a/b".into()));
    }

    #[test]
    fn common_root_root_dir_ignored_when_non_empty() {
        // This is the motivating case: dir / is non-empty (file is under it),
        // so it should NOT pull the common root down to /. Only the file's
        // parent contributes.
        let m = abs_manifest(
            vec![FileEntry::file("/a/b/c.txt", 10, 1)],
            vec![DirEntry::new("/")],
        );
        assert_eq!(m.common_root(), Some("/a/b".into()));
    }

    #[test]
    fn common_root_file_with_empty_dir_elsewhere() {
        // dir /c is empty (nothing in the manifest under it). File parent is /a.
        // Common of {/a, /c} is /.
        let m = abs_manifest(
            vec![FileEntry::file("/a/b.txt", 10, 1)],
            vec![DirEntry::new("/c")],
        );
        assert_eq!(m.common_root(), Some("/".into()));
    }

    #[test]
    fn common_root_with_nested_empty_leaf_dir() {
        // dir /a is non-empty (dir /a/b is under it). dir /a/b is empty.
        // File /a/c.txt contributes parent /a.
        // Contributors: {/a, /a/b} → common = /a.
        let m = abs_manifest(
            vec![FileEntry::file("/a/c.txt", 10, 1)],
            vec![DirEntry::new("/a"), DirEntry::new("/a/b")],
        );
        assert_eq!(m.common_root(), Some("/a".into()));
    }

    #[test]
    fn common_root_deleted_entries_ignored() {
        // Deleted file and deleted dir are ignored. Only live file contributes.
        let m = abs_diff_manifest(
            vec![
                FileEntry::file("/a/b.txt", 10, 1),
                FileEntry::deleted("/x/y.txt"),
            ],
            vec![DirEntry::deleted("/x")],
        );
        assert_eq!(m.common_root(), Some("/a".into()));
    }

    #[test]
    fn common_root_all_deleted_returns_none() {
        let m = abs_diff_manifest(
            vec![FileEntry::deleted("/a/b.txt")],
            vec![DirEntry::deleted("/c")],
        );
        assert_eq!(m.common_root(), None);
    }

    #[test]
    fn common_root_partial_component_not_confused_with_directory() {
        // Parents are /app/bar and /app/baz. The common byte prefix is /app/ba,
        // but the common *directory* prefix is /app.
        let m = abs_manifest(
            vec![
                FileEntry::file("/app/bar/x.txt", 10, 1),
                FileEntry::file("/app/baz/y.txt", 20, 1),
            ],
            vec![],
        );
        assert_eq!(m.common_root(), Some("/app".into()));
    }

    #[test]
    fn common_root_similar_names_not_confused() {
        // File parents are /a and /ab. Common directory is /.
        let m = abs_manifest(
            vec![
                FileEntry::file("/a/x.txt", 10, 1),
                FileEntry::file("/ab/y.txt", 20, 1),
            ],
            vec![],
        );
        assert_eq!(m.common_root(), Some("/".into()));
    }

    #[test]
    fn common_root_only_empty_dir_at_absolute_root() {
        // Degenerate but well-defined: one dir entry at "/", no files.
        let m = abs_manifest(vec![], vec![DirEntry::new("/")]);
        assert_eq!(m.common_root(), Some("/".into()));
    }

    #[test]
    fn common_root_windows_style_absolute_paths() {
        // Absolute Windows paths normalize to forward slashes via FileEntry::new.
        let m = abs_manifest(
            vec![
                FileEntry::file("C:/Users/Alice/a.txt", 10, 1),
                FileEntry::file("C:/Users/Alice/b.txt", 20, 1),
            ],
            vec![],
        );
        assert_eq!(m.common_root(), Some("C:/Users/Alice".into()));
    }

    #[test]
    fn common_root_windows_drive_letter_single_file() {
        let m = abs_manifest(vec![FileEntry::file("C:/foo.txt", 10, 1)], vec![]);
        // Parent of C:/foo.txt is the drive root C:/ (with trailing slash, to
        // distinguish from the bare drive specifier C: which means "current
        // directory on drive C").
        assert_eq!(m.common_root(), Some("C:/".into()));
    }

    #[test]
    fn common_root_windows_drive_root_from_cross_subdirs() {
        // Parents: C:/foo and C:/bar. Common byte prefix = "C:/" (3 bytes).
        // Neither parent has length 3, and at index 3 the bytes are 'f' and
        // 'b', so the algorithm walks back to the last '/'. That lands at
        // "C:/", which must be preserved (not stripped to bare "C:").
        let m = abs_manifest(
            vec![
                FileEntry::file("C:/foo/a.txt", 10, 1),
                FileEntry::file("C:/bar/b.txt", 20, 1),
            ],
            vec![],
        );
        assert_eq!(m.common_root(), Some("C:/".into()));
    }

    #[test]
    fn common_root_file_and_deeper_file_rooted_at_absolute() {
        // Parents: / and /b. Common = /.
        let m = abs_manifest(
            vec![
                FileEntry::file("/a.txt", 10, 1),
                FileEntry::file("/b/c.txt", 20, 1),
            ],
            vec![],
        );
        assert_eq!(m.common_root(), Some("/".into()));
    }

    #[test]
    fn common_root_empty_dir_deepens_root_beyond_file_parent() {
        // File /a/b.txt (parent /a) plus empty dir /a/b/c (contributes /a/b/c).
        // Common directory: /a.
        let m = abs_manifest(
            vec![FileEntry::file("/a/b.txt", 10, 1)],
            vec![DirEntry::new("/a/b/c")],
        );
        assert_eq!(m.common_root(), Some("/a".into()));
    }

    #[test]
    fn common_root_dir_is_empty_when_only_deleted_descendants() {
        // dir /a has no non-deleted descendants (only /a/b.txt which is deleted),
        // so dir /a counts as empty and contributes itself. The deleted file
        // contributes nothing. Common root = /a.
        let m = abs_diff_manifest(
            vec![FileEntry::deleted("/a/b.txt")],
            vec![DirEntry::new("/a")],
        );
        assert_eq!(m.common_root(), Some("/a".into()));
    }
}
