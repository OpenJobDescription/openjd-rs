# AGENTS.md

## Project Overview

openjd-rs is a Rust implementation of the [Open Job Description](https://github.com/OpenJobDescription) specification. It provides a model library, expression language, sessions runtime, and CLI for working with OpenJD job templates.

The canonical specification lives in the [openjd-specifications](https://github.com/OpenJobDescription/openjd-specifications) repo. The reference Python implementation lives in [openjd-model-for-python](https://github.com/OpenJobDescription/openjd-model-for-python).

## Building

```bash
cargo build --release
```

The CLI binary is produced at `target/release/openjd-rs`.

## Running Tests

```bash
# All tests
cargo test

# Single crate
cargo test --package openjd-expr

# Single test file
cargo test --package openjd-expr --test test_arithmetic
```

### S3 Integration Tests

The `openjd-snapshots` crate has integration tests that run against a real S3 bucket. These are `#[ignore]`d by default and require environment variables:

| Variable | Required | Description |
|----------|----------|-------------|
| `OPENJD_TEST_S3_BUCKET` | Yes | S3 bucket name (tests skip if unset) |
| `OPENJD_TEST_S3_PREFIX` | No | Key prefix (default: `openjd-snapshots-test`) |
| `AWS_REGION` | No | AWS region (default: `us-west-2`) |

AWS credentials are resolved via the standard credential chain (`AWS_PROFILE`, `AWS_ACCESS_KEY_ID`/`AWS_SECRET_ACCESS_KEY`, `~/.aws/credentials`, IMDS, etc.).

```bash
# Run S3 integration tests
AWS_PROFILE=GammaSandbox \
OPENJD_TEST_S3_BUCKET=rendering-agent-spaces-workshop \
OPENJD_TEST_S3_PREFIX=OpenJDSnapshotsTests \
cargo test -p openjd-snapshots --test test_s3_integration -- --ignored
```

## Test Quality Standard for Error Cases

When writing tests that check for validation or evaluation failures, assert on the
**full error message content** — not just that an error occurred. This ensures error
messages are stable, human-readable, and match the Python implementation.

### openjd-expr: assert message + expression + caret

Every expression evaluation error test must assert the multi-line error including
the message, the expression source line, and the caret indicator pointing at the
error location. See `tests/test_error_formatting.rs` for the pattern:

```rust
#[test] fn type_error_in_middle() {
    assert_err("1 + int('bad') + 2", &[
        "Cannot convert 'bad' to int\n",
        "  1 + int('bad') + 2\n",
        "      ^~~~~~~~~~",
    ]);
}
```

### openjd-model: assert path + message

Every template validation error test must assert the field path and message,
matching the Python Pydantic error format. See `tests/test_error_messages.rs`
for the pattern:

```rust
#[test]
fn empty_command() {
    check_err(r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "steps": [{"name": "S", "script": {"actions": {"onRun": {"command": ""}}}}]
    }"#, &[
        "steps[0] -> script -> actions -> onRun -> command:\n\tmust not be empty.",
    ]);
}
```

### Why

- Catches regressions in error message quality
- Ensures Rust and Python implementations produce comparable output
- Makes error paths testable (not just "did it fail?")
- Conformance tests only check pass/fail — these tests verify the diagnostics

## Crates

### openjd-expr (`crates/openjd-expr`)

Expression language implementation. This is the most mature crate.

- **Type system** (`src/types.rs`) — `ExprType` with type codes for primitives, lists, unions, type variables (`T`, `T1`, `T2`, `T3`), `unresolved[T]`, `noreturn`, and `any`. Includes string parsing (`ExprType::parse("list[int]")`), normalization (union flattening, unresolved hoisting), and type matching/substitution for generic function signatures.
- **Values** (`src/value.rs`) — `ExprValue` enum with typed list variants (`ListBool`, `ListInt`, `ListFloat`, `ListString`, `ListPath`, `ListList`), float passthrough for preserving original string representations, and `Unresolved` for static type checking.
- **Parser** (`src/eval/parse.rs`) — Uses `ruff_python_parser` to parse Python expression syntax. Handles contextual keywords (Python keywords used as attribute names after `.`) via same-length identifier replacement.
- **Evaluator** (`src/eval/evaluator.rs`) — Walks the ruff AST with memory-bounded and operation-bounded execution. Implements arithmetic, comparison, logical ops, conditionals, function calls, method calls, list comprehensions, slicing, string operations, path operations, regex, and repr functions.
- **Format strings** (`src/format_string.rs`) — Parses `{{Param.Name}}` and `{{Expr.Name}}` syntax in template strings.
- **Range expressions** (`src/range_expr.rs`) — Parses range expressions like `1-10`, `1-100:10`, `1,5,10-20`.
- **Path mapping** (`src/path_mapping.rs`) — Applies source→destination path mapping rules.
- **Symbol table** (`src/symbol_table.rs`) — Hierarchical key-value store supporting dotted paths (`Param.Frame`).

### openjd-model (`crates/openjd-model`)

Template parsing, validation, and job creation. Parses YAML/JSON templates, validates against the 2023-09 schema, resolves format strings, and creates job structures.

### openjd-sessions (`crates/openjd-sessions`)

Local job execution runtime. Manages session lifecycle, runs actions via subprocess, handles environment setup/teardown.

### openjd-cli (`crates/openjd-cli`)

CLI binary (`openjd-rs`) with `check` and `run` subcommands.

```bash
# Validate a template
openjd-rs check path/to/template.yaml

# Run a job template locally
openjd-rs run path/to/template.yaml -p Key=Value
```

## Running the Conformance Suite

The [openjd-specifications](https://github.com/OpenJobDescription/openjd-specifications) repo contains an implementation-agnostic conformance test suite. To run it against the Rust CLI:

### Linux / macOS

```bash
# Build the Rust CLI and create a local symlink the test runner can find as "openjd"
cd ~/openjd-rs
cargo build --release
mkdir -p bin
ln -sf "$(pwd)/target/release/openjd-rs" bin/openjd

# Run the conformance suite (requires openjd-specifications repo)
cd ~/openjd-specifications/conformance-tests
PATH="$HOME/openjd-rs/bin:$PATH" uv run run_openjd_cli_tests.py '2023-09/*'
```

### Windows

```bash
# Build the Rust CLI (skip openjd-snapshots which has pre-existing Windows build issues)
cd C:\Dev\ojd\openjd-rs
cargo build --release -p openjd-cli

# Copy the binary as "openjd.exe" so the test runner can find it
mkdir -p bin
cp target/release/openjd-rs.exe bin/openjd.exe

# Run the conformance suite (requires openjd-specifications repo)
PATH="$(pwd)/bin:$PATH" uv run ../openjd-specifications/conformance-tests/run_openjd_cli_tests.py '2023-09/*'
```

### Filtering tests

To run only EXPR extension tests:
```bash
PATH="$HOME/openjd-rs/bin:$PATH" uv run run_openjd_cli_tests.py '2023-09/EXPR/*'
```

To run a single test:
```bash
PATH="$HOME/openjd-rs/bin:$PATH" uv run run_openjd_cli_tests.py '2023-09/EXPR/jobs/expr1.1.3--keyword-attrs-in-exprs.test.yaml'
```

The test runner expects the CLI to have `check` and `run` subcommands with the same interface as the Python `openjd` CLI.

## Key Design Decisions

- **ruff_python_parser** for expression parsing — the EXPR extension uses Python expression syntax, and the spec recommends ruff for Rust implementations. See `specs/expr/parser.md`.
- **Typed list variants** in `ExprValue` — instead of a single `List(Vec<ExprValue>)`, uses `ListInt(Vec<i64>)`, `ListString(Vec<String>)`, etc. for memory efficiency. See `specs/typed-list-refactor.md`.
- **Expression language spec** — the authoritative reference is `openjd-specifications/wiki/2026-02-Expression-Language.md`.
