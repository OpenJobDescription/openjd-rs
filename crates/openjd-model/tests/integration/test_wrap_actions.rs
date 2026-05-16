// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Integration tests for RFC 0008 `WRAP_ACTIONS` extension.
//!
//! These tests cover the three layers of validation added for RFC 0008:
//!
//! 1. **Schema parse**: `onWrapEnter`, `onWrapTaskRun`, `onWrapExit` on
//!    `<EnvironmentActions>` and `runOnHost` on `<Action>` decode from YAML
//!    without error.
//! 2. **Extension gating**: using any of those fields without declaring
//!    `WRAP_ACTIONS` in `extensions` produces a specific, path-annotated
//!    validation error.
//! 3. **Single-layer rule**: with `WRAP_ACTIONS` enabled, at most one
//!    environment in a session stack (job envs + step envs) may define any
//!    wrap hook.
//!
//! Error assertions follow the repo convention of asserting on the full
//! Pydantic-style error path + message so regressions are caught.

use openjd_model::{decode_environment_template, decode_job_template, CallerLimits};

const ALL_EXTS: &[&str] = &["EXPR", "FEATURE_BUNDLE_1", "TASK_CHUNKING", "WRAP_ACTIONS"];
const NO_WRAP_EXTS: &[&str] = &["EXPR", "FEATURE_BUNDLE_1", "TASK_CHUNKING"];

fn yaml_val(s: &str) -> serde_json::Value {
    serde_saphyr::from_str(s).unwrap()
}

fn expect_job_err(template: &str, allowed_exts: &[&str], expected_substrings: &[&str]) {
    let err = decode_job_template(
        yaml_val(template),
        Some(allowed_exts),
        &CallerLimits::default(),
    )
    .expect_err("Expected validation error");
    let msg = err.to_string();
    for line in expected_substrings {
        assert!(
            msg.contains(line),
            "Missing expected substring {line:?} in error output:\n{msg}"
        );
    }
}

fn expect_env_err(template: &str, allowed_exts: &[&str], expected_substrings: &[&str]) {
    let err = decode_environment_template(yaml_val(template), Some(allowed_exts))
        .expect_err("Expected validation error");
    let msg = err.to_string();
    for line in expected_substrings {
        assert!(
            msg.contains(line),
            "Missing expected substring {line:?} in error output:\n{msg}"
        );
    }
}

fn expect_job_ok(template: &str, allowed_exts: &[&str]) {
    decode_job_template(
        yaml_val(template),
        Some(allowed_exts),
        &CallerLimits::default(),
    )
    .expect("expected successful decode");
}

fn expect_env_ok(template: &str, allowed_exts: &[&str]) {
    decode_environment_template(yaml_val(template), Some(allowed_exts))
        .expect("expected successful decode");
}

// ════════════════════════════════════════════════════════════════════
// Happy path — WRAP_ACTIONS enabled, new fields accepted
// ════════════════════════════════════════════════════════════════════

#[test]
fn wrap_hooks_accepted_with_extension() {
    // An environment template defining all three wrap hooks plus the
    // existing onEnter/onExit. WRAP_ACTIONS is declared.
    expect_env_ok(
        r#"{
            "specificationVersion": "environment-2023-09",
            "extensions": ["WRAP_ACTIONS", "EXPR"],
            "environment": {
                "name": "Wrapper",
                "script": {
                    "actions": {
                        "onEnter": {"command": "echo", "args": ["enter"]},
                        "onWrapEnter": {"command": "echo", "args": ["wrap-enter"]},
                        "onWrapTaskRun": {"command": "echo", "args": ["wrap-task"]},
                        "onWrapExit": {"command": "echo", "args": ["wrap-exit"]},
                        "onExit": {"command": "echo", "args": ["exit"]}
                    }
                }
            }
        }"#,
        ALL_EXTS,
    );
}

// ════════════════════════════════════════════════════════════════════
// Extension gating — fields rejected without WRAP_ACTIONS
// ════════════════════════════════════════════════════════════════════

#[test]
fn on_wrap_enter_rejected_without_extension() {
    expect_env_err(
        r#"{
            "specificationVersion": "environment-2023-09",
            "environment": {
                "name": "E",
                "script": {
                    "actions": {
                        "onEnter": {"command": "echo"},
                        "onWrapEnter": {"command": "echo"}
                    }
                }
            }
        }"#,
        NO_WRAP_EXTS,
        &[
            "environment -> script -> actions -> onWrapEnter:\n\tonWrapEnter requires the WRAP_ACTIONS extension.",
        ],
    );
}

#[test]
fn on_wrap_task_run_rejected_without_extension() {
    expect_env_err(
        r#"{
            "specificationVersion": "environment-2023-09",
            "environment": {
                "name": "E",
                "script": {
                    "actions": {
                        "onEnter": {"command": "echo"},
                        "onWrapTaskRun": {"command": "echo"}
                    }
                }
            }
        }"#,
        NO_WRAP_EXTS,
        &[
            "environment -> script -> actions -> onWrapTaskRun:\n\tonWrapTaskRun requires the WRAP_ACTIONS extension.",
        ],
    );
}

#[test]
fn on_wrap_exit_rejected_without_extension() {
    expect_env_err(
        r#"{
            "specificationVersion": "environment-2023-09",
            "environment": {
                "name": "E",
                "script": {
                    "actions": {
                        "onEnter": {"command": "echo"},
                        "onWrapExit": {"command": "echo"}
                    }
                }
            }
        }"#,
        NO_WRAP_EXTS,
        &[
            "environment -> script -> actions -> onWrapExit:\n\tonWrapExit requires the WRAP_ACTIONS extension.",
        ],
    );
}

// ════════════════════════════════════════════════════════════════════
// Single-wrap-layer rule
// ════════════════════════════════════════════════════════════════════

#[test]
fn two_job_envs_with_wrap_hooks_rejected() {
    // Two job environments both defining wrap hooks — invalid.
    expect_job_err(
        r#"{
            "specificationVersion": "jobtemplate-2023-09",
            "name": "Test",
            "extensions": ["WRAP_ACTIONS", "EXPR"],
            "jobEnvironments": [
                {"name": "A", "script": {"actions": {
                    "onEnter": {"command": "echo"},
                    "onWrapEnter": {"command": "echo"},
                    "onWrapTaskRun": {"command": "echo"},
                    "onWrapExit": {"command": "echo"}
                }}},
                {"name": "B", "script": {"actions": {
                    "onEnter": {"command": "echo"},
                    "onWrapEnter": {"command": "echo"},
                    "onWrapTaskRun": {"command": "echo"},
                    "onWrapExit": {"command": "echo"}
                }}}
            ],
            "steps": [{
                "name": "S",
                "script": {"actions": {"onRun": {"command": "echo"}}}
            }]
        }"#,
        ALL_EXTS,
        &[
            "only one environment in the session stack may define any of onWrapEnter, onWrapTaskRun, onWrapExit (RFC 0008).",
        ],
    );
}

#[test]
fn job_env_and_step_env_with_wrap_hooks_rejected() {
    // A wrap hook in a job env AND a wrap hook in a step env — invalid
    // for that step's session.
    expect_job_err(
        r#"{
            "specificationVersion": "jobtemplate-2023-09",
            "name": "Test",
            "extensions": ["WRAP_ACTIONS", "EXPR"],
            "jobEnvironments": [{
                "name": "Outer",
                "script": {"actions": {
                    "onEnter": {"command": "echo"},
                    "onWrapEnter": {"command": "echo"},
                    "onWrapTaskRun": {"command": "echo"},
                    "onWrapExit": {"command": "echo"}
                }}
            }],
            "steps": [{
                "name": "S",
                "stepEnvironments": [{
                    "name": "Inner",
                    "script": {"actions": {
                        "onEnter": {"command": "echo"},
                        "onWrapEnter": {"command": "echo"},
                        "onWrapTaskRun": {"command": "echo"},
                        "onWrapExit": {"command": "echo"}
                    }}
                }],
                "script": {"actions": {"onRun": {"command": "echo"}}}
            }]
        }"#,
        ALL_EXTS,
        &[
            "steps[0] -> stepEnvironments:\n\tonly one environment in the session stack may define any of onWrapEnter, onWrapTaskRun, onWrapExit (RFC 0008).",
        ],
    );
}

#[test]
fn single_wrap_layer_in_job_env_ok() {
    // Single wrap layer in a job env, step env without wrap hooks — valid.
    expect_job_ok(
        r#"{
            "specificationVersion": "jobtemplate-2023-09",
            "name": "Test",
            "extensions": ["WRAP_ACTIONS", "EXPR"],
            "jobEnvironments": [{
                "name": "Outer",
                "script": {"actions": {
                    "onEnter": {"command": "echo"},
                    "onWrapEnter": {"command": "echo"},
                    "onWrapTaskRun": {"command": "echo"},
                    "onWrapExit": {"command": "echo"},
                    "onExit": {"command": "echo"}
                }}
            }],
            "steps": [{
                "name": "S",
                "stepEnvironments": [{
                    "name": "Inner",
                    "script": {"actions": {
                        "onEnter": {"command": "echo"},
                        "onExit": {"command": "echo"}
                    }}
                }],
                "script": {"actions": {"onRun": {"command": "echo"}}}
            }]
        }"#,
        ALL_EXTS,
    );
}

#[test]
fn single_wrap_layer_in_step_env_ok() {
    // Single wrap layer in a step env with no wrap hooks elsewhere — valid.
    expect_job_ok(
        r#"{
            "specificationVersion": "jobtemplate-2023-09",
            "name": "Test",
            "extensions": ["WRAP_ACTIONS", "EXPR"],
            "steps": [{
                "name": "S",
                "stepEnvironments": [{
                    "name": "Wrapper",
                    "script": {"actions": {
                        "onEnter": {"command": "echo"},
                        "onWrapEnter": {"command": "echo"},
                        "onWrapTaskRun": {"command": "echo"},
                        "onWrapExit": {"command": "echo"},
                        "onExit": {"command": "echo"}
                    }}
                }],
                "script": {"actions": {"onRun": {"command": "echo"}}}
            }]
        }"#,
        ALL_EXTS,
    );
}

// ════════════════════════════════════════════════════════════════════
// Wrap hooks + plain inner step environments happy path
// ════════════════════════════════════════════════════════════════════

#[test]
fn wrap_hooks_with_plain_inner_step_envs_ok() {
    // Canonical scenario: one wrapping queue env and one plain step env
    // (which the wrap hooks intercept) plus a task.
    expect_job_ok(
        r#"{
            "specificationVersion": "jobtemplate-2023-09",
            "name": "Render",
            "extensions": ["WRAP_ACTIONS", "EXPR"],
            "jobEnvironments": [{
                "name": "Docker",
                "script": {"actions": {
                    "onEnter": {"command": "echo", "args": ["start-container"]},
                    "onWrapEnter": {"command": "echo", "args": ["wrap-enter"]},
                    "onWrapTaskRun": {"command": "echo", "args": ["wrap-task"]},
                    "onWrapExit": {"command": "echo", "args": ["wrap-exit"]},
                    "onExit": {"command": "echo", "args": ["stop-container"]}
                }}
            }],
            "steps": [{
                "name": "Render",
                "stepEnvironments": [
                    {
                        "name": "BlenderSetup",
                        "script": {"actions": {
                            "onEnter": {"command": "pip", "args": ["install", "blender"]},
                            "onExit": {"command": "pip", "args": ["uninstall", "-y", "blender"]}
                        }}
                    }
                ],
                "script": {"actions": {"onRun": {"command": "blender"}}}
            }]
        }"#,
        ALL_EXTS,
    );
}

// ════════════════════════════════════════════════════════════════════
// Template-variable registration (RFC 0008)
//
// These tests exercise the per-hook symbol scopes that
// `validate_env_format_strings` sets up:
// - `onWrapTaskRun` sees `Task.Action.{Command,Args,Environment,Timeout}`
// - `onWrapEnter` / `onWrapExit` see `Env.Wrapped.Name` and
//   `Env.Wrapped.Action.{Command,Args,Environment,Timeout}`
// - Cross-scope references (Task.Action.* in onWrapEnter, Env.Wrapped.* in
//   onWrapTaskRun, Env.Wrapped.* in onEnter) must be rejected with a
//   clear "Undefined variable" error.
// ════════════════════════════════════════════════════════════════════

#[test]
fn wrap_task_run_can_reference_task_scope_symbols() {
    expect_env_ok(
        r#"{
            "specificationVersion": "environment-2023-09",
            "extensions": ["WRAP_ACTIONS", "EXPR"],
            "environment": {
                "name": "Wrapper",
                "script": {
                    "actions": {
                        "onEnter": {"command": "echo"},
                        "onWrapEnter": {"command": "echo"},
                        "onWrapTaskRun": {
                            "command": "bash",
                            "args": [
                                "-c",
                                "echo cmd={{Task.Action.Command}} args={{ repr_sh(Task.Action.Args) }} env={{ repr_sh(Task.Action.Environment) }} t={{Task.Action.Timeout}}"
                            ]
                        },
                        "onWrapExit": {"command": "echo"}
                    }
                }
            }
        }"#,
        ALL_EXTS,
    );
}

#[test]
fn wrap_enter_can_reference_env_wrapped_scope_symbols() {
    expect_env_ok(
        r#"{
            "specificationVersion": "environment-2023-09",
            "extensions": ["WRAP_ACTIONS", "EXPR"],
            "environment": {
                "name": "Wrapper",
                "script": {
                    "actions": {
                        "onEnter": {"command": "echo"},
                        "onWrapEnter": {
                            "command": "bash",
                            "args": [
                                "-c",
                                "echo name={{Env.Wrapped.Name}} cmd={{Env.Wrapped.Action.Command}} args={{ repr_sh(Env.Wrapped.Action.Args) }} t={{Env.Wrapped.Action.Timeout}}"
                            ]
                        },
                        "onWrapTaskRun": {"command": "echo"},
                        "onWrapExit": {"command": "echo"}
                    }
                }
            }
        }"#,
        ALL_EXTS,
    );
}

#[test]
fn wrap_exit_can_reference_env_wrapped_scope_symbols() {
    expect_env_ok(
        r#"{
            "specificationVersion": "environment-2023-09",
            "extensions": ["WRAP_ACTIONS", "EXPR"],
            "environment": {
                "name": "Wrapper",
                "script": {
                    "actions": {
                        "onEnter": {"command": "echo"},
                        "onWrapEnter": {"command": "echo"},
                        "onWrapTaskRun": {"command": "echo"},
                        "onWrapExit": {
                            "command": "bash",
                            "args": [
                                "-c",
                                "echo name={{Env.Wrapped.Name}} env={{ repr_sh(Env.Wrapped.Action.Environment) }}"
                            ]
                        }
                    }
                }
            }
        }"#,
        ALL_EXTS,
    );
}

#[test]
fn task_scope_not_available_in_wrap_enter() {
    // `Task.Action.Command` is specific to onWrapTaskRun; referencing it in
    // onWrapEnter surfaces as a plain undefined-variable error.
    // Embedded in a job template so the format-string validation pass runs.
    expect_job_err(
        r#"{
            "specificationVersion": "jobtemplate-2023-09",
            "name": "Test",
            "extensions": ["WRAP_ACTIONS", "EXPR"],
            "jobEnvironments": [{
                "name": "Wrapper",
                "script": {
                    "actions": {
                        "onEnter": {"command": "echo"},
                        "onWrapEnter": {
                            "command": "bash",
                            "args": ["-c", "echo {{Task.Action.Command}}"]
                        }
                    }
                }
            }],
            "steps": [{"name": "S", "script": {"actions": {"onRun": {"command": "echo"}}}}]
        }"#,
        ALL_EXTS,
        &["Undefined variable: 'Task.Action.Command'"],
    );
}

#[test]
fn env_wrapped_scope_not_available_in_wrap_task_run() {
    // `Env.Wrapped.Name` is specific to onWrapEnter/onWrapExit.
    expect_job_err(
        r#"{
            "specificationVersion": "jobtemplate-2023-09",
            "name": "Test",
            "extensions": ["WRAP_ACTIONS", "EXPR"],
            "jobEnvironments": [{
                "name": "Wrapper",
                "script": {
                    "actions": {
                        "onEnter": {"command": "echo"},
                        "onWrapTaskRun": {
                            "command": "bash",
                            "args": ["-c", "echo {{Env.Wrapped.Name}}"]
                        }
                    }
                }
            }],
            "steps": [{"name": "S", "script": {"actions": {"onRun": {"command": "echo"}}}}]
        }"#,
        ALL_EXTS,
        &["Undefined variable: 'Env.Wrapped.Name'"],
    );
}

#[test]
fn task_scope_not_available_in_plain_on_enter() {
    // `Task.Action.Command` must not leak into the plain onEnter scope just
    // because the environment also defines onWrapTaskRun.
    expect_job_err(
        r#"{
            "specificationVersion": "jobtemplate-2023-09",
            "name": "Test",
            "extensions": ["WRAP_ACTIONS", "EXPR"],
            "jobEnvironments": [{
                "name": "Wrapper",
                "script": {
                    "actions": {
                        "onEnter": {
                            "command": "bash",
                            "args": ["-c", "echo {{Task.Action.Command}}"]
                        },
                        "onWrapTaskRun": {"command": "echo"}
                    }
                }
            }],
            "steps": [{"name": "S", "script": {"actions": {"onRun": {"command": "echo"}}}}]
        }"#,
        ALL_EXTS,
        &["Undefined variable: 'Task.Action.Command'"],
    );
}

#[test]
fn env_wrapped_scope_not_available_in_plain_on_exit() {
    // Symmetric check: onExit does not see Env.Wrapped.*.
    expect_job_err(
        r#"{
            "specificationVersion": "jobtemplate-2023-09",
            "name": "Test",
            "extensions": ["WRAP_ACTIONS", "EXPR"],
            "jobEnvironments": [{
                "name": "Wrapper",
                "script": {
                    "actions": {
                        "onEnter": {"command": "echo"},
                        "onExit": {
                            "command": "bash",
                            "args": ["-c", "echo {{Env.Wrapped.Name}}"]
                        },
                        "onWrapExit": {"command": "echo"}
                    }
                }
            }],
            "steps": [{"name": "S", "script": {"actions": {"onRun": {"command": "echo"}}}}]
        }"#,
        ALL_EXTS,
        &["Undefined variable: 'Env.Wrapped.Name'"],
    );
}
