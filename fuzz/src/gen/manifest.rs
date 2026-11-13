// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Snapshot manifest generator.
//!
//! Paths are drawn from a tiny component alphabet so two generated manifests
//! overlap heavily — the interesting cases for diff/compose/subtree/partition
//! are shared prefixes, a file in one manifest that is a directory in the
//! other, symlinks pointing in and out of a subtree, and deletions.

use super::{flip, one_in, pick, upto};
use arbitrary::Unstructured;
use openjd_snapshots::{DirEntry, FileEntry, HashAlgorithm, Manifest};

const COMPONENTS: &[&str] = &["a", "b", "c", "d", "日", "x y", "a.b", ".z"];

/// A relative path of 1–4 components.
pub fn rel_path(u: &mut Unstructured<'_>) -> String {
    let n = upto(u, 3) + 1;
    (0..n)
        .map(|_| *pick(u, COMPONENTS))
        .collect::<Vec<_>>()
        .join("/")
}

/// The absolute root manifests are generated under.
pub const ABS_ROOT: &str = "/r";

/// An absolute path under [`ABS_ROOT`] (occasionally a sibling of it).
pub fn abs_path(u: &mut Unstructured<'_>) -> String {
    if one_in(u, 10) {
        format!("/s/{}", rel_path(u))
    } else {
        format!("{ABS_ROOT}/{}", rel_path(u))
    }
}

fn hash(u: &mut Unstructured<'_>) -> String {
    format!("{:032x}", u.int_in_range(0u128..=7).unwrap_or(0))
}

/// File and directory entries. `abs` selects absolute paths; `allow_deleted`
/// permits deletion markers (diff manifests).
pub fn entries(
    u: &mut Unstructured<'_>,
    abs: bool,
    allow_deleted: bool,
) -> (Vec<FileEntry>, Vec<DirEntry>) {
    let mk = |u: &mut Unstructured<'_>| if abs { abs_path(u) } else { rel_path(u) };
    let mut files = Vec::new();
    for _ in 0..upto(u, 6) {
        let path = mk(u);
        let f = if allow_deleted && one_in(u, 4) {
            FileEntry::deleted(path)
        } else if one_in(u, 6) {
            FileEntry::symlink(path, mk(u))
        } else {
            let size = *pick(u, &[0u64, 1, 10, 1 << 20, u64::MAX / 4]);
            let mut f = FileEntry::file(path, size, u.int_in_range(0u64..=3).unwrap_or(0));
            f.hash = Some(hash(u));
            f.runnable = flip(u);
            f
        };
        files.push(f);
    }
    let mut dirs = Vec::new();
    for _ in 0..upto(u, 2) {
        let path = mk(u);
        dirs.push(if allow_deleted && one_in(u, 3) {
            DirEntry::deleted(path)
        } else {
            DirEntry::new(path)
        });
    }
    (files, dirs)
}

/// A manifest with generated entries.
pub fn manifest<P, K>(u: &mut Unstructured<'_>, abs: bool, allow_deleted: bool) -> Manifest<P, K> {
    let (files, dirs) = entries(u, abs, allow_deleted);
    let chunk = *pick(u, &[-1i64, 256 * 1024 * 1024, 1, 7]);
    Manifest::new(HashAlgorithm::Xxh128, chunk)
        .with_files(files)
        .with_dirs(dirs)
}
