// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Fuzz snapshot manifest operations on attacker-controlled decoded manifests.
//!
//! `snapshot_decode` only exercises decode + `validate`. The manifest
//! *operations* — diffing, composing, partitioning, subtree extraction,
//! filtering — consume decoded manifests and do their own merging, size
//! accounting, and path arithmetic over that data. A malformed-but-decodable
//! manifest could drive those into a panic that `validate()` alone wouldn't
//! catch. This target decodes two absolute snapshots from the fuzz input and
//! runs the operations typed for them.
//!
//! Every operation MUST return `Ok`/`Err` (or a value) — never panic, abort,
//! or hang.

#![no_main]

use libfuzzer_sys::fuzz_target;
use openjd_snapshots::{
    compose_diffs, decode_manifest, diff_snapshots, filter_manifest, partition_manifest,
    subtree_manifest, AbsManifest, DecodedManifest, DiffOptions, PartitionOptions, SymlinkPolicy,
};

// Two JSON manifest documents separated by a NUL, so the binary op (diff) gets
// two independently-mutated inputs.
fuzz_target!(|data: &[u8]| {
    let (a_bytes, b_bytes) = match data.iter().position(|&b| b == 0) {
        Some(i) => (&data[..i], &data[i + 1..]),
        None => (data, &b""[..]),
    };
    let (Ok(a_str), Ok(b_str)) = (std::str::from_utf8(a_bytes), std::str::from_utf8(b_bytes))
    else {
        return;
    };

    // The ops below are typed for absolute full snapshots (`AbsSnapshot`).
    // Other decoded shapes still exercised the decoder; here we keep only the
    // variant the ops accept.
    let decode_abs = |s: &str| match decode_manifest(s) {
        Ok(DecodedManifest::AbsSnapshot(m)) => Some(m),
        _ => None,
    };

    if let Some(a) = decode_abs(a_str) {
        // Unary ops on a single decoded snapshot.
        let _ = partition_manifest(&a, &PartitionOptions::default());
        let _ = filter_manifest(&a, &|_entry| true);
        // subtree_manifest takes an `AbsManifest` wrapper enum. Cloning is
        // fine — the point is to exercise the traversal/path arithmetic.
        let _ = subtree_manifest(&AbsManifest::Snapshot(a.clone()), "some/dir", SymlinkPolicy::Preserve);

        if let Some(b) = decode_abs(b_str) {
            // Binary op: diff two decoded snapshots, then compose the result
            // back — a round-trip through the diff/compose arithmetic.
            if let Ok(d) = diff_snapshots(&a, &b, &DiffOptions::default()) {
                let _ = compose_diffs(&[&d]);
            }
        }
    }
});
