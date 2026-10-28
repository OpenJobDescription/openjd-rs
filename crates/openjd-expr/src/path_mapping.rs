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

    /// Which format wins when rules of two formats match a path with the
    /// same number of names (higher wins). Only POSIX and WINDOWS rules can
    /// match the same path (`//srv/share/x`), and that spelling is the UNC
    /// form, so WINDOWS wins. A fixed order keeps the result independent of
    /// the order the rules are given in.
    fn tie_priority(self) -> u8 {
        match self {
            PathFormat::Windows => 2,
            PathFormat::Posix => 1,
            PathFormat::Uri => 0,
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

/// The longest path, in bytes, that path mapping handles: a rule's
/// `source_path` or `destination_path` longer than this is rejected, and a
/// longer path matches no rule. 32 KiB covers a Windows extended-length path
/// (up to 32,767 UTF-16 code units) of ASCII characters and is eight times
/// Linux's `PATH_MAX`, while keeping a rule's trie, about 240 bytes per
/// component, small.
pub const MAX_PATH_LEN: usize = 32 * 1024;

/// The default limit, in bytes, on the estimated size of a [`PathMapper`]'s
/// trie (30 MiB). See [`PathMapper::with_max_trie_bytes`].
pub const DEFAULT_MAX_TRIE_BYTES: usize = 30 * 1024 * 1024;

/// Estimated bytes for one trie node besides its key: the node itself and
/// its entry in the parent's map. Measured at about 230 bytes per
/// one-character component.
const TRIE_NODE_BYTES: usize = 224;

/// The source formats, in [`PathFormat::trie_index`] order.
const SOURCE_FORMATS: [PathFormat; 3] = [PathFormat::Posix, PathFormat::Windows, PathFormat::Uri];

/// A path mapping rule.
///
/// Serializes to/from the JSON format specified in the OpenJD spec:
/// <https://github.com/OpenJobDescription/openjd-specifications/wiki/How-Jobs-Are-Run#path-mapping>
///
/// Rules are applied through a [`PathMapper`], which rejects a rule whose
/// `source_path` is not absolute for its format, or whose `source_path` or
/// `destination_path` is longer than [`MAX_PATH_LEN`] bytes (see
/// [`validate`]).
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
    /// Check that `source_path` is absolute for `source_path_format`, and
    /// that neither path is longer than [`MAX_PATH_LEN`] bytes.
    ///
    /// # Errors
    ///
    /// Returns [`InvalidPathMappingRule`], with `index` set to `None`, when
    /// `source_path` or `destination_path` is longer than [`MAX_PATH_LEN`]
    /// bytes, or `source_path` is not absolute: not starting with `/` for
    /// `POSIX`, with neither a drive (`C:\`) nor a UNC share
    /// (`\\server\share`) for `WINDOWS`, or without a `<scheme>://` prefix for
    /// `URI`.
    pub fn validate(&self) -> Result<(), InvalidPathMappingRule> {
        self.checked_split().map(|_| ())
    }

    /// [`validate`](Self::validate), returning the split `source_path`.
    fn checked_split(&self) -> Result<SplitPath<'_>, InvalidPathMappingRule> {
        for (field, value) in [
            ("source_path", &self.source_path),
            ("destination_path", &self.destination_path),
        ] {
            if value.len() > MAX_PATH_LEN {
                return Err(InvalidPathMappingRule {
                    index: None,
                    reason: format!(
                        "{field} is {} bytes long, more than the limit of {MAX_PATH_LEN} bytes",
                        value.len()
                    ),
                });
            }
        }
        self.split_source().ok_or_else(|| self.invalid())
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
            PathFormat::Uri => {
                "a URI, starting with '<scheme>://' where the scheme has at least two characters"
            }
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
    /// The text after the anchor; [`SplitPath::parts`] splits it.
    rest: &'a str,
    /// The format the path was split for.
    format: PathFormat,
}

impl<'a> SplitPath<'a> {
    /// The components after the anchor, as written, produced one at a time
    /// so a long path is never collected.
    fn parts(&self) -> impl Iterator<Item = &'a str> + use<'a> {
        components(self.rest, self.format)
    }
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

/// Append `part`, folded the way Windows compares file names, to `buf`.
fn fold_windows_into(part: &str, buf: &mut String) {
    buf.extend(part.chars().map(crate::windows_upcase::upcase));
}

/// The components of the text after an anchor in `format`.
///
/// POSIX and WINDOWS drop empty components (from repeated or trailing
/// separators) and `.`, as `pathlib` does. URI keeps every segment, empty
/// ones included, and the text after the authority must start with `/` to
/// have any.
fn components(rest: &str, format: PathFormat) -> impl Iterator<Item = &str> {
    let windows = format == PathFormat::Windows;
    let uri = format == PathFormat::Uri;
    let body = if uri {
        rest.strip_prefix('/')
    } else {
        Some(rest)
    };
    body.into_iter()
        .flat_map(move |b| b.split(move |c| c == '/' || (windows && c == '\\')))
        .filter(move |p| uri || (!p.is_empty() && *p != "."))
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
                rest,
                format,
            })
        }
        PathFormat::Windows => {
            let (anchor, anchor_depth, rest) = split_windows_anchor(path)?;
            Some(SplitPath {
                anchor,
                anchor_depth,
                rest,
                format,
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
            Some(SplitPath {
                anchor: path[..path_start].to_ascii_lowercase(),
                anchor_depth: 1,
                rest: &path[path_start..],
                format,
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
/// indexes in [`PathMapper::nodes`] of the nodes for the next component.
///
/// Nodes live in one flat `Vec` and refer to each other by index, so
/// building, dropping, cloning and formatting a trie never recurse. A rule
/// with tens of thousands of components would otherwise overflow the stack
/// when the mapper is dropped.
#[derive(Debug, Clone, Default)]
struct TrieNode {
    rule: Option<usize>,
    children: HashMap<String, usize>,
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
    /// The text after the path's anchor, for that format.
    rest: &'a str,
    /// How many of the path's components the source path covers.
    consumed: usize,
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
/// Between a POSIX and a WINDOWS rule with the same number of names (both
/// can match a path such as `//srv/share/x`), the WINDOWS rule wins.
#[derive(Debug, Clone)]
pub struct PathMapper {
    rules: Vec<PathMappingRule>,
    precedence: Vec<usize>,
    /// The estimated size of the trie, in bytes.
    trie_bytes: usize,
    /// The trie nodes. The first three are the roots of the POSIX, WINDOWS
    /// and URI tries, in [`PathFormat::trie_index`] order.
    nodes: Vec<TrieNode>,
}

impl Default for PathMapper {
    fn default() -> Self {
        PathMapper {
            rules: Vec::new(),
            precedence: Vec::new(),
            trie_bytes: 0,
            nodes: vec![TrieNode::default(); SOURCE_FORMATS.len()],
        }
    }
}

impl PathMapper {
    /// The child of `node` for `key`, added if it does not exist yet. Adds
    /// the estimated size of a new node to `bytes`.
    fn child_or_insert(
        nodes: &mut Vec<TrieNode>,
        node: usize,
        key: String,
        bytes: &mut usize,
    ) -> usize {
        if let Some(&child) = nodes[node].children.get(&key) {
            return child;
        }
        *bytes = bytes.saturating_add(TRIE_NODE_BYTES + key.len());
        let child = nodes.len();
        nodes.push(TrieNode::default());
        nodes[node].children.insert(key, child);
        child
    }

    /// Validate `rules` and build the mapper, with the trie limited to
    /// [`DEFAULT_MAX_TRIE_BYTES`]. The rules keep the order given, which
    /// decides between rules for the same location.
    ///
    /// # Errors
    ///
    /// Returns [`InvalidPathMappingRule`], with `index` set to the rule's
    /// position, for the first invalid rule: a `source_path` that is not
    /// absolute for its format, or a `source_path` or `destination_path`
    /// longer than [`MAX_PATH_LEN`] bytes (see [`PathMappingRule::validate`]);
    /// or for the rule that makes the trie larger than the limit (see
    /// [`with_max_trie_bytes`](PathMapper::with_max_trie_bytes)).
    pub fn new(rules: Vec<PathMappingRule>) -> Result<Self, InvalidPathMappingRule> {
        Self::with_max_trie_bytes(rules, DEFAULT_MAX_TRIE_BYTES)
    }

    /// [`new`](PathMapper::new), with the trie limited to `max_trie_bytes`.
    ///
    /// Any number of rules is allowed; the limit is on the estimated size
    /// of the trie ([`trie_bytes`](PathMapper::trie_bytes)), which grows
    /// only for components a rule does not share with earlier rules. Rules
    /// under a common root share that root's nodes.
    ///
    /// # Errors
    ///
    /// As [`new`](PathMapper::new), and for the first rule whose new
    /// components make the trie larger than `max_trie_bytes`.
    pub fn with_max_trie_bytes(
        rules: Vec<PathMappingRule>,
        max_trie_bytes: usize,
    ) -> Result<Self, InvalidPathMappingRule> {
        let mut nodes = vec![TrieNode::default(); SOURCE_FORMATS.len()];
        let mut trie_bytes = 0usize;
        let mut depths = Vec::with_capacity(rules.len());
        let mut replaced = vec![false; rules.len()];
        for (i, rule) in rules.iter().enumerate() {
            let format = rule.source_path_format;
            let split = rule.checked_split().map_err(|e| InvalidPathMappingRule {
                index: Some(i),
                ..e
            })?;
            let bytes = &mut trie_bytes;
            let mut node =
                Self::child_or_insert(&mut nodes, format.trie_index(), split.anchor.clone(), bytes);
            let mut depth = split.anchor_depth;
            for part in split.parts() {
                let key = part_key(part, format).into_owned();
                node = Self::child_or_insert(&mut nodes, node, key, bytes);
                depth += 1;
            }
            if trie_bytes > max_trie_bytes {
                return Err(InvalidPathMappingRule {
                    index: Some(i),
                    reason: format!(
                        "the rules' trie would need about {trie_bytes} bytes, more than the \
                         limit of {max_trie_bytes} bytes"
                    ),
                });
            }
            depths.push(depth);
            // A later rule for the same location replaces an earlier one.
            if let Some(earlier) = nodes[node].rule.replace(i) {
                replaced[earlier] = true;
            }
        }
        let mut precedence: Vec<usize> = (0..rules.len()).filter(|&i| !replaced[i]).collect();
        let key = |i: usize| (depths[i], rules[i].source_path_format.tie_priority(), i);
        precedence.sort_by_key(|&i| std::cmp::Reverse(key(i)));
        Ok(PathMapper {
            rules,
            precedence,
            trie_bytes,
            nodes,
        })
    }

    /// The estimated size of the trie in bytes: for each node, its key's
    /// length plus a fixed estimate for the node and its map entry. This is
    /// what [`with_max_trie_bytes`](PathMapper::with_max_trie_bytes)
    /// limits.
    pub fn trie_bytes(&self) -> usize {
        self.trie_bytes
    }

    /// The rules, in the order given to [`new`](PathMapper::new).
    pub fn rules(&self) -> &[PathMappingRule] {
        &self.rules
    }

    /// The rules that can apply, in precedence order: most source path
    /// names first (see [`PathMapper`]), then WINDOWS before POSIX before
    /// URI, then the one given last first. A rule replaced by a later rule
    /// for the same location is left out, so each location appears once.
    /// The first rule in this order that matches a path is the one
    /// [`apply`](PathMapper::apply) uses, and a mapper built from these
    /// rules, in any order, maps every path the same way.
    pub fn rules_by_precedence(&self) -> impl Iterator<Item = &PathMappingRule> + '_ {
        self.precedence.iter().map(|&i| &self.rules[i])
    }

    /// Whether the mapper has no rules.
    pub fn is_empty(&self) -> bool {
        self.rules.is_empty()
    }

    /// Map `path`, joining the components after the matched source path to
    /// the destination with this host's separator. Returns `None` when no
    /// rule matches, which includes a path longer than [`MAX_PATH_LEN`]
    /// bytes.
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
        for part in components(found.rest, found.format).skip(found.consumed) {
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

    /// The winning rule for `path`, if any rule matches. A path longer
    /// than [`MAX_PATH_LEN`] bytes matches none.
    fn find<'a>(&self, path: &'a str) -> Option<Match<'a>> {
        if path.len() > MAX_PATH_LEN {
            return None;
        }
        let mut best: Option<Match<'a>> = None;
        // Reused for each folded WINDOWS component, so the walk allocates
        // only for the longest component.
        let mut key = String::new();
        for format in SOURCE_FORMATS {
            let root = &self.nodes[format.trie_index()];
            if root.children.is_empty() {
                continue;
            }
            let Some(split) = split_path(path, format) else {
                continue;
            };
            let Some(&anchor) = root.children.get(&split.anchor) else {
                continue;
            };
            let mut node = &self.nodes[anchor];
            let mut found = node.rule.map(|rule| (rule, 0));
            for (i, part) in split.parts().enumerate() {
                let lookup = if format == PathFormat::Windows {
                    key.clear();
                    fold_windows_into(part, &mut key);
                    key.as_str()
                } else {
                    part
                };
                match node.children.get(lookup) {
                    Some(&next) => node = &self.nodes[next],
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
            // wins, and between equally deep ones, the format with the
            // higher tie priority. Within one trie a node holds one rule.
            let rank = |depth: usize, format: PathFormat| (depth, format.tie_priority());
            if best
                .as_ref()
                .is_none_or(|b| rank(depth, format) > rank(b.depth, b.format))
            {
                best = Some(Match {
                    rule,
                    depth,
                    format,
                    rest: split.rest,
                    consumed,
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
