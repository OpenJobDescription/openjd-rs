# `#[non_exhaustive]` Policy

## The rule

Ask what a *new* variant or field means for a downstream consumer that has
not updated their code.

| | |
|---|---|
| **`#[non_exhaustive]`** | Decode-time input the consumer **reads**. New properties arrive by extension RFC and are gated by the decode-time allowlist, so ignoring an unknown one is correct behavior. |
| **Closed** | Anything the consumer must **react to**, and anything that cannot grow. Silently ignoring a new case would be a bug, so a compile error on upgrade is the point. |

Concretely: `template::*` is `#[non_exhaustive]`; `job::*`, the runtime
state machines, caller-built config, and decidable concepts stay closed.

## Why the allowlist decides it

Extensions are gated at decode time on a caller-supplied allowlist — see
`decode_extensions` in `crates/openjd-model/src/template/parse.rs`. A
consumer who never opts into an extension gets templates using it
**rejected at decode with a clear error**, and the new fields stay `None`
for them.

That is what makes `#[non_exhaustive]` safe on `template::*`: the
protection a compile error would give is already provided at runtime, by a
switch the consumer controls. The cost of leaving those types closed — a
SemVer-major bump per RFC — is real, and for a data-model crate it
partitions the ecosystem, since a host and a plugin on different major
versions cannot exchange a `Job` at all.

Where no allowlist stands in front of a change, that reasoning inverts.

## `template::*` vs `job::*`

The two families look alike; they are on opposite sides of the rule.

- **`template::*`** (`JobTemplate`, `HostRequirements`, the
  parameter-definition and `UserInterface` types) is the deserialize-time
  model. Serde builds it from YAML/JSON, new properties are
  allowlist-gated, and consumers read it. → **`#[non_exhaustive]`**

- **`job::*`** (`Job`, `Step`, `Action`, `EnvironmentActions`, …) is the
  instantiated model. Extensions are already resolved, so the allowlist no
  longer applies, and the `openjd-sessions` runtime **constructs and
  exhaustively matches** these to decide what to execute. A new field is
  execution behavior the runner must handle — the `WRAP_ACTIONS` hook
  fields on `EnvironmentActions` are the precedent — and a `_` arm that
  swallowed it would mean the action silently never runs. → **closed**

## Other closed categories

- **Runtime state machines** (`ActionState`, `SessionState`,
  `ScriptRunnerState`, `ActionMessage`, `CancelMethod`) — not
  allowlist-gated; consumers match to react. A `_ => {}` that swallows a
  new state is a correctness bug.
- **Caller-built configuration** (`SessionConfig`, the `openjd-snapshots`
  `*Options`) — exists to be assembled by callers. Marking it would force
  `Default` plus field mutation at every call site, since
  functional-record-update cannot construct a non-exhaustive struct from
  another crate.
- **Decidable concepts** — the variant set *is* the definition:
  `EndOfLine`, `ObjectType`, `DataFlow` (newline mode, entity kind, data
  direction); closed dichotomies like `PathElement` (Field | Index) and
  `DecodedTemplate` (Job | Environment), where matching every arm is the
  API; `DiagnosticSpan` (an offset and a length).
- **Single-field wrappers** (`FlexInt`, `Description`, `ExtensionName`) —
  one validated value behind a distinct type. Callers construct and
  destructure them directly, and a second field would make it not a
  newtype.
- **Zero-field markers** (`Abs`, `Rel`, `Full`, `Diff`) — nothing to add.

## Recording the decision

Both lints are `deny`, so an undecided public type is a hard error under a
plain `cargo clippy` — it fails while the type is being written, not later
under CI's `-D warnings`. Every public type carries either
`#[non_exhaustive]` or an `#[expect]` naming which rule applies:

```rust
#[expect(
    clippy::exhaustive_enums,
    reason = "runtime state machine: consumers match on these to react, and \
              a new state must be a compile error rather than a silently \
              ignored `_` arm. Not extension-gated."
)]
pub enum ActionState { /* … */ }
```

Prefer `#[expect]` over `#[allow]`: it is itself linted, so marking the
type `#[non_exhaustive]` later leaves an unfulfilled expectation that must
be removed. Use `#[allow]` only where the lint fires in some build
configurations but not others (`Identifier` is the one such case).

`openjd-cli` (a binary) and `openjd-for-js` (`publish = false` WASM
bindings) are exempt at the crate level — neither exposes a public Rust
API.

## Construction

`#[non_exhaustive]` blocks cross-crate *literal construction* and
*exhaustive destructuring*. It does **not** block field reads or `..`
patterns — but note it *does* block `..Default::default()`, which is why
caller-built config stays closed.

Types consumers legitimately build by hand get a constructor instead:
`PathMappingRule::new` and the `CallerLimits::with_*` builder exist for
that reason.
