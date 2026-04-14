# Function Library

## Overview

`FunctionLibrary` is a registry of function overloads with signature-based multiple
dispatch.

Defined in `function_library.rs` (dispatch and registration) and `default_library.rs`
(default library construction). Function implementations live in `functions/`.

## Design Rationale

Making function signatures first-class data provides four key capabilities:

1. **Static type checking** — return types can be determined from argument types without
   running the expression, which the model layer needs for template validation
2. **Extensibility** — host-context functions like `apply_path_mapping` are added by
   merging a separate library, without modifying the evaluator
3. **Single source of type truth** — operator and function type rules live in signatures
   rather than being duplicated between dispatch and type inference
4. **Introspection** — callers can query whether a function call is valid for given types,
   enabling better error messages and tooling

## FunctionEntry

Each registered overload pairs a type signature with a function pointer:

```rust
pub struct FunctionEntry {
    pub signature: ExprType,  // TypeCode::Signature — e.g., (int, int) -> int
    pub implementation: fn(&mut dyn EvalContext, &[ExprValue]) -> Result<ExprValue, ExpressionError>,
}
```

Function pointers (not closures) keep `FunctionEntry` simple, `Clone`, and `Send + Sync`.
Functions that need evaluator state (path format, path mapping rules) access it through
the `EvalContext` trait.

## EvalContext Trait

```rust
pub trait EvalContext {
    fn path_format(&self) -> PathFormat;
    fn path_mapping_rules(&self) -> &[PathMappingRule];
    fn count_op(&mut self) -> Result<(), ExpressionError>;
    fn count_ops(&mut self, n: usize) -> Result<(), ExpressionError>;
    fn count_string_ops(&mut self, len: usize) -> Result<(), ExpressionError>;
    fn get_or_compile_regex(&mut self, pattern: &str) -> Result<regex::Regex, ExpressionError> {
        // Default: compile without caching
        regex::Regex::new(pattern).map_err(|e| ExpressionError::new(format!("Invalid regex: {e}")))
    }
}
```

`get_or_compile_regex` has a default implementation that compiles the pattern on every
call. The evaluator overrides it with a caching version that stores compiled regexes
for reuse across repeated calls with the same pattern.

The evaluator implements `EvalContext` directly. This trait boundary prevents function
implementations from calling evaluation methods (like `evaluate` or `dispatch`),
enforcing the separation between evaluation control flow and pure function logic.

## Registration

```rust
let mut lib = FunctionLibrary::new();

// From ExprType signature
lib.register("abs", ExprType::signature(vec![ExprType::INT], ExprType::INT), abs_int);

// From spec notation string (convenient for bulk registration)
lib.register_sig("abs", "(int) -> int", abs_int);
lib.register_sig("abs", "(float) -> float", abs_float);
lib.register_sig("min", "(list[T1]) -> T1", min_list);
```

## Three-Phase Dispatch

```rust
library.call(name, &args, ctx)
```

1. **Exact non-generic match** — signature params match arg types exactly (types derived from args)
2. **Non-generic with coercion** — try implicit coercions (INT→FLOAT, PATH→STRING);
   skip receiver coercion for method calls to prevent `42.upper()`
3. **Generic match** — bind type variables (T, T1, T2, T3) and check consistency

If no match is found, the error message includes:
- The function name (using operator symbols for dunders: `__add__` → `+`)
- The argument types provided
- Available signatures for that name
- "Did you mean?" suggestions via edit distance for unknown function names

## Operator Mapping

Operators are registered as dunder-named functions:

| Operator | Function name | Example signatures |
|----------|--------------|-------------------|
| `+` | `__add__` | `(int, int) -> int`, `(string, string) -> string`, `(path, string) -> path`, `(list[T1], list[T1]) -> list[T1]` |
| `-` | `__sub__` | `(int, int) -> int`, `(float, float) -> float` |
| `*` | `__mul__` | `(int, int) -> int`, `(string, int) -> string`, `(list[T1], int) -> list[T1]` |
| `/` | `__truediv__` | `(int, int) -> float`, `(path, string) -> path`, `(path, path) -> path` |
| `//` | `__floordiv__` | `(int, int) -> int`, `(float, float) -> int` |
| `%` | `__mod__` | `(int, int) -> int`, `(float, float) -> float` |
| `**` | `__pow__` | `(int, int) -> int`, `(float, float) -> float` |
| unary `-` | `__neg__` | `(int) -> int`, `(float) -> float` |
| unary `+` | `__pos__` | `(int) -> int`, `(float) -> float` |
| `not` | `__not__` | `(bool) -> bool` |
| `==` | `__eq__` | `(T1, T2) -> bool` |
| `!=` | `__ne__` | `(T1, T2) -> bool` |
| `<` | `__lt__` | `(T1, T2) -> bool` |
| `<=` | `__le__` | `(T1, T2) -> bool` |
| `>` | `__gt__` | `(T1, T2) -> bool` |
| `>=` | `__ge__` | `(T1, T2) -> bool` |
| `x[i]` | `__getitem__` | `(list[T1], int) -> T1`, `(string, int) -> string`, `(range_expr, int) -> int`, plus slice overloads |
| `in` | `__contains__` | `(list[T1], T1) -> bool`, `(string, string) -> bool`, `(range_expr, int) -> bool` |
| `not in` | `__not_contains__` | Same as `__contains__` |

## Property Access

Properties are registered as `__property_NAME__` functions:

```rust
lib.register_sig("__property_name__", "(path) -> string", path_name);
lib.register_sig("__property_stem__", "(path) -> string", path_stem);
lib.register_sig("__property_suffix__", "(path) -> string", path_suffix);
lib.register_sig("__property_suffixes__", "(path) -> list[string]", path_suffixes);
lib.register_sig("__property_parent__", "(path) -> path", path_parent);
lib.register_sig("__property_parts__", "(path) -> list[string]", path_parts);
```

The evaluator's `eval_attribute` dispatches to `__property_NAME__` when the attribute
isn't found as a variable in the symbol table.

## Static Type Checking

The library supports type-level queries without evaluation:

```rust
// What type does len(list[int]) return?
lib.derive_return_type("len", &[ExprType::LIST_INT])  // → Some(ExprType::INT)

// What type is path.name?
lib.get_property_type(&ExprType::PATH, "name")  // → Some(ExprType::STRING)
```

This is used by the model layer for template validation with unresolved values.

## Sub-Library Composition

Each function category is a separate module returning its own `FunctionLibrary`. The
default library merges them:

```rust
pub fn default_library() -> FunctionLibrary {
    FunctionLibrary::new()
        .merge(arithmetic::library())
        .merge(string_ops::library())
        .merge(list_ops::library())
        .merge(comparison::library())
        .merge(math_ops::library())
        .merge(path_ops::library())
        .merge(repr_ops::library())
        .merge(regex_ops::library())
        .merge(conversion::library())
        .merge(misc::library())
}
```

Each sub-library is self-contained and testable in isolation.

## Caching

The default library is immutable after construction and contains only `fn` pointers,
so it's `Send + Sync`. It's cached in a global static via `LazyLock`:

```rust
static DEFAULT_LIBRARY: LazyLock<FunctionLibrary> = LazyLock::new(default_library);

pub fn get_default_library() -> &'static FunctionLibrary {
    &DEFAULT_LIBRARY
}
```

## Host Context

Host-context functions (like `apply_path_mapping`) are unavailable during template
validation but available during runtime evaluation on a worker host. They're added
via `with_host_context()`:

```rust
let lib = get_default_library().clone().merge(host_library());
```

Cloning copies the `HashMap<String, Vec<FunctionEntry>>` (all `fn` pointers, no
heap-allocated closures), so this is cheap.

## Function Coverage

200 Rust signatures registered in `default_library.rs`. All function names from the
specification are present. Some per-type overloads use generic `T1` signatures where
the specification spells out per concrete type (e.g., `repr_pwsh` has separate
signatures for string, int, float, bool, path, range_expr, list in the spec but uses
generic `T1` in Rust).

## Function Semantics

Individual function behaviors (arithmetic semantics, string methods, regex restrictions,
path operations, etc.) are defined in the
[OpenJD Expression Language specification](https://github.com/OpenJobDescription/openjd-specifications/wiki/2026-02-Expression-Language),
sections 2.1 (Operators) and 2.2 (Built-in Functions). Key implementation choices:

- **Integer arithmetic** uses Python-style floored division and modulo (§2.1.1)
- **`round()`** uses banker's rounding / round-half-even (§2.2.2)
- **Regex functions** reject lookahead, lookbehind, backreferences, and `\Z` (§2.2.5)
- **`repr_sh/cmd/pwsh`** produce shell-safe quoting per platform conventions (§2.2.6)
- **Path operations** are format-aware (POSIX/Windows/URI) without using `std::path` (§2.3)
- **Slicing a `range_expr`** returns `range_expr` for positive step, `list[int]` for
  negative step (§2.1.8)
