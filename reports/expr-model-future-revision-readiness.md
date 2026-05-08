# Readiness of `openjd-expr` and `openjd-model` for Future Spec Revisions and Extensions

**Date:** 2026-05-08
**Scope:** `openjd-expr`, `openjd-model`
**Focus:** Can the current public interface and internal implementation
accommodate a new spec revision (e.g. `2027-xx`) and new extensions that
add functions, modify function semantics, or change template/expression
interpretation rules?

This report **supersedes** the earlier 2026-05-07 report of the same
name. The earlier pass identified four priority tiers of refactors; the
Priority 1 and Priority 2 tiers have since been implemented (profile
types, `FunctionLibrary::for_profile`, per-profile cache,
revision-dispatch scaffolding in `EffectiveLimits` / `validation::`
/ `decode_*`, `ModelProfile::to_expr_profile`, `create_job` taking a
`ValidationContext`). This pass re-evaluates against the current
codebase, verifies which claims in the prior report's
"Resolved" markers actually hold, and focuses on what remains.

## Executive Summary

The readiness picture has improved substantially since the prior report:

- **Profile plumbing is in place end-to-end.** `ExprProfile` / `ModelProfile` are
  first-class types, `FunctionLibrary::for_profile` replaces the
  `get_default_library` + `with_*_host_context` triple, and the
  revision-dispatch pattern (`match ctx.profile.revision() { … }`) is
  installed at every decision site the prior report called out:
  decode, `EffectiveLimits::from_context`, `validation::validate_*`,
  `create_job`, `JobTemplate::default_validation_context`, and the
  session's derived-library rebuild.
- **Rules-independent profile caching works.** Per-profile libraries are
  cached keyed on `(revision, extensions, host-kind)` so that path-mapping
  rules (which are per-call) don't thrash the cache. The session and
  CLI hot paths pay near-zero registration cost.
- **The library skeleton is now revision-aware.** `build_library_skeleton(profile)`
  has an explicit `match profile.revision()` whose single arm is a
  compile-error sentinel for the first revision bump.

Against the original question — "is the library ready to accept a new
revision and new extensions that add/modify/remove functions or change
the language subset?" — the answer is now **partially yes**:

- ✅ Ready for a revision that **adds or removes functions / signatures**
  at the library level. The profile machinery cleanly selects a
  skeleton, and the in-crate match on `ExprRevision` forces an explicit
  decision for each new revision.
- ✅ Ready for a revision that **changes effective limits, rules, and
  parameter-type allowances** via `EffectiveLimits::from_context_vXXXX_XX`
  and `EffectiveRules::from_context`.
- ⚠️ **Partially ready** for a revision that **changes function
  signatures in place** (e.g., `round(float, int) -> int` in
  2027 vs `float | int` today). The library can hold the new signatures,
  but several callers (evaluator keyword-arg rejection, derive-return-type
  heuristics, coercion rules) have baked-in assumptions that would
  need re-examination. No single obvious failure — but also no forcing
  function like the enum match that would catch the drift automatically.
- ⚠️ **Partially ready** for a revision or extension that **adds a new
  primitive type** (e.g., `Duration`, `Url`). `TypeCode` is
  `#[non_exhaustive]` and the dispatch generalises, but `ExprValue`
  itself is *not* `#[non_exhaustive]` — adding a new variant is a
  breaking change. The parser's literal handlers (`NumberLiteral`,
  `StringLiteral`) would need conditional paths based on revision.
- ❌ **Not ready** for a revision or extension that changes the
  **Python subset the language accepts** — dict comprehensions, walrus,
  multiple `for` clauses, lambda, tuple literals, set comprehensions,
  etc. Those are rejected by hardcoded match arms in
  `validate_structure` in `eval/parse.rs`. There is no profile hook.
- ❌ **Not ready** for a revision or extension that **adds a new
  operator or renames an existing one**. The `Operator::* → "__add__"`
  mapping is a hardcoded `match` in `eval_binop`; `eval_compare` has
  the same pattern. There's no data-driven operator table.
- ❌ **Not ready** for a revision that **adds a reserved identifier** or
  removes one. `PYTHON_KEYWORDS: &[&str]` in `eval/parse.rs` is a hardcoded
  const and the contextual-keyword rename mechanism iterates it directly.
- ⚠️ **`#[non_exhaustive]` coverage is uneven.** The prior report claimed
  this tier was resolved, but inspection shows `ModelExtension`,
  `TaskParameterType`, `TemplateSpecificationVersion`, `FileType`, and
  `ExprValue` (the outer enum, not just the `Path` variant) are
  **not** marked. These are realistic growth axes — especially
  `ModelExtension` and `ExprValue`.
- ❌ **Public-API specs are missing** for both crates
  (`specs/expr/public-api.md`, `specs/model/public-api.md`). Only
  `openjd-snapshots` has one. This is both a gap against the repo's
  own convention (AGENTS.md, "Every crate's spec directory must include
  a `public-api.md`") and a practical obstacle to reasoning about
  stability: there is no single authoritative inventory of what the
  profile refactor has actually exposed.

The most concentrated risk going forward is not the profile machinery —
that part of the design is now good — but three specific hardcoded
tables that any non-trivial language-level extension or revision will
want to change:

1. The operator → dunder name dispatch in `evaluator.rs`.
2. The `PYTHON_KEYWORDS` reserved-word list in `eval/parse.rs`.
3. The unsupported-AST-node rejection list in `validate_structure`
   in `eval/parse.rs`.

Priority 3 and Priority 4 of the prior report remain open and are the
main body of work left. Priority 1 and Priority 2 are effectively
closed, with two specific exceptions under Priority 1 item 5
(non-exhaustive enums) that slipped through.

## 1. Verified state of prior Resolved claims

This section walks every item from the prior report's
recommendations list and records whether it is actually resolved in
the current tree.

### Priority 1 — Do before release

| # | Prior claim | Verification | Status |
|---|---|---|---|
| 1 | `ExprProfile`, `ExprRevision`, `ExprExtension`, `HostContext` added | Present in `crates/openjd-expr/src/profile.rs`, re-exported from `lib.rs`, `ExprRevision` and `ExprExtension` both `#[non_exhaustive]`, `HostContext::{None, Unresolved, WithRules(Arc<Vec<PathMappingRule>>)}` | ✅ **Resolved** |
| 2 | `FunctionLibrary::for_profile` replaces `get_default_library` | Present in `default_library.rs`. `get_default_library` removed from public surface entirely (grep of `crates/` turns up only internal usages in evaluator and JS bindings) | ✅ **Resolved** (cleaner than claimed — the deprecated alias was removed outright) |
| 3 | Per-profile cache keyed on rules-independent key | `PROFILE_CACHE: LazyLock<Mutex<HashMap<ProfileKey, Arc<FunctionLibrary>>>>` in `default_library.rs`; `ProfileKey` excludes rules. Tests `cache_returns_same_arc_for_none_profile`, `cache_returns_same_arc_for_unresolved_profile`, `with_rules_does_not_cache_rules_variant` all pass | ✅ **Resolved** |
| 4 | `HostContext` collapses `with_host_context` + `with_unresolved_host_context` | Single enum, applied via `profile.with_host_context(...)`. The old methods on `FunctionLibrary` are gone from public use | ✅ **Resolved** |
| 5 | Mark all relevant cross-crate public enums `#[non_exhaustive]` | Marked: `SpecificationRevision`, `JobParameterType`, `TypeCode`, `ExprRevision`, `ExprExtension`, `ModelError`, `ExpressionErrorKind`. **Not** marked: `ModelExtension`, `TaskParameterType`, `TemplateSpecificationVersion`, `FileType`, `ExprValue`. The prior report claimed `TaskParameterType`, `TemplateSpecificationVersion`, `FileType` were resolved but they are bare enums in `openjd-model/src/types.rs`. `ExprValue` has `#[non_exhaustive]` only on the `Path` variant, not on the enum itself | ⚠️ **Partially resolved** — see §3 for the specific gaps |

### Priority 2 — Plumb the profile through the model

| # | Prior claim | Verification | Status |
|---|---|---|---|
| 6 | `create_job` takes `&ValidationContext` + `JobTemplate::default_validation_context()` convenience | `create_job::create_job(&JobTemplate, &JobParameterInputValues, &ValidationContext) -> Result<Job, ModelError>` in `lib.rs`; `JobTemplate::default_validation_context()` and `JobTemplate::profile()` in `template/job_template.rs` | ✅ **Resolved** |
| 7 | `EffectiveLimits::from_context` used at every limit check; no stray `default()` | No `impl Default for EffectiveLimits` exists; `max_env_template_param_count` field present. Grep for "EffectiveLimits" across the crate shows only `from_context` construction | ✅ **Resolved** |
| 8 | `EffectiveLimits` / `EffectiveRules` dispatch on revision | `EffectiveLimits::from_context` has the required `match ctx.profile.revision() { SpecificationRevision::V2023_09 => Self::from_context_v2023_09(ctx) }` pattern. `EffectiveRules::from_context` **does not** yet use the same dispatch pattern — it reads extensions directly without a revision match. Minor regression: the intent in item 8 was for both to branch on revision | ⚠️ **Partially resolved** — `EffectiveRules` needs the same `match` wrapper |
| 9 | `template/validation/` layer for revision-neutral dispatch | Present. `template::validation::validate_job_template` / `validate_environment_template` dispatch via `match ctx.profile.revision()` into `validate_v2023_09::*` | ✅ **Resolved** (conservative form, as the prior note said) |
| 10 | Decode layer dispatches on revision | `decode_job_template` now has `match version.revision() { V2023_09 => serde_json::from_value(...) }`. The env-template sibling `decode_environment_template` derives the revision via `version.revision()` and passes it into the context, but does **not** wrap the `serde_json::from_value` call in a revision match. Minor asymmetry: one decoder will produce a compile error at the first revision bump, the other will silently keep using the 2023-09 struct layout | ⚠️ **Partially resolved** — `decode_environment_template` needs the same wrapper |

### Priority 3 — Internal cleanup

| # | Prior claim | Verification | Status |
|---|---|---|---|
| 11 | Operator → dunder table driven by data | Not implemented. `eval_binop` still uses `match b.op { ast::Operator::Add => "__add__", ... }` (evaluator.rs:633). `eval_compare` has the same pattern for `CmpOp` (evaluator.rs:802). Nothing consults the profile | ❌ **Not resolved** |
| 12 | `PYTHON_KEYWORDS` behind a profile-derived set | Not implemented. `const PYTHON_KEYWORDS: &[&str] = &[…]` in `eval/parse.rs` is a static list, referenced directly by `make_replacement` and by the keyword-rename loop in `parse_inner` | ❌ **Not resolved** |
| 13 | Replace `host_context_enabled: bool` with set | Not implemented. `FunctionLibrary` still has `pub host_context_enabled: bool` | ❌ **Not resolved** |

### Priority 4 — Documentation

| # | Prior claim | Verification | Status |
|---|---|---|---|
| 14 | `specs/expr/public-api.md` and `specs/model/public-api.md` | Neither file exists. Only `specs/snapshots/public-api.md` is present | ❌ **Not resolved** |
| 15 | Document stable/unstable surface of `openjd-expr` | Not done. There is no spec document enumerating which types are `#[non_exhaustive]` or construction-only | ❌ **Not resolved** |

### Summary

| Tier | Items | Resolved | Partially | Not resolved |
|------|------:|---------:|----------:|-------------:|
| P1 (core future-proofing) | 5 | 4 | 1 | 0 |
| P2 (model plumbing) | 5 | 3 | 2 | 0 |
| P3 (internal cleanup) | 3 | 0 | 0 | 3 |
| P4 (documentation) | 2 | 0 | 0 | 2 |

The pattern is sharp: everything structural and typed is done or nearly
done; the remaining work is three specific hardcoded tables (operators,
keywords, AST node whitelist) and the missing spec documentation.

## 2. Current profile architecture — how it handles future rev/ext

The refactor that went in between the two reports settled on a clean
three-axis model, matching §4 of the prior report:

- **Axis A — revision.** `ExprRevision` in `openjd-expr`,
  `SpecificationRevision` in `openjd-model`. Both `#[non_exhaustive]`
  and exactly one variant today.
- **Axis B — extensions.** `ExprExtension` (empty `#[non_exhaustive]`)
  in `openjd-expr`, `ModelExtension` in `openjd-model` (not `#[non_exhaustive]` —
  see §3). The crates are independent: `ModelProfile::to_expr_profile`
  is the bridge.
- **Axis C — host state.** `HostContext::{None, Unresolved, WithRules}`
  on `ExprProfile`. Carried as a method call argument (not a profile
  field) into `ModelProfile::to_expr_profile`, since the model has no
  opinion on it.

Each axis has a single place where "for revision R with extensions E",
a compute-derived answer is produced:

| Question | Location | Revision-aware? |
|----------|----------|-----------------|
| Which limits apply? | `EffectiveLimits::from_context` | ✅ `match` arm |
| Which rules apply? | `EffectiveRules::from_context` | ❌ Extensions only |
| Which function library? | `FunctionLibrary::for_profile` → `build_library_skeleton` | ✅ `match` arm |
| Which template types validate? | `template::validation::validate_*_template` | ✅ `match` arm |
| Which template shape decodes? | `decode_*_template` | ✅ `match` arm |
| Which Python subset parses? | `eval/parse.rs::validate_structure` | ❌ Hardcoded list |
| Which operators are active? | `eval/evaluator.rs::eval_binop`, `eval_compare` | ❌ Hardcoded map |
| Which reserved words rename? | `eval/parse.rs::PYTHON_KEYWORDS` | ❌ Hardcoded const |

The top five rows are the profile-driven part — and they cover the
majority of "a new revision changes limits / rules / functions / which
shape decodes". The bottom three rows are the still-hardcoded part and
determine how ready the crate is for a revision or extension that
changes the *language itself*.

## 3. Remaining public-API gaps for future revisions

The specific issues that the prior report's claims missed:

### 3.1 `ModelExtension` is not `#[non_exhaustive]`

```rust
// crates/openjd-model/src/types.rs:326
pub enum ModelExtension {
    TaskChunking,
    RedactedEnvVars,
    FeatureBundle1,
    Expr,
}
```

`ModelExtension` is *the* enum that grows every time an extension
ships. Today it has four variants. Adding a fifth (e.g. the next
feature bundle, or the expression-level extensions the expr crate is
reserving space for) would be a SemVer break for anyone pattern-matching
`ModelExtension`. This one is the highest-value single change in this
report.

### 3.2 `ExprValue` is not `#[non_exhaustive]`

```rust
// crates/openjd-expr/src/value.rs:120
pub enum ExprValue {
    Null,
    Bool(bool),
    Int(i64),
    Float(Float64),
    String(String),
    #[non_exhaustive]
    Path { value: String, format: PathFormat },
    ListBool(Vec<bool>),
    // …
}
```

The `Path` variant is `#[non_exhaustive]`, but the outer enum is not.
Adding a new variant such as `Duration(i64)` or `Url(String)` to
support a future revision's new primitive type would be a SemVer
break. Downstream Rust code frequently exhaustively matches
`ExprValue` — the openjd-model crate's parameter-coercion paths,
for example, cover all ~12 variants — so adding a variant is not
purely theoretical.

### 3.3 `TaskParameterType`, `TemplateSpecificationVersion`, `FileType` are not `#[non_exhaustive]`

```rust
// TaskParameterType (types.rs:235)
pub enum TaskParameterType { Int, Float, String, Path, ChunkInt }

// TemplateSpecificationVersion (types.rs:107)
pub enum TemplateSpecificationVersion {
    JobTemplate2023_09,
    Environment2023_09,
}

// FileType (types.rs:22)
pub enum FileType { Text }
```

- `TaskParameterType`: `ChunkInt` was added via `TASK_CHUNKING`; a
  future `LIST[INT]` task parameter type (analogous to `JobParameterType::ListInt`)
  would break exhaustive matches.
- `TemplateSpecificationVersion`: a `JobTemplate2027_XX` variant is
  essentially certain to exist at the next revision.
- `FileType`: has only `Text` today but the spec has reserved space
  for e.g. `Binary` since RFC 0001 discussion.

All three grow with the spec. All three are exhaustively matched inside
the crate and would be silently forced into needing wildcard arms
on the next addition if external consumers also exhaustive-match.

### 3.4 `EffectiveRules::from_context` does not dispatch on revision

```rust
// template/validate_v2023_09/mod.rs (current)
impl EffectiveRules {
    pub fn from_context(ctx: &ValidationContext) -> Self {
        let expr = ctx.profile.has_extension(ModelExtension::Expr);
        let fb1 = ctx.profile.has_extension(ModelExtension::FeatureBundle1);
        // … no `match ctx.profile.revision()` — directly reads extensions
    }
}
```

`EffectiveLimits::from_context` now dispatches on revision via
`match { V2023_09 => from_context_v2023_09(ctx) }`, but its sibling
`EffectiveRules::from_context` was never given the same treatment.
This is the specific gap from Priority 2 item 8. The fix is one-line
and mirrors the pattern already established for limits; leaving it
out means the first revision bump will have one call site that
silently inherits 2023-09 rules instead of forcing an explicit
per-revision decision.

### 3.5 `build_library_skeleton` ignores `profile.extensions()`

```rust
// default_library.rs:32
fn build_library_skeleton(profile: &ExprProfile) -> FunctionLibrary {
    match profile.revision() {
        ExprRevision::V2026_02 => {
            // Expression-level extensions would be merged in based on
            // `profile.extensions()`; today there are no variants in
            // `ExprExtension`, so no conditional merges are needed.
            build_default_library()
        }
    }
}
```

This is correct *today* (there are no `ExprExtension` variants), but
the comment describes the convention rather than enforcing it. When
the first variant is added, nothing in the code will force the author
to update this function. A small safeguard is to have the function
iterate `profile.extensions()` explicitly, even if the match body for
each extension is empty today, so that adding a variant to
`ExprExtension` produces an exhaustive-match compile error here too.
(The same pattern `EffectiveLimits::from_context` uses for revision.)

### 3.6 `FunctionLibrary::host_context_enabled: bool`

```rust
// function_library.rs:62
pub struct FunctionLibrary {
    functions: HashMap<String, Vec<FunctionEntry>>,
    pub host_context_enabled: bool,
}
```

This flag is currently meaningful only for `apply_path_mapping`. Any
future host-state-dependent function (e.g., a hypothetical
`host_env_var(name)` registered via a `SECRETS` extension) collides
with this single bit. Readers today are `tests/test_function_context.rs`
and the doc examples in `profile.rs` / `default_library.rs`; all of
them use the bool as a "is the host context active?" shorthand. The
cleanest fix is to replace it with a `HashSet<HostFeature>` (parallel
to `Extensions`) so "is feature X active?" remains a single-bit read
but generalises to multiple features. If that seems heavyweight for a
single-feature system, a method `is_host_enabled()` that derives the
answer from signature inspection keeps the reading API stable while
letting the field disappear.

### 3.7 `decode_environment_template` does not wrap struct decoding in a revision match

```rust
// template/parse.rs — env template decoder
let et: EnvironmentTemplate = serde_json::from_value(template)
    .map_err(|e| ModelError::DecodeValidation(format!("'{version_str}' failed checks: {e}")))?;
// … compared to decode_job_template, which has:
let jt: JobTemplate = match version.revision() {
    SpecificationRevision::V2023_09 => serde_json::from_value(template)
        .map_err(|e| ModelError::DecodeValidation(format!("'{version_str}' failed checks: {e}")))?,
};
```

The two decoders diverge. `decode_job_template` was updated to gate
the struct-layout choice behind a revision match (so a future revision
that changes `JobTemplate`'s fields produces a compile error at this
site); `decode_environment_template` was not. The fix is to wrap its
`from_value` call in the same match. One-line change, parallels the
Priority 2 item 10 dispatch work.

## 4. Internal implementation readiness for language changes

The following three items are the concrete Priority 3 work from the
prior report. None has been done.

### 4.1 Operator dispatch is a hardcoded match

```rust
// eval/evaluator.rs:631
fn eval_binop(&mut self, b: &ast::ExprBinOp) -> Result<ExprValue, ExpressionError> {
    let op_name = match b.op {
        ast::Operator::Add => "__add__",
        ast::Operator::Sub => "__sub__",
        // ... 10 more arms ...
        ast::Operator::BitAnd => {
            return Err(ExpressionError::unsupported(
                "Bitwise AND (&) is not supported",
            ))
        }
        // ... more rejected operators ...
    };
    // ...
}
```

The same pattern repeats in `eval_compare` (CmpOp → "__eq__" etc.)
and `eval_boolop`. Consequences for future rev/ext:

- A revision that introduces a new binary operator (say `|>` for
  pipeline application) would need source edits to `eval_binop`
  plus a new AST node handler, rather than "register the dunder and
  wire a profile flag."
- An extension that wants to *remove* `**` (pow) or `%` (mod) has no
  hook: the match always accepts them and dispatches. `FunctionLibrary`
  would fail the call with "no matching signature," but the error
  message would be wrong for the case ("Cannot use '**' operator
  with int and int" instead of "operator ** is not available under
  this profile").
- An extension that remaps `@` (MatMult) to a domain-specific
  operation, as a pure plugin feature, has no hook at all: the match
  unconditionally rejects `@`.

The cleanest refactor is an `OperatorTable` type on (or derived from)
`FunctionLibrary` that maps `ast::Operator` / `ast::CmpOp` / `ast::UnaryOp`
to dunder names, with reject-list support. A single `lookup(op) ->
Result<&str, &'static str>` replaces 14 match arms at each call site,
and the table itself is a tiny associated-const or
`LazyLock<HashMap<…>>`.

### 4.2 Python-subset acceptance is a hardcoded match

```rust
// eval/parse.rs::validate_structure_inner
// ~100 lines of `ast::Expr::Named(_) => return err("Walrus operator (:=) is not supported", …)`,
// `ast::Expr::Lambda(_) => ...`, `ast::Expr::Tuple(_) => ...`, `ast::Expr::DictComp(_) => ...`,
// `ast::Expr::SetComp(_) => ...`, `ast::Expr::Generator(_) => ...`, `ast::Expr::FString(_) => ...`,
// `ast::Expr::EllipsisLiteral(_) => ...`, `ast::Expr::Starred(_) => ...`, `ast::Expr::Await(_) => ...`,
// plus ListComp constraints ("Multiple 'for' clauses ... are not supported",
// "Tuple unpacking ... is not supported", "Multiple 'if' clauses ... are not supported").
```

This list answers the question "what Python-subset does OpenJD
accept?" — precisely the thing a future revision or extension would
most plausibly want to widen (allow dict literals so users can pass
`{"key": value}`? allow f-strings? lift the "multiple `for`
clauses" restriction?). Every one of those decisions is currently a
match arm, not a profile option.

An extension that wanted to lift the "no `match` statements" rule,
for example, would need either:
- A profile-threaded parameter into `validate_structure`, with a
  `profile.ast_allows(NodeKind::Match)` gate inside each rejection arm, or
- A data-driven `AstAcceptance` set on the profile that the match
  consults, with each arm becoming `if !self.ast_allows(NodeKind::Match) { return err(…) }`.

Either way, `validate_structure` today takes no profile. The function
signature is `validate_structure_inner(node, source, depth)`.

The same shape applies to `eval/parse.rs::check_comprehension_shadowing`
(a validation rule specific to one aspect of the accepted subset)
and to the list-comp restrictions inside `validate_structure_inner`.

### 4.3 `PYTHON_KEYWORDS` is a hardcoded const

```rust
// eval/parse.rs:47
const PYTHON_KEYWORDS: &[&str] = &[
    "False", "None", "True", "and", "as", "assert", "async", "await", "break", "class", "continue",
    "def", "del", "elif", "else", "except", "finally", "for", "from", "global", "if", "import",
    "in", "is", "lambda", "nonlocal", "not", "or", "pass", "raise", "return", "try", "while",
    "with", "yield",
];
```

This is the list the contextual-keyword-rename mechanism iterates to
recover from parse errors ("user wrote `Param.if`, rewrite to
`Param.xf`, reparse"). It's reachable because Python's grammar is
context-insensitive but OpenJD wants `.if` to be a legal attribute.

A future revision could plausibly widen or narrow the set:
- If a future Python parser (ruff is on a rolling version) adds a new
  reserved word (e.g., `match`/`case` as hard keywords in a future
  Python), this const silently falls out of sync — the parser will
  reject `Param.match`, but the fallback rename won't kick in because
  `match` isn't in the list.
- If OpenJD decides to allow users to name identifiers that clash
  with Python keywords by some other mechanism (`\if` escape? a
  dedicated syntax?), the rename code needs rewriting. A profile
  hook lets the decision be per-revision.

The refactor is small: move the const into the profile (or into a
library-owned table derived from the profile), and pass it into
`parse_inner`.

## 5. Composite scenario walkthroughs

To make the gaps concrete, here is how four realistic future RFCs
would hit the codebase today.

### 5.1 RFC: "Revision 2027-XX raises `max_identifier_len` baseline to 128"

1. Add `V2027_XX` variant to `SpecificationRevision` (non_exhaustive —
   no SemVer break). ✅
2. Compile error in `EffectiveLimits::from_context` forces a decision.
   Add `V2027_XX => Self::from_context_v2027_xx(ctx)` arm. ✅
3. Compile error in `decode_job_template` → match `version.revision()`
   forces a decision about `JobTemplate` struct layout. `decode_environment_template`
   **silently** keeps using the 2023-09 `EnvironmentTemplate` struct
   because its `from_value` call isn't gated. ⚠️ (Gap §3.7.)
4. Compile error in `template::validation::validate_*_template`
   dispatch forces a decision about pipeline reuse. ✅
5. Compile error in `build_library_skeleton` forces a decision about
   library. ✅
6. `EffectiveRules::from_context` **silently** returns 2023-09 rules —
   no compile error because the function doesn't match on revision.
   ❌ (Gap §3.4.)

Outcome: mostly caught by the compiler, two silent gaps
(§3.4, §3.7).

### 5.2 RFC: "New extension `DICT_LITERAL` adds dict literals"

1. Add `DictLiteral` variant to `ModelExtension`. **Breaks any
   external pattern-match** because `ModelExtension` is not
   `#[non_exhaustive]`. ❌ (Gap §3.1.)
2. Parser's `ast::Expr::Dict(_) => return err("Dict literals are not
   supported", source, node)` in `validate_structure_inner` unconditionally
   rejects. **No profile threading into `validate_structure`**. ❌ (§4.2.)
3. Evaluator has no `eval_dict` handler. Would need adding — but under
   what profile gate? `validate_structure` is not profile-aware so the
   evaluator can trust that only accepted node shapes reach it. ❌
4. `ExprValue` has no `Dict(HashMap<_, _>)` variant. Adding one breaks
   exhaustive matches. ❌ (Gap §3.2.)

Outcome: impossible to add this extension without structural code
changes in at least four places; none of them produce compile errors
against the un-upgraded baseline. All four are gaps listed above.

### 5.3 RFC: "Revision 2027-XX changes `round(float, int) -> int` (drops the `int | float` union)"

1. `FunctionLibrary::for_profile` for the new revision can register
   the new signature *instead of* the old one — the library supports
   per-profile signature sets cleanly. ✅
2. In `build_library_skeleton`, the new revision's arm builds a
   library without the old signature. ✅
3. Test cases in `crates/openjd-expr/tests/` that use `round(x, 1)`
   and expect `float | int` return would need updating — but these
   would fail at test time against the new profile. ✅
4. `derive_return_type` correctly returns `int` for the new signature.
   ✅

Outcome: this case is handled well. The profile design does its job.

### 5.4 RFC: "New extension `PIPELINE_OP` adds `|>` as a new binary operator"

1. `ruff_python_parser` does not parse `|>`. This extension would need
   to change parsers or add a pre-processor. ❌ Out-of-scope for this
   report, but worth noting.
2. If the parser accepted `|>` and produced `ast::Operator::Pipeline`,
   `eval_binop`'s match would not cover it and produce a warning
   (non-exhaustive match) at compile time — but `ast::Operator` is
   external, so the match today uses exhaustive coverage and would
   need a new arm. No profile gating. ❌ (§4.1.)
3. The dispatch would wire through `dispatch_with_node("__pipeline__", ...)`
   and `FunctionLibrary` would register the dunder cleanly. ✅

Outcome: the library accommodates the new operator, but the dispatch
layer is code-shaped, not data-shaped, so the extension has to patch
two files rather than one.

## 6. Specific recommendations

Ordered by value-for-effort, with each item scoped to a single PR.

### Urgent (before release)

1. **Mark `ModelExtension` `#[non_exhaustive]`.** One-line change.
   Highest value because `ModelExtension` is the enum with the highest
   expected rate of change post-release. (Gap §3.1.)

2. **Mark `ExprValue` `#[non_exhaustive]`** (the outer enum, not just
   the `Path` variant). One-line change; the existing `Path`
   attribute is kept for its separate purpose (preventing struct
   construction). (Gap §3.2.)

3. **Mark `TaskParameterType`, `TemplateSpecificationVersion`, `FileType`
   `#[non_exhaustive]`.** Three one-line changes, same rationale.
   (Gap §3.3.)

4. **Add `match ctx.profile.revision()` wrapper to
   `EffectiveRules::from_context`**, dispatching into a
   `from_context_v2023_09(ctx)` helper. Mirrors `EffectiveLimits`
   exactly. (Gap §3.4.)

5. **Make `build_library_skeleton` iterate `profile.extensions()`
   explicitly** (even with an empty match body per extension today), so
   that the first added `ExprExtension` variant produces a compile
   error here. (Gap §3.5.)

6. **Wrap `serde_json::from_value::<EnvironmentTemplate>` in a
   `match version.revision()`** in `decode_environment_template`,
   mirroring `decode_job_template`. (Gap §3.7.)

The six together are probably 35 lines of code and close every
structural Priority 1/2 gap.

### Priority — before first non-trivial extension lands

7. **Replace `host_context_enabled: bool` with a
   `HashSet<HostFeature>`**, or hide it behind an `is_host_enabled()`
   method so callers stop depending on the field directly. (Gap §3.6.)

8. **Extract the operator-to-dunder map.** Move the `match b.op`
   arms in `eval_binop`, the `match op` arms in `eval_compare`, and
   the `UnaryOp` → dunder mapping into a single `OperatorTable`.
   Start with the table owning exactly today's behavior (all accepts
   + the BitOp reject list), then allow profile-driven overrides
   as a second step. (§4.1, Priority 3 item 11.)

9. **Thread the profile into `validate_structure`.** Add a
   `profile: &ExprProfile` parameter to `validate_structure_inner`
   and each rejection arm. Start with every arm reading an empty
   default (no behaviour change), then add `if !profile.allows_dict_literals() { return err(...) }`
   kinds of gates as extensions require them. This is Priority 3
   item 12 generalized. (§4.2.)

10. **Move `PYTHON_KEYWORDS` to a profile-owned set.** Smallest of
    the Priority 3 items. (§4.3.)

### Documentation debt

11. **Write `specs/expr/public-api.md`.** The re-exports in
    `crates/openjd-expr/src/lib.rs` are the starting inventory; each
    item needs a one-line description and a stability classification
    (stable / stable construction-only / non-exhaustive). Use this
    as the opportunity to document the profile concept from first
    principles. (Priority 4 item 14.)

12. **Write `specs/model/public-api.md`.** Same, for `openjd-model`.
    Especially call out `ModelProfile::to_expr_profile` as the
    supported bridge to `openjd-expr`. (Priority 4 item 14.)

13. **Document the `#[non_exhaustive]` surface.** Either in the
    public-api.md docs above, or in a short `specs/expr/stability.md`
    (and model equivalent). The list is small enough to enumerate.
    (Priority 4 item 15.)

## 7. What the current architecture gets right

Since the refactor, several things are notably well-designed for
forward compatibility; worth preserving as the above recommendations
are implemented:

- **The `From<SpecificationRevision>` to `ExprRevision` conversion
  in `ModelProfile::to_expr_profile` is explicit** (a match, not a
  default). When the two enums' variant sets diverge (e.g., model
  V2023_09 keeps working with expr V2026_02 but V2027_XX changes
  both), this is the single place to record the mapping. Well-placed.

- **`ProfileKey` excludes rules** (`HostKind` discriminates only
  `None` / `Unresolved` / `WithRules` presence, not the rules
  themselves), so the session's hot path of "build a library with
  every new set of rules" is a cheap clone-and-register on top of a
  cached skeleton. The comment in `default_library.rs` explaining
  this is also worth keeping.

- **Host state is an argument to `to_expr_profile`, not a field of
  `ModelProfile`.** Correct: the model has no opinion on host state,
  and sessions/CLI do. The current signature `to_expr_profile(&self,
  host_context: HostContext) -> ExprProfile` reflects that cleanly.

- **`JobTemplate::default_validation_context()` and `JobTemplate::profile()`
  give callers a one-call ergonomic hook** for the "just do what the
  template says" case, with override still possible. The session
  hot path and CLI use this pattern consistently.

- **`create_job` takes the validation context already** — so a caller
  that wants to (for example) enforce stricter caller limits at
  job-creation time distinct from decode time can do so, matching
  the prior report's item 6.

- **Tests like `cache_returns_same_arc_for_none_profile` and
  `with_rules_does_not_cache_rules_variant` codify the cache
  behavior as invariants**, not just "probably works." The
  `for_profile_tests` module in `default_library.rs` deliberately
  avoids the deprecated surface to prove the new API stands alone.

## 8. Build and test verification

```text
$ cargo build -p openjd-expr -p openjd-model
   Compiling openjd-model v0.1.0 (.../crates/openjd-model)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 5.18s

$ cargo test -p openjd-expr -p openjd-model --lib
# (truncated) test result: ok. 333 passed; 0 failed; 0 ignored
```

Clean build, no warnings, no failed tests. Baseline is sound; the
gaps above are about structure and specification, not correctness.

## 9. Appendix — Verified file/line references

For reviewers checking this report:

| Claim | File | Anchor |
|-------|------|--------|
| `ExprProfile` exists and is `#[non_exhaustive]` | `crates/openjd-expr/src/profile.rs` | lines 42–77 |
| `FunctionLibrary::for_profile` + cache | `crates/openjd-expr/src/default_library.rs` | lines 17–131 |
| `build_library_skeleton` revision match | `crates/openjd-expr/src/default_library.rs` | lines 36–46 |
| `ModelProfile::to_expr_profile` | `crates/openjd-model/src/types.rs` | ~line 468 (method body) |
| `EffectiveLimits::from_context` revision match | `crates/openjd-model/src/template/validate_v2023_09/mod.rs` | `from_context` + `from_context_v2023_09` |
| `EffectiveRules::from_context` **missing** revision match | `crates/openjd-model/src/template/validate_v2023_09/mod.rs` | `EffectiveRules::from_context` |
| `validation::validate_*_template` revision match | `crates/openjd-model/src/template/validation/mod.rs` | lines 35–57 |
| `decode_job_template` revision match | `crates/openjd-model/src/template/parse.rs` | `match version.revision()` around the `from_value` call |
| `create_job` takes `&ValidationContext` | `crates/openjd-model/src/lib.rs` | `pub use job::create_job::create_job;` |
| `JobTemplate::default_validation_context` + `profile` | `crates/openjd-model/src/template/job_template.rs` | trailing impl block |
| Operator dispatch hardcoded | `crates/openjd-expr/src/eval/evaluator.rs` | lines 631–680 (`eval_binop`), 795–811 (`eval_compare`) |
| `PYTHON_KEYWORDS` const | `crates/openjd-expr/src/eval/parse.rs` | line 47 |
| `validate_structure_inner` accept/reject arms | `crates/openjd-expr/src/eval/parse.rs` | in `validate_structure_inner` — dozen+ `ast::Expr::… => return err(...)` arms |
| `FunctionLibrary::host_context_enabled` bool | `crates/openjd-expr/src/function_library.rs` | line 62 |
| `ModelExtension` (not `#[non_exhaustive]`) | `crates/openjd-model/src/types.rs` | around line 327 |
| `ExprValue` outer enum (not `#[non_exhaustive]`) | `crates/openjd-expr/src/value.rs` | around line 120 |
| `TaskParameterType`, `TemplateSpecificationVersion`, `FileType` (not `#[non_exhaustive]`) | `crates/openjd-model/src/types.rs` | lines 22, 108, 236 |
| No `specs/expr/public-api.md` or `specs/model/public-api.md` | `specs/expr/`, `specs/model/` | directory listing |
