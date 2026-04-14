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

1. All same type → use that typed variant directly
2. Mix of INT and FLOAT → promote all to FLOAT (`ListFloat`)
3. Mix of PATH and STRING → promote all to STRING (`ListString`)
4. Nested lists → `ListList` (validates max 2 nesting levels)
5. Empty list → variant selected by `hint_type` (defaults to `ListInt` for `list[nulltype]`, compatible with any list type)

Nested list promotion rules (applied when elements are themselves lists):

- `list[int]` + `list[float]` → promotes inner `ListInt` elements to `ListFloat`
- `list[path]` + `list[string]` → promotes inner `ListPath` elements to `ListString`

This matches the Python `_from_list` logic but produces typed Rust vectors instead of
tagged `ExprValue` vectors.

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
