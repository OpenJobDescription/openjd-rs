# openjd-expr

[![crates.io](https://img.shields.io/crates/v/openjd-expr.svg)](https://crates.io/crates/openjd-expr)
[![docs.rs](https://img.shields.io/docsrs/openjd-expr)](https://docs.rs/openjd-expr)
[![license](https://img.shields.io/crates/l/openjd-expr.svg)](https://github.com/OpenJobDescription/openjd-rs#license)

Expression language for [Open Job Description] (OpenJD) templates. Part of the
[`openjd-rs`] workspace — a Rust implementation of the OpenJD specification.

[Open Job Description]: https://github.com/OpenJobDescription/openjd-specifications
[`openjd-rs`]: https://github.com/OpenJobDescription/openjd-rs

## What this crate provides

- **Format string resolution** — `{{Param.Foo}}`, `{{Task.Param.Bar}}`, and
  `{{Expr.expr}}` interpolation in template fields.
- **EXPR extension expression evaluation** — a subset of Python expression
  syntax (arithmetic, comparisons, conditionals, function calls, list
  comprehensions, slicing, string operations, regex, path operations) with
  memory- and operation-bounded execution.
- **Type system** (`ExprType`, `TypeCode`) including primitives, typed lists,
  unions, type variables, and generics for function signatures.
- **Runtime values** (`ExprValue`) with float-passthrough for preserving
  original string representations during round-trips.
- **Symbol tables** with hierarchical key-value lookup and dotted paths
  (`Param.Frame`).
- **Range expressions** (`1-10`, `1-100:10`, `1,5,10-20`).
- **Path mapping** — applying source→destination path rules across
  platforms.

The parser is built on [`ruff_python_parser`], the same Python parser Astral
uses in Ruff and the OpenJD spec recommends for Rust implementations.

[`ruff_python_parser`]: https://crates.io/crates/rustpython-ruff_python_parser

## Minimum supported Rust version

Rust **1.92**. Enforced in CI.

## Quick example

```rust
use openjd_expr::{ExprValue, ParsedExpression, SymbolTable};

let parsed = ParsedExpression::new("Param.Frame * 2 + 1").unwrap();

let mut symbols = SymbolTable::new();
symbols.set("Param.Frame", 42).unwrap();

let value = parsed.evaluate(&[&symbols]).unwrap();
assert_eq!(value, ExprValue::Int(85));
```

For format strings:

```rust
use openjd_expr::FormatString;

let fs = FormatString::new("hello {{Param.Name}}").unwrap();
```

See the [crate documentation on docs.rs][docs] for the full API, including
evaluator resource limits, the function library, and error reporting.

[docs]: https://docs.rs/openjd-expr

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
