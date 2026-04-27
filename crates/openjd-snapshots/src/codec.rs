// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Encode/decode manifests to/from on-disk v2023 and v2025 JSON formats.

use crate::hash::HashAlgorithm;
use crate::manifest::{
    AbsSnapshot, AbsSnapshotDiff, DirEntry, FileEntry, Manifest, Snapshot, SnapshotDiff,
    SymlinkPolicy,
};
use crate::ops::subtree_rel_snapshot;
use crate::{Result, SnapshotError, DEFAULT_FILE_CHUNK_SIZE, WHOLE_FILE_CHUNK_SIZE};
use serde_json::Value;
use std::collections::HashMap;
use tracing::warn;

// --- Public types ---

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ManifestFormat {
    V2023,
    V2025,
}

#[derive(Debug)]
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

/// Encode a Snapshot to v2023 JSON.
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
        "totalSize": collapsed.total_size,
    });

    // sort_keys + no whitespace = canonical JSON
    Ok(canonical_json(&obj))
}

/// Encode a SnapshotDiff to v2023 JSON (drops deletions).
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
    let total_size: u64 = paths.iter().map(|p| p["size"].as_u64().unwrap_or(0)).sum();

    let obj = serde_json::json!({
        "hashAlg": hash_alg_str(collapsed.hash_alg),
        "manifestVersion": V2023_MANIFEST_VERSION,
        "paths": paths,
        "totalSize": total_size,
    });

    Ok(canonical_json(&obj))
}

/// Encode any manifest to v2025 JSON.
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
pub fn encode_abs_snapshot_v2025(m: &AbsSnapshot) -> Result<String> {
    encode_v2025(m, SPEC_ABS_SNAPSHOT)
}

/// Encode an absolute snapshot diff to v2025 JSON.
pub fn encode_abs_snapshot_diff_v2025(m: &AbsSnapshotDiff) -> Result<String> {
    encode_v2025(m, SPEC_ABS_DIFF)
}

/// Encode a relative snapshot to v2025 JSON.
pub fn encode_snapshot_v2025(m: &Snapshot) -> Result<String> {
    validate_symlink_targets_relative(&m.files)?;
    encode_v2025(m, SPEC_REL_SNAPSHOT)
}

/// Encode a relative snapshot diff to v2025 JSON.
pub fn encode_snapshot_diff_v2025(m: &SnapshotDiff) -> Result<String> {
    validate_symlink_targets_relative(&m.files)?;
    encode_v2025(m, SPEC_REL_DIFF)
}

// --- Decode ---

/// Auto-detect format and decode.
pub fn decode_manifest(json: &str) -> Result<DecodedManifest> {
    let data: Value = serde_json::from_str(json)
        .map_err(|e| SnapshotError::Validation(format!("Invalid JSON: {e}")))?;

    if data.get("manifestVersion").is_some() {
        let snap = decode_v2023(json)?;
        Ok(DecodedManifest::Snapshot(snap))
    } else if data.get("specificationVersion").is_some() {
        decode_v2025(json)
    } else {
        Err(SnapshotError::Validation(
            "Unknown manifest format: no manifestVersion or specificationVersion".into(),
        ))
    }
}

/// Decode v2023 JSON to Snapshot.
pub fn decode_v2023(json: &str) -> Result<Snapshot> {
    let data: Value = serde_json::from_str(json)
        .map_err(|e| SnapshotError::Validation(format!("Invalid JSON: {e}")))?;

    let version = data["manifestVersion"]
        .as_str()
        .ok_or_else(|| SnapshotError::Validation("missing manifestVersion".into()))?;
    if version != V2023_MANIFEST_VERSION {
        return Err(SnapshotError::Validation(format!(
            "expected manifestVersion '{V2023_MANIFEST_VERSION}', got '{version}'"
        )));
    }

    let hash_alg = parse_hash_alg(data["hashAlg"].as_str())?;
    let total_size = data["totalSize"]
        .as_u64()
        .ok_or_else(|| SnapshotError::Validation("missing or invalid totalSize".into()))?;

    let raw_paths = data["paths"]
        .as_array()
        .ok_or_else(|| SnapshotError::Validation("missing or invalid paths".into()))?;

    let mut files = Vec::with_capacity(raw_paths.len());
    for p in raw_paths {
        let path = p["path"]
            .as_str()
            .ok_or_else(|| SnapshotError::Validation("path entry missing 'path'".into()))?;
        let hash = p["hash"]
            .as_str()
            .ok_or_else(|| SnapshotError::Validation("path entry missing 'hash'".into()))?;
        let size = p["size"]
            .as_u64()
            .ok_or_else(|| SnapshotError::Validation("path entry missing 'size'".into()))?;
        let mtime = p["mtime"]
            .as_u64()
            .ok_or_else(|| SnapshotError::Validation("path entry missing 'mtime'".into()))?;

        let mut entry = FileEntry::file(path, size, mtime);
        entry.hash = Some(hash.to_string());
        files.push(entry);
    }

    let mut m: Snapshot = Manifest::new(hash_alg, WHOLE_FILE_CHUNK_SIZE);
    m.files = files;
    m.total_size = total_size;
    m.validate()?;
    Ok(m)
}

/// Decode v2025 JSON to the appropriate manifest type.
pub fn decode_v2025(json: &str) -> Result<DecodedManifest> {
    let data: Value = serde_json::from_str(json)
        .map_err(|e| SnapshotError::Validation(format!("Invalid JSON: {e}")))?;

    let spec = data["specificationVersion"]
        .as_str()
        .ok_or_else(|| SnapshotError::Validation("missing specificationVersion".into()))?;

    let hash_alg = parse_hash_alg(data["hashAlg"].as_str())?;
    let total_size = data["totalSize"]
        .as_u64()
        .ok_or_else(|| SnapshotError::Validation("missing or invalid totalSize".into()))?;
    let parent_hash = data
        .get("parentManifestHash")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    // Build dir_index
    let raw_dirs = data.get("dirs").and_then(|v| v.as_array());
    let dir_index = build_dir_index(raw_dirs)?;

    // Decode dirs
    let dirs = decode_dirs(raw_dirs, &dir_index)?;

    // Decode files
    let raw_files = data.get("files").and_then(|v| v.as_array());
    let files = decode_files(raw_files, &dir_index)?;

    let file_chunk_size_bytes = data
        .get("fileChunkSizeBytes")
        .and_then(|v| v.as_i64())
        .unwrap_or(DEFAULT_FILE_CHUNK_SIZE);

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

fn build_dir_index(raw_dirs: Option<&Vec<Value>>) -> Result<HashMap<usize, String>> {
    let mut index: HashMap<usize, String> = HashMap::new();
    if let Some(dirs) = raw_dirs {
        for (i, d) in dirs.iter().enumerate() {
            let name = d["name"]
                .as_str()
                .ok_or_else(|| SnapshotError::Validation("dir entry missing 'name'".into()))?;
            let expanded = expand_path_reference(name, &index)?;
            index.insert(i, expanded);
        }
    }
    Ok(index)
}

fn decode_dirs(
    raw_dirs: Option<&Vec<Value>>,
    dir_index: &HashMap<usize, String>,
) -> Result<Vec<DirEntry>> {
    let mut result = Vec::new();
    if let Some(dirs) = raw_dirs {
        for (i, d) in dirs.iter().enumerate() {
            let path = dir_index
                .get(&i)
                .ok_or_else(|| SnapshotError::Validation("dir index mismatch".into()))?;
            let deleted = d.get("delete").and_then(|v| v.as_bool()).unwrap_or(false);
            let mut entry = DirEntry::new(path);
            entry.deleted = deleted;
            result.push(entry);
        }
    }
    Ok(result)
}

fn decode_files(
    raw_files: Option<&Vec<Value>>,
    dir_index: &HashMap<usize, String>,
) -> Result<Vec<FileEntry>> {
    let mut result = Vec::new();
    if let Some(files) = raw_files {
        for f in files {
            let name = f["name"]
                .as_str()
                .ok_or_else(|| SnapshotError::Validation("file entry missing 'name'".into()))?;
            let path = expand_path_reference(name, dir_index)?;

            let hash = f
                .get("hash")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let chunk_hashes = f.get("chunkhashes").and_then(|v| v.as_array()).map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect::<Vec<_>>()
            });
            let symlink_target = f
                .get("symlink")
                .and_then(|v| v.get("name"))
                .and_then(|v| v.as_str())
                .map(|s| expand_path_reference(s, dir_index))
                .transpose()?;

            let size = f.get("size").and_then(|v| v.as_u64());
            let mtime = f.get("mtime").and_then(|v| v.as_u64());
            let runnable = f.get("runnable").and_then(|v| v.as_bool()).unwrap_or(false);
            let deleted = f.get("delete").and_then(|v| v.as_bool()).unwrap_or(false);

            let mut entry = FileEntry::new(&path);
            entry.hash = hash;
            entry.size = size;
            entry.mtime = mtime;
            entry.chunk_hashes = chunk_hashes;
            entry.symlink_target = symlink_target;
            entry.runnable = runnable;
            entry.deleted = deleted;
            result.push(entry);
        }
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
