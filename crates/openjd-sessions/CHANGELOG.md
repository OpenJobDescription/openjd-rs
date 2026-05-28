# Changelog

All notable changes to this crate are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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

