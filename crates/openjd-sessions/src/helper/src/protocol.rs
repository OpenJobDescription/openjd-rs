// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::Write;

/// Expected length of the per-helper authentication token in bytes.
///
/// The session generates a 22-character ASCII string from a 64-element
/// URL-safe alphabet (128 bits of entropy, 6 bits per character). The helper
/// stores and compares raw bytes so it doesn't have to know anything about
/// the alphabet — only the length.
pub const AUTH_TOKEN_LEN: usize = 22;

/// A run-command request from the session.
#[derive(Debug, Deserialize)]
pub struct RunCommand {
    pub command: String,
    pub args: Vec<String>,
    #[serde(default)]
    pub env: HashMap<String, String>,
    pub cwd: String,
}

/// Cancel method, matching the OpenJD spec's `cancelationMethod` semantics:
///
/// - `Terminate`: immediate hard kill (process tree).
/// - `NotifyThenTerminate { notify_period_in_seconds }`: send a platform-
///   appropriate soft signal (SIGTERM / CTRL_BREAK), then escalate to a hard
///   kill after `notify_period_in_seconds` if the child hasn't exited.
///
/// Wire format:
///   `{"token":"...","cancel":"TERMINATE"}`
///   `{"token":"...","cancel":"NOTIFY_THEN_TERMINATE","notifyPeriodInSeconds":<u64>}`
#[derive(Debug, Clone)]
pub enum CancelMethod {
    Terminate,
    NotifyThenTerminate { notify_period_in_seconds: u64 },
}

/// Commands received on stdin from the session.
///
/// Every command carries a copy of the shared auth token. The helper verifies
/// the token in constant time before acting on the command; mismatches are
/// reported as `{"error":"invalid token"}` and ignored.
#[derive(Debug)]
pub enum Command {
    Run { token: String, run: RunCommand },
    Cancel { token: String, method: CancelMethod },
    Shutdown { token: String },
}

impl Command {
    /// The token string carried by this command.
    pub fn token(&self) -> &str {
        match self {
            Command::Run { token, .. } => token,
            Command::Cancel { token, .. } => token,
            Command::Shutdown { token } => token,
        }
    }
}

/// Responses sent on stdout to the session.
#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum Response {
    Pid { pid: u32 },
    Out { out: String },
    Exited { exited: i32 },
    Error { error: String },
}

impl<'de> Deserialize<'de> for Command {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = serde_json::Value::deserialize(deserializer)?;
        let serde_json::Value::Object(mut map) = value else {
            return Err(serde::de::Error::custom("expected JSON object"));
        };

        // Extract the token field. Missing or non-string → treat as an empty
        // token; the main loop's constant-time check will still reject it.
        let token = match map.remove("token") {
            Some(serde_json::Value::String(s)) => s,
            Some(_) | None => String::new(),
        };

        if map.contains_key("command") {
            let run: RunCommand = serde_json::from_value(serde_json::Value::Object(map))
                .map_err(serde::de::Error::custom)?;
            Ok(Command::Run { token, run })
        } else if let Some(sig) = map.get("cancel") {
            let method_str = sig
                .as_str()
                .ok_or_else(|| serde::de::Error::custom("cancel must be a string"))?;
            let method = match method_str {
                "TERMINATE" => CancelMethod::Terminate,
                "NOTIFY_THEN_TERMINATE" => {
                    let notify_period_in_seconds = map
                        .get("notifyPeriodInSeconds")
                        .and_then(|v| v.as_u64())
                        .ok_or_else(|| {
                            serde::de::Error::custom(
                                "NOTIFY_THEN_TERMINATE requires notifyPeriodInSeconds (u64)",
                            )
                        })?;
                    CancelMethod::NotifyThenTerminate {
                        notify_period_in_seconds,
                    }
                }
                other => {
                    return Err(serde::de::Error::custom(format!(
                        "unknown cancel method: {other}"
                    )));
                }
            };
            Ok(Command::Cancel { token, method })
        } else if map
            .get("shutdown")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
        {
            Ok(Command::Shutdown { token })
        } else {
            Err(serde::de::Error::custom("unknown command object"))
        }
    }
}

/// Constant-time equality check between a received token and the expected
/// bytes. Returns `false` for any length mismatch before touching contents,
/// which is fine: the token's length is fixed and public.
///
/// For equal-length inputs, runs `a.len()` byte operations regardless of
/// where the first mismatching byte sits, so the timing of the compare
/// doesn't reveal how many bytes matched.
pub fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff: u8 = 0;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

/// Write a JSON response line to stdout and flush.
pub fn send(response: &Response) {
    let stdout = std::io::stdout();
    let mut lock = stdout.lock();
    let _ = serde_json::to_writer(&mut lock, response);
    let _ = lock.write_all(b"\n");
    let _ = lock.flush();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_run_command_with_token() {
        let cmd: Command = serde_json::from_str(
            r#"{"token":"AbCdEfGhIjKlMnOpQrStUv","command":"echo","args":["hi"],"cwd":"/tmp"}"#,
        )
        .unwrap();
        match cmd {
            Command::Run { token, run } => {
                assert_eq!(token, "AbCdEfGhIjKlMnOpQrStUv");
                assert_eq!(run.command, "echo");
                assert_eq!(run.args, vec!["hi"]);
            }
            _ => panic!("expected Run"),
        }
    }

    #[test]
    fn parses_cancel_with_token() {
        let cmd: Command = serde_json::from_str(
            r#"{"token":"AbCdEfGhIjKlMnOpQrStUv","cancel":"TERMINATE"}"#,
        )
        .unwrap();
        match cmd {
            Command::Cancel { token, method } => {
                assert_eq!(token, "AbCdEfGhIjKlMnOpQrStUv");
                assert!(matches!(method, CancelMethod::Terminate));
            }
            _ => panic!("expected Cancel"),
        }
    }

    #[test]
    fn parses_notify_then_terminate_with_token() {
        let cmd: Command = serde_json::from_str(
            r#"{"token":"t","cancel":"NOTIFY_THEN_TERMINATE","notifyPeriodInSeconds":5}"#,
        )
        .unwrap();
        match cmd {
            Command::Cancel {
                method: CancelMethod::NotifyThenTerminate {
                    notify_period_in_seconds: 5,
                },
                ..
            } => {}
            _ => panic!("expected NotifyThenTerminate(5)"),
        }
    }

    #[test]
    fn parses_shutdown_with_token() {
        let cmd: Command =
            serde_json::from_str(r#"{"token":"AbCdEfGhIjKlMnOpQrStUv","shutdown":true}"#).unwrap();
        match cmd {
            Command::Shutdown { token } => assert_eq!(token, "AbCdEfGhIjKlMnOpQrStUv"),
            _ => panic!("expected Shutdown"),
        }
    }

    #[test]
    fn missing_token_parses_as_empty_string() {
        // The main loop is responsible for rejecting this; the deserializer
        // just succeeds with an empty token so the command loop stays aligned.
        let cmd: Command =
            serde_json::from_str(r#"{"command":"echo","args":[],"cwd":"/tmp"}"#).unwrap();
        assert_eq!(cmd.token(), "");
    }

    #[test]
    fn bare_shutdown_string_is_rejected() {
        // The old bare-string form is no longer accepted: shutdown is always
        // an object with a token.
        let err = serde_json::from_str::<Command>(r#""shutdown""#).unwrap_err();
        assert!(err.to_string().contains("expected JSON object"));
    }

    #[test]
    fn constant_time_eq_basic() {
        assert!(constant_time_eq(b"abc", b"abc"));
        assert!(!constant_time_eq(b"abc", b"abd"));
        assert!(!constant_time_eq(b"abc", b"abcd"));
        assert!(!constant_time_eq(b"", b"x"));
        assert!(constant_time_eq(b"", b""));
    }
}
