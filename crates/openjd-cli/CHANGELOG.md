# Changelog

All notable changes to this crate are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).
## [0.2.0](https://github.com/OpenJobDescription/openjd-rs/compare/0.1.0...0.2.0) - 2026-05-14

### Bug fixes

- Harden file input handling in run, check, summary, help

- Harden openjd-sessions per security review recommendations

- Add YAML document depth limit and fix Windows stack overflow

- Resolve quality report findings and port Python chunking tests

- Validate parameter space associations at create_job time

- Address quality report issues and fix PathParameterOptions API

- Fix openjd-sessions quality issues from evaluation report

- Address model evaluation report findings

- Resolve all clippy warnings, rustfmt, and rustdoc issues


### CI

- Add dry-run release-plz automation for crates.io publishing


### Documentation

- Fix sessions spec alignment and quality issues


### Features

- Add echo_openjd_directives config option

- Rename CLI binary to `openjd`; mark snapshots and for-js experimental; add per-crate READMEs

- Introduce ExprProfile and FunctionLibrary::for_profile

- [**breaking**] Align SymbolTable + supportedExtensions with Python bindings

- Add CallerLimits for caller-imposed template limits

- Windows cross-user helper and path_format parameter

- Use agentic workflow to port Python OpenJD to Rust


### Miscellaneous

- Switch to dual Apache-2.0 OR MIT license with attribution tooling


### Refactor

- Introduce ModelProfile and tighten extension handling

- Priority 2 — thread ValidationContext, revision dispatch, remove deprecated expr APIs

- Replace serde_yaml with serde-saphyr

- Simplify format string & apply_path_mapping plumbing

- Openjd-expr API ergonomics (report §4 friction points 1, 5, 6, 7, 8)

