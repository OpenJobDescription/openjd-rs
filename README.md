# openjd-rs

A Rust implementation of the [Open Job Description](https://github.com/OpenJobDescription) specification — model library, expression language, sessions runtime, job attachments snapshots, and CLI.

## Crates

| Crate | Description |
|-------|-------------|
| [`openjd-expr`](crates/openjd-expr) | Expression language: type system, ruff-based parser, bounded evaluator, range expressions, path mapping |
| [`openjd-model`](crates/openjd-model) | Core library: template parsing, validation, job creation, parameter spaces, step dependency graphs |
| [`openjd-sessions`](crates/openjd-sessions) | Session runtime: async action execution, environment lifecycle, cross-user subprocess management, structured logging |
| [`openjd-snapshots`](crates/openjd-snapshots) | Job attachments: content-addressed file tree snapshots, xxHash3 hashing, manifest diffing, S3 upload/download |
| [`openjd-cli`](crates/openjd-cli) | CLI binary (`openjd-rs`): `check` and `run` commands |

## Building

```bash
cargo build --release
```

## Usage

```bash
# Validate a template
./target/release/openjd-rs check path/to/template.yaml

# Run a job template locally
./target/release/openjd-rs run path/to/template.yaml -p Key=Value
./target/release/openjd-rs run path/to/template.yaml -p file://params.yaml
```

## Testing

### Unit tests

```bash
# All crates
cargo test --workspace

# Single crate
cargo test -p openjd-sessions
```

The workspace has 5,300+ unit and integration tests across all crates.

### Cross-user tests (Docker)

Cross-user subprocess execution (via `sudo`) requires a multi-user Linux environment. Docker containers provide this:

```bash
# Local users (primary)
scripts/run_cross_user_tests.sh

# LDAP-managed users (validates NSS/PAM integration)
scripts/run_cross_user_tests.sh --ldap
```

This runs 12 cross-user tests covering subprocess execution, signal delivery (SIGTERM/SIGKILL), process tree killing, session cleanup, and TempDir permissions. See [the proposal](docs/proposals/docker-based-cross-user-tests.md) for details.

### Code coverage

```bash
scripts/coverage.sh
```

Current coverage: 87.3% line coverage (see [COVERAGE_REPORT.md](COVERAGE_REPORT.md)).

## Conformance

Passes 100% of the [OpenJD conformance test suite](https://github.com/OpenJobDescription/openjd-specifications/tree/mainline/conformance-tests) (1,038 tests on Linux):

| Category | Tests |
|----------|-------|
| Template validation (base) | 444 |
| Template validation (EXPR) | 202 |
| Template validation (FEATURE_BUNDLE_1) | 36 |
| Template validation (TASK_CHUNKING) | 11 |
| Environment template validation | 31 |
| Job execution (base) | 163 |
| Job execution (EXPR) | 123 |
| Job execution (FEATURE_BUNDLE_1) | 13 |
| Job execution (TASK_CHUNKING) | 7 |
| Job execution (REDACTED_ENV_VARS) | 8 |
| **Total** | **1,038** |

## Architecture

The crate dependency graph:

```
openjd-cli
├── openjd-sessions
│   ├── openjd-model
│   │   └── openjd-expr
│   └── openjd-expr
└── openjd-model

openjd-snapshots (standalone)
```

Design documents are in [`specs/`](specs/):

| Document | Topic |
|----------|-------|
| [architecture.md](specs/architecture.md) | Overall crate structure and design |
| [sessions-architecture.md](specs/sessions-architecture.md) | Sessions runtime async design |
| [action-message-streaming.md](specs/action-message-streaming.md) | Real-time stdout message streaming |
| [parsing-walkthrough.md](specs/parsing-walkthrough.md) | Template parsing pipeline |
| [template-validation.md](specs/template-validation.md) | Validation rules and error reporting |
| [model-parameters.md](specs/model-parameters.md) | Parameter type system |
| [job-creation.md](specs/job-creation.md) | Job instantiation from templates |
| [format-string.md](specs/format-string.md) | Format string resolution |
| [expr-implementation.md](specs/expr-implementation.md) | Expression language implementation |
| [function-library.md](specs/function-library.md) | Built-in function library |
| [job-attachments-snapshots.md](specs/job-attachments-snapshots.md) | Snapshot file format and operations |

## Security

See [CONTRIBUTING](CONTRIBUTING.md#security-issue-notifications) for more information.

## License

This project is licensed under the Apache-2.0 License or the MIT License, at your option.
