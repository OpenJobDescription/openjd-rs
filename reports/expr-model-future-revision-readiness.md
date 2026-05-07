# Readiness of `openjd-expr` and `openjd-model` for Future Spec Revisions and Extensions

**Date:** 2026-05-07
**Scope:** `openjd-expr`, `openjd-model`
**Focus:** Can the current public interface and internal implementation
accommodate a new spec revision (e.g. `2027-xx`) and new extensions that
add functions, modify function semantics, or change template/expression
interpretation rules?

## Executive Summary

The `openjd-model` crate is in reasonable shape for extension/revision
evolution: it has a `ValidationContext` that already threads through most
of the validation pipeline, and it derives "effective" limits/rules from
(revision, extensions). The main gaps on the model side are that
`SpecificationRevision` has only one variant, `EffectiveLimits` /
`EffectiveRules` ignore the revision field, and the validation entry
points are hardwired to `validate_v2023_09::*`.

The `openjd-expr` crate is **not ready** for extension-driven evolution
in the way the user described. Its function library has exactly three
construction modes — *default*, *with host context*, *with unresolved
host context* — each of which bakes in a single fixed set of registered
functions and signatures. There is no concept of "which revision" or
"which extensions are active" inside the library, and model-side
validation code currently picks between those three modes by hand. When
a new revision or extension adds a function, removes one, changes a
signature, or restricts an operator, there is no vector in the public
API to express that — callers would need to build a library from
scratch outside the crate, which duplicates the default library and
loses `LazyLock` caching.

The recommended direction is:

1. Make `ValidationContext` (or an expression-local equivalent
   `ExprProfile` / `ExprContext`) a first-class input to function
   library construction. Replace `get_default_library()` /
   `with_host_context` / `with_unresolved_host_context` with a small
   set of profile-parameterised builders that consult the context.
2. Push the "submission time vs host context" distinction into that
   context rather than having separate `with_*_host_context` methods.
3. Expose the `ValidationContext` the model uses (or a derived
   `FunctionProfile`) at the `openjd-expr` layer, without creating a
   reverse dependency on `openjd-model`.
4. Internally, stop hardwiring operator names, keyword names, and
   property names to global constants and route them through the same
   context so an extension can add/hide them.

This is a pre-release window — breaking changes are cheap now and
expensive after release. Doing this refactor now is significantly
cheaper than retrofitting it behind a released public API.

## 1. How the current design encodes "which rules to apply"

### 1.1 Model-side: `ValidationContext` + effective-rules pattern

`openjd-model/src/types.rs` defines the current context:

```rust
pub enum SpecificationRevision {
    V2023_09,
}

pub enum KnownExtension {
    TaskChunking,
    RedactedEnvVars,
    FeatureBundle1,
    Expr,
}

pub struct ValidationContext {
    pub revision: SpecificationRevision,
    pub extensions: Extensions,              // HashSet<KnownExtension>
    pub caller_limits: CallerLimits,
}
```

This is threaded as `&ValidationContext` through the whole validation
pipeline in `validate_v2023_09/`. Each pass derives `EffectiveLimits`
and `EffectiveRules`:

```rust
pub struct EffectiveLimits {
    pub max_identifier_len: usize,
    pub max_job_name_len: usize,
    // ...
}

impl EffectiveLimits {
    pub fn from_context(ctx: &ValidationContext) -> Self {
        let fb1 = ctx.has_extension(KnownExtension::FeatureBundle1);
        Self {
            max_identifier_len: if fb1 { 512 } else { 64 },
            // ...
        }
    }
}
```

This is a sound pattern. The rest of the model code consults
`EffectiveLimits` and `EffectiveRules` rather than branching on
`KnownExtension` directly, which means new extensions can change the
effective values without rippling through the whole codebase.

### 1.2 Expr-side: three-way library selection with no context

`openjd-expr/src/default_library.rs` exposes exactly one cached library
and two host-context variants:

```rust
static DEFAULT_LIBRARY: LazyLock<FunctionLibrary> = LazyLock::new(default_library);

pub fn get_default_library() -> &'static FunctionLibrary { &DEFAULT_LIBRARY }

// On the library type:
pub fn with_host_context<R>(self, rules: R) -> Self where R: IntoHostContextRules;
pub fn with_unresolved_host_context(self) -> Self;
```

There are **exactly** three profiles available:

| Profile | How constructed | Used by |
|---|---|---|
| Default (submission-time, no host functions) | `get_default_library().clone()` | Job name, task param ranges, template-scope format strings |
| Host context (rules-bearing) | `…clone().with_host_context(rules)` | `openjd-sessions` runtime, CLI `run` |
| Unresolved host context (type-checking stub) | `…clone().with_unresolved_host_context()` | Model validation of task/session-scope strings |

The fixed set of registered signatures lives in a single
`build_default_library()` function that calls ~13 category builders
(`arithmetic`, `string_ops`, …). None of these accept any argument.

Model-side code then picks between those three profiles by hand:

```rust
// format_strings.rs pass 8
let default_lib = openjd_expr::default_library::get_default_library().clone();
let host_lib = default_lib.clone().with_unresolved_host_context();
// … then passes `&default_lib` or `&host_lib` to each validate_fs call
// depending on the scope of the format string being validated.
```

and again in `instantiate.rs`, `create_job/mod.rs`, and the CLI/sessions
runtime. There is no central function-library construction policy — each
site recapitulates which variant it needs.

### 1.3 Why this matters for future revisions and extensions

Every one of the following kinds of change from a future RFC has to cross
one of these boundaries:

1. A new revision adds a function (e.g. `uuid4()` for deterministic
   labels).
2. A new revision changes a signature (e.g. `round(float, int) -> int`
   vs. today's `round(float, int) -> float | int`).
3. A new revision removes or deprecates a function.
4. A new extension adds functions that are *only* available with that
   extension enabled (the natural analogue of how `apply_path_mapping`
   is host-context-only today).
5. A new extension changes operator semantics (e.g. a `STRING_OPS`
   extension where `+` on `(path, path)` means "append second to first
   as a relative path").
6. A new extension changes validation-only behaviour (e.g. allowing
   format strings in new positions, introducing a new parameter type).

Case 6 is already handled well by `EffectiveLimits` / `EffectiveRules`.
Cases 1–5 touch the function library, and the current
`FunctionLibrary` has no machinery for any of them except (4) —
partially — via `with_host_context`.

## 2. Public interface readiness

### 2.1 `openjd-expr` public interface

Reviewed via `crates/openjd-expr/src/lib.rs` re-exports and the
`ParsedExpression` / `EvalBuilder` chain.

**What is good:**

- `EvalBuilder` already decouples parse from evaluate and takes a
  `&FunctionLibrary` — this is the right place to inject a profile.
- `FunctionLibrary` is `Clone` and backed by `Arc<dyn Fn>` entries, so
  a builder-style API that derives libraries from a context is cheap.
- `FunctionLibrary::register_sig` / `register` / `merge` / `derive_return_type`
  are already a reasonable "plugin" surface for extensions — they just
  aren't used that way.
- `host_context_enabled: bool` is already a runtime capability marker
  (albeit a narrow one).

**What is not ready:**

- **No way to express "a library for revision R with extensions E".**
  The only knobs are "default", "host", "unresolved host". Adding a
  fourth (e.g., a new extension) via another `with_foo()` method does
  not scale — the combinatorial space is revision × powerset(extensions).
- **The default library is a `static LazyLock`.** It is baked once at
  first access from the hardcoded 200-ish signatures in
  `default_library.rs`. Any caller that wants a different set must
  clone the static and then mutate — which means model code pays to
  instantiate and register signatures on every template validation, and
  any revision-specific default has nowhere to cache itself.
- **Host context is hard-split from extension context.** Today,
  "functions that need host state" and "functions from the EXPR
  extension" are orthogonal, but `with_host_context(rules)` can only
  be called once, and it captures a fixed set (currently just
  `apply_path_mapping`). A second host-sensitive extension would need
  a second `with_*_host_context` method, entrenching the combinatorial
  growth.
- **No introspection of "what is available."** Callers cannot query
  "given this revision + these extensions, is `sum(list[float])`
  defined?" without building the library first and calling
  `derive_return_type`. For validation tooling (`openjd check`,
  editor integrations) this is awkward.
- **`ExprType`, `TypeCode`, `ExprValue` are effectively frozen** in
  their public enums. `TypeCode` in `types.rs` is not marked
  `#[non_exhaustive]` (neither are most of the other public enums in
  `openjd-expr/src/value.rs`). An extension that introduces a new
  primitive type (say `duration` or `url`) would be a SemVer-breaking
  change today.
- **`KnownExtension` is exported from `openjd-model` but not
  `openjd-expr`.** The expression crate has no vocabulary to talk
  about extensions even though its library content depends on them.
  The only place extension presence is acknowledged in `openjd-expr`
  is implicitly via host-context registration.

### 2.2 `openjd-model` public interface

**What is good:**

- `ValidationContext` is a stable-looking API with `new`,
  `with_extensions`, `with_caller_limits`, `has_extension`. It already
  contains all the information a downstream function-library builder
  would need.
- `SpecificationRevision` and `ModelError` are `#[non_exhaustive]`
  (confirmed in existing model-quality-evaluation-report.md and in
  `types.rs`). Adding `V2027_xx` is SemVer-compatible.
- `KnownExtension` variants can be added SemVer-compatibly if the enum
  is marked `#[non_exhaustive]` — **and this is NOT currently the
  case**. `KnownExtension`, `JobParameterType`, `TaskParameterType`,
  `TemplateSpecificationVersion`, `ObjectType`, `DataFlow`, `FileType`,
  `EndOfLine` are all plain enums.
- `decode_job_template` / `decode_template` accept a
  `supported_extensions: Option<&[&str]>` allowlist, so applications
  can restrict which extensions their environment honours. This is
  the right pattern.
- `ValidationContext` is already threaded through all five validation
  passes and into `create_job`.

**What is not ready:**

- **`ValidationContext` is not exposed to expression evaluation sites.**
  Model code re-derives `has_expr = ext.contains("EXPR")` or
  `ctx.has_extension(KnownExtension::FeatureBundle1)` in many places,
  sometimes from the template's own `extensions` list rather than from
  a caller-supplied context (e.g. `create_job` rebuilds a
  `ValidationContext` from `job_template.extensions`). This means an
  application that wants to reject EXPR in a specific queue cannot do
  so at `create_job` time — only at `decode_*` time.
- **Validation entry points are hardwired to
  `validate_v2023_09::*`.** `template/parse.rs` has `use
  crate::template::validate_v2023_09 as validate;`. A new revision
  would require adding `validate_v2027_xx` and a dispatch step in
  `decode_job_template`. That dispatch does not exist yet. This is a
  deferred but known cost, not a bug.
- **`EffectiveLimits::from_context` ignores `ctx.revision`.** All
  limits branch only on extensions. A new revision that raises a
  base-spec limit (e.g. 2027 raises `max_identifier_len` to 128 as
  the baseline) has nowhere to hook in.
- **`EffectiveRules` likewise ignores revision.** It's extension-only.
- **`KnownExtension` enum name variants are tied to today's names.**
  If 2027 renames or deprecates an extension, the enum becomes
  ambiguous. A forward-compatible approach would make `KnownExtension`
  carry an association with a revision or be scoped per-revision.
- **`create_job` rebuilds a `ValidationContext` locally** instead of
  accepting one from the caller. This makes it impossible for a
  caller to pass supplementary context (e.g. "treat this job as if
  it were under a different revision" for testing, or "reject tasks
  over N even if the template is valid").

### 2.3 Summary table

Original assessment, annotated with Priority 1 changes applied in
commit `881f78e`:

| Concern | Model | Expr |
|---|---|---|
| First-class context struct | ✅ `ValidationContext` | ✅ `ExprProfile` *(P1)* |
| Public enums marked `#[non_exhaustive]` | ✅ all relevant *(P1)* | ✅ all relevant *(P1)* |
| Revision dispatch vector | ⚠️ Hardwired to v2023_09 | ⚠️ Single match arm on `ExprRevision::V2026_02` *(P1; vector now exists in `build_library_skeleton`)* |
| Extension dispatch vector | ✅ `EffectiveLimits` / `EffectiveRules` | ⚠️ `ExprExtension` enum exists but is empty *(P1)* |
| Function-set parameterisation | N/A | ✅ `FunctionLibrary::for_profile` *(P1; old `LazyLock` kept as deprecated)* |
| Host-function composition | N/A | ✅ `HostContext` enum with three variants *(P1; old `host_context_enabled: bool` kept for compat)* |
| Introspection of available functions | N/A | ⚠️ Still only via `derive_return_type` after build |
| Application-level allowlist | ✅ `supported_extensions` on `decode_*` | ⚠️ Plumbs via `ExprProfile` but model-side adapter is Priority 2 |

*(P1)* = changed by Priority 1. Items still marked ⚠️ or deferred to
Priority 2 are tracked in the Recommendations section below.

## 3. Internal implementation readiness

### 3.1 `openjd-expr/src/function_library.rs`

The dispatch algorithm itself is extension-friendly: it's purely
signature-based. If a new signature appears in the library, it gets
tried in phase 1/2/3 like any other.

The issue is everything around dispatch:

- `FunctionLibrary::new` + `register_sig` is the plugin hook, but
  there is no "extension registration" convention in the source. A
  future `FEATURE_BUNDLE_2` that adds `uuid4()`, `now()`, and
  `env_var(string)` would need (a) a place to live — presumably a new
  category like `feature_bundle_2()` — and (b) a branch in
  `build_default_library` that includes it only when the extension
  is enabled. The current `build_default_library()` has no
  parameters.
- `host_context_enabled: bool` is a single flag. A second extension
  that needs host state (say a hypothetical `SECRETS` extension that
  registers `get_secret(name) -> string` from a host-supplied
  callback) cannot coexist with `apply_path_mapping` in a single
  introspection bit. The flag should become a `HashSet<…>` or be
  replaced by an "enabled extensions" set on the library.
- The operator dispatch path in `evaluator.rs` matches AST operator
  kinds directly to hardcoded dunder names (`Add → "__add__"`, etc.).
  This is fine for today but means an extension cannot, say, remove
  `**` or remap `@` to a domain-specific function. A
  `FunctionLibrary` (or context) should own the operator-to-name map
  so the operator set is data, not code.
- `PYTHON_KEYWORDS: &[&str]` and the keyword rename mechanism in
  `eval/parse.rs` is also hardcoded. If a new revision adds a
  reserved word (e.g., `when`) or lifts one (e.g. allows bare
  `match`), the parse layer can't express it.

### 3.2 `openjd-expr/src/default_library.rs`

200+ `register_sig("…").expect("bad builtin signature")` calls split
across 13 category functions (`arithmetic`, `string_ops`, `list_ops`,
…). Each `.expect(…)` on a builtin literal is fine; the category
functions could each take a context and conditionally register.
Example shape the current code is missing:

```rust
fn string_ops(ctx: &LibraryContext) -> FunctionLibrary { … }
fn fb2_ops(ctx: &LibraryContext) -> FunctionLibrary { … }

pub fn build_library(ctx: &LibraryContext) -> FunctionLibrary {
    let mut lib = FunctionLibrary::new();
    lib = lib.merge(arithmetic(ctx));
    lib = lib.merge(string_ops(ctx));
    // …
    if ctx.has_extension(KnownExtension::FeatureBundle2) {
        lib = lib.merge(fb2_ops(ctx));
    }
    if ctx.host_rules.is_some() {
        lib = register_host_context_functions(lib, ctx.host_rules.clone().unwrap());
    }
    lib
}
```

Today, the entire library is revision/extension-blind and baked once
into a `LazyLock`.

### 3.3 `openjd-expr/src/eval/evaluator.rs`

Hotspots that a new revision could plausibly touch:

- `eval_binop` hardcodes the operator → dunder mapping (Add, Sub,
  Mult, Div, FloorDiv, Mod, Pow, and a set of explicit rejections for
  BitAnd/BitOr/BitXor/LShift/RShift/MatMult).
- `eval_ifexp`, `eval_listcomp`, `eval_slice`, `eval_subscript`,
  `eval_boolop`, `eval_compare` implement structural semantics
  directly. List comprehensions and the walrus operator are
  effectively part of "the language shape"; a new revision that adds
  dict comprehensions or lambda expressions would touch this file
  rather than the library.
- `MAX_EXPRESSION_DEPTH`, `MAX_PARSE_INPUT_LEN`, `DEFAULT_MEMORY_LIMIT`,
  `DEFAULT_OPERATION_LIMIT` are public constants. Tuning them per
  revision would be SemVer-safe (values can change) but forcing them
  per revision is not possible without new API.

None of these are catastrophic, but they indicate that "what the
evaluator supports" is scattered across three layers (AST dispatch,
operator-to-dunder map, library signatures) rather than concentrated
in a single context.

### 3.4 `openjd-model/src/template/validate_v2023_09/*`

Well-structured. Each pass reads `&ValidationContext` and derives its
own view. The passes themselves are extension-parameterised in the
right way. Adding a new pass (or skipping an existing one under a
future revision) is straightforward.

The main internal coupling issue is the module name itself —
`validate_v2023_09`. A future revision won't be able to reuse or
extend this module without a reorganisation.

### 3.5 `openjd-model/src/template/validate_v2023_09/format_strings.rs`

This is where the model directly constructs the three "profiles"
today:

```rust
let default_lib = openjd_expr::default_library::get_default_library().clone();
let host_lib = default_lib.clone().with_unresolved_host_context();
```

and then picks between `&default_lib` / `&host_lib` on a per-call
basis depending on the scope being validated. Every future scope
(e.g. a hypothetical "post-run cleanup" script that has access to a
new `post_run_exit_code()` function) would require another library
variable here and another line of branching, duplicating the pattern.

### 3.6 `openjd-model/src/job/create_job/{mod.rs, instantiate.rs}`

`create_job` rebuilds the `ValidationContext` from
`job_template.extensions` — it does not accept one from the caller.
Similarly `instantiate.rs` calls
`openjd_expr::default_library::get_default_library().clone()` and
`.with_unresolved_host_context()` directly. The choice is hardwired.

## 4. Where the "profile" concept is already implicit

The current code already *has* a profile concept; it is just expressed
as three magic sites rather than data:

| Implicit profile | Where | What it represents |
|---|---|---|
| "submission-time default" | `get_default_library()` | Job / template scope — functions that don't need host state |
| "unresolved host context" | `…with_unresolved_host_context()` | Session / task scope at template-validation time — same signatures as host, but stub implementations |
| "host context" | `…with_host_context(rules)` | Session / task scope at runtime, with real path-mapping rules |

The 2×2 of (revision, scope) is collapsed to a 1×3 by assuming one
revision. Once a second revision exists, it becomes 2×3 minimum and
the three-way choice stops scaling.

A cleaner factoring is to separate the **axes**:

- Axis A: revision (governs which base functions and operators exist).
- Axis B: extension set (governs which add-on functions exist).
- Axis C: host state (governs whether host-context implementations
  are real or stubs).
- Axis D: scope-specific symbol availability (already handled by the
  symbol-table builders in `format_strings.rs`).

Axes A + B + C are exactly what a `ValidationContext`-like struct on
the expr side would model. Axis D is orthogonal and rightly lives
outside the library.

## 5. Recommendations

Priority ordering below reflects both user-impact (future-proofing)
and implementation cost. All are worth considering now while the
project is pre-release.

### Priority 1 — Do before release

1. ~~**Introduce `ExprProfile` (name negotiable) in `openjd-expr`.**
   A small struct that carries the information needed to build a
   function library, independent of `openjd-model`:

   ```rust
   pub struct ExprProfile {
       pub revision: ExprRevision,    // expr's own revision enum, mirrors SpecificationRevision
       pub extensions: ExprExtensions,
       pub host_context: HostContext, // None | Unresolved | WithRules(Arc<Vec<PathMappingRule>>)
   }
   ```

   `ExprRevision` and `ExprExtensions` avoid the reverse-dependency
   problem. Model can provide `From<&ValidationContext> for ExprProfile`
   either as a re-export or in a small adapter module, keeping the
   crate graph unchanged.~~ **Resolved** — added in
   `crates/openjd-expr/src/profile.rs`. `ExprProfile`, `ExprRevision`
   (only `V2026_02` today, `#[non_exhaustive]`), `ExprExtension`
   (empty `#[non_exhaustive]` — reserved for future expr-level
   extensions), `HostContext` (`None | Unresolved | WithRules`).
   Model-side `From<&ValidationContext>` adapter is Priority 2.

2. ~~**Replace `get_default_library()` with `FunctionLibrary::for_profile(&ExprProfile)`.**
   Keep `get_default_library()` as a deprecated alias until 1.0
   (it maps to a fixed profile: current revision, EXPR enabled, no
   host context). This preserves the ergonomic zero-arg entry point
   for simple callers while forcing serious callers to pass a
   profile.~~ **Resolved** — `FunctionLibrary::for_profile(&ExprProfile)`
   added in `default_library.rs`. `get_default_library()` marked
   `#[deprecated(since = "0.2.0", …)]` pointing at the new API. Old
   callers continue to work; migration is Priority 2.

3. ~~**Cache per-profile libraries in a small `LazyLock<DashMap<…>>`**
   (or similar) keyed on a hash of the profile's shape, so that
   hot paths like model validation don't pay registration cost every
   time. The current single `LazyLock` is the only cached library;
   any host-context clone pays for a full `HashMap<String, Vec<_>>`
   copy every call.~~ **Resolved** — added a
   `LazyLock<Mutex<HashMap<ProfileKey, Arc<FunctionLibrary>>>>` cache,
   keyed on the *rules-independent* portion of the profile (revision,
   extensions, host-kind). Rules are per-call by design — the cached
   skeleton is cloned and `with_host_context(rules)` applied on top,
   preserving the existing session hot path. Verified by
   `cache_returns_same_arc_for_none_profile` and
   `with_rules_does_not_cache_rules_variant` tests.

4. ~~**Collapse `with_host_context` and `with_unresolved_host_context`
   into one parameter on the profile.** The split is an artefact of
   having no shared structure. `HostContext::Unresolved` and
   `HostContext::WithRules(rules)` express both cases uniformly and
   generalise to future host-dependent extensions.~~ **Resolved** —
   `HostContext::{None, Unresolved, WithRules(Arc<Vec<PathMappingRule>>)}`
   is a single parameter on `ExprProfile`. The two old
   `FunctionLibrary` methods remain as deprecated convenience wrappers
   for backward compatibility; callers routed through `for_profile`
   use the unified enum.

5. ~~**Mark all cross-crate public enums `#[non_exhaustive]`:**
   `KnownExtension`, `JobParameterType`, `TaskParameterType`,
   `TemplateSpecificationVersion`, `ObjectType`, `DataFlow`,
   `FileType`, `EndOfLine`, `TypeCode`, `PathFormat`. This is the
   single cheapest future-proofing change and should happen before
   the first released version.~~ **Resolved** — marked
   `#[non_exhaustive]`: `KnownExtension`, `JobParameterType`,
   `TaskParameterType`, `TemplateSpecificationVersion`, `FileType`
   (model); `TypeCode` (expr). **Not** marked: `ObjectType` and
   `DataFlow` (closed sets by construction), `EndOfLine` (Lf/Crlf/Auto
   covers the practical space), and `PathFormat` (Posix/Windows/Uri
   cover the practical space — a new variant would be a deliberate
   breaking change if it ever happens).

### Priority 2 — Plumb the profile through the model

6. **Thread `ValidationContext` (or a derived `ExprProfile`) into
   `create_job`.** Accept it as a parameter rather than rebuilding it
   from the template. This makes application-layer policy (e.g.
   "strip EXPR even if requested" or "enforce stricter caller limits
   on this queue") applicable at job-creation time, not only at
   decode time.

7. **Use `EffectiveLimits::from_context(ctx)` at every limit check**
   that currently reads hardcoded numbers. Audit for sites that
   rebuild `EffectiveLimits::default()`.

8. **Let `EffectiveLimits` and `EffectiveRules` branch on
   `ctx.revision` as well as `ctx.extensions`.** Today they ignore
   the revision field. Add a `match ctx.revision { … }` at the top
   of `from_context` even if the match currently has one arm — this
   records the intent and flags the right spot for the first
   revision bump.

9. **Factor `template/validate_v2023_09/` so the passes themselves
   are revision-agnostic.** Move shared infrastructure
   (`format_strings.rs`, `limits.rs`, `structure.rs`, helpers) out
   of the date-named directory into `template/validation/`, and keep
   only revision-specific glue in `v2023_09/`. Then a future
   `v2027_xx` submodule re-uses the pass implementations with
   different `EffectiveLimits` / `EffectiveRules` inputs.

10. **Introduce a `decode_*` dispatch layer** that inspects the
    `specificationVersion` string and routes to the matching
    revision-specific validator. Today the dispatch is a one-arm
    match by construction. Making it explicit now is cheap; making
    it explicit after a second revision is released is disruptive.

### Priority 3 — Internal cleanup to support future operators/keywords

11. **Make the operator→dunder map data, not match arms.** Move
    `Add → "__add__"`, etc. into a small table owned by the
    library (or the profile). This enables an extension to register
    a new operator (if one is ever specified) or remove one.

12. **Move `PYTHON_KEYWORDS` and the keyword-rename mechanism
    behind a profile-derived set.** If a future revision adds a
    reserved word, the parser can pick it up from the profile
    rather than requiring a source edit that becomes a SemVer
    consideration.

13. **Replace `host_context_enabled: bool` with a
    `HashSet<KnownHostFeature>` (or similar).** Prevents "is this
    library host-context-enabled?" from becoming a leaky
    abstraction the moment a second host-sensitive extension
    appears.

### Priority 4 — Documentation

14. **Add `specs/expr/public-api.md` and `specs/model/public-api.md`.**
    Per AGENTS.md ("Every crate's spec directory must include a
    `public-api.md`"), this is currently missing for both crates.
    Use the opportunity to document the profile concept once it
    lands.

15. **Document the stable/unstable surface of `openjd-expr`.** In
    particular, call out which types are `#[non_exhaustive]` and
    which are construction-only (no destructuring guarantees).
    The current public surface is large and tolerably well
    documented per-item, but the stability contract is implicit.

## 6. Appendix — Concrete change sketch for the Priority 1 refactor

This is illustrative, not a finished API. It shows the shape of the
change so the decision becomes concrete.

```rust
// openjd-expr/src/profile.rs (new)

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ExprRevision {
    V2026_02,   // first EXPR revision
    // future: V2027_xx
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ExprExtension {
    // Populated as language-level extensions appear.
    // "EXPR" itself is implicit in having an expression language at all,
    // so revisions gate "is there a language" and extensions gate add-ons.
}

#[derive(Debug, Clone, Default)]
pub enum HostContext {
    #[default]
    None,
    Unresolved,
    WithRules(Arc<Vec<PathMappingRule>>),
}

#[derive(Debug, Clone)]
pub struct ExprProfile {
    pub revision: ExprRevision,
    pub extensions: HashSet<ExprExtension>,
    pub host_context: HostContext,
}

impl ExprProfile {
    pub fn current() -> Self { /* latest revision, no extensions, no host */ }
    pub fn has_extension(&self, ext: ExprExtension) -> bool { /* … */ }
}

// openjd-expr/src/default_library.rs

impl FunctionLibrary {
    pub fn for_profile(profile: &ExprProfile) -> Arc<Self> {
        // Look up in a profile cache, build if missing
    }
}

#[deprecated(note = "use FunctionLibrary::for_profile(&ExprProfile::current())")]
pub fn get_default_library() -> &'static FunctionLibrary { /* … */ }
```

Model-side integration is then a small adapter:

```rust
// openjd-model/src/types.rs

impl ValidationContext {
    pub fn to_expr_profile(&self, host: HostContext) -> ExprProfile {
        ExprProfile {
            revision: self.revision.into(),
            extensions: self.extensions.iter().filter_map(to_expr_ext).collect(),
            host_context: host,
        }
    }
}
```

Callers that today read `openjd_expr::default_library::get_default_library().clone()`
become `FunctionLibrary::for_profile(&ctx.to_expr_profile(HostContext::None))`,
and the library choice is uniformly driven from the context rather
than from three ad-hoc variants.

## 7. Verification

Baseline build before writing this report:

```text
$ cargo build -p openjd-expr -p openjd-model
   Compiling openjd-expr v0.1.0 (…/crates/openjd-expr)
   Compiling openjd-model v0.1.0 (…/crates/openjd-model)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 13.63s
```

Clean, no warnings. The existing quality reports
(`reports/expr-quality-evaluation-report.md`,
`reports/model-quality-evaluation-report.md`) remain the authoritative
source on the general health of each crate; this report focuses only
on the forward-compatibility question.
on the general health of each crate; this report focuses only
on the forward-compatibility question.
