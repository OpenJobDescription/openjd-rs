use crate::manifest::{Diff, FileEntry, Full, Manifest};
use std::collections::HashMap;

struct TrieNode {
    children: HashMap<String, TrieNode>,
    file_entry: Option<FileEntry>,
    dir_deleted: bool,
}

impl TrieNode {
    fn new() -> Self {
        Self {
            children: HashMap::new(),
            file_entry: None,
            dir_deleted: false,
        }
    }

    fn insert(&mut self, path: &str, entry: FileEntry) {
        let parts: Vec<&str> = path.split('/').collect();
        let mut node = self;
        for &part in &parts[..parts.len() - 1] {
            node = node
                .children
                .entry(part.to_string())
                .or_insert_with(TrieNode::new);
        }
        let leaf = node
            .children
            .entry(parts[parts.len() - 1].to_string())
            .or_insert_with(TrieNode::new);
        leaf.file_entry = Some(entry);
        leaf.dir_deleted = false;
    }

    fn delete_file(&mut self, path: &str) {
        let parts: Vec<&str> = path.split('/').collect();
        self.delete_file_recursive(&parts);
    }

    fn delete_file_recursive(&mut self, parts: &[&str]) {
        if parts.is_empty() {
            return;
        }
        if parts.len() == 1 {
            self.children.remove(parts[0]);
            return;
        }
        if let Some(child) = self.children.get_mut(parts[0]) {
            child.delete_file_recursive(&parts[1..]);
        }
    }

    fn delete_if_empty(&mut self, path: &str) {
        let parts: Vec<&str> = path.split('/').collect();
        self.delete_if_empty_recursive(&parts);
    }

    fn delete_if_empty_recursive(&mut self, parts: &[&str]) {
        if parts.is_empty() {
            return;
        }
        if parts.len() == 1 {
            if let Some(node) = self.children.get(parts[0]) {
                if node.children.is_empty() && node.file_entry.is_none() {
                    self.children.remove(parts[0]);
                }
            }
            return;
        }
        if let Some(child) = self.children.get_mut(parts[0]) {
            child.delete_if_empty_recursive(&parts[1..]);
        }
    }

    fn mark_deleted(&mut self, path: &str) {
        let parts: Vec<&str> = path.split('/').collect();
        let mut node = self;
        for &part in &parts[..parts.len() - 1] {
            node = node
                .children
                .entry(part.to_string())
                .or_insert_with(TrieNode::new);
        }
        let leaf = node
            .children
            .entry(parts[parts.len() - 1].to_string())
            .or_insert_with(TrieNode::new);
        leaf.file_entry = None;
        leaf.children.clear();
        leaf.dir_deleted = true;
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

    fn collect_entries(&self, prefix: &str) -> Vec<FileEntry> {
        let mut result = Vec::new();
        for (name, child) in &self.children {
            let path = if name.is_empty() {
                "/".to_string()
            } else {
                Self::join_path(prefix, name)
            };
            if let Some(ref entry) = child.file_entry {
                result.push(entry.clone());
            }
            result.extend(child.collect_entries(&path));
        }
        result
    }

    fn collect_entries_with_deletions(&self, prefix: &str) -> Vec<FileEntry> {
        let mut result = Vec::new();
        for (name, child) in &self.children {
            let path = if name.is_empty() {
                "/".to_string()
            } else {
                Self::join_path(prefix, name)
            };
            if child.dir_deleted && child.file_entry.is_none() {
                result.push(FileEntry::deleted(&path));
            } else if let Some(ref entry) = child.file_entry {
                result.push(entry.clone());
            }
            result.extend(child.collect_entries_with_deletions(&path));
        }
        result
    }

    fn reconcile_deleted_flags(&mut self) -> bool {
        let mut has_live_descendant = false;
        for child in self.children.values_mut() {
            if child.reconcile_deleted_flags() {
                has_live_descendant = true;
            }
        }
        if has_live_descendant {
            self.dir_deleted = false;
        }
        !self.dir_deleted && (self.file_entry.is_some() || has_live_descendant)
    }
}

/// Composes a base snapshot with one or more diff manifests.
///
/// Applies each diff sequentially: file deletions remove entries,
/// directory deletions remove only empty directories (deepest first),
/// and additions/modifications update the trie. Returns the final snapshot.
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

    let mut dir_set: HashMap<String, bool> =
        base.dirs.iter().map(|d| (d.path.clone(), false)).collect();

    for f in &base.files {
        trie.insert(&f.path, f.clone());
    }

    for diff in diffs {
        // Apply file deletions
        for f in &diff.files {
            if f.deleted {
                trie.delete_file(&f.path);
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

        // Apply directory changes
        for d in &diff.dirs {
            if d.deleted {
                dir_set.remove(&d.path);
            } else {
                dir_set.insert(d.path.clone(), false);
            }
        }
    }

    let files = trie.collect_entries("");
    let dirs = dir_set
        .into_keys()
        .map(|path| crate::manifest::DirEntry::new(&path))
        .collect();
    let mut result = Manifest::new(base.hash_alg, base.file_chunk_size_bytes);
    result.files = files;
    result.dirs = dirs;
    result.parent_manifest_hash = None;
    result.recompute_total_size();
    Ok(result)
}

/// Composes multiple diff manifests into a single diff.
///
/// The result is equivalent to applying all input diffs in order.
/// Uses `reconcile_deleted_flags` to handle directories that are
/// deleted then re-populated by later diffs.
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
        for f in &diff.files {
            if f.deleted {
                trie.mark_deleted(&f.path);
            } else {
                trie.insert(&f.path, f.clone());
            }
        }
    }

    trie.reconcile_deleted_flags();

    // Track directory entries across diffs
    let mut dir_state: HashMap<String, bool> = HashMap::new(); // path -> deleted
    for diff in diffs {
        for d in &diff.dirs {
            dir_state.insert(d.path.clone(), d.deleted);
        }
    }

    let files = trie.collect_entries_with_deletions("");
    let dirs = dir_state
        .into_iter()
        .map(|(path, deleted)| {
            if deleted {
                crate::manifest::DirEntry::deleted(&path)
            } else {
                crate::manifest::DirEntry::new(&path)
            }
        })
        .collect();
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
