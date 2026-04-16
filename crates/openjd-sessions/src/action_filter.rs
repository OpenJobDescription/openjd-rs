// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Action monitoring filter — parses openjd directives from stdout lines.
//!
//! Mirrors Python `_action_filter.py`.

use regex::Regex;
use std::collections::HashSet;
use std::sync::LazyLock;

/// The kind of an openjd directive message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActionMessageKind {
    Progress,
    Status,
    Fail,
    Env,
    RedactedEnv,
    UnsetEnv,
    SessionRuntimeLoglevel,
}

/// The value associated with a directive callback.
#[derive(Debug, Clone, PartialEq)]
pub enum ActionMessageValue {
    Float(f64),
    String(String),
    EnvVar { name: String, value: String },
    LogLevel(u32),
}

/// A parsed callback from the filter.
#[derive(Debug, Clone, PartialEq)]
pub struct FilterCallback {
    pub kind: ActionMessageKind,
    pub value: ActionMessageValue,
    pub cancel: bool,
}

const OPENJD_PREFIX: &str = "openjd_";

/// Known directive names mapped to their kind.
fn parse_directive(line: &str) -> Option<(ActionMessageKind, &str)> {
    let rest = line.strip_prefix(OPENJD_PREFIX)?;
    let colon_pos = rest.find(": ")?;
    let kind_str = &rest[..colon_pos];
    let payload = &rest[colon_pos + 2..];
    if payload.is_empty() {
        return None;
    }
    let kind = match kind_str {
        "progress" => ActionMessageKind::Progress,
        "status" => ActionMessageKind::Status,
        "fail" => ActionMessageKind::Fail,
        "env" => ActionMessageKind::Env,
        "redacted_env" => ActionMessageKind::RedactedEnv,
        "unset_env" => ActionMessageKind::UnsetEnv,
        "session_runtime_loglevel" => ActionMessageKind::SessionRuntimeLoglevel,
        _ => return None,
    };
    Some((kind, payload))
}

/// Check if a line is a near-miss malformed env command (wrong case, missing space, etc.).
fn is_malformed_env_command(line: &str) -> bool {
    let lower = line.trim_start().to_lowercase();
    // Require the directive name to be followed by a colon (with or without space)
    // or end-of-string, to avoid false positives on lines like "openjd_environment_setup".
    lower.starts_with("openjd_env:")
        || lower.starts_with("openjd_env ")
        || lower == "openjd_env"
        || lower.starts_with("openjd_redacted_env:")
        || lower.starts_with("openjd_redacted_env ")
        || lower == "openjd_redacted_env"
        || lower.starts_with("openjd_unset_env:")
        || lower.starts_with("openjd_unset_env ")
        || lower == "openjd_unset_env"
}

static ENVVAR_SET_REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"^"?[A-Za-z_][A-Za-z0-9_]*=.*$"#).unwrap());

static ENVVAR_UNSET_REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[A-Za-z_][A-Za-z0-9_]*$").unwrap());

/// Action monitoring filter that parses openjd directives from output lines.
///
/// Mirrors Python's `ActionMonitoringFilter`.
pub struct ActionFilter {
    session_id: String,
    suppress_filtered: bool,
    redactions_enabled: bool,
    redacted_values: HashSet<String>,
    redacted_lines: HashSet<String>,
    log_level: u32,
}

impl ActionFilter {
    pub fn new(session_id: &str, suppress_filtered: bool, redactions_enabled: bool) -> Self {
        Self {
            session_id: session_id.to_string(),
            suppress_filtered,
            redactions_enabled,
            redacted_values: HashSet::new(),
            redacted_lines: HashSet::new(),
            log_level: 20, // INFO
        }
    }

    /// Current minimum log level for command output (10=DEBUG, 20=INFO, 30=WARNING, 40=ERROR).
    pub fn min_log_level(&self) -> u32 {
        self.log_level
    }

    /// Seed the filter with previously collected redacted values.
    pub fn add_redacted_values(&mut self, values: &[String]) {
        for v in values {
            if !v.is_empty() {
                self.redacted_values.insert(v.clone());
            }
        }
    }

    /// Process a log line. Returns (callbacks, pass_through, modified_message).
    /// - callbacks: directives parsed from the line
    /// - pass_through: whether the message should be kept in the log
    /// - modified_message: the message after redaction/error annotation
    pub fn filter_message(
        &mut self,
        message: &str,
        session_id: &str,
    ) -> (Vec<FilterCallback>, bool, String) {
        if session_id != self.session_id {
            return (vec![], true, message.to_string());
        }

        let mut callbacks = Vec::new();
        let mut msg = message.to_string();
        let mut pass_through = true;

        if let Some((kind, payload)) = parse_directive(message) {
            match kind {
                ActionMessageKind::Progress => match self.handle_progress(payload) {
                    Ok(cb) => callbacks.push(cb),
                    Err(err) => {
                        msg = format!("{message} -- ERROR: {err}");
                        return (callbacks, true, self.apply_redaction(&msg));
                    }
                },
                ActionMessageKind::Status => {
                    callbacks.push(FilterCallback {
                        kind: ActionMessageKind::Status,
                        value: ActionMessageValue::String(payload.to_string()),
                        cancel: false,
                    });
                }
                ActionMessageKind::Fail => {
                    callbacks.push(FilterCallback {
                        kind: ActionMessageKind::Fail,
                        value: ActionMessageValue::String(payload.to_string()),
                        cancel: false,
                    });
                }
                ActionMessageKind::Env => match self.handle_env(payload) {
                    Ok(cb) => callbacks.push(cb),
                    Err(err) => {
                        msg = format!("{message} -- ERROR: {err}");
                        callbacks.push(FilterCallback {
                            kind: ActionMessageKind::Env,
                            value: ActionMessageValue::String(err),
                            cancel: true,
                        });
                        return (callbacks, true, self.apply_redaction(&msg));
                    }
                },
                ActionMessageKind::RedactedEnv => {
                    let (cbs, new_msg) = self.handle_redacted_env(payload, message);
                    callbacks.extend(cbs);
                    msg = new_msg;
                    // Always show the redacted line — it's safe to display
                    return (callbacks, true, msg);
                }
                ActionMessageKind::UnsetEnv => match self.handle_unset_env(payload) {
                    Ok(cb) => callbacks.push(cb),
                    Err(err) => {
                        msg = format!("{message} -- ERROR: {err}");
                        callbacks.push(FilterCallback {
                            kind: ActionMessageKind::UnsetEnv,
                            value: ActionMessageValue::String(err),
                            cancel: true,
                        });
                        return (callbacks, true, self.apply_redaction(&msg));
                    }
                },
                ActionMessageKind::SessionRuntimeLoglevel => {
                    if let Some(cb) = self.handle_loglevel(payload) {
                        callbacks.push(cb);
                    }
                }
            }
            pass_through = !self.suppress_filtered;
        } else {
            // Check for "almost" matching env commands
            if is_malformed_env_command(message) {
                let err = format!(
                    "Open Job Description: Incorrectly formatted openjd env command ({message})"
                );
                msg = format!("{message} -- ERROR: {err}");
                callbacks.push(FilterCallback {
                    kind: ActionMessageKind::Fail,
                    value: ActionMessageValue::String(err),
                    cancel: true,
                });
                return (callbacks, true, self.apply_redaction(&msg));
            }
        }

        msg = self.apply_redaction(&msg);
        (callbacks, pass_through, msg)
    }

    /// Apply redaction to a message string.
    pub fn apply_redaction(&self, message: &str) -> String {
        if self.redacted_values.is_empty() && self.redacted_lines.is_empty() {
            return message.to_string();
        }

        // Check line-level redaction
        if self.redacted_lines.contains(message) {
            return "********".to_string();
        }

        // Find all byte-offset segments to redact
        let mut segments: Vec<(usize, usize)> = Vec::new();
        for value in &self.redacted_values {
            if value.is_empty() {
                continue;
            }
            let mut start = 0;
            while let Some(pos) = message[start..].find(value.as_str()) {
                let abs_pos = start + pos;
                segments.push((abs_pos, abs_pos + value.len()));
                start = abs_pos + 1;
            }
        }

        if segments.is_empty() {
            return message.to_string();
        }

        // Sort and merge overlapping segments
        segments.sort();
        let mut merged = vec![segments[0]];
        for &(s, e) in &segments[1..] {
            let last = merged.last_mut().unwrap();
            if s <= last.1 {
                last.1 = last.1.max(e);
            } else {
                merged.push((s, e));
            }
        }

        // Apply redactions from end to start using byte offsets
        let mut result = message.to_string();
        for &(s, e) in merged.iter().rev() {
            result.replace_range(s..e, "********");
        }
        result
    }

    /// Get the set of redacted values collected during filtering.
    #[allow(dead_code)]
    pub fn redacted_values(&self) -> &HashSet<String> {
        &self.redacted_values
    }

    fn handle_progress(&self, payload: &str) -> Result<FilterCallback, String> {
        let trimmed = payload.trim();
        match trimmed.parse::<f64>() {
            Ok(v) if (0.0..=100.0).contains(&v) => Ok(FilterCallback {
                kind: ActionMessageKind::Progress,
                value: ActionMessageValue::Float(v),
                cancel: false,
            }),
            _ => Err(
                "Progress must be a floating point value between 0.0 and 100.0, inclusive."
                    .to_string(),
            ),
        }
    }

    fn parse_env_variable(message: &str) -> Result<(String, String), String> {
        let trimmed = message.trim_start();
        if !ENVVAR_SET_REGEX.is_match(trimmed) {
            if trimmed.contains('=') {
                return Err("Failed to parse environment variable assignment.".to_string());
            }
            return Err("Failed to parse environment variable assignment.".to_string());
        }

        // Handle JSON-encoded format
        if trimmed.starts_with('"') {
            let decoded: String =
                serde_json::from_str(trimmed).map_err(|e| format!("JSON decode error: {e}"))?;
            let (name, value) = decoded
                .split_once('=')
                .ok_or("Failed to parse environment variable assignment.")?;
            return Ok((name.to_string(), value.to_string()));
        }

        let (name, value) = trimmed
            .split_once('=')
            .ok_or("Failed to parse environment variable assignment.")?;
        Ok((name.to_string(), value.to_string()))
    }

    fn handle_env(&self, payload: &str) -> Result<FilterCallback, String> {
        let (name, value) = Self::parse_env_variable(payload)?;
        Ok(FilterCallback {
            kind: ActionMessageKind::Env,
            value: ActionMessageValue::EnvVar { name, value },
            cancel: false,
        })
    }

    fn handle_unset_env(&self, payload: &str) -> Result<FilterCallback, String> {
        let trimmed = payload.trim_start();
        if !ENVVAR_UNSET_REGEX.is_match(trimmed) {
            return Err("Failed to parse environment variable name.".to_string());
        }
        Ok(FilterCallback {
            kind: ActionMessageKind::UnsetEnv,
            value: ActionMessageValue::String(trimmed.to_string()),
            cancel: false,
        })
    }

    fn handle_loglevel(&mut self, payload: &str) -> Option<FilterCallback> {
        let level = match payload.trim().to_uppercase().as_str() {
            "DEBUG" => 10,
            "INFO" => 20,
            "WARNING" => 30,
            "ERROR" => 40,
            _ => return None,
        };
        self.log_level = level;
        Some(FilterCallback {
            kind: ActionMessageKind::SessionRuntimeLoglevel,
            value: ActionMessageValue::LogLevel(level),
            cancel: false,
        })
    }

    fn handle_redacted_env(
        &mut self,
        payload: &str,
        original_message: &str,
    ) -> (Vec<FilterCallback>, String) {
        let trimmed = payload.trim_start();
        let mut callbacks = Vec::new();

        let parse_result = Self::parse_env_variable(trimmed);

        match &parse_result {
            Ok((name, value)) => {
                // Add value to redaction set
                if !value.is_empty() {
                    self.redacted_values.insert(value.clone());
                    // Handle multiline values
                    let parts: Vec<&str> = value.split('\n').collect();
                    for (i, part) in parts.iter().enumerate() {
                        if !part.is_empty() {
                            if i == 0 || i == parts.len() - 1 {
                                self.redacted_values.insert(part.to_string());
                            } else {
                                self.redacted_lines.insert(part.to_string());
                            }
                        }
                    }
                }

                if !self.redactions_enabled {
                    // Still notify session about the redacted value for log redaction,
                    // but don't set the env var (handled by session based on extension flag)
                    callbacks.push(FilterCallback {
                        kind: ActionMessageKind::RedactedEnv,
                        value: ActionMessageValue::EnvVar {
                            name: name.clone(),
                            value: value.clone(),
                        },
                        cancel: false,
                    });
                    let msg = self.redact_env_message(original_message, trimmed);
                    return (callbacks, msg);
                }

                // Set env var via callback
                callbacks.push(FilterCallback {
                    kind: ActionMessageKind::RedactedEnv,
                    value: ActionMessageValue::EnvVar {
                        name: name.clone(),
                        value: value.clone(),
                    },
                    cancel: false,
                });
                let msg = self.redact_env_message(original_message, trimmed);
                (callbacks, msg)
            }
            Err(_) => {
                // Invalid format — still redact what we can
                if let Some(eq_pos) = trimmed.find('=') {
                    let val = &trimmed[eq_pos + 1..];
                    if !val.is_empty() {
                        self.redacted_values.insert(val.to_string());
                    }
                } else {
                    if !trimmed.is_empty() {
                        self.redacted_values.insert(trimmed.to_string());
                    }
                }

                let msg = self.redact_env_message(original_message, trimmed);
                callbacks.push(FilterCallback {
                    kind: ActionMessageKind::Env,
                    value: ActionMessageValue::String(
                        "Failed to parse environment variable assignment.".to_string(),
                    ),
                    cancel: true,
                });
                (callbacks, msg)
            }
        }
    }

    /// Redact the value portion of a redacted_env message.
    fn redact_env_message(&self, original: &str, payload: &str) -> String {
        if let Some(eq_pos) = payload.find('=') {
            let prefix_end = original.find(payload).unwrap_or(0) + eq_pos + 1;
            format!("{}********", &original[..prefix_end])
        } else {
            // No equals sign — redact everything after the prefix
            let token = "openjd_redacted_env: ";
            if let Some(pos) = original.find(token) {
                format!("{}********", &original[..pos + token.len()])
            } else {
                "openjd_redacted_env: ********".to_string()
            }
        }
    }
}

/// Redact sensitive information in command strings before execution.
///
/// Mirrors Python `redact_openjd_redacted_env_requests`.
pub fn redact_openjd_redacted_env_requests(command: &str) -> String {
    let token = "openjd_redacted_env:";
    match command.find(token) {
        None => command.to_string(),
        Some(pos) => format!("{} ********", &command[..pos + token.len()]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_filter(suppress: bool, redactions_enabled: bool) -> ActionFilter {
        ActionFilter::new("foo", suppress, redactions_enabled)
    }

    // === test_captures_suppress (parametrized) ===

    #[test]
    fn test_captures_suppress_progress() {
        let mut f = make_filter(true, false);
        let (cbs, pass, _) = f.filter_message("openjd_progress: 50.0", "foo");
        assert_eq!(cbs.len(), 1);
        assert_eq!(cbs[0].kind, ActionMessageKind::Progress);
        assert_eq!(cbs[0].value, ActionMessageValue::Float(50.0));
        assert!(!cbs[0].cancel);
        assert!(!pass, "Message should be suppressed");
    }

    #[test]
    fn test_captures_suppress_status() {
        let mut f = make_filter(true, false);
        let (cbs, pass, _) = f.filter_message("openjd_status: a status string", "foo");
        assert_eq!(cbs.len(), 1);
        assert_eq!(cbs[0].kind, ActionMessageKind::Status);
        assert_eq!(
            cbs[0].value,
            ActionMessageValue::String("a status string".into())
        );
        assert!(!pass);
    }

    #[test]
    fn test_captures_suppress_fail() {
        let mut f = make_filter(true, false);
        let (cbs, pass, _) = f.filter_message("openjd_fail: an error message", "foo");
        assert_eq!(cbs.len(), 1);
        assert_eq!(cbs[0].kind, ActionMessageKind::Fail);
        assert_eq!(
            cbs[0].value,
            ActionMessageValue::String("an error message".into())
        );
        assert!(!pass);
    }

    #[test]
    fn test_captures_suppress_env() {
        let mut f = make_filter(true, false);
        let (cbs, pass, _) = f.filter_message("openjd_env: foo=bar", "foo");
        assert_eq!(cbs.len(), 1);
        assert_eq!(cbs[0].kind, ActionMessageKind::Env);
        assert_eq!(
            cbs[0].value,
            ActionMessageValue::EnvVar {
                name: "foo".into(),
                value: "bar".into()
            }
        );
        assert!(!pass);
    }

    #[test]
    fn test_captures_suppress_env_allowable_chars() {
        let mut f = make_filter(true, false);
        let (cbs, _, _) = f.filter_message("openjd_env: F_F_12=bar", "foo");
        assert_eq!(
            cbs[0].value,
            ActionMessageValue::EnvVar {
                name: "F_F_12".into(),
                value: "bar".into()
            }
        );
    }

    #[test]
    fn test_captures_suppress_env_assign_empty() {
        let mut f = make_filter(true, false);
        let (cbs, _, _) = f.filter_message("openjd_env: foo=", "foo");
        assert_eq!(
            cbs[0].value,
            ActionMessageValue::EnvVar {
                name: "foo".into(),
                value: "".into()
            }
        );
    }

    #[test]
    fn test_captures_suppress_env_assign_whitespace() {
        let mut f = make_filter(true, false);
        let (cbs, _, _) = f.filter_message("openjd_env: foo= ", "foo");
        assert_eq!(
            cbs[0].value,
            ActionMessageValue::EnvVar {
                name: "foo".into(),
                value: " ".into()
            }
        );
    }

    #[test]
    fn test_captures_suppress_env_leading_whitespace() {
        let mut f = make_filter(true, false);
        let (cbs, _, _) = f.filter_message("openjd_env:  \t foo=bar", "foo");
        assert_eq!(
            cbs[0].value,
            ActionMessageValue::EnvVar {
                name: "foo".into(),
                value: "bar".into()
            }
        );
    }

    #[test]
    fn test_captures_suppress_unset_env() {
        let mut f = make_filter(true, false);
        let (cbs, pass, _) = f.filter_message("openjd_unset_env: foo", "foo");
        assert_eq!(cbs[0].kind, ActionMessageKind::UnsetEnv);
        assert_eq!(cbs[0].value, ActionMessageValue::String("foo".into()));
        assert!(!pass);
    }

    #[test]
    fn test_captures_suppress_unset_env_allowable_chars() {
        let mut f = make_filter(true, false);
        let (cbs, _, _) = f.filter_message("openjd_unset_env: F_F_12", "foo");
        assert_eq!(cbs[0].value, ActionMessageValue::String("F_F_12".into()));
    }

    #[test]
    fn test_captures_suppress_unset_env_leading_whitespace() {
        let mut f = make_filter(true, false);
        let (cbs, _, _) = f.filter_message("openjd_unset_env:  \t foo", "foo");
        assert_eq!(cbs[0].value, ActionMessageValue::String("foo".into()));
    }

    #[test]
    fn test_captures_suppress_loglevel_debug() {
        let mut f = make_filter(true, false);
        let (cbs, _, _) = f.filter_message("openjd_session_runtime_loglevel: DEBUG", "foo");
        assert_eq!(cbs[0].kind, ActionMessageKind::SessionRuntimeLoglevel);
        assert_eq!(cbs[0].value, ActionMessageValue::LogLevel(10));
    }

    #[test]
    fn test_captures_suppress_loglevel_info() {
        let mut f = make_filter(true, false);
        let (cbs, _, _) = f.filter_message("openjd_session_runtime_loglevel: INFO", "foo");
        assert_eq!(cbs[0].value, ActionMessageValue::LogLevel(20));
    }

    #[test]
    fn test_captures_suppress_loglevel_warning() {
        let mut f = make_filter(true, false);
        let (cbs, _, _) = f.filter_message("openjd_session_runtime_loglevel: WARNING", "foo");
        assert_eq!(cbs[0].value, ActionMessageValue::LogLevel(30));
    }

    #[test]
    fn test_captures_suppress_loglevel_error() {
        let mut f = make_filter(true, false);
        let (cbs, _, _) = f.filter_message("openjd_session_runtime_loglevel: ERROR", "foo");
        assert_eq!(cbs[0].value, ActionMessageValue::LogLevel(40));
    }

    // === test_ignores_different_session ===

    #[test]
    fn test_ignores_different_session() {
        let mut f = make_filter(true, false);
        let (cbs, pass, _) = f.filter_message("openjd_fail: an error message", "other_session");
        assert!(cbs.is_empty());
        assert!(pass);
    }

    // === test_captures_no_suppress (parametrized) ===

    #[test]
    fn test_captures_no_suppress_progress() {
        let mut f = make_filter(false, false);
        let (cbs, pass, msg) = f.filter_message("openjd_progress: 50.0", "foo");
        assert_eq!(cbs.len(), 1);
        assert_eq!(cbs[0].value, ActionMessageValue::Float(50.0));
        assert!(pass, "Message should pass through");
        assert_eq!(msg, "openjd_progress: 50.0");
    }

    #[test]
    fn test_captures_no_suppress_env() {
        let mut f = make_filter(false, false);
        let (cbs, pass, msg) = f.filter_message("openjd_env: foo=bar", "foo");
        assert_eq!(cbs.len(), 1);
        assert!(pass);
        assert_eq!(msg, "openjd_env: foo=bar");
    }

    // === test_malformed_does_not_match_no_callback ===

    #[test]
    fn test_malformed_progress_no_space() {
        let mut f = make_filter(false, false);
        let (cbs, _, _) = f.filter_message("openjd_progress:50.0", "foo");
        assert!(cbs.is_empty());
    }

    #[test]
    fn test_malformed_progress_uppercase() {
        let mut f = make_filter(false, false);
        let (cbs, _, _) = f.filter_message("OPENJD_PROGRESS: 50.0", "foo");
        assert!(cbs.is_empty());
    }

    #[test]
    fn test_malformed_progress_leading_whitespace() {
        let mut f = make_filter(false, false);
        let (cbs, _, _) = f.filter_message(" openjd_progress: 50.0", "foo");
        assert!(cbs.is_empty());
    }

    #[test]
    fn test_malformed_status_no_space() {
        let mut f = make_filter(false, false);
        let (cbs, _, _) = f.filter_message("openjd_status:a status string", "foo");
        assert!(cbs.is_empty());
    }

    #[test]
    fn test_malformed_fail_no_space() {
        let mut f = make_filter(false, false);
        let (cbs, _, _) = f.filter_message("openjd_fail:an error message", "foo");
        assert!(cbs.is_empty());
    }

    // === test_malformed_set_env_assignment ===

    #[test]
    fn test_malformed_env_missing_assignment() {
        let mut f = make_filter(false, false);
        let (cbs, _, _) = f.filter_message("openjd_env: foo", "foo");
        assert_eq!(cbs.len(), 1);
        assert_eq!(cbs[0].kind, ActionMessageKind::Env);
        assert_eq!(
            cbs[0].value,
            ActionMessageValue::String("Failed to parse environment variable assignment.".into())
        );
        assert!(cbs[0].cancel);
    }

    #[test]
    fn test_malformed_env_extra_whitespace() {
        let mut f = make_filter(false, false);
        let (cbs, _, _) = f.filter_message("openjd_env: foo =value", "foo");
        assert_eq!(
            cbs[0].value,
            ActionMessageValue::String("Failed to parse environment variable assignment.".into())
        );
        assert!(cbs[0].cancel);
    }

    #[test]
    fn test_malformed_env_start_with_digit() {
        let mut f = make_filter(false, false);
        let (cbs, _, _) = f.filter_message("openjd_env: 1F_F_12=bar", "foo");
        assert_eq!(
            cbs[0].value,
            ActionMessageValue::String("Failed to parse environment variable assignment.".into())
        );
        assert!(cbs[0].cancel);
    }

    #[test]
    fn test_malformed_redacted_env_missing_assignment() {
        let mut f = make_filter(false, false);
        let (cbs, _, _) = f.filter_message("openjd_redacted_env: foo", "foo");
        // When redactions not enabled, should get env error callback
        assert!(!cbs.is_empty());
    }

    // === test_malformed_openjd_regex ===

    #[test]
    fn test_malformed_env_no_space_after_colon() {
        let mut f = make_filter(false, false);
        let (cbs, _, _) = f.filter_message("openjd_env:foo=bar", "foo");
        assert_eq!(cbs.len(), 1);
        assert_eq!(cbs[0].kind, ActionMessageKind::Fail);
        assert!(cbs[0].cancel);
        if let ActionMessageValue::String(ref s) = cbs[0].value {
            assert!(s.contains("Incorrectly formatted openjd env command"));
        }
    }

    #[test]
    fn test_malformed_env_uppercase() {
        let mut f = make_filter(false, false);
        let (cbs, _, _) = f.filter_message("OPENJD_ENV: foo=bar", "foo");
        assert_eq!(cbs.len(), 1);
        assert_eq!(cbs[0].kind, ActionMessageKind::Fail);
        assert!(cbs[0].cancel);
    }

    #[test]
    fn test_malformed_env_leading_whitespace() {
        let mut f = make_filter(false, false);
        let (cbs, _, _) = f.filter_message(" openjd_env: foo=bar", "foo");
        assert_eq!(cbs.len(), 1);
        assert_eq!(cbs[0].kind, ActionMessageKind::Fail);
        assert!(cbs[0].cancel);
    }

    #[test]
    fn test_malformed_unset_env_no_space() {
        let mut f = make_filter(false, false);
        let (cbs, _, _) = f.filter_message("openjd_unset_env:foo", "foo");
        assert_eq!(cbs[0].kind, ActionMessageKind::Fail);
        assert!(cbs[0].cancel);
    }

    #[test]
    fn test_malformed_unset_env_uppercase() {
        let mut f = make_filter(false, false);
        let (cbs, _, _) = f.filter_message("OPENJD_UNSET_ENV: foo", "foo");
        assert_eq!(cbs[0].kind, ActionMessageKind::Fail);
        assert!(cbs[0].cancel);
    }

    // === test_malformed_does_not_match_unset_env ===

    #[test]
    fn test_malformed_unset_env_bad_value() {
        let mut f = make_filter(false, false);
        let (cbs, _, _) = f.filter_message("openjd_unset_env: foo=bar", "foo");
        assert_eq!(cbs[0].kind, ActionMessageKind::UnsetEnv);
        assert_eq!(
            cbs[0].value,
            ActionMessageValue::String("Failed to parse environment variable name.".into())
        );
        assert!(cbs[0].cancel);
    }

    #[test]
    fn test_malformed_unset_env_start_with_digit() {
        let mut f = make_filter(false, false);
        let (cbs, _, _) = f.filter_message("openjd_unset_env: 1F_F_12", "foo");
        assert_eq!(
            cbs[0].value,
            ActionMessageValue::String("Failed to parse environment variable name.".into())
        );
        assert!(cbs[0].cancel);
    }

    // === test_progress_appends_error ===

    #[test]
    fn test_progress_not_a_float() {
        let mut f = make_filter(false, false);
        let (cbs, pass, msg) = f.filter_message("openjd_progress: fifty", "foo");
        assert!(cbs.is_empty());
        assert!(pass);
        assert!(msg.contains("ERROR: Progress must be a floating point value"));
    }

    #[test]
    fn test_progress_too_small() {
        let mut f = make_filter(false, false);
        let (cbs, pass, msg) = f.filter_message("openjd_progress: -0.01", "foo");
        assert!(cbs.is_empty());
        assert!(pass);
        assert!(msg.contains("ERROR:"));
    }

    #[test]
    fn test_progress_too_big() {
        let mut f = make_filter(false, false);
        let (cbs, pass, msg) = f.filter_message("openjd_progress: 100.1", "foo");
        assert!(cbs.is_empty());
        assert!(pass);
        assert!(msg.contains("ERROR:"));
    }

    // === test_redacted_env_redacts_value ===

    #[test]
    fn test_redacted_env_redacts_value() {
        let mut f = make_filter(false, true);
        let (cbs, _, msg) = f.filter_message("openjd_redacted_env: PASSWORD=secret123", "foo");
        // Should callback with redacted env var
        assert_eq!(cbs.len(), 1);
        assert_eq!(cbs[0].kind, ActionMessageKind::RedactedEnv);
        assert_eq!(
            cbs[0].value,
            ActionMessageValue::EnvVar {
                name: "PASSWORD".into(),
                value: "secret123".into()
            }
        );
        // Message should be redacted
        assert!(msg.contains("PASSWORD=********"));
        assert!(!msg.contains("secret123"));
    }

    // === test_redacted_env_with_warning ===

    #[test]
    fn test_redacted_env_with_warning_no_extension() {
        let mut f = make_filter(false, false);
        let (cbs, _, msg) = f.filter_message("openjd_redacted_env: SECRET_VAR=secret_value", "foo");
        // RedactedEnv callback returned so session can track value for redaction
        assert_eq!(cbs.len(), 1);
        assert_eq!(cbs[0].kind, ActionMessageKind::RedactedEnv);
        // Message should still be redacted
        assert!(msg.contains("SECRET_VAR=********"));
        assert!(!msg.contains("secret_value"));
    }

    // === test_redacted_env_uses_fixed_length_redaction ===

    #[test]
    fn test_redacted_env_uses_fixed_length_redaction() {
        let mut f = make_filter(false, false);
        let (_, _, msg1) = f.filter_message("openjd_redacted_env: KEY=x", "foo");
        let (_, _, msg2) = f.filter_message(
            "openjd_redacted_env: TOKEN=abcdefghijklmnopqrstuvwxyz1234567890",
            "foo",
        );
        assert_eq!(msg1, "openjd_redacted_env: KEY=********");
        assert_eq!(msg2, "openjd_redacted_env: TOKEN=********");
    }

    // === test_redacted_env_redacts_subsequent_occurrences ===

    #[test]
    fn test_redacted_env_redacts_subsequent_occurrences() {
        let mut f = make_filter(false, true);
        let (_, _, msg1) = f.filter_message("openjd_redacted_env: PASSWORD=supersecret123", "foo");
        assert!(!msg1.contains("supersecret123"));

        let (_, _, msg2) = f.filter_message(
            "Here is the password: supersecret123 for your reference",
            "foo",
        );
        assert!(!msg2.contains("supersecret123"));
        assert!(msg2.contains("Here is the password: ********"));
    }

    // === test_redacted_env_handles_multiple_values ===

    #[test]
    fn test_redacted_env_handles_multiple_values() {
        let mut f = make_filter(false, true);
        f.filter_message("openjd_redacted_env: PASSWORD=password123", "foo");
        f.filter_message("openjd_redacted_env: API_KEY=abcdef123456", "foo");

        let (_, _, msg) = f.filter_message(
            "Using PASSWORD=password123 and API_KEY=abcdef123456 for authentication",
            "foo",
        );
        assert!(!msg.contains("password123"));
        assert!(!msg.contains("abcdef123456"));
        assert!(msg.contains("Using PASSWORD=******** and API_KEY=******** for authentication"));
    }

    // === test_redacted_env_with_extension ===

    #[test]
    fn test_redacted_env_with_extension() {
        let mut f = make_filter(false, true);
        let (cbs, _, msg) = f.filter_message("openjd_redacted_env: PASSWORD=secret123", "foo");
        assert_eq!(cbs.len(), 1);
        assert_eq!(
            cbs[0].value,
            ActionMessageValue::EnvVar {
                name: "PASSWORD".into(),
                value: "secret123".into()
            }
        );
        assert!(msg.contains("PASSWORD=********"));
        assert!(!msg.contains("secret123"));
    }

    // === test_malformed_redacted_env_commands ===

    #[test]
    fn test_malformed_redacted_env_space_after_key() {
        let mut f = make_filter(false, true);
        let (_, _, msg) = f.filter_message("openjd_redacted_env: PASSWORD =secret123", "foo");
        assert!(msg.contains("PASSWORD =********"));
        assert!(!msg.contains("secret123"));
    }

    #[test]
    fn test_malformed_redacted_env_missing_equals() {
        let mut f = make_filter(false, true);
        let (_, _, msg) = f.filter_message("openjd_redacted_env: SECRETsensitivedata", "foo");
        assert!(msg.contains("openjd_redacted_env: ********"));
        assert!(!msg.contains("SECRETsensitivedata"));
    }

    // === test_redact_openjd_redacted_env_requests ===

    #[test]
    fn test_redact_command_no_redaction_needed() {
        assert_eq!(
            redact_openjd_redacted_env_requests("echo hello world"),
            "echo hello world"
        );
    }

    #[test]
    fn test_redact_command_with_redacted_env() {
        let cmd = "python -c \"print('openjd_redacted_env: PASSWORD=secret123')\"";
        assert_eq!(
            redact_openjd_redacted_env_requests(cmd),
            "python -c \"print('openjd_redacted_env: ********"
        );
    }

    #[test]
    fn test_redact_command_multiple_redacted_env() {
        let cmd = r#"echo "openjd_redacted_env: PASSWORD=secret123"; echo "openjd_redacted_env: API_KEY=abc123""#;
        assert_eq!(
            redact_openjd_redacted_env_requests(cmd),
            r#"echo "openjd_redacted_env: ********"#
        );
    }

    // === test_basic_redacted_env ===

    #[test]
    fn test_basic_redacted_env() {
        let mut f = make_filter(false, true);
        let (cbs, _, msg) = f.filter_message("openjd_redacted_env: KEY=VALUE", "foo");
        assert_eq!(cbs.len(), 1);
        assert_eq!(
            cbs[0].value,
            ActionMessageValue::EnvVar {
                name: "KEY".into(),
                value: "VALUE".into()
            }
        );
        assert!(!msg.contains("VALUE") || msg.contains("********"));
    }

    #[test]
    fn test_redacted_values_accessor() {
        let mut f = make_filter(false, true);
        assert!(f.redacted_values().is_empty());
        f.filter_message("openjd_redacted_env: SECRET=hunter2", "foo");
        assert!(f.redacted_values().contains("hunter2"));
    }

    // === test_redacted_env_edge_cases ===

    #[test]
    fn test_edge_case_space_after_equals() {
        let mut f = make_filter(false, true);
        let (cbs, _, msg) = f.filter_message("openjd_redacted_env: KEY= VALUE", "foo");
        assert_eq!(cbs.len(), 1);
        assert_eq!(
            cbs[0].value,
            ActionMessageValue::EnvVar {
                name: "KEY".into(),
                value: " VALUE".into()
            }
        );
        // Subsequent log should redact
        let (_, _, msg2) = f.filter_message("The value is:  VALUE", "foo");
        assert!(!msg2.contains(" VALUE") || msg2.contains("********"));
        let _ = msg;
    }

    #[test]
    fn test_edge_case_space_before_equals() {
        let mut f = make_filter(false, true);
        let (cbs, _, msg) = f.filter_message("openjd_redacted_env: KEY =VALUE", "foo");
        // Should not set env var (invalid format)
        let env_cbs: Vec<_> = cbs
            .iter()
            .filter(|c| matches!(c.value, ActionMessageValue::EnvVar { .. }))
            .collect();
        assert!(env_cbs.is_empty());
        // But should still redact VALUE in subsequent logs
        let (_, _, msg2) = f.filter_message("The value is: VALUE", "foo");
        assert!(!msg2.contains("VALUE") || msg2.contains("********"));
        let _ = msg;
    }

    #[test]
    fn test_edge_case_no_equals() {
        let mut f = make_filter(false, true);
        let (cbs, _, _) = f.filter_message("openjd_redacted_env: KEYVALUE", "foo");
        let env_cbs: Vec<_> = cbs
            .iter()
            .filter(|c| matches!(c.value, ActionMessageValue::EnvVar { .. }))
            .collect();
        assert!(env_cbs.is_empty());
    }

    #[test]
    fn test_edge_case_multiple_equals() {
        let mut f = make_filter(false, true);
        let (cbs, _, _) = f.filter_message("openjd_redacted_env: KEY=VALUE=MORE", "foo");
        assert_eq!(cbs.len(), 1);
        assert_eq!(
            cbs[0].value,
            ActionMessageValue::EnvVar {
                name: "KEY".into(),
                value: "VALUE=MORE".into()
            }
        );
    }

    #[test]
    fn test_edge_case_empty_value() {
        let mut f = make_filter(false, true);
        let (cbs, _, _) = f.filter_message("openjd_redacted_env: KEY=", "foo");
        assert_eq!(cbs.len(), 1);
        assert_eq!(
            cbs[0].value,
            ActionMessageValue::EnvVar {
                name: "KEY".into(),
                value: "".into()
            }
        );
    }

    // === test_redacted_env_special_values ===

    #[test]
    fn test_special_chars() {
        let mut f = make_filter(false, true);
        let val = "p@$$w0rd!*&^%";
        let (cbs, _, msg) =
            f.filter_message(&format!("openjd_redacted_env: TEST_VAR={val}"), "foo");
        assert_eq!(cbs.len(), 1);
        assert!(!msg.contains(val));
        assert!(msg.contains("********"));
    }

    #[test]
    fn test_windows_paths() {
        let mut f = make_filter(false, true);
        let val = "C:\\Program Files\\App\\bin;D:\\Tools";
        let (cbs, _, msg) =
            f.filter_message(&format!("openjd_redacted_env: TEST_VAR={val}"), "foo");
        assert_eq!(cbs.len(), 1);
        assert!(!msg.contains(val));
    }

    // === test_redacted_env_json_format ===

    #[test]
    fn test_json_format_standard() {
        let mut f = make_filter(false, true);
        let (cbs, _, msg) = f.filter_message(r#"openjd_redacted_env: "FOO=BAR""#, "foo");
        let env_cbs: Vec<_> = cbs
            .iter()
            .filter(|c| matches!(c.value, ActionMessageValue::EnvVar { .. }))
            .collect();
        assert!(!env_cbs.is_empty());
        assert!(!msg.contains("BAR") || msg.contains("********"));
    }

    #[test]
    fn test_json_format_with_newline() {
        let mut f = make_filter(false, true);
        let (cbs, _, msg) = f.filter_message(r#"openjd_redacted_env: "FOO=BAR\nBAZ""#, "foo");
        let env_cbs: Vec<_> = cbs
            .iter()
            .filter(|c| matches!(c.value, ActionMessageValue::EnvVar { .. }))
            .collect();
        assert!(!env_cbs.is_empty());
        assert!(!msg.contains("BAR"));
        assert!(!msg.contains("BAZ"));
    }

    #[test]
    fn test_json_format_empty_value() {
        let mut f = make_filter(false, true);
        let (cbs, _, _) = f.filter_message(r#"openjd_redacted_env: "FOO=""#, "foo");
        let env_cbs: Vec<_> = cbs
            .iter()
            .filter(|c| matches!(c.value, ActionMessageValue::EnvVar { .. }))
            .collect();
        assert!(!env_cbs.is_empty());
    }

    // === test_subsequent_redaction ===

    #[test]
    fn test_subsequent_redaction() {
        let mut f = make_filter(false, true);
        f.filter_message("openjd_redacted_env: API_KEY=abcdef123456", "foo");
        let (_, _, msg) = f.filter_message("Using API key: abcdef123456", "foo");
        assert!(!msg.contains("abcdef123456"));
        assert!(msg.contains("Using API key: ********"));
    }

    // === test_redaction_persists_after_unset ===

    #[test]
    fn test_redaction_persists_after_unset() {
        let mut f = make_filter(false, true);
        f.filter_message("openjd_redacted_env: SECRETVAR=SECRETVAL", "foo");
        // Unset the variable
        let (cbs, _, _) = f.filter_message("openjd_unset_env: SECRETVAR", "foo");
        assert_eq!(cbs[0].kind, ActionMessageKind::UnsetEnv);
        // Value should still be redacted
        let (_, _, msg) = f.filter_message("The value is: SECRETVAL", "foo");
        assert!(msg.contains("The value is: ********"));
        assert!(!msg.contains("SECRETVAL"));
    }

    // === test_redacted_env_with_linebreak ===

    #[test]
    fn test_redacted_env_with_linebreak() {
        let mut f = make_filter(false, true);
        f.filter_message(r#"openjd_redacted_env: "SECRETVAR2=line\nbreak""#, "foo");
        let (_, _, msg) = f.filter_message("We set SECRETVAR2 to line\nbreak", "foo");
        assert!(!msg.contains("line"));
        assert!(!msg.contains("break"));
    }

    // === test_redacted_env_with_multiline_redaction ===

    #[test]
    fn test_multiline_first_part_redacted() {
        let mut f = make_filter(false, true);
        f.filter_message(
            r#"openjd_redacted_env: "SECRETVAR=first_line\nsecond_line\nthird_line""#,
            "foo",
        );
        let (_, _, msg) = f.filter_message("The first part is: first_line", "foo");
        assert!(!msg.contains("first_line"));
        assert!(msg.contains("The first part is: ********"));
    }

    #[test]
    fn test_multiline_middle_line_exact_match() {
        let mut f = make_filter(false, true);
        f.filter_message(
            r#"openjd_redacted_env: "SECRETVAR=first_line\nsecond_line\nthird_line""#,
            "foo",
        );
        // Middle line by itself should be fully redacted (line-level redaction)
        let (_, _, msg) = f.filter_message("second_line", "foo");
        assert_eq!(msg, "********");
    }

    #[test]
    fn test_multiline_middle_line_with_prefix_not_redacted() {
        let mut f = make_filter(false, true);
        f.filter_message(
            r#"openjd_redacted_env: "SECRETVAR=first_line\nsecond_line\nthird_line""#,
            "foo",
        );
        // Middle line with prefix should NOT be redacted (only exact match)
        let (_, _, msg) = f.filter_message("Prefix second_line", "foo");
        assert!(msg.contains("Prefix second_line"));
    }

    // === test_redacted_env_with_multiline_redaction_last_part ===

    #[test]
    fn test_multiline_last_part_with_prefix() {
        let mut f = make_filter(false, true);
        f.filter_message(
            r#"openjd_redacted_env: "SECRETVAR=first_line\nmiddle_line\nlast_line""#,
            "foo",
        );
        // Last line with prefix should be redacted (first/last go in regular redaction set)
        let (_, _, msg) = f.filter_message("Prefix last_line", "foo");
        assert!(!msg.contains("last_line"));
        assert!(msg.contains("Prefix ********"));
    }

    #[test]
    fn test_multiline_middle_with_prefix_not_redacted() {
        let mut f = make_filter(false, true);
        f.filter_message(
            r#"openjd_redacted_env: "SECRETVAR=first_line\nmiddle_line\nlast_line""#,
            "foo",
        );
        // Middle line with prefix should NOT be redacted
        let (_, _, msg) = f.filter_message("Prefix middle_line", "foo");
        assert!(msg.contains("Prefix middle_line"));
    }

    // === test_env_redacted_env_consistency ===

    #[test]
    fn test_consistency_standard_format() {
        let mut f_env = ActionFilter::new("foo", false, false);
        let mut f_red = ActionFilter::new("foo", false, true);

        let (cbs_env, _, _) = f_env.filter_message("openjd_env: KEY=VALUE", "foo");
        let (cbs_red, _, _) = f_red.filter_message("openjd_redacted_env: KEY=VALUE", "foo");

        // Both should produce env var callbacks with same name/value
        let env_vars: Vec<_> = cbs_env
            .iter()
            .filter(|c| matches!(c.value, ActionMessageValue::EnvVar { .. }))
            .collect();
        let red_vars: Vec<_> = cbs_red
            .iter()
            .filter(|c| matches!(c.value, ActionMessageValue::EnvVar { .. }))
            .collect();
        assert_eq!(env_vars.len(), 1);
        assert_eq!(red_vars.len(), 1);
        assert_eq!(env_vars[0].value, red_vars[0].value);
    }

    #[test]
    fn test_consistency_failure_cases() {
        let mut f_env = ActionFilter::new("foo", false, false);
        let mut f_red = ActionFilter::new("foo", false, true);

        // Invalid format: space before equals
        let (cbs_env, _, _) = f_env.filter_message("openjd_env: KEY =VALUE", "foo");
        let (cbs_red, _, _) = f_red.filter_message("openjd_redacted_env: KEY =VALUE", "foo");

        let env_vars: Vec<_> = cbs_env
            .iter()
            .filter(|c| matches!(c.value, ActionMessageValue::EnvVar { .. }))
            .collect();
        let red_vars: Vec<_> = cbs_red
            .iter()
            .filter(|c| matches!(c.value, ActionMessageValue::EnvVar { .. }))
            .collect();
        assert!(env_vars.is_empty());
        assert!(red_vars.is_empty());
    }

    #[test]
    fn test_redact_no_redaction() {
        assert_eq!(
            redact_openjd_redacted_env_requests("echo hello world"),
            "echo hello world"
        );
    }

    #[test]
    fn test_redact_with_redacted_env() {
        let cmd = "python -c \"print('openjd_redacted_env: PASSWORD=secret123')\"";
        assert_eq!(
            redact_openjd_redacted_env_requests(cmd),
            "python -c \"print('openjd_redacted_env: ********"
        );
    }

    // === test_redaction_with_string_formatting ===
    // In Rust, we don't have Python's % formatting, but we test the equivalent:
    // apply_redaction on pre-formatted strings

    #[test]
    fn test_redaction_after_formatting() {
        let mut f = ActionFilter::new("s", false, true);
        f.filter_message("openjd_redacted_env: PASSWORD=secret123", "s");

        // Simulate formatted string
        let (_, _, msg) = f.filter_message("Command: echo secret123", "s");
        assert_eq!(msg, "Command: echo ********");
    }

    #[test]
    fn test_redaction_multiple_occurrences() {
        let mut f = ActionFilter::new("s", false, true);
        f.filter_message("openjd_redacted_env: PASSWORD=secret123", "s");

        let (_, _, msg) = f.filter_message("First: secret123, Second: hello", "s");
        assert_eq!(msg, "First: ********, Second: hello");
    }

    // === TestRedactionCore::test_redaction_preserves_spaces ===

    #[test]
    fn test_redaction_preserves_spaces() {
        let mut f = ActionFilter::new("s", false, true);
        f.filter_message("openjd_redacted_env: SECRETVAR=SECRETVAL", "s");

        let (_, _, msg) = f.filter_message("SECRETVAR is . SECRETVAL ;", "s");
        assert!(msg.contains("SECRETVAR is . ******** ;"));
        assert!(!msg.contains("SECRETVAL"));
    }

    // === TestRedactionCore::test_overlapping_redactions ===

    #[test]
    fn test_overlapping_redactions_at_boundary() {
        let mut f = ActionFilter::new("s", false, true);
        f.filter_message("openjd_redacted_env: KEY1=FOOOBAR", "s");
        f.filter_message("openjd_redacted_env: KEY2=BARKEY", "s");

        let (_, _, msg) = f.filter_message("The value is: FOOOBARKEY", "s");
        assert!(msg.contains("The value is: ********"));
        assert!(!msg.contains("FOOOBARKEY"));
    }

    #[test]
    fn test_overlapping_redactions_nested() {
        let mut f = ActionFilter::new("s", false, true);
        f.filter_message("openjd_redacted_env: KEY3=SUPERSECRETPASSWORD", "s");
        f.filter_message("openjd_redacted_env: KEY4=SECRET", "s");

        let (_, _, msg) = f.filter_message("The value is: SUPERSECRETPASSWORD", "s");
        assert!(msg.contains("The value is: ********"));
        assert!(!msg.contains("SUPERSECRETPASSWORD"));
    }

    #[test]
    fn test_session_runtime_loglevel_changes_min_level() {
        let mut f = ActionFilter::new("s", false, false);
        assert_eq!(f.min_log_level(), 20); // default INFO
        f.filter_message("openjd_session_runtime_loglevel: DEBUG", "s");
        assert_eq!(f.min_log_level(), 10);
        f.filter_message("openjd_session_runtime_loglevel: WARNING", "s");
        assert_eq!(f.min_log_level(), 30);
        f.filter_message("openjd_session_runtime_loglevel: ERROR", "s");
        assert_eq!(f.min_log_level(), 40);
    }

    #[test]
    fn test_session_runtime_loglevel_unknown_ignored() {
        let mut f = ActionFilter::new("s", false, false);
        f.filter_message("openjd_session_runtime_loglevel: BOGUS", "s");
        assert_eq!(f.min_log_level(), 20); // unchanged
    }

    // === Multibyte UTF-8 redaction tests ===

    #[test]
    fn test_redaction_with_multibyte_prefix() {
        let mut f = ActionFilter::new("s", false, true);
        f.filter_message("openjd_redacted_env: KEY=secret", "s");
        let (_, _, msg) = f.filter_message("Ünïcödé secret here", "s");
        assert_eq!(msg, "Ünïcödé ******** here");
    }

    #[test]
    fn test_redaction_with_many_multibyte_chars() {
        let mut f = ActionFilter::new("s", false, true);
        f.filter_message("openjd_redacted_env: KEY=secret", "s");
        let (_, _, msg) = f.filter_message("ääääääääää secret", "s");
        assert_eq!(msg, "ääääääääää ********");
    }

    #[test]
    fn test_redaction_multibyte_value() {
        let mut f = ActionFilter::new("s", false, true);
        f.filter_message("openjd_redacted_env: KEY=sëcrét", "s");
        let (_, _, msg) = f.filter_message("the password is sëcrét ok", "s");
        assert_eq!(msg, "the password is ******** ok");
    }

    #[test]
    fn test_redaction_multibyte_both() {
        let mut f = ActionFilter::new("s", false, true);
        f.filter_message("openjd_redacted_env: KEY=pässwörd", "s");
        let (_, _, msg) = f.filter_message("ünïcödé pässwörd täïl", "s");
        assert_eq!(msg, "ünïcödé ******** täïl");
    }

    #[test]
    fn test_malformed_env_command_no_false_positive_on_prefix() {
        // "openjd_environment_setup" starts with "openjd_env" but is NOT a malformed
        // env command — it's a legitimate log line. Should pass through with no callbacks.
        let mut f = make_filter(false, false);
        let (cbs, pass, _) = f.filter_message("openjd_environment_setup complete", "foo");
        assert!(
            cbs.is_empty(),
            "should not trigger malformed detection: {cbs:?}"
        );
        assert!(pass);
    }

    #[test]
    fn test_malformed_redacted_env_when_redactions_enabled() {
        // When redactions ARE enabled, a malformed openjd_redacted_env should still
        // push a cancel callback — not silently swallow the error.
        let mut f = make_filter(false, true);
        let (cbs, _, _) = f.filter_message("openjd_redacted_env: bad_no_equals", "foo");
        assert!(
            cbs.iter().any(|cb| cb.cancel),
            "malformed redacted_env with redactions enabled should cancel: {cbs:?}"
        );
    }
}
