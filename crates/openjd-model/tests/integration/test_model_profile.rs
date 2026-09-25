// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Integration tests verifying that `ModelProfile::current()` and
//! `ModelProfile::latest()` drive distinct behavior through `create_job`.
//!
//! `latest()` enables the Expr extension (`has_expr = true`), which
//! causes `create_job` to populate `Job.Name` and `Step.Name` in the
//! step's resolved symbol table. A context that does *not* enable an
//! extension the template declares is rejected outright — `create_job`
//! requires the context's extensions to cover the template's declared
//! ones (matching revision likewise).

use openjd_expr::path_mapping::PathFormat;
use openjd_model::types::{ModelExtension, ModelProfile, ValidationContext};
use openjd_model::{
    create_job, decode_job_template, preprocess_job_parameters, CallerLimits,
    JobParameterInputValues, JobParameterValues, PathParameterOptions,
};

fn yaml_val(s: &str) -> serde_json::Value {
    serde_saphyr::from_str(s).unwrap()
}

/// Helper: preprocess with POSIX path options and no user-supplied values.
fn preprocess_posix_defaults(jt: &openjd_model::template::JobTemplate) -> JobParameterValues {
    preprocess_job_parameters(
        jt,
        &JobParameterInputValues::new(),
        &[],
        &PathParameterOptions {
            job_template_dir: "/job_template_dir",
            current_working_dir: "/current_working_dir",
            allow_template_dir_walk_up: true,
            path_format: PathFormat::Posix,
            allow_uri_path_values: true,
        },
    )
    .unwrap()
}

// ─── Test A: current() succeeds on a simple (no-extension) template ─────────

#[test]
fn current_profile_succeeds_on_simple_template() {
    let tpl = yaml_val(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "SimpleJob",
        "steps": [{"name": "Step1", "script": {"actions": {"onRun": {"command": "echo"}}}}]
    }"#,
    );
    let jt = decode_job_template(tpl, None, &CallerLimits::default()).unwrap();
    let params = preprocess_posix_defaults(&jt);

    let ctx = ValidationContext::from_profile(ModelProfile::current());
    let job = create_job(&jt, &params, &ctx).unwrap();

    assert_eq!(job.name, "SimpleJob");
    assert_eq!(job.steps.len(), 1);
    assert_eq!(job.steps[0].name, "Step1");

    // Verify profile properties
    assert!(!ModelProfile::current().has_extension(ModelExtension::Expr));
    assert!(ModelProfile::current().extensions().is_empty());
}

// ─── Test B: latest() populates Job.Name and Step.Name in resolved symtab ───

#[test]
fn latest_profile_populates_expr_symbols_in_symtab() {
    // Template declares EXPR extension; action references {{Job.Name}} and {{Step.Name}}
    let tpl = yaml_val(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "RenderJob",
        "extensions": ["EXPR"],
        "steps": [{"name": "Composite", "script": {"actions": {"onRun": {"command": "run", "args": ["{{Job.Name}}", "{{Step.Name}}"]}}}}]
    }"#,
    );
    let jt = decode_job_template(tpl, Some(&["EXPR"]), &CallerLimits::default()).unwrap();
    let params = preprocess_posix_defaults(&jt);

    let ctx = ValidationContext::from_profile(ModelProfile::latest());
    let job = create_job(&jt, &params, &ctx).unwrap();

    assert_eq!(job.name, "RenderJob");

    // With Expr enabled, resolved_symtab carries Job.Name and Step.Name
    let symtab = job.steps[0]
        .resolved_symtab
        .as_ref()
        .unwrap()
        .to_symtab(PathFormat::Posix)
        .unwrap();
    assert_eq!(
        symtab.get_string("Job.Name"),
        Some("RenderJob"),
        "latest() must populate Job.Name in resolved symtab"
    );
    assert_eq!(
        symtab.get_string("Step.Name"),
        Some("Composite"),
        "latest() must populate Step.Name in resolved symtab"
    );

    // Verify latest() indeed has Expr enabled
    assert!(ModelProfile::latest().has_extension(ModelExtension::Expr));
}

// ─── Test C: a context missing a declared extension is rejected ─────────────

#[test]
fn context_missing_declared_extension_is_rejected() {
    // Same EXPR template as Test B, but fed through current() context,
    // which has Expr OFF. create_job requires the context to enable
    // every extension the template declares: a context enabling fewer
    // would make downstream evaluation errors ambiguous (template
    // defect vs context artifact), which is what lets the job-creation
    // checks report them strictly. An application that does not
    // support an extension rejects the template at decode via
    // `supported_extensions` instead.
    let tpl = yaml_val(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "RenderJob",
        "extensions": ["EXPR"],
        "steps": [{"name": "Composite", "script": {"actions": {"onRun": {"command": "run", "args": ["{{Job.Name}}", "{{Step.Name}}"]}}}}]
    }"#,
    );
    // Decode still accepts the template (extensions were allowed at decode time)
    let jt = decode_job_template(tpl, Some(&["EXPR"]), &CallerLimits::default()).unwrap();
    let params = preprocess_posix_defaults(&jt);

    let ctx = ValidationContext::from_profile(ModelProfile::current());
    let err = create_job(&jt, &params, &ctx).unwrap_err();
    assert_eq!(
        err.to_string(),
        "Compatibility error: create_job requires a context enabling every extension the \
         template declares: missing EXPR. An application that does not support an extension \
         should reject the template at decode via its supported-extensions list."
    );
}

#[test]
fn context_missing_several_declared_extensions_lists_them_sorted() {
    // The missing set is derived from the template's own extension set
    // (a HashSet) and sorted, so the message is deterministic and
    // exhaustive regardless of declaration order.
    let tpl = yaml_val(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "RenderJob",
        "extensions": ["FEATURE_BUNDLE_1", "EXPR"],
        "steps": [{"name": "S", "script": {"actions": {"onRun": {"command": "run"}}}}]
    }"#,
    );
    let jt = decode_job_template(
        tpl,
        Some(&["EXPR", "FEATURE_BUNDLE_1"]),
        &CallerLimits::default(),
    )
    .unwrap();
    let params = preprocess_posix_defaults(&jt);

    let ctx = ValidationContext::from_profile(ModelProfile::current());
    let err = create_job(&jt, &params, &ctx).unwrap_err();
    assert_eq!(
        err.to_string(),
        "Compatibility error: create_job requires a context enabling every extension the \
         template declares: missing EXPR, FEATURE_BUNDLE_1. An application that does not \
         support an extension should reject the template at decode via its \
         supported-extensions list."
    );
}

// ─── Test D: latest() has all ModelExtension::ALL variants ──────────────────

#[test]
fn latest_profile_enables_all_extensions() {
    let profile = ModelProfile::latest();
    for ext in ModelExtension::ALL {
        assert!(profile.has_extension(*ext), "latest() must enable {ext:?}");
    }
    assert_eq!(ModelExtension::ALL.len(), 5);
}
