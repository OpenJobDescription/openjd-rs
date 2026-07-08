# openjd-sessions

[![crates.io](https://img.shields.io/crates/v/openjd-sessions.svg)](https://crates.io/crates/openjd-sessions)
[![docs.rs](https://img.shields.io/docsrs/openjd-sessions)](https://docs.rs/openjd-sessions)
[![license](https://img.shields.io/crates/l/openjd-sessions.svg)](https://github.com/OpenJobDescription/openjd-rs#license)

Local session runtime for [Open Job Description] (OpenJD) jobs — the Rust
counterpart of the Python [`openjd-sessions`][py-sessions] library. Part of
the [`openjd-rs`] workspace.

[Open Job Description]: https://github.com/OpenJobDescription/openjd-specifications
[py-sessions]: https://github.com/OpenJobDescription/openjd-sessions-for-python
[`openjd-rs`]: https://github.com/OpenJobDescription/openjd-rs

## What this crate provides

- **Session lifecycle** (`Session`, `SessionConfig`) — enter environments,
  run task actions, tear down in the correct order.
- **Subprocess management** — async (tokio) process spawning, streaming
  I/O, signal handling (SIGTERM/SIGKILL), and whole-process-tree teardown.
- **Action-message filter** — real-time parsing, redaction, and
  annotation of `openjd_*` messages emitted on stdout/stderr.
- **Cross-user execution** — running subprocesses as a different OS user
  on Linux/macOS (via `sudo`) and on Windows (via a logon-based helper
  binary), with an **embedded helper** built by `build.rs` and baked
  into the library.
- **Embedded files** — staging `embeddedFiles` entries to disk with the
  right permissions.
- **TempDir** — per-session scratch space with safe cleanup on all
  platforms.

## Platform support

- **Linux** — primary target. Cross-user execution requires `sudo`.
- **macOS** — supported; no cross-user support.
- **Windows** — supported, including cross-user execution via the
  embedded helper (logon-token based, mirroring the Python
  reference implementation).

## Minimum supported Rust version

Rust **1.94.1**. Enforced in CI.

## Quick example

Running a task action from a decoded job template:

```rust,no_run
use openjd_sessions::{Session, SessionConfig};
use std::sync::Arc;

# async fn run() -> Result<(), Box<dyn std::error::Error>> {
// Construct a SessionConfig (normally derived from a job template +
// task parameters via openjd-model — see the `openjd-rs` CLI for a full
// end-to-end example).
let config: SessionConfig = /* ... */ unimplemented!();

let session = Session::new(config).await?;
// Enter environments, run actions, collect results...
# Ok(()) }
```

The full end-to-end integration — parsing a template, instantiating a
job, and running it locally — lives in the
[`openjd-cli`](https://crates.io/crates/openjd-cli) crate.

See the [crate documentation on docs.rs][docs] for the session state
machine, action lifecycle, and the embedded-files staging API.

[docs]: https://docs.rs/openjd-sessions

## Relationship to other crates

- Depends on [`openjd-model`](https://crates.io/crates/openjd-model) for
  job-template types.
- Depends on [`openjd-expr`](https://crates.io/crates/openjd-expr) for
  expression evaluation and path mapping (re-exported at
  `openjd_sessions::path_mapping`).

## Versioning

This crate follows [Semantic Versioning](https://semver.org/). While the
major version is `0.x`, the public API may have breaking changes in minor
version bumps, in keeping with Cargo's
[pre-1.0 semver rules][pre-1.0]. Each crate in the `openjd-rs` workspace
is versioned independently.

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
