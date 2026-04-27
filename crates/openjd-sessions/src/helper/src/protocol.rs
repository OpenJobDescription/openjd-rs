// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::Write;

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
///   `{"cancel": "TERMINATE"}`
///   `{"cancel": "NOTIFY_THEN_TERMINATE", "notifyPeriodInSeconds": <u64>}`
#[derive(Debug, Clone)]
pub enum CancelMethod {
    Terminate,
    NotifyThenTerminate { notify_period_in_seconds: u64 },
}

/// Commands received on stdin from the session.
#[derive(Debug)]
pub enum Command {
    Run(RunCommand),
    Cancel(CancelMethod),
    Shutdown,
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
        match &value {
            serde_json::Value::String(s) if s == "shutdown" => Ok(Command::Shutdown),
            serde_json::Value::Object(map) => {
                if map.contains_key("command") {
                    let run: RunCommand = serde_json::from_value(value)
                        .map_err(serde::de::Error::custom)?;
                    Ok(Command::Run(run))
                } else if let Some(sig) = map.get("cancel") {
                    let method = sig
                        .as_str()
                        .ok_or_else(|| serde::de::Error::custom("cancel must be a string"))?;
                    match method {
                        "TERMINATE" => Ok(Command::Cancel(CancelMethod::Terminate)),
                        "NOTIFY_THEN_TERMINATE" => {
                            let notify_period_in_seconds = map
                                .get("notifyPeriodInSeconds")
                                .and_then(|v| v.as_u64())
                                .ok_or_else(|| {
                                    serde::de::Error::custom(
                                        "NOTIFY_THEN_TERMINATE requires notifyPeriodInSeconds (u64)",
                                    )
                                })?;
                            Ok(Command::Cancel(CancelMethod::NotifyThenTerminate {
                                notify_period_in_seconds,
                            }))
                        }
                        other => Err(serde::de::Error::custom(format!(
                            "unknown cancel method: {other}"
                        ))),
                    }
                } else {
                    Err(serde::de::Error::custom("unknown command object"))
                }
            }
            _ => Err(serde::de::Error::custom("expected object or \"shutdown\"")),
        }
    }
}

/// Write a JSON response line to stdout and flush.
pub fn send(response: &Response) {
    let stdout = std::io::stdout();
    let mut lock = stdout.lock();
    let _ = serde_json::to_writer(&mut lock, response);
    let _ = lock.write_all(b"\n");
    let _ = lock.flush();
}
