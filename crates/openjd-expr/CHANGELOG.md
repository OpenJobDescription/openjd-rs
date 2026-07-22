# Changelog

All notable changes to this crate are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).
## [0.2.1](https://github.com/OpenJobDescription/openjd-rs/compare/openjd-expr-v0.2.0...openjd-expr-v0.2.1) - 2026-07-22

### Bug fixes

- Harden arithmetic against overflow, saturation, and unbudgeted allocation  ([#272](https://github.com/OpenJobDescription/openjd-rs/pull/272))


## [0.2.0](https://github.com/OpenJobDescription/openjd-rs/compare/openjd-expr-v0.1.2...openjd-expr-v0.2.0) - 2026-07-15

### Bug fixes

- Sort all_paths output so SymbolTable serialization is canonical


### Features

- Implement PartialEq and Hash for instantiated job types


### Refactor

- [**breaking**] Close out remaining expr report recommendations


## [0.1.2](https://github.com/OpenJobDescription/openjd-rs/compare/openjd-expr-v0.1.1...openjd-expr-v0.1.2) - 2026-05-28

### Bug fixes

- Align path operators and stem/suffix parsing with Python pathlib


## [0.1.1](https://github.com/OpenJobDescription/openjd-rs/compare/openjd-expr-v0.1.0...openjd-expr-v0.1.1) - 2026-05-20

### Bug fixes

- Accept union target_type via match-or-coerce

- Evaluate operator operands with unconstrained target type

