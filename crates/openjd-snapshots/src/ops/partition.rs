// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

use crate::manifest::{AbsSnapshot, Manifest, Snapshot, SymlinkPolicy};
use crate::ops::subtree::{subtree_rel_snapshot, subtree_snapshot};
use crate::path_util::{
    ancestor_dirs, is_absolute_path, is_volume_root, longest_common_dir_prefix, normalize_path,
    strip_root, volume_root,
};
use std::collections::BTreeMap;
use tracing::debug;

#[allow(
    missing_docs,
    reason = "missing-docs burn-down: predates the lint; document these items, then delete this attribute"
)]
pub struct PartitionOptions {
    pub roots: Option<Vec<String>>,
    pub referenced_paths: Option<Vec<String>>,
    pub symlink_policy: SymlinkPolicy,
}

impl Default for PartitionOptions {
    fn default() -> Self {
        Self {
            roots: None,
            referenced_paths: None,
            symlink_policy: SymlinkPolicy::CollapseEscaping,
        }
    }
}

/// Returns true if `path` is `root` or lies beneath it, by the rule
/// `subtree_snapshot` uses to select a root's entries.
fn is_under(path: &str, root: &str) -> bool {
    strip_root(path, root).is_some()
}

fn validate_no_nested_roots(roots: &[String]) -> crate::Result<()> {
    for a in roots {
        for b in roots {
            if a != b && is_under(a, b) {
                return Err(crate::SnapshotError::Validation(format!(
                    "root '{a}' is a subpath of root '{b}'"
                )));
            }
        }
    }
    Ok(())
}

/// Groups `paths` by their [`volume_root`], in volume order. Paths on
/// different volumes (`/`, each drive root, the relative root `.`) share no
/// directory, so each volume gets its own roots.
fn by_volume<'a>(paths: &[&'a str]) -> BTreeMap<&'a str, Vec<&'a str>> {
    let mut groups: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for &p in paths {
        groups.entry(volume_root(p)).or_default().push(p);
    }
    groups
}

/// The roots for `paths` when no roots are given: one per volume, the
/// longest common directory of that volume's paths. For a single-volume
/// manifest this is [`Manifest::common_root`].
fn auto_roots(paths: &[&str]) -> Vec<String> {
    by_volume(paths)
        .values()
        .map(|group| longest_common_dir_prefix(group))
        .collect()
}

/// The top-level directory under its volume root that `path` lies in, or
/// `path` itself when it is a volume root or directly beneath one.
fn top_level_dir(path: &str) -> &str {
    std::iter::once(path)
        .chain(ancestor_dirs(path))
        .take_while(|a| !is_volume_root(a))
        .last()
        .unwrap_or(path)
}

/// The additional roots for `paths` that no given root contains: per volume,
/// the longest common directory if it is below the volume root, otherwise
/// one root per top-level directory, so the remainder is not collapsed onto
/// the volume root. A path that is itself the volume root (such as a file
/// directly under it) can only be held by the volume root.
fn remainder_roots(paths: &[&str]) -> Vec<String> {
    let mut roots = Vec::new();
    for (volume, group) in by_volume(paths) {
        let common = longest_common_dir_prefix(&group);
        if !is_volume_root(&common) || group.contains(&volume) {
            roots.push(common);
            continue;
        }
        let mut by_top: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
        for p in group {
            by_top.entry(top_level_dir(p)).or_default().push(p);
        }
        roots.extend(by_top.values().map(|g| longest_common_dir_prefix(g)));
    }
    roots
}

/// Detect whether the manifest uses absolute paths by checking the first file or dir entry.
fn manifest_is_absolute<P: Clone, K: Clone>(manifest: &Manifest<P, K>) -> Option<bool> {
    manifest
        .files
        .first()
        .map(|f| is_absolute_path(&f.path))
        .or_else(|| manifest.dirs.first().map(|d| is_absolute_path(&d.path)))
}

/// Validate that option paths match the manifest's path style.
fn validate_path_style<P: Clone, K: Clone>(
    manifest: &Manifest<P, K>,
    options: &PartitionOptions,
) -> crate::Result<()> {
    let manifest_abs = match manifest_is_absolute(manifest) {
        Some(v) => v,
        None => return Ok(()), // empty manifest, nothing to validate
    };

    if let Some(ref roots) = options.roots {
        for r in roots {
            let root_abs = is_absolute_path(r);
            if root_abs && !manifest_abs {
                return Err(crate::SnapshotError::Validation(
                    "absolute root with relative manifest paths".into(),
                ));
            }
            if !root_abs && manifest_abs {
                return Err(crate::SnapshotError::Validation(
                    "relative root with absolute manifest paths".into(),
                ));
            }
        }
    }

    if let Some(ref rp) = options.referenced_paths {
        for p in rp {
            let rp_abs = is_absolute_path(p);
            if rp_abs != manifest_abs {
                return Err(crate::SnapshotError::Validation(
                    "absolute referenced_paths with relative manifest paths".into(),
                ));
            }
        }
    }

    Ok(())
}

fn compute_roots<P: Clone, K: Clone>(
    manifest: &Manifest<P, K>,
    options: &PartitionOptions,
) -> crate::Result<Vec<String>> {
    let referenced: Vec<String> = options
        .referenced_paths
        .iter()
        .flatten()
        .map(|p| normalize_path(p))
        .collect();
    let mut all_paths = manifest.root_contributors();
    all_paths.extend(referenced.iter().map(String::as_str));
    all_paths.sort_unstable();
    all_paths.dedup();

    let explicit_roots: Vec<String> = options
        .roots
        .as_ref()
        .map(|r| r.iter().map(|s| normalize_path(s)).collect())
        .unwrap_or_default();

    if !explicit_roots.is_empty() {
        validate_no_nested_roots(&explicit_roots)?;
    }

    let auto_roots = if explicit_roots.is_empty() {
        auto_roots(&all_paths)
    } else {
        let remaining: Vec<&str> = all_paths
            .into_iter()
            .filter(|p| !explicit_roots.iter().any(|r| is_under(p, r)))
            .collect();
        remainder_roots(&remaining)
    };

    let mut all_roots = explicit_roots;
    let mut sorted_auto = auto_roots;
    sorted_auto.sort();
    for r in sorted_auto {
        if !all_roots.contains(&r) {
            all_roots.push(r);
        }
    }

    Ok(all_roots)
}

/// Partitions an absolute snapshot into multiple `(root, Snapshot)` pairs.
pub fn partition_manifest(
    manifest: &AbsSnapshot,
    options: &PartitionOptions,
) -> crate::Result<Vec<(String, Snapshot)>> {
    if options.symlink_policy == SymlinkPolicy::Preserve
        || options.symlink_policy == SymlinkPolicy::TransitiveIncludeTargets
    {
        return Err(crate::SnapshotError::Validation(format!(
            "symlink_policy {} is not supported for partition",
            options.symlink_policy
        )));
    }

    validate_path_style(manifest, options)?;

    let all_roots = compute_roots(manifest, options)?;
    debug!(roots = ?all_roots, "partition roots determined");

    let mut result = Vec::new();
    for root in &all_roots {
        let snapshot = subtree_snapshot(manifest, root, options.symlink_policy)?;
        debug!(root = %root, files = snapshot.files.len(), "partitioned root");
        result.push((root.clone(), snapshot));
    }

    Ok(result)
}

/// Partitions a relative snapshot into multiple `(root, Snapshot)` pairs.
pub fn partition_rel_manifest(
    manifest: &Snapshot,
    options: &PartitionOptions,
) -> crate::Result<Vec<(String, Snapshot)>> {
    if options.symlink_policy == SymlinkPolicy::Preserve
        || options.symlink_policy == SymlinkPolicy::TransitiveIncludeTargets
    {
        return Err(crate::SnapshotError::Validation(format!(
            "symlink_policy {} is not supported for partition",
            options.symlink_policy
        )));
    }

    validate_path_style(manifest, options)?;

    let all_roots = compute_roots(manifest, options)?;

    let mut result = Vec::new();
    for root in &all_roots {
        let snapshot = subtree_rel_snapshot(manifest, root, options.symlink_policy)?;
        result.push((root.clone(), snapshot));
    }

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hash::HashAlgorithm;
    use crate::{DirEntry, FileEntry, Manifest, DEFAULT_FILE_CHUNK_SIZE};

    fn make_abs(files: Vec<FileEntry>, dirs: Vec<DirEntry>) -> AbsSnapshot {
        Manifest::new(HashAlgorithm::Xxh128, DEFAULT_FILE_CHUNK_SIZE)
            .with_files(files)
            .with_dirs(dirs)
    }

    #[test]
    fn single_directory_auto_partitions() {
        let m = make_abs(
            vec![
                FileEntry::file("/projects/scene/a.txt", 10, 1),
                FileEntry::file("/projects/scene/b.txt", 20, 2),
            ],
            vec![],
        );
        let result = partition_manifest(&m, &PartitionOptions::default()).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].0, "/projects/scene");
        assert_eq!(result[0].1.files.len(), 2);
    }

    #[test]
    fn multiple_dirs_under_common_root() {
        let m = make_abs(
            vec![
                FileEntry::file("/root/a/file1.txt", 10, 1),
                FileEntry::file("/root/b/file2.txt", 20, 2),
            ],
            vec![],
        );
        let result = partition_manifest(&m, &PartitionOptions::default()).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].0, "/root");
        assert_eq!(result[0].1.files.len(), 2);
    }

    #[test]
    fn explicit_roots_partition() {
        let m = make_abs(
            vec![
                FileEntry::file("/a/file1.txt", 10, 1),
                FileEntry::file("/b/file2.txt", 20, 2),
            ],
            vec![],
        );
        let opts = PartitionOptions {
            roots: Some(vec!["/a".into(), "/b".into()]),
            ..Default::default()
        };
        let result = partition_manifest(&m, &opts).unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].0, "/a");
        assert_eq!(result[0].1.files.len(), 1);
        assert_eq!(result[0].1.files[0].path, "file1.txt");
        assert_eq!(result[1].0, "/b");
        assert_eq!(result[1].1.files.len(), 1);
    }

    #[test]
    fn empty_partition_for_explicit_root() {
        let m = make_abs(vec![FileEntry::file("/a/file.txt", 10, 1)], vec![]);
        let opts = PartitionOptions {
            roots: Some(vec!["/a".into(), "/empty".into()]),
            ..Default::default()
        };
        let result = partition_manifest(&m, &opts).unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result[1].0, "/empty");
        assert!(result[1].1.files.is_empty());
    }

    #[test]
    fn referenced_paths_influence_root() {
        let m = make_abs(
            vec![FileEntry::file("/projects/scene/render/out.exr", 10, 1)],
            vec![],
        );
        let opts = PartitionOptions {
            referenced_paths: Some(vec!["/projects/scene/assets/tex.png".into()]),
            ..Default::default()
        };
        let result = partition_manifest(&m, &opts).unwrap();
        assert_eq!(result.len(), 1);
        // Root should be /projects/scene (common prefix of render/ and assets/)
        assert_eq!(result[0].0, "/projects/scene");
    }

    #[test]
    fn nested_roots_rejected() {
        let m = make_abs(vec![], vec![]);
        let opts = PartitionOptions {
            roots: Some(vec!["/a".into(), "/a/b".into()]),
            ..Default::default()
        };
        assert!(partition_manifest(&m, &opts).is_err());
    }

    fn roots(m: &AbsSnapshot, opts: &PartitionOptions) -> Vec<String> {
        partition_manifest(m, opts)
            .unwrap()
            .into_iter()
            .map(|(r, _)| r)
            .collect()
    }

    #[test]
    fn drive_root_keeps_its_slash() {
        let m = make_abs(
            vec![
                FileEntry::file("C:/foo/a.txt", 1, 1),
                FileEntry::file("C:/bar/b.txt", 1, 1),
            ],
            vec![],
        );
        assert_eq!(roots(&m, &PartitionOptions::default()), ["C:/"]);
        let m = make_abs(vec![FileEntry::file("C:/a.txt", 1, 1)], vec![]);
        assert_eq!(roots(&m, &PartitionOptions::default()), ["C:/"]);
    }

    #[test]
    fn one_auto_root_per_drive() {
        let m = make_abs(
            vec![
                FileEntry::file("D:/proj/out/b.exr", 1, 1),
                FileEntry::file("C:/proj/scene/a.ma", 1, 1),
                FileEntry::file("C:/proj/tex/t.png", 1, 1),
            ],
            vec![],
        );
        let parts = partition_manifest(&m, &PartitionOptions::default()).unwrap();
        let got: Vec<(&str, usize)> = parts
            .iter()
            .map(|(r, s)| (r.as_str(), s.files.len()))
            .collect();
        assert_eq!(got, [("C:/proj", 2), ("D:/proj/out", 1)]);
    }

    #[test]
    fn non_empty_dir_does_not_widen_root() {
        // `/a/b` is implied by the file beneath it, as in `common_root`.
        let m = make_abs(
            vec![FileEntry::file("/a/b/c/f.txt", 1, 1)],
            vec![DirEntry::new("/a/b")],
        );
        assert_eq!(roots(&m, &PartitionOptions::default()), ["/a/b/c"]);
        // An empty dir does widen it.
        let m = make_abs(
            vec![FileEntry::file("/a/b/c/f.txt", 1, 1)],
            vec![DirEntry::new("/a/e")],
        );
        assert_eq!(roots(&m, &PartitionOptions::default()), ["/a"]);
    }

    #[test]
    fn explicit_volume_root_covers_everything() {
        let m = make_abs(
            vec![
                FileEntry::file("/a/f.txt", 1, 1),
                FileEntry::file("/b/c/g.txt", 1, 1),
            ],
            vec![],
        );
        let opts = PartitionOptions {
            roots: Some(vec!["/".into()]),
            ..Default::default()
        };
        assert_eq!(roots(&m, &opts), ["/"]);

        let m = make_abs(vec![FileEntry::file("C:/a/f.txt", 1, 1)], vec![]);
        let opts = PartitionOptions {
            roots: Some(vec!["C:/".into()]),
            ..Default::default()
        };
        assert_eq!(roots(&m, &opts), ["C:/"]);
    }

    #[test]
    fn explicit_root_nested_under_volume_root_rejected() {
        let m = make_abs(vec![], vec![]);
        let opts = PartitionOptions {
            roots: Some(vec!["/".into(), "/a".into()]),
            ..Default::default()
        };
        assert_eq!(
            partition_manifest(&m, &opts)
                .map(|_| ())
                .unwrap_err()
                .to_string(),
            "Manifest validation error: root '/a' is a subpath of root '/'"
        );
    }

    #[test]
    fn remainder_roots_split_by_top_level_dir() {
        let m = make_abs(
            vec![
                FileEntry::file("/x/a.txt", 1, 1),
                FileEntry::file("/p/q/b.txt", 1, 1),
                FileEntry::file("/r/c.txt", 1, 1),
                FileEntry::file("C:/s/t/d.txt", 1, 1),
            ],
            vec![],
        );
        let opts = PartitionOptions {
            roots: Some(vec!["/x".into()]),
            ..Default::default()
        };
        assert_eq!(roots(&m, &opts), ["/x", "/p/q", "/r", "C:/s/t"]);
    }

    /// Pins auto-partitioning of a single-volume manifest to
    /// [`Manifest::common_root`]: both route through the same contributor
    /// and common-prefix rules, so they must pick the same root.
    #[test]
    fn auto_root_matches_common_root() {
        let cases: Vec<(Vec<FileEntry>, Vec<DirEntry>)> = vec![
            (vec![FileEntry::file("/foo/bar/baz.txt", 1, 1)], vec![]),
            (vec![], vec![DirEntry::new("/foo/bar")]),
            (
                vec![
                    FileEntry::file("/a/b.txt", 1, 1),
                    FileEntry::file("/a/c.txt", 1, 1),
                ],
                vec![],
            ),
            (
                vec![FileEntry::file("/a/b.txt", 1, 1)],
                vec![DirEntry::new("/c")],
            ),
            (
                vec![FileEntry::file("/a/b/c/f.txt", 1, 1)],
                vec![DirEntry::new("/a/b"), DirEntry::new("/")],
            ),
            (
                vec![
                    FileEntry::file("/app/bar/x", 1, 1),
                    FileEntry::file("/app/baz/y", 1, 1),
                ],
                vec![],
            ),
            (vec![FileEntry::file("/f.txt", 1, 1)], vec![]),
            (
                vec![
                    FileEntry::file("C:/foo/a.txt", 1, 1),
                    FileEntry::file("C:/bar/b.txt", 1, 1),
                ],
                vec![],
            ),
            (vec![FileEntry::file("C:/foo/a.txt", 1, 1)], vec![]),
        ];
        for (files, dirs) in cases {
            let m = make_abs(files, dirs);
            let expected = m.common_root().unwrap();
            assert_eq!(roots(&m, &PartitionOptions::default()), [expected], "{m:?}");
        }

        let rel_cases: Vec<Vec<FileEntry>> = vec![
            vec![FileEntry::file("foo/bar/baz.txt", 1, 1)],
            vec![FileEntry::file("baz.txt", 1, 1)],
            vec![
                FileEntry::file("foo/a.txt", 1, 1),
                FileEntry::file("bar/b.txt", 1, 1),
            ],
            vec![
                FileEntry::file("a.txt", 1, 1),
                FileEntry::file("d/b.txt", 1, 1),
            ],
        ];
        for files in rel_cases {
            let m: Snapshot =
                Manifest::new(HashAlgorithm::Xxh128, DEFAULT_FILE_CHUNK_SIZE).with_files(files);
            let got: Vec<String> = partition_rel_manifest(&m, &PartitionOptions::default())
                .unwrap()
                .into_iter()
                .map(|(r, _)| r)
                .collect();
            assert_eq!(got, [m.common_root().unwrap()], "{m:?}");
        }
    }

    #[test]
    fn preserve_policy_rejected() {
        let m = make_abs(vec![], vec![]);
        let opts = PartitionOptions {
            symlink_policy: SymlinkPolicy::Preserve,
            ..Default::default()
        };
        assert!(partition_manifest(&m, &opts).is_err());
    }
}
