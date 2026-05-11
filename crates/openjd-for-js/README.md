# openjd-for-js

> **Status: experimental.** This crate is under active development and is
> **not published** to crates.io or to npm (`publish = false`). The JS
> surface, the underlying Rust API, the WASM package layout, and the
> build pipeline may all change without notice. Not recommended for
> production use.

ECMAScript/WebAssembly bindings for [Open Job Description] (OpenJD)
template validation, job creation, and expression evaluation. Part of
the [`openjd-rs`] workspace.

[Open Job Description]: https://github.com/OpenJobDescription/openjd-specifications
[`openjd-rs`]: https://github.com/OpenJobDescription/openjd-rs

## What this crate provides

Compiles [`openjd-model`](https://crates.io/crates/openjd-model) and
[`openjd-expr`](https://crates.io/crates/openjd-expr) to
`wasm32-unknown-unknown` and produces JS + TypeScript declarations via
[`wasm-bindgen`]. Intended use cases:

- Template viewers and editors that validate and visualize OpenJD
  templates in the browser.
- VS Code / IDE extensions for inline template diagnostics.
- CI/CD tools that validate templates without requiring a Python or Rust
  toolchain.
- Web-based job submission portals with client-side validation.

The Rust logic is the same engine that powers the CLI and the planned
Python bindings, so JS consumers get authoritative,
automatically-maintained spec behavior instead of reimplementing the
spec in ECMAScript.

[`wasm-bindgen`]: https://crates.io/crates/wasm-bindgen

## Build

```sh
# From the workspace root:
cd crates/openjd-for-js
npm run build        # cargo build --target wasm32-unknown-unknown + wasm-bindgen
npm test             # vitest
```

The build produces an npm-style package in `pkg/` with `.wasm`, `.js`,
and `.d.ts` files.

## Minimum supported Rust version

Rust **1.92**. Enforced in CI.

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
