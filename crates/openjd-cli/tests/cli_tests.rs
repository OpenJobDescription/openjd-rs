// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Port of Python openjd-cli unit tests to Rust integration tests.
//! Tests the openjd-rs binary via `Command` invocations.

use std::path::PathBuf;
use std::process::Command;

fn openjd_bin() -> PathBuf {
    // Use cargo to find the binary
    let mut path = PathBuf::from(env!("CARGO_BIN_EXE_openjd-rs"));
    if !path.exists() {
        // Fallback
        path = PathBuf::from("target/debug/openjd-rs");
    }
    path
}

fn templates_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/templates")
}

fn run_cli(args: &[&str]) -> (i32, String, String) {
    let output = Command::new(openjd_bin())
        .args(args)
        .env("RUSTUP_TOOLCHAIN", "1.94.1")
        .output()
        .expect("failed to execute openjd-rs");
    let exit_code = output.status.code().unwrap_or(-1);
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    (exit_code, stdout, stderr)
}

// ============================================================
// Group 1: CLI Entrypoint / Argument Parsing (test_main.py)
// ============================================================

mod cli_entrypoint {
    use super::*;

    #[test]
    fn test_cli_check_success() {
        let template = templates_dir().join("basic.yaml");
        let (code, stdout, _stderr) = run_cli(&["check", template.to_str().unwrap()]);
        assert_eq!(code, 0, "check should succeed");
        assert!(
            stdout.contains("passes validation checks"),
            "stdout: {stdout}"
        );
    }

    #[test]
    fn test_cli_run_success_base() {
        let tdir = templates_dir();
        let (code, stdout, stderr) = run_cli(&[
            "run",
            tdir.join("job_with_test_steps.yaml").to_str().unwrap(),
            "--step",
            "BareStep",
            "--extensions",
            "",
        ]);
        assert_eq!(code, 0, "base run should succeed. stderr: {stderr}");
        assert!(
            stdout.contains("All actions completed successfully!"),
            "stdout: {stdout}"
        );
    }

    #[test]
    fn test_cli_run_success_with_params() {
        let tdir = templates_dir();
        let (code, _stdout, stderr) = run_cli(&[
            "run",
            tdir.join("basic.yaml").to_str().unwrap(),
            "--step",
            "First",
            "-p",
            "J=value1",
            "--extensions",
            "",
        ]);
        assert_eq!(code, 0, "run with params should succeed. stderr: {stderr}");
    }

    #[test]
    fn test_cli_run_success_with_multiple_params() {
        let tdir = templates_dir();
        let (code, _stdout, stderr) = run_cli(&[
            "run",
            tdir.join("basic.yaml").to_str().unwrap(),
            "--step",
            "First",
            "-p",
            "J=value1",
            "--run-dependencies",
            "--extensions",
            "",
        ]);
        assert_eq!(
            code, 0,
            "run with multiple options should succeed. stderr: {stderr}"
        );
    }

    #[test]
    fn test_cli_argument_errors_no_command() {
        let (code, _stdout, _stderr) = run_cli(&[]);
        assert_ne!(code, 0, "no command should fail");
    }

    #[test]
    fn test_cli_argument_errors_nonexistent_command() {
        let (code, _stdout, _stderr) = run_cli(&["notarealcommand"]);
        assert_ne!(code, 0, "nonexistent command should fail");
    }

    #[test]
    fn test_cli_argument_errors_check_no_args() {
        let (code, _stdout, _stderr) = run_cli(&["check"]);
        assert_ne!(code, 0, "check with no args should fail");
    }

    #[test]
    fn test_cli_argument_errors_run_no_step_arg_value() {
        let tdir = templates_dir();
        let (code, _stdout, _stderr) =
            run_cli(&["run", tdir.join("basic.yaml").to_str().unwrap(), "--step"]);
        assert_ne!(code, 0, "missing step value should fail");
    }
}

// ============================================================
// Group 2: Check Command (test_check_command.py)
// ============================================================

mod check_command {
    use super::*;
    use std::io::Write;
    use tempfile::{NamedTempFile, TempDir};

    #[test]
    fn test_do_check_file_success_json() {
        let mut f = NamedTempFile::with_suffix(".template.json").unwrap();
        write!(
            f,
            r#"{{
            "specificationVersion": "jobtemplate-2023-09",
            "name": "test",
            "steps": [{{"name": "s1", "script": {{"actions": {{"onRun": {{"command": "echo"}}}}}}}}]
        }}"#
        )
        .unwrap();
        let (code, stdout, stderr) = run_cli(&["check", f.path().to_str().unwrap()]);
        assert_eq!(code, 0, "JSON check should succeed. stderr: {stderr}");
        assert!(
            stdout.contains("passes validation checks"),
            "stdout: {stdout}"
        );
    }

    #[test]
    fn test_do_check_file_success_yaml() {
        let mut f = NamedTempFile::with_suffix(".template.yaml").unwrap();
        write!(
            f,
            r#"specificationVersion: "jobtemplate-2023-09"
name: test
steps:
  - name: s1
    script:
      actions:
        onRun:
          command: echo
"#
        )
        .unwrap();
        let (code, stdout, stderr) = run_cli(&["check", f.path().to_str().unwrap()]);
        assert_eq!(code, 0, "YAML check should succeed. stderr: {stderr}");
        assert!(
            stdout.contains("passes validation checks"),
            "stdout: {stdout}"
        );
    }

    #[test]
    fn test_do_check_file_error_nonexistent() {
        let (code, _stdout, stderr) = run_cli(&["check", "error-file.json"]);
        assert_ne!(code, 0, "nonexistent file should fail");
        assert!(stderr.contains("does not exist"), "stderr: {stderr}");
    }

    #[test]
    fn test_do_check_bundle_error_directory() {
        let dir = TempDir::new().unwrap();
        let (code, _stdout, stderr) = run_cli(&["check", dir.path().to_str().unwrap()]);
        assert_ne!(code, 0, "directory should fail");
        assert!(stderr.contains("not a file"), "stderr: {stderr}");
    }

    #[test]
    fn test_do_check_file_success_ojdt() {
        let mut f = NamedTempFile::with_suffix(".ojdt").unwrap();
        write!(
            f,
            r#"specificationVersion: "jobtemplate-2023-09"
name: test
steps:
  - name: s1
    script:
      actions:
        onRun:
          command: echo
"#
        )
        .unwrap();
        let (code, stdout, stderr) = run_cli(&["check", f.path().to_str().unwrap()]);
        assert_eq!(
            code, 0,
            ".ojdt check should succeed (parsed as YAML). stderr: {stderr}"
        );
        assert!(
            stdout.contains("passes validation checks"),
            "stdout: {stdout}"
        );
    }

    #[test]
    fn test_do_run_file_success_ojdt() {
        let mut f = NamedTempFile::with_suffix(".ojdt").unwrap();
        write!(
            f,
            r#"specificationVersion: "jobtemplate-2023-09"
name: test
steps:
  - name: s1
    script:
      actions:
        onRun:
          command: echo
          args: ["ojdt works"]
"#
        )
        .unwrap();
        let (code, stdout, stderr) = run_cli(&["run", f.path().to_str().unwrap()]);
        assert_eq!(code, 0, ".ojdt run should succeed. stderr: {stderr}");
        assert!(stdout.contains("ojdt works"), "stdout: {stdout}");
    }

    #[test]
    fn test_do_summary_file_success_ojdt() {
        let mut f = NamedTempFile::with_suffix(".ojdt").unwrap();
        write!(
            f,
            r#"specificationVersion: "jobtemplate-2023-09"
name: OjdtJob
steps:
  - name: s1
    script:
      actions:
        onRun:
          command: echo
"#
        )
        .unwrap();
        let (code, stdout, stderr) = run_cli(&["summary", f.path().to_str().unwrap()]);
        assert_eq!(code, 0, ".ojdt summary should succeed. stderr: {stderr}");
        assert!(stdout.contains("OjdtJob"), "stdout: {stdout}");
    }
}

// ============================================================
// Group 3: Common Utilities — Parameter Parsing (test_common.py)
// ============================================================

mod common_params {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    // Test parameter parsing via the run command (since Rust doesn't expose internal functions directly)
    // We test that parameters are correctly parsed by running a template that echoes them.

    #[test]
    fn test_params_from_key_value_pair() {
        let template = templates_dir().join("simple_with_j_param.yaml");
        let (code, stdout, stderr) =
            run_cli(&["run", template.to_str().unwrap(), "-p", "J=TestValue"]);
        assert_eq!(code, 0, "should succeed. stderr: {stderr}");
        assert!(stdout.contains("DoTask TestValue"), "stdout: {stdout}");
    }

    #[test]
    fn test_params_from_file() {
        let mut f = NamedTempFile::with_suffix(".json").unwrap();
        write!(f, r#"{{"J": "FromFile"}}"#).unwrap();
        let file_arg = format!("file://{}", f.path().display());
        let template = templates_dir().join("simple_with_j_param.yaml");
        let (code, stdout, stderr) = run_cli(&["run", template.to_str().unwrap(), "-p", &file_arg]);
        assert_eq!(code, 0, "should succeed. stderr: {stderr}");
        assert!(stdout.contains("DoTask FromFile"), "stdout: {stdout}");
    }

    #[test]
    fn test_params_value_with_equals() {
        // Create a template that accepts a STRING param and echoes it
        let mut f = NamedTempFile::with_suffix(".yaml").unwrap();
        write!(
            f,
            r#"specificationVersion: "jobtemplate-2023-09"
name: test
parameterDefinitions:
  - name: MyParam
    type: STRING
steps:
  - name: s1
    script:
      actions:
        onRun:
          command: echo
          args: ["{{{{Param.MyParam}}}}"]
"#
        )
        .unwrap();
        let (code, stdout, stderr) =
            run_cli(&["run", f.path().to_str().unwrap(), "-p", "MyParam=One=Two"]);
        assert_eq!(code, 0, "should succeed. stderr: {stderr}");
        assert!(stdout.contains("One=Two"), "stdout: {stdout}");
    }

    #[test]
    fn test_run_error_nonexistent_template() {
        let (code, _stdout, stderr) = run_cli(&["run", "some-file.json"]);
        assert_ne!(code, 0, "nonexistent file should fail");
        assert!(stderr.contains("does not exist"), "stderr: {stderr}");
    }

    #[test]
    fn test_params_yaml_file() {
        let mut f = NamedTempFile::with_suffix(".yaml").unwrap();
        writeln!(f, "J: YamlValue").unwrap();
        let file_arg = format!("file://{}", f.path().display());
        let template = templates_dir().join("simple_with_j_param.yaml");
        let (code, stdout, stderr) = run_cli(&["run", template.to_str().unwrap(), "-p", &file_arg]);
        assert_eq!(code, 0, "should succeed. stderr: {stderr}");
        assert!(stdout.contains("YamlValue"), "stdout: {stdout}");
    }

    #[test]
    fn test_params_combination_kvp_and_file() {
        let mut f = NamedTempFile::with_suffix(".json").unwrap();
        write!(f, r#"{{"J": "FromFile"}}"#).unwrap();
        let file_arg = format!("file://{}", f.path().display());
        // Use a template with two params: J and an extra one
        let mut tmpl = NamedTempFile::with_suffix(".yaml").unwrap();
        write!(
            tmpl,
            r#"specificationVersion: "jobtemplate-2023-09"
name: test
parameterDefinitions:
  - name: J
    type: STRING
  - name: Extra
    type: STRING
steps:
  - name: s1
    script:
      actions:
        onRun:
          command: echo
          args: ["{{{{Param.J}}}} {{{{Param.Extra}}}}"]
"#
        )
        .unwrap();
        let (code, stdout, stderr) = run_cli(&[
            "run",
            tmpl.path().to_str().unwrap(),
            "-p",
            &file_arg,
            "-p",
            "Extra=ExtraVal",
        ]);
        assert_eq!(code, 0, "should succeed. stderr: {stderr}");
        assert!(stdout.contains("FromFile"), "stdout: {stdout}");
        assert!(stdout.contains("ExtraVal"), "stdout: {stdout}");
    }

    #[test]
    fn test_params_bad_json_file() {
        let mut f = NamedTempFile::with_suffix(".json").unwrap();
        write!(f, "{{bad json}}").unwrap();
        let file_arg = format!("file://{}", f.path().display());
        let template = templates_dir().join("simple_with_j_param.yaml");
        let (code, _stdout, stderr) =
            run_cli(&["run", template.to_str().unwrap(), "-p", &file_arg]);
        assert_ne!(code, 0, "bad JSON should fail");
        assert!(!stderr.is_empty(), "stderr: {stderr}");
    }

    #[test]
    fn test_params_non_dict_json_file() {
        let mut f = NamedTempFile::with_suffix(".json").unwrap();
        write!(f, r#"["not", "a", "dict"]"#).unwrap();
        let file_arg = format!("file://{}", f.path().display());
        let template = templates_dir().join("simple_with_j_param.yaml");
        let (code, _stdout, stderr) =
            run_cli(&["run", template.to_str().unwrap(), "-p", &file_arg]);
        assert_ne!(code, 0, "non-dict JSON should fail");
        assert!(stderr.contains("dictionary"), "stderr: {stderr}");
    }

    #[test]
    fn test_params_directory_as_file() {
        let dir = tempfile::TempDir::new().unwrap();
        let file_arg = format!("file://{}", dir.path().display());
        let template = templates_dir().join("simple_with_j_param.yaml");
        let (code, _stdout, stderr) =
            run_cli(&["run", template.to_str().unwrap(), "-p", &file_arg]);
        assert_ne!(code, 0, "directory as param file should fail");
        assert!(!stderr.is_empty(), "stderr: {stderr}");
    }

    #[test]
    fn test_params_not_json_string() {
        let template = templates_dir().join("simple_with_j_param.yaml");
        let (code, _stdout, stderr) =
            run_cli(&["run", template.to_str().unwrap(), "-p", "- not json -"]);
        assert_ne!(code, 0, "non-json non-kvp should fail");
        assert!(
            stderr.contains("not formatted correctly") || stderr.contains("format"),
            "stderr: {stderr}"
        );
    }

    #[test]
    fn test_params_json_array_string() {
        let template = templates_dir().join("simple_with_j_param.yaml");
        let (code, _stdout, stderr) =
            run_cli(&["run", template.to_str().unwrap(), "-p", r#"["a", "b"]"#]);
        assert_ne!(code, 0, "JSON array string should fail");
        assert!(
            stderr.contains("not formatted correctly") || stderr.contains("format"),
            "stderr: {stderr}"
        );
    }
}

// ============================================================
// Group 9: Run with Environment Templates (test_run_with_env.py)
// ============================================================

mod run_with_env {
    use super::*;

    #[test]
    fn test_run_job_with_env_default_params() {
        let tdir = templates_dir();
        let (code, stdout, stderr) = run_cli(&[
            "run",
            tdir.join("simple_with_j_param.yaml").to_str().unwrap(),
            "-p",
            "J=Jvalue",
            "--env",
            tdir.join("env_with_param.yaml").to_str().unwrap(),
        ]);
        assert_eq!(code, 0, "should succeed. stderr: {stderr}");
        assert!(
            stdout.contains("EnvWithParam Enter DefaultForEnvParam"),
            "stdout: {stdout}"
        );
        assert!(stdout.contains("DoTask Jvalue"), "stdout: {stdout}");
        assert!(
            stdout.contains("EnvWithParam Exit DefaultForEnvParam"),
            "stdout: {stdout}"
        );
    }

    #[test]
    fn test_run_job_with_env_provide_env_param() {
        let tdir = templates_dir();
        let (code, stdout, stderr) = run_cli(&[
            "run",
            tdir.join("simple_with_j_param.yaml").to_str().unwrap(),
            "-p",
            "J=Jvalue",
            "-p",
            "EnvParam=EnvParamValue",
            "--env",
            tdir.join("env_with_param.yaml").to_str().unwrap(),
        ]);
        assert_eq!(code, 0, "should succeed. stderr: {stderr}");
        assert!(
            stdout.contains("EnvWithParam Enter EnvParamValue"),
            "stdout: {stdout}"
        );
        assert!(stdout.contains("DoTask Jvalue"), "stdout: {stdout}");
        assert!(
            stdout.contains("EnvWithParam Exit EnvParamValue"),
            "stdout: {stdout}"
        );
    }
}

// ============================================================
// Group 10: Feature Bundle 1 (test_feature_bundle_1.py)
// ============================================================

mod feature_bundle_1 {
    use super::*;

    #[test]
    fn test_python_syntax_sugar() {
        let template = templates_dir().join("feature_bundle_1_python.yaml");
        let (code, stdout, stderr) = run_cli(&["run", template.to_str().unwrap()]);
        assert_eq!(code, 0, "should succeed. stderr: {stderr}");
        assert!(stdout.contains("Hello from Python!"), "stdout: {stdout}");
    }

    #[test]
    fn test_bash_syntax_sugar() {
        let template = templates_dir().join("feature_bundle_1_bash.yaml");
        let (code, stdout, stderr) = run_cli(&["run", template.to_str().unwrap()]);
        assert_eq!(code, 0, "should succeed. stderr: {stderr}");
        assert!(stdout.contains("Hello from Bash!"), "stdout: {stdout}");
    }

    #[test]
    fn test_format_string_timeout() {
        let template = templates_dir().join("feature_bundle_1_timeout.yaml");
        let (code, stdout, stderr) = run_cli(&["run", template.to_str().unwrap()]);
        assert_eq!(code, 0, "should succeed. stderr: {stderr}");
        assert!(
            stdout.contains("Running with timeout 5s"),
            "stdout: {stdout}"
        );
    }

    #[test]
    fn test_format_string_amount_minmax() {
        let template = templates_dir().join("feature_bundle_1_amount_minmax.yaml");
        let (code, stdout, stderr) = run_cli(&["run", template.to_str().unwrap()]);
        assert_eq!(code, 0, "should succeed. stderr: {stderr}");
        assert!(stdout.contains("Amount min/max works!"), "stdout: {stdout}");
    }

    #[test]
    fn test_format_string_notify_period() {
        let template = templates_dir().join("feature_bundle_1_notify_period.yaml");
        let (code, stdout, stderr) = run_cli(&["run", template.to_str().unwrap()]);
        assert_eq!(code, 0, "should succeed. stderr: {stderr}");
        assert!(stdout.contains("Notify period works!"), "stdout: {stdout}");
    }

    #[test]
    fn test_extended_step_name() {
        let template = templates_dir().join("feature_bundle_1_long_name.yaml");
        let (code, stdout, stderr) = run_cli(&["run", template.to_str().unwrap()]);
        assert_eq!(code, 0, "should succeed. stderr: {stderr}");
        assert!(stdout.contains("Long step name works!"), "stdout: {stdout}");
    }

    #[test]
    fn test_end_of_line_lf() {
        let template = templates_dir().join("feature_bundle_1_eol_lf.yaml");
        let (code, stdout, stderr) = run_cli(&["run", template.to_str().unwrap()]);
        assert_eq!(code, 0, "should succeed. stderr: {stderr}");
        assert!(
            stdout.contains("310a 6c69 6e65 320a 6c69"),
            "stdout: {stdout}"
        );
    }

    #[test]
    fn test_end_of_line_crlf() {
        let template = templates_dir().join("feature_bundle_1_eol_crlf.yaml");
        let (code, stdout, stderr) = run_cli(&["run", template.to_str().unwrap()]);
        assert_eq!(code, 0, "should succeed. stderr: {stderr}");
        assert!(
            stdout.contains("310d 0a6c 696e 6532 0d0a"),
            "stdout: {stdout}"
        );
    }

    #[test]
    fn test_end_of_line_auto() {
        let template = templates_dir().join("feature_bundle_1_eol_auto.yaml");
        let (code, stdout, stderr) = run_cli(&["run", template.to_str().unwrap()]);
        assert_eq!(code, 0, "should succeed. stderr: {stderr}");
        if cfg!(windows) {
            // On Windows, AUTO should produce CRLF
            assert!(
                stdout.contains("310d 0a6c 696e 6532 0d0a"),
                "stdout: {stdout}"
            );
        } else {
            // On Linux/macOS, AUTO should produce LF
            assert!(
                stdout.contains("310a 6c69 6e65 320a 6c69"),
                "stdout: {stdout}"
            );
        }
    }

    #[test]
    fn test_check_validates_extension() {
        let template = templates_dir().join("feature_bundle_1_python.yaml");
        let (code, stdout, _stderr) = run_cli(&["check", template.to_str().unwrap()]);
        assert_eq!(code, 0, "check should succeed");
        assert!(
            stdout.contains("passes validation checks"),
            "stdout: {stdout}"
        );
    }
}

// ============================================================
// Group 12: Redacted Environment Variables (test_redacted_env.py)
// ============================================================

mod redacted_env {
    use super::*;

    #[test]
    fn test_run_job_with_redacted_env() {
        let template = templates_dir().join("redacted_env.yaml");
        let (code, stdout, stderr) = run_cli(&["run", template.to_str().unwrap()]);
        assert_eq!(code, 0, "should succeed. stderr: {stderr}");
        assert!(stdout.contains("Setting redacted vars"), "stdout: {stdout}");
        // Verify the openjd_redacted_env protocol lines are not leaked
        assert!(
            !stdout.contains("openjd_redacted_env: SECRETVAR=SECRETVAL"),
            "should not leak protocol. stdout: {stdout}"
        );
        assert!(
            !stdout.contains("openjd_redacted_env: KEYSPACE =SECRETVAL"),
            "should not leak protocol. stdout: {stdout}"
        );
        assert!(
            !stdout.contains("openjd_redacted_env: VALSPACE= SPACEVAL"),
            "should not leak protocol. stdout: {stdout}"
        );
    }
}

// ============================================================
// Group 6: Run Command (test_run_command.py)
// ============================================================

mod run_command {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    // --- test_do_run_success parametrized cases ---

    #[test]
    fn test_run_first_step() {
        let tdir = templates_dir();
        let (code, stdout, stderr) = run_cli(&[
            "run",
            tdir.join("basic.yaml").to_str().unwrap(),
            "--step",
            "First",
            "-p",
            "J=Jvalue",
            "--run-dependencies",
            "--extensions",
            "",
        ]);
        assert_eq!(code, 0, "stderr: {stderr}");
        assert!(stdout.contains("J1 Enter"), "stdout: {stdout}");
        assert!(stdout.contains("J2 Enter"), "stdout: {stdout}");
        assert!(stdout.contains("J=Jvalue"), "stdout: {stdout}");
        assert!(stdout.contains("Foo=1. Bar=Bar1"), "stdout: {stdout}");
        assert!(stdout.contains("Foo=1. Bar=Bar2"), "stdout: {stdout}");
    }

    #[test]
    fn test_run_second_step_with_dep() {
        let tdir = templates_dir();
        let (code, stdout, stderr) = run_cli(&[
            "run",
            tdir.join("basic_dependency_job.yaml").to_str().unwrap(),
            "--step",
            "Second",
            "-p",
            "J=Jvalue",
            "--run-dependencies",
            "--extensions",
            "",
        ]);
        assert_eq!(code, 0, "stderr: {stderr}");
        assert!(stdout.contains("J=Jvalue Fuz=1"), "stdout: {stdout}");
        assert!(stdout.contains("J=Jvalue Fuz=2"), "stdout: {stdout}");
        // Should also run First step (dependency)
        assert!(stdout.contains("Foo=1. Bar=Bar1"), "stdout: {stdout}");
    }

    #[test]
    fn test_run_second_step_no_dep() {
        let tdir = templates_dir();
        let (code, stdout, stderr) = run_cli(&[
            "run",
            tdir.join("basic_dependency_job.yaml").to_str().unwrap(),
            "--step",
            "Second",
            "-p",
            "J=Jvalue",
            "--extensions",
            "",
        ]);
        assert_eq!(code, 0, "stderr: {stderr}");
        assert!(stdout.contains("J=Jvalue Fuz=1"), "stdout: {stdout}");
        // Should NOT run First step
        assert!(
            !stdout.contains("Foo=1. Bar=Bar1"),
            "should not run dep. stdout: {stdout}"
        );
    }

    #[test]
    fn test_run_with_one_env() {
        let tdir = templates_dir();
        let (code, stdout, stderr) = run_cli(&[
            "run",
            tdir.join("basic.yaml").to_str().unwrap(),
            "--step",
            "First",
            "-p",
            "J=Jvalue",
            "--run-dependencies",
            "--environment",
            tdir.join("env_1.yaml").to_str().unwrap(),
            "--extensions",
            "",
        ]);
        assert_eq!(code, 0, "stderr: {stderr}");
        assert!(stdout.contains("Env1 Enter"), "stdout: {stdout}");
        assert!(stdout.contains("Env1 Exit"), "stdout: {stdout}");
        assert!(stdout.contains("J=Jvalue"), "stdout: {stdout}");
    }

    #[test]
    fn test_run_with_two_envs() {
        let tdir = templates_dir();
        let (code, stdout, stderr) = run_cli(&[
            "run",
            tdir.join("basic.yaml").to_str().unwrap(),
            "--step",
            "First",
            "-p",
            "J=Jvalue",
            "--run-dependencies",
            "--environment",
            tdir.join("env_1.yaml").to_str().unwrap(),
            "--environment",
            tdir.join("env_2.yaml").to_str().unwrap(),
            "--extensions",
            "",
        ]);
        assert_eq!(code, 0, "stderr: {stderr}");
        assert!(stdout.contains("Env1 Enter"), "stdout: {stdout}");
        assert!(stdout.contains("Env2 Enter"), "stdout: {stdout}");
        assert!(stdout.contains("Env2 Exit"), "stdout: {stdout}");
        assert!(stdout.contains("Env1 Exit"), "stdout: {stdout}");
    }

    #[test]
    fn test_run_error_nonexistent_template() {
        let (code, _stdout, stderr) = run_cli(&["run", "some-file.json"]);
        assert_ne!(code, 0);
        assert!(stderr.contains("does not exist"), "stderr: {stderr}");
    }

    #[test]
    fn test_run_nonexistent_step() {
        let tdir = templates_dir();
        let (code, _stdout, stderr) = run_cli(&[
            "run",
            tdir.join("basic.yaml").to_str().unwrap(),
            "--step",
            "FakeStep",
            "-p",
            "J=Jvalue",
            "--extensions",
            "",
        ]);
        assert_ne!(code, 0);
        assert!(
            stderr.contains("No Step with name 'FakeStep'"),
            "stderr: {stderr}"
        );
    }

    #[test]
    fn test_preserve_option() {
        let mut f = NamedTempFile::with_suffix(".json").unwrap();
        write!(
            f,
            r#"{{
            "specificationVersion": "jobtemplate-2023-09",
            "name": "TestJob",
            "steps": [{{
                "name": "TestStep",
                "script": {{"actions": {{"onRun": {{"command": "echo", "args": ["hello"]}}}}}}
            }}]
        }}"#
        )
        .unwrap();
        let (code, stdout, stderr) = run_cli(&["run", f.path().to_str().unwrap(), "--preserve"]);
        assert_eq!(code, 0, "stderr: {stderr}");
        assert!(
            stdout.contains("Working directory preserved at"),
            "stdout: {stdout}"
        );
    }

    #[test]
    fn test_run_path_mapping_rules() {
        let mut template_f = NamedTempFile::with_suffix(".json").unwrap();
        let (source_format, source_path, dest_path, param_value, expected) = if cfg!(windows) {
            (
                "WINDOWS",
                r"D:\\home\\work",
                r"E:\\mnt\\work",
                r"D:\home\work",
                r"Mapped:E:\mnt\work",
            )
        } else {
            (
                "POSIX",
                "/home/test",
                "/mnt/test",
                "/home/test",
                "Mapped:/mnt/test",
            )
        };
        write!(template_f, r#"{{
            "specificationVersion": "jobtemplate-2023-09",
            "name": "Job",
            "parameterDefinitions": [{{"name": "TestPath", "type": "PATH"}}],
            "steps": [{{
                "name": "TestStep",
                "script": {{"actions": {{"onRun": {{"command": "python", "args": ["-c", "print('Mapped:{{{{Param.TestPath}}}}')"]}}}}}}
            }}]
        }}"#).unwrap();

        let mut rules_f = NamedTempFile::with_suffix(".rules.json").unwrap();
        write!(
            rules_f,
            r#"{{
            "version": "pathmapping-1.0",
            "path_mapping_rules": [{{
                "source_path_format": "{source_format}",
                "source_path": "{source_path}",
                "destination_path": "{dest_path}"
            }}]
        }}"#
        )
        .unwrap();

        let rules_arg = format!("file://{}", rules_f.path().display());
        let param_arg = format!("TestPath={param_value}");
        let (code, stdout, stderr) = run_cli(&[
            "run",
            template_f.path().to_str().unwrap(),
            "-p",
            &param_arg,
            "--path-mapping-rules",
            &rules_arg,
        ]);
        assert_eq!(code, 0, "stderr: {stderr}");
        assert!(stdout.contains(expected), "stdout: {stdout}");
    }

    // --- Run local session tests ---

    #[test]
    fn test_run_bare_step() {
        let tdir = templates_dir();
        let (code, stdout, stderr) = run_cli(&[
            "run",
            tdir.join("job_with_test_steps.yaml").to_str().unwrap(),
            "--step",
            "BareStep",
            "--extensions",
            "",
        ]);
        assert_eq!(code, 0, "stderr: {stderr}");
        assert!(
            stdout.contains("All actions completed successfully!"),
            "stdout: {stdout}"
        );
    }

    #[test]
    fn test_run_normal_step() {
        let tdir = templates_dir();
        let (code, stdout, stderr) = run_cli(&[
            "run",
            tdir.join("job_with_test_steps.yaml").to_str().unwrap(),
            "--step",
            "NormalStep",
            "--extensions",
            "",
        ]);
        assert_eq!(code, 0, "stderr: {stderr}");
        assert!(stdout.contains("Hello, world!"), "stdout: {stdout}");
        assert!(
            stdout.contains("All actions completed successfully!"),
            "stdout: {stdout}"
        );
    }

    #[test]
    fn test_run_dependent_step_with_deps() {
        let tdir = templates_dir();
        let (code, stdout, stderr) = run_cli(&[
            "run",
            tdir.join("job_with_test_steps.yaml").to_str().unwrap(),
            "--step",
            "DependentStep",
            "--run-dependencies",
            "--extensions",
            "",
        ]);
        assert_eq!(code, 0, "stderr: {stderr}");
        assert!(
            stdout.contains("Running step 'BareStep'"),
            "stdout: {stdout}"
        );
        assert!(
            stdout.contains("Running step 'DependentStep'"),
            "stdout: {stdout}"
        );
        assert!(
            stdout.contains("All actions completed successfully!"),
            "stdout: {stdout}"
        );
    }

    #[test]
    fn test_run_dependent_step_no_deps() {
        let tdir = templates_dir();
        let (code, stdout, stderr) = run_cli(&[
            "run",
            tdir.join("job_with_test_steps.yaml").to_str().unwrap(),
            "--step",
            "DependentStep",
            "--extensions",
            "",
        ]);
        assert_eq!(code, 0, "stderr: {stderr}");
        assert!(
            !stdout.contains("Running step 'BareStep'"),
            "should not run dep. stdout: {stdout}"
        );
        assert!(stdout.contains("I am dependent!"), "stdout: {stdout}");
    }

    #[test]
    fn test_run_extra_dependent_step() {
        let tdir = templates_dir();
        let (code, stdout, stderr) = run_cli(&[
            "run",
            tdir.join("job_with_test_steps.yaml").to_str().unwrap(),
            "--step",
            "ExtraDependentStep",
            "--run-dependencies",
            "--extensions",
            "",
        ]);
        assert_eq!(code, 0, "stderr: {stderr}");
        assert!(
            stdout.contains("Running step 'BareStep'"),
            "stdout: {stdout}"
        );
        assert!(
            stdout.contains("Running step 'DependentStep'"),
            "stdout: {stdout}"
        );
        assert!(
            stdout.contains("Running step 'TaskParamStep'"),
            "stdout: {stdout}"
        );
        assert!(
            stdout.contains("Running step 'ExtraDependentStep'"),
            "stdout: {stdout}"
        );
    }

    #[test]
    fn test_run_task_param_step() {
        let tdir = templates_dir();
        let (code, stdout, stderr) = run_cli(&[
            "run",
            tdir.join("job_with_test_steps.yaml").to_str().unwrap(),
            "--step",
            "TaskParamStep",
            "--extensions",
            "",
        ]);
        assert_eq!(code, 0, "stderr: {stderr}");
        assert!(stdout.contains("1.Hi!"), "stdout: {stdout}");
        assert!(
            stdout.contains("All actions completed successfully!"),
            "stdout: {stdout}"
        );
    }

    #[test]
    fn test_run_bad_command() {
        let tdir = templates_dir();
        let (code, _stdout, stderr) = run_cli(&[
            "run",
            tdir.join("job_with_test_steps.yaml").to_str().unwrap(),
            "--step",
            "BadCommand",
            "--extensions",
            "",
        ]);
        assert_ne!(code, 0, "bad command should fail");
        assert!(
            !stderr.is_empty() || !_stdout.is_empty(),
            "should have error output"
        );
    }

    #[test]
    fn test_step_dep_has_step_env() {
        let tdir = templates_dir();
        let (code, stdout, stderr) = run_cli(&[
            "run",
            tdir.join("job_with_test_steps.yaml").to_str().unwrap(),
            "--step",
            "StepDepHasStepEnv",
            "--run-dependencies",
            "--extensions",
            "",
        ]);
        assert_eq!(code, 0, "stderr: {stderr}");
        assert!(
            stdout.contains("Running step 'NormalStep'"),
            "stdout: {stdout}"
        );
        assert!(
            stdout.contains("Running step 'StepDepHasStepEnv'"),
            "stdout: {stdout}"
        );
    }

    // --- Env/task failure tests (from test_do_run_success parametrized cases) ---

    #[test]
    fn test_enter_env_fails() {
        let tdir = templates_dir();
        let (code, stdout, _stderr) = run_cli(&[
            "run",
            tdir.join("simple_with_j_param.yaml").to_str().unwrap(),
            "-p",
            "J=Jvalue",
            "--environment",
            tdir.join("env_fails_enter.yaml").to_str().unwrap(),
        ]);
        assert_ne!(code, 0, "should fail when env enter fails");
        assert!(stdout.contains("EnvEnterFail Enter"), "stdout: {stdout}");
        // Should not run the task
        assert!(
            !stdout.contains("DoTask"),
            "should not run task. stdout: {stdout}"
        );
    }

    #[test]
    fn test_task_fails_still_exits_env() {
        let tdir = templates_dir();
        let (code, stdout, _stderr) = run_cli(&[
            "run",
            tdir.join("simple_with_j_param_exit_1.yaml")
                .to_str()
                .unwrap(),
            "-p",
            "J=Jvalue",
            "--environment",
            tdir.join("env_1.yaml").to_str().unwrap(),
        ]);
        assert_ne!(code, 0, "should fail when task exits 1");
        assert!(stdout.contains("Env1 Enter"), "stdout: {stdout}");
        assert!(stdout.contains("DoTask"), "stdout: {stdout}");
    }

    #[test]
    fn test_env_exit_fails() {
        let tdir = templates_dir();
        let (code, stdout, _stderr) = run_cli(&[
            "run",
            tdir.join("simple_with_j_param.yaml").to_str().unwrap(),
            "-p",
            "J=Jvalue",
            "--environment",
            tdir.join("env_fails_exit.yaml").to_str().unwrap(),
        ]);
        // Task should still run even if env exit fails
        assert!(stdout.contains("EnvExitFail Enter"), "stdout: {stdout}");
        assert!(stdout.contains("DoTask"), "stdout: {stdout}");
        let _ = code; // exit code may vary
    }

    // --- Step let bindings ---

    #[test]
    fn test_step_let_bindings_in_step_env() {
        let tdir = templates_dir();
        let (code, stdout, stderr) = run_cli(&[
            "run",
            tdir.join("step_let_in_step_env.yaml").to_str().unwrap(),
        ]);
        assert_eq!(code, 0, "stderr: {stderr}");
        assert!(stdout.contains("ENTER_VAL:21"), "stdout: {stdout}");
        assert!(stdout.contains("ENTER_LABEL:item_21"), "stdout: {stdout}");
        assert!(stdout.contains("ENV_VAL:21"), "stdout: {stdout}");
        assert!(stdout.contains("ENV_LABEL:item_21"), "stdout: {stdout}");
        assert!(stdout.contains("TASK_VAL:21"), "stdout: {stdout}");
        assert!(stdout.contains("EXIT_VAL:21"), "stdout: {stdout}");
        assert!(stdout.contains("EXIT_LABEL:item_21"), "stdout: {stdout}");
    }

    #[test]
    fn test_task_timeout() {
        // Action-level timeout is enforced: the task should be killed before
        // printing EXIT_NORMAL.
        let tdir = templates_dir();
        let (code, stdout, _stderr) = run_cli(&[
            "run",
            tdir.join("job_sleep_exit_normal.yaml").to_str().unwrap(),
            "--step",
            "Timeout",
            "-p",
            "J=x",
        ]);
        assert!(
            stdout.contains("SLEEP"),
            "should print SLEEP. stdout: {stdout}"
        );
        assert!(
            !stdout.contains("EXIT_NORMAL"),
            "should NOT print EXIT_NORMAL (killed by timeout). stdout: {stdout}"
        );
        assert_ne!(code, 0, "task should fail due to timeout");
    }

    /// Regression test: NaN / Infinity values for a FLOAT task parameter must
    /// produce a clean error rather than panicking inside `Float64::new`.
    /// See the 2026-05-06 security review ("[LOW] Potential Panic via unwrap()
    /// on Float64::new() with NaN Input").
    #[test]
    fn test_run_task_param_float_rejects_nan_and_infinity() {
        use std::io::Write;
        use tempfile::NamedTempFile;

        let mut f = NamedTempFile::with_suffix(".yaml").unwrap();
        write!(
            f,
            r#"specificationVersion: "jobtemplate-2023-09"
name: float-task-param
steps:
  - name: s1
    parameterSpace:
      taskParameterDefinitions:
        - name: MyFloat
          type: FLOAT
          range: [1.0, 2.0, 3.0]
    script:
      actions:
        onRun:
          command: python
          args: ["-c", "print('{{{{Task.Param.MyFloat}}}}')"]
"#
        )
        .unwrap();
        let path = f.path().to_str().unwrap();

        for bad in ["NaN", "inf", "-inf", "infinity"] {
            let tasks = format!(r#"[{{"MyFloat":"{bad}"}}]"#);
            let (code, _stdout, stderr) =
                run_cli(&["run", path, "--step", "s1", "--tasks", &tasks]);
            assert_ne!(
                code, 0,
                "expected non-zero exit for '{bad}', stderr: {stderr}"
            );
            // Must not be a Rust panic — check for our friendly error message.
            assert!(
                !stderr.contains("panicked"),
                "must not panic on '{bad}'; stderr: {stderr}"
            );
            assert!(
                stderr.contains("must be finite"),
                "expected 'must be finite' error for '{bad}'; stderr: {stderr}"
            );
        }
    }
}

// ============================================================
// Group 3 continued: Common Utilities — Error Cases
// ============================================================

mod common_errors {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_check_job_template_parsing_error_json() {
        let mut f = NamedTempFile::with_suffix(".template.json").unwrap();
        write!(f, r#"{{ "specificationVersion": "jobtemplate-2023-09" }}"#).unwrap();
        let (code, _stdout, stderr) = run_cli(&["check", f.path().to_str().unwrap()]);
        assert_ne!(code, 0, "should fail validation");
        assert!(
            stderr.contains("validation") || stderr.contains("missing") || stderr.contains("error"),
            "stderr: {stderr}"
        );
    }

    #[test]
    fn test_check_job_template_parsing_error_yaml() {
        let mut f = NamedTempFile::with_suffix(".template.yaml").unwrap();
        writeln!(f, "specificationVersion: \"jobtemplate-2023-09\"").unwrap();
        let (code, _stdout, stderr) = run_cli(&["check", f.path().to_str().unwrap()]);
        assert_ne!(code, 0, "should fail validation");
        assert!(
            stderr.contains("validation") || stderr.contains("missing") || stderr.contains("error"),
            "stderr: {stderr}"
        );
    }

    #[test]
    fn test_check_env_template_parsing_error() {
        let mut f = NamedTempFile::with_suffix(".template.yaml").unwrap();
        writeln!(f, "specificationVersion: \"environment-2023-09\"").unwrap();
        let (code, _stdout, stderr) = run_cli(&["check", f.path().to_str().unwrap()]);
        assert_ne!(code, 0, "should fail validation");
        assert!(
            stderr.contains("validation") || stderr.contains("missing") || stderr.contains("error"),
            "stderr: {stderr}"
        );
    }

    #[test]
    fn test_run_extra_params_error() {
        let mut f = NamedTempFile::with_suffix(".yaml").unwrap();
        write!(
            f,
            r#"specificationVersion: "jobtemplate-2023-09"
name: test
steps:
  - name: s1
    script:
      actions:
        onRun:
          command: echo
"#
        )
        .unwrap();
        let (code, _stdout, stderr) =
            run_cli(&["run", f.path().to_str().unwrap(), "-p", "ExtraParam=value"]);
        assert_ne!(code, 0, "extra params should fail");
        assert!(
            stderr.contains("not defined") || stderr.contains("parameter"),
            "stderr: {stderr}"
        );
    }

    #[test]
    fn test_run_missing_required_params_error() {
        let mut f = NamedTempFile::with_suffix(".yaml").unwrap();
        write!(
            f,
            r#"specificationVersion: "jobtemplate-2023-09"
name: test
parameterDefinitions:
  - name: Required
    type: INT
    minValue: 3
    maxValue: 8
steps:
  - name: s1
    script:
      actions:
        onRun:
          command: echo
"#
        )
        .unwrap();
        let (code, _stdout, stderr) = run_cli(&["run", f.path().to_str().unwrap()]);
        assert_ne!(code, 0, "missing params should fail");
        assert!(
            stderr.contains("missing") || stderr.contains("required") || stderr.contains("Values"),
            "stderr: {stderr}"
        );
    }

    #[test]
    fn test_run_invalid_param_type_error() {
        let mut f = NamedTempFile::with_suffix(".yaml").unwrap();
        write!(
            f,
            r#"specificationVersion: "jobtemplate-2023-09"
name: test
parameterDefinitions:
  - name: Count
    type: INT
steps:
  - name: s1
    script:
      actions:
        onRun:
          command: echo
"#
        )
        .unwrap();
        let (code, _stdout, stderr) =
            run_cli(&["run", f.path().to_str().unwrap(), "-p", "Count=notanumber"]);
        assert_ne!(code, 0, "invalid type should fail");
        assert!(
            stderr.contains("integer") || stderr.contains("INT") || stderr.contains("error"),
            "stderr: {stderr}"
        );
    }

    #[test]
    fn test_run_param_constraint_violation() {
        let mut f = NamedTempFile::with_suffix(".yaml").unwrap();
        write!(
            f,
            r#"specificationVersion: "jobtemplate-2023-09"
name: test
parameterDefinitions:
  - name: Title
    type: STRING
    minLength: 3
  - name: Required
    type: INT
    minValue: 3
    maxValue: 8
steps:
  - name: s1
    script:
      actions:
        onRun:
          command: echo
"#
        )
        .unwrap();
        let (code, _stdout, stderr) = run_cli(&[
            "run",
            f.path().to_str().unwrap(),
            "-p",
            "Title=a",
            "-p",
            "Required=5",
        ]);
        assert_ne!(code, 0, "constraint violation should fail");
        assert!(
            stderr.contains("length")
                || stderr.contains("characters")
                || stderr.contains("at least"),
            "stderr: {stderr}"
        );
    }

    #[test]
    fn test_run_param_file_nonexistent() {
        let mut f = NamedTempFile::with_suffix(".yaml").unwrap();
        write!(
            f,
            r#"specificationVersion: "jobtemplate-2023-09"
name: test
parameterDefinitions:
  - name: P
    type: STRING
steps:
  - name: s1
    script:
      actions:
        onRun:
          command: echo
"#
        )
        .unwrap();
        let (code, _stdout, stderr) = run_cli(&[
            "run",
            f.path().to_str().unwrap(),
            "-p",
            "file:///nonexistent/params.json",
        ]);
        assert_ne!(code, 0, "nonexistent param file should fail");
        assert!(
            stderr.to_lowercase().contains("no such file")
                || stderr.to_lowercase().contains("cannot read")
                || stderr.to_lowercase().contains("not found")
                || stderr.to_lowercase().contains("does not exist"),
            "stderr: {stderr}"
        );
    }

    #[test]
    fn test_run_invalid_param_format() {
        let mut f = NamedTempFile::with_suffix(".yaml").unwrap();
        write!(
            f,
            r#"specificationVersion: "jobtemplate-2023-09"
name: test
steps:
  - name: s1
    script:
      actions:
        onRun:
          command: echo
"#
        )
        .unwrap();
        let (code, _stdout, stderr) = run_cli(&[
            "run",
            f.path().to_str().unwrap(),
            "-p",
            "bad format no equals",
        ]);
        assert_ne!(code, 0, "badly formatted param should fail");
        assert!(
            stderr.contains("Invalid parameter format") || stderr.contains("not formatted"),
            "stderr: {stderr}"
        );
    }

    /// Regression test: `file://` parameter files larger than
    /// `MAX_FILE_INPUT_SIZE` must be rejected with a clear error rather
    /// than being fully read into memory.
    /// See the 2026-05-06 security review ("[LOW] Unbounded File Read via
    /// `file://` Parameter Paths").
    #[test]
    fn test_run_job_param_file_size_limit() {
        // 11 MiB sparse file — exceeds the 10 MiB cap. Uses set_len so the
        // test doesn't actually write gigabytes of data.
        let oversized = NamedTempFile::with_suffix(".json").unwrap();
        oversized
            .as_file()
            .set_len(11 * 1024 * 1024)
            .expect("set_len should succeed");
        let file_arg = format!("file://{}", oversized.path().display());

        let mut tpl = NamedTempFile::with_suffix(".yaml").unwrap();
        write!(
            tpl,
            r#"specificationVersion: "jobtemplate-2023-09"
name: test
steps:
  - name: s1
    script:
      actions:
        onRun:
          command: echo
"#
        )
        .unwrap();

        let (code, _stdout, stderr) =
            run_cli(&["run", tpl.path().to_str().unwrap(), "-p", &file_arg]);
        assert_ne!(code, 0, "oversized param file should be rejected");
        assert!(stderr.contains("exceeds maximum size"), "stderr: {stderr}");
    }

    /// Regression test: oversized `file://` tasks files are also rejected.
    #[test]
    fn test_run_tasks_file_size_limit() {
        let oversized = NamedTempFile::with_suffix(".json").unwrap();
        oversized
            .as_file()
            .set_len(11 * 1024 * 1024)
            .expect("set_len should succeed");
        let tasks_arg = format!("file://{}", oversized.path().display());

        let tdir = templates_dir();
        let (code, _stdout, stderr) = run_cli(&[
            "run",
            tdir.join("job_with_test_steps.yaml").to_str().unwrap(),
            "--step",
            "TaskParamStep",
            "--tasks",
            &tasks_arg,
            "--extensions",
            "",
        ]);
        assert_ne!(code, 0, "oversized tasks file should be rejected");
        assert!(stderr.contains("exceeds maximum size"), "stderr: {stderr}");
    }

    /// Regression test: oversized `file://` path-mapping-rules files are
    /// also rejected.
    #[test]
    fn test_run_path_mapping_rules_file_size_limit() {
        let oversized = NamedTempFile::with_suffix(".json").unwrap();
        oversized
            .as_file()
            .set_len(11 * 1024 * 1024)
            .expect("set_len should succeed");
        let rules_arg = format!("file://{}", oversized.path().display());

        let mut tpl = NamedTempFile::with_suffix(".yaml").unwrap();
        write!(
            tpl,
            r#"specificationVersion: "jobtemplate-2023-09"
name: test
steps:
  - name: s1
    script:
      actions:
        onRun:
          command: echo
"#
        )
        .unwrap();

        let (code, _stdout, stderr) = run_cli(&[
            "run",
            tpl.path().to_str().unwrap(),
            "--path-mapping-rules",
            &rules_arg,
        ]);
        assert_ne!(code, 0, "oversized path-mapping file should be rejected");
        assert!(stderr.contains("exceeds maximum size"), "stderr: {stderr}");
    }
}

// ============================================================
// Group 11: Chunked Job (test_chunked_job.py)
// ============================================================

mod chunked_job {
    use super::*;

    #[test]
    fn test_check_chunked_job_default() {
        let template = templates_dir().join("chunked_job.yaml");
        let (code, stdout, _stderr) = run_cli(&["check", template.to_str().unwrap()]);
        assert_eq!(code, 0, "check should succeed with default extensions");
        assert!(
            stdout.contains("passes validation checks"),
            "stdout: {stdout}"
        );
    }

    #[test]
    fn test_check_chunked_job_no_extensions() {
        let template = templates_dir().join("chunked_job.yaml");
        let (code, _stdout, stderr) =
            run_cli(&["check", template.to_str().unwrap(), "--extensions", ""]);
        assert_ne!(code, 0, "should fail without TASK_CHUNKING extension");
        assert!(
            stderr.contains("TASK_CHUNKING")
                || stderr.contains("extension")
                || stderr.contains("Unsupported"),
            "stderr: {stderr}"
        );
    }

    #[test]
    fn test_check_chunked_job_with_extension() {
        let template = templates_dir().join("chunked_job.yaml");
        let (code, stdout, _stderr) = run_cli(&[
            "check",
            template.to_str().unwrap(),
            "--extensions",
            "TASK_CHUNKING",
        ]);
        assert_eq!(code, 0, "check should succeed with TASK_CHUNKING");
        assert!(
            stdout.contains("passes validation checks"),
            "stdout: {stdout}"
        );
    }

    #[test]
    fn test_run_chunked_job_default_options() {
        let template = templates_dir().join("chunked_job.yaml");
        let (code, stdout, stderr) =
            run_cli(&["run", template.to_str().unwrap(), "--step", "Chunked Step"]);
        assert_eq!(code, 0, "stderr: {stderr}");
        // Should run 4 chunks of 10 items each (1-10, 11-20, 21-30, 31-40)
        assert!(stdout.contains("1-10"), "stdout: {stdout}");
        assert!(stdout.contains("11-20"), "stdout: {stdout}");
        assert!(stdout.contains("21-30"), "stdout: {stdout}");
        assert!(stdout.contains("31-40"), "stdout: {stdout}");
    }

    #[test]
    fn test_run_chunked_job_maximum_task_count() {
        let template = templates_dir().join("chunked_job.yaml");
        let (code, stdout, stderr) = run_cli(&[
            "run",
            template.to_str().unwrap(),
            "--step",
            "Chunked Step",
            "-p",
            "ChunkSize=3",
            "-p",
            "TargetRuntime=0",
            "--maximum-tasks",
            "3",
        ]);
        assert_eq!(code, 0, "stderr: {stderr}");
        // Should only run 3 chunks
        assert!(stdout.contains("Chunks run: 3"), "stdout: {stdout}");
    }

    #[test]
    fn test_run_chunked_job_adaptive_chunking() {
        let template = templates_dir().join("chunked_job.yaml");
        let (code, stdout, stderr) = run_cli(&[
            "run",
            template.to_str().unwrap(),
            "--step",
            "Chunked Step",
            "-p",
            "ChunkSize=1",
            "-p",
            "TargetRuntime=10000",
        ]);
        assert_eq!(code, 0, "stderr: {stderr}");
        // With TargetRuntime very high and ChunkSize=1, adaptive chunking should
        // produce first chunk of 1 item, then remainder in second chunk
        assert!(stdout.contains("1"), "stdout: {stdout}");
        assert!(stdout.contains("2-40"), "stdout: {stdout}");
    }

    #[test]
    fn test_chunked_job_bad_task_param_out_of_range() {
        // Item=0 is outside the 1-40 range — the CLI now validates task param
        // values against the parameter space range.
        let template = templates_dir().join("chunked_job.yaml");
        let (code, _stdout, stderr) = run_cli(&[
            "run",
            template.to_str().unwrap(),
            "--step",
            "Chunked Step",
            "-t",
            "Item=0",
        ]);
        assert_ne!(code, 0, "should reject out-of-range task param value");
        assert!(
            stderr.contains("not in the parameter space") || stderr.contains("not a valid chunk"),
            "stderr: {stderr}"
        );
    }

    #[test]
    fn test_chunked_job_bad_task_param_not_range_expr() {
        // "1;2" is not a valid range expression — the CLI rejects it at parse time
        let template = templates_dir().join("chunked_job.yaml");
        let (code, _stdout, stderr) = run_cli(&[
            "run",
            template.to_str().unwrap(),
            "--step",
            "Chunked Step",
            "-t",
            "Item=1;2",
        ]);
        assert_ne!(code, 0, "task should fail with invalid range expression");
        assert!(
            stderr.contains("invalid range expression"),
            "stderr: {stderr}"
        );
    }

    #[test]
    fn test_chunked_job_bad_task_param_interval_out_of_range() {
        // Item=30-41 extends beyond the 1-40 range — the CLI now validates
        // task param values against the parameter space range.
        let template = templates_dir().join("chunked_job.yaml");
        let (code, _stdout, stderr) = run_cli(&[
            "run",
            template.to_str().unwrap(),
            "--step",
            "Chunked Step",
            "-t",
            "Item=30-41",
        ]);
        assert_ne!(
            code, 0,
            "should reject out-of-range interval task param values"
        );
        assert!(
            stderr.contains("not in the parameter space")
                || stderr.contains("not a valid chunk")
                || stderr.contains("not a subset"),
            "stderr: {stderr}"
        );
    }
}

// ============================================================
// Group 7: Context-Aware Help (test_help_formatter.py + TestContextAwareHelp)
// ============================================================

mod context_aware_help {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_help_with_yaml_template() {
        let tdir = templates_dir();
        let (code, stdout, _stderr) = run_cli(&[
            "run",
            tdir.join("job_with_test_steps.yaml").to_str().unwrap(),
            "-h",
        ]);
        assert_eq!(code, 0);
        assert!(stdout.contains("Job: my-job"), "stdout: {stdout}");
        assert!(
            stdout.contains("Job Parameters (-p/--job-param PARAM_NAME=VALUE):"),
            "stdout: {stdout}"
        );
        assert!(stdout.contains("Message (STRING)"), "stdout: {stdout}");
        assert!(
            stdout.contains("[default: 'Hello, world!']"),
            "stdout: {stdout}"
        );
    }

    #[test]
    fn test_help_with_long_flag() {
        let tdir = templates_dir();
        let (code, stdout, _stderr) =
            run_cli(&["run", tdir.join("basic.yaml").to_str().unwrap(), "--help"]);
        assert_eq!(code, 0);
        assert!(stdout.contains("Job: Job"), "stdout: {stdout}");
        assert!(stdout.contains("J (STRING)"), "stdout: {stdout}");
        assert!(stdout.contains("[required]"), "stdout: {stdout}");
    }

    #[test]
    fn test_help_with_description() {
        let mut f = NamedTempFile::with_suffix(".json").unwrap();
        write!(
            f,
            r#"{{
            "specificationVersion": "jobtemplate-2023-09",
            "name": "TestJob",
            "description": "This is a test job with a description",
            "steps": [{{
                "name": "TestStep",
                "script": {{"actions": {{"onRun": {{"command": "echo"}}}}}}
            }}]
        }}"#
        )
        .unwrap();
        let (code, stdout, _stderr) = run_cli(&["run", f.path().to_str().unwrap(), "-h"]);
        assert_eq!(code, 0);
        assert!(stdout.contains("Job: TestJob"), "stdout: {stdout}");
        assert!(
            stdout.contains("This is a test job with a description"),
            "stdout: {stdout}"
        );
    }

    #[test]
    fn test_help_with_multiple_parameters() {
        let mut f = NamedTempFile::with_suffix(".json").unwrap();
        write!(f, r#"{{
            "specificationVersion": "jobtemplate-2023-09",
            "name": "MultiParamJob",
            "parameterDefinitions": [
                {{"name": "StringParam", "type": "STRING", "default": "hello", "description": "A string parameter"}},
                {{"name": "IntParam", "type": "INT", "minValue": 1, "maxValue": 10}},
                {{"name": "FloatParam", "type": "FLOAT", "default": 3.14}},
                {{"name": "PathParam", "type": "PATH"}}
            ],
            "steps": [{{
                "name": "TestStep",
                "script": {{"actions": {{"onRun": {{"command": "echo"}}}}}}
            }}]
        }}"#).unwrap();
        let (code, stdout, _stderr) = run_cli(&["run", f.path().to_str().unwrap(), "--help"]);
        assert_eq!(code, 0);
        assert!(
            stdout.contains("StringParam (STRING) [default: 'hello']"),
            "stdout: {stdout}"
        );
        assert!(stdout.contains("A string parameter"), "stdout: {stdout}");
        assert!(
            stdout.contains("IntParam (INT) [required]"),
            "stdout: {stdout}"
        );
        assert!(stdout.contains("range: 1 to 10"), "stdout: {stdout}");
        assert!(
            stdout.contains("FloatParam (FLOAT) [default: 3.14]"),
            "stdout: {stdout}"
        );
        assert!(
            stdout.contains("PathParam (PATH) [required]"),
            "stdout: {stdout}"
        );
    }

    #[test]
    fn test_help_includes_standard_options() {
        let tdir = templates_dir();
        let (code, stdout, _stderr) =
            run_cli(&["run", tdir.join("basic.yaml").to_str().unwrap(), "-h"]);
        assert_eq!(code, 0);
        assert!(stdout.contains("Standard Options:"), "stdout: {stdout}");
        assert!(stdout.contains("--job-param"), "stdout: {stdout}");
        assert!(stdout.contains("--environment"), "stdout: {stdout}");
        assert!(stdout.contains("--verbose"), "stdout: {stdout}");
        assert!(
            stdout.contains("leave out template to list all options"),
            "stdout: {stdout}"
        );
    }

    #[test]
    fn test_help_without_template_shows_standard_help() {
        let (code, stdout, _stderr) = run_cli(&["run", "--help"]);
        assert_eq!(code, 0);
        assert!(
            !stdout.contains("Job:"),
            "should not show job info. stdout: {stdout}"
        );
        assert!(
            !stdout.contains("Job Parameters"),
            "should not show params. stdout: {stdout}"
        );
    }

    #[test]
    fn test_help_with_nonexistent_template() {
        let (code, _stdout, stderr) = run_cli(&["run", "nonexistent_template.json", "-h"]);
        assert_ne!(code, 0);
        assert!(stderr.contains("Error:"), "stderr: {stderr}");
        assert!(
            stderr.contains("not found") || stderr.contains("does not exist"),
            "stderr: {stderr}"
        );
    }

    #[test]
    fn test_help_with_invalid_json_template() {
        let mut f = NamedTempFile::with_suffix(".json").unwrap();
        write!(f, "{{invalid json content").unwrap();
        let (code, _stdout, stderr) = run_cli(&["run", f.path().to_str().unwrap(), "-h"]);
        assert_ne!(code, 0);
        assert!(stderr.contains("Error:"), "stderr: {stderr}");
    }

    #[test]
    fn test_help_with_schema_validation_failure() {
        let mut f = NamedTempFile::with_suffix(".json").unwrap();
        write!(f, r#"{{ "specificationVersion": "jobtemplate-2023-09" }}"#).unwrap();
        let (code, _stdout, stderr) = run_cli(&["run", f.path().to_str().unwrap(), "-h"]);
        assert_ne!(code, 0);
        assert!(stderr.contains("Error:"), "stderr: {stderr}");
        assert!(
            stderr.contains("Invalid job template") || stderr.contains("validation"),
            "stderr: {stderr}"
        );
    }

    #[test]
    fn test_help_error_messages_are_user_friendly() {
        let (code, _stdout, stderr) = run_cli(&["run", "does_not_exist.json", "--help"]);
        assert_ne!(code, 0);
        assert!(stderr.contains("Error:"), "stderr: {stderr}");
        assert!(
            !stderr.contains("Traceback"),
            "should not show traceback. stderr: {stderr}"
        );
        assert!(
            !stderr.contains("panicked"),
            "should not show panic. stderr: {stderr}"
        );
    }

    #[test]
    fn test_help_with_constraints() {
        let mut f = NamedTempFile::with_suffix(".json").unwrap();
        write!(f, r#"{{
            "specificationVersion": "jobtemplate-2023-09",
            "name": "constraint-job",
            "description": "Job with various constraints",
            "parameterDefinitions": [
                {{"name": "Environment", "type": "STRING", "default": "dev", "allowedValues": ["dev", "staging", "prod"]}},
                {{"name": "Ratio", "type": "FLOAT", "default": 0.5, "minValue": 0.0, "maxValue": 1.0}},
                {{"name": "Username", "type": "STRING", "default": "user", "minLength": 3, "maxLength": 20}}
            ],
            "steps": [{{
                "name": "Step1",
                "script": {{"actions": {{"onRun": {{"command": "echo"}}}}}}
            }}]
        }}"#).unwrap();
        let (code, stdout, _stderr) = run_cli(&["run", f.path().to_str().unwrap(), "-h"]);
        assert_eq!(code, 0);
        assert!(
            stdout.contains("Environment (STRING) [default: 'dev']"),
            "stdout: {stdout}"
        );
        assert!(
            stdout.contains("allowed: 'dev', 'staging', 'prod'"),
            "stdout: {stdout}"
        );
        assert!(
            stdout.contains("Ratio (FLOAT) [default: 0.5]"),
            "stdout: {stdout}"
        );
        assert!(stdout.contains("range: 0 to 1"), "stdout: {stdout}");
        assert!(
            stdout.contains("Username (STRING) [default: 'user']"),
            "stdout: {stdout}"
        );
        assert!(
            stdout.contains("length: 3 to 20 characters"),
            "stdout: {stdout}"
        );
    }

    #[test]
    fn test_normal_execution_unaffected() {
        let tdir = templates_dir();
        let (code, stdout, _stderr) = run_cli(&[
            "run",
            tdir.join("simple_with_j_param.yaml").to_str().unwrap(),
            "-p",
            "J=TestValue",
            "--extensions",
            "",
        ]);
        assert_eq!(code, 0);
        assert!(stdout.contains("DoTask"), "stdout: {stdout}");
        assert!(
            !stdout.contains("Job Parameters"),
            "should not show help. stdout: {stdout}"
        );
    }

    #[test]
    fn test_no_params_template_no_params_section() {
        let mut f = NamedTempFile::with_suffix(".json").unwrap();
        write!(
            f,
            r#"{{
            "specificationVersion": "jobtemplate-2023-09",
            "name": "no-params-job",
            "description": "A job without parameters",
            "steps": [{{
                "name": "Step1",
                "script": {{"actions": {{"onRun": {{"command": "echo"}}}}}}
            }}]
        }}"#
        )
        .unwrap();
        let (code, stdout, _stderr) = run_cli(&["run", f.path().to_str().unwrap(), "-h"]);
        assert_eq!(code, 0);
        assert!(stdout.contains("Job: no-params-job"), "stdout: {stdout}");
        assert!(
            stdout.contains("A job without parameters"),
            "stdout: {stdout}"
        );
        assert!(
            !stdout.contains("Job Parameters"),
            "should not show params section. stdout: {stdout}"
        );
        assert!(stdout.contains("Standard Options:"), "stdout: {stdout}");
    }

    #[test]
    fn test_job_info_before_standard_options() {
        let tdir = templates_dir();
        let (code, stdout, _stderr) = run_cli(&[
            "run",
            tdir.join("job_with_test_steps.yaml").to_str().unwrap(),
            "-h",
        ]);
        assert_eq!(code, 0);
        let job_idx = stdout.find("Job: my-job").unwrap();
        let params_idx = stdout.find("Job Parameters").unwrap();
        let options_idx = stdout.find("Standard Options:").unwrap();
        assert!(job_idx < params_idx, "job name should come before params");
        assert!(
            params_idx < options_idx,
            "params should come before standard options"
        );
    }

    #[test]
    fn test_help_with_invalid_yaml_template() {
        let mut f = NamedTempFile::with_suffix(".yaml").unwrap();
        write!(f, "invalid: yaml: content: [unclosed").unwrap();
        let (code, _stdout, stderr) = run_cli(&["run", f.path().to_str().unwrap(), "-h"]);
        assert_ne!(code, 0);
        assert!(stderr.contains("Error"), "stderr: {stderr}");
    }

    #[test]
    fn test_required_vs_optional_parameters() {
        let mut f = NamedTempFile::with_suffix(".json").unwrap();
        write!(
            f,
            r#"{{
            "specificationVersion": "jobtemplate-2023-09",
            "name": "ReqOptTest",
            "parameterDefinitions": [
                {{"name": "RequiredParam", "type": "STRING"}},
                {{"name": "OptionalParam", "type": "STRING", "default": "default_value"}},
                {{"name": "Count", "type": "INT", "default": 0}},
                {{"name": "Message", "type": "STRING", "default": ""}}
            ],
            "steps": [{{"name": "S1", "script": {{"actions": {{"onRun": {{"command": "echo"}}}}}}}}]
        }}"#
        )
        .unwrap();
        let (code, stdout, _stderr) = run_cli(&["run", f.path().to_str().unwrap(), "-h"]);
        assert_eq!(code, 0);
        assert!(stdout.contains("RequiredParam"), "stdout: {stdout}");
        assert!(stdout.contains("[required]"), "stdout: {stdout}");
        assert!(stdout.contains("OptionalParam"), "stdout: {stdout}");
        assert!(stdout.contains("default_value"), "stdout: {stdout}");
        assert!(stdout.contains("Count"), "stdout: {stdout}");
        assert!(
            stdout.contains("[default: 0]"),
            "Count with default=0 should show as default. stdout: {stdout}"
        );
        assert!(stdout.contains("Message"), "stdout: {stdout}");
        assert!(
            stdout.contains("[default: '']"),
            "Message with default='' should show as default. stdout: {stdout}"
        );
    }

    #[test]
    fn test_string_parameter_with_multiline_default() {
        let mut f = NamedTempFile::with_suffix(".json").unwrap();
        write!(f, r#"{{
            "specificationVersion": "jobtemplate-2023-09",
            "name": "MultilineTest",
            "parameterDefinitions": [
                {{"name": "Script", "type": "STRING", "default": "echo 'Hello'\necho 'World'\nls -la", "description": "A bash script to run"}}
            ],
            "steps": [{{"name": "S1", "script": {{"actions": {{"onRun": {{"command": "echo"}}}}}}}}]
        }}"#).unwrap();
        let (code, stdout, _stderr) = run_cli(&["run", f.path().to_str().unwrap(), "-h"]);
        assert_eq!(code, 0);
        assert!(stdout.contains("Script"), "stdout: {stdout}");
        assert!(stdout.contains("A bash script to run"), "stdout: {stdout}");
        assert!(stdout.contains("echo 'Hello'"), "stdout: {stdout}");
        assert!(stdout.contains("echo 'World'"), "stdout: {stdout}");
        assert!(stdout.contains("ls -la"), "stdout: {stdout}");
    }

    #[test]
    fn test_path_parameter_with_multiline_default() {
        let mut f = NamedTempFile::with_suffix(".json").unwrap();
        write!(f, r#"{{
            "specificationVersion": "jobtemplate-2023-09",
            "name": "PathMultilineTest",
            "parameterDefinitions": [
                {{"name": "ConfigFile", "type": "PATH", "default": "/path/to/file1\n/path/to/file2"}}
            ],
            "steps": [{{"name": "S1", "script": {{"actions": {{"onRun": {{"command": "echo"}}}}}}}}]
        }}"#).unwrap();
        let (code, stdout, _stderr) = run_cli(&["run", f.path().to_str().unwrap(), "-h"]);
        assert_eq!(code, 0);
        assert!(stdout.contains("ConfigFile"), "stdout: {stdout}");
        assert!(stdout.contains("/path/to/file1"), "stdout: {stdout}");
        assert!(stdout.contains("/path/to/file2"), "stdout: {stdout}");
    }

    #[test]
    fn test_missing_parameters_shows_help() {
        let mut f = NamedTempFile::with_suffix(".json").unwrap();
        write!(f, r#"{{
            "specificationVersion": "jobtemplate-2023-09",
            "name": "TestJob",
            "description": "A test job with required parameters",
            "parameterDefinitions": [
                {{"name": "RequiredParam1", "type": "STRING", "description": "First required parameter"}},
                {{"name": "RequiredParam2", "type": "INT", "description": "Second required parameter"}}
            ],
            "steps": [{{"name": "S", "script": {{"actions": {{"onRun": {{"command": "echo"}}}}}}}}]
        }}"#).unwrap();
        let (code, stdout, stderr) = run_cli(&["run", f.path().to_str().unwrap()]);
        assert_ne!(code, 0, "should fail with missing params");
        let output = format!("{stdout}{stderr}");
        // Should show the missing params error
        assert!(
            output.contains("missing") || output.contains("Missing"),
            "should mention missing params. output: {output}"
        );
        // Should also show context-aware help with job info
        assert!(
            output.contains("Job: TestJob"),
            "should show job name. output: {output}"
        );
        assert!(
            output.contains("A test job with required parameters"),
            "should show job description. output: {output}"
        );
        assert!(
            output.contains("Job Parameters"),
            "should show parameters section. output: {output}"
        );
        assert!(
            output.contains("RequiredParam1")
                && output.contains("STRING")
                && output.contains("[required]"),
            "should show RequiredParam1 info. output: {output}"
        );
        assert!(
            output.contains("First required parameter"),
            "should show param1 description. output: {output}"
        );
        assert!(
            output.contains("RequiredParam2")
                && output.contains("INT")
                && output.contains("[required]"),
            "should show RequiredParam2 info. output: {output}"
        );
        assert!(
            output.contains("Second required parameter"),
            "should show param2 description. output: {output}"
        );
    }
}

// ============================================================
// Group 9 continued: EXPR extension in env
// ============================================================

mod run_with_env_expr {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_run_job_with_env_expr_extension() {
        let mut job_f = NamedTempFile::with_suffix(".yaml").unwrap();
        write!(
            job_f,
            r#"specificationVersion: jobtemplate-2023-09
name: TestJob
extensions:
  - FEATURE_BUNDLE_1
steps:
  - name: TestStep
    bash:
      script: echo "Task ran"
"#
        )
        .unwrap();

        let mut env_f = NamedTempFile::with_suffix(".yaml").unwrap();
        write!(
            env_f,
            r#"specificationVersion: environment-2023-09
extensions:
  - EXPR
environment:
  name: ExprEnv
  script:
    actions:
      onEnter:
        command: echo
        args:
          - "Enter {{{{ 1 + 2 }}}}"
      onExit:
        command: echo
        args:
          - "Exit"
"#
        )
        .unwrap();

        let (code, stdout, stderr) = run_cli(&[
            "run",
            job_f.path().to_str().unwrap(),
            "--environment",
            env_f.path().to_str().unwrap(),
        ]);
        assert_eq!(code, 0, "stderr: {stderr}");
        assert!(
            stdout.contains("Enter 3"),
            "EXPR should evaluate. stdout: {stdout}"
        );
    }
}

mod new_cli_options {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_run_task_param_explicit() {
        // --task-param should run a single task with explicit values
        let tdir = templates_dir();
        let (code, stdout, stderr) = run_cli(&[
            "run",
            tdir.join("job_with_test_steps.yaml").to_str().unwrap(),
            "--step",
            "TaskParamStep",
            "--task-param",
            "TaskNumber=1",
            "--task-param",
            "TaskMessage=Hi!",
            "--extensions",
            "",
        ]);
        assert_eq!(code, 0, "stderr: {stderr}");
        assert!(stdout.contains("Running Task"), "stdout: {stdout}");
    }

    #[test]
    fn test_run_tasks_inline_json() {
        // --tasks with inline JSON array
        let tdir = templates_dir();
        let (code, stdout, stderr) = run_cli(&[
            "run",
            tdir.join("job_with_test_steps.yaml").to_str().unwrap(),
            "--step",
            "TaskParamStep",
            "--tasks",
            r#"[{"TaskNumber": "1", "TaskMessage": "Hi!"}]"#,
            "--extensions",
            "",
        ]);
        assert_eq!(code, 0, "stderr: {stderr}");
        assert!(stdout.contains("Running Task"), "stdout: {stdout}");
    }

    #[test]
    fn test_run_verbose_flag() {
        // --verbose should be accepted without error
        let tdir = templates_dir();
        let (code, _stdout, stderr) = run_cli(&[
            "run",
            tdir.join("job_with_test_steps.yaml").to_str().unwrap(),
            "--step",
            "BareStep",
            "--verbose",
            "--extensions",
            "",
        ]);
        assert_eq!(code, 0, "stderr: {stderr}");
    }

    #[test]
    fn test_run_timestamp_format_utc() {
        let tdir = templates_dir();
        let (code, stdout, stderr) = run_cli(&[
            "run",
            tdir.join("job_with_test_steps.yaml").to_str().unwrap(),
            "--step",
            "BareStep",
            "--timestamp-format",
            "utc",
            "--extensions",
            "",
        ]);
        assert_eq!(code, 0, "stderr: {stderr}");
        // UTC timestamps contain 'T' and 'Z'
        assert!(
            stdout.contains('T') && stdout.contains('Z'),
            "Expected UTC timestamps, stdout: {stdout}"
        );
    }

    #[test]
    fn test_run_timestamp_format_local() {
        let tdir = templates_dir();
        let (code, stdout, stderr) = run_cli(&[
            "run",
            tdir.join("job_with_test_steps.yaml").to_str().unwrap(),
            "--step",
            "BareStep",
            "--timestamp-format",
            "local",
            "--extensions",
            "",
        ]);
        assert_eq!(code, 0, "stderr: {stderr}");
        // Local timestamps contain 'T' but not 'Z'
        assert!(
            stdout.contains('T'),
            "Expected local timestamps, stdout: {stdout}"
        );
    }

    #[test]
    fn test_run_output_json() {
        let tdir = templates_dir();
        let (code, stdout, stderr) = run_cli(&[
            "run",
            tdir.join("job_with_test_steps.yaml").to_str().unwrap(),
            "--step",
            "BareStep",
            "--output",
            "json",
            "--extensions",
            "",
        ]);
        assert_eq!(code, 0, "stderr: {stderr}");
        assert!(
            stdout.contains(r#""status": "success""#),
            "Expected JSON output, stdout: {stdout}"
        );
    }

    #[test]
    fn test_run_output_yaml() {
        let tdir = templates_dir();
        let (code, stdout, stderr) = run_cli(&[
            "run",
            tdir.join("job_with_test_steps.yaml").to_str().unwrap(),
            "--step",
            "BareStep",
            "--output",
            "yaml",
            "--extensions",
            "",
        ]);
        assert_eq!(code, 0, "stderr: {stderr}");
        assert!(stdout.contains("status: success"), "stdout: {stdout}");
        assert!(stdout.contains("job_name:"), "stdout: {stdout}");
        assert!(stdout.contains("duration:"), "stdout: {stdout}");
        assert!(stdout.contains("chunks_run: 1"), "stdout: {stdout}");
    }

    #[test]
    fn test_run_auto_select_single_step() {
        // Job with one step should auto-select it without --step
        let mut f = NamedTempFile::with_suffix(".yaml").unwrap();
        write!(
            f,
            r#"specificationVersion: "jobtemplate-2023-09"
name: SingleStepJob
steps:
  - name: OnlyStep
    script:
      actions:
        onRun:
          command: echo
          args: ["auto-selected"]
"#
        )
        .unwrap();
        let (code, stdout, stderr) = run_cli(&["run", f.path().to_str().unwrap()]);
        assert_eq!(code, 0, "stderr: {stderr}");
        assert!(stdout.contains("auto-selected"), "stdout: {stdout}");
    }

    #[test]
    fn test_run_task_param_conflicts_with_maximum_tasks() {
        let tdir = templates_dir();
        let (code, _stdout, _stderr) = run_cli(&[
            "run",
            tdir.join("job_with_test_steps.yaml").to_str().unwrap(),
            "--step",
            "TaskParamStep",
            "--task-param",
            "TaskNumber=1",
            "--maximum-tasks",
            "5",
            "--extensions",
            "",
        ]);
        assert_ne!(code, 0, "conflicting options should fail");
    }

    #[test]
    fn test_run_inline_json_params() {
        // -p with inline JSON
        let mut f = NamedTempFile::with_suffix(".yaml").unwrap();
        write!(
            f,
            r#"specificationVersion: "jobtemplate-2023-09"
name: test
parameterDefinitions:
  - name: Greeting
    type: STRING
steps:
  - name: s1
    script:
      actions:
        onRun:
          command: echo
          args: ["{{{{Param.Greeting}}}}"]
"#
        )
        .unwrap();
        let (code, stdout, stderr) = run_cli(&[
            "run",
            f.path().to_str().unwrap(),
            "-p",
            r#"{"Greeting": "HelloJSON"}"#,
        ]);
        assert_eq!(code, 0, "stderr: {stderr}");
        assert!(stdout.contains("HelloJSON"), "stdout: {stdout}");
    }

    #[test]
    fn test_run_preserve_shows_working_dir() {
        let tdir = templates_dir();
        let (code, stdout, stderr) = run_cli(&[
            "run",
            tdir.join("job_with_test_steps.yaml").to_str().unwrap(),
            "--step",
            "BareStep",
            "--preserve",
            "--extensions",
            "",
        ]);
        assert_eq!(code, 0, "stderr: {stderr}");
        assert!(
            stdout.contains("Working directory preserved at:"),
            "stdout: {stdout}"
        );
    }

    #[test]
    fn test_run_task_params_require_step_for_multistep() {
        // --task-param without --step on a multi-step job should error
        let tdir = templates_dir();
        let (code, _stdout, stderr) = run_cli(&[
            "run",
            tdir.join("job_with_test_steps.yaml").to_str().unwrap(),
            "--task-param",
            "TaskNumber=1",
            "--extensions",
            "",
        ]);
        assert_ne!(code, 0, "should fail without --step on multi-step job");
        assert!(
            stderr.contains("requires a specified step") || stderr.contains("single step"),
            "stderr: {stderr}"
        );
    }

    #[test]
    fn test_task_param_value_with_equals() {
        // TaskParamStep has TaskNumber(INT) and TaskMessage(STRING)
        // Pass a value containing '=' to verify split_once behavior
        let tdir = templates_dir();
        let (code, stdout, stderr) = run_cli(&[
            "run",
            tdir.join("job_with_test_steps.yaml").to_str().unwrap(),
            "--step",
            "TaskParamStep",
            "-t",
            "TaskNumber=1",
            "-t",
            "TaskMessage=One=Two",
            "--extensions",
            "",
        ]);
        assert_eq!(code, 0, "stderr: {stderr}");
        assert!(stdout.contains("One=Two"), "stdout: {stdout}");
    }

    #[test]
    fn test_task_param_bad_format() {
        let tdir = templates_dir();
        let (code, _stdout, stderr) = run_cli(&[
            "run",
            tdir.join("job_with_test_steps.yaml").to_str().unwrap(),
            "--step",
            "TaskParamStep",
            "-t",
            "NoEquals",
            "--extensions",
            "",
        ]);
        assert_ne!(code, 0, "bad format should fail");
        assert!(
            stderr.contains("defined incorrectly") || stderr.contains("format"),
            "stderr: {stderr}"
        );
    }

    #[test]
    fn test_task_param_duplicate() {
        let tdir = templates_dir();
        let (code, _stdout, stderr) = run_cli(&[
            "run",
            tdir.join("job_with_test_steps.yaml").to_str().unwrap(),
            "--step",
            "TaskParamStep",
            "-t",
            "TaskNumber=1",
            "-t",
            "TaskNumber=2",
            "--extensions",
            "",
        ]);
        assert_ne!(code, 0, "duplicate should fail");
        assert!(
            stderr.contains("more than once") || stderr.contains("duplicate"),
            "stderr: {stderr}"
        );
    }

    #[test]
    fn test_tasks_from_json_file() {
        let mut f = NamedTempFile::with_suffix(".json").unwrap();
        write!(f, r#"[{{"TaskNumber": "1", "TaskMessage": "Hi!"}}]"#).unwrap();
        let tasks_arg = format!("file://{}", f.path().display());
        let tdir = templates_dir();
        let (code, stdout, stderr) = run_cli(&[
            "run",
            tdir.join("job_with_test_steps.yaml").to_str().unwrap(),
            "--step",
            "TaskParamStep",
            "--tasks",
            &tasks_arg,
            "--extensions",
            "",
        ]);
        assert_eq!(code, 0, "stderr: {stderr}");
        assert!(stdout.contains("Running Task"), "stdout: {stdout}");
    }

    #[test]
    fn test_tasks_from_yaml_file() {
        let mut f = NamedTempFile::with_suffix(".yaml").unwrap();
        write!(f, "- TaskNumber: \"1\"\n  TaskMessage: \"Hi!\"\n").unwrap();
        let tasks_arg = format!("file://{}", f.path().display());
        let tdir = templates_dir();
        let (code, stdout, stderr) = run_cli(&[
            "run",
            tdir.join("job_with_test_steps.yaml").to_str().unwrap(),
            "--step",
            "TaskParamStep",
            "--tasks",
            &tasks_arg,
            "--extensions",
            "",
        ]);
        assert_eq!(code, 0, "stderr: {stderr}");
        assert!(stdout.contains("Running Task"), "stdout: {stdout}");
    }

    #[test]
    fn test_tasks_not_a_list() {
        let tdir = templates_dir();
        let (code, _stdout, stderr) = run_cli(&[
            "run",
            tdir.join("job_with_test_steps.yaml").to_str().unwrap(),
            "--step",
            "TaskParamStep",
            "--tasks",
            r#"{"TaskNumber": "1"}"#,
            "--extensions",
            "",
        ]);
        assert_ne!(code, 0, "non-list should fail");
        assert!(
            stderr.contains("list") || stderr.contains("must be"),
            "stderr: {stderr}"
        );
    }

    #[test]
    fn test_tasks_not_list_of_dicts() {
        let tdir = templates_dir();
        let (code, _stdout, stderr) = run_cli(&[
            "run",
            tdir.join("job_with_test_steps.yaml").to_str().unwrap(),
            "--step",
            "TaskParamStep",
            "--tasks",
            "[1, 2, 3]",
            "--extensions",
            "",
        ]);
        assert_ne!(code, 0, "non-dict items should fail");
        assert!(
            stderr.contains("maps") || stderr.contains("must be"),
            "stderr: {stderr}"
        );
    }

    #[test]
    fn test_tasks_numeric_values_coerced() {
        let tdir = templates_dir();
        let (code, stdout, stderr) = run_cli(&[
            "run",
            tdir.join("job_with_test_steps.yaml").to_str().unwrap(),
            "--step",
            "TaskParamStep",
            "--tasks",
            r#"[{"TaskNumber": 1, "TaskMessage": "Hi!"}]"#,
            "--extensions",
            "",
        ]);
        assert_eq!(code, 0, "numeric coercion should succeed. stderr: {stderr}");
        assert!(stdout.contains("Running Task"), "stdout: {stdout}");
    }
}

mod summary_command {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_summary_basic_job() {
        let mut f = NamedTempFile::with_suffix(".yaml").unwrap();
        write!(
            f,
            r#"specificationVersion: "jobtemplate-2023-09"
name: TestJob
steps:
  - name: Step1
    script:
      actions:
        onRun:
          command: echo
          args: ["hello"]
"#
        )
        .unwrap();
        let (code, stdout, stderr) = run_cli(&["summary", f.path().to_str().unwrap()]);
        assert_eq!(code, 0, "stderr: {stderr}");
        assert!(stdout.contains("Summary for 'TestJob'"), "stdout: {stdout}");
        assert!(stdout.contains("Total steps: 1"), "stdout: {stdout}");
        assert!(stdout.contains("Total tasks: 1"), "stdout: {stdout}");
        assert!(
            stdout.contains("'Step1' (1 total Tasks)"),
            "stdout: {stdout}"
        );
    }

    #[test]
    fn test_summary_with_parameters() {
        let mut f = NamedTempFile::with_suffix(".yaml").unwrap();
        write!(
            f,
            r#"specificationVersion: "jobtemplate-2023-09"
name: ParamJob
parameterDefinitions:
  - name: Greeting
    type: STRING
    default: Hello
steps:
  - name: S1
    script:
      actions:
        onRun:
          command: echo
"#
        )
        .unwrap();
        let (code, stdout, stderr) = run_cli(&["summary", f.path().to_str().unwrap()]);
        assert_eq!(code, 0, "stderr: {stderr}");
        assert!(
            stdout.contains("Greeting (STRING): Hello"),
            "stdout: {stdout}"
        );
    }

    #[test]
    fn test_summary_with_task_params() {
        let mut f = NamedTempFile::with_suffix(".yaml").unwrap();
        write!(
            f,
            r#"specificationVersion: "jobtemplate-2023-09"
name: TaskParamJob
steps:
  - name: Render
    parameterSpace:
      taskParameterDefinitions:
        - name: Frame
          type: INT
          range: "1-10"
    script:
      actions:
        onRun:
          command: echo
"#
        )
        .unwrap();
        let (code, stdout, stderr) = run_cli(&["summary", f.path().to_str().unwrap()]);
        assert_eq!(code, 0, "stderr: {stderr}");
        assert!(stdout.contains("10 total Tasks"), "stdout: {stdout}");
        assert!(stdout.contains("Frame (INT)"), "stdout: {stdout}");
    }

    #[test]
    fn test_summary_step_filter() {
        let tdir = templates_dir();
        let (code, stdout, stderr) = run_cli(&[
            "summary",
            tdir.join("job_with_test_steps.yaml").to_str().unwrap(),
            "--step",
            "TaskParamStep",
            "--extensions",
            "",
        ]);
        assert_eq!(code, 0, "stderr: {stderr}");
        assert!(
            stdout.contains("Summary for Step 'TaskParamStep'"),
            "stdout: {stdout}"
        );
        assert!(stdout.contains("Total tasks:"), "stdout: {stdout}");
    }

    #[test]
    fn test_summary_nonexistent_step() {
        let tdir = templates_dir();
        let (code, _stdout, stderr) = run_cli(&[
            "summary",
            tdir.join("job_with_test_steps.yaml").to_str().unwrap(),
            "--step",
            "NoSuchStep",
            "--extensions",
            "",
        ]);
        assert_ne!(code, 0);
        assert!(stderr.contains("does not exist"), "stderr: {stderr}");
    }

    #[test]
    fn test_summary_json_output() {
        let mut f = NamedTempFile::with_suffix(".yaml").unwrap();
        write!(
            f,
            r#"specificationVersion: "jobtemplate-2023-09"
name: JsonJob
steps:
  - name: S1
    script:
      actions:
        onRun:
          command: echo
"#
        )
        .unwrap();
        let (code, stdout, stderr) =
            run_cli(&["summary", f.path().to_str().unwrap(), "--output", "json"]);
        assert_eq!(code, 0, "stderr: {stderr}");
        assert!(
            stdout.contains(r#""status": "success""#),
            "stdout: {stdout}"
        );
        assert!(stdout.contains(r#""name": "JsonJob""#), "stdout: {stdout}");
    }

    #[test]
    fn test_summary_yaml_output() {
        let mut f = NamedTempFile::with_suffix(".yaml").unwrap();
        write!(
            f,
            r#"specificationVersion: "jobtemplate-2023-09"
name: YamlJob
parameterDefinitions:
  - name: Greeting
    type: STRING
    default: Hello
steps:
  - name: S1
    script:
      actions:
        onRun:
          command: echo
"#
        )
        .unwrap();
        let (code, stdout, stderr) =
            run_cli(&["summary", f.path().to_str().unwrap(), "--output", "yaml"]);
        assert_eq!(code, 0, "stderr: {stderr}");
        assert!(stdout.contains("status: success"), "stdout: {stdout}");
        assert!(stdout.contains("name: YamlJob"), "stdout: {stdout}");
        assert!(stdout.contains("total_steps: 1"), "stdout: {stdout}");
        assert!(stdout.contains("total_tasks: 1"), "stdout: {stdout}");
        // Should include structured step data (not just top-level keys)
        assert!(
            stdout.contains("- name: S1"),
            "steps should be structured. stdout: {stdout}"
        );
    }

    #[test]
    fn test_summary_with_dependencies() {
        let tdir = templates_dir();
        let (code, stdout, stderr) = run_cli(&[
            "summary",
            tdir.join("basic_dependency_job.yaml").to_str().unwrap(),
            "-p",
            "J=hello",
            "--extensions",
            "",
        ]);
        assert_eq!(code, 0, "stderr: {stderr}");
        assert!(stdout.contains("dependencies"), "stdout: {stdout}");
    }

    #[test]
    fn test_summary_with_environments() {
        let mut f = NamedTempFile::with_suffix(".yaml").unwrap();
        write!(
            f,
            r#"specificationVersion: "jobtemplate-2023-09"
name: EnvJob
steps:
  - name: S1
    stepEnvironments:
      - name: MyEnv
        description: "A test environment"
        script:
          actions:
            onEnter:
              command: echo
              args: ["entering"]
    script:
      actions:
        onRun:
          command: echo
"#
        )
        .unwrap();
        let (code, stdout, stderr) = run_cli(&["summary", f.path().to_str().unwrap()]);
        assert_eq!(code, 0, "stderr: {stderr}");
        assert!(stdout.contains("Total environments: 1"), "stdout: {stdout}");
        assert!(stdout.contains("MyEnv (from 'S1')"), "stdout: {stdout}");
        assert!(stdout.contains("A test environment"), "stdout: {stdout}");
    }

    #[test]
    fn test_summary_missing_required_param() {
        let mut f = NamedTempFile::with_suffix(".yaml").unwrap();
        write!(
            f,
            r#"specificationVersion: "jobtemplate-2023-09"
name: test
parameterDefinitions:
  - name: Required
    type: STRING
steps:
  - name: S1
    script:
      actions:
        onRun:
          command: echo
"#
        )
        .unwrap();
        let (code, _stdout, stderr) = run_cli(&["summary", f.path().to_str().unwrap()]);
        assert_ne!(code, 0);
        assert!(
            stderr.contains("missing") || stderr.contains("Required"),
            "stderr: {stderr}"
        );
    }

    #[test]
    fn test_summary_step_with_params() {
        let tdir = templates_dir();
        let (code, stdout, stderr) = run_cli(&[
            "summary",
            tdir.join("job_with_test_steps.yaml").to_str().unwrap(),
            "--step",
            "TaskParamStep",
            "--extensions",
            "",
        ]);
        assert_eq!(code, 0, "stderr: {stderr}");
        assert!(stdout.contains("Total tasks: 9"), "stdout: {stdout}");
        assert!(
            stdout.contains("Total task parameters: 2"),
            "stdout: {stdout}"
        );
    }

    #[test]
    fn test_summary_step_bare() {
        let tdir = templates_dir();
        let (code, stdout, stderr) = run_cli(&[
            "summary",
            tdir.join("job_with_test_steps.yaml").to_str().unwrap(),
            "--step",
            "BareStep",
            "--extensions",
            "",
        ]);
        assert_eq!(code, 0, "stderr: {stderr}");
        assert!(stdout.contains("Total tasks: 1"), "stdout: {stdout}");
        assert!(stdout.contains("Total environments: 0"), "stdout: {stdout}");
    }

    #[test]
    fn test_summary_with_combination_expr() {
        let mut f = NamedTempFile::with_suffix(".yaml").unwrap();
        write!(
            f,
            r#"specificationVersion: "jobtemplate-2023-09"
name: ComboJob
steps:
  - name: ComboStep
    parameterSpace:
      taskParameterDefinitions:
        - name: A
          type: INT
          range: "1-3"
        - name: B
          type: INT
          range: "1-3"
        - name: C
          type: INT
          range: "1-2"
      combination: "(A, B) * C"
    script:
      actions:
        onRun:
          command: echo
"#
        )
        .unwrap();
        let (code, stdout, stderr) = run_cli(&["summary", f.path().to_str().unwrap()]);
        assert_eq!(code, 0, "stderr: {stderr}");
        assert!(stdout.contains("ComboStep"), "stdout: {stdout}");
        assert!(stdout.contains("A (INT)"), "stdout: {stdout}");
        assert!(stdout.contains("B (INT)"), "stdout: {stdout}");
        assert!(stdout.contains("C (INT)"), "stdout: {stdout}");
    }

    #[test]
    fn test_summary_job_with_root_envs() {
        let mut f = NamedTempFile::with_suffix(".yaml").unwrap();
        write!(
            f,
            r#"specificationVersion: "jobtemplate-2023-09"
name: RootEnvJob
jobEnvironments:
  - name: GlobalEnv
    variables:
      FOO: bar
steps:
  - name: S1
    script:
      actions:
        onRun:
          command: echo
"#
        )
        .unwrap();
        let (code, stdout, stderr) = run_cli(&["summary", f.path().to_str().unwrap()]);
        assert_eq!(code, 0, "stderr: {stderr}");
        assert!(stdout.contains("Total environments: 1"), "stdout: {stdout}");
        assert!(stdout.contains("GlobalEnv"), "stdout: {stdout}");
    }

    #[test]
    fn test_summary_step_and_params() {
        let tdir = templates_dir();
        let (code, stdout, stderr) = run_cli(&[
            "summary",
            tdir.join("job_with_test_steps.yaml").to_str().unwrap(),
            "-p",
            "Message=CustomMsg",
            "--step",
            "TaskParamStep",
            "--extensions",
            "",
        ]);
        assert_eq!(code, 0, "stderr: {stderr}");
        assert!(stdout.contains("TaskParamStep"), "stdout: {stdout}");
        assert!(stdout.contains("Total tasks: 9"), "stdout: {stdout}");
    }
}

// ============================================================
// Group: All-steps topological sort
// ============================================================

mod all_steps_topo_sort {
    use super::*;

    #[test]
    fn test_all_steps_respects_dependency_order() {
        // Template defines StepC (depends on StepB), StepB (depends on StepA), StepA
        // Without topo sort, they'd run in definition order: C, B, A (wrong).
        // With topo sort, they must run: A, B, C.
        let tdir = templates_dir();
        let (code, stdout, stderr) = run_cli(&[
            "run",
            tdir.join("reverse_dependency_order.yaml").to_str().unwrap(),
            "--extensions",
            "",
        ]);
        assert_eq!(code, 0, "stderr: {stderr}");
        let a_pos = stdout
            .find("Running step 'StepA'")
            .expect("StepA should run");
        let b_pos = stdout
            .find("Running step 'StepB'")
            .expect("StepB should run");
        let c_pos = stdout
            .find("Running step 'StepC'")
            .expect("StepC should run");
        assert!(
            a_pos < b_pos,
            "StepA must run before StepB. stdout: {stdout}"
        );
        assert!(
            b_pos < c_pos,
            "StepB must run before StepC. stdout: {stdout}"
        );
    }
}

// ============================================================
// Python CLI Compatibility Tests
// ============================================================

mod python_compat {
    use super::*;

    #[test]
    fn test_job_param_long_flag_accepted() {
        // Python uses --job-param; Rust should accept it too
        let tdir = templates_dir();
        let (code, _stdout, stderr) = run_cli(&[
            "run",
            tdir.join("basic.yaml").to_str().unwrap(),
            "--step",
            "First",
            "--job-param",
            "J=value1",
            "--extensions",
            "",
        ]);
        assert_eq!(code, 0, "stderr: {stderr}");
    }

    #[test]
    fn test_job_param_short_flag_p() {
        // Both Python and Rust use -p
        let tdir = templates_dir();
        let (code, _stdout, stderr) = run_cli(&[
            "run",
            tdir.join("basic.yaml").to_str().unwrap(),
            "--step",
            "First",
            "-p",
            "J=value1",
            "--extensions",
            "",
        ]);
        assert_eq!(code, 0, "stderr: {stderr}");
    }

    #[test]
    fn test_summary_job_param_long_flag() {
        let tdir = templates_dir();
        let (code, _stdout, stderr) = run_cli(&[
            "summary",
            tdir.join("basic.yaml").to_str().unwrap(),
            "--job-param",
            "J=value1",
            "--extensions",
            "",
        ]);
        assert_eq!(code, 0, "stderr: {stderr}");
    }

    #[test]
    fn test_task_param_tp_short_flag() {
        // Python uses -tp; Rust should accept it
        let tdir = templates_dir();
        let (_code, _stdout, stderr) = run_cli(&[
            "run",
            tdir.join("job_with_test_steps.yaml").to_str().unwrap(),
            "--step",
            "BareStep",
            "-tp",
            "Foo=bar",
            "--extensions",
            "",
        ]);
        // BareStep has no task params, so this may fail for a different reason,
        // but it should NOT fail with "unrecognized argument -tp"
        assert!(
            !stderr.contains("unrecognized") && !stderr.contains("unexpected argument"),
            "-tp should be recognized as task-param flag. stderr: {stderr}"
        );
    }

    #[test]
    fn test_run_json_output_uses_chunks_run() {
        // Python uses "chunks_run" in JSON output; Rust should match
        let tdir = templates_dir();
        let (code, stdout, stderr) = run_cli(&[
            "run",
            tdir.join("job_with_test_steps.yaml").to_str().unwrap(),
            "--step",
            "BareStep",
            "--output",
            "json",
            "--extensions",
            "",
        ]);
        assert_eq!(code, 0, "stderr: {stderr}");
        assert!(
            stdout.contains("\"chunks_run\""),
            "JSON output should use 'chunks_run' to match Python CLI. stdout: {stdout}"
        );
    }

    #[test]
    fn test_run_yaml_output_uses_chunks_run() {
        // Python uses "chunks_run" in YAML output; Rust should match
        let tdir = templates_dir();
        let (code, stdout, stderr) = run_cli(&[
            "run",
            tdir.join("job_with_test_steps.yaml").to_str().unwrap(),
            "--step",
            "BareStep",
            "--output",
            "yaml",
            "--extensions",
            "",
        ]);
        assert_eq!(code, 0, "stderr: {stderr}");
        assert!(
            stdout.contains("chunks_run:"),
            "YAML output should use 'chunks_run' to match Python CLI. stdout: {stdout}"
        );
    }

    #[test]
    fn test_run_human_output_uses_chunks_run() {
        // Python uses "Chunks run:" in human output; Rust should match
        let tdir = templates_dir();
        let (code, stdout, stderr) = run_cli(&[
            "run",
            tdir.join("job_with_test_steps.yaml").to_str().unwrap(),
            "--step",
            "BareStep",
            "--extensions",
            "",
        ]);
        assert_eq!(code, 0, "stderr: {stderr}");
        assert!(
            stdout.contains("Chunks run:"),
            "Human output should say 'Chunks run:' to match Python CLI. stdout: {stdout}"
        );
    }

    #[test]
    fn test_context_help_shows_job_param() {
        // Context-aware help should show --job-param, not --parameter
        let tdir = templates_dir();
        let (code, stdout, _stderr) = run_cli(&[
            "run",
            tdir.join("basic.yaml").to_str().unwrap(),
            "--extensions",
            "",
            "-h",
        ]);
        assert_eq!(code, 0);
        assert!(
            stdout.contains("--job-param"),
            "Context help should show --job-param. stdout: {stdout}"
        );
    }
}
