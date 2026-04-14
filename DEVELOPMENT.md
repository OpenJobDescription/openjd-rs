# Development documentation

This documentation provides guidance on developer workflows for working with the code in this repository.

Table of Contents:
* [Development Environment Setup](#development-environment-setup)
* [The Development Loop](#the-development-loop)
* [Testing](#testing)
* [Things to Know](#things-to-know)
   * [Workspace Structure](#workspace-structure)
   * [Coding Style Requirements](#coding-style-requirements)

## Development Environment Setup

To develop the Rust code in this repository you will need:

1. A [Rust toolchain](https://rustup.rs/) (stable channel).
2. `cargo` (included with the Rust toolchain).

You can develop on a Linux, macOS, or Windows workstation.

## The Development Loop

This is a Cargo [workspace](https://doc.rust-lang.org/book/ch14-03-cargo-workspaces.html) with the following crates:

| Crate | Description |
|---|---|
| `openjd-expr` | Expression evaluation engine |
| `openjd-model` | Template data model and validation |
| `openjd-sessions` | Session management and action running |
| `openjd-cli` | Command-line interface |
| `openjd-snapshots` | Data snapshot utilities |

Common commands:

* `cargo build --workspace` — Build all crates.
* `cargo test --workspace` — Run all tests.
* `cargo clippy --all-features --all-targets --workspace -- -D warnings` — Run lints.
* `cargo fmt --all -- --check` — Check formatting (requires nightly rustfmt for some options).
* `cargo doc --no-deps --workspace` — Build documentation.

## Testing

All tests are located in `tests/` directories within each crate, or as `#[cfg(test)]` modules
within source files. If you are adding or modifying functionality, then you will almost always
want to be writing one or more tests to demonstrate that your logic behaves as expected and that
future changes do not accidentally break your change.

### Running Tests

```sh
# Run all tests
cargo test --workspace

# Run tests for a specific crate
cargo test -p openjd-model

# Run a specific test
cargo test -p openjd-model test_name
```

## Things to Know

### Workspace Structure

```
openjd-rs/
├── Cargo.toml          # Workspace root
├── crates/
│   ├── openjd-expr/
│   ├── openjd-model/
│   ├── openjd-sessions/
│   ├── openjd-cli/
│   └── openjd-snapshots/
```

### Coding Style Requirements

* Run `cargo fmt` before committing.
* All public items should have documentation comments (`///`).
* Use `clippy` and resolve all warnings before submitting a PR.
* Prefer returning `Result` types over panicking.
