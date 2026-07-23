# Range Expressions

## Overview

`RangeExpr` represents a sorted set of integers expressed as sorted, non-overlapping integer ranges like `"1-10"`,
`"1-10:2"`, or `"1-5,10-15"`. It's used for frame ranges and other integer sequences
in job templates.

Defined in `range_expr.rs`.

## Syntax

```
range_expr  = sub_range ("," sub_range)*
sub_range   = integer                        # single value: "5"
            | integer "-" integer            # range with step 1: "1-10"
            | integer "-" integer ":" integer # range with step: "1-10:2"
```

Examples:
- `"5"` → [5]
- `"1-10"` → [1, 2, 3, 4, 5, 6, 7, 8, 9, 10]
- `"1-10:2"` → [1, 3, 5, 7, 9]
- `"1-5,10-15"` → [1, 2, 3, 4, 5, 10, 11, 12, 13, 14, 15]
- `"10-1:-1"` → [10, 9, 8, 7, 6, 5, 4, 3, 2, 1] (descending, requires negative step)
- `"10-1:-3"` → [10, 7, 4, 1] (descending with step)

Note: Descending ranges (where start > end) require a negative step. `"10-1"` without
a step is invalid. Use `"10-1:-1"` for step -1, or `"10-1:-N"` for larger steps.

## Internal Representation

```rust
pub struct RangeExpr {
    ranges: Vec<IntRange>,
    length: u64,                    // packed: low 63 bits = exact element count,
                                    // MSB = contiguous display flag
    cumulative_lengths: Vec<u64>,   // exact cumulative lengths for binary
                                    // search indexing (mirrors the Python
                                    // reference's _range_length_indices)
}

struct IntRange {
    start: i64,  // |start| < 2^62 (see Value Bounds)
    end: i64,    // inclusive, actual last value; |end| < 2^62
    step: i64,   // always positive after construction; step < 2^62
}
```

All lengths and counts are exact u64 values — with the value bounds
below, no count can reach 2^63, so nothing saturates and all index
arithmetic is plain i64/u64.

Ranges are stored in ascending order with positive steps regardless of how they were
specified. Descending ranges like `"10-1:-1"` are normalized to ascending form during
`IntRange::new`: the original direction is not retained. Iteration, indexing, and
`Display` all operate on the canonical ascending form.

This is a deliberate simplification over the Python reference implementation, which
preserves the user-supplied direction. The Rust crate chose canonical form because
(a) every consumer of `RangeExpr` in openjd-rs treats a range as an unordered
sorted set of integers, and (b) it eliminates a whole class of edge cases
(descending-with-positive-step, one-element ranges) from the indexing arithmetic.

## Parsing

`RangeExpr::from_str(s)` uses a self-contained tokenizer and recursive descent parser:

- **Tokenizer** produces: POSINT, HYPHEN, COLON, COMMA tokens
- **Parser** consumes tokens to build `IntRange` values
- Validates no overlapping ranges
- Validates steps are positive and non-zero

## Defensive Caps

`RangeExpr::from_str` and `RangeExpr::from_ranges` enforce a cap on the
number of comma-separated sub-ranges, intended as defense-in-depth against
pathological inputs. A parameter value flowing through `from_str_coerce` is
not protected by the evaluator's memory limit, so an unbounded input here
would allocate proportional memory before evaluation begins.

| Constant | Value | Check |
|---|---|---|
| [`MAX_RANGE_EXPR_CHUNKS`](../../crates/openjd-expr/src/range_expr.rs) | 10,000 | Count of comma-separated sub-ranges |

The chunk cap is checked twice: inside `parse_range_expr` as each sub-range
is appended (so an attacker cannot force the parser to consume gigabytes of
comma-separated source text), and again inside `from_ranges` after parsing
completes (so direct callers of `from_ranges` cannot bypass the cap).

The cap targets the **source-text and heap dimensions** of a `RangeExpr`.
It does not bound the logical element count of any single chunk: `RangeExpr`
stores chunks symbolically as `(start, end, step)`, so a one-chunk range
`"1-100000000000"` allocates a single `IntRange` regardless of its 10¹¹
logical length. The model layer relies on this laziness to support very
large parameter spaces (millions of frames, billions of iterations) without
materialization. Downstream conversion (`list(range_expr)`, comprehensions
over the range) is already bounded by the evaluator's per-element operation
charge and memory limit.

## Value Bounds

Range endpoints and steps are restricted to magnitudes **strictly below
2^62** (`MAX_RANGE_VALUE_MAGNITUDE`). Inputs at or beyond the bound —
through parsing, `IntRange::new`, `RangeExpr::from_values`, or
`RangeExpr::from_ranges` — are rejected with an integer overflow error
(`ExpressionErrorKind::IntegerOverflow`) at construction time.

This is a deliberate narrowing of the language surface chosen so that
**every derived quantity is exactly representable in 64 bits**:

- The widest possible endpoint span is below 2^63, so element counts fit
  i64/u64 exactly — `len()` never saturates, and the cumulative index
  table used for O(log m) `get()` is always exact.
- Index products (`i * step`), normalization intermediates
  (`start + (count - 1) * step`), adjacency checks (`end + step`), and
  `from_values` gap detection all stay within i64 with 2x headroom.
- No widened (i128/u128) arithmetic and no saturating-vs-exact length
  split exists anywhere in the implementation; the element count can
  never collide with the contiguous-display flag packed into the length
  field's high bit.

The total element count across all chunks is also below 2^63: chunks are
non-overlapping subsets of the bounded value domain, so the sum cannot
exceed the domain size.

**Divergence from the Python reference:** Python's implementation uses
arbitrary-precision integers and accepts any endpoint. 2^62 ≈ 4.6×10^18 is
nine orders of magnitude beyond any realistic frame range or task index,
so the restriction is not expected to affect real templates. One slicing
edge case remains: a slice whose selected-value spacing
(`chunk_step × slice_step`) reaches the step bound stores the (at most
two) selected values as singleton chunks rather than a stepped chunk.

This cap is orders of magnitude above any realistic render-farm frame
range: real multi-chunk inputs select frames from a handful of shots, well
under 10,000 chunks. Hitting the cap means either a malicious input or a
codegen bug upstream — both warrant failing loudly.

Exceeding the cap produces an `ExpressionError` whose message names the
violated limit. The crate does not allocate a dedicated
`ExpressionErrorKind` variant for this case because `RangeExpr::from_str`
errors are routed through `ExpressionError::parse_error` to match the
existing diagnostic style.

## Indexing

`RangeExpr` supports O(log n) random access via binary search on `cumulative_lengths`:

```rust
let r = RangeExpr::from_str("1-5,10-15")?;
r[0]   // → 1
r[4]   // → 5
r[5]   // → 10
r[10]  // → 15
r.len() // → 11
```

The `cumulative_lengths` array stores cumulative element counts, enabling binary search
to find which sub-range contains a given index, then computing the element within that
sub-range.

## Iteration

`RangeExpr` implements `IntoIterator` and provides `iter()`. Iteration is lazy — it
walks through sub-ranges without materializing the full list.

## Conversion

| From | To | Method |
|------|----|--------|
| `&str` | `RangeExpr` | `RangeExpr::from_str(s)` |
| `Vec<i64>` | `RangeExpr` | `RangeExpr::from_values(values)` — sorts, deduplicates, auto-detects step patterns |
| `RangeExpr` | `String` | `to_string()` — canonical form |
| `RangeExpr` | `Vec<i64>` | `iter().collect()` |
| `RangeExpr` | `ExprValue::ListInt` | Via `list()` function in expression language |

`from_values` sorts and deduplicates the input, then detects arithmetic sequences to
reconstruct compact range representations. For example, `[1, 3, 5, 7, 9]` becomes `"1-9:2"`.

## Slicing

`RangeExpr` supports slicing with `slice(start, stop, step)`, returning a new `RangeExpr`
without materializing elements:

```rust
let r = RangeExpr::from_str("1-100")?;
let sliced = r.slice(10, 50, 2)?;  // elements at indices 10, 12, 14, ..., 48
```

The algorithm operates in O(m) time where m is the number of sub-ranges, by walking
the sub-ranges and computing intersections with the requested index range. No element
vector is allocated — the result is a new `RangeExpr` built directly from computed
sub-ranges. Step must be positive.

When a slice selects exactly two values from one sub-range and their spacing
(`chunk_step × slice_step`) reaches the value bound, the result is
represented as two singleton chunks, since the combined step would violate
the bounded step domain of the stored `IntRange` (see Value Bounds).

## Containment

`RangeExpr` supports O(log n) containment checks via binary search:

```rust
let r = RangeExpr::from_str("1-10:2")?;  // 1, 3, 5, 7, 9
r.contains(5)  // → true
r.contains(4)  // → false
```

## Contiguous Display Mode

`RangeExpr` supports a contiguous display flag that changes how single values are
formatted:

```rust
let r = RangeExpr::from_str("5")?.with_contiguous(true);
r.to_string()  // → "5-5" (not "5")

let r = RangeExpr::from_str("1-10")?.with_contiguous(true);
r.to_string()  // → "1-10" (unchanged)
```

When the contiguous flag is set, `Display` always uses `"{start}-{end}"` format, even
for single values. This is used by the model layer's step parameter space chunking,
where a range expression represents a contiguous chunk of work assigned to a task. A
chunk containing a single frame (e.g., frame 5) must display as `"5-5"` rather than
`"5"` so that the consuming code can unambiguously parse it as a range chunk.

The flag is packed into the high bit of the `length` field to avoid increasing the
struct size. The packing matters because `RangeExpr` is instantiated **once per task**
during step parameter space chunking — a step producing a million tasks allocates a
million `RangeExpr` chunk descriptors, so an extra 8 bytes per instance would cost
8 MB for that one step. The flag only affects `Display`; it is not preserved through
constructors like `from_values` and does not affect equality comparison or iteration.

## Expression Language Integration

In the expression language, `RangeExpr` values support:

| Operation | Result |
|-----------|--------|
| `len(r)` | Element count |
| `r[i]` | Index access |
| `r[i:j]` / `r[i:j:k]` | Slice → `list[int]` |
| `x in r` | Containment check |
| `min(r)` / `max(r)` / `sum(r)` | Aggregate operations |
| `list(r)` | Convert to `list[int]` |
| `string(r)` | Canonical string form |
| `r + r2` | Concatenate → `list[int]` |
| `r + list` / `list + r` | Concatenate → `list[int]` |
