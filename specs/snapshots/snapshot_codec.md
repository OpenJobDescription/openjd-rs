# Manifest Codec

[README](README.md) · Manifest Codec

**Location:** `codec.rs`, `codec/raw.rs`, `codec/stream.rs`

This document describes how the encoders and decoders read and write the
v2023 and v2025 JSON manifest formats. For the signatures, see
[public-api.md § Codec](public-api.md#codec). For the format status (v2023
stable, v2025 experimental draft), see
[snapshot_overview.md](snapshot_overview.md#status).

## Encoding

Both v2023 encoders write `totalSize` as the sum of the encoded entries'
sizes and return `SnapshotError::Validation` if that sum exceeds `u64::MAX`;
they never write a clamped total. The v2025 encoders write the manifest's
`total_size` field as is, which `recompute_total_size` may have clamped to
`u64::MAX`.

This is an intentional divergence from Python. Python's v2023 encoder uses
arbitrary-precision integers, so it writes the exact sum (for example
`18446744073709551616` for two `2^63`-byte files), and Python reads that
back. Rust stores sizes as `u64`, so it cannot represent such a manifest: the
v2023 encoders reject it rather than write a `totalSize` that disagrees with
the entries, and `decode_v2023` rejects a `totalSize` above `u64::MAX` as
`missing or invalid totalSize`. No real file set reaches this size; the limit
matters only for crafted manifests.

`encode_snapshot_v2023` and `encode_snapshot_diff_v2023` share one encoder;
the diff form passes its entries, deletions included, to it as a snapshot.
Both write `hashAlg` as `HashAlgorithm::extension()`, which the decoders
parse back with `HashAlgorithm::from_extension`.

The v2023 encoders and the relative v2025 encoders check symlink targets
with the rule `Manifest::validate` applies to a relative manifest, so a bad
target fails with the same `symlink_target path style mismatch for <path>:
<target>` message validation gives.

### v2025 Directories and Drive Roots

The v2025 encoder lists every ancestor directory of the manifest's entries,
except the POSIX root `/` and the relative root `.`, which the format leaves
implicit. A directory name has no trailing slash, because `$N/name` expands
to `<dir>/name`, so a Windows drive root is listed as `C:` and `C:/a/b.txt`
is written as a reference to it. This holds for an explicit `C:/` dir entry
too, which decodes back to `C:/`. The decoder rejects a name containing `/`
that is neither absolute (`/...`) nor a `$N/` reference (`Invalid path
format 'C:/': paths with '/' must use $N/ reference`).

## Decoding

### Format Detection

`decode_manifest` decodes a document that has a `manifestVersion` key as
v2023, otherwise one that has a `specificationVersion` key as v2025, and
otherwise fails with `Unknown manifest format: no manifestVersion or
specificationVersion`. It goes by whether the key is present, not by its
value, so `{"manifestVersion": null}` fails as `missing manifestVersion`.

### Reading Rules

All four decoders (`decode_manifest`, `decode_v2023`, `decode_v2023_as_diff`,
`decode_v2025`) read their input with the same rules, and the
[streaming decoders](#streaming-v2023-decode) read values the same way:

- Text that is not valid JSON fails as `Invalid JSON: <serde_json error>`,
  with the error's line and column. That includes members the decoder does
  not use: an out-of-range number, an invalid `\u` escape, or nesting past
  serde_json's recursion limit anywhere in the document is rejected.
- A member of the wrong JSON type is treated as absent. A required member
  then fails with its "missing" error (for example, a v2023 entry whose
  `size` is `"2"`, `-2`, or `2.0` fails as `path entry missing 'size'`),
  and an optional one takes its default (a v2025 `runnable` of `"yes"` is
  `false`). An array entry that is not an object has no members. Non-string
  elements of a v2025 `chunkhashes` array are dropped.
- When an object has a key more than once, the last value is used.
- Unknown members are ignored.

These rules assume serde_json's default number handling. A build that turns
on serde_json's `arbitrary_precision` feature (Cargo unifies features, so a
dependent crate can) accepts out-of-range numbers, and reads `-0` in
`fileChunkSizeBytes`, the only signed member, as absent instead of `0`.

### Error Order

Header errors are reported before entry errors, except that v2025 reports
an unknown `specificationVersion` after its `dirs` and `files` errors.
`tests/integration/test_codec_decode.rs` asserts every decode error message
and each of the reading rules above.

### Keeping Decode Memory Low

Manifests can be large, so decoding should use little memory beyond the
input text. `tests/decode_memory.rs` enforces this: it fails if a decoder's
peak heap use passes a fixed multiple of the input size, or, for the
streaming v2023 decoders, a fixed number of bytes. The test is the
constraint. This section is guidance.

How the decoders keep memory low today (`src/codec.rs`,
`src/codec/raw.rs`):

- They parse the input once, straight into typed values for the members
  the decoders read. Other members are parsed and discarded.
- They move hashes, and v2023 paths, out of the parsed values instead of
  copying them.
- `decode_v2025` drops the parsed directory list once it has built the
  directory entries.

When changing decoding code:

- Try to read into typed structs with serde, and to parse the input only
  once, deciding what to do from the parsed result.
- Avoid building a generic tree such as `serde_json::Value` for a whole
  document. Each object in it is a map with its own key strings, which
  costs several times the text.
- Avoid `serde::de::IgnoredAny` for skipped members when invalid JSON must
  still be rejected. It accepts out-of-range numbers, unpaired surrogate
  `\u` escapes, and nesting past the recursion limit.

## Streaming v2023 Decode

`decode_v2023_try_for_each_file` and
`decode_v2023_try_for_each_file_from_reader` (`codec/stream.rs`) decode a
v2023 manifest without building it. See
[snapshot_overview.md](snapshot_overview.md) design decision 14 for why they
take a callback rather than returning an iterator, and why there is no v2025
equivalent.

### Memory

One entry is held at a time, so memory use does not depend on the number of
entries. `tests/decode_memory.rs` bounds the peak at 16 KiB for a
20,000-entry manifest (it measures about 200 bytes from a `&str`, plus the
`BufReader`'s 8 KiB buffer from a reader). Memory does grow with the largest
entry and the longest string: normalizing a path while building its
`FileEntry` can take over ten times the path's length, serde_json unescapes a
string that contains escapes into a buffer even when the member is not used,
and the reader form copies every string, used or not, into that buffer as it
reads it.

### Reader Form

The reader form reads the text as it parses, so callers should pass it a
buffered reader. It returns read errors as `SnapshotError::Io`. After the
manifest's closing `}` it reads on to the end of input, to reject trailing
text, so it does not return until the reader reports end of input.

### Differences from `decode_v2023`

Values are read, each entry checked, and errors worded as `decode_v2023`
does, with these differences, which streaming requires:

- The header is checked when `paths` arrives, with the values read so far,
  so `manifestVersion` and `hashAlg` must come before `paths`, as they do in
  canonical (sorted-key) v2023 output, which `encode_snapshot_v2023`
  writes. One that has not appeared by then
  fails with `missing manifestVersion before paths` (or `hashAlg`), one
  that appears again after `paths` with `manifestVersion must come before
  paths` (or `hashAlg`), and a second `paths` key with `paths appears more
  than once`. `decode_v2023` uses the last value of a repeated key instead.
- Errors are mostly reported in document order, and entries are delivered
  before the rest of the document is read. Each entry is checked in full
  before the next one is read, so with several bad entries the first bad
  one is reported (`decode_v2023` reports the first entry missing a member
  before it applies its other checks). An error found ahead of a repeated
  key is reported, even if `decode_v2023` would have used the later value.
  `totalSize` is checked at the end, wherever it appears, since a later one
  replaces an earlier one, so a bad entry is reported before a missing or
  invalid `totalSize`.
- After an error, the entries already delivered should be discarded: the
  manifest is invalid, or the decode was stopped by the callback or a read
  error, so they may not be all of its entries.
- Paths are not checked for duplicates, which would take memory in
  proportion to the manifest. `decode_v2023` rejects a repeated path.
- From a reader, serde_json can report a syntax error's column one past
  where it reports it for a `&str`.

### Testing

`tests/integration/test_codec_stream.rs` covers these differences. On every
input, the `snapshots_decode_text` fuzz target checks that the two streaming
functions agree with each other (the same entries, and the same error apart
from its position). It checks that they agree with `decode_v2023`: one may
succeed where the other fails only for a key order or repeated key that
streaming rejects, or a repeated path, and when both fail on valid JSON in an
order streaming reads, the messages match except that each may report a
different bad entry, or streaming a bad entry where `decode_v2023` reports
`totalSize`.
