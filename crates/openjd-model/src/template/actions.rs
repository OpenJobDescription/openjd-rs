// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Action types per spec §5.

use crate::format_string::FormatString;
use serde::Deserialize;

/// §5 Action
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Action {
    pub command: FormatString,
    pub args: Option<Vec<FormatString>>,
    pub cancelation: Option<CancelationMode>,
    pub timeout: Option<FormatString>,
}

/// §5.3 CancelationMethod — discriminated union on `mode`.
#[derive(Debug, Clone)]
pub enum CancelationMode {
    /// §5.3.1 — immediate termination, no extra fields allowed.
    Terminate,
    /// §5.3.2 — notify then terminate, with optional grace period.
    NotifyThenTerminate {
        notify_period_in_seconds: Option<FormatString>,
    },
}

impl<'de> Deserialize<'de> for CancelationMode {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        use std::collections::HashMap;
        let map = HashMap::<String, serde_json::Value>::deserialize(deserializer)?;
        let mode = map
            .get("mode")
            .and_then(|v| v.as_str())
            .ok_or_else(|| serde::de::Error::missing_field("mode"))?;
        match mode {
            "TERMINATE" => {
                let extra: Vec<_> = map.keys().filter(|k| *k != "mode").collect();
                if !extra.is_empty() {
                    return Err(serde::de::Error::custom(format!(
                        "unknown field `{}`, TERMINATE accepts no additional fields",
                        extra[0]
                    )));
                }
                Ok(CancelationMode::Terminate)
            }
            "NOTIFY_THEN_TERMINATE" => {
                let extra: Vec<_> = map
                    .keys()
                    .filter(|k| *k != "mode" && *k != "notifyPeriodInSeconds")
                    .collect();
                if !extra.is_empty() {
                    return Err(serde::de::Error::custom(format!(
                        "unknown field `{}`, expected `notifyPeriodInSeconds`",
                        extra[0]
                    )));
                }
                let notify = map
                    .get("notifyPeriodInSeconds")
                    .map(|v| FormatString::deserialize(v.clone()))
                    .transpose()
                    .map_err(serde::de::Error::custom)?;
                Ok(CancelationMode::NotifyThenTerminate {
                    notify_period_in_seconds: notify,
                })
            }
            other => Err(serde::de::Error::custom(format!(
                "unknown variant `{other}`, expected `TERMINATE` or `NOTIFY_THEN_TERMINATE`"
            ))),
        }
    }
}

/// §3.5.1 StepActions
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StepActions {
    pub on_run: Action,
}

/// §4.1 EnvironmentActions
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EnvironmentActions {
    pub on_enter: Option<Action>,
    /// RFC 0008 — wraps inner environments' `onEnter` actions. Requires the
    /// `WRAP_ACTIONS` extension.
    pub on_wrap_enter: Option<Action>,
    /// RFC 0008 — wraps tasks' `onRun` actions. Requires the
    /// `WRAP_ACTIONS` extension.
    pub on_wrap_task_run: Option<Action>,
    /// RFC 0008 — wraps inner environments' `onExit` actions. Requires the
    /// `WRAP_ACTIONS` extension.
    pub on_wrap_exit: Option<Action>,
    pub on_exit: Option<Action>,
}
