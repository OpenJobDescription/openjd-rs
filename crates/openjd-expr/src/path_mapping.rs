// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Path formats, path mapping rules, and the [`PathMapper`] that applies them.
//!
//! A [`PathMapper`] holds a validated set of [`PathMappingRule`]s in tries
//! keyed by path component, one trie per source format. Mapping a path walks
//! its components down the tries once, so the cost depends on the length of
//! the path and not on the number of rules. The rule whose source path has
//! the most names wins; between rules naming the same location, the
//! one given last wins. See `specs/expr/path-mapping.md`.

use std::borrow::Cow;
use std::collections::HashMap;
use std::fmt;

use serde::{Deserialize, Serialize};

/// Path format (POSIX, Windows, or URI).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
#[allow(
    missing_docs,
    reason = "missing-docs burn-down: predates the lint; document these items, then delete this attribute"
)]
pub enum PathFormat {
    #[serde(alias = "posix", alias = "Posix")]
    Posix,
    #[serde(alias = "windows", alias = "Windows")]
    Windows,
    #[serde(alias = "URI", alias = "uri", alias = "Uri")]
    Uri,
}

impl PathFormat {
    #[allow(
        missing_docs,
        reason = "missing-docs burn-down: predates the lint; document these items, then delete this attribute"
    )]
    pub fn host() -> Self {
        if cfg!(windows) {
            PathFormat::Windows
        } else {
            PathFormat::Posix
        }
    }

    /// The position of this source format's trie in [`PathMapper`].
    fn trie_index(self) -> usize {
        match self {
            PathFormat::Posix => 0,
            PathFormat::Windows => 1,
            PathFormat::Uri => 2,
        }
    }
}

/// The source formats, in [`PathFormat::trie_index`] order.
const SOURCE_FORMATS: [PathFormat; 3] = [PathFormat::Posix, PathFormat::Windows, PathFormat::Uri];

/// A path mapping rule.
///
/// Serializes to/from the JSON format specified in the OpenJD spec:
/// <https://github.com/OpenJobDescription/openjd-specifications/wiki/How-Jobs-Are-Run#path-mapping>
///
/// Rules are applied through a [`PathMapper`], which rejects a rule whose
/// `source_path` is not absolute for its format (see [`validate`]).
///
/// [`validate`]: PathMappingRule::validate
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PathMappingRule {
    /// The format of `source_path`, which decides how paths are split into
    /// components and compared.
    pub source_path_format: PathFormat,
    /// The path prefix to map. It must be absolute for `source_path_format`:
    /// starting with `/` for `POSIX`; with a drive (`C:\`) or a UNC share
    /// (`\\server\share`) for `WINDOWS`; with `<scheme>://` for `URI`.
    pub source_path: String,
    /// The prefix that replaces `source_path`, used as written.
    pub destination_path: String,
}

impl PathMappingRule {
    /// Check that `source_path` is absolute for `source_path_format`.
    ///
    /// # Errors
    ///
    /// Returns [`InvalidPathMappingRule`], with `index` set to `None`, when
    /// `source_path` is not absolute: not starting with `/` for `POSIX`, with
    /// neither a drive (`C:\`) nor a UNC share (`\\server\share`) for
    /// `WINDOWS`, or without a `<scheme>://` prefix for `URI`.
    pub fn validate(&self) -> Result<(), InvalidPathMappingRule> {
        match self.split_source() {
            Some(_) => Ok(()),
            None => Err(self.invalid()),
        }
    }

    /// Split `source_path` for the trie, or `None` when it is not absolute
    /// for its format.
    fn split_source(&self) -> Option<SplitPath<'_>> {
        if self.source_path_format == PathFormat::Uri && !crate::uri_path::is_uri(&self.source_path)
        {
            return None;
        }
        split_path(&self.source_path, self.source_path_format)
    }

    /// The error for a `source_path` that is not absolute.
    fn invalid(&self) -> InvalidPathMappingRule {
        let expected = match self.source_path_format {
            PathFormat::Posix => "an absolute POSIX path, starting with '/'",
            PathFormat::Windows => {
                "an absolute WINDOWS path, starting with a drive such as 'C:\\' or a UNC share such as '\\\\server\\share'"
            }
            PathFormat::Uri => "a URI, starting with '<scheme>://'",
        };
        InvalidPathMappingRule {
            index: None,
            reason: format!("source_path '{}' must be {expected}", self.source_path),
        }
    }
}

/// A path mapping rule that [`PathMapper::new`] or
/// [`PathMappingRule::validate`] rejected.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct InvalidPathMappingRule {
    /// The rule's position in the list given to [`PathMapper::new`], or
    /// `None` when the error comes from [`PathMappingRule::validate`].
    pub index: Option<usize>,
    /// What is wrong with the rule.
    pub reason: String,
}

impl fmt::Display for InvalidPathMappingRule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.index {
            Some(i) => write!(f, "path mapping rule {i}: {}", self.reason),
            None => f.write_str(&self.reason),
        }
    }
}

impl std::error::Error for InvalidPathMappingRule {}

/// A path split for matching.
struct SplitPath<'a> {
    /// The trie key of the path's anchor: `/` for POSIX; the folded drive
    /// (`C:`) or UNC share (`\\SERVER\SHARE`) for Windows; the
    /// ASCII-lowercased `scheme://authority` for URI.
    anchor: String,
    /// How many names the anchor counts as when ranking rules: none for
    /// the POSIX root, one for a drive or a URI's `scheme://authority`, two
    /// for a UNC share (its server and share). Within one format this only
    /// shifts every rule by the same amount; across formats it makes
    /// `/srv/share` and `\\srv\share`, which name the same two names, tie.
    anchor_depth: usize,
    /// The components after the anchor, as written.
    parts: Vec<&'a str>,
}

/// Whether `c` separates Windows path components.
fn is_windows_sep(c: char) -> bool {
    c == '\\' || c == '/'
}

/// Fold a Windows path component the way Windows compares file names: each
/// UTF-16 code unit through the Windows uppercase table.
fn fold_windows(s: &str) -> String {
    s.chars().map(crate::windows_upcase::upcase).collect()
}

/// Filesystem components after the anchor. Empty components (from repeated
/// or trailing separators) and `.` are dropped, as `pathlib` does.
fn filesystem_parts(rest: &str, windows: bool) -> Vec<&str> {
    rest.split(|c| c == '/' || (windows && c == '\\'))
        .filter(|p| !p.is_empty() && *p != ".")
        .collect()
}

/// Split `path` for matching against rules of `format`, or `None` when the
/// path is not absolute for that format and so matches none of them.
fn split_path(path: &str, format: PathFormat) -> Option<SplitPath<'_>> {
    match format {
        PathFormat::Posix => {
            let rest = path.strip_prefix('/')?;
            Some(SplitPath {
                anchor: "/".to_string(),
                anchor_depth: 0,
                parts: filesystem_parts(rest, false),
            })
        }
        PathFormat::Windows => {
            let (anchor, anchor_depth, rest) = split_windows_anchor(path)?;
            Some(SplitPath {
                anchor,
                anchor_depth,
                parts: filesystem_parts(rest, true),
            })
        }
        PathFormat::Uri => {
            // Scheme and authority compare ASCII case-insensitively (RFC
            // 3986); the path compares exactly, segment by segment, with
            // empty segments kept.
            let authority_start = path.find("://")? + 3;
            let path_start = path[authority_start..]
                .find('/')
                .map_or(path.len(), |i| authority_start + i);
            let parts = match path[path_start..].strip_prefix('/') {
                Some(segments) => segments.split('/').collect(),
                None => Vec::new(),
            };
            Some(SplitPath {
                anchor: path[..path_start].to_ascii_lowercase(),
                anchor_depth: 1,
                parts,
            })
        }
    }
}

/// Split an absolute Windows path into its anchor's trie key, the anchor's
/// depth (see [`SplitPath::anchor_depth`]) and the text after the anchor.
/// The anchor is a drive followed by a separator (`C:\`) or a UNC share
/// (`\\server\share`), with `\` and `/` interchangeable. Drive-relative
/// (`C:dir`) and rooted-without-drive (`\dir`) paths are not absolute and
/// give `None`.
fn split_windows_anchor(path: &str) -> Option<(String, usize, &str)> {
    let bytes = path.as_bytes();
    if bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && is_windows_sep(char::from(bytes[2]))
    {
        let drive = char::from(bytes[0]).to_ascii_uppercase();
        return Some((format!("{drive}:"), 1, &path[3..]));
    }
    let after = path
        .strip_prefix(is_windows_sep)?
        .strip_prefix(is_windows_sep)?;
    let server_end = after.find(is_windows_sep)?;
    let server = &after[..server_end];
    let after_server = &after[server_end + 1..];
    let share_end = after_server
        .find(is_windows_sep)
        .unwrap_or(after_server.len());
    let share = &after_server[..share_end];
    if server.is_empty() || share.is_empty() {
        return None;
    }
    let anchor = format!("\\\\{}\\{}", fold_windows(server), fold_windows(share));
    Some((anchor, 2, &after_server[share_end..]))
}

/// The trie key of a component of a path in `format`.
fn part_key(part: &str, format: PathFormat) -> Cow<'_, str> {
    match format {
        PathFormat::Windows => Cow::Owned(fold_windows(part)),
        PathFormat::Posix | PathFormat::Uri => Cow::Borrowed(part),
    }
}

/// A trie node: the rule whose source path ends here, if any, and the
/// nodes for the next component.
#[derive(Debug, Clone, Default)]
struct TrieNode {
    rule: Option<usize>,
    children: HashMap<String, TrieNode>,
}

/// A rule matched against a path.
struct Match<'a> {
    /// Index of the rule in [`PathMapper::rules`].
    rule: usize,
    /// Number of names in the rule's source path, counting the anchor as
    /// [`SplitPath::anchor_depth`] names.
    depth: usize,
    /// The source format of the trie that matched.
    format: PathFormat,
    /// The path's components after the matched source path.
    remaining: Vec<&'a str>,
}

/// Applies a set of [`PathMappingRule`]s to paths.
///
/// [`new`](PathMapper::new) validates the rules and builds one trie per
/// source format, keyed by path component. Mapping a path walks its
/// components down each trie once, so the cost is linear in the length of
/// the path, whatever the number of rules.
///
/// How each source format splits and compares paths:
/// - `POSIX`: components split on `/` and compare exactly.
/// - `WINDOWS`: components split on `\` or `/` and compare the way Windows
///   compares file names, mapping each UTF-16 code unit through the Windows
///   uppercase table. A drive (`C:`) or UNC share (`\\server\share`) is one
///   component.
/// - `URI`: `scheme://authority` is one component and compares ASCII
///   case-insensitively; path segments split on `/` and compare exactly.
///
/// For POSIX and WINDOWS, empty components and `.` are ignored, as `pathlib`
/// does, and a path that is not absolute matches no rule.
///
/// When several rules match, the one whose source path has the most names
/// wins: a POSIX root counts as none, a drive or a URI's
/// `scheme://authority` as one, and a UNC share as two (its server and
/// share), so `/srv/share` and `\\srv\share` have two each. Rules with the
/// same components name the same location, and the one given last wins.
#[derive(Debug, Clone, Default)]
pub struct PathMapper {
    rules: Vec<PathMappingRule>,
    precedence: Vec<usize>,
    tries: [TrieNode; 3],
}

impl PathMapper {
    /// Validate `rules` and build the mapper. The rules keep the order
    /// given, which decides between rules for the same location.
    ///
    /// # Errors
    ///
    /// Returns [`InvalidPathMappingRule`], with `index` set to the rule's
    /// position, for the first rule whose `source_path` is not absolute for
    /// its format (see [`PathMappingRule::validate`]).
    pub fn new(rules: Vec<PathMappingRule>) -> Result<Self, InvalidPathMappingRule> {
        let mut tries: [TrieNode; 3] = Default::default();
        let mut depths = Vec::with_capacity(rules.len());
        for (i, rule) in rules.iter().enumerate() {
            let format = rule.source_path_format;
            let split = rule.split_source().ok_or_else(|| InvalidPathMappingRule {
                index: Some(i),
                ..rule.invalid()
            })?;
            depths.push(split.anchor_depth + split.parts.len());
            let mut node = tries[format.trie_index()]
                .children
                .entry(split.anchor)
                .or_default();
            for part in split.parts {
                node = node
                    .children
                    .entry(part_key(part, format).into_owned())
                    .or_default();
            }
            // A later rule for the same location replaces an earlier one.
            node.rule = Some(i);
        }
        let mut precedence: Vec<usize> = (0..rules.len()).collect();
        precedence.sort_by(|&a, &b| depths[b].cmp(&depths[a]).then(b.cmp(&a)));
        Ok(PathMapper {
            rules,
            precedence,
            tries,
        })
    }

    /// The rules, in the order given to [`new`](PathMapper::new).
    pub fn rules(&self) -> &[PathMappingRule] {
        &self.rules
    }

    /// The rules in precedence order: most source path names first (see
    /// [`PathMapper`]), and among rules with the same number, the one given
    /// last first.
    /// Applying the first rule in this order that matches a path gives the
    /// same result as [`apply`](PathMapper::apply).
    pub fn rules_by_precedence(&self) -> impl Iterator<Item = &PathMappingRule> + '_ {
        self.precedence.iter().map(|&i| &self.rules[i])
    }

    /// Whether the mapper has no rules.
    pub fn is_empty(&self) -> bool {
        self.rules.is_empty()
    }

    /// Map `path`, joining the components after the matched source path to
    /// the destination with this host's separator. Returns `None` when no
    /// rule matches.
    pub fn apply(&self, path: &str) -> Option<String> {
        self.apply_with_format(path, PathFormat::host())
    }

    /// Map `path`, joining the components after the matched source path to
    /// the destination with the separator of `output_format`: `\` for
    /// `Windows`, `/` otherwise. A trailing separator on `path` is kept.
    /// Returns `None` when no rule matches.
    pub fn apply_with_format(&self, path: &str, output_format: PathFormat) -> Option<String> {
        let sep = match output_format {
            PathFormat::Windows => '\\',
            PathFormat::Posix | PathFormat::Uri => '/',
        };
        let found = self.find(path)?;
        let mut result = self.rules[found.rule].destination_path.clone();
        for part in found.remaining {
            result.push(sep);
            result.push_str(part);
        }
        let trailing = match found.format {
            PathFormat::Windows => path.ends_with(is_windows_sep),
            PathFormat::Posix | PathFormat::Uri => path.ends_with('/'),
        };
        if trailing && !result.ends_with(sep) {
            result.push(sep);
        }
        Some(result)
    }

    /// [`apply`](PathMapper::apply), returning `path` unchanged when no
    /// rule matches.
    #[must_use]
    pub fn map(&self, path: &str) -> String {
        self.apply(path).unwrap_or_else(|| path.to_string())
    }

    /// [`apply_with_format`](PathMapper::apply_with_format), returning
    /// `path` unchanged when no rule matches.
    #[must_use]
    pub fn map_with_format(&self, path: &str, output_format: PathFormat) -> String {
        self.apply_with_format(path, output_format)
            .unwrap_or_else(|| path.to_string())
    }

    /// The winning rule for `path`, if any rule matches.
    fn find<'a>(&self, path: &'a str) -> Option<Match<'a>> {
        let mut best: Option<Match<'a>> = None;
        for format in SOURCE_FORMATS {
            let trie = &self.tries[format.trie_index()];
            if trie.children.is_empty() {
                continue;
            }
            let Some(split) = split_path(path, format) else {
                continue;
            };
            let Some(mut node) = trie.children.get(&split.anchor) else {
                continue;
            };
            let mut found = node.rule.map(|rule| (rule, 0));
            for (i, part) in split.parts.iter().enumerate() {
                match node.children.get(part_key(part, format).as_ref()) {
                    Some(next) => node = next,
                    None => break,
                }
                if let Some(rule) = node.rule {
                    found = Some((rule, i + 1));
                }
            }
            let Some((rule, consumed)) = found else {
                continue;
            };
            let depth = split.anchor_depth + consumed;
            // A path can be absolute in more than one format (`//srv/share`
            // is a POSIX path and a Windows UNC share). The deeper match
            // wins, and between equally deep ones, the rule given last.
            if best
                .as_ref()
                .is_none_or(|b| (depth, rule) > (b.depth, b.rule))
            {
                best = Some(Match {
                    rule,
                    depth,
                    format,
                    remaining: split.parts[consumed..].to_vec(),
                });
            }
        }
        best
    }
}

/// Check if a string is a URI (has a `scheme://` prefix).
///
/// Delegates to [`crate::uri_path::is_uri`]: the scheme must be at least two
/// characters, so `C://data/x` is a Windows drive path, not a URI.
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
        assert_eq!(roundtrip.path_mapping_rules, file.path_mapping_rules);
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
