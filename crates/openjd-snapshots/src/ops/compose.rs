// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

use crate::manifest::{Diff, DirEntry, FileEntry, Full, Manifest};
use std::collections::HashMap;

/// A node in the manifest trie structure.
///
/// Each node represents a path component in the directory tree. Nodes can hold:
/// - A file/symlink entry (for leaf nodes representing files)
/// - Child nodes (for subdirectories)
/// - A deleted flag (for diff composition, to track deletion markers)
///
/// Any node without a file_entry and without the deleted flag is implicitly a directory.
/// This enables efficient insertion, deletion of entire subtrees, and traversal to
/// collect all entries for the final manifest.
struct TrieNode {
    children: HashMap<String, TrieNode>,
    file_entry: Option<FileEntry>,
    deleted: bool,
}

impl TrieNode {
    fn new() -> Self {
        Self {
            children: HashMap::new(),
            file_entry: None,
            deleted: false,
        }
    }

    /// Navigate to or create the node at the given path.
    fn insert_path(&mut self, path: &str) -> &mut TrieNode {
        let parts: Vec<&str> = path.split('/').collect();
        let mut node = self;
        for &part in &parts {
            node = node
                .children
                .entry(part.to_string())
                .or_insert_with(TrieNode::new);
        }
        node
    }

    /// Insert a file entry at the given path, clearing any deleted flag.
    fn insert(&mut self, path: &str, entry: FileEntry) {
        let node = self.insert_path(path);
        node.file_entry = Some(entry);
        node.deleted = false;
    }

    /// Delete the node at the given path and all its children.
    fn delete_subtree(&mut self, path: &str) -> bool {
        let parts: Vec<&str> = path.split('/').collect();
        if parts.is_empty() {
            return false;
        }
        let mut node = &mut *self;
        for &part in &parts[..parts.len() - 1] {
            match node.children.get_mut(part) {
                Some(child) => node = child,
                None => return false,
            }
        }
        node.children.remove(parts[parts.len() - 1]).is_some()
    }

    /// Delete the node at the given path only if it has no children and no file_entry.
    fn delete_if_empty(&mut self, path: &str) -> bool {
        let parts: Vec<&str> = path.split('/').collect();
        if parts.is_empty() {
            return false;
        }
        let mut node = &mut *self;
        for &part in &parts[..parts.len() - 1] {
            match node.children.get_mut(part) {
                Some(child) => node = child,
                None => return false,
            }
        }
        let target = parts[parts.len() - 1];
        if let Some(child) = node.children.get(target) {
            if child.children.is_empty() && child.file_entry.is_none() {
                node.children.remove(target);
                return true;
            }
        }
        false
    }

    /// Mark a path as deleted (for diff composition).
    ///
    /// Creates the node if it doesn't exist, clears any file_entry,
    /// and sets the deleted flag. Does NOT clear children — directory
    /// deletion markers only represent empty directory deletion.
    fn mark_deleted(&mut self, path: &str) {
        let node = self.insert_path(path);
        node.file_entry = None;
        node.deleted = true;
    }

    fn join_path(prefix: &str, name: &str) -> String {
        if prefix.is_empty() {
            name.to_string()
        } else if prefix == "/" || prefix.ends_with('/') {
            format!("{prefix}{name}")
        } else {
            format!("{prefix}/{name}")
        }
    }

    fn make_path(prefix: &str, name: &str) -> String {
        if name.is_empty() {
            "/".to_string()
        } else {
            Self::join_path(prefix, name)
        }
    }

    /// Collect all non-deleted file entries (for snapshot output).
    fn collect_files(&self, prefix: &str) -> Vec<FileEntry> {
        let mut result = Vec::new();
        for (name, child) in &self.children {
            let path = Self::make_path(prefix, name);
            if child.file_entry.is_some() && !child.deleted {
                result.push(child.file_entry.clone().unwrap());
            }
            result.extend(child.collect_files(&path));
        }
        result
    }

    /// Collect all file entries including deletion markers (for diff output).
    fn collect_files_with_deletions(&self, prefix: &str) -> Vec<FileEntry> {
        let mut result = Vec::new();
        for (name, child) in &self.children {
            let path = Self::make_path(prefix, name);
            if child.deleted && child.file_entry.is_none() && child.children.is_empty() {
                // Deleted leaf with no file_entry = deleted file marker
                result.push(FileEntry::deleted(&path));
            } else if let Some(ref entry) = child.file_entry {
                if child.deleted {
                    result.push(FileEntry::deleted(&path));
                } else {
                    result.push(entry.clone());
                }
            }
            result.extend(child.collect_files_with_deletions(&path));
        }
        result
    }

    /// Collect all non-deleted directory paths.
    ///
    /// A directory is any node that has children, no file_entry, and is not deleted.
    fn collect_dirs(&self, prefix: &str) -> Vec<DirEntry> {
        let mut result = Vec::new();
        for (name, child) in &self.children {
            let path = Self::make_path(prefix, name);
            if child.file_entry.is_none() && !child.deleted {
                result.push(DirEntry::new(&path));
            }
            result.extend(child.collect_dirs(&path));
        }
        result
    }

    /// Collect all directory entries including deletion markers (for diff output).
    ///
    /// A deleted directory is a node with deleted=true and no file_entry.
    fn collect_dirs_with_deletions(&self, prefix: &str) -> Vec<DirEntry> {
        let mut result = Vec::new();
        for (name, child) in &self.children {
            let path = Self::make_path(prefix, name);
            if child.file_entry.is_none() {
                if child.deleted {
                    result.push(DirEntry::deleted(&path));
                } else {
                    result.push(DirEntry::new(&path));
                }
            }
            result.extend(child.collect_dirs_with_deletions(&path));
        }
        result
    }

    /// Reconcile deleted flags after all diffs have been applied.
    ///
    /// A directory that was marked deleted but now has non-deleted children
    /// must have its deleted flag cleared. This handles the case where:
    /// 1. diff1 deletes /dir/ and all its contents
    /// 2. diff2 adds /dir/newfile.txt
    ///
    /// After diff2, /dir/ should not be marked as deleted because it has
    /// a non-deleted child.
    ///
    /// Returns true if this node has any non-deleted content.
    fn reconcile_deleted_flags(&mut self) -> bool {
        let mut has_non_deleted_children = false;
        for child in self.children.values_mut() {
            if child.reconcile_deleted_flags() {
                has_non_deleted_children = true;
            }
        }

        let has_non_deleted_file = self.file_entry.is_some() && !self.deleted;
        let has_non_deleted_content = has_non_deleted_file || has_non_deleted_children;

        if self.deleted && has_non_deleted_children {
            self.deleted = false;
        }

        has_non_deleted_content
    }
}

/// Composes a base snapshot with one or more diff manifests.
///
/// Applies each diff sequentially using a trie that models the directory tree:
/// file deletions remove subtrees, directory deletions remove only empty directories
/// (deepest first), and additions/modifications update the trie. Directories are
/// tracked implicitly by the trie structure. Returns the final snapshot.
pub fn compose_snapshot_with_diffs<P: Clone>(
    base: &Manifest<P, Full>,
    diffs: &[&Manifest<P, Diff>],
) -> crate::Result<Manifest<P, Full>> {
    for diff in diffs {
        if diff.file_chunk_size_bytes != base.file_chunk_size_bytes {
            return Err(crate::SnapshotError::Validation(format!(
                "file_chunk_size_bytes mismatch: base has {}, diff has {}",
                base.file_chunk_size_bytes, diff.file_chunk_size_bytes
            )));
        }
    }

    let mut trie = TrieNode::new();

    // Load base snapshot into trie
    for f in &base.files {
        trie.insert(&f.path, f.clone());
    }
    for d in &base.dirs {
        trie.insert_path(&d.path);
    }

    // Apply each diff in order
    for diff in diffs {
        // Apply file deletions first
        for f in &diff.files {
            if f.deleted {
                trie.delete_subtree(&f.path);
            }
        }

        // Apply directory deletions (empty only, deepest first)
        let mut deleted_dirs: Vec<&str> = diff
            .dirs
            .iter()
            .filter(|d| d.deleted)
            .map(|d| d.path.as_str())
            .collect();
        deleted_dirs.sort_by_key(|b| std::cmp::Reverse(b.len()));
        for dir_path in deleted_dirs {
            trie.delete_if_empty(dir_path);
        }

        // Apply file additions/modifications
        for f in &diff.files {
            if !f.deleted {
                trie.insert(&f.path, f.clone());
            }
        }

        // Apply directory additions
        for d in &diff.dirs {
            if !d.deleted {
                trie.insert_path(&d.path);
            }
        }
    }

    let files = trie.collect_files("");
    let dirs = trie.collect_dirs("");
    let mut result = Manifest::new(base.hash_alg, base.file_chunk_size_bytes);
    result.files = files;
    result.dirs = dirs;
    result.parent_manifest_hash = None;
    result.recompute_total_size();
    Ok(result)
}

/// Composes multiple diff manifests into a single diff.
///
/// The result is equivalent to applying all input diffs in order. Uses a trie
/// with deleted flags to track the cumulative state. After all diffs are applied,
/// `reconcile_deleted_flags` handles directories that were deleted then
/// re-populated by later diffs. Directories are tracked implicitly by the trie
/// structure rather than a separate data structure.
pub fn compose_diffs<P: Clone>(diffs: &[&Manifest<P, Diff>]) -> crate::Result<Manifest<P, Diff>> {
    if diffs.is_empty() {
        return Err(crate::SnapshotError::Validation(
            "cannot compose empty list of diffs".into(),
        ));
    }

    let expected = diffs[0].file_chunk_size_bytes;
    for diff in &diffs[1..] {
        if diff.file_chunk_size_bytes != expected {
            return Err(crate::SnapshotError::Validation(format!(
                "file_chunk_size_bytes mismatch: first diff has {}, another has {}",
                expected, diff.file_chunk_size_bytes
            )));
        }
    }

    let mut trie = TrieNode::new();

    for diff in diffs {
        // Apply file deletions first
        for f in &diff.files {
            if f.deleted {
                trie.mark_deleted(&f.path);
            }
        }

        // Apply directory deletions
        for d in &diff.dirs {
            if d.deleted {
                trie.mark_deleted(&d.path);
            }
        }

        // Apply file additions/modifications
        for f in &diff.files {
            if !f.deleted {
                let node = trie.insert_path(&f.path);
                node.deleted = false;
                node.file_entry = Some(f.clone());
            }
        }

        // Apply directory additions
        for d in &diff.dirs {
            if !d.deleted {
                let node = trie.insert_path(&d.path);
                node.deleted = false;
            }
        }
    }

    trie.reconcile_deleted_flags();

    let files = trie.collect_files_with_deletions("");
    let dirs = trie.collect_dirs_with_deletions("");
    let mut result = Manifest::new(diffs[0].hash_alg, diffs[0].file_chunk_size_bytes);
    result.files = files;
    result.dirs = dirs;
    result.parent_manifest_hash = diffs[0].parent_manifest_hash.clone();
    result.recompute_total_size();
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hash::HashAlgorithm;
    use crate::manifest::{Diff, DirEntry, Full, Rel};
    use crate::{FileEntry, Manifest, DEFAULT_FILE_CHUNK_SIZE};

    type RelSnapshot = Manifest<Rel, Full>;
    type RelDiff = Manifest<Rel, Diff>;

    fn make_snapshot(files: Vec<FileEntry>) -> RelSnapshot {
        Manifest::new(HashAlgorithm::Xxh128, DEFAULT_FILE_CHUNK_SIZE).with_files(files)
    }

    fn make_diff(files: Vec<FileEntry>) -> RelDiff {
        Manifest::new(HashAlgorithm::Xxh128, DEFAULT_FILE_CHUNK_SIZE).with_files(files)
    }

    #[test]
    fn compose_snapshot_with_diff_additions_appear() {
        let base = make_snapshot(vec![FileEntry::file("a.txt", 10, 1)]);
        let diff = make_diff(vec![FileEntry::file("b.txt", 20, 2)]);
        let result = compose_snapshot_with_diffs(&base, &[&diff]).unwrap();
        assert_eq!(result.files.len(), 2);
        assert!(result.files.iter().any(|f| f.path == "a.txt"));
        assert!(result.files.iter().any(|f| f.path == "b.txt"));
    }

    #[test]
    fn compose_snapshot_with_diff_deletions_removed() {
        let base = make_snapshot(vec![
            FileEntry::file("a.txt", 10, 1),
            FileEntry::file("b.txt", 20, 2),
        ]);
        let diff = make_diff(vec![FileEntry::deleted("b.txt")]);
        let result = compose_snapshot_with_diffs(&base, &[&diff]).unwrap();
        assert_eq!(result.files.len(), 1);
        assert_eq!(result.files[0].path, "a.txt");
    }

    #[test]
    fn compose_snapshot_multiple_diffs_cumulative() {
        let base = make_snapshot(vec![
            FileEntry::file("a.txt", 10, 1),
            FileEntry::file("b.txt", 20, 2),
        ]);
        let diff1 = make_diff(vec![
            FileEntry::file("c.txt", 30, 3),
            FileEntry::deleted("a.txt"),
        ]);
        let diff2 = make_diff(vec![
            FileEntry::file("d.txt", 40, 4),
            FileEntry::deleted("b.txt"),
        ]);
        let result = compose_snapshot_with_diffs(&base, &[&diff1, &diff2]).unwrap();
        assert_eq!(result.files.len(), 2);
        assert!(result.files.iter().any(|f| f.path == "c.txt"));
        assert!(result.files.iter().any(|f| f.path == "d.txt"));
    }

    #[test]
    fn compose_diffs_deletion_markers_preserved() {
        let diff1 = make_diff(vec![FileEntry::file("a.txt", 10, 1)]);
        let diff2 = make_diff(vec![FileEntry::deleted("b.txt")]);
        let result = compose_diffs(&[&diff1, &diff2]).unwrap();
        assert!(result.files.iter().any(|f| f.path == "a.txt" && !f.deleted));
        assert!(result.files.iter().any(|f| f.path == "b.txt" && f.deleted));
    }

    #[test]
    fn compose_diffs_reconcile_deleted_flags() {
        // diff1 deletes dir/
        let diff1 = make_diff(vec![FileEntry::deleted("dir/old.txt")]);
        // diff2 adds dir/new.txt
        let diff2 = make_diff(vec![FileEntry::file("dir/new.txt", 10, 1)]);
        let result = compose_diffs(&[&diff1, &diff2]).unwrap();
        // dir/old.txt should still be deleted, dir/new.txt should be present
        let new = result
            .files
            .iter()
            .find(|f| f.path == "dir/new.txt")
            .unwrap();
        assert!(!new.deleted);
    }

    #[test]
    fn compose_diffs_empty_returns_error() {
        let result = compose_diffs::<Rel>(&[]);
        assert!(result.is_err());
    }

    #[test]
    fn compose_diffs_parent_hash_from_first() {
        let diff1 =
            make_diff(vec![FileEntry::file("a.txt", 10, 1)]).with_parent_hash(Some("hash1".into()));
        let diff2 =
            make_diff(vec![FileEntry::file("b.txt", 20, 2)]).with_parent_hash(Some("hash2".into()));
        let result = compose_diffs(&[&diff1, &diff2]).unwrap();
        assert_eq!(result.parent_manifest_hash.as_deref(), Some("hash1"));
    }

    #[test]
    fn compose_snapshot_total_size_recomputed() {
        let base = make_snapshot(vec![
            FileEntry::file("a.txt", 100, 1),
            FileEntry::file("b.txt", 200, 2),
        ]);
        let diff = make_diff(vec![FileEntry::deleted("b.txt")]);
        let result = compose_snapshot_with_diffs(&base, &[&diff]).unwrap();
        assert_eq!(result.total_size, 100);
    }

    #[test]
    fn compose_snapshot_chunk_size_mismatch_error() {
        let base = make_snapshot(vec![FileEntry::file("a.txt", 10, 1)]);
        let diff: RelDiff = Manifest::new(HashAlgorithm::Xxh128, 1024)
            .with_files(vec![FileEntry::file("b.txt", 20, 2)]);
        let result = compose_snapshot_with_diffs(&base, &[&diff]);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("file_chunk_size_bytes mismatch"));
    }

    #[test]
    fn compose_diffs_chunk_size_mismatch_error() {
        let diff1 = make_diff(vec![FileEntry::file("a.txt", 10, 1)]);
        let diff2: RelDiff = Manifest::new(HashAlgorithm::Xxh128, 1024)
            .with_files(vec![FileEntry::file("b.txt", 20, 2)]);
        let result = compose_diffs(&[&diff1, &diff2]);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("file_chunk_size_bytes mismatch"));
    }

    #[test]
    fn compose_snapshot_dir_delete_only_removes_empty() {
        let base = make_snapshot(vec![
            FileEntry::file("dir/a.txt", 10, 1),
            FileEntry::file("dir/b.txt", 20, 2),
        ]);
        let diff = make_diff(vec![FileEntry::deleted("dir/a.txt")])
            .with_dirs(vec![DirEntry::deleted("dir")]);
        let result = compose_snapshot_with_diffs(&base, &[&diff]).unwrap();
        assert_eq!(result.files.len(), 1);
        assert_eq!(result.files[0].path, "dir/b.txt");
    }
}
