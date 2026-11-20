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
#[test]
fn unbound_return_text_ignores_string_literals() {
    use openjd_fuzz::targets::expr::unbound_return_in_text as unbound;
    assert!(unbound("max(y) + 1"));
    assert!(unbound("1 + sum (L)"));
    assert!(unbound("L + range_expr('1-3')"));
    assert!(!unbound("upper(Param.S + ' sum(1) ')"));
    assert!(!unbound("my_max(x) + 1"));
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

/// Run `check_expression` on `src` in an environment whose static and
/// concrete tables say different things, which simulates a library bug, and
/// return the oracle violation it reports, if any.
fn simulated_violation(
    src: &str,
    statics: &[(&str, openjd_expr::ExprType)],
    values: &[(&str, openjd_expr::ExprValue)],
    generated: Option<bool>,
) -> Option<String> {
    use openjd_expr::{ExprValue, ParsedExpression, SymbolTable};
    use openjd_fuzz::gen::expr::Symbol;
    use openjd_fuzz::targets::expr::{check_expression, Env};
    install_hook();
    let mut unresolved = SymbolTable::new();
    let mut symbols = Vec::new();
    for (name, ty) in statics {
        unresolved
            .set(name, ExprValue::unresolved(ty.clone()))
            .unwrap();
        symbols.push(Symbol {
            name: name.to_string(),
            ty: ty.clone(),
        });
    }
    let mut concrete = SymbolTable::new();
    for (name, v) in values {
        concrete.set(name, v.clone()).unwrap();
    }
    let env = Env {
        symbols,
        concrete,
        unresolved,
        rules: Vec::new(),
    };
    let parsed = ParsedExpression::new(src).unwrap();
    IN_REPLAY.with(|r| r.set(true));
    let outcome = catch_unwind(AssertUnwindSafe(|| {
        check_expression(
            &parsed,
            &env,
            &mut arbitrary::Unstructured::new(&[0; 16]),
            generated,
        )
    }));
    IN_REPLAY.with(|r| r.set(false));
    outcome
        .is_err()
        .then(|| LAST_PANIC.with(|p| p.borrow().clone()))
}

/// [`simulated_violation`] with static values instead of `Unresolved`
/// placeholders, for well-typed input: the static pass computes with
/// `statics` and the run with `values`.
fn simulated_violation_with_static_values(
    src: &str,
    statics: &[(&str, openjd_expr::ExprValue)],
    values: &[(&str, openjd_expr::ExprValue)],
) -> Option<String> {
    use openjd_expr::{ParsedExpression, SymbolTable};
    use openjd_fuzz::gen::expr::Symbol;
    use openjd_fuzz::targets::expr::{check_expression, Env};
    install_hook();
    let mut unresolved = SymbolTable::new();
    let mut symbols = Vec::new();
    for (name, v) in statics {
        unresolved.set(name, v.clone()).unwrap();
        symbols.push(Symbol {
            name: name.to_string(),
            ty: v.expr_type(),
        });
    }
    let mut concrete = SymbolTable::new();
    for (name, v) in values {
        concrete.set(name, v.clone()).unwrap();
    }
    let env = Env {
        symbols,
        concrete,
        unresolved,
        rules: Vec::new(),
    };
    let parsed = ParsedExpression::new(src).unwrap();
    IN_REPLAY.with(|r| r.set(true));
    let outcome = catch_unwind(AssertUnwindSafe(|| {
        check_expression(
            &parsed,
            &env,
            &mut arbitrary::Unstructured::new(&[0; 16]),
            Some(false),
        )
    }));
    IN_REPLAY.with(|r| r.set(false));
    outcome
        .is_err()
        .then(|| LAST_PANIC.with(|p| p.borrow().clone()))
}

/// Each hard oracle of `check_expression` fires on the disagreement it
/// guards against, and the allowed direction passes.
#[test]
fn expression_oracles_fire() {
    use openjd_expr::{ExprType, ExprValue};
    let s = ExprType::STRING;
    let int = |i| ExprValue::Int(i);
    // A run-time value outside its static type.
    let v = simulated_violation(
        "Param.X",
        &[("Param.X", s.clone())],
        &[("Param.X", int(1))],
        None,
    );
    assert!(
        v.as_deref()
            .is_some_and(|m| m.contains("static type string")),
        "{v:?}"
    );
    // Rejected statically, but the (well-typed) expression runs.
    let v = simulated_violation(
        "Param.X + 1",
        &[("Param.X", s.clone())],
        &[("Param.X", int(1))],
        Some(false),
    );
    assert!(
        v.as_deref()
            .is_some_and(|m| m.contains("static checking rejects it")),
        "{v:?}"
    );
    // The same in a conditional, where the static error is compound.
    let v = simulated_violation(
        "(Param.X + 1) if Param.B else (Param.X + 2)",
        &[("Param.X", s.clone()), ("Param.B", ExprType::BOOL)],
        &[("Param.X", int(1)), ("Param.B", ExprValue::Bool(true))],
        Some(false),
    );
    assert!(
        v.as_deref()
            .is_some_and(|m| m.contains("static checking rejects it")),
        "{v:?}"
    );
    // Accepted statically, then a failure no value can cause.
    let v = simulated_violation("Param.Y", &[("Param.Y", s.clone())], &[], None);
    assert!(
        v.as_deref()
            .is_some_and(|m| m.contains("accepted statically")),
        "{v:?}"
    );
    // Rejected statically for a reason some value causes, though the text
    // runs: a violation for text too.
    let v = simulated_violation(
        "Param.X + 1",
        &[("Param.X", s.clone())],
        &[("Param.X", int(1))],
        None,
    );
    assert!(
        v.as_deref()
            .is_some_and(|m| m.contains("static checking rejects it")),
        "{v:?}"
    );
    // A name error in a branch no run takes is rejected on purpose.
    let v = simulated_violation(
        "1 if Param.B else nosuch(1)",
        &[("Param.B", ExprType::BOOL)],
        &[("Param.B", ExprValue::Bool(true))],
        None,
    );
    assert!(v.is_none(), "{v:?}");
    // A budget error the static pass alone hits (a longer static value
    // simulates an over-charge): a violation without a speculative
    // construct, the known issue with one.
    let long = ExprValue::String("x".repeat(100));
    let short = ExprValue::String("x".into());
    let budget = |src: &str| {
        simulated_violation_with_static_values(
            src,
            &[("Param.X", long.clone())],
            &[("Param.X", short.clone())],
        )
    };
    let v = budget("len(Param.X * 100000) > 0");
    assert!(
        v.as_deref()
            .is_some_and(|m| m.contains("static checking rejects it")),
        "{v:?}"
    );
    assert!(budget("len(Param.X * 100000) > 0 or False").is_none());
    // A list value outside its list type is excused only for a generated
    // expression through an unbound-return signature.
    let ls = ExprType::list(ExprType::STRING);
    let li = ExprValue::ListInt(vec![1]);
    let v = simulated_violation(
        "Param.L",
        &[("Param.L", ls.clone())],
        &[("Param.L", li.clone())],
        None,
    );
    assert!(
        v.as_deref()
            .is_some_and(|m| m.contains("static type list[string]")),
        "{v:?}"
    );
    let v = simulated_violation(
        "Param.L",
        &[("Param.L", ls)],
        &[("Param.L", li)],
        Some(true),
    );
    assert!(v.is_none(), "{v:?}");
    // A value folded around a host-context function is not strict.
    let v = simulated_violation(
        "(path('/a').relative_to(apply_path_mapping('/b')) == path('x')) and False",
        &[],
        &[],
        None,
    );
    assert!(v.is_none(), "{v:?}");
    // Accepted statically, then a value-dependent failure: allowed.
    let v = simulated_violation(
        "upper(Param.X)",
        &[("Param.X", s.clone())],
        &[("Param.X", int(1))],
        None,
    );
    assert!(v.is_none(), "{v:?}");
}

/// After static acceptance, a run-time failure is a violation exactly when
/// the static pass folded a value, or no value could cause it.
#[test]
fn accepted_then_fails_rule() {
    use openjd_expr::{ExprProfile, ExprValue, FunctionLibrary, ParsedExpression, SymbolTable};
    use openjd_fuzz::targets::expr::check_accepted_then_fails;
    install_hook();
    let lib = FunctionLibrary::for_profile(&ExprProfile::latest());
    let err = |src: &str| {
        ParsedExpression::new(src)
            .unwrap()
            .with_library(&lib)
            .evaluate(&[&SymbolTable::new()])
            .expect_err(src)
    };
    let fires = |e: &openjd_expr::ExpressionError, folded: Option<&ExprValue>| {
        IN_REPLAY.with(|r| r.set(true));
        let r = catch_unwind(AssertUnwindSafe(|| {
            check_accepted_then_fails(e, folded, || "msg".to_string())
        }));
        IN_REPLAY.with(|r| r.set(false));
        r.is_err()
    };
    for value_independent in ["nosuch(1)", "Nope.X", "(1).nosuch"] {
        assert!(fires(&err(value_independent), None), "{value_independent}");
    }
    for allowed in ["upper(1)", "'a' - 1", "int('x')", "1 // 0"] {
        assert!(!fires(&err(allowed), None), "{allowed}");
        assert!(
            fires(&err(allowed), Some(&ExprValue::Int(1))),
            "{allowed} folded"
        );
    }
}
