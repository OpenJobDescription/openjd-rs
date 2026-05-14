# Changelog

All notable changes to this crate are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).
## [0.2.0](https://github.com/OpenJobDescription/openjd-rs/compare/0.1.0...0.2.0) - 2026-05-14

### Bug fixes

- [**breaking**] Make openjd_temp_dir's directory parameterizable

- Flaky Windows cross-user test and record_pr workflow error

- Address CodeQL security scan findings

- Allow `cargo publish` by renaming nested helper manifest

- Shrink post-terminate c.wait() grace to 2s

- Harden openjd-sessions per security review recommendations

- Add 128KB line length limit to helper protocol reader

- Add YAML document depth limit and fix Windows stack overflow

- Box::pin runner futures to avoid Windows stack overflow

- [**breaking**] Remove shell script fallback for cross-user execution

- Validate embedded file filenames for path traversal

- Harden cross-user helper and wrapper script paths against TOCTOU

- Interleave directory creation with file download submission

- Resolve quality report findings and port Python chunking tests

- Address quality report issues and fix PathParameterOptions API

- P-3, P-4 from quality report and flaky macOS CI test

- Handle \r\n on Windows line processing

- Make NTT cancel test robust against shell trap variance

- Cancelation flows for session helper subprocess

- Align helper cancel protocol with OpenJD spec, fix Windows CI flake

- Resolve CI test failures on macOS and Windows

- Fix openjd-sessions quality issues from evaluation report

- Windows cross-user helper stdin inheritance and timeout handling

- Make the cross-user windows helper Send

- Address sessions quality report findings

- Resolve all clippy warnings, rustfmt, and rustdoc issues

- Address all findings from quality evaluation


### CI

- Add copyright header and THIRD-PARTY-LICENSES freshness checks

- Reduce Windows CI time and fix masked test failure

- Add dry-run release-plz automation for crates.io publishing


### Documentation

- Document safety invariants of Windows unsafe impl blocks

- Fix sessions spec alignment and quality issues

- Align openjd-expr specs with implementation (report §1-§3)


### Features

- Add echo_openjd_directives config option

- Rename CLI binary to `openjd`; mark snapshots and for-js experimental; add per-crate READMEs

- Introduce ExprProfile and FunctionLibrary::for_profile

- Authenticate cross-user helper requests with a shared token

- Harden Windows helper binary DACL and expand DACL test parity

- Set OPENJD_SESSION_WORKING_DIR env var in subprocesses

- Add CallerLimits for caller-imposed template limits

- Standardize error handling across all openjd-rs crates

- Async helper I/O, cancel fixes, remove CAP_KILL

- Windows cross-user helper and path_format parameter

- Use agentic workflow to port Python OpenJD to Rust


### Miscellaneous

- Switch to dual Apache-2.0 OR MIT license with attribution tooling

- Update Cargo dependencies to latest versions

- Fix clippy warnings on Windows


### Performance

- Consolidate each crate's integration tests into a single binary

- Eliminate redundant workspace recompiles in Build & Test


### Refactor

- Introduce ModelProfile and tighten extension handling

- Priority 2 — thread ValidationContext, revision dispatch, remove deprecated expr APIs

- Replace serde_yaml with serde-saphyr

- Simplify format string & apply_path_mapping plumbing

- Openjd-expr API ergonomics (report §4 friction points 1, 5, 6, 7, 8)


### Testing

- Assert BadCredentialsError variant mapping on Windows

- Add cross-user Windows helper tests

- Port permission validation tests from Python openjd-sessions

- Increase margins on time-sensitive cancelation tests

- Update crate quality evaluation reports

