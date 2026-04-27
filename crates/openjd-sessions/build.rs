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

    println!("cargo:rerun-if-changed=src/helper/");

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

        // Also place the binary where integration tests expect it
        // (tests can't access OUT_DIR, so they look in the helper's own target dir)
        let test_dir = helper_dir.join("target/release");
        std::fs::create_dir_all(&test_dir).expect("Failed to create helper test dir");
        std::fs::copy(&built, test_dir.join(binary_name))
            .expect("Failed to copy helper binary for tests");
    } else {
        // Unsupported platform: write empty placeholder so include_bytes! doesn't fail
        std::fs::write(&helper_out, b"").expect("Failed to write placeholder");
    }
}
