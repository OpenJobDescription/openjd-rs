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
    length: usize,                    // total element count across all ranges
    cumulative_lengths: Vec<usize>,   // cumulative lengths for binary search indexing
}

struct IntRange {
    start: i64,
    end: i64,   // inclusive, actual last value
    step: i64,  // always positive after construction
}
```

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
