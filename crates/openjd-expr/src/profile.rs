// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Expression profile: the tuple of (revision, extensions, host context)
//! that governs which functions, operators, and types are available for a
//! given evaluation.
//!
//! A profile is passed to
//! [`FunctionLibrary::for_profile`](crate::FunctionLibrary::for_profile) to
//! obtain a library that matches the requested revision, extensions, and
//! host context. Libraries are cached per *rules-independent* profile key,
//! so callers that construct many libraries with the same spec shape and
//! different path-mapping rules pay only the host-context registration
//! cost per call.
//!
//! The three axes modelled here correspond to the axes identified in the
//! forward-compatibility evaluation report:
//!
//! - **Axis A — revision**: which base functions and operators exist
//!   (see [`ExprRevision`]).
//! - **Axis B — extensions**: which add-on functions exist
//!   (see [`ExprExtension`]).
//! - **Axis C — host state**: whether host-context implementations are
//!   real, stubbed, or absent (see [`HostContext`]).
//!
//! Axis D (scope-specific symbol availability) is handled by the caller
//! building an appropriate [`SymbolTable`](crate::SymbolTable) — it is
//! orthogonal to the profile.

use std::collections::HashSet;
use std::sync::Arc;

use crate::path_mapping::PathMappingRule;

/// Expression-language specification revision.
///
/// Mirrors the `SpecificationRevision` enum in `openjd-model` but lives in
/// `openjd-expr` so the expression crate can model which revision it is
/// operating under without depending on the model crate.
///
/// Marked `#[non_exhaustive]` so future revisions can be added without a
/// SemVer break.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[non_exhaustive]
pub enum ExprRevision {
    /// The `2026-02` revision — the first revision to define the
    /// expression language (RFC 0005).
    V2026_02,
}

impl ExprRevision {
    /// The current revision. Equivalent to the most recent variant.
    pub const CURRENT: ExprRevision = ExprRevision::V2026_02;
}

impl Default for ExprRevision {
    fn default() -> Self {
        ExprRevision::CURRENT
    }
}

/// Expression-language extensions.
///
/// Expression-level extensions add or modify functions, operators, or
/// types beyond what the base revision provides. Today no such
/// extensions exist — the "EXPR" extension in `openjd-model` gates
/// whether the expression language is *available at all*, not which
/// functions are registered once it is available. This enum is therefore
/// defined as empty-but-`#[non_exhaustive]`, reserving the API shape for
/// the first expr-level extension.
///
/// Empty non-exhaustive enums are legal Rust and correctly express
/// "values may exist in the future, none exist today."
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ExprExtension {}

/// Host-context state available to expression evaluation.
///
/// Host-context functions (today: `apply_path_mapping`) need host-supplied
/// state that the evaluator has no knowledge of. This enum expresses the
/// three possible states of host availability in a single type, replacing
/// the previous split between `FunctionLibrary::with_host_context` and
/// `FunctionLibrary::with_unresolved_host_context`.
#[derive(Debug, Clone, Default)]
pub enum HostContext {
    /// No host-context functions are registered. Default.
    #[default]
    None,
    /// Host-context function *signatures* are registered with stub
    /// implementations that return `Unresolved(T)`. Use this at
    /// template-validation time, when real host state is not yet
    /// available but signatures must be known for type checking.
    Unresolved,
    /// Host-context functions are registered with implementations that
    /// use the supplied path mapping rules. Use this at runtime.
    ///
    /// Rules are shared via `Arc` so cloning a library is cheap.
    WithRules(Arc<Vec<PathMappingRule>>),
}

impl HostContext {
    /// Convenience constructor: take ownership of a `Vec<PathMappingRule>`
    /// and wrap it in an `Arc`.
    pub fn with_rules(rules: Vec<PathMappingRule>) -> Self {
        HostContext::WithRules(Arc::new(rules))
    }

    /// Whether this host context registers any host-context functions.
    pub fn is_enabled(&self) -> bool {
        !matches!(self, HostContext::None)
    }

    /// Whether this host context uses unresolved stub implementations.
    pub fn is_unresolved(&self) -> bool {
        matches!(self, HostContext::Unresolved)
    }
}

/// A complete expression profile: revision, enabled extensions, and host
/// context.
///
/// Passed to
/// [`FunctionLibrary::for_profile`](crate::FunctionLibrary::for_profile)
/// to obtain a library matching the profile.
///
/// # Examples
///
/// ```
/// use openjd_expr::{ExprProfile, ExprRevision, HostContext, FunctionLibrary};
///
/// // Default profile: current revision, no extensions, no host context.
/// let profile = ExprProfile::current();
/// let lib = FunctionLibrary::for_profile(&profile);
/// assert!(!lib.host_context_enabled);
///
/// // Template-validation profile: same as above but with unresolved host.
/// let profile = ExprProfile::current().with_host_context(HostContext::Unresolved);
/// let lib = FunctionLibrary::for_profile(&profile);
/// assert!(lib.host_context_enabled);
/// ```
#[derive(Debug, Clone)]
pub struct ExprProfile {
    revision: ExprRevision,
    extensions: HashSet<ExprExtension>,
    host_context: HostContext,
}

impl ExprProfile {
    /// Build a profile for the given revision with no extensions and no
    /// host context.
    pub fn new(revision: ExprRevision) -> Self {
        Self {
            revision,
            extensions: HashSet::new(),
            host_context: HostContext::None,
        }
    }

    /// Shortcut for `ExprProfile::new(ExprRevision::CURRENT)`.
    pub fn current() -> Self {
        Self::new(ExprRevision::CURRENT)
    }

    /// Set the enabled extensions (replaces any existing set).
    #[must_use]
    pub fn with_extensions(mut self, extensions: HashSet<ExprExtension>) -> Self {
        self.extensions = extensions;
        self
    }

    /// Set the host context.
    #[must_use]
    pub fn with_host_context(mut self, host_context: HostContext) -> Self {
        self.host_context = host_context;
        self
    }

    /// The specification revision this profile targets.
    pub fn revision(&self) -> ExprRevision {
        self.revision
    }

    /// The set of enabled extensions.
    pub fn extensions(&self) -> &HashSet<ExprExtension> {
        &self.extensions
    }

    /// The host context.
    pub fn host_context(&self) -> &HostContext {
        &self.host_context
    }

    /// Whether the given extension is enabled in this profile.
    pub fn has_extension(&self, ext: ExprExtension) -> bool {
        self.extensions.contains(&ext)
    }

    /// The cache key for the *rules-independent* portion of this profile.
    ///
    /// Libraries are cached on this key — profiles that differ only in
    /// which `Arc<Vec<PathMappingRule>>` they carry share a single cached
    /// skeleton, and `with_host_context(rules)` is applied on top when
    /// needed.
    pub(crate) fn cache_key(&self) -> ProfileKey {
        ProfileKey {
            revision: self.revision,
            extensions: {
                let mut v: Vec<ExprExtension> = self.extensions.iter().copied().collect();
                // ExprExtension is copyable and has no Ord today; compare
                // by hash-compatible means. With the current empty enum
                // the vec is always empty, but keep the sort for when
                // extensions are added.
                v.sort_by_key(|e| {
                    // Use Debug-formatted name as a stable order key.
                    // With an empty enum this branch is unreachable.
                    format!("{:?}", e)
                });
                v
            },
            host_kind: HostKind::from(&self.host_context),
        }
    }
}

impl Default for ExprProfile {
    fn default() -> Self {
        Self::current()
    }
}

/// The rules-independent portion of an [`ExprProfile`] used as a cache key.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct ProfileKey {
    pub(crate) revision: ExprRevision,
    pub(crate) extensions: Vec<ExprExtension>,
    pub(crate) host_kind: HostKind,
}

/// Which variety of [`HostContext`] is in use, ignoring any attached
/// rules. Used as part of the cache key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum HostKind {
    None,
    Unresolved,
    WithRules,
}

impl From<&HostContext> for HostKind {
    fn from(h: &HostContext) -> Self {
        match h {
            HostContext::None => HostKind::None,
            HostContext::Unresolved => HostKind::Unresolved,
            HostContext::WithRules(_) => HostKind::WithRules,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_profile_is_current() {
        let p = ExprProfile::default();
        assert_eq!(p.revision(), ExprRevision::CURRENT);
        assert!(p.extensions().is_empty());
        assert!(matches!(p.host_context(), HostContext::None));
    }

    #[test]
    fn current_matches_v2026_02() {
        // Until a second revision exists, CURRENT must be V2026_02.
        assert_eq!(ExprRevision::CURRENT, ExprRevision::V2026_02);
    }

    #[test]
    fn with_host_context_unresolved() {
        let p = ExprProfile::current().with_host_context(HostContext::Unresolved);
        assert!(p.host_context().is_enabled());
        assert!(p.host_context().is_unresolved());
    }

    #[test]
    fn with_host_context_rules() {
        let rules = vec![];
        let p = ExprProfile::current().with_host_context(HostContext::with_rules(rules));
        assert!(p.host_context().is_enabled());
        assert!(!p.host_context().is_unresolved());
    }

    #[test]
    fn cache_key_ignores_rules_content() {
        // Two profiles with different rules must produce the same cache key,
        // because `HostKind::WithRules` is the cache bucket, not the rules.
        use crate::path_mapping::{PathFormat, PathMappingRule};
        let r1 = PathMappingRule {
            source_path_format: PathFormat::Posix,
            source_path: "/a".into(),
            destination_path: "/b".into(),
        };
        let r2 = PathMappingRule {
            source_path_format: PathFormat::Posix,
            source_path: "/c".into(),
            destination_path: "/d".into(),
        };
        let p1 = ExprProfile::current().with_host_context(HostContext::with_rules(vec![r1]));
        let p2 = ExprProfile::current().with_host_context(HostContext::with_rules(vec![r2]));
        assert_eq!(p1.cache_key(), p2.cache_key());
    }

    #[test]
    fn cache_key_distinguishes_host_kinds() {
        let a = ExprProfile::current().cache_key(); // None
        let b = ExprProfile::current()
            .with_host_context(HostContext::Unresolved)
            .cache_key();
        let c = ExprProfile::current()
            .with_host_context(HostContext::with_rules(vec![]))
            .cache_key();
        assert_ne!(a, b);
        assert_ne!(a, c);
        assert_ne!(b, c);
    }
}
