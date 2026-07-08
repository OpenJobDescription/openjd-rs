# openjd-model

[![crates.io](https://img.shields.io/crates/v/openjd-model.svg)](https://crates.io/crates/openjd-model)
[![docs.rs](https://img.shields.io/docsrs/openjd-model)](https://docs.rs/openjd-model)
[![license](https://img.shields.io/crates/l/openjd-model.svg)](https://github.com/OpenJobDescription/openjd-rs#license)

[Open Job Description] (OpenJD) model library for Rust — parsing,
validation, and job creation for OpenJD templates. Part of the
[`openjd-rs`] workspace.

[Open Job Description]: https://github.com/OpenJobDescription/openjd-specifications
[`openjd-rs`]: https://github.com/OpenJobDescription/openjd-rs

## What this crate provides

- **Template parsing** from YAML or JSON (via `serde` +
  [`serde-saphyr`]) into strongly typed 2023-09 model structures.
- **Validation** — structural and cross-field constraint checks with
  Pydantic-compatible error paths (`steps[0] -> script -> actions -> onRun`)
  so messages match the Python reference implementation.
- **Job creation** — instantiating jobs from templates: parameter
  resolution, format-string evaluation, parameter-space iteration, step
  dependency graphs.
- **Host capability matching** — matching host requirements against
  runtime capabilities.
- **Extensions** — the `EXPR`, `FEATURE_BUNDLE_1`, `TASK_CHUNKING`, and
  `REDACTED_ENV_VARS` extensions from the 2023-09 specification.

The crate passes 100% of the upstream [OpenJD conformance test suite] —
over 700 template validation and environment-template tests on Linux,
macOS, and Windows.

[`serde-saphyr`]: https://crates.io/crates/serde-saphyr
[OpenJD conformance test suite]: https://github.com/OpenJobDescription/openjd-specifications/tree/mainline/conformance-tests

## Minimum supported Rust version

Rust **1.94.1**. Enforced in CI.

## Quick example

```rust
use openjd_model::{parse, CallerLimits, DocumentType};

let yaml = r#"
specificationVersion: jobtemplate-2023-09
name: DemoJob
steps:
  - name: DemoStep
    script:
      actions:
        onRun:
          command: python
          args: ["-c", "print('Hello')"]
"#;

// Parse YAML into an intermediate `serde_json::Value`, then validate into
// a strongly typed `JobTemplate`. On failure you get human-friendly error
// paths; on success, a fully-typed template.
let caller_limits = CallerLimits::default();
let value = parse::document_string_to_object(yaml, DocumentType::Yaml, &caller_limits)?;
let template = parse::decode_job_template(value, None, &caller_limits)?;

assert_eq!(template.name.raw(), "DemoJob");
# Ok::<(), openjd_model::ModelError>(())
```

See the [crate documentation on docs.rs][docs] for the full API, including
`create_job`, `StepParameterSpaceIterator`, and `StepDependencyGraph`.

[docs]: https://docs.rs/openjd-model

## Relationship to `openjd-expr`

`openjd-model` depends on [`openjd-expr`](https://crates.io/crates/openjd-expr)
for expression parsing, format-string resolution, and symbol tables.
`FormatString` and `SymbolTable` are re-exported at the crate root for
convenience.

## Versioning

This crate follows [Semantic Versioning](https://semver.org/). While the
major version is `0.x`, the public API may have breaking changes in minor
version bumps, in keeping with Cargo's
[pre-1.0 semver rules][pre-1.0]. Each crate in the `openjd-rs` workspace is
versioned independently.

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
