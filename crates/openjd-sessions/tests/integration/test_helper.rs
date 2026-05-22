// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

// Integration tests for the helper subprocess on both POSIX and Windows.
//
// The helper binary is a standalone child process that the session uses to
// run commands as a different user. These tests drive the binary directly
// (without cross-user elevation) to exercise the protocol and cancellation
// paths on both platforms.

use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};

/// A constant token for these tests. Must be exactly [`AUTH_TOKEN_LEN`]
/// characters from the URL-safe alphabet (the helper enforces the length
/// before reading any stdin). The actual contents don't matter.
const TEST_TOKEN: &str = "AbCdEfGhIjKlMnOpQrStUv";

// ────────────────────────────────────────────────────────────────────
// Platform helpers: the helper binary, long-running commands, shells.
// ────────────────────────────────────────────────────────────────────

fn helper_path() -> PathBuf {
    let p = PathBuf::from(env!("OPENJD_HELPER_BINARY_PATH"));
    if !p.exists() {
        panic!(
            "Helper binary not found at {}. Build openjd-sessions first (its build.rs \
             compiles the helper).",
            p.display()
        );
    }
    p
}

/// Temp dir for the helper's `cwd`. Both platforms accept their native temp dir.
fn tmp_cwd() -> String {
    std::env::temp_dir().to_string_lossy().into_owned()
}

/// A process that just sleeps for a long time and can be cancelled.
///
/// On POSIX: `sleep 30`.
/// On Windows: `ping -n 31 127.0.0.1` (pings once per second, prints to stdout
/// so CTRL_BREAK can actually interrupt; `timeout` would need a tty).
fn long_running_cmd() -> (&'static str, Vec<&'static str>) {
    if cfg!(windows) {
        ("ping", vec!["-n", "31", "127.0.0.1"])
    } else {
        ("sleep", vec!["30"])
    }
}

/// A command that prints a fixed string and exits.
fn echo_cmd(what: &str) -> String {
    if cfg!(windows) {
        format!(
            r#"{{"token": "{TEST_TOKEN}", "command": "cmd", "args": ["/c", "echo {}"], "env": {{}}, "cwd": "{}"}}"#,
            what,
            tmp_cwd().replace('\\', "\\\\")
        )
    } else {
        format!(
            r#"{{"token": "{TEST_TOKEN}", "command": "echo", "args": ["{}"], "env": {{}}, "cwd": "{}"}}"#,
            what,
            tmp_cwd()
        )
    }
}

/// A shell command that exits with the given code.
fn exit_with_code_cmd(code: i32) -> String {
    if cfg!(windows) {
        format!(
            r#"{{"token": "{TEST_TOKEN}", "command": "cmd", "args": ["/c", "exit {}"], "env": {{}}, "cwd": "{}"}}"#,
            code,
            tmp_cwd().replace('\\', "\\\\")
        )
    } else {
        format!(
            r#"{{"token": "{TEST_TOKEN}", "command": "sh", "args": ["-c", "exit {}"], "env": {{}}, "cwd": "{}"}}"#,
            code,
            tmp_cwd()
        )
    }
}

/// A shell command that echoes an env var value.
fn echo_env_var_cmd(var_name: &str, var_value: &str) -> String {
    if cfg!(windows) {
        // cmd echoes the literal %VAR% when the env isn't set at the cmd level,
        // but Command::envs is applied to the child process env, which cmd inherits.
        format!(
            r#"{{"token": "{TEST_TOKEN}", "command": "cmd", "args": ["/c", "echo %{}%"], "env": {{"{}": "{}"}}, "cwd": "{}"}}"#,
            var_name,
            var_name,
            var_value,
            tmp_cwd().replace('\\', "\\\\")
        )
    } else {
        format!(
            r#"{{"token": "{TEST_TOKEN}", "command": "sh", "args": ["-c", "echo ${}"], "env": {{"{}": "{}"}}, "cwd": "{}"}}"#,
            var_name,
            var_name,
            var_value,
            tmp_cwd()
        )
    }
}

fn long_running_run_json() -> String {
    let (cmd, args) = long_running_cmd();
    let args_json = args
        .iter()
        .map(|a| format!("\"{a}\""))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        r#"{{"token": "{TEST_TOKEN}", "command": "{}", "args": [{}], "env": {{}}, "cwd": "{}"}}"#,
        cmd,
        args_json,
        tmp_cwd().replace('\\', "\\\\")
    )
}

// ────────────────────────────────────────────────────────────────────
// Helper driver: wraps the subprocess so tests can send/receive JSON.
// ────────────────────────────────────────────────────────────────────

struct Helper {
    child: std::process::Child,
    stdin: std::process::ChildStdin,
    reader: BufReader<std::process::ChildStdout>,
}

impl Helper {
    fn spawn() -> Self {
        let mut child = Command::new(helper_path())
            .args(["--auth-token", TEST_TOKEN])
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
        writeln!(self.stdin, "{msg}").expect("write to helper stdin");
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
        self.send(&format!(r#"{{"token": "{TEST_TOKEN}", "shutdown": true}}"#));
        self.child.wait().expect("wait for helper")
    }
}

// ════════════════════════════════════════════════════════════════════
// Cross-platform tests (run on POSIX and Windows).
// ════════════════════════════════════════════════════════════════════

#[test]
fn test_helper_startup_and_shutdown() {
    let helper = Helper::spawn();
    let status = helper.shutdown();
    assert!(status.success(), "helper should exit cleanly: {status:?}");
}

#[test]
fn test_helper_sequential_commands() {
    let mut h = Helper::spawn();

    h.send(&echo_cmd("hello"));
    let resp = h.read_until_done();
    assert!(
        resp.iter().any(|v| v.get("pid").is_some()),
        "should get pid"
    );
    assert!(
        resp.iter().any(|v| v
            .get("out")
            .and_then(|o| o.as_str())
            .is_some_and(|s| s.contains("hello"))),
        "should get hello output, got: {resp:?}"
    );
    assert_eq!(
        resp.last().unwrap()["exited"],
        0,
        "first command should exit 0"
    );

    h.send(&echo_cmd("world"));
    let resp = h.read_until_done();
    assert!(
        resp.iter().any(|v| v
            .get("out")
            .and_then(|o| o.as_str())
            .is_some_and(|s| s.contains("world"))),
        "should get world output, got: {resp:?}"
    );
    assert_eq!(
        resp.last().unwrap()["exited"],
        0,
        "second command should exit 0"
    );

    assert!(h.shutdown().success());
}

#[test]
fn test_helper_child_exit_code() {
    let mut h = Helper::spawn();

    h.send(&exit_with_code_cmd(42));
    let resp = h.read_until_done();
    assert_eq!(
        resp.last().unwrap()["exited"],
        42,
        "should get exit code 42"
    );

    assert!(h.shutdown().success());
}

#[test]
fn test_helper_command_not_found() {
    let mut h = Helper::spawn();

    let cwd = tmp_cwd().replace('\\', "\\\\");
    h.send(&format!(
        r#"{{"token": "{TEST_TOKEN}", "command": "nonexistent_binary_xyz_12345", "args": [], "env": {{}}, "cwd": "{cwd}"}}"#
    ));
    let resp = h.read_until_done();
    assert!(
        resp.last().unwrap().get("error").is_some(),
        "should get error for nonexistent binary, got: {resp:?}"
    );

    assert!(h.shutdown().success());
}

#[test]
fn test_helper_env_vars() {
    let mut h = Helper::spawn();

    h.send(&echo_env_var_cmd("MY_VAR", "test_value"));
    let resp = h.read_until_done();
    assert!(
        resp.iter().any(|v| {
            v.get("out")
                .and_then(|o| o.as_str())
                .is_some_and(|s| s.contains("test_value"))
        }),
        "output should contain test_value, got: {resp:?}"
    );
    assert_eq!(resp.last().unwrap()["exited"], 0);

    assert!(h.shutdown().success());
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
        "error should start with 'parse error:', got: {err_msg}"
    );

    // Helper should still work after protocol error
    h.send(&echo_cmd("still_alive"));
    let resp = h.read_until_done();
    assert!(
        resp.iter().any(|v| v
            .get("out")
            .and_then(|o| o.as_str())
            .is_some_and(|s| s.contains("still_alive"))),
        "command after protocol error should produce output"
    );
    assert_eq!(
        resp.last().unwrap()["exited"],
        0,
        "command after protocol error should succeed"
    );

    assert!(h.shutdown().success());
}

// ════════════════════════════════════════════════════════════════════
// POSIX-only tests
// ════════════════════════════════════════════════════════════════════

#[cfg(unix)]
#[test]
fn test_helper_cancel_during_execution_unix() {
    let mut h = Helper::spawn();

    h.send(&long_running_run_json());
    let pid_resp = h.read_line();
    assert!(pid_resp.get("pid").is_some(), "should get pid");

    h.send(&format!(
        r#"{{"token": "{TEST_TOKEN}", "cancel": "NOTIFY_THEN_TERMINATE", "notifyPeriodInSeconds": 5}}"#
    ));

    let resp = h.read_until_done();
    let last = resp.last().unwrap();
    assert!(last.get("exited").is_some());
    assert_ne!(
        last["exited"], 0,
        "cancelled process should have non-zero exit code"
    );

    assert!(h.shutdown().success());
}

#[cfg(unix)]
#[test]
fn test_helper_crash_during_execution_unix() {
    // If the helper itself dies mid-command, reading should return EOF/error
    // rather than hang.
    let mut h = Helper::spawn();
    let helper_pid = h.child.id();

    h.send(&long_running_run_json());
    let pid_resp = h.read_line();
    assert!(pid_resp.get("pid").is_some());

    nix::sys::signal::kill(
        nix::unistd::Pid::from_raw(helper_pid as i32),
        nix::sys::signal::Signal::SIGKILL,
    )
    .expect("kill helper");

    let mut line = String::new();
    let _ = h.reader.read_line(&mut line);
    // The key assertion: we got here without hanging.
}

// ════════════════════════════════════════════════════════════════════
// Windows-only tests
// ════════════════════════════════════════════════════════════════════

#[cfg(windows)]
#[test]
fn test_helper_terminate_during_execution_windows() {
    // TERMINATE is a hard kill — works regardless of console state, so this
    // is the reliable cancellation path to assert on.
    let mut h = Helper::spawn();

    h.send(&long_running_run_json());
    let pid_resp = h.read_line();
    assert!(pid_resp.get("pid").is_some(), "should get pid");
    let child_pid = pid_resp["pid"].as_u64().unwrap() as u32;

    h.send(&format!(
        r#"{{"token": "{TEST_TOKEN}", "cancel": "TERMINATE"}}"#
    ));

    let start = std::time::Instant::now();
    let resp = h.read_until_done();
    let elapsed = start.elapsed();

    let last = resp.last().unwrap();
    assert!(last.get("exited").is_some(), "got: {resp:?}");
    assert!(
        elapsed < std::time::Duration::from_secs(10),
        "TERMINATE should kill quickly, took {elapsed:?}"
    );

    // The child PID should no longer exist.
    assert!(
        !process_is_alive(child_pid),
        "child pid {child_pid} should be dead after TERMINATE"
    );

    assert!(h.shutdown().success());
}

#[cfg(windows)]
#[test]
fn test_helper_ctrl_break_falls_back_to_terminate_windows() {
    // NOTIFY_THEN_TERMINATE sends a platform-appropriate soft signal (CTRL_BREAK
    // on Windows) and, if the child doesn't exit within the notify period, the
    // helper escalates to a hard kill. Either way the child should die and the
    // helper should report `exited`.
    let mut h = Helper::spawn();

    h.send(&long_running_run_json());
    let pid_resp = h.read_line();
    let child_pid = pid_resp["pid"].as_u64().unwrap() as u32;

    h.send(&format!(
        r#"{{"token": "{TEST_TOKEN}", "cancel": "NOTIFY_THEN_TERMINATE", "notifyPeriodInSeconds": 5}}"#
    ));

    let start = std::time::Instant::now();
    let resp = h.read_until_done();
    let elapsed = start.elapsed();

    let last = resp.last().unwrap();
    assert!(last.get("exited").is_some(), "got: {resp:?}");
    assert!(
        elapsed < std::time::Duration::from_secs(10),
        "cancel should finish within the grace window, took {elapsed:?}"
    );
    assert!(!process_is_alive(child_pid));

    assert!(h.shutdown().success());
}

#[cfg(windows)]
#[test]
fn test_helper_crash_during_execution_windows() {
    // If the helper itself dies mid-command, reading should return EOF/error
    // rather than hang. Additionally: the workload it spawned must die too.
    //
    // The helper places itself in a Job Object with KILL_ON_JOB_CLOSE at
    // startup, so when we TerminateProcess() it here, the kernel closes the
    // job and terminates every workload that inherited the job. Without this
    // guarantee, an orphaned workload would keep inherited handles alive and
    // can stall the cargo-test runner long enough for GitHub Actions'
    // watchdog to declare the runner lost.
    let mut h = Helper::spawn();
    let helper_pid = h.child.id();

    h.send(&long_running_run_json());
    let pid_resp = h.read_line();
    let workload_pid = pid_resp["pid"].as_u64().expect("pid in response") as u32;

    terminate_process(helper_pid);

    let mut line = String::new();
    let _ = h.reader.read_line(&mut line);
    // The helper's stdout pipe should be at EOF — got here without hanging.

    // Wait briefly for the kernel to tear down the job. KILL_ON_JOB_CLOSE
    // is synchronous with the last handle close, but the helper's pending
    // cleanup (and any time it takes for `ping` to actually exit after
    // being signalled) means we should give it a small grace window.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while process_is_alive(workload_pid) && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(50));
    }

    if process_is_alive(workload_pid) {
        // Defensive cleanup so we don't leak a `ping` into the next test —
        // an orphan here would do exactly what we're trying to prevent.
        terminate_process(workload_pid);
        panic!(
            "workload pid {workload_pid} survived helper crash; \
             Job Object KILL_ON_JOB_CLOSE not enforced — \
             this indicates the helper is leaking grandchildren \
             on abnormal exit, which can stall CI."
        );
    }
}

// ────────────────────────────────────────────────────────────────────
// Windows helpers for the tests above.
// ────────────────────────────────────────────────────────────────────

#[cfg(windows)]
fn process_is_alive(pid: u32) -> bool {
    use windows::Win32::Foundation::{CloseHandle, STILL_ACTIVE};
    use windows::Win32::System::Threading::{
        GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    unsafe {
        match OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) {
            Ok(h) => {
                let mut code: u32 = 0;
                let alive =
                    GetExitCodeProcess(h, &mut code).is_ok() && code == STILL_ACTIVE.0 as u32;
                let _ = CloseHandle(h);
                alive
            }
            Err(_) => false,
        }
    }
}

#[cfg(windows)]
fn terminate_process(pid: u32) {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::{OpenProcess, TerminateProcess, PROCESS_TERMINATE};
    unsafe {
        if let Ok(h) = OpenProcess(PROCESS_TERMINATE, false, pid) {
            let _ = TerminateProcess(h, 1);
            let _ = CloseHandle(h);
        }
    }
}
