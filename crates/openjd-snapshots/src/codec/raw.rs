// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! A lenient, typed view of a manifest document, read in one parse.
//!
//! The decoders need a handful of top-level fields and, per entry, a few
//! scalars. This module reads exactly those into typed slots, so decoding
//! never builds a [`serde_json::Value`] tree, whose per-entry maps and key
//! strings cost several times the size of the JSON text.
//!
//! The view has the same semantics as reading the document through `Value`
//! and its accessors (`as_str`, `as_u64`, `as_i64`, `as_bool`, `as_array`,
//! `get`), which is how the decoders' validation is specified:
//!
//! - A value of the wrong JSON type reads as absent, as the accessors return
//!   `None` for it. An entry that is not an object reads as having no fields.
//! - When a key appears more than once in an object, the last value wins.
//! - Keys the decoders do not use are ignored.
//! - Every value is still parsed in full, including ignored ones, so input
//!   that `Value` rejects (an out-of-range number, an invalid `\u` escape,
//!   nesting past the recursion limit) fails here too, with the same error.
//!   That is why skipped values go through `deserialize_any` rather than
//!   [`serde::de::IgnoredAny`], which accepts such input.

use crate::{Result, SnapshotError};
use serde::de::{Deserialize, Deserializer, MapAccess, SeqAccess, Visitor};
use std::fmt;
use std::marker::PhantomData;

/// A value read from any JSON input. Input of the wrong JSON type reads as
/// `Self::default()`, so every implementation's default means "absent".
trait Lenient: Default {
    fn from_bool(_: bool) -> Self {
        Self::default()
    }
    fn from_i64(_: i64) -> Self {
        Self::default()
    }
    fn from_u64(_: u64) -> Self {
        Self::default()
    }
    fn from_text(_: &str) -> Self {
        Self::default()
    }
    fn from_string(s: String) -> Self {
        Self::from_text(&s)
    }
    fn from_seq<'de, A: SeqAccess<'de>>(mut seq: A) -> std::result::Result<Self, A::Error> {
        while seq.next_element::<Read<()>>()?.is_some() {}
        Ok(Self::default())
    }
    fn from_map<'de, A: MapAccess<'de>>(mut map: A) -> std::result::Result<Self, A::Error> {
        while map.next_entry::<Read<()>, Read<()>>()?.is_some() {}
        Ok(Self::default())
    }
}

/// Deserializes any JSON value into `T` through [`Lenient`].
struct Read<T>(T);

impl<'de, T: Lenient> Deserialize<'de> for Read<T> {
    fn deserialize<D: Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        d.deserialize_any(ReadVisitor(PhantomData))
    }
}

struct ReadVisitor<T>(PhantomData<T>);

impl<'de, T: Lenient> Visitor<'de> for ReadVisitor<T> {
    type Value = Read<T>;

    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("any JSON value")
    }
    fn visit_bool<E>(self, v: bool) -> std::result::Result<Read<T>, E> {
        Ok(Read(T::from_bool(v)))
    }
    fn visit_i64<E>(self, v: i64) -> std::result::Result<Read<T>, E> {
        Ok(Read(T::from_i64(v)))
    }
    fn visit_u64<E>(self, v: u64) -> std::result::Result<Read<T>, E> {
        Ok(Read(T::from_u64(v)))
    }
    fn visit_f64<E>(self, _: f64) -> std::result::Result<Read<T>, E> {
        Ok(Read(T::default()))
    }
    fn visit_str<E>(self, v: &str) -> std::result::Result<Read<T>, E> {
        Ok(Read(T::from_text(v)))
    }
    fn visit_string<E>(self, v: String) -> std::result::Result<Read<T>, E> {
        Ok(Read(T::from_string(v)))
    }
    fn visit_unit<E>(self) -> std::result::Result<Read<T>, E> {
        Ok(Read(T::default()))
    }
    fn visit_seq<A: SeqAccess<'de>>(self, seq: A) -> std::result::Result<Read<T>, A::Error> {
        T::from_seq(seq).map(Read)
    }
    fn visit_map<A: MapAccess<'de>>(self, map: A) -> std::result::Result<Read<T>, A::Error> {
        T::from_map(map).map(Read)
    }
}

/// Reads a map value as `T`.
fn value<'de, A: MapAccess<'de>, T: Lenient>(map: &mut A) -> std::result::Result<T, A::Error> {
    Ok(map.next_value::<Read<T>>()?.0)
}

/// A value that is parsed and discarded.
impl Lenient for () {}

/// `Value::as_str`.
impl Lenient for Option<String> {
    fn from_text(s: &str) -> Self {
        Some(s.to_owned())
    }
    fn from_string(s: String) -> Self {
        Some(s)
    }
}

/// `Value::as_u64`: a non-negative integer.
impl Lenient for Option<u64> {
    fn from_i64(n: i64) -> Self {
        u64::try_from(n).ok()
    }
    fn from_u64(n: u64) -> Self {
        Some(n)
    }
}

/// `Value::as_i64`: an integer that fits in `i64`.
impl Lenient for Option<i64> {
    fn from_i64(n: i64) -> Self {
        Some(n)
    }
    fn from_u64(n: u64) -> Self {
        i64::try_from(n).ok()
    }
}

/// `Value::as_bool`.
impl Lenient for Option<bool> {
    fn from_bool(b: bool) -> Self {
        Some(b)
    }
}

/// `Value::as_array`, with each element read as `T`.
impl<T: Lenient> Lenient for Option<Vec<T>> {
    fn from_seq<'de, A: SeqAccess<'de>>(mut seq: A) -> std::result::Result<Self, A::Error> {
        let mut items = Vec::new();
        while let Some(Read(item)) = seq.next_element::<Read<T>>()? {
            items.push(item);
        }
        Ok(Some(items))
    }
}

/// Implements [`Lenient`] for a struct of `Option` slots read from a JSON
/// object, one slot per key. Keys are matched without allocating, through a
/// field-identifier enum whose `Other` variant catches every other key.
macro_rules! lenient_object {
    ($ty:ident, $key:ident { $($variant:ident => $field:ident),+ $(,)? }) => {
        impl Lenient for $ty {
            fn from_map<'de, A: MapAccess<'de>>(
                mut map: A,
            ) -> std::result::Result<Self, A::Error> {
                let mut out = Self::default();
                while let Some(key) = map.next_key::<$key>()? {
                    match key {
                        $($key::$variant => out.$field = value(&mut map)?,)+
                        $key::Other => value::<A, ()>(&mut map)?,
                    }
                }
                Ok(out)
            }
        }
    };
}

/// The top-level object of a v2023 or v2025 manifest.
#[derive(Default)]
pub(super) struct RawManifest {
    /// Set when the `manifestVersion` key is present, whatever its value.
    pub(super) has_manifest_version: bool,
    /// Set when the `specificationVersion` key is present, whatever its value.
    pub(super) has_specification_version: bool,
    pub(super) manifest_version: Option<String>,
    pub(super) specification_version: Option<String>,
    pub(super) hash_alg: Option<String>,
    pub(super) total_size: Option<u64>,
    pub(super) parent_manifest_hash: Option<String>,
    pub(super) file_chunk_size_bytes: Option<i64>,
    /// v2023 entries.
    pub(super) paths: Option<Vec<RawV2023Path>>,
    /// v2025 directory entries.
    pub(super) dirs: Option<Vec<RawV2025Dir>>,
    /// v2025 file and symlink entries.
    pub(super) files: Option<Vec<RawV2025File>>,
}

#[derive(serde::Deserialize)]
#[serde(field_identifier, rename_all = "camelCase")]
enum ManifestKey {
    ManifestVersion,
    SpecificationVersion,
    HashAlg,
    TotalSize,
    ParentManifestHash,
    FileChunkSizeBytes,
    Paths,
    Dirs,
    Files,
    #[serde(other)]
    Other,
}

impl Lenient for RawManifest {
    fn from_map<'de, A: MapAccess<'de>>(mut map: A) -> std::result::Result<Self, A::Error> {
        let mut out = Self::default();
        while let Some(key) = map.next_key::<ManifestKey>()? {
            match key {
                ManifestKey::ManifestVersion => {
                    out.has_manifest_version = true;
                    out.manifest_version = value(&mut map)?;
                }
                ManifestKey::SpecificationVersion => {
                    out.has_specification_version = true;
                    out.specification_version = value(&mut map)?;
                }
                ManifestKey::HashAlg => out.hash_alg = value(&mut map)?,
                ManifestKey::TotalSize => out.total_size = value(&mut map)?,
                ManifestKey::ParentManifestHash => out.parent_manifest_hash = value(&mut map)?,
                ManifestKey::FileChunkSizeBytes => {
                    out.file_chunk_size_bytes = value(&mut map)?;
                }
                ManifestKey::Paths => out.paths = value(&mut map)?,
                ManifestKey::Dirs => out.dirs = value(&mut map)?,
                ManifestKey::Files => out.files = value(&mut map)?,
                ManifestKey::Other => value::<A, ()>(&mut map)?,
            }
        }
        Ok(out)
    }
}

/// One element of a v2023 `paths` array.
#[derive(Default)]
pub(super) struct RawV2023Path {
    pub(super) path: Option<String>,
    pub(super) hash: Option<String>,
    pub(super) size: Option<u64>,
    pub(super) mtime: Option<u64>,
}

#[derive(serde::Deserialize)]
#[serde(field_identifier, rename_all = "lowercase")]
enum V2023PathKey {
    Path,
    Hash,
    Size,
    Mtime,
    #[serde(other)]
    Other,
}

lenient_object!(RawV2023Path, V2023PathKey {
    Path => path,
    Hash => hash,
    Size => size,
    Mtime => mtime,
});

/// One element of a v2025 `dirs` array.
#[derive(Default)]
pub(super) struct RawV2025Dir {
    pub(super) name: Option<String>,
    pub(super) delete: Option<bool>,
}

#[derive(serde::Deserialize)]
#[serde(field_identifier, rename_all = "lowercase")]
enum V2025DirKey {
    Name,
    Delete,
    #[serde(other)]
    Other,
}

lenient_object!(RawV2025Dir, V2025DirKey {
    Name => name,
    Delete => delete,
});

/// One element of a v2025 `files` array.
#[derive(Default)]
pub(super) struct RawV2025File {
    pub(super) name: Option<String>,
    pub(super) hash: Option<String>,
    /// The string elements of `chunkhashes`; other elements read as `None`.
    pub(super) chunkhashes: Option<Vec<Option<String>>>,
    pub(super) symlink: RawV2025Symlink,
    pub(super) size: Option<u64>,
    pub(super) mtime: Option<u64>,
    pub(super) runnable: Option<bool>,
    pub(super) delete: Option<bool>,
}

#[derive(serde::Deserialize)]
#[serde(field_identifier, rename_all = "lowercase")]
enum V2025FileKey {
    Name,
    Hash,
    Chunkhashes,
    Symlink,
    Size,
    Mtime,
    Runnable,
    Delete,
    #[serde(other)]
    Other,
}

lenient_object!(RawV2025File, V2025FileKey {
    Name => name,
    Hash => hash,
    Chunkhashes => chunkhashes,
    Symlink => symlink,
    Size => size,
    Mtime => mtime,
    Runnable => runnable,
    Delete => delete,
});

/// The `symlink` object of a v2025 file entry. Any other value reads as
/// having no `name`.
#[derive(Default)]
pub(super) struct RawV2025Symlink {
    pub(super) name: Option<String>,
}

#[derive(serde::Deserialize)]
#[serde(field_identifier, rename_all = "lowercase")]
enum V2025SymlinkKey {
    Name,
    #[serde(other)]
    Other,
}

lenient_object!(RawV2025Symlink, V2025SymlinkKey { Name => name });

/// Parses a manifest document. Fails only when the text is not valid JSON;
/// every structural check is left to the decoders.
pub(super) fn parse(json: &str) -> Result<RawManifest> {
    serde_json::from_str::<Read<RawManifest>>(json)
        .map(|Read(raw)| raw)
        .map_err(|e| SnapshotError::Validation(format!("Invalid JSON: {e}")))
}
