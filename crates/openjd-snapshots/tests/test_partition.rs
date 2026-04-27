// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

// Ported from deadline-cloud test_partition_manifest.py (~35 tests)

use std::collections::HashSet;

use openjd_snapshots::{
    partition_manifest, AbsSnapshot, DirEntry, FileEntry, HashAlgorithm, Manifest,
    PartitionOptions, Snapshot, SymlinkPolicy, DEFAULT_FILE_CHUNK_SIZE,
};

// --- Helpers ---

fn abs(files: Vec<FileEntry>, dirs: Vec<DirEntry>) -> AbsSnapshot {
    Manifest::new(HashAlgorithm::Xxh128, DEFAULT_FILE_CHUNK_SIZE)
        .with_files(files)
        .with_dirs(dirs)
}

fn hf(path: &str, hash: &str, size: u64, mtime: u64) -> FileEntry {
    let mut f = FileEntry::file(path, size, mtime);
    f.hash = Some(hash.into());
    f
}

fn roots(result: &[(String, Snapshot)]) -> Vec<&str> {
    result.iter().map(|(r, _)| r.as_str()).collect()
}

fn file_paths(m: &Snapshot) -> HashSet<String> {
    m.files.iter().map(|f| f.path.clone()).collect()
}

fn dir_paths(m: &Snapshot) -> HashSet<String> {
    m.dirs.iter().map(|d| d.path.clone()).collect()
}

fn opts_with_roots(r: &[&str]) -> PartitionOptions {
    PartitionOptions {
        roots: Some(r.iter().map(|s| s.to_string()).collect()),
        ..Default::default()
    }
}

// ===== Auto-root determination =====

#[test]
fn auto_root_single_directory() {
    // All files under same dir -> that dir is root
    let m = abs(
        vec![
            hf("/projects/scene/assets/model.blend", "h1", 100, 1000),
            hf("/projects/scene/assets/texture.png", "h2", 200, 2000),
            hf("/projects/scene/render/output.exr", "h3", 300, 3000),
        ],
        vec![],
    );
    let result = partition_manifest(&m, &PartitionOptions::default()).unwrap();
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].0, "/projects/scene");
}

#[test]
fn auto_root_common_prefix() {
    let m = abs(
        vec![
            hf("/root/a/file1.txt", "h1", 10, 1),
            hf("/root/b/file2.txt", "h2", 20, 2),
        ],
        vec![],
    );
    let result = partition_manifest(&m, &PartitionOptions::default()).unwrap();
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].0, "/root");
    assert_eq!(result[0].1.files.len(), 2);
}

#[test]
fn auto_root_single_file_uses_parent() {
    let m = abs(vec![hf("/a/b/file.txt", "h1", 10, 1)], vec![]);
    let result = partition_manifest(&m, &PartitionOptions::default()).unwrap();
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].0, "/a/b");
}

// ===== Explicit roots =====

#[test]
fn explicit_roots_partition_correctly() {
    let m = abs(
        vec![
            hf("/assets/textures/wood.png", "h1", 100, 1000),
            hf("/assets/models/chair.blend", "h2", 200, 2000),
        ],
        vec![],
    );
    let opts = opts_with_roots(&["/assets/textures", "/assets/models"]);
    let result = partition_manifest(&m, &opts).unwrap();
    assert_eq!(result.len(), 2);
    assert_eq!(roots(&result), vec!["/assets/textures", "/assets/models"]);

    assert_eq!(result[0].1.files.len(), 1);
    assert_eq!(result[0].1.files[0].path, "wood.png");
    assert_eq!(result[1].1.files.len(), 1);
    assert_eq!(result[1].1.files[0].path, "chair.blend");
}

#[test]
fn empty_partition_for_explicit_root_with_no_entries() {
    let m = abs(
        vec![hf("/assets/textures/wood.png", "h1", 100, 1000)],
        vec![],
    );
    let opts = opts_with_roots(&["/assets/textures", "/assets/models"]);
    let result = partition_manifest(&m, &opts).unwrap();
    assert_eq!(result.len(), 2);
    assert_eq!(roots(&result), vec!["/assets/textures", "/assets/models"]);
    assert!(result[1].1.files.is_empty());
}

#[test]
fn explicit_root_covers_all_no_remainder() {
    let m = abs(
        vec![
            hf("/project/src/main.py", "h1", 100, 1000),
            hf("/project/src/utils.py", "h2", 200, 2000),
            hf("/project/tests/test.py", "h3", 300, 3000),
        ],
        vec![],
    );
    let opts = opts_with_roots(&["/project"]);
    let result = partition_manifest(&m, &opts).unwrap();
    assert_eq!(roots(&result), vec!["/project"]);
}

#[test]
fn explicit_roots_first_in_order() {
    let m = abs(
        vec![
            hf("/z/file.txt", "h1", 100, 1000),
            hf("/a/file.txt", "h2", 100, 1000),
            hf("/m/file.txt", "h3", 100, 1000),
        ],
        vec![],
    );
    let opts = opts_with_roots(&["/z", "/a", "/m"]);
    let result = partition_manifest(&m, &opts).unwrap();
    assert_eq!(roots(&result), vec!["/z", "/a", "/m"]);
}

// ===== Explicit roots with remainder =====

#[test]
fn explicit_roots_with_remainder_creates_additional_roots() {
    let m = abs(
        vec![
            hf("/projects/scene/model.blend", "h1", 100, 1000),
            hf("/data/shared/texture.png", "h2", 200, 2000),
            hf("/home/user/cache/temp.bin", "h3", 300, 3000),
        ],
        vec![],
    );
    let opts = opts_with_roots(&["/projects/scene"]);
    let result = partition_manifest(&m, &opts).unwrap();
    let r = roots(&result);
    assert_eq!(r[0], "/projects/scene");
    let mut remaining: Vec<&str> = r[1..].to_vec();
    remaining.sort();
    assert_eq!(remaining, vec!["/data/shared", "/home/user/cache"]);
}

#[test]
fn many_remainder_paths_same_toplevel_find_deepest_common() {
    let m = abs(
        vec![
            hf("/projects/scene/model.blend", "h1", 100, 1000),
            hf("/data/assets/textures/wood.png", "h2", 200, 2000),
            hf("/data/assets/textures/metal.png", "h3", 300, 3000),
            hf("/data/assets/models/chair.obj", "h4", 400, 4000),
            hf("/data/assets/models/table.obj", "h5", 500, 5000),
        ],
        vec![],
    );
    let opts = opts_with_roots(&["/projects/scene"]);
    let result = partition_manifest(&m, &opts).unwrap();
    let r = roots(&result);
    assert_eq!(r[0], "/projects/scene");
    assert!(r.contains(&"/data/assets"));
}

#[test]
fn remainder_with_nested_explicit_root() {
    let m = abs(
        vec![
            hf(
                "/projects/client/job/scene/assets/model.blend",
                "h1",
                100,
                1000,
            ),
            hf("/shared/lib/utils.py", "h2", 200, 2000),
            hf("/tmp/cache/data.bin", "h3", 300, 3000),
        ],
        vec![],
    );
    let opts = opts_with_roots(&["/projects/client/job/scene/assets"]);
    let result = partition_manifest(&m, &opts).unwrap();
    let r = roots(&result);
    assert_eq!(r[0], "/projects/client/job/scene/assets");
    let mut remaining: Vec<&str> = r[1..].to_vec();
    remaining.sort();
    assert_eq!(remaining, vec!["/shared/lib", "/tmp/cache"]);
}

#[test]
fn multiple_explicit_roots_with_remainder() {
    let m = abs(
        vec![
            hf("/project/src/main.py", "h1", 100, 1000),
            hf("/project/tests/test_main.py", "h2", 200, 2000),
            hf("/libs/utils/helpers.py", "h3", 300, 3000),
        ],
        vec![],
    );
    let opts = opts_with_roots(&["/project/src", "/project/tests"]);
    let result = partition_manifest(&m, &opts).unwrap();
    let r = roots(&result);
    assert_eq!(r[0], "/project/src");
    assert_eq!(r[1], "/project/tests");
    assert!(r.contains(&"/libs/utils"));
}

#[test]
fn single_file_remainder_gets_parent_as_root() {
    let m = abs(
        vec![
            hf("/projects/scene/model.blend", "h1", 100, 1000),
            hf("/etc/config.ini", "h2", 200, 2000),
        ],
        vec![],
    );
    let opts = opts_with_roots(&["/projects/scene"]);
    let result = partition_manifest(&m, &opts).unwrap();
    let r = roots(&result);
    assert_eq!(r[0], "/projects/scene");
    assert!(r.contains(&"/etc"));
}

#[test]
fn remainder_with_common_prefix_at_different_depths() {
    let m = abs(
        vec![
            hf("/project/main.py", "h1", 100, 1000),
            hf("/libs/a/b/c/deep.py", "h2", 200, 2000),
            hf("/libs/a/b/c/deeper.py", "h3", 300, 3000),
            hf("/libs/a/shallow.py", "h4", 400, 4000),
        ],
        vec![],
    );
    let opts = opts_with_roots(&["/project"]);
    let result = partition_manifest(&m, &opts).unwrap();
    let r = roots(&result);
    assert_eq!(r[0], "/project");
    assert!(r.contains(&"/libs/a"));
}

// ===== referenced_paths =====

#[test]
fn referenced_paths_affects_root_determination() {
    let m = abs(vec![hf("/project/src/main.py", "h1", 100, 1000)], vec![]);

    // Without referenced_paths, root is /project/src
    let result_without = partition_manifest(&m, &PartitionOptions::default()).unwrap();
    assert_eq!(result_without[0].0, "/project/src");

    // With referenced_paths at project level, root should be /project
    let opts = PartitionOptions {
        referenced_paths: Some(vec!["/project/output".into()]),
        ..Default::default()
    };
    let result_with = partition_manifest(&m, &opts).unwrap();
    assert_eq!(result_with[0].0, "/project");
}

#[test]
fn referenced_paths_creates_additional_roots() {
    let m = abs(vec![hf("/project/src/main.py", "h1", 100, 1000)], vec![]);
    let opts = PartitionOptions {
        roots: Some(vec!["/project".into()]),
        referenced_paths: Some(vec!["/other/output".into()]),
        ..Default::default()
    };
    let result = partition_manifest(&m, &opts).unwrap();
    let r = roots(&result);
    assert!(r.contains(&"/project"));
    assert!(r.iter().any(|root| root.contains("other")));
}

#[test]
fn referenced_paths_only_introduces_remainder() {
    let m = abs(vec![hf("/project/src/main.py", "h1", 100, 1000)], vec![]);
    let opts = PartitionOptions {
        roots: Some(vec!["/project".into()]),
        referenced_paths: Some(vec!["/output/renders/final".into()]),
        ..Default::default()
    };
    let result = partition_manifest(&m, &opts).unwrap();
    let r = roots(&result);
    assert!(r.contains(&"/project"));
    assert!(r.contains(&"/output/renders/final"));
}

#[test]
fn referenced_paths_deepens_remainder_root() {
    let m = abs(
        vec![
            hf("/project/src/main.py", "h1", 100, 1000),
            hf("/data/assets/texture.png", "h2", 200, 2000),
        ],
        vec![],
    );

    // Without referenced_paths, remainder root is /data/assets
    let opts_without = opts_with_roots(&["/project"]);
    let result_without = partition_manifest(&m, &opts_without).unwrap();
    assert!(roots(&result_without).contains(&"/data/assets"));

    // With referenced_paths at /data level, remainder root should be /data
    let opts_with = PartitionOptions {
        roots: Some(vec!["/project".into()]),
        referenced_paths: Some(vec!["/data/cache".into()]),
        ..Default::default()
    };
    let result_with = partition_manifest(&m, &opts_with).unwrap();
    assert!(roots(&result_with).contains(&"/data"));
}

// ===== Directory entries =====

#[test]
fn empty_dir_affects_auto_root() {
    // Files only under /a/b, but empty dir at /a/c means root should be /a
    let m = abs(
        vec![hf("/a/b/file.txt", "h1", 100, 1000)],
        vec![DirEntry::new("/a/c")],
    );
    let result = partition_manifest(&m, &PartitionOptions::default()).unwrap();
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].0, "/a");
    assert_eq!(result[0].1.files.len(), 1);
    assert_eq!(result[0].1.files[0].path, "b/file.txt");
    assert!(dir_paths(&result[0].1).contains("c"));
}

#[test]
fn preserves_directories_in_partitions() {
    let m = abs(
        vec![hf("/project/src/main.py", "h1", 100, 1000)],
        vec![
            DirEntry::new("/project/src"),
            DirEntry::new("/project/empty"),
        ],
    );
    let opts = opts_with_roots(&["/project"]);
    let result = partition_manifest(&m, &opts).unwrap();
    assert_eq!(result.len(), 1);
    let dp = dir_paths(&result[0].1);
    assert!(dp.contains("src"));
    assert!(dp.contains("empty"));
}

#[test]
fn single_partition_with_dirs() {
    let m = abs(
        vec![
            hf("/project/src/main.py", "h1", 100, 1000),
            hf("/project/src/utils.py", "h2", 200, 2000),
            hf("/project/tests/test_main.py", "h3", 150, 3000),
        ],
        vec![
            DirEntry::new("/project"),
            DirEntry::new("/project/src"),
            DirEntry::new("/project/tests"),
        ],
    );
    let result = partition_manifest(&m, &PartitionOptions::default()).unwrap();
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].0, "/project");
    assert_eq!(result[0].1.files.len(), 3);
}

// ===== Validation =====

#[test]
fn overlapping_roots_raises_error() {
    let m = abs(vec![hf("/a/b/c/file.txt", "h1", 100, 1000)], vec![]);
    let opts = opts_with_roots(&["/a/b", "/a/b/c"]);
    assert!(partition_manifest(&m, &opts).is_err());
}

#[test]
fn preserve_policy_raises_error() {
    let m = abs(vec![hf("/a/b/file.txt", "h1", 100, 1000)], vec![]);
    let opts = PartitionOptions {
        roots: Some(vec!["/a/b".into()]),
        symlink_policy: SymlinkPolicy::Preserve,
        ..Default::default()
    };
    assert!(partition_manifest(&m, &opts).is_err());
}

#[test]
fn transitive_include_targets_raises_error() {
    let m = abs(vec![hf("/a/b/file.txt", "h1", 100, 1000)], vec![]);
    let opts = PartitionOptions {
        roots: Some(vec!["/a/b".into()]),
        symlink_policy: SymlinkPolicy::TransitiveIncludeTargets,
        ..Default::default()
    };
    assert!(partition_manifest(&m, &opts).is_err());
}

// ===== Symlink handling =====

#[test]
fn preserves_symlinks_within_partition() {
    let m = abs(
        vec![
            hf("/project/src/main.py", "h1", 100, 1000),
            FileEntry::symlink("/project/src/link", "/project/src/main.py"),
        ],
        vec![DirEntry::new("/project"), DirEntry::new("/project/src")],
    );
    let opts = opts_with_roots(&["/project"]);
    let result = partition_manifest(&m, &opts).unwrap();
    assert_eq!(result.len(), 1);
    let link = result[0]
        .1
        .files
        .iter()
        .find(|f| f.path == "src/link")
        .unwrap();
    assert_eq!(link.symlink_target.as_deref(), Some("src/main.py"));
}

#[test]
fn escaping_symlink_collapsed() {
    let m = abs(
        vec![
            hf("/project/src/main.py", "h1", 100, 1000),
            FileEntry::symlink("/project/src/link", "/shared/lib.py"),
            hf("/shared/lib.py", "h2", 200, 2000),
        ],
        vec![
            DirEntry::new("/project"),
            DirEntry::new("/project/src"),
            DirEntry::new("/shared"),
        ],
    );
    let opts = PartitionOptions {
        roots: Some(vec!["/project".into(), "/shared".into()]),
        symlink_policy: SymlinkPolicy::CollapseEscaping,
        ..Default::default()
    };
    let result = partition_manifest(&m, &opts).unwrap();
    let project = result.iter().find(|(r, _)| r == "/project").unwrap();
    let link = project
        .1
        .files
        .iter()
        .find(|f| f.path == "src/link")
        .unwrap();
    assert!(link.symlink_target.is_none());
    assert_eq!(link.hash.as_deref(), Some("h2"));
}

#[test]
fn escaping_symlink_excluded_with_exclude_all() {
    let m = abs(
        vec![
            hf("/project/src/main.py", "h1", 100, 1000),
            FileEntry::symlink("/project/src/link", "/shared/lib.py"),
        ],
        vec![DirEntry::new("/project"), DirEntry::new("/project/src")],
    );
    let opts = PartitionOptions {
        roots: Some(vec!["/project".into()]),
        symlink_policy: SymlinkPolicy::ExcludeAll,
        ..Default::default()
    };
    let result = partition_manifest(&m, &opts).unwrap();
    let fp = file_paths(&result[0].1);
    assert!(!fp.contains("src/link"));
    assert!(fp.contains("src/main.py"));
}

#[test]
fn escaping_symlink_excluded_with_exclude_escaping() {
    let m = abs(
        vec![
            hf("/project/src/main.py", "h1", 100, 1000),
            FileEntry::symlink("/project/src/link", "/shared/lib.py"),
            hf("/shared/lib.py", "h2", 200, 2000),
        ],
        vec![
            DirEntry::new("/project"),
            DirEntry::new("/project/src"),
            DirEntry::new("/shared"),
        ],
    );
    let opts = PartitionOptions {
        roots: Some(vec!["/project".into(), "/shared".into()]),
        symlink_policy: SymlinkPolicy::ExcludeEscaping,
        ..Default::default()
    };
    let result = partition_manifest(&m, &opts).unwrap();
    let project = result.iter().find(|(r, _)| r == "/project").unwrap();
    let fp = file_paths(&project.1);
    assert!(!fp.contains("src/link"));
    assert!(fp.contains("src/main.py"));
}

#[test]
fn non_escaping_symlink_preserved_with_exclude_escaping() {
    let m = abs(
        vec![
            hf("/project/src/main.py", "h1", 100, 1000),
            FileEntry::symlink("/project/src/link", "/project/src/main.py"),
        ],
        vec![DirEntry::new("/project"), DirEntry::new("/project/src")],
    );
    let opts = PartitionOptions {
        roots: Some(vec!["/project".into()]),
        symlink_policy: SymlinkPolicy::ExcludeEscaping,
        ..Default::default()
    };
    let result = partition_manifest(&m, &opts).unwrap();
    let link = result[0]
        .1
        .files
        .iter()
        .find(|f| f.path == "src/link")
        .unwrap();
    assert_eq!(link.symlink_target.as_deref(), Some("src/main.py"));
}

#[test]
fn exclude_escaping_vs_collapse_escaping() {
    let m = abs(
        vec![
            hf("/project/src/main.py", "h1", 100, 1000),
            FileEntry::symlink("/project/src/link", "/shared/lib.py"),
            hf("/shared/lib.py", "h2", 200, 2000),
        ],
        vec![
            DirEntry::new("/project"),
            DirEntry::new("/project/src"),
            DirEntry::new("/shared"),
        ],
    );

    // EXCLUDE_ESCAPING: symlink excluded
    let opts_exclude = PartitionOptions {
        roots: Some(vec!["/project".into(), "/shared".into()]),
        symlink_policy: SymlinkPolicy::ExcludeEscaping,
        ..Default::default()
    };
    let result_exclude = partition_manifest(&m, &opts_exclude).unwrap();
    let project_exclude = result_exclude
        .iter()
        .find(|(r, _)| r == "/project")
        .unwrap();
    assert!(!file_paths(&project_exclude.1).contains("src/link"));

    // COLLAPSE_ESCAPING: symlink collapsed to real file
    let opts_collapse = PartitionOptions {
        roots: Some(vec!["/project".into(), "/shared".into()]),
        symlink_policy: SymlinkPolicy::CollapseEscaping,
        ..Default::default()
    };
    let result_collapse = partition_manifest(&m, &opts_collapse).unwrap();
    let project_collapse = result_collapse
        .iter()
        .find(|(r, _)| r == "/project")
        .unwrap();
    let link = project_collapse
        .1
        .files
        .iter()
        .find(|f| f.path == "src/link")
        .unwrap();
    assert!(link.symlink_target.is_none());
    assert_eq!(link.hash.as_deref(), Some("h2"));
}

// ===== Ordering =====

#[test]
fn auto_roots_sorted_alphabetically() {
    let m = abs(
        vec![
            hf("/zebra/file.txt", "h1", 100, 1000),
            hf("/apple/file.txt", "h2", 100, 1000),
            hf("/mango/file.txt", "h3", 100, 1000),
        ],
        vec![],
    );
    let opts = opts_with_roots(&["/zebra"]);
    let result = partition_manifest(&m, &opts).unwrap();
    let r = roots(&result);
    assert_eq!(r[0], "/zebra"); // explicit first
    let remaining: Vec<&str> = r[1..].to_vec();
    let mut sorted = remaining.clone();
    sorted.sort();
    assert_eq!(remaining, sorted);
}

// ===== Multiple auto-determined roots =====

#[test]
fn auto_root_no_common_prefix_gives_slash() {
    let m = abs(
        vec![
            hf("/a/file1.txt", "h1", 10, 1),
            hf("/b/file2.txt", "h2", 20, 2),
        ],
        vec![],
    );
    let result = partition_manifest(&m, &PartitionOptions::default()).unwrap();
    // Common prefix of /a and /b is /
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].0, "/");
}

#[test]
fn explicit_root_is_subpath_of_potential_remainder() {
    let m = abs(
        vec![
            hf("/data/project/scene/model.blend", "h1", 100, 1000),
            hf("/data/shared/texture.png", "h2", 200, 2000),
            hf("/home/user/cache.bin", "h3", 300, 3000),
        ],
        vec![],
    );
    let opts = opts_with_roots(&["/data/project/scene"]);
    let result = partition_manifest(&m, &opts).unwrap();
    let r = roots(&result);
    assert_eq!(r[0], "/data/project/scene");
    let mut remaining: Vec<&str> = r[1..].to_vec();
    remaining.sort();
    assert_eq!(remaining, vec!["/data/shared", "/home/user"]);
}

// ===== Edge cases =====

#[test]
fn empty_manifest_with_explicit_roots() {
    let m = abs(vec![], vec![]);
    let opts = opts_with_roots(&["/a", "/b"]);
    let result = partition_manifest(&m, &opts).unwrap();
    assert_eq!(result.len(), 2);
    assert!(result[0].1.files.is_empty());
    assert!(result[1].1.files.is_empty());
}

#[test]
fn empty_manifest_no_roots() {
    let m = abs(vec![], vec![]);
    let result = partition_manifest(&m, &PartitionOptions::default()).unwrap();
    assert!(result.is_empty());
}

#[test]
fn total_size_recomputed_per_partition() {
    let m = abs(
        vec![
            hf("/a/file1.txt", "h1", 100, 1),
            hf("/b/file2.txt", "h2", 200, 2),
        ],
        vec![],
    );
    let opts = opts_with_roots(&["/a", "/b"]);
    let result = partition_manifest(&m, &opts).unwrap();
    assert_eq!(result[0].1.total_size, 100);
    assert_eq!(result[1].1.total_size, 200);
}

// ===== Relative manifest support =====

fn rel(files: Vec<FileEntry>, dirs: Vec<DirEntry>) -> openjd_snapshots::Snapshot {
    Manifest::new(HashAlgorithm::Xxh128, DEFAULT_FILE_CHUNK_SIZE)
        .with_files(files)
        .with_dirs(dirs)
}

#[test]
fn root_level_files_returns_dot_root() {
    let m = rel(
        vec![
            hf("file1.txt", "h1", 100, 1000),
            hf("file2.txt", "h2", 200, 2000),
        ],
        vec![],
    );
    let result =
        openjd_snapshots::partition_rel_manifest(&m, &PartitionOptions::default()).unwrap();
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].0, ".");
    assert_eq!(result[0].1.files.len(), 2);
}

#[test]
fn relative_root_with_absolute_manifest_raises_error() {
    let m = abs(vec![hf("/home/user/file.txt", "h1", 100, 1000)], vec![]);
    let opts = opts_with_roots(&["subdir"]);
    assert!(partition_manifest(&m, &opts).is_err());
}

#[test]
fn absolute_root_with_relative_manifest_raises_error() {
    let m = rel(vec![hf("assets/file.txt", "h1", 100, 1000)], vec![]);
    let opts = opts_with_roots(&["/root"]);
    assert!(openjd_snapshots::partition_rel_manifest(&m, &opts).is_err());
}

#[test]
fn referenced_paths_style_mismatch_raises_error() {
    let m = rel(vec![hf("assets/file.txt", "h1", 100, 1000)], vec![]);
    let opts = PartitionOptions {
        referenced_paths: Some(vec!["/absolute/output".into()]),
        ..Default::default()
    };
    assert!(openjd_snapshots::partition_rel_manifest(&m, &opts).is_err());
}

#[test]
fn relative_explicit_root_with_remainder() {
    let m = rel(
        vec![
            hf("project/src/main.py", "h1", 100, 1000),
            hf("libs/common/utils.py", "h2", 200, 2000),
            hf("libs/common/helpers.py", "h3", 300, 3000),
            hf("data/cache/temp.bin", "h4", 400, 4000),
        ],
        vec![],
    );
    let opts = opts_with_roots(&["project/src"]);
    let result = openjd_snapshots::partition_rel_manifest(&m, &opts).unwrap();
    let r = roots(&result);
    assert_eq!(r[0], "project/src");
    let mut remaining: Vec<&str> = r[1..].to_vec();
    remaining.sort();
    assert_eq!(remaining, vec!["data/cache", "libs/common"]);
}
