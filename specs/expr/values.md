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
    Path { value: String, format: PathFormat },
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
that negative zero is not observable.

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
ExprValue::Path { value: "/tmp/out".into(), format: PathFormat::Posix }
ExprValue::Null
ExprValue::Bool(true)

// Lists — make_list(elements, hint_type) handles type promotion
ExprValue::make_list(vec![ExprValue::Int(1), ExprValue::Int(2)], ExprType::NULLTYPE)  // → ListInt
ExprValue::make_list(vec![ExprValue::Int(1), ExprValue::Float(..)], ExprType::NULLTYPE)  // → ListFloat (int→float)
ExprValue::make_list(vec![ExprValue::Path{..}, ExprValue::String(..)], ExprType::NULLTYPE)  // → ListString (path→string)
ExprValue::make_list(vec![], ExprType::INT)  // → ListInt (empty, hint selects variant)

// Unresolved — type-only placeholder for static checking
ExprValue::unresolved(ExprType::INT)
```

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
explicitly: Int↔Float compares via `(i as f64) == f`, String↔Path compares the string
values, and list↔list iterates element-wise using `equals()` recursively. List↔RangeExpr
comparison materializes the range and compares element-by-element.

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

Method calls skip receiver coercion to prevent nonsensical calls like `42.upper()`.

### Target Type Coercion (after evaluation, for format string context)

Applied when the evaluation result needs to match an expected type:

- any → STRING (via `to_string()`)
- STRING → PATH
- FLOAT → INT (only if exact, e.g., `3.0` → `3`)
- STRING → INT (parse)
- STRING → FLOAT (parse)
- INT → FLOAT
- RANGE_EXPR → STRING
- RANGE_EXPR → LIST[INT]
- LIST[T] → LIST[U] (element-wise coercion)

### from_str_coerce

`ExprValue::from_str_coerce(s, target_type)` parses a string into a typed value,
used when binding parameter values from their string representations.

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

When any argument to a function is unresolved, the function returns
`Unresolved(return_type)` instead of computing a value. This propagates type information
through the entire expression, catching type mismatches at validation time.

Calling `item()` or `to_string()` on an unresolved value panics — they are type-only
placeholders that must never reach runtime evaluation.
