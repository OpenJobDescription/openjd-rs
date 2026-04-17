// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Integration tests for the helper binary protocol.
//! These run the helper as the current user (no cross-user setup needed).

use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

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

fn spawn_helper() -> std::process::Child {
    Command::new(helper_path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("Failed to spawn helper")
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

#[test]
fn test_helper_echo_command() {
    let mut child = spawn_helper();
    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = BufReader::new(child.stdout.take().unwrap());

    // Send a simple echo command
    let cwd = std::env::current_dir().unwrap().to_string_lossy().to_string();
    let cmd = if cfg!(windows) {
        format!(r#"{{"command":"cmd","args":["/C","echo hello"],"cwd":"{}"}}"#, cwd.replace('\\', "\\\\"))
    } else {
        format!(r#"{{"command":"echo","args":["hello"],"cwd":"{}"}}"#, cwd)
    };
    send_cmd(&mut stdin, &cmd);

    // Read pid response
    let pid_line = read_line(&mut stdout);
    let pid_json: serde_json::Value = serde_json::from_str(&pid_line).unwrap();
    assert!(pid_json.get("pid").is_some(), "Expected pid response, got: {pid_line}");

    // Read output line
    let out_line = read_line(&mut stdout);
    let out_json: serde_json::Value = serde_json::from_str(&out_line).unwrap();
    let output = out_json.get("out").and_then(|v| v.as_str()).unwrap_or("");
    assert!(output.contains("hello"), "Expected 'hello' in output, got: {output}");

    // Read exit response
    let exit_line = read_line(&mut stdout);
    let exit_json: serde_json::Value = serde_json::from_str(&exit_line).unwrap();
    assert_eq!(exit_json.get("exited").and_then(|v| v.as_i64()), Some(0));

    // Shutdown
    send_cmd(&mut stdin, "\"shutdown\"");
    child.wait().unwrap();
}

#[test]
fn test_helper_cancel_terminates_quickly() {
    let mut child = spawn_helper();
    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = BufReader::new(child.stdout.take().unwrap());

    // Send a long-running command
    let cwd = std::env::current_dir().unwrap().to_string_lossy().to_string();
    let cmd = if cfg!(windows) {
        format!(
            r#"{{"command":"powershell","args":["-Command","Start-Sleep 300"],"cwd":"{}"}}"#,
            cwd.replace('\\', "\\\\")
        )
    } else {
        format!(r#"{{"command":"sleep","args":["300"],"cwd":"{}"}}"#, cwd)
    };
    send_cmd(&mut stdin, &cmd);

    // Read pid response
    let pid_line = read_line(&mut stdout);
    let pid_json: serde_json::Value = serde_json::from_str(&pid_line).unwrap();
    assert!(pid_json.get("pid").is_some(), "Expected pid response, got: {pid_line}");

    // Wait a moment for the process to start
    std::thread::sleep(Duration::from_millis(500));

    // Send terminate cancel
    let cancel_cmd = r#"{"cancel":"TERMINATE"}"#;
    let start = Instant::now();
    send_cmd(&mut stdin, cancel_cmd);

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
    send_cmd(&mut stdin, "\"shutdown\"");
    child.wait().unwrap();
}

#[test]
fn test_helper_nonexistent_command() {
    let mut child = spawn_helper();
    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = BufReader::new(child.stdout.take().unwrap());

    let cwd = std::env::current_dir().unwrap().to_string_lossy().to_string();
    let cmd = format!(
        r#"{{"command":"nonexistentcommand12345","args":[],"cwd":"{}"}}"#,
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

    send_cmd(&mut stdin, "\"shutdown\"");
    child.wait().unwrap();
}
