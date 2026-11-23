// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Encode/decode manifests to/from on-disk v2023 and v2025 JSON formats.
//!
//! # Format Status
//!
//! - **v2023** — **Stable.** The on-disk format used by AWS Deadline Cloud's
//!   job attachments. Identified on disk by `"manifestVersion": "2023-03-03"`.
//!   Entry points: [`encode_snapshot_v2023`], [`encode_snapshot_diff_v2023`],
//!   [`decode_v2023`], [`decode_v2023_as_diff`], and, to read the entries
//!   one at a time without building the manifest, [`decode_v2023_try_for_each_file`]
//!   and [`decode_v2023_try_for_each_file_from_reader`].
//! - **v2025** — **Experimental draft.** A proposed evolution adding diffs,
//!   explicit directories, symlinks, and file chunking. Identified on disk by
//!   `specificationVersion` strings that include the `beta-2025-12` tag. The
//!   wire format is expected to change before any stable release and is not
//!   suitable for long-term storage or external interop. Entry points:
//!   [`encode_v2025`], [`encode_abs_snapshot_v2025`],
//!   [`encode_abs_snapshot_diff_v2025`], [`encode_snapshot_v2025`],
//!   [`encode_snapshot_diff_v2025`], [`decode_v2025`].

use crate::hash::HashAlgorithm;
use crate::manifest::{
    check_symlink_target, AbsSnapshot, AbsSnapshotDiff, DirEntry, FileEntry, Manifest, Rel,
    Snapshot, SnapshotDiff, SymlinkPolicy, ValidateKind,
};
use crate::ops::subtree_rel_snapshot;
use crate::path_util::implied_dirs;
use crate::{Result, SnapshotError, DEFAULT_FILE_CHUNK_SIZE, WHOLE_FILE_CHUNK_SIZE};
use serde_json::Value;
use std::collections::HashMap;
use tracing::warn;

mod raw;
mod stream;

// --- Public types ---

/// Identifier for the two supported on-disk manifest formats.
///
/// See the [module-level docs](self) for the status of each format.
///
/// - [`V2023`](Self::V2023) — stable; the on-disk format used by AWS
///   Deadline Cloud's job attachments.
/// - [`V2025`](Self::V2025) — **experimental draft**. The on-disk wire format
///   is expected to change before any stable release (its
///   `specificationVersion` strings carry the `beta-2025-12` tag).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(
    missing_docs,
    reason = "missing-docs burn-down: predates the lint; document these items, then delete this attribute"
)]
pub enum ManifestFormat {
    V2023,
    V2025,
}

#[derive(Debug)]
#[allow(
    missing_docs,
    reason = "missing-docs burn-down: predates the lint; document these items, then delete this attribute"
)]
pub enum DecodedManifest {
    AbsSnapshot(AbsSnapshot),
    AbsSnapshotDiff(AbsSnapshotDiff),
    Snapshot(Snapshot),
    SnapshotDiff(SnapshotDiff),
}

// --- Specification version strings ---

const V2023_MANIFEST_VERSION: &str = "2023-03-03";
const SPEC_ABS_SNAPSHOT: &str = "absolute-manifest-snapshot-beta-2025-12";
const SPEC_ABS_DIFF: &str = "absolute-manifest-diff-beta-2025-12";
const SPEC_REL_SNAPSHOT: &str = "relative-manifest-snapshot-beta-2025-12";
const SPEC_REL_DIFF: &str = "relative-manifest-diff-beta-2025-12";

// --- Encode ---

/// Checks every symlink target in a relative manifest with the rule
/// `Manifest::validate` applies, so an encoder rejects a bad target with
/// the same message validation gives.
fn check_relative_symlink_targets(files: &[FileEntry]) -> Result<()> {
    for f in files {
        if let Some(ref target) = f.symlink_target {
            check_symlink_target::<Rel>(&f.path, target)?;
        }
    }
    Ok(())
}

/// Sum the `size` fields of encoded v2023 path entries.
///
/// Checked rather than saturating: sizes are caller data, and a clamped
/// `totalSize` would disagree with the entries. Python writes the exact
/// (arbitrary-precision) sum instead; Rust's `u64` sizes cannot, so this is
/// an intentional divergence (see `specs/snapshots/snapshot_codec.md`).
fn v2023_total_size(paths: &[Value]) -> Result<u64> {
    paths
        .iter()
        .try_fold(0u64, |acc, p| {
            acc.checked_add(p["size"].as_u64().unwrap_or(0))
        })
        .ok_or_else(|| {
            SnapshotError::Validation(
                "v2023 totalSize: the file sizes sum past u64::MAX".to_string(),
            )
        })
}

/// Encode a Snapshot to v2023 JSON. Fails with `SnapshotError::Validation`
/// if a file is missing a hash, a symlink target is not a valid relative
/// path, the chunk size is not whole-file, or the file sizes sum past
/// `u64::MAX`.
pub fn encode_snapshot_v2023(manifest: &Snapshot) -> Result<String> {
    encode_v2023(manifest)
}

/// Encode a SnapshotDiff to v2023 JSON (drops deletions). Fails with
/// `SnapshotError::Validation` under the same conditions as
/// [`encode_snapshot_v2023`], including file sizes that sum past `u64::MAX`.
pub fn encode_snapshot_diff_v2023(manifest: &SnapshotDiff) -> Result<String> {
    // Symlink collapsing works on a Snapshot, so carry the diff's entries,
    // deletions included, into one; `encode_v2023` drops the deletions.
    let mut snap: Snapshot = Manifest::new(manifest.hash_alg, manifest.file_chunk_size_bytes);
    snap.files = manifest.files.clone();
    snap.dirs = manifest.dirs.clone();
    snap.total_size = manifest.total_size;
    encode_v2023(&snap)
}

/// The v2023 encoding both [`encode_snapshot_v2023`] and
/// [`encode_snapshot_diff_v2023`] produce. v2023 has no symlinks,
/// directories, or deletions: symlinks are collapsed into the files they
/// point at, and empty directories and deleted entries are dropped with a
/// warning.
fn encode_v2023(manifest: &Snapshot) -> Result<String> {
    check_relative_symlink_targets(&manifest.files)?;
    if manifest.file_chunk_size_bytes != WHOLE_FILE_CHUNK_SIZE {
        return Err(SnapshotError::Validation(format!(
            "v2023 format requires fileChunkSizeBytes={WHOLE_FILE_CHUNK_SIZE}, got {}",
            manifest.file_chunk_size_bytes
        )));
    }

    // Collapse symlinks via identity subtree
    let collapsed = subtree_rel_snapshot(manifest, ".", SymlinkPolicy::CollapseAll)?;

    let deleted_count = collapsed.files.iter().filter(|f| f.deleted).count()
        + collapsed.dirs.iter().filter(|d| d.deleted).count();
    if deleted_count > 0 {
        warn!(
            count = deleted_count,
            "dropping deletions not supported in v2023 format"
        );
    }

    let implied = implied_dirs(
        collapsed
            .files
            .iter()
            .filter(|f| !f.deleted)
            .map(|f| f.path.as_str())
            .chain(
                collapsed
                    .dirs
                    .iter()
                    .filter(|d| !d.deleted)
                    .map(|d| d.path.as_str()),
            ),
    );
    let empty_count = collapsed
        .dirs
        .iter()
        .filter(|d| !d.deleted && !implied.contains(d.path.as_str()))
        .count();
    if empty_count > 0 {
        warn!(
            count = empty_count,
            "dropping empty directories not supported in v2023 format"
        );
    }

    let mut paths: Vec<Value> = Vec::new();
    for f in &collapsed.files {
        if f.deleted || f.symlink_target.is_some() {
            continue;
        }
        let hash = f.hash.as_ref().ok_or_else(|| {
            SnapshotError::Validation(format!("File '{}' is missing a hash", f.path))
        })?;
        paths.push(serde_json::json!({
            "hash": hash,
            "mtime": f.mtime,
            "path": f.path,
            "size": f.size,
        }));
    }

    // Sort by UTF-16 BE encoding of path
    paths.sort_by(|a, b| {
        let pa = a["path"].as_str().unwrap();
        let pb = b["path"].as_str().unwrap();
        utf16_be_bytes(pa).cmp(&utf16_be_bytes(pb))
    });

    // totalSize counts the written (non-deleted, non-symlink) files only.
    let obj = serde_json::json!({
        "hashAlg": collapsed.hash_alg.extension(),
        "manifestVersion": V2023_MANIFEST_VERSION,
        "paths": paths,
        "totalSize": v2023_total_size(&paths)?,
    });

    // sort_keys + no whitespace = canonical JSON
    Ok(canonical_json(&obj))
}

/// Encode any manifest to v2025 JSON.
///
/// **Experimental draft format.** The v2025 wire format is expected to change
/// before any stable release; see the [module-level docs](self) for details.
pub fn encode_v2025<P: Clone + std::fmt::Debug, K: Clone + std::fmt::Debug>(
    manifest: &Manifest<P, K>,
    spec_version: &str,
) -> Result<String> {
    // Collect all directories (explicit + inferred from file/symlink paths)
    let all_dirs = collect_all_directories(&manifest.dirs, &manifest.files);

    // Sort and deduplicate
    let mut sorted_dirs = all_dirs;
    sorted_dirs.sort_by(|a, b| a.path.cmp(&b.path));
    sorted_dirs.dedup_by(|a, b| a.path == b.path);

    // Build dir_index
    let dir_index: HashMap<&str, usize> = sorted_dirs
        .iter()
        .enumerate()
        .map(|(i, d)| (d.path.as_str(), i))
        .collect();

    // Encode dirs
    let dirs_json: Vec<Value> = sorted_dirs
        .iter()
        .map(|d| {
            let name = encode_path_with_dir_index(&d.path, &dir_index);
            let mut entry = serde_json::json!({"name": name});
            if d.deleted {
                entry["delete"] = Value::Bool(true);
            }
            entry
        })
        .collect();

    // Sort files by UTF-16 BE
    let mut sorted_files = manifest.files.clone();
    sorted_files.sort_by_key(|a| utf16_be_bytes(&a.path));

    // Encode files
    let files_json: Vec<Value> = sorted_files
        .iter()
        .map(|f| {
            let name = encode_path_with_dir_index(&f.path, &dir_index);
            let mut entry = serde_json::json!({"name": name});

            if let Some(ref h) = f.hash {
                entry["hash"] = Value::String(h.clone());
            } else if let Some(ref ch) = f.chunk_hashes {
                entry["chunkhashes"] = serde_json::json!(ch);
            } else if let Some(ref target) = f.symlink_target {
                let encoded = encode_path_with_dir_index(target, &dir_index);
                entry["symlink"] = serde_json::json!({"name": encoded});
            }

            if !f.deleted && f.symlink_target.is_none() {
                if let Some(s) = f.size {
                    entry["size"] = Value::Number(s.into());
                }
                if let Some(m) = f.mtime {
                    entry["mtime"] = Value::Number(m.into());
                }
                if f.runnable {
                    entry["runnable"] = Value::Bool(true);
                }
            }

            if f.deleted {
                entry["delete"] = Value::Bool(true);
            }

            entry
        })
        .collect();

    let mut obj = serde_json::json!({
        "dirs": dirs_json,
        "files": files_json,
        "hashAlg": manifest.hash_alg.extension(),
        "specificationVersion": spec_version,
        "totalSize": manifest.total_size,
    });

    if let Some(ref pmh) = manifest.parent_manifest_hash {
        obj["parentManifestHash"] = Value::String(pmh.clone());
    }

    obj["fileChunkSizeBytes"] = Value::Number(manifest.file_chunk_size_bytes.into());

    Ok(canonical_json(&obj))
}

/// Encode an absolute snapshot to v2025 JSON.
///
/// **Experimental draft format.** See the [module-level docs](self).
pub fn encode_abs_snapshot_v2025(m: &AbsSnapshot) -> Result<String> {
    encode_v2025(m, SPEC_ABS_SNAPSHOT)
}

/// Encode an absolute snapshot diff to v2025 JSON.
///
/// **Experimental draft format.** See the [module-level docs](self).
pub fn encode_abs_snapshot_diff_v2025(m: &AbsSnapshotDiff) -> Result<String> {
    encode_v2025(m, SPEC_ABS_DIFF)
}

/// Encode a relative snapshot to v2025 JSON.
///
/// **Experimental draft format.** See the [module-level docs](self).
pub fn encode_snapshot_v2025(m: &Snapshot) -> Result<String> {
    check_relative_symlink_targets(&m.files)?;
    encode_v2025(m, SPEC_REL_SNAPSHOT)
}

/// Encode a relative snapshot diff to v2025 JSON.
///
/// **Experimental draft format.** See the [module-level docs](self).
pub fn encode_snapshot_diff_v2025(m: &SnapshotDiff) -> Result<String> {
    check_relative_symlink_targets(&m.files)?;
    encode_v2025(m, SPEC_REL_DIFF)
}

// --- Decode ---
//
// Each decoder parses its input once, into the typed view in `raw`, and then
// validates that view. See `raw` for why no `serde_json::Value` is built.

/// Auto-detect format and decode.
///
/// A document with a `manifestVersion` key is decoded as v2023, otherwise one
/// with a `specificationVersion` key as v2025. The input is parsed once.
pub fn decode_manifest(json: &str) -> Result<DecodedManifest> {
    let raw = raw::parse(json)?;

    if raw.has_manifest_version {
        Ok(DecodedManifest::Snapshot(decode_v2023_raw(raw)?))
    } else if raw.has_specification_version {
        decode_v2025_raw(raw)
    } else {
        Err(SnapshotError::Validation(
            "Unknown manifest format: no manifestVersion or specificationVersion".into(),
        ))
    }
}

/// Decode v2023 JSON as a relative-path full [`Snapshot`].
///
/// The v2023 manifest format has no structural concept of deletions, so every
/// successfully decoded v2023 manifest is, structurally, also a valid full
/// snapshot. Use [`decode_v2023_as_diff`] instead if you need a
/// [`SnapshotDiff`] (e.g. to pass to
/// [`compose_snapshot_with_diffs`](crate::compose_snapshot_with_diffs) without
/// reconstructing the manifest).
pub fn decode_v2023(json: &str) -> Result<Snapshot> {
    decode_v2023_raw(raw::parse(json)?)
}

/// Decode v2023 JSON as a relative-path [`SnapshotDiff`].
///
/// The v2023 format has no structural distinction between a full snapshot and
/// a diff — the phantom-type distinction is Rust-side only. Because v2023
/// manifests never contain deleted entries, the resulting diff always has an
/// empty deleted-entries set. This is purely a convenience for callers that
/// want to layer a v2023-formatted manifest into a diff pipeline (such as
/// [`compose_snapshot_with_diffs`](crate::compose_snapshot_with_diffs))
/// without manually converting from [`Snapshot`].
pub fn decode_v2023_as_diff(json: &str) -> Result<SnapshotDiff> {
    decode_v2023_raw(raw::parse(json)?)
}

/// What [`decode_v2023_try_for_each_file`] and
/// [`decode_v2023_try_for_each_file_from_reader`] read from a v2023 manifest
/// besides its entries.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct V2023Summary {
    /// The manifest's `hashAlg`.
    pub hash_alg: HashAlgorithm,
    /// The manifest's `totalSize`, as written (it is not recomputed).
    pub total_size: u64,
    /// The number of entries passed to the callback.
    pub total_files: usize,
}

/// Decode v2023 JSON one entry at a time, without building the manifest.
///
/// Each element of `paths` is turned into a [`FileEntry`] and passed to `f`,
/// with the manifest's hash algorithm, as soon as it is parsed. On success,
/// returns the manifest's [`V2023Summary`].
///
/// Memory use beyond `json` itself does not depend on the number of
/// entries: one entry is held at a time. It does grow with the largest entry,
/// since building a [`FileEntry`] normalizes its path, which can take over
/// ten times the path's length while it runs, and with the longest string
/// containing escapes, which serde_json unescapes into a buffer even when the
/// member is not used.
///
/// As with [`Iterator::try_for_each`], the first error `f` returns stops the
/// decode and is returned unchanged, so return one to stop early. Decode
/// errors are converted into `E` with `From<SnapshotError>`, so `E` can be
/// [`SnapshotError`] itself, or an application error type. Unless `f`
/// returns an `Err` of a concrete type somewhere, name `E` in its result, as
/// in `Ok::<_, SnapshotError>(())`: `?` inside `f` converts into any `E`, so
/// it does not determine it.
///
/// Values are read as [`decode_v2023`] reads them, each entry is checked as
/// it checks entries, and errors have its messages, with these differences:
///
/// - The header is checked when `paths` arrives, with the values read so
///   far, so `manifestVersion` and `hashAlg` must come before `paths`, as
///   they do in canonical (sorted-key) v2023 output, which
///   [`encode_snapshot_v2023`] writes. If either has not
///   appeared by then, the error is `missing manifestVersion before paths`
///   (or `hashAlg`). If either appears again after `paths`, the error is
///   `manifestVersion must come before paths` (or `hashAlg`), and a second
///   `paths` key fails with `paths appears more than once`. [`decode_v2023`]
///   instead uses the last value of a repeated key.
/// - Errors are mostly reported in document order, and entries are delivered
///   before the rest of the document is read. Each entry is checked in full
///   before the next is read, so the first bad entry is reported, where
///   [`decode_v2023`] reports the first entry missing a member, and then the
///   first one failing a later check. An error found before a key that
///   [`decode_v2023`] would have taken instead (a bad header value or entry
///   ahead of a repeated key) is reported. `totalSize` is checked at the end,
///   wherever it appears, since a later `totalSize` replaces an earlier one,
///   so a bad entry is reported before a missing or invalid `totalSize`.
/// - If the function returns an error, discard the entries already
///   delivered: the manifest is invalid, or the decode was stopped by `f` or
///   by a read error, so the entries may not be all of them.
/// - Paths are not checked for duplicates, since that needs every path at
///   once. [`decode_v2023`] rejects a manifest that lists a path twice.
///
/// # Examples
///
/// ```
/// use openjd_snapshots::{decode_v2023_try_for_each_file, SnapshotError};
///
/// let json = r#"{"hashAlg":"xxh128","manifestVersion":"2023-03-03","paths":[
///     {"hash":"a1","mtime":1,"path":"a.txt","size":3},
///     {"hash":"b2","mtime":1,"path":"b.txt","size":4}],"totalSize":7}"#;
///
/// let mut keys = Vec::new();
/// let summary = decode_v2023_try_for_each_file(json, |alg, entry| {
///     let hash = entry.hash.expect("v2023 entries always have a hash");
///     keys.push(format!("Data/{hash}.{alg}"));
///     Ok::<_, SnapshotError>(())
/// })?;
/// assert_eq!(keys, ["Data/a1.xxh128", "Data/b2.xxh128"]);
/// assert_eq!(summary.total_size, 7);
/// # Ok::<_, SnapshotError>(())
/// ```
pub fn decode_v2023_try_for_each_file<F, E>(
    json: &str,
    f: F,
) -> std::result::Result<V2023Summary, E>
where
    F: FnMut(HashAlgorithm, FileEntry) -> std::result::Result<(), E>,
    E: From<SnapshotError>,
{
    stream::decode(serde_json::de::StrRead::new(json), f)
}

/// Like [`decode_v2023_try_for_each_file`], but reads the JSON from `reader`,
/// so the text is never held in memory as a whole.
///
/// The reader is read a byte at a time, so pass a buffered one, such as a
/// [`std::io::BufReader`]. A read error is returned as [`SnapshotError::Io`].
/// Each string in the document, including the values of members that are
/// not used, is copied into a buffer as it is read, so memory also grows
/// with the longest string.
/// Text after the manifest's closing `}` is read until the end of input, and
/// anything but whitespace there is an error, as it is for
/// [`decode_v2023_try_for_each_file`]. A syntax error's reported column can be
/// one past the one [`decode_v2023_try_for_each_file`] reports, as serde_json
/// counts positions differently when reading from a reader.
pub fn decode_v2023_try_for_each_file_from_reader<R, F, E>(
    reader: R,
    f: F,
) -> std::result::Result<V2023Summary, E>
where
    R: std::io::Read,
    F: FnMut(HashAlgorithm, FileEntry) -> std::result::Result<(), E>,
    E: From<SnapshotError>,
{
    stream::decode(serde_json::de::IoRead::new(reader), f)
}

/// Shared implementation for v2023 decoding. `K` selects the phantom-type
/// kind (`Full` or `Diff`); v2023 never produces deleted entries so either
/// choice is equally valid at runtime.
fn decode_v2023_raw<K: ValidateKind>(raw: raw::RawManifest) -> Result<Manifest<Rel, K>> {
    check_v2023_version(raw.manifest_version.as_deref())?;
    let hash_alg = parse_hash_alg(raw.hash_alg.as_deref())?;
    let total_size = require_total_size(raw.total_size)?;
    let raw_paths = raw.paths.ok_or_else(invalid_paths_error)?;

    // Sized up front: collecting `Result`s would grow the vector by doubling.
    let mut files = Vec::with_capacity(raw_paths.len());
    for p in raw_paths {
        files.push(v2023_file_entry(p)?);
    }

    let mut m: Manifest<Rel, K> = Manifest::new(hash_alg, WHOLE_FILE_CHUNK_SIZE);
    m.files = files;
    m.total_size = total_size;
    m.validate()?;
    Ok(m)
}

/// Checks a v2023 document's `manifestVersion`.
fn check_v2023_version(manifest_version: Option<&str>) -> Result<()> {
    let version = manifest_version
        .ok_or_else(|| SnapshotError::Validation("missing manifestVersion".into()))?;
    if version != V2023_MANIFEST_VERSION {
        return Err(SnapshotError::Validation(format!(
            "expected manifestVersion '{V2023_MANIFEST_VERSION}', got '{version}'"
        )));
    }
    Ok(())
}

/// The `totalSize` read from a v2023 manifest, which must be present.
fn require_total_size(total_size: Option<u64>) -> Result<u64> {
    total_size.ok_or_else(|| SnapshotError::Validation("missing or invalid totalSize".into()))
}

/// The error for a v2023 `paths` that is absent or not an array.
fn invalid_paths_error() -> SnapshotError {
    SnapshotError::Validation("missing or invalid paths".into())
}

/// Builds the file entry for one element of a v2023 `paths` array.
fn v2023_file_entry(p: raw::RawV2023Path) -> Result<FileEntry> {
    let path = p
        .path
        .ok_or_else(|| SnapshotError::Validation("path entry missing 'path'".into()))?;
    let hash = p
        .hash
        .ok_or_else(|| SnapshotError::Validation("path entry missing 'hash'".into()))?;
    let size = p
        .size
        .ok_or_else(|| SnapshotError::Validation("path entry missing 'size'".into()))?;
    let mtime = p
        .mtime
        .ok_or_else(|| SnapshotError::Validation("path entry missing 'mtime'".into()))?;

    let mut entry = FileEntry::file(path, size, mtime);
    entry.hash = Some(hash);
    Ok(entry)
}

/// Decode v2025 JSON to the appropriate manifest type.
///
/// **Experimental draft format.** See the [module-level docs](self).
pub fn decode_v2025(json: &str) -> Result<DecodedManifest> {
    decode_v2025_raw(raw::parse(json)?)
}

fn decode_v2025_raw(raw: raw::RawManifest) -> Result<DecodedManifest> {
    let raw::RawManifest {
        specification_version,
        hash_alg,
        total_size,
        parent_manifest_hash: parent_hash,
        file_chunk_size_bytes,
        dirs: raw_dirs,
        files: raw_files,
        ..
    } = raw;

    let spec = specification_version
        .as_deref()
        .ok_or_else(|| SnapshotError::Validation("missing specificationVersion".into()))?;

    let hash_alg = parse_hash_alg(hash_alg.as_deref())?;
    let total_size = total_size
        .ok_or_else(|| SnapshotError::Validation("missing or invalid totalSize".into()))?;

    let raw_dirs = raw_dirs.unwrap_or_default();
    let dir_index = build_dir_index(&raw_dirs)?;
    let dirs = decode_dirs(&raw_dirs, &dir_index)?;
    drop(raw_dirs);

    let files = decode_files(raw_files.unwrap_or_default(), &dir_index)?;

    let file_chunk_size_bytes = file_chunk_size_bytes.unwrap_or(DEFAULT_FILE_CHUNK_SIZE);

    match spec {
        SPEC_ABS_SNAPSHOT => {
            let mut m: AbsSnapshot = Manifest::new(hash_alg, file_chunk_size_bytes);
            m.files = files;
            m.dirs = dirs;
            m.total_size = total_size;
            m.parent_manifest_hash = parent_hash;
            m.validate()?;
            Ok(DecodedManifest::AbsSnapshot(m))
        }
        SPEC_ABS_DIFF => {
            let mut m: AbsSnapshotDiff = Manifest::new(hash_alg, file_chunk_size_bytes);
            m.files = files;
            m.dirs = dirs;
            m.total_size = total_size;
            m.parent_manifest_hash = parent_hash;
            m.validate()?;
            Ok(DecodedManifest::AbsSnapshotDiff(m))
        }
        SPEC_REL_SNAPSHOT => {
            let mut m: Snapshot = Manifest::new(hash_alg, file_chunk_size_bytes);
            m.files = files;
            m.dirs = dirs;
            m.total_size = total_size;
            m.parent_manifest_hash = parent_hash;
            m.validate()?;
            Ok(DecodedManifest::Snapshot(m))
        }
        SPEC_REL_DIFF => {
            let mut m: SnapshotDiff = Manifest::new(hash_alg, file_chunk_size_bytes);
            m.files = files;
            m.dirs = dirs;
            m.total_size = total_size;
            m.parent_manifest_hash = parent_hash;
            m.validate()?;
            Ok(DecodedManifest::SnapshotDiff(m))
        }
        _ => Err(SnapshotError::Validation(format!(
            "Unknown specificationVersion: {spec}"
        ))),
    }
}

// --- Internal helpers ---

/// Parses a manifest's `hashAlg` field, spelled as
/// [`HashAlgorithm::extension`].
fn parse_hash_alg(s: Option<&str>) -> Result<HashAlgorithm> {
    let name = s.ok_or_else(|| SnapshotError::Validation("missing hashAlg".into()))?;
    HashAlgorithm::from_extension(name)
        .ok_or_else(|| SnapshotError::Validation(format!("Unsupported hash algorithm: {name}")))
}

fn utf16_be_bytes(s: &str) -> Vec<u8> {
    s.encode_utf16().flat_map(|u| u.to_be_bytes()).collect()
}

/// Produce canonical JSON: sorted keys, no whitespace, ASCII-safe strings.
fn canonical_json(value: &Value) -> String {
    match value {
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            let entries: Vec<String> = keys
                .iter()
                .map(|k| {
                    format!(
                        "{}:{}",
                        canonical_json(&Value::String((*k).clone())),
                        canonical_json(&map[*k])
                    )
                })
                .collect();
            format!("{{{}}}", entries.join(","))
        }
        Value::Array(arr) => {
            let items: Vec<String> = arr.iter().map(canonical_json).collect();
            format!("[{}]", items.join(","))
        }
        Value::String(s) => json_encode_string(s),
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Null => "null".to_string(),
    }
}

/// Encode a string as JSON with non-ASCII characters escaped as \uXXXX,
/// matching Python's `json.dumps(ensure_ascii=True)` behavior.
fn json_encode_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\x08' => out.push_str("\\b"),
            '\x0c' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c < '\x20' => {
                // Control characters
                out.push_str(&format!("\\u{:04x}", c as u32));
            }
            c if c.is_ascii() => out.push(c),
            c => {
                // Non-ASCII: encode as \uXXXX (surrogate pairs for chars > U+FFFF)
                let mut buf = [0u16; 2];
                let encoded = c.encode_utf16(&mut buf);
                for unit in encoded.iter() {
                    out.push_str(&format!("\\u{:04x}", unit));
                }
            }
        }
    }
    out.push('"');
    out
}

fn encode_path_with_dir_index(path: &str, dir_index: &HashMap<&str, usize>) -> String {
    match path.rfind('/') {
        None => path.to_string(),
        Some(pos) => {
            let dir = &path[..pos];
            let name = &path[pos + 1..];
            if let Some(&idx) = dir_index.get(dir) {
                format!("${idx}/{name}")
            } else {
                path.to_string()
            }
        }
    }
}

fn collect_all_directories(dirs: &[DirEntry], files: &[FileEntry]) -> Vec<DirEntry> {
    let explicit: HashMap<&str, &DirEntry> = dirs.iter().map(|d| (d.path.as_str(), d)).collect();
    let mut all_paths: Vec<&str> = Vec::new();

    for f in files {
        all_paths.push(&f.path);
        if let Some(ref t) = f.symlink_target {
            all_paths.push(t);
        }
    }
    for d in dirs {
        all_paths.push(&d.path);
    }

    // Every ancestor directory, except the POSIX root `/` and the relative
    // root `.`, which the format leaves implicit. A drive root (`C:/`) is
    // listed; the decoder does not accept a drive-root name, so v2025 does
    // not round-trip drive-letter paths (see `snapshot_codec.md`).
    let mut result: Vec<DirEntry> = dirs.to_vec();
    for dir_path in implied_dirs(all_paths) {
        if dir_path != "/" && dir_path != "." && !explicit.contains_key(dir_path) {
            result.push(DirEntry::new(dir_path));
        }
    }
    result
}

fn build_dir_index(raw_dirs: &[raw::RawV2025Dir]) -> Result<HashMap<usize, String>> {
    let mut index: HashMap<usize, String> = HashMap::with_capacity(raw_dirs.len());
    for (i, d) in raw_dirs.iter().enumerate() {
        let name = d
            .name
            .as_deref()
            .ok_or_else(|| SnapshotError::Validation("dir entry missing 'name'".into()))?;
        let expanded = expand_path_reference(name, &index)?;
        index.insert(i, expanded);
    }
    Ok(index)
}

fn decode_dirs(
    raw_dirs: &[raw::RawV2025Dir],
    dir_index: &HashMap<usize, String>,
) -> Result<Vec<DirEntry>> {
    let mut result = Vec::with_capacity(raw_dirs.len());
    for (i, d) in raw_dirs.iter().enumerate() {
        let path = dir_index
            .get(&i)
            .ok_or_else(|| SnapshotError::Validation("dir index mismatch".into()))?;
        let mut entry = DirEntry::new(path);
        entry.deleted = d.delete.unwrap_or(false);
        result.push(entry);
    }
    Ok(result)
}

fn decode_files(
    raw_files: Vec<raw::RawV2025File>,
    dir_index: &HashMap<usize, String>,
) -> Result<Vec<FileEntry>> {
    let mut result = Vec::with_capacity(raw_files.len());
    for f in raw_files {
        let name = f
            .name
            .as_deref()
            .ok_or_else(|| SnapshotError::Validation("file entry missing 'name'".into()))?;
        let path = expand_path_reference(name, dir_index)?;

        // Non-string elements of `chunkhashes` are dropped.
        let chunk_hashes = f
            .chunkhashes
            .map(|hashes| hashes.into_iter().flatten().collect::<Vec<_>>());
        let symlink_target = f
            .symlink
            .name
            .as_deref()
            .map(|s| expand_path_reference(s, dir_index))
            .transpose()?;

        let mut entry = FileEntry::new(path);
        entry.hash = f.hash;
        entry.size = f.size;
        entry.mtime = f.mtime;
        entry.chunk_hashes = chunk_hashes;
        entry.symlink_target = symlink_target;
        entry.runnable = f.runnable.unwrap_or(false);
        entry.deleted = f.delete.unwrap_or(false);
        result.push(entry);
    }
    Ok(result)
}

fn expand_path_reference(name: &str, dir_index: &HashMap<usize, String>) -> Result<String> {
    let parts: Vec<&str> = name.splitn(2, '/').collect();
    if parts.len() == 1 {
        return Ok(name.to_string());
    }
    let (prefix, component) = (parts[0], parts[1]);
    if prefix.is_empty() {
        // Absolute path like "/project"
        return Ok(name.to_string());
    }
    if !prefix.starts_with('$') {
        return Err(SnapshotError::Validation(format!(
            "Invalid path format '{name}': paths with '/' must use $N/ reference"
        )));
    }
    let idx: usize = prefix[1..].parse().map_err(|_| {
        SnapshotError::Validation(format!(
            "Invalid directory reference '{name}': expected $N/component format"
        ))
    })?;
    let dir = dir_index.get(&idx).ok_or_else(|| {
        SnapshotError::Validation(format!(
            "Invalid directory reference '{name}': index {idx} not found"
        ))
    })?;
    Ok(format!("{dir}/{component}"))
}
