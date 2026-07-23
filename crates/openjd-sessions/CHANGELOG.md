# Changelog

All notable changes to this crate are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.4.1](https://github.com/OpenJobDescription/openjd-rs/compare/openjd-sessions-v0.4.0...openjd-sessions-v0.4.1) - 2026-07-23

### Bug fixes

- Close remaining RFC 0008 wrap-action gaps in hook scoping, cleanup, and preflight


## [0.4.0](https://github.com/OpenJobDescription/openjd-rs/compare/openjd-sessions-v0.3.2...openjd-sessions-v0.4.0) - 2026-07-22

### Bug fixes

- Close top three RFC 0008 wrap-action review gaps ([#265](https://github.com/OpenJobDescription/openjd-rs/pull/265))

- Address post-merge review comments on PR #261 ([#264](https://github.com/OpenJobDescription/openjd-rs/pull/264))

- Cap helper cancel notify period at the action's declared terminate_delay ([#258](https://github.com/OpenJobDescription/openjd-rs/pull/258))

- Forward WrappedAction.Cancelation.* to wrap hooks

- [**breaking**] Cancel handle, setup-failure reporting, plain filename, let scope


## [0.3.2](https://github.com/OpenJobDescription/openjd-rs/compare/openjd-sessions-v0.3.1...openjd-sessions-v0.3.2) - 2026-07-15

### Bug fixes

- Sort all_paths output so SymbolTable serialization is canonical


## [0.3.1](https://github.com/OpenJobDescription/openjd-rs/compare/openjd-sessions-v0.3.0...openjd-sessions-v0.3.1) - 2026-07-02

### Bug fixes

- Retry LookupAccountNameW with exponential backoff on ERROR_NONE_MAPPED ([#221](https://github.com/OpenJobDescription/openjd-rs/pull/221))


### Miscellaneous

- Add helper dependencies to Cargo.toml for offline builds ([#232](https://github.com/OpenJobDescription/openjd-rs/pull/232))


## [0.3.0](https://github.com/OpenJobDescription/openjd-rs/compare/openjd-sessions-v0.2.3...openjd-sessions-v0.3.0) - 2026-06-29

### Bug fixes

- Resolve Windows Send bound and WASM extension-count test failures


### Features

- Implement RFC 0008 WRAP_ACTIONS extension

- Add support for domain users ([#219](https://github.com/OpenJobDescription/openjd-rs/pull/219))


## [0.2.3](https://github.com/OpenJobDescription/openjd-rs/compare/openjd-sessions-v0.2.2...openjd-sessions-v0.2.3) - 2026-05-28

### Bug fixes

- Tie helper grandchildren to a Windows Job Object; harden CI


## [0.2.2](https://github.com/OpenJobDescription/openjd-rs/compare/openjd-sessions-v0.2.1...openjd-sessions-v0.2.2) - 2026-05-25

### Refactor

- Make `template` a public module with typed parameter definitions


## [0.2.0](https://github.com/OpenJobDescription/openjd-rs/compare/openjd-sessions-v0.1.0...openjd-sessions-v0.2.0) - 2026-05-15

### Bug fixes

- [**breaking**] Make openjd_temp_dir's directory parameterizable

- Flaky Windows cross-user test and record_pr workflow error

- Address CodeQL security scan findings


### Features

- Add echo_openjd_directives config option


### Testing

- Assert BadCredentialsError variant mapping on Windows

