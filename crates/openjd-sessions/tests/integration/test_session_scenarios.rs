// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! YAML-based session scenario tests — mirrors Python test_session_scenarios.py.
//!
//! Each scenario YAML references a job template and specifies parameters + expectations.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use openjd_model::create_job::{create_job, preprocess_job_parameters};
use openjd_model::step_param_space::StepParameterSpaceIterator;
use openjd_model::template::parse::decode_job_template;
use openjd_model::types::JobParameterInputValues;
use openjd_sessions::session::{Session, SessionConfig, SessionState};
use openjd_sessions::PathMappingRule;

fn scenarios_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/scenarios")
}

#[derive(Debug, serde::Deserialize)]
struct Scenario {
    name: String,
    #[serde(default)]
    run_on: String,
    job_template_file: String,
    #[serde(default)]
    job_parameters: serde_json::Value,
    #[serde(default)]
    path_mapping_rules: Vec<serde_json::Value>,
    #[serde(default)]
    step: Option<String>,
    #[serde(default)]
    expect: Expect,
    #[serde(default)]
    expect_posix: Option<Expect>,
    #[serde(default)]
    expect_windows: Option<Expect>,
}

#[derive(Debug, Default, serde::Deserialize)]
struct Expect {
    #[serde(default = "default_true")]
    success: bool,
    #[serde(default)]
    output_contains: Vec<String>,
    #[serde(default)]
    output_excludes: Vec<String>,
}

fn default_true() -> bool {
    true
}

fn should_run(run_on: &str) -> bool {
    match run_on {
        "" | "all" => true,
        "posix" => cfg!(unix),
        "windows" => cfg!(windows),
        _ => false,
    }
}

fn yaml_to_input_values(val: &serde_json::Value) -> JobParameterInputValues {
    let mut result = HashMap::new();
    if let Some(map) = val.as_object() {
        for (k, v) in map {
            let expr_val = yaml_to_expr_value(v);
            result.insert(k.clone(), expr_val);
        }
    }
    result
}

fn yaml_to_expr_value(v: &serde_json::Value) -> openjd_expr::ExprValue {
    match v {
        serde_json::Value::String(s) => openjd_expr::ExprValue::String(s.clone()),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                openjd_expr::ExprValue::Int(i)
            } else if let Some(f) = n.as_f64() {
                openjd_expr::ExprValue::Float(openjd_expr::value::Float64::new(f).unwrap())
            } else {
                openjd_expr::ExprValue::String(n.to_string())
            }
        }
        serde_json::Value::Bool(b) => openjd_expr::ExprValue::Bool(*b),
        serde_json::Value::Array(seq) => {
            let items: Vec<openjd_expr::ExprValue> = seq.iter().map(yaml_to_expr_value).collect();
            if items.is_empty() {
                return openjd_expr::ExprValue::make_list(vec![], openjd_expr::ExprType::STRING)
                    .unwrap();
            }
            // Infer list type from first element
            let elem_type = match &items[0] {
                openjd_expr::ExprValue::Int(_) => openjd_expr::ExprType::INT,
                openjd_expr::ExprValue::Float(_) => openjd_expr::ExprType::FLOAT,
                openjd_expr::ExprValue::Bool(_) => openjd_expr::ExprType::BOOL,
                openjd_expr::ExprValue::Path { .. } => openjd_expr::ExprType::PATH,
                openjd_expr::ExprValue::ListInt(_) => {
                    openjd_expr::ExprType::list(openjd_expr::ExprType::INT)
                }
                _ => openjd_expr::ExprType::STRING,
            };
            openjd_expr::ExprValue::make_list(items, elem_type).unwrap()
        }
        _ => openjd_expr::ExprValue::String(String::new()),
    }
}

fn parse_path_mapping_rules(rules: &[serde_json::Value]) -> Vec<PathMappingRule> {
    rules
        .iter()
        .filter_map(|r| {
            let map = r.as_object()?;
            let fmt = map.get("source_path_format")?.as_str()?;
            let src = map.get("source_path")?.as_str()?;
            let dst = map.get("destination_path")?.as_str()?;
            let format = match fmt.to_uppercase().as_str() {
                "POSIX" => openjd_sessions::PathFormat::Posix,
                "WINDOWS" => openjd_sessions::PathFormat::Windows,
                _ => return None,
            };
            Some(PathMappingRule {
                source_path_format: format,
                source_path: src.to_string(),
                destination_path: dst.to_string(),
            })
        })
        .collect()
}

async fn run_scenario(scenario_path: &Path) {
    let scenario_text = std::fs::read_to_string(scenario_path).unwrap();
    let scenario: Scenario = serde_saphyr::from_str(&scenario_text).unwrap();

    if !should_run(&scenario.run_on) {
        eprintln!(
            "Skipping scenario '{}' (run_on={})",
            scenario.name, scenario.run_on
        );
        return;
    }

    // Load template
    let template_path = scenario_path
        .parent()
        .unwrap()
        .join(&scenario.job_template_file);
    let template_text = std::fs::read_to_string(&template_path).unwrap();
    let template_yaml: serde_json::Value = serde_saphyr::from_str(&template_text).unwrap();

    // Get extensions
    let extensions: Vec<String> = template_yaml
        .get("extensions")
        .and_then(|v| v.as_array())
        .map(|seq| {
            seq.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    let ext_refs: Vec<&str> = extensions.iter().map(|s| s.as_str()).collect();

    // Decode template
    let job_template = decode_job_template(
        template_yaml,
        Some(&ext_refs),
        &openjd_model::CallerLimits::default(),
    )
    .unwrap_or_else(|e| panic!("Failed to decode template for '{}': {e}", scenario.name));

    // Build parameter values
    let input_values = yaml_to_input_values(&scenario.job_parameters);

    // Path mapping rules
    let path_rules = parse_path_mapping_rules(&scenario.path_mapping_rules);

    // Derive path_format from the source_path_format of the first mapping rule,
    // since input paths are in the source format. Fall back to Posix because
    // scenario templates use forward-slash paths regardless of host OS.
    let path_format = path_rules
        .first()
        .map(|r| match r.source_path_format {
            openjd_sessions::PathFormat::Windows => openjd_expr::path_mapping::PathFormat::Windows,
            openjd_sessions::PathFormat::Posix | openjd_sessions::PathFormat::Uri => {
                openjd_expr::path_mapping::PathFormat::Posix
            }
        })
        .unwrap_or(openjd_expr::path_mapping::PathFormat::Posix);

    let job_params = preprocess_job_parameters(
        &job_template,
        &input_values,
        &[],
        &openjd_model::PathParameterOptions {
            job_template_dir: "",
            current_working_dir: "",
            path_format,
            allow_template_dir_walk_up: true,
            allow_uri_path_values: true,
        },
    )
    .unwrap_or_else(|e| panic!("Failed to preprocess params for '{}': {e}", scenario.name));

    // Create job
    let ctx = {
        let mut exts = std::collections::HashSet::new();
        if let Some(ext_list) = &job_template.extensions {
            exts.extend(
                ext_list
                    .iter()
                    .filter_map(|e| e.as_str().parse::<openjd_model::ModelExtension>().ok()),
            );
        }
        openjd_model::ValidationContext::with_extensions(
            openjd_model::SpecificationRevision::V2023_09,
            exts,
        )
    };
    let job = create_job(&job_template, &job_params, &ctx)
        .unwrap_or_else(|e| panic!("Failed to create job for '{}': {e}", scenario.name));

    // Select step
    let step = if let Some(ref step_name) = scenario.step {
        job.steps
            .iter()
            .find(|s| s.name == *step_name)
            .unwrap_or_else(|| panic!("Step '{}' not found in '{}'", step_name, scenario.name))
    } else {
        &job.steps[0]
    };

    // Build session
    let tmp = tempfile::TempDir::new().unwrap();

    // Build model profile for extensions
    let profile = if !extensions.is_empty() {
        Some(
            openjd_model::ModelProfile::new(openjd_model::types::SpecificationRevision::V2023_09)
                .with_extensions(
                    extensions
                        .iter()
                        .filter_map(|s| s.parse::<openjd_model::types::ModelExtension>().ok())
                        .collect(),
                ),
        )
    } else {
        None
    };

    let config = SessionConfig {
        session_id: "scenario-test".into(),
        job_parameter_values: job_params.clone(),
        path_mapping_rules: if path_rules.is_empty() {
            None
        } else {
            Some(path_rules)
        },
        retain_working_dir: false,
        callback: None,
        os_env_vars: None,
        session_root_directory: Some(tmp.path().to_path_buf()),
        user: None,
        profile,
        cancel_token: None,
        debug_collect_stdout: true,
        echo_openjd_directives: true,
        sticky_bit_policy: openjd_sessions::StickyBitPolicy::Disabled,
    };
    let mut session = Session::with_config(config).unwrap();

    // Session automatically wraps its library with host-context using the
    // configured path-mapping rules; no need to register manually here.

    let mut all_output = Vec::new();
    // Use a shared vec to capture output from callbacks
    let _captured = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));

    // Enter job environments — each environment carries its own resolved_symtab
    // filtered to only the symbols its format strings reference.
    let mut job_env_entries: Vec<(String, Option<openjd_expr::SerializedSymbolTable>)> = Vec::new();
    if let Some(ref envs) = job.job_environments {
        for env in envs {
            match session
                .enter_environment_with_output(env, env.resolved_symtab.as_ref(), None, None)
                .await
            {
                Ok((id, stdout)) => {
                    job_env_entries.push((id, env.resolved_symtab.clone()));
                    for line in stdout.lines() {
                        all_output.push(line.to_string());
                    }
                }
                Err(e) => {
                    all_output.push(format!("openjd_fail: {e}"));
                    break;
                }
            }
        }
    }

    // Enter step environments
    let mut step_env_ids = Vec::new();
    if session.state() == SessionState::Ready {
        if let Some(ref envs) = step.step_environments {
            for env in envs {
                match session
                    .enter_environment_with_output(env, step.resolved_symtab.as_ref(), None, None)
                    .await
                {
                    Ok((id, stdout)) => {
                        step_env_ids.push(id);
                        for line in stdout.lines() {
                            all_output.push(line.to_string());
                        }
                    }
                    Err(e) => {
                        all_output.push(format!("openjd_fail: {e}"));
                        break;
                    }
                }
            }
        }
    }

    // Run tasks
    if session.state() == SessionState::Ready {
        if let Some(ref space) = step.parameter_space {
            let iter = StepParameterSpaceIterator::new(space).unwrap();
            for task_params in iter {
                let result = session
                    .run_task(
                        "test_step",
                        &step.script,
                        Some(&task_params),
                        step.resolved_symtab.as_ref(),
                        None,
                    )
                    .await;
                match result {
                    Ok(r) => {
                        for line in r.stdout.lines() {
                            all_output.push(line.to_string());
                        }
                    }
                    Err(e) => {
                        all_output.push(format!("openjd_fail: {e}"));
                        break;
                    }
                }
            }
        } else {
            // No parameter space — run once with no task params
            let result = session
                .run_task(
                    "test_step",
                    &step.script,
                    None,
                    step.resolved_symtab.as_ref(),
                    None,
                )
                .await;
            match result {
                Ok(r) => {
                    for line in r.stdout.lines() {
                        all_output.push(line.to_string());
                    }
                }
                Err(e) => all_output.push(format!("openjd_fail: {e}")),
            }
        }
    }

    // Exit step environments (reverse order)
    for id in step_env_ids.iter().rev() {
        if session.state() == SessionState::Ready || session.state() == SessionState::ReadyEnding {
            let _ = session
                .exit_environment(id, step.resolved_symtab.as_ref(), true, None)
                .await
                .map(|out| {
                    for line in out.lines() {
                        all_output.push(line.to_string());
                    }
                });
        }
    }

    // Exit job environments (reverse order) — use each env's own resolved_symtab
    for (id, resolved) in job_env_entries.iter().rev() {
        if session.state() == SessionState::Ready || session.state() == SessionState::ReadyEnding {
            let _ = session
                .exit_environment(id, resolved.as_ref(), true, None)
                .await
                .map(|out| {
                    for line in out.lines() {
                        all_output.push(line.to_string());
                    }
                });
        }
    }

    session.cleanup();

    // Verify expectations
    let mut expect = scenario.expect;
    if cfg!(unix) {
        if let Some(pe) = scenario.expect_posix {
            expect.output_contains.extend(pe.output_contains);
            expect.output_excludes.extend(pe.output_excludes);
        }
    }
    if cfg!(windows) {
        if let Some(we) = scenario.expect_windows {
            expect.output_contains.extend(we.output_contains);
            expect.output_excludes.extend(we.output_excludes);
        }
    }

    if expect.success {
        for msg in &all_output {
            assert!(
                !msg.to_lowercase().contains("openjd_fail:"),
                "Scenario '{}': Unexpected failure: {msg}",
                scenario.name
            );
        }
    }

    for pattern in &expect.output_contains {
        let found = all_output.iter().any(|msg| msg.contains(pattern));
        assert!(
            found,
            "Scenario '{}': Expected output containing '{}' not found.\nAll output:\n{}",
            scenario.name,
            pattern,
            all_output.join("\n")
        );
    }

    for pattern in &expect.output_excludes {
        let found = all_output.iter().any(|msg| msg.contains(pattern));
        assert!(
            !found,
            "Scenario '{}': Unexpected output containing '{}'",
            scenario.name, pattern
        );
    }
}

// Generate one test per scenario file
macro_rules! scenario_test {
    ($name:ident, $path:expr) => {
        #[tokio::test]
        async fn $name() {
            let path = scenarios_dir().join($path);
            run_scenario(&path).await;
        }
    };
    (ignore: $name:ident, $path:expr) => {
        #[tokio::test]
        #[ignore = "blocked by openjd-expr limitation"]
        async fn $name() {
            let path = scenarios_dir().join($path);
            run_scenario(&path).await;
        }
    };
}

// let_bindings scenarios
scenario_test!(
    scenario_let_bindings,
    "let_bindings/let_bindings_scenario.yaml"
);
scenario_test!(
    scenario_let_host_context,
    "let_bindings/let_host_context_scenario.yaml"
);
scenario_test!(
    scenario_step_let_in_step_env,
    "let_bindings/step_let_in_step_env_scenario.yaml"
);

// env_file_let_bindings scenarios
scenario_test!(
    scenario_env_file_let_bindings,
    "env_file_let_bindings/env_file_let_bindings_scenario.yaml"
);

// parameter_types scenarios
scenario_test!(
    scenario_task_params,
    "parameter_types/task_params_scenario.yaml"
);
scenario_test!(
    scenario_all_types,
    "parameter_types/all_types_scenario.yaml"
);
scenario_test!(
    scenario_all_task_param_types,
    "parameter_types/all_task_param_types_scenario.yaml"
);
scenario_test!(
    scenario_basic_task_param_types,
    "parameter_types/basic_task_param_types_scenario.yaml"
);
scenario_test!(
    scenario_task_path_param_mapping,
    "parameter_types/task_path_param_mapping_scenario.yaml"
);
scenario_test!(
    scenario_path_param_win_to_posix,
    "parameter_types/path_param_win_to_posix_scenario.yaml"
);
scenario_test!(
    scenario_path_param_posix_to_win,
    "parameter_types/path_param_posix_to_win_scenario.yaml"
);
scenario_test!(
    scenario_apply_path_mapping_win_to_posix,
    "parameter_types/apply_path_mapping_win_to_posix_scenario.yaml"
);
scenario_test!(
    scenario_apply_path_mapping_posix_to_win,
    "parameter_types/apply_path_mapping_posix_to_win_scenario.yaml"
);
scenario_test!(
    scenario_task_path_param_mapping_posix_to_win,
    "parameter_types/task_path_param_mapping_posix_to_win_scenario.yaml"
);
scenario_test!(
    scenario_list_path_param_win_to_posix,
    "parameter_types/list_path_param_win_to_posix_scenario.yaml"
);
scenario_test!(
    scenario_list_path_param_posix_to_win,
    "parameter_types/list_path_param_posix_to_win_scenario.yaml"
);
scenario_test!(
    scenario_task_path_from_list_path,
    "parameter_types/task_path_from_list_path_scenario.yaml"
);
scenario_test!(
    scenario_task_path_from_list_path_posix_to_win,
    "parameter_types/task_path_from_list_path_posix_to_win_scenario.yaml"
);
