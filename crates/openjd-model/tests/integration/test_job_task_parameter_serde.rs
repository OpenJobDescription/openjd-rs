// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Wire-format tests for `job::TaskParameter` (see `specs/model/job-types.md`).
//!
//! `TaskParameter::Int` used to carry `chunks: Option<ResolvedChunks>`, which
//! nothing set and which serialized as `"chunks": null` on every `int`
//! parameter. A chunked parameter is `chunkInt` only. These tests pin that a
//! document written before the field was removed still reads back, and that an
//! `int` parameter holding a chunk configuration is rejected rather than read
//! as an unchunked parameter.

use openjd_model::job::{StepParameterSpace, TaskParamRange, TaskParameter};
use openjd_model::StepParameterSpaceIterator;

/// The error for an `int` parameter that carries a chunk configuration.
const INT_WITH_CHUNKS: &str =
    "an `int` task parameter cannot have `chunks`; a chunked parameter is `chunkInt` (CHUNK[INT])";

#[test]
fn int_serializes_without_chunks() {
    let param = TaskParameter::Int {
        range: TaskParamRange::List(vec![1, 2]),
    };
    let json = serde_json::to_string(&param).unwrap();
    assert_eq!(json, r#"{"int":{"range":{"list":[1,2]}}}"#);
    let back: TaskParameter = serde_json::from_str(&json).unwrap();
    assert_eq!(back, param);
}

#[test]
fn explicit_null_chunks_on_int_reads_as_int() {
    // Released versions wrote `"chunks": null` for every `int` parameter, so a
    // job or step document they serialized must still read back.
    let param: TaskParameter =
        serde_json::from_str(r#"{"int":{"range":{"list":[1,2,3]},"chunks":null}}"#).unwrap();
    assert_eq!(
        param,
        TaskParameter::Int {
            range: TaskParamRange::List(vec![1, 2, 3]),
        }
    );
}

#[test]
fn chunks_on_int_is_rejected() {
    // Before the field was removed this read as `Int { chunks: Some(..) }`, a
    // second spelling of a chunked parameter that the iterator's chunk metadata
    // did not see. Dropping the key instead would read it as unchunked and lose
    // the configuration without a signal.
    for chunks in [
        r#"{"defaultTaskCount":2,"targetRuntimeSeconds":null,"rangeConstraint":"CONTIGUOUS"}"#,
        // Rejected without being parsed: the value is not a valid chunk
        // configuration, and the error is still the one about the shape.
        r#"{}"#,
        r#"0"#,
    ] {
        let doc = format!(r#"{{"int":{{"range":{{"list":[1,2]}},"chunks":{chunks}}}}}"#);
        let err = serde_json::from_str::<TaskParameter>(&doc).unwrap_err();
        assert_eq!(err.to_string(), INT_WITH_CHUNKS, "chunks = {chunks}");
    }
}

#[test]
fn chunks_on_int_is_rejected_inside_a_parameter_space() {
    let doc = r#"{
        "taskParameterDefinitions": {
            "Frame": {"int": {"range": {"list": [1, 2, 3]},
                      "chunks": {"defaultTaskCount": 2, "rangeConstraint": "NONCONTIGUOUS"}}}
        },
        "combination": null
    }"#;
    let err = serde_json::from_str::<StepParameterSpace>(doc).unwrap_err();
    assert!(err.to_string().starts_with(INT_WITH_CHUNKS), "got: {err}");
}

#[test]
fn chunk_int_round_trips_and_drives_the_chunk_metadata() {
    // The one representation of a chunked parameter reads back and is what the
    // iterator's chunk metadata reports.
    let doc = r#"{
        "taskParameterDefinitions": {
            "Frame": {"chunkInt": {"range": {"list": [1, 2, 3, 4, 5, 6, 7, 8, 9, 10]},
                      "chunks": {"defaultTaskCount": 4, "targetRuntimeSeconds": null,
                                 "rangeConstraint": "CONTIGUOUS"}}}
        },
        "combination": null
    }"#;
    let space: StepParameterSpace = serde_json::from_str(doc).unwrap();
    let back: StepParameterSpace =
        serde_json::from_str(&serde_json::to_string(&space).unwrap()).unwrap();
    assert_eq!(back, space);
    let iter = StepParameterSpaceIterator::new(&space).unwrap();
    assert_eq!(iter.chunks_parameter_name(), Some("Frame"));
    assert_eq!(iter.chunks_default_task_count(), Some(4));
    assert_eq!(iter.len(), 3);
}

#[test]
fn other_unknown_keys_on_int_are_still_ignored() {
    // Only `chunks` changed meaning. Other keys keep serde's default of being
    // ignored, as on every job type without `deny_unknown_fields`.
    let param: TaskParameter =
        serde_json::from_str(r#"{"int":{"range":{"list":[7]},"note":"x"}}"#).unwrap();
    assert_eq!(
        param,
        TaskParameter::Int {
            range: TaskParamRange::List(vec![7]),
        }
    );
}
