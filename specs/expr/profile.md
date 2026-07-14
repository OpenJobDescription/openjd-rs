# Expression Profiles

The profile system (`src/profile.rs`) models *which* expression language a
caller wants: the specification revision, the enabled extensions, and the
host-context state. A profile is the single input from which the crate
derives the accepted syntax, the registered function library, and the
availability of host-dependent functions.

## The Three Axes

A profile is the tuple of three independent axes:

| Axis | Type | Question it answers |
|------|------|---------------------|
| A — revision | `ExprRevision` | Which base functions, operators, and syntax exist? |
| B — extensions | `HashSet<ExprExtension>` | Which add-on features are enabled on top of the revision? |
| C — host state | `HostContext` | Are host-context function implementations real, stubbed, or absent? |

A fourth axis — scope-specific symbol availability (which `Param.*` /
`Task.*` names are defined) — is deliberately *not* part of the profile.
Callers express it by building an appropriate `SymbolTable`; it is
orthogonal to the language definition.

## Types

### `ExprRevision`

```rust
#[non_exhaustive]
pub enum ExprRevision {
    V2026_02,   // first revision to define the expression language (RFC 0005)
}

impl ExprRevision {
    pub const CURRENT: ExprRevision = ExprRevision::V2026_02;
}
```

Mirrors `SpecificationRevision` in `openjd-model`, but lives here so the
expression crate has no model dependency. `ModelProfile::to_expr_profile`
in `openjd-model` is the documented bridge between the two.
`#[non_exhaustive]` so future revisions are not a SemVer break.

### `ExprExtension`

```rust
#[non_exhaustive]
pub enum ExprExtension {}   // empty today — reserved API shape

impl ExprExtension {
    pub const ALL: &'static [ExprExtension] = &[];
}
```

Expression-*level* extensions add or modify functions, operators, or types
beyond the base revision. None exist today: the model-level "EXPR"
extension gates whether the expression language is available *at all*, not
which features are registered once it is. The empty-but-`#[non_exhaustive]`
enum reserves the shape for the first real extension.

Code that must react to new extensions matches on `ExprExtension`
exhaustively *without a wildcard arm* (e.g. `build_library_skeleton`'s
`for ext in profile.extensions() { match *ext {} }` and
`extension_syntax_v2026_02`). Adding the first variant is therefore a
compile error at each site that must decide what the variant means —
the forcing-function idiom used throughout the workspace.

### `HostContext`

```rust
#[derive(Default)]
pub enum HostContext {
    #[default]
    None,                                    // no host functions registered
    Unresolved,                              // stub impls returning Unresolved(T)
    WithRules(Arc<Vec<PathMappingRule>>),    // real impls using these rules
}
```

Host-context functions (today: `apply_path_mapping`) need host-supplied
state the evaluator has no knowledge of. The three variants cover the
three call sites:

- **`None`** — plain expression evaluation; `apply_path_mapping` is an
  unknown function.
- **`Unresolved`** — template validation time: signatures must be known
  for type checking, but no rules exist yet. Stub implementations return
  `Unresolved(PATH)`.
- **`WithRules`** — runtime: real implementations capture the rules via
  `Arc`, so cloning libraries is cheap. `HostContext::with_rules(vec)` is
  the owning-constructor convenience.

Whether a given library instance has host functions is discoverable only
one way: whether they are registered — `lib.get_signatures("apply_path_mapping")`.
There is no separate flag.

### `SyntaxFeature` (crate-private)

An enum of every optional Python-syntax point the parser can accept or
reject (lambda, walrus, dict/set literals, f-strings, bitwise operators,
keyword arguments, multi-`for` comprehensions, …). Consulted only by the
parser's structural validator through `ExprProfile::allows_syntax`; it is
`pub(crate)` so new variants are not SemVer-visible. External callers
describe their language flavor with a revision + extension set, never
with `SyntaxFeature` directly.

For gated *operators* specifically (bitwise ops, shifts, `@`, `~`, `is`,
`is not`), the structural validator does not hardcode the feature/message
pairs: it reads them from the `OperatorTable` in `eval/op_table.rs`, the
same table the evaluator dispatches through, so the parse-time gate and
the eval-time reject list share one definition (see the evaluator spec's
"Operator dispatch table" section).

### `ExprProfile`

```rust
pub struct ExprProfile {
    revision: ExprRevision,
    extensions: HashSet<ExprExtension>,
    host_context: HostContext,
}
```

Constructors and builders:

| Method | Meaning |
|--------|---------|
| `ExprProfile::new(revision)` | Given revision, no extensions, no host context |
| `ExprProfile::current()` | `new(ExprRevision::CURRENT)` — the *stable baseline* |
| `ExprProfile::latest()` | Current revision + **every** extension in `ExprExtension::ALL` — intentionally **unstable across crate versions** |
| `.with_extensions(set)` | Replace the extension set |
| `.with_host_context(hc)` | Set the host context |

`ParsedExpression::new` and `FormatString::new` use `latest()` as a
quick-start default; `ParsedExpression::with_profile` /
`FormatString::with_profile` give callers parse behavior that is stable
across crate versions.

## Syntax Resolution: Two-Stage `allows_syntax`

`ExprProfile::allows_syntax(feature)` resolves in two stages:

1. **Revision baseline.** Each revision has a per-revision helper
   (`baseline_syntax_v2026_02`) deciding which features are in that
   revision's baseline. Under 2026-02 every `SyntaxFeature` is rejected,
   matching the Python reference implementation. The helper's match over
   `SyntaxFeature` is exhaustive, so a new feature variant cannot
   silently become allowed.
2. **Extension layer (additive).** Each revision also has a helper
   (`extension_syntax_v2026_02`) that iterates the enabled extensions and
   asks whether any grants the feature *under this revision*. Extensions
   can only add features, never remove baseline-allowed ones. The
   extension-layer dispatch matches on revision too, because an
   extension's meaning is revision-scoped.

## Library Selection and the Profile Cache

`FunctionLibrary::for_profile(&profile)` returns the library matching a
profile. Libraries are cached in a
`LazyLock<Mutex<HashMap<ProfileKey, Arc<FunctionLibrary>>>>` keyed on the
**rules-independent** portion of the profile:

```rust
pub(crate) struct ProfileKey {
    revision: ExprRevision,
    extensions: Vec<ExprExtension>,   // sorted for stable equality
    host_kind: HostKind,              // None | Unresolved | WithRules — no rules payload
}
```

`HostKind` records only *which variety* of `HostContext` is in use, not
the rules themselves. Consequences:

- `None` and `Unresolved` profiles hit the cache directly; repeated calls
  return the same `Arc` (codified by the
  `cache_returns_same_arc_for_*_profile` tests).
- `WithRules` profiles share one cached **no-host skeleton**; the
  rules-carrying `apply_path_mapping` closure is registered on a cheap
  clone per call, and the returned `Arc` is fresh each time
  (`with_rules_does_not_cache_rules_variant`). Sessions that rebuild a
  library per rule set pay near-zero registration cost and never thrash
  the cache.

`build_library_skeleton(profile)` is where the revision and extension
axes turn into registrations: an exhaustive `match profile.revision()`
plus an exhaustive `for ext in profile.extensions() { match *ext {} }`,
each a compile-error sentinel for the first new variant.

## Relationship to Other Documents

- [function-library.md](function-library.md) — dispatch and the
  host-context registration primitives that `for_profile` composes.
- [parser.md](parser.md) — the structural validator that consults
  `allows_syntax`.
- [public-api.md](public-api.md) — full signatures and stability
  classification of the profile types.
- `reports/expr-model-future-revision-readiness.md` — the evaluation
  report whose recommendations produced this design, including composite
  walkthroughs of how future revisions/extensions land.
