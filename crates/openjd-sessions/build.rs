// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

use std::path::PathBuf;

fn main() {
    let out_dir = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let manifest_dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let target = std::env::var("TARGET").unwrap();

    let helper_dir = manifest_dir.join("src/helper");
    let helper_out = out_dir.join("openjd_helper");

    // Rerun only when helper sources change. Watching the whole `src/helper/`
    // directory would include `src/helper/target/` which this script writes
    // into, causing spurious re-runs on every cargo invocation (and
    // recompilation of openjd-sessions + openjd-cli every time).
    println!(
        "cargo:rerun-if-changed={}",
        helper_dir.join("Cargo.toml").display()
    );
    println!(
        "cargo:rerun-if-changed={}",
        helper_dir.join("Cargo.lock").display()
    );
    println!(
        "cargo:rerun-if-changed={}",
        helper_dir.join("src").display()
    );

    let is_unix = target.contains("linux") || target.contains("unix") || cfg!(unix);
    let is_windows = target.contains("windows") || cfg!(windows);

    if is_unix || is_windows {
        let status = std::process::Command::new("cargo")
            .args([
                "build",
                "--release",
                "--manifest-path",
                &helper_dir.join("Cargo.toml").to_string_lossy(),
                "--target-dir",
                &out_dir.join("helper_build").to_string_lossy(),
                "--target",
                &target,
            ])
            .status()
            .expect("Failed to run cargo for helper binary");
        assert!(status.success(), "Helper binary compilation failed");

        let binary_name = if is_windows {
            "openjd_helper.exe"
        } else {
            "openjd_helper"
        };
        let built = out_dir
            .join("helper_build")
            .join(&target)
            .join("release")
            .join(binary_name);
        std::fs::copy(&built, &helper_out).expect("Failed to copy helper binary");

        // Expose the built helper binary path to integration tests via
        // env!("OPENJD_HELPER_BINARY_PATH"). This keeps the helper binary
        // inside OUT_DIR where it belongs, instead of copying it back into
        // the source tree (which would dirty the cargo fingerprint on every
        // run).
        println!(
            "cargo:rustc-env=OPENJD_HELPER_BINARY_PATH={}",
            built.display()
        );
    } else {
        // Unsupported platform: write empty placeholder so include_bytes! doesn't fail
        std::fs::write(&helper_out, b"").expect("Failed to write placeholder");
        // Still set the env var so env!() compiles; tests that need the helper
        // will skip on unsupported platforms.
        println!(
            "cargo:rustc-env=OPENJD_HELPER_BINARY_PATH={}",
            helper_out.display()
        );
    }
}
