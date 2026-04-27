// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

use crate::manifest::{DirEntry, FileEntry, Manifest, ManifestEntry};

/// Filters a manifest's entries using the provided predicate.
///
/// Returns a new manifest of the same type containing only entries
/// for which the filter returns `true`. Total size is recomputed.
pub fn filter_manifest<P: Clone, K: Clone>(
    manifest: &Manifest<P, K>,
    filter: &dyn Fn(&ManifestEntry) -> bool,
) -> Manifest<P, K> {
    let files: Vec<FileEntry> = manifest
        .files
        .iter()
        .filter(|f| filter(&ManifestEntry::File(f)))
        .cloned()
        .collect();
    let dirs: Vec<DirEntry> = manifest
        .dirs
        .iter()
        .filter(|d| filter(&ManifestEntry::Dir(d)))
        .cloned()
        .collect();

    let mut result = Manifest::new(manifest.hash_alg, manifest.file_chunk_size_bytes);
    result.files = files;
    result.dirs = dirs;
    result.parent_manifest_hash = manifest.parent_manifest_hash.clone();
    result.recompute_total_size();
    result
}

pub struct IncludeExcludePathsFilter {
    include: Vec<glob::Pattern>,
    exclude: Vec<glob::Pattern>,
}

impl IncludeExcludePathsFilter {
    pub fn new(include: &[&str], exclude: &[&str]) -> crate::Result<Self> {
        fn compile(patterns: &[&str]) -> crate::Result<Vec<glob::Pattern>> {
            patterns
                .iter()
                .map(|p| {
                    glob::Pattern::new(p).map_err(|e| {
                        crate::SnapshotError::Validation(format!("invalid glob pattern '{p}': {e}"))
                    })
                })
                .collect()
        }
        Ok(Self {
            include: compile(include)?,
            exclude: compile(exclude)?,
        })
    }

    pub fn matches(&self, entry: &ManifestEntry) -> bool {
        self.matches_path(entry.path())
    }

    pub fn matches_path(&self, path: &str) -> bool {
        let included = self.include.is_empty() || self.include.iter().any(|p| p.matches(path));
        included && !self.exclude.iter().any(|p| p.matches(path))
    }
}

impl std::fmt::Debug for IncludeExcludePathsFilter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "IncludeExcludePathsFilter {{ include: {:?}, exclude: {:?} }}",
            self.include.iter().map(|p| p.as_str()).collect::<Vec<_>>(),
            self.exclude.iter().map(|p| p.as_str()).collect::<Vec<_>>(),
        )
    }
}

impl std::fmt::Display for IncludeExcludePathsFilter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Debug::fmt(self, f)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hash::HashAlgorithm;
    use crate::{DirEntry, FileEntry, Manifest, Snapshot, DEFAULT_FILE_CHUNK_SIZE};

    fn make_snapshot(files: Vec<FileEntry>, dirs: Vec<DirEntry>) -> Snapshot {
        Manifest::new(HashAlgorithm::Xxh128, DEFAULT_FILE_CHUNK_SIZE)
            .with_files(files)
            .with_dirs(dirs)
    }

    #[test]
    fn filter_keeps_matching_removes_nonmatching() {
        let m = make_snapshot(
            vec![
                FileEntry::file("a.txt", 10, 1),
                FileEntry::file("b.rs", 20, 2),
            ],
            vec![],
        );
        let result = filter_manifest(&m, &|e| e.path().ends_with(".txt"));
        assert_eq!(result.files.len(), 1);
        assert_eq!(result.files[0].path, "a.txt");
    }

    #[test]
    fn include_patterns() {
        let m = make_snapshot(
            vec![
                FileEntry::file("a.txt", 10, 1),
                FileEntry::file("b.rs", 20, 2),
                FileEntry::file("c.txt", 30, 3),
            ],
            vec![],
        );
        let f = IncludeExcludePathsFilter::new(&["*.txt"], &[]).unwrap();
        let result = filter_manifest(&m, &|e| f.matches(e));
        assert_eq!(result.files.len(), 2);
        assert!(result.files.iter().all(|f| f.path.ends_with(".txt")));
    }

    #[test]
    fn exclude_patterns() {
        let m = make_snapshot(
            vec![
                FileEntry::file("a.txt", 10, 1),
                FileEntry::file("b.tmp", 20, 2),
                FileEntry::file("c.txt", 30, 3),
            ],
            vec![],
        );
        let f = IncludeExcludePathsFilter::new(&[], &["*.tmp"]).unwrap();
        let result = filter_manifest(&m, &|e| f.matches(e));
        assert_eq!(result.files.len(), 2);
        assert!(result.files.iter().all(|f| !f.path.ends_with(".tmp")));
    }

    #[test]
    fn include_and_exclude_patterns() {
        let m = make_snapshot(
            vec![
                FileEntry::file("a.txt", 10, 1),
                FileEntry::file("backup.txt", 20, 2),
                FileEntry::file("c.rs", 30, 3),
            ],
            vec![],
        );
        let f = IncludeExcludePathsFilter::new(&["*.txt"], &["backup*"]).unwrap();
        let result = filter_manifest(&m, &|e| f.matches(e));
        assert_eq!(result.files.len(), 1);
        assert_eq!(result.files[0].path, "a.txt");
    }

    #[test]
    fn empty_include_means_include_all() {
        let m = make_snapshot(
            vec![
                FileEntry::file("a.txt", 10, 1),
                FileEntry::file("b.rs", 20, 2),
            ],
            vec![],
        );
        let f = IncludeExcludePathsFilter::new(&[], &[]).unwrap();
        let result = filter_manifest(&m, &|e| f.matches(e));
        assert_eq!(result.files.len(), 2);
    }

    #[test]
    fn total_size_recomputed() {
        let m = make_snapshot(
            vec![
                FileEntry::file("a.txt", 100, 1),
                FileEntry::file("b.txt", 200, 2),
                FileEntry::file("c.rs", 300, 3),
            ],
            vec![],
        );
        assert_eq!(m.total_size, 600);
        let result = filter_manifest(&m, &|e| e.path().ends_with(".txt"));
        assert_eq!(result.total_size, 300);
    }

    #[test]
    fn filter_dirs() {
        let m = make_snapshot(vec![], vec![DirEntry::new("src"), DirEntry::new("build")]);
        let result = filter_manifest(&m, &|e| e.path() == "src");
        assert_eq!(result.dirs.len(), 1);
        assert_eq!(result.dirs[0].path, "src");
    }

    #[test]
    fn preserves_parent_manifest_hash() {
        let m = make_snapshot(vec![FileEntry::file("a.txt", 10, 1)], vec![])
            .with_parent_hash(Some("abc123".into()));
        let result = filter_manifest(&m, &|_| true);
        assert_eq!(result.parent_manifest_hash.as_deref(), Some("abc123"));
    }
}
