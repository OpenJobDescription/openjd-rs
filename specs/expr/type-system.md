# Type System

## Overview

`ExprType` represents the type of an expression value. It supports concrete types,
parameterized types (lists, unions), type variables for generic function signatures,
and an `Unresolved` wrapper for static type checking.

The type system is defined in the specification (RFC 0005 §Type System) and implemented
in `types.rs`.

## TypeCode

```rust
pub enum TypeCode {
    Null,        // null literal
    Bool,        // true / false
    Int,         // 64-bit signed integer
    Float,       // 64-bit IEEE 754 (no NaN, Inf, or -0.0)
    String,      // UTF-8 string
    Path,        // Filesystem or URI path with format
    List,        // Homogeneous list with element type parameter
    RangeExpr,   // Sorted non-overlapping integer ranges
    Any,         // Matches any type (for generic signatures)
    Union,       // Union of multiple types
    NoReturn,    // Return type of `fail()` — never produces a value
    Unresolved,  // Type-only placeholder for static checking
    TypeVarT,    // Generic type variable T
    TypeVarT1,   // Generic type variable T1
    TypeVarT2,   // Generic type variable T2
    TypeVarT3,   // Generic type variable T3
    Signature,   // Function signature: (params) -> return
}
```

## ExprType Construction

Pre-defined constants for common types:

```rust
ExprType::INT           // TypeCode::Int, no params
ExprType::FLOAT         // TypeCode::Float
ExprType::STRING        // TypeCode::String
ExprType::BOOL          // TypeCode::Bool
ExprType::PATH          // TypeCode::Path
ExprType::NULLTYPE      // TypeCode::NullType
ExprType::NORETURN      // TypeCode::NoReturn
ExprType::RANGE_EXPR    // TypeCode::RangeExpr
ExprType::ANY           // TypeCode::Any
ExprType::T, T1, T2, T3 // Type variables
```

Parameterized constructors:

```rust
ExprType::list(ExprType::INT)                    // list[int]
ExprType::union(vec![ExprType::INT, ExprType::STRING])  // int | string
ExprType::unresolved(ExprType::INT)              // unresolved[int]
ExprType::signature(params, return_type)         // (params) -> return
ExprType::parse("(list[T1], int) -> T1")         // From spec notation
```

## Type Matching and Substitution

Type matching is the core of generic function dispatch. Given a signature type and
concrete argument types, `match_call` determines whether the call is valid and binds
type variables:

```rust
let sig = ExprType::parse("(list[T1], int) -> T1");
let bindings = sig.match_call(&[ExprType::LIST_INT, ExprType::INT]);
// → Some({T1: int})

let ret = sig.sig_return().substitute(&bindings);
// → int
```

Key methods:

| Method | Purpose |
|--------|---------|
| `match_call(&[ExprType]) -> Option<HashMap>` | Match argument types, bind type variables |
| `resolve_call(&[ExprType]) -> Option<ExprType>` | Match + resolve return type in one step |
| `substitute(&HashMap) -> ExprType` | Replace type variables with bound types |
| `is_symbolic() -> bool` | Contains type variables (T, T1, T2, T3) |
| `is_concrete() -> bool` | No type variables or unresolved wrappers |
| `satisfies(&ExprType) -> bool` | Directional: may a value of this type pass unchanged where the argument is required? |
| `sig_params() -> &[ExprType]` | Parameter types of a Signature |
| `sig_return() -> &ExprType` | Return type of a Signature |

Crate-internal helpers (`pub(crate)`, not part of the public API):

| Method | Purpose |
|--------|---------|
| `denotes_runtime_values() -> bool` | Does this type denote a set of runtime values? Gates satisfaction and coercion targets |
| `erase_type_vars() -> ExprType` | Replace every type variable with `any` — the wildcard reading of unbound variables in coercion sources |

## Union Normalization

Union types are normalized on construction (matching the Python implementation):

1. **Flatten** — `union[int, union[string, float]]` → `union[int, string, float]`
2. **Deduplicate** — `union[int, int, string]` → `union[int, string]`
3. **ANY absorption** — `union[int, any]` → `any`
4. **NORETURN collapse** — `union[int, noreturn]` → `int`
5. **Unresolved hoisting** — `union[unresolved[int], unresolved[string]]` → `unresolved[union[int, string]]`
6. **Singleton unwrap** — `union[int]` → `int`
7. **Sort** — deterministic ordering by string representation (`to_string()`)

## Unresolved Type Normalization

Unresolved types are always the outermost constructor:

- `list[unresolved[T]]` → `unresolved[list[T]]`
- `unresolved[unresolved[T]]` → `unresolved[T]`

This ensures that checking `is_unresolved()` on a value is sufficient to know whether
it carries a concrete value, without needing to inspect nested types.

## Satisfaction

`satisfies` answers "may a value of this type be used, unchanged, where
`target` is required?" It is the test [target-type coercion](values.md#coercion)
uses to decide whether any conversion is needed at all.

| Case | Rule |
|------|------|
| `any` target | Always satisfied — RFC 0005 lists `any` as "matches anything" |
| Union **source** | Satisfied only when **every** member is, since any one could be the runtime type |
| Union **target** | Satisfied when **any** member is |
| `list[T]` | Covariant in the element: `list[int]` satisfies `list[any]` and `list[int \| string]` |
| `unresolved[T]` source | Delegates to `T`, the type the resolved value will have |
| `any` / type-variable source | Satisfies nothing narrower — an unknown type is not *known* to belong to a smaller set |
| Type variable, `noreturn`, `unresolved[T]`, signature target | Never satisfied; see `denotes_runtime_values` below |
| Otherwise | The two types must be equal |

The union source case is checked before the union target case. Testing the
target first would ask whether the *whole* source satisfies a single member,
which no multi-member union does — not even itself.

### Contrast with `match_type`

The two are easy to confuse and are not interchangeable:

| | `satisfies` | `match_type` |
|---|---|---|
| Direction | Directional | Symmetric |
| Purpose | May this value pass through unchanged? | Unify two types, binding type variables |
| Used by | Target-type coercion | Generic function signature dispatch |
| `int` vs `any` | `int.satisfies(any)` yes; `any.satisfies(int)` no | Match in both directions |
| Type variables | Never satisfied — there is no binding to record | Bind on whichever side they appear |

Using `match_type` to decide coercion would accept a `list[T1]` target by
binding `T1` and then discarding the binding, producing a value that does not
actually satisfy the requested type.

### `denotes_runtime_values`

A crate-internal predicate (`satisfies` and coercion expose its effects).
Type variables, `noreturn`, `unresolved[T]`, and signatures do not denote sets
of runtime values: no value can ever be one of them. Nothing satisfies them,
they are rejected as coercion targets, and the check is recursive, since these
codes are just as unusable nested — `list[T1]` cannot be built without a
binding for `T1` any more than `T1` can.

Unions are the one exception: a union is usable when **any** member is, because
a value only has to land in one of them, and both `satisfies` and the coercion
destination list consider members individually. A union containing a type
variable cannot arise from signature dispatch anyway — `call_arg_targets` drops
symbolic positions before building one.

## Implicit Coercions

Two implicit coercions are applied during function dispatch:

| From | To | Rationale |
|------|----|-----------|
| INT | FLOAT | Lossless widening (i64 → f64 may lose precision for large values, but this matches Python semantics) |
| PATH | STRING | Every path has a string representation |

These coercions are tried in the second phase of dispatch (after exact match fails).
Method calls skip receiver coercion — `"hello".upper()` works but `42.upper()` does not
try coercing 42 to a string.

## Divergence from Python

The Rust implementation adds `TypeCode::Signature` for representing function signatures
as types, enabling the `FunctionLibrary` to store signatures as `ExprType` values. The
Python implementation uses a separate `FunctionSignature` dataclass instead.
