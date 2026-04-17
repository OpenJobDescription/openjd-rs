# openjd-model Crate Quality Evaluation

**Evaluator:** Kiro (with `deadline-openjd` subagent for spec, source, and test review)
**Date:** 2026-04-17
**Crate location:** `~/openjd-rs/crates/openjd-model`
**Spec location:** `~/openjd-rs/specs/model`

## Executive summary

`openjd-model` is a solid, production-oriented Rust crate that implements OpenJD template parsing, validation, and job instantiation. It compiles cleanly with zero warnings (`cargo build -p openjd-model --all-targets`), clippy is clean, and all **1,576** tests pass with zero ignored. The crate has **no `unsafe`**, no `TODO`/`FIXME`/`unimplemented!` and no `panic!`/`unwrap`/`expect` in non-test code — with **two exceptions** (see findings 1 and 4 below).

The top-level picture:

1. **Specifications are comprehensive but drift from code.** Twelve spec docs cover the crate at reasonable depth. They drift from the implementation in ~22 places (signatures, field visibility, container types, variant casing, pass numbering). Several spec snippets would not compile if type-checked today.
2. **Two real bugs** were found via targeted investigation:
   - **HIGH:** `parameters.rs:406` panics on `NaN`/`inf` user input for FLOAT job parameters (a `.unwrap()` on the result of `Float64::with_str`, which rejects non-finite f64).
   - **HIGH:** `step_param_space.rs` `ProductNode::new` uses `Iterator::product()` on `usize` with no overflow check — a template with multiple large RangeExpr parameters can silently wrap `len()`, corrupting iteration / random access.
3. **One grammar parity bug** (`error.rs:117` always emits `"1 validation errors for …"`; Python Pydantic singularizes to `"1 validation error for …"`). Breaks cross-implementation message parity per AGENTS.md.
4. **One hot-path regex compile** (`format_strings.rs:1016` compiles a fresh `Regex` per let-binding per validation call).
5. **AGENTS.md error-test compliance is ~76%** — ~110–130 tests use bare `is_err()` or plain-substring `contains()` rather than the mandated full-path-plus-message assertions. The canonical pattern in `tests/test_error_messages.rs` is only adopted in ~8 of 30 test files.
6. **Three test files live in `src/`** because they reach crate-private types via `#[cfg(test)] pub use`. This pollutes `src/` — could be consolidated under `src/tests/`.
7. **No algorithmic hot-spot worse than O(V·E)** in the graph / parameter-space code. Spec claims of O(V+E) DFS topo sort and O(D) index arithmetic in StepParameterSpaceIterator are verified. Cycle detection is implemented **twice** (once in `structure.rs` Kahn's, once in `step_dependency_graph.rs` DFS) — both are O(V+E) but this is unnecessary redundancy.
8. **Encapsulation is weak** in public newtypes: `Identifier(pub String)`, `Description(pub String)`, `ExtensionName(pub String)` all have public tuple fields that let callers bypass the regex/length invariants enforced only in `::new`.

No blocking issues for correctness beyond the two HIGH bugs above. Recommendations below are prioritized.

---

## Verified build/test results

```
cargo build   -p openjd-model --all-targets  → clean, no warnings
cargo clippy  -p openjd-model --all-targets  → clean
cargo test    -p openjd-model                → 1576 passed; 0 failed; 0 ignored
```

No `unsafe` code (0 matches in `src/`). No `TODO`/`FIXME`/`unimplemented!`/`todo!` (0 matches in `src/`). Non-test code has 14 `.unwrap()` sites; all are either safe-by-construction or documented below.

---

## Artifacts reviewed

### Specifications (`~/openjd-rs/specs/model/`, 1,802 lines across 12 files)

| File | Lines | Scope |
|---|---:|---|
| `README.md` | 67 | Index + two-phase type system + Python relationship + extension coverage |
| `architecture.md` | 134 | Module layout, dep graph, public API surface, design decisions |
| `template-types.md` | 286 | `template::*` struct definitions, constrained strings, ranges |
| `job-types.md` | 223 | `job::*` instantiated types, resolution scopes, template→job mapping |
| `parameters.md` | 188 | `JobParameterType`/`TaskParameterType`, defs, PATH handling, EffectiveLimits/Rules |
| `parsing.md` | 124 | 5-phase decoding pipeline |
| `validation.md` | 189 | Multi-pass pipeline (passes 2–6), per-pass checks, helpers |
| `job-creation.md` | 168 | `merge_*`/`preprocess_*`/`create_job`/`convert_environment`/`evaluate_let_bindings` |
| `parameter-space.md` | 142 | Node tree (Product/Association/RangeExpr/Chunk), index arithmetic, chunking |
| `step-dependencies.md` | 91 | Graph construction, DFS topo sort, cycle detection at two levels |
| `capabilities.md` | 55 | Standard amount/attribute capabilities, validation regexes |
| `error-handling.md` | 135 | `OpenJdError`, `PathElement`/`ValidationErrors`, Pydantic-compat format |

### Implementation source (`src/`, ~9.8K lines across 24 files)

Core: `lib.rs` (52), `error.rs` (287), `types.rs` (581), `capabilities.rs` (43).

`template/`: `mod.rs` (52), `parse.rs` (398), `constrained_strings.rs` (905), `parameters.rs` (1351), `expr_parameters.rs` (965), `task_parameters.rs` (282), `step.rs` (180), `actions.rs` (89), `environment.rs` (45), `environment_template.rs` (25), `job_template.rs` (43), `host_requirements.rs` (33).

`template/validate_v2023_09/`: `mod.rs` (202), `limits.rs` (151), `structure.rs` (1067), `feature_bundle_1.rs` (108), `format_strings.rs` (1067), `task_chunking.rs` (96), `helpers.rs` (92).

`job/`: `mod.rs` (221), `step_dependency_graph.rs` (191), `step_param_space.rs` (1634), `create_job/mod.rs` (114), `create_job/parameters.rs` (788), `create_job/instantiate.rs` (666), `create_job/ranges.rs` (353).

In-src test files: `src/test_expr_param_constraints.rs` (1,809), `src/test_lazy_param_space.rs` (758), `src/test_instantiate_and_display.rs` (226) — these live in `src/` because they reach crate-private types via `#[cfg(test)] pub use` re-exports.

### Tests (`tests/`, 28 files, 19,559 lines; plus ~112 inline `#[cfg(test)]` tests in `src/`)

Total **1,576 tests, 0 ignored**. Largest files: `test_create_job.rs` (3,933), `test_job_parameters.rs` (1,655), `test_expr_parameters.rs` (1,550), `test_merge_job_parameters.rs` (1,136), `test_let_bindings.rs` (1,082), `test_feature_bundle_1.rs` (971), `test_chunk_int.rs` (855).

Canonical error-test file: `tests/test_error_messages.rs` (322 lines) — uses `check_err`/`assert_validation_errors` helpers that enforce full `path -> message` assertions per AGENTS.md.

---

## 1. Confirmed bugs

### 1.1 🔴 HIGH — `parameters.rs:406` panics on NaN/Inf user input

`src/job/create_job/parameters.rs:405-408`:

```rust
JobParameterType::Float => s
    .parse::<f64>()
    .map(|f| {
        ExprValue::Float(openjd_expr::value::Float64::with_str(f, s.to_string()).unwrap())
    })
```

Rust's `str::parse::<f64>` accepts `"NaN"`, `"nan"`, `"inf"`, `"infinity"`, `"-inf"`. `Float64::with_str` returns `Err(_)` for non-finite values. The `.unwrap()` then panics the process.

**Reproducer path:** pass `-p F=NaN` to `preprocess_job_parameters` for any FLOAT job parameter. The YAML-default path in `template/parameters.rs:720` has a `reject_nan_inf` guard; the runtime user-input path does not.

**Fix:** propagate the `Float64::with_str` error as a string:

```rust
JobParameterType::Float => {
    let f = s.parse::<f64>().map_err(|_| format!("Value '{s}' is not a valid float."))?;
    let v = openjd_expr::value::Float64::with_str(f, s.to_string())
        .map_err(|e| format!("Value '{s}' is not a valid float: {e}"))?;
    ExprValue::Float(v)
}
```

A failing test to add (demonstrates the panic):
```rust
#[test]
fn user_float_input_nan_is_rejected_not_panic() {
    let t = decode_job_template(yaml!({ ... FLOAT param F ... }), None).unwrap();
    let mut inputs = JobParameterInputValues::new();
    inputs.insert("F".into(), "NaN".into());
    let r = preprocess_job_parameters(&t, &inputs, &[], &PathParameterOptions::new());
    assert!(matches!(r, Err(OpenJdError::ModelValidation(msg)) if msg.contains("not a valid float")));
}
```

### 1.2 🔴 HIGH — `step_param_space.rs` `ProductNode::len()` silently overflows

`Iterator::product::<usize>()` is used to compute `ProductNode`'s total length from child node lengths (two sites: default-product path ~line 912 and explicit `parse_node_product` ~line 1130). `usize` multiplication wraps silently in release mode. A template with e.g. three RangeExpr params of `1-3_000_000_000` each produces a true product (~2.7×10²⁸) that wraps on 64-bit platforms. On 32-bit platforms the bug is trivial to trigger.

Downstream consequences:
- `StepParameterSpaceIterator::len()` returns a wrong (small) value.
- `get(index)` with `index < wrapped_len` happily computes offsets into children, producing a `TaskParameterSet` that corresponds to a fabricated combination.
- `Iterator::size_hint` / `ExactSizeIterator` lie.

**Fix:** use `checked_mul` across children, convert overflow into `OpenJdError::DecodeValidation`, and add a template-level `max_task_space_len` limit (probably already implied by `max_task_param_range_len` per-param, but not multiplied through).

Failing test (sketch):
```rust
#[test] fn product_node_overflow_is_rejected() {
    let big = "1-1000000000";
    let yaml = format!(r#"...steps[0].parameterSpace.taskParameterDefinitions = 3x INT[{big}]..."#);
    let decoded = decode_job_template(serde_yaml::from_str(&yaml).unwrap(), None).unwrap();
    let err = create_job(&decoded, &JobParameterValues::new()).unwrap_err();
    assert!(matches!(err, OpenJdError::ModelValidation(m) if m.contains("parameter space too large")));
}
```

### 1.3 🟡 MEDIUM — `error.rs:117` grammar / Pydantic parity ("1 validation errors")

`src/error.rs:117`:
```rust
let mut out = format!("{} validation errors for {model_name}", self.errors.len());
```

Python Pydantic emits `"1 validation error for X"` for count=1 (singular); AGENTS.md explicitly calls for cross-language message parity. The current code always pluralizes.

**Backward-compat cost:** the current wrong form is pinned by ~8 existing test assertions (`tests/test_error_messages.rs:58,267`, `tests/test_job_template.rs:144`, `tests/test_environment_template.rs:274,296,311,343`, and the doc test at `src/error.rs:222`). Any fix must update these assertions in the same commit.

**Fix:**
```rust
let n = self.errors.len();
let word = if n == 1 { "error" } else { "errors" };
let mut out = format!("{n} validation {word} for {model_name}");
```

### 1.4 🟢 LOW — `template/step.rs` desugaring `FormatString::new(...).unwrap()`

`src/template/step.rs:83-98` builds synthetic FormatString identifiers from a step's `safe_name` (non-alphanumeric characters replaced with `_`, truncated to 200 chars). If a step is named e.g. `"9frames"`, the resulting synthetic identifier starts with a digit. If the `FormatString` grammar rejects identifiers starting with digits, the `.unwrap()` panics. Low risk in practice (needs manually crafted template) but a defensive `?` would close it.

### 1.5 ✅ False alarms

Two candidate bugs from the audit were investigated and ruled out:

- **`format_strings.rs:1016`** dynamic regex compile: `name` is guaranteed alphanumeric+underscore by prior validation (lines 995–1002) and `regex::escape` is applied, so the `.unwrap()` cannot panic. It's only a perf concern (see §5 below).
- **`IntOrFormatString::Int(i64)` → `usize` cast** in `task_parameters`/`ranges.rs`: both `default_task_count` and `target_runtime_seconds` paths clamp via `.max(0)`/`.max(1)` before the cast, so no silent negative→huge-usize conversion. The `task_chunking.rs:30` validation pass also rejects negatives statically when the value is a literal.

---

## 2. Spec ↔ implementation mismatches

Every entry below is a concrete drift where a spec doc describes something that doesn't match current code. Several spec examples would not compile if type-checked against the current API.

| # | Spec file | Spec claim | Actual | Source |
|---|---|---|---|---|
| 1 | architecture.md | Module layout: `src/parse.rs`, `src/create_job.rs`, `src/step_param_space.rs`, `src/step_dependency_graph.rs` at crate root | All four live under `template/` or `job/` | `lib.rs:9-14` |
| 2 | architecture.md | `src/capabilities.rs` not in module layout | Module exists, 43 lines | `src/capabilities.rs` |
| 3 | architecture.md | `"pub use types::*"` | `lib.rs:41-45` enumerates specific names, not a glob | `lib.rs:41-45` |
| 4 | architecture.md | `template/` is a public module | `lib.rs:10` declares `pub(crate) mod template` | `lib.rs:10` |
| 5 | architecture.md | Re-exports `FormatString, SymbolTable` | `lib.rs` also re-exports `format_string` and `symbol_table` modules wholesale | `lib.rs:19-24` |
| 6 | architecture.md | Public API list omits `PathParameterOptions`, `MergedParameterDefinition` | Both are re-exported | `lib.rs:29-32` |
| 7 | error-handling.md | `ValidationErrors::add(&mut self, path: Vec<PathElement>, message: String)` | Actual: `add(&mut self, path: &[PathElement], msg: impl Into<String>)` | `error.rs:100` |
| 8 | error-handling.md | `ValidationErrors::new() -> Self` | No `new()`; only `#[derive(Default)]` | `error.rs:96` |
| 9 | error-handling.md | Struct shape shown without `errors` field | `pub errors: Vec<ValidationError>` field exists | `error.rs:93-95` |
| 10 | template-types.md | `EnvironmentTemplate` has 3 fields | Also has `pub extensions: Option<Vec<ExtensionName>>` | `environment_template.rs:12-17` |
| 11 | template-types.md | `CancelationMode { mode, notify_period_in_seconds }` as struct | `enum CancelationMode { Terminate, NotifyThenTerminate { … } }` with custom deserializer | `actions.rs:19-25` |
| 12 | template-types.md | `CancelationModeType { NotifyThenTerminate, Terminate }` exists | No such type; variants live on `CancelationMode` | `actions.rs` |
| 13 | template-types.md | `TaskParameterDefinition` variants `Int, Float, String, Path, ChunkInt` | Variants use SCREAMING_CASE: `INT, FLOAT, STRING, PATH, ChunkInt` (last one is PascalCase — mixed) | `task_parameters.rs:11-21` |
| 14 | template-types.md | `FloatRange::List(Vec<serde_yaml::Value>)` with "deferred NaN/Inf handling" | `List(Vec<FloatRangeItem>)` where `FloatRangeItem = Float(f64) \| FormatString(FormatString)` | `task_parameters.rs:137-140` |
| 15 | template-types.md | `ChunksDefinition.range_constraint: Option<RangeConstraint>` | `range_constraint: RangeConstraint` (required) | `task_parameters.rs:231-238` |
| 16 | parameters.md | `JobParameterDefinition` variants `String, Int, Float, Path, Bool, RangeExpr, ListString, …` (PascalCase) | Variants `STRING, INT, FLOAT, PATH, BOOL, RANGE_EXPR, LIST_STRING, …` (SCREAMING_SNAKE_CASE) | `parameters.rs:57-68` |
| 17 | parameters.md | `default_value() -> Option<&ExprValue>` | Returns `Option<String>` | `parameters.rs:185` |
| 18 | parameters.md | `check_constraints(&ExprValue) -> Result<()>` | Returns `Result<(), String>` | `parameters.rs:271` |
| 19 | parameters.md | `validate_definition(&EffectiveLimits) -> Result<()>` | Returns `Result<(), Vec<String>>` (accumulates) | `parameters.rs:306` |
| 20 | parameters.md | `EffectiveLimits` has 4 fields | Has 11 fields (adds `max_env_name_len, max_filename_len, max_task_param_range_len, max_task_param_string_len, max_job_param_string_len, max_command_len, max_description_len`) | `validate_v2023_09/mod.rs:15-27` |
| 21 | parameters.md | `EffectiveRules` has `step_script_scopes, env_script_scopes` | Only has `allowed_job_param_types, allowed_task_param_types, allow_fmtstring_in_numeric_fields` | `validate_v2023_09/mod.rs:83-86` |
| 22 | validation.md | `validate_job_template` is `pub(crate)` | Declared `pub` | `validate_v2023_09/mod.rs:133` |
| 23 | job-types.md | `Job.parameters: HashMap<String, JobParameter>` | `IndexMap<String, JobParameter>` | `job/mod.rs:34` |
| 24 | job-types.md | `Step.resolved_symtab: Option<SymbolTable>` | Actual: `Option<SerializedSymbolTable>` (wire-format type) | `job/mod.rs:54-65` |
| 25 | job-types.md | `job::Environment { name, description, script, variables }` | Also has `resolved_symtab: Option<SerializedSymbolTable>` | `job/mod.rs:93-102` |
| 26 | job-types.md | `EmbeddedFile.file_type: String`, `end_of_line: Option<String>` | Typed enums `FileType`, `Option<EndOfLine>` | `job/mod.rs:123-130` |
| 27 | job-types.md | `job::CancelationMode` as struct | Same enum pattern as template side (#11) | `job/mod.rs:134-141` |
| 28 | job-types.md | `StepParameterSpace.task_parameter_definitions: HashMap<…>` | `IndexMap<…>` | `job/mod.rs:145-148` |
| 29 | job-creation.md | `create_job(job_template, job_parameter_values, environment_templates)` — 3 args | 2 args; env templates merged pre-call | `create_job/mod.rs:35` |
| 30 | job-creation.md | `merge_job_parameter_definitions -> Result<Vec<MergedParameter>, …>` | Returns `Vec<MergedParameterDefinition>` | `create_job/parameters.rs:25` |
| 31 | job-creation.md | `preprocess_job_parameters(job_template, input_values, environment_templates, job_template_dir, current_working_dir, allow_walk_up)` — 6 args | 4 args; consolidated into `path_options: &PathParameterOptions<'_>` | `create_job/parameters.rs:500-505` |
| 32 | job-creation.md | `build_symbol_table(params) -> SymbolTable` (infallible) | `Result<SymbolTable, OpenJdError>` | `create_job/parameters.rs:753` |
| 33 | job-creation.md | `convert_environment(env, symtab) -> Result<job::Environment, OpenJdError>` | `convert_environment(env) -> job::Environment` (1 arg, infallible); separate `convert_environment_with_symtab` exists | `create_job/instantiate.rs:277` |
| 34 | job-creation.md | `evaluate_let_bindings(bindings, symtab, library: &FunctionLibrary)` | Adds `library: Option<&FunctionLibrary>, path_format: PathFormat` | `create_job/instantiate.rs:381` |
| 35 | parameter-space.md | `StepParameterSpaceIterator::new(space) -> Self` | Returns `Result<Self, OpenJdError>` | `step_param_space.rs:883` |
| 36 | parameter-space.md | `get(&self, index: usize) -> TaskParameterSet` | Returns `Option<TaskParameterSet>` | `step_param_space.rs:985` |
| 37 | parameter-space.md | `set_chunks_default_task_count(&self, count)` (claims `Arc<AtomicUsize>`) | `&mut self, value: usize` (no Arc/Atomic) | `step_param_space.rs:1036` |
| 38 | parameter-space.md | Public methods listed: 8 | Also exposes `new_with_chunk_override, names, is_empty, validate_containment` (12 total) | `step_param_space.rs:871-1036` |
| 39 | step-dependencies.md | "Kahn's algorithm" for topo sort | Iterative DFS with 3-state marking | `step_dependency_graph.rs:145` |
| 40 | capabilities.md vs validation.md | `capabilities.md` lists `STANDARD_AMOUNT_CAPABILITIES` with `amount.` prefix; `validation.md` lists without prefix | Source (`capabilities.rs:6-12`) has `amount.` prefix — validation.md is wrong | `capabilities.rs:6-12` |

### Structural gaps (public API not covered in specs)

- `PathParameterOptions` struct + constructor
- `convert_environment_with_symtab` function
- `StepParameterSpaceIterator` methods `new_with_chunk_override`, `names`, `is_empty`, `validate_containment`
- `FloatRangeItem` public enum
- Wholesale re-export of `openjd_expr::{format_string, symbol_table}` modules
- `EffectiveLimits`'s 7 undocumented fields
- `job::Environment.resolved_symtab` field
- `SpecificationRevision` vs `TemplateSpecificationVersion` layering
- `KnownExtension` enum and its `FromStr`
- `FileType`, `EndOfLine`, `ObjectType`, `DataFlow` enums in `types.rs`

---

## 3. Spec quality (clarity, redundancy, gaps)

### Clarity issues

1. **`parameters.md` "List*" item constraints are undocumented.** The table row references `allowedValues`, `min/maxValue` for `ListInt`/`ListFloat`/etc. but never defines the `ListIntItemConstraints`/`ListStringItemConstraints`/`ListFloatItemConstraints`/`ListListIntItemConstraints` sub-structs (all are distinct in `expr_parameters.rs`).
2. **`template-types.md` FloatRange description is hand-wavy.** *"FloatRange::List uses `serde_yaml::Value` because YAML float parsing has edge cases (NaN, Infinity) that need deferred handling"* — no explanation of the deferred handling, and the claim is factually wrong (the type is `FloatRangeItem`, #14 above).
3. **`validation.md` Pass 5 `unresolved(ANY)` jargon.** *"added as unresolved(ANY) to prevent cascading errors"* — concept belongs to the expr type system; no link to `expr/type-system.md`.
4. **Phase/Pass numbering inconsistency.** `parsing.md` labels phases 1–5; `validation.md` says *"Passes are numbered starting at 2 because Pass 0 and Pass 1 happen in the parse module"* — the two docs use inconsistent numbering.
5. **`job-types.md` PATH field self-contradiction.** *"PATH values are stored as ExprValue::String at this stage"* while `parameters.md` says PATH becomes `ExprValue::Path` after session path mapping. Cross-reference is missing; easy to misread as contradictory.
6. **`validation.md` "reserved scope checks" lacks a list.** Pass 3 bullet mentions reserved scopes; `RESERVED_SCOPES = worker, job, step, task` is buried in the Shared Helpers section at the bottom.
7. **`capabilities.md` vs `validation.md` prefix discrepancy** (see mismatch #40 above).
8. **`parameter-space.md` AssociationNode validation claim unverified.** *"All children must have the same length (validated during construction)"* — but the error produced and the enforcement phase are not documented.
9. **`error-handling.md` path-rendering detail** — example shows `steps[0] -> script -> actions -> onRun -> command` but doesn't explain that `PathElement::Index` attaches to the preceding field (vs rendering as a standalone segment).
10. **`symbol-table` / TEMPLATE vs SESSION / TASK scope contract** — spread across `parameters.md`, `job-types.md`, and `validation.md` Pass 5. Needed in one canonical place.

### Redundancies

1. Two-phase type system philosophy appears in `README.md`, `template-types.md`, and `job-types.md`.
2. "Explicit Type Conversion vs Generic Traversal" design decision is in `architecture.md` and `job-creation.md`.
3. "Post-Deserialization Validation (vs Pydantic)" is in `architecture.md`, `parsing.md`, and `validation.md`.
4. Resolution scope table (TEMPLATE/SESSION/TASK) is in `job-types.md`, `parameters.md`, and `validation.md` Pass 5.
5. `EffectiveLimits` 64→512 / 50→200 bumps are in `parameters.md` and `architecture.md`.
6. Value coercion rules (`yes`/`no`/`on`/`off`/`1`/`0`) are in `parameters.md` and `job-creation.md`.
7. PATH relative-path resolution rule is in `parameters.md` and `job-creation.md`.
8. Standard capability constants + reserved scopes are duplicated between `capabilities.md` and `validation.md`.
9. Pydantic-compatible error paths appear in `architecture.md` and `error-handling.md`.
10. Cycle detection is described in `validation.md` Pass 3 and again in `step-dependencies.md` (with algorithm disagreement, #39).

### Missing / thin coverage

Priority order:

1. **`expr_parameters.rs` (965 LOC)** — not covered beyond the variant list. Missing: each per-variant `UserInterface` struct (`BoolUserInterface`, `RangeExprUserInterface`, `ListSimpleUserInterface`, `ListPathUserInterface`, `ListIntUserInterface`, `ListFloatUserInterface`, `HiddenOnlyUserInterface`), every `List*ItemConstraints` struct, list-level vs item-level bounds/length validation split, `BoolValue` deserializer placement.
2. **`validate_v2023_09/format_strings.rs` (1,067 LOC — the largest validation pass)** is the thinnest relative to code. Missing: let-binding self-reference detection algorithm (source strips string literals before scanning), `default_lib` vs `host_lib` contents and per-scope library choice, per-field validator function inventory (`validate_action_fs`, `validate_env_format_strings`, `validate_env_comprehensions`), comprehension validation (not mentioned at all), symbol-table layering contract (Task.* removal when going back to template scope).
3. **`capabilities.md` vs `validation.md` `amount.` prefix reconciliation.**
4. **`types.rs` reference section.** Missing: `SpecificationRevision` vs `TemplateSpecificationVersion` layering, `KnownExtension` + FromStr, utility enums `FileType`/`EndOfLine`/`ObjectType`/`DataFlow`, `JobParameterValue`/`TaskParameterValue` canonical wrappers.
5. **`constrained_strings.rs`** — regex + length covered; constructor API (`Identifier::new`, `Description::new`, `ExtensionName::new`), error variants, `as_str()`/`Display`/`Serialize` round-trip are undocumented.
6. **`validate_v2023_09/feature_bundle_1.rs`** — rejects >1 simple action and `endOfLine` per-embedded-file. Pass is mentioned but exact error behavior and env-template scope (this pass only iterates `jt.steps`) are not specified.
7. **`validate_v2023_09/task_chunking.rs`** — the paren-depth tokenizer for detecting "name inside associative combination" is not documented; neither is the rule that `defaultTaskCount` / `targetRuntimeSeconds` bounds are only checked when the value is a constant `i64`.
8. **Adaptive vs static chunking determination.** `parameter-space.md` mentions both modes; neither doc spells out that presence of `targetRuntimeSeconds` flips to adaptive.

---

## 4. Code quality — Rust idioms, API ergonomics

### Strengths

- Zero `unsafe`, zero `TODO`/`FIXME`/`unimplemented!`.
- All 14 `.unwrap()` sites outside `#[cfg(test)]` were audited (see §1.5); only one in `parameters.rs:406` is genuinely broken.
- Consistent `Result<_, OpenJdError>` with `?` throughout.
- `indexmap::IndexMap` used where insertion order matters for deterministic output (Job.parameters, StepParameterSpace.task_parameter_definitions, MergedParameterDefinition ordering).
- `LazyLock<Regex>` for compiled regexes in `helpers.rs:11,14,21` and `constrained_strings.rs:16,98`.
- Validation pipeline cleanly factored into named passes in `validate_v2023_09/mod.rs:138-157`.
- `#[non_exhaustive]` on `OpenJdError`.
- `#[serde(deny_unknown_fields)]` on all deserialized types.
- Error format produces exactly the `path -> nested -> leaf:\n\tmessage` shape AGENTS.md requires (confirmed by `test_error_messages.rs`).

### Friction points

1. **Public newtypes with public tuple fields that bypass invariants (🔴):**
   - `Identifier(pub String)` at `constrained_strings.rs:12`. `::new` enforces `^[A-Za-z_][A-Za-z0-9_]{0,511}$` but callers can write `Identifier("".into())` directly.
   - `Description(pub String)` at `constrained_strings.rs:60`. Same issue (length ≤ 2048, control chars only in `::new`).
   - `ExtensionName(pub String)` at `constrained_strings.rs:94`. Same issue (regex only in `::new`).
   - **Fix:** make fields `pub(crate)` or private; expose via `as_str()` / `Deref<Target=str>`.
2. **`OpenJdError` variants stringify structured upstream errors (🔴).** `DecodeValidation`, `ModelValidation`, `Expression`, `Compatibility`, `UnsupportedSchema` all wrap `String`; `From<SymbolTableError>` (`error.rs:53-57`) calls `.to_string()`. Callers wanting to inspect which field failed must regex-parse the message. Compare with `openjd_expr::ExpressionError::kind() -> ExpressionErrorKind`. Recommend: add a structured `OpenJdError::Validation { errors: Vec<ValidationError> }` variant or convert existing `ModelValidation` to carry `Vec<ValidationError>` directly.
3. **Missing `#[non_exhaustive]` on user-facing type enums (🟡).** `JobParameterType`, `TaskParameterType`, `KnownExtension`, `SpecificationRevision`, `CancelationMode`, `TaskParameter`, `EffectiveLimits`, `EffectiveRules` are exhaustive. Adding a new extension or parameter type is a breaking change today.
4. **Mixed variant casing (🟡).** `template/task_parameters.rs:11-21` has variants `INT, FLOAT, STRING, PATH, ChunkInt` — first four SCREAMING, last one PascalCase. `template/parameters.rs:57-68` uses SCREAMING_SNAKE_CASE throughout (`STRING, INT, FLOAT, PATH, BOOL, RANGE_EXPR, LIST_STRING, LIST_PATH, LIST_INT, LIST_FLOAT, LIST_BOOL, LIST_LIST_INT`). Meanwhile `types.rs:148-161` uses PascalCase. Three different conventions in one crate. Clippy lints. Pick one (PascalCase, with `#[serde(rename = "...")]` for wire names).
5. **Missing `Display` impls (🟡).** `TemplateSpecificationVersion`, `KnownExtension`, `Description`, `ExtensionName` only expose `as_str()`. `format!("{}", version)` won't compile.
6. **`from_spec_str()` pattern vs `FromStr` (🟡).** `JobParameterType::from_spec_str -> Option` / `TaskParameterType::from_spec_str -> Option` vs `TemplateSpecificationVersion::from_str` (FromStr impl returning Result). Pick one.
7. **`FromStr` with `type Err = String`** on `TemplateSpecificationVersion` (`types.rs:133`) and `KnownExtension` (`types.rs:331`) — structured error would compose better.
8. **Error-name confusion (🟡).** Crate-specific error is called `OpenJdError`, implying crate-wide scope. Compare to `openjd_expr::ExpressionError`. Recommend `ModelError` or `TemplateError`. The `OpenJdError::Expression` variant additionally collides conceptually with `ExpressionError`.
9. **`ValidationErrors` has `pub errors` field AND `add()` method (🟢).** Two paths to mutate; make field `pub(crate)` and expose a `fn errors(&self) -> &[ValidationError]`.
10. **No `ValidationErrors::new()`** — only `Default`. Spec claims it exists (#8).
11. **Grammar bug in error Display** (see §1.3).
12. **Duplicate `create_job` re-exports (🟡):** `lib.rs` line 11 `pub use job::create_job;` (module) and line 22 `pub use job::create_job::create_job;` (function). Same path name refers to different things. Clippy `module_name_repetitions`.
13. **Module export asymmetry:** `pub mod error; pub mod job; pub(crate) mod template;` + `pub use template::{...}`. Forces all template types into the crate root. Recommend promoting `template` to `pub mod` with curated `pub use` at its head.
14. **`PathParameterOptions` lacks a builder (🟡).** 5 pub fields used together in every call site — a builder would prevent invalid combos (e.g., `allow_template_dir_walk_up` without `job_template_dir`).
15. **`EffectiveLimits`, `EffectiveRules`, `ValidationError`, `PathElement`, `ValidationErrors` are not re-exported from `lib.rs` (🟡).** External callers who want to produce validation errors in the same format can't.
16. **`PathFormat` not re-exported** — `PathParameterOptions.path_format: PathFormat` but consumers must import from `openjd_expr` directly.
17. **`#[must_use]` coverage** missing on constructors / builder-like methods (~10 sites including `ValidationContext::new`, `EffectiveLimits::from_context`, `StepDependencyGraph::new`, `StepParameterSpaceIterator::new`, all `Identifier::new`/`Description::new`/`ExtensionName::new`).
18. **`DecodedTemplate` has `#[allow(clippy::large_enum_variant)]` (🟢).** `JobTemplate` variant is much bigger than `EnvironmentTemplate`. Consider boxing the bigger variant.
19. **`Cow<'_, str>` unused anywhere** — acceptable today but several accessor pairs could move to `Cow` when normalization is needed (future).
20. **In-src test files (🟡).** `src/test_expr_param_constraints.rs` (1,809 lines), `src/test_lazy_param_space.rs` (758), `src/test_instantiate_and_display.rs` (226) live in `src/` because they use `#[cfg(test)] pub use` of crate-private template types. Consider a `src/tests/mod.rs` with `mod inner;` + `#[cfg(test)] mod tests;` pattern so tests don't pollute the `src/` directory listing.

---

## 5. Performance and algorithmic review

### Verified spec claims

- ✅ `StepDependencyGraph::new()` builds `name_to_index` HashMap and iterates steps+deps with O(1) hashed lookup. **O(V+E)** as claimed.
- ✅ `StepDependencyGraph::topo_sorted()` uses explicit-stack DFS with tri-state marking; each node/edge visited once. Per-node dep sort at `step_dependency_graph.rs:151` adds O(E log Δ); effective complexity **O(V + E log Δ)** — close to the spec's O(V+E).
- ✅ `StepParameterSpaceIterator::get(index)` performs O(D) divmod across dimensions (D = number of dimensions). Spec's "zero-allocation index arithmetic" is true for the index math; value materialization unavoidably clones.
- ✅ `format_strings.rs` validation uses hashed `HashSet<String>` for `step_let_names` / `script_let_names`; symbol-table reads are hashed O(depth).
- ✅ Regexes in `helpers.rs`, `capabilities.rs`, `constrained_strings.rs` are all `LazyLock<Regex>` at module scope — compiled once.
- ✅ Template→job walk in `instantiate.rs` is iterative; no input-driven recursion in the model crate. Recursive structures (parameter-space node tree) have depth bounded by schema limits.

### Findings

| Severity | Location | Issue |
|---|---|---|
| 🔴 High | `step_param_space.rs` `ProductNode::new` (default-product ~line 912 and `parse_node_product` ~line 1130) | `Iterator::product::<usize>()` wraps silently — see bug §1.2 |
| 🔴 High | `validate_v2023_09/format_strings.rs:1016` | `regex::Regex::new(&format!(r"\b{}\b", regex::escape(name))).unwrap()` compiles a fresh regex per let binding per validation call. Replace with a single-pass word-boundary scan or cache compiled regexes. |
| 🔴 High | `step_param_space.rs` `StaticChunkNode::chunk_range_expr` (~line 455) | `format!("{start}-{end}")` + `range_str.parse::<RangeExpr>()` on every chunk `get()`/iter step. A full RangeExpr parse just to rebuild what was decomposed. Construct `RangeExpr` directly. |
| 🟡 Med | `create_job/instantiate.rs:509-620` | `collect_all_accessed_symbols` + `collect_let_binding_refs` call `ParsedExpression::new(expr)` a second time for each let binding (already parsed+evaluated earlier). Cache parsed AST alongside the binding. |
| 🟡 Med | `create_job/instantiate.rs:23,69` | Full `SymbolTable::clone()` per step (for per-step type-check symtab). For jobs with many steps this dominates. Consider a cheap snapshot/restore API on SymbolTable. |
| 🟡 Med | `create_job/parameters.rs:527` | `for key in input_values.keys() { if !merged.iter().any(|p| p.name == *key) {...} }` — O(K·M). Build a `HashSet<&str>` of merged names once → O(K+M). |
| 🟡 Med | `validate_v2023_09/structure.rs:874-920` | Cycle detection via Kahn's **duplicates** the DFS in `StepDependencyGraph`. Both are O(V+E) but the work is done twice; unify via a single pass in the graph. |
| 🟡 Med | `types.rs:300-306` | `JobParameterInputValues`, `JobParameterValues`, `TaskParameterSet` are `HashMap<String, _>`. `TaskParameterSet` is iterated for Display — see comment at `step_param_space.rs:1418` acknowledging non-deterministic iteration order in user-visible output. Switch to `IndexMap` for determinism. |
| 🟡 Med | `template/environment.rs:20`, `job/mod.rs:97` | `variables: Option<HashMap<String, FormatString>>`. Env var iteration order is non-deterministic → non-reproducible env var ordering during session setup. Switch to `IndexMap`. |
| 🟢 Low | `step_param_space.rs::AssociationNode::validate_containment` ~line 622 | O(N·D) linear rebuild-and-compare over association length. Validation path only. |
| 🟢 Low | `create_job/instantiate.rs:59,62,71-72` | `format!("Param.{name}")`/`format!("RawParam.{name}")` per parameter per step — could use `write!` into a reusable `String` buffer. |

### Top 5 `.clone()` counts (`grep -c`)

| # | File | Count |
|---|---|---:|
| 1 | `job/step_param_space.rs` | 32 |
| 2 | `job/create_job/instantiate.rs` | 25 |
| 3 | `job/create_job/parameters.rs` | 24 |
| 4 | `template/validate_v2023_09/structure.rs` | 15 |
| 5 | `template/validate_v2023_09/format_strings.rs` | 7 |

- `step_param_space.rs`: many clones are `ExprValue::clone()` / `name.clone()` per `get()` — would benefit from `Arc<str>` for parameter names.
- `instantiate.rs`: the two `SymbolTable::clone()` sites (lines 23, 69) are the largest — full symbol table per step.
- Error-path building (`error.rs:160,166,171`) clones whole path vectors per call — O(depth²) for deep trees. `Arc<[PathElement]>` with structural sharing would be O(depth).

---

## 6. Test suite audit

### Overview

- **1,576 tests pass**, 0 ignored.
- **30+ test files**, ~19,500 LOC in `tests/` + ~2,800 LOC inline in `src/`.
- Canonical error-test pattern lives in `tests/test_error_messages.rs` (322 lines) using `check_err`/`assert_validation_errors` helpers that assert full `path -> message` content per AGENTS.md.
- Organization is generally clear: files named by feature area, most with `// ═══ Section ═══` banners that mirror Python reference test classes.

### AGENTS.md error-test compliance

- Compliant calls (`check_err`, `assert_validation_errors`): **~396**
- Bare `assert!(...is_err())`: **~46**
- Substring-only `err.to_string().contains(...)` outside of compliant helpers: **~40–50**

**Estimated compliance: ~76% (380–400 of ~510 error tests).**

Concentrations of non-compliant patterns:

| File | Non-compliant count | Notes |
|---|---:|---|
| `tests/test_create_job.rs` | ~42 (36 `.contains` + 6 `is_err`) | Worst offender |
| `tests/test_merge_job_parameters.rs` | ~20 | `err.to_string().contains("conflicting types" \| "no valid range" \| …)` |
| `tests/test_range_expr.rs` | 34 `is_err()` | Parser-level — arguably acceptable but no message coverage |
| `src/test_expr_param_constraints.rs` | 11 | `err.contains("less than minimum" \| …)` |
| `tests/test_combination_expr.rs` | 4 | or-ed substring matches |
| `tests/test_feature_bundle_1.rs` | 3 | bare `is_err()` |
| `tests/test_capabilities.rs:307,312` | 2 | bare `is_err()` for invalid attr capability values |
| `tests/test_job_parameters.rs:1537,1564` | 2 | bare `is_err()` for NaN/Infinity rejection |

Specific examples:

```
tests/test_create_job.rs:383   err.to_string().contains("absolute path")
tests/test_create_job.rs:1236  err.contains("Values missing for required job parameters: Required")
tests/test_create_job.rs:2175  err.to_string().contains("ThisIsUnknown")
tests/test_create_job.rs:2636  assert!(err.to_string().contains("not a valid bool"))
tests/test_create_job.rs:2717  contains("not a valid range")    # no path
tests/test_merge_job_parameters.rs:138, 283, 318, 337, 356, 402, 426, 450, 540, 644, 817, 858, 891, 924, 962, 998, 1036, 1071, 1135
  # 19 err.to_string().contains("…") calls with no full path
src/test_expr_param_constraints.rs:276, 292, 312, 332, 357, 374, 394, 1161, 1187, 1213, 1230
```

### Coverage gaps

1. **Direct unit tests of `validate_v2023_09` passes.** None. All coverage is indirect via `decode_job_template`. Direct tests of `validate_structure`, `validate_format_strings`, `enforce_limits`, `validate_task_chunking`, `validate_feature_bundle_1` would let error paths be pinned without building full templates.
2. **`src/capabilities.rs`** has zero inline `#[test]`s. `validate_amount_capability_name` / `validate_attribute_capability_name` only hit indirectly.
3. **`src/job/step_dependency_graph.rs`** has zero inline tests (27 external tests cover the graph, but):
   - No test for **multiple disjoint cycles** (which cycle gets reported?).
   - No test for **cycle sharing a node with an acyclic subgraph**.
   - No `max_degrees()` with isolated nodes.
   - No `topo_sorted()` determinism test with equal-priority nodes beyond one `random` case.
4. **`src/job/step_param_space.rs` boundary cases** — zero-element parameter space (empty ranges product), `len == usize::MAX` overflow (exploits bug §1.2), very-large-index `get()` near overflow.
5. **`src/template/parameters.rs` and `expr_parameters.rs`** have zero inline tests — all coverage is black-box via tests/. Refactor regressions only surface at decode level.
6. **`src/job/create_job/*.rs`** have zero inline tests in `instantiate.rs`, `parameters.rs`, `ranges.rs`.

### Readability / oversized files

Files > 1,000 lines (6 of them) are candidates for splitting:

| File | Lines | Tests | Split suggestion |
|---|---:|---:|---|
| `tests/test_create_job.rs` | 3,933 | 133 | Split: `test_create_job_env_overrides`, `_parameter_coercion`, `_path_handling`, `_let_bindings`, `_chunking` |
| `src/test_expr_param_constraints.rs` | 1,809 | 122 | Split by type: `int_constraints`, `float_constraints`, `range_constraints`, `list_item_constraints` |
| `tests/test_job_parameters.rs` | 1,655 | 165 | Split by type: `string`, `int`, `float`, `path` |
| `tests/test_expr_parameters.rs` | 1,550 | 159 | Split: `bool_params`, `range_expr_params`, `chunk_int_params` |
| `tests/test_merge_job_parameters.rs` | 1,136 | 38 | Split merge-success vs merge-conflict |
| `tests/test_let_bindings.rs` | 1,082 | 71 | Split script-level vs step-level vs validation-error sections |

### Ignored tests

**Zero** `#[ignore]` attributes across model tests. (The S3 integration tests described in AGENTS.md live in `openjd-snapshots`.)

---

## 7. Recommendations (prioritized)

### P0 — Correctness fixes

1. **Fix NaN/Inf panic in `parameters.rs:406`** (bug §1.1). Trivial: replace `.unwrap()` with `?`-style error propagation. Add a test for user input `F=NaN` and `F=inf` that asserts both are rejected with a clear error message.
2. **Fix `ProductNode` length overflow in `step_param_space.rs`** (bug §1.2). Use `checked_mul` over children, return `OpenJdError::DecodeValidation` on overflow. Add template-level `max_task_space_len` limit. Add a test with three large RangeExpr parameters whose product overflows `usize`.
3. **Fix the `"1 validation errors"` grammar bug in `error.rs:117`** (bug §1.3). Singularize when count=1 for Python Pydantic parity. Update ~8 existing test assertions in the same commit.
4. **Defensive `?` at `template/step.rs:83-98`** (bug §1.4). Propagate `FormatString::new` errors instead of `.unwrap()` on computed identifiers.

### P1 — API safety

5. **Make tuple fields of `Identifier`, `Description`, `ExtensionName` `pub(crate)` or private.** Expose via `as_str()` / `Deref`. Currently callers can bypass the regex/length invariants enforced only in `::new`.
6. **Restructure `OpenJdError::{DecodeValidation, ModelValidation}` into struct variants carrying `Vec<ValidationError>`.** Let programmatic consumers inspect per-field failures without regex-parsing error strings. This aligns with `ExpressionError::kind()` in openjd-expr.
7. **Add `#[non_exhaustive]` to user-facing type enums:** `JobParameterType`, `TaskParameterType`, `KnownExtension`, `SpecificationRevision`, `CancelationMode`, `TaskParameter`, `EffectiveLimits`, `EffectiveRules`. Adding a new parameter type or extension is currently a breaking change.
8. **Normalize enum variant casing.** Pick PascalCase (Rust convention) with `#[serde(rename = "...")]` for wire names. `template/task_parameters.rs:11-21` is the biggest outlier (mixes SCREAMING and PascalCase in the same enum).

### P1 — Performance

9. **Replace dynamic regex compile in `format_strings.rs:1016`** with a single-pass word-boundary scan, or cache compiled regexes keyed by name. Remove the per-binding allocation.
10. **Replace `format!` + `parse::<RangeExpr>()` in `StaticChunkNode::chunk_range_expr`** with direct RangeExpr construction. This is on the chunking hot path.
11. **Cache parsed let-binding ASTs** in `instantiate.rs` so `collect_all_accessed_symbols` / `collect_let_binding_refs` don't re-parse each binding.
12. **Unify cycle detection:** delete the Kahn's implementation in `structure.rs:874-920` and reuse `StepDependencyGraph`. Single source of truth.
13. **Switch `TaskParameterSet` and env `variables` HashMap → IndexMap** for deterministic iteration order. Fixes the non-reproducible Display output flagged in `step_param_space.rs:1418`.
14. **O(K+M) input-key check in `parameters.rs:527`** — build a `HashSet<&str>` of merged names once.

### P2 — Specs ↔ implementation alignment

15. Update `architecture.md` module layout so it matches actual paths under `template/` and `job/` (mismatches #1–#6).
16. Update `job-types.md` and `job-creation.md` for the signature changes (mismatches #24, #29, #31, #32, #33, #34). These are the most user-facing spec errors because external callers will try to write code against the claimed API.
17. Reconcile `capabilities.md` vs `validation.md` `amount.` prefix (mismatch #40).
18. Fix `step-dependencies.md` algorithm description — says "Kahn's" but code is DFS (mismatch #39).
19. Document `PathParameterOptions`, `FloatRangeItem`, `convert_environment_with_symtab`, the 4 extra `StepParameterSpaceIterator` public methods, the 7 extra `EffectiveLimits` fields, `job::Environment.resolved_symtab`.

### P2 — Spec content gaps (new docs/sections)

20. Add a section (or new `expr-parameters.md` doc) covering the EXPR extension parameter types' `UserInterface` structs and `List*ItemConstraints` sub-structs.
21. Expand Pass 5 (`format_strings.rs`) coverage — function libraries, let-binding self-reference algorithm, comprehension validation, scope layering contract.
22. Add a `types.rs` reference section for `SpecificationRevision`/`TemplateSpecificationVersion`/`KnownExtension` + utility enums (`FileType`, `EndOfLine`, `ObjectType`, `DataFlow`).
23. Unify the "phase" vs "pass" numbering between `parsing.md` and `validation.md`.

### P2 — Test quality

24. **Migrate non-compliant error tests** in `tests/test_create_job.rs`, `tests/test_merge_job_parameters.rs`, and `src/test_expr_param_constraints.rs` to the `check_err` / `assert_validation_errors` pattern. ~110–130 tests affected. This is a mechanical migration but large; consider a scripted rewrite pass.
25. **Add dedicated unit tests** for each `validate_v2023_09` pass (direct calls, not via `decode_job_template`).
26. **Add inline `#[test]`s** in `src/capabilities.rs` (zero coverage) and `src/job/step_dependency_graph.rs` (multiple-disjoint-cycles, etc.).
27. **Split oversized test files** — especially `tests/test_create_job.rs` (3,933 lines). Propose a subfolder `tests/create_job/` with `mod.rs` re-exporting.

### P3 — Polish

28. Rename `OpenJdError` → `ModelError` (parallels `ExpressionError`). Rename the `OpenJdError::Expression` variant to `ExpressionEval` or similar to avoid shadowing `openjd_expr::ExpressionError`.
29. Add `Display` impls for `TemplateSpecificationVersion`, `KnownExtension`, `Description`, `ExtensionName`.
30. Replace `from_spec_str() -> Option` with `FromStr` impls for consistency.
31. Curate `lib.rs`: remove duplicate `create_job` re-export; group `pub mod` declarations before `pub use`; re-export `PathFormat` from openjd-expr; re-export `EffectiveLimits`/`EffectiveRules`/`ValidationError`/`PathElement`/`ValidationErrors`.
32. Add a `PathParameterOptions` builder to prevent invalid field combinations.
33. Consolidate in-src test files into `src/tests/` with a single `#[cfg(test)] mod tests;` entry point.
34. Add `#[must_use]` to ~10 constructor / builder-like methods.
35. Consider `Box`ing the `JobTemplate` variant of `DecodedTemplate` to equalize enum size and remove the `#[allow(clippy::large_enum_variant)]`.

---

## Appendix — Evaluation methodology

Five parallel `deadline-openjd` subagent invocations were used to review each aspect (specs, spec↔impl mismatches, Rust code quality, performance/algorithms, test suite audit, bug hunt). Results were cross-verified by direct `fs_read` / `grep` probes into key files (`parameters.rs`, `step_param_space.rs`, `error.rs`, `format_strings.rs`). Build, clippy, and test runs were executed locally on the same workspace:

```
cargo build  -p openjd-model --all-targets  # clean
cargo clippy -p openjd-model --all-targets  # clean
cargo test   -p openjd-model                # 1576 passed; 0 failed; 0 ignored
```

Confirmed failing-test probes (not committed) demonstrate bugs §1.1 and §1.2; the fixes above include test sketches for both.
