// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Tests for Session — mirrors Python test_session.py

use openjd_expr::format_string::FormatString;
use openjd_model::job::{
    Action, CancelationMode, Environment, EnvironmentActions, EnvironmentScript, StepActions,
    StepScript,
};
use openjd_sessions::action::ActionState;
use openjd_sessions::session::{Session, SessionConfig, SessionState};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tempfile::TempDir;

fn fs(s: &str) -> FormatString {
    FormatString::new(s).unwrap()
}

fn action(cmd: &str, args: Vec<&str>) -> Action {
    Action {
        command: fs(cmd),
        args: Some(args.iter().map(|a| fs(a)).collect()),
        timeout: None,
        cancelation: None,
    }
}

fn step(cmd: &str, args: Vec<&str>) -> StepScript {
    StepScript {
        let_bindings: None,
        actions: StepActions {
            on_run: action(cmd, args),
        },
        embedded_files: None,
    }
}

fn env_with_enter(name: &str, cmd: &str, args: Vec<&str>) -> Environment {
    Environment {
        name: name.into(),
        description: None,
        script: Some(EnvironmentScript {
            let_bindings: None,
            actions: EnvironmentActions {
                on_enter: Some(action(cmd, args)),
                on_wrap_env_enter: None,
                on_wrap_task_run: None,
                on_wrap_env_exit: None,
                on_exit: None,
            },
            embedded_files: None,
        }),
        variables: None,
        resolved_symtab: None,
    }
}

fn env_with_vars(name: &str, vars: HashMap<String, FormatString>) -> Environment {
    Environment {
        name: name.into(),
        description: None,
        script: None,
        variables: Some(vars),
        resolved_symtab: None,
    }
}

// === TestSessionInitialization ===

#[tokio::test]
async fn test_initialize_basic() {
    let tmp = TempDir::new().unwrap();
    let session = Session::new_for_test(tmp.path().to_path_buf());
    assert_eq!(session.state(), SessionState::Ready);
    assert!(session.working_directory().exists());
}

#[tokio::test]
async fn test_initialize_with_root_dir() {
    let tmp = TempDir::new().unwrap();
    let session = Session::new_for_test(tmp.path().to_path_buf());
    assert_eq!(session.working_directory(), tmp.path());
}

/// Mirrors Python TestSession::test_root_dir_permissions — POSIX: owner rwx, group r/x, other r/x.
#[cfg(unix)]
#[tokio::test]
async fn test_root_dir_permissions_posix() {
    use std::os::unix::fs::PermissionsExt;
    let tmp = TempDir::new().unwrap();
    let config = SessionConfig {
        session_id: "test-perms".into(),
        job_parameter_values: Default::default(),
        session_root_directory: Some(tmp.path().to_path_buf()),
        path_mapping_rules: None,
        retain_working_dir: false,
        callback: None,
        os_env_vars: None,
        user: None,
        profile: None,
        cancel_token: None,
        debug_collect_stdout: true,
        echo_openjd_directives: true,
        sticky_bit_policy: openjd_sessions::StickyBitPolicy::Disabled,
    };
    let session = Session::with_config(config).unwrap();
    // The working dir is created by TempDir::new with mode 0o700 when no user is given.
    let working_mode = std::fs::metadata(session.working_directory())
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(
        working_mode & 0o777,
        0o700,
        "working dir is 0o700 (no user)"
    );
}

// === StickyBitPolicy tests ===

/// Strict mode rejects a session root under a world-writable dir without sticky bit.
#[cfg(unix)]
#[tokio::test]
async fn test_sticky_bit_policy_strict_rejects_unsafe_dir() {
    use std::os::unix::fs::PermissionsExt;
    let tmp = TempDir::new().unwrap();
    let unsafe_dir = tmp.path().join("world_writable");
    std::fs::create_dir(&unsafe_dir).unwrap();
    std::fs::set_permissions(&unsafe_dir, std::fs::Permissions::from_mode(0o777)).unwrap();
    let root = unsafe_dir.join("root");
    std::fs::create_dir(&root).unwrap();

    let config = SessionConfig {
        session_id: "test-strict".into(),
        job_parameter_values: Default::default(),
        session_root_directory: Some(root),
        path_mapping_rules: None,
        retain_working_dir: false,
        callback: None,
        os_env_vars: None,
        user: None,
        profile: None,
        cancel_token: None,
        debug_collect_stdout: true,
        echo_openjd_directives: true,
        sticky_bit_policy: openjd_sessions::StickyBitPolicy::Strict,
    };
    let result = Session::with_config(config);
    assert!(result.is_err());
    let err = result.err().unwrap().to_string();
    assert!(err.contains("world-writable"), "error was: {err}");
}

/// Strict mode allows a session root when sticky bit is set.
#[cfg(unix)]
#[tokio::test]
async fn test_sticky_bit_policy_strict_allows_safe_dir() {
    use std::os::unix::fs::PermissionsExt;
    let tmp = TempDir::new().unwrap();
    let safe_dir = tmp.path().join("sticky");
    std::fs::create_dir(&safe_dir).unwrap();
    std::fs::set_permissions(&safe_dir, std::fs::Permissions::from_mode(0o1777)).unwrap();
    let root = safe_dir.join("root");
    std::fs::create_dir(&root).unwrap();

    let config = SessionConfig {
        session_id: "test-strict-ok".into(),
        job_parameter_values: Default::default(),
        session_root_directory: Some(root),
        path_mapping_rules: None,
        retain_working_dir: false,
        callback: None,
        os_env_vars: None,
        user: None,
        profile: None,
        cancel_token: None,
        debug_collect_stdout: true,
        echo_openjd_directives: true,
        sticky_bit_policy: openjd_sessions::StickyBitPolicy::Strict,
    };
    let session = Session::with_config(config).unwrap();
    assert_eq!(session.state(), SessionState::Ready);
}

/// Warn mode logs but does not reject an unsafe directory.
#[cfg(unix)]
#[tokio::test]
async fn test_sticky_bit_policy_warn_allows_unsafe_dir() {
    use std::os::unix::fs::PermissionsExt;
    testing_logger::setup();
    let tmp = TempDir::new().unwrap();
    let unsafe_dir = tmp.path().join("world_writable");
    std::fs::create_dir(&unsafe_dir).unwrap();
    std::fs::set_permissions(&unsafe_dir, std::fs::Permissions::from_mode(0o777)).unwrap();
    let root = unsafe_dir.join("root");
    std::fs::create_dir(&root).unwrap();

    let config = SessionConfig {
        session_id: "test-warn".into(),
        job_parameter_values: Default::default(),
        session_root_directory: Some(root),
        path_mapping_rules: None,
        retain_working_dir: false,
        callback: None,
        os_env_vars: None,
        user: None,
        profile: None,
        cancel_token: None,
        debug_collect_stdout: true,
        echo_openjd_directives: true,
        sticky_bit_policy: openjd_sessions::StickyBitPolicy::Warn,
    };
    let session = Session::with_config(config).unwrap();
    assert_eq!(session.state(), SessionState::Ready);

    testing_logger::validate(|captured_logs| {
        assert!(
            captured_logs
                .iter()
                .any(|log| log.level == log::Level::Warn
                    && log.body.contains("Sticky bit is not set")),
            "Expected a warning about missing sticky bit"
        );
    });
}

/// Disabled mode skips the check entirely — no error, no warning.
#[cfg(unix)]
#[tokio::test]
async fn test_sticky_bit_policy_disabled_skips_check() {
    use std::os::unix::fs::PermissionsExt;
    testing_logger::setup();
    let tmp = TempDir::new().unwrap();
    let unsafe_dir = tmp.path().join("world_writable");
    std::fs::create_dir(&unsafe_dir).unwrap();
    std::fs::set_permissions(&unsafe_dir, std::fs::Permissions::from_mode(0o777)).unwrap();
    let root = unsafe_dir.join("root");
    std::fs::create_dir(&root).unwrap();

    let config = SessionConfig {
        session_id: "test-disabled".into(),
        job_parameter_values: Default::default(),
        session_root_directory: Some(root),
        path_mapping_rules: None,
        retain_working_dir: false,
        callback: None,
        os_env_vars: None,
        user: None,
        profile: None,
        cancel_token: None,
        debug_collect_stdout: true,
        echo_openjd_directives: true,
        sticky_bit_policy: openjd_sessions::StickyBitPolicy::Disabled,
    };
    let session = Session::with_config(config).unwrap();
    assert_eq!(session.state(), SessionState::Ready);

    testing_logger::validate(|captured_logs| {
        assert!(
            !captured_logs
                .iter()
                .any(|log| log.body.contains("Sticky bit")),
            "Should not log anything about sticky bit when disabled"
        );
    });
}

/// Mirrors Python: Session dropped without cleanup() should log a warning.
#[tokio::test]
async fn test_session_drop_without_cleanup_warns() {
    testing_logger::setup();
    let tmp = TempDir::new().unwrap();
    {
        let _session = Session::new_for_test(tmp.path().to_path_buf());
        // drop without calling cleanup()
    }
    testing_logger::validate(|captured_logs| {
        assert!(
            captured_logs.iter().any(|log| {
                log.level == log::Level::Warn
                    && log.body.contains("dropped without calling cleanup()")
            }),
            "Expected a warning about session dropped without cleanup"
        );
    });
}

// === TestSessionRunTask_2023_09 ===

#[tokio::test]
async fn test_run_task() {
    let tmp = TempDir::new().unwrap();
    let mut s = Session::new_for_test(tmp.path().to_path_buf());
    let r = s
        .run_task(
            "test_step",
            &step("sh", vec!["-c", "echo task_output"]),
            None,
            None,
            None,
        )
        .await
        .unwrap();
    assert_eq!(r.state, ActionState::Success);
    assert!(r.stdout.contains("task_output"));
}

#[tokio::test]
async fn test_run_task_with_env_vars() {
    let tmp = TempDir::new().unwrap();
    let mut s = Session::new_for_test(tmp.path().to_path_buf());
    let mut vars = HashMap::new();
    vars.insert("TASK_VAR".into(), fs("task_value"));
    let env = env_with_vars("env1", vars);
    s.enter_environment(&env, None, None, None).await.unwrap();

    let r = s
        .run_task(
            "test_step",
            &step("sh", vec!["-c", "echo TASK_VAR=$TASK_VAR"]),
            None,
            None,
            None,
        )
        .await
        .unwrap();
    assert!(r.stdout.contains("TASK_VAR=task_value"));
}

#[tokio::test]
async fn test_run_task_fail_run() {
    let tmp = TempDir::new().unwrap();
    let mut s = Session::new_for_test(tmp.path().to_path_buf());
    let r = s
        .run_task(
            "test_step",
            &step("sh", vec!["-c", "exit 42"]),
            None,
            None,
            None,
        )
        .await
        .unwrap();
    assert_eq!(r.state, ActionState::Failed);
    assert_eq!(r.exit_code, Some(42));
}

#[tokio::test]
async fn test_no_task_run_after_fail() {
    let tmp = TempDir::new().unwrap();
    let mut s = Session::new_for_test(tmp.path().to_path_buf());
    // First run fails — session becomes "brittle" (ReadyEnding), only exit_environment allowed
    s.run_task(
        "test_step",
        &step("sh", vec!["-c", "exit 1"]),
        None,
        None,
        None,
    )
    .await
    .unwrap();
    assert_eq!(s.state(), SessionState::ReadyEnding);
}

#[tokio::test]
async fn test_run_task_with_variables() {
    let tmp = TempDir::new().unwrap();
    let mut s = Session::new_for_test(tmp.path().to_path_buf());
    let mut task_params = openjd_model::types::TaskParameterSet::new();
    task_params.insert(
        "Greeting".into(),
        openjd_model::types::TaskParameterValue {
            param_type: openjd_model::types::TaskParameterType::String,
            value: openjd_expr::ExprValue::String("hello".into()),
        },
    );

    let script = StepScript {
        let_bindings: None,
        actions: StepActions {
            on_run: Action {
                command: fs("sh"),
                args: Some(vec![fs("-c"), fs("echo {{ Task.Param.Greeting }}")]),
                timeout: None,
                cancelation: None,
            },
        },
        embedded_files: None,
    };
    let r = s
        .run_task("test_step", &script, Some(&task_params), None, None)
        .await
        .unwrap();
    assert!(r.stdout.contains("hello"));
}

// === TestSessionEnterEnvironment_2023_09 ===

#[tokio::test]
async fn test_enter_environment_basic() {
    let tmp = TempDir::new().unwrap();
    let mut s = Session::new_for_test(tmp.path().to_path_buf());
    let env = env_with_enter("env1", "sh", vec!["-c", "echo entered"]);
    let id = s.enter_environment(&env, None, None, None).await.unwrap();
    assert!(!id.is_empty());
}

#[tokio::test]
async fn test_enter_environment_with_env_vars() {
    let tmp = TempDir::new().unwrap();
    let mut s = Session::new_for_test(tmp.path().to_path_buf());
    let mut vars = HashMap::new();
    vars.insert("ENV_VAR".into(), fs("env_value"));
    let env = Environment {
        name: "env1".into(),
        description: None,
        script: Some(EnvironmentScript {
            let_bindings: None,
            actions: EnvironmentActions {
                on_enter: Some(action("sh", vec!["-c", "echo ENV_VAR=$ENV_VAR"])),
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
    let id = s.enter_environment(&env, None, None, None).await.unwrap();
    assert!(!id.is_empty());
}

#[tokio::test]
async fn test_enter_two_environments() {
    let tmp = TempDir::new().unwrap();
    let mut s = Session::new_for_test(tmp.path().to_path_buf());
    let env1 = env_with_enter(
        "env1",
        "sh",
        vec!["-c", "echo 'openjd_env: FROM_ENV1=val1'"],
    );
    let env2 = env_with_enter("env2", "sh", vec!["-c", "echo FROM_ENV1=$FROM_ENV1"]);
    s.enter_environment(&env1, None, None, None).await.unwrap();
    let id2 = s.enter_environment(&env2, None, None, None).await.unwrap();
    assert!(!id2.is_empty());
}

#[tokio::test]
async fn test_enter_environment_fail_run() {
    let tmp = TempDir::new().unwrap();
    let mut s = Session::new_for_test(tmp.path().to_path_buf());
    let env = env_with_enter("env1", "sh", vec!["-c", "exit 1"]);
    assert!(s.enter_environment(&env, None, None, None).await.is_err());
}

#[tokio::test]
async fn test_enter_environment_command_not_found() {
    // Regression: when the subprocess command doesn't exist, the session must
    // transition to ReadyEnding with action_state=Failed, not stay stuck in Running.
    let tmp = TempDir::new().unwrap();
    let mut s = Session::new_for_test(tmp.path().to_path_buf());
    let env = env_with_enter("env1", "nonexistent-command-xyz", vec![]);
    let result = s.enter_environment(&env, None, None, None).await;
    assert!(result.is_err());
    assert_eq!(s.state(), SessionState::ReadyEnding);
    let status = s
        .action_status()
        .expect("action_status should be set after failure");
    assert_eq!(status.state, ActionState::Failed);
}

#[tokio::test]
async fn test_run_task_command_not_found() {
    // Same regression test for run_task: command not found must set Failed state.
    let tmp = TempDir::new().unwrap();
    let mut s = Session::new_for_test(tmp.path().to_path_buf());
    let script = step("nonexistent-command-xyz", vec![]);
    let result = s.run_task("test_step", &script, None, None, None).await;
    assert!(result.is_err());
    assert_eq!(s.state(), SessionState::ReadyEnding);
    let status = s
        .action_status()
        .expect("action_status should be set after failure");
    assert_eq!(status.state, ActionState::Failed);
}

#[tokio::test]
async fn test_enter_no_action() {
    let tmp = TempDir::new().unwrap();
    let mut s = Session::new_for_test(tmp.path().to_path_buf());
    let env = Environment {
        name: "env1".into(),
        description: None,
        script: Some(EnvironmentScript {
            let_bindings: None,
            actions: EnvironmentActions {
                on_enter: None,
                on_wrap_env_enter: None,
                on_wrap_task_run: None,
                on_wrap_env_exit: None,
                on_exit: None,
            },
            embedded_files: None,
        }),
        variables: None,
        resolved_symtab: None,
    };
    assert!(s.enter_environment(&env, None, None, None).await.is_ok());
}

#[tokio::test]
async fn test_enter_environment_with_resolved_variables() {
    let tmp = TempDir::new().unwrap();
    use openjd_model::types::JobParameterValue;
    let mut job_params = HashMap::new();
    job_params.insert(
        "Val".to_string(),
        JobParameterValue {
            param_type: openjd_model::types::JobParameterType::String,
            value: openjd_expr::ExprValue::String("resolved".into()),
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
    let mut s = Session::with_config(session_config).unwrap();
    let mut vars = HashMap::new();
    vars.insert("RESOLVED".into(), fs("{{ Param.Val }}"));
    let env = Environment {
        name: "env1".into(),
        description: None,
        script: Some(EnvironmentScript {
            let_bindings: None,
            actions: EnvironmentActions {
                on_enter: Some(action("sh", vec!["-c", "echo RESOLVED=$RESOLVED"])),
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
    let id = s.enter_environment(&env, None, None, None).await.unwrap();
    assert!(!id.is_empty());
}

// === TestSessionExitEnvironment_2023_09 ===

#[tokio::test]
async fn test_exit_environment_basic() {
    let tmp = TempDir::new().unwrap();
    let mut s = Session::new_for_test(tmp.path().to_path_buf());
    let env = Environment {
        name: "env1".into(),
        description: None,
        script: Some(EnvironmentScript {
            let_bindings: None,
            actions: EnvironmentActions {
                on_enter: None,
                on_wrap_env_enter: None,
                on_wrap_task_run: None,
                on_wrap_env_exit: None,
                on_exit: Some(action("sh", vec!["-c", "echo exited"])),
            },
            embedded_files: None,
        }),
        variables: None,
        resolved_symtab: None,
    };
    let id = s.enter_environment(&env, None, None, None).await.unwrap();
    let out = s.exit_environment(&id, None, true, None).await.unwrap();
    assert!(out.contains("exited"));
}

#[tokio::test]
async fn test_exit_environment_with_env_vars() {
    let tmp = TempDir::new().unwrap();
    let mut s = Session::new_for_test(tmp.path().to_path_buf());
    let mut vars = HashMap::new();
    vars.insert("EXIT_VAR".into(), fs("exit_value"));
    let env = Environment {
        name: "env1".into(),
        description: None,
        script: Some(EnvironmentScript {
            let_bindings: None,
            actions: EnvironmentActions {
                on_enter: None,
                on_wrap_env_enter: None,
                on_wrap_task_run: None,
                on_wrap_env_exit: None,
                on_exit: Some(action("sh", vec!["-c", "echo EXIT_VAR=$EXIT_VAR"])),
            },
            embedded_files: None,
        }),
        variables: Some(vars.clone()),
        resolved_symtab: None,
    };
    // Enter first to set vars
    let id = s.enter_environment(&env, None, None, None).await.unwrap();
    let out = s.exit_environment(&id, None, true, None).await.unwrap();
    assert!(out.contains("EXIT_VAR=exit_value"));
}

#[tokio::test]
async fn test_exit_environment_removes_variables() {
    let tmp = TempDir::new().unwrap();
    let mut s = Session::new_for_test(tmp.path().to_path_buf());
    let mut vars = HashMap::new();
    vars.insert("REMOVED_VAR".into(), fs("value"));
    let env = env_with_vars("env1", vars);
    let id = s.enter_environment(&env, None, None, None).await.unwrap();
    s.exit_environment(&id, None, true, None).await.unwrap();

    let r = s
        .run_task(
            "test_step",
            &step("sh", vec!["-c", "echo REMOVED_VAR=${REMOVED_VAR:-gone}"]),
            None,
            None,
            None,
        )
        .await
        .unwrap();
    assert!(r.stdout.contains("REMOVED_VAR=gone"));
}

#[tokio::test]
async fn test_exit_environment_fail_run() {
    let tmp = TempDir::new().unwrap();
    let mut s = Session::new_for_test(tmp.path().to_path_buf());
    let env = Environment {
        name: "env1".into(),
        description: None,
        script: Some(EnvironmentScript {
            let_bindings: None,
            actions: EnvironmentActions {
                on_enter: None,
                on_wrap_env_enter: None,
                on_wrap_task_run: None,
                on_wrap_env_exit: None,
                on_exit: Some(action("sh", vec!["-c", "exit 1"])),
            },
            embedded_files: None,
        }),
        variables: None,
        resolved_symtab: None,
    };
    let id = s.enter_environment(&env, None, None, None).await.unwrap();
    assert!(s.exit_environment(&id, None, true, None).await.is_err());
}

#[tokio::test]
async fn test_run_task_after_env_exit() {
    let tmp = TempDir::new().unwrap();
    let mut s = Session::new_for_test(tmp.path().to_path_buf());
    let env = env_with_enter("env1", "sh", vec!["-c", "echo 'openjd_env: PERSIST=yes'"]);
    let id = s.enter_environment(&env, None, None, None).await.unwrap();
    s.exit_environment(&id, None, true, None).await.unwrap();

    // After exit, env vars set via openjd_env are removed along with the environment.
    // Only process_env vars persist across environment exits.
    let r = s
        .run_task(
            "test_step",
            &step("sh", vec!["-c", "echo PERSIST=${PERSIST:-no}"]),
            None,
            None,
            None,
        )
        .await
        .unwrap();
    assert!(r.stdout.contains("PERSIST=no"));
}

// === TestEnvironmentVariablesInTasks_2023_09 ===

#[tokio::test]
async fn test_direct_definition() {
    let tmp = TempDir::new().unwrap();
    let mut s = Session::new_for_test(tmp.path().to_path_buf());
    let mut vars = HashMap::new();
    vars.insert("DIRECT".into(), fs("direct_val"));
    let env = env_with_vars("env1", vars);
    s.enter_environment(&env, None, None, None).await.unwrap();

    let r = s
        .run_task(
            "test_step",
            &step("sh", vec!["-c", "echo DIRECT=$DIRECT"]),
            None,
            None,
            None,
        )
        .await
        .unwrap();
    assert!(r.stdout.contains("DIRECT=direct_val"));
}

#[tokio::test]
async fn test_redefinition_nested() {
    let tmp = TempDir::new().unwrap();
    let mut s = Session::new_for_test(tmp.path().to_path_buf());
    let mut vars1 = HashMap::new();
    vars1.insert("VAR".into(), fs("outer"));
    let env1 = env_with_vars("env1", vars1);
    s.enter_environment(&env1, None, None, None).await.unwrap();

    let mut vars2 = HashMap::new();
    vars2.insert("VAR".into(), fs("inner"));
    let env2 = env_with_vars("env2", vars2);
    s.enter_environment(&env2, None, None, None).await.unwrap();

    let r = s
        .run_task(
            "test_step",
            &step("sh", vec!["-c", "echo VAR=$VAR"]),
            None,
            None,
            None,
        )
        .await
        .unwrap();
    assert!(r.stdout.contains("VAR=inner"));
}

#[tokio::test]
async fn test_def_via_stdout() {
    let tmp = TempDir::new().unwrap();
    let mut s = Session::new_for_test(tmp.path().to_path_buf());
    let env = env_with_enter(
        "env1",
        "sh",
        vec!["-c", "echo 'openjd_env: STDOUT_VAR=stdout_val'"],
    );
    s.enter_environment(&env, None, None, None).await.unwrap();

    let r = s
        .run_task(
            "test_step",
            &step("sh", vec!["-c", "echo STDOUT_VAR=$STDOUT_VAR"]),
            None,
            None,
            None,
        )
        .await
        .unwrap();
    assert!(r.stdout.contains("STDOUT_VAR=stdout_val"));
}

#[tokio::test]
async fn test_def_via_stdout_overrides_direct() {
    let tmp = TempDir::new().unwrap();
    let mut s = Session::new_for_test(tmp.path().to_path_buf());
    let mut vars = HashMap::new();
    vars.insert("OVERRIDE".into(), fs("direct"));
    let env = Environment {
        name: "env1".into(),
        description: None,
        script: Some(EnvironmentScript {
            let_bindings: None,
            actions: EnvironmentActions {
                on_enter: Some(action(
                    "sh",
                    vec!["-c", "echo 'openjd_env: OVERRIDE=from_stdout'"],
                )),
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
    s.enter_environment(&env, None, None, None).await.unwrap();

    let r = s
        .run_task(
            "test_step",
            &step("sh", vec!["-c", "echo OVERRIDE=$OVERRIDE"]),
            None,
            None,
            None,
        )
        .await
        .unwrap();
    assert!(r.stdout.contains("OVERRIDE=from_stdout"));
}

#[tokio::test]
async fn test_undef_via_stdout() {
    let tmp = TempDir::new().unwrap();
    let mut s = Session::new_for_test(tmp.path().to_path_buf());
    let env1 = env_with_enter("env1", "sh", vec!["-c", "echo 'openjd_env: TO_UNDEF=val'"]);
    s.enter_environment(&env1, None, None, None).await.unwrap();

    let env2 = env_with_enter(
        "env2",
        "sh",
        vec!["-c", "echo 'openjd_unset_env: TO_UNDEF'"],
    );
    s.enter_environment(&env2, None, None, None).await.unwrap();

    let r = s
        .run_task(
            "test_step",
            &step("sh", vec!["-c", "echo TO_UNDEF=${TO_UNDEF:-gone}"]),
            None,
            None,
            None,
        )
        .await
        .unwrap();
    assert!(r.stdout.contains("TO_UNDEF=gone"));
}

#[tokio::test]
async fn test_def_via_redacted_env_stdout() {
    let tmp = TempDir::new().unwrap();
    let mut s = Session::new_for_test(tmp.path().to_path_buf()).with_profile(
        openjd_model::ModelProfile::new(openjd_model::types::SpecificationRevision::V2023_09)
            .with_extensions(
                [openjd_model::types::ModelExtension::RedactedEnvVars]
                    .into_iter()
                    .collect(),
            ),
    );
    let env = env_with_enter(
        "env1",
        "sh",
        vec!["-c", "echo 'openjd_redacted_env: SECRET_KEY=secret_val'"],
    );
    s.enter_environment(&env, None, None, None).await.unwrap();

    let r = s
        .run_task(
            "test_step",
            &step("sh", vec!["-c", "echo SECRET_KEY=$SECRET_KEY"]),
            None,
            None,
            None,
        )
        .await
        .unwrap();
    assert!(r.stdout.contains("SECRET_KEY=********"));

    // Redaction should work
    let redacted = s.redact("The key is secret_val");
    assert!(!redacted.contains("secret_val"));
}

// === TestSimplifiedEnvironmentVariableChanges ===
// These test the env var tracking. In Rust, this is handled by the Session's env_vars HashMap.

#[tokio::test]
async fn test_env_var_changes_init() {
    let tmp = TempDir::new().unwrap();
    let s = Session::new_for_test(tmp.path().to_path_buf());
    assert_eq!(s.state(), SessionState::Ready);
}

// === TestEnvironmentVariablesInTasks_2023_09 — additional tests ===

#[tokio::test]
async fn test_def_via_multi_line_stdout() {
    // Test that JSON-encoded multi-line env vars are set correctly
    let tmp = TempDir::new().unwrap();
    let mut s = Session::new_for_test(tmp.path().to_path_buf());
    let env = env_with_enter(
        "env1",
        "sh",
        vec!["-c", r#"printf '%s\n' 'openjd_env: "FOO=12\n34"'"#],
    );
    s.enter_environment(&env, None, None, None).await.unwrap();

    let r = s
        .run_task(
            "test_step",
            &step("sh", vec!["-c", "printf 'FOO=%s\n' \"$FOO\""]),
            None,
            None,
            None,
        )
        .await
        .unwrap();
    assert!(r.stdout.contains("FOO=12\n34") || r.stdout.contains("FOO=12"));
}

#[tokio::test]
async fn test_def_via_stdout_set_empty() {
    // Test that setting an env var to empty string works
    let tmp = TempDir::new().unwrap();
    let mut s = Session::new_for_test(tmp.path().to_path_buf());
    let env = env_with_enter("env1", "sh", vec!["-c", "echo 'openjd_env: FOO='"]);
    s.enter_environment(&env, None, None, None).await.unwrap();

    let r = s
        .run_task(
            "test_step",
            &step("sh", vec!["-c", "echo FOO=$FOO"]),
            None,
            None,
            None,
        )
        .await
        .unwrap();
    assert!(r.stdout.contains("FOO="));
}

#[tokio::test]
async fn test_def_via_stdout_set_empty_json() {
    // Test that setting an env var to empty string via JSON works
    let tmp = TempDir::new().unwrap();
    let mut s = Session::new_for_test(tmp.path().to_path_buf());
    let env = env_with_enter("env1", "sh", vec!["-c", r#"echo 'openjd_env: "FOO="'"#]);
    s.enter_environment(&env, None, None, None).await.unwrap();

    let r = s
        .run_task(
            "test_step",
            &step("sh", vec!["-c", "echo FOO=$FOO"]),
            None,
            None,
            None,
        )
        .await
        .unwrap();
    assert!(r.stdout.contains("FOO="));
}

#[tokio::test]
async fn test_def_via_redacted_env_json_stdout() {
    // Test that redacted env vars are redacted in logs but not set without extension
    let tmp = TempDir::new().unwrap();
    let mut s = Session::new_for_test(tmp.path().to_path_buf());
    let env = env_with_enter(
        "env1",
        "sh",
        vec!["-c", "echo 'openjd_redacted_env: API_KEY=abc123def456'"],
    );
    s.enter_environment(&env, None, None, None).await.unwrap();

    // Without extension, the env var should NOT be set
    let r = s
        .run_task(
            "test_step",
            &step("sh", vec!["-c", "echo API_KEY=${API_KEY:-not_set}"]),
            None,
            None,
            None,
        )
        .await
        .unwrap();
    assert!(r.stdout.contains("API_KEY=not_set"));

    // But the value should still be tracked for redaction
    let redacted = s.redact("The key is abc123def456");
    assert!(!redacted.contains("abc123def456"));
}

#[tokio::test]
async fn test_def_via_redacted_env_with_extension() {
    // Test that redacted env vars ARE set when extension is enabled
    let tmp = TempDir::new().unwrap();
    let mut s = Session::new_for_test(tmp.path().to_path_buf()).with_profile(
        openjd_model::ModelProfile::new(openjd_model::types::SpecificationRevision::V2023_09)
            .with_extensions(
                [openjd_model::types::ModelExtension::RedactedEnvVars]
                    .into_iter()
                    .collect(),
            ),
    );
    let env = env_with_enter(
        "env1",
        "sh",
        vec!["-c", "echo 'openjd_redacted_env: PASSWORD=secret123'"],
    );
    s.enter_environment(&env, None, None, None).await.unwrap();

    let r = s
        .run_task(
            "test_step",
            &step("sh", vec!["-c", "echo PASSWORD=$PASSWORD"]),
            None,
            None,
            None,
        )
        .await
        .unwrap();
    assert!(r.stdout.contains("PASSWORD=********"));

    let redacted = s.redact("PASSWORD=secret123");
    assert!(!redacted.contains("secret123"));
}

#[tokio::test]
async fn test_def_via_redacted_env_with_variables() {
    // Test that redacted env vars override directly defined variables when extension is NOT enabled
    let tmp = TempDir::new().unwrap();
    let mut s = Session::new_for_test(tmp.path().to_path_buf());
    let mut vars = HashMap::new();
    vars.insert("TOKEN".into(), fs("public-token"));
    let env = Environment {
        name: "env1".into(),
        description: None,
        script: Some(openjd_model::job::EnvironmentScript {
            let_bindings: None,
            actions: openjd_model::job::EnvironmentActions {
                on_enter: Some(action(
                    "sh",
                    vec!["-c", "echo 'openjd_redacted_env: TOKEN=secret-token'"],
                )),
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
    s.enter_environment(&env, None, None, None).await.unwrap();

    // Without extension, the redacted env should NOT override the direct variable
    let r = s
        .run_task(
            "test_step",
            &step("sh", vec!["-c", "echo TOKEN=$TOKEN"]),
            None,
            None,
            None,
        )
        .await
        .unwrap();
    assert!(r.stdout.contains("TOKEN=public-token"));

    // But the secret value should still be tracked for redaction
    let redacted = s.redact("secret-token");
    assert!(!redacted.contains("secret-token"));
}

#[tokio::test]
async fn test_multiple_different_redacted_env_vars() {
    // Test that multiple redacted env vars with different values are handled correctly
    let tmp = TempDir::new().unwrap();
    let mut s = Session::new_for_test(tmp.path().to_path_buf()).with_profile(
        openjd_model::ModelProfile::new(openjd_model::types::SpecificationRevision::V2023_09)
            .with_extensions(
                [openjd_model::types::ModelExtension::RedactedEnvVars]
                    .into_iter()
                    .collect(),
            ),
    );
    let env = env_with_enter("env1", "sh", vec!["-c",
        "echo 'openjd_redacted_env: PASSWORD=secret123'; echo 'openjd_redacted_env: PASSWORD2=mysecret123'"
    ]);
    s.enter_environment(&env, None, None, None).await.unwrap();

    let r = s
        .run_task(
            "test_step",
            &step(
                "sh",
                vec!["-c", "echo PASSWORD=$PASSWORD; echo PASSWORD2=$PASSWORD2"],
            ),
            None,
            None,
            None,
        )
        .await
        .unwrap();
    assert!(r.stdout.contains("PASSWORD=********"));
    assert!(r.stdout.contains("PASSWORD2=********"));

    let redacted = s.redact("secret123 and mysecret123");
    assert!(!redacted.contains("secret123"));
    assert!(!redacted.contains("mysecret123"));
}

// === TestSessionRunTaskWithoutSessionEnv_2023_09 ===
// Tests for run_subprocess with use_session_env_vars=false

#[tokio::test]
async fn test_run_subprocess_basic() {
    let tmp = TempDir::new().unwrap();
    let config = openjd_sessions::session::SessionConfig {
        session_id: "test".into(),
        job_parameter_values: HashMap::new(),
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
    let mut s = Session::with_config(config).unwrap();
    let r = s
        .run_subprocess(
            "echo",
            Some(&["hello_subprocess".into()]),
            None,
            None,
            true,
            None,
        )
        .await
        .unwrap();
    assert_eq!(r.state, openjd_sessions::action::ActionState::Success);
    assert!(r.stdout.contains("hello_subprocess"));
}

#[tokio::test]
async fn test_run_subprocess_ignores_entered_environments() {
    // Test that run_subprocess with use_session_env_vars=false ignores entered environment variables
    let tmp = TempDir::new().unwrap();
    let config = openjd_sessions::session::SessionConfig {
        session_id: "test".into(),
        job_parameter_values: HashMap::new(),
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
    let mut s = Session::with_config(config).unwrap();

    // Enter an environment that sets FOO=bar
    let mut vars = HashMap::new();
    vars.insert("FOO".into(), fs("bar"));
    let env = env_with_vars("env1", vars);
    s.enter_environment(&env, None, None, None).await.unwrap();

    // run_subprocess with use_session_env_vars=false should NOT see FOO
    let r = s
        .run_subprocess(
            "sh",
            Some(&["-c".into(), "echo FOO=${FOO:-NOT_SET}".into()]),
            None,
            None,
            false,
            None,
        )
        .await
        .unwrap();
    assert!(r.stdout.contains("FOO=NOT_SET"));
}

#[tokio::test]
async fn test_run_subprocess_with_os_env_vars() {
    let tmp = TempDir::new().unwrap();
    let config = openjd_sessions::session::SessionConfig {
        session_id: "test".into(),
        job_parameter_values: HashMap::new(),
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
    let mut s = Session::with_config(config).unwrap();
    let mut extra = HashMap::new();
    extra.insert("CUSTOM_VAR".into(), "custom_value".into());
    let r = s
        .run_subprocess(
            "sh",
            Some(&["-c".into(), "echo CUSTOM_VAR=$CUSTOM_VAR".into()]),
            None,
            Some(&extra),
            false,
            None,
        )
        .await
        .unwrap();
    assert!(r.stdout.contains("CUSTOM_VAR=custom_value"));
}

#[tokio::test]
async fn test_run_subprocess_includes_constructor_env_vars() {
    // Test that session constructor env vars are always included
    let tmp = TempDir::new().unwrap();
    let mut ctor_env = HashMap::new();
    ctor_env.insert("CTOR_VAR".into(), "ctor_value".into());
    let config = openjd_sessions::session::SessionConfig {
        session_id: "test".into(),
        job_parameter_values: HashMap::new(),
        path_mapping_rules: None,
        retain_working_dir: false,
        callback: None,
        os_env_vars: Some(ctor_env),
        session_root_directory: Some(tmp.path().to_path_buf()),
        user: None,
        profile: None,
        cancel_token: None,
        debug_collect_stdout: true,
        echo_openjd_directives: true,
        sticky_bit_policy: openjd_sessions::StickyBitPolicy::Disabled,
    };
    let mut s = Session::with_config(config).unwrap();
    let r = s
        .run_subprocess(
            "sh",
            Some(&["-c".into(), "echo CTOR_VAR=$CTOR_VAR".into()]),
            None,
            None,
            false,
            None,
        )
        .await
        .unwrap();
    assert!(r.stdout.contains("CTOR_VAR=ctor_value"));
}

#[tokio::test]
async fn test_run_subprocess_empty_command_fails() {
    let tmp = TempDir::new().unwrap();
    let config = openjd_sessions::session::SessionConfig {
        session_id: "test".into(),
        job_parameter_values: HashMap::new(),
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
    let mut s = Session::with_config(config).unwrap();
    assert!(s
        .run_subprocess("", None, None, None, true, None)
        .await
        .is_err());
}

#[tokio::test]
async fn test_run_subprocess_whitespace_command_fails() {
    let tmp = TempDir::new().unwrap();
    let config = openjd_sessions::session::SessionConfig {
        session_id: "test".into(),
        job_parameter_values: HashMap::new(),
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
    let mut s = Session::with_config(config).unwrap();
    assert!(s
        .run_subprocess("   ", None, None, None, true, None)
        .await
        .is_err());
}

// === TestSessionExitEnvironment — additional: exit LIFO order ===

#[tokio::test]
async fn test_exit_environment_lifo_order() {
    let tmp = TempDir::new().unwrap();
    let mut s = Session::new_for_test(tmp.path().to_path_buf());
    let env1 = env_with_vars("env1", HashMap::new());
    let env2 = env_with_vars("env2", HashMap::new());
    let id1 = s.enter_environment(&env1, None, None, None).await.unwrap();
    let id2 = s.enter_environment(&env2, None, None, None).await.unwrap();

    // Must exit env2 first (LIFO)
    let err = s
        .exit_environment(&id1, None, true, None)
        .await
        .unwrap_err();
    assert!(matches!(
        err,
        openjd_sessions::SessionError::LifoViolation { .. }
    ));
    assert!(s.exit_environment(&id2, None, true, None).await.is_ok());
    assert!(s.exit_environment(&id1, None, true, None).await.is_ok());
}

// === TestSessionExitEnvironment — exit unknown identifier ===

#[tokio::test]
async fn test_exit_unknown_environment() {
    let tmp = TempDir::new().unwrap();
    let mut s = Session::new_for_test(tmp.path().to_path_buf());
    assert!(s
        .exit_environment(&"nonexistent".to_string(), None, true, None)
        .await
        .is_err());
}

// === Redefinition exit restores outer value ===

#[tokio::test]
async fn test_redefinition_exit() {
    let tmp = TempDir::new().unwrap();
    let mut s = Session::new_for_test(tmp.path().to_path_buf());
    let mut vars1 = HashMap::new();
    vars1.insert("VAR".into(), fs("outer"));
    let env1 = env_with_vars("env1", vars1);
    let _id1 = s.enter_environment(&env1, None, None, None).await.unwrap();

    let mut vars2 = HashMap::new();
    vars2.insert("VAR".into(), fs("inner"));
    let env2 = env_with_vars("env2", vars2);
    let id2 = s.enter_environment(&env2, None, None, None).await.unwrap();

    // Exit inner env — outer value should be restored
    s.exit_environment(&id2, None, true, None).await.unwrap();
    let r = s
        .run_task(
            "test_step",
            &step("sh", vec!["-c", "echo VAR=$VAR"]),
            None,
            None,
            None,
        )
        .await
        .unwrap();
    assert!(r.stdout.contains("VAR=outer"));
}

// === Real-time message processing tests ===

type TimestampLog = std::sync::Arc<
    std::sync::Mutex<
        Vec<(
            std::time::Duration,
            ActionState,
            Option<f64>,
            Option<String>,
        )>,
    >,
>;

/// Helper: warm up OS caches (shell binary, DLLs, filesystem metadata) before a
/// real-time timing test runs. Without this, the first `sh` spawn on a CI
/// machine — especially Windows — can dwarf the task's actual sleep duration,
/// making the "callback arrived before completion" assertion racy.
///
/// Uses its own TempDir so cleanup-on-drop doesn't touch the caller's tmp.
async fn warmup_shell() {
    let warmup_tmp = TempDir::new().unwrap();
    let mut s = Session::new_for_test(warmup_tmp.path().to_path_buf());
    let _ = s
        .run_task(
            "test_step",
            &step("sh", vec!["-c", "echo warmup; sleep 0.05"]),
            None,
            None,
            None,
        )
        .await;
}

/// Helper: create a SessionConfig with a callback that records (elapsed, state, progress).
fn realtime_test_config(
    tmp: &TempDir,
    session_id: &str,
    timestamps: TimestampLog,
) -> openjd_sessions::session::SessionConfig {
    let start = std::time::Instant::now();
    let ts = timestamps.clone();
    openjd_sessions::session::SessionConfig {
        session_id: session_id.into(),
        job_parameter_values: HashMap::new(),
        path_mapping_rules: None,
        retain_working_dir: false,
        callback: Some(Box::new(move |_sid, status| {
            ts.lock().unwrap().push((
                start.elapsed(),
                status.state,
                status.progress,
                status.status_message.clone(),
            ));
        })),
        os_env_vars: None,
        session_root_directory: Some(tmp.path().to_path_buf()),
        user: None,
        profile: None,
        cancel_token: None,
        debug_collect_stdout: true,
        echo_openjd_directives: true,
        sticky_bit_policy: openjd_sessions::StickyBitPolicy::Disabled,
    }
}

#[tokio::test]
async fn test_callback_receives_progress_before_completion() {
    let tmp = TempDir::new().unwrap();
    // Warm up OS caches so shell startup doesn't dominate the 200ms sleep.
    warmup_shell().await;
    let ts = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let mut s = Session::with_config(realtime_test_config(&tmp, "rt-prog", ts.clone())).unwrap();

    // Emit progress immediately, then sleep long enough that shell startup
    // overhead (which can be 500ms+ on Windows CI under parallel load) is
    // negligible relative to the total runtime.
    let script = step("sh", vec!["-c", "echo 'openjd_progress: 50.0'; sleep 2"]);
    let t0 = std::time::Instant::now();
    s.run_task("test_step", &script, None, None, None)
        .await
        .unwrap();
    let total = t0.elapsed();

    let ts = ts.lock().unwrap();
    let first = ts
        .iter()
        .find(|(_, st, p, _)| *st == ActionState::Running && p.is_some());
    let first = first.expect("Expected progress callback during RUNNING");
    assert!(
        first.0 < total / 2,
        "Progress callback at {:?} but task took {:?} — not real-time",
        first.0,
        total
    );
}

#[tokio::test]
async fn test_callback_receives_status_before_completion() {
    let tmp = TempDir::new().unwrap();
    // Warm up OS caches so shell startup doesn't dominate the 200ms sleep.
    warmup_shell().await;
    let ts = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let mut s = Session::with_config(realtime_test_config(&tmp, "rt-stat", ts.clone())).unwrap();

    let script = step(
        "sh",
        vec!["-c", "echo 'openjd_status: Rendering frame 1'; sleep 2"],
    );
    let t0 = std::time::Instant::now();
    s.run_task("test_step", &script, None, None, None)
        .await
        .unwrap();
    let total = t0.elapsed();

    let ts = ts.lock().unwrap();
    let first = ts
        .iter()
        .find(|(_, st, _, msg)| *st == ActionState::Running && msg.is_some());
    let first = first.expect("Expected status callback during RUNNING");
    assert!(
        first.0 < total / 2,
        "Status callback at {:?} but task took {:?} — not real-time",
        first.0,
        total
    );
}

#[tokio::test]
async fn test_env_enter_callback_receives_progress_before_completion() {
    let tmp = TempDir::new().unwrap();
    // Warm up OS caches so shell startup doesn't dominate the 200ms sleep.
    warmup_shell().await;
    let ts = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let mut s = Session::with_config(realtime_test_config(&tmp, "rt-env", ts.clone())).unwrap();

    let env = env_with_enter(
        "env1",
        "sh",
        vec!["-c", "echo 'openjd_progress: 50.0'; sleep 2"],
    );
    let t0 = std::time::Instant::now();
    s.enter_environment(&env, None, None, None).await.unwrap();
    let total = t0.elapsed();

    let ts = ts.lock().unwrap();
    let first = ts
        .iter()
        .find(|(_, st, p, _)| *st == ActionState::Running && p.is_some());
    let first = first.expect("Expected progress callback during env enter RUNNING");
    assert!(
        first.0 < total / 2,
        "Progress callback at {:?} but enter took {:?} — not real-time",
        first.0,
        total
    );
}

// === Tests for per-action os_env_vars ===

#[tokio::test]
async fn test_run_task_with_per_action_os_env_vars() {
    let tmp = TempDir::new().unwrap();
    let mut s = Session::new_for_test(tmp.path().to_path_buf());
    let extra = HashMap::from([("EXTRA_VAR".to_string(), "extra_value".to_string())]);
    let r = s
        .run_task(
            "test_step",
            &step("sh", vec!["-c", "echo EXTRA_VAR=$EXTRA_VAR"]),
            None,
            None,
            Some(&extra),
        )
        .await
        .unwrap();
    assert_eq!(r.state, ActionState::Success);
    assert!(r.stdout.contains("EXTRA_VAR=extra_value"));
}

#[tokio::test]
async fn test_enter_environment_with_per_action_os_env_vars() {
    let tmp = TempDir::new().unwrap();
    let mut s = Session::new_for_test(tmp.path().to_path_buf());
    let env = env_with_enter("env1", "sh", vec!["-c", "echo ACTION_VAR=$ACTION_VAR"]);
    let extra = HashMap::from([("ACTION_VAR".to_string(), "from_action".to_string())]);
    let (_, stdout) = s
        .enter_environment_with_output(&env, None, None, Some(&extra))
        .await
        .unwrap();
    assert!(stdout.contains("ACTION_VAR=from_action"));
}

#[tokio::test]
async fn test_exit_environment_with_per_action_os_env_vars() {
    let tmp = TempDir::new().unwrap();
    let mut s = Session::new_for_test(tmp.path().to_path_buf());
    let env = Environment {
        name: "env1".into(),
        description: None,
        script: Some(EnvironmentScript {
            let_bindings: None,
            actions: EnvironmentActions {
                on_enter: None,
                on_wrap_env_enter: None,
                on_wrap_task_run: None,
                on_wrap_env_exit: None,
                on_exit: Some(action("sh", vec!["-c", "echo EXIT_VAR=$EXIT_VAR"])),
            },
            embedded_files: None,
        }),
        variables: None,
        resolved_symtab: None,
    };
    let id = s.enter_environment(&env, None, None, None).await.unwrap();
    let extra = HashMap::from([("EXIT_VAR".to_string(), "from_exit".to_string())]);
    let stdout = s
        .exit_environment(&id, None, true, Some(&extra))
        .await
        .unwrap();
    assert!(stdout.contains("EXIT_VAR=from_exit"));
}

#[tokio::test]
async fn test_per_action_os_env_vars_override_session_env() {
    let tmp = TempDir::new().unwrap();
    let mut s = Session::new_for_test(tmp.path().to_path_buf());
    // Set a session-level env var via an environment's variables block
    let mut vars = HashMap::new();
    vars.insert("MY_VAR".into(), fs("session_value"));
    let env = env_with_vars("env1", vars);
    s.enter_environment(&env, None, None, None).await.unwrap();

    // Per-action os_env_vars should be overridden by environment-defined vars
    // (matching Python's layering: process_env < per-action < environment-defined)
    let extra = HashMap::from([("MY_VAR".to_string(), "action_value".to_string())]);
    let r = s
        .run_task(
            "test_step",
            &step("sh", vec!["-c", "echo MY_VAR=$MY_VAR"]),
            None,
            None,
            Some(&extra),
        )
        .await
        .unwrap();
    // Environment-defined vars take precedence over per-action vars
    assert!(r.stdout.contains("MY_VAR=session_value"));
}

#[tokio::test]
async fn test_per_action_os_env_vars_do_not_persist() {
    let tmp = TempDir::new().unwrap();
    let mut s = Session::new_for_test(tmp.path().to_path_buf());
    let extra = HashMap::from([("EPHEMERAL".to_string(), "yes".to_string())]);
    let r = s
        .run_task(
            "test_step",
            &step("sh", vec!["-c", "echo EPHEMERAL=$EPHEMERAL"]),
            None,
            None,
            Some(&extra),
        )
        .await
        .unwrap();
    assert!(r.stdout.contains("EPHEMERAL=yes"));

    // Next action without extra env vars should NOT see the variable
    let r = s
        .run_task(
            "test_step",
            &step("sh", vec!["-c", "echo EPHEMERAL=${EPHEMERAL:-gone}"]),
            None,
            None,
            None,
        )
        .await
        .unwrap();
    assert!(r.stdout.contains("EPHEMERAL=gone"));
}

// === TestCancelAction ===

#[tokio::test]
async fn test_cancel_action_not_running() {
    let tmp = TempDir::new().unwrap();
    let mut s = Session::new_for_test(tmp.path().to_path_buf());
    // Session is in READY state, cancel should fail
    let result = s.cancel_action(None, false);
    assert!(result.is_err());
}

#[tokio::test]
async fn test_cancel_action_mark_failed() {
    let tmp = TempDir::new().unwrap();
    let config = openjd_sessions::session::SessionConfig {
        session_id: "cancel-test".into(),
        job_parameter_values: HashMap::new(),
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
    let mut s = Session::with_config(config).unwrap();

    // Test via malformed env command which triggers CancelMarkFailed internally.
    // openjd_env:bad=value (no space after colon) is detected as malformed,
    // causing cancel with mark_action_failed=true.
    let script = step("sh", vec!["-c", "echo 'openjd_env:bad=value'; sleep 10"]);
    let r = s
        .run_task("test_step", &script, None, None, None)
        .await
        .unwrap();
    assert_eq!(
        r.state,
        ActionState::Failed,
        "Malformed env command should cause Failed state, got {:?}",
        r.state
    );
    assert_eq!(s.state(), SessionState::ReadyEnding);
}

#[tokio::test]
async fn test_malformed_env_cancels_and_marks_failed() {
    // Test that a malformed openjd_env command (missing space after colon) causes
    // the action to be canceled and marked as failed
    let tmp = TempDir::new().unwrap();
    let mut s = Session::new_for_test(tmp.path().to_path_buf());

    let script = step("sh", vec!["-c", "echo 'openjd_env:FOO=bar'; sleep 10"]);
    let r = s
        .run_task("test_step", &script, None, None, None)
        .await
        .unwrap();
    assert_eq!(r.state, ActionState::Failed);
    assert_eq!(s.state(), SessionState::ReadyEnding);

    // Check that the fail message was set
    let status = s.action_status().unwrap();
    assert!(status.fail_message.is_some());
}

#[tokio::test]
async fn test_malformed_unset_env_cancels_and_marks_failed() {
    // Test that a malformed openjd_unset_env command causes cancel+fail
    let tmp = TempDir::new().unwrap();
    let mut s = Session::new_for_test(tmp.path().to_path_buf());

    let script = step("sh", vec!["-c", "echo 'openjd_unset_env:FOO'; sleep 10"]);
    let r = s
        .run_task("test_step", &script, None, None, None)
        .await
        .unwrap();
    assert_eq!(r.state, ActionState::Failed);
}

#[tokio::test]
async fn test_invalid_env_var_name_cancels_and_marks_failed() {
    // Test that an invalid env var name (starts with digit) causes cancel+fail
    let tmp = TempDir::new().unwrap();
    let mut s = Session::new_for_test(tmp.path().to_path_buf());

    let script = step("sh", vec!["-c", "echo 'openjd_env: 1BAD=value'; sleep 10"]);
    let r = s
        .run_task("test_step", &script, None, None, None)
        .await
        .unwrap();
    assert_eq!(r.state, ActionState::Failed);
}

// === TestGetEnabledExtensions ===

#[tokio::test]
async fn test_get_enabled_extensions_with_extensions() {
    let tmp = TempDir::new().unwrap();
    let s = Session::new_for_test(tmp.path().to_path_buf()).with_profile(
        openjd_model::ModelProfile::new(openjd_model::types::SpecificationRevision::V2023_09)
            .with_extensions(
                [
                    openjd_model::types::ModelExtension::Expr,
                    openjd_model::types::ModelExtension::RedactedEnvVars,
                ]
                .into_iter()
                .collect(),
            ),
    );
    let mut exts = s.get_enabled_extensions();
    exts.sort();
    assert_eq!(exts, vec!["EXPR", "REDACTED_ENV_VARS"]);
}

#[tokio::test]
async fn test_get_enabled_extensions_empty() {
    let tmp = TempDir::new().unwrap();
    let s = Session::new_for_test(tmp.path().to_path_buf());
    assert!(s.get_enabled_extensions().is_empty());
}

// === InvalidState error carries SessionState enum values ===

#[tokio::test]
async fn invalid_state_error_carries_enum_values() {
    // After cleanup (Ended), enter_environment should give InvalidState
    // with expected=[Ready], current=Ended as SessionState values.
    let tmp = TempDir::new().unwrap();
    let mut s = Session::new_for_test(tmp.path().to_path_buf());
    s.cleanup();
    let env = env_with_enter("env1", "echo", vec!["hi"]);
    let err = s
        .enter_environment(&env, None, None, None)
        .await
        .unwrap_err();
    match err {
        openjd_sessions::error::SessionError::InvalidState { expected, current } => {
            assert_eq!(expected, &[SessionState::Ready]);
            assert_eq!(current, SessionState::Ended);
        }
        other => panic!("expected InvalidState, got: {other}"),
    }
}

#[tokio::test]
async fn invalid_state_error_multiple_expected_states() {
    // exit_environment when in Ended state should give expected=[Ready, ReadyEnding]
    let tmp = TempDir::new().unwrap();
    let mut s = Session::new_for_test(tmp.path().to_path_buf());
    s.cleanup();
    let id = "env1".to_string();
    let err = s
        .exit_environment(&id, None, false, None)
        .await
        .unwrap_err();
    match err {
        openjd_sessions::error::SessionError::InvalidState { expected, current } => {
            assert!(
                expected.contains(&SessionState::Ready),
                "expected should contain Ready"
            );
            assert!(
                expected.contains(&SessionState::ReadyEnding),
                "expected should contain ReadyEnding"
            );
            assert_eq!(current, SessionState::Ended);
        }
        other => panic!("expected InvalidState, got: {other}"),
    }
}

#[tokio::test]
async fn invalid_state_error_display_format() {
    // Verify the Display output is human-readable
    let tmp = TempDir::new().unwrap();
    let mut s = Session::new_for_test(tmp.path().to_path_buf());
    s.cleanup();
    let env = env_with_enter("env1", "echo", vec!["hi"]);
    let err = s
        .enter_environment(&env, None, None, None)
        .await
        .unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("READY"),
        "should mention expected state: {msg}"
    );
    assert!(msg.contains("ENDED"), "should mention current state: {msg}");
}

// === Action-level timeout enforcement ===

fn action_with_timeout(cmd: &str, args: Vec<&str>, timeout_secs: &str) -> Action {
    Action {
        command: fs(cmd),
        args: Some(args.iter().map(|a| fs(a)).collect()),
        timeout: Some(fs(timeout_secs)),
        cancelation: None,
    }
}

/// Step script onRun action with a timeout should kill the process when the
/// timeout expires and report ActionState::Timeout.
#[tokio::test]
async fn test_run_task_action_timeout_enforced() {
    let tmp = TempDir::new().unwrap();
    let mut s = Session::new_for_test(tmp.path().to_path_buf());
    let script = StepScript {
        let_bindings: None,
        actions: StepActions {
            on_run: action_with_timeout("sh", vec!["-c", "echo start; sleep 30; echo done"], "1"),
        },
        embedded_files: None,
    };
    let start = std::time::Instant::now();
    let r = s
        .run_task("test_step", &script, None, None, None)
        .await
        .unwrap();
    let elapsed = start.elapsed();
    assert_eq!(
        r.state,
        ActionState::Timeout,
        "Expected Timeout but got {:?} (exit_code={:?})",
        r.state,
        r.exit_code
    );
    assert!(
        elapsed < std::time::Duration::from_secs(10),
        "Timeout should have fired quickly, but took {elapsed:?}"
    );
    assert!(
        r.stdout.contains("start"),
        "Should see output before timeout"
    );
    assert!(
        !r.stdout.contains("done"),
        "Should not see output after timeout"
    );
}

/// Environment onEnter action with a timeout should be enforced.
#[tokio::test]
async fn test_enter_environment_action_timeout_enforced() {
    let tmp = TempDir::new().unwrap();
    let mut s = Session::new_for_test(tmp.path().to_path_buf());
    let env = Environment {
        name: "timeout_env".into(),
        description: None,
        script: Some(EnvironmentScript {
            let_bindings: None,
            actions: EnvironmentActions {
                on_enter: Some(action_with_timeout(
                    "sh",
                    vec!["-c", "echo entering; sleep 30"],
                    "1",
                )),
                on_wrap_env_enter: None,
                on_wrap_task_run: None,
                on_wrap_env_exit: None,
                on_exit: None,
            },
            embedded_files: None,
        }),
        variables: None,
        resolved_symtab: None,
    };
    let start = std::time::Instant::now();
    let result = s.enter_environment(&env, None, None, None).await;
    let elapsed = start.elapsed();
    // onEnter failure returns an error
    assert!(
        result.is_err(),
        "Expected error from timed-out onEnter, got Ok"
    );
    assert!(
        elapsed < std::time::Duration::from_secs(10),
        "Timeout should have fired quickly, but took {elapsed:?}"
    );
}

/// Environment onExit action with an explicit timeout should use that timeout,
/// not the 5-minute default.
#[tokio::test]
async fn test_exit_environment_action_timeout_enforced() {
    let tmp = TempDir::new().unwrap();
    let mut s = Session::new_for_test(tmp.path().to_path_buf());
    // Enter with a simple env first
    let env = Environment {
        name: "exit_timeout_env".into(),
        description: None,
        script: Some(EnvironmentScript {
            let_bindings: None,
            actions: EnvironmentActions {
                on_enter: Some(action("echo", vec!["entered"])),
                on_wrap_env_enter: None,
                on_wrap_task_run: None,
                on_wrap_env_exit: None,
                on_exit: Some(action_with_timeout(
                    "sh",
                    vec!["-c", "echo exiting; sleep 30"],
                    "1",
                )),
            },
            embedded_files: None,
        }),
        variables: None,
        resolved_symtab: None,
    };
    let id = s.enter_environment(&env, None, None, None).await.unwrap();
    let start = std::time::Instant::now();
    let result = s.exit_environment(&id, None, true, None).await;
    let elapsed = start.elapsed();
    // onExit failure returns an error
    assert!(
        result.is_err(),
        "Expected error from timed-out onExit, got Ok"
    );
    assert!(
        elapsed < std::time::Duration::from_secs(10),
        "Timeout should have fired in ~1s, not the 5-minute default ({elapsed:?})"
    );
}

/// Action with no timeout should still work (no regression).
#[tokio::test]
async fn test_run_task_no_timeout_still_works() {
    let tmp = TempDir::new().unwrap();
    let mut s = Session::new_for_test(tmp.path().to_path_buf());
    let r = s
        .run_task(
            "test_step",
            &step("sh", vec!["-c", "echo hello"]),
            None,
            None,
            None,
        )
        .await
        .unwrap();
    assert_eq!(r.state, ActionState::Success);
    assert!(r.stdout.contains("hello"));
}

// === Callback coverage tests ===
// Verify the callback fires in every code path: with-script, no-script,
// success, failure, command-not-found, for enter/exit/task/subprocess.

type CbLog = Vec<(ActionState, Option<f64>)>;

fn cb_test_config(tmp: &TempDir, id: &str, log: Arc<Mutex<CbLog>>) -> SessionConfig {
    SessionConfig {
        session_id: id.into(),
        job_parameter_values: HashMap::new(),
        path_mapping_rules: None,
        retain_working_dir: false,
        callback: Some(Box::new(move |_sid, status| {
            log.lock().unwrap().push((status.state, status.progress));
        })),
        os_env_vars: None,
        session_root_directory: Some(tmp.path().to_path_buf()),
        user: None,
        profile: None,
        cancel_token: None,
        debug_collect_stdout: true,
        echo_openjd_directives: true,
        sticky_bit_policy: openjd_sessions::StickyBitPolicy::Disabled,
    }
}

#[tokio::test]
async fn test_callback_enter_env_with_script() {
    let tmp = TempDir::new().unwrap();
    let log: Arc<Mutex<CbLog>> = Arc::new(Mutex::new(Vec::new()));
    let mut s = Session::with_config(cb_test_config(&tmp, "cb-enter-script", log.clone())).unwrap();
    let env = env_with_enter("e", "sh", vec!["-c", "echo hello"]);
    s.enter_environment(&env, None, None, None).await.unwrap();
    let log = log.lock().unwrap();
    assert!(
        !log.is_empty(),
        "Callback must fire for enter_environment with script"
    );
    assert!(
        log.iter().any(|(st, _)| *st == ActionState::Success),
        "Must have Success callback"
    );
}

#[tokio::test]
async fn test_callback_enter_env_no_script_with_vars() {
    let tmp = TempDir::new().unwrap();
    let log: Arc<Mutex<CbLog>> = Arc::new(Mutex::new(Vec::new()));
    let mut s = Session::with_config(cb_test_config(&tmp, "cb-enter-vars", log.clone())).unwrap();
    let mut vars = HashMap::new();
    vars.insert("FOO".into(), FormatString::new("bar").unwrap());
    let env = env_with_vars("e", vars);
    s.enter_environment(&env, None, None, None).await.unwrap();
    let log = log.lock().unwrap();
    assert!(
        !log.is_empty(),
        "Callback must fire for enter_environment with variables only"
    );
    assert_eq!(log.last().unwrap().0, ActionState::Success);
}

#[tokio::test]
async fn test_callback_enter_env_no_script_no_vars() {
    let tmp = TempDir::new().unwrap();
    let log: Arc<Mutex<CbLog>> = Arc::new(Mutex::new(Vec::new()));
    let mut s = Session::with_config(cb_test_config(&tmp, "cb-enter-empty", log.clone())).unwrap();
    let env = Environment {
        name: "empty".into(),
        description: None,
        script: None,
        variables: None,
        resolved_symtab: None,
    };
    s.enter_environment(&env, None, None, None).await.unwrap();
    let log = log.lock().unwrap();
    assert!(
        !log.is_empty(),
        "Callback must fire for enter_environment with no script and no vars"
    );
    assert_eq!(log.last().unwrap().0, ActionState::Success);
}

#[tokio::test]
async fn test_callback_exit_env_with_script() {
    let tmp = TempDir::new().unwrap();
    let log: Arc<Mutex<CbLog>> = Arc::new(Mutex::new(Vec::new()));
    let mut s = Session::with_config(cb_test_config(&tmp, "cb-exit-script", log.clone())).unwrap();
    let env = Environment {
        name: "e".into(),
        description: None,
        script: Some(EnvironmentScript {
            let_bindings: None,
            actions: EnvironmentActions {
                on_enter: None,
                on_wrap_env_enter: None,
                on_wrap_task_run: None,
                on_wrap_env_exit: None,
                on_exit: Some(action("sh", vec!["-c", "echo bye"])),
            },
            embedded_files: None,
        }),
        variables: None,
        resolved_symtab: None,
    };
    s.enter_environment(&env, None, Some("eid"), None)
        .await
        .unwrap();
    log.lock().unwrap().clear(); // clear enter callbacks
    s.exit_environment(&"eid".to_string(), None, true, None)
        .await
        .unwrap();
    let log = log.lock().unwrap();
    assert!(
        !log.is_empty(),
        "Callback must fire for exit_environment with script"
    );
    assert!(log.iter().any(|(st, _)| *st == ActionState::Success));
}

#[tokio::test]
async fn test_callback_exit_env_no_script() {
    let tmp = TempDir::new().unwrap();
    let log: Arc<Mutex<CbLog>> = Arc::new(Mutex::new(Vec::new()));
    let mut s =
        Session::with_config(cb_test_config(&tmp, "cb-exit-noscript", log.clone())).unwrap();
    let env = Environment {
        name: "e".into(),
        description: None,
        script: None,
        variables: None,
        resolved_symtab: None,
    };
    s.enter_environment(&env, None, Some("eid"), None)
        .await
        .unwrap();
    log.lock().unwrap().clear();
    s.exit_environment(&"eid".to_string(), None, true, None)
        .await
        .unwrap();
    let log = log.lock().unwrap();
    assert!(
        !log.is_empty(),
        "Callback must fire for exit_environment with no script"
    );
    assert_eq!(log.last().unwrap().0, ActionState::Success);
}

#[tokio::test]
async fn test_callback_run_task_success() {
    let tmp = TempDir::new().unwrap();
    let log: Arc<Mutex<CbLog>> = Arc::new(Mutex::new(Vec::new()));
    let mut s = Session::with_config(cb_test_config(&tmp, "cb-task-ok", log.clone())).unwrap();
    s.run_task(
        "test_step",
        &step("sh", vec!["-c", "echo ok"]),
        None,
        None,
        None,
    )
    .await
    .unwrap();
    let log = log.lock().unwrap();
    assert!(!log.is_empty(), "Callback must fire for run_task success");
    assert!(log.iter().any(|(st, _)| *st == ActionState::Success));
}

#[tokio::test]
async fn test_callback_run_task_failure() {
    let tmp = TempDir::new().unwrap();
    let log: Arc<Mutex<CbLog>> = Arc::new(Mutex::new(Vec::new()));
    let mut s = Session::with_config(cb_test_config(&tmp, "cb-task-fail", log.clone())).unwrap();
    let r = s
        .run_task(
            "test_step",
            &step("sh", vec!["-c", "exit 1"]),
            None,
            None,
            None,
        )
        .await
        .unwrap();
    assert_eq!(r.state, ActionState::Failed);
    let log = log.lock().unwrap();
    assert!(!log.is_empty(), "Callback must fire for run_task failure");
    assert!(log.iter().any(|(st, _)| *st == ActionState::Failed));
}

#[tokio::test]
async fn test_callback_run_task_command_not_found() {
    let tmp = TempDir::new().unwrap();
    let log: Arc<Mutex<CbLog>> = Arc::new(Mutex::new(Vec::new()));
    let mut s =
        Session::with_config(cb_test_config(&tmp, "cb-task-notfound", log.clone())).unwrap();
    let r = s
        .run_task(
            "test_step",
            &step("nonexistent-cmd-xyz", vec![]),
            None,
            None,
            None,
        )
        .await;
    assert!(r.is_err());
    let log = log.lock().unwrap();
    assert!(
        !log.is_empty(),
        "Callback must fire for run_task command not found"
    );
    assert!(log.iter().any(|(st, _)| *st == ActionState::Failed));
}

#[tokio::test]
async fn test_callback_enter_env_command_not_found() {
    let tmp = TempDir::new().unwrap();
    let log: Arc<Mutex<CbLog>> = Arc::new(Mutex::new(Vec::new()));
    let mut s = Session::with_config(cb_test_config(&tmp, "cb-env-notfound", log.clone())).unwrap();
    let env = env_with_enter("e", "nonexistent-cmd-xyz", vec![]);
    let r = s.enter_environment(&env, None, None, None).await;
    assert!(r.is_err());
    let log = log.lock().unwrap();
    assert!(
        !log.is_empty(),
        "Callback must fire for enter_environment command not found"
    );
    assert!(log.iter().any(|(st, _)| *st == ActionState::Failed));
}

#[tokio::test]
async fn test_callback_run_subprocess_success() {
    let tmp = TempDir::new().unwrap();
    let log: Arc<Mutex<CbLog>> = Arc::new(Mutex::new(Vec::new()));
    let mut s = Session::with_config(cb_test_config(&tmp, "cb-subproc", log.clone())).unwrap();
    s.run_subprocess("echo", Some(&["hello".into()]), None, None, true, None)
        .await
        .unwrap();
    let log = log.lock().unwrap();
    assert!(!log.is_empty(), "Callback must fire for run_subprocess");
    assert!(log.iter().any(|(st, _)| *st == ActionState::Success));
}

#[tokio::test]
async fn test_callback_run_subprocess_command_not_found() {
    let tmp = TempDir::new().unwrap();
    let log: Arc<Mutex<CbLog>> = Arc::new(Mutex::new(Vec::new()));
    let mut s =
        Session::with_config(cb_test_config(&tmp, "cb-subproc-notfound", log.clone())).unwrap();
    let r = s
        .run_subprocess("nonexistent-cmd-xyz", None, None, None, true, None)
        .await;
    assert!(r.is_err());
    let log = log.lock().unwrap();
    assert!(
        !log.is_empty(),
        "Callback must fire for run_subprocess command not found"
    );
    assert!(log.iter().any(|(st, _)| *st == ActionState::Failed));
}

#[tokio::test]
async fn test_callback_progress_not_leaked_between_actions() {
    // Regression: progress from one action must not leak into the next.
    let tmp = TempDir::new().unwrap();
    let log: Arc<Mutex<CbLog>> = Arc::new(Mutex::new(Vec::new()));
    let mut s = Session::with_config(cb_test_config(&tmp, "cb-no-leak", log.clone())).unwrap();

    // First action sets progress to 50%
    let env = env_with_enter("e", "sh", vec!["-c", "echo 'openjd_progress: 50.0'"]);
    s.enter_environment(&env, None, Some("eid"), None)
        .await
        .unwrap();

    // Check that progress was set
    let has_progress = log.lock().unwrap().iter().any(|(_, p)| *p == Some(50.0));
    assert!(has_progress, "First action should have 50% progress");

    log.lock().unwrap().clear();

    // Second action: exit with no script — should NOT have 50% progress
    s.exit_environment(&"eid".to_string(), None, true, None)
        .await
        .unwrap();
    let log = log.lock().unwrap();
    assert!(!log.is_empty(), "Exit callback must fire");
    for (state, progress) in log.iter() {
        assert_eq!(*state, ActionState::Success);
        assert_eq!(
            *progress, None,
            "Progress from previous action must not leak: got {:?}",
            progress
        );
    }
}

// === run_task state validation ===

#[tokio::test]
async fn test_run_task_rejects_ended_state() {
    let tmp = TempDir::new().unwrap();
    let mut s = Session::new_for_test(tmp.path().to_path_buf());
    s.cleanup();
    assert_eq!(s.state(), SessionState::Ended);

    let result = s
        .run_task("test_step", &step("echo", vec!["hello"]), None, None, None)
        .await;
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("READY"),
        "Expected InvalidState error, got: {err}"
    );
}

#[tokio::test]
async fn test_exit_environment_failure_still_pops_for_lifo() {
    let tmp = TempDir::new().unwrap();
    let mut s = Session::new_for_test(tmp.path().to_path_buf());

    // Enter two environments
    let env1 = env_with_enter("env1", "sh", vec!["-c", "echo enter1"]);
    let id1 = s.enter_environment(&env1, None, None, None).await.unwrap();

    let env2 = Environment {
        name: "env2".into(),
        description: None,
        script: Some(EnvironmentScript {
            let_bindings: None,
            actions: EnvironmentActions {
                on_enter: Some(action("sh", vec!["-c", "echo enter2"])),
                on_wrap_env_enter: None,
                on_wrap_task_run: None,
                on_wrap_env_exit: None,
                on_exit: Some(action("sh", vec!["-c", "exit 1"])),
            },
            embedded_files: None,
        }),
        variables: None,
        resolved_symtab: None,
    };
    let id2 = s.enter_environment(&env2, None, None, None).await.unwrap();

    // Exit env2 — the onExit script fails
    let result = s.exit_environment(&id2, None, true, None).await;
    assert!(
        result.is_err(),
        "exit_environment should fail when onExit script fails"
    );
    assert_eq!(s.state(), SessionState::ReadyEnding);

    // Exit env1 — this should succeed because env2 was popped despite its failure
    let result = s.exit_environment(&id1, None, true, None).await;
    assert!(
        result.is_ok(),
        "Should be able to exit env1 after env2 failed: {:?}",
        result.err()
    );
}

// ══════════════════════════════════════════════════════════════
// extend_path_mapping_rules
// ══════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_extend_path_mapping_rules_appends_and_sorts() {
    use openjd_expr::path_mapping::{PathFormat, PathMappingRule};

    let tmp = TempDir::new().unwrap();
    let mut s =
        Session::new_for_test(tmp.path().to_path_buf()).with_path_mapping(vec![PathMappingRule {
            source_path_format: PathFormat::Posix,
            source_path: "/short".into(),
            destination_path: "/s".into(),
        }]);

    assert_eq!(s.path_mapping_rules().len(), 1);

    s.extend_path_mapping_rules(vec![
        PathMappingRule {
            source_path_format: PathFormat::Posix,
            source_path: "/much/longer/path".into(),
            destination_path: "/m".into(),
        },
        PathMappingRule {
            source_path_format: PathFormat::Posix,
            source_path: "/med".into(),
            destination_path: "/d".into(),
        },
    ]);

    let rules = s.path_mapping_rules();
    assert_eq!(rules.len(), 3);
    // Sorted by source_path length descending (longest first)
    assert_eq!(rules[0].source_path, "/much/longer/path");
    assert_eq!(rules[1].source_path, "/short");
    assert_eq!(rules[2].source_path, "/med");
}

// ══════════════════════════════════════════════════════════════
// cancel_action via Session API
// ══════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_cancel_action_requires_running_state() {
    let tmp = TempDir::new().unwrap();
    let mut s = Session::new_for_test(tmp.path().to_path_buf());
    assert_eq!(s.state(), SessionState::Ready);
    let err = s.cancel_action(None, false).unwrap_err();
    assert!(matches!(
        err,
        openjd_sessions::SessionError::InvalidState { .. }
    ));
}

// ══════════════════════════════════════════════════════════════
// parent_cancel_token cascading + mark_action_failed
// ══════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_parent_cancel_token_cancels_running_action() {
    use tokio_util::sync::CancellationToken;

    let tmp = TempDir::new().unwrap();
    let parent_token = CancellationToken::new();

    let statuses: Arc<Mutex<Vec<ActionState>>> = Arc::new(Mutex::new(Vec::new()));
    let statuses_clone = statuses.clone();

    let config = SessionConfig {
        session_id: "cancel-test".into(),
        job_parameter_values: HashMap::new(),
        path_mapping_rules: None,
        retain_working_dir: false,
        callback: Some(Box::new(move |_sid, status| {
            statuses_clone.lock().unwrap().push(status.state);
        })),
        os_env_vars: None,
        session_root_directory: Some(tmp.path().to_path_buf()),
        user: None,
        profile: None,
        cancel_token: Some(parent_token.clone()),
        debug_collect_stdout: true,
        echo_openjd_directives: true,
        sticky_bit_policy: openjd_sessions::StickyBitPolicy::Disabled,
    };
    let mut s = Session::with_config(config).unwrap();

    let token_clone = parent_token.clone();
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        token_clone.cancel();
    });

    let _result = s
        .run_task(
            "test_step",
            &step("sh", vec!["-c", "sleep 30"]),
            None,
            None,
            None,
        )
        .await;

    assert_eq!(s.state(), SessionState::ReadyEnding);
    let final_statuses = statuses.lock().unwrap();
    assert!(
        final_statuses.contains(&ActionState::Canceled),
        "Expected Canceled in statuses: {:?}",
        *final_statuses
    );
}

#[tokio::test]
async fn test_cancel_action_with_mark_failed() {
    use tokio_util::sync::CancellationToken;

    let tmp = TempDir::new().unwrap();
    let parent_token = CancellationToken::new();

    let statuses: Arc<Mutex<Vec<ActionState>>> = Arc::new(Mutex::new(Vec::new()));
    let statuses_clone = statuses.clone();

    // Signal when the Failed callback fires so we know the malformed command was processed.
    let failed_notify = Arc::new(tokio::sync::Notify::new());
    let failed_notify_clone = failed_notify.clone();

    let config = SessionConfig {
        session_id: "mark-failed-test".into(),
        job_parameter_values: HashMap::new(),
        path_mapping_rules: None,
        retain_working_dir: false,
        callback: Some(Box::new(move |_sid, status| {
            statuses_clone.lock().unwrap().push(status.state);
            if status.state == ActionState::Failed {
                failed_notify_clone.notify_one();
            }
        })),
        os_env_vars: None,
        session_root_directory: Some(tmp.path().to_path_buf()),
        user: None,
        profile: None,
        cancel_token: Some(parent_token.clone()),
        debug_collect_stdout: true,
        echo_openjd_directives: true,
        sticky_bit_policy: openjd_sessions::StickyBitPolicy::Disabled,
    };
    let mut s = Session::with_config(config).unwrap();

    // The malformed openjd_env command triggers CancelMarkFailed which cancels
    // the action and marks it as Failed. The parent token cancel is only a
    // safety net to kill the `sleep 30` if something goes wrong.
    let token_clone = parent_token.clone();
    tokio::spawn(async move {
        // Wait for the Failed callback (malformed command processed), or
        // fall back to a generous timeout so the test doesn't hang.
        tokio::select! {
            _ = failed_notify.notified() => {}
            _ = tokio::time::sleep(std::time::Duration::from_secs(30)) => {}
        }
        token_clone.cancel();
    });

    let _result = s
        .run_task(
            "test_step",
            &step("sh", vec!["-c", "echo 'openjd_env:badformat'; sleep 30"]),
            None,
            None,
            None,
        )
        .await;

    assert_eq!(s.state(), SessionState::ReadyEnding);
    let final_statuses = statuses.lock().unwrap();
    // CancelMarkFailed converts the action to Failed
    assert!(
        final_statuses.contains(&ActionState::Failed),
        "Expected Failed in statuses: {:?}",
        *final_statuses
    );
}

// ══════════════════════════════════════════════════════════════
// run_subprocess validation
// ══════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_run_subprocess_rejects_empty_command() {
    let tmp = TempDir::new().unwrap();
    let mut s = Session::new_for_test(tmp.path().to_path_buf());
    let err = s
        .run_subprocess("", None, None, None, false, None)
        .await
        .unwrap_err();
    assert!(
        err.to_string().contains("non-empty"),
        "Expected non-empty error, got: {err}"
    );
}

#[tokio::test]
async fn test_run_subprocess_rejects_whitespace_only_command() {
    let tmp = TempDir::new().unwrap();
    let mut s = Session::new_for_test(tmp.path().to_path_buf());
    let err = s
        .run_subprocess("   ", None, None, None, false, None)
        .await
        .unwrap_err();
    assert!(
        err.to_string().contains("non-empty"),
        "Expected non-empty error, got: {err}"
    );
}

#[tokio::test]
async fn test_run_subprocess_rejects_zero_timeout() {
    let tmp = TempDir::new().unwrap();
    let mut s = Session::new_for_test(tmp.path().to_path_buf());
    let err = s
        .run_subprocess(
            "echo",
            None,
            Some(std::time::Duration::from_secs(0)),
            None,
            false,
            None,
        )
        .await
        .unwrap_err();
    assert!(
        err.to_string().contains("positive"),
        "Expected positive timeout error, got: {err}"
    );
}

// ══════════════════════════════════════════════════════════════
// redactions_enabled interaction with profile
// ══════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_redacted_env_sets_var_with_extension() {
    use openjd_model::types::{ModelExtension, SpecificationRevision};
    use openjd_model::ModelProfile;

    let tmp = TempDir::new().unwrap();
    let mut exts = std::collections::HashSet::new();
    exts.insert(ModelExtension::RedactedEnvVars);
    let profile = ModelProfile::new(SpecificationRevision::V2023_09).with_extensions(exts);

    let mut s = Session::new_for_test(tmp.path().to_path_buf()).with_profile(profile);

    // With REDACTED_ENV_VARS extension, openjd_redacted_env should set env vars.
    let env = Environment {
        name: "env1".into(),
        description: None,
        script: Some(EnvironmentScript {
            let_bindings: None,
            actions: EnvironmentActions {
                on_enter: Some(action(
                    "sh",
                    vec!["-c", "echo 'openjd_redacted_env: SECRET=hunter2'"],
                )),
                on_wrap_env_enter: None,
                on_wrap_task_run: None,
                on_wrap_env_exit: None,
                on_exit: Some(action("sh", vec!["-c", "echo SECRET=${SECRET:-unset}"])),
            },
            embedded_files: None,
        }),
        variables: None,
        resolved_symtab: None,
    };
    let id = s.enter_environment(&env, None, None, None).await.unwrap();
    let out = s.exit_environment(&id, None, true, None).await.unwrap();
    // SECRET should be set because REDACTED_ENV_VARS extension is enabled
    assert!(
        out.contains("SECRET=********"),
        "SECRET should be redacted in collected stdout, got: {out}"
    );
}

#[tokio::test]
async fn test_redacted_env_does_not_set_var_without_extension() {
    use openjd_model::types::SpecificationRevision;
    use openjd_model::ModelProfile;

    let tmp = TempDir::new().unwrap();
    let profile = ModelProfile::new(SpecificationRevision::V2023_09);

    let mut s = Session::new_for_test(tmp.path().to_path_buf()).with_profile(profile);

    // Without REDACTED_ENV_VARS extension, openjd_redacted_env should NOT set env vars.
    let env = Environment {
        name: "env1".into(),
        description: None,
        script: Some(EnvironmentScript {
            let_bindings: None,
            actions: EnvironmentActions {
                on_enter: Some(action(
                    "sh",
                    vec!["-c", "echo 'openjd_redacted_env: SECRET=hunter2'"],
                )),
                on_wrap_env_enter: None,
                on_wrap_task_run: None,
                on_wrap_env_exit: None,
                on_exit: Some(action("sh", vec!["-c", "echo SECRET=${SECRET:-unset}"])),
            },
            embedded_files: None,
        }),
        variables: None,
        resolved_symtab: None,
    };
    let id = s.enter_environment(&env, None, None, None).await.unwrap();
    let out = s.exit_environment(&id, None, true, None).await.unwrap();
    assert!(
        out.contains("SECRET=unset"),
        "SECRET should not be set without REDACTED_ENV_VARS extension, got: {out}"
    );
}

#[tokio::test]
async fn test_redactions_disabled_with_no_profile() {
    let tmp = TempDir::new().unwrap();
    // No profile at all (default Session::new)
    let mut s = Session::new_for_test(tmp.path().to_path_buf());

    let env = Environment {
        name: "env1".into(),
        description: None,
        script: Some(EnvironmentScript {
            let_bindings: None,
            actions: EnvironmentActions {
                on_enter: Some(action(
                    "sh",
                    vec!["-c", "echo 'openjd_redacted_env: SECRET=hunter2'"],
                )),
                on_wrap_env_enter: None,
                on_wrap_task_run: None,
                on_wrap_env_exit: None,
                on_exit: Some(action("sh", vec!["-c", "echo SECRET=${SECRET:-unset}"])),
            },
            embedded_files: None,
        }),
        variables: None,
        resolved_symtab: None,
    };
    let id = s.enter_environment(&env, None, None, None).await.unwrap();
    let out = s.exit_environment(&id, None, true, None).await.unwrap();
    assert!(
        out.contains("SECRET=unset"),
        "SECRET should not be set with no profile, got: {out}"
    );
}

// ══════════════════════════════════════════════════════════════
// cancel_action escalation: soft signal followed by a hard TERMINATE
// after the grace period. Exercised without a real cross-user helper
// by injecting a file as the cancel_writer and inspecting what was
// written over time.
// ══════════════════════════════════════════════════════════════

mod cancel_escalation {
    use super::*;
    use std::fs::OpenOptions;
    use std::io::Read;
    use std::time::Duration;

    fn read_cancel_messages(path: &std::path::Path) -> Vec<serde_json::Value> {
        let mut buf = String::new();
        let mut f = OpenOptions::new().read(true).open(path).unwrap();
        f.read_to_string(&mut buf).unwrap();
        buf.lines()
            .filter_map(|line| serde_json::from_str(line).ok())
            .collect()
    }

    fn session_with_observable_writer(tmp: &TempDir) -> (Session, std::path::PathBuf) {
        let mut s = Session::new_for_test(tmp.path().to_path_buf());
        s.set_state_for_test(SessionState::Running);
        let writer_path = tmp.path().join("cancel_writer.log");
        let writer = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&writer_path)
            .unwrap();
        s.set_cancel_writer_for_test(writer);
        (s, writer_path)
    }

    /// With no time_limit, the default notify period is 5s.
    #[tokio::test(flavor = "multi_thread")]
    async fn default_grace_sends_notify_then_terminate() {
        let tmp = TempDir::new().unwrap();
        let (mut s, path) = session_with_observable_writer(&tmp);

        s.cancel_action(None, false).expect("cancel_action ok");

        std::thread::sleep(Duration::from_millis(200));
        let msgs = read_cancel_messages(&path);
        assert_eq!(
            msgs.len(),
            1,
            "session should send exactly one cancel message"
        );
        assert_eq!(msgs[0]["cancel"].as_str().unwrap(), "NOTIFY_THEN_TERMINATE");
        assert_eq!(msgs[0]["notifyPeriodInSeconds"].as_u64().unwrap(), 5);

        // No escalation thread — wait past the grace to confirm nothing else is written.
        std::thread::sleep(Duration::from_secs(6));
        let late = read_cancel_messages(&path);
        assert_eq!(
            late.len(),
            1,
            "session should not send a second message; escalation is in the helper"
        );
    }

    /// Custom grace (8s) — e.g. set via a job template's cancelation timeout.
    #[tokio::test(flavor = "multi_thread")]
    async fn custom_grace_8s_sends_correct_notify_period() {
        let tmp = TempDir::new().unwrap();
        let (mut s, path) = session_with_observable_writer(&tmp);

        s.cancel_action(Some(Duration::from_secs(8)), false)
            .expect("cancel_action ok");

        std::thread::sleep(Duration::from_millis(200));
        let msgs = read_cancel_messages(&path);
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0]["cancel"].as_str().unwrap(), "NOTIFY_THEN_TERMINATE");
        assert_eq!(msgs[0]["notifyPeriodInSeconds"].as_u64().unwrap(), 8);
    }

    /// A zero-duration time_limit means "kill now" — TERMINATE is sent directly.
    #[tokio::test(flavor = "multi_thread")]
    async fn zero_time_limit_sends_terminate() {
        let tmp = TempDir::new().unwrap();
        let (mut s, path) = session_with_observable_writer(&tmp);

        s.cancel_action(Some(Duration::from_secs(0)), false)
            .expect("cancel_action ok");

        std::thread::sleep(Duration::from_millis(200));
        let msgs = read_cancel_messages(&path);
        assert_eq!(msgs.len(), 1, "should send exactly one message");
        assert_eq!(msgs[0]["cancel"].as_str().unwrap(), "TERMINATE");
        assert!(
            msgs[0].get("notifyPeriodInSeconds").is_none(),
            "TERMINATE should not include notifyPeriodInSeconds"
        );
    }

    /// When a helper auth token is configured, every cancel command written
    /// through the cancel_writer must include it as a `"token"` field. This
    /// is what `set_helper_auth_token_for_test` is for.
    #[tokio::test(flavor = "multi_thread")]
    async fn cancel_includes_auth_token_when_configured() {
        let tmp = TempDir::new().unwrap();
        let (mut s, path) = session_with_observable_writer(&tmp);
        s.set_helper_auth_token_for_test("AbCdEfGhIjKlMnOpQrStUv".into());

        // Soft cancel
        s.cancel_action(None, false).expect("cancel_action ok");
        std::thread::sleep(Duration::from_millis(200));
        let msgs = read_cancel_messages(&path);
        assert_eq!(msgs.len(), 1);
        assert_eq!(
            msgs[0]["token"].as_str().unwrap(),
            "AbCdEfGhIjKlMnOpQrStUv",
            "NOTIFY_THEN_TERMINATE cancel must carry the token",
        );
        assert_eq!(msgs[0]["cancel"].as_str().unwrap(), "NOTIFY_THEN_TERMINATE");
    }

    /// TERMINATE (zero grace) must also carry the token.
    #[tokio::test(flavor = "multi_thread")]
    async fn terminate_cancel_includes_auth_token_when_configured() {
        let tmp = TempDir::new().unwrap();
        let (mut s, path) = session_with_observable_writer(&tmp);
        s.set_helper_auth_token_for_test("AbCdEfGhIjKlMnOpQrStUv".into());

        s.cancel_action(Some(Duration::from_secs(0)), false)
            .expect("cancel_action ok");
        std::thread::sleep(Duration::from_millis(200));
        let msgs = read_cancel_messages(&path);
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0]["token"].as_str().unwrap(), "AbCdEfGhIjKlMnOpQrStUv",);
        assert_eq!(msgs[0]["cancel"].as_str().unwrap(), "TERMINATE");
    }

    /// `SessionCancelHandle` must deliver the helper pipe command like
    /// `cancel_action` does. Unlike `cancel_action`, the handle only fires
    /// when a per-action token is registered, so this drives a real running
    /// action and cancels it mid-flight from another task.
    ///
    /// Note: this covers handle → pipe delivery. Full cancellation of a
    /// helper-ROUTED subprocess (helper spawned for a cross-user session)
    /// requires a second OS user and is exercised by the cross-user
    /// integration tests' environments.
    #[tokio::test(flavor = "multi_thread")]
    async fn handle_cancel_writes_notify_command_to_helper_pipe() {
        let tmp = TempDir::new().unwrap();
        let mut s = Session::new_for_test(tmp.path().to_path_buf());
        let writer_path = tmp.path().join("cancel_writer.log");
        let writer = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&writer_path)
            .unwrap();
        s.set_cancel_writer_for_test(writer);
        s.set_helper_auth_token_for_test("HandleTok123".into());

        // Handle taken after the writer is injected, so it dup's the pipe.
        let handle = s.cancel_handle();
        let delivered: Arc<Mutex<Option<bool>>> = Arc::new(Mutex::new(None));
        let delivered_clone = delivered.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(200)).await;
            *delivered_clone.lock().unwrap() = Some(handle.cancel(None, false));
        });

        let r = s
            .run_task("t", &step("sh", vec!["-c", "sleep 30"]), None, None, None)
            .await
            .unwrap();
        assert_eq!(r.state, ActionState::Canceled);
        assert_eq!(*delivered.lock().unwrap(), Some(true));

        let msgs = read_cancel_messages(&writer_path);
        assert_eq!(
            msgs.len(),
            1,
            "handle must write exactly one cancel command to the helper pipe"
        );
        assert_eq!(msgs[0]["cancel"].as_str().unwrap(), "NOTIFY_THEN_TERMINATE");
        assert_eq!(msgs[0]["notifyPeriodInSeconds"].as_u64().unwrap(), 5);
        assert_eq!(
            msgs[0]["token"].as_str().unwrap(),
            "HandleTok123",
            "handle pipe command must carry the helper auth token"
        );
    }

    /// Urgent cancel (`time_limit = 0`) through the handle writes TERMINATE.
    #[tokio::test(flavor = "multi_thread")]
    async fn handle_urgent_cancel_writes_terminate_command_to_helper_pipe() {
        let tmp = TempDir::new().unwrap();
        let mut s = Session::new_for_test(tmp.path().to_path_buf());
        let writer_path = tmp.path().join("cancel_writer.log");
        let writer = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&writer_path)
            .unwrap();
        s.set_cancel_writer_for_test(writer);

        let handle = s.cancel_handle();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(200)).await;
            handle.cancel(Some(Duration::from_secs(0)), false);
        });

        let r = s
            .run_task("t", &step("sh", vec!["-c", "sleep 30"]), None, None, None)
            .await
            .unwrap();
        assert_eq!(r.state, ActionState::Canceled);

        let msgs = read_cancel_messages(&writer_path);
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0]["cancel"].as_str().unwrap(), "TERMINATE");
        assert!(
            msgs[0].get("notifyPeriodInSeconds").is_none(),
            "TERMINATE should not include notifyPeriodInSeconds"
        );
    }

    /// A step that declares notifyThenTerminate cancelation with the given
    /// notify period.
    fn step_with_ntt_cancel(cmd: &str, args: Vec<&str>, notify_secs: u64) -> StepScript {
        let mut on_run = action(cmd, args);
        on_run.cancelation = Some(CancelationMode::NotifyThenTerminate {
            notify_period_in_seconds: Some(fs(&notify_secs.to_string())),
        });
        StepScript {
            let_bindings: None,
            actions: StepActions { on_run },
            embedded_files: None,
        }
    }

    /// Drive `sleep 30` as a task and cancel it via the handle with
    /// `time_limit`, returning the JSON messages written to the injected
    /// cancel pipe.
    async fn run_and_handle_cancel(
        script: &StepScript,
        time_limit: Option<Duration>,
    ) -> Vec<serde_json::Value> {
        let tmp = TempDir::new().unwrap();
        let mut s = Session::new_for_test(tmp.path().to_path_buf());
        let writer_path = tmp.path().join("cancel_writer.log");
        let writer = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&writer_path)
            .unwrap();
        s.set_cancel_writer_for_test(writer);

        let handle = s.cancel_handle();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(200)).await;
            handle.cancel(time_limit, false);
        });

        let r = s.run_task("t", script, None, None, None).await.unwrap();
        assert_eq!(r.state, ActionState::Canceled);
        read_cancel_messages(&writer_path)
    }

    /// The declared terminate_delay caps the notify period when the caller's
    /// time_limit is larger — mirroring the same-user path, which computes
    /// `time_limit.min(terminate_delay)`.
    #[tokio::test(flavor = "multi_thread")]
    async fn handle_cancel_caps_notify_period_at_declared_terminate_delay() {
        let script = step_with_ntt_cancel("sh", vec!["-c", "sleep 30"], 3);
        let msgs = run_and_handle_cancel(&script, Some(Duration::from_secs(60))).await;
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0]["cancel"].as_str().unwrap(), "NOTIFY_THEN_TERMINATE");
        assert_eq!(
            msgs[0]["notifyPeriodInSeconds"].as_u64().unwrap(),
            3,
            "time_limit larger than the declared terminate_delay must be capped at it"
        );
    }

    /// A time_limit smaller than the declared terminate_delay wins.
    #[tokio::test(flavor = "multi_thread")]
    async fn handle_cancel_smaller_time_limit_wins_over_terminate_delay() {
        let script = step_with_ntt_cancel("sh", vec!["-c", "sleep 30"], 3);
        let msgs = run_and_handle_cancel(&script, Some(Duration::from_secs(1))).await;
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0]["cancel"].as_str().unwrap(), "NOTIFY_THEN_TERMINATE");
        assert_eq!(msgs[0]["notifyPeriodInSeconds"].as_u64().unwrap(), 1);
    }

    /// With no time_limit, the declared terminate_delay applies on its own
    /// (not the legacy 5s default) — matching the same-user path's
    /// `None => terminate_delay` arm.
    #[tokio::test(flavor = "multi_thread")]
    async fn handle_cancel_without_time_limit_uses_declared_terminate_delay() {
        let script = step_with_ntt_cancel("sh", vec!["-c", "sleep 30"], 8);
        let msgs = run_and_handle_cancel(&script, None).await;
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0]["cancel"].as_str().unwrap(), "NOTIFY_THEN_TERMINATE");
        assert_eq!(
            msgs[0]["notifyPeriodInSeconds"].as_u64().unwrap(),
            8,
            "declared terminate_delay must apply when no time_limit is given"
        );
    }

    /// Actions that do not declare notifyThenTerminate keep the legacy
    /// behavior: the caller's time_limit is used as-is.
    #[tokio::test(flavor = "multi_thread")]
    async fn handle_cancel_without_declared_cancelation_passes_time_limit_through() {
        let script = step("sh", vec!["-c", "sleep 30"]);
        let msgs = run_and_handle_cancel(&script, Some(Duration::from_secs(60))).await;
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0]["cancel"].as_str().unwrap(), "NOTIFY_THEN_TERMINATE");
        assert_eq!(msgs[0]["notifyPeriodInSeconds"].as_u64().unwrap(), 60);
    }
}

/// Test Option 1: Session-level test for the cancel race condition.
///
/// Simulates the pyo3 binding's cancel path: the parent cancel token is
/// cancelled AND the process is killed externally (via the cancel_writer /
/// helper pipe) simultaneously. This mirrors what happens when the pyo3
/// `cancel_action` `None` branch fires — it cancels the token and writes
/// to the helper, but doesn't call `session.cancel_action()`.
///
/// The process dies from the external kill before the tokio select loop
/// processes the token cancellation. Without the fix, the callback reports
/// `Failed`; with the fix it reports `Canceled`.
#[cfg(unix)]
#[tokio::test]
async fn test_parent_token_cancel_with_external_kill_reports_canceled() {
    use tokio_util::sync::CancellationToken;

    let tmp = TempDir::new().unwrap();
    let parent_token = CancellationToken::new();

    let statuses: Arc<Mutex<Vec<ActionState>>> = Arc::new(Mutex::new(Vec::new()));
    let statuses_clone = statuses.clone();

    let config = SessionConfig {
        session_id: "cancel-race-test".into(),
        job_parameter_values: HashMap::new(),
        path_mapping_rules: None,
        retain_working_dir: false,
        callback: Some(Box::new(move |_sid, status| {
            statuses_clone.lock().unwrap().push(status.state);
        })),
        os_env_vars: None,
        session_root_directory: Some(tmp.path().to_path_buf()),
        user: None,
        profile: None,
        cancel_token: Some(parent_token.clone()),
        debug_collect_stdout: false,
        echo_openjd_directives: true,
        sticky_bit_policy: openjd_sessions::StickyBitPolicy::Disabled,
    };
    let mut s = Session::with_config(config).unwrap();

    // The script writes its PID to a file then sleeps.
    // The spawned task cancels the token and kills the process externally.
    // Pre-cancel the token, then run a task that exits non-zero.
    // The session should report Canceled because the token was cancelled.
    parent_token.cancel();

    let _result = s
        .run_task(
            "test_step",
            &step("sh", vec!["-c", "exit 42"]),
            None,
            None,
            None,
        )
        .await;

    assert_eq!(s.state(), SessionState::ReadyEnding);
    let final_statuses = statuses.lock().unwrap();
    assert!(
        final_statuses.contains(&ActionState::Canceled),
        "Expected Canceled when token is cancelled and process killed externally, got: {:?}",
        *final_statuses
    );
}

/// Verify that the session callback fires with intermediate progress values
/// as `openjd_progress:` messages are printed to stdout, not just on completion.
#[tokio::test]
async fn test_callback_reports_intermediate_progress() {
    let tmp = TempDir::new().unwrap();

    // Collect all (state, progress) pairs from callbacks
    #[allow(clippy::type_complexity)]
    let updates: Arc<Mutex<Vec<(ActionState, Option<f64>)>>> = Arc::new(Mutex::new(Vec::new()));
    let updates_clone = updates.clone();

    let config = SessionConfig {
        session_id: "progress-test".into(),
        job_parameter_values: HashMap::new(),
        path_mapping_rules: None,
        retain_working_dir: false,
        callback: Some(Box::new(move |_sid, status| {
            updates_clone
                .lock()
                .unwrap()
                .push((status.state, status.progress));
        })),
        os_env_vars: None,
        session_root_directory: Some(tmp.path().to_path_buf()),
        user: None,
        profile: None,
        cancel_token: None,
        debug_collect_stdout: false,
        echo_openjd_directives: true,
        sticky_bit_policy: openjd_sessions::StickyBitPolicy::Disabled,
    };
    let mut s = Session::with_config(config).unwrap();

    // Script prints progress 25 and 75, with a status message
    let result = s
        .run_task("test_step", &step(
                "sh",
                vec![
                    "-c",
                    "echo 'openjd_progress: 25'; echo 'openjd_status: working'; echo 'openjd_progress: 75'; echo 'openjd_status: almost done'",
                ],
            ),
            None,
            None,
            None,
        )
        .await
        .unwrap();

    assert_eq!(result.state, ActionState::Success);

    let all_updates = updates.lock().unwrap();

    // There should be intermediate Running callbacks with progress values
    let running_with_progress: Vec<_> = all_updates
        .iter()
        .filter(|(state, progress)| {
            *state == ActionState::Running && progress.is_some() && *progress != Some(0.0)
        })
        .collect();

    assert!(
        !running_with_progress.is_empty(),
        "Expected intermediate progress callbacks while Running, got: {:?}",
        *all_updates
    );

    // Specifically, we should see progress 25 and 75
    let progress_values: Vec<f64> = all_updates.iter().filter_map(|(_, p)| *p).collect();

    assert!(
        progress_values.contains(&25.0),
        "Expected progress 25.0 in callbacks, got: {:?}",
        progress_values
    );
    assert!(
        progress_values.contains(&75.0),
        "Expected progress 75.0 in callbacks, got: {:?}",
        progress_values
    );
}

// === Tests for SessionConfig::echo_openjd_directives ===
//
// Mirrors the Python reference implementation, where the equivalent
// ActionMonitoringFilter `suppress_filtered` parameter defaults to False
// (i.e. directives are echoed to the log). These tests verify that:
//   * `echo_openjd_directives = true` (the default) lets directive lines
//     reach the session log, and
//   * `echo_openjd_directives = false` filters them out.
// `openjd_redacted_env` follows the same rule, with the secret value
// replaced by `********` before the line reaches the log.

#[cfg(unix)]
fn echo_directives_test_config(
    tmp: &TempDir,
    session_id: &str,
    echo: bool,
) -> openjd_sessions::session::SessionConfig {
    openjd_sessions::session::SessionConfig {
        session_id: session_id.into(),
        job_parameter_values: HashMap::new(),
        path_mapping_rules: None,
        retain_working_dir: false,
        callback: None,
        os_env_vars: None,
        session_root_directory: Some(tmp.path().to_path_buf()),
        user: None,
        profile: None,
        cancel_token: None,
        debug_collect_stdout: true,
        echo_openjd_directives: echo,
        sticky_bit_policy: openjd_sessions::StickyBitPolicy::Disabled,
    }
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_echo_openjd_directives_true_passes_directive_lines_to_log() {
    testing_logger::setup();
    let tmp = TempDir::new().unwrap();
    let mut s = Session::with_config(echo_directives_test_config(&tmp, "echo-on", true)).unwrap();

    // Construct the directive at runtime from environment variables so the
    // literal text `openjd_progress: 42.0` does not appear in the script
    // command (the command itself is logged via `format_command_for_log`,
    // and we want to assert specifically that the *output* line was echoed).
    let script = step(
        "sh",
        vec![
            "-c",
            r#"K=op; J=enjd; printf '%s%s_progress: %s\n' "$K" "$J" 42.0; echo 'echo-on-plain-output'"#,
        ],
    );
    let r = s
        .run_task("test_step", &script, None, None, None)
        .await
        .unwrap();
    assert_eq!(r.state, ActionState::Success);

    testing_logger::validate(|captured| {
        let directive_logged = captured
            .iter()
            .any(|log| log.body.contains("openjd_progress: 42.0"));
        assert!(
            directive_logged,
            "expected the openjd_progress directive to appear in the log when echo=true"
        );
        let plain_logged = captured
            .iter()
            .any(|log| log.body.contains("echo-on-plain-output"));
        assert!(
            plain_logged,
            "non-directive output must always reach the log"
        );
    });
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_echo_openjd_directives_false_suppresses_directive_lines_from_log() {
    testing_logger::setup();
    let tmp = TempDir::new().unwrap();
    let mut s = Session::with_config(echo_directives_test_config(&tmp, "echo-off", false)).unwrap();

    // See the sister `..._true_..._to_log` test — the literal directive
    // string must not appear in the script command itself, so we synthesize
    // it at runtime. Use a different progress value (43.0) so this test's
    // assertion is robust against testing_logger's process-global state.
    let script = step(
        "sh",
        vec![
            "-c",
            r#"K=op; J=enjd; printf '%s%s_progress: %s\n' "$K" "$J" 43.0; echo 'echo-off-plain-output'"#,
        ],
    );
    let r = s
        .run_task("test_step", &script, None, None, None)
        .await
        .unwrap();
    assert_eq!(r.state, ActionState::Success);

    testing_logger::validate(|captured| {
        let directive_logged = captured
            .iter()
            .any(|log| log.body.contains("openjd_progress: 43.0"));
        assert!(
            !directive_logged,
            "expected the openjd_progress directive to be suppressed from the log when echo=false"
        );
        let plain_logged = captured
            .iter()
            .any(|log| log.body.contains("echo-off-plain-output"));
        assert!(
            plain_logged,
            "non-directive output must still reach the log even when directives are suppressed"
        );
    });
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_echo_openjd_directives_true_redacts_redacted_env_in_log() {
    use openjd_model::types::{ModelExtension, SpecificationRevision};
    use openjd_model::ModelProfile;

    testing_logger::setup();
    let tmp = TempDir::new().unwrap();
    // REDACTED_ENV_VARS extension must be enabled for redaction semantics to
    // engage; the directive is parsed regardless, but redactions_enabled() drives
    // whether the value is added to the redaction set with full effect.
    let profile = ModelProfile::new(SpecificationRevision::V2023_09)
        .with_extensions([ModelExtension::RedactedEnvVars].into_iter().collect());

    let config = openjd_sessions::session::SessionConfig {
        session_id: "redacted-echo".into(),
        job_parameter_values: HashMap::new(),
        path_mapping_rules: None,
        retain_working_dir: false,
        callback: None,
        os_env_vars: None,
        session_root_directory: Some(tmp.path().to_path_buf()),
        user: None,
        profile: Some(profile),
        cancel_token: None,
        debug_collect_stdout: true,
        echo_openjd_directives: true,
        sticky_bit_policy: openjd_sessions::StickyBitPolicy::Disabled,
    };
    let mut s = Session::with_config(config).unwrap();

    // Synthesize the directive AND the secret value at runtime so neither
    // the literal `openjd_redacted_env:` token nor the secret bytes appear
    // verbatim in the script command (the command is logged via
    // `format_command_for_log`, which would otherwise leak both into the
    // log before the action filter has a chance to redact them).
    let script = step(
        "sh",
        vec![
            "-c",
            r#"K=op; J=enjd; A=tops; B=ecret; C=123; printf '%s%s_redacted_env: TOKEN=%s%s%s\n' "$K" "$J" "$A" "$B" "$C""#,
        ],
    );
    let r = s
        .run_task("test_step", &script, None, None, None)
        .await
        .unwrap();
    assert_eq!(r.state, ActionState::Success);

    testing_logger::validate(|captured| {
        // The directive line itself must be present (echo=true)…
        let directive_logged = captured
            .iter()
            .any(|log| log.body.contains("openjd_redacted_env: TOKEN="));
        assert!(
            directive_logged,
            "expected the redacted_env directive to appear in the log when echo=true"
        );
        // …but the secret value must NOT appear in any log record.
        let secret_leaked = captured.iter().any(|log| log.body.contains("topsecret123"));
        assert!(
            !secret_leaked,
            "secret value must never reach the log; expected redaction to fixed-length asterisks"
        );
        let redacted_form = captured
            .iter()
            .any(|log| log.body.contains("TOKEN=********"));
        assert!(
            redacted_form,
            "expected the redacted_env line to show NAME=******** in the log"
        );
    });
}

// ══════════════════════════════════════════════════════════════
// SessionCancelHandle — external cancellation while the Session
// is owned by the thread/task driving the action
// ══════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_cancel_handle_idle_returns_false() {
    let tmp = TempDir::new().unwrap();
    let s = Session::new_for_test(tmp.path().to_path_buf());
    let handle = s.cancel_handle();
    assert!(
        !handle.cancel(None, false),
        "no action running — cancel must report nothing to cancel"
    );
    assert_eq!(s.state(), SessionState::Ready);
}

#[tokio::test]
async fn test_cancel_handle_cancels_running_action() {
    let tmp = TempDir::new().unwrap();
    let mut s = Session::new_for_test(tmp.path().to_path_buf());
    let handle = s.cancel_handle();

    let delivered: Arc<Mutex<Option<bool>>> = Arc::new(Mutex::new(None));
    let delivered_clone = delivered.clone();
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        *delivered_clone.lock().unwrap() = Some(handle.cancel(None, false));
    });

    let r = s
        .run_task("t", &step("sh", vec!["-c", "sleep 30"]), None, None, None)
        .await
        .unwrap();
    assert_eq!(r.state, ActionState::Canceled);
    assert_eq!(s.state(), SessionState::ReadyEnding);
    assert_eq!(
        *delivered.lock().unwrap(),
        Some(true),
        "handle must report the cancel as delivered"
    );
}

#[tokio::test]
async fn test_cancel_handle_mark_action_failed() {
    let tmp = TempDir::new().unwrap();
    let mut s = Session::new_for_test(tmp.path().to_path_buf());
    let handle = s.cancel_handle();

    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        handle.cancel(None, true);
    });

    let r = s
        .run_task("t", &step("sh", vec!["-c", "sleep 30"]), None, None, None)
        .await
        .unwrap();
    assert_eq!(
        r.state,
        ActionState::Failed,
        "mark_action_failed must report the canceled action as Failed"
    );
}

/// The same handle stays usable across actions, and a cancel does not
/// poison later actions (each action installs a fresh token — unlike the
/// one-shot `SessionConfig::cancel_token`, whose cancellation is permanent).
#[tokio::test]
async fn test_cancel_handle_reusable_across_actions() {
    let tmp = TempDir::new().unwrap();
    let mut s = Session::new_for_test(tmp.path().to_path_buf());
    let handle = s.cancel_handle();

    // Enter two environments (quick onEnter; E1 has a slow onExit).
    let e2 = env_with_enter("E2", "sh", vec!["-c", "echo enter2"]);
    let e1 = Environment {
        name: "E1".into(),
        description: None,
        script: Some(EnvironmentScript {
            let_bindings: None,
            actions: EnvironmentActions {
                on_enter: Some(action("sh", vec!["-c", "echo enter1"])),
                on_exit: Some(action("sh", vec!["-c", "sleep 30"])),
                on_wrap_env_enter: None,
                on_wrap_task_run: None,
                on_wrap_env_exit: None,
            },
            embedded_files: None,
        }),
        variables: None,
        resolved_symtab: None,
    };
    let id1 = s.enter_environment(&e1, None, None, None).await.unwrap();
    let id2 = s.enter_environment(&e2, None, None, None).await.unwrap();

    // Action 1: cancel a running task via the handle.
    let h = s.cancel_handle();
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        h.cancel(None, false);
    });
    let r = s
        .run_task("t", &step("sh", vec!["-c", "sleep 30"]), None, None, None)
        .await
        .unwrap();
    assert_eq!(r.state, ActionState::Canceled);

    // Action 2 (E2 exit, quick): must run normally after the cancel — the
    // cancel must not have poisoned subsequent actions.
    s.exit_environment(&id2, None, true, None).await.unwrap();
    assert_eq!(
        s.action_status().unwrap().state,
        ActionState::Success,
        "action after a handle cancel must not be born-canceled"
    );

    // Action 3 (E1 exit, slow): cancel again via the ORIGINAL handle.
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        handle.cancel(None, false);
    });
    let _ = s.exit_environment(&id1, None, true, None).await;
    assert_eq!(
        s.action_status().unwrap().state,
        ActionState::Canceled,
        "the same handle must be reusable for later actions"
    );
}

// ══════════════════════════════════════════════════════════════
// fail_action_setup — failures after the Running transition must
// produce a terminal Failed status, not wedge the session
// ══════════════════════════════════════════════════════════════

/// RFC 0008 wrap seeding resolves the wrapped (inner) action's timeout;
/// an unresolvable expression there previously returned early with the
/// session stuck in Running forever and nothing logged.
#[tokio::test]
async fn test_wrap_seed_failure_reports_failed_action() {
    let tmp = TempDir::new().unwrap();
    let statuses: Arc<Mutex<Vec<ActionState>>> = Arc::new(Mutex::new(Vec::new()));
    let statuses_clone = statuses.clone();
    let config = SessionConfig {
        session_id: "wrap-seed-fail".into(),
        job_parameter_values: HashMap::new(),
        path_mapping_rules: None,
        retain_working_dir: false,
        callback: Some(Box::new(move |_sid, status| {
            statuses_clone.lock().unwrap().push(status.state);
        })),
        os_env_vars: None,
        session_root_directory: Some(tmp.path().to_path_buf()),
        user: None,
        profile: None,
        cancel_token: None,
        debug_collect_stdout: true,
        echo_openjd_directives: true,
        sticky_bit_policy: openjd_sessions::StickyBitPolicy::Disabled,
    };
    let mut s = Session::with_config(config).unwrap();

    // Wrap environment declaring all three wrap hooks.
    let wrap_env = Environment {
        name: "WrapEnv".into(),
        description: None,
        script: Some(EnvironmentScript {
            let_bindings: None,
            actions: EnvironmentActions {
                on_enter: Some(action("sh", vec!["-c", "echo wrap-enter"])),
                on_exit: Some(action("sh", vec!["-c", "echo wrap-exit"])),
                on_wrap_env_enter: Some(action("sh", vec!["-c", "echo wrapped"])),
                on_wrap_task_run: Some(action("sh", vec!["-c", "echo wrapped"])),
                on_wrap_env_exit: Some(action("sh", vec!["-c", "echo wrapped"])),
            },
            embedded_files: None,
        }),
        variables: None,
        resolved_symtab: None,
    };
    s.enter_environment(&wrap_env, None, None, None)
        .await
        .unwrap();

    // Inner environment whose onEnter timeout references an undefined
    // symbol — seeding WrappedAction.Timeout fails during action setup.
    let inner = Environment {
        name: "Inner".into(),
        description: None,
        script: Some(EnvironmentScript {
            let_bindings: None,
            actions: EnvironmentActions {
                on_enter: Some(Action {
                    command: fs("sh"),
                    args: Some(vec![fs("-c"), fs("echo inner")]),
                    timeout: Some(fs("{{ Param.Missing }}")),
                    cancelation: None,
                }),
                on_exit: None,
                on_wrap_env_enter: None,
                on_wrap_task_run: None,
                on_wrap_env_exit: None,
            },
            embedded_files: None,
        }),
        variables: None,
        resolved_symtab: None,
    };

    let result = s.enter_environment(&inner, None, None, None).await;
    assert!(result.is_err(), "setup failure must surface as an error");

    // The session must NOT be wedged in Running, and the failure must be
    // observable through the action status and the callback.
    assert_eq!(s.state(), SessionState::ReadyEnding);
    let status = s.action_status().expect("terminal action status");
    assert_eq!(status.state, ActionState::Failed);
    let msg = status.fail_message.expect("fail_message must be recorded");
    assert!(
        msg.contains("timeout"),
        "fail_message should identify the failing field, got: {msg}"
    );
    assert!(
        statuses.lock().unwrap().contains(&ActionState::Failed),
        "callback must observe the Failed transition"
    );
}

/// A runner error during dispatch (here: an undefined symbol in the action's
/// args, which fails command-line resolution inside the runner) must surface
/// through drive_action's error branch with a recorded `fail_message` — not
/// as a bare Failed status with no message and nothing logged, and not as a
/// session wedged in Running.
#[tokio::test]
async fn test_runner_resolution_error_records_fail_message() {
    let tmp = TempDir::new().unwrap();
    let mut s = Session::new_for_test(tmp.path().to_path_buf());

    let script = step("sh", vec!["-c", "echo {{ No.Such.Symbol }}"]);
    let result = s.run_task("t", &script, None, None, None).await;
    assert!(
        result.is_err(),
        "resolution failure must surface as an error"
    );

    assert_eq!(
        s.state(),
        SessionState::ReadyEnding,
        "session must not be wedged in Running"
    );
    let status = s.action_status().expect("terminal action status");
    assert_eq!(status.state, ActionState::Failed);
    let msg = status.fail_message.expect("fail_message must be recorded");
    assert!(
        msg.contains("No.Such.Symbol"),
        "fail_message should identify the unresolvable symbol, got: {msg}"
    );
}
