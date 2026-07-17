// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Tests covering the public `template::*` surface.
//!
//! After promoting `template` from `pub(crate)` to `pub mod` and
//! re-exporting the per-variant `JobParameterDefinition` inner
//! struct types, external callers can pattern-match on enum variants
//! and read per-variant fields by name. This file is a smoke test of
//! that surface — it exercises every type that is now nameable from
//! outside the crate, so a future change that accidentally narrows
//! the public surface (e.g. flipping a `pub use` back to
//! `#[cfg(test)]`) breaks the build here rather than silently
//! breaking a downstream consumer.

use openjd_model::decode_environment_template;
use openjd_model::decode_job_template;
use openjd_model::template::{
    Action, AmountRequirement, AttributeRequirement, BoolUserInterface, CancelationMode,
    ChunkIntTaskParameterDefinition, ChunksDefinition, Description, EmbeddedFile, Environment,
    EnvironmentActions, EnvironmentScript, EnvironmentTemplate, ExtensionName, FileFilter,
    FlexFloat, FlexInt, FloatRange, FloatRangeItem, FloatTaskParameterDefinition,
    FloatUserInterface, HiddenOnlyUserInterface, HostRequirements, IntOrFormatString, IntRange,
    IntTaskParameterDefinition, IntUserInterface, JobBoolParameterDefinition,
    JobFloatParameterDefinition, JobIntParameterDefinition, JobListBoolParameterDefinition,
    JobListFloatParameterDefinition, JobListIntParameterDefinition,
    JobListListIntParameterDefinition, JobListPathParameterDefinition,
    JobListStringParameterDefinition, JobParameterDefinition, JobPathParameterDefinition,
    JobRangeExprParameterDefinition, JobStringParameterDefinition, JobTemplate,
    ListFloatUserInterface, ListIntUserInterface, ListPathUserInterface, ListSimpleUserInterface,
    PathTaskParameterDefinition, PathUserInterface, RangeConstraint, RangeExprUserInterface,
    SimpleAction, StepActions, StepDependency, StepParameterSpaceDefinition, StepScript,
    StepTemplate, StringRange, StringTaskParameterDefinition, StringUserInterface,
    TaskParameterDefinition,
};
use openjd_model::CallerLimits;

fn yaml(s: &str) -> serde_json::Value {
    serde_saphyr::from_str(s).unwrap()
}

fn decode_jt(s: &str) -> JobTemplate {
    decode_job_template(yaml(s), None, &CallerLimits::default())
        .unwrap_or_else(|e| panic!("decode failure: {e}"))
}

fn decode_jt_with_extensions(s: &str, exts: &[&str]) -> JobTemplate {
    decode_job_template(yaml(s), Some(exts), &CallerLimits::default())
        .unwrap_or_else(|e| panic!("decode failure: {e}"))
}

fn decode_et(s: &str) -> EnvironmentTemplate {
    decode_environment_template(yaml(s), None).unwrap_or_else(|e| panic!("decode failure: {e}"))
}

#[test]
fn template_namespace_is_public() {
    // Smoke test: every type re-exported from `openjd_model::template::*`
    // must be nameable. The imports at the top of this file are the
    // real test — if any one of them stops being `pub use`d, the test
    // binary fails to compile.
    //
    // The `_` patterns below additionally exercise that we can write
    // function signatures that take `&template::StepTemplate` etc.
    fn _take_step(_: &StepTemplate) {}
    fn _take_env(_: &Environment) {}
    fn _take_action(_: &Action) {}
    fn _take_embedded(_: &EmbeddedFile) {}
    fn _take_step_script(_: &StepScript) {}
    fn _take_env_script(_: &EnvironmentScript) {}
    fn _take_step_actions(_: &StepActions) {}
    fn _take_env_actions(_: &EnvironmentActions) {}
    fn _take_cancelation(_: &CancelationMode) {}
    fn _take_host_req(_: &HostRequirements) {}
    fn _take_amount(_: &AmountRequirement) {}
    fn _take_attr(_: &AttributeRequirement) {}
    fn _take_dep(_: &StepDependency) {}
    fn _take_simple(_: &SimpleAction) {}
    fn _take_desc(_: &Description) {}
    fn _take_ext_name(_: &ExtensionName) {}
    fn _take_jpd(_: &JobParameterDefinition) {}
    fn _take_tpd(_: &TaskParameterDefinition) {}
    fn _take_rc(_: &RangeConstraint) {}
    // Per-variant TaskParameterDefinition struct types
    fn _take_int_tpd(_: &IntTaskParameterDefinition) {}
    fn _take_float_tpd(_: &FloatTaskParameterDefinition) {}
    fn _take_string_tpd(_: &StringTaskParameterDefinition) {}
    fn _take_path_tpd(_: &PathTaskParameterDefinition) {}
    fn _take_chunk_int_tpd(_: &ChunkIntTaskParameterDefinition) {}
    fn _take_chunks_def(_: &ChunksDefinition) {}
    fn _take_step_param_space_def(_: &StepParameterSpaceDefinition) {}
    // Range types
    fn _take_int_range(_: &IntRange) {}
    fn _take_string_range(_: &StringRange) {}
    fn _take_float_range(_: &FloatRange) {}
    fn _take_float_range_item(_: &FloatRangeItem) {}
    fn _take_int_or_fs(_: &IntOrFormatString) {}
    fn _take_flex_int(_: &FlexInt) {}
    fn _take_flex_float(_: &FlexFloat) {}
    // userInterface types
    fn _take_string_ui(_: &StringUserInterface) {}
    fn _take_int_ui(_: &IntUserInterface) {}
    fn _take_float_ui(_: &FloatUserInterface) {}
    fn _take_path_ui(_: &PathUserInterface) {}
    fn _take_bool_ui(_: &BoolUserInterface) {}
    fn _take_range_expr_ui(_: &RangeExprUserInterface) {}
    fn _take_list_simple_ui(_: &ListSimpleUserInterface) {}
    fn _take_list_path_ui(_: &ListPathUserInterface) {}
    fn _take_list_int_ui(_: &ListIntUserInterface) {}
    fn _take_list_float_ui(_: &ListFloatUserInterface) {}
    fn _take_hidden_only_ui(_: &HiddenOnlyUserInterface) {}
    fn _take_file_filter(_: &FileFilter) {}
}

#[test]
fn job_template_field_access() {
    let jt = decode_jt_with_extensions(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Foo",
        "description": "some text",
        "extensions": ["FEATURE_BUNDLE_1"],
        "steps": [
            {"name": "S0", "script": {"actions": {"onRun": {"command": "echo"}}}},
            {"name": "S1", "dependencies": [{"dependsOn": "S0"}],
             "script": {"actions": {"onRun": {"command": "echo"}}}}
        ]
    }"#,
        &["FEATURE_BUNDLE_1"],
    );

    // Top-level fields through the public surface.
    assert_eq!(jt.specification_version, "jobtemplate-2023-09");
    assert_eq!(jt.name.raw(), "Foo");
    assert_eq!(jt.description.as_ref().unwrap().0, "some text");
    let exts = jt.extensions.as_ref().unwrap();
    assert_eq!(exts.len(), 1);
    assert_eq!(exts[0].as_str(), "FEATURE_BUNDLE_1");

    // Steps as `Vec<StepTemplate>`.
    let steps: &Vec<StepTemplate> = &jt.steps;
    assert_eq!(steps.len(), 2);
    assert_eq!(steps[0].name, "S0");
    assert_eq!(steps[1].name, "S1");

    // StepDependency.
    let dep = &steps[1].dependencies.as_ref().unwrap()[0];
    assert_eq!(dep.depends_on, "S0");
}

#[test]
fn step_template_full_surface() {
    let jt = decode_jt(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Foo",
        "steps": [
            {
                "name": "S",
                "description": "a step",
                "stepEnvironments": [
                    {"name": "E", "variables": {"X": "1"}}
                ],
                "hostRequirements": {
                    "amounts": [{"name": "amount.worker.vcpu", "min": "4"}],
                    "attributes": [
                        {"name": "attr.worker.os.family", "anyOf": ["linux"]}
                    ]
                },
                "script": {
                    "actions": {
                        "onRun": {
                            "command": "echo",
                            "args": ["hi"],
                            "timeout": "60",
                            "cancelation": {
                                "mode": "NOTIFY_THEN_TERMINATE",
                                "notifyPeriodInSeconds": "30"
                            }
                        }
                    },
                    "embeddedFiles": [
                        {"name": "f", "type": "TEXT", "data": "hello"}
                    ]
                }
            }
        ]
    }"#,
    );
    let s = &jt.steps[0];

    assert_eq!(s.name, "S");
    assert_eq!(s.description.as_ref().unwrap().0, "a step");

    let envs = s.step_environments.as_ref().unwrap();
    assert_eq!(envs.len(), 1);
    assert_eq!(envs[0].name, "E");

    let hr: &HostRequirements = s.host_requirements.as_ref().unwrap();
    let amount: &AmountRequirement = &hr.amounts.as_ref().unwrap()[0];
    assert_eq!(amount.name, "amount.worker.vcpu");
    assert_eq!(amount.min.as_ref().unwrap().raw(), "4");
    let attr: &AttributeRequirement = &hr.attributes.as_ref().unwrap()[0];
    assert_eq!(attr.name, "attr.worker.os.family");
    assert_eq!(attr.any_of.as_ref().unwrap()[0].raw(), "linux");

    let script: &StepScript = s.script.as_ref().unwrap();
    let actions: &StepActions = &script.actions;
    let on_run: &Action = &actions.on_run;
    assert_eq!(on_run.command.raw(), "echo");
    assert_eq!(on_run.args.as_ref().unwrap()[0].raw(), "hi");
    assert_eq!(on_run.timeout.as_ref().unwrap().raw(), "60");

    match on_run.cancelation.as_ref().unwrap() {
        CancelationMode::NotifyThenTerminate {
            notify_period_in_seconds,
        } => {
            assert_eq!(notify_period_in_seconds.as_ref().unwrap().raw(), "30");
        }
        CancelationMode::Terminate | CancelationMode::DeferredMode { .. } => {
            panic!("expected NotifyThenTerminate")
        }
    }

    let ef: &EmbeddedFile = &script.embedded_files.as_ref().unwrap()[0];
    assert_eq!(ef.name, "f");
    assert_eq!(ef.data.as_ref().unwrap().raw(), "hello");
}

#[test]
fn simple_action_sugar_field_access() {
    let jt = decode_jt_with_extensions(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Foo",
        "extensions": ["FEATURE_BUNDLE_1"],
        "steps": [
            {"name": "S", "bash": {"script": "echo hi"}}
        ]
    }"#,
        &["FEATURE_BUNDLE_1"],
    );

    let s = &jt.steps[0];
    assert!(s.script.is_none());
    let bash: &SimpleAction = s.bash.as_ref().unwrap();
    assert_eq!(bash.script, "echo hi");
    assert!(s.python.is_none());
}

#[test]
fn environment_template_field_access() {
    let et = decode_et(
        r#"{
        "specificationVersion": "environment-2023-09",
        "environment": {
            "name": "EnvX",
            "description": "an env",
            "variables": {"K": "v"},
            "script": {
                "actions": {"onEnter": {"command": "echo"}}
            }
        }
    }"#,
    );
    assert_eq!(et.specification_version, "environment-2023-09");
    let env: &Environment = &et.environment;
    assert_eq!(env.name, "EnvX");
    assert_eq!(env.description.as_ref().unwrap().0, "an env");
    let vars = env.variables.as_ref().unwrap();
    assert_eq!(vars["K"].raw(), "v");
    let escr: &EnvironmentScript = env.script.as_ref().unwrap();
    let eactions: &EnvironmentActions = &escr.actions;
    let on_enter: &Action = eactions.on_enter.as_ref().unwrap();
    assert_eq!(on_enter.command.raw(), "echo");
    assert!(eactions.on_exit.is_none());
}

// ── JobParameterDefinition variant pattern-matching ──

#[test]
fn job_parameter_definition_string_variant() {
    let jt = decode_jt(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Foo",
        "parameterDefinitions": [{
            "name": "S",
            "type": "STRING",
            "description": "a string",
            "default": "hello",
            "allowedValues": ["hello", "world"],
            "minLength": 1,
            "maxLength": 100
        }],
        "steps": [{"name": "Step", "script": {"actions": {"onRun": {"command": "echo"}}}}]
    }"#,
    );

    let defs = jt.parameter_definitions.as_ref().unwrap();
    match &defs[0] {
        JobParameterDefinition::STRING(p) => {
            let _: &JobStringParameterDefinition = p;
            assert_eq!(p.name.as_str(), "S");
            assert_eq!(p.description.as_ref().unwrap().0, "a string");
            assert_eq!(p.default.as_deref(), Some("hello"));
            let allowed = p.allowed_values.as_ref().unwrap();
            assert_eq!(allowed, &vec!["hello".to_string(), "world".to_string()]);
            assert_eq!(p.min_length, Some(1));
            assert_eq!(p.max_length, Some(100));
        }
        other => panic!("expected STRING, got {other:?}"),
    }
}

#[test]
fn job_parameter_definition_int_variant() {
    let jt = decode_jt(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Foo",
        "parameterDefinitions": [{
            "name": "I",
            "type": "INT",
            "default": 42,
            "minValue": 1,
            "maxValue": 100,
            "allowedValues": [1, 2, 42]
        }],
        "steps": [{"name": "Step", "script": {"actions": {"onRun": {"command": "echo"}}}}]
    }"#,
    );

    let defs = jt.parameter_definitions.as_ref().unwrap();
    match &defs[0] {
        JobParameterDefinition::INT(p) => {
            let _: &JobIntParameterDefinition = p;
            assert_eq!(p.name.as_str(), "I");
            assert_eq!(p.default.as_ref().map(|f| f.0), Some(42));
            assert_eq!(p.min_value.as_ref().map(|f| f.0), Some(1));
            assert_eq!(p.max_value.as_ref().map(|f| f.0), Some(100));
            let allowed = p.allowed_values.as_ref().unwrap();
            let nums: Vec<i64> = allowed.iter().map(|f| f.0).collect();
            assert_eq!(nums, vec![1, 2, 42]);
        }
        other => panic!("expected INT, got {other:?}"),
    }
}

#[test]
fn job_parameter_definition_float_variant() {
    let jt = decode_jt(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Foo",
        "parameterDefinitions": [{
            "name": "F",
            "type": "FLOAT",
            "default": 2.5,
            "minValue": 0.0,
            "maxValue": 10.0
        }],
        "steps": [{"name": "Step", "script": {"actions": {"onRun": {"command": "echo"}}}}]
    }"#,
    );

    let defs = jt.parameter_definitions.as_ref().unwrap();
    match &defs[0] {
        JobParameterDefinition::FLOAT(p) => {
            let _: &JobFloatParameterDefinition = p;
            assert_eq!(p.name.as_str(), "F");
            assert_eq!(p.default.as_ref().map(|f| f.0), Some(2.5));
            assert_eq!(p.min_value.as_ref().map(|f| f.0), Some(0.0));
            assert_eq!(p.max_value.as_ref().map(|f| f.0), Some(10.0));
        }
        other => panic!("expected FLOAT, got {other:?}"),
    }
}

#[test]
fn job_parameter_definition_path_variant() {
    let jt = decode_jt(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Foo",
        "parameterDefinitions": [{
            "name": "P",
            "type": "PATH",
            "default": "/tmp/x",
            "objectType": "FILE",
            "dataFlow": "IN"
        }],
        "steps": [{"name": "Step", "script": {"actions": {"onRun": {"command": "echo"}}}}]
    }"#,
    );

    let defs = jt.parameter_definitions.as_ref().unwrap();
    match &defs[0] {
        JobParameterDefinition::PATH(p) => {
            let _: &JobPathParameterDefinition = p;
            assert_eq!(p.name.as_str(), "P");
            assert_eq!(p.default.as_deref(), Some("/tmp/x"));
            assert!(p.object_type.is_some());
            assert!(p.data_flow.is_some());
        }
        other => panic!("expected PATH, got {other:?}"),
    }
}

// ── EXPR-extension job parameter variants ──

#[test]
fn job_parameter_definition_bool_variant() {
    let jt = decode_jt_with_extensions(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Foo",
        "extensions": ["EXPR"],
        "parameterDefinitions": [{"name": "B", "type": "BOOL", "default": true}],
        "steps": [{"name": "Step", "script": {"actions": {"onRun": {"command": "echo"}}}}]
    }"#,
        &["EXPR"],
    );

    let defs = jt.parameter_definitions.as_ref().unwrap();
    match &defs[0] {
        JobParameterDefinition::BOOL(p) => {
            let _: &JobBoolParameterDefinition = p;
            assert_eq!(p.name.as_str(), "B");
            assert_eq!(p.default.as_ref().map(|b| b.0), Some(true));
        }
        other => panic!("expected BOOL, got {other:?}"),
    }
}

#[test]
fn job_parameter_definition_range_expr_variant() {
    let jt = decode_jt_with_extensions(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Foo",
        "extensions": ["EXPR"],
        "parameterDefinitions": [{
            "name": "R",
            "type": "RANGE_EXPR",
            "default": "1-10",
            "minLength": 1,
            "maxLength": 100
        }],
        "steps": [{"name": "Step", "script": {"actions": {"onRun": {"command": "echo"}}}}]
    }"#,
        &["EXPR"],
    );

    let defs = jt.parameter_definitions.as_ref().unwrap();
    match &defs[0] {
        JobParameterDefinition::RANGE_EXPR(p) => {
            let _: &JobRangeExprParameterDefinition = p;
            assert_eq!(p.name.as_str(), "R");
            assert_eq!(p.default.as_deref(), Some("1-10"));
            assert_eq!(p.min_length, Some(1));
            assert_eq!(p.max_length, Some(100));
        }
        other => panic!("expected RANGE_EXPR, got {other:?}"),
    }
}

#[test]
fn job_parameter_definition_list_variants() {
    let jt = decode_jt_with_extensions(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Foo",
        "extensions": ["EXPR"],
        "parameterDefinitions": [
            {"name": "LS", "type": "LIST[STRING]", "default": ["a", "b"]},
            {"name": "LP", "type": "LIST[PATH]", "default": ["/x"], "objectType": "DIRECTORY", "dataFlow": "OUT"},
            {"name": "LI", "type": "LIST[INT]", "default": [1, 2, 3]},
            {"name": "LF", "type": "LIST[FLOAT]", "default": [1.1, 2.2]},
            {"name": "LB", "type": "LIST[BOOL]", "default": [true, false]},
            {"name": "LLI", "type": "LIST[LIST[INT]]", "default": [[1, 2], [3, 4]]}
        ],
        "steps": [{"name": "Step", "script": {"actions": {"onRun": {"command": "echo"}}}}]
    }"#,
        &["EXPR"],
    );

    let defs = jt.parameter_definitions.as_ref().unwrap();
    assert_eq!(defs.len(), 6);

    match &defs[0] {
        JobParameterDefinition::LIST_STRING(p) => {
            let _: &JobListStringParameterDefinition = p;
            assert_eq!(
                p.default.as_ref().unwrap(),
                &vec!["a".to_string(), "b".to_string()]
            );
        }
        other => panic!("expected LIST_STRING, got {other:?}"),
    }
    match &defs[1] {
        JobParameterDefinition::LIST_PATH(p) => {
            let _: &JobListPathParameterDefinition = p;
            assert!(p.object_type.is_some());
            assert!(p.data_flow.is_some());
        }
        other => panic!("expected LIST_PATH, got {other:?}"),
    }
    match &defs[2] {
        JobParameterDefinition::LIST_INT(p) => {
            let _: &JobListIntParameterDefinition = p;
            let nums: Vec<i64> = p.default.as_ref().unwrap().iter().map(|f| f.0).collect();
            assert_eq!(nums, vec![1, 2, 3]);
        }
        other => panic!("expected LIST_INT, got {other:?}"),
    }
    match &defs[3] {
        JobParameterDefinition::LIST_FLOAT(p) => {
            let _: &JobListFloatParameterDefinition = p;
            let nums: Vec<f64> = p.default.as_ref().unwrap().iter().map(|f| f.0).collect();
            assert_eq!(nums, vec![1.1, 2.2]);
        }
        other => panic!("expected LIST_FLOAT, got {other:?}"),
    }
    match &defs[4] {
        JobParameterDefinition::LIST_BOOL(p) => {
            let _: &JobListBoolParameterDefinition = p;
            let bools: Vec<bool> = p.default.as_ref().unwrap().iter().map(|b| b.0).collect();
            assert_eq!(bools, vec![true, false]);
        }
        other => panic!("expected LIST_BOOL, got {other:?}"),
    }
    match &defs[5] {
        JobParameterDefinition::LIST_LIST_INT(p) => {
            let _: &JobListListIntParameterDefinition = p;
            let nums: Vec<Vec<i64>> = p
                .default
                .as_ref()
                .unwrap()
                .iter()
                .map(|inner| inner.iter().map(|f| f.0).collect())
                .collect();
            assert_eq!(nums, vec![vec![1, 2], vec![3, 4]]);
        }
        other => panic!("expected LIST_LIST_INT, got {other:?}"),
    }
}

#[test]
fn function_signature_takes_template_types_by_reference() {
    // Verify `&template::*` types are usable in function signatures.
    fn count_args(action: &Action) -> usize {
        action.args.as_ref().map(|a| a.len()).unwrap_or(0)
    }

    fn step_name(step: &StepTemplate) -> &str {
        &step.name
    }

    let jt = decode_jt(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Foo",
        "steps": [{
            "name": "Hello",
            "script": {"actions": {"onRun": {"command": "echo", "args": ["a", "b", "c"]}}}
        }]
    }"#,
    );

    assert_eq!(step_name(&jt.steps[0]), "Hello");
    let on_run = &jt.steps[0].script.as_ref().unwrap().actions.on_run;
    assert_eq!(count_args(on_run), 3);
}

// ─── TaskParameterDefinition variants — field access ──────────────────

#[test]
fn task_parameter_definition_int_variant_field_access() {
    let jt = decode_jt(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Foo",
        "steps": [{
            "name": "S",
            "parameterSpace": {
                "taskParameterDefinitions": [
                    {"name": "Frame", "type": "INT", "range": [1, 2, 3]}
                ]
            },
            "script": {"actions": {"onRun": {"command": "echo"}}}
        }]
    }"#,
    );
    let ps: &StepParameterSpaceDefinition = jt.steps[0].parameter_space.as_ref().unwrap();
    assert_eq!(ps.task_parameter_definitions.len(), 1);
    match &ps.task_parameter_definitions[0] {
        TaskParameterDefinition::INT(p) => {
            let _: &IntTaskParameterDefinition = p;
            assert_eq!(p.name.as_str(), "Frame");
            match &p.range {
                IntRange::List(items) => {
                    let nums: Vec<i64> = items.iter().map(|f| f.0).collect();
                    assert_eq!(nums, vec![1, 2, 3]);
                }
                IntRange::Expression(_) => panic!("expected List, got Expression"),
            }
        }
        other => panic!("expected INT, got {other:?}"),
    }
}

#[test]
fn task_parameter_definition_int_range_expression() {
    let jt = decode_jt(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Foo",
        "steps": [{
            "name": "S",
            "parameterSpace": {
                "taskParameterDefinitions": [
                    {"name": "Frame", "type": "INT", "range": "1-10:2"}
                ]
            },
            "script": {"actions": {"onRun": {"command": "echo"}}}
        }]
    }"#,
    );
    match &jt.steps[0]
        .parameter_space
        .as_ref()
        .unwrap()
        .task_parameter_definitions[0]
    {
        TaskParameterDefinition::INT(p) => match &p.range {
            IntRange::Expression(fs) => {
                assert_eq!(fs.raw(), "1-10:2");
            }
            IntRange::List(_) => panic!("expected Expression, got List"),
        },
        _ => unreachable!(),
    }
}

#[test]
fn task_parameter_definition_float_variant_field_access() {
    let jt = decode_jt_with_extensions(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Foo",
        "extensions": ["EXPR"],
        "steps": [{
            "name": "S",
            "parameterSpace": {
                "taskParameterDefinitions": [
                    {"name": "F", "type": "FLOAT", "range": [1.5, "{{Param.X}}", 3.5]}
                ]
            },
            "script": {"actions": {"onRun": {"command": "echo"}}}
        }]
    }"#,
        &["EXPR"],
    );
    match &jt.steps[0]
        .parameter_space
        .as_ref()
        .unwrap()
        .task_parameter_definitions[0]
    {
        TaskParameterDefinition::FLOAT(p) => {
            let _: &FloatTaskParameterDefinition = p;
            match &p.range {
                FloatRange::List(items) => {
                    assert_eq!(items.len(), 3);
                    assert!(matches!(items[0], FloatRangeItem::Float(f) if f == 1.5));
                    assert!(matches!(&items[1], FloatRangeItem::FormatString(_)));
                    assert!(matches!(items[2], FloatRangeItem::Float(f) if f == 3.5));
                }
                FloatRange::Expression(_) => panic!("expected List"),
            }
        }
        _ => unreachable!(),
    }
}

#[test]
fn task_parameter_definition_string_path_variants() {
    let jt = decode_jt(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Foo",
        "steps": [{
            "name": "S",
            "parameterSpace": {
                "taskParameterDefinitions": [
                    {"name": "Color", "type": "STRING", "range": ["red", "blue"]},
                    {"name": "OutDir", "type": "PATH", "range": ["/tmp/a", "/tmp/b"]}
                ]
            },
            "script": {"actions": {"onRun": {"command": "echo"}}}
        }]
    }"#,
    );
    let defs = &jt.steps[0]
        .parameter_space
        .as_ref()
        .unwrap()
        .task_parameter_definitions;
    match &defs[0] {
        TaskParameterDefinition::STRING(p) => {
            let _: &StringTaskParameterDefinition = p;
            match &p.range {
                StringRange::List(items) => {
                    let strs: Vec<&str> = items.iter().map(|fs| fs.raw()).collect();
                    assert_eq!(strs, vec!["red", "blue"]);
                }
                StringRange::Expression(_) => panic!("expected List"),
            }
        }
        _ => unreachable!(),
    }
    match &defs[1] {
        TaskParameterDefinition::PATH(p) => {
            let _: &PathTaskParameterDefinition = p;
            match &p.range {
                StringRange::List(items) => {
                    let strs: Vec<&str> = items.iter().map(|fs| fs.raw()).collect();
                    assert_eq!(strs, vec!["/tmp/a", "/tmp/b"]);
                }
                StringRange::Expression(_) => panic!("expected List"),
            }
        }
        _ => unreachable!(),
    }
}

#[test]
fn task_parameter_definition_chunk_int_with_chunks_definition() {
    let jt = decode_jt_with_extensions(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Foo",
        "extensions": ["TASK_CHUNKING"],
        "steps": [{
            "name": "S",
            "parameterSpace": {
                "taskParameterDefinitions": [
                    {
                        "name": "F",
                        "type": "CHUNK[INT]",
                        "range": "1-100",
                        "chunks": {
                            "defaultTaskCount": 4,
                            "targetRuntimeSeconds": 60,
                            "rangeConstraint": "CONTIGUOUS"
                        }
                    }
                ]
            },
            "script": {"actions": {"onRun": {"command": "echo"}}}
        }]
    }"#,
        &["TASK_CHUNKING"],
    );
    match &jt.steps[0]
        .parameter_space
        .as_ref()
        .unwrap()
        .task_parameter_definitions[0]
    {
        TaskParameterDefinition::CHUNK_INT(p) => {
            let _: &ChunkIntTaskParameterDefinition = p;
            assert!(matches!(&p.range, IntRange::Expression(_)));
            let chunks: &ChunksDefinition = &p.chunks;
            // default_task_count: literal int → IntOrFormatString::Int
            match &chunks.default_task_count {
                IntOrFormatString::Int(n) => assert_eq!(*n, 4),
                IntOrFormatString::FormatString(_) => panic!("expected Int"),
            }
            // target_runtime_seconds: Some(literal int)
            match chunks.target_runtime_seconds.as_ref().unwrap() {
                IntOrFormatString::Int(n) => assert_eq!(*n, 60),
                IntOrFormatString::FormatString(_) => panic!("expected Int"),
            }
            assert_eq!(chunks.range_constraint, RangeConstraint::Contiguous);
        }
        _ => unreachable!(),
    }
}

#[test]
fn chunks_definition_format_string_default_task_count() {
    let jt = decode_jt_with_extensions(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Foo",
        "extensions": ["TASK_CHUNKING", "EXPR"],
        "parameterDefinitions": [
            {"name": "ChunkSize", "type": "INT", "default": 10}
        ],
        "steps": [{
            "name": "S",
            "parameterSpace": {
                "taskParameterDefinitions": [
                    {
                        "name": "F",
                        "type": "CHUNK[INT]",
                        "range": "1-100",
                        "chunks": {
                            "defaultTaskCount": "{{Param.ChunkSize}}",
                            "rangeConstraint": "NONCONTIGUOUS"
                        }
                    }
                ]
            },
            "script": {"actions": {"onRun": {"command": "echo"}}}
        }]
    }"#,
        &["TASK_CHUNKING", "EXPR"],
    );
    match &jt.steps[0]
        .parameter_space
        .as_ref()
        .unwrap()
        .task_parameter_definitions[0]
    {
        TaskParameterDefinition::CHUNK_INT(p) => {
            match &p.chunks.default_task_count {
                IntOrFormatString::FormatString(fs) => {
                    assert_eq!(fs.raw(), "{{Param.ChunkSize}}");
                }
                IntOrFormatString::Int(_) => panic!("expected FormatString"),
            }
            assert!(p.chunks.target_runtime_seconds.is_none());
            assert_eq!(p.chunks.range_constraint, RangeConstraint::Noncontiguous);
        }
        _ => unreachable!(),
    }
}

// ─── *UserInterface — field access ────────────────────────────────────

#[test]
fn job_int_parameter_definition_user_interface_field_access() {
    let jt = decode_jt(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Foo",
        "parameterDefinitions": [{
            "name": "Frame",
            "type": "INT",
            "default": 1,
            "userInterface": {
                "control": "SPIN_BOX",
                "label": "Frame",
                "groupLabel": "Animation",
                "singleStepDelta": 5
            }
        }],
        "steps": [{"name": "S", "script": {"actions": {"onRun": {"command": "echo"}}}}]
    }"#,
    );
    let defs = jt.parameter_definitions.as_ref().unwrap();
    match &defs[0] {
        JobParameterDefinition::INT(p) => {
            let ui: &IntUserInterface = p.user_interface.as_ref().unwrap();
            assert_eq!(ui.control.as_deref(), Some("SPIN_BOX"));
            assert_eq!(ui.label.as_deref(), Some("Frame"));
            assert_eq!(ui.group_label.as_deref(), Some("Animation"));
            assert_eq!(ui.single_step_delta.as_ref().map(|f| f.0), Some(5));
        }
        _ => unreachable!(),
    }
}

#[test]
fn job_path_parameter_definition_user_interface_with_file_filters() {
    let jt = decode_jt(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Foo",
        "parameterDefinitions": [{
            "name": "InputFile",
            "type": "PATH",
            "objectType": "FILE",
            "dataFlow": "IN",
            "userInterface": {
                "control": "CHOOSE_INPUT_FILE",
                "label": "Input file",
                "fileFilters": [
                    {"label": "Images", "patterns": ["*.png", "*.jpg"]}
                ],
                "fileFilterDefault": {"label": "Images", "patterns": ["*.png", "*.jpg"]}
            }
        }],
        "steps": [{"name": "S", "script": {"actions": {"onRun": {"command": "echo"}}}}]
    }"#,
    );
    let defs = jt.parameter_definitions.as_ref().unwrap();
    match &defs[0] {
        JobParameterDefinition::PATH(p) => {
            let ui: &PathUserInterface = p.user_interface.as_ref().unwrap();
            assert_eq!(ui.control.as_deref(), Some("CHOOSE_INPUT_FILE"));
            let filters = ui.file_filters.as_ref().unwrap();
            assert_eq!(filters.len(), 1);
            let f0: &FileFilter = &filters[0];
            assert_eq!(f0.label, "Images");
            assert_eq!(f0.patterns, vec!["*.png".to_string(), "*.jpg".to_string()]);
            // Default filter is also exposed.
            let default: &FileFilter = ui.file_filter_default.as_ref().unwrap();
            assert_eq!(default.label, "Images");
        }
        _ => unreachable!(),
    }
}

#[test]
fn job_float_parameter_definition_user_interface_with_decimals() {
    let jt = decode_jt(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Foo",
        "parameterDefinitions": [{
            "name": "X",
            "type": "FLOAT",
            "userInterface": {
                "control": "SPIN_BOX",
                "decimals": 3,
                "singleStepDelta": 0.25
            }
        }],
        "steps": [{"name": "S", "script": {"actions": {"onRun": {"command": "echo"}}}}]
    }"#,
    );
    match &jt.parameter_definitions.as_ref().unwrap()[0] {
        JobParameterDefinition::FLOAT(p) => {
            let ui: &FloatUserInterface = p.user_interface.as_ref().unwrap();
            assert_eq!(ui.decimals.as_ref().map(|f| f.0), Some(3));
            assert_eq!(ui.single_step_delta.as_ref().map(|f| f.0), Some(0.25));
        }
        _ => unreachable!(),
    }
}
