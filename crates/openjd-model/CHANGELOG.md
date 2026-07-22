# Changelog

All notable changes to this crate are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).
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

