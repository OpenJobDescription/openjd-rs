# Changelog

All notable changes to this crate are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).
## [0.5.2](https://github.com/OpenJobDescription/openjd-rs/compare/openjd-model-v0.5.1...openjd-model-v0.5.2) - 2026-08-21

### Bug fixes

- Coerce list parameter elements to their declared element type ([#335](https://github.com/OpenJobDescription/openjd-rs/pull/335))

- Accept the `let` wire key when deserializing job-side step scripts ([#334](https://github.com/OpenJobDescription/openjd-rs/pull/334))

- Coerce parameter values to their declared types when seeding the symbol table ([#330](https://github.com/OpenJobDescription/openjd-rs/pull/330))


## [0.5.1](https://github.com/OpenJobDescription/openjd-rs/compare/openjd-model-v0.5.0...openjd-model-v0.5.1) - 2026-08-14

### Bug fixes

- Do not cap <IntRangeExpr> expansion at the list-form limit


## [0.5.0](https://github.com/OpenJobDescription/openjd-rs/compare/openjd-model-v0.4.0...openjd-model-v0.5.0) - 2026-08-13

### Bug fixes

- Accept Task.File property access in format strings ([#292](https://github.com/OpenJobDescription/openjd-rs/pull/292))


### Features

- [**breaking**] Bounded range values, budgeted range equality, exact int/float comparison  ([#276](https://github.com/OpenJobDescription/openjd-rs/pull/276))


### Miscellaneous

- Update serde-saphyr to 1.0.1 ([#304](https://github.com/OpenJobDescription/openjd-rs/pull/304))


## [0.4.0](https://github.com/OpenJobDescription/openjd-rs/compare/openjd-model-v0.3.1...openjd-model-v0.4.0) - 2026-07-22

### Bug fixes

- Close top three RFC 0008 wrap-action review gaps ([#265](https://github.com/OpenJobDescription/openjd-rs/pull/265))

- Address post-merge review comments on PR #261 ([#264](https://github.com/OpenJobDescription/openjd-rs/pull/264))

- Forward WrappedAction.Cancelation.* to wrap hooks

- [**breaking**] Cancel handle, setup-failure reporting, plain filename, let scope


## [0.3.1](https://github.com/OpenJobDescription/openjd-rs/compare/openjd-model-v0.3.0...openjd-model-v0.3.1) - 2026-07-15

### Bug fixes

- Reject sibling-dir escape in PATH default walk-up guard


### Features

- Add SpecificationRevision::CURRENT and ModelProfile::current/latest

- Validate format strings and extensions in environment templates

- Implement PartialEq and Hash for instantiated job types


## [0.3.0](https://github.com/OpenJobDescription/openjd-rs/compare/openjd-model-v0.2.1...openjd-model-v0.3.0) - 2026-06-29

### Features

- Implementation for RFC008 Wrap Actions Comments

- Implement RFC 0008 WRAP_ACTIONS extension


## [0.2.1](https://github.com/OpenJobDescription/openjd-rs/compare/openjd-model-v0.2.0...openjd-model-v0.2.1) - 2026-05-28

### Miscellaneous

- Updated the following local packages: openjd-expr


## [0.2.0](https://github.com/OpenJobDescription/openjd-rs/compare/openjd-model-v0.1.1...openjd-model-v0.2.0) - 2026-05-25

### Bug fixes

- Correct AssociationNode containment for nested expressions


### Features

- Expose typed TaskParameterDefinition variants and userInterface types


### Refactor

- Make `template` a public module with typed parameter definitions


## [0.1.1](https://github.com/OpenJobDescription/openjd-rs/compare/openjd-model-v0.1.0...openjd-model-v0.1.1) - 2026-05-20

### Features

- Add StepParameterSpaceIterator::reset and Send+Sync NodeIterator bound

