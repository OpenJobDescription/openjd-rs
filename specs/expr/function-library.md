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

Each registered overload pairs a type signature with a function implementation:

```rust
pub type FunctionImpl = Arc<
    dyn Fn(&mut dyn EvalContext, &[ExprValue]) -> Result<ExprValue, ExpressionError>
        + Send
        + Sync,
>;

pub struct FunctionEntry {
    pub signature: ExprType,     // TypeCode::Signature — e.g., (int, int) -> int
    pub implementation: FunctionImpl,
}

pub struct FunctionLibrary {
    functions: HashMap<String, Vec<FunctionEntry>>,
}
```

The implementation is an `Arc<dyn Fn … + Send + Sync>` — not a bare `fn`
pointer — so closures that capture environment (host state, AWS clients,
config) can be registered alongside plain functions. `Arc` (rather than `Box`)
keeps `FunctionLibrary` `Clone`, which many call sites rely on when cloning the
default library to add host-context extensions.

Whether host-only functions like `apply_path_mapping` are available on a
library instance is determined by whether they are registered — check with
`get_signatures("apply_path_mapping")`. `FunctionLibrary::for_profile`
registers them when the profile's `host_context` is
`HostContext::WithRules(...)` or `HostContext::Unresolved`.

Function pointers and closures both work — `Arc<dyn Fn + Send + Sync>` is
`Clone` and thread-safe, preserving `FunctionLibrary: Clone + Send + Sync`.
Functions that need evaluator state (path format, resource counting, regex
cache) access it through the `EvalContext` trait. Functions that need *host*
state (e.g. path mapping rules for `apply_path_mapping`) capture it via a
closure registered at library-construction time — see
[Host Context](#host-context) below.

## EvalContext Trait

```rust
pub trait EvalContext {
    fn path_format(&self) -> PathFormat;
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
call via `RegexBuilder` with a 1 MiB compiled-program size limit (to defend against
adversarial patterns). The evaluator overrides it with a caching version that stores
compiled regexes for reuse across repeated calls with the same pattern — the cache
is per-evaluation, not global.

The evaluator implements `EvalContext` directly. This trait boundary prevents function
implementations from calling evaluation methods (like `evaluate` or `dispatch`),
enforcing the separation between evaluation control flow and pure function logic.

## Registration

```rust
let mut lib = FunctionLibrary::new();

// Plain function pointer
lib.register("abs", ExprType::signature(vec![ExprType::INT], ExprType::INT), abs_int);

// From spec notation string (convenient for bulk registration)
lib.register_sig("abs", "(int) -> int", abs_int);
lib.register_sig("abs", "(float) -> float", abs_float);
lib.register_sig("min", "(list[T1]) -> T1", min_list);

// Closures work too — useful for host-integrated functions that wrap
// captured state (AWS clients, config, caches). Must be `Send + Sync + 'static`.
let prefix = "host-".to_string();
lib.register_sig(
    "brand",
    "(string) -> string",
    move |_ctx: &mut dyn EvalContext, a: &[ExprValue]| match &a[0] {
        ExprValue::String(s) => Ok(ExprValue::String(format!("{prefix}{s}"))),
        _ => Err(ExpressionError::type_error("expected string")),
    },
);
```

## Three-Phase Dispatch

```rust
library.call(name, &args, ctx)
```

1. **Exact non-generic match** — signature params match arg types exactly (types derived from args)
2. **Non-generic with coercion** — try implicit coercions (INT→FLOAT, PATH→STRING);
   skip receiver coercion for method calls to prevent `42.upper()`
3. **Generic match** — bind type variables (T, T1, T2, T3) and check consistency

The method-vs-function distinction is made by the evaluator before dispatch:
`eval_call` transforms `obj.method(args)` into `method(obj, args)` via UFCS and sets
a per-call flag on the dispatch indicating that position 0 is a receiver. The library
honors that flag in phase 2 by declining to coerce arg 0. Ordinary function calls
carry no such flag and coerce all arguments uniformly.

If no match is found, the error message includes:
- The function name (using operator symbols for dunders: `__add__` → `+`)
- The argument types provided
- Available signatures for that name
- "Did you mean?" suggestions via edit distance for unknown function names

## Operator Mapping

Operators are registered as dunder-named functions:

| Operator | Function name | Example signatures |
|----------|--------------|-------------------|
| `+` | `__add__` | `(int, int) -> int`, `(string, string) -> string`, `(path, string) -> path`, `(list[T1], list[T2]) -> list[T3]` |
| `-` | `__sub__` | `(int, int) -> int`, `(float, float) -> float` |
| `*` | `__mul__` | `(int, int) -> int`, `(string, int) -> string`, `(list[T1], int) -> list[T1]` |
| `/` | `__truediv__` | `(int, int) -> float`, `(path, string) -> path`, `(path, path) -> path` |
| `//` | `__floordiv__` | `(int, int) -> int`, `(float, float) -> int` |
| `%` | `__mod__` | `(int, int) -> int`, `(float, float) -> float` |
| `**` | `__pow__` | `(int, int) -> float\|int`, `(float, float) -> float` |
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

## Static Type Derivation

The library supports type-level queries without evaluation:

```rust
// What type does len(list[int]) return?
lib.derive_return_type("len", &[ExprType::list(ExprType::INT)])  // → Some(ExprType::INT)

// What type is path.name?
lib.get_property_type(&ExprType::PATH, "name")  // → Some(ExprType::STRING)
```

This is used by the model layer for template validation with unresolved values.

### Non-Union Fast Path

When no argument type is a union, `derive_return_type` tries each registered signature
in two passes:

1. **Exact match** — `match_call` checks each parameter against the argument type,
   binding type variables (T1, T2, etc.) and checking consistency
2. **Coerced match** — applies implicit coercions (INT→FLOAT, PATH→STRING) to the
   argument types and retries

This is O(S) where S is the number of signatures for the function name.

### Union Path: Per-Signature Recursive Matching

When any argument is a union type (e.g., `union[int, string]`), the function must
determine all possible return types across all valid type combinations. The naive
approach — generating the Cartesian product of all union members upfront — is O(M^N)
where M is the max union size and N is the argument count.

Instead, `derive_return_type` uses per-signature recursive matching (matching the
Python implementation's `_match_signature` approach):

1. Flatten each argument's types into a set (union members + implicit coercions)
2. For each registered signature, recurse through argument positions:
   - At position `i`, try each type from arg `i`'s set against the signature's
     parameter `i`
   - If `param.match_type(arg_type)` fails → prune (skip all deeper combinations)
   - If a type variable binding conflicts with an earlier binding → prune
   - If all positions match → record the substituted return type
3. Collect, deduplicate, and return as a union (or single type if all paths agree)

This prunes aggressively in practice because most signatures are concrete. For example,
with signature `(int, int) -> int` and args `[union[int, string, path], union[int, float]]`:
- The Cartesian product approach generates 6 combinations and checks each
- The recursive approach tries `int` at arg 0 → matches → recurses to arg 1 (2 checks),
  then tries `string` at arg 0 → fails immediately (0 further checks), then `path` →
  fails immediately. Total: 4 checks instead of 6

The savings grow with more arguments and larger unions. For generic signatures (e.g.,
`(T1, T2) -> bool` for comparison operators), pruning is less effective since most
types match, but binding conflict detection still prunes some branches (e.g., `(T, T)`
rejects `(int, string)`).

## Sub-Library Composition

Each function category is a module-private free function returning its own
`FunctionLibrary`. The default library merges them inside `default_library.rs`:

```rust
fn build_default_library() -> FunctionLibrary {
    FunctionLibrary::new()
        .merge(arithmetic())
        .merge(string_ops())
        .merge(list_ops())
        .merge(comparison())
        .merge(math_ops())
        .merge(string_functions())
        .merge(list_functions())
        .merge(conversion())
        .merge(path_ops())
        .merge(repr_ops())
        .merge(regex_ops())
        .merge(misc())
}
```

The category builders (`arithmetic()`, `string_ops()`, …) are not part of the public
API; the publicly exported entry point is
[`FunctionLibrary::for_profile(&ExprProfile)`](#for_profile).

## Caching

The default library skeleton (no host context) is immutable and contains only
`fn` pointers, so it is `Send + Sync`. A crate-private `LazyLock<FunctionLibrary>`
holds it; `FunctionLibrary::for_profile` builds on top of it per the profile's
host-context setting and caches the result in a per-profile `Arc` map keyed
on the rules-independent portion of the profile (see
[profile.md § ProfileKey](../../crates/openjd-expr/src/profile.rs)).

```rust
pub fn for_profile(profile: &ExprProfile) -> Arc<FunctionLibrary>;
```

Profiles that differ only in their `Arc<Vec<PathMappingRule>>` share a single
cached no-host skeleton; the rules closure is applied on top as a cheap clone
per call.

## Host Context

Host-context functions (like `apply_path_mapping`) need host-supplied state
(path mapping rules) that the expression evaluator has no knowledge of. They
are registered on a library obtained from `FunctionLibrary::for_profile`
when the profile carries a non-`None` host context:

```rust
use std::sync::Arc;
use openjd_expr::{ExprProfile, FunctionLibrary, HostContext, PathMappingRule};

let rules: Vec<PathMappingRule> = /* ... */;

// Runtime: real rules baked into an `apply_path_mapping` closure.
let profile = ExprProfile::current()
    .with_host_context(HostContext::with_rules(rules));
let lib: Arc<FunctionLibrary> = FunctionLibrary::for_profile(&profile);
assert!(!lib.get_signatures("apply_path_mapping").is_empty());
```

`HostContext::with_rules(Vec<PathMappingRule>)` takes ownership of the rules
and wraps them in an `Arc` internally. Callers that already have an
`Arc<Vec<PathMappingRule>>` can construct `HostContext::WithRules(arc)`
directly. The `apply_path_mapping` closure captures the `Arc` by reference,
so many libraries built from the same rules share a single allocation.

For template validation at job creation time (when path mapping rules aren't
available yet), use `HostContext::Unresolved`. This registers the
host-context function signatures with stub implementations that return
`Unresolved(PATH)`:

```rust
let profile = ExprProfile::current()
    .with_host_context(HostContext::Unresolved);
let lib = FunctionLibrary::for_profile(&profile);
assert!(!lib.get_signatures("apply_path_mapping").is_empty());
```

This allows the type checker to verify that calls to host-context functions
are well-typed without requiring actual rules. The validation stub takes no
arguments — rules don't matter for a signature-only check.

### Low-level primitives

`FunctionLibrary::for_profile` composes internally from two crate-public
free functions in `default_library`, exposed for advanced callers that
want to layer host-context functions onto a custom-built library:

```rust
pub fn default_library::register_host_context_functions(
    lib: &mut FunctionLibrary,
    rules: Arc<Vec<PathMappingRule>>,
);

pub fn default_library::register_unresolved_host_context_functions(
    lib: &mut FunctionLibrary,
);
```

Most callers should not call these directly — go through
`FunctionLibrary::for_profile(&profile)` to get the cached result. These
primitives are documented in [public-api.md § Default Library](public-api.md#default-library-default_library).

### Rule-change semantics

Rules are captured at the point `FunctionLibrary::for_profile` resolves a
`HostContext::WithRules(rules)` profile. The `apply_path_mapping` closure
holds an immutable `Arc<Vec<PathMappingRule>>` reference for the library's
lifetime. If the host's rules change (e.g. `openjd-sessions` accumulates
path mapping rules across let-binding evaluations), the host must rebuild
the library by calling `for_profile` with a new rules-carrying profile:

```rust
// Rebuild with updated rules.
let updated_profile = ExprProfile::current()
    .with_host_context(HostContext::with_rules(new_rules));
let updated_lib = FunctionLibrary::for_profile(&updated_profile);
```

The underlying no-host skeleton is cached and shared across `for_profile`
calls, so the per-call cost is a `HashMap` clone plus one
`apply_path_mapping` registration — not a full library rebuild.

### Host-Context Function Inventory

The host-context namespace is deliberately small. The only function currently
registered is:

| Function | Signature | Runtime behavior | Validation stub |
|---|---|---|---|
| `apply_path_mapping` | `(string) -> path` | Applies the closure-captured path mapping rules to the string, returning a `path` in the configured output format (or the original string normalized to that format if no rule matches). | Returns `Unresolved(path)` so type checking proceeds without real rules. |

Adding a new host-context function that captures host state:

1. Write a closure factory `make_foo_fn(state: Arc<State>) -> impl Fn(&mut dyn EvalContext, &[ExprValue]) -> R + Send + Sync + 'static`.
2. Register it from `register_host_context_functions(lib, ...)` using `lib.register_sig(name, signature, make_foo_fn(state.clone()))`.
3. Register a matching stub in `register_unresolved_host_context_functions` that returns `Unresolved(expected_type)`.

Functions that don't need host state can be plain fn pointers registered in
the default library category modules (see `default_library.rs`).

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
- **Regex functions** reject lookahead, lookbehind, backreferences, and `\Z` (§2.2.5).
  Validation uses `regex_syntax::Parser` to parse the pattern into its HIR and
  inspect the result, rather than a substring scan. This correctly ignores
  lookaround-shaped syntax that appears inside character classes, escaped
  sequences, or regex comments (e.g., `[(?=]`, `\?=`, `(?#...)`). The parser
  rejects forbidden constructs at parse time; the translated error names the
  specific feature (e.g., "Unsupported regex feature: lookahead") so callers
  can produce stable diagnostics.
- **`repr_sh/cmd/pwsh`** produce shell-safe quoting per platform conventions (§2.2.6)
- **Path operations** are format-aware (POSIX/Windows/URI) without using `std::path` (§2.3)
- **`path(list[string])` constructor** follows Python `PurePosixPath(*parts)` /
  `PureWindowsPath(*parts)` semantics: an absolute component in the list resets the
  accumulator (discarding earlier components), empty strings are ignored, `.` segments
  are removed, duplicate separators are collapsed, and `..` is preserved without
  resolution. For Windows, drive letters and UNC prefixes follow pathlib's rules:
  a different drive replaces everything, a root-only component replaces from root
  while keeping the existing drive, and a same-drive relative component appends.
- **Slicing a `range_expr`** returns `range_expr` for positive step, `list[int]` for
  negative step (§2.1.8)
