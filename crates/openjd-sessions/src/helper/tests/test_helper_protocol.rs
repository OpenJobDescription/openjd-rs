// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Integration tests for the helper binary protocol.
//! These run the helper as the current user (no cross-user setup needed).

use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// A token that matches the 22-character URL-safe alphabet used by the real
/// session. Content doesn't matter for most tests — only length and charset do.
const TEST_TOKEN: &str = "AbCdEfGhIjKlMnOpQrStUv";
/// A second valid-looking token that differs from TEST_TOKEN, for
/// "wrong-token" tests.
const WRONG_TOKEN: &str = "ZyXwVuTsRqPoNmLkJiHgFe";

fn helper_path() -> std::path::PathBuf {
    // The helper binary is built alongside the tests
    let mut path = std::env::current_exe().unwrap();
    path.pop(); // remove test binary name
    path.pop(); // remove "deps"
    path.push("openjd_helper");
    if cfg!(windows) {
        path.set_extension("exe");
    }
    path
}

/// Spawn the helper with the given argv (beyond the binary path).
fn spawn_helper_with_args(extra_args: &[&str]) -> std::process::Child {
    Command::new(helper_path())
        .args(extra_args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("Failed to spawn helper")
}

/// Spawn the helper with a valid `--auth-token` argument.
fn spawn_helper_with_token(token: &str) -> std::process::Child {
    spawn_helper_with_args(&["--auth-token", token])
}

fn send_cmd(stdin: &mut impl Write, cmd: &str) {
    writeln!(stdin, "{}", cmd).unwrap();
    stdin.flush().unwrap();
}

fn read_line(reader: &mut BufReader<std::process::ChildStdout>) -> String {
    let mut line = String::new();
    reader.read_line(&mut line).unwrap();
    line
}

fn echo_cmd_json(token: &str) -> String {
    let cwd = std::env::current_dir()
        .unwrap()
        .to_string_lossy()
        .to_string();
    if cfg!(windows) {
        format!(
            r#"{{"token":"{}","command":"cmd","args":["/C","echo hello"],"cwd":"{}"}}"#,
            token,
            cwd.replace('\\', "\\\\"),
        )
    } else {
        format!(
            r#"{{"token":"{}","command":"echo","args":["hello"],"cwd":"{}"}}"#,
            token, cwd,
        )
    }
}

fn sleep_cmd_json(token: &str) -> String {
    let cwd = std::env::current_dir()
        .unwrap()
        .to_string_lossy()
        .to_string();
    if cfg!(windows) {
        format!(
            r#"{{"token":"{}","command":"powershell","args":["-Command","Start-Sleep 300"],"cwd":"{}"}}"#,
            token,
            cwd.replace('\\', "\\\\"),
        )
    } else {
        format!(
            r#"{{"token":"{}","command":"sleep","args":["300"],"cwd":"{}"}}"#,
            token, cwd
        )
    }
}

fn shutdown_json(token: &str) -> String {
    format!(r#"{{"token":"{}","shutdown":true}}"#, token)
}

#[test]
fn test_helper_echo_command() {
    let mut child = spawn_helper_with_token(TEST_TOKEN);
    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = BufReader::new(child.stdout.take().unwrap());

    send_cmd(&mut stdin, &echo_cmd_json(TEST_TOKEN));

    // Read pid response
    let pid_line = read_line(&mut stdout);
    let pid_json: serde_json::Value = serde_json::from_str(&pid_line).unwrap();
    assert!(
        pid_json.get("pid").is_some(),
        "Expected pid response, got: {pid_line}"
    );

    // Read output line
    let out_line = read_line(&mut stdout);
    let out_json: serde_json::Value = serde_json::from_str(&out_line).unwrap();
    let output = out_json.get("out").and_then(|v| v.as_str()).unwrap_or("");
    assert!(
        output.contains("hello"),
        "Expected 'hello' in output, got: {output}"
    );

    // Read exit response
    let exit_line = read_line(&mut stdout);
    let exit_json: serde_json::Value = serde_json::from_str(&exit_line).unwrap();
    assert_eq!(exit_json.get("exited").and_then(|v| v.as_i64()), Some(0));

    // Shutdown
    send_cmd(&mut stdin, &shutdown_json(TEST_TOKEN));
    child.wait().unwrap();
}

#[test]
fn test_helper_cancel_terminates_quickly() {
    let mut child = spawn_helper_with_token(TEST_TOKEN);
    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = BufReader::new(child.stdout.take().unwrap());

    send_cmd(&mut stdin, &sleep_cmd_json(TEST_TOKEN));

    // Read pid response
    let pid_line = read_line(&mut stdout);
    let pid_json: serde_json::Value = serde_json::from_str(&pid_line).unwrap();
    assert!(
        pid_json.get("pid").is_some(),
        "Expected pid response, got: {pid_line}"
    );

    // Wait a moment for the process to start
    std::thread::sleep(Duration::from_millis(500));

    // Send terminate cancel
    let cancel_cmd = format!(r#"{{"token":"{}","cancel":"TERMINATE"}}"#, TEST_TOKEN);
    let start = Instant::now();
    send_cmd(&mut stdin, &cancel_cmd);

    // Read exit response — should come quickly
    let exit_line = read_line(&mut stdout);
    let elapsed = start.elapsed();
    let exit_json: serde_json::Value = serde_json::from_str(&exit_line).unwrap();
    assert!(
        exit_json.get("exited").is_some(),
        "Expected exited response, got: {exit_line}"
    );
    assert!(
        elapsed < Duration::from_secs(5),
        "Cancel took {:?}, expected < 5s",
        elapsed
    );

    // Shutdown
    send_cmd(&mut stdin, &shutdown_json(TEST_TOKEN));
    child.wait().unwrap();
}

#[test]
fn test_helper_nonexistent_command() {
    let mut child = spawn_helper_with_token(TEST_TOKEN);
    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = BufReader::new(child.stdout.take().unwrap());

    let cwd = std::env::current_dir()
        .unwrap()
        .to_string_lossy()
        .to_string();
    let cmd = format!(
        r#"{{"token":"{}","command":"nonexistentcommand12345","args":[],"cwd":"{}"}}"#,
        TEST_TOKEN,
        cwd.replace('\\', "\\\\")
    );
    send_cmd(&mut stdin, &cmd);

    // Should get an error response
    let resp_line = read_line(&mut stdout);
    let resp_json: serde_json::Value = serde_json::from_str(&resp_line).unwrap();
    assert!(
        resp_json.get("error").is_some(),
        "Expected error response for nonexistent command, got: {resp_line}"
    );

    send_cmd(&mut stdin, &shutdown_json(TEST_TOKEN));
    child.wait().unwrap();
}

// ── New token-focused tests ──

#[test]
fn test_helper_rejects_missing_auth_token_cli() {
    // Spawn with no --auth-token argument at all. Helper should exit non-zero
    // and produce no JSON on stdout (it prints a diagnostic on stderr before
    // exiting).
    let mut child = spawn_helper_with_args(&[]);
    // Close stdin so the helper can't block waiting for commands.
    drop(child.stdin.take());

    let status = child
        .wait_timeout_or_kill(Duration::from_secs(5))
        .expect("helper should exit quickly on missing --auth-token");
    assert!(
        !status.success(),
        "helper should exit non-zero on missing --auth-token"
    );

    // stdout should be empty (no JSON produced).
    let mut stdout_buf = String::new();
    child
        .stdout
        .take()
        .unwrap()
        .read_to_string(&mut stdout_buf)
        .unwrap();
    assert!(
        stdout_buf.trim().is_empty(),
        "helper stdout should be empty on missing --auth-token, got: {stdout_buf:?}"
    );
}

#[test]
fn test_helper_rejects_malformed_auth_token() {
    // --auth-token argument present but with a value of the wrong length.
    // Helper should exit non-zero before reading stdin.
    let mut child = spawn_helper_with_args(&["--auth-token", "too-short"]);
    drop(child.stdin.take());

    let status = child
        .wait_timeout_or_kill(Duration::from_secs(5))
        .expect("helper should exit quickly on malformed --auth-token");
    assert!(
        !status.success(),
        "helper should exit non-zero on malformed --auth-token"
    );
}

#[test]
fn test_helper_rejects_missing_token_field() {
    // Valid CLI token, but the run command omits the "token" field.
    // Helper should emit {"error":"invalid token"} and stay alive.
    let mut child = spawn_helper_with_token(TEST_TOKEN);
    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = BufReader::new(child.stdout.take().unwrap());

    let cwd = std::env::current_dir()
        .unwrap()
        .to_string_lossy()
        .to_string();
    let bad_cmd = if cfg!(windows) {
        format!(
            r#"{{"command":"cmd","args":["/C","echo hello"],"cwd":"{}"}}"#,
            cwd.replace('\\', "\\\\"),
        )
    } else {
        format!(r#"{{"command":"echo","args":["hello"],"cwd":"{}"}}"#, cwd)
    };
    send_cmd(&mut stdin, &bad_cmd);

    let resp_line = read_line(&mut stdout);
    let resp_json: serde_json::Value = serde_json::from_str(&resp_line).unwrap();
    assert_eq!(
        resp_json.get("error").and_then(|v| v.as_str()),
        Some("invalid token"),
        "expected {{\"error\":\"invalid token\"}}, got: {resp_line}",
    );

    // Helper should still be alive and accept a correct-token request next.
    send_cmd(&mut stdin, &echo_cmd_json(TEST_TOKEN));
    let pid_line = read_line(&mut stdout);
    let pid_json: serde_json::Value = serde_json::from_str(&pid_line).unwrap();
    assert!(
        pid_json.get("pid").is_some(),
        "Expected pid response after recovery, got: {pid_line}"
    );
    // Drain echo output + exit
    let _out = read_line(&mut stdout);
    let exit_line = read_line(&mut stdout);
    let exit_json: serde_json::Value = serde_json::from_str(&exit_line).unwrap();
    assert_eq!(exit_json.get("exited").and_then(|v| v.as_i64()), Some(0));

    send_cmd(&mut stdin, &shutdown_json(TEST_TOKEN));
    child.wait().unwrap();
}

#[test]
fn test_helper_rejects_wrong_token() {
    // Valid CLI token, but the run command carries a different token.
    let mut child = spawn_helper_with_token(TEST_TOKEN);
    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = BufReader::new(child.stdout.take().unwrap());

    send_cmd(&mut stdin, &echo_cmd_json(WRONG_TOKEN));

    let resp_line = read_line(&mut stdout);
    let resp_json: serde_json::Value = serde_json::from_str(&resp_line).unwrap();
    assert_eq!(
        resp_json.get("error").and_then(|v| v.as_str()),
        Some("invalid token"),
        "expected {{\"error\":\"invalid token\"}}, got: {resp_line}",
    );

    // Follow up with a correct-token request; helper must still be alive.
    send_cmd(&mut stdin, &echo_cmd_json(TEST_TOKEN));
    let pid_line = read_line(&mut stdout);
    assert!(serde_json::from_str::<serde_json::Value>(&pid_line)
        .unwrap()
        .get("pid")
        .is_some());
    let _out = read_line(&mut stdout);
    let exit_line = read_line(&mut stdout);
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&exit_line)
            .unwrap()
            .get("exited")
            .and_then(|v| v.as_i64()),
        Some(0),
    );

    send_cmd(&mut stdin, &shutdown_json(TEST_TOKEN));
    child.wait().unwrap();
}

#[test]
fn test_helper_cancel_rejects_wrong_token() {
    // Start a long-running command, send a cancel with the wrong token
    // (which must NOT kill the child), then cancel with the right token
    // and verify the child exits.
    let mut child = spawn_helper_with_token(TEST_TOKEN);
    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = BufReader::new(child.stdout.take().unwrap());

    send_cmd(&mut stdin, &sleep_cmd_json(TEST_TOKEN));

    let pid_line = read_line(&mut stdout);
    assert!(serde_json::from_str::<serde_json::Value>(&pid_line)
        .unwrap()
        .get("pid")
        .is_some());

    std::thread::sleep(Duration::from_millis(500));

    // Cancel with the WRONG token.
    send_cmd(
        &mut stdin,
        &format!(r#"{{"token":"{}","cancel":"TERMINATE"}}"#, WRONG_TOKEN),
    );

    // Expect {"error":"invalid token"} and the child is still running.
    let err_line = read_line(&mut stdout);
    let err_json: serde_json::Value = serde_json::from_str(&err_line).unwrap();
    assert_eq!(
        err_json.get("error").and_then(|v| v.as_str()),
        Some("invalid token"),
        "wrong-token cancel should produce invalid token error, got: {err_line}",
    );

    // Now cancel with the correct token.
    let start = Instant::now();
    send_cmd(
        &mut stdin,
        &format!(r#"{{"token":"{}","cancel":"TERMINATE"}}"#, TEST_TOKEN),
    );

    let exit_line = read_line(&mut stdout);
    let exit_json: serde_json::Value = serde_json::from_str(&exit_line).unwrap();
    assert!(
        exit_json.get("exited").is_some(),
        "Expected exited after correct-token cancel, got: {exit_line}",
    );
    assert!(
        start.elapsed() < Duration::from_secs(5),
        "Correct-token cancel took too long",
    );

    send_cmd(&mut stdin, &shutdown_json(TEST_TOKEN));
    child.wait().unwrap();
}

#[test]
fn test_helper_shutdown_requires_token() {
    // A shutdown object with the wrong token must NOT stop the helper.
    // A shutdown object with the right token must.
    let mut child = spawn_helper_with_token(TEST_TOKEN);
    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = BufReader::new(child.stdout.take().unwrap());

    // Wrong-token shutdown → invalid-token error, helper stays alive.
    send_cmd(&mut stdin, &shutdown_json(WRONG_TOKEN));
    let err_line = read_line(&mut stdout);
    let err_json: serde_json::Value = serde_json::from_str(&err_line).unwrap();
    assert_eq!(
        err_json.get("error").and_then(|v| v.as_str()),
        Some("invalid token"),
        "wrong-token shutdown should produce invalid token error, got: {err_line}",
    );

    // Now issue a correct-token echo and verify the helper is still serving.
    send_cmd(&mut stdin, &echo_cmd_json(TEST_TOKEN));
    let pid_line = read_line(&mut stdout);
    assert!(serde_json::from_str::<serde_json::Value>(&pid_line)
        .unwrap()
        .get("pid")
        .is_some());
    let _out = read_line(&mut stdout);
    let _exit = read_line(&mut stdout);

    // Correct-token shutdown → process exits.
    send_cmd(&mut stdin, &shutdown_json(TEST_TOKEN));
    let status = child
        .wait_timeout_or_kill(Duration::from_secs(5))
        .expect("helper should exit on correct-token shutdown");
    assert!(status.success(), "helper should exit cleanly on shutdown");
}

// ── A tiny wait-with-timeout helper that avoids pulling in wait-timeout crate ──

trait ChildWaitExt {
    fn wait_timeout_or_kill(&mut self, timeout: Duration) -> Option<std::process::ExitStatus>;
}

impl ChildWaitExt for std::process::Child {
    fn wait_timeout_or_kill(&mut self, timeout: Duration) -> Option<std::process::ExitStatus> {
        let start = Instant::now();
        loop {
            match self.try_wait() {
                Ok(Some(status)) => return Some(status),
                Ok(None) => {
                    if start.elapsed() >= timeout {
                        let _ = self.kill();
                        let _ = self.wait();
                        return None;
                    }
                    std::thread::sleep(Duration::from_millis(50));
                }
                Err(_) => return None,
            }
        }
    }
}
