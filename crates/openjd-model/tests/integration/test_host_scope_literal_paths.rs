// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! A literal path in a host-scope field is `unresolved[path]` when the
//! template is validated and when the job is created, because the worker's
//! path format isn't known yet (Expression Language "Path Format";
//! specs/model/validation.md). So an expression a Windows worker runs is
//! not rejected by a POSIX fold. TEMPLATE scope stays POSIX.

use openjd_model::template::JobTemplate;
use openjd_model::{create_job, decode_job_template, CallerLimits, ModelError};
use std::collections::HashMap;

/// Host-scope expressions that a Windows worker runs and that fail when
/// folded in POSIX.
const WINDOWS_RUNS: &[&str] = &[
    r#"int(path("C:\\a\\5").name)"#,
    r#""ok" if path("C:\\a").is_absolute() else fail("relative")"#,
    r#"path("C:\\a\\b").parts[2]"#,
    r#"range(len(path("C:\\a\\b").parts))[2]"#,
    r#"path(["C:\\a", "b"]).parts[2]"#,
    r#"with_number("C:\\#################################\\b", 1)"#,
    // The POSIX parent of C:\a\b is "." (1 character), which would bound
    // the value at 2049 characters, over the 2048 limit on an environment
    // variable value; a Windows worker resolves 2046.
    r#""x" * (2050 - len(path("C:\\a\\b").parent))"#,
];

fn template(expr: &str, field: &str) -> serde_json::Value {
    let fs = format!("{{{{ {expr} }}}}");
    let mut script = serde_json::json!({
        "actions": {"onRun": {"command": "echo", "args": ["x"]}}
    });
    let mut job_envs = serde_json::Value::Null;
    let mut step_envs = serde_json::Value::Null;
    match field {
        "arg" => script["actions"]["onRun"]["args"][0] = fs.into(),
        "command" => script["actions"]["onRun"]["command"] = fs.into(),
        "let" => script["let"] = serde_json::json!([format!("v = {expr}")]),
        "embedded file" => {
            script["embeddedFiles"] = serde_json::json!([{"name": "F", "type": "TEXT", "data": fs}])
        }
        "job env var" => job_envs = serde_json::json!([{"name": "E", "variables": {"V": fs}}]),
        "step env script let" => {
            step_envs = serde_json::json!([{"name": "E", "script": {
                "let": [format!("v = {expr}")],
                "actions": {"onEnter": {"command": "echo", "args": ["{{ v }}"]}}
            }}])
        }
        other => panic!("unknown field {other}"),
    }
    let mut step = serde_json::json!({"name": "S", "script": script});
    if !step_envs.is_null() {
        step["stepEnvironments"] = step_envs;
    }
    let mut t = serde_json::json!({
        "specificationVersion": "jobtemplate-2023-09",
        "extensions": ["EXPR"],
        "name": "J",
        "steps": [step],
    });
    if !job_envs.is_null() {
        t["jobEnvironments"] = job_envs;
    }
    t
}

fn decode(t: serde_json::Value) -> Result<JobTemplate, ModelError> {
    decode_job_template(t, Some(&["EXPR"]), &CallerLimits::default())
}

const HOST_FIELDS: &[&str] = &[
    "arg",
    "command",
    "let",
    "embedded file",
    "job env var",
    "step env script let",
];

#[test]
fn host_scope_literal_paths_pass_validation_and_job_creation() {
    let mut failures = Vec::new();
    for expr in WINDOWS_RUNS {
        for field in HOST_FIELDS {
            let jt = match decode(template(expr, field)) {
                Ok(jt) => jt,
                Err(e) => {
                    failures.push(format!("{field}: {expr}\n  validation: {e}"));
                    continue;
                }
            };
            let ctx = jt.default_validation_context();
            if let Err(e) = create_job(&jt, &HashMap::new(), &ctx) {
                failures.push(format!("{field}: {expr}\n  create_job: {e}"));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn literal_path_value_errors_wait_for_the_worker() {
    // These fail on every worker. Before the host is known they can't be
    // told apart from the expressions above, so they fail at run time.
    for expr in [
        r#"path("/a/b").relative_to(path("/x/y"))"#,
        r#"path("/out/file_%099d.exr").with_number(1)"#,
        r#"bool(path("/tmp"))"#,
    ] {
        decode(template(expr, "arg")).unwrap_or_else(|e| panic!("{expr}: {e}"));
    }
}

#[test]
fn host_scope_literal_path_type_errors_are_still_reported() {
    let err = decode(template(r#"path("/a").startswith("/")"#, "arg"))
        .unwrap_err()
        .to_string();
    assert!(
        err.contains(
            "steps[0] -> script -> actions -> onRun -> args[0]:\n\tFailed to parse interpolation \
             expression at [0, 32]. startswith() is not available for path. Available for: \
             string\n  path(\"/a\").startswith(\"/\")\n  ~~~~~~~~~~~^~~~~~~~~~~~~~~"
        ),
        "{err}"
    );
}

#[test]
fn host_scope_literal_path_subscripts_are_still_reported() {
    // A subscript of a path fails on every worker (2026-02 §2.1.7 has no
    // path receiver), and whether it does depends on no value, so it is
    // refused at validation like the method call above, in every host
    // field, for an index and for a slice.
    for expr in [r#"path("/a/b")[0]"#, r#"path("/a/b")[0:1]"#] {
        for field in HOST_FIELDS {
            let err = decode(template(expr, field))
                .err()
                .unwrap_or_else(|| panic!("{field}: {expr} should be refused"))
                .to_string();
            assert!(
                err.contains("Cannot subscript type path"),
                "{field}: {expr}\n{err}"
            );
        }
    }
}

#[test]
fn template_scope_paths_are_unresolved_in_host_scope() {
    // A path a step-level let builds is TEMPLATE scope (POSIX), but the
    // session converts it to the worker's format before host-scope fields
    // use it (specs/sessions/session.md, `to_symtab(PathFormat::host())`),
    // so a host-scope check can't fold it in POSIX either.
    let mut failures = Vec::new();
    for (lets, expr) in [
        (vec![r#"p = path("C:\\a\\b")"#], "p.parts[2]"),
        (vec![r#"p = path("/x")"#], r#"int((p / "a\\5").name)"#),
        (vec![r#"q = [path("C:\\a\\b")]"#], "q[0].parts[2]"),
        (
            vec![r#"p = path("C:\\#################################\\b")"#],
            "p.with_number(1)",
        ),
    ] {
        for field in ["arg", "let", "step env script let"] {
            let mut t = template(expr, field);
            t["steps"][0]["let"] = serde_json::json!(lets);
            let jt = match decode(t) {
                Ok(jt) => jt,
                Err(e) => {
                    failures.push(format!("{field}: {lets:?} {expr}\n  validation: {e}"));
                    continue;
                }
            };
            let ctx = jt.default_validation_context();
            if let Err(e) = create_job(&jt, &HashMap::new(), &ctx) {
                failures.push(format!("{field}: {lets:?} {expr}\n  create_job: {e}"));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn template_scope_literal_paths_stay_posix() {
    // The job name and a step-level let are TEMPLATE scope, which is POSIX:
    // C:\a\b is one path component there.
    let mut name = template("1", "arg");
    name["name"] = r#"{{ path("C:\\a\\b").parts[2] }}"#.into();
    let err = decode(name).unwrap_err().to_string();
    assert!(
        err.contains(
            "name:\n\tFailed to parse interpolation expression at [0, 31]. Index 2 out of \
             bounds for list of length 1\n  path(\"C:\\\\a\\\\b\").parts[2]\n  \
             ~~~~~~~~~~~~~~~~~~~~~~^~~"
        ),
        "{err}"
    );
    let mut step_let = template("1", "arg");
    step_let["steps"][0]["let"] = serde_json::json!([r#"v = path("C:\\a\\b").parts[2]"#]);
    let err = decode(step_let).unwrap_err().to_string();
    assert!(
        err.contains(
            "steps[0] -> let[0]:\n\tInvalid expression in let binding 'v': Index 2 out of \
             bounds for list of length 1\n  v = path(\"C:\\\\a\\\\b\").parts[2]\n      \
             ~~~~~~~~~~~~~~~~~~~~~~^~~"
        ),
        "{err}"
    );
}
