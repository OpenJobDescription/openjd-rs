// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Path format and mapping rules.
//!
//! Mirrors Python `openjd.expr._path_mapping`.

use serde::{Deserialize, Serialize};

/// Path format (POSIX, Windows, or URI).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum PathFormat {
    #[serde(alias = "posix", alias = "Posix")]
    Posix,
    #[serde(alias = "windows", alias = "Windows")]
    Windows,
    #[serde(alias = "URI", alias = "uri", alias = "Uri")]
    Uri,
}

impl PathFormat {
    pub fn host() -> Self {
        if cfg!(windows) {
            PathFormat::Windows
        } else {
            PathFormat::Posix
        }
    }
}

/// A path mapping rule.
///
/// Serializes to/from the JSON format specified in the OpenJD spec:
/// <https://github.com/OpenJobDescription/openjd-specifications/wiki/How-Jobs-Are-Run#path-mapping>
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PathMappingRule {
    pub source_path_format: PathFormat,
    pub source_path: String,
    pub destination_path: String,
}

impl PathMappingRule {
    /// Apply this rule using host-native output separators.
    /// Equivalent to Python's behavior (uses `os.name` to pick separator).
    pub fn apply(&self, path: &str) -> Option<String> {
        self.apply_with_format(path, PathFormat::host())
    }

    /// Apply this rule, using `output_format` to determine the output separator.
    /// - `Posix` / `Uri` → `/`
    /// - `Windows` → `\`
    pub fn apply_with_format(&self, path: &str, output_format: PathFormat) -> Option<String> {
        let sep = match output_format {
            PathFormat::Windows => '\\',
            _ => '/',
        };
        match self.source_path_format {
            PathFormat::Uri => self.apply_uri(path, sep),
            PathFormat::Posix => self.apply_filesystem(path, false, sep),
            PathFormat::Windows => self.apply_filesystem(path, true, sep),
        }
    }

    /// Find the byte offset where the path component begins in a URI (after `scheme://authority`).
    /// Returns `None` if there is no `://` separator.
    fn uri_path_start(uri: &str) -> Option<usize> {
        let authority_start = uri.find("://")? + 3;
        Some(
            uri[authority_start..]
                .find('/')
                .map_or(uri.len(), |i| authority_start + i),
        )
    }

    fn apply_uri(&self, path: &str, sep: char) -> Option<String> {
        // Per RFC 3986: scheme and authority are case-insensitive, path is case-sensitive.
        let src_path_start = Self::uri_path_start(&self.source_path).unwrap_or(0);
        let inp_path_start = Self::uri_path_start(path).unwrap_or(0);

        // Scheme+authority must match case-insensitively
        if !path[..inp_path_start].eq_ignore_ascii_case(&self.source_path[..src_path_start]) {
            return None;
        }
        // Path portion must match case-sensitively
        let src_path = &self.source_path[src_path_start..];
        let inp_path = &path[inp_path_start..];
        if !inp_path.starts_with(src_path) {
            return None;
        }
        let remainder = &inp_path[src_path.len()..];
        if !remainder.is_empty() && !remainder.starts_with('/') {
            return None;
        }
        let child_parts: Vec<&str> = if remainder.is_empty() {
            Vec::new()
        } else {
            remainder[1..].split('/').collect()
        };
        let mut result = self.destination_path.clone();
        for part in &child_parts {
            result.push(sep);
            result.push_str(part);
        }
        if path.ends_with('/') && !result.ends_with(sep) {
            result.push(sep);
        }
        Some(result)
    }

    fn apply_filesystem(&self, path: &str, case_insensitive: bool, sep: char) -> Option<String> {
        let source_parts = split_path_parts(&self.source_path);
        let path_parts = split_path_parts(path);

        if path_parts.len() < source_parts.len() {
            return None;
        }

        for (sp, pp) in source_parts.iter().zip(path_parts.iter()) {
            let matches = if case_insensitive {
                sp.eq_ignore_ascii_case(pp)
            } else {
                sp == pp
            };
            if !matches {
                return None;
            }
        }

        let remaining = &path_parts[source_parts.len()..];
        let mut result = self.destination_path.clone();
        for part in remaining {
            result.push(sep);
            result.push_str(part);
        }

        if has_trailing_slash(path, case_insensitive) && !result.ends_with(sep) {
            result.push(sep);
        }

        Some(result)
    }
}

/// Split a path into parts, handling both `/` and `\` separators.
/// Preserves drive letters as the first part (e.g., "C:" from "C:\foo").
fn split_path_parts(path: &str) -> Vec<&str> {
    path.split(&['/', '\\'][..])
        .filter(|s| !s.is_empty())
        .collect()
}

/// Check if a path has a trailing slash.
/// For Windows paths, both `\` and `/` count.
fn has_trailing_slash(path: &str, windows: bool) -> bool {
    if windows {
        path.ends_with('\\') || path.ends_with('/')
    } else {
        path.ends_with('/')
    }
}

/// Apply a list of path mapping rules to a path. First match wins.
///
/// Rules must be pre-sorted by decreasing `source_path` length (longest match first).
/// The session layer handles this sorting; see `Session::new()`.
#[must_use]
pub fn apply_rules(rules: &[PathMappingRule], path: &str) -> String {
    apply_rules_with_format(rules, path, PathFormat::host())
}

/// Apply rules with an explicit output format.
///
/// Rules must be pre-sorted by decreasing `source_path` length (longest match first).
#[must_use]
pub fn apply_rules_with_format(
    rules: &[PathMappingRule],
    path: &str,
    output_format: PathFormat,
) -> String {
    for rule in rules {
        if let Some(mapped) = rule.apply_with_format(path, output_format) {
            return mapped;
        }
    }
    path.to_string()
}

/// Check if a string is a URI (has a scheme:// prefix).
pub fn is_uri(path: &str) -> bool {
    crate::uri_path::is_uri(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Spec JSON from https://github.com/OpenJobDescription/openjd-specifications/wiki/How-Jobs-Are-Run#path-mapping
    const SPEC_JSON: &str = r#"{
        "version": "pathmapping-1.0",
        "path_mapping_rules": [
            {
                "source_path_format": "POSIX",
                "source_path": "/home/user",
                "destination_path": "/mnt/shared/user"
            },
            {
                "source_path_format": "WINDOWS",
                "source_path": "C:\\Users\\user",
                "destination_path": "/mnt/shared/user"
            }
        ]
    }"#;

    #[derive(Serialize, Deserialize)]
    struct PathMappingFile {
        version: String,
        path_mapping_rules: Vec<PathMappingRule>,
    }

    #[test]
    fn deserialize_spec_json() {
        let file: PathMappingFile = serde_json::from_str(SPEC_JSON).unwrap();
        assert_eq!(file.version, "pathmapping-1.0");
        assert_eq!(file.path_mapping_rules.len(), 2);

        assert_eq!(
            file.path_mapping_rules[0].source_path_format,
            PathFormat::Posix
        );
        assert_eq!(file.path_mapping_rules[0].source_path, "/home/user");
        assert_eq!(
            file.path_mapping_rules[0].destination_path,
            "/mnt/shared/user"
        );

        assert_eq!(
            file.path_mapping_rules[1].source_path_format,
            PathFormat::Windows
        );
        assert_eq!(file.path_mapping_rules[1].source_path, "C:\\Users\\user");
        assert_eq!(
            file.path_mapping_rules[1].destination_path,
            "/mnt/shared/user"
        );
    }

    #[test]
    fn serialize_roundtrip() {
        let file: PathMappingFile = serde_json::from_str(SPEC_JSON).unwrap();
        let json = serde_json::to_string(&file).unwrap();
        let roundtrip: PathMappingFile = serde_json::from_str(&json).unwrap();

        assert_eq!(
            roundtrip.path_mapping_rules.len(),
            file.path_mapping_rules.len()
        );
        for (a, b) in file
            .path_mapping_rules
            .iter()
            .zip(roundtrip.path_mapping_rules.iter())
        {
            assert_eq!(a.source_path_format, b.source_path_format);
            assert_eq!(a.source_path, b.source_path);
            assert_eq!(a.destination_path, b.destination_path);
        }
    }

    #[test]
    fn serialize_posix_format() {
        let rule = PathMappingRule {
            source_path_format: PathFormat::Posix,
            source_path: "/src".into(),
            destination_path: "/dst".into(),
        };
        let json = serde_json::to_value(&rule).unwrap();
        assert_eq!(json["source_path_format"], "POSIX");
    }

    #[test]
    fn serialize_windows_format() {
        let rule = PathMappingRule {
            source_path_format: PathFormat::Windows,
            source_path: "C:\\src".into(),
            destination_path: "/dst".into(),
        };
        let json = serde_json::to_value(&rule).unwrap();
        assert_eq!(json["source_path_format"], "WINDOWS");
    }

    #[test]
    fn serialize_uri_format() {
        let rule = PathMappingRule {
            source_path_format: PathFormat::Uri,
            source_path: "s3://bucket/assets".into(),
            destination_path: "/mnt/assets".into(),
        };
        let json = serde_json::to_value(&rule).unwrap();
        assert_eq!(json["source_path_format"], "URI");
    }

    #[test]
    fn deserialize_uri_format() {
        let json =
            r#"{"source_path_format":"URI","source_path":"s3://bucket","destination_path":"/mnt"}"#;
        let rule: PathMappingRule = serde_json::from_str(json).unwrap();
        assert_eq!(rule.source_path_format, PathFormat::Uri);
    }

    #[test]
    fn serialize_field_names_match_spec() {
        let rule = PathMappingRule {
            source_path_format: PathFormat::Posix,
            source_path: "/a".into(),
            destination_path: "/b".into(),
        };
        let json = serde_json::to_value(&rule).unwrap();
        let obj = json.as_object().unwrap();
        assert!(obj.contains_key("source_path_format"));
        assert!(obj.contains_key("source_path"));
        assert!(obj.contains_key("destination_path"));
        assert_eq!(obj.len(), 3, "no extra fields");
    }
}
