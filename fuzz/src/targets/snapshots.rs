// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! openjd-snapshots targets: manifest codecs and manifest algebra.
//!
//! - **Codec round trip** (90a7bce, report §3.2): every manifest that
//!   decodes re-encodes deterministically and decodes back to the same
//!   entries, chunk size, total size, and parent hash.
//! - **Diff/compose algebra**: `compose(a, [diff(a, b)])` reproduces `b`'s
//!   files; composing diffs one at a time equals composing their
//!   `compose_diffs`; `diff(a, a)` is empty.
//! - **Subtree/join/partition/filter**: results validate, stay within the
//!   input, and partitions cover every file.

use crate::gen::manifest::{manifest, ABS_ROOT};
use crate::gen::{one_in, pick, upto};
use crate::targets::text_and_control;
use crate::{known_issue, oracle};
use arbitrary::Unstructured;
use openjd_snapshots::{
    compose_diffs, compose_snapshot_with_diffs, decode_manifest, diff_snapshots,
    encode_abs_snapshot_diff_v2025, encode_abs_snapshot_v2025, encode_snapshot_diff_v2023,
    encode_snapshot_diff_v2025, encode_snapshot_v2023, encode_snapshot_v2025, entries_differ,
    filter_manifest, join_snapshot, partition_manifest, subtree_snapshot, AbsSnapshot,
    AbsSnapshotDiff, DecodedManifest, DiffOptions, FileEntry, IncludeExcludePathsFilter, Manifest,
    PartitionOptions, SymlinkPolicy,
};
use std::collections::BTreeMap;

/// Same files and metadata. Directory entries may differ: v2025 makes the
/// implicit parent directories of a v2023 manifest explicit.
fn same<P, K>(a: &Manifest<P, K>, b: &Manifest<P, K>) -> bool {
    // Encoders emit entries in canonical order, so compare as sets.
    let sorted = |m: &Manifest<P, K>| {
        let mut v = m.files.clone();
        v.sort_by(|x, y| x.path.cmp(&y.path));
        v
    };
    // A decoded manifest's stated total may disagree with its files (decode
    // accepts it); encoders write the computed total, so compare totals only
    // when `b` (the original) is self-consistent.
    let computed = b
        .files
        .iter()
        .filter(|f| !f.deleted)
        .try_fold(0u64, |acc, f| acc.checked_add(f.size.unwrap_or(0)));
    sorted(a) == sorted(b)
        && (computed != Some(b.total_size) || a.total_size == b.total_size)
        && a.parent_manifest_hash == b.parent_manifest_hash
        && a.file_chunk_size_bytes == b.file_chunk_size_bytes
        && a.hash_alg == b.hash_alg
}

fn roundtrip<P, K>(
    m: &Manifest<P, K>,
    encode: impl Fn(&Manifest<P, K>) -> openjd_snapshots::Result<String>,
    unwrap: impl Fn(DecodedManifest) -> Option<Manifest<P, K>>,
    what: &str,
) where
    P: Clone + openjd_snapshots::manifest::ValidatePaths,
    K: Clone + openjd_snapshots::manifest::ValidateKind,
{
    // Only valid manifests are promised to round-trip.
    if m.validate().is_err() {
        return;
    }
    let text = match encode(m) {
        Ok(t) => t,
        Err(e) => {
            // v2023 has no chunking: it requires fileChunkSizeBytes = -1.
            let unrepresentable = what.contains("v2023") && m.file_chunk_size_bytes != -1;
            oracle!(
                unrepresentable,
                "{what}: a valid manifest fails to encode: {e}"
            );
            return;
        }
    };
    let again = encode(m).ok();
    oracle!(
        again.as_deref() == Some(text.as_str()),
        "{what}: encoding is nondeterministic"
    );
    match decode_manifest(&text).map(&unwrap) {
        Ok(Some(back)) => {
            {
                let first_diff = m.files.iter().zip(&back.files).find(|(x, y)| x != y);
                oracle!(
                    same(&back, m),
                    "{what}: round trip changed the manifest: first differing file {first_diff:?}; \
                     total {} -> {}, chunk {} -> {}, parent {:?} -> {:?}, files {} -> {}",
                    m.total_size,
                    back.total_size,
                    m.file_chunk_size_bytes,
                    back.file_chunk_size_bytes,
                    m.parent_manifest_hash,
                    back.parent_manifest_hash,
                    m.files.len(),
                    back.files.len()
                );
            }
            oracle!(
                back.files.len() == m.files.len(),
                "{what}: round trip changed the file count {} -> {}\n{text}",
                m.files.len(),
                back.files.len()
            );
            if let Ok(t2) = encode(&back) {
                oracle!(
                    t2 == text,
                    "{what}: encode(decode(encode(m))) != encode(m)\n{text}\n{t2}"
                );
            }
        }
        Ok(None) => oracle!(
            false,
            "{what}: decodes as a different manifest kind\n{text}"
        ),
        Err(e) if symlink_target_under_file(m) => known_issue!(
            "snapshots-v2025-symlink-target-dirs",
            false,
            "{what}: encoded manifest does not decode: {e}\n{text}"
        ),
        Err(e) if file_is_ancestor(m) => known_issue!(
            "snapshots-file-as-ancestor",
            false,
            "{what}: encoded manifest does not decode: {e}\n{text}"
        ),
        Err(e) => oracle!(
            false,
            "{what}: encoded manifest does not decode: {e}\nfiles={:?}\ndirs={:?}\n{text}",
            m.files
                .iter()
                .map(|f| (&f.path, f.deleted, &f.symlink_target))
                .collect::<Vec<_>>(),
            m.dirs
        ),
    }
}

/// A diff the v2023 encoder accepts decodes back through
/// `decode_v2023_as_diff` to its live entries: v2023 has no deletions, so the
/// encoder drops deleted entries (documented) and the format has no
/// `runnable` flag. Symlinks are collapsed by the encoder, so with symlinks
/// only the regular files' survival is checked.
fn v2023_diff_roundtrip(m: &openjd_snapshots::SnapshotDiff, what: &str) {
    let Ok(text) = encode_snapshot_diff_v2023(m) else {
        return;
    };
    let back = match openjd_snapshots::decode_v2023_as_diff(&text) {
        Ok(back) => back,
        Err(e) => {
            oracle!(
                false,
                "{what}: v2023-encoded diff does not decode: {e}\n{text}"
            );
            return;
        }
    };
    // v2023 entries carry no `runnable` flag either.
    let mut live = m.clone();
    live.files.retain(|f| !f.deleted);
    for f in &mut live.files {
        f.runnable = false;
    }
    let has_symlinks = live.files.iter().any(|f| f.symlink_target.is_some());
    if has_symlinks {
        let paths = |x: &openjd_snapshots::SnapshotDiff| {
            let mut v: Vec<String> = x.files.iter().map(|f| f.path.clone()).collect();
            v.sort();
            v
        };
        // Collapsing adds a symlinked directory's files under the link, so
        // only require every live regular file to survive.
        let back_paths = paths(&back);
        let lost: Vec<&FileEntry> = live
            .files
            .iter()
            .filter(|f| f.symlink_target.is_none() && !back_paths.contains(&f.path))
            .collect();
        oracle!(
            lost.is_empty(),
            "{what}: v2023 round trip lost regular files {lost:?}\n{text}"
        );
    } else {
        oracle!(
            by_path(&back.files) == by_path(&live.files),
            "{what}: v2023 round trip changed the live entries: {:?}",
            by_path(&live.files)
                .into_iter()
                .zip(by_path(&back.files))
                .find(|(x, y)| x != y)
        );
    }
}

/// `decode_manifest` + `validate` + re-encoding, on arbitrary JSON text.
pub fn decode_text(data: &[u8]) {
    let Some((text, _)) = text_and_control(data) else {
        return;
    };
    let Ok(decoded) = decode_manifest(text) else {
        return;
    };
    match decoded {
        DecodedManifest::AbsSnapshot(m) => {
            if m.validate().is_ok() {
                roundtrip(
                    &m,
                    encode_abs_snapshot_v2025,
                    |d| match d {
                        DecodedManifest::AbsSnapshot(x) => Some(x),
                        _ => None,
                    },
                    "abs snapshot v2025",
                );
                let _ = m.common_root();
            }
        }
        DecodedManifest::AbsSnapshotDiff(m) => {
            if m.validate().is_ok() {
                roundtrip(
                    &m,
                    encode_abs_snapshot_diff_v2025,
                    |d| match d {
                        DecodedManifest::AbsSnapshotDiff(x) => Some(x),
                        _ => None,
                    },
                    "abs diff v2025",
                );
            }
        }
        DecodedManifest::Snapshot(m) => {
            if m.validate().is_ok() {
                roundtrip(
                    &m,
                    encode_snapshot_v2025,
                    |d| match d {
                        DecodedManifest::Snapshot(x) => Some(x),
                        _ => None,
                    },
                    "snapshot v2025",
                );
                roundtrip(
                    &m,
                    encode_snapshot_v2023,
                    |d| match d {
                        DecodedManifest::Snapshot(x) => Some(x),
                        _ => None,
                    },
                    "snapshot v2023",
                );
            }
        }
        DecodedManifest::SnapshotDiff(m) => {
            if m.validate().is_ok() {
                roundtrip(
                    &m,
                    encode_snapshot_diff_v2025,
                    |d| match d {
                        DecodedManifest::SnapshotDiff(x) => Some(x),
                        _ => None,
                    },
                    "diff v2025",
                );
                v2023_diff_roundtrip(&m, "diff");
            }
        }
    }
    let _ = openjd_snapshots::decode_v2023(text);
    let _ = openjd_snapshots::decode_v2025(text);
}

/// A file's content identity: hash, size, symlink target, runnable.
type Content = (Option<String>, Option<u64>, Option<String>, bool);

/// File entries by path, ignoring mtime (diffs are content-based).
/// The `AbsManifest` / `RelManifest` accessors report the wrapped manifest's
/// fields.
macro_rules! wrapper_matches {
    ($w:expr, $m:expr) => {{
        let (w, m) = (&$w, &$m);
        oracle!(
            w.files() == m.files.as_slice()
                && w.dirs() == m.dirs.as_slice()
                && w.hash_alg() == m.hash_alg
                && w.file_chunk_size_bytes() == m.file_chunk_size_bytes
                && w.total_size() == m.total_size
                && w.parent_manifest_hash() == m.parent_manifest_hash.as_deref(),
            "manifest wrapper accessors disagree with the wrapped manifest"
        );
    }};
}

fn by_path(files: &[FileEntry]) -> BTreeMap<String, Content> {
    files
        .iter()
        .filter(|f| !f.deleted)
        .map(|f| {
            (
                f.path.clone(),
                (f.hash.clone(), f.size, f.symlink_target.clone(), f.runnable),
            )
        })
        .collect()
}

/// Dedupe paths so generated manifests are valid snapshots.
fn dedupe(m: &mut AbsSnapshot) {
    let mut seen = std::collections::HashSet::new();
    m.files.retain(|f| seen.insert(f.path.clone()));
    m.dirs.retain(|d| seen.insert(d.path.clone()));
    m.recompute_total_size();
}

/// Is some file also an ancestor of another entry (`/a` and `/a/b`)? No
/// filesystem produces that, `validate()` does not reject it, and the
/// manifest operations then disagree (known issue
/// `snapshots-file-as-ancestor`).
fn file_is_ancestor<P, K>(m: &Manifest<P, K>) -> bool {
    m.files.iter().any(|f| {
        let prefix = format!("{}/", f.path);
        m.files.iter().any(|g| g.path.starts_with(&prefix))
            || m.dirs.iter().any(|d| d.path.starts_with(&prefix))
    })
}

/// Does a symlink target lie beneath a file entry (including the symlink
/// itself)? The v2025 encoder makes directory entries for a target's
/// ancestors, which then collide with the file (known issue
/// `snapshots-v2025-symlink-target-dirs`).
fn symlink_target_under_file<P, K>(m: &Manifest<P, K>) -> bool {
    m.files
        .iter()
        .filter_map(|f| f.symlink_target.as_deref())
        .any(|t| {
            m.files
                .iter()
                .any(|f| t.starts_with(&format!("{}/", f.path)))
        })
}

/// Generated manifests through every pure manifest operation.
pub fn ops_gen(data: &[u8]) {
    let mut u = Unstructured::new(data);
    let mut a: AbsSnapshot = manifest(&mut u, true, false);
    let mut b: AbsSnapshot = manifest(&mut u, true, false);
    b.file_chunk_size_bytes = a.file_chunk_size_bytes;
    dedupe(&mut a);
    dedupe(&mut b);
    if a.validate().is_err() || b.validate().is_err() {
        return;
    }
    let opts = DiffOptions::default();

    // diff(a, a) is empty.
    if let Ok(d) = diff_snapshots(&a, &a, &opts) {
        oracle!(
            d.files.is_empty() && d.dirs.is_empty(),
            "diff(a, a) is not empty: {:?}",
            d.files
        );
    }
    for f in &a.files {
        oracle!(
            !entries_differ(f, f, false, false),
            "entries_differ(x, x) for {f:?}"
        );
    }

    // compose(a, [diff(a, b)]) == b, by file content.
    if let Ok(d) = diff_snapshots(&a, &b, &opts) {
        let type_change = a
            .files
            .iter()
            .any(|f| b.dirs.iter().any(|x| x.path == f.path))
            || b.files
                .iter()
                .any(|f| a.dirs.iter().any(|x| x.path == f.path));
        if type_change {
            known_issue!(
                "snapshots-type-change-diff",
                d.validate().is_ok(),
                "diff produced an invalid manifest: {:?}",
                d.validate()
            );
        } else {
            oracle!(
                d.validate().is_ok(),
                "diff produced an invalid manifest: {:?}",
                d.validate()
            );
        }
        match compose_snapshot_with_diffs(&a, &[&d]) {
            Ok(c) => {
                if file_is_ancestor(&a) || file_is_ancestor(&b) {
                    known_issue!(
                        "snapshots-file-as-ancestor",
                        by_path(&c.files) == by_path(&b.files),
                        "compose(a, diff(a, b)) != b\na={:?}\nb={:?}\nd={:?}\nc={:?}",
                        a.files,
                        b.files,
                        d.files,
                        c.files
                    );
                } else {
                    oracle!(
                        by_path(&c.files) == by_path(&b.files),
                        "compose(a, diff(a, b)) != b\na={:?}\nb={:?}\nd={:?}\nc={:?}",
                        a.files,
                        b.files,
                        d.files,
                        c.files
                    );
                }
                match c.validate() {
                    Ok(()) => {}
                    // The known shape: synthesized ancestor dirs repeat a
                    // path ("/" twice).
                    Err(e) if e.to_string().contains("duplicate path: ") => {
                        crate::known::hit("snapshots-compose-dirs", || {
                            format!("compose produced an invalid manifest: {e}")
                        })
                    }
                    Err(e) => oracle!(false, "compose produced an invalid manifest: {e}"),
                }
            }
            Err(e) => oracle!(false, "compose(a, [diff(a, b)]) failed: {e}"),
        }
        // Composing diffs is associative with applying them in sequence.
        let mut c3: AbsSnapshot = manifest(&mut u, true, false);
        c3.file_chunk_size_bytes = a.file_chunk_size_bytes;
        dedupe(&mut c3);
        if c3.validate().is_ok() {
            if let Ok(d2) = diff_snapshots(&b, &c3, &opts) {
                let seq = compose_snapshot_with_diffs(&a, &[&d, &d2]);
                let combined = compose_diffs(&[&d, &d2]);
                if let (Ok(seq), Ok(comb)) = (seq, combined) {
                    match compose_snapshot_with_diffs(&a, &[&comb]) {
                        Ok(one) => known_issue!(
                            "snapshots-compose-dirs",
                            by_path(&one.files) == by_path(&seq.files),
                            "compose(a, [compose_diffs(d1, d2)]) != compose(a, [d1, d2])"
                        ),
                        Err(e) => oracle!(false, "compose with combined diff failed: {e}"),
                    }
                }
            }
        }
    }

    // Every encoder round-trips generated manifests (generated manifests
    // cover shapes the mined seeds lack: absolute snapshots and diffs).
    roundtrip(
        &a,
        encode_abs_snapshot_v2025,
        |d| match d {
            DecodedManifest::AbsSnapshot(x) => Some(x),
            _ => None,
        },
        "generated abs snapshot v2025",
    );
    let _ = (
        openjd_snapshots::HashAlgorithm::Xxh128.extension(),
        openjd_snapshots::hash::hash_data(a.files.len().to_string().as_bytes()),
        openjd_snapshots::human_readable_file_size(a.total_size),
    );
    let mut cleared = a.clone().with_parent_hash(Some("p".into()));
    cleared.clear_hashes();
    oracle!(
        cleared
            .files
            .iter()
            .all(|f| f.hash.is_none() && f.chunk_hashes.is_none()),
        "clear_hashes left a hash"
    );
    if let Ok(d) = diff_snapshots(&a, &b, &opts) {
        roundtrip(
            &d,
            encode_abs_snapshot_diff_v2025,
            |x| match x {
                DecodedManifest::AbsSnapshotDiff(y) => Some(y),
                _ => None,
            },
            "generated abs diff v2025",
        );
        let am = openjd_snapshots::AbsManifest::Diff(d.clone());
        wrapper_matches!(am, d);
        // Preserve fails on escaping symlinks; fall back to excluding them.
        let rel = [
            SymlinkPolicy::Preserve,
            SymlinkPolicy::ExcludeEscaping,
            SymlinkPolicy::ExcludeAll,
        ]
        .into_iter()
        .find_map(|p| openjd_snapshots::subtree_snapshot_diff(&d, ABS_ROOT, p).ok());
        if let Some(rel) = rel {
            roundtrip(
                &rel,
                encode_snapshot_diff_v2025,
                |x| match x {
                    DecodedManifest::SnapshotDiff(y) => Some(y),
                    _ => None,
                },
                "generated diff v2025",
            );
            v2023_diff_roundtrip(&rel, "generated diff");
            let _ = openjd_snapshots::join_snapshot_diff(&rel, ABS_ROOT);
            let _ = openjd_snapshots::join_snapshot_diff_rel(&rel, "pre");
            let _ = openjd_snapshots::subtree_rel_snapshot_diff(&rel, "a", SymlinkPolicy::Preserve);
            let rm = openjd_snapshots::RelManifest::Diff(rel.clone());
            wrapper_matches!(rm, rel);
            let _ = openjd_snapshots::join_manifest(&rm, ABS_ROOT);
            let _ = openjd_snapshots::join_manifest_rel(&rm, "pre");
            let _ = openjd_snapshots::subtree_rel_manifest(&rm, "a", SymlinkPolicy::Preserve);
        }
        let _ = openjd_snapshots::subtree_manifest(&am, ABS_ROOT, SymlinkPolicy::Preserve);
    }
    let rel = [
        SymlinkPolicy::Preserve,
        SymlinkPolicy::ExcludeEscaping,
        SymlinkPolicy::ExcludeAll,
    ]
    .into_iter()
    .find_map(|p| subtree_snapshot(&a, ABS_ROOT, p).ok());
    if let Some(rel) = rel {
        let _ = openjd_snapshots::join_snapshot_rel(&rel, "pre");
        let _ = openjd_snapshots::partition_rel_manifest(&rel, &PartitionOptions::default());
    }

    // Generated diffs (with deletions) compose without panicking.
    let d1: AbsSnapshotDiff = manifest(&mut u, true, true);
    let d2: AbsSnapshotDiff = manifest(&mut u, true, true);
    let _ = compose_diffs(&[&d1, &d2]);
    let _ = compose_snapshot_with_diffs(&a, &[&d1]);

    let policy = *pick(
        &mut u,
        &[
            SymlinkPolicy::Preserve,
            SymlinkPolicy::CollapseEscaping,
            SymlinkPolicy::CollapseAll,
            SymlinkPolicy::ExcludeEscaping,
            SymlinkPolicy::ExcludeAll,
            SymlinkPolicy::TransitiveIncludeTargets,
        ],
    );

    // subtree then join stays within the input.
    let sub_root = if one_in(&mut u, 2) {
        ABS_ROOT.to_string()
    } else {
        format!("{ABS_ROOT}/{}", crate::gen::manifest::rel_path(&mut u))
    };
    if let Ok(sub) = subtree_snapshot(&a, &sub_root, policy) {
        let nested_symlink = matches!(
            sub.validate(),
            Err(openjd_snapshots::SnapshotError::Validation(ref m)) if m.contains("symlink_target path style mismatch")
        );
        if nested_symlink {
            known_issue!(
                "snapshots-collapse-nested-symlink",
                false,
                "subtree({sub_root:?}, {policy}) produced an invalid manifest: {:?}\na={:?}\nsub={:?}",
                sub.validate(),
                a.files,
                sub.files
            );
        } else if file_is_ancestor(&a) {
            known_issue!(
                "snapshots-file-as-ancestor",
                sub.validate().is_ok(),
                "subtree({sub_root:?}, {policy}) produced an invalid manifest: {:?}\na={:?}\nsub={:?}",
                sub.validate(),
                a.files,
                sub.files
            );
        } else {
            oracle!(
                sub.validate().is_ok(),
                "subtree({sub_root:?}, {policy}) produced an invalid manifest: {:?}\na={:?}\nsub={:?}",
                sub.validate(),
                a.files,
                sub.files
            );
        }
        if let Ok(joined) = join_snapshot(&sub, &sub_root) {
            let orig = by_path(&a.files);
            for f in &joined.files {
                oracle!(
                    f.path.starts_with(&sub_root),
                    "subtree/join produced {:?} outside {sub_root:?}",
                    f.path
                );
                if policy == SymlinkPolicy::Preserve && f.symlink_target.is_none() {
                    oracle!(
                        orig.contains_key(&f.path),
                        "subtree/join invented {:?}",
                        f.path
                    );
                }
            }
        }
    }

    // Partitions cover every file exactly once.
    let popts = PartitionOptions {
        symlink_policy: SymlinkPolicy::Preserve,
        ..Default::default()
    };
    if let Ok(parts) = partition_manifest(&a, &popts) {
        let mut total = 0;
        for (root, p) in &parts {
            oracle!(p.validate().is_ok(), "partition {root:?} is invalid");
            total += p.files.len();
        }
        oracle!(
            total == a.files.len(),
            "partitions hold {total} files, manifest has {}",
            a.files.len()
        );
    }

    // Filters keep a subset, and every kept entry matches.
    let pats = [
        "**",
        "/r/a/**",
        "**/*.b",
        "/r/?",
        "[a-c]*",
        "/r/日/**",
        "**/.z",
    ];
    let include: Vec<&str> = (0..upto(&mut u, 2)).map(|_| *pick(&mut u, &pats)).collect();
    let exclude: Vec<&str> = (0..upto(&mut u, 2)).map(|_| *pick(&mut u, &pats)).collect();
    if let Ok(filter) = IncludeExcludePathsFilter::new(&include, &exclude) {
        let kept = filter_manifest(&a, &|e| filter.matches(e));
        oracle!(
            kept.files.len() <= a.files.len(),
            "filter grew the manifest"
        );
        for f in &kept.files {
            oracle!(
                filter.matches_path(&f.path),
                "filter kept non-matching {:?}",
                f.path
            );
        }
    }

    if let Some(root) = a.common_root() {
        for f in a.files.iter().filter(|f| !f.deleted) {
            oracle!(
                f.path.starts_with(&root),
                "common_root {root:?} is not a prefix of {:?}",
                f.path
            );
        }
    }
}
