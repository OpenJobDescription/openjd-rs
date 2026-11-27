// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

use crate::manifest::{
    AbsSnapshot, AbsSnapshotDiff, DirEntry, FileEntry, Manifest, Snapshot, SnapshotDiff,
    SymlinkPolicy,
};
use crate::path_util::{normalize_path, strip_root};
use std::cell::OnceCell;
use std::collections::{HashMap, HashSet};
use tracing::warn;

/// The manifest's files, looked up by path and listed in path order so the
/// files under a directory are one contiguous range.
struct FileIndex<'a> {
    by_path: HashMap<&'a str, &'a FileEntry>,
    sorted: Vec<&'a FileEntry>,
}

impl<'a> FileIndex<'a> {
    fn new(files: &'a [FileEntry]) -> Self {
        let mut sorted: Vec<&FileEntry> = files.iter().collect();
        sorted.sort_by(|a, b| a.path.cmp(&b.path));
        let by_path = files.iter().map(|f| (f.path.as_str(), f)).collect();
        Self { by_path, sorted }
    }

    /// The files strictly under directory `dir`, in path order. Matches
    /// [`strip_root`]: a `dir` ending in `/` (`/`, `C:/`) is a prefix as-is.
    fn under(&self, dir: &str) -> &[&'a FileEntry] {
        let owned;
        let prefix = if dir.ends_with('/') {
            dir
        } else {
            owned = format!("{dir}/");
            owned.as_str()
        };
        let lo = self.sorted.partition_point(|f| f.path.as_str() < prefix);
        let len = self.sorted[lo..].partition_point(|f| f.path.starts_with(prefix));
        &self.sorted[lo..lo + len]
    }
}

/// What a symlink target finally names, after following symlink chains.
enum Resolved<'a> {
    /// A regular file entry.
    File(&'a FileEntry),
    /// A directory: the path has no entry but files lie under it.
    Dir(&'a str),
}

/// Resolve a symlink target, following chains (up to 64 hops) and detecting
/// cycles. `None` means the target is missing or the chain is a cycle.
fn resolve_target<'a>(target: &'a str, index: &FileIndex<'a>) -> Option<Resolved<'a>> {
    let mut current = target;
    let mut visited = HashSet::new();
    for _ in 0..64 {
        if !visited.insert(current) {
            warn!(target, "symlink cycle detected, skipping");
            return None;
        }
        match index.by_path.get(current) {
            Some(entry) => match entry.symlink_target.as_deref() {
                None => return Some(Resolved::File(entry)),
                Some(next) => current = next,
            },
            None if !index.under(current).is_empty() => return Some(Resolved::Dir(current)),
            None => return None,
        }
    }
    None
}

/// State shared by the collapse of one top-level symlink.
struct Collapse<'a, 'c> {
    index: &'c FileIndex<'a>,
    symlink_policy: SymlinkPolicy,
    prefix: &'c str,
    excluded: &'c [String],
    /// The subtree root and the directories being expanded. A link to one of
    /// them, or to a directory containing one, is a cycle.
    expanding: Vec<&'c str>,
    /// Nested directory symlinks already expanded. Each is expanded at most
    /// once per top-level symlink, which bounds the output when directory
    /// links converge.
    visited: HashSet<&'a str>,
}

impl<'a: 'c, 'c> Collapse<'a, 'c> {
    /// Collapse a symlink to `target` into output path `out_path`: a file
    /// target becomes a copy of the file entry, a directory target is
    /// expanded under `out_path`, and a missing target or a cycle gives
    /// nothing. `nested` is the manifest path of a nested symlink, whose
    /// directory target is expanded only the first time it is reached.
    fn collapse(
        &mut self,
        out_path: &str,
        nested: Option<&'a str>,
        target: &'a str,
        result: &mut Vec<FileEntry>,
    ) {
        match resolve_target(target, self.index) {
            Some(Resolved::File(real)) => {
                let mut entry = real.clone();
                entry.path = out_path.to_string();
                result.push(entry);
            }
            Some(Resolved::Dir(dir)) => {
                if self.expanding.iter().any(|e| strip_root(e, dir).is_some()) {
                    warn!(target, "symlink cycle detected, skipping");
                    return;
                }
                if nested.is_some_and(|link| !self.visited.insert(link)) {
                    return;
                }
                self.expand_dir(out_path, dir, result);
            }
            None => {}
        }
    }

    /// Expand directory `dir` under output path `out_path`.
    ///
    /// Nested symlinks inside the expanded directory are handled according to
    /// `symlink_policy`:
    /// - `CollapseAll`: nested symlinks are collapsed.
    /// - `CollapseEscaping`: a nested symlink whose target is strictly inside
    ///   the subtree at `prefix` (and not under an `excluded` root) is kept,
    ///   with its target made relative to the subtree. Any other nested
    ///   symlink is collapsed, so the result never holds a link to a target
    ///   outside the subtree.
    fn expand_dir(&mut self, out_path: &str, dir: &'a str, result: &mut Vec<FileEntry>) {
        self.expanding.push(dir);
        let index = self.index;
        for f in index.under(dir) {
            let suffix = strip_root(&f.path, dir).unwrap_or(&f.path);
            let new_path = format!("{out_path}/{suffix}");
            let Some(sym_target) = f.symlink_target.as_deref() else {
                let mut entry = (*f).clone();
                entry.path = new_path;
                result.push(entry);
                continue;
            };
            if self.symlink_policy != SymlinkPolicy::CollapseAll {
                if let Some(rel_target) = subtree_rel_path(sym_target, self.prefix, self.excluded)
                    .filter(|r| !r.is_empty())
                {
                    let mut entry = (*f).clone();
                    entry.path = new_path;
                    entry.symlink_target = Some(rel_target.to_string());
                    result.push(entry);
                    continue;
                }
            }
            self.collapse(&new_path, Some(f.path.as_str()), sym_target, result);
        }
        self.expanding.pop();
    }
}

/// The path of `path` relative to the subtree at `prefix`, or `None` if it is
/// outside the subtree or under one of the `excluded` roots, which other
/// subtrees hold.
fn subtree_rel_path<'a>(path: &'a str, prefix: &str, excluded: &[String]) -> Option<&'a str> {
    if excluded.iter().any(|e| strip_root(path, e).is_some()) {
        return None;
    }
    strip_root(path, prefix)
}

fn process_files(
    files: &[FileEntry],
    prefix: &str,
    excluded: &[String],
    symlink_policy: SymlinkPolicy,
) -> crate::Result<Vec<FileEntry>> {
    // Built on first use: a subtree without symlinks to collapse needs none.
    let index = OnceCell::new();
    let mut result = Vec::new();
    for f in files {
        let rel_path = match subtree_rel_path(&f.path, prefix, excluded) {
            Some(p) if !p.is_empty() => p,
            _ => continue,
        };
        let Some(target) = f.symlink_target.as_deref() else {
            let mut entry = f.clone();
            entry.path = rel_path.to_string();
            result.push(entry);
            continue;
        };
        match symlink_policy {
            SymlinkPolicy::ExcludeAll => continue,
            SymlinkPolicy::Preserve | SymlinkPolicy::TransitiveIncludeTargets => {
                return Err(crate::SnapshotError::Validation(format!(
                    "symlink_policy {} is not supported for subtree",
                    symlink_policy
                )));
            }
            SymlinkPolicy::CollapseEscaping | SymlinkPolicy::ExcludeEscaping => {
                // A target under an excluded root is outside this subtree,
                // so the link escapes it.
                if let Some(rel_target) = subtree_rel_path(target, prefix, excluded) {
                    let mut entry = f.clone();
                    entry.path = rel_path.to_string();
                    entry.symlink_target = Some(rel_target.to_string());
                    result.push(entry);
                    continue;
                }
                if symlink_policy == SymlinkPolicy::ExcludeEscaping {
                    continue;
                }
            }
            SymlinkPolicy::CollapseAll => {}
        }
        let mut collapse = Collapse {
            index: index.get_or_init(|| FileIndex::new(files)),
            symlink_policy,
            prefix,
            excluded,
            expanding: if prefix.is_empty() || prefix == "." {
                Vec::new()
            } else {
                vec![prefix]
            },
            visited: HashSet::new(),
        };
        collapse.collapse(rel_path, None, target, &mut result);
    }
    Ok(result)
}

fn process_dirs(dirs: &[DirEntry], prefix: &str, excluded: &[String]) -> Vec<DirEntry> {
    dirs.iter()
        .filter_map(|d| {
            subtree_rel_path(&d.path, prefix, excluded).and_then(|rel| {
                if rel.is_empty() {
                    None
                } else {
                    Some(if d.deleted {
                        DirEntry::deleted(rel)
                    } else {
                        DirEntry::new(rel)
                    })
                }
            })
        })
        .collect()
}

fn subtree_impl<P: Clone, K: Clone>(
    manifest: &Manifest<P, K>,
    subtree: &str,
    symlink_policy: SymlinkPolicy,
) -> crate::Result<Manifest<crate::manifest::Rel, K>> {
    subtree_excluding(manifest, subtree, &[], symlink_policy)
}

/// Extracts the subtree at `subtree`, leaving out the entries under the
/// normalized `excluded` roots. A symlink whose target is under an excluded
/// root escapes the subtree, and `symlink_policy` applies to it as to any
/// escaping link. Partitioning uses this so that a derived root containing a
/// given root holds neither that root's entries nor links into them.
pub(crate) fn subtree_excluding<P: Clone, K: Clone>(
    manifest: &Manifest<P, K>,
    subtree: &str,
    excluded: &[String],
    symlink_policy: SymlinkPolicy,
) -> crate::Result<Manifest<crate::manifest::Rel, K>> {
    if symlink_policy == SymlinkPolicy::Preserve
        || symlink_policy == SymlinkPolicy::TransitiveIncludeTargets
    {
        return Err(crate::SnapshotError::Validation(format!(
            "symlink_policy {} is not supported for subtree",
            symlink_policy
        )));
    }

    let prefix = normalize_path(subtree);

    // Validate path style consistency (skip for identity subtree)
    if prefix != "." && !prefix.is_empty() {
        let subtree_is_abs = crate::path_util::is_absolute_path(&prefix);
        let manifest_is_abs = manifest
            .files
            .first()
            .map(|f| crate::path_util::is_absolute_path(&f.path))
            .or_else(|| {
                manifest
                    .dirs
                    .first()
                    .map(|d| crate::path_util::is_absolute_path(&d.path))
            });
        if let Some(m_abs) = manifest_is_abs {
            if subtree_is_abs && !m_abs {
                return Err(crate::SnapshotError::Validation(format!(
                    "subtree path is absolute ('{}') but manifest uses relative paths",
                    prefix
                )));
            }
            if !subtree_is_abs && m_abs {
                return Err(crate::SnapshotError::Validation(format!(
                    "subtree path is relative ('{}') but manifest uses absolute paths",
                    prefix
                )));
            }
        }
    }

    let files = process_files(&manifest.files, &prefix, excluded, symlink_policy)?;
    let dirs = process_dirs(&manifest.dirs, &prefix, excluded);

    let mut result = Manifest::new(manifest.hash_alg, manifest.file_chunk_size_bytes);
    result.files = files;
    result.dirs = dirs;
    result.parent_manifest_hash = None;
    result.recompute_total_size();
    Ok(result)
}

/// Extracts a subtree from an absolute snapshot, producing a relative-path snapshot.
pub fn subtree_snapshot(
    manifest: &AbsSnapshot,
    subtree: &str,
    symlink_policy: SymlinkPolicy,
) -> crate::Result<Snapshot> {
    subtree_impl(manifest, subtree, symlink_policy)
}

/// Extracts a subtree from an absolute snapshot diff, producing a relative-path diff.
pub fn subtree_snapshot_diff(
    manifest: &AbsSnapshotDiff,
    subtree: &str,
    symlink_policy: SymlinkPolicy,
) -> crate::Result<SnapshotDiff> {
    subtree_impl(manifest, subtree, symlink_policy)
}

/// Extracts a subtree from a relative snapshot, producing a relative-path snapshot.
pub fn subtree_rel_snapshot(
    manifest: &Snapshot,
    subtree: &str,
    symlink_policy: SymlinkPolicy,
) -> crate::Result<Snapshot> {
    subtree_impl(manifest, subtree, symlink_policy)
}

/// Extracts a subtree from a relative snapshot diff, producing a relative-path diff.
pub fn subtree_rel_snapshot_diff(
    manifest: &SnapshotDiff,
    subtree: &str,
    symlink_policy: SymlinkPolicy,
) -> crate::Result<SnapshotDiff> {
    subtree_impl(manifest, subtree, symlink_policy)
}

use crate::manifest::{AbsManifest, RelManifest};

/// Extracts a subtree from an absolute manifest, producing a RelManifest.
pub fn subtree_manifest(
    manifest: &AbsManifest,
    subtree: &str,
    symlink_policy: SymlinkPolicy,
) -> crate::Result<RelManifest> {
    match manifest {
        AbsManifest::Snapshot(s) => Ok(RelManifest::Snapshot(subtree_snapshot(
            s,
            subtree,
            symlink_policy,
        )?)),
        AbsManifest::Diff(d) => Ok(RelManifest::Diff(subtree_snapshot_diff(
            d,
            subtree,
            symlink_policy,
        )?)),
    }
}

/// Extracts a subtree from a relative manifest, producing a RelManifest.
pub fn subtree_rel_manifest(
    manifest: &RelManifest,
    subtree: &str,
    symlink_policy: SymlinkPolicy,
) -> crate::Result<RelManifest> {
    match manifest {
        RelManifest::Snapshot(s) => Ok(RelManifest::Snapshot(subtree_rel_snapshot(
            s,
            subtree,
            symlink_policy,
        )?)),
        RelManifest::Diff(d) => Ok(RelManifest::Diff(subtree_rel_snapshot_diff(
            d,
            subtree,
            symlink_policy,
        )?)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hash::HashAlgorithm;
    use crate::{AbsSnapshot, DirEntry, FileEntry, Manifest, Snapshot, DEFAULT_FILE_CHUNK_SIZE};

    fn make_abs(files: Vec<FileEntry>, dirs: Vec<DirEntry>) -> AbsSnapshot {
        Manifest::new(HashAlgorithm::Xxh128, DEFAULT_FILE_CHUNK_SIZE)
            .with_files(files)
            .with_dirs(dirs)
    }

    fn make_rel(files: Vec<FileEntry>, dirs: Vec<DirEntry>) -> Snapshot {
        Manifest::new(HashAlgorithm::Xxh128, DEFAULT_FILE_CHUNK_SIZE)
            .with_files(files)
            .with_dirs(dirs)
    }

    #[test]
    fn extract_subtree_produces_relative_paths() {
        let m = make_abs(
            vec![
                FileEntry::file("/root/sub/a.txt", 10, 1),
                FileEntry::file("/root/sub/b.txt", 20, 2),
            ],
            vec![],
        );
        let result = subtree_snapshot(&m, "/root/sub", SymlinkPolicy::CollapseEscaping).unwrap();
        assert_eq!(result.files.len(), 2);
        assert_eq!(result.files[0].path, "a.txt");
        assert_eq!(result.files[1].path, "b.txt");
    }

    #[test]
    fn files_outside_subtree_excluded() {
        let m = make_abs(
            vec![
                FileEntry::file("/root/sub/a.txt", 10, 1),
                FileEntry::file("/root/other/b.txt", 20, 2),
            ],
            vec![],
        );
        let result = subtree_snapshot(&m, "/root/sub", SymlinkPolicy::CollapseEscaping).unwrap();
        assert_eq!(result.files.len(), 1);
        assert_eq!(result.files[0].path, "a.txt");
    }

    #[test]
    fn symlink_within_subtree_preserved_with_rebased_target() {
        let m = make_abs(
            vec![
                FileEntry::file("/root/sub/real.txt", 10, 1),
                FileEntry::symlink("/root/sub/link", "/root/sub/real.txt"),
            ],
            vec![],
        );
        let result = subtree_snapshot(&m, "/root/sub", SymlinkPolicy::CollapseEscaping).unwrap();
        assert_eq!(result.files.len(), 2);
        let link = result.files.iter().find(|f| f.path == "link").unwrap();
        assert_eq!(link.symlink_target.as_deref(), Some("real.txt"));
    }

    #[test]
    fn escaping_symlink_collapsed() {
        let m = make_abs(
            vec![
                FileEntry::file("/root/other/real.txt", 42, 100),
                FileEntry::symlink("/root/sub/link", "/root/other/real.txt"),
            ],
            vec![],
        );
        let result = subtree_snapshot(&m, "/root/sub", SymlinkPolicy::CollapseEscaping).unwrap();
        assert_eq!(result.files.len(), 1);
        let f = &result.files[0];
        assert_eq!(f.path, "link");
        assert!(f.symlink_target.is_none());
        assert_eq!(f.size, Some(42));
    }

    #[test]
    fn escaping_symlink_excluded() {
        let m = make_abs(
            vec![
                FileEntry::file("/root/other/real.txt", 42, 100),
                FileEntry::symlink("/root/sub/link", "/root/other/real.txt"),
            ],
            vec![],
        );
        let result = subtree_snapshot(&m, "/root/sub", SymlinkPolicy::ExcludeEscaping).unwrap();
        assert!(result.files.is_empty());
    }

    #[test]
    fn exclude_all_removes_all_symlinks() {
        let m = make_abs(
            vec![
                FileEntry::file("/root/sub/a.txt", 10, 1),
                FileEntry::symlink("/root/sub/link1", "/root/sub/a.txt"),
                FileEntry::symlink("/root/sub/link2", "/root/other/b.txt"),
            ],
            vec![],
        );
        let result = subtree_snapshot(&m, "/root/sub", SymlinkPolicy::ExcludeAll).unwrap();
        assert_eq!(result.files.len(), 1);
        assert_eq!(result.files[0].path, "a.txt");
    }

    #[test]
    fn parent_manifest_hash_cleared() {
        let m = make_abs(vec![FileEntry::file("/root/sub/a.txt", 10, 1)], vec![])
            .with_parent_hash(Some("oldhash".into()));
        let result = subtree_snapshot(&m, "/root/sub", SymlinkPolicy::CollapseEscaping).unwrap();
        assert!(result.parent_manifest_hash.is_none());
    }

    #[test]
    fn identity_subtree_dot() {
        let m = make_rel(
            vec![
                FileEntry::file("a.txt", 10, 1),
                FileEntry::file("sub/b.txt", 20, 2),
            ],
            vec![],
        );
        let result = subtree_rel_snapshot(&m, ".", SymlinkPolicy::CollapseEscaping).unwrap();
        assert_eq!(result.files.len(), 2);
        assert_eq!(result.files[0].path, "a.txt");
        assert_eq!(result.files[1].path, "sub/b.txt");
    }

    #[test]
    fn identity_subtree_empty() {
        let m = make_rel(vec![FileEntry::file("a.txt", 10, 1)], vec![]);
        let result = subtree_rel_snapshot(&m, "", SymlinkPolicy::CollapseEscaping).unwrap();
        assert_eq!(result.files.len(), 1);
        assert_eq!(result.files[0].path, "a.txt");
    }

    #[test]
    fn dirs_filtered_and_rebased() {
        let m = make_abs(
            vec![],
            vec![
                DirEntry::new("/root/sub/dir1"),
                DirEntry::new("/root/other/dir2"),
            ],
        );
        let result = subtree_snapshot(&m, "/root/sub", SymlinkPolicy::CollapseEscaping).unwrap();
        assert_eq!(result.dirs.len(), 1);
        assert_eq!(result.dirs[0].path, "dir1");
    }

    #[test]
    fn preserve_policy_returns_error() {
        let m = make_abs(vec![], vec![]);
        let result = subtree_snapshot(&m, "/root", SymlinkPolicy::Preserve);
        assert!(result.is_err());
    }

    #[test]
    fn total_size_recomputed() {
        let m = make_abs(
            vec![
                FileEntry::file("/root/sub/a.txt", 100, 1),
                FileEntry::file("/root/other/b.txt", 200, 2),
            ],
            vec![],
        );
        let result = subtree_snapshot(&m, "/root/sub", SymlinkPolicy::CollapseEscaping).unwrap();
        assert_eq!(result.total_size, 100);
    }

    #[test]
    fn absolute_subtree_with_relative_manifest_errors() {
        let m = make_rel(vec![FileEntry::file("a.txt", 10, 1)], vec![]);
        let result = subtree_rel_snapshot(&m, "/absolute/path", SymlinkPolicy::CollapseEscaping);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("absolute"));
    }

    #[test]
    fn relative_subtree_with_absolute_manifest_errors() {
        let m = make_abs(vec![FileEntry::file("/root/a.txt", 10, 1)], vec![]);
        let result = subtree_snapshot(&m, "relative/path", SymlinkPolicy::CollapseEscaping);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("relative"));
    }
}
