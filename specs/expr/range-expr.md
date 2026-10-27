# Range Expressions

## Overview

`RangeExpr` represents a set of integers written as a range expression like `"1-10"`,
`"1-10:2"`, or `"1-5,10-15"`. It's used for frame ranges and other integer sequences
in job templates. Its value is its list of values in increasing order (Template Schemas
§3.4.1.1.1); how it was written is not part of the value. It is stored as the runs of its
canonical form, so equality, hashing, and `Display` all follow from the values alone.

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
    ranges: Vec<IntRange>,          // the runs of the canonical form
    cumulative_lengths: Vec<u64>,   // exact cumulative lengths for binary
                                    // search indexing (mirrors the Python
                                    // reference's _range_length_indices)
    length: u64,                    // packed: low 63 bits = exact element count,
                                    // MSB = contiguous display flag
}

struct IntRange {
    start: i64,  // first value; never i64::MIN (see Value Bounds)
    end: i64,    // inclusive, actual last value
    step: i64,   // always positive after construction
}
```

An `IntRange` is one arithmetic progression. Ranges are stored in ascending order with
positive steps regardless of how they were specified. Descending ranges like `"10-1:-1"`
are normalized to ascending form during `IntRange::new`, and an end that the step does not
reach is replaced by the last value reached (`"1-10:4"` stores end 9): the original
spelling is not retained.

This is a deliberate simplification over the Python reference implementation, which
preserves the user-supplied direction. The Rust crate chose ascending form because
(a) every consumer of `RangeExpr` in openjd-rs treats a range as an unordered
sorted set of integers, and (b) it eliminates a whole class of edge cases
(descending-with-positive-step, one-element ranges) from the indexing arithmetic.

### Canonical runs

Every constructor (`from_str`, `from_ranges`, `from_values`, `slice`, and deserialization)
normalizes `ranges` to the **runs of the canonical form** of Template Schemas §3.4.1.1.1.
Over the sorted values $v_1 < \dots < v_n$, runs are taken greedily from the left: at
$v_i$, let $s = v_{i+1} - v_i$ and extend to the last $v_j$ reached with constant
difference $s$; a run of three or more values is $v_i..v_j$ and the next run starts at
$v_{j+1}$, otherwise $v_i$ is a run alone and the next run starts at $v_{i+1}$. So each run
is one value (stored with step 1) or three or more values, never two, and the runs are a
function of the list of values alone:

| Written | Runs | `Display` |
|---------|------|-----------|
| `"1,3,5,7,9"`, `"1-9:2"`, `"1-10:2"` | 1..9 step 2 | `"1-9:2"` |
| `"1-2"`, `"1,2"` | 1; 2 | `"1,2"` |
| `"13-18,19"`, `"19-18:-5,13-18"` | 13..19 step 1 | `"13-19"` |
| `"2,4,6,8,9,10"` | 2..8 step 2; 9; 10 | `"2-8:2,9,10"` |
| `"1,3,5-7"` | 1..5 step 2; 6; 7 | `"1-5:2,6,7"` |
| `"1,3,4,5"` | 1; 3..5 step 1 | `"1,3-5"` |

`canonical_runs` computes the runs from a sequence of progressions whose values are
strictly increasing, **without listing the values**. This is the informative algorithm of
§3.4.1.1.1: a run walks positions (progression, offset); along a progression whose step
equals the run's difference it jumps to that progression's end in one move, and across a
boundary it compares the next progression's first value. Each progression is visited a
bounded number of times, so the work is O(k) in the number of progressions and never
depends on the number of values; the output has at most two runs per progression. A
range like `"1-6000000000000000000"` or `"0-4000000000000000000:2,4000000000000000001"`
(more than 10^18 values) is canonicalized, compared, and printed in constant time.

Per constructor:

- `from_str` and `from_ranges`: O(k log k) to sort the k elements, then O(k).
- `from_values`: the list is already expanded, so O(n log n) to sort and O(n) for the
  runs.
- `slice`: O(m) in the m runs of the source, producing at most one progression per run
  (two single values when two selected values are further apart than `i64::MAX`), then
  O(m) to canonicalize.

Differences between consecutive values are computed as `u64` (`abs_diff`), since two
values can be up to 2^64 - 2 apart. The step of a run of three or more values is at most
half its span, so it always fits `i64`.

## Canonical Form (`Display`)

`Display` writes the canonical form: each run as `x` (one value), `x-y` (step 1), or
`x-y:s`, in base 10 with a leading `-` for negatives, joined by `,` with no white space.
Because the runs are stored, this is O(number of runs). Every path that writes a range
expression value as a string goes through it: `string(r)`, `+` with a string,
format-string interpolation, `repr_py`/`repr_json`/`repr_pwsh`, the JSON and display forms
of a list containing a range, and the model, sessions, and CLI layers that print task
parameter values. The canonical form parses back to the same value and prints the same
string again. The only other form is the contiguous chunk form (see Contiguous Display
Mode).

`IntRange` also implements `Display`, writing one progression as `x`, `x,y` (two values),
`x-y`, or `x-y:s`; for a run of the canonical form this is the run's text. It is used in
error messages about elements, like the overlap error.

## Equality and Hashing

Two `RangeExpr` values are equal exactly when they have the same list of values
(Expression Language §1.2.5): `range_expr("1,3,5,7,9") == range_expr("1-9:2")` is `true`.
Since the runs are canonical, `PartialEq` compares `ranges` (O(number of runs), no
expansion) and `Hash` hashes them, which keeps `Hash` consistent with `Eq`. The
contiguous display flag is not part of the value. `ExprValue` equality and hashing for
`range_expr` delegate to these, and so do the model's `TaskParamRange` equality and chunk
containment checks.

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
charge and memory limit. (The count is bounded only by `MAX_RANGE_EXPR_LEN`; see Value
Bounds.) Canonicalization can split an element into at most two runs, so a range that
passes the cap stores at most 20,000 runs.

This cap is orders of magnitude above any realistic render-farm frame
range: real multi-chunk inputs select frames from a handful of shots, well
under 10,000 chunks. Hitting the cap means either a malicious input or a
codegen bug upstream — both warrant failing loudly.

Exceeding the cap produces an `ExpressionError` whose message names the
violated limit. The crate does not allocate a dedicated
`ExpressionErrorKind` variant for this case because `RangeExpr::from_str`
errors are routed through `ExpressionError::parse_error` to match the
existing diagnostic style.

## Value Bounds

A range value, endpoint, or step may be **any `i64` except `i64::MIN`**, and a range
expression holds **at most `MAX_RANGE_EXPR_LEN` = `i64::MAX` values**:

- `i64::MIN` is rejected with an integer overflow error
  (`ExpressionErrorKind::IntegerOverflow`), through `IntRange::new`,
  `RangeExpr::from_values`, and `RangeExpr::from_ranges`. Its negation overflows, and the
  parser cannot read it either (see below), so excluding it keeps every canonical form
  parseable.
- A range or element with more than `i64::MAX` values is rejected with
  `Range expression has too many values; the maximum is 9223372036854775807`. So
  `"-9223372036854775807,9223372036854775807"` (two values) and `"1-9223372036854775807"`
  (the largest count) are accepted, and `"-9223372036854775807-9223372036854775807"`
  (2^64 - 1 values) is not.

The count bound exists because the expression language's `int` is 64-bit: `len(r)` and
every index into `r` must fit one. With it, counts and cumulative lengths are exact in
`u64` and convert to `i64` exactly, index arithmetic in `slice` and `get` is plain
`i64`/`u64`, and the element count never reaches the contiguous flag packed into the
length field's high bit. Spans between values, however, can reach 2^64 - 2, so
`IntRange` computes them with `abs_diff` (u64) and applies offsets with
`wrapping_add_unsigned`, which is exact because each result is a value of the range.

**Divergence from the Python reference:** Python's implementation uses arbitrary-precision
integers and accepts any endpoint and any count. Values beyond `i64` cannot occur in an
openjd-rs expression, whose `int` is 64-bit.

**Known gap:** the parser reads an integer's digits as a positive `i64` and then negates
it, so `"-9223372036854775808"` (`i64::MIN`) fails to parse with `Invalid integer`. That
value is out of the domain above anyway.

The model narrows the domain further for `CHUNK[INT]` task parameters (magnitudes below
2^62), whose chunking arithmetic is plain i64; see
[model parameter-space.md](../model/parameter-space.md#chunking).

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
| `Vec<i64>` | `RangeExpr` | `RangeExpr::from_values(values)` — sorts, deduplicates, finds the canonical runs |
| `RangeExpr` | `String` | `to_string()` — canonical form (or `<N>-<M>` with the contiguous flag) |
| `RangeExpr` | `Vec<i64>` | `iter().collect()` |
| `RangeExpr` | `ExprValue::ListInt` | Via `list()` function in expression language |

`from_values` sorts and deduplicates the input, then finds the runs of the canonical
form. For example, `[1, 3, 5, 7, 9]` becomes `"1-9:2"` and `[1, 3, 4, 5]` becomes
`"1,3-5"`.

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
progressions and then canonicalized, so `range_expr("1-10")[::2]` is written `"1-9:2"`
and equals `range_expr("1,3,5,7,9")`. Step must be positive.

When a slice selects exactly two values from one run and their spacing
(`run_step × slice_step`) exceeds `i64::MAX`, they are passed to canonicalization as two
single values, since the combined step does not fit an `IntRange` step.

## Containment

`RangeExpr` supports O(log n) containment checks via binary search:

```rust
let r = RangeExpr::from_str("1-10:2")?;  // 1, 3, 5, 7, 9
r.contains(5)  // → true
r.contains(4)  // → false
```

## Contiguous Display Mode

`RangeExpr` supports a contiguous display flag for the one value the specification does
not write in canonical form: a `CONTIGUOUS` chunk of a `CHUNK[INT]` task parameter, which
is always written `<N>-<M>`, even with one or two values (Template Schemas §3.4.1.5,
Expression Language §1.2.2):

```rust
let r = RangeExpr::from_str("5")?.with_contiguous(true);
r.to_string()  // → "5-5" (not "5")

let r = RangeExpr::from_str("5-6")?.with_contiguous(true);
r.to_string()  // → "5-6" (not "5,6")

let r = RangeExpr::from_str("1-10")?.with_contiguous(true);
r.to_string()  // → "1-10" (unchanged)
```

When the flag is set and the values are contiguous (`last - first + 1 == len`), `Display`
writes `"{first}-{last}"`; otherwise it writes the canonical form. The model layer sets
the flag on every `CONTIGUOUS` chunk it builds, and every string path (`Task.Param`,
`Task.RawParam`, `string()`, `repr_*`, `+`) goes through `Display`, so the chunk form
reaches all of them.

The flag is packed into the high bit of the `length` field to avoid increasing the
struct size. The packing matters because `RangeExpr` is instantiated **once per task**
during step parameter space chunking — a step producing a million tasks allocates a
million `RangeExpr` chunk descriptors, so an extra 8 bytes per instance would cost
8 MB for that one step. The flag is not part of the value: it does not affect equality,
hashing, or iteration, and no constructor preserves it, so a range computed from a chunk
(a slice, `range_expr(list(...))`) is written in canonical form.

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
| `r + list` / `list + r` | Concatenate as `list[int]` with the list → the common element type (`list[float]` with a `list[float]`, empty or not); an incompatible list fails, naming the operands in order ("Cannot concatenate range_expr and list[string]", "Cannot concatenate list[string] and range_expr") |
