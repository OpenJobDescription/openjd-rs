// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

use crate::manifest::{
    AbsSnapshot, AbsSnapshotDiff, DirEntry, FileEntry, Manifest, Snapshot, SnapshotDiff,
    SymlinkPolicy,
};
use crate::path_util::normalize_path;
use std::collections::{HashMap, HashSet};
use tracing::warn;

fn strip_prefix(path: &str, prefix: &str) -> Option<String> {
    if prefix.is_empty() || prefix == "." {
        return Some(path.to_string());
    }
    if path == prefix {
        return Some(String::new());
    }
    let with_slash = if prefix.ends_with('/') {
        prefix.to_string()
    } else {
        format!("{prefix}/")
    };
    path.strip_prefix(&with_slash).map(|s| s.to_string())
}

/// Resolve a symlink target to a non-symlink file entry, following chains and detecting cycles.
fn resolve_symlink<'a>(
    target: &str,
    file_lookup: &HashMap<&str, &'a FileEntry>,
    max_depth: usize,
) -> Option<&'a FileEntry> {
    let mut current = target;
    let mut visited = HashSet::new();
    for _ in 0..max_depth {
        if !visited.insert(current) {
            warn!(target, "symlink cycle detected, skipping");
            return None; // cycle
        }
        match file_lookup.get(current) {
            Some(entry) => {
                if entry.symlink_target.is_none() {
                    return Some(entry);
                }
                current = entry.symlink_target.as_ref().unwrap();
            }
            None => return None,
        }
    }
    None
}

/// Check if a target is a directory by looking for files under it.
fn is_dir_target(target: &str, file_lookup: &HashMap<&str, &FileEntry>) -> bool {
    let prefix = format!("{target}/");
    file_lookup.keys().any(|k| k.starts_with(&prefix))
}

/// Expand a directory symlink: find all files under the target directory and remap them.
///
/// Nested symlinks inside the expanded directory are handled according to `symlink_policy`:
/// - `CollapseAll`: nested symlinks are resolved to their real targets.
/// - `CollapseEscaping`: nested symlinks are preserved (remapped but kept as symlinks),
///   since the escaping decision was already made on the parent directory symlink.
fn expand_dir_symlink(
    link_rel_path: &str,
    target: &str,
    files: &[FileEntry],
    file_lookup: &HashMap<&str, &FileEntry>,
    symlink_policy: SymlinkPolicy,
) -> Vec<FileEntry> {
    let target_prefix = format!("{target}/");
    let mut result = Vec::new();
    for f in files {
        if !f.path.starts_with(&target_prefix) {
            continue;
        }
        let suffix = &f.path[target_prefix.len()..];
        let new_path = format!("{link_rel_path}/{suffix}");
        if let Some(ref sym_target) = f.symlink_target {
            match symlink_policy {
                SymlinkPolicy::CollapseAll => {
                    // Resolve nested symlinks to their real targets
                    if let Some(real) = resolve_symlink(sym_target, file_lookup, 64) {
                        let mut entry = real.clone();
                        entry.path = new_path;
                        result.push(entry);
                    }
                }
                _ => {
                    // Preserve nested symlinks with remapped path
                    let mut entry = f.clone();
                    entry.path = new_path;
                    result.push(entry);
                }
            }
        } else {
            let mut entry = f.clone();
            entry.path = new_path;
            result.push(entry);
        }
    }
    result
}

fn process_files(
    files: &[FileEntry],
    prefix: &str,
    symlink_policy: SymlinkPolicy,
    file_lookup: &HashMap<&str, &FileEntry>,
) -> crate::Result<Vec<FileEntry>> {
    let mut result = Vec::new();
    for f in files {
        let rel_path = match strip_prefix(&f.path, prefix) {
            Some(p) if !p.is_empty() => p,
            Some(_) => continue,
            None => continue,
        };

        if let Some(ref target) = f.symlink_target {
            match symlink_policy {
                SymlinkPolicy::ExcludeAll => continue,
                SymlinkPolicy::CollapseAll => {
                    // Check if target is a directory
                    if is_dir_target(target, file_lookup) {
                        result.extend(expand_dir_symlink(
                            &rel_path,
                            target,
                            files,
                            file_lookup,
                            symlink_policy,
                        ));
                        continue;
                    }
                    // cycle or missing - skip
                    if let Some(real) = resolve_symlink(target, file_lookup, 64) {
                        let mut entry = real.clone();
                        entry.path = rel_path;
                        result.push(entry);
                    }
                    continue;
                }
                SymlinkPolicy::Preserve | SymlinkPolicy::TransitiveIncludeTargets => {
                    return Err(crate::SnapshotError::Validation(format!(
                        "symlink_policy {} is not supported for subtree",
                        symlink_policy
                    )));
                }
                SymlinkPolicy::CollapseEscaping | SymlinkPolicy::ExcludeEscaping => {
                    match strip_prefix(target, prefix) {
                        Some(rel_target) => {
                            // Non-escaping symlink
                            let mut entry = f.clone();
                            entry.path = rel_path;
                            entry.symlink_target = Some(rel_target);
                            result.push(entry);
                        }
                        None => {
                            // Escaping symlink
                            if symlink_policy == SymlinkPolicy::ExcludeEscaping {
                                continue;
                            }
                            // CollapseEscaping: check if directory symlink
                            if is_dir_target(target, file_lookup) {
                                result.extend(expand_dir_symlink(
                                    &rel_path,
                                    target,
                                    files,
                                    file_lookup,
                                    symlink_policy,
                                ));
                                continue;
                            }
                            // cycle or missing - skip
                            if let Some(real) = resolve_symlink(target, file_lookup, 64) {
                                let mut entry = real.clone();
                                entry.path = rel_path;
                                result.push(entry);
                            }
                        }
                    }
                }
            }
        } else {
            let mut entry = f.clone();
            entry.path = rel_path;
            result.push(entry);
        }
    }
    Ok(result)
}

fn process_dirs(dirs: &[DirEntry], prefix: &str) -> Vec<DirEntry> {
    dirs.iter()
        .filter_map(|d| {
            strip_prefix(&d.path, prefix).and_then(|rel| {
                if rel.is_empty() {
                    None
                } else {
                    Some(if d.deleted {
                        DirEntry::deleted(&rel)
                    } else {
                        DirEntry::new(&rel)
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

    let file_lookup: HashMap<&str, &FileEntry> = manifest
        .files
        .iter()
        .map(|f| (f.path.as_str(), f))
        .collect();

    let files = process_files(&manifest.files, &prefix, symlink_policy, &file_lookup)?;
    let dirs = process_dirs(&manifest.dirs, &prefix);

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
