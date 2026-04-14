use crate::manifest::{AbsSnapshot, Manifest, Snapshot, SymlinkPolicy};
use crate::ops::subtree::{subtree_rel_snapshot, subtree_snapshot};
use crate::path_util::{is_absolute_path, normalize_path};
use tracing::debug;

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

/// Returns the longest common directory prefix of the given paths.
fn longest_common_prefix(dirs: &[&str]) -> String {
    if dirs.is_empty() {
        return String::new();
    }
    if dirs.len() == 1 {
        return dirs[0].to_string();
    }

    let parts: Vec<Vec<&str>> = dirs.iter().map(|p| p.split('/').collect()).collect();
    let min_len = parts.iter().map(|p| p.len()).min().unwrap_or(0);
    let mut common = Vec::new();

    for i in 0..min_len {
        let component = parts[0][i];
        if parts.iter().all(|p| p[i] == component) {
            common.push(component);
        } else {
            break;
        }
    }

    // For absolute paths, splitting "/a/b" gives ["", "a", "b"].
    // If only the empty-string prefix matched, the common root is "/".
    if common.len() == 1 && common[0].is_empty() {
        return "/".to_string();
    }
    common.join("/")
}

fn parent_dir(path: &str) -> String {
    match path.rfind('/') {
        Some(0) => "/".to_string(),
        Some(pos) => path[..pos].to_string(),
        None => String::new(),
    }
}

/// Collect all directory paths relevant for root determination.
fn all_dir_paths_generic<P: Clone, K: Clone>(manifest: &Manifest<P, K>) -> Vec<String> {
    let mut dirs = std::collections::HashSet::new();
    for f in &manifest.files {
        dirs.insert(parent_dir(&f.path));
    }
    for d in &manifest.dirs {
        dirs.insert(d.path.clone());
    }
    dirs.into_iter().collect()
}

fn find_root_for_path<'a>(path: &str, roots: &'a [String]) -> Option<&'a str> {
    roots.iter().find_map(|r| {
        let r_str = r.as_str();
        if r_str == "." {
            // "." root matches everything in a relative manifest
            Some(r_str)
        } else if path == r_str || path.starts_with(&format!("{r}/")) {
            Some(r_str)
        } else {
            None
        }
    })
}

fn validate_no_nested_roots(roots: &[String]) -> crate::Result<()> {
    for (i, a) in roots.iter().enumerate() {
        for (j, b) in roots.iter().enumerate() {
            if i != j && (a.starts_with(&format!("{b}/")) || b.starts_with(&format!("{a}/"))) {
                return Err(crate::SnapshotError::Validation(format!(
                    "root '{a}' is a subpath of root '{b}'"
                )));
            }
        }
    }
    Ok(())
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

/// Group remaining directory paths into roots.
fn group_into_roots(paths: &[&str]) -> Vec<String> {
    if paths.is_empty() {
        return vec![];
    }
    if paths.len() == 1 {
        return vec![paths[0].to_string()];
    }

    let prefix = longest_common_prefix(paths);
    if !prefix.is_empty() && prefix != "/" {
        return vec![prefix];
    }

    let mut groups: std::collections::HashMap<String, Vec<&str>> = std::collections::HashMap::new();
    for &p in paths {
        let key = if let Some(stripped) = p.strip_prefix('/') {
            match stripped.find('/') {
                Some(pos) => p[..pos + 1].to_string(),
                None => p.to_string(),
            }
        } else {
            match p.find('/') {
                Some(pos) => p[..pos].to_string(),
                None => p.to_string(),
            }
        };
        groups.entry(key).or_default().push(p);
    }

    let mut roots: Vec<String> = groups
        .values()
        .map(|group| longest_common_prefix(group))
        .collect();
    roots.sort();
    roots.dedup();
    roots
}

fn compute_roots<P: Clone, K: Clone>(
    manifest: &Manifest<P, K>,
    options: &PartitionOptions,
) -> crate::Result<Vec<String>> {
    let is_abs = manifest_is_absolute(manifest).unwrap_or(true);

    let mut all_paths = all_dir_paths_generic(manifest);
    if let Some(ref rp) = options.referenced_paths {
        for p in rp {
            all_paths.push(normalize_path(p));
        }
    }

    let explicit_roots: Vec<String> = options
        .roots
        .as_ref()
        .map(|r| r.iter().map(|s| normalize_path(s)).collect())
        .unwrap_or_default();

    if !explicit_roots.is_empty() {
        validate_no_nested_roots(&explicit_roots)?;
    }

    let auto_roots = if explicit_roots.is_empty() {
        if all_paths.is_empty() {
            vec![]
        } else {
            // For relative manifests, root-level files have parent_dir == "".
            // Filter those out for prefix computation, then use "." if all are root-level.
            let refs: Vec<&str> = all_paths.iter().map(|s| s.as_str()).collect();
            let non_empty: Vec<&str> = refs.iter().copied().filter(|s| !s.is_empty()).collect();
            if non_empty.is_empty() && !is_abs {
                // All files at root level of relative manifest
                vec![".".to_string()]
            } else if non_empty.len() < refs.len() && !is_abs {
                // Mix of root-level and nested — include root-level in prefix calc as "."
                let mut with_dot = non_empty.clone();
                with_dot.push(".");
                let prefix = longest_common_prefix(&with_dot);
                if prefix.is_empty() {
                    vec![".".to_string()]
                } else {
                    vec![prefix]
                }
            } else {
                let prefix = longest_common_prefix(&refs);
                vec![prefix]
            }
        }
    } else {
        let remaining: Vec<&str> = all_paths
            .iter()
            .filter(|p| {
                let p_str = p.as_str();
                // For relative manifests, empty string means root-level
                if p_str.is_empty() && !is_abs {
                    // Check if any explicit root is "." which covers root-level
                    !explicit_roots.iter().any(|r| r == ".")
                } else {
                    find_root_for_path(p_str, &explicit_roots).is_none()
                }
            })
            .map(|s| s.as_str())
            .collect();
        if remaining.is_empty() {
            vec![]
        } else {
            group_into_roots(&remaining)
        }
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
