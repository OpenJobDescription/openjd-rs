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
///
/// `DeferredMode` carries a format-string `mode` (FEATURE_BUNDLE_1) whose
/// TERMINATE-vs-NOTIFY_THEN_TERMINATE decision is deferred to run time:
/// `mode` is the schema selector, so it normally must be known at parse
/// time, but a forwarded value like `{{WrappedAction.Cancelation.Mode}}`
/// (RFC 0008 round-trip forwarding) only exists at run time. See
/// `specs/model/template-types.md` § CancelationMode for the full design
/// rationale, and openjd-specifications Template Schemas §5.3 / RFC 0008
/// "Cancelation behavior" for the normative rules.
#[derive(Debug, Clone)]
pub enum CancelationMode {
    /// §5.3.1 — immediate termination, no extra fields allowed.
    Terminate,
    /// §5.3.2 — notify then terminate, with optional grace period.
    NotifyThenTerminate {
        notify_period_in_seconds: Option<FormatString>,
    },
    /// §5.3 (FEATURE_BUNDLE_1) — the mode is a format string, resolved at
    /// run time. See the type-level docs for why this exists.
    DeferredMode {
        mode: FormatString,
        notify_period_in_seconds: Option<FormatString>,
    },
}

impl<'de> Deserialize<'de> for CancelationMode {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        use std::collections::HashMap;
        let map = HashMap::<String, serde_json::Value>::deserialize(deserializer)?;
        let mode_value = map
            .get("mode")
            .ok_or_else(|| serde::de::Error::missing_field("mode"))?;
        let mode = mode_value
            .as_str()
            .ok_or_else(|| serde::de::Error::custom("`mode` must be a string"))?;
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
                // An explicit null is treated as "not provided", matching
                // the Python implementation (pydantic Optional).
                let notify = map
                    .get("notifyPeriodInSeconds")
                    .filter(|v| !v.is_null())
                    .map(|v| FormatString::deserialize(v.clone()))
                    .transpose()
                    .map_err(serde::de::Error::custom)?;
                Ok(CancelationMode::NotifyThenTerminate {
                    notify_period_in_seconds: notify,
                })
            }
            other if other.contains("{{") => {
                // A format-string mode defers the TERMINATE-vs-
                // NOTIFY_THEN_TERMINATE decision to run time (see the
                // type-level docs). Because the shape is not yet known,
                // accept the union of the two shapes' fields; the
                // resolved object is validated against the resolved
                // variant's shape at run time. FEATURE_BUNDLE_1 gating is
                // enforced by template validation, not here.
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
                let mode = FormatString::deserialize(mode_value.clone())
                    .map_err(serde::de::Error::custom)?;
                let notify = map
                    .get("notifyPeriodInSeconds")
                    .filter(|v| !v.is_null())
                    .map(|v| FormatString::deserialize(v.clone()))
                    .transpose()
                    .map_err(serde::de::Error::custom)?;
                Ok(CancelationMode::DeferredMode {
                    mode,
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
    pub on_wrap_env_enter: Option<Action>,
    /// RFC 0008 — wraps tasks' `onRun` actions. Requires the
    /// `WRAP_ACTIONS` extension.
    pub on_wrap_task_run: Option<Action>,
    /// RFC 0008 — wraps inner environments' `onExit` actions. Requires the
    /// `WRAP_ACTIONS` extension.
    pub on_wrap_env_exit: Option<Action>,
    pub on_exit: Option<Action>,
}

/// RFC 0008: the per-hook companion template variable a wrap hook exposes
/// in addition to `WrappedAction.*`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WrapHookScope {
    /// `WrappedEnv.Name` — available in `onWrapEnvEnter` and `onWrapEnvExit`.
    EnvName,
    /// `WrappedStep.Name` — available in `onWrapTaskRun`.
    StepName,
}

/// Generate the shared accessor/iteration helpers for an
/// `EnvironmentActions` struct.
///
/// The template-side (`template::actions`) and job-side (`job`) structs
/// have identical field names but distinct `Action` types and derives, so
/// the five action slots — and the three RFC 0008 wrap hooks — are
/// enumerated here exactly once. Every consumer that needs to "walk the
/// actions" goes through these methods instead of re-listing the fields,
/// which is what keeps a field rename from rippling across the codebase.
macro_rules! impl_environment_actions_helpers {
    ($ty:ty, $action:ty) => {
        impl $ty {
            /// All five action slots paired with their camelCase schema
            /// name, in declaration order.
            pub fn named_slots(&self) -> [(&'static str, &Option<$action>); 5] {
                [
                    ("onEnter", &self.on_enter),
                    ("onWrapEnvEnter", &self.on_wrap_env_enter),
                    ("onWrapTaskRun", &self.on_wrap_task_run),
                    ("onWrapEnvExit", &self.on_wrap_env_exit),
                    ("onExit", &self.on_exit),
                ]
            }

            /// The defined actions, each paired with its schema name, in
            /// declaration order.
            pub fn iter_named(&self) -> impl Iterator<Item = (&'static str, &$action)> {
                self.named_slots()
                    .into_iter()
                    .filter_map(|(name, slot)| slot.as_ref().map(|a| (name, a)))
            }

            /// The defined actions, in declaration order, without names.
            pub fn iter_actions(&self) -> impl Iterator<Item = &$action> {
                self.iter_named().map(|(_, action)| action)
            }

            /// The three RFC 0008 wrap hooks, each paired with its schema
            /// name and the companion template variable it exposes.
            pub fn wrap_hooks(
                &self,
            ) -> [(
                &'static str,
                &Option<$action>,
                $crate::template::WrapHookScope,
            ); 3] {
                use $crate::template::WrapHookScope::{EnvName, StepName};
                [
                    ("onWrapEnvEnter", &self.on_wrap_env_enter, EnvName),
                    ("onWrapTaskRun", &self.on_wrap_task_run, StepName),
                    ("onWrapEnvExit", &self.on_wrap_env_exit, EnvName),
                ]
            }

            /// True iff at least one of the five actions is defined.
            pub fn has_any_action(&self) -> bool {
                self.named_slots().iter().any(|(_, slot)| slot.is_some())
            }

            /// True iff any of the three RFC 0008 wrap hooks is defined.
            pub fn has_any_wrap_hook(&self) -> bool {
                self.wrap_hooks().iter().any(|(_, slot, _)| slot.is_some())
            }
        }
    };
}
pub(crate) use impl_environment_actions_helpers;

impl_environment_actions_helpers!(EnvironmentActions, Action);
