// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! On MSVC, link libFuzzer's `main` into the fuzz binaries.
//!
//! The MSVC linker only pulls `main` out of the libFuzzer `.rlib` when told
//! to with `/include:main`. cargo-fuzz adds that flag to `RUSTFLAGS`, which
//! reaches every crate, and a dependency that also builds a DLL (`crc-fast`,
//! through `openjd-snapshots`) then fails to link: a DLL has no `main`.
//! `scripts/fuzz.sh` passes `--no-include-main-msvc` on Windows, and this
//! adds the flag back for this package's binaries only.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    let msvc = std::env::var("CARGO_CFG_TARGET_ENV").is_ok_and(|env| env == "msvc");
    // Only the libFuzzer build has an external `main`; the stable replay
    // (`--no-default-features`) builds no binaries.
    let libfuzzer = std::env::var_os("CARGO_FEATURE_LIBFUZZER").is_some();
    if msvc && libfuzzer {
        println!("cargo:rustc-link-arg-bins=/include:main");
    }
}
