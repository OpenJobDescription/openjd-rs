use crate::hash::HashAlgorithm;
use crate::path_util::{is_absolute_path, normalize_path};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::marker::PhantomData;

// --- Helper for serde skip_serializing_if ---

fn is_false(v: &bool) -> bool {
    !v
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
            } else if f.symlink_target.is_some() {
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
    fn validate_chunkhashes_ceil_division() {
        // 1000 / 256 = 3.90625, ceil = 4 chunks
        let chunk_size = 256i64;
        let mut f = FileEntry::file("/tmp/big.bin", 1000, 1);
        f.chunk_hashes = Some(vec!["a".into(), "b".into(), "c".into(), "d".into()]);
        let m: AbsSnapshot = Manifest::new(HashAlgorithm::Xxh128, chunk_size).with_files(vec![f]);
        assert!(m.validate().is_ok());
    }
}
