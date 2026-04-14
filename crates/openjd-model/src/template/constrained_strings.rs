// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Constrained string types per spec §7.

use crate::error::OpenJdError;
use regex::Regex;
use serde::de::{self, Deserializer};
use std::sync::LazyLock;

/// §7.1 Identifier: `[A-Za-z_][A-Za-z0-9_]*`, length 1..=512 (64 base, 512 with FEATURE_BUNDLE_1)
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Identifier(pub String);

static IDENTIFIER_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[A-Za-z_][A-Za-z0-9_]*$").unwrap());

impl Identifier {
    pub fn new(s: &str) -> Result<Self, OpenJdError> {
        if s.is_empty() || s.len() > 512 {
            return Err(OpenJdError::DecodeValidation(format!(
                "Identifier length must be 1..=512, got {}",
                s.len()
            )));
        }
        if !IDENTIFIER_RE.is_match(s) {
            return Err(OpenJdError::DecodeValidation(format!(
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
    pub fn new(s: &str) -> Result<Self, OpenJdError> {
        if s.chars().count() > 2048 {
            return Err(OpenJdError::DecodeValidation(
                "Description exceeds 2048 characters".into(),
            ));
        }
        if s.chars()
            .any(|c| c.is_control() && c != '\n' && c != '\r' && c != '\t')
        {
            return Err(OpenJdError::DecodeValidation(
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
    pub fn new(s: &str) -> Result<Self, OpenJdError> {
        if !EXTENSION_NAME_RE.is_match(s) {
            return Err(OpenJdError::DecodeValidation(format!(
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
