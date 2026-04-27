// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

use crate::manifest::{
    Abs, AbsSnapshot, AbsSnapshotDiff, Diff, DirEntry, FileEntry, Full, Manifest, Snapshot,
    SnapshotDiff,
};
use crate::path_util::normalize_path;

fn join_path(prefix: &str, path: &str) -> String {
    normalize_path(&format!("{prefix}/{path}"))
}

fn join_file(prefix: &str, f: &FileEntry) -> FileEntry {
    let mut entry = f.clone();
    entry.path = join_path(prefix, &f.path);
    if let Some(ref target) = f.symlink_target {
        entry.symlink_target = Some(join_path(prefix, target));
    }
    entry
}

fn join_dir(prefix: &str, d: &DirEntry) -> DirEntry {
    if d.deleted {
        DirEntry::deleted(join_path(prefix, &d.path))
    } else {
        DirEntry::new(join_path(prefix, &d.path))
    }
}

fn join_impl<P, K, Q>(manifest: &Manifest<P, K>, prefix: &str) -> Manifest<Q, K>
where
    P: Clone,
    K: Clone,
{
    let files = manifest
        .files
        .iter()
        .map(|f| join_file(prefix, f))
        .collect();
    let dirs = manifest.dirs.iter().map(|d| join_dir(prefix, d)).collect();
    let mut result = Manifest::new(manifest.hash_alg, manifest.file_chunk_size_bytes);
    result.files = files;
    result.dirs = dirs;
    result.total_size = manifest.total_size;
    result.parent_manifest_hash = None;
    result
}

fn validate_prefix(prefix: &str) -> crate::Result<()> {
    if prefix.is_empty() {
        return Err(crate::SnapshotError::Validation(
            "prefix cannot be empty".into(),
        ));
    }
    Ok(())
}

/// Joins an absolute prefix to a relative snapshot, producing an absolute snapshot.
pub fn join_snapshot(manifest: &Snapshot, prefix: &str) -> crate::Result<AbsSnapshot> {
    validate_prefix(prefix)?;
    Ok(join_impl::<_, Full, Abs>(manifest, prefix))
}

/// Joins an absolute prefix to a relative diff, producing an absolute diff.
pub fn join_snapshot_diff(manifest: &SnapshotDiff, prefix: &str) -> crate::Result<AbsSnapshotDiff> {
    validate_prefix(prefix)?;
    Ok(join_impl::<_, Diff, Abs>(manifest, prefix))
}

/// Joins a relative prefix to a relative snapshot, producing a relative snapshot.
pub fn join_snapshot_rel(manifest: &Snapshot, prefix: &str) -> crate::Result<Snapshot> {
    validate_prefix(prefix)?;
    Ok(join_impl::<_, Full, crate::manifest::Rel>(manifest, prefix))
}

/// Joins a relative prefix to a relative diff, producing a relative diff.
pub fn join_snapshot_diff_rel(
    manifest: &SnapshotDiff,
    prefix: &str,
) -> crate::Result<SnapshotDiff> {
    validate_prefix(prefix)?;
    Ok(join_impl::<_, Diff, crate::manifest::Rel>(manifest, prefix))
}

use crate::manifest::{AbsManifest, RelManifest};

/// Joins a prefix to all paths in a relative manifest, producing an AbsManifest.
pub fn join_manifest(manifest: &RelManifest, prefix: &str) -> crate::Result<AbsManifest> {
    match manifest {
        RelManifest::Snapshot(s) => Ok(AbsManifest::Snapshot(join_snapshot(s, prefix)?)),
        RelManifest::Diff(d) => Ok(AbsManifest::Diff(join_snapshot_diff(d, prefix)?)),
    }
}

/// Joins a relative prefix to all paths in a relative manifest, producing a RelManifest.
pub fn join_manifest_rel(manifest: &RelManifest, prefix: &str) -> crate::Result<RelManifest> {
    match manifest {
        RelManifest::Snapshot(s) => Ok(RelManifest::Snapshot(join_snapshot_rel(s, prefix)?)),
        RelManifest::Diff(d) => Ok(RelManifest::Diff(join_snapshot_diff_rel(d, prefix)?)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hash::HashAlgorithm;
    use crate::{DirEntry, FileEntry, Manifest, DEFAULT_FILE_CHUNK_SIZE};

    fn make_snapshot(files: Vec<FileEntry>, dirs: Vec<DirEntry>) -> Snapshot {
        Manifest::new(HashAlgorithm::Xxh128, DEFAULT_FILE_CHUNK_SIZE)
            .with_files(files)
            .with_dirs(dirs)
    }

    fn make_snapshot_diff(files: Vec<FileEntry>, dirs: Vec<DirEntry>) -> SnapshotDiff {
        Manifest::new(HashAlgorithm::Xxh128, DEFAULT_FILE_CHUNK_SIZE)
            .with_files(files)
            .with_dirs(dirs)
    }

    #[test]
    fn join_with_absolute_prefix() {
        let m = make_snapshot(
            vec![FileEntry::file("a.txt", 100, 1)],
            vec![DirEntry::new("subdir")],
        );
        let result = join_snapshot(&m, "/root/project").unwrap();
        assert_eq!(result.files[0].path, "/root/project/a.txt");
        assert_eq!(result.dirs[0].path, "/root/project/subdir");
    }

    #[test]
    fn symlink_targets_prefixed() {
        let m = make_snapshot(vec![FileEntry::symlink("link", "target.txt")], vec![]);
        let result = join_snapshot(&m, "/root").unwrap();
        assert_eq!(result.files[0].path, "/root/link");
        assert_eq!(
            result.files[0].symlink_target.as_deref(),
            Some("/root/target.txt")
        );
    }

    #[test]
    fn parent_manifest_hash_cleared() {
        let m = make_snapshot(vec![FileEntry::file("a.txt", 10, 1)], vec![])
            .with_parent_hash(Some("oldhash".into()));
        let result = join_snapshot(&m, "/root").unwrap();
        assert!(result.parent_manifest_hash.is_none());
    }

    #[test]
    fn total_size_preserved() {
        let m = make_snapshot(
            vec![
                FileEntry::file("a.txt", 100, 1),
                FileEntry::file("b.txt", 200, 2),
            ],
            vec![],
        );
        let result = join_snapshot(&m, "/root").unwrap();
        assert_eq!(result.total_size, 300);
    }

    #[test]
    fn join_snapshot_diff_works() {
        let m = make_snapshot_diff(
            vec![FileEntry::file("a.txt", 50, 1), FileEntry::deleted("b.txt")],
            vec![],
        );
        let result = join_snapshot_diff(&m, "/out").unwrap();
        assert_eq!(result.files[0].path, "/out/a.txt");
        assert_eq!(result.files[1].path, "/out/b.txt");
        assert!(result.files[1].deleted);
    }

    #[test]
    fn join_normalizes_paths() {
        let m = make_snapshot(vec![FileEntry::file("sub/../a.txt", 10, 1)], vec![]);
        let result = join_snapshot(&m, "/root/./dir").unwrap();
        assert_eq!(result.files[0].path, "/root/dir/a.txt");
    }
}
