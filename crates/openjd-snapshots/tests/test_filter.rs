// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Ported from deadline-cloud test_filter_manifest.py

use openjd_snapshots::{
    filter_manifest, AbsSnapshot, AbsSnapshotDiff, DirEntry, FileEntry, HashAlgorithm,
    IncludeExcludePathsFilter, Manifest, ManifestEntry, Snapshot, SnapshotDiff,
    DEFAULT_FILE_CHUNK_SIZE, WHOLE_FILE_CHUNK_SIZE,
};
use std::collections::HashSet;

// --- Helpers ---

fn hashed_file(path: &str, hash: &str, size: u64, mtime: u64) -> FileEntry {
    let mut f = FileEntry::file(path, size, mtime);
    f.hash = Some(hash.into());
    f
}

fn abs_snapshot_with(files: Vec<FileEntry>, dirs: Vec<DirEntry>) -> AbsSnapshot {
    Manifest::new(HashAlgorithm::Xxh128, DEFAULT_FILE_CHUNK_SIZE)
        .with_files(files)
        .with_dirs(dirs)
}

fn rel_snapshot_with(files: Vec<FileEntry>, dirs: Vec<DirEntry>) -> Snapshot {
    Manifest::new(HashAlgorithm::Xxh128, DEFAULT_FILE_CHUNK_SIZE)
        .with_files(files)
        .with_dirs(dirs)
}

fn abs_diff_with(files: Vec<FileEntry>, dirs: Vec<DirEntry>) -> AbsSnapshotDiff {
    Manifest::new(HashAlgorithm::Xxh128, DEFAULT_FILE_CHUNK_SIZE)
        .with_files(files)
        .with_dirs(dirs)
}

fn rel_diff_with(files: Vec<FileEntry>, dirs: Vec<DirEntry>) -> SnapshotDiff {
    Manifest::new(HashAlgorithm::Xxh128, DEFAULT_FILE_CHUNK_SIZE)
        .with_files(files)
        .with_dirs(dirs)
}

fn paths(m: &impl HasFiles) -> HashSet<String> {
    m.file_entries().iter().map(|f| f.path.clone()).collect()
}

fn dir_paths(m: &impl HasDirs) -> HashSet<String> {
    m.dir_entries().iter().map(|d| d.path.clone()).collect()
}

// Trait helpers to extract files/dirs generically
trait HasFiles {
    fn file_entries(&self) -> &[FileEntry];
}
trait HasDirs {
    fn dir_entries(&self) -> &[DirEntry];
}
impl<P, K> HasFiles for Manifest<P, K> {
    fn file_entries(&self) -> &[FileEntry] {
        &self.files
    }
}
impl<P, K> HasDirs for Manifest<P, K> {
    fn dir_entries(&self) -> &[DirEntry] {
        &self.dirs
    }
}

// --- TestMatchesPatterns ---

#[test]
fn matches_patterns_empty_patterns_matches_all() {
    let f = IncludeExcludePathsFilter::new(&[], &[]).unwrap();
    assert!(f.matches_path("any/path.txt"));
    assert!(f.matches_path("another.blend"));
}

#[test]
fn matches_patterns_include_pattern_matches() {
    let f = IncludeExcludePathsFilter::new(&["*.blend"], &[]).unwrap();
    assert!(f.matches_path("model.blend"));
    assert!(f.matches_path("scene.blend"));
}

#[test]
fn matches_patterns_include_pattern_no_match() {
    let f = IncludeExcludePathsFilter::new(&["*.blend"], &[]).unwrap();
    assert!(!f.matches_path("texture.png"));
    assert!(!f.matches_path("notes.txt"));
}

#[test]
fn matches_patterns_multiple_include_patterns() {
    let f = IncludeExcludePathsFilter::new(&["*.blend", "*.png"], &[]).unwrap();
    assert!(f.matches_path("model.blend"));
    assert!(f.matches_path("texture.png"));
    assert!(!f.matches_path("notes.txt"));
}

#[test]
fn matches_patterns_exclude_pattern_matches() {
    let f = IncludeExcludePathsFilter::new(&[], &["backup/*"]).unwrap();
    assert!(!f.matches_path("backup/file.txt"));
    assert!(!if f.matches_path("cache/data.bin") {
        false
    } else {
        // cache/* doesn't match backup/* pattern
        let f2 = IncludeExcludePathsFilter::new(&[], &["cache/*"]).unwrap();
        !f2.matches_path("cache/data.bin")
    });
}

#[test]
fn matches_patterns_exclude_backup() {
    let f = IncludeExcludePathsFilter::new(&[], &["backup/*"]).unwrap();
    assert!(!f.matches_path("backup/file.txt"));
}

#[test]
fn matches_patterns_exclude_cache() {
    let f = IncludeExcludePathsFilter::new(&[], &["cache/*"]).unwrap();
    assert!(!f.matches_path("cache/data.bin"));
}

#[test]
fn matches_patterns_exclude_no_match() {
    let f = IncludeExcludePathsFilter::new(&[], &["backup/*"]).unwrap();
    assert!(f.matches_path("src/file.txt"));
}

#[test]
fn matches_patterns_include_and_exclude_combined() {
    let f = IncludeExcludePathsFilter::new(&["*.blend"], &["backup/*"]).unwrap();
    assert!(f.matches_path("model.blend"));
    assert!(!f.matches_path("backup/old.blend"));
    assert!(!f.matches_path("texture.png"));
}

#[test]
fn matches_patterns_wildcard_patterns() {
    let f = IncludeExcludePathsFilter::new(&["*/*.txt"], &[]).unwrap();
    assert!(f.matches_path("subdir/file.txt"));
    assert!(!f.matches_path("file.txt"));
}

#[test]
fn matches_patterns_recursive_wildcard() {
    let f = IncludeExcludePathsFilter::new(&["*/*/*.txt"], &[]).unwrap();
    assert!(f.matches_path("a/b/c.txt"));
}

// --- TestIncludeExcludePathsFilter ---

#[test]
fn filter_obj_empty_patterns_matches_all() {
    let f = IncludeExcludePathsFilter::new(&[], &[]).unwrap();
    let entry = hashed_file("any/path.txt", "h1", 10, 1000);
    assert!(f.matches(&ManifestEntry::File(&entry)));
}

#[test]
fn filter_obj_include_pattern_matches() {
    let f = IncludeExcludePathsFilter::new(&["*.blend"], &[]).unwrap();
    let e1 = hashed_file("model.blend", "h1", 10, 1000);
    let e2 = hashed_file("texture.png", "h2", 20, 2000);
    assert!(f.matches(&ManifestEntry::File(&e1)));
    assert!(!f.matches(&ManifestEntry::File(&e2)));
}

#[test]
fn filter_obj_exclude_pattern_matches() {
    let f = IncludeExcludePathsFilter::new(&[], &["backup/*"]).unwrap();
    let e1 = hashed_file("src/main.py", "h1", 10, 1000);
    let e2 = hashed_file("backup/old.py", "h2", 20, 2000);
    assert!(f.matches(&ManifestEntry::File(&e1)));
    assert!(!f.matches(&ManifestEntry::File(&e2)));
}

#[test]
fn filter_obj_combined_patterns() {
    let f = IncludeExcludePathsFilter::new(&["*.blend"], &["backup/*"]).unwrap();
    let e1 = hashed_file("model.blend", "h1", 10, 1000);
    let e2 = hashed_file("backup/old.blend", "h2", 20, 2000);
    let e3 = hashed_file("texture.png", "h3", 30, 3000);
    assert!(f.matches(&ManifestEntry::File(&e1)));
    assert!(!f.matches(&ManifestEntry::File(&e2)));
    assert!(!f.matches(&ManifestEntry::File(&e3)));
}

#[test]
fn filter_obj_filters_directory_entries() {
    let f = IncludeExcludePathsFilter::new(&["src*"], &[]).unwrap();
    let d1 = DirEntry::new("src");
    let d2 = DirEntry::new("backup");
    assert!(f.matches(&ManifestEntry::Dir(&d1)));
    assert!(!f.matches(&ManifestEntry::Dir(&d2)));
}

#[test]
fn filter_obj_debug_repr() {
    let f = IncludeExcludePathsFilter::new(&["*.blend"], &["backup/*"]).unwrap();
    let repr = format!("{:?}", f);
    assert!(repr.contains("IncludeExcludePathsFilter"));
    assert!(repr.contains("*.blend"));
    assert!(repr.contains("backup/*"));
}

// --- TestFilterManifestAbsSnapshot ---

#[test]
fn abs_filter_with_include_pattern() {
    let m = abs_snapshot_with(
        vec![
            hashed_file("/project/model.blend", "hash1", 100, 1000),
            hashed_file("/project/texture.png", "hash2", 200, 2000),
            hashed_file("/project/notes.txt", "hash3", 50, 3000),
        ],
        vec![],
    );
    let f = IncludeExcludePathsFilter::new(&["*.blend"], &[]).unwrap();
    let filtered = filter_manifest(&m, &|e| f.matches(e));
    assert_eq!(filtered.files.len(), 1);
    assert_eq!(filtered.files[0].path, "/project/model.blend");
    assert_eq!(filtered.total_size, 100);
}

#[test]
fn abs_filter_with_exclude_pattern() {
    let m = abs_snapshot_with(
        vec![
            hashed_file("/src/main.py", "hash1", 100, 1000),
            hashed_file("/backup/old.py", "hash2", 200, 2000),
            hashed_file("/src/utils.py", "hash3", 50, 3000),
        ],
        vec![],
    );
    let f = IncludeExcludePathsFilter::new(&[], &["/backup/*"]).unwrap();
    let filtered = filter_manifest(&m, &|e| f.matches(e));
    assert_eq!(filtered.files.len(), 2);
    let p = paths(&filtered);
    assert!(p.contains("/src/main.py"));
    assert!(p.contains("/src/utils.py"));
    assert_eq!(filtered.total_size, 150);
}

#[test]
fn abs_filter_with_both_patterns() {
    let m = abs_snapshot_with(
        vec![
            hashed_file("/project/model.blend", "hash1", 100, 1000),
            hashed_file("/backup/old.blend", "hash2", 200, 2000),
            hashed_file("/project/texture.png", "hash3", 50, 3000),
        ],
        vec![],
    );
    let f = IncludeExcludePathsFilter::new(&["*.blend"], &["/backup/*"]).unwrap();
    let filtered = filter_manifest(&m, &|e| f.matches(e));
    assert_eq!(filtered.files.len(), 1);
    assert_eq!(filtered.files[0].path, "/project/model.blend");
}

#[test]
fn abs_filter_empty_patterns_returns_all() {
    let m = abs_snapshot_with(
        vec![
            hashed_file("/a.txt", "hash1", 10, 1000),
            hashed_file("/b.txt", "hash2", 20, 2000),
        ],
        vec![],
    );
    let f = IncludeExcludePathsFilter::new(&[], &[]).unwrap();
    let filtered = filter_manifest(&m, &|e| f.matches(e));
    assert_eq!(filtered.files.len(), 2);
}

#[test]
fn abs_filter_no_matches_returns_empty() {
    let m = abs_snapshot_with(
        vec![
            hashed_file("/a.txt", "hash1", 10, 1000),
            hashed_file("/b.txt", "hash2", 20, 2000),
        ],
        vec![],
    );
    let f = IncludeExcludePathsFilter::new(&["*.blend"], &[]).unwrap();
    let filtered = filter_manifest(&m, &|e| f.matches(e));
    assert_eq!(filtered.files.len(), 0);
    assert_eq!(filtered.total_size, 0);
}

#[test]
fn abs_filter_preserves_hash_algorithm() {
    let m = abs_snapshot_with(vec![hashed_file("/a.txt", "hash1", 10, 1000)], vec![]);
    let f = IncludeExcludePathsFilter::new(&[], &[]).unwrap();
    let filtered = filter_manifest(&m, &|e| f.matches(e));
    assert_eq!(filtered.hash_alg, HashAlgorithm::Xxh128);
}

#[test]
fn abs_filter_preserves_file_chunk_size_bytes_whole() {
    let mut m: AbsSnapshot = Manifest::new(HashAlgorithm::Xxh128, WHOLE_FILE_CHUNK_SIZE);
    m.files = vec![hashed_file("/a.txt", "hash1", 10, 1000)];
    m.recompute_total_size();
    let f = IncludeExcludePathsFilter::new(&[], &[]).unwrap();
    let filtered = filter_manifest(&m, &|e| f.matches(e));
    assert_eq!(filtered.file_chunk_size_bytes, WHOLE_FILE_CHUNK_SIZE);
}

#[test]
fn abs_filter_preserves_file_chunk_size_bytes_custom() {
    let custom: i64 = 128 * 1024 * 1024;
    let mut m: AbsSnapshot = Manifest::new(HashAlgorithm::Xxh128, custom);
    m.files = vec![hashed_file("/a.txt", "hash1", 10, 1000)];
    m.recompute_total_size();
    let f = IncludeExcludePathsFilter::new(&[], &[]).unwrap();
    let filtered = filter_manifest(&m, &|e| f.matches(e));
    assert_eq!(filtered.file_chunk_size_bytes, custom);
}

#[test]
fn abs_filter_preserves_entry_metadata() {
    let m = abs_snapshot_with(
        vec![hashed_file("/test.txt", "abc123", 42, 1234567890)],
        vec![],
    );
    let f = IncludeExcludePathsFilter::new(&[], &[]).unwrap();
    let filtered = filter_manifest(&m, &|e| f.matches(e));
    let entry = &filtered.files[0];
    assert_eq!(entry.path, "/test.txt");
    assert_eq!(entry.hash.as_deref(), Some("abc123"));
    assert_eq!(entry.size, Some(42));
    assert_eq!(entry.mtime, Some(1234567890));
}

#[test]
fn abs_filter_does_not_mutate_original() {
    let m = abs_snapshot_with(
        vec![
            hashed_file("/keep.txt", "hash1", 10, 1000),
            hashed_file("/remove.txt", "hash2", 20, 2000),
        ],
        vec![],
    );
    let original_count = m.files.len();
    let f = IncludeExcludePathsFilter::new(&["/keep.txt"], &[]).unwrap();
    let _ = filter_manifest(&m, &|e| f.matches(e));
    assert_eq!(m.files.len(), original_count);
}

// --- TestFilterManifestRelSnapshot ---

#[test]
fn rel_filter_files_with_include_pattern() {
    let m = rel_snapshot_with(
        vec![
            hashed_file("model.blend", "h1", 100, 1000),
            hashed_file("texture.png", "h2", 200, 2000),
        ],
        vec![],
    );
    let f = IncludeExcludePathsFilter::new(&["*.blend"], &[]).unwrap();
    let filtered = filter_manifest(&m, &|e| f.matches(e));
    assert_eq!(filtered.files.len(), 1);
    assert_eq!(filtered.files[0].path, "model.blend");
}

#[test]
fn rel_filter_directories() {
    let m = rel_snapshot_with(
        vec![hashed_file("src/main.py", "h1", 100, 1000)],
        vec![
            DirEntry::new("src"),
            DirEntry::new("backup"),
            DirEntry::new("cache"),
        ],
    );
    let f = IncludeExcludePathsFilter::new(&["src*"], &[]).unwrap();
    let filtered = filter_manifest(&m, &|e| f.matches(e));
    let dp = dir_paths(&filtered);
    assert_eq!(dp, ["src".to_string()].into_iter().collect());
}

#[test]
fn rel_filter_symlinks() {
    let m = rel_snapshot_with(
        vec![
            FileEntry::symlink("link.blend", "target.blend"),
            FileEntry::symlink("link.png", "target.png"),
        ],
        vec![],
    );
    let f = IncludeExcludePathsFilter::new(&["*.blend"], &[]).unwrap();
    let filtered = filter_manifest(&m, &|e| f.matches(e));
    assert_eq!(filtered.files.len(), 1);
    assert_eq!(filtered.files[0].path, "link.blend");
    assert_eq!(
        filtered.files[0].symlink_target.as_deref(),
        Some("target.blend")
    );
}

#[test]
fn rel_filter_preserves_runnable() {
    let mut fe = hashed_file("script.sh", "h1", 100, 1000);
    fe.runnable = true;
    let m = rel_snapshot_with(vec![fe], vec![]);
    let f = IncludeExcludePathsFilter::new(&[], &[]).unwrap();
    let filtered = filter_manifest(&m, &|e| f.matches(e));
    assert!(filtered.files[0].runnable);
}

#[test]
fn rel_filter_preserves_chunkhashes() {
    let mut fe = FileEntry::file("large.bin", 512 * 1024 * 1024, 1000);
    fe.chunk_hashes = Some(vec!["chunk1".into(), "chunk2".into()]);
    let m = rel_snapshot_with(vec![fe], vec![]);
    let f = IncludeExcludePathsFilter::new(&[], &[]).unwrap();
    let filtered = filter_manifest(&m, &|e| f.matches(e));
    assert_eq!(
        filtered.files[0].chunk_hashes.as_deref(),
        Some(&["chunk1".to_string(), "chunk2".to_string()][..])
    );
}

#[test]
fn rel_filter_recalculates_total_size() {
    let m = rel_snapshot_with(
        vec![
            hashed_file("keep.txt", "h1", 100, 1000),
            hashed_file("remove.txt", "h2", 200, 2000),
        ],
        vec![],
    );
    let f = IncludeExcludePathsFilter::new(&["keep.txt"], &[]).unwrap();
    let filtered = filter_manifest(&m, &|e| f.matches(e));
    assert_eq!(filtered.total_size, 100);
}

#[test]
fn rel_filter_excludes_symlinks_from_total_size() {
    let m = rel_snapshot_with(
        vec![
            hashed_file("file.txt", "h1", 100, 1000),
            FileEntry::symlink("link.txt", "file.txt"),
        ],
        vec![],
    );
    let f = IncludeExcludePathsFilter::new(&[], &[]).unwrap();
    let filtered = filter_manifest(&m, &|e| f.matches(e));
    assert_eq!(filtered.total_size, 100);
}

// --- TestFilterManifestAbsDiff ---

#[test]
fn abs_diff_filter_deleted_entries() {
    let m = abs_diff_with(
        vec![
            FileEntry::deleted("/keep.blend"),
            FileEntry::deleted("/remove.txt"),
        ],
        vec![],
    );
    let f = IncludeExcludePathsFilter::new(&["*.blend"], &[]).unwrap();
    let filtered = filter_manifest(&m, &|e| f.matches(e));
    assert_eq!(filtered.files.len(), 1);
    assert_eq!(filtered.files[0].path, "/keep.blend");
    assert!(filtered.files[0].deleted);
}

#[test]
fn abs_diff_filter_preserves_parent_hash() {
    let m = abs_diff_with(vec![hashed_file("/a.txt", "h1", 10, 1000)], vec![])
        .with_parent_hash(Some("parent123".into()));
    let f = IncludeExcludePathsFilter::new(&[], &[]).unwrap();
    let filtered = filter_manifest(&m, &|e| f.matches(e));
    assert_eq!(filtered.parent_manifest_hash.as_deref(), Some("parent123"));
}

#[test]
fn abs_diff_filter_excludes_deleted_from_total_size() {
    let m = abs_diff_with(
        vec![
            hashed_file("/existing.txt", "h1", 100, 1000),
            FileEntry::deleted("/deleted.txt"),
        ],
        vec![],
    );
    let f = IncludeExcludePathsFilter::new(&[], &[]).unwrap();
    let filtered = filter_manifest(&m, &|e| f.matches(e));
    assert_eq!(filtered.total_size, 100);
}

// --- TestFilterManifestRelDiff ---

#[test]
fn rel_diff_filter_deleted_entries() {
    let m = rel_diff_with(
        vec![
            FileEntry::deleted("keep.blend"),
            FileEntry::deleted("remove.txt"),
        ],
        vec![],
    );
    let f = IncludeExcludePathsFilter::new(&["*.blend"], &[]).unwrap();
    let filtered = filter_manifest(&m, &|e| f.matches(e));
    assert_eq!(filtered.files.len(), 1);
    assert_eq!(filtered.files[0].path, "keep.blend");
    assert!(filtered.files[0].deleted);
}

// --- TestFilterManifestDiffScenarios ---

#[test]
fn diff_scenario_filter_both_manifests_same_patterns() {
    let parent = abs_snapshot_with(
        vec![
            hashed_file("/model.blend", "h1", 100, 1000),
            hashed_file("/texture.png", "h2", 200, 2000),
            hashed_file("/notes.txt", "h3", 50, 3000),
        ],
        vec![],
    );
    let current = abs_snapshot_with(
        vec![
            hashed_file("/model.blend", "h1", 100, 1000),
            hashed_file("/texture.png", "h2", 200, 2000),
            hashed_file("/new.blend", "h4", 150, 4000),
        ],
        vec![],
    );
    let f = IncludeExcludePathsFilter::new(&["*.blend"], &[]).unwrap();
    let filtered_parent = filter_manifest(&parent, &|e| f.matches(e));
    let filtered_current = filter_manifest(&current, &|e| f.matches(e));

    let pp = paths(&filtered_parent);
    assert_eq!(pp, ["/model.blend".to_string()].into_iter().collect());

    let cp = paths(&filtered_current);
    assert_eq!(
        cp,
        ["/model.blend".to_string(), "/new.blend".to_string()]
            .into_iter()
            .collect()
    );
}

#[test]
fn diff_scenario_filter_directories_for_diff() {
    let parent = abs_snapshot_with(
        vec![],
        vec![DirEntry::new("/src"), DirEntry::new("/backup")],
    );
    let current = abs_snapshot_with(
        vec![],
        vec![DirEntry::new("/src"), DirEntry::new("/new_dir")],
    );
    let f = IncludeExcludePathsFilter::new(&[], &["/backup*"]).unwrap();
    let filtered_parent = filter_manifest(&parent, &|e| f.matches(e));
    let filtered_current = filter_manifest(&current, &|e| f.matches(e));

    let pdp = dir_paths(&filtered_parent);
    assert_eq!(pdp, ["/src".to_string()].into_iter().collect());

    let cdp = dir_paths(&filtered_current);
    assert_eq!(
        cdp,
        ["/src".to_string(), "/new_dir".to_string()]
            .into_iter()
            .collect()
    );
}

// --- TestCustomFilterCallables ---

#[test]
fn custom_filter_by_size() {
    let m = abs_snapshot_with(
        vec![
            hashed_file("/small.txt", "h1", 100, 1000),
            hashed_file("/medium.txt", "h2", 1000, 2000),
            hashed_file("/large.txt", "h3", 10000, 3000),
        ],
        vec![],
    );
    let filtered = filter_manifest(&m, &|e| match e {
        ManifestEntry::File(f) => f.size.is_none_or(|s| s >= 1000),
        ManifestEntry::Dir(_) => true,
    });
    let p = paths(&filtered);
    assert_eq!(
        p,
        ["/medium.txt".to_string(), "/large.txt".to_string()]
            .into_iter()
            .collect()
    );
}

#[test]
fn custom_filter_by_extension_case_insensitive() {
    let m = abs_snapshot_with(
        vec![
            hashed_file("/model.BLEND", "h1", 100, 1000),
            hashed_file("/scene.blend", "h2", 200, 2000),
            hashed_file("/texture.PNG", "h3", 300, 3000),
        ],
        vec![],
    );
    let filtered = filter_manifest(&m, &|e| e.path().to_lowercase().ends_with(".blend"));
    let p = paths(&filtered);
    assert_eq!(
        p,
        ["/model.BLEND".to_string(), "/scene.blend".to_string()]
            .into_iter()
            .collect()
    );
}

#[test]
fn custom_filter_exclude_runnable() {
    let mut runnable = hashed_file("/script.sh", "h1", 100, 1000);
    runnable.runnable = true;
    let m = abs_snapshot_with(
        vec![runnable, hashed_file("/data.txt", "h2", 200, 2000)],
        vec![],
    );
    let filtered = filter_manifest(&m, &|e| match e {
        ManifestEntry::File(f) => !f.runnable,
        ManifestEntry::Dir(_) => true,
    });
    assert_eq!(filtered.files.len(), 1);
    assert_eq!(filtered.files[0].path, "/data.txt");
}

#[test]
fn custom_filter_always_true() {
    let m = abs_snapshot_with(
        vec![hashed_file("/file.txt", "h1", 100, 1000)],
        vec![DirEntry::new("/dir1")],
    );
    let filtered = filter_manifest(&m, &|_| true);
    assert_eq!(filtered.files.len(), 1);
    assert_eq!(filtered.dirs.len(), 1);
}

#[test]
fn custom_filter_always_false() {
    let m = abs_snapshot_with(
        vec![hashed_file("/file.txt", "h1", 100, 1000)],
        vec![DirEntry::new("/dir1")],
    );
    let filtered = filter_manifest(&m, &|_| false);
    assert_eq!(filtered.files.len(), 0);
    assert_eq!(filtered.dirs.len(), 0);
    assert_eq!(filtered.total_size, 0);
}
