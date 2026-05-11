// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

use std::path::{Path, PathBuf};

fn copy_dir_recursive(src: &Path, dst: &Path) {
    std::fs::create_dir_all(dst).unwrap();
    for entry in std::fs::read_dir(src).unwrap() {
        let entry = entry.unwrap();
        let ty = entry.file_type().unwrap();
        let dest_path = dst.join(entry.file_name());
        if ty.is_dir() {
            if entry.file_name() == "target" {
                continue;
            }
            copy_dir_recursive(&entry.path(), &dest_path);
        } else {
            std::fs::copy(entry.path(), &dest_path).unwrap();
        }
    }
}

fn main() {
    let out_dir = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let manifest_dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let target = std::env::var("TARGET").unwrap();

    let helper_dir = manifest_dir.join("src/helper");
    let helper_out = out_dir.join("openjd_helper");

    // Rerun only when helper sources change.
    println!(
        "cargo:rerun-if-changed={}",
        helper_dir.join("Cargo.bundled.toml").display()
    );
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
        // Copy the helper source tree into OUT_DIR so we never write into the
        // source tree. The manifest is stored as `Cargo.bundled.toml` so that
        // cargo doesn't treat src/helper/ as a separate crate during packaging
        // (its check matches the literal filename `Cargo.toml`), while keeping
        // the `.toml` extension so editor TOML tooling still works.
        // If `Cargo.toml` exists (local dev before rename), use it as-is.
        let build_src = out_dir.join("helper_src");
        copy_dir_recursive(&helper_dir, &build_src);

        let manifest_in_build = build_src.join("Cargo.toml");
        if !manifest_in_build.exists() {
            let bundled = build_src.join("Cargo.bundled.toml");
            std::fs::rename(&bundled, &manifest_in_build)
                .expect("Failed to rename Cargo.bundled.toml to Cargo.toml");
        }

        let status = std::process::Command::new("cargo")
            .args([
                "build",
                "--release",
                "--manifest-path",
                &manifest_in_build.to_string_lossy(),
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

        println!(
            "cargo:rustc-env=OPENJD_HELPER_BINARY_PATH={}",
            built.display()
        );
    } else {
        std::fs::write(&helper_out, b"").expect("Failed to write placeholder");
        println!(
            "cargo:rustc-env=OPENJD_HELPER_BINARY_PATH={}",
            helper_out.display()
        );
    }
}
