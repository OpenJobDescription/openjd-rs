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
//!   [`decode_v2023`], [`decode_v2023_as_diff`].
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
    AbsSnapshot, AbsSnapshotDiff, DirEntry, FileEntry, Manifest, Rel, Snapshot, SnapshotDiff,
    SymlinkPolicy, ValidateKind,
};
use crate::ops::subtree_rel_snapshot;
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

/// Validates that all symlink targets in a relative manifest are relative paths.
fn validate_symlink_targets_relative(files: &[FileEntry]) -> Result<()> {
    for f in files {
        if let Some(ref target) = f.symlink_target {
            if crate::path_util::is_absolute_path(target) {
                return Err(SnapshotError::Validation(format!(
                    "symlink '{}' target must be a relative path, got absolute: '{}'",
                    f.path, target
                )));
            }
        }
    }
    Ok(())
}

/// Sum the `size` fields of encoded v2023 path entries.
///
/// Checked rather than saturating: sizes are caller data, and a clamped
/// `totalSize` would disagree with the entries. Python writes the exact
/// (arbitrary-precision) sum instead; Rust's `u64` sizes cannot, so this is
/// an intentional divergence (see `specs/snapshots/public-api.md`).
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
/// if a file is missing a hash, a symlink target is absolute, the chunk size
/// is not whole-file, or the file sizes sum past `u64::MAX`.
pub fn encode_snapshot_v2023(manifest: &Snapshot) -> Result<String> {
    validate_symlink_targets_relative(&manifest.files)?;
    if manifest.file_chunk_size_bytes != WHOLE_FILE_CHUNK_SIZE {
        return Err(SnapshotError::Validation(format!(
            "v2023 format requires fileChunkSizeBytes={WHOLE_FILE_CHUNK_SIZE}, got {}",
            manifest.file_chunk_size_bytes
        )));
    }

    // Collapse symlinks via identity subtree
    let collapsed = subtree_rel_snapshot(manifest, ".", SymlinkPolicy::CollapseAll)?;

    // Warn about empty directories being dropped (v2023 has no directory support)
    if !collapsed.dirs.is_empty() {
        let implied: std::collections::HashSet<String> = collapsed
            .files
            .iter()
            .filter(|f| !f.deleted)
            .filter_map(|f| f.path.rfind('/').map(|i| f.path[..i].to_string()))
            .collect();
        let empty_count = collapsed
            .dirs
            .iter()
            .filter(|d| !implied.contains(&d.path))
            .count();
        if empty_count > 0 {
            warn!(
                count = empty_count,
                "dropping empty directories not supported in v2023 format"
            );
        }
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

    let obj = serde_json::json!({
        "hashAlg": hash_alg_str(collapsed.hash_alg),
        "manifestVersion": V2023_MANIFEST_VERSION,
        "paths": paths,
        "totalSize": v2023_total_size(&paths)?,
    });

    // sort_keys + no whitespace = canonical JSON
    Ok(canonical_json(&obj))
}

/// Encode a SnapshotDiff to v2023 JSON (drops deletions). Fails with
/// `SnapshotError::Validation` under the same conditions as
/// [`encode_snapshot_v2023`], including file sizes that sum past `u64::MAX`.
pub fn encode_snapshot_diff_v2023(manifest: &SnapshotDiff) -> Result<String> {
    validate_symlink_targets_relative(&manifest.files)?;
    if manifest.file_chunk_size_bytes != WHOLE_FILE_CHUNK_SIZE {
        return Err(SnapshotError::Validation(format!(
            "v2023 format requires fileChunkSizeBytes={WHOLE_FILE_CHUNK_SIZE}, got {}",
            manifest.file_chunk_size_bytes
        )));
    }

    // Build a Snapshot from non-deleted files for symlink collapsing
    let mut snap: Snapshot = Manifest::new(manifest.hash_alg, manifest.file_chunk_size_bytes);
    snap.files = manifest.files.clone();
    snap.dirs = manifest.dirs.clone();
    snap.total_size = manifest.total_size;

    let collapsed = subtree_rel_snapshot(&snap, ".", SymlinkPolicy::CollapseAll)?;

    // Warn about deletions being dropped (v2023 has no deletion support)
    let deleted_count = collapsed.files.iter().filter(|f| f.deleted).count()
        + collapsed.dirs.iter().filter(|d| d.deleted).count();
    if deleted_count > 0 {
        warn!(
            count = deleted_count,
            "dropping deletions not supported in v2023 format"
        );
    }

    // Warn about empty directories being dropped
    if !collapsed.dirs.is_empty() {
        let implied: std::collections::HashSet<String> = collapsed
            .files
            .iter()
            .filter(|f| !f.deleted)
            .filter_map(|f| f.path.rfind('/').map(|i| f.path[..i].to_string()))
            .collect();
        let empty_count = collapsed
            .dirs
            .iter()
            .filter(|d| !d.deleted && !implied.contains(&d.path))
            .count();
        if empty_count > 0 {
            warn!(
                count = empty_count,
                "dropping empty directories not supported in v2023 format"
            );
        }
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

    paths.sort_by(|a, b| {
        let pa = a["path"].as_str().unwrap();
        let pb = b["path"].as_str().unwrap();
        utf16_be_bytes(pa).cmp(&utf16_be_bytes(pb))
    });

    // Recompute total_size from non-deleted files only
    let total_size = v2023_total_size(&paths)?;

    let obj = serde_json::json!({
        "hashAlg": hash_alg_str(collapsed.hash_alg),
        "manifestVersion": V2023_MANIFEST_VERSION,
        "paths": paths,
        "totalSize": total_size,
    });

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
        "hashAlg": hash_alg_str(manifest.hash_alg),
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
    validate_symlink_targets_relative(&m.files)?;
    encode_v2025(m, SPEC_REL_SNAPSHOT)
}

/// Encode a relative snapshot diff to v2025 JSON.
///
/// **Experimental draft format.** See the [module-level docs](self).
pub fn encode_snapshot_diff_v2025(m: &SnapshotDiff) -> Result<String> {
    validate_symlink_targets_relative(&m.files)?;
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

/// What [`decode_v2023_for_each`] read from a v2023 manifest besides its
/// entries.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct V2023Summary {
    /// The manifest's `hashAlg`.
    pub hash_alg: HashAlgorithm,
    /// The manifest's `totalSize`, as written (it is not recomputed).
    pub total_size: u64,
    /// The number of entries passed to the callback.
    pub file_count: u64,
}

/// Decode v2023 JSON one entry at a time, without building the manifest.
///
/// Each element of `paths` is turned into a [`FileEntry`] and passed to `f`,
/// with the manifest's hash algorithm, as soon as it is parsed. Memory use
/// beyond `json` itself stays constant, whatever the number of entries. On
/// success, returns the manifest's [`V2023Summary`].
///
/// To stop early, return an error from `f`; it is returned unchanged. Decode
/// errors are converted into `E` with `From<SnapshotError>`, so `E` can be
/// [`SnapshotError`] itself, or an application error type.
///
/// Values are read as [`decode_v2023`] reads them, entries are checked the
/// same way, and errors have the same messages, with these differences:
///
/// - `manifestVersion` and `hashAlg` must come before `paths`, as every
///   v2023 writer puts them (keys are sorted). Otherwise the error is
///   `manifestVersion and hashAlg must come before paths`. A repeated `paths`
///   key is an error (`paths appears more than once`) rather than replacing
///   the earlier array.
/// - Entries are delivered before the rest of the document is read. If the
///   function returns an error (invalid JSON later in the text, a bad entry,
///   a missing `totalSize`), the entries already delivered came from a
///   manifest that [`decode_v2023`] would reject, and should be discarded.
/// - Errors are reported in document order: a bad entry is reported before a
///   missing `totalSize`, which comes after `paths`.
/// - Paths are not checked for duplicates, since that needs every path at
///   once. [`decode_v2023`] rejects a manifest that lists a path twice.
///
/// # Example
///
/// ```
/// use openjd_snapshots::{decode_v2023_for_each, SnapshotError};
///
/// let json = r#"{"hashAlg":"xxh128","manifestVersion":"2023-03-03","paths":[
///     {"hash":"a1","mtime":1,"path":"a.txt","size":3},
///     {"hash":"b2","mtime":1,"path":"b.txt","size":4}],"totalSize":7}"#;
///
/// let mut keys = Vec::new();
/// let summary = decode_v2023_for_each(json, |alg, entry| {
///     let hash = entry.hash.expect("v2023 entries always have a hash");
///     keys.push(format!("Data/{hash}.{alg}"));
///     Ok::<_, SnapshotError>(())
/// })?;
/// assert_eq!(keys, ["Data/a1.xxh128", "Data/b2.xxh128"]);
/// assert_eq!(summary.total_size, 7);
/// # Ok::<_, SnapshotError>(())
/// ```
pub fn decode_v2023_for_each<F, E>(json: &str, f: F) -> std::result::Result<V2023Summary, E>
where
    F: FnMut(HashAlgorithm, FileEntry) -> std::result::Result<(), E>,
    E: From<SnapshotError>,
{
    stream::decode(serde_json::de::StrRead::new(json), f)
}

/// [`decode_v2023_for_each`], reading the JSON from `reader`, so the text is
/// never held in memory as a whole.
///
/// The reader is read a byte at a time, so pass a buffered one, such as a
/// [`std::io::BufReader`]. A read error is returned as [`SnapshotError::Io`].
/// Text after the manifest's closing `}` is read until the end of input, and
/// anything but whitespace there is an error, as it is for
/// [`decode_v2023_for_each`]. A syntax error's reported column can be one
/// past the one [`decode_v2023_for_each`] reports, as serde_json counts
/// positions differently when reading from a reader.
pub fn decode_v2023_for_each_from_reader<R, F, E>(
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
    v2023_version(raw.manifest_version.as_deref())?;
    let hash_alg = parse_hash_alg(raw.hash_alg.as_deref())?;
    let total_size = v2023_read_total_size(raw.total_size)?;
    let raw_paths = raw.paths.ok_or_else(v2023_invalid_paths)?;

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
fn v2023_version(manifest_version: Option<&str>) -> Result<()> {
    let version = manifest_version
        .ok_or_else(|| SnapshotError::Validation("missing manifestVersion".into()))?;
    if version != V2023_MANIFEST_VERSION {
        return Err(SnapshotError::Validation(format!(
            "expected manifestVersion '{V2023_MANIFEST_VERSION}', got '{version}'"
        )));
    }
    Ok(())
}

fn v2023_read_total_size(total_size: Option<u64>) -> Result<u64> {
    total_size.ok_or_else(|| SnapshotError::Validation("missing or invalid totalSize".into()))
}

fn v2023_invalid_paths() -> SnapshotError {
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

fn hash_alg_str(alg: HashAlgorithm) -> &'static str {
    match alg {
        HashAlgorithm::Xxh128 => "xxh128",
    }
}

fn parse_hash_alg(s: Option<&str>) -> Result<HashAlgorithm> {
    match s {
        Some("xxh128") => Ok(HashAlgorithm::Xxh128),
        Some(other) => Err(SnapshotError::Validation(format!(
            "Unsupported hash algorithm: {other}"
        ))),
        None => Err(SnapshotError::Validation("missing hashAlg".into())),
    }
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

    let mut inferred: std::collections::HashSet<String> = std::collections::HashSet::new();
    for path in &all_paths {
        extract_parent_dirs(path, &mut inferred);
    }

    let mut result: Vec<DirEntry> = dirs.to_vec();
    for dir_path in inferred {
        if !explicit.contains_key(dir_path.as_str()) {
            result.push(DirEntry::new(&dir_path));
        }
    }
    result
}

fn extract_parent_dirs(path: &str, result: &mut std::collections::HashSet<String>) {
    let mut last_slash = path.rfind('/');
    let mut current = path;
    while let Some(pos) = last_slash {
        if pos == 0 {
            break;
        }
        let parent = &current[..pos];
        if !result.insert(parent.to_string()) {
            break;
        }
        current = parent;
        last_slash = current.rfind('/');
    }
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
