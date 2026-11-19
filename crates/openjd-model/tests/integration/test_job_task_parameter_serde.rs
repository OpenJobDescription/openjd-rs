// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Wire-format tests for `job::TaskParameter` (see `specs/model/job-types.md`).
//!
//! A chunked parameter is `chunkInt` only; `TaskParameter::Int` has no
//! `chunks`. The wire form is the one released versions wrote, when `Int` had
//! a never-set `chunks: Option<ResolvedChunks>`: `int` is written with
//! `"chunks": null`, so documents read the same in both directions. These tests pin that form, that a document
//! without the key still reads, and that an `int` parameter holding a chunk
//! configuration is rejected rather than read as an unchunked parameter.

use openjd_model::job::{StepParameterSpace, TaskParamRange, TaskParameter};
use openjd_model::StepParameterSpaceIterator;

/// The error for an `int` parameter that carries a chunk configuration.
const INT_WITH_CHUNKS: &str =
    "an `int` task parameter cannot have `chunks`; a chunked parameter is `chunkInt` (CHUNK[INT])";

#[test]
fn int_serializes_with_null_chunks_as_released_versions_did() {
    let param = TaskParameter::Int {
        range: TaskParamRange::List(vec![1, 2]),
    };
    let json = serde_json::to_string(&param).unwrap();
    assert_eq!(json, r#"{"int":{"range":{"list":[1,2]},"chunks":null}}"#);
    let back: TaskParameter = serde_json::from_str(&json).unwrap();
    assert_eq!(back, param);
}

#[test]
fn every_variant_round_trips_and_keeps_its_wire_form() {
    let chunks =
        r#"{"defaultTaskCount":3,"targetRuntimeSeconds":null,"rangeConstraint":"NONCONTIGUOUS"}"#;
    for doc in [
        r#"{"int":{"range":{"list":[1,2]},"chunks":null}}"#.to_string(),
        r#"{"float":{"range":[1.5,"2.50"]}}"#.to_string(),
        r#"{"string":{"range":["a","b"]}}"#.to_string(),
        r#"{"path":{"range":["/x"]}}"#.to_string(),
        format!(r#"{{"chunkInt":{{"range":{{"list":[1,2,3]}},"chunks":{chunks}}}}}"#),
    ] {
        let param: TaskParameter = serde_json::from_str(&doc).unwrap();
        assert_eq!(serde_json::to_string(&param).unwrap(), doc);
    }
}

#[test]
fn sequence_form_matches_the_released_shape() {
    // A struct variant can also be written as a sequence of its fields, which
    // is how positional formats see it. `int` still has two fields there, so
    // what is written reads back, and a one-field `int` is rejected as before.
    let param: TaskParameter = serde_json::from_str(r#"{"int":[{"list":[4]},null]}"#).unwrap();
    assert_eq!(
        param,
        TaskParameter::Int {
            range: TaskParamRange::List(vec![4]),
        }
    );
    let err = serde_json::from_str::<TaskParameter>(r#"{"int":[{"list":[4]}]}"#).unwrap_err();
    assert_eq!(
        err.to_string(),
        "invalid length 1, expected struct variant TaskParameter::Int with 2 elements at line 1 column 21"
    );
    let err =
        serde_json::from_str::<TaskParameter>(r#"{"int":[{"list":[4]},{"x":1}]}"#).unwrap_err();
    assert_eq!(
        err.to_string(),
        format!("{INT_WITH_CHUNKS} at line 1 column 21")
    );
}

#[test]
fn shape_errors_name_the_public_type() {
    // The wire type is private; serde's messages name `TaskParameter`.
    for (doc, want) in [
        (
            r#"{"float":5}"#,
            "invalid type: integer `5`, expected struct variant TaskParameter::Float at line 1 column 10",
        ),
        (
            r#"{"int":null}"#,
            "invalid type: null, expected struct variant TaskParameter::Int at line 1 column 11",
        ),
        (
            r#"{"chunkInt":{"range":{"list":[1]}}}"#,
            "missing field `chunks` at line 1 column 34",
        ),
    ] {
        let err = serde_json::from_str::<TaskParameter>(doc).unwrap_err();
        assert_eq!(err.to_string(), want, "{doc}");
    }
}

#[test]
fn chunks_on_int_is_rejected() {
    // A chunk configuration on `int` is not a chunked parameter, which is
    // `chunkInt`. Dropping the key would read it as unchunked and lose the
    // configuration without a signal, so it is an error.
    for chunks in [
        r#"{"defaultTaskCount":2,"targetRuntimeSeconds":null,"rangeConstraint":"CONTIGUOUS"}"#,
        // Rejected without being read: these are not valid chunk configurations,
        // and the error is still the one about the shape.
        r#"{}"#,
        r#"0"#,
    ] {
        let doc = format!(r#"{{"int":{{"range":{{"list":[1,2]}},"chunks":{chunks}}}}}"#);
        let err = serde_json::from_str::<TaskParameter>(&doc).unwrap_err();
        // The position is where the `chunks` value starts.
        assert_eq!(
            err.to_string(),
            format!("{INT_WITH_CHUNKS} at line 1 column 40"),
            "chunks = {chunks}"
        );
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
    assert_eq!(
        err.to_string(),
        format!("{INT_WITH_CHUNKS} at line 4 column 32")
    );
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

/// The serde calls that formats see but JSON hides: the type name, the
/// variant index and the field count. Positional formats encode the index, so
/// these must stay as the released derive made them.
mod wire_shape {
    use serde::de::{self, Deserializer, EnumAccess, IntoDeserializer, VariantAccess, Visitor};
    use serde::ser::{self, Impossible, Serialize, Serializer};
    use std::fmt;

    /// The error both probes stop with, carrying what they saw.
    #[derive(Debug)]
    pub struct Seen(pub String);
    impl fmt::Display for Seen {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str(&self.0)
        }
    }
    impl std::error::Error for Seen {}
    impl ser::Error for Seen {
        fn custom<T: fmt::Display>(msg: T) -> Self {
            Seen(msg.to_string())
        }
    }
    impl de::Error for Seen {
        fn custom<T: fmt::Display>(msg: T) -> Self {
            Seen(msg.to_string())
        }
    }

    /// Records the `serialize_struct_variant` call a value makes, as
    /// `name/index/variant/fields`. Any other top-level call is an error.
    pub fn struct_variant_of<T: Serialize>(value: &T) -> String {
        match value.serialize(StructVariantOf) {
            Ok(()) => unreachable!("StructVariantOf never succeeds"),
            Err(Seen(s)) => s,
        }
    }

    struct StructVariantOf;

    macro_rules! not_a_struct_variant {
        ($($method:ident($($arg:ty),*) -> $ret:ty;)*) => {$(
            fn $method(self, $(_: $arg),*) -> Result<$ret, Seen> {
                Err(Seen(concat!("not a struct variant: ", stringify!($method)).into()))
            }
        )*};
    }

    impl Serializer for StructVariantOf {
        type Ok = ();
        type Error = Seen;
        type SerializeSeq = Impossible<(), Seen>;
        type SerializeTuple = Impossible<(), Seen>;
        type SerializeTupleStruct = Impossible<(), Seen>;
        type SerializeTupleVariant = Impossible<(), Seen>;
        type SerializeMap = Impossible<(), Seen>;
        type SerializeStruct = Impossible<(), Seen>;
        type SerializeStructVariant = Impossible<(), Seen>;

        not_a_struct_variant! {
            serialize_bool(bool) -> ();
            serialize_i8(i8) -> ();
            serialize_i16(i16) -> ();
            serialize_i32(i32) -> ();
            serialize_i64(i64) -> ();
            serialize_u8(u8) -> ();
            serialize_u16(u16) -> ();
            serialize_u32(u32) -> ();
            serialize_u64(u64) -> ();
            serialize_f32(f32) -> ();
            serialize_f64(f64) -> ();
            serialize_char(char) -> ();
            serialize_str(&str) -> ();
            serialize_bytes(&[u8]) -> ();
            serialize_none() -> ();
            serialize_unit() -> ();
            serialize_unit_struct(&'static str) -> ();
            serialize_unit_variant(&'static str, u32, &'static str) -> ();
            serialize_seq(Option<usize>) -> Self::SerializeSeq;
            serialize_tuple(usize) -> Self::SerializeTuple;
            serialize_tuple_struct(&'static str, usize) -> Self::SerializeTupleStruct;
            serialize_tuple_variant(&'static str, u32, &'static str, usize)
                -> Self::SerializeTupleVariant;
            serialize_map(Option<usize>) -> Self::SerializeMap;
            serialize_struct(&'static str, usize) -> Self::SerializeStruct;
        }

        fn serialize_some<T: ?Sized + Serialize>(self, _: &T) -> Result<(), Seen> {
            Err(Seen("not a struct variant: serialize_some".into()))
        }
        fn serialize_newtype_struct<T: ?Sized + Serialize>(
            self,
            _: &'static str,
            _: &T,
        ) -> Result<(), Seen> {
            Err(Seen(
                "not a struct variant: serialize_newtype_struct".into(),
            ))
        }
        fn serialize_newtype_variant<T: ?Sized + Serialize>(
            self,
            _: &'static str,
            _: u32,
            _: &'static str,
            _: &T,
        ) -> Result<(), Seen> {
            Err(Seen(
                "not a struct variant: serialize_newtype_variant".into(),
            ))
        }
        fn serialize_struct_variant(
            self,
            name: &'static str,
            index: u32,
            variant: &'static str,
            len: usize,
        ) -> Result<Self::SerializeStructVariant, Seen> {
            Err(Seen(format!("{name}/{index}/{variant}/{len}")))
        }
    }

    /// Deserializes `T` from an enum whose variant is given by index, as a
    /// positional format does, and reports the struct variant and fields that
    /// index selects (or the error for an index out of range).
    pub fn struct_variant_at<T: de::DeserializeOwned>(index: u32) -> String {
        match T::deserialize(VariantAt(index)) {
            Ok(_) => unreachable!("VariantAt never succeeds"),
            Err(Seen(s)) => s,
        }
    }

    struct VariantAt(u32);

    impl<'de> Deserializer<'de> for VariantAt {
        type Error = Seen;
        fn deserialize_any<V: Visitor<'de>>(self, _: V) -> Result<V::Value, Seen> {
            Err(Seen("not an enum".into()))
        }
        fn deserialize_enum<V: Visitor<'de>>(
            self,
            name: &'static str,
            variants: &'static [&'static str],
            visitor: V,
        ) -> Result<V::Value, Seen> {
            // Formats that write the type name or the variant names read them here.
            visitor
                .visit_enum(self)
                .map_err(|Seen(s)| Seen(format!("{name} {variants:?} {s}")))
        }
        serde::forward_to_deserialize_any! {
            bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str string
            bytes byte_buf option unit unit_struct newtype_struct seq tuple
            tuple_struct map struct identifier ignored_any
        }
    }

    impl<'de> EnumAccess<'de> for VariantAt {
        type Error = Seen;
        type Variant = Self;
        fn variant_seed<S: de::DeserializeSeed<'de>>(
            self,
            seed: S,
        ) -> Result<(S::Value, Self), Seen> {
            let variant = seed.deserialize(self.0.into_deserializer())?;
            Ok((variant, self))
        }
    }

    impl<'de> VariantAccess<'de> for VariantAt {
        type Error = Seen;
        fn unit_variant(self) -> Result<(), Seen> {
            Err(Seen("unit variant".into()))
        }
        fn newtype_variant_seed<S: de::DeserializeSeed<'de>>(self, _: S) -> Result<S::Value, Seen> {
            Err(Seen("newtype variant".into()))
        }
        fn tuple_variant<V: Visitor<'de>>(self, _: usize, _: V) -> Result<V::Value, Seen> {
            Err(Seen("tuple variant".into()))
        }
        fn struct_variant<V: Visitor<'de>>(
            self,
            fields: &'static [&'static str],
            visitor: V,
        ) -> Result<V::Value, Seen> {
            // The visitor's `expecting` names the variant it was built for.
            let err: Seen = de::Error::invalid_type(de::Unexpected::Unit, &visitor);
            Err(Seen(format!("{fields:?}: {err}")))
        }
    }
}

#[test]
fn positional_wire_shape_is_the_released_one() {
    // The released derive's order: Int, Float, String, Path, ChunkInt, with
    // `int` carrying two fields (`range`, `chunks`).
    let chunks = openjd_model::job::ResolvedChunks {
        default_task_count: 1,
        target_runtime_seconds: None,
        range_constraint: openjd_model::template::RangeConstraint::Contiguous,
    };
    let cases = [
        (
            TaskParameter::Int {
                range: TaskParamRange::List(vec![1]),
            },
            "TaskParameter/0/int/2",
            r#"TaskParameter ["int", "float", "string", "path", "chunkInt"] ["range", "chunks"]: invalid type: unit value, expected struct variant TaskParameter::Int"#,
        ),
        (
            TaskParameter::Float { range: vec![] },
            "TaskParameter/1/float/1",
            r#"TaskParameter ["int", "float", "string", "path", "chunkInt"] ["range"]: invalid type: unit value, expected struct variant TaskParameter::Float"#,
        ),
        (
            TaskParameter::String { range: vec![] },
            "TaskParameter/2/string/1",
            r#"TaskParameter ["int", "float", "string", "path", "chunkInt"] ["range"]: invalid type: unit value, expected struct variant TaskParameter::String"#,
        ),
        (
            TaskParameter::Path { range: vec![] },
            "TaskParameter/3/path/1",
            r#"TaskParameter ["int", "float", "string", "path", "chunkInt"] ["range"]: invalid type: unit value, expected struct variant TaskParameter::Path"#,
        ),
        (
            TaskParameter::ChunkInt {
                range: TaskParamRange::List(vec![1]),
                chunks,
            },
            "TaskParameter/4/chunkInt/2",
            r#"TaskParameter ["int", "float", "string", "path", "chunkInt"] ["range", "chunks"]: invalid type: unit value, expected struct variant TaskParameter::ChunkInt"#,
        ),
    ];
    for (index, (param, written, read)) in (0u32..).zip(cases) {
        assert_eq!(wire_shape::struct_variant_of(&param), written);
        assert_eq!(
            wire_shape::struct_variant_at::<TaskParameter>(index),
            read,
            "variant index {index}"
        );
    }
    assert_eq!(
        wire_shape::struct_variant_at::<TaskParameter>(5),
        r#"TaskParameter ["int", "float", "string", "path", "chunkInt"] invalid value: integer `5`, expected variant index 0 <= i < 5"#
    );
}
