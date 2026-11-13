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

/// cargo-fuzz builds with the release profile, which this test (dev
/// profile) doesn't use, so check that profile's settings directly: the
/// release profile turns both on, and no profile section (per-package
/// overrides included) turns either off.
#[test]
fn release_profile_keeps_overflow_checks() {
    let manifest =
        std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml")).unwrap();
    let mut section = String::new();
    let mut release_on = BTreeSet::new();
    for line in manifest.lines() {
        let line: String = line
            .split('#')
            .next()
            .unwrap()
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect();
        if line.starts_with('[') {
            section = line;
            continue;
        }
        for key in ["overflow-checks", "debug-assertions"] {
            if line == format!("{key}=true") && section == "[profile.release]" {
                release_on.insert(key);
            }
            assert!(
                !(section.starts_with("[profile.")
                    && line.starts_with(&format!("{key}="))
                    && line != format!("{key}=true")),
                "fuzz/Cargo.toml {section} sets `{line}`: fuzz builds would silently disarm \
                 the overflow oracles"
            );
        }
    }
    for key in ["overflow-checks", "debug-assertions"] {
        assert!(
            release_on.contains(key),
            "fuzz/Cargo.toml [profile.release] must set `{key} = true`"
        );
    }
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

/// `(name, version)` of every package in a `Cargo.lock`.
fn locked_packages(path: &Path) -> BTreeSet<(String, String)> {
    let text = std::fs::read_to_string(path).unwrap();
    let mut out = BTreeSet::new();
    let mut name = None;
    for line in text.lines() {
        if let Some(n) = line.strip_prefix("name = \"") {
            name = Some(n.trim_end_matches('"').to_string());
        } else if let Some(v) = line.strip_prefix("version = \"") {
            if let Some(n) = name.take() {
                out.insert((n, v.trim_end_matches('"').to_string()));
            }
        }
    }
    out
}

/// The fuzz workspace has its own lock. Every package it shares with the
/// workspace must be locked at a version the workspace also uses, so the
/// harness tests the dependency versions the crates ship with. Fix a
/// failure with `scripts/fuzz.sh sync-lock`.
#[test]
fn fuzz_lock_matches_workspace_lock() {
    let ws = locked_packages(&root().join("../Cargo.lock"));
    let ws_names = names(ws.iter().map(|(n, _)| n.clone()));
    let drift: Vec<_> = locked_packages(&root().join("Cargo.lock"))
        .into_iter()
        .filter(|(n, _)| ws_names.contains(n))
        .filter(|p| !ws.contains(p))
        .collect();
    assert!(
        drift.is_empty(),
        "fuzz/Cargo.lock pins versions the workspace lock doesn't \
         (run scripts/fuzz.sh sync-lock): {drift:?}"
    );
}

/// Every target is fuzzed in CI: the `fuzz` job runs `scripts/fuzz.sh run`
/// with a target list that only a manual run's `targets` input can fill, and
/// an empty list fuzzes every target `cargo fuzz list` knows
/// (`registry_matches_bins_files_and_corpus` keeps those equal to
/// `targets::TARGETS`).
#[test]
fn ci_fuzzes_every_target() {
    let wf = std::fs::read_to_string(root().join("../.github/workflows/fuzz.yml")).unwrap();
    let doc: serde_json::Value = serde_saphyr::from_str(&wf).unwrap();
    let steps = doc["jobs"]["fuzz"]["steps"]
        .as_array()
        .expect("fuzz.yml: jobs.fuzz.steps");
    let fuzz = steps
        .iter()
        .find(|s| s["name"] == "Fuzz")
        .expect("fuzz.yml: the fuzz job's `Fuzz` step");
    assert_eq!(
        fuzz["run"]
            .as_str()
            .and_then(|r| r.lines().map(str::trim).next_back()),
        Some("scripts/fuzz.sh run $FUZZ_TARGETS"),
        "fuzz.yml's Fuzz step"
    );
    assert_eq!(
        fuzz["env"]["FUZZ_TARGETS"].as_str(),
        Some("${{ inputs.targets }}"),
        "fuzz.yml's Fuzz step: FUZZ_TARGETS must be empty except on a manual run"
    );
    assert!(
        doc["jobs"]["fuzz"]["strategy"].is_null(),
        "fuzz.yml's fuzz job is one job over every target, not a matrix"
    );
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

/// Every `regressions/<target>/known-<id>[--note]` file still reproduces
/// known issue `<id>`, and every known issue has such a reproducer. A fixed
/// bug therefore fails here until its gate becomes an `oracle!`.
#[test]
fn known_issue_reproducers_reproduce() {
    use openjd_fuzz::known::{take_recorded, KNOWN_ISSUES};
    install_hook();
    let mut problems = Vec::new();
    let mut reproduced = BTreeSet::new();
    for (name, target) in TARGETS {
        for file in files_in(&root().join("regressions").join(name)) {
            let stem = file.file_name().unwrap().to_string_lossy().to_string();
            let Some(rest) = stem.strip_prefix("known-") else {
                continue;
            };
            let id = rest.split("--").next().unwrap().to_string();
            // A `--posix-host` / `--windows-host` note marks a reproducer
            // that depends on the host's path format (static validation
            // runs in it); elsewhere it is not run, but still counts.
            let host = openjd_expr::PathFormat::host();
            let other_host = (stem.contains("--posix-host")
                && host != openjd_expr::PathFormat::Posix)
                || (stem.contains("--windows-host") && host != openjd_expr::PathFormat::Windows);
            if other_host {
                reproduced.insert(id);
                continue;
            }
            let data = std::fs::read(&file).unwrap();
            take_recorded();
            IN_REPLAY.with(|r| r.set(true));
            let _ = catch_unwind(AssertUnwindSafe(|| target(&data)));
            IN_REPLAY.with(|r| r.set(false));
            let hit = take_recorded();
            if hit.contains(&id) {
                reproduced.insert(id);
            } else {
                problems.push(format!("{name}/{stem}: records {hit:?}, not {id:?}"));
            }
        }
    }
    for (id, _) in KNOWN_ISSUES {
        if !reproduced.contains(*id) {
            problems.push(format!(
                "known issue {id:?} has no reproducing regressions/*/known-{id}[--note] file"
            ));
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

/// `expr-static-fold-host-path-format` explains only mismatches the path
/// format causes: a mismatch from another cause, in an expression that
/// merely mentions a path, is still reported as that other cause alone.
/// (`known_issue_reproducers_reproduce` checks only that an id is recorded.)
#[test]
fn host_path_fold_does_not_claim_other_mismatches() {
    use openjd_fuzz::known::take_recorded;
    install_hook();
    // The range-slice reproducer's expression, wrapped in a path condition
    // whose branches are the same, with its control bytes.
    let base = std::fs::read(
        root().join("regressions/expr_eval_text/known-expr-range-slice-static-type--param-step"),
    )
    .unwrap();
    let ctl = &base[base.iter().position(|&b| b == 0).unwrap()..];
    let mut data =
        b"Session.R[::Session.N] if path('a').is_absolute() else Session.R[::Session.N]".to_vec();
    data.extend_from_slice(ctl);
    let (_, target) = TARGETS
        .iter()
        .find(|(n, _)| *n == "expr_eval_text")
        .unwrap();
    take_recorded();
    IN_REPLAY.with(|r| r.set(true));
    let outcome = catch_unwind(AssertUnwindSafe(|| target(&data)));
    IN_REPLAY.with(|r| r.set(false));
    assert!(
        outcome.is_ok(),
        "unexpected oracle violation: {}",
        LAST_PANIC.with(|p| p.borrow().clone())
    );
    let hit = take_recorded();
    assert_eq!(
        hit,
        BTreeSet::from(["expr-range-slice-static-type".to_string()]),
        "recorded known issues"
    );
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

/// Text in string literals is not code: it must not switch on a known-issue
/// gate and mask a real static/run-time disagreement.
/// Text Facts: only `a if c else b` is a conditional, not a comprehension
/// filter.
#[test]
fn conditional_expressions() {
    use openjd_fuzz::targets::expr::has_conditional_expr as cond;
    for yes in [
        "a if c else b",
        "[x if x > 1 else 0 for x in L]",
        "f(a if c else b)",
        "[x for x in L if (1 if c else 2)]",
    ] {
        assert!(cond(yes), "{yes}");
    }
    for no in [
        "[x for x in L if c]",
        "[upper(x) for x in [1] if True]",
        "[x for x in L if c] + [y]",
        "a else b",
    ] {
        assert!(!cond(no), "{no}");
    }
}

/// The `expr-range-slice-static-type` gate: any step but a non-negative
/// literal may be negative, whether it is written with `-` or not.
#[test]
fn negative_step_slices() {
    use openjd_fuzz::targets::expr::has_negative_step_slice as neg;
    for may in [
        "R[::-1]",
        "R[1:2:Param.N]",
        "R[::int(  )]",
        "R[::(0-1)]",
        "f(R[::x])[0]",
    ] {
        assert!(neg(may), "{may}");
    }
    for not in [
        "R[::2]",
        "R[1:2:]",
        "R[::None]",
        "R[1:2]",
        "R[0]",
        "f(a, b)[::1]",
        "R[g(1:2)]",
    ] {
        assert!(!neg(not), "{not}");
    }
}

#[test]
fn text_facts_ignore_string_literals() {
    use openjd_fuzz::targets::expr::Facts;
    let f = Facts::from_text("upper(Param.S + ' or flatten(x) if [] ')");
    assert!(!f.boolop && !f.union && !f.conditional, "{f:?}");
    let f = Facts::from_text(r#"upper("a\" and b") + 'sum(1)'"#);
    assert!(!f.boolop && !f.unbound_return, "{f:?}");
    let f = Facts::from_text("a or flatten(x) + max (y) if c else []");
    assert!(
        f.boolop && f.union && f.unbound_return && f.conditional,
        "{f:?}"
    );
    assert!(!Facts::from_text("my_flatten(x)").union);
    // A format string's literal text is not code either, and an apostrophe
    // there does not hide the interpolation after it.
    let f = Facts::from_format_text("it's or {{ a and b }} flatten(x)");
    assert!(f.boolop && !f.union, "{f:?}");
}
