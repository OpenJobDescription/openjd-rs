# Values

## Overview

`ExprValue` is the runtime representation of expression values. It uses typed list
variants for memory efficiency and carries path format information for path values.

Defined in `value.rs`.

## ExprValue Enum

```rust
pub enum ExprValue {
    Null,
    Bool(bool),
    Int(i64),
    Float(Float64),
    String(String),
    #[non_exhaustive]
    Path { value: String, format: PathFormat },  // construct only via ExprValue::new_path
    ListBool(Vec<bool>),
    ListInt(Vec<i64>),
    ListFloat(Vec<Float64>),
    ListString(Vec<String>, usize),                   // (elements, cached_memory_size)
    ListPath(Vec<String>, PathFormat, usize),          // (elements, format, cached_memory_size)
    ListList(Vec<ExprValue>, ExprType, usize),         // (elements, element_type_hint, cached_memory_size)
    RangeExpr(RangeExpr),
    Unresolved(ExprType),
}
```

The `usize` fields on `ListString`, `ListPath`, and `ListList` cache the heap memory
size at construction time, enabling O(1) memory tracking without recomputing sizes on
every `heap_size()` call. `ListList` also stores an `ExprType` element type hint used
to preserve the element type for empty nested lists.

## Float64

A wrapper around `f64` that optionally preserves the original string representation
for lossless round-tripping (e.g., `3.50` stays `"3.50"` not `"3.5"`):

```rust
pub struct Float64(pub f64, pub Option<Box<str>>);
```

`Box<str>` instead of `String` saves 8 bytes per value (no capacity field). Most floats
computed at runtime won't have an original string, so the `Option` is usually `None`.

Invariants enforced on construction:
- No NaN
- No Infinity / -Infinity
- -0.0 normalized to 0.0

These match the specification's requirement that float values are always finite and
that negative zero is not observable. The rationale is threefold: **determinism**
(NaN breaks reflexive equality and produces implementation-defined sort orders);
**cross-language parity** (Python OpenJD applies the same invariants, so templates
evaluate identically in Rust and Python); and **hashability** (NaN's `NaN != NaN`
would break the `a == b ⇒ hash(a) == hash(b)` contract that `ExprValue` relies on
for `HashMap` and list deduplication).

**Precision note:** Integer true-division (`/`) converts both operands to `f64` before
dividing, so results lose precision for integers above 2^53. This matches Python's
behavior where `int / int` returns a `float`.

## Typed List Variants

The Python implementation uses a single `List` with `elements: list[ExprValue]` and
`elem_type: ExprType`. The Rust implementation uses specialized variants for significant
memory savings:

| Type | Python (per element) | Rust (per element) | Savings |
|------|---------------------|--------------------|---------|
| list[bool] | ~40 bytes (tagged ExprValue) | 1 byte | 97% |
| list[int] | ~40 bytes | 8 bytes | 80% |
| list[float] | ~40 bytes | 16 bytes | 60% |
| list[string] | ~64 bytes | 24 bytes (String) | 63% |
| list[list[T]] | same | same (dynamic ExprValue) | — |

`ListList(Vec<ExprValue>)` handles nested lists (max 2 levels per spec). Only nested
lists pay the cost of dynamic dispatch.

The variable-size list variants (`ListString`, `ListPath`, `ListList`) cache their heap
memory size at construction time to avoid recomputation during memory tracking.

## Value Construction

```rust
// Scalars
ExprValue::Int(42)
ExprValue::Float(Float64::new(3.14))
ExprValue::Float(Float64::with_str(3.14, "3.14".into()))  // pre-parsed f64 + original string for lossless display
ExprValue::String("hello".into())
ExprValue::new_path("/tmp/out", PathFormat::Posix)  // PATH — only public constructor
ExprValue::Null
ExprValue::Bool(true)

// Lists — make_list(elements, hint_type) handles type promotion
ExprValue::make_list(vec![ExprValue::Int(1), ExprValue::Int(2)], ExprType::NULLTYPE)  // → ListInt
ExprValue::make_list(vec![ExprValue::Int(1), ExprValue::Float(..)], ExprType::NULLTYPE)  // → ListFloat (int→float)
ExprValue::make_list(vec![ExprValue::Path{..}, ExprValue::String(..)], ExprType::NULLTYPE)  // → ListString (path→string)
ExprValue::make_list(vec![], ExprType::INT)  // → ListInt (empty, hint selects variant)

// Memory-checked list construction — prefer this from evaluator/function contexts
ExprValue::make_list_checked(ctx, elements, hint_type)  // pre-checks ctx.check_memory(...)

// Unresolved — type-only placeholder for static checking
ExprValue::unresolved(ExprType::INT)
```

### List Construction: `make_list` vs `make_list_checked`

The crate offers two list constructors with identical promotion semantics
but different memory-safety guarantees:

| Function | Caller has `EvalContext`? | Memory pre-check? | Use from |
|---|---|---|---|
| `make_list(elements, hint)` | No | No | Transport deserialization, tests, coercion paths, and any static construction |
| `make_list_checked(ctx, elements, hint)` | Yes | `ctx.check_memory(estimate)` before allocation | Evaluator dispatch and function implementations |

`make_list_checked` is the defense-in-depth path. It computes an upper-bound
estimate of the list's heap footprint, calls `ctx.check_memory(estimate)` to
fail cleanly against the evaluator's memory budget, then forwards to
`make_list`. Call sites with an `EvalContext` available **must** prefer
`make_list_checked`: the existing per-element operation charges catch most
oversized inputs, but `make_list_checked` closes the remaining gap for
future code paths that construct a list without proportional op charges.

The estimator is intentionally conservative — it sums `len * size_of::<ExprValue>()`
plus each element's `heap_size()`, which is an upper bound on the resulting
list's own heap size regardless of which typed-list variant `make_list`
ultimately produces. A false early rejection (estimator over-counts) is
preferable to a late rejection (allocation already happened).

See `crates/openjd-expr/src/value.rs`.

### Path Encapsulation

`ExprValue::Path` is marked `#[non_exhaustive]`, so downstream crates cannot
use a struct literal (`ExprValue::Path { value, format }`) to construct it
directly — doing so is a compile error (E0639). The only public constructor is
`ExprValue::new_path(value, format)`, which normalizes separators according to
the supplied `PathFormat`:

- `PathFormat::Posix`   → no normalization (backslash is a valid filename character)
- `PathFormat::Windows` → `/` replaced with `\` (skipped for URI-valued strings)
- `PathFormat::Uri`     → no normalization (URIs are opaque)

Pattern matching still works from outside the crate, but the `..` token must
be included (as it must for any non-exhaustive struct/enum-variant):

```rust
if let ExprValue::Path { value, format, .. } = &v {
    // ...
}
```

This preserves the separator-normalization invariant workspace-wide: every
`ExprValue::Path` value in existence has been produced by `new_path`.

### make_list Type Promotion

`make_list(elements, hint_type)` infers the element type and promotes elements when
necessary. The `hint_type` parameter determines the element type for empty lists — when
the list is non-empty, the element type is inferred from the elements themselves.

Promotion rules are applied in priority order — the first matching rule wins:

1. All same type → use that typed variant directly
2. Mix of INT and FLOAT → promote all to FLOAT (`ListFloat`)
3. Nested `list[int]` + `list[float]` → promote inner `ListInt` elements to `ListFloat`
4. Nested `list[path]` + `list[string]` → promote inner `ListPath` elements to `ListString`
5. Mix of PATH and STRING → promote all to STRING (`ListString`)
6. First element determines variant (homogeneous case)
7. Incompatible types → error (e.g., INT + STRING, BOOL + FLOAT)

The nested list promotion rules (3, 4) mirror the scalar rules (2, 5) but operate on
the inner element types. For example, `[ListInt([1,2]), ListFloat([3.0])]` promotes the
`ListInt` to `ListFloat` before wrapping in `ListList`. This matches the Python
`_from_list` logic.

Per the specification (section 1.2.6), incompatible element types are always an error —
there is no silent fallback to string conversion.

Empty list variant selection by `hint_type`:

| `hint_type` | Variant |
|-------------|---------|
| BOOL | `ListBool([])` |
| INT | `ListInt([])` |
| FLOAT | `ListFloat([])` |
| PATH | `ListPath([], host_format)` |
| LIST[T] | `ListList([], T)` |
| anything else | `ListInt([])` (canonical empty list) |

## Display Strings

`ExprValue::to_display_string()` is the conversion used wherever a value becomes
text without an explicit repr: the `string()` builtin, format string
interpolation with surrounding text, and `as_str_repr()`.
`write_display(&mut String)` is the same renderer writing into a caller-supplied
buffer. No repr renders a list in display form: `repr_pwsh`, whose generic
`list[T]` overload admits nested lists, recurses into its own renderer, so
`repr_pwsh([["a", "b"], ["c"]])` yields the nested PowerShell array literal
`@(@('a', 'b'), @('c'))` (a one-element outer list uses the unary comma —
`@(,@(1, 2))` — because `@(@(1, 2))` flattens in PowerShell; round-trip tests
in `tests/integration/test_repr_pwsh_roundtrip.rs` execute `powershell.exe` to
verify reconstruction). `repr_sh` and `repr_cmd` register no `list[list[...]]`
overload, so a nested list fails signature dispatch with `No matching
signature` (pinned by `repr_sh_rejects_nested_lists` and
`repr_cmd_rejects_unsupported_lists` in `tests/integration/test_strings.rs`);
the `ValueRef::List` arms remaining in `repr.rs` are defensive dead code.
Only one display implementation exists — the duplicate that once lived in
`repr.rs` let a list-escaping bug survive in two places (openjd-rs#312).

Scalars render as themselves: `null`, `true`/`false`, decimal integers, the
float's preserved or shortest representation, and strings and paths unquoted.

### List display strings

A list renders as a JSON array (RFC 0006 §2.2.1, "convert list to JSON string
representation"). Elements are separated by `", "` — `json.dumps` defaults, and
what the conformance suite asserts (`[1, 2, 3]`, not `[1,2,3]`). Bools, ints,
and floats render bare, nested lists recurse, and strings and paths are
double-quoted with `"`, `\`, and characters below `U+0020` escaped via
`crate::json_escape`.

Escaping non-ASCII is left to the implementation — JSON accepts those characters
directly and the spec requires only that the output parse, so conformance tests
assert neither form. We preserve them, since display strings land in log lines
and command arguments that people read. `repr_json` escapes them as `\uXXXX`
(matching `json.dumps(ensure_ascii=True)`) because its output is embedded in
scripts and transports that may not be UTF-8 clean; it is the only one of the
two that guarantees ASCII.

Escaping can expand a string up to six bytes per input byte, so `string()`
charges the same budget as `repr_json` before rendering a list
(`repr::preflight_display_list`). Scalars cannot expand and are not charged.

## Memory Sizing

Every `ExprValue` reports its memory size via `memory_size()`, which returns
`size_of::<ExprValue>() + heap_size()`. The inline `ExprValue` enum is a fixed size
regardless of variant. The variable part is heap allocations:

| Value | Heap size |
|-------|-----------|
| Null, Bool, Int, Unresolved | 0 |
| Float | original string length (if preserved, else 0) |
| String, Path | string capacity |
| ListBool | vec capacity |
| ListInt | vec capacity × 8 |
| ListFloat | vec capacity × 16 |
| ListString, ListPath, ListList | cached `usize` field (sum of element heap sizes + vec buffer) |
| RangeExpr | heap size of internal range vectors |

The evaluator calls `_track(value)` after creating a value and `_release(value)` before
consuming it, maintaining a running `current_memory` counter checked against the limit.

## ListIter

`ListIter` provides zero-allocation iteration over list elements, yielding `ExprValue`
references without copying the underlying typed storage:

```rust
pub enum ListIter<'a> {
    Bool(std::slice::Iter<'a, bool>),
    Int(std::slice::Iter<'a, i64>),
    Float(std::slice::Iter<'a, Float64>),
    String(std::slice::Iter<'a, String>),
    Path(std::slice::Iter<'a, String>, PathFormat),
    List(std::slice::Iter<'a, ExprValue>),
}
```

Obtained via `ExprValue::iter()` on any list variant. Implements `Iterator<Item = ExprValue>`
and `ExactSizeIterator`. Each `next()` call wraps the underlying element in the
appropriate `ExprValue` variant — this is a copy for scalar types but avoids cloning
the backing storage.

### Clone-on-Yield Semantics

The iterator's `Item` type is `ExprValue` (owned), not `&ExprValue`, because the typed
list variants store raw values (e.g., `Vec<i64>`) that must be wrapped in `ExprValue`
on yield. The cost varies by variant:

| Variant | Yield cost |
|---------|-----------|
| Bool, Int | Bitwise copy (1 or 8 bytes) — zero allocation |
| Float | `Float64::clone()` — copies the f64 and clones the optional `Box<str>` |
| String, Path | `String::clone()` — allocates a new heap buffer |
| List | `ExprValue::clone()` — deep clone of the nested value |

For Bool and Int, this is effectively zero-cost. For String/Path/List, each `next()`
call allocates. This is acceptable because the evaluator tracks memory for each yielded
value individually, and the alternative (returning references) would require GATs or
unsafe code to handle the typed-to-tagged conversion.

### ExactSizeIterator

`ListIter` implements `ExactSizeIterator` by delegating `size_hint()` to the underlying
`std::slice::Iter`, which always returns an exact `(len, Some(len))`. This enables
callers (e.g., `make_list`, `equals()`) to pre-allocate output vectors or short-circuit
on length mismatch without iterating.

## Equality and Hashing Semantics

`ExprValue` implements `Hash` and `PartialEq` with cross-type equivalence rules:

| Comparison | Result | Rationale |
|---|---|---|
| `Int(1) == Float(1.0)` | `true` | Int-float equivalence when float is whole |
| `String("x") == Path { value: "x", .. }` | `true` | Path coerces to its string value |
| `ListInt([]) == ListFloat([])` | `true` | Empty lists are equal regardless of element type |
| `ListBool([]) == ListString([])` | `true` | Same — all empty lists hash and compare equally |
| `ListInt([1]) == ListFloat([1.0])` | `true` | Element-wise cross-type comparison via `equals()` |

`PartialEq` delegates to the `equals()` method, which handles cross-type matching
explicitly: Int↔Float compares exactly in the integer domain (see below),
String↔Path compares the string values, same-variant typed lists compare their
primitive elements directly (charged per comparison performed, like the generic
arm), and mixed list↔list comparison iterates element-wise recursively. List↔RangeExpr
comparison decides length mismatches in O(1) with no charge (range lengths are
exact arithmetic under the bounded value domain), then walks the range's lazy
iterator against the list — never materializing the range — charged per element
comparison performed.

Both `equals()` and the evaluator's `==`/`!=`/`in` operators are backed by one
core, `equals_charged`, which takes a charge callback invoked with the number of
element comparisons about to be performed. The evaluator passes the operation
budget (so comparing large lists charges `count_ops` and can raise
`OperationLimitExceeded`); `equals()`/`PartialEq` pass an infallible no-op.
Comparisons decidable by length alone are decided in O(1) with no charge.

Int↔Float equality is exact, matching Python: the float must be an integer
exactly representable in i64 whose converted value equals the int
(`float_as_exact_i64`). A widening comparison like `(i as f64) == v` would
round `i` first, making distinct integers near 2^63 compare equal to the
same float (`float(2**63) == 2**63 - 1` is False in Python). The same rule
drives `Hash` (an integral in-range float hashes as its integer) and
range membership (`x in range_expr(...)` for float `x`).

Int↔Float *ordering* uses the same exact semantics (`int_float_cmp`):
floats at or beyond the i64 domain order strictly by sign against every
int, and in-domain floats compare via exact truncation with a fractional
tie-break. This keeps `<`/`<=`/`>`/`>=` consistent with equality —
`i64::MAX < float(2**63)` is true, where a widening comparison would call
them equal while `==` says they differ.

String↔Path ordering compares the path's string value while preserving operand order.
For example, `path("/a") < "/z"` is the same comparison as `"/a" < "/z"`.
This rule also applies recursively during lexicographic list ordering.

### Tag-Based Hashing Strategy

The `Hash` implementation must satisfy the contract that `a == b` implies
`hash(a) == hash(b)`. Because `equals()` treats certain different variants as equal,
the `Hash` implementation uses discriminant tags that group equivalent types together
rather than using the enum's natural discriminant:

| Tag | Variants | Why grouped |
|-----|----------|-------------|
| `0` | Null | — |
| `1` | Bool | — |
| `2` | Int, Float (when whole and in i64 range) | `Int(1) == Float(1.0)` |
| `12` | Float (fractional or out of i64 range) | No Int equivalent exists |
| `3` | String, Path | `String("x") == Path { value: "x", .. }` |
| `4` | All list variants | Empty lists are equal across types |
| `10` | RangeExpr | — |
| `11` | Unresolved | — |

For Float values, the hash checks whether the float is a whole number in i64 range. If
so, it hashes with tag `2` and the `i64` cast — identical to how `Int` hashes. Otherwise
it uses tag `12` with the raw `f64` bits. This ensures `Int(1)` and `Float(1.0)` produce
the same hash.

List element hashing mirrors this: each element within a list is hashed using its
`ExprValue`-equivalent tag, not the raw storage type. So `ListInt([1])` hashes tag `4`,
then tag `2` + `1i64`, and `ListFloat([1.0])` hashes tag `4`, then tag `2` + `1i64`
(because `1.0` is whole), producing identical hashes.

## Coercion

Two levels of coercion serve different purposes:

### Dispatch Coercion (during function call matching)

Applied in the second phase of dispatch when exact match fails:

- INT → FLOAT
- PATH → STRING

See [type-system.md § Implicit Coercions](type-system.md#implicit-coercions) for the
full rationale and rules that govern these two coercions across the crate. Method
calls skip receiver coercion to prevent nonsensical calls like `42.upper()`.

### Target Type Coercion (after evaluation, for format string context)

Applied when the evaluation result needs to match an expected type. The
non-destructive conversions, matching RFC 0005 §"Implicit Type Coercion"
(the scalar rules live in one table in the code,
`scalar_conversion_rank`, which also ranks them — see
[Destinations](#destinations)):

- `bool`/`int`/`float`/`path`/`range_expr` → `string` (a `range_expr`
  produces its canonical form, like `"1-5"`)
- `string` → `path` (every string is a valid path)
- `float`/`string` → `int` (error if the value cannot be represented
  exactly, e.g. `3.75`, `""`, `"3.1"`)
- `int`/`string` → `float` (error if the string cannot be parsed, e.g.
  `""`, `"nothing"`)
- `string` → `bool`, accepting the same case-insensitive spellings as the
  explicit `bool()` conversion (RFC 0006): `"true"`/`"yes"`/`"on"`/`"1"`
  become `true` and `"false"`/`"no"`/`"off"`/`"0"` become `false` (error
  otherwise)
- `string` → `range_expr` (error if the string does not parse as a range
  expression, e.g. `""`, `"1-"`)
- `range_expr` → `list[int]` (the only list type; see below)
- `list[T]` → `list[U]` (element-wise coercion, recursively)
- `list[nulltype]` → `list[T]` for any `T` (an empty list is compatible
  with every list type)

Notably absent: `null` and lists do not convert to string, and nothing
converts to `nulltype`.

#### Satisfaction, then conversion

A target type need not be concrete. It may be `any`, a union, or a list
whose element type is either of those. Coercion stays well defined for
these by answering two questions in order, and never mixing them:

1. **Satisfaction** — does the value's type already satisfy the target,
   per [`ExprType::satisfies`](type-system.md#satisfaction)? Then the
   value is returned **unchanged**. RFC 0005 §"Implicit Type Coercion"
   calls for coercion only "where the intent is obvious"; a value that
   is already acceptable needs none.
2. **Conversion** — otherwise apply the table above. A conversion has to
   produce one specific type, so the target is first decomposed into its
   candidate **destinations**; the first destination that converts wins.

Because step 1 returns the value untouched, the result's type is the
*source* type, not the target: coercing a `list[int]` to `list[any]`
yields a `list[int]`. In both steps the result satisfies the target, but
only step 2 produces the target's own type.

Splitting the two is what makes abstract targets behave predictably.
Satisfaction is a **directional** relation and must not be confused with
[`match_type`](type-system.md), which is symmetric unification used to
bind type variables during signature dispatch: `match_type` reports a
match in both directions, so it would accept `list[T1]` as a target by
binding `T1` and discarding the binding.

#### Destinations

A **non-union** target is its own single destination.

A **union** target contributes each of its members, since converting to
any one of them satisfies the union. Members are tried **non-list before
list**: converting to a scalar produces a single value whose cost does
not depend on the source, while converting to a list materializes
elements and can fail on size limits. A `range_expr` against
`list[int] | string` therefore becomes the `string` its canonical form
already is, rather than expanding the range.

Within each shape group, destinations are tried in the **source type's
preference order** (RFC 0005). Two principles set it: a value prefers to
stay within its own kind — a number remains a number before it becomes
text — and a conversion that can fail is tried before one that always
succeeds, since a universal fallback tried first would make every
destination after it unreachable.

The full ordering, matching RFC 0005's destination table:

| Result type | Destinations, in order | Notes |
|-------------|------------------------|-------|
| `bool` | `string` | only conversion |
| `int` | `float`, then `string` | stays a number first; text is the universal fallback |
| `float` | `int`, then `string` | `int` succeeds only for exact whole values, so `3.0` against `int \| string` gives `3` while `3.5` gives `"3.5"` |
| `string` | `int`, then `float`, then `bool`, then `range_expr`, then `path` | every int-string also parses as float, so `int` first; `bool` and `range_expr` are selective parses, after the numeric ones; every string is a valid `path`, so `path` last |
| `path` | `string` | only conversion |
| `range_expr` | `string`, then `list[int]` | the non-list-first rule; when a target offers both, `list[int]` is unreachable — use the explicit `list()` conversion (RFC 0006) for the list |
| `list[S]` | list destinations in `S`'s order, applied to their element types | `list[float]` against `list[int] \| list[string]` tries `list[int]` first |

So `5` against `float | string` becomes `5.0`, and `"5"` against
`int | float` becomes `5` while `"5.0"` becomes `5.0` (the stricter parse
is tried first and the string's own lexical form routes it). Destinations
with equal rank keep the union's normalized order; this is what gives an
empty list its nominal element type — it follows the first list
destination in the union's normalized member order, per RFC 0005.

The `string` source's `bool` and `range_expr` destinations are parsed by
sharing [from_str_coerce](#from_str_coerce) in `convert_to`.

Because destinations are considered individually, a union target accepts
at least everything its members accept. Adding an alternative to a target
can never remove an accepted conversion. Acceptance is monotone in this
sense; the resulting *value* is not, since a newly added member may be
tried first — a `range_expr` yields a `list[int]` for a `list[int]` target
but the string `"1-3"` for `list[int] | string`.

`nulltype` is never a destination. RFC 0005 lists no `X → nulltype`
conversion, so `null` is reached only by a value already being `null` —
which step 1 handles. Turning the *text* `"null"` into `null` is a
transport-decode rule belonging to
[`from_str_coerce`](#from_str_coerce), not an implicit coercion, so it
fires for neither a bare `nulltype` target nor a union such as `int?`.

Discounting `nulltype` is also what lets a real conversion win where one
exists: against `path? | list[path]`, the string `"null"` becomes
`Path("null")`, since every string is a valid path.

Types that are not sets of runtime values contribute no destinations, and
nothing satisfies them, so they are rejected as targets outright:
**type variables** (`T`, `T1`, `T2`, `T3`), `noreturn`, `unresolved[T]`,
and signatures. Type variables are placeholders in generic function
signatures, resolved by signature matching before any value is coerced,
so a *target* containing one still unbound is always an error. (A
*source* containing one reads as a wildcard instead — see
[Unresolved Values](#unresolved-values).) The check
is recursive — `list[T1]` is no more usable than `T1` — with unions the
one exception: a union is usable when *any* member is, because the value
only has to land in one of them. (`call_arg_targets` drops symbolic
positions before building a union, so a union containing a type variable
cannot arise from signature dispatch.)

An **`any`** target is unconstrained (RFC 0005 lists it as "matches
anything"): every type satisfies it, so step 1 always succeeds, every
value is returned unchanged, and an `any` target can never be the reason
a coercion fails (issue #291, case C).

#### Consequences for the listed rules

Implicit rules do not chain: `list[int]` is the only list type a
`range_expr` implicitly coerces to (RFC 0005), so the materialized
`list[int]` is never widened element-wise toward the destination. The
destination is accepted exactly when a `list[int]` already satisfies it —
`list[int]`, `list[any]`, `list[int | string]` — and rejected otherwise,
including `list[float]`, `list[string]`, and `list[bool]`. Templates that
want the widened list chain the explicit `list()` conversion (RFC 0006),
whose `list[int]` result the `LIST[T] → LIST[U]` rule then applies to.

For union targets, the two steps give: a `string` value against
`int | string` stays a `string` and a `null` value against `T?` stays a
`null` (step 1); a `path` value against `string | int` becomes a `string`
(step 2, `path → string`); and a `path` against `int | float` errors, as
no non-destructive `path → int` conversion exists. An error names the
whole target rather than whichever destination was tried last.

### from_str_coerce

`ExprValue::from_str_coerce(s, target_type, path_format)` parses a string into a
typed value. Used when binding parameter values from their string representations
(e.g., CLI `-p Frame=42`, template parameter defaults, JSON transport decode).

| `target_type` | Rule | Example |
|---|---|---|
| `int` | `i64::from_str` | `"42"` → `Int(42)`; `"3.0"` → error |
| `float` | `f64::from_str`, rejecting NaN/Inf | `"3.14"` → `Float(3.14)` with `"3.14"` preserved |
| `bool` | case-insensitive `true`/`yes`/`on`/`1` vs `false`/`no`/`off`/`0` | `"Yes"` → `Bool(true)` |
| `string` | identity | `"hi"` → `String("hi")` |
| `path` | wrap with supplied `path_format` (no parsing) | `"/tmp"` → `Path { value: "/tmp", format }` |
| `range_expr` | `RangeExpr::from_str` | `"1-10"` → `RangeExpr(1..=10)` |
| `nulltype` | only `"null"` parses | `"null"` → `Null` |
| any other | error | — |

`from_str_coerce` is not a subset of the target-type coercion table above, and the
two are deliberately distinct: this is the entry point from outside the expression
language, where strings are the only transport format. Coercion between already-typed
values (e.g., `FLOAT` → `INT` for exact wholes) belongs to the target-type coercion
path and is absent here; conversely the `nulltype` row is a decode rule with no
target-type counterpart, so an expression producing the *text* `"null"` never
implicitly becomes `null`.

## JSON Transport Format

`ExprValue` supports JSON serialization for cross-process transport:

```rust
let json = value.to_json_transport();   // ExprValue → serde_json::Value
let value = ExprValue::from_json_transport(&json, PathFormat::Posix)?;  // reverse
```

The transport format uses `{"type", "value"}` objects where `type` is the `ExprType`
display string and scalar values are serialized as JSON strings:

| ExprValue | JSON |
|---|---|
| `Int(42)` | `{"type": "int", "value": "42"}` |
| `Float(3.14)` | `{"type": "float", "value": "3.14"}` |
| `String("hi")` | `{"type": "string", "value": "hi"}` |
| `Bool(true)` | `{"type": "bool", "value": "true"}` |
| `Path { value, .. }` | `{"type": "path", "value": "/tmp"}` |
| `ListInt([1,2])` | `{"type": "list[int]", "value": ["1", "2"]}` |
| `Null` | `{"type": "nulltype", "value": ""}` |

`from_json_transport` takes a `PathFormat` parameter to construct path values with the
correct format for the receiving process.

### Shared Format with SerializedSymbolTable

The `{"type", "value"}` encoding is shared with `SerializedSymbolTable` (see
symbol-table.md). The `SymbolTable` serializer calls `transport_value()` for each entry's
value, and the deserializer calls `from_transport_value()` to reconstruct it — the same
internal methods that `to_json_transport()` and `from_json_transport()` use. The only
difference is that `SerializedSymbolTable` entries add a `"name"` field for the dotted
path:

```json
// ExprValue transport (to_json_transport):
{"type": "int", "value": "42"}

// SerializedSymbolTable entry (same type/value encoding + name):
{"name": "Param.Frame", "type": "int", "value": "42"}
```

This shared encoding means that any changes to value serialization (e.g., adding a new
type) automatically apply to both individual value transport and symbol table
serialization.

## Unresolved Values

`ExprValue::Unresolved(ExprType)` carries type information without a concrete value.
Used during template validation when parameter values aren't known yet:

```rust
// Build symbol table with type placeholders
let mut symtab = SymbolTable::new();
symtab.set("Param.Frame", ExprValue::unresolved(ExprType::INT));
symtab.set("Param.Name", ExprValue::unresolved(ExprType::STRING));

// Evaluate — catches type errors without runtime values
let result = evaluate_expression("Param.Frame + Param.Name", &symtab);
// → TypeError: cannot add int and string
```

Unresolved values are **type-only placeholders**: they carry an `ExprType` but no
concrete data. Because they're wrapped values, they can pass through the evaluator's
memory tracking and dispatch without a special code path. `Display` on an unresolved
value renders as `unresolved[T]` for debug/error output.

All operations involving unresolved values use existential validation. For an
`unresolved[T]` operand, `T` describes the set of concrete values and types to which
that operand could resolve. With multiple unresolved operands, the possibilities are
all combinations of their permitted resolutions.

An operation succeeds symbolically if at least one possible combination would
succeed. Possibilities that would fail are discarded. If no combination can succeed,
the operation fails immediately. Otherwise, unless evaluation can prove a concrete
result independent of the unresolved inputs, the result is unresolved and its
constraint is the union of the result types from all successful possibilities.

As expression processing progresses and unresolved operands become narrower or
concrete, the same operation is evaluated with fewer possibilities. It may then
produce a concrete result or report a value-dependent error that could not be proven
at an earlier stage.

Target-type coercion applies the same two steps to unresolved types that it
applies to concrete values. The payload remains unresolved, but its type is
narrowed to the coercion result. For example, `unresolved[int]` against a
`string` target becomes `unresolved[string]`, and
`unresolved[int | string]` against an `int` target becomes `unresolved[int]`.
Likewise, coercing `unresolved[int | list[int]]` to `int` succeeds as
`unresolved[int]`: the `int` possibility succeeds even though the `list[int]`
possibility cannot.

The result is the type the applicable rule produces, which is not always the
target. Satisfaction returns the source type, so `unresolved[list[int]]`
against a `list[any]` target stays `unresolved[list[int]]` rather than widening
to `unresolved[list[any]]` — matching the concrete `list[int]` value, which is
returned unchanged. Conversion returns the type its own rule produces, so
`unresolved[range_expr]` against `list[any]` becomes `unresolved[list[int]]`,
because materializing a range only ever yields a `list[int]`.

The result always satisfies the target, and it always **describes** the
concrete result: the concrete result's type satisfies the narrowed
constraint. The type level cannot see the payload that decides which
destination of a union target wins, so conversion narrows to the union of
every destination with a type-level rule rather than betting on any one of
them. Against `int | string`, an `unresolved[float]` narrows to
`unresolved[int | string]`: a `Float(3.0)` payload takes the `float → int`
rule while a `Float(3.5)` payload fails it and falls through to `string`,
and both outcomes lie within the constraint. For a non-union target exactly
one destination exists, so the constraint is exactly the type evaluation
will produce.

Checks that require a concrete payload are deferred until runtime. For example,
`unresolved[string]` can narrow to `unresolved[int]`; once resolved, the string
must still parse as an integer. Any `unresolved[list[S]]` against any `list[U]`
target is accepted for the same reason: the value could resolve to the empty
list, which coerces to every list type, so element compatibility can only be
checked once the payload is known. A source and target with no type-level
coercion rule at all, such as `unresolved[list[int]]` against `int`, is
rejected during validation.

A source constraint may contain unbound type variables. Generic return
types can leave one unbound when an argument is unresolved — list
concatenation's signature is `(list[T1], list[T2]) -> list[T3]`, and no
parameter binds `T3` — and callers of the public API can construct such
constraints directly. A concrete value can never have a type variable in
its type, so on the source side an unbound variable reads as a
**wildcard**: it is erased to `any` before the rules apply. The shape
around it still constrains the outcome — `list[T1]` behaves exactly as
`list[any]`, converting to other list types but never to a scalar — and
a bare `T1` behaves as `any` itself. When a wholly unknown source is
promised the target, a union target is first filtered to its
value-denoting members, so `unresolved[any]` against `int | T1` narrows
to `unresolved[int]` — never to a constraint that fails to satisfy the
target it was coerced to.

The two directions are deliberately asymmetric, and only in one direction.
Unresolved coercion may accept a pair the concrete value later rejects, because
the deciding information is a payload the placeholder does not carry. It must
never reject a pair the concrete value would accept: doing so fails a template at
validation time that would have run correctly, which no later stage can recover
from. Any such case is a bug in the type-level table, not a deliberate
narrowing — the `range_expr → list[int]` and type-variable-target rules above apply
identically on both paths for this reason.

These invariants — never-reject, results satisfying the target, the concrete
result's type satisfying the narrowed constraint, and non-union targets
narrowing to exactly the concrete result type — are enforced mechanically by
`tests/integration/test_coercion_drift.rs`, which sweeps a catalog of sample
values against a catalog of target types and compares the concrete and
type-level outcomes.
