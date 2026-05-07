# openjd-for-js — ECMAScript/WebAssembly Bindings for OpenJD

## Overview

`openjd-for-js` provides ECMAScript bindings for the OpenJD Rust implementation (`openjd-rs`), compiled to WebAssembly. It enables spec-compliant template parsing, validation, job creation, expression evaluation, and parameter space expansion in browsers and VS Code extensions.

This is the JS equivalent of the Python bindings (`_openjd_rs` via PyO3) implemented in [mwiebe:openjd-model-for-python:bindings-rs](https://github.com/OpenJobDescription/openjd-model-for-python/compare/mainline...mwiebe:openjd-model-for-python:bindings-rs).

## Motivation

ECMAScript applications that work with Open Job Description templates (e.g., template viewers, editors, CI validators, browser-based tools) need access to spec-compliant validation, expression evaluation, and job creation logic. Without authoritative bindings, JS implementations must:

- Reimplement spec logic in ECMAScript, risking divergence from the reference implementation
- Handle only a subset of the spec (e.g., missing EXPR extension support for arithmetic, conditionals, function calls)
- Manually track spec changes across two codebases

By compiling the Rust implementation to WebAssembly, consumers get complete, authoritative, and automatically-maintained JS bindings — the same engine that powers the CLI and Python bindings.

**Example use cases:**
- Template viewers that validate and visualize OpenJD templates in the browser
- VS Code/IDE extensions for inline template diagnostics
- CI/CD tools that validate templates without requiring Python or Rust toolchains
- Web-based job submission portals with client-side validation

## Design

### Architecture

```
openjd-rs/
├── crates/
│   ├── openjd-expr/          ← Expression engine (Rust)
│   ├── openjd-model/         ← Template model (Rust)
│   ├── openjd-sessions/      ← Runtime sessions (Rust, not exposed to JS)
│   └── openjd-for-js/        ← WASM bindings crate — all JS concerns collocated
│       ├── Cargo.toml        ← Rust crate manifest
│       ├── package.json      ← npm manifest ("openjd-for-js")
│       ├── package-lock.json
│       ├── vitest.config.ts  ← JS test runner config
│       ├── src/              ← Rust sources
│       │   ├── lib.rs         ← Module registration
│       │   ├── expr.rs        ← Expression engine bindings
│       │   ├── model.rs       ← Model bindings (decode, job, etc.)
│       │   └── errors.rs      ← Error type bindings
│       ├── tests/            ← Rust (rlib) integration tests
│       ├── js-tests/         ← JS-side integration tests (vitest)
│       └── pkg/              ← wasm-bindgen output (gitignored)
```

### Build Pipeline

```
Rust source → cargo build --target wasm32-unknown-unknown
           → wasm-bindgen --target web → .wasm + .js + .d.ts
           → wasm-opt -Oz → optimized .wasm (~400KB gzipped)
           → npm package in crates/openjd-for-js/
```

### Binding Pattern

Each Rust type gets a JS wrapper class using `wasm_bindgen`. The wrapper holds an internal reference to the Rust struct and exposes read-only getters:

```rust
#[wasm_bindgen]
pub struct JobTemplate {
    inner: openjd_model::JobTemplate,
}

#[wasm_bindgen]
impl JobTemplate {
    #[wasm_bindgen(getter)]
    pub fn name(&self) -> String {
        self.inner.name.clone()
    }

    #[wasm_bindgen(getter, js_name = "specificationVersion")]
    pub fn specification_version(&self) -> String {
        self.inner.specification_version.to_string()
    }
}
```

This mirrors the PyO3 pattern used in the Python bindings where `PyJobTemplate` wraps `openjd_model::JobTemplate`.

## Interface Specification

### Functions

#### Template Decoding

| Function | Signature | Description |
|----------|-----------|-------------|
| `decodeJobTemplate` | `(input: string) → JobTemplate` | Parse + validate a job template from JSON/YAML string. Throws `DecodeValidationError` or `ModelValidationError` on failure. |
| `decodeJobTemplateFromObject` | `(obj: object) → JobTemplate` | Parse + validate from a JS object (already parsed JSON). |
| `decodeEnvironmentTemplate` | `(input: string) → EnvironmentTemplate` | Parse + validate an environment template from string. |
| `decodeEnvironmentTemplateFromObject` | `(obj: object) → EnvironmentTemplate` | Parse + validate from a JS object. |
| `validateTemplate` | `(input: string) → string[]` | Validate without throwing. Returns array of error messages (empty = valid). |

#### Job Creation

| Function | Signature | Description |
|----------|-----------|-------------|
| `createJob` | `(template: JobTemplate, params: Record<string, string>) → Job` | Create a fully resolved Job from a template and parameter values. Resolves all format strings, expands parameter spaces. |
| `preprocessJobParameters` | `(template: JobTemplate, rawValues: Record<string, string>) → Map<string, ExprValue>` | Coerce raw string parameter values to typed ExprValues per the parameter definitions. |
| `mergeJobParameterDefinitions` | `(templates: (JobTemplate \| EnvironmentTemplate)[]) → JobParameterDefinition[]` | Merge parameter definitions from multiple templates per the spec's merging rules. |
| `evaluateLetBindings` | `(bindings: string[], symbols: SymbolTable) → SymbolTable` | Evaluate `let` bindings and add results to the symbol table. |
| `createEnvironment` | `(envTemplate: EnvironmentTemplate, params: Record<string, string>) → Environment` | Create a resolved Environment from a template. |
| `deserializeStep` | `(obj: object) → StepTemplate` | Deserialize a step from a JS object. |

#### Expression Engine

| Function | Signature | Description |
|----------|-----------|-------------|
| `evaluateExpression` | `(expr: string, symbols: SymbolTable, lib?: FunctionLibrary) → ExprValue` | Evaluate an EXPR extension expression. |
| `parseExpression` | `(expr: string) → ParsedExpression` | Parse an expression for repeated evaluation. |
| `getDefaultLibrary` | `() → FunctionLibrary` | Get the default function library (includes builtins like `len`, `str`, `int`, etc.). |
| `escapeFormatString` | `(s: string) → string` | Escape `{{` and `}}` in a string for literal use in format strings. |
| `parseRangeExpr` | `(expr: string) → number[]` | Parse an IntRangeExpr (e.g., `"1-10:2"`) into an array of integers. |

### Classes — Model

#### `JobTemplate`

Read-only. Returned by `decodeJobTemplate`.

| Property | Type | Description |
|----------|------|-------------|
| `name` | `string` | Template name (may contain format strings) |
| `specificationVersion` | `string` | `"jobtemplate-2023-09"` |
| `description` | `string \| undefined` | Optional description |
| `parameterDefinitions` | `JobParameterDefinition[]` | Parameter definitions |
| `steps` | `StepTemplate[]` | Step templates |
| `jobEnvironments` | `EnvironmentTemplate[]` | Job-level environments |
| `extensions` | `string[] \| undefined` | Enabled extensions (e.g., `["EXPR", "TASK_CHUNKING"]`) |

#### `StepTemplate`

| Property | Type |
|----------|------|
| `name` | `string` |
| `description` | `string \| undefined` |
| `dependencies` | `StepDependency[]` |
| `parameterSpace` | `StepParameterSpace \| undefined` |
| `script` | `StepScript` |
| `hostRequirements` | `object \| undefined` |
| `stepEnvironments` | `EnvironmentTemplate[]` |

#### `Job`

Returned by `createJob`. All format strings are resolved.

| Property | Type |
|----------|------|
| `name` | `string` |
| `steps` | `Step[]` |
| `jobEnvironments` | `Environment[]` |
| `parameters` | `Map<string, ExprValue>` |

#### `Step`

| Property | Type |
|----------|------|
| `name` | `string` |
| `script` | `StepScript` |
| `taskCount` | `number` |
| `dependencies` | `string[]` |
| `environments` | `Environment[]` |

#### `Environment`

| Property | Type |
|----------|------|
| `name` | `string` |
| `script` | `EnvironmentScript \| undefined` |
| `variables` | `Map<string, string>` |

#### `Action`

| Property | Type |
|----------|------|
| `command` | `string` |
| `args` | `string[]` |
| `timeout` | `number \| undefined` |
| `cancelation` | `CancelationMode \| undefined` |

#### `StepScript` / `EnvironmentScript`

| Property | Type |
|----------|------|
| `actions` | `StepActions \| EnvironmentActions` |
| `embeddedFiles` | `EmbeddedFile[]` |

#### `StepActions`

| Property | Type |
|----------|------|
| `onRun` | `Action` |

#### `EnvironmentActions`

| Property | Type |
|----------|------|
| `onEnter` | `Action \| undefined` |
| `onExit` | `Action \| undefined` |

#### `EmbeddedFile`

| Property | Type |
|----------|------|
| `name` | `string` |
| `type` | `string` |
| `filename` | `string \| undefined` |
| `data` | `string` |
| `runnable` | `boolean` |

#### `JobParameterDefinition`

| Property | Type |
|----------|------|
| `name` | `string` |
| `type` | `JobParameterType` |
| `description` | `string \| undefined` |
| `default` | `ExprValue \| undefined` |
| `allowedValues` | `ExprValue[] \| undefined` |
| `minValue` | `number \| undefined` |
| `maxValue` | `number \| undefined` |
| `minLength` | `number \| undefined` |
| `maxLength` | `number \| undefined` |
| `userInterface` | `object \| undefined` |

#### `StepDependency`

| Property | Type |
|----------|------|
| `dependsOn` | `string` |

#### `StepParameterSpace`

| Property | Type |
|----------|------|
| `taskParameterDefinitions` | `TaskParameterDefinition[]` |
| `combination` | `string \| undefined` |

#### `StepDependencyGraph`

| Constructor | `new StepDependencyGraph(job: Job)` |
|-------------|------|
| `topologicalOrder()` | `string[]` — Step names in dependency order |
| `hasCycles()` | `boolean` |

#### `StepParameterSpaceIterator`

| Constructor | `new StepParameterSpaceIterator(paramSpace: StepParameterSpace, symbols: SymbolTable)` |
|-------------|------|
| `count()` | `number` — Total task count |
| `[Symbol.iterator]()` | `Iterator<Map<string, ExprValue>>` — Iterate over task parameter sets |

### Classes — Expression Engine

#### `ExprValue`

| Static Constructor | Returns |
|---|---|
| `ExprValue.string(v)` | String value |
| `ExprValue.int(v)` | Integer value |
| `ExprValue.float(v)` | Float value |
| `ExprValue.bool(v)` | Boolean value |
| `ExprValue.path(v)` | Path value |

| Method | Returns |
|---|---|
| `toString()` | `string` |
| `toJSON()` | `any` — Native JS value |
| `type` (getter) | `ExprType` |

#### `ExprType`

Enum: `String`, `Int`, `Float`, `Bool`, `Path`, `RangeExpr`, `ListString`, `ListInt`, `ListFloat`, `ListBool`, `ListPath`, `ListListInt`

#### `FormatString`

| Constructor | `new FormatString(input: string)` |
|---|---|
| `resolve(symbols: SymbolTable)` | `string` — Resolved string |
| `references` (getter) | `string[]` — Referenced symbols (e.g., `["Param.Frames"]`) |

#### `ParsedExpression`

| Constructor | Via `parseExpression(expr)` |
|---|---|
| `evaluate(symbols: SymbolTable, lib?: FunctionLibrary)` | `ExprValue` |

#### `SymbolTable`

| Constructor | `new SymbolTable()` |
|---|---|
| `set(scope, name, value)` | `void` — e.g., `set("Param", "Frames", ExprValue.string("1-10"))` |
| `get(scope, name)` | `ExprValue \| undefined` |

#### `FunctionLibrary`

| Static | `FunctionLibrary.default()` — Built-in functions |
|---|---|
| `withPathMappingRules(rules: PathMappingRule[])` | `FunctionLibrary` — Library with path mapping |

#### `PathMappingRule`

| Constructor | `new PathMappingRule(sourceOs: PathFormat, sourcePath: string, destPath: string)` |
|---|---|

#### `PathFormat`

Enum: `Posix`, `Windows`

### Error Types

| Error | When thrown |
|-------|-----------|
| `DecodeValidationError` | Structural parse failure (bad YAML/JSON, missing fields, wrong types) |
| `ModelValidationError` | Semantic validation failure (template parsed but violates spec rules) |
| `UnsupportedSchemaError` | Unknown specification version |
| `ExpressionError` | Expression evaluation failure |
| `ExpressionTypeError` | Type mismatch in expression |
| `RangeExprError` | Invalid range expression syntax |
| `FormatStringValidationError` | Invalid format string syntax |

All errors extend ECMAScript's `Error` class with a `.message` property.

### Constants

| Constant | Value | Description |
|----------|-------|-------------|
| `DEFAULT_MEMORY_LIMIT` | `number` | Default memory limit for expression evaluation |
| `DEFAULT_OPERATION_LIMIT` | `number` | Default operation limit for expression evaluation |

## Python Bindings Parity

### Covered (full parity)

- All template decode functions (str and dict variants)
- All job creation functions (createJob, preprocessJobParameters, mergeJobParameterDefinitions, evaluateLetBindings)
- All model types (JobTemplate, Job, Step, Action, Environment, etc.)
- All expression types (ExprValue, FormatString, SymbolTable, FunctionLibrary, ParsedExpression)
- All error types
- Step dependency graph
- Step parameter space iteration

### Not covered (not applicable to browser)

| Python binding | Reason for exclusion |
|---|---|
| `Session` | Requires filesystem, process spawning, OS signals |
| `CancellationToken` | Runtime-only concept |
| `SessionCallbacks` | Runtime-only concept |
| `ActionStatus` / `SessionStatus` | Runtime-only concept |

Sessions are excluded because they manage OS processes and filesystem I/O, which are not available in the browser WASM sandbox. The sessions crate could be exposed in a future Node.js (non-browser) target via wasi or napi.

## Size Budget

| Component | Estimated size (gzipped) |
|-----------|-------------------------|
| WASM binary (optimized) | ~400KB |
| JS glue code | ~15KB |
| TypeScript definitions | ~5KB |
| **Total** | **~420KB** |

For comparison: pako (zlib) is ~45KB, js-yaml is ~50KB. The WASM module is larger because it includes the full expression parser (ruff_python_parser for EXPR extension support).

## Browser & Runtime Compatibility

| Environment | Support |
|-------------|---------|
| Chrome 57+ | ✅ |
| Firefox 52+ | ✅ |
| Safari 11+ | ✅ |
| Edge 16+ | ✅ |
| Node.js 8+ | ✅ |
| VS Code webview | ✅ (requires `wasm-unsafe-eval` in CSP) |
| Deno | ✅ |

## Usage Example

```js
import init, {
  decodeJobTemplate,
  createJob,
  StepDependencyGraph,
  FormatString,
  SymbolTable,
  ExprValue,
} from 'openjd-for-js';

// Initialize WASM (once, async)
await init();

// Decode and validate
const template = decodeJobTemplate(yamlString);
console.log(template.name, template.steps.length, "steps");

// Create a resolved job
const job = createJob(template, {
  Frames: "1-10",
  OutputDir: "/renders/shot01",
});

// Get execution order
const graph = new StepDependencyGraph(job);
console.log("Execution order:", graph.topologicalOrder());

// Inspect resolved actions
for (const step of job.steps) {
  const action = step.script.actions.onRun;
  console.log(`${step.name}: ${action.command} ${action.args.join(" ")}`);
  console.log(`  Tasks: ${step.taskCount}`);
}

// Resolve a format string manually
const symbols = new SymbolTable();
symbols.set("Param", "OutputDir", ExprValue.string("/renders/shot01"));
const fmt = new FormatString("{{Param.OutputDir}}/frame.####.exr");
console.log(fmt.resolve(symbols)); // "/renders/shot01/frame.####.exr"
```

## Implementation Phases

### Phase 1: Core (MVP for viewer integration)
- `decodeJobTemplate`, `decodeEnvironmentTemplate`, `validateTemplate`
- `JobTemplate`, `EnvironmentTemplate` wrapper types with getters
- Error types
- Build pipeline (cargo + wasm-bindgen + wasm-opt)
- npm package structure

### Phase 2: Job Creation
- `createJob`, `preprocessJobParameters`
- `Job`, `Step`, `Action`, `Environment` wrapper types
- `StepDependencyGraph`
- Requires `Serialize` derives on Rust model types OR wrapper getters

### Phase 3: Expression Engine
- `FormatString`, `SymbolTable`, `ExprValue`
- `evaluateExpression`, `parseExpression`
- `FunctionLibrary`, `PathMappingRule`
- `StepParameterSpaceIterator`
- `parseRangeExpr`

### Phase 4: Polish
- TypeScript wrapper package in `crates/openjd-for-js/`
- Comprehensive tests (port from Python binding tests)
- CI/CD for WASM builds
- npm publishing
- Documentation and examples

## References

- [Python bindings PR](https://github.com/OpenJobDescription/openjd-model-for-python/compare/mainline...mwiebe:openjd-model-for-python:bindings-rs) — Mark Wiebe's PyO3 bindings
- [js-rattler](https://github.com/conda/rattler/tree/main/js-rattler) — Reference implementation for Rust→WASM→JS bindings pattern
- [OpenJD 2023-09 Template Schema](https://github.com/OpenJobDescription/openjd-specifications/wiki/2023-09-Template-Schemas)
- [wasm-bindgen guide](https://rustwasm.github.io/docs/wasm-bindgen/)


## Development Methodology

Based on the proven methodology used to port OpenJD from Python to Rust (see `specs/rust-port-agent-method.md`), the JS bindings will follow the same staged approach adapted for the WASM/JS boundary.

### Stage 1: Conformance Suite + Basic Bindings

**Goal:** Get the JS bindings passing the OpenJD conformance suite via Node.js.

The conformance suite in `openjd-specifications/conformance-tests` exercises template validation and job execution through `openjd check` and `openjd run`. We adapt this for JS:

1. Load the WASM module in Node.js (Vitest test runner)
2. For each conformance test case, call `validateTemplate()` or `decodeJobTemplate()` through the JS bindings
3. Assert pass/fail matches the expected result
4. Grind until 100% conformance suite pass rate

At this stage, the bindings may be rough — the goal is correctness, not ergonomics.

### Stage 2: Port Unit Tests + Quality Evaluation + Refactoring

**Goal:** Comprehensive test coverage and production-quality API surface.

This stage alternates between three activities:

#### 2a. Port Python binding unit tests

Mark's Python bindings (`_openjd_rs`) have 1575 expr + model tests. Port each group to JS:

> Enumerate every group of unit tests in the Python bindings one by one. Create a `TEST_PORT_CHECKLIST.md` with a checklist and track progress. For each group, evaluate how to perform the same equivalent tests against the JS/WASM bindings. Implement those tests in Vitest, revise the JS bindings to pass. Continue until every group is processed.

Key test categories to port:
- Template decode (valid/invalid for every field, every parameter type)
- Job creation (parameter preprocessing, format string resolution, let bindings)
- Expression evaluation (arithmetic, conditionals, functions, type coercion)
- Format string parsing and resolution
- Range expression parsing
- Step dependency graph (topological sort, cycle detection)
- Parameter space iteration (product, associative combinations)
- Error messages (full multi-line assertions with caret indicators)

#### 2b. Quality evaluation

Repeated evaluation prompts comparing JS bindings against Python bindings and specs:

> Go through each class in the Python bindings (`rust/src/model/`, `rust/src/expr/`), and compare to the corresponding JS binding. Check: Are all methods exposed? Are return types equivalent? Are error messages identical? Make a checklist, enumerate findings, and produce a report.

> Go through the OpenJD specifications in `wiki/` and compare to the JS bindings. Check: Is every specified behavior testable through the JS API? Are there any spec requirements that the JS bindings cannot express?

#### 2c. Refactoring based on findings

Address findings from quality evaluations. Common patterns:
- Missing methods on wrapper types
- Type mismatches at the WASM boundary (e.g., `u32` vs `number`)
- Error messages truncated or reformatted during serialization
- Memory leaks from un-freed WASM objects

### Stage 3: Overall Quality Assurance

**Goal:** High confidence in correctness, completeness, and usability.

Repeated application of evaluation prompts through different lenses:

#### JS vs Python parity

> Compare every public function and class in the Python bindings to the JS bindings. For each one: Is it present? Does it have the same signature? Does it produce the same output for the same input? Document any intentional differences and their rationale.

#### JS vs Spec compliance

> Go through the OpenJD 2023-09 Template Schema specification section by section. For each requirement, identify the JS binding function or class that implements it. Write a test that exercises that requirement. Flag any unimplemented or partially implemented requirements.

#### JS-specific quality

> Evaluate the JS bindings for ECMAScript/TypeScript idioms. Are classes using proper getters? Are iterators implementing `Symbol.iterator`? Are errors extending `Error` properly? Are TypeScript types accurate and complete? Is the async initialization pattern correct?

#### Serialization boundary audit

> Audit every value that crosses the Rust→JS boundary via `wasm-bindgen`. For each crossing point: What Rust type is being converted? What JS type does it become? Are there edge cases (empty strings, NaN, Infinity, very large integers, Unicode, null vs undefined) that could cause data loss or type confusion? Write tests for each edge case.

### Quality Criteria

The JS bindings are considered production-ready when:

1. **100% conformance suite pass rate** — All conformance tests pass through the JS bindings
2. **Unit test parity** — Every Python binding unit test has a JS equivalent that passes
3. **Zero known behavioral differences** — Any difference from Python bindings is documented and intentional
4. **TypeScript types are complete** — Every exported function and class has accurate `.d.ts` types
5. **Error messages match** — Error messages from JS bindings are identical to Python binding error messages
6. **Memory safety** — No WASM memory leaks under normal usage patterns
7. **Clean compilation** — `cargo build` produces no warnings, `tsc` produces no errors
8. **All tests pass** — `cargo test` (Rust unit tests) + `vitest` (JS integration tests) all green

## Test Architecture

```
crates/openjd-for-js/
├── tests/               ← Rust (rlib) integration tests for wasm_bindgen wrappers
├── js-tests/            ← JS-side integration tests (run via vitest)
│   ├── conformance/     ← Conformance suite adapted for JS
│   ├── expr/            ← Ported from Python expr binding tests
│   ├── model/           ← Ported from Python model binding tests
│   ├── boundary/        ← WASM serialization boundary edge cases
│   └── memory/          ← Memory leak / lifecycle tests
├── TEST_PORT_CHECKLIST.md
└── vitest.config.ts
```

### Conformance Tests

Load each template from `openjd-specifications/conformance-tests`, call through JS bindings, assert pass/fail:

```js
import { decodeJobTemplate, validateTemplate } from 'openjd-for-js';

// For each .json/.yaml in conformance-tests/check-pass/
test('conformance: check-pass/basic-job.yaml', () => {
  expect(() => decodeJobTemplate(readFile('basic-job.yaml'))).not.toThrow();
});

// For each .json/.yaml in conformance-tests/check-fail/
test('conformance: check-fail/missing-steps.yaml', () => {
  const errors = validateTemplate(readFile('missing-steps.yaml'));
  expect(errors.length).toBeGreaterThan(0);
});
```

### Boundary Tests

Test every type crossing the WASM boundary:

```js
describe('WASM boundary: string handling', () => {
  test('empty string', () => { ... });
  test('unicode (emoji)', () => { ... });
  test('unicode (CJK)', () => { ... });
  test('very long string (1MB)', () => { ... });
  test('string with null bytes', () => { ... });
});

describe('WASM boundary: numeric handling', () => {
  test('integer max safe', () => { ... });
  test('integer overflow', () => { ... });
  test('float NaN', () => { ... });
  test('float Infinity', () => { ... });
  test('float negative zero', () => { ... });
});
```

### Memory Tests

Verify WASM objects are properly cleaned up:

```js
describe('memory management', () => {
  test('JobTemplate is freed after going out of scope', () => {
    // Create many templates, verify memory doesn't grow unbounded
  });

  test('SymbolTable entries are freed', () => {
    // Set many values, verify cleanup
  });
});
```

## CI/CD Pipeline

```
┌─────────────┐    ┌──────────────┐    ┌─────────────┐    ┌──────────┐
│ cargo build  │ →  │ wasm-bindgen │ →  │ wasm-opt    │ →  │ npm pack │
│ --target     │    │ --target web │    │ -Oz         │    │          │
│ wasm32       │    │              │    │             │    │          │
└─────────────┘    └──────────────┘    └─────────────┘    └──────────┘
       │                                                        │
       ▼                                                        ▼
┌─────────────┐                                          ┌──────────┐
│ cargo test  │                                          │ vitest   │
│ (Rust unit) │                                          │ (JS e2e) │
└─────────────┘                                          └──────────┘
```

### CI Steps

1. `cargo build --target wasm32-unknown-unknown -p openjd-for-js --release`
2. `wasm-bindgen --target web --out-dir crates/openjd-for-js/pkg target/wasm32-unknown-unknown/release/openjd_for_js.wasm`
3. `wasm-opt -Oz crates/openjd-for-js/pkg/openjd_for_js_bg.wasm -o crates/openjd-for-js/pkg/openjd_for_js_bg.wasm`
4. `cargo test -p openjd-for-js` (Rust-side tests)
5. `cd crates/openjd-for-js && npm install && npx vitest run` (JS-side tests)
6. `cd crates/openjd-for-js && npm pack` (produce distributable)

Steps 1–3 are combined into `npm run build` when invoked from `crates/openjd-for-js/`.

## Relationship to Other Bindings

```
                    ┌─────────────────────┐
                    │   openjd-rs crates   │
                    │  (expr, model, etc.) │
                    └──────────┬──────────┘
                               │
              ┌────────────────┼────────────────┐
              │                │                │
              ▼                ▼                ▼
     ┌────────────┐   ┌──────────────┐   ┌──────────┐
     │ Python     │   │ JS/WASM      │   │ (future) │
     │ bindings   │   │ bindings     │   │ C/FFI    │
     │ (PyO3)     │   │ (wasm-bindgen│   │ bindings │
     │            │   │  + TS)       │   │          │
     │ _openjd_rs │   │ openjd-for-js    │   │          │
     └────────────┘   └──────────────┘   └──────────┘
```

All bindings share the same Rust core. A bug fix or spec update in the Rust crates automatically propagates to all binding targets on rebuild. The JS bindings should maintain API parity with the Python bindings (minus sessions) so that documentation and examples are transferable between languages.
