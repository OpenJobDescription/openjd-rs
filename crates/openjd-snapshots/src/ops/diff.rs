use crate::manifest::{Diff, DirEntry, FileEntry, Full, Manifest};
use std::collections::HashMap;

#[derive(Default)]
pub struct DiffOptions {
    pub parent_manifest_hash: Option<String>,
    pub ignore_hashes: bool,
    pub preserve_runnable: bool,
}

/// Compares two file entries to determine if they differ.
///
/// Checks entry type transitions, content hashes (unless `ignore_hashes`),
/// and metadata (size, mtime, runnable).
pub fn entries_differ(
    parent: &FileEntry,
    current: &FileEntry,
    ignore_hashes: bool,
    preserve_runnable: bool,
) -> bool {
    let parent_is_symlink = parent.symlink_target.is_some();
    let current_is_symlink = current.symlink_target.is_some();

    // Type transition always differs
    if parent_is_symlink != current_is_symlink {
        return true;
    }

    // Symlinks: compare target only
    if current_is_symlink {
        return parent.symlink_target != current.symlink_target;
    }

    // Regular files
    if parent.size != current.size || parent.mtime != current.mtime {
        return true;
    }
    if !ignore_hashes
        && (parent.hash != current.hash || parent.chunk_hashes != current.chunk_hashes)
    {
        return true;
    }
    if !preserve_runnable && parent.runnable != current.runnable {
        return true;
    }
    false
}

/// Returns `None` if no regular files, `Some(true)` if any have hashes, `Some(false)` if none do.
fn has_hashed_files<P, K>(manifest: &Manifest<P, K>) -> Option<bool> {
    let regular_files: Vec<_> = manifest
        .files
        .iter()
        .filter(|f| f.symlink_target.is_none() && !f.deleted)
        .collect();
    if regular_files.is_empty() {
        return None;
    }
    Some(
        regular_files
            .iter()
            .any(|f| f.hash.is_some() || f.chunk_hashes.is_some()),
    )
}

/// Computes the difference between two snapshot manifests.
///
/// Returns a diff manifest containing new/modified entries and deletion
/// markers. Both manifests must have the same path style. When a directory
/// is deleted, all its contents receive explicit deletion markers.
pub fn diff_snapshots<P: Clone>(
    parent: &Manifest<P, Full>,
    current: &Manifest<P, Full>,
    options: &DiffOptions,
) -> crate::Result<Manifest<P, Diff>> {
    if !options.ignore_hashes {
        let parent_hashed = has_hashed_files(parent);
        let current_hashed = has_hashed_files(current);
        if let (Some(ph), Some(ch)) = (parent_hashed, current_hashed) {
            if ph && !ch {
                return Err(crate::SnapshotError::Validation(
                    "cannot diff hashed parent manifest against unhashed current manifest when ignore_hashes=false".into(),
                ));
            }
            if !ph && ch {
                return Err(crate::SnapshotError::Validation(
                    "cannot diff unhashed parent manifest against hashed current manifest when ignore_hashes=false".into(),
                ));
            }
        }
    }

    let parent_files: HashMap<&str, &FileEntry> =
        parent.files.iter().map(|f| (f.path.as_str(), f)).collect();
    let parent_dirs: HashMap<&str, &DirEntry> =
        parent.dirs.iter().map(|d| (d.path.as_str(), d)).collect();
    let current_files: HashMap<&str, &FileEntry> =
        current.files.iter().map(|f| (f.path.as_str(), f)).collect();
    let current_dirs: HashMap<&str, &DirEntry> =
        current.dirs.iter().map(|d| (d.path.as_str(), d)).collect();

    let mut files = Vec::new();

    // New and modified files
    for cf in &current.files {
        match parent_files.get(cf.path.as_str()) {
            None => files.push(cf.clone()),
            Some(pf) => {
                if entries_differ(pf, cf, options.ignore_hashes, options.preserve_runnable) {
                    let mut entry = cf.clone();
                    if options.preserve_runnable && cf.symlink_target.is_none() {
                        entry.runnable = pf.runnable;
                    }
                    files.push(entry);
                }
            }
        }
    }

    // Deleted files
    let mut deleted_file_paths: std::collections::HashSet<&str> = std::collections::HashSet::new();
    for pf in &parent.files {
        if !current_files.contains_key(pf.path.as_str()) {
            deleted_file_paths.insert(&pf.path);
        }
    }

    let mut dirs = Vec::new();

    // New dirs
    for cd in &current.dirs {
        if !parent_dirs.contains_key(cd.path.as_str()) {
            dirs.push(cd.clone());
        }
    }

    // Deleted dirs
    let mut deleted_dir_paths: std::collections::HashSet<&str> = std::collections::HashSet::new();
    for pd in &parent.dirs {
        if !current_dirs.contains_key(pd.path.as_str()) {
            deleted_dir_paths.insert(&pd.path);
        }
    }

    // Ensure deleted directories have all their contents deleted too.
    for deleted_dir in deleted_dir_paths.iter().copied().collect::<Vec<_>>() {
        let dir_prefix = format!("{}/", deleted_dir);
        for pf in &parent.files {
            if pf.path.starts_with(&dir_prefix) && !current_files.contains_key(pf.path.as_str()) {
                deleted_file_paths.insert(&pf.path);
            }
        }
        for pd in &parent.dirs {
            if pd.path.starts_with(&dir_prefix) && !current_dirs.contains_key(pd.path.as_str()) {
                deleted_dir_paths.insert(&pd.path);
            }
        }
    }

    // Add file deletion markers
    for path in &deleted_file_paths {
        files.push(FileEntry::deleted(*path));
    }

    // Add dir deletion markers
    let sorted_deleted_dirs: Vec<&str> = deleted_dir_paths.into_iter().collect();
    for path in sorted_deleted_dirs {
        dirs.push(DirEntry::deleted(path));
    }

    // Sort: non-deleted files first (by path), then deleted files (by path)
    files.sort_by(|a, b| match (a.deleted, b.deleted) {
        (false, true) => std::cmp::Ordering::Less,
        (true, false) => std::cmp::Ordering::Greater,
        _ => a.path.cmp(&b.path),
    });

    // Sort: non-deleted dirs first (by path), then deleted dirs (deepest-first, then by path)
    dirs.sort_by(|a, b| match (a.deleted, b.deleted) {
        (false, true) => std::cmp::Ordering::Less,
        (true, false) => std::cmp::Ordering::Greater,
        (false, false) => a.path.cmp(&b.path),
        (true, true) => b.path.len().cmp(&a.path.len()).then(a.path.cmp(&b.path)),
    });

    let mut result = Manifest::new(parent.hash_alg, parent.file_chunk_size_bytes);
    result.files = files;
    result.dirs = dirs;
    result.parent_manifest_hash = options.parent_manifest_hash.clone();
    result.recompute_total_size();
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hash::HashAlgorithm;
    use crate::manifest::{Full, Rel};
    use crate::{DirEntry, FileEntry, Manifest, DEFAULT_FILE_CHUNK_SIZE};

    type RelSnapshot = Manifest<Rel, Full>;

    fn make(files: Vec<FileEntry>, dirs: Vec<DirEntry>) -> RelSnapshot {
        Manifest::new(HashAlgorithm::Xxh128, DEFAULT_FILE_CHUNK_SIZE)
            .with_files(files)
            .with_dirs(dirs)
    }

    fn default_opts() -> DiffOptions {
        DiffOptions::default()
    }

    #[test]
    fn no_changes_empty_diff() {
        let m = make(vec![FileEntry::file("a.txt", 100, 1000)], vec![]);
        let diff = diff_snapshots(&m, &m, &default_opts()).unwrap();
        assert!(diff.files.is_empty());
        assert!(diff.dirs.is_empty());
    }

    #[test]
    fn new_file_detected() {
        let parent = make(vec![], vec![]);
        let current = make(vec![FileEntry::file("new.txt", 50, 1)], vec![]);
        let diff = diff_snapshots(&parent, &current, &default_opts()).unwrap();
        assert_eq!(diff.files.len(), 1);
        assert_eq!(diff.files[0].path, "new.txt");
        assert!(!diff.files[0].deleted);
    }

    #[test]
    fn modified_file_by_mtime() {
        let parent = make(vec![FileEntry::file("a.txt", 100, 1000)], vec![]);
        let current = make(vec![FileEntry::file("a.txt", 100, 2000)], vec![]);
        let diff = diff_snapshots(&parent, &current, &default_opts()).unwrap();
        assert_eq!(diff.files.len(), 1);
        assert_eq!(diff.files[0].mtime, Some(2000));
    }

    #[test]
    fn modified_file_by_hash() {
        let mut pf = FileEntry::file("a.txt", 100, 1000);
        pf.hash = Some("aaa".into());
        let mut cf = FileEntry::file("a.txt", 100, 1000);
        cf.hash = Some("bbb".into());
        let parent = make(vec![pf], vec![]);
        let current = make(vec![cf], vec![]);
        let diff = diff_snapshots(&parent, &current, &default_opts()).unwrap();
        assert_eq!(diff.files.len(), 1);
        assert_eq!(diff.files[0].hash.as_deref(), Some("bbb"));
    }

    #[test]
    fn deleted_file_marker() {
        let parent = make(vec![FileEntry::file("gone.txt", 100, 1)], vec![]);
        let current = make(vec![], vec![]);
        let diff = diff_snapshots(&parent, &current, &default_opts()).unwrap();
        assert_eq!(diff.files.len(), 1);
        assert!(diff.files[0].deleted);
        assert_eq!(diff.files[0].path, "gone.txt");
    }

    #[test]
    fn ignore_hashes_mode() {
        let mut pf = FileEntry::file("a.txt", 100, 1000);
        pf.hash = Some("aaa".into());
        let mut cf = FileEntry::file("a.txt", 100, 1000);
        cf.hash = Some("bbb".into());
        let parent = make(vec![pf], vec![]);
        let current = make(vec![cf], vec![]);
        let opts = DiffOptions {
            ignore_hashes: true,
            ..default_opts()
        };
        let diff = diff_snapshots(&parent, &current, &opts).unwrap();
        assert!(diff.files.is_empty(), "hash-only change should be ignored");
    }

    #[test]
    fn preserve_runnable_copies_from_parent() {
        let mut pf = FileEntry::file("script.sh", 100, 1000);
        pf.runnable = true;
        // Current has different mtime (modified) but runnable=false (Windows)
        let cf = FileEntry::file("script.sh", 100, 2000);
        let parent = make(vec![pf], vec![]);
        let current = make(vec![cf], vec![]);
        let opts = DiffOptions {
            preserve_runnable: true,
            ..default_opts()
        };
        let diff = diff_snapshots(&parent, &current, &opts).unwrap();
        assert_eq!(diff.files.len(), 1);
        assert!(
            diff.files[0].runnable,
            "runnable should be copied from parent"
        );
    }

    #[test]
    fn symlink_change_detected() {
        let parent = make(vec![FileEntry::symlink("link", "target_a")], vec![]);
        let current = make(vec![FileEntry::symlink("link", "target_b")], vec![]);
        let diff = diff_snapshots(&parent, &current, &default_opts()).unwrap();
        assert_eq!(diff.files.len(), 1);
        assert_eq!(diff.files[0].symlink_target.as_deref(), Some("target_b"));
    }

    #[test]
    fn dir_additions_and_deletions() {
        let parent = make(vec![], vec![DirEntry::new("old_dir")]);
        let current = make(vec![], vec![DirEntry::new("new_dir")]);
        let diff = diff_snapshots(&parent, &current, &default_opts()).unwrap();
        assert_eq!(diff.dirs.len(), 2);
        let new = diff.dirs.iter().find(|d| d.path == "new_dir").unwrap();
        assert!(!new.deleted);
        let old = diff.dirs.iter().find(|d| d.path == "old_dir").unwrap();
        assert!(old.deleted);
    }

    #[test]
    fn hash_state_mismatch_hashed_parent_unhashed_current() {
        let mut pf = FileEntry::file("a.txt", 100, 1000);
        pf.hash = Some("aaa".into());
        let parent = make(vec![pf], vec![]);
        let current = make(vec![FileEntry::file("a.txt", 100, 1000)], vec![]);
        let result = diff_snapshots(&parent, &current, &default_opts());
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("hashed parent"));
    }

    #[test]
    fn hash_state_mismatch_unhashed_parent_hashed_current() {
        let parent = make(vec![FileEntry::file("a.txt", 100, 1000)], vec![]);
        let mut cf = FileEntry::file("a.txt", 100, 1000);
        cf.hash = Some("bbb".into());
        let current = make(vec![cf], vec![]);
        let result = diff_snapshots(&parent, &current, &default_opts());
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("unhashed parent"));
    }

    #[test]
    fn hash_state_mismatch_allowed_with_ignore_hashes() {
        let mut pf = FileEntry::file("a.txt", 100, 1000);
        pf.hash = Some("aaa".into());
        let parent = make(vec![pf], vec![]);
        let current = make(vec![FileEntry::file("a.txt", 100, 1000)], vec![]);
        let opts = DiffOptions {
            ignore_hashes: true,
            ..default_opts()
        };
        assert!(diff_snapshots(&parent, &current, &opts).is_ok());
    }

    #[test]
    fn hash_state_empty_manifests_compatible() {
        let parent = make(vec![], vec![]);
        let mut cf = FileEntry::file("a.txt", 100, 1000);
        cf.hash = Some("aaa".into());
        let current = make(vec![cf], vec![]);
        // Empty parent has no regular files -> None, so no mismatch
        assert!(diff_snapshots(&parent, &current, &default_opts()).is_ok());
    }

    #[test]
    fn hash_state_symlink_only_compatible() {
        let parent = make(vec![FileEntry::symlink("link", "target")], vec![]);
        let mut cf = FileEntry::file("a.txt", 100, 1000);
        cf.hash = Some("aaa".into());
        let current = make(vec![cf], vec![]);
        // Parent has only symlinks -> None for has_hashed_files
        assert!(diff_snapshots(&parent, &current, &default_opts()).is_ok());
    }

    #[test]
    fn deleted_dir_cascades_to_contents() {
        let parent = make(
            vec![
                FileEntry::file("dir/a.txt", 10, 1),
                FileEntry::file("dir/sub/b.txt", 20, 2),
            ],
            vec![DirEntry::new("dir"), DirEntry::new("dir/sub")],
        );
        let current = make(vec![], vec![]);
        let diff = diff_snapshots(&parent, &current, &default_opts()).unwrap();
        // All files under deleted dirs should have deletion markers
        let deleted_files: Vec<&str> = diff
            .files
            .iter()
            .filter(|f| f.deleted)
            .map(|f| f.path.as_str())
            .collect();
        assert!(deleted_files.contains(&"dir/a.txt"));
        assert!(deleted_files.contains(&"dir/sub/b.txt"));
        // Deleted dirs should also be present
        let deleted_dirs: Vec<&str> = diff
            .dirs
            .iter()
            .filter(|d| d.deleted)
            .map(|d| d.path.as_str())
            .collect();
        assert!(deleted_dirs.contains(&"dir"));
        assert!(deleted_dirs.contains(&"dir/sub"));
    }

    #[test]
    fn parent_manifest_hash_set() {
        let m = make(vec![], vec![]);
        let opts = DiffOptions {
            parent_manifest_hash: Some("hash123".into()),
            ..default_opts()
        };
        let diff = diff_snapshots(&m, &m, &opts).unwrap();
        assert_eq!(diff.parent_manifest_hash.as_deref(), Some("hash123"));
    }
}
