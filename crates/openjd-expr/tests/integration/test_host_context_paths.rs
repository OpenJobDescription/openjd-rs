// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! A library built for `HostContext::Unresolved` checks host-scope
//! expressions before the worker is known, so before its path format is
//! known: `path()` and `with_number` on a string, the functions that read a
//! literal string as a filesystem path, return `unresolved[T]` there
//! (specs/expr/function-library.md § Host Context). Template scope
//! (`HostContext::None`) and run time (`HostContext::WithRules`) keep
//! building concrete paths.

use openjd_expr::*;

fn library(hc: HostContext) -> std::sync::Arc<FunctionLibrary> {
    FunctionLibrary::for_profile(&ExprProfile::current().with_host_context(hc))
}

fn eval_in(expr: &str, lib: &FunctionLibrary, fmt: PathFormat) -> Result<ExprValue, String> {
    ParsedExpression::new(expr)
        .and_then(|p| {
            p.with_library(lib)
                .with_path_format(fmt)
                .evaluate(&[&SymbolTable::new()])
        })
        .map_err(|e| e.to_string())
}

fn unresolved(expr: &str) -> ExprValue {
    let lib = library(HostContext::Unresolved);
    eval_in(expr, &lib, PathFormat::Posix).unwrap_or_else(|e| panic!("{expr}: {e}"))
}

/// Expressions that fail when folded in POSIX but that a Windows worker
/// runs, with the value it gives there.
const WINDOWS_RUNS: &[(&str, &str)] = &[
    (r"int(path('C:\\a\\5').name)", "5"),
    (
        r"'ok' if path('C:\\a').is_absolute() else fail('relative')",
        "ok",
    ),
    (r"path('C:\\a\\b').parts[2]", "b"),
    (r"range(len(path('C:\\a\\b').parts))[2]", "2"),
    (r"path(['C:\\a', 'b']).parts[2]", "b"),
    (
        r"with_number('C:\\#################################\\b', 1)",
        r"C:\#################################\b_0001",
    ),
];

#[test]
fn host_check_accepts_what_a_windows_worker_runs() {
    let unresolved_lib = library(HostContext::Unresolved);
    let runtime = library(HostContext::with_rules(Vec::new()).unwrap());
    for (expr, windows_value) in WINDOWS_RUNS {
        // A POSIX fold fails.
        let posix_run = eval_in(expr, &runtime, PathFormat::Posix);
        assert!(posix_run.is_err(), "{expr} runs on POSIX: {posix_run:?}");
        // A Windows worker runs it.
        let win = eval_in(expr, &runtime, PathFormat::Windows).unwrap();
        assert_eq!(win.to_display_string(), *windows_value, "{expr}");
        // The host check, in either format, leaves it unresolved.
        for fmt in [PathFormat::Posix, PathFormat::Windows] {
            let v = eval_in(expr, &unresolved_lib, fmt).unwrap_or_else(|e| panic!("{expr}: {e}"));
            assert!(v.is_unresolved(), "{expr} ({fmt:?}) folded to {v:?}");
        }
    }
}

#[test]
fn literal_paths_are_unresolved_before_the_host() {
    for (expr, ty) in [
        (r"path('C:\\a')", "unresolved[path]"),
        (r"path(['C:\\a', 'b'])", "unresolved[path]"),
        (r"path('/a/b')", "unresolved[path]"),
        (r"path('s3://bucket/a')", "unresolved[path]"),
        (r"path('C:\\a').parent", "unresolved[path]"),
        (r"len(path('C:\\x\\y').parts)", "unresolved[int]"),
        (r"path('C:\\a') / 'b'", "unresolved[path]"),
        (
            r"1 if path('C:\\a').is_absolute() else 'no'",
            "unresolved[int | string]",
        ),
        (r"with_number('C:\\a\\f_%04d.exr', 3)", "unresolved[string]"),
        (r"path('C:\\a').with_number(3)", "unresolved[path]"),
    ] {
        assert_eq!(unresolved(expr).expr_type().to_string(), ty, "{expr}");
    }
}

#[test]
fn type_errors_on_literal_paths_are_still_reported_before_the_host() {
    let lib = library(HostContext::Unresolved);
    let e = eval_in("path('/a').startswith('/')", &lib, PathFormat::Posix).unwrap_err();
    assert_eq!(
        e,
        "startswith() is not available for path. Available for: string\n  \
         path('/a').startswith('/')\n  ~~~~~~~~~~~^~~~~~~~~~~~~~~"
    );
    let e = eval_in("path(1)", &lib, PathFormat::Posix).unwrap_err();
    assert_eq!(
        e,
        "No matching signature for path(int)\n  path(1)\n  ^~~~~~~"
    );
}

#[test]
fn with_number_on_a_concrete_path_stays_concrete_before_the_host() {
    // Only `with_number(string, int)` reads a string as a path; a path
    // receiver already carries its format, so its result is still folded.
    let lib = library(HostContext::Unresolved);
    let mut st = SymbolTable::new();
    st.set(
        "P",
        ExprValue::new_path("/a/f_%04d.exr".to_string(), PathFormat::Posix),
    )
    .unwrap();
    let v = ParsedExpression::new("P.with_number(3)")
        .and_then(|p| {
            p.with_library(&lib)
                .with_path_format(PathFormat::Posix)
                .evaluate(&[&st])
        })
        .unwrap();
    assert_eq!(
        v,
        ExprValue::new_path("/a/f_0003.exr".to_string(), PathFormat::Posix)
    );
}

#[test]
fn every_path_overload_is_unresolved_before_the_host() {
    // Whatever overloads `path` has, none builds a concrete path in a
    // host-context check, and each keeps its signature.
    let base = FunctionLibrary::for_profile(&ExprProfile::current());
    let lib = library(HostContext::Unresolved);
    let sigs = |l: &FunctionLibrary| {
        l.get_signatures("path")
            .iter()
            .map(|e| e.signature.to_string())
            .collect::<Vec<_>>()
    };
    assert_eq!(sigs(&lib), sigs(&base));
    for entry in lib.get_signatures("path") {
        let mut ctx_lib = FunctionLibrary::new();
        ctx_lib.register("path", entry.signature.clone(), {
            let f = entry.implementation.clone();
            move |ctx, a| f(ctx, a)
        });
        let arg = match entry.signature.sig_params()[0].to_string().as_str() {
            "string" => "'C:\\\\a'",
            "list[string]" => "['C:\\\\a', 'b']",
            "list[nulltype]" => "[]",
            other => panic!("path overload with an untested parameter {other}"),
        };
        let v = eval_in(&format!("path({arg})"), &ctx_lib, PathFormat::Posix).unwrap();
        assert_eq!(v, ExprValue::unresolved(ExprType::PATH), "path({arg})");
    }
    // `with_number` keeps a concrete path receiver's result concrete:
    // only a string, read as a path in the worker's format, is unknown.
    let with_number: Vec<_> = lib
        .get_signatures("with_number")
        .iter()
        .map(|e| e.signature.to_string())
        .collect();
    assert_eq!(
        with_number,
        ["(path, int) -> path", "(string, int) -> string"]
    );
}

#[test]
fn template_scope_and_run_time_paths_stay_concrete() {
    // TEMPLATE scope is POSIX (Expression Language "Path Format").
    let template = library(HostContext::None);
    let v = eval_in(r"path('C:\\a\\b').parts", &template, PathFormat::Posix).unwrap();
    assert_eq!(v.to_display_string(), r#"["C:\\a\\b"]"#);
    let v = eval_in(
        r"with_number('/a/f_%04d.exr', 3)",
        &template,
        PathFormat::Posix,
    )
    .unwrap();
    assert_eq!(v, ExprValue::String("/a/f_0003.exr".into()));
    // At run time the worker's format applies.
    let runtime = library(HostContext::with_rules(Vec::new()).unwrap());
    let v = eval_in(r"path('C:\\a\\b').parts", &runtime, PathFormat::Windows).unwrap();
    assert_eq!(v.to_display_string(), r#"["C:\\", "a", "b"]"#);
}

#[test]
fn literal_path_contributes_no_resolved_length_before_the_host() {
    // `min_resolved_string_len` must not count a POSIX fold: the POSIX
    // parent of `C:\a\b` is `.` (1 character), the Windows one `C:\a`.
    let fs = FormatString::new(r"{{ 'x' * (10 - len(path('C:\\a\\b').parent)) }}").unwrap();
    let host = library(HostContext::Unresolved);
    let sr = fs
        .validate_expressions(
            &SymbolTable::new(),
            &FormatStringOptions::new()
                .with_library(&*host)
                .with_path_format(PathFormat::Posix),
        )
        .unwrap();
    assert_eq!(sr.min_resolved_string_len, 0);
    assert!(sr.resolved_value.is_none());
    let template = library(HostContext::None);
    let sr = fs
        .validate_expressions(
            &SymbolTable::new(),
            &FormatStringOptions::new()
                .with_library(&*template)
                .with_path_format(PathFormat::Posix),
        )
        .unwrap();
    assert_eq!(sr.min_resolved_string_len, 9);
}
