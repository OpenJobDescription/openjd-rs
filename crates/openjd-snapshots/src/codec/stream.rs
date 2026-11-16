// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Streaming decode of a v2023 manifest.
//!
//! The top-level object is read member by member. When `paths` arrives, the
//! header read so far is checked, and then each element of the array is
//! turned into a [`FileEntry`] and handed to the caller's callback as soon as
//! it is parsed. Nothing proportional to the number of entries is kept.
//!
//! Values are read with the same rules as the batch decoders (see `raw`):
//! a value of the wrong type is absent, ignored members are still fully
//! parsed. Errors from the checks and from the callback stop the parse at
//! once; they are kept in a slot beside the deserializer and returned in
//! place of the placeholder error the deserializer reports.

use super::raw::{self, RawV2023Path, Read};
use super::{
    parse_hash_alg, v2023_file_entry, v2023_invalid_paths, v2023_read_total_size, v2023_version,
    V2023Summary,
};
use crate::hash::HashAlgorithm;
use crate::manifest::{FileEntry, Snapshot};
use crate::SnapshotError;
use serde::de::{self, DeserializeSeed, Deserializer, MapAccess, SeqAccess, Visitor};
use std::fmt;

/// The error for a header key that has not appeared by the time `paths` does.
fn missing_before_paths(key: &str) -> SnapshotError {
    SnapshotError::Validation(format!("missing {key} before paths"))
}

/// The error for a header key that appears after `paths`.
fn after_paths(key: &str) -> SnapshotError {
    SnapshotError::Validation(format!("{key} must come before paths"))
}

/// Reads a v2023 document from `read`, passing each entry to `f`.
pub(super) fn decode<'de, R, F, E>(read: R, mut f: F) -> Result<V2023Summary, E>
where
    R: serde_json::de::Read<'de>,
    F: FnMut(HashAlgorithm, FileEntry) -> Result<(), E>,
    E: From<SnapshotError>,
{
    let mut failure = None;
    let mut de = serde_json::Deserializer::new(read);
    let parsed = Document {
        f: &mut f,
        failure: &mut failure,
    }
    .deserialize(&mut de)
    .and_then(|header| de.end().map(|()| header));
    if let Some(e) = failure {
        return Err(e);
    }
    let header = parsed.map_err(json_error)?;
    Ok(header.finish()?)
}

/// I/O errors keep their kind; everything else is a syntax error in the text.
fn json_error(e: serde_json::Error) -> SnapshotError {
    if e.is_io() {
        SnapshotError::Io(e.into())
    } else {
        SnapshotError::Validation(format!("Invalid JSON: {e}"))
    }
}

/// Puts `e` in the failure slot and returns the error that stops the parse.
fn stop<E, Er: de::Error>(failure: &mut Option<E>, e: E) -> Er {
    *failure = Some(e);
    Er::custom("decode stopped")
}

/// Top-level members read so far.
#[derive(Default)]
struct Header {
    has_manifest_version: bool,
    has_hash_alg: bool,
    manifest_version: Option<String>,
    hash_alg: Option<String>,
    total_size: Option<u64>,
    /// Set once `paths` has been read.
    paths: Option<PathsRead>,
    file_count: u64,
}

struct PathsRead {
    hash_alg: HashAlgorithm,
    /// Whether `paths` was an array.
    was_array: bool,
}

impl Header {
    /// Checks the header read so far when `paths` arrives, in
    /// `decode_v2023`'s order.
    fn check_before_paths(&self) -> crate::Result<HashAlgorithm> {
        if !self.has_manifest_version {
            return Err(missing_before_paths("manifestVersion"));
        }
        v2023_version(self.manifest_version.as_deref())?;
        if !self.has_hash_alg {
            return Err(missing_before_paths("hashAlg"));
        }
        parse_hash_alg(self.hash_alg.as_deref())
    }

    /// The checks that need the whole document, in `decode_v2023`'s order.
    fn finish(self) -> crate::Result<V2023Summary> {
        let hash_alg = match &self.paths {
            Some(p) => p.hash_alg,
            None => {
                v2023_version(self.manifest_version.as_deref())?;
                parse_hash_alg(self.hash_alg.as_deref())?
            }
        };
        let total_size = v2023_read_total_size(self.total_size)?;
        if !self.paths.is_some_and(|p| p.was_array) {
            return Err(v2023_invalid_paths());
        }
        Ok(V2023Summary {
            hash_alg,
            total_size,
            file_count: self.file_count,
        })
    }
}

#[derive(serde::Deserialize)]
#[serde(field_identifier, rename_all = "camelCase")]
enum Key {
    ManifestVersion,
    HashAlg,
    TotalSize,
    Paths,
    #[serde(other)]
    Other,
}

/// Implements the scalar `Visitor` methods so that each returns `$value`.
macro_rules! scalars_read_as {
    ($value:expr) => {
        fn visit_bool<Er>(self, _: bool) -> Result<Self::Value, Er> {
            Ok($value)
        }
        fn visit_i64<Er>(self, _: i64) -> Result<Self::Value, Er> {
            Ok($value)
        }
        fn visit_u64<Er>(self, _: u64) -> Result<Self::Value, Er> {
            Ok($value)
        }
        fn visit_f64<Er>(self, _: f64) -> Result<Self::Value, Er> {
            Ok($value)
        }
        fn visit_str<Er>(self, _: &str) -> Result<Self::Value, Er> {
            Ok($value)
        }
        fn visit_unit<Er>(self) -> Result<Self::Value, Er> {
            Ok($value)
        }
    };
}

/// The whole document. A value that is not an object has no members.
struct Document<'a, F, E> {
    f: &'a mut F,
    failure: &'a mut Option<E>,
}

impl<'de, F, E> DeserializeSeed<'de> for Document<'_, F, E>
where
    F: FnMut(HashAlgorithm, FileEntry) -> Result<(), E>,
    E: From<SnapshotError>,
{
    type Value = Header;

    fn deserialize<D: Deserializer<'de>>(self, d: D) -> Result<Header, D::Error> {
        d.deserialize_any(self)
    }
}

impl<'de, F, E> Visitor<'de> for Document<'_, F, E>
where
    F: FnMut(HashAlgorithm, FileEntry) -> Result<(), E>,
    E: From<SnapshotError>,
{
    type Value = Header;

    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("a v2023 manifest")
    }

    scalars_read_as!(Header::default());

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Header, A::Error> {
        while seq.next_element::<Read<()>>()?.is_some() {}
        Ok(Header::default())
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Header, A::Error> {
        let mut h = Header::default();
        while let Some(key) = map.next_key::<Key>()? {
            match key {
                Key::ManifestVersion if h.paths.is_some() => {
                    return Err(stop(self.failure, after_paths("manifestVersion").into()));
                }
                Key::HashAlg if h.paths.is_some() => {
                    return Err(stop(self.failure, after_paths("hashAlg").into()));
                }
                Key::ManifestVersion => {
                    h.has_manifest_version = true;
                    h.manifest_version = raw::value(&mut map)?;
                }
                Key::HashAlg => {
                    h.has_hash_alg = true;
                    h.hash_alg = raw::value(&mut map)?;
                }
                Key::TotalSize => h.total_size = raw::value(&mut map)?,
                Key::Paths => {
                    if h.paths.is_some() {
                        let e = SnapshotError::Validation("paths appears more than once".into());
                        return Err(stop(self.failure, e.into()));
                    }
                    let hash_alg = match h.check_before_paths() {
                        Ok(alg) => alg,
                        Err(e) => return Err(stop(self.failure, e.into())),
                    };
                    let was_array = map.next_value_seed(Paths {
                        hash_alg,
                        f: &mut *self.f,
                        failure: &mut *self.failure,
                        count: &mut h.file_count,
                    })?;
                    h.paths = Some(PathsRead {
                        hash_alg,
                        was_array,
                    });
                }
                Key::Other => raw::value::<A, ()>(&mut map)?,
            }
        }
        Ok(h)
    }
}

/// The `paths` value. Reads as `true` when it is an array, after every
/// element has been passed to the callback.
struct Paths<'a, F, E> {
    hash_alg: HashAlgorithm,
    f: &'a mut F,
    failure: &'a mut Option<E>,
    count: &'a mut u64,
}

impl<'de, F, E> DeserializeSeed<'de> for Paths<'_, F, E>
where
    F: FnMut(HashAlgorithm, FileEntry) -> Result<(), E>,
    E: From<SnapshotError>,
{
    type Value = bool;

    fn deserialize<D: Deserializer<'de>>(self, d: D) -> Result<bool, D::Error> {
        d.deserialize_any(self)
    }
}

impl<'de, F, E> Visitor<'de> for Paths<'_, F, E>
where
    F: FnMut(HashAlgorithm, FileEntry) -> Result<(), E>,
    E: From<SnapshotError>,
{
    type Value = bool;

    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("an array of v2023 path entries")
    }

    scalars_read_as!(false);

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<bool, A::Error> {
        while map.next_entry::<Read<()>, Read<()>>()?.is_some() {}
        Ok(false)
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<bool, A::Error> {
        while let Some(Read(raw)) = seq.next_element::<Read<RawV2023Path>>()? {
            let entry = match v2023_file_entry(raw)
                .and_then(|entry| Snapshot::validate_file(&entry).map(|()| entry))
            {
                Ok(entry) => entry,
                Err(e) => return Err(stop(self.failure, e.into())),
            };
            if let Err(e) = (self.f)(self.hash_alg, entry) {
                return Err(stop(self.failure, e));
            }
            *self.count += 1;
        }
        Ok(true)
    }
}
