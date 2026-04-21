// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Constrained string types per spec §7.

use crate::error::ModelError;
use regex::Regex;
use serde::de::{self, Deserializer};
use std::sync::LazyLock;

/// §7.1 Identifier: `[A-Za-z_][A-Za-z0-9_]*`, length 1..=512 (64 base, 512 with FEATURE_BUNDLE_1)
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Identifier(pub String);

static IDENTIFIER_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[A-Za-z_][A-Za-z0-9_]*$").unwrap());

impl Identifier {
    pub fn new(s: &str) -> Result<Self, ModelError> {
        if s.is_empty() || s.len() > 512 {
            return Err(ModelError::DecodeValidation(format!(
                "Identifier length must be 1..=512, got {}",
                s.len()
            )));
        }
        if !IDENTIFIER_RE.is_match(s) {
            return Err(ModelError::DecodeValidation(format!(
                "Identifier '{s}' does not match pattern [A-Za-z_][A-Za-z0-9_]*"
            )));
        }
        Ok(Self(s.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> serde::Deserialize<'de> for Identifier {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        Identifier::new(&s).map_err(de::Error::custom)
    }
}

impl serde::Serialize for Identifier {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.0.serialize(serializer)
    }
}

impl std::fmt::Display for Identifier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// §7.2 Description: any unicode except Cc category, length 0..=2048
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Description(pub String);

impl Description {
    pub fn new(s: &str) -> Result<Self, ModelError> {
        if s.chars().count() > 2048 {
            return Err(ModelError::DecodeValidation(
                "Description exceeds 2048 characters".into(),
            ));
        }
        if s.chars()
            .any(|c| c.is_control() && c != '\n' && c != '\r' && c != '\t')
        {
            return Err(ModelError::DecodeValidation(
                "Description contains control characters".into(),
            ));
        }
        Ok(Self(s.to_string()))
    }
}

impl<'de> serde::Deserialize<'de> for Description {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        Description::new(&s).map_err(serde::de::Error::custom)
    }
}

impl serde::Serialize for Description {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.0.serialize(serializer)
    }
}

/// §1.1.2 ExtensionName: `[A-Z_0-9]{3,128}`
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ExtensionName(pub String);

static EXTENSION_NAME_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[A-Z_0-9]{3,128}$").unwrap());

impl ExtensionName {
    pub fn new(s: &str) -> Result<Self, ModelError> {
        if !EXTENSION_NAME_RE.is_match(s) {
            return Err(ModelError::DecodeValidation(format!(
                "Extension name '{s}' does not match pattern [A-Z_0-9]{{3,128}}"
            )));
        }
        Ok(Self(s.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> serde::Deserialize<'de> for ExtensionName {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        ExtensionName::new(&s).map_err(de::Error::custom)
    }
}

impl serde::Serialize for ExtensionName {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.0.serialize(serializer)
    }
}

#[cfg(test)]
mod tests {
    //! Tests ported from Python test/openjd/model/v2023_09/test_strings.py
    //!
    //! Gold standard: failure tests assert the full error message including path.

    use super::{Description, ExtensionName, Identifier};
    use crate::decode_job_template;

    fn yaml_val(s: &str) -> serde_yaml::Value {
        serde_yaml::from_str(s).unwrap()
    }

    fn decode_ok(s: &str) {
        let v = yaml_val(s);
        decode_job_template(v, None).unwrap_or_else(|_| panic!("Expected success for: {s}"));
    }

    /// For validation-pipeline errors (path + message format).
    fn check_err(s: &str, expected: &[&str]) {
        let v = yaml_val(s);
        let err = decode_job_template(v, None).expect_err(&format!("Expected error for: {s}"));
        let msg = err.to_string();
        for line in expected {
            assert!(
                msg.contains(line),
                "Missing in error output: {line:?}\nGot:\n{msg}"
            );
        }
    }

    /// For serde-level errors (rejected during deserialization).
    fn check_serde_err(s: &str, expected: &[&str]) {
        let v = yaml_val(s);
        let err = decode_job_template(v, None).expect_err(&format!("Expected error for: {s}"));
        let msg = err.to_string();
        for line in expected {
            assert!(
                msg.contains(line),
                "Missing in error output: {line:?}\nGot:\n{msg}"
            );
        }
    }

    // ══════════════════════════════════════════════════════════════
    // Identifier — unit-level tests via Identifier::new()
    // ══════════════════════════════════════════════════════════════

    #[test]
    fn identifier_valid_simple() {
        Identifier::new("Foo").unwrap();
        Identifier::new("_foo").unwrap();
        Identifier::new("foo_bar").unwrap();
        Identifier::new("A").unwrap();
        Identifier::new("a1").unwrap();
    }

    #[test]
    fn identifier_shortest_upper_a() {
        Identifier::new("A").unwrap();
    }

    #[test]
    fn identifier_longest_upper_a() {
        Identifier::new(&"A".repeat(64)).unwrap();
    }

    #[test]
    fn identifier_shortest_lower_a() {
        Identifier::new("a").unwrap();
    }

    #[test]
    fn identifier_longest_lower_a() {
        Identifier::new(&"a".repeat(64)).unwrap();
    }

    #[test]
    fn identifier_trailing_digits() {
        Identifier::new(&format!("A{}", "0".repeat(63))).unwrap();
        Identifier::new(&format!("A{}", "9".repeat(63))).unwrap();
    }

    #[test]
    fn identifier_all_underscores() {
        Identifier::new(&"_".repeat(64)).unwrap();
    }

    #[test]
    fn identifier_65_chars_succeeds_at_type_level() {
        // 65 chars is within the 512 max that Identifier::new() allows.
        // The tighter 64-char limit is enforced in the validation pass.
        Identifier::new(&"a".repeat(65)).unwrap();
    }

    #[test]
    fn identifier_512_chars_succeeds_at_type_level() {
        Identifier::new(&"a".repeat(512)).unwrap();
    }

    #[test]
    fn identifier_513_chars_fails() {
        let err = Identifier::new(&"a".repeat(513)).unwrap_err();
        assert!(
            err.to_string()
                .contains("Identifier length must be 1..=512, got 513"),
            "Got: {err}"
        );
    }

    #[test]
    fn identifier_empty() {
        let err = Identifier::new("").unwrap_err();
        assert!(
            err.to_string()
                .contains("Identifier length must be 1..=512, got 0"),
            "Got: {err}"
        );
    }

    #[test]
    fn identifier_starts_with_digit_0() {
        let err = Identifier::new("0").unwrap_err();
        assert!(
            err.to_string().contains("does not match pattern"),
            "Got: {err}"
        );
    }

    #[test]
    fn identifier_starts_with_digit_9() {
        let err = Identifier::new("9").unwrap_err();
        assert!(
            err.to_string().contains("does not match pattern"),
            "Got: {err}"
        );
    }

    #[test]
    fn identifier_leading_space() {
        let err = Identifier::new(" a").unwrap_err();
        assert!(
            err.to_string().contains("does not match pattern"),
            "Got: {err}"
        );
    }

    #[test]
    fn identifier_trailing_space() {
        let err = Identifier::new("a ").unwrap_err();
        assert!(
            err.to_string().contains("does not match pattern"),
            "Got: {err}"
        );
    }

    #[test]
    fn identifier_starts_with_bang() {
        let err = Identifier::new("!foo").unwrap_err();
        assert!(
            err.to_string().contains("does not match pattern"),
            "Got: {err}"
        );
    }

    #[test]
    fn identifier_only_alphanum_bang() {
        let err = Identifier::new("F!").unwrap_err();
        assert!(
            err.to_string().contains("does not match pattern"),
            "Got: {err}"
        );
    }

    #[test]
    fn identifier_with_hyphen() {
        let err = Identifier::new("foo-bar").unwrap_err();
        assert!(
            err.to_string().contains("does not match pattern"),
            "Got: {err}"
        );
    }

    #[test]
    fn identifier_with_dot() {
        let err = Identifier::new("foo.bar").unwrap_err();
        assert!(
            err.to_string().contains("does not match pattern"),
            "Got: {err}"
        );
    }

    // Test disallowed printable characters (from Python's parametrized list)
    #[test]
    fn identifier_disallowed_chars() {
        for ch in "!\"#$%&'()*+,-./:;<=>?@[\\]^`{|}~ \t\n\r".chars() {
            let s = format!("a{ch}");
            let err = Identifier::new(&s).unwrap_err();
            assert!(
                err.to_string().contains("does not match pattern"),
                "Expected pattern error for char {ch:?}, got: {err}"
            );
        }
    }

    // ══════════════════════════════════════════════════════════════
    // Description — unit-level tests via Description::new()
    // ══════════════════════════════════════════════════════════════

    #[test]
    fn description_min_length() {
        Description::new("A").unwrap();
    }

    #[test]
    fn description_max_length() {
        Description::new(&"A".repeat(2048)).unwrap();
    }

    #[test]
    fn description_empty_ok() {
        Description::new("").unwrap();
    }

    #[test]
    fn description_with_newlines_tabs() {
        Description::new("With\nnewlines\nand\ttabs").unwrap();
    }

    #[test]
    fn description_printable_ranges() {
        Description::new("\u{0020}").unwrap(); // start of first printable range
        Description::new("\u{007e}").unwrap(); // end of first printable range
        Description::new("\u{00a0}").unwrap(); // start of second printable range
    }

    #[test]
    fn description_too_long() {
        let err = Description::new(&"a".repeat(2049)).unwrap_err();
        assert!(
            err.to_string().contains("exceeds 2048 characters"),
            "Got: {err}"
        );
    }

    #[test]
    fn description_control_char_null() {
        let err = Description::new("\u{0000}").unwrap_err();
        assert!(err.to_string().contains("control characters"), "Got: {err}");
    }

    #[test]
    fn description_control_char_1f() {
        let err = Description::new("\u{001f}").unwrap_err();
        assert!(err.to_string().contains("control characters"), "Got: {err}");
    }

    #[test]
    fn description_control_char_del() {
        let err = Description::new("\u{007f}").unwrap_err();
        assert!(err.to_string().contains("control characters"), "Got: {err}");
    }

    #[test]
    fn description_control_char_9f() {
        let err = Description::new("\u{009f}").unwrap_err();
        assert!(err.to_string().contains("control characters"), "Got: {err}");
    }

    #[test]
    fn description_disallowed_after_newline() {
        let err = Description::new("a\n\u{0000}").unwrap_err();
        assert!(err.to_string().contains("control characters"), "Got: {err}");
    }

    // ══════════════════════════════════════════════════════════════
    // ExtensionName — unit-level tests via ExtensionName::new()
    // ══════════════════════════════════════════════════════════════

    #[test]
    fn extension_name_valid() {
        ExtensionName::new("EXPR").unwrap();
        ExtensionName::new("FEATURE_BUNDLE_1").unwrap();
        ExtensionName::new("ABC").unwrap();
        ExtensionName::new("123").unwrap();
        ExtensionName::new("A_B").unwrap();
    }

    #[test]
    fn extension_name_too_short() {
        let err = ExtensionName::new("AB").unwrap_err();
        assert!(
            err.to_string().contains("does not match pattern"),
            "Got: {err}"
        );
    }

    #[test]
    fn extension_name_single_char() {
        let err = ExtensionName::new("A").unwrap_err();
        assert!(
            err.to_string().contains("does not match pattern"),
            "Got: {err}"
        );
    }

    #[test]
    fn extension_name_empty() {
        let err = ExtensionName::new("").unwrap_err();
        assert!(
            err.to_string().contains("does not match pattern"),
            "Got: {err}"
        );
    }

    #[test]
    fn extension_name_lowercase() {
        let err = ExtensionName::new("expr").unwrap_err();
        assert!(
            err.to_string().contains("does not match pattern"),
            "Got: {err}"
        );
    }

    #[test]
    fn extension_name_mixed_case() {
        let err = ExtensionName::new("ExPr").unwrap_err();
        assert!(
            err.to_string().contains("does not match pattern"),
            "Got: {err}"
        );
    }

    #[test]
    fn extension_name_with_spaces() {
        let err = ExtensionName::new("FOO BAR").unwrap_err();
        assert!(
            err.to_string().contains("does not match pattern"),
            "Got: {err}"
        );
    }

    #[test]
    fn extension_name_with_hyphen() {
        let err = ExtensionName::new("FOO-BAR").unwrap_err();
        assert!(
            err.to_string().contains("does not match pattern"),
            "Got: {err}"
        );
    }

    // ══════════════════════════════════════════════════════════════
    // String constraints through job template — validation pipeline
    // ══════════════════════════════════════════════════════════════

    // --- Job name ---

    #[test]
    fn job_name_valid_shortest() {
        decode_ok(
            r#"{
            "specificationVersion": "jobtemplate-2023-09",
            "name": "A",
            "steps": [{"name": "S", "script": {"actions": {"onRun": {"command": "foo"}}}}]
        }"#,
        );
    }

    #[test]
    fn job_name_empty() {
        check_err(
            r#"{
            "specificationVersion": "jobtemplate-2023-09",
            "name": "",
            "steps": [{"name": "S", "script": {"actions": {"onRun": {"command": "foo"}}}}]
        }"#,
            &["name:\n\tmust not be empty."],
        );
    }

    #[test]
    fn job_name_too_long() {
        let name = "a".repeat(513);
        let tmpl = format!(
            r#"{{
            "specificationVersion": "jobtemplate-2023-09",
            "name": "{name}",
            "steps": [{{"name": "S", "script": {{"actions": {{"onRun": {{"command": "foo"}}}}}}}}]
        }}"#
        );
        check_err(&tmpl, &["name:\n\texceeds 128 characters."]);
    }

    #[test]
    fn job_name_with_control_char_null() {
        check_err(
            r#"{
            "specificationVersion": "jobtemplate-2023-09",
            "name": "a\u0000b",
            "steps": [{"name": "S", "script": {"actions": {"onRun": {"command": "foo"}}}}]
        }"#,
            &["name:\n\tcontains control characters."],
        );
    }

    #[test]
    fn job_name_with_control_char_1f() {
        check_err(
            r#"{
            "specificationVersion": "jobtemplate-2023-09",
            "name": "Job\u001fName",
            "steps": [{"name": "S", "script": {"actions": {"onRun": {"command": "foo"}}}}]
        }"#,
            &["name:\n\tcontains control characters."],
        );
    }

    #[test]
    fn job_name_with_control_char_del() {
        check_err(
            r#"{
            "specificationVersion": "jobtemplate-2023-09",
            "name": "a\u007fb",
            "steps": [{"name": "S", "script": {"actions": {"onRun": {"command": "foo"}}}}]
        }"#,
            &["name:\n\tcontains control characters."],
        );
    }

    #[test]
    fn job_name_with_control_char_9f() {
        check_err(
            r#"{
            "specificationVersion": "jobtemplate-2023-09",
            "name": "a\u009fb",
            "steps": [{"name": "S", "script": {"actions": {"onRun": {"command": "foo"}}}}]
        }"#,
            &["name:\n\tcontains control characters."],
        );
    }

    #[test]
    fn job_name_with_newline() {
        check_err(
            r#"{
            "specificationVersion": "jobtemplate-2023-09",
            "name": "a\nb",
            "steps": [{"name": "S", "script": {"actions": {"onRun": {"command": "foo"}}}}]
        }"#,
            &["name:\n\tcontains control characters."],
        );
    }

    // --- Step name ---

    #[test]
    fn step_name_valid_shortest() {
        decode_ok(
            r#"{
            "specificationVersion": "jobtemplate-2023-09",
            "name": "Job",
            "steps": [{"name": "A", "script": {"actions": {"onRun": {"command": "foo"}}}}]
        }"#,
        );
    }

    #[test]
    fn step_name_empty() {
        check_err(
            r#"{
            "specificationVersion": "jobtemplate-2023-09",
            "name": "Job",
            "steps": [{"name": "", "script": {"actions": {"onRun": {"command": "foo"}}}}]
        }"#,
            &["steps[0] -> name:\n\tmust not be empty."],
        );
    }

    #[test]
    fn step_name_too_long() {
        let name = "a".repeat(513);
        let tmpl = format!(
            r#"{{
            "specificationVersion": "jobtemplate-2023-09",
            "name": "Job",
            "steps": [{{"name": "{name}", "script": {{"actions": {{"onRun": {{"command": "foo"}}}}}}}}]
        }}"#
        );
        check_err(&tmpl, &["steps[0] -> name:\n\texceeds 64 characters."]);
    }

    #[test]
    fn step_name_with_control_char() {
        check_err(
            r#"{
            "specificationVersion": "jobtemplate-2023-09",
            "name": "Job",
            "steps": [{"name": "Step\u001fName", "script": {"actions": {"onRun": {"command": "foo"}}}}]
        }"#,
            &["steps[0] -> name:\n\tcontains control characters."],
        );
    }

    // --- Environment name ---

    #[test]
    fn env_name_with_control_char() {
        check_err(
            r#"{
            "specificationVersion": "jobtemplate-2023-09",
            "name": "Job",
            "steps": [{"name": "S", "script": {"actions": {"onRun": {"command": "foo"}}}}],
            "jobEnvironments": [{"name": "Env\u001fName", "script": {"actions": {"onEnter": {"command": "foo"}}}}]
        }"#,
            &["jobEnvironments[0] -> name:\n\tcontains control characters."],
        );
    }

    // --- Command ---

    #[test]
    fn command_empty() {
        check_err(
            r#"{
            "specificationVersion": "jobtemplate-2023-09",
            "name": "Job",
            "steps": [{"name": "S", "script": {"actions": {"onRun": {"command": ""}}}}]
        }"#,
            &["steps[0] -> script -> actions -> onRun -> command:\n\tmust not be empty."],
        );
    }

    // --- Description through template ---

    #[test]
    fn description_with_newlines_in_template_ok() {
        decode_ok(
            r#"{
            "specificationVersion": "jobtemplate-2023-09",
            "name": "Job",
            "description": "Line1\nLine2\nLine3",
            "steps": [{"name": "S", "script": {"actions": {"onRun": {"command": "foo"}}}}]
        }"#,
        );
    }

    #[test]
    fn description_too_long_in_template() {
        let desc = "a".repeat(2049);
        let tmpl = format!(
            r#"{{
            "specificationVersion": "jobtemplate-2023-09",
            "name": "Job",
            "description": "{desc}",
            "steps": [{{"name": "S", "script": {{"actions": {{"onRun": {{"command": "foo"}}}}}}}}]
        }}"#
        );
        check_serde_err(&tmpl, &["Description exceeds 2048 characters"]);
    }

    #[test]
    fn description_control_char_null_in_template() {
        check_serde_err(
            r#"{
            "specificationVersion": "jobtemplate-2023-09",
            "name": "Job",
            "description": "\u0000",
            "steps": [{"name": "S", "script": {"actions": {"onRun": {"command": "foo"}}}}]
        }"#,
            &["Description contains control characters"],
        );
    }

    #[test]
    fn description_control_char_1f_in_template() {
        check_serde_err(
            r#"{
            "specificationVersion": "jobtemplate-2023-09",
            "name": "Job",
            "description": "\u001f",
            "steps": [{"name": "S", "script": {"actions": {"onRun": {"command": "foo"}}}}]
        }"#,
            &["Description contains control characters"],
        );
    }

    #[test]
    fn description_control_char_del_in_template() {
        check_serde_err(
            r#"{
            "specificationVersion": "jobtemplate-2023-09",
            "name": "Job",
            "description": "\u007f",
            "steps": [{"name": "S", "script": {"actions": {"onRun": {"command": "foo"}}}}]
        }"#,
            &["Description contains control characters"],
        );
    }

    #[test]
    fn description_control_char_9f_in_template() {
        check_serde_err(
            r#"{
            "specificationVersion": "jobtemplate-2023-09",
            "name": "Job",
            "description": "\u009f",
            "steps": [{"name": "S", "script": {"actions": {"onRun": {"command": "foo"}}}}]
        }"#,
            &["Description contains control characters"],
        );
    }

    // --- Identifier through template (serde-level via task parameter name) ---

    #[test]
    fn identifier_empty_in_template() {
        check_serde_err(
            r#"{
            "specificationVersion": "jobtemplate-2023-09",
            "name": "Test",
            "steps": [{"name": "S", "parameterSpace": {"taskParameterDefinitions": [{"name": "", "type": "INT", "range": [1]}]}, "script": {"actions": {"onRun": {"command": "foo"}}}}]
        }"#,
            &["Identifier length must be 1..=512, got 0"],
        );
    }

    #[test]
    fn identifier_starts_with_digit_in_template() {
        check_serde_err(
            r#"{
            "specificationVersion": "jobtemplate-2023-09",
            "name": "Test",
            "steps": [{"name": "S", "parameterSpace": {"taskParameterDefinitions": [{"name": "1foo", "type": "INT", "range": [1]}]}, "script": {"actions": {"onRun": {"command": "foo"}}}}]
        }"#,
            &["does not match pattern"],
        );
    }

    #[test]
    fn identifier_with_hyphen_in_template() {
        check_serde_err(
            r#"{
            "specificationVersion": "jobtemplate-2023-09",
            "name": "Test",
            "steps": [{"name": "S", "parameterSpace": {"taskParameterDefinitions": [{"name": "foo-bar", "type": "INT", "range": [1]}]}, "script": {"actions": {"onRun": {"command": "foo"}}}}]
        }"#,
            &["does not match pattern"],
        );
    }

    #[test]
    fn identifier_with_bang_in_template() {
        check_serde_err(
            r#"{
            "specificationVersion": "jobtemplate-2023-09",
            "name": "Test",
            "steps": [{"name": "S", "parameterSpace": {"taskParameterDefinitions": [{"name": "F!", "type": "INT", "range": [1]}]}, "script": {"actions": {"onRun": {"command": "foo"}}}}]
        }"#,
            &["does not match pattern"],
        );
    }

    #[test]
    fn identifier_leading_space_in_template() {
        check_serde_err(
            r#"{
            "specificationVersion": "jobtemplate-2023-09",
            "name": "Test",
            "steps": [{"name": "S", "parameterSpace": {"taskParameterDefinitions": [{"name": " a", "type": "INT", "range": [1]}]}, "script": {"actions": {"onRun": {"command": "foo"}}}}]
        }"#,
            &["does not match pattern"],
        );
    }

    // Identifier exceeds 64 chars — validation pipeline error
    #[test]
    fn identifier_too_long_in_template() {
        let name = "a".repeat(65);
        let tmpl = format!(
            r#"{{
            "specificationVersion": "jobtemplate-2023-09",
            "name": "Test",
            "steps": [{{"name": "S", "parameterSpace": {{"taskParameterDefinitions": [{{"name": "{name}", "type": "INT", "range": [1]}}]}}, "script": {{"actions": {{"onRun": {{"command": "foo"}}}}}}}}]
        }}"#
        );
        check_err(&tmpl, &[
            "steps[0] -> parameterSpace -> taskParameterDefinitions[0]:\n\tname exceeds 64 characters.",
        ]);
    }

    // --- ExtensionName through template (serde-level) ---

    #[test]
    fn extension_name_lowercase_in_template() {
        check_serde_err(
            r#"{
            "specificationVersion": "jobtemplate-2023-09",
            "name": "Test",
            "steps": [{"name": "S", "script": {"actions": {"onRun": {"command": "foo"}}}}],
            "extensions": ["expr"]
        }"#,
            &["does not match pattern"],
        );
    }

    #[test]
    fn extension_name_too_short_in_template() {
        check_serde_err(
            r#"{
            "specificationVersion": "jobtemplate-2023-09",
            "name": "Test",
            "steps": [{"name": "S", "script": {"actions": {"onRun": {"command": "foo"}}}}],
            "extensions": ["AB"]
        }"#,
            &["does not match pattern"],
        );
    }

    #[test]
    fn extension_name_with_spaces_in_template() {
        check_serde_err(
            r#"{
            "specificationVersion": "jobtemplate-2023-09",
            "name": "Test",
            "steps": [{"name": "S", "script": {"actions": {"onRun": {"command": "foo"}}}}],
            "extensions": ["FOO BAR"]
        }"#,
            &["does not match pattern"],
        );
    }

    // --- Identifier through job parameter name (serde-level) ---

    #[test]
    fn param_name_empty_in_template() {
        check_serde_err(
            r#"{
            "specificationVersion": "jobtemplate-2023-09",
            "name": "Test",
            "parameterDefinitions": [{"name": "", "type": "INT"}],
            "steps": [{"name": "S", "script": {"actions": {"onRun": {"command": "foo"}}}}]
        }"#,
            &["Identifier length must be 1..=512, got 0"],
        );
    }

    #[test]
    fn param_name_starts_with_digit_in_template() {
        check_serde_err(
            r#"{
            "specificationVersion": "jobtemplate-2023-09",
            "name": "Test",
            "parameterDefinitions": [{"name": "0foo", "type": "INT"}],
            "steps": [{"name": "S", "script": {"actions": {"onRun": {"command": "foo"}}}}]
        }"#,
            &["does not match pattern"],
        );
    }

    #[test]
    fn description_max_length_multibyte_unicode() {
        // 2048 CJK characters (3 bytes each in UTF-8) must be accepted.
        // The limit is 2048 characters, not 2048 bytes.
        let desc: String = std::iter::repeat_n('一', 2048).collect();
        assert_eq!(desc.chars().count(), 2048);
        Description::new(&desc).unwrap();
    }

    #[test]
    fn description_too_long_multibyte_unicode() {
        let desc: String = std::iter::repeat_n('一', 2049).collect();
        let err = Description::new(&desc).unwrap_err();
        assert!(
            err.to_string().contains("exceeds 2048 characters"),
            "Got: {err}"
        );
    }
}
