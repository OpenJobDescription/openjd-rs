// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Tests for Session environment and step execution — mirrors Python
//! test_runner_env_script.py and test_runner_step_script.py

use openjd_expr::format_string::FormatString;
use openjd_model::job::{
    Action, EmbeddedFile, Environment, EnvironmentActions, EnvironmentScript, StepActions,
    StepScript,
};
use openjd_sessions::action::ActionState;
use openjd_sessions::session::{Session, SessionState};
use std::collections::HashMap;
use tempfile::TempDir;

fn fs(s: &str) -> FormatString {
    FormatString::new(s).unwrap()
}

fn make_env(name: &str, on_enter: Option<Action>, on_exit: Option<Action>) -> Environment {
    Environment {
        name: name.to_string(),
        description: None,
        script: Some(EnvironmentScript {
            let_bindings: None,
            actions: EnvironmentActions {
                on_enter,
                on_wrap_env_enter: None,
                on_wrap_task_run: None,
                on_wrap_env_exit: None,
                on_exit,
            },
            embedded_files: None,
        }),
        variables: None,
        resolved_symtab: None,
    }
}

fn make_action(command: &str, args: Vec<&str>) -> Action {
    Action {
        command: fs(command),
        args: Some(args.iter().map(|a| fs(a)).collect()),
        timeout: None,
        cancelation: None,
    }
}

fn make_step_script(command: &str, args: Vec<&str>) -> StepScript {
    StepScript {
        let_bindings: None,
        actions: StepActions {
            on_run: make_action(command, args),
        },
        embedded_files: None,
    }
}

// === TestEnvironmentScriptRunner::test_run_basic ===

#[tokio::test]
async fn test_env_enter_basic() {
    let tmp = TempDir::new().unwrap();
    let mut session = Session::new_for_test(tmp.path().to_path_buf());
    let env = make_env(
        "test_env",
        Some(make_action("sh", vec!["-c", "echo Hello"])),
        None,
    );
    let result = session.enter_environment(&env, None, None, None).await;
    assert!(result.is_ok());
    assert!(!result.unwrap().is_empty());
}

#[tokio::test]
async fn test_env_exit_basic() {
    let tmp = TempDir::new().unwrap();
    let mut session = Session::new_for_test(tmp.path().to_path_buf());
    let env = make_env(
        "test_env",
        None,
        Some(make_action("sh", vec!["-c", "echo Goodbye"])),
    );
    let id = session
        .enter_environment(&env, None, None, None)
        .await
        .unwrap();
    let result = session.exit_environment(&id, None, true, None).await;
    assert!(result.is_ok());
    assert!(result.unwrap().contains("Goodbye"));
}

// === test_run_handles_none ===

#[tokio::test]
async fn test_env_enter_no_action() {
    let tmp = TempDir::new().unwrap();
    let mut session = Session::new_for_test(tmp.path().to_path_buf());
    let env = make_env(
        "test_env",
        None,
        Some(make_action("sh", vec!["-c", "echo x"])),
    );
    let result = session.enter_environment(&env, None, None, None).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_env_exit_no_action() {
    let tmp = TempDir::new().unwrap();
    let mut session = Session::new_for_test(tmp.path().to_path_buf());
    let env = make_env(
        "test_env",
        Some(make_action("sh", vec!["-c", "echo x"])),
        None,
    );
    let id = session
        .enter_environment(&env, None, None, None)
        .await
        .unwrap();
    let result = session.exit_environment(&id, None, true, None).await;
    assert!(result.is_ok());
}

// === test_run_handles_none_script ===

#[tokio::test]
async fn test_env_no_script() {
    let tmp = TempDir::new().unwrap();
    let mut session = Session::new_for_test(tmp.path().to_path_buf());
    let env = Environment {
        name: "test_env".to_string(),
        description: None,
        script: None,
        variables: None,
        resolved_symtab: None,
    };
    assert!(session
        .enter_environment(&env, None, None, None)
        .await
        .is_ok());
    let id = session.environments_entered().last().unwrap().clone();
    assert!(session
        .exit_environment(&id, None, true, None)
        .await
        .is_ok());
}

// === test_run_with_files ===

#[tokio::test]
async fn test_env_with_embedded_files() {
    let tmp = TempDir::new().unwrap();
    let mut session = Session::new_for_test(tmp.path().to_path_buf());
    let env = Environment {
        name: "test_env".to_string(),
        description: None,
        script: Some(EnvironmentScript {
            let_bindings: None,
            actions: EnvironmentActions {
                on_enter: Some(Action {
                    command: fs("cat"),
                    args: Some(vec![fs("{{ Env.File.Script }}")]),
                    timeout: None,
                    cancelation: None,
                }),
                on_wrap_env_enter: None,
                on_wrap_task_run: None,
                on_wrap_env_exit: None,
                on_exit: None,
            },
            embedded_files: Some(vec![EmbeddedFile {
                name: "Script".to_string(),
                file_type: openjd_model::types::FileType::Text,
                filename: Some("script.txt".to_string()),
                data: Some(fs("file content here")),
                runnable: None,
                end_of_line: None,
            }]),
        }),
        variables: None,
        resolved_symtab: None,
    };
    let result = session.enter_environment(&env, None, None, None).await;
    assert!(result.is_ok());
}

// === test_env_with_variables ===

#[tokio::test]
async fn test_env_with_variables() {
    let tmp = TempDir::new().unwrap();
    let mut session = Session::new_for_test(tmp.path().to_path_buf());
    let mut vars = HashMap::new();
    vars.insert("MY_VAR".to_string(), fs("my_value"));
    let env = Environment {
        name: "test_env".to_string(),
        description: None,
        script: Some(EnvironmentScript {
            let_bindings: None,
            actions: EnvironmentActions {
                on_enter: Some(make_action("sh", vec!["-c", "echo MY_VAR=$MY_VAR"])),
                on_wrap_env_enter: None,
                on_wrap_task_run: None,
                on_wrap_env_exit: None,
                on_exit: None,
            },
            embedded_files: None,
        }),
        variables: Some(vars),
        resolved_symtab: None,
    };
    let result = session.enter_environment(&env, None, None, None).await;
    assert!(result.is_ok());
}

// === test_env_exit_removes_variables ===

#[tokio::test]
async fn test_env_exit_removes_variables() {
    let tmp = TempDir::new().unwrap();
    let mut session = Session::new_for_test(tmp.path().to_path_buf());
    let mut vars = HashMap::new();
    vars.insert("MY_VAR".to_string(), fs("my_value"));
    let env = Environment {
        name: "test_env".to_string(),
        description: None,
        script: None,
        variables: Some(vars),
        resolved_symtab: None,
    };
    session
        .enter_environment(&env, None, None, None)
        .await
        .unwrap();
    let id = session.environments_entered().last().unwrap().clone();
    session
        .exit_environment(&id, None, true, None)
        .await
        .unwrap();

    let script = make_step_script("sh", vec!["-c", "echo MY_VAR=${MY_VAR:-unset}"]);
    let result = session
        .run_task("test_step", &script, None, None, None)
        .await
        .unwrap();
    assert!(result.stdout.contains("MY_VAR=unset"));
}

// === TestStepScriptRunner::test_run_basic ===

#[tokio::test]
async fn test_step_run_basic() {
    let tmp = TempDir::new().unwrap();
    let mut session = Session::new_for_test(tmp.path().to_path_buf());
    let script = make_step_script("sh", vec!["-c", "echo Hello from step"]);
    let result = session
        .run_task("test_step", &script, None, None, None)
        .await;
    assert!(result.is_ok());
    let r = result.unwrap();
    assert_eq!(r.state, ActionState::Success);
    assert!(r.stdout.contains("Hello from step"));
}

// === test_step_failing ===

#[tokio::test]
async fn test_step_run_failing() {
    let tmp = TempDir::new().unwrap();
    let mut session = Session::new_for_test(tmp.path().to_path_buf());
    let script = make_step_script("sh", vec!["-c", "exit 1"]);
    let result = session
        .run_task("test_step", &script, None, None, None)
        .await;
    assert!(result.is_ok());
    let r = result.unwrap();
    assert_eq!(r.state, ActionState::Failed);
    assert_eq!(r.exit_code, Some(1));
    assert_eq!(session.state(), SessionState::ReadyEnding);
}

// === test_env_enter_fail ===

#[tokio::test]
async fn test_env_enter_fail() {
    let tmp = TempDir::new().unwrap();
    let mut session = Session::new_for_test(tmp.path().to_path_buf());
    let env = make_env(
        "test_env",
        Some(make_action("sh", vec!["-c", "exit 1"])),
        None,
    );
    assert!(session
        .enter_environment(&env, None, None, None)
        .await
        .is_err());
}

// === test_env_exit_fail ===

#[tokio::test]
async fn test_env_exit_fail() {
    let tmp = TempDir::new().unwrap();
    let mut session = Session::new_for_test(tmp.path().to_path_buf());
    let env = make_env(
        "test_env",
        None,
        Some(make_action("sh", vec!["-c", "exit 1"])),
    );
    let id = session
        .enter_environment(&env, None, None, None)
        .await
        .unwrap();
    assert!(session
        .exit_environment(&id, None, true, None)
        .await
        .is_err());
}

// === test_env_sets_env_vars_via_stdout ===

#[tokio::test]
async fn test_env_sets_env_vars_via_stdout() {
    let tmp = TempDir::new().unwrap();
    let mut session = Session::new_for_test(tmp.path().to_path_buf());
    let env = make_env(
        "test_env",
        Some(make_action(
            "sh",
            vec!["-c", "echo 'openjd_env: DYNAMIC_VAR=dynamic_value'"],
        )),
        None,
    );
    session
        .enter_environment(&env, None, None, None)
        .await
        .unwrap();

    let script = make_step_script("sh", vec!["-c", "echo DYNAMIC_VAR=$DYNAMIC_VAR"]);
    let result = session
        .run_task("test_step", &script, None, None, None)
        .await
        .unwrap();
    assert!(result.stdout.contains("DYNAMIC_VAR=dynamic_value"));
}

// === test_env_unsets_env_vars_via_stdout ===

#[tokio::test]
async fn test_env_unsets_env_vars_via_stdout() {
    let tmp = TempDir::new().unwrap();
    let mut session = Session::new_for_test(tmp.path().to_path_buf());

    let env1 = make_env(
        "env1",
        Some(make_action(
            "sh",
            vec!["-c", "echo 'openjd_env: TO_UNSET=value'"],
        )),
        None,
    );
    session
        .enter_environment(&env1, None, None, None)
        .await
        .unwrap();

    let env2 = make_env(
        "env2",
        Some(make_action(
            "sh",
            vec!["-c", "echo 'openjd_unset_env: TO_UNSET'"],
        )),
        None,
    );
    session
        .enter_environment(&env2, None, None, None)
        .await
        .unwrap();

    let script = make_step_script("sh", vec!["-c", "echo TO_UNSET=${TO_UNSET:-unset}"]);
    let result = session
        .run_task("test_step", &script, None, None, None)
        .await
        .unwrap();
    assert!(result.stdout.contains("TO_UNSET=unset"));
}

// === test_session_state_transitions ===

#[tokio::test]
async fn test_session_state_ready_after_success() {
    let tmp = TempDir::new().unwrap();
    let mut session = Session::new_for_test(tmp.path().to_path_buf());
    assert_eq!(session.state(), SessionState::Ready);

    let script = make_step_script("echo", vec!["ok"]);
    session
        .run_task("test_step", &script, None, None, None)
        .await
        .unwrap();
    assert_eq!(session.state(), SessionState::Ready);
}

#[tokio::test]
async fn test_session_state_ended_after_failure() {
    let tmp = TempDir::new().unwrap();
    let mut session = Session::new_for_test(tmp.path().to_path_buf());
    let script = make_step_script("sh", vec!["-c", "exit 1"]);
    session
        .run_task("test_step", &script, None, None, None)
        .await
        .unwrap();
    assert_eq!(session.state(), SessionState::ReadyEnding);
}

// === test_redacted_env_via_stdout ===

#[tokio::test]
async fn test_redacted_env_via_stdout() {
    let tmp = TempDir::new().unwrap();
    let mut session = Session::new_for_test(tmp.path().to_path_buf()).with_profile(
        openjd_model::ModelProfile::new(openjd_model::types::SpecificationRevision::V2023_09)
            .with_extensions(
                [openjd_model::types::ModelExtension::RedactedEnvVars]
                    .into_iter()
                    .collect(),
            ),
    );
    let env = make_env(
        "test_env",
        Some(make_action(
            "sh",
            vec!["-c", "echo 'openjd_redacted_env: SECRET=mysecret'"],
        )),
        None,
    );
    session
        .enter_environment(&env, None, None, None)
        .await
        .unwrap();

    let script = make_step_script("sh", vec!["-c", "echo SECRET=$SECRET"]);
    let result = session
        .run_task("test_step", &script, None, None, None)
        .await
        .unwrap();
    assert!(result.stdout.contains("SECRET=********"));

    let redacted = session.redact("The secret is mysecret");
    assert!(!redacted.contains("mysecret"));
    assert!(redacted.contains("********"));
}

// === test_env_with_resolved_variables ===

#[tokio::test]
async fn test_env_with_resolved_variables() {
    let tmp = TempDir::new().unwrap();
    use openjd_model::types::JobParameterValue;
    let mut job_params = HashMap::new();
    job_params.insert(
        "Value".to_string(),
        JobParameterValue {
            param_type: openjd_model::types::JobParameterType::String,
            value: openjd_expr::ExprValue::String("resolved_value".into()),
        },
    );
    let session_config = openjd_sessions::session::SessionConfig {
        session_id: "test".into(),
        job_parameter_values: job_params,
        path_mapping_rules: None,
        retain_working_dir: false,
        callback: None,
        os_env_vars: None,
        session_root_directory: Some(tmp.path().to_path_buf()),
        user: None,
        profile: None,
        cancel_token: None,
        debug_collect_stdout: true,
        echo_openjd_directives: true,
        sticky_bit_policy: openjd_sessions::StickyBitPolicy::Disabled,
    };
    let mut session = Session::with_config(session_config).unwrap();
    let mut vars = HashMap::new();
    vars.insert("RESOLVED".to_string(), fs("{{ Param.Value }}"));
    let env = Environment {
        name: "test_env".to_string(),
        description: None,
        script: Some(EnvironmentScript {
            let_bindings: None,
            actions: EnvironmentActions {
                on_enter: Some(make_action("sh", vec!["-c", "echo RESOLVED=$RESOLVED"])),
                on_wrap_env_enter: None,
                on_wrap_task_run: None,
                on_wrap_env_exit: None,
                on_exit: None,
            },
            embedded_files: None,
        }),
        variables: Some(vars),
        resolved_symtab: None,
    };
    let result = session.enter_environment(&env, None, None, None).await;
    assert!(result.is_ok());
}

// === test_env_with_let_bindings_and_embedded_files (two-phase) ===

#[tokio::test]
async fn test_env_with_let_bindings_and_embedded_files() {
    let tmp = TempDir::new().unwrap();
    let files_dir = tmp.path().join("embedded_files");
    std::fs::create_dir_all(&files_dir).unwrap();
    let mut session = Session::new_for_test(tmp.path().to_path_buf());
    let env = Environment {
        name: "test_env".to_string(),
        description: None,
        script: Some(EnvironmentScript {
            let_bindings: Some(vec!["configPath = Env.File.Config".to_string()]),
            actions: EnvironmentActions {
                on_enter: Some(Action {
                    command: fs("sh"),
                    args: Some(vec![fs("-c"), fs("echo {{ configPath }}")]),
                    timeout: None,
                    cancelation: None,
                }),
                on_wrap_env_enter: None,
                on_wrap_task_run: None,
                on_wrap_env_exit: None,
                on_exit: None,
            },
            embedded_files: Some(vec![EmbeddedFile {
                name: "Config".to_string(),
                file_type: openjd_model::types::FileType::Text,
                filename: Some("config.txt".to_string()),
                data: Some(fs("config data")),
                runnable: None,
                end_of_line: None,
            }]),
        }),
        variables: None,
        resolved_symtab: None,
    };
    let result = session.enter_environment(&env, None, None, None).await;
    assert!(result.is_ok());
    // The configPath should resolve to the file path
    let config_path = files_dir.join("config.txt");
    assert!(config_path.exists());
    assert_eq!(
        std::fs::read_to_string(&config_path).unwrap(),
        "config data"
    );
}

// === test_step_with_let_bindings_and_embedded_files ===

#[tokio::test]
async fn test_step_with_let_bindings_and_embedded_files() {
    let tmp = TempDir::new().unwrap();
    let files_dir = tmp.path().join("embedded_files");
    std::fs::create_dir_all(&files_dir).unwrap();
    let mut session = Session::new_for_test(tmp.path().to_path_buf());
    let script = StepScript {
        let_bindings: Some(vec!["greeting = 'hello'".to_string()]),
        actions: StepActions {
            on_run: Action {
                command: fs("cat"),
                args: Some(vec![fs("{{ Task.File.Data }}")]),
                timeout: None,
                cancelation: None,
            },
        },
        embedded_files: Some(vec![EmbeddedFile {
            name: "Data".to_string(),
            file_type: openjd_model::types::FileType::Text,
            filename: Some("data.txt".to_string()),
            data: Some(fs("{{ greeting }}")),
            runnable: None,
            end_of_line: None,
        }]),
    };
    let result = session
        .run_task("test_step", &script, None, None, None)
        .await;
    assert!(result.is_ok());
    let data_path = files_dir.join("data.txt");
    assert!(data_path.exists());
    assert_eq!(std::fs::read_to_string(&data_path).unwrap(), "hello");
}
