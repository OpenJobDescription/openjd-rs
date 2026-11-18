# openjd-rs

A Rust implementation of the [Open Job Description](https://github.com/OpenJobDescription) specification — model library, expression language, sessions runtime, job attachments snapshots, and CLI.

## Crates

| Crate | Status | Description |
|-------|--------|-------------|
| [`openjd-expr`](crates/openjd-expr) | Beta | Expression language: type system, ruff-based parser, bounded evaluator, range expressions, path mapping |
| [`openjd-model`](crates/openjd-model) | Beta | Core library: template parsing, validation, job creation, parameter spaces, step dependency graphs |
| [`openjd-sessions`](crates/openjd-sessions) | Beta | Session runtime: async action execution, environment lifecycle, cross-user subprocess management, structured logging |
| [`openjd-snapshots`](crates/openjd-snapshots) | **Experimental** | Job attachments: content-addressed file tree snapshots, xxHash3 hashing, manifest diffing, S3 upload/download |
| [`openjd-cli`](crates/openjd-cli) | Beta | CLI binary (`openjd`): `check` and `run` commands |
| [`openjd-for-js`](crates/openjd-for-js) | **Experimental** | ECMAScript/WebAssembly bindings for `openjd-model` and `openjd-expr` |

The "Beta" crates are released to crates.io via
[release-plz](https://release-plz.dev). They are still on the 0.x
version line, so breaking changes are possible in minor-version bumps
per Cargo's [pre-1.0 semver rules][pre-1.0]. The **experimental** crates
are under active development and their public APIs may change without
notice. For `openjd-snapshots`, the v2025 on-disk manifest format is a
**draft** — the v2023 format is what AWS Deadline Cloud uses and is
stable. `openjd-for-js` is built as an npm package and is not on
crates.io.

[pre-1.0]: https://doc.rust-lang.org/cargo/reference/semver.html

## Building

```bash
cargo build --release
```

## Usage

```bash
# Validate a template
./target/release/openjd check path/to/template.yaml

# Run a job template locally
./target/release/openjd run path/to/template.yaml -p Key=Value
./target/release/openjd run path/to/template.yaml -p file://params.yaml
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

This runs 12 cross-user tests covering subprocess execution, signal delivery (SIGTERM/SIGKILL), process tree killing, session cleanup, and TempDir permissions. See [specs/sessions/cross-user-testing.md](specs/sessions/cross-user-testing.md) for details.

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

The crate dependency graph (normal dependencies between workspace crates):

```
openjd-cli
├── openjd-sessions
│   ├── openjd-model
│   │   └── openjd-expr
│   └── openjd-expr
├── openjd-model
└── openjd-expr

openjd-for-js (experimental)
├── openjd-model
└── openjd-expr

openjd-snapshots (experimental, standalone)
```

Design documents are in [`specs/`](specs/). Start with
[architecture.md](specs/architecture.md) for the project layout, then the crate's spec index:

| Document | Topic |
|----------|-------|
| [architecture.md](specs/architecture.md) | Overall crate structure and design |
| [expr/README.md](specs/expr/README.md) | `openjd-expr`: type system, parser, evaluator, format strings, function library |
| [model/README.md](specs/model/README.md) | `openjd-model`: template types, parsing, validation, job creation |
| [sessions/README.md](specs/sessions/README.md) | `openjd-sessions`: session runtime, subprocesses, cross-user execution |
| [cli/README.md](specs/cli/README.md) | `openjd-cli`: commands and output formatting |
| [snapshots/README.md](specs/snapshots/README.md) | `openjd-snapshots`: snapshot operations and manifest formats |
| [openjd-for-js/spec.md](specs/openjd-for-js/spec.md) | `openjd-for-js`: ECMAScript/WebAssembly bindings |
| [job-attachments-snapshots.md](specs/job-attachments-snapshots.md) | Snapshot file format and operations |

Some frequently used crate documents:

| Document | Topic |
|----------|-------|
| [sessions/architecture.md](specs/sessions/architecture.md) | Sessions runtime: modules, data flow, cancelation |
| [sessions/action-messages.md](specs/sessions/action-messages.md) | Real-time stdout message streaming |
| [model/parsing.md](specs/model/parsing.md) | Template parsing pipeline |
| [model/validation.md](specs/model/validation.md) | Validation rules and error reporting |
| [model/parameters.md](specs/model/parameters.md) | Parameter type system |
| [model/job-creation.md](specs/model/job-creation.md) | Job instantiation from templates |
| [expr/format-string.md](specs/expr/format-string.md) | Format string resolution |
| [expr/architecture.md](specs/expr/architecture.md) | Expression language implementation |
| [expr/function-library.md](specs/expr/function-library.md) | Built-in function library |

## Security

We take all security reports seriously. When we receive such reports, we will
investigate and subsequently address any potential vulnerabilities as quickly
as possible. If you discover a potential security issue in this project, please
notify AWS/Amazon Security via our [vulnerability reporting page](http://aws.amazon.com/security/vulnerability-reporting/)
or directly via email to [AWS Security](aws-security@amazon.com). Please do not
create a public GitHub issue in this project.

## License

This project is licensed under the Apache-2.0 License or the MIT License, at your option.
