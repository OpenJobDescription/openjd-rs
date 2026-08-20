// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Fuzz snapshot manifest decoding + validation.
//!
//! `decode_manifest` auto-detects the manifest format (v2023 / v2025 absolute
//! or relative, snapshot or diff) from an untrusted JSON string and
//! deserializes it into a `Manifest`. The `file_chunk_size_bytes` guard
//! (reject zero / invalid negative) and the `total_size` / path invariants are
//! enforced by `Manifest::validate()`. Decoding attacker-controlled manifest
//! JSON MUST NOT panic (e.g. a div-by-zero or overflow in chunk math), and
//! validating a decoded manifest MUST return `Err` on invariant violations
//! rather than aborting.

#![no_main]

use libfuzzer_sys::fuzz_target;
use openjd_snapshots::{decode_manifest, DecodedManifest};

fuzz_target!(|data: &[u8]| {
    let Ok(json) = std::str::from_utf8(data) else {
        return;
    };

    // decode_manifest returns the version/kind-appropriate decoded form, each
    // of which is a `Manifest<P, K>` type alias exposing `validate()`. A parse
    // or schema error is an expected outcome; only a panic/abort is a finding.
    let result = decode_manifest(json);
    match result {
        Ok(DecodedManifest::AbsSnapshot(m)) => {
            let _ = m.validate();
        }
        Ok(DecodedManifest::AbsSnapshotDiff(m)) => {
            let _ = m.validate();
        }
        Ok(DecodedManifest::Snapshot(m)) => {
            let _ = m.validate();
        }
        Ok(DecodedManifest::SnapshotDiff(m)) => {
            let _ = m.validate();
        }
        Err(_) => {}
    }
});
