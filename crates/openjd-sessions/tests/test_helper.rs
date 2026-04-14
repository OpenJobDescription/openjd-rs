// The helper binary is Linux-only for now, so these tests only run on Unix.
#![cfg(unix)]

use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};

fn helper_path() -> PathBuf {
    let p =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/helper/target/release/openjd_helper");
    if !p.exists() {
        panic!(
            "Helper binary not found at {}. Build it first: cd crates/openjd-sessions/src/helper && cargo build --release",
            p.display()
        );
    }
    p
}

struct Helper {
    child: std::process::Child,
    stdin: std::process::ChildStdin,
    reader: BufReader<std::process::ChildStdout>,
}

impl Helper {
    fn spawn() -> Self {
        let mut child = Command::new(helper_path())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("failed to spawn helper");
        let stdin = child.stdin.take().unwrap();
        let reader = BufReader::new(child.stdout.take().unwrap());
        Self {
            child,
            stdin,
            reader,
        }
    }

    fn send(&mut self, msg: &str) {
        writeln!(self.stdin, "{}", msg).expect("write to helper stdin");
        self.stdin.flush().expect("flush helper stdin");
    }

    fn read_line(&mut self) -> serde_json::Value {
        let mut line = String::new();
        self.reader
            .read_line(&mut line)
            .expect("read from helper stdout");
        serde_json::from_str(line.trim()).expect("parse helper response JSON")
    }

    /// Read responses until we get an "exited" or "error" response.
    fn read_until_done(&mut self) -> Vec<serde_json::Value> {
        let mut responses = Vec::new();
        loop {
            let v = self.read_line();
            let done = v.get("exited").is_some() || v.get("error").is_some();
            responses.push(v);
            if done {
                break;
            }
        }
        responses
    }

    fn shutdown(mut self) -> std::process::ExitStatus {
        self.send("\"shutdown\"");
        self.child.wait().expect("wait for helper")
    }
}

#[test]
fn test_helper_startup_and_shutdown() {
    let helper = Helper::spawn();
    let status = helper.shutdown();
    assert!(status.success(), "helper should exit cleanly: {:?}", status);
}

#[test]
fn test_helper_sequential_commands() {
    let mut h = Helper::spawn();

    // First command
    h.send(r#"{"command": "echo", "args": ["hello"], "env": {}, "cwd": "/tmp"}"#);
    let resp = h.read_until_done();
    assert!(
        resp.iter().any(|v| v.get("pid").is_some()),
        "should get pid"
    );
    assert!(
        resp.iter()
            .any(|v| v.get("out") == Some(&serde_json::json!("hello"))),
        "should get hello output"
    );
    let last = resp.last().unwrap();
    assert_eq!(last["exited"], 0, "first command should exit 0");

    // Second command
    h.send(r#"{"command": "echo", "args": ["world"], "env": {}, "cwd": "/tmp"}"#);
    let resp = h.read_until_done();
    assert!(
        resp.iter()
            .any(|v| v.get("out") == Some(&serde_json::json!("world"))),
        "should get world output"
    );
    let last = resp.last().unwrap();
    assert_eq!(last["exited"], 0, "second command should exit 0");

    let status = h.shutdown();
    assert!(status.success());
}

#[test]
fn test_helper_cancel_during_execution() {
    let mut h = Helper::spawn();

    h.send(r#"{"command": "sleep", "args": ["30"], "env": {}, "cwd": "/tmp"}"#);
    // Wait for pid response
    let pid_resp = h.read_line();
    assert!(pid_resp.get("pid").is_some(), "should get pid");

    // Send cancel
    h.send(r#"{"cancel": "SIGTERM"}"#);

    let resp = h.read_until_done();
    let last = resp.last().unwrap();
    assert!(
        last.get("exited").is_some(),
        "should get exited after cancel"
    );
    assert_ne!(
        last["exited"], 0,
        "cancelled process should have non-zero exit code"
    );

    let status = h.shutdown();
    assert!(status.success());
}

#[test]
fn test_helper_child_crash() {
    let mut h = Helper::spawn();

    h.send(r#"{"command": "sh", "args": ["-c", "exit 42"], "env": {}, "cwd": "/tmp"}"#);
    let resp = h.read_until_done();
    let last = resp.last().unwrap();
    assert_eq!(last["exited"], 42, "should get exit code 42");

    let status = h.shutdown();
    assert!(status.success());
}

#[test]
fn test_helper_command_not_found() {
    let mut h = Helper::spawn();

    h.send(r#"{"command": "nonexistent_binary_xyz_12345", "args": [], "env": {}, "cwd": "/tmp"}"#);
    let resp = h.read_until_done();
    let last = resp.last().unwrap();
    assert!(
        last.get("error").is_some(),
        "should get error for nonexistent binary"
    );

    let status = h.shutdown();
    assert!(status.success());
}

#[test]
fn test_helper_env_vars() {
    let mut h = Helper::spawn();

    h.send(r#"{"command": "sh", "args": ["-c", "echo $MY_VAR"], "env": {"MY_VAR": "test_value"}, "cwd": "/tmp"}"#);
    let resp = h.read_until_done();
    assert!(
        resp.iter().any(|v| {
            v.get("out")
                .and_then(|o| o.as_str())
                .is_some_and(|s| s.contains("test_value"))
        }),
        "output should contain test_value"
    );
    let last = resp.last().unwrap();
    assert_eq!(last["exited"], 0);

    let status = h.shutdown();
    assert!(status.success());
}

#[test]
fn test_helper_protocol_error() {
    let mut h = Helper::spawn();

    // Send malformed JSON
    h.send("this is not json");
    let resp = h.read_line();
    assert!(
        resp.get("error").is_some(),
        "should get error for malformed JSON"
    );
    let err_msg = resp["error"].as_str().unwrap();
    assert!(
        err_msg.starts_with("parse error:"),
        "error should start with 'parse error:', got: {}",
        err_msg
    );

    // Helper should still work after protocol error
    h.send(r#"{"command": "echo", "args": ["still_alive"], "env": {}, "cwd": "/tmp"}"#);
    let resp = h.read_until_done();
    assert!(resp
        .iter()
        .any(|v| v.get("out") == Some(&serde_json::json!("still_alive"))));
    let last = resp.last().unwrap();
    assert_eq!(
        last["exited"], 0,
        "command after protocol error should succeed"
    );

    let status = h.shutdown();
    assert!(status.success());
}

#[test]
fn test_helper_crash_during_execution() {
    // Verify that if the helper process dies mid-command, read_line returns
    // an error or EOF rather than hanging forever.
    let mut h = Helper::spawn();
    let helper_pid = h.child.id();

    // Start a long-running command
    h.send(r#"{"command": "sleep", "args": ["60"], "env": {}, "cwd": "/tmp"}"#);

    // Read the pid response
    let pid_resp = h.read_line();
    assert!(pid_resp.get("pid").is_some(), "should get pid response");

    // Kill the helper process (simulating OOM/crash)
    nix::sys::signal::kill(
        nix::unistd::Pid::from_raw(helper_pid as i32),
        nix::sys::signal::Signal::SIGKILL,
    )
    .expect("kill helper");

    // Reading should now return EOF or error, not hang
    let mut line = String::new();
    let result = h.reader.read_line(&mut line);
    match result {
        Ok(0) => {}  // EOF — correct behavior
        Ok(_) => {}  // Got a partial response — acceptable
        Err(_) => {} // IO error — acceptable
    }
    // The key assertion: we got here without hanging
}
