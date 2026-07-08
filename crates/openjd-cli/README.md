# openjd-cli

[![crates.io](https://img.shields.io/crates/v/openjd-cli.svg)](https://crates.io/crates/openjd-cli)
[![docs.rs](https://img.shields.io/docsrs/openjd-cli)](https://docs.rs/openjd-cli)
[![license](https://img.shields.io/crates/l/openjd-cli.svg)](https://github.com/OpenJobDescription/openjd-rs#license)

Command-line tool for [Open Job Description] (OpenJD) job templates —
validation, summary, and local execution. Part of the [`openjd-rs`]
workspace.

[Open Job Description]: https://github.com/OpenJobDescription/openjd-specifications
[`openjd-rs`]: https://github.com/OpenJobDescription/openjd-rs

Installs an `openjd` binary that is drop-in compatible with the Python
[`openjd`][py-cli] CLI.

[py-cli]: https://github.com/OpenJobDescription/openjd-cli

## Installation

From crates.io:

```sh
cargo install openjd-cli
```

This places an `openjd` binary on your `PATH`.

## Commands

| Command | Purpose |
|---------|---------|
| `openjd check FILE [--extensions EXT...]` | Validate a job or environment template. Exit code 0 on success, non-zero on validation error, with per-field error paths. |
| `openjd summary FILE [-p NAME=VALUE...]` | Print a human-readable summary of a job: steps, parameters, dependency graph, estimated task count. |
| `openjd run FILE [-p NAME=VALUE...] [--step NAME] [--task-params ...]` | Instantiate and run a job template locally, streaming action output with timestamps. |

Run `openjd --help` for the full flag list, or `openjd COMMAND --help`
for a specific command.

## Examples

```sh
# Validate a template
openjd check my-template.yaml

# Validate with an extension enabled
openjd check my-template.yaml --extensions TASK_CHUNKING

# Summarize a job (expanding parameters)
openjd summary my-template.yaml -p Frames=1-100

# Run a job locally, one task at a time
openjd run my-template.yaml -p Frames=1-10
```

## Conformance

Passes 100% of the upstream [OpenJD conformance test suite][conformance]
(1,000+ tests) on Linux, macOS, and Windows.

[conformance]: https://github.com/OpenJobDescription/openjd-specifications/tree/mainline/conformance-tests

## Minimum supported Rust version

Rust **1.94.1**. Enforced in CI.

## Relationship to other crates

`openjd-cli` builds on top of the rest of the `openjd-rs` workspace:

- [`openjd-model`](https://crates.io/crates/openjd-model) — template
  parsing, validation, job creation.
- [`openjd-sessions`](https://crates.io/crates/openjd-sessions) —
  local session runtime for `run`.
- [`openjd-expr`](https://crates.io/crates/openjd-expr) — expression
  evaluation and format-string interpolation.

If you are embedding OpenJD support in your own program, depend on those
libraries directly rather than this CLI.

## Versioning

This crate follows [Semantic Versioning](https://semver.org/). While the
major version is `0.x`, the public API may have breaking changes in minor
version bumps, in keeping with Cargo's
[pre-1.0 semver rules][pre-1.0]. Each crate in the `openjd-rs` workspace
is versioned independently.

[pre-1.0]: https://doc.rust-lang.org/cargo/reference/semver.html

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-Apache-2.0] or
  <http://www.apache.org/licenses/LICENSE-2.0>)
- MIT License ([LICENSE-MIT] or <http://opensource.org/licenses/MIT>)

at your option.

[LICENSE-Apache-2.0]: LICENSE-Apache-2.0
[LICENSE-MIT]: LICENSE-MIT

### Contribution

Unless you explicitly state otherwise, any contribution intentionally
submitted for inclusion in the work by you, as defined in the Apache-2.0
license, shall be dual licensed as above, without any additional terms or
conditions.

## Security

See <https://aws.amazon.com/security/vulnerability-reporting/> or email
[AWS Security](mailto:aws-security@amazon.com) to report a potential
security issue. Please do not create a public GitHub issue.
