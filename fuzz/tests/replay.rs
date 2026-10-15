// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Replay every committed input through its target on stable Rust, and
//! check that the harness is wired consistently.
//!
//! `cargo test --manifest-path fuzz/Cargo.toml --no-default-features`
//!
//! - `corpus/<target>/` — the committed seed corpus (minimized).
//! - `regressions/<target>/` — inputs that once found a bug. Each must keep
//!   passing; a fix lands together with its reproducer here.

use openjd_fuzz::targets::TARGETS;
use std::collections::BTreeSet;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn files_in(dir: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|rd| {
            rd.filter_map(|e| e.ok().map(|e| e.path()))
                .filter(|p| p.is_file())
                .collect()
        })
        .unwrap_or_default();
    v.sort();
    v
}

fn names(it: impl IntoIterator<Item = String>) -> BTreeSet<String> {
    it.into_iter().collect()
}

#[test]
fn overflow_checks_are_enabled() {
    let r = catch_unwind(|| std::hint::black_box(i64::MAX) + std::hint::black_box(1));
    assert!(
        r.is_err(),
        "overflow checks are off: every arithmetic-overflow oracle is disarmed"
    );
}

#[test]
fn registry_matches_bins_files_and_corpus() {
    let registry = names(TARGETS.iter().map(|(n, _)| n.to_string()));
    assert_eq!(registry.len(), TARGETS.len(), "duplicate target names");

    let manifest = std::fs::read_to_string(root().join("Cargo.toml")).unwrap();
    let bins = names(
        manifest
            .lines()
            .filter_map(|l| l.strip_prefix("name = \""))
            .filter_map(|l| l.strip_suffix('"'))
            .filter(|n| *n != "openjd-fuzz")
            .map(str::to_string),
    );
    assert_eq!(
        registry, bins,
        "targets::TARGETS vs [[bin]] entries in Cargo.toml"
    );

    let shims = names(
        files_in(&root().join("fuzz_targets"))
            .iter()
            .filter_map(|p| p.file_stem()?.to_str().map(str::to_string)),
    );
    assert_eq!(registry, shims, "targets::TARGETS vs fuzz_targets/*.rs");

    for (name, _) in TARGETS {
        let shim = std::fs::read_to_string(root().join(format!("fuzz_targets/{name}.rs"))).unwrap();
        assert!(
            shim.contains("openjd_fuzz::targets::"),
            "fuzz_targets/{name}.rs does not call an openjd_fuzz::targets function"
        );
    }

    for (name, _) in TARGETS {
        let dir = root().join("corpus").join(name);
        assert!(
            !files_in(&dir).is_empty(),
            "corpus/{name}/ is missing or empty; run `scripts/fuzz.sh corpus`"
        );
    }
}

thread_local! {
    static LAST_PANIC: std::cell::RefCell<String> = const { std::cell::RefCell::new(String::new()) };
    static IN_REPLAY: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Install (once, process-wide) a panic hook that records a replayed
/// input's panic instead of printing it, so a failing replay reports one
/// line per input. Panics outside a replay go to the default hook.
fn install_hook() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let default = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            if !IN_REPLAY.with(|r| r.get()) {
                return default(info);
            }
            let msg = info
                .payload()
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| info.payload().downcast_ref::<&str>().map(|s| s.to_string()))
                .unwrap_or_default();
            let at = info.location().map(|l| l.to_string()).unwrap_or_default();
            LAST_PANIC.with(|p| *p.borrow_mut() = format!("{msg} (at {at})"));
        }));
    });
}

fn replay(sub: &str) {
    // `OPENJD_FUZZ_REPLAY_DIR` replays another corpus tree (for example a
    // fuzzing run's grown corpus) instead of the committed one, and
    // `OPENJD_FUZZ_TARGET` restricts the replay to one target.
    let base = match std::env::var_os("OPENJD_FUZZ_REPLAY_DIR") {
        Some(d) if sub == "corpus" => PathBuf::from(d),
        _ => root().join(sub),
    };
    let only = std::env::var("OPENJD_FUZZ_TARGET").ok();
    install_hook();
    let mut failures = Vec::new();
    let mut count = 0;
    for (name, target) in TARGETS {
        if only.as_deref().is_some_and(|o| o != *name) {
            continue;
        }
        for file in files_in(&base.join(name)) {
            let data = std::fs::read(&file).unwrap();
            count += 1;
            IN_REPLAY.with(|r| r.set(true));
            let result = catch_unwind(AssertUnwindSafe(|| target(&data)));
            IN_REPLAY.with(|r| r.set(false));
            if result.is_err() {
                let msg = LAST_PANIC.with(|p| p.borrow().clone());
                failures.push(format!("{}: {msg}", file.display()));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "{} of {count} {sub} inputs failed:\n{}",
        failures.len(),
        failures
            .iter()
            .take(60)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    );
}

#[test]
fn replay_corpus() {
    replay("corpus");
}

#[test]
fn replay_regressions() {
    replay("regressions");
}

#[test]
fn expression_dictionary_covers_the_library() {
    let dict = std::fs::read_to_string(root().join("dicts/expr.dict")).unwrap();
    let missing: Vec<String> = openjd_fuzz::gen::expr::library_tokens()
        .into_iter()
        .filter(|t| !dict.contains(&format!("\"{t}\"")))
        .collect();
    assert!(
        missing.is_empty(),
        "dicts/expr.dict is missing library names {missing:?}; run `scripts/fuzz.sh dict`"
    );
}
