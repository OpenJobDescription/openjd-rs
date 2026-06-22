// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Integration tests for RFC 0008 `WRAP_ACTIONS` extension.
//!
//! These tests cover the three layers of validation added for RFC 0008:
//!
//! 1. **Schema parse**: `onWrapEnvEnter`, `onWrapTaskRun`, `onWrapEnvExit` on
//!    `<EnvironmentActions>` decode from YAML without error.
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

// These tests only exercise WRAP_ACTIONS and its EXPR prerequisite, so the
// extension lists are scoped to exactly those. `WRAP_EXTS` enables wrap hooks;
// `NO_WRAP_EXTS` omits WRAP_ACTIONS to exercise the gating/rejection paths.
const WRAP_EXTS: &[&str] = &["EXPR", "WRAP_ACTIONS"];
const NO_WRAP_EXTS: &[&str] = &["EXPR"];

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
                        "onWrapEnvEnter": {"command": "echo", "args": ["wrap-enter"]},
                        "onWrapTaskRun": {"command": "echo", "args": ["wrap-task"]},
                        "onWrapEnvExit": {"command": "echo", "args": ["wrap-exit"]},
                        "onExit": {"command": "echo", "args": ["exit"]}
                    }
                }
            }
        }"#,
        WRAP_EXTS,
    );
}

// ════════════════════════════════════════════════════════════════════
// Extension gating — fields rejected without WRAP_ACTIONS
// ════════════════════════════════════════════════════════════════════

#[test]
fn on_wrap_env_enter_rejected_without_extension() {
    expect_env_err(
        r#"{
            "specificationVersion": "environment-2023-09",
            "environment": {
                "name": "E",
                "script": {
                    "actions": {
                        "onEnter": {"command": "echo"},
                        "onWrapEnvEnter": {"command": "echo"}
                    }
                }
            }
        }"#,
        NO_WRAP_EXTS,
        &[
            "environment -> script -> actions -> onWrapEnvEnter:\n\tonWrapEnvEnter requires the WRAP_ACTIONS extension.",
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
fn on_wrap_env_exit_rejected_without_extension() {
    expect_env_err(
        r#"{
            "specificationVersion": "environment-2023-09",
            "environment": {
                "name": "E",
                "script": {
                    "actions": {
                        "onEnter": {"command": "echo"},
                        "onWrapEnvExit": {"command": "echo"}
                    }
                }
            }
        }"#,
        NO_WRAP_EXTS,
        &[
            "environment -> script -> actions -> onWrapEnvExit:\n\tonWrapEnvExit requires the WRAP_ACTIONS extension.",
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
                    "onWrapEnvEnter": {"command": "echo"},
                    "onWrapTaskRun": {"command": "echo"},
                    "onWrapEnvExit": {"command": "echo"}
                }}},
                {"name": "B", "script": {"actions": {
                    "onEnter": {"command": "echo"},
                    "onWrapEnvEnter": {"command": "echo"},
                    "onWrapTaskRun": {"command": "echo"},
                    "onWrapEnvExit": {"command": "echo"}
                }}}
            ],
            "steps": [{
                "name": "S",
                "script": {"actions": {"onRun": {"command": "echo"}}}
            }]
        }"#,
        WRAP_EXTS,
        &[
            "only one environment in the session stack may define any of onWrapEnvEnter, onWrapTaskRun, onWrapEnvExit (RFC 0008).",
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
                    "onWrapEnvEnter": {"command": "echo"},
                    "onWrapTaskRun": {"command": "echo"},
                    "onWrapEnvExit": {"command": "echo"}
                }}
            }],
            "steps": [{
                "name": "S",
                "stepEnvironments": [{
                    "name": "Inner",
                    "script": {"actions": {
                        "onEnter": {"command": "echo"},
                        "onWrapEnvEnter": {"command": "echo"},
                        "onWrapTaskRun": {"command": "echo"},
                        "onWrapEnvExit": {"command": "echo"}
                    }}
                }],
                "script": {"actions": {"onRun": {"command": "echo"}}}
            }]
        }"#,
        WRAP_EXTS,
        &[
            "steps[0] -> stepEnvironments:\n\tonly one environment in the session stack may define any of onWrapEnvEnter, onWrapTaskRun, onWrapEnvExit (RFC 0008).",
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
                    "onWrapEnvEnter": {"command": "echo"},
                    "onWrapTaskRun": {"command": "echo"},
                    "onWrapEnvExit": {"command": "echo"},
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
        WRAP_EXTS,
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
                        "onWrapEnvEnter": {"command": "echo"},
                        "onWrapTaskRun": {"command": "echo"},
                        "onWrapEnvExit": {"command": "echo"},
                        "onExit": {"command": "echo"}
                    }}
                }],
                "script": {"actions": {"onRun": {"command": "echo"}}}
            }]
        }"#,
        WRAP_EXTS,
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
                    "onWrapEnvEnter": {"command": "echo", "args": ["wrap-enter"]},
                    "onWrapTaskRun": {"command": "echo", "args": ["wrap-task"]},
                    "onWrapEnvExit": {"command": "echo", "args": ["wrap-exit"]},
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
        WRAP_EXTS,
    );
}

// ════════════════════════════════════════════════════════════════════
// Template-variable registration (RFC 0008)
//
// These tests exercise the per-hook symbol scopes that
// `validate_env_format_strings` sets up:
// - All three wrap hooks see `WrappedAction.{Command,Args,Environment,Timeout}`
//   with identical shape.
// - `onWrapEnvEnter` / `onWrapEnvExit` additionally see `WrappedEnv.Name`.
// - `onWrapTaskRun` additionally sees `WrappedStep.Name`.
// - References to `WrappedAction.*` outside any wrap hook,
//   `WrappedEnv.Name` outside `onWrapEnvEnter`/`onWrapEnvExit`, or
//   `WrappedStep.Name` outside `onWrapTaskRun`, must be
//   rejected with a clear "Undefined variable" error.
// ════════════════════════════════════════════════════════════════════

#[test]
fn wrap_task_run_can_reference_wrapped_action_symbols() {
    expect_env_ok(
        r#"{
            "specificationVersion": "environment-2023-09",
            "extensions": ["WRAP_ACTIONS", "EXPR"],
            "environment": {
                "name": "Wrapper",
                "script": {
                    "actions": {
                        "onEnter": {"command": "echo"},
                        "onWrapEnvEnter": {"command": "echo"},
                        "onWrapTaskRun": {
                            "command": "bash",
                            "args": [
                                "-c",
                                "echo cmd={{WrappedAction.Command}} args={{ repr_sh(WrappedAction.Args) }} env={{ repr_sh(WrappedAction.Environment) }} t={{WrappedAction.Timeout}}"
                            ]
                        },
                        "onWrapEnvExit": {"command": "echo"}
                    }
                }
            }
        }"#,
        WRAP_EXTS,
    );
}

#[test]
fn wrap_env_enter_can_reference_wrapped_action_and_wrapped_env_name() {
    expect_env_ok(
        r#"{
            "specificationVersion": "environment-2023-09",
            "extensions": ["WRAP_ACTIONS", "EXPR"],
            "environment": {
                "name": "Wrapper",
                "script": {
                    "actions": {
                        "onEnter": {"command": "echo"},
                        "onWrapEnvEnter": {
                            "command": "bash",
                            "args": [
                                "-c",
                                "echo name={{WrappedEnv.Name}} cmd={{WrappedAction.Command}} args={{ repr_sh(WrappedAction.Args) }} t={{WrappedAction.Timeout}}"
                            ]
                        },
                        "onWrapTaskRun": {"command": "echo"},
                        "onWrapEnvExit": {"command": "echo"}
                    }
                }
            }
        }"#,
        WRAP_EXTS,
    );
}

#[test]
fn wrap_env_exit_can_reference_wrapped_action_and_wrapped_env_name() {
    expect_env_ok(
        r#"{
            "specificationVersion": "environment-2023-09",
            "extensions": ["WRAP_ACTIONS", "EXPR"],
            "environment": {
                "name": "Wrapper",
                "script": {
                    "actions": {
                        "onEnter": {"command": "echo"},
                        "onWrapEnvEnter": {"command": "echo"},
                        "onWrapTaskRun": {"command": "echo"},
                        "onWrapEnvExit": {
                            "command": "bash",
                            "args": [
                                "-c",
                                "echo name={{WrappedEnv.Name}} env={{ repr_sh(WrappedAction.Environment) }}"
                            ]
                        }
                    }
                }
            }
        }"#,
        WRAP_EXTS,
    );
}

#[test]
fn wrap_task_run_can_reference_wrapped_step_name() {
    expect_env_ok(
        r#"{
            "specificationVersion": "environment-2023-09",
            "extensions": ["WRAP_ACTIONS", "EXPR"],
            "environment": {
                "name": "Wrapper",
                "script": {
                    "actions": {
                        "onEnter": {"command": "echo"},
                        "onWrapEnvEnter": {"command": "echo"},
                        "onWrapTaskRun": {
                            "command": "bash",
                            "args": [
                                "-c",
                                "echo step={{WrappedStep.Name}} cmd={{WrappedAction.Command}}"
                            ]
                        },
                        "onWrapEnvExit": {"command": "echo"}
                    }
                }
            }
        }"#,
        WRAP_EXTS,
    );
}

#[test]
fn wrapped_step_name_not_available_in_wrap_env_enter() {
    // `WrappedStep.Name` is specific to onWrapTaskRun.
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
                        "onWrapEnvEnter": {
                            "command": "bash",
                            "args": ["-c", "echo {{WrappedStep.Name}}"]
                        },
                        "onWrapTaskRun": {"command": "echo"},
                        "onWrapEnvExit": {"command": "echo"}
                    }
                }
            }],
            "steps": [{"name": "S", "script": {"actions": {"onRun": {"command": "echo"}}}}]
        }"#,
        WRAP_EXTS,
        &["Undefined variable: 'WrappedStep.Name'"],
    );
}

#[test]
fn wrapped_step_name_not_available_in_wrap_env_exit() {
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
                        "onWrapEnvEnter": {"command": "echo"},
                        "onWrapTaskRun": {"command": "echo"},
                        "onWrapEnvExit": {
                            "command": "bash",
                            "args": ["-c", "echo {{WrappedStep.Name}}"]
                        }
                    }
                }
            }],
            "steps": [{"name": "S", "script": {"actions": {"onRun": {"command": "echo"}}}}]
        }"#,
        WRAP_EXTS,
        &["Undefined variable: 'WrappedStep.Name'"],
    );
}

#[test]
fn wrapped_env_name_not_available_in_wrap_task_run() {
    // `WrappedEnv.Name` is specific to onWrapEnvEnter/onWrapEnvExit; the
    // task-run hook sees `WrappedStep.Name` instead.
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
                        "onWrapEnvEnter": {"command": "echo"},
                        "onWrapTaskRun": {
                            "command": "bash",
                            "args": ["-c", "echo {{WrappedEnv.Name}}"]
                        },
                        "onWrapEnvExit": {"command": "echo"}
                    }
                }
            }],
            "steps": [{"name": "S", "script": {"actions": {"onRun": {"command": "echo"}}}}]
        }"#,
        WRAP_EXTS,
        &["Undefined variable: 'WrappedEnv.Name'"],
    );
}

#[test]
fn wrapped_action_not_available_in_plain_on_enter() {
    // `WrappedAction.Command` must not leak into the plain onEnter scope
    // just because the environment also defines wrap hooks.
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
                            "args": ["-c", "echo {{WrappedAction.Command}}"]
                        },
                        "onWrapEnvEnter": {"command": "echo"},
                        "onWrapTaskRun": {"command": "echo"},
                        "onWrapEnvExit": {"command": "echo"}
                    }
                }
            }],
            "steps": [{"name": "S", "script": {"actions": {"onRun": {"command": "echo"}}}}]
        }"#,
        WRAP_EXTS,
        &["Undefined variable: 'WrappedAction.Command'"],
    );
}

#[test]
fn wrapped_env_name_not_available_in_plain_on_exit() {
    // Symmetric check: onExit does not see WrappedEnv.Name.
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
                            "args": ["-c", "echo {{WrappedEnv.Name}}"]
                        },
                        "onWrapEnvEnter": {"command": "echo"},
                        "onWrapTaskRun": {"command": "echo"},
                        "onWrapEnvExit": {"command": "echo"}
                    }
                }
            }],
            "steps": [{"name": "S", "script": {"actions": {"onRun": {"command": "echo"}}}}]
        }"#,
        WRAP_EXTS,
        &["Undefined variable: 'WrappedEnv.Name'"],
    );
}

// ════════════════════════════════════════════════════════════════════
// All-or-nothing rule (RFC 0008)
//
// An environment that defines any wrap hook must define all three.
// ════════════════════════════════════════════════════════════════════

#[test]
fn defining_only_on_wrap_task_run_rejected() {
    expect_env_err(
        r#"{
            "specificationVersion": "environment-2023-09",
            "extensions": ["WRAP_ACTIONS", "EXPR"],
            "environment": {
                "name": "Partial",
                "script": {
                    "actions": {
                        "onWrapTaskRun": {"command": "echo"}
                    }
                }
            }
        }"#,
        WRAP_EXTS,
        &["must define all three"],
    );
}

#[test]
fn defining_only_on_wrap_env_enter_rejected() {
    expect_env_err(
        r#"{
            "specificationVersion": "environment-2023-09",
            "extensions": ["WRAP_ACTIONS", "EXPR"],
            "environment": {
                "name": "Partial",
                "script": {
                    "actions": {
                        "onEnter": {"command": "echo"},
                        "onWrapEnvEnter": {"command": "echo"}
                    }
                }
            }
        }"#,
        WRAP_EXTS,
        &["must define all three"],
    );
}

#[test]
fn defining_two_of_three_wrap_hooks_rejected() {
    expect_env_err(
        r#"{
            "specificationVersion": "environment-2023-09",
            "extensions": ["WRAP_ACTIONS", "EXPR"],
            "environment": {
                "name": "Partial",
                "script": {
                    "actions": {
                        "onWrapEnvEnter": {"command": "echo"},
                        "onWrapTaskRun": {"command": "echo"}
                    }
                }
            }
        }"#,
        WRAP_EXTS,
        &["must define all three"],
    );
}

#[test]
fn defining_zero_wrap_hooks_accepted() {
    // The rule is "any → all three"; zero wrap hooks is valid.
    expect_env_ok(
        r#"{
            "specificationVersion": "environment-2023-09",
            "extensions": ["WRAP_ACTIONS", "EXPR"],
            "environment": {
                "name": "Plain",
                "script": {
                    "actions": {
                        "onEnter": {"command": "echo"},
                        "onExit": {"command": "echo"}
                    }
                }
            }
        }"#,
        WRAP_EXTS,
    );
}

// ════════════════════════════════════════════════════════════════════
// EXPR prerequisite (RFC 0008)
//
// A template that lists WRAP_ACTIONS must also list EXPR.
// ════════════════════════════════════════════════════════════════════

#[test]
fn wrap_actions_without_expr_extension_rejected_in_env_template() {
    expect_env_err(
        r#"{
            "specificationVersion": "environment-2023-09",
            "extensions": ["WRAP_ACTIONS"],
            "environment": {
                "name": "Wrapper",
                "script": {
                    "actions": {
                        "onWrapEnvEnter": {"command": "echo"},
                        "onWrapTaskRun": {"command": "echo"},
                        "onWrapEnvExit": {"command": "echo"}
                    }
                }
            }
        }"#,
        WRAP_EXTS,
        &["WRAP_ACTIONS requires EXPR"],
    );
}

#[test]
fn wrap_actions_without_expr_extension_rejected_in_job_template() {
    expect_job_err(
        r#"{
            "specificationVersion": "jobtemplate-2023-09",
            "name": "Test",
            "extensions": ["WRAP_ACTIONS"],
            "jobEnvironments": [{
                "name": "Wrapper",
                "script": {
                    "actions": {
                        "onEnter": {"command": "echo"},
                        "onWrapEnvEnter": {"command": "echo"},
                        "onWrapTaskRun": {"command": "echo"},
                        "onWrapEnvExit": {"command": "echo"}
                    }
                }
            }],
            "steps": [{"name": "S", "script": {"actions": {"onRun": {"command": "echo"}}}}]
        }"#,
        WRAP_EXTS,
        &["WRAP_ACTIONS requires EXPR"],
    );
}
