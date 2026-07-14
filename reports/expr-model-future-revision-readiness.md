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
  `#[non_exhaustive]`, `ExprValue` is now `#[non_exhaustive]` (so adding
  a new variant is no longer a SemVer break), and the dispatch
  generalises. The remaining gap is that the parser's literal handlers
  (`NumberLiteral`, `StringLiteral`) would need conditional paths based
  on revision to recognise new literal forms for such a type.
- ✅ **Ready** for a revision or extension that changes the **Python
  subset the language accepts** — dict comprehensions, walrus,
  multiple `for` clauses, lambda, tuple literals, set comprehensions,
  etc. `ParsedExpression::with_profile` and `FormatString::with_profile`
  thread an `&ExprProfile` through `validate_structure_inner`, and each
  rejection is gated on `profile.allows_syntax(SyntaxFeature::X)`.
  `ExprProfile::allows_syntax` resolves in two stages: a revision
  baseline followed by an additive extension layer, so a future
  revision can widen the baseline or a future extension can grant
  additional features. Under V2026_02 both layers return `false`,
  preserving today's behavior.
- ✅ **Ready** for a revision or extension that **adds a new
  operator or renames an existing one** at the dispatch level.
  ~~The `Operator::* → "__add__"` mapping is a hardcoded `match` in
  `eval_binop`; `eval_compare` has the same pattern. There's no
  data-driven operator table.~~ **Resolved** — `OperatorTable` in
  `eval/op_table.rs` centralizes all operator→dunder mappings and the
  gate list, and is consulted by both the parse-time `SyntaxFeature`
  gate and the evaluator's dispatch, so the two layers cannot drift;
  profile-specific eval tables can be constructed when a revision
  first needs one. (Parsing a genuinely new operator token still
  requires parser support — see §5.4.)
- ❌ **Not ready** for a revision that **adds a reserved identifier** or
  removes one. `PYTHON_KEYWORDS: &[&str]` in `eval/parse.rs` is a hardcoded
  const and the contextual-keyword rename mechanism iterates it directly.
- ✅ **`#[non_exhaustive]` coverage is complete.** Every realistic
  growth-axis enum is now marked: `SpecificationRevision`,
  `TemplateSpecificationVersion`, `JobParameterType`,
  `TaskParameterType`, `ModelExtension`, `FileType`, `ExprRevision`,
  `ExprExtension`, `ExprValue` (outer enum), `ModelError`,
  `ExpressionErrorKind`, `TypeCode`. The two enums in §3.1 and §3.3
  that the prior pass missed (`ModelExtension`, `TaskParameterType`,
  `FileType`) were completed in the same pass that fixed the
  accounting errors.
- ✅ **Public-API specs now exist** for both crates
  (`specs/expr/public-api.md`, `specs/model/public-api.md`) plus
  `specs/sessions/public-api.md`, each with an explicit
  Versioning and Stability Conventions section enumerating the
  `#[non_exhaustive]` surface.

The most concentrated risk remaining is one specific hardcoded table.
The other two items from the prior pass have been resolved:

1. ~~The operator → dunder name dispatch in `evaluator.rs`.~~
   **Resolved** — centralized in `eval/op_table.rs::OperatorTable`.
2. The `PYTHON_KEYWORDS` reserved-word list in `eval/parse.rs` —
   **deliberately deferred**: Python's keyword set is stable, so this
   is left as-is until a future revision actually needs to change it.
3. ~~The unsupported-AST-node rejection list in `validate_structure`
   in `eval/parse.rs`.~~ **Resolved** — now profile-gated.

Priority 3 is closed apart from the deliberately-deferred
`PYTHON_KEYWORDS` relocation (the operator-to-dunder table and the
`host_context_enabled` removal are both done). Priority 4 is closed:
public-api.md docs exist for `openjd-expr`, `openjd-model`, and
`openjd-sessions`, each with a Versioning and Stability Conventions
section. Priority 1 is now fully closed — the three enums from §1
item 5 that the earlier passes missed (`ModelExtension`,
`TaskParameterType`, `FileType`) have been marked `#[non_exhaustive]`,
and `build_library_skeleton` now exhaustively matches over
`ExprExtension` so the first added variant produces a compile error.
Priority 2 is fully closed — the two minor dispatch gaps called
out in the previous pass (`EffectiveRules::from_context`,
`decode_environment_template`)
are both resolved.

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
| 5 | Mark all relevant cross-crate public enums `#[non_exhaustive]` | Marked: `SpecificationRevision`, `JobParameterType`, `TypeCode`, `ExprRevision`, `ExprExtension`, `ModelError`, `ExpressionErrorKind`, `TemplateSpecificationVersion`, `ExprValue` (outer enum), `ModelExtension`, `TaskParameterType`, `FileType`. The `Path` variant of `ExprValue` retains its own `#[non_exhaustive]` for its separate construction-restriction purpose | ✅ **Resolved** |

### Priority 2 — Plumb the profile through the model

| # | Prior claim | Verification | Status |
|---|---|---|---|
| 6 | `create_job` takes `&ValidationContext` + `JobTemplate::default_validation_context()` convenience | `create_job::create_job(&JobTemplate, &JobParameterInputValues, &ValidationContext) -> Result<Job, ModelError>` in `lib.rs`; `JobTemplate::default_validation_context()` and `JobTemplate::profile()` in `template/job_template.rs` | ✅ **Resolved** |
| 7 | `EffectiveLimits::from_context` used at every limit check; no stray `default()` | No `impl Default for EffectiveLimits` exists; `max_env_template_param_count` field present. Grep for "EffectiveLimits" across the crate shows only `from_context` construction | ✅ **Resolved** |
| 8 | `EffectiveLimits` / `EffectiveRules` dispatch on revision | `EffectiveLimits::from_context` has the required `match ctx.profile.revision() { SpecificationRevision::V2023_09 => Self::from_context_v2023_09(ctx) }` pattern. `EffectiveRules::from_context` ~~**does not** yet use the same dispatch pattern — it reads extensions directly without a revision match. Minor regression: the intent in item 8 was for both to branch on revision~~ **Resolved** — `EffectiveRules::from_context` now wraps its extension-reading body in `match ctx.profile.revision() { V2023_09 => Self::from_context_v2023_09(ctx) }`, mirroring the limits dispatch exactly | ✅ **Resolved** |
| 9 | `template/validation/` layer for revision-neutral dispatch | Present. `template::validation::validate_job_template` / `validate_environment_template` dispatch via `match ctx.profile.revision()` into `validate_v2023_09::*` | ✅ **Resolved** (conservative form, as the prior note said) |
| 10 | Decode layer dispatches on revision | `decode_job_template` now has `match version.revision() { V2023_09 => serde_json::from_value(...) }`. ~~The env-template sibling `decode_environment_template` derives the revision via `version.revision()` and passes it into the context, but does **not** wrap the `serde_json::from_value` call in a revision match. Minor asymmetry: one decoder will produce a compile error at the first revision bump, the other will silently keep using the 2023-09 struct layout~~ **Resolved** — `decode_environment_template` now wraps its `serde_json::from_value::<EnvironmentTemplate>` call in the same `match version.revision()` pattern | ✅ **Resolved** |

### Priority 3 — Internal cleanup

| # | Prior claim | Verification | Status |
|---|---|---|---|
| 11 | Operator → dunder table driven by data | Implemented — `OperatorTable` in `eval/op_table.rs` centralizes the binop/unaryop/cmpop mappings and the reject list; `eval_binop`, `eval_unaryop`, and `eval_compare` consult it | ✅ **Resolved** |
| 12 | `PYTHON_KEYWORDS` behind a profile-derived set | Not implemented. `const PYTHON_KEYWORDS: &[&str] = &[…]` in `eval/parse.rs` is a static list, referenced directly by `make_replacement` and by the keyword-rename loop in `parse_inner`. **Deliberately skipped** — Python's keyword set is stable; revisit only if a future revision actually changes it | ❌ **Won't do (for now)** |
| 13 | Replace `host_context_enabled: bool` with set | Resolved by removal — the field was vestigial after the profile refactor (no production readers); host-function availability is checked via `get_signatures` | ✅ **Resolved** |

### Priority 4 — Documentation

| # | Prior claim | Verification | Status |
|---|---|---|---|
| 14 | `specs/expr/public-api.md` and `specs/model/public-api.md` | Neither file exists. Only `specs/snapshots/public-api.md` is present | ❌ **Not resolved** |
| 15 | Document stable/unstable surface of `openjd-expr` | Not done. There is no spec document enumerating which types are `#[non_exhaustive]` or construction-only | ❌ **Not resolved** |

### Summary

| Tier | Items | Resolved | Partially | Not resolved |
|------|------:|---------:|----------:|-------------:|
| P1 (core future-proofing) | 5 | 4 | 1 | 0 |
| P2 (model plumbing) | 5 | 5 | 0 | 0 |
| P3 (internal cleanup) | 3 | 2 | 0 | 1 (deferred) |
| P4 (documentation) | 2 | 2 | 0 | 0 |

Everything structural and typed is done. The operator→dunder dispatch
now lives in `eval/op_table.rs::OperatorTable` and the vestigial
`host_context_enabled: bool` has been removed. The one remaining item —
relocating `PYTHON_KEYWORDS` behind the profile — is deliberately
deferred because Python's keyword set is stable. The AST-node
whitelist and the missing spec documentation are both resolved.

## 2. Current profile architecture — how it handles future rev/ext

The refactor that went in between the two reports settled on a clean
three-axis model, matching §4 of the prior report:

- **Axis A — revision.** `ExprRevision` in `openjd-expr`,
  `SpecificationRevision` in `openjd-model`. Both `#[non_exhaustive]`
  and exactly one variant today.
- **Axis B — extensions.** `ExprExtension` (empty `#[non_exhaustive]`)
  in `openjd-expr`, `ModelExtension` (`#[non_exhaustive]` with four
  variants today) in `openjd-model`. The crates are independent:
  `ModelProfile::to_expr_profile` is the bridge.
- **Axis C — host state.** `HostContext::{None, Unresolved, WithRules}`
  on `ExprProfile`. Carried as a method call argument (not a profile
  field) into `ModelProfile::to_expr_profile`, since the model has no
  opinion on it.

Each axis has a single place where "for revision R with extensions E",
a compute-derived answer is produced:

| Question | Location | Revision-aware? |
|----------|----------|-----------------|
| Which limits apply? | `EffectiveLimits::from_context` | ✅ `match` arm |
| Which rules apply? | `EffectiveRules::from_context` | ✅ `match` arm |
| Which function library? | `FunctionLibrary::for_profile` → `build_library_skeleton` | ✅ `match` arm |
| Which template types validate? | `template::validation::validate_*_template` | ✅ `match` arm |
| Which template shape decodes? | `decode_*_template` | ✅ `match` arm |
| Which Python subset parses? | `eval/parse.rs::validate_structure` | ✅ Profile-threaded, data-driven via `SyntaxFeature` |
| Which operators are active? | `eval/op_table.rs::OperatorTable` | ✅ Single data table consulted by both parse (profile-gated via `SyntaxFeature`) and eval (unconditional backstop); profile-specific eval tables when first needed |
| Which reserved words rename? | `eval/parse.rs::PYTHON_KEYWORDS` | ❌ Hardcoded const (deliberately deferred — Python's keyword set is stable) |

The top five rows are the profile-driven part — and they cover the
majority of "a new revision changes limits / rules / functions / which
shape decodes". The bottom three rows are the still-hardcoded part and
determine how ready the crate is for a revision or extension that
changes the *language itself*.

## 3. Remaining public-API gaps for future revisions

The specific issues that the prior report's claims missed:

### 3.1 `ModelExtension` is not `#[non_exhaustive]`

**Resolved.** `ModelExtension` now carries `#[non_exhaustive]`.
Adding a new variant (e.g. the next feature bundle, or an
expression-level extension the expr crate is reserving space for)
is no longer a SemVer break for downstream crates that pattern-match
on it. The prior state is preserved below for historical reference.

```rust
// crates/openjd-model/src/types.rs:326 (previous, NOT non_exhaustive)
pub enum ModelExtension {
    TaskChunking,
    RedactedEnvVars,
    FeatureBundle1,
    Expr,
}
```

~~`ModelExtension` is *the* enum that grows every time an extension
ships. Today it has four variants. Adding a fifth (e.g. the next
feature bundle, or the expression-level extensions the expr crate is
reserving space for) would be a SemVer break for anyone pattern-matching
`ModelExtension`. This one is the highest-value single change in this
report.~~

### 3.2 `ExprValue` is not `#[non_exhaustive]`

**Resolved.** The outer `ExprValue` enum now carries
`#[non_exhaustive]`. Adding a new variant such as `Duration(i64)` or
`Url(String)` to support a future revision's new primitive type is no
longer a SemVer break. The `Path` variant retains its own
`#[non_exhaustive]` attribute, which serves a separate purpose —
preventing direct struct-literal construction from outside the crate
so that `ExprValue::new_path` can enforce the separator-normalization
invariant.

```rust
// crates/openjd-expr/src/value.rs (previous)
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

~~The `Path` variant is `#[non_exhaustive]`, but the outer enum is not.
Adding a new variant such as `Duration(i64)` or `Url(String)` to
support a future revision's new primitive type would be a SemVer
break. Downstream Rust code frequently exhaustively matches
`ExprValue` — the openjd-model crate's parameter-coercion paths,
for example, cover all ~12 variants — so adding a variant is not
purely theoretical.~~

### 3.3 `TaskParameterType` and `FileType` are not `#[non_exhaustive]` (`TemplateSpecificationVersion` is)

**Resolved.** Both `TaskParameterType` and `FileType` now carry
`#[non_exhaustive]`, as does `TemplateSpecificationVersion` (which
was resolved earlier). The prior state is preserved below for
historical reference.

```rust
// TaskParameterType (types.rs:241) — previously NOT non_exhaustive
pub enum TaskParameterType { Int, Float, String, Path, ChunkInt }

// FileType (types.rs:22) — previously NOT non_exhaustive
pub enum FileType { Text }
```

~~- `TaskParameterType`: `ChunkInt` was added via `TASK_CHUNKING`; a
  future `LIST[INT]` task parameter type (analogous to `JobParameterType::ListInt`)
  would break exhaustive matches.~~
~~- `FileType`: has only `Text` today but the spec has reserved space
  for e.g. `Binary` since RFC 0001 discussion.~~

~~Both grow with the spec. Both are exhaustively matched inside
the crate and would be silently forced into needing wildcard arms
on the next addition if external consumers also exhaustive-match.~~

### 3.4 `EffectiveRules::from_context` does not dispatch on revision

**Resolved.** `EffectiveRules::from_context` now wraps its body in
`match ctx.profile.revision() { V2023_09 => Self::from_context_v2023_09(ctx) }`,
mirroring `EffectiveLimits::from_context` exactly. The prior form is
preserved below for historical reference.

```rust
// template/validate_v2023_09/mod.rs (previous)
impl EffectiveRules {
    pub fn from_context(ctx: &ValidationContext) -> Self {
        let expr = ctx.profile.has_extension(ModelExtension::Expr);
        let fb1 = ctx.profile.has_extension(ModelExtension::FeatureBundle1);
        // … no `match ctx.profile.revision()` — directly reads extensions
    }
}
```

~~`EffectiveLimits::from_context` now dispatches on revision via
`match { V2023_09 => from_context_v2023_09(ctx) }`, but its sibling
`EffectiveRules::from_context` was never given the same treatment.
This is the specific gap from Priority 2 item 8. The fix is one-line
and mirrors the pattern already established for limits; leaving it
out means the first revision bump will have one call site that
silently inherits 2023-09 rules instead of forcing an explicit
per-revision decision.~~

### 3.5 `build_library_skeleton` ignores `profile.extensions()`

**Resolved.** `build_library_skeleton` now iterates
`profile.extensions()` with an exhaustive empty match on each
`ExprExtension` variant. Since `ExprExtension` has no variants today,
the loop body is unreachable — but the `match *ext {}` is the
forcing function: adding the first variant produces a compile error
at this site, requiring the author to declare how the new variant
modifies the library. The prior form is preserved below for
historical reference.

```rust
// default_library.rs (previous form — comment-only, no forcing function)
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

~~This is correct *today* (there are no `ExprExtension` variants), but
the comment describes the convention rather than enforcing it. When
the first variant is added, nothing in the code will force the author
to update this function. A small safeguard is to have the function
iterate `profile.extensions()` explicitly, even if the match body for
each extension is empty today, so that adding a variant to
`ExprExtension` produces an exhaustive-match compile error here too.
(The same pattern `EffectiveLimits::from_context` uses for revision.)~~

### 3.6 `FunctionLibrary::host_context_enabled: bool`

**Resolved** — the field was removed outright. After the profile
refactor no production code read it (zero references in
`openjd-model`, `openjd-sessions`, `openjd-for-js`, and none in the
expr evaluator itself); the only readers were tests and doc examples
asserting the flag was set. Host-function availability is determined
by whether the function is actually registered — callers check
`get_signatures("apply_path_mapping")` — so the single-bit collision
concern below no longer applies. The prior state is preserved for
historical reference.

```rust
// function_library.rs:62 (previous)
pub struct FunctionLibrary {
    functions: HashMap<String, Vec<FunctionEntry>>,
    pub host_context_enabled: bool,
}
```

~~This flag is currently meaningful only for `apply_path_mapping`. Any
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
letting the field disappear.~~

### 3.7 `decode_environment_template` does not wrap struct decoding in a revision match

**Resolved.** `decode_environment_template` now wraps its
`serde_json::from_value::<EnvironmentTemplate>` call in
`match version.revision() { SpecificationRevision::V2023_09 => … }`,
matching `decode_job_template`. A future revision that changes the
`EnvironmentTemplate` struct layout will produce a compile error at
this site. The prior asymmetry is preserved below for historical
reference.

```rust
// template/parse.rs — env template decoder (previous)
let et: EnvironmentTemplate = serde_json::from_value(template)
    .map_err(|e| ModelError::DecodeValidation(format!("'{version_str}' failed checks: {e}")))?;
// … compared to decode_job_template, which already had:
let jt: JobTemplate = match version.revision() {
    SpecificationRevision::V2023_09 => serde_json::from_value(template)
        .map_err(|e| ModelError::DecodeValidation(format!("'{version_str}' failed checks: {e}")))?,
};
```

~~The two decoders diverge. `decode_job_template` was updated to gate
the struct-layout choice behind a revision match (so a future revision
that changes `JobTemplate`'s fields produces a compile error at this
site); `decode_environment_template` was not. The fix is to wrap its
`from_value` call in the same match. One-line change, parallels the
Priority 2 item 10 dispatch work.~~

## 4. Internal implementation readiness for language changes

The following three items are the concrete Priority 3 work from the
prior report. §4.1 and §4.2 are resolved; §4.3 is deliberately
deferred.

### 4.1 Operator dispatch is a hardcoded match

**Resolved** — the operator→dunder mappings for `eval_binop`,
`eval_unaryop`, and `eval_compare`, along with the gate list
(bitwise ops, shifts, `@`, `~`, `is`, `is not`), moved into a single
`OperatorTable` type in `eval/op_table.rs`. Each operator maps to
either a dunder (`OpSpec::Dunder` / `CmpOpSpec::Dispatch`, where
`CmpDispatch` also carries the `in`/`not in` argument-order flag) or
a gate (`OpSpec::Gated`) pairing the `SyntaxFeature` that would enable
it with its rejection message. Both layers consult the same table:
`validate_structure` in `eval/parse.rs` reads the `*_spec` lookups and
applies `profile.allows_syntax` to the gate feature, while the
evaluator's `Result`-returning lookups reject gated operators
unconditionally as a defense-in-depth backstop behind the parse gate —
so the parse-time and eval-time error messages cannot drift apart, and
granting an operator's `SyntaxFeature` is the single switch at parse
time. (Eval-side enablement additionally needs profile plumbing into
the evaluator — e.g. a table derived from `FunctionLibrary` — plus a
registered dunder, when an extension first needs it.) The matches are
exhaustive without wildcards so a new operator variant from a
`ruff_python_ast` upgrade is a compile error in one file. The prior
state is preserved below for historical reference.

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

**Resolved.** `validate_structure` now takes an `&ExprProfile`, each
rejection arm is gated on `profile.allows_syntax(SyntaxFeature::X)`,
and the `SyntaxFeature` enum enumerates the full set of optional
syntax points (lambda, walrus, dict/set literals, f-strings, each
bitwise operator, keyword arguments, multi-`for`/multi-`if`
comprehensions, etc.). Under V2026_02 every feature returns `false`,
preserving today's rejection list exactly.

`ExprProfile::allows_syntax` resolves in two stages:

1. **Revision baseline** — each revision has a per-revision helper
   (today `baseline_syntax_v2026_02`) that decides which features are
   in the baseline for that revision. A future revision that wants to
   fold dict literals into the core language flips one match arm
   here.
2. **Extension layer (additive)** — each revision also has a helper
   (today `extension_syntax_v2026_02`) that iterates the profile's
   enabled `ExprExtension` variants and returns `true` if any of them
   grants the feature under the current revision. Extensions can only
   add features; they cannot remove features already in the baseline.
   The shape is in place even though `ExprExtension` has no variants
   today — adding a variant produces a compile error inside the
   exhaustive `match *ext` that forces the contributor to declare
   which syntax features (if any) the new variant grants.

`ParsedExpression::new(expr)` now uses `ExprProfile::latest()` (the
current revision + every `ExprExtension::ALL`, intentionally unstable
across crate versions). `ParsedExpression::with_profile(expr, profile)`
gives callers stability. The same split applies to `FormatString::new`
and `FormatString::with_profile`. Production call sites in
`openjd-model` (let-binding evaluation in `instantiate.rs`,
let-binding validation in
`template/validate_v2023_09/format_strings.rs`) have been migrated to
`with_profile` using the `ValidationContext`'s model profile.

The prior state is preserved below for historical reference:

```rust
// eval/parse.rs::validate_structure_inner (previous)
// ~100 lines of `ast::Expr::Named(_) => return err("Walrus operator (:=) is not supported", …)`,
// `ast::Expr::Lambda(_) => ...`, `ast::Expr::Tuple(_) => ...`, `ast::Expr::DictComp(_) => ...`,
// `ast::Expr::SetComp(_) => ...`, `ast::Expr::Generator(_) => ...`, `ast::Expr::FString(_) => ...`,
// `ast::Expr::EllipsisLiteral(_) => ...`, `ast::Expr::Starred(_) => ...`, `ast::Expr::Await(_) => ...`,
// plus ListComp constraints ("Multiple 'for' clauses ... are not supported",
// "Tuple unpacking ... is not supported", "Multiple 'if' clauses ... are not supported").
```

~~This list answers the question "what Python-subset does OpenJD
accept?" — precisely the thing a future revision or extension would
most plausibly want to widen (allow dict literals so users can pass
`{"key": value}`? allow f-strings? lift the "multiple `for`
clauses" restriction?). Every one of those decisions is currently a
match arm, not a profile option.~~

~~An extension that wanted to lift the "no `match` statements" rule,
for example, would need either:
- A profile-threaded parameter into `validate_structure`, with a
  `profile.ast_allows(NodeKind::Match)` gate inside each rejection arm, or
- A data-driven `AstAcceptance` set on the profile that the match
  consults, with each arm becoming `if !self.ast_allows(NodeKind::Match) { return err(…) }`.~~

~~Either way, `validate_structure` today takes no profile. The function
signature is `validate_structure_inner(node, source, depth)`.~~

The same shape applies to `eval/parse.rs::check_comprehension_shadowing`
(a validation rule specific to one aspect of the accepted subset)
and to the list-comp restrictions inside `validate_structure_inner`.
Those remain open — `check_comprehension_shadowing` still runs
unconditionally. However, comprehension shadowing is a semantic
constraint (what does a repeated name mean?) rather than a
Python-subset gate, so it is much less likely to be something a
future revision wants to toggle.

### 4.3 `PYTHON_KEYWORDS` is a hardcoded const

**Deliberately deferred** — Python's keyword set is stable, so the
value of profile-owning this list is low until a revision or Python
grammar change actually alters it. Left as-is by explicit decision.

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
   forces a decision about `JobTemplate` struct layout. Compile error
   in `decode_environment_template` likewise forces a decision about
   `EnvironmentTemplate` struct layout. ✅ (Gap §3.7 resolved.)
4. Compile error in `template::validation::validate_*_template`
   dispatch forces a decision about pipeline reuse. ✅
5. Compile error in `build_library_skeleton` forces a decision about
   library. ✅
6. Compile error in `EffectiveRules::from_context` forces a decision
   about per-revision rules. ✅ (Gap §3.4 resolved.)

Outcome: fully caught by the compiler at every top-level dispatch site.

### 5.2 RFC: "New extension `DICT_LITERAL` adds dict literals"

1. Add `DictLiteral` variant to `ModelExtension`. No SemVer break —
   `ModelExtension` is now `#[non_exhaustive]`. ✅ (Gap §3.1 resolved.)
2. Gate `SyntaxFeature::DictLiteral` (or similar new variant) to
   return `true` for the new extension inside
   `ExprProfile::extension_syntax_v2026_02`. The profile-threaded
   `validate_structure` will then accept `ast::Expr::Dict(_)` when
   the extension is enabled. ✅ (§4.2 resolved.)
3. Evaluator needs an `eval_dict` handler. With the profile-gated
   parser letting dict literals through only under the extension,
   the evaluator can branch on the new AST node kind and build the
   value. Still a source edit, but no operator-table surgery.
4. Add a `Dict(HashMap<_, _>)` variant to `ExprValue`. No SemVer break —
   `ExprValue` is now `#[non_exhaustive]`. ✅ (Gap §3.2 resolved.)

Outcome: every structural SemVer blocker is resolved. Adding the
extension is now a well-scoped source edit: one variant on
`ModelExtension`, one `SyntaxFeature` entry, one `eval_dict` handler,
and one `ExprValue` variant — plus corresponding test cases.

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
   the exhaustive match inside `OperatorTable::binop` would fail to
   compile until a mapping (or rejection) is added — a single edit in
   `eval/op_table.rs`. ✅ (§4.1 resolved.)
3. The dispatch would wire through `dispatch_with_node("__pipeline__", ...)`
   and `FunctionLibrary` would register the dunder cleanly. ✅

Outcome: apart from the parser itself, the dispatch layer is now
data-shaped — the extension patches the operator table and registers
the dunder. For an operator that is currently *gated* (e.g. an
extension enabling `&`), the parse-time switch is granting the
`SyntaxFeature` in the profile, which the table's gate entries key on;
the evaluator's unconditional backstop then additionally needs profile
plumbing (e.g. a table derived from `FunctionLibrary`) before the
operator evaluates — that plumbing is deliberately deferred until an
extension first needs it.

## 6. Specific recommendations

Ordered by value-for-effort, with each item scoped to a single PR.

### Urgent (before release)

1. ~~**Mark `ModelExtension` `#[non_exhaustive]`.** One-line change.
   Highest value because `ModelExtension` is the enum with the highest
   expected rate of change post-release. The 2026-05-07 pass claimed
   this was resolved and the 2026-05-08 pass initially repeated that
   claim; both were wrong. `types.rs:332` is still a plain enum.
   (Gap §3.1.)~~ **Resolved.**

2. ~~**Mark `ExprValue` `#[non_exhaustive]`** (the outer enum, not just
   the `Path` variant). One-line change; the existing `Path`
   attribute is kept for its separate purpose (preventing struct
   construction). (Gap §3.2.)~~ **Resolved.**

3. ~~**Mark `TaskParameterType` and `FileType` `#[non_exhaustive]`.**
   Two one-line changes, same rationale.
   `TemplateSpecificationVersion` (originally bundled with these)
   has since been marked; `TaskParameterType` at `types.rs:241` and
   `FileType` at `types.rs:22` remain plain enums. (Gap §3.3.)~~ **Resolved.**

4. ~~**Add `match ctx.profile.revision()` wrapper to
   `EffectiveRules::from_context`**, dispatching into a
   `from_context_v2023_09(ctx)` helper. Mirrors `EffectiveLimits`
   exactly. (Gap §3.4.)~~ **Resolved.**

5. ~~**Make `build_library_skeleton` iterate `profile.extensions()`
   explicitly** (even with an empty match body per extension today), so
   that the first added `ExprExtension` variant produces a compile
   error here. (Gap §3.5.)~~ **Resolved** — `build_library_skeleton`
   now contains `for ext in profile.extensions() { match *ext {} }`
   after the revision match. The empty match on an uninhabited-today
   `ExprExtension` is the forcing function: adding the first variant
   produces a compile error at this site, requiring the author to
   declare how the new variant affects the library.

6. ~~**Wrap `serde_json::from_value::<EnvironmentTemplate>` in a
   `match version.revision()`** in `decode_environment_template`,
   mirroring `decode_job_template`. (Gap §3.7.)~~ **Resolved.**

The six together are probably 35 lines of code and close every
structural Priority 1/2 gap.

### Priority — before first non-trivial extension lands

7. ~~**Replace `host_context_enabled: bool` with a
   `HashSet<HostFeature>`**, or hide it behind an `is_host_enabled()`
   method so callers stop depending on the field directly. (Gap §3.6.)~~
   **Resolved** — the field was removed entirely; no production code
   read it after the profile refactor. Host-function availability is
   now checked via `get_signatures("apply_path_mapping")`.

8. ~~**Extract the operator-to-dunder map.** Move the `match b.op`
   arms in `eval_binop`, the `match op` arms in `eval_compare`, and
   the `UnaryOp` → dunder mapping into a single `OperatorTable`.
   Start with the table owning exactly today's behavior (all accepts
   + the BitOp reject list), then allow profile-driven overrides
   as a second step. (§4.1, Priority 3 item 11.)~~ **Resolved** —
   `OperatorTable` in `eval/op_table.rs` owns all three mappings and
   the reject list, with exactly the prior behavior. Profile-driven
   overrides remain a second step for when a revision or extension
   first needs one.

9. ~~**Thread the profile into `validate_structure`.** Add a
   `profile: &ExprProfile` parameter to `validate_structure_inner`
   and each rejection arm. Start with every arm reading an empty
   default (no behaviour change), then add `if !profile.allows_dict_literals() { return err(...) }`
   kinds of gates as extensions require them. This is Priority 3
   item 12 generalized. (§4.2.)~~ **Resolved.** `ParsedExpression::with_profile`
   and `FormatString::with_profile` thread an `&ExprProfile` through
   `validate_structure_inner`. Every previously-hardcoded rejection is
   gated on `profile.allows_syntax(SyntaxFeature::X)`; under V2026_02
   every `SyntaxFeature` variant returns `false`, preserving behavior.
   `ParsedExpression::new` / `FormatString::new` retain their zero-arg
   form but now use the "latest" profile (current revision + every
   known extension enabled), explicitly documented as unstable across
   crate versions.

10. **Move `PYTHON_KEYWORDS` to a profile-owned set.** Smallest of
    the Priority 3 items. (§4.3.) **Deliberately deferred** — Python's
    keyword set is stable; revisit only if a future revision or a
    Python grammar change actually alters it.

### Documentation debt

11. ~~**Write `specs/expr/public-api.md`.** The re-exports in
    `crates/openjd-expr/src/lib.rs` are the starting inventory; each
    item needs a one-line description and a stability classification
    (stable / stable construction-only / non-exhaustive). Use this
    as the opportunity to document the profile concept from first
    principles. (Priority 4 item 14.)~~ **Resolved** — added in
    `specs/expr/public-api.md`. Organised by module role (profile,
    library, types, values, symbol table, format string, range
    expression, path mapping, error), with explicit stability
    classification per enum (which ones are `#[non_exhaustive]`,
    which are intentionally closed, what the defensive caps'
    contract is). Documents the profile axes (revision / extension
    / host-context) from first principles.

12. ~~**Write `specs/model/public-api.md`.** Same, for `openjd-model`.
    Especially call out `ModelProfile::to_expr_profile` as the
    supported bridge to `openjd-expr`. (Priority 4 item 14.)~~
    **Resolved** — added in `specs/model/public-api.md`. Covers every
    crate-root re-export plus the reachable `job::*` and `template::*`
    surfaces, with emphasis on `ModelProfile::to_expr_profile` as the
    documented bridge to `openjd-expr`. Also resolves the parallel
    need for `specs/sessions/public-api.md`, which was out of scope
    of the original report but shares the same conventions.

13. ~~**Document the `#[non_exhaustive]` surface.** Either in the
    public-api.md docs above, or in a short `specs/expr/stability.md`
    (and model equivalent). The list is small enough to enumerate.
    (Priority 4 item 15.)~~ **Resolved** — each of the three
    public-api.md documents ends with a Versioning and Stability
    Conventions section enumerating the enums marked
    `#[non_exhaustive]`, the enums intentionally closed (with
    rationale), and the defensive caps whose values form part of
    the stability contract.

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
| `ExprProfile` exists and is `#[non_exhaustive]` | `crates/openjd-expr/src/profile.rs` | `ExprRevision` at line 47, `ExprExtension` at line 75 |
| `FunctionLibrary::for_profile` + cache | `crates/openjd-expr/src/default_library.rs` | lines 17–131 |
| `build_library_skeleton` revision + extension match | `crates/openjd-expr/src/default_library.rs` | revision match at lines ~47–53; `for ext in profile.extensions() { match *ext {} }` immediately below |
| `ModelProfile::to_expr_profile` | `crates/openjd-model/src/types.rs` | method on `ModelProfile` |
| `EffectiveLimits::from_context` revision match | `crates/openjd-model/src/template/validate_v2023_09/mod.rs` | `impl EffectiveLimits` at line 52, `from_context` at line 53, `from_context_v2023_09` at line 64 |
| `EffectiveRules::from_context` revision match (resolved) | `crates/openjd-model/src/template/validate_v2023_09/mod.rs` | `impl EffectiveRules` at line 92, `from_context` at line 93 |
| `validation::validate_*_template` revision match | `crates/openjd-model/src/template/validation/mod.rs` | `validate_job_template` at line 41, `validate_environment_template` immediately below |
| `decode_job_template` revision match | `crates/openjd-model/src/template/parse.rs` | line 218 |
| `decode_environment_template` revision match (resolved) | `crates/openjd-model/src/template/parse.rs` | line 275 |
| `create_job` takes `&ValidationContext` | `crates/openjd-model/src/job/create_job/mod.rs` | line 49 |
| `JobTemplate::default_validation_context` + `profile` | `crates/openjd-model/src/template/job_template.rs` | `profile()` at line 59, `default_validation_context()` at line 90 |
| Operator dispatch table (resolved) | `crates/openjd-expr/src/eval/op_table.rs` | `OperatorTable` with `binop` / `unaryop` / `cmpop` lookups; consulted by `eval_binop`, `eval_unaryop`, `eval_compare` |
| `PYTHON_KEYWORDS` const | `crates/openjd-expr/src/eval/parse.rs` | line 52 |
| `validate_structure_inner` profile-gated | `crates/openjd-expr/src/eval/parse.rs` | `validate_structure(&expr_node, expr_str, profile)` at line 511; `SyntaxFeature` imported at line 8 |
| `FunctionLibrary::host_context_enabled` bool (removed) | `crates/openjd-expr/src/function_library.rs` | field deleted; availability checked via `get_signatures` |
| `ModelExtension` `#[non_exhaustive]` | `crates/openjd-model/src/types.rs` | attribute + enum at ~line 335 |
| `TemplateSpecificationVersion` `#[non_exhaustive]` | `crates/openjd-model/src/types.rs` | attribute at line 112, enum at line 113 |
| `TaskParameterType` `#[non_exhaustive]` | `crates/openjd-model/src/types.rs` | attribute + enum at ~line 247 |
| `FileType` `#[non_exhaustive]` | `crates/openjd-model/src/types.rs` | attribute + enum at ~line 26 |
| `ExprValue` outer enum `#[non_exhaustive]` | `crates/openjd-expr/src/value.rs` | attribute at line 128, enum at line 129 |
| `specs/expr/public-api.md` exists | `specs/expr/public-api.md` | ~59 KB, includes Versioning and Stability Conventions |
| `specs/model/public-api.md` exists | `specs/model/public-api.md` | ~36 KB, includes Versioning and Stability Conventions |
| `specs/sessions/public-api.md` exists | `specs/sessions/public-api.md` | ~35 KB |
