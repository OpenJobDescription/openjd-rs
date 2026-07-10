# Public API

[README](README.md) · Public API

This document is the authoritative reference for the `openjd-expr` crate's public
API. All public types, functions, traits, and constants are listed here with
their signatures, organized by module role.

The crate implements the format-string syntax from the 2023-09 Template Schemas
([§7.3](https://github.com/OpenJobDescription/openjd-specifications/wiki/2023-09-Template-Schemas#73-format-strings))
and the full expression language from the 2026-02 Expression Language extension
spec ([EXPR](https://github.com/OpenJobDescription/openjd-specifications/wiki/2026-02-Expression-Language)).
Both are needed for OpenJD template processing: the base spec's `{{Param.Name}}`
interpolation is a subset of the extended `{{<StringInterpExpr>}}` grammar the
EXPR extension enables. The same parser and evaluator handle both.

A typical caller integrates the crate in three ways:

1. **Static parsing + evaluation** — build a [`ParsedExpression`] once, then
   evaluate it many times against different [`SymbolTable`] instances. The
   parse stage captures accessed-symbols metadata that enables validation and
   dependency analysis without the symbol table being available.
2. **Format-string resolution** — take a [`FormatString`] off a deserialized
   template field and call `resolve_string_with` / `resolve_with` to produce
   the substituted value. This is the integration point for the model crate.
3. **Type-checking at validation time** — populate a `SymbolTable` with
   [`ExprValue::unresolved(T)`] entries for symbols whose concrete values are
   not yet known, then evaluate. Operations on unresolved values propagate
   the `unresolved[T]` type through the expression so type errors surface
   at template-validation time rather than at runtime.

All expression evaluation is memory-bounded, operation-bounded, deterministic,
and has no access to the filesystem, network, or environment variables — these
are hard constraints from the spec, enforced at the evaluator level.

## Module Structure

```
openjd_expr                 — crate root; most commonly used items re-exported here
├── default_library         — deprecated module alias for FunctionLibrary::for_profile
├── error                   — ExpressionError, ExpressionErrorKind
├── eval                    — ParsedExpression, EvalBuilder, EvalResult, limits
├── format_string           — FormatString, FormatStringOptions, validation errors
├── function_library        — FunctionLibrary, FunctionEntry, FunctionImpl, EvalContext
├── functions               — sub-library modules (module-path-only — see below)
│   ├── arithmetic, comparison, conversion, list, math, misc, path,
│   │   path_parse, regex, repr, string
├── path_mapping            — PathFormat, PathMappingRule, apply_rules*, is_uri
├── profile                 — ExprProfile, ExprRevision, ExprExtension, HostContext
├── range_expr              — RangeExpr, IntRange, RangeExprError
├── symbol_table            — SymbolTable, SymbolTableEntry, SymbolTableError,
│                             SerializedSymbolTable, MAX_SYMBOL_TABLE_ENTRIES
├── types                   — ExprType, TypeCode
├── uri_path                — URI-aware path helpers (module-path-only)
└── value                   — ExprValue, Float64, ListIter, format_float
```

The `functions::*` submodules hold the individual function implementations
that the default library registers. They are `pub` so implementations can
be referenced by name from `register_sig(...)` calls, but are not intended
as a stable surface for downstream code to call directly — route function
calls through the library dispatch instead. Same story for
`uri_path::*`: callers should typically reach URI behavior via the PATH
property methods on `ExprValue`, with the module-level functions available
for building custom host-side tools.

`Evaluator` is crate-private; the public evaluation surface is
[`ParsedExpression`] + [`EvalBuilder`] + [`EvalResult`].

## Entry Points

### `ParsedExpression` — parse once, evaluate many

```rust
pub struct ParsedExpression { /* private fields */ }

impl ParsedExpression {
    /// Parse a Python-syntax expression string under the "latest" profile
    /// ([`ExprProfile::latest`]): the current revision with every known
    /// extension enabled. Runs ruff's Python parser plus structural
    /// validation against the OpenJD language subset (no lambda, no
    /// walrus, no dict/set literals, list comprehensions limited to one
    /// `for` + at most one `if`, etc., per the EXPR spec §1).
    ///
    /// **This constructor is intentionally unstable across crate versions.**
    /// The set of accepted syntax grows as new extensions/revisions land.
    /// For parse behavior that is stable across crate versions, build a
    /// profile explicitly and use [`with_profile`](Self::with_profile).
    ///
    /// Long inputs (>200 bytes) are parsed on a dedicated 32 MB-stack
    /// worker thread to survive recursive-descent depth on debug builds;
    /// this is transparent to the caller.
    pub fn new(expr: &str) -> Result<ParsedExpression, ExpressionError>;

    /// Parse an expression under a caller-supplied profile.
    ///
    /// The profile governs which syntax features the parser accepts (via
    /// [`ExprProfile::allows_syntax`]) and — once expression-level
    /// extensions exist — which functions and types are in scope. An
    /// expression that parses under one profile may fail under another.
    pub fn with_profile(
        expr: &str,
        profile: &ExprProfile,
    ) -> Result<ParsedExpression, ExpressionError>;

    /// The trimmed expression source.
    pub fn expression(&self) -> &str;

    // ── Parse metadata (collected during parse, usable without evaluating) ──

    /// Symbols this expression reads (dotted names like `Param.Frame`).
    pub fn accessed_symbols(&self) -> &HashSet<String>;

    /// Functions this expression calls (both bare calls like `len(x)` and
    /// method calls like `x.upper()`).
    pub fn called_functions(&self) -> &HashSet<String>;

    /// Loop variable names bound by list comprehensions in this expression.
    pub fn local_bindings(&self) -> &HashSet<String>;

    /// If the expression is exactly a simple dotted name (e.g. `Param.Name`),
    /// return that name. Returns None for anything more complex — arithmetic,
    /// function calls, subscripts, etc. Callers use this to detect the
    /// "just a lookup" case and skip full evaluation.
    pub fn as_name_lookup(&self) -> Option<&str>;

    // ── Shortcut evaluation ──

    /// Evaluate against a single symbol table with default configuration.
    pub fn evaluate(&self, values: &SymbolTable) -> Result<ExprValue, ExpressionError>;

    /// Evaluate against a slice of symbol tables (searched in order).
    /// Returns the value plus peak memory and operation count.
    pub fn evaluate_with_metrics(
        &self,
        symtabs: &[&SymbolTable],
    ) -> Result<EvalResult, ExpressionError>;

    // ── Builder-style configured evaluation ──
    //
    // Each `with_*` method returns an `EvalBuilder` capturing the setting;
    // terminal `evaluate(...)` / `evaluate_with_metrics(...)` on the
    // builder runs the evaluation.

    pub fn with_library<'a>(&'a self, library: &'a FunctionLibrary) -> EvalBuilder<'a>;
    pub fn with_memory_limit(&self, limit: usize) -> EvalBuilder<'_>;
    pub fn with_operation_limit(&self, limit: usize) -> EvalBuilder<'_>;
    pub fn with_path_format(&self, format: PathFormat) -> EvalBuilder<'_>;
    pub fn with_target_type<'a>(&'a self, target_type: &'a ExprType) -> EvalBuilder<'a>;
}
```

The parse/evaluate split mirrors the spec's recommendation that expressions
be validated once at template check time and evaluated many times per job
execution. `accessed_symbols` is the foundation for dependency analysis
(and the topological-evaluation RFC linked from the architecture spec):
comparing which symbols one expression reads against those another writes
lets callers build dependency graphs without running any code.

### `EvalBuilder`

```rust
#[must_use]
pub struct EvalBuilder<'a> { /* private fields */ }

impl<'a> EvalBuilder<'a> {
    pub fn with_library(self, library: &'a FunctionLibrary) -> Self;
    pub fn with_memory_limit(self, limit: usize) -> Self;
    pub fn with_operation_limit(self, limit: usize) -> Self;
    pub fn with_path_format(self, format: PathFormat) -> Self;
    pub fn with_target_type(self, target_type: &'a ExprType) -> Self;

    /// Bind symbol tables and run the evaluator. Returns just the value.
    pub fn evaluate(
        self,
        symtabs: &'a [&'a SymbolTable],
    ) -> Result<ExprValue, ExpressionError>;

    /// Same, but includes `peak_memory` and `operation_count`.
    pub fn evaluate_with_metrics(
        self,
        symtabs: &'a [&'a SymbolTable],
    ) -> Result<EvalResult, ExpressionError>;
}
```

Symbol-table binding is deferred until the terminal `evaluate` call so that
one `ParsedExpression` can be evaluated many times without re-parsing,
potentially with different per-call configuration and different symbol
tables. This maps onto the spec's template → job-creation → task-execution
processing stages: the parse artefact is built once; the configuration
flows from the stage; the symbol table is rebuilt per-evaluation as
more values become known.

### `EvalResult`

```rust
#[derive(Debug)]
pub struct EvalResult {
    pub value: ExprValue,
    pub peak_memory: usize,
    pub operation_count: usize,
}
```

## Constants

```rust
/// Default memory limit for expression evaluation: 100 MB.
///
/// Matches the value recommended by the EXPR spec's Design Constraint §2
/// (memory-bounded evaluation).
pub const DEFAULT_MEMORY_LIMIT: usize = 100_000_000;

/// Default operation limit: 10 million. Matches EXPR spec Design
/// Constraint §3.
pub const DEFAULT_OPERATION_LIMIT: usize = 10_000_000;

/// Maximum AST nesting depth. Bounds the stack use of the
/// recursive-descent evaluator and the ruff parser's structural validator.
/// Exceeding raises `ExpressionErrorKind::ExpressionTooDeep`.
pub const MAX_EXPRESSION_DEPTH: usize = 64;

/// Maximum source length (in bytes) for a single `ParsedExpression::new`
/// call. Sized to leave generous headroom on a debug-profile 32 MB
/// parser-thread stack.
pub const MAX_PARSE_INPUT_LEN: usize = 64 * 1024;
```

Additional per-module caps (see each module section below):
- `format_string::MAX_FORMAT_STRING_LEN` (1 MB)
- `format_string::MAX_FORMAT_STRING_SEGMENTS` (1,000)
- `range_expr::MAX_RANGE_EXPR_CHUNKS` (10,000)
- `symbol_table::MAX_SYMBOL_TABLE_ENTRIES` (100,000 — transport-only)

All caps are orders of magnitude above any legitimate usage and exist to
reject pathological inputs early, before they allocate.

## Profile

The [`profile`] module implements the revision / extensions / host-context
triple that selects a function library. See the
`reports/expr-model-future-revision-readiness.md` architecture note for
the full rationale.

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[non_exhaustive]
pub enum ExprRevision {
    /// The 2026-02 revision — the first revision to define the expression language.
    V2026_02,
}

impl ExprRevision {
    pub const CURRENT: ExprRevision = ExprRevision::V2026_02;
}

/// Expression-language extensions. Defined as empty-but-`#[non_exhaustive]`
/// — no expr-level extensions exist today, the API shape is reserved for
/// the first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ExprExtension {}

/// Host-context state available to expression evaluation. Today this gates
/// `apply_path_mapping`.
#[derive(Debug, Clone, Default)]
pub enum HostContext {
    /// No host-context functions registered. Default.
    #[default]
    None,
    /// Host-context signatures registered with stub implementations that
    /// return `Unresolved(T)`. Use at template-validation time.
    Unresolved,
    /// Real implementations with path-mapping rules registered. Use at
    /// runtime.
    WithRules(Arc<Vec<PathMappingRule>>),
}

impl HostContext {
    /// Construct a `WithRules` from an owned `Vec` (wraps it in an `Arc`).
    pub fn with_rules(rules: Vec<PathMappingRule>) -> Self;

    pub fn is_enabled(&self) -> bool;       // true iff not None
    pub fn is_unresolved(&self) -> bool;    // true iff Unresolved
}

/// A complete expression profile: revision, extensions, host context.
#[derive(Debug, Clone)]
pub struct ExprProfile { /* private fields */ }

impl ExprProfile {
    pub fn new(revision: ExprRevision) -> Self;
    pub fn current() -> Self;  // ExprProfile::new(ExprRevision::CURRENT)
    pub fn latest() -> Self;   // current revision + every ExprExtension::ALL enabled

    #[must_use]
    pub fn with_extensions(self, extensions: HashSet<ExprExtension>) -> Self;
    #[must_use]
    pub fn with_host_context(self, host_context: HostContext) -> Self;

    pub fn revision(&self) -> ExprRevision;
    pub fn extensions(&self) -> &HashSet<ExprExtension>;
    pub fn host_context(&self) -> &HostContext;
    pub fn has_extension(&self, ext: ExprExtension) -> bool;

    // `allows_syntax(SyntaxFeature)` is crate-private — consulted by
    // the parser's structural validator. External callers describe
    // their language flavor by choosing the profile's revision and
    // extensions; they never construct `SyntaxFeature` values directly.
}

impl Default for ExprProfile { /* ::current() */ }
```

**Syntax-acceptance resolution (crate-internal).** `ExprProfile`
internally resolves "does this profile accept syntax feature X" in two
stages, both per-revision:

1. **Revision baseline** — each revision defines a baseline set of
   accepted features. Under V2026_02 the baseline rejects every
   optional syntax feature, matching the Python implementation.
2. **Extension layer (additive)** — enabled extensions may grant
   additional features the baseline rejects. Extensions never remove
   features the baseline allows.

The concrete feature set (`SyntaxFeature` enum with variants for
walrus, lambda, dict/set literals, bitwise operators, keyword
arguments, multi-for/multi-if comprehensions, etc.) is a crate-private
implementation detail, not part of the stability contract. Callers
influence the outcome through the profile's revision and extensions,
not by naming features directly.

`ExprProfile::latest()` is intentionally **unstable across crate versions**:
it tracks `ExprRevision::CURRENT` and `ExprExtension::ALL`, both of which
grow over time. Use it for ad-hoc parsing where the caller opts in to
"whatever the newest crate version accepts." For parse behavior that is
stable across crate versions, use `ExprProfile::new(revision)` or
`ExprProfile::current()` explicitly and pass to
`ParsedExpression::with_profile` / `FormatString::with_profile`.
```

A profile is passed to [`FunctionLibrary::for_profile`] to obtain the library
appropriate to that profile. Libraries are cached per rules-independent
profile key, so multiple sessions with different `PathMappingRule` sets
share a cached skeleton and pay only per-call host-function registration
cost.

## Function Library

```rust
/// Trait function implementations use to access evaluator state —
/// operation counting, memory checks, regex compilation cache, path format.
pub trait EvalContext {
    fn path_format(&self) -> PathFormat;
    fn count_op(&mut self) -> Result<(), ExpressionError>;
    fn count_ops(&mut self, n: usize) -> Result<(), ExpressionError>;
    fn count_string_ops(&mut self, len: usize) -> Result<(), ExpressionError>;
    /// Pre-check that an allocation of `bytes` would not exceed the
    /// memory limit. Call this before large allocations so that a
    /// memory-bounded evaluator fails cleanly rather than temporarily
    /// exceeding the limit.
    fn check_memory(&self, bytes: usize) -> Result<(), ExpressionError>;
    /// Default implementation compiles each regex from scratch. Custom
    /// `EvalContext` impls (including the built-in evaluator) cache
    /// compiled regexes to amortize cost across reused patterns.
    fn get_or_compile_regex(&mut self, pattern: &str) -> Result<regex::Regex, ExpressionError>;
}

/// Arc-boxed function implementation. `Arc` (rather than `Box`) keeps
/// `FunctionLibrary` `Clone`. Closures are supported so callers can
/// register functions that close over host state (path mapping rules,
/// AWS clients, configuration) without plumbing it through `EvalContext`.
pub type FunctionImpl = Arc<
    dyn Fn(&mut dyn EvalContext, &[ExprValue]) -> Result<ExprValue, ExpressionError>
        + Send + Sync,
>;

/// A registered function overload: a signature and its implementation.
#[derive(Clone)]
pub struct FunctionEntry {
    pub signature: ExprType,       // TypeCode::Signature
    pub implementation: FunctionImpl,
}

/// Registry of functions available in expression evaluation.
///
/// Dispatch is signature-based with three phases (exact match, coerced
/// match, generic/typevar match) and supports implicit `int→float` and
/// `path→string` coercion. See the EXPR spec §2 for the canonical
/// catalogue of operators and functions.
#[derive(Clone, Default)]
pub struct FunctionLibrary {
    /* functions: private */
    /// True if this library has any host-context functions registered
    /// (today: `apply_path_mapping`).
    pub host_context_enabled: bool,
}

impl FunctionLibrary {
    pub fn new() -> Self;

    /// Obtain (or build-and-cache) the library matching the profile.
    /// Rules-independent profile shape (revision, extensions, host-kind)
    /// is the cache key; any `HostContext::WithRules(_)` profile builds
    /// on the cached no-host skeleton per call.
    pub fn for_profile(profile: &ExprProfile) -> Arc<FunctionLibrary>;

    /// Register one overload. Closures capturing environment are welcome.
    pub fn register<F>(&mut self, name: &str, signature: ExprType, implementation: F)
    where
        F: Fn(&mut dyn EvalContext, &[ExprValue]) -> Result<ExprValue, ExpressionError>
            + Send + Sync + 'static;

    /// Register from a signature string (as in the spec):
    /// `lib.register_sig("len", "(list[T1]) -> int", len_list)?`.
    pub fn register_sig<F>(
        &mut self, name: &str, sig_str: &str, implementation: F,
    ) -> Result<(), String>
    where F: /* same bounds */;

    pub fn get_signatures(&self, name: &str) -> &[FunctionEntry];
    pub fn function_names(&self) -> impl Iterator<Item = &str>;

    /// Merge another library's registrations into this one (consuming).
    #[must_use]
    pub fn merge(self, other: FunctionLibrary) -> Self;

    /// Dispatch a function call — exact → coerced → generic phases.
    pub fn call(
        &self, name: &str, args: &[ExprValue], ctx: &mut dyn EvalContext,
    ) -> Result<ExprValue, ExpressionError>;

    /// Dispatch a method call. Differs from `call` only in that the first
    /// argument (the receiver) is not considered for implicit coercion.
    pub fn call_method(
        &self, name: &str, args: &[ExprValue], ctx: &mut dyn EvalContext,
    ) -> Result<ExprValue, ExpressionError>;

    /// Static type-checking: derive the return type of a call from
    /// argument types, without evaluating. Accepts union types in
    /// `arg_types` and returns a union of possible return types.
    pub fn derive_return_type(
        &self, name: &str, arg_types: &[ExprType],
    ) -> Option<ExprType>;

    /// Static type-checking for property access: `x.name`.
    /// Looks up `__property_name__(base_type)` in the library.
    pub fn get_property_type(
        &self, base_type: &ExprType, property_name: &str,
    ) -> Option<ExprType>;
}
```

### Why `Arc<FunctionLibrary>` from `for_profile`?

The evaluator holds `&FunctionLibrary`, so callers that want to keep a
library around across many evaluations need the `Arc` to borrow from.
The profile cache stores `Arc<FunctionLibrary>` so that concurrent
evaluations for the same profile share one allocation.

## Types (`types::*`)

### `TypeCode`

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize)]
#[non_exhaustive]
pub enum TypeCode {
    NullType, Bool, Int, Float, String, Path, List, RangeExpr,
    Any, Union, NoReturn, Unresolved,
    TypeVarT, TypeVarT1, TypeVarT2, TypeVarT3,
    Signature,
}
```

Marked `#[non_exhaustive]` because future revisions / extensions may add
primitive types (e.g. `Duration`, `Url`). External pattern-matches
require a wildcard arm.

### `ExprType`

```rust
/// Represents a type in the expression language — primitive type, list of
/// T, union, type variable, `unresolved[T]`, function signature, etc.
///
/// Fields are private; construction goes through the constructors so
/// normalization invariants hold (see below).
#[derive(Debug, Clone, Eq, serde::Serialize)]
pub struct ExprType { /* code + params */ }
```

Associated constants (all compile-time constructible):

```rust
impl ExprType {
    pub const BOOL: ExprType;
    pub const INT: ExprType;
    pub const FLOAT: ExprType;
    pub const STRING: ExprType;
    pub const PATH: ExprType;
    pub const RANGE_EXPR: ExprType;
    pub const NULLTYPE: ExprType;
    pub const ANY: ExprType;
    pub const NORETURN: ExprType;
    pub const T: ExprType;
    pub const T1: ExprType;
    pub const T2: ExprType;
    pub const T3: ExprType;
}
```

Constructors + queries:

```rust
impl ExprType {
    /// Construct with normalization. Prefer the named constructors below
    /// unless you're building a generic type dynamically.
    pub fn new(code: TypeCode, params: Vec<ExprType>) -> Self;

    pub fn list(elem: ExprType) -> Self;
    pub fn union(types: Vec<ExprType>) -> Self;
    pub fn unresolved(constraint: ExprType) -> Self;
    pub fn signature(param_types: Vec<ExprType>, return_type: ExprType) -> Self;

    pub fn code(&self) -> TypeCode;
    pub fn params(&self) -> &[ExprType];

    pub fn is_list(&self) -> bool;
    pub fn list_element_type(&self) -> Option<&ExprType>;

    /// True if this type or any nested param is a type variable.
    pub fn is_symbolic(&self) -> bool;
    /// True if this type and all nested params are concrete
    /// (no `any`, `union`, `unresolved`, type variable, or signature).
    pub fn is_concrete(&self) -> bool;

    // ── Signature access (debug-asserts on `code == Signature`) ──
    pub fn sig_params(&self) -> &[ExprType];
    pub fn sig_return(&self) -> &ExprType;

    // ── Type matching and substitution for generic dispatch ──

    /// Try to match a call's argument types against this signature.
    /// Returns type-variable bindings on success.
    pub fn match_call(
        &self, arg_types: &[ExprType],
    ) -> Option<HashMap<TypeCode, ExprType>>;

    /// Match this (possibly symbolic) type against another, producing bindings.
    pub fn match_type(
        &self, other: &ExprType,
    ) -> Option<HashMap<TypeCode, ExprType>>;

    /// Substitute type variables with concrete types.
    pub fn substitute(&self, bindings: &HashMap<TypeCode, ExprType>) -> ExprType;

    /// Convenience: match + substitute on the return type in one call.
    pub fn resolve_call(&self, arg_types: &[ExprType]) -> Option<ExprType>;

    // ── Parsing ──

    /// Parse a spec-form type string: `"int"`, `"list[string]"`,
    /// `"int?"`, `"int | string"`, `"(int, int) -> int"`, etc.
    pub fn parse(s: &str) -> Result<ExprType, String>;
}
```

`ExprType` implements `PartialEq` / `Eq` / `Hash` / `Display` / `Serialize`.
Display produces the canonical spec form (e.g., `"list[int]"`, `"int?"`,
`"int | string"`, `"(int) -> string"`).

**Normalization invariants** (enforced by constructors and preserved by
`new`):

- `list[unresolved[T]]` → `unresolved[list[T]]`
- `T | unresolved[S]` → `unresolved[T | S]`
- `unresolved[unresolved[T]]` → `unresolved[T]`
- Unions are sorted, deduplicated, and flatten nested unions.
- `X | any` → `any`; `X | noreturn` → `X`.
- Single-member unions unwrap to that member.

These mirror the EXPR spec's Union Type Normalization and Unknown Type
Normalization rules, so `ExprType::union(vec![a, b])` always produces
the spec's canonical form.

## Values (`value::*`)

### `Float64`

```rust
/// An f64 wrapper that rejects NaN and infinity, normalizes -0.0 → 0.0,
/// and optionally preserves the source-string representation so that
/// e.g. `"3.140"` round-trips through evaluation rather than becoming
/// `3.14`.
///
/// Float64 is the only legal way to build `ExprValue::Float`; this keeps
/// the `Eq`/`Hash` implementations on `ExprValue` consistent (no NaN in
/// the domain).
#[derive(Debug, Clone, serde::Serialize)]
pub struct Float64 { /* value: f64, original: Option<Box<str>> */ }

impl Float64 {
    /// Reject NaN / infinity, normalize -0.0 → 0.0.
    pub fn new(v: f64) -> Result<Self, ExpressionError>;

    /// Same, but preserve the original string form for lossless display.
    pub fn with_str(v: f64, s: String) -> Result<Self, ExpressionError>;

    pub fn value(&self) -> f64;

    /// Display string: the preserved original if any, otherwise
    /// `format_float(v)`.
    pub fn to_display_string(&self) -> String;
}

impl std::fmt::Display for Float64 { /* ... */ }
impl std::ops::Deref<Target = f64> for Float64 { /* ... */ }
impl PartialEq<f64> for Float64 { /* ... */ }
impl PartialOrd<f64> for Float64 { /* ... */ }
impl Hash for Float64 { /* uses to_bits() */ }
```

Accessible as `openjd_expr::value::Float64`. Not re-exported at the crate root.

### `ExprValue`

```rust
/// A typed value during expression evaluation.
///
/// `PartialEq` uses the `equals()` semantics — cross-type equality is
/// meaningful where the spec defines it (e.g. `Int(1) == Float(1.0)`,
/// `String("x") == Path{value:"x",...}`, empty lists of different element
/// types are equal). `Hash` is consistent with `PartialEq`.
#[derive(Debug, Clone, serde::Serialize)]
pub enum ExprValue {
    Null,
    Bool(bool),
    Int(i64),
    Float(Float64),
    String(String),
    /// A PATH value, paired with the format its separators live in.
    ///
    /// The `Path` variant is marked `#[non_exhaustive]` so callers must
    /// construct paths via `ExprValue::new_path`, which enforces the
    /// separator-normalization invariant (`\` ↔ `/` per `PathFormat`,
    /// no normalization for URIs).
    #[non_exhaustive]
    Path { value: String, format: PathFormat },
    ListBool(Vec<bool>),
    ListInt(Vec<i64>),
    ListFloat(Vec<Float64>),
    ListString(Vec<String>, /* cached heap size */ usize),
    ListPath(Vec<String>, PathFormat, /* cached heap size */ usize),
    ListList(Vec<ExprValue>, /* element type hint */ ExprType, /* cached heap size */ usize),
    RangeExpr(RangeExpr),
    /// A value whose concrete form is not known, but whose type
    /// satisfies the embedded constraint. Used for static type checking
    /// and for runtime-only symbols at validation time.
    Unresolved(ExprType),
}
```

The typed list variants (`ListBool`, `ListInt`, …) are a departure from
a naive `Vec<ExprValue>` representation. They cut per-element memory use
for homogeneously-typed lists (the overwhelming common case per the EXPR
function catalogue) and keep the cached `heap_size` up front so the
memory-bounded evaluator can check allocations cheaply.

```rust
impl ExprValue {
    /// Construct a path with separator normalization.
    pub fn new_path(value: impl Into<String>, format: PathFormat) -> Self;

    /// Construct an `Unresolved(constraint)` value.
    pub fn unresolved(constraint: ExprType) -> Self;
    pub fn is_unresolved(&self) -> bool;

    // ── List construction ──

    /// Build a typed list from elements, promoting element types where
    /// needed (int+float → float, path+string → string). `hint_type`
    /// determines the element type for an empty list.
    pub fn make_list(
        elements: Vec<ExprValue>, hint_type: ExprType,
    ) -> Result<Self, ExpressionError>;

    /// Memory-checked variant: pre-checks the evaluator's memory budget
    /// before any allocation. Prefer this when an `EvalContext` is in
    /// scope.
    pub fn make_list_checked(
        ctx: &mut dyn EvalContext,
        elements: Vec<ExprValue>,
        hint_type: ExprType,
    ) -> Result<Self, ExpressionError>;

    // ── Coercion ──

    /// Coerce a string into the target type. Used by the job-parameter
    /// pipeline to turn CLI string inputs into typed parameters.
    pub fn from_str_coerce(
        s: &str, target: &ExprType, path_format: PathFormat,
    ) -> Result<Self, String>;

    /// Coerce `self` toward `target`. Consuming call; errors pass the
    /// value back on failure via the error message, not as a type.
    pub fn coerce(
        self, target: &ExprType, path_format: PathFormat,
    ) -> Result<Self, String>;

    // ── Queries ──

    pub fn expr_type(&self) -> ExprType;
    pub fn type_name(&self) -> &'static str;
    pub fn is_list(&self) -> bool;
    pub fn list_len(&self) -> Option<usize>;
    pub fn list_elements(&self) -> Option<Vec<ExprValue>>;   // allocates
    pub fn list_iter(&self) -> Option<ListIter<'_>>;          // zero-alloc
    pub fn list_get(&self, index: i64) -> Option<ExprValue>;
    pub fn list_elem_type(&self) -> Option<ExprType>;
    pub fn into_list(self) -> Option<(Vec<ExprValue>, ExprType)>;

    /// Heap allocation footprint, used by the memory-bounded evaluator.
    pub fn memory_size(&self) -> usize;

    /// Display / conversion.
    pub fn repr_python(&self) -> String;       // matches Python repr
    pub fn to_display_string(&self) -> String; // human-readable form
    pub fn as_str_repr(&self) -> std::borrow::Cow<'_, str>;

    // ── JSON transport (template-scope → session-scope boundary) ──

    pub fn to_json_transport(&self) -> serde_json::Value;
    pub fn transport_value(&self) -> serde_json::Value;
    pub fn from_json_transport(
        raw: &serde_json::Value, binding_type: &ExprType, path_format: PathFormat,
    ) -> Result<Self, String>;
    pub fn from_transport_value(
        raw: &serde_json::Value, binding_type: &ExprType, path_format: PathFormat,
    ) -> Result<Self, String>;

    /// Equality with spec-defined cross-type semantics.
    pub fn equals(&self, other: &ExprValue) -> bool;

    /// Ordering comparison. Returns `Err` for incomparable types.
    pub fn compare(
        &self, other: &ExprValue,
    ) -> Result<std::cmp::Ordering, ExpressionError>;
}

impl PartialEq for ExprValue { /* uses equals() */ }
impl Eq for ExprValue {}
impl Hash for ExprValue { /* consistent with equals() */ }

// Ergonomic conversions
impl From<bool> for ExprValue;
impl From<i32> for ExprValue;
impl From<i64> for ExprValue;
impl From<String> for ExprValue;
impl From<&str> for ExprValue;
impl From<ExprType> for ExprValue;   // wraps as `ExprValue::Unresolved(t)`
```

### `ListIter`

```rust
/// Zero-allocation iterator over the elements of an `ExprValue` list.
/// Yields freshly-cloned `ExprValue` on each `next()`.
pub enum ListIter<'a> {
    Bool(std::slice::Iter<'a, bool>),
    Int(std::slice::Iter<'a, i64>),
    Float(std::slice::Iter<'a, Float64>),
    String(std::slice::Iter<'a, String>),
    Path(std::slice::Iter<'a, String>, PathFormat),
    List(std::slice::Iter<'a, ExprValue>),
}

impl<'a> Iterator for ListIter<'a> { type Item = ExprValue; /* ... */ }
impl<'a> ExactSizeIterator for ListIter<'a> {}
```

Accessible as `openjd_expr::value::ListIter` (not re-exported at the crate root).

### `format_float`

```rust
/// Format an `f64` in the canonical spec form used by
/// `Float64::to_display_string`. Exponent notation outside the range
/// `[1e-4, 1e16)`; "0.0" for zero; preserves exact decimal where
/// possible.
pub fn value::format_float(f: f64) -> String;
```

## Symbol Table (`symbol_table::*`)

```rust
/// Hierarchical key/value store supporting dotted paths: setting
/// `"Param.Frame"` creates the nested table `Param -> Frame`.
///
/// The spec describes value references like `Param.Name`, `Task.Param.X`,
/// and `Env.File.script` as dotted names, reflected 1:1 in this tree.
/// Subtables can be used to namespace a whole scope (e.g. merging a
/// `Task` subtable onto an existing table).
#[derive(Debug, Clone, Default)]
pub struct SymbolTable { /* private field */ }

/// A leaf value or a nested subtable.
#[derive(Debug, Clone)]
pub enum SymbolTableEntry {
    Table(SymbolTable),
    Value(ExprValue),
}

#[derive(Debug, Clone)]
pub struct SymbolTableError {
    pub key: String,
    pub conflict: String,
}
```

```rust
impl SymbolTable {
    pub fn new() -> Self;

    /// Construct from any iterable of `(&str, ExprValue)` pairs.
    pub fn from_pairs<'a, I>(pairs: I) -> Result<Self, SymbolTableError>
    where I: IntoIterator<Item = (&'a str, ExprValue)>;

    /// Set a value at a dotted path. Auto-creates intermediate tables.
    /// Accepts anything `Into<ExprValue>` — `bool`, `i32`/`i64`,
    /// `&str`/`String`, `ExprValue`, and `ExprType` (auto-wrapped as
    /// `ExprValue::Unresolved(T)` for type-checking tables).
    /// Returns an error if an intermediate path component is already a
    /// scalar.
    pub fn set(
        &mut self, key: &str, value: impl Into<ExprValue>,
    ) -> Result<(), SymbolTableError>;

    pub fn set_string(&mut self, key: &str, value: &str) -> Result<(), SymbolTableError>;
    pub fn set_table(&mut self, key: &str, subtable: SymbolTable);

    pub fn get(&self, key: &str) -> Option<&SymbolTableEntry>;
    pub fn get_value(&self, key: &str) -> Option<&ExprValue>;
    pub fn get_table(&self, key: &str) -> Option<&SymbolTable>;
    pub fn get_string(&self, key: &str) -> Option<&str>;

    pub fn contains(&self, key: &str) -> bool;
    pub fn keys(&self) -> impl Iterator<Item = &str>;

    /// Collect every leaf path in the table in lexicographic order,
    /// each prefixed with `"{prefix}."` (use `""` for a top-level walk).
    /// The sort makes serialization canonical: identical tables always
    /// produce identical transport bytes.
    pub fn all_paths(&self, prefix: &str) -> Vec<String>;

    /// Recursively merge `other` into `self`, with leaves from `other`
    /// overwriting on conflict.
    pub fn merge_from(&mut self, other: &SymbolTable);
}

/// Panics on path conflict — use `from_pairs` for fallible construction.
impl<'a> FromIterator<(&'a str, ExprValue)> for SymbolTable;

impl serde::Serialize for SymbolTable { /* JSON transport array */ }
impl<'de> serde::Deserialize<'de> for SymbolTable { /* ... */ }
```

```rust
/// Maximum entries accepted when deserializing a transport-format
/// symbol table. Applies to the serde paths; direct in-process
/// `SymbolTable::set` calls are unbounded (host code is trusted).
pub const symbol_table::MAX_SYMBOL_TABLE_ENTRIES: usize = 100_000;
```

### `SerializedSymbolTable`

The boundary type between template scope (spec-defined Posix paths) and
session scope (host-native paths). The session crate stores instances
of this on resolved job structures; at runtime, the session deserializes
with `PathFormat::host()` so path separators match the worker OS.

```rust
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct SerializedSymbolTable(serde_json::Value);

impl SerializedSymbolTable {
    pub fn from_value(v: serde_json::Value) -> Self;
    pub fn from_json_str(s: &str) -> Result<Self, serde_json::Error>;
    pub fn from_symtab(st: &SymbolTable) -> Self;

    /// Deserialize with the given path format applied to PATH-typed entries.
    pub fn to_symtab(&self, path_format: PathFormat) -> Result<SymbolTable, String>;

    pub fn as_value(&self) -> &serde_json::Value;
}
```

### `symtab!` Macro

```rust
/// Construct a `SymbolTable` from key-value pairs. Panics on path
/// conflict — use `SymbolTable::from_pairs` if you need error handling.
///
/// ```
/// use openjd_expr::{symtab, ExprType};
/// let st = symtab! {
///     "Param.Frame" => 42,
///     "Param.Name" => "test",
///     "Session.Dir" => ExprType::PATH,  // auto-wraps as Unresolved
/// };
/// ```
#[macro_export]
macro_rules! symtab { /* ... */ }
```

## Format String (`format_string::*`)

```rust
/// Parsed form of a format string as defined by the 2023-09 Template
/// Schemas §7.3. Holds the raw source plus pre-parsed segments, so that
/// repeated resolution against different symbol tables does not re-parse.
///
/// A base-spec field like `"The frame is {{Param.Frame}}"` and an
/// EXPR-extension field like `"{{Param.Frame * 2 + 1}}"` are both valid
/// inputs — the EXPR extension widens the `<StringInterpExpr>` grammar
/// only; the outer `{{...}}` parse is identical.
#[derive(Debug, Clone)]
pub struct FormatString { /* private fields */ }
```

```rust
impl FormatString {
    /// Parse a format string under the "latest" profile
    /// ([`ExprProfile::latest`]). Embedded `{{...}}` expressions are
    /// parsed via `ParsedExpression::new` (same instability caveat).
    pub fn new(input: &str) -> Result<Self, ExpressionError>;

    /// Parse a format string under a caller-supplied profile. Embedded
    /// `{{...}}` expressions are parsed via
    /// `ParsedExpression::with_profile` with the same profile.
    pub fn with_profile(
        input: &str,
        profile: &ExprProfile,
    ) -> Result<Self, ExpressionError>;

    /// The raw source text.
    pub fn raw(&self) -> &str;

    /// Resolve to an `ExprValue`. If the format string is one expression
    /// with no surrounding literals, returns the raw typed value
    /// (possibly a list, path, int, etc.). Otherwise concatenates the
    /// segments as a string and returns `ExprValue::String`.
    pub fn resolve_with(
        &self, symtab: &SymbolTable, opts: &FormatStringOptions<'_>,
    ) -> Result<ExprValue, ExpressionError>;

    /// Resolve to a `String`. Single-expression forms that aren't
    /// naturally string-typed (int, float, list, path) are converted via
    /// `ExprValue::to_display_string()`.
    pub fn resolve_string_with(
        &self, symtab: &SymbolTable, opts: &FormatStringOptions<'_>,
    ) -> Result<String, ExpressionError>;

    /// Validate every interpolation against a type-checking symbol table
    /// (typically populated with `ExprValue::unresolved(T)` values).
    /// Returns structured errors with the position of the first failing
    /// interpolation so callers can produce diagnostics.
    pub fn validate_expressions(
        &self, symtab: &SymbolTable, library: &FunctionLibrary,
    ) -> Result<(), FormatStringValidationError>;

    /// Validate that list-comprehension loop variables in this format
    /// string's interpolations don't shadow names from the enclosing
    /// scope (typically the let-binding names active at this point in
    /// the template).
    pub fn validate_comprehension_vars(
        &self, let_names: &HashSet<String>,
    ) -> Result<(), ExpressionError>;

    /// Does this format string contain any non-simple-name interpolation?
    /// A cheap check for "can I skip the EXPR extension type-check pass?"
    pub fn has_complex_expressions(&self) -> bool;

    /// Names of the symbols referenced by simple-name interpolations.
    /// Complex expressions are skipped.
    pub fn expression_names(&self) -> Vec<&str>;

    /// True iff this format string contains no `{{...}}` interpolations.
    pub fn is_literal(&self) -> bool;

    /// All symbol names accessed across every interpolation.
    pub fn accessed_symbols(&self) -> std::collections::HashSet<String>;

    /// Copy only the symbols this format string uses from `source` into
    /// `dest`. Used by the model crate to build the filtered
    /// `resolved_symtab` on resolved environments.
    pub fn copy_used_symtab_values(
        &self, source: &SymbolTable, dest: &mut SymbolTable,
    );
}

impl std::fmt::Display for FormatString;  // displays the raw source
impl PartialEq for FormatString;
impl Eq for FormatString;
impl<'de> serde::Deserialize<'de> for FormatString;  // accepts str/int/float/bool
```

```rust
/// Options for `FormatString::resolve_with` / `resolve_string_with`.
/// Builder-style; each `with_*` returns the modified options by value.
#[derive(Clone)]
pub struct FormatStringOptions<'a> { /* private fields */ }

impl<'a> FormatStringOptions<'a> {
    pub fn new() -> Self;       // all defaults

    /// `None` means "use the default (current-profile) library."
    #[must_use]
    pub fn with_library(
        self,
        library: impl Into<Option<&'a FunctionLibrary>>,
    ) -> Self;

    #[must_use] pub fn with_path_format(self, fmt: PathFormat) -> Self;
    #[must_use] pub fn with_target_type(self, t: &'a ExprType) -> Self;
}

impl<'a> Default for FormatStringOptions<'a> { /* ... */ }
```

```rust
/// Validation error from `FormatString::validate_expressions`.
#[derive(Debug, Clone)]
pub struct FormatStringValidationError {
    pub message: String,
    pub input: String,
    pub start: usize,
    pub end: usize,
    pub expression_error: Option<Box<ExpressionError>>,
}

impl std::fmt::Display for FormatStringValidationError;
impl std::error::Error for FormatStringValidationError;
```

```rust
/// Maximum raw length (in bytes) for `FormatString::new`. Several orders
/// of magnitude above any legitimate template field.
pub const format_string::MAX_FORMAT_STRING_LEN: usize = 1024 * 1024;

/// Maximum number of `{{...}}` segments in one format string.
pub const format_string::MAX_FORMAT_STRING_SEGMENTS: usize = 1_000;

/// Escape `{{` and `}}` in a string so the format-string parser treats
/// them as literals — necessary when synthesizing format strings from
/// user input that may contain double braces.
pub fn escape_format_string(value: &str) -> String;
```

## Range Expressions (`range_expr::*`)

Range expressions like `"1-10"`, `"1-100:10"`, `"1,5,10-20"` appear in
INT task parameter ranges (spec §3.4.2) and come back out at runtime via
`ExprValue::RangeExpr`. The `CHUNK[INT]` task parameter type (TASK_CHUNKING
extension) also resolves to `RangeExpr`.

```rust
#[derive(Debug, Clone)]
pub struct RangeExprError {
    pub expr: String,
    pub message: String,
    pub position: Option<usize>,
}

impl RangeExprError {
    pub fn new(expr: impl Into<String>, message: impl Into<String>) -> Self;
    pub fn at(expr: impl Into<String>, message: impl Into<String>, position: usize) -> Self;
}

impl std::fmt::Display for RangeExprError;
impl std::error::Error for RangeExprError;
```

```rust
/// A single contiguous integer range with step.
#[derive(Debug, Clone)]
pub struct IntRange { /* start, end, step: private */ }

impl IntRange {
    pub fn new(start: i64, end: i64, step: i64) -> Result<Self, ExpressionError>;
    pub fn len(&self) -> usize;
    pub fn is_empty(&self) -> bool;
    pub fn contains(&self, value: i64) -> bool;
    pub fn iter(&self) -> impl Iterator<Item = i64> + '_;
    pub fn get(&self, index: usize) -> Option<i64>;
}
```

```rust
/// A sorted set of non-overlapping integer ranges. Represents the
/// expanded form of a range expression.
#[derive(Debug, Clone, Eq, serde::Serialize)]
pub struct RangeExpr { /* private fields */ }
```

Maximum chunk count:

```rust
/// Defensive cap on the number of disjoint ranges a single `RangeExpr`
/// may contain. 10,000 is two orders of magnitude above any realistic
/// shot-list submission.
pub const range_expr::MAX_RANGE_EXPR_CHUNKS: usize = 10_000;
```

Construction + inspection:

```rust
impl RangeExpr {
    pub fn from_values(values: Vec<i64>) -> Self;
    pub fn from_ranges(ranges: Vec<IntRange>) -> Result<Self, ExpressionError>;

    /// Display mode: when true, single-element ranges render as
    /// `"5-5"` rather than `"5"`. Useful for contiguous chunk emissions.
    #[must_use]
    pub fn with_contiguous(self, contiguous: bool) -> Self;

    pub fn len(&self) -> usize;
    pub fn is_empty(&self) -> bool;
    pub fn contains(&self, value: i64) -> bool;
    pub fn get(&self, index: i64) -> Option<i64>;
    pub fn ranges(&self) -> &[IntRange];
    pub fn cumulative_lengths(&self) -> &[usize];
    pub fn iter(&self) -> impl Iterator<Item = i64> + '_;
    pub fn to_vec(&self) -> Vec<i64>;

    /// Slice with Python-style start/stop/step. Returns a new `RangeExpr`.
    pub fn slice(
        &self, start: i64, stop: i64, step: i64,
    ) -> Result<RangeExpr, ExpressionError>;

    /// Heap allocation footprint — for memory-budget accounting on
    /// `ExprValue::RangeExpr`.
    pub fn heap_size(&self) -> usize;
}

impl std::str::FromStr for RangeExpr {
    type Err = ExpressionError;
    /// Parse spec-form strings like "1-10", "1-100:10", "1,5,10-20".
}

impl PartialEq for RangeExpr;  // compares ranges only, ignores display bit
impl Hash for RangeExpr;       // consistent with PartialEq
impl<'de> serde::Deserialize<'de> for RangeExpr;
```

## Path Mapping (`path_mapping::*`)

Path mapping rules (spec [How-Jobs-Are-Run §Path Mapping](https://github.com/OpenJobDescription/openjd-specifications/wiki/How-Jobs-Are-Run#path-mapping))
let submitters say "everywhere you see source path X, replace with destination
path Y" — essential for farms where the submitter and worker see different
filesystem layouts. The worker agent reads the rules at session start,
sorts them longest-source-first, and registers `apply_path_mapping` into
the `FunctionLibrary`.

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum PathFormat {
    #[serde(alias = "posix", alias = "Posix")]
    Posix,
    #[serde(alias = "windows", alias = "Windows")]
    Windows,
    #[serde(alias = "URI", alias = "uri", alias = "Uri")]
    Uri,
}

impl PathFormat {
    /// `Windows` on Windows, `Posix` elsewhere.
    pub fn host() -> Self;
}
```

```rust
/// A path mapping rule. Field names match the spec's JSON form.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PathMappingRule {
    pub source_path_format: PathFormat,
    pub source_path: String,
    pub destination_path: String,
}

impl PathMappingRule {
    /// Apply the rule with host-native output separators.
    pub fn apply(&self, path: &str) -> Option<String>;

    /// Apply with explicit output format.
    pub fn apply_with_format(
        &self, path: &str, output_format: PathFormat,
    ) -> Option<String>;
}
```

Rule-list helpers (the `apply_path_mapping` function in the library
composes these):

```rust
/// Apply rules in order with first-match-wins. Rules must be pre-sorted
/// by decreasing `source_path` length so that a nested rule (`/a/b`) is
/// tried before its parent (`/a`).
#[must_use]
pub fn path_mapping::apply_rules(rules: &[PathMappingRule], path: &str) -> String;

#[must_use]
pub fn path_mapping::apply_rules_with_format(
    rules: &[PathMappingRule], path: &str, output_format: PathFormat,
) -> String;

/// True iff `path` has a `scheme://` URI prefix.
pub fn path_mapping::is_uri(path: &str) -> bool;
```

## URI Paths (`uri_path::*`)

Helpers for URI-typed path values — `s3://bucket/key`, `https://host/path`,
etc. — used by PATH methods/properties when the underlying string is a URI.

```rust
#[derive(Debug, Clone)]
pub struct uri_path::UriParts {
    pub authority: String,       // "scheme://host"
    pub path_parts: Vec<String>,
}

pub fn uri_path::is_uri(path: &str) -> bool;
pub fn uri_path::parse(path: &str) -> Option<UriParts>;

pub fn uri_path::name(path: &str) -> String;
pub fn uri_path::parent(path: &str) -> String;
pub fn uri_path::suffix(path: &str) -> String;
pub fn uri_path::suffixes(path: &str) -> Vec<String>;
pub fn uri_path::stem(path: &str) -> String;
pub fn uri_path::parts(path: &str) -> Vec<String>;
pub fn uri_path::from_parts(parts: &[String]) -> String;
pub fn uri_path::join(path: &str, child: &str) -> String;
```

These are not re-exported at the crate root. Downstream callers
should prefer `ExprValue`'s PATH methods / properties for expression-time
URI handling; the module-level functions exist mainly for host-side
tooling that needs to reproduce expression-semantics on URI strings
(e.g. a submission UI previewing path-derived filenames).

## Error Types

```rust
/// Structured error kinds. Callers can match on specific variants for
/// programmatic handling rather than parsing error-message strings.
#[derive(Debug, Clone, thiserror::Error)]
#[non_exhaustive]
pub enum ExpressionErrorKind {
    UndefinedVariable { name: String, suggestion: String },
    UnknownFunction { name: String },
    TypeError { message: String },
    IntegerOverflow,
    DivisionByZero { op: &'static str },
    FloatError { message: String },  // NaN / infinity produced
    IndexOutOfBounds { message: String },
    /// Exceeded `memory_limit` (default `DEFAULT_MEMORY_LIMIT`).
    MemoryLimitExceeded { used: usize, limit: usize },
    /// Exceeded `operation_limit` (default `DEFAULT_OPERATION_LIMIT`).
    OperationLimitExceeded { count: usize, limit: usize },
    /// Exceeded `MAX_EXPRESSION_DEPTH`.
    ExpressionTooDeep { depth: usize, limit: usize },
    /// A Python syntax feature that's not in the OpenJD subset
    /// (lambdas, walrus, dict/set literals, chained `is`, etc.).
    UnsupportedSyntax { feature: String },
    /// The `fail()` function was called explicitly.
    ExplicitFail(String),
    /// A parse-stage error (underlying parser or range expression parser).
    ParseError(String),
    /// Catch-all for errors without a structured category.
    Other(String),
}
```

```rust
/// Expression-language error with optional source-location context.
/// Box-backed (8 bytes on the stack) to keep `Result<ExprValue, _>` compact.
#[derive(Debug, Clone)]
pub struct ExpressionError { /* private inner */ }

impl ExpressionError {
    pub fn from_kind(kind: ExpressionErrorKind) -> Self;
    pub fn new(message: impl Into<String>) -> Self;

    // Convenience constructors — one per common kind.
    pub fn integer_overflow() -> Self;
    pub fn division_by_zero(op: &'static str) -> Self;
    pub fn float_error(message: impl Into<String>) -> Self;
    pub fn type_error(message: impl Into<String>) -> Self;
    pub fn index_out_of_bounds(message: impl Into<String>) -> Self;
    pub fn unsupported(feature: impl Into<String>) -> Self;
    pub fn explicit_fail(message: impl Into<String>) -> Self;
    pub fn parse_error(message: impl Into<String>) -> Self;
    pub fn expression_too_deep(depth: usize, limit: usize) -> Self;

    // Accessors
    pub fn kind(&self) -> &ExpressionErrorKind;
    pub fn message(&self) -> String;
    pub fn sub_errors(&self) -> &[ExpressionError];
    pub fn expr(&self) -> Option<&str>;
    pub fn col_offset(&self) -> Option<usize>;
    pub fn end_col_offset(&self) -> Option<usize>;
    pub fn caret_offset(&self) -> Option<usize>;

    // Builder-style context attachment
    #[must_use] pub fn with_sub_errors(self, sub_errors: Vec<ExpressionError>) -> Self;
    #[must_use] pub fn with_node(self, expr_source: &str, node: &ruff_python_ast::Expr) -> Self;
    #[must_use] pub fn with_span(self, expr_source: &str, col: usize, end_col: usize) -> Self;
    pub fn set_source_span(
        &mut self, expr_source: &str, col: usize, end_col: usize, caret_offset: usize,
    );

    /// Render with a prefix prepended to the expression line and caret
    /// shifted accordingly. Used for let-binding errors where the
    /// expression is embedded in a larger construct like
    /// `"x = Param.Frame + \"oops\""`.
    pub fn message_with_expr_prefix(&self, prefix: &str) -> String;
}

impl std::fmt::Display for ExpressionError;   // multi-line caret-annotated form
impl std::error::Error for ExpressionError;   // source() returns &kind
```

## Function Implementations (`functions::*`)

Accessible via module paths like `openjd_expr::functions::arithmetic::add_int`.
Every submodule (`arithmetic`, `comparison`, `conversion`, `list`,
`math`, `misc`, `path`, `path_parse`, `regex`, `repr`, `string`) is
public. The implementations have the shape required by `FunctionImpl`:

```rust
pub fn <name>(
    ctx: &mut dyn EvalContext, args: &[ExprValue],
) -> Result<ExprValue, ExpressionError>;
```

These are exposed so the default library's `register_sig(...)` calls can
reference them by name. They are not intended as a stable, direct-call
surface — semantics are defined only in the context of a dispatch
(expected argument types, coercion, op counting). External code should
call functions through `FunctionLibrary::call` / `call_method` instead.

## Default Library (`default_library`)

```rust
/// Register host-context functions onto a library clone. Takes an `Arc`
/// of rules so that the registered closure can share ownership of the
/// rule set without copying.
pub fn default_library::register_host_context_functions(
    lib: &mut FunctionLibrary, rules: Arc<Vec<PathMappingRule>>,
);

/// Register host-context signatures with stub implementations that
/// return `Unresolved(T)`. Used at template-validation time when real
/// host state is not yet available but signatures must be known for
/// type checking.
pub fn default_library::register_unresolved_host_context_functions(
    lib: &mut FunctionLibrary,
);
```

These are the primitives that `FunctionLibrary::for_profile` composes
internally when the profile's `HostContext` is `WithRules(..)` or
`Unresolved`. Most callers should not call them directly — go through
`for_profile(profile)` to get the cached result. The functions are
public so that advanced callers building a custom library from scratch
can layer host-context functions onto their own skeleton.

## Re-exports at the Crate Root

```rust
pub use error::{ExpressionError, ExpressionErrorKind};
pub use eval::{
    EvalBuilder, EvalResult, ParsedExpression,
    DEFAULT_MEMORY_LIMIT, DEFAULT_OPERATION_LIMIT,
    MAX_EXPRESSION_DEPTH, MAX_PARSE_INPUT_LEN,
};
pub use format_string::{
    escape_format_string, FormatString, FormatStringOptions, FormatStringValidationError,
};
pub use function_library::{EvalContext, FunctionLibrary};
pub use path_mapping::{PathFormat, PathMappingRule};
pub use profile::{ExprExtension, ExprProfile, ExprRevision, HostContext};
pub use range_expr::{RangeExpr, RangeExprError, MAX_RANGE_EXPR_CHUNKS};
pub use symbol_table::{
    SerializedSymbolTable, SymbolTable, SymbolTableError, MAX_SYMBOL_TABLE_ENTRIES,
};
pub use types::{ExprType, TypeCode};
pub use value::ExprValue;
```

Types accessible only via their module paths:

- `value::Float64` — the `ExprValue::Float` payload type.
- `value::ListIter`, `value::format_float` — the list-iteration helper
  and float-formatting helper.
- `function_library::{FunctionEntry, FunctionImpl}` — the library
  registration types (`register` / `register_sig` / `merge` cover most
  use cases; these types appear only when inspecting internals).
- `symbol_table::SymbolTableEntry` — Table-vs-Value discriminant (use
  `get`/`get_value`/`get_table` for typed access).
- `range_expr::IntRange` — component of `RangeExpr` (usually accessed
  via `ranges()`).
- `uri_path::{UriParts, is_uri, parse, name, parent, ...}` — URI helpers.
- `functions::*` — per-category implementation modules.
- `default_library::{register_host_context_functions, register_unresolved_host_context_functions}`
  — primitives used by `for_profile`.

## Design Constraints (from the EXPR spec)

The crate enforces these at the `EvalContext` level; they are not
merely recommendations:

1. **No filesystem, network, or environment variable access** — no
   functions in the library perform any of these. The only host-state
   dependence is `apply_path_mapping`, which sees only the
   caller-supplied path-mapping rules.
2. **Memory-bounded evaluation** — every `ExprValue` has a computable
   `memory_size()`; the evaluator tracks current and peak memory and
   raises `MemoryLimitExceeded` before the limit is breached.
3. **Operation-bounded evaluation** — each function call counts as 1
   op; iterating a list adds `n` ops; processing a string adds
   `ceil(len / 256)` ops. `OperationLimitExceeded` surfaces at the
   first op that crosses the limit.
4. **Deterministic evaluation** — no mutable globals in the dispatch
   path; regex caches are local to each `Evaluator` instance.
5. **No user-defined functions** — `FunctionLibrary::register` exists
   but is a *host-side* API for building custom libraries; the language
   itself has no function-definition syntax. The OpenJD subset of
   Python explicitly excludes `lambda` and `def`.
6. **Backward compatibility** — base-spec format strings
   (`{{Param.Name}}`) evaluate identically whether EXPR is enabled or
   not. The same `ParsedExpression` handles both.
7. **Fail-fast errors** — `ExprValue::Unresolved(T)` propagation through
   evaluation lets validators catch type errors at template-check time
   rather than deferring them to runtime.

## Versioning and Stability Conventions

This crate targets the 2026-02 Expression Language revision. The
profile machinery (see `specs/expr/README.md` → Normative References
for the revision/extensions architecture) is in place but only one
revision is defined today.

Enums marked `#[non_exhaustive]`:

- `ExprRevision`
- `ExprExtension` (empty today — reserved for future expression-level extensions)
- `TypeCode`
- `ExpressionErrorKind`
- `ExprValue` (outer enum) — so new primitive types can be added to
  future revisions without a SemVer break

The crate-private `SyntaxFeature` enum is also `#[non_exhaustive]` for
internal consistency, but it is not part of the public API — callers
describe their desired language flavor by choosing the profile's
revision and extensions, not by naming syntax features directly.

The `Path` variant of `ExprValue` is marked `#[non_exhaustive]` so
that external code cannot build paths without going through
`ExprValue::new_path` (which enforces separator normalization). This
is a separate concern from the outer-enum `#[non_exhaustive]` —
together they give a stable growth path for both new variants and new
fields within existing variants.

Enums that are intentionally closed:

- `PathFormat` — Posix / Windows / Uri cover the practical space; a
  new variant would be a deliberate breaking change.
- `SymbolTableEntry` — Table vs Value is a structural property with no
  room to grow.
- `HostContext` — the three-valued `None` / `Unresolved` / `WithRules`
  shape mirrors the three stages at which a library is needed
  (template validation, runtime without rules, runtime with rules) and
  is complete for that purpose.
- `ListIter` — one variant per typed list storage; adding a new list
  type requires adding a new `ExprValue` variant anyway, so any
  expansion here is already a structural change.

Caps that are part of the stability contract (raising them is a
minor-version change; lowering them is a breaking change):

- `DEFAULT_MEMORY_LIMIT`, `DEFAULT_OPERATION_LIMIT`: match the EXPR
  spec's recommended defaults and should track spec revisions rather
  than crate convenience.
- `MAX_EXPRESSION_DEPTH`, `MAX_PARSE_INPUT_LEN`,
  `MAX_FORMAT_STRING_LEN`, `MAX_FORMAT_STRING_SEGMENTS`,
  `MAX_RANGE_EXPR_CHUNKS`, `MAX_SYMBOL_TABLE_ENTRIES`: defensive caps.
  Values may rise to accommodate new legitimate use cases; values will
  not drop without a breaking-change bump.
