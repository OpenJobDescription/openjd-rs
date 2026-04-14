use crate::hash::HashAlgorithm;
use crate::manifest::{AbsSnapshot, DirEntry, FileEntry, Manifest, SymlinkPolicy};
use crate::path_util::normalize_path;
use crate::{SnapshotError, DEFAULT_FILE_CHUNK_SIZE};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;
use tracing::{debug, warn};
use walkdir::WalkDir;

#[derive(Debug, Clone)]
pub struct CollectOptions {
    pub optional_filenames: Vec<PathBuf>,
    pub symlink_policy: SymlinkPolicy,
    pub file_chunk_size_bytes: Option<i64>,
}

impl Default for CollectOptions {
    fn default() -> Self {
        Self {
            optional_filenames: vec![],
            symlink_policy: SymlinkPolicy::CollapseEscaping,
            file_chunk_size_bytes: None,
        }
    }
}

/// Metadata helper: get (size, mtime_micros, runnable) from std::fs::Metadata.
fn file_meta(meta: &std::fs::Metadata) -> crate::Result<(u64, u64, bool)> {
    let size = meta.len();
    let mtime = meta
        .modified()?
        .duration_since(UNIX_EPOCH)
        .map_err(|e| SnapshotError::Other(e.to_string()))?
        .as_micros() as u64;
    #[cfg(unix)]
    let runnable = {
        use std::os::unix::fs::PermissionsExt;
        meta.permissions().mode() & 0o111 != 0
    };
    #[cfg(not(unix))]
    let runnable = false;
    Ok((size, mtime, runnable))
}

fn abs_normalized(p: &Path) -> crate::Result<String> {
    let abs = std::path::absolute(p).map_err(|e| {
        SnapshotError::Io(std::io::Error::new(
            e.kind(),
            format!("{}: {}", p.display(), e),
        ))
    })?;
    Ok(normalize_path(&abs.to_string_lossy()))
}

/// Like abs_normalized but doesn't follow the final symlink component.
fn abs_normalized_no_follow(p: &Path) -> crate::Result<String> {
    let abs = std::path::absolute(p).map_err(|e| {
        SnapshotError::Io(std::io::Error::new(
            e.kind(),
            format!("{}: {}", p.display(), e),
        ))
    })?;
    Ok(normalize_path(&abs.to_string_lossy()))
}

/// Represents a deferred symlink found during directory walking.
struct DeferredSymlink {
    /// The absolute normalized path of the symlink itself.
    path: String,
    /// The filesystem path for read_link / metadata calls.
    fs_path: PathBuf,
}

/// Collects directories and files into an absolute-path snapshot manifest.
///
/// Walks the given directories recursively and includes the specified filenames,
/// capturing metadata (size, mtime, permissions) but not computing hashes.
/// Symlinks are handled according to the given policy.
pub fn collect_abs_snapshot(
    directories: &[impl AsRef<Path>],
    filenames: &[impl AsRef<Path>],
    options: CollectOptions,
) -> crate::Result<AbsSnapshot> {
    let chunk_size = options
        .file_chunk_size_bytes
        .unwrap_or(DEFAULT_FILE_CHUNK_SIZE);
    let mut files: Vec<FileEntry> = Vec::new();
    let mut dirs: Vec<DirEntry> = Vec::new();
    let mut deferred_symlinks: Vec<DeferredSymlink> = Vec::new();
    let mut seen_paths: HashSet<String> = HashSet::new();

    // Walk directories
    for dir in directories {
        let dir = dir.as_ref();
        if !dir.exists() {
            return Err(SnapshotError::FileNotFound(dir.display().to_string()));
        }
        if !dir.is_dir() {
            return Err(SnapshotError::Validation(format!(
                "not a directory: {}",
                dir.display()
            )));
        }
        for entry in WalkDir::new(dir).follow_links(false) {
            let entry =
                entry.map_err(|e| SnapshotError::Io(std::io::Error::other(e.to_string())))?;
            let ft = entry.file_type();
            let path = entry.path();

            if ft.is_symlink() {
                let norm = abs_normalized_no_follow(path)?;
                if seen_paths.insert(norm.clone()) {
                    deferred_symlinks.push(DeferredSymlink {
                        path: norm,
                        fs_path: path.to_path_buf(),
                    });
                }
            } else if ft.is_file() {
                let norm = abs_normalized(path)?;
                if seen_paths.insert(norm.clone()) {
                    let meta = std::fs::metadata(path)?;
                    let (size, mtime, runnable) = file_meta(&meta)?;
                    let mut entry = FileEntry::file(norm, size, mtime);
                    entry.runnable = runnable;
                    files.push(entry);
                }
            } else if ft.is_dir() {
                let norm = abs_normalized(path)?;
                if seen_paths.insert(norm.clone()) {
                    dirs.push(DirEntry::new(norm));
                }
            }
        }
    }

    // Process required filenames
    for filename in filenames {
        let filename = filename.as_ref();
        if !filename.exists() {
            // Check with symlink_metadata too - the file truly doesn't exist
            match std::fs::symlink_metadata(filename) {
                Ok(_) => {} // exists as a symlink
                Err(_) => {
                    return Err(SnapshotError::FileNotFound(filename.display().to_string()));
                }
            }
        }
        collect_single_file(
            filename,
            &mut files,
            &mut deferred_symlinks,
            &mut seen_paths,
        )?;
    }

    // Process optional filenames
    for filename in &options.optional_filenames {
        // Use symlink_metadata to detect even broken symlinks
        if std::fs::symlink_metadata(filename).is_err() {
            debug!(path = %filename.display(), "optional file not found, skipping");
            continue;
        }
        collect_single_file(
            filename,
            &mut files,
            &mut deferred_symlinks,
            &mut seen_paths,
        )?;
    }

    // Process deferred symlinks based on policy
    process_symlinks(
        deferred_symlinks,
        options.symlink_policy,
        &seen_paths,
        &mut files,
        &mut dirs,
    )?;

    let mut manifest = Manifest::new(HashAlgorithm::Xxh128, chunk_size)
        .with_files(files)
        .with_dirs(dirs);
    manifest.recompute_total_size();
    debug!(
        files = manifest.files.len(),
        dirs = manifest.dirs.len(),
        total_size = manifest.total_size,
        "collect complete"
    );
    Ok(manifest)
}

fn collect_single_file(
    path: &Path,
    files: &mut Vec<FileEntry>,
    deferred: &mut Vec<DeferredSymlink>,
    seen: &mut HashSet<String>,
) -> crate::Result<()> {
    let sym_meta = std::fs::symlink_metadata(path)?;
    if sym_meta.file_type().is_symlink() {
        let norm = abs_normalized_no_follow(path)?;
        if seen.insert(norm.clone()) {
            deferred.push(DeferredSymlink {
                path: norm,
                fs_path: path.to_path_buf(),
            });
        }
    } else {
        let norm = abs_normalized(path)?;
        if seen.insert(norm.clone()) {
            let (size, mtime, runnable) = file_meta(&sym_meta)?;
            let mut entry = FileEntry::file(norm, size, mtime);
            entry.runnable = runnable;
            files.push(entry);
        }
    }
    Ok(())
}

fn process_symlinks(
    deferred: Vec<DeferredSymlink>,
    policy: SymlinkPolicy,
    collected_set: &HashSet<String>,
    files: &mut Vec<FileEntry>,
    dirs: &mut Vec<DirEntry>,
) -> crate::Result<()> {
    match policy {
        SymlinkPolicy::ExcludeAll => {
            debug!(count = deferred.len(), "excluding all symlinks per policy");
        }
        SymlinkPolicy::Preserve => {
            for sym in &deferred {
                let target = std::fs::read_link(&sym.fs_path)?;
                let abs_target = if target.is_absolute() {
                    normalize_path(&target.to_string_lossy())
                } else {
                    let parent = sym.fs_path.parent().unwrap_or(Path::new("/"));
                    normalize_path(&parent.join(&target).to_string_lossy())
                };
                files.push(FileEntry::symlink(&sym.path, abs_target));
            }
        }
        SymlinkPolicy::CollapseEscaping => {
            let mut visited = HashSet::new();
            for sym in &deferred {
                let target = std::fs::read_link(&sym.fs_path)?;
                let resolved = resolve_symlink_target(&sym.fs_path, &target)?;
                if is_escaping(&resolved, collected_set) {
                    // Collapse: replace symlink with target content
                    collapse_symlink(
                        &sym.path,
                        &sym.fs_path,
                        files,
                        dirs,
                        &mut visited,
                        None,
                        None,
                    )?;
                } else {
                    // Preserve as symlink with absolute target
                    files.push(FileEntry::symlink(&sym.path, &resolved));
                }
            }
        }
        SymlinkPolicy::CollapseAll => {
            let mut visited = HashSet::new();
            for sym in &deferred {
                collapse_symlink(
                    &sym.path,
                    &sym.fs_path,
                    files,
                    dirs,
                    &mut visited,
                    None,
                    None,
                )?;
            }
        }
        SymlinkPolicy::ExcludeEscaping => {
            for sym in &deferred {
                let target = std::fs::read_link(&sym.fs_path)?;
                let resolved = resolve_symlink_target(&sym.fs_path, &target)?;
                if !is_escaping(&resolved, collected_set) {
                    files.push(FileEntry::symlink(&sym.path, &resolved));
                }
            }
        }
        SymlinkPolicy::TransitiveIncludeTargets => {
            let mut queue: Vec<DeferredSymlink> = deferred;
            let mut all_seen: HashSet<String> = collected_set.clone();
            while let Some(sym) = queue.pop() {
                let target = std::fs::read_link(&sym.fs_path)?;
                let abs_target = resolve_symlink_target(&sym.fs_path, &target)?;
                files.push(FileEntry::symlink(&sym.path, &abs_target));

                // Add target to manifest if not already collected
                if all_seen.insert(abs_target.clone()) {
                    let target_path = PathBuf::from(&abs_target);
                    if let Ok(sym_meta) = std::fs::symlink_metadata(&target_path) {
                        if sym_meta.file_type().is_symlink() {
                            queue.push(DeferredSymlink {
                                path: abs_target,
                                fs_path: target_path,
                            });
                        } else if sym_meta.is_file() {
                            let (size, mtime, runnable) = file_meta(&sym_meta)?;
                            let mut entry = FileEntry::file(abs_target, size, mtime);
                            entry.runnable = runnable;
                            files.push(entry);
                        } else if sym_meta.is_dir() {
                            for entry in WalkDir::new(&target_path).follow_links(false) {
                                let entry = entry.map_err(|e| {
                                    SnapshotError::Io(std::io::Error::other(e.to_string()))
                                })?;
                                let ft = entry.file_type();
                                let ep = entry.path();
                                let norm = abs_normalized(ep)?;
                                if !all_seen.insert(norm.clone()) {
                                    continue;
                                }
                                if ft.is_symlink() {
                                    queue.push(DeferredSymlink {
                                        path: norm,
                                        fs_path: ep.to_path_buf(),
                                    });
                                } else if ft.is_file() {
                                    let meta = std::fs::metadata(ep)?;
                                    let (size, mtime, runnable) = file_meta(&meta)?;
                                    let mut entry = FileEntry::file(norm, size, mtime);
                                    entry.runnable = runnable;
                                    files.push(entry);
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    Ok(())
}

fn resolve_symlink_target(symlink_path: &Path, target: &Path) -> crate::Result<String> {
    let abs = if target.is_absolute() {
        target.to_path_buf()
    } else {
        let parent = symlink_path.parent().unwrap_or(Path::new("/"));
        parent.join(target)
    };
    Ok(normalize_path(&abs.to_string_lossy()))
}

fn is_escaping(resolved_target: &str, collected_set: &HashSet<String>) -> bool {
    if collected_set.contains(resolved_target) {
        return false;
    }
    for p in collected_set {
        if resolved_target.starts_with(&format!("{p}/")) {
            return false;
        }
    }
    true
}

fn collapse_symlink(
    symlink_manifest_path: &str,
    symlink_fs_path: &Path,
    files: &mut Vec<FileEntry>,
    dirs: &mut Vec<DirEntry>,
    visited: &mut HashSet<String>,
    collapse_root: Option<&Path>,
    manifest_root: Option<&str>,
) -> crate::Result<()> {
    // Cycle detection
    let norm = normalize_path(&symlink_fs_path.to_string_lossy());
    if !visited.insert(norm) {
        warn!(path = %symlink_manifest_path, "symlink cycle detected, skipping");
        return Ok(()); // cycle detected, skip
    }

    // Follow the symlink to get target metadata
    let target_meta = match std::fs::metadata(symlink_fs_path) {
        Ok(m) => m,
        Err(e) => {
            warn!(path = %symlink_manifest_path, error = %e, "broken symlink, skipping");
            return Ok(());
        }
    };

    if target_meta.is_file() {
        let (size, mtime, runnable) = file_meta(&target_meta)?;
        let mut entry = FileEntry::file(symlink_manifest_path.to_string(), size, mtime);
        entry.runnable = runnable;
        files.push(entry);
    } else if target_meta.is_dir() {
        // Walk the target directory, mapping paths under the symlink's manifest path
        let real_target = std::fs::canonicalize(symlink_fs_path)?;
        let root = collapse_root.unwrap_or(&real_target);
        let mroot = manifest_root.unwrap_or(symlink_manifest_path);

        for entry in WalkDir::new(&real_target).follow_links(false) {
            let entry =
                entry.map_err(|e| SnapshotError::Io(std::io::Error::other(e.to_string())))?;
            let ft = entry.file_type();
            let ep = entry.path();

            // Compute the relative path from the real target
            let rel = ep.strip_prefix(&real_target).unwrap_or(ep);
            if rel.as_os_str().is_empty() {
                continue; // skip the root dir itself
            }
            let mapped = format!(
                "{}/{}",
                symlink_manifest_path,
                normalize_path(&rel.to_string_lossy())
            );

            if ft.is_symlink() {
                // Resolve the nested symlink's target
                let link_target = match std::fs::read_link(ep) {
                    Ok(t) => t,
                    Err(_) => continue,
                };
                let abs_target = if link_target.is_absolute() {
                    link_target
                } else {
                    ep.parent().unwrap_or(Path::new("/")).join(&link_target)
                };
                let resolved = match abs_target.canonicalize() {
                    Ok(p) => p,
                    Err(_) => continue, // broken symlink
                };

                if let Ok(rel_to_root) = resolved.strip_prefix(root) {
                    // Internal symlink: preserve with translated target
                    let translated = format!(
                        "{}/{}",
                        mroot,
                        normalize_path(&rel_to_root.to_string_lossy())
                    );
                    files.push(FileEntry::symlink(&mapped, &translated));
                } else if resolved.is_dir() {
                    // External directory symlink: recursively collapse
                    collapse_symlink(&mapped, ep, files, dirs, visited, Some(root), Some(mroot))?;
                } else if resolved.is_file() {
                    // External file symlink: collapse to file
                    if let Ok(meta) = std::fs::metadata(ep) {
                        let (size, mtime, runnable) = file_meta(&meta)?;
                        let mut fe = FileEntry::file(mapped, size, mtime);
                        fe.runnable = runnable;
                        files.push(fe);
                    }
                }
            } else if ft.is_file() {
                let meta = std::fs::metadata(ep)?;
                let (size, mtime, runnable) = file_meta(&meta)?;
                let mut entry = FileEntry::file(mapped, size, mtime);
                entry.runnable = runnable;
                files.push(entry);
            } else if ft.is_dir() {
                dirs.push(DirEntry::new(mapped));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn setup_dir_with_files(dir: &Path) {
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::write(dir.join("a.txt"), "hello").unwrap();
        std::fs::write(dir.join("sub/b.txt"), "world").unwrap();
    }

    #[test]
    fn collect_directory_with_files() {
        let tmp = TempDir::new().unwrap();
        setup_dir_with_files(tmp.path());

        let result = collect_abs_snapshot(
            &[tmp.path().to_path_buf()],
            &[] as &[PathBuf],
            CollectOptions::default(),
        )
        .unwrap();

        assert_eq!(result.files.len(), 2);
        assert!(result.files.iter().any(|f| f.path.ends_with("a.txt")));
        assert!(result.files.iter().any(|f| f.path.ends_with("sub/b.txt")));
    }

    #[test]
    fn file_sizes_and_mtimes_captured() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("f.txt"), "12345").unwrap();

        let result = collect_abs_snapshot(
            &[tmp.path().to_path_buf()],
            &[] as &[PathBuf],
            CollectOptions::default(),
        )
        .unwrap();

        let f = &result.files[0];
        assert_eq!(f.size, Some(5));
        assert!(f.mtime.unwrap() > 0);
    }

    #[test]
    fn empty_directories_included() {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join("empty_dir")).unwrap();
        std::fs::write(tmp.path().join("f.txt"), "x").unwrap();

        let result = collect_abs_snapshot(
            &[tmp.path().to_path_buf()],
            &[] as &[PathBuf],
            CollectOptions::default(),
        )
        .unwrap();

        assert!(result.dirs.iter().any(|d| d.path.ends_with("empty_dir")));
    }

    #[test]
    fn all_directories_recorded() {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join("a/b")).unwrap();
        std::fs::write(tmp.path().join("a/b/file.txt"), "x").unwrap();

        let result = collect_abs_snapshot(
            &[tmp.path().to_path_buf()],
            &[] as &[PathBuf],
            CollectOptions::default(),
        )
        .unwrap();

        // Should have the root dir, a/, and a/b/ (3 dirs total)
        // Plus the file
        assert_eq!(result.files.len(), 1);
        assert!(
            result.dirs.len() >= 3,
            "expected at least 3 dirs, got {}: {:?}",
            result.dirs.len(),
            result.dirs.iter().map(|d| &d.path).collect::<Vec<_>>()
        );
        assert!(result.dirs.iter().any(|d| d.path.ends_with("/a")));
        assert!(result.dirs.iter().any(|d| d.path.ends_with("/a/b")));
    }

    #[test]
    fn required_filename_missing_errors() {
        let result = collect_abs_snapshot(
            &[] as &[PathBuf],
            &[PathBuf::from("/nonexistent/file.txt")],
            CollectOptions::default(),
        );
        assert!(result.is_err());
        match result.unwrap_err() {
            SnapshotError::FileNotFound(p) => assert!(p.contains("nonexistent")),
            e => panic!("expected FileNotFound, got: {e}"),
        }
    }

    #[test]
    fn optional_filename_missing_skipped() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("exists.txt"), "ok").unwrap();

        let result = collect_abs_snapshot(
            &[] as &[PathBuf],
            &[tmp.path().join("exists.txt")],
            CollectOptions {
                optional_filenames: vec![tmp.path().join("nope.txt")],
                ..Default::default()
            },
        )
        .unwrap();

        assert_eq!(result.files.len(), 1);
        assert!(result.files[0].path.ends_with("exists.txt"));
    }

    #[test]
    fn all_paths_absolute_and_normalized() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("f.txt"), "x").unwrap();

        let result = collect_abs_snapshot(
            &[tmp.path().to_path_buf()],
            &[] as &[PathBuf],
            CollectOptions::default(),
        )
        .unwrap();

        for f in &result.files {
            assert!(
                crate::path_util::is_absolute_path(&f.path),
                "not absolute: {}",
                f.path
            );
            assert!(!f.path.contains("/../"), "not normalized: {}", f.path);
        }
    }

    #[test]
    fn nested_directories_walked_recursively() {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join("a/b/c")).unwrap();
        std::fs::write(tmp.path().join("a/b/c/deep.txt"), "deep").unwrap();

        let result = collect_abs_snapshot(
            &[tmp.path().to_path_buf()],
            &[] as &[PathBuf],
            CollectOptions::default(),
        )
        .unwrap();

        assert_eq!(result.files.len(), 1);
        assert!(result.files[0].path.ends_with("a/b/c/deep.txt"));
    }

    #[cfg(unix)]
    #[test]
    fn symlinks_preserve_policy() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("target.txt"), "data").unwrap();
        std::os::unix::fs::symlink(tmp.path().join("target.txt"), tmp.path().join("link.txt"))
            .unwrap();

        let result = collect_abs_snapshot(
            &[tmp.path().to_path_buf()],
            &[] as &[PathBuf],
            CollectOptions {
                symlink_policy: SymlinkPolicy::Preserve,
                ..Default::default()
            },
        )
        .unwrap();

        let symlink_entry = result
            .files
            .iter()
            .find(|f| f.path.ends_with("link.txt"))
            .unwrap();
        assert!(symlink_entry.symlink_target.is_some());
    }

    #[cfg(unix)]
    #[test]
    fn symlinks_exclude_all_policy() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("target.txt"), "data").unwrap();
        std::os::unix::fs::symlink(tmp.path().join("target.txt"), tmp.path().join("link.txt"))
            .unwrap();

        let result = collect_abs_snapshot(
            &[tmp.path().to_path_buf()],
            &[] as &[PathBuf],
            CollectOptions {
                symlink_policy: SymlinkPolicy::ExcludeAll,
                ..Default::default()
            },
        )
        .unwrap();

        // Only the real file, no symlink
        assert_eq!(result.files.len(), 1);
        assert!(result.files[0].path.ends_with("target.txt"));
        assert!(result.files[0].symlink_target.is_none());
    }

    #[test]
    fn hashes_are_none() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("f.txt"), "x").unwrap();

        let result = collect_abs_snapshot(
            &[tmp.path().to_path_buf()],
            &[] as &[PathBuf],
            CollectOptions::default(),
        )
        .unwrap();

        for f in &result.files {
            assert!(f.hash.is_none());
        }
    }

    #[cfg(unix)]
    #[test]
    fn collapse_escaping_dir_preserves_internal_symlinks() {
        let tmp = TempDir::new().unwrap();

        // Create target directory outside the root
        let outside = tmp.path().join("outside");
        std::fs::create_dir_all(outside.join("sub")).unwrap();
        std::fs::write(outside.join("file.txt"), "content").unwrap();
        // Internal symlink: points within the same target directory
        std::os::unix::fs::symlink(outside.join("file.txt"), outside.join("sub/internal_link"))
            .unwrap();

        // Create root with escaping dir symlink
        let root = tmp.path().join("root");
        std::fs::create_dir_all(&root).unwrap();
        std::os::unix::fs::symlink(&outside, root.join("link_dir")).unwrap();

        let m = collect_abs_snapshot(
            std::slice::from_ref(&root),
            &[] as &[PathBuf],
            CollectOptions {
                symlink_policy: SymlinkPolicy::CollapseEscaping,
                ..Default::default()
            },
        )
        .unwrap();

        // The internal symlink should be preserved with a translated target
        let internal = m
            .files
            .iter()
            .find(|f| f.path.ends_with("sub/internal_link"))
            .unwrap();
        assert!(
            internal.symlink_target.is_some(),
            "internal symlink should be preserved"
        );
        let target = internal.symlink_target.as_ref().unwrap();
        assert!(
            target.ends_with("link_dir/file.txt"),
            "target should be translated, got: {}",
            target
        );

        // The regular file should be collapsed normally
        let file = m
            .files
            .iter()
            .find(|f| f.path.ends_with("link_dir/file.txt"))
            .unwrap();
        assert!(file.symlink_target.is_none());
        assert_eq!(file.size, Some(7));
    }

    #[cfg(unix)]
    #[test]
    fn collapse_escaping_dir_collapses_external_symlinks() {
        let tmp = TempDir::new().unwrap();

        // Create an external file that a nested symlink will point to
        let far_away = tmp.path().join("far_away.txt");
        std::fs::write(&far_away, "far").unwrap();

        // Create target directory outside the root
        let outside = tmp.path().join("outside");
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(outside.join("file.txt"), "content").unwrap();
        // External symlink: points outside the target directory
        std::os::unix::fs::symlink(&far_away, outside.join("external_link")).unwrap();

        // Create root with escaping dir symlink
        let root = tmp.path().join("root");
        std::fs::create_dir_all(&root).unwrap();
        std::os::unix::fs::symlink(&outside, root.join("link_dir")).unwrap();

        let m = collect_abs_snapshot(
            std::slice::from_ref(&root),
            &[] as &[PathBuf],
            CollectOptions {
                symlink_policy: SymlinkPolicy::CollapseEscaping,
                ..Default::default()
            },
        )
        .unwrap();

        // The external symlink should be collapsed to a regular file
        let external = m
            .files
            .iter()
            .find(|f| f.path.ends_with("external_link"))
            .unwrap();
        assert!(
            external.symlink_target.is_none(),
            "external symlink should be collapsed"
        );
        assert_eq!(external.size, Some(3)); // "far"
    }
}
