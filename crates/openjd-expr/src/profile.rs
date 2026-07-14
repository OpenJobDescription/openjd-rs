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

impl ExprExtension {
    /// All extension variants, in a stable order. Used by
    /// [`ExprProfile::latest`] to construct a profile with every
    /// expression-level extension enabled.
    ///
    /// When a new variant is added, include it here. With no variants
    /// today the slice is empty; the constant still provides the
    /// contract that downstream code can rely on.
    pub const ALL: &'static [ExprExtension] = &[];
}

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

/// Optional language-syntax features that a profile may accept or reject.
///
/// **Crate-private**: this enum is consulted only by the parser's
/// structural validator ([`validate_structure`](crate::eval::parse)) via
/// [`ExprProfile::allows_syntax`]. External callers describe their
/// language flavor by constructing an `ExprProfile` with the appropriate
/// revision and extensions; they never reach for `SyntaxFeature`
/// directly. Keeping it `pub(crate)` means new variants and new match
/// arms in `allows_syntax` are not SemVer-visible.
///
/// The expression language accepts a Python subset. Which AST shapes
/// the parser accepts is governed by the profile's revision *and*
/// extensions: [`ExprProfile::allows_syntax`] resolves the decision in
/// two stages. The revision supplies a baseline (under 2026-02 every
/// variant below is rejected, matching the original Python
/// implementation); enabled extensions may then *additively* allow
/// features the baseline rejects. Extensions cannot remove features
/// the baseline allows.
///
/// A future revision may move a feature into its baseline (so the
/// extension is no longer needed under that revision) or define a
/// different set of extensions that contribute syntax.
///
/// Marked `#[non_exhaustive]` inside the crate as well — treated as
/// "never pattern-match non-exhaustively, because new variants will be
/// added," which keeps the exhaustive matches inside
/// `baseline_syntax_v2026_02` honest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub(crate) enum SyntaxFeature {
    // ── Expression-level syntax ──
    /// Walrus operator `:=`.
    Walrus,
    /// Lambda expressions, e.g. `lambda x: x + 1`.
    Lambda,
    /// Tuple literals, e.g. `(1, 2, 3)`.
    TupleLiteral,
    /// Dict literals, e.g. `{"a": 1}`.
    DictLiteral,
    /// Set literals, e.g. `{1, 2, 3}`.
    SetLiteral,
    /// Dict comprehensions, e.g. `{k: v for k, v in pairs}`.
    DictComprehension,
    /// Set comprehensions, e.g. `{x for x in xs}`.
    SetComprehension,
    /// Generator expressions, e.g. `(x for x in xs)`.
    GeneratorExpression,
    /// f-strings, e.g. `f"x={x}"`.
    FString,
    /// Ellipsis literal `...`.
    Ellipsis,
    /// Starred expressions, e.g. `*x`.
    Starred,
    /// Await expressions, e.g. `await x`.
    Await,
    /// Unicode string prefix, e.g. `u"..."`.
    UnicodeStringPrefix,
    /// Bytes literal, e.g. `b"..."`.
    BytesLiteral,

    // ── Binary / comparison / unary operators ──
    /// Bitwise AND `&`.
    BitwiseAnd,
    /// Bitwise OR `|`.
    BitwiseOr,
    /// Bitwise XOR `^`.
    BitwiseXor,
    /// Bitwise NOT `~`.
    BitwiseNot,
    /// Left shift `<<`.
    LeftShift,
    /// Right shift `>>`.
    RightShift,
    /// Matrix multiply `@`.
    MatMult,
    /// Identity operator `is`.
    IsOperator,
    /// Identity operator `is not`.
    IsNotOperator,

    // ── Call-site features ──
    /// Keyword arguments in function calls, e.g. `f(name=value)`.
    KeywordArguments,

    // ── List-comprehension features ──
    /// Multiple `for` clauses in a list comprehension,
    /// e.g. `[x for a in A for b in B]`.
    MultipleForClauses,
    /// Tuple unpacking as the loop target in a list comprehension,
    /// e.g. `[x for (a, b) in pairs]`.
    TupleUnpackingInComprehension,
    /// Multiple `if` clauses in a list comprehension,
    /// e.g. `[x for x in xs if a if b]`.
    MultipleIfClauses,
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
/// assert!(lib.get_signatures("apply_path_mapping").is_empty());
///
/// // Template-validation profile: same as above but with unresolved host.
/// let profile = ExprProfile::current().with_host_context(HostContext::Unresolved);
/// let lib = FunctionLibrary::for_profile(&profile);
/// assert!(!lib.get_signatures("apply_path_mapping").is_empty());
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
    ///
    /// Builds a profile with the current revision, *no* extensions, and
    /// no host context. Use this when you want a stable baseline: future
    /// crate versions that ship a new revision will change what
    /// [`ExprRevision::CURRENT`] points to, but the extensions set will
    /// remain explicitly empty, and the accepted syntax/functions are
    /// whatever the current revision defines without opt-in.
    pub fn current() -> Self {
        Self::new(ExprRevision::CURRENT)
    }

    /// Build a profile with the latest revision *and every known
    /// extension enabled*.
    ///
    /// **This profile is intentionally unstable across crate versions.**
    /// As new extensions are added to [`ExprExtension::ALL`] and new
    /// revisions land at [`ExprRevision::CURRENT`], the set of accepted
    /// syntax, functions, and types grows. An expression that parses
    /// under `latest()` today may fail to parse against a future version
    /// of this crate if its meaning changes under the new revision.
    ///
    /// `ParsedExpression::new` and `FormatString::new` use this profile
    /// as a quick-start default. For parse behavior that is stable
    /// across crate versions, construct a profile with an explicit
    /// revision and extension set via [`ExprProfile::new`] or
    /// [`ExprProfile::current`] and use
    /// [`ParsedExpression::with_profile`](crate::ParsedExpression::with_profile)
    /// / [`FormatString::with_profile`](crate::FormatString::with_profile).
    pub fn latest() -> Self {
        Self {
            revision: ExprRevision::CURRENT,
            extensions: ExprExtension::ALL.iter().copied().collect(),
            host_context: HostContext::None,
        }
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

    /// Whether this profile accepts the given optional syntax feature.
    ///
    /// **Crate-private**: consulted by the parser's structural
    /// validator; external callers do not construct `SyntaxFeature`
    /// values. They describe their desired language flavor through the
    /// profile's revision and extensions; this method is how the
    /// parser interrogates those choices.
    ///
    /// Resolved in two stages:
    ///
    /// 1. **Revision baseline.** Each revision defines a baseline set of
    ///    accepted features. Under 2026-02 every [`SyntaxFeature`]
    ///    variant is rejected by the baseline — the language accepts the
    ///    same Python subset as the original Python implementation.
    ///    A future revision may flip specific features to allowed at
    ///    baseline (e.g. if dict literals become part of the core
    ///    language).
    /// 2. **Extension layer.** Any extension enabled on the profile may
    ///    *additively* grant features the baseline rejects. Extensions
    ///    cannot take features away; if the baseline allows a feature,
    ///    the feature is allowed regardless of extensions. Which
    ///    extensions contribute which features is itself a
    ///    per-revision decision (an extension that enables feature X
    ///    under one revision may not exist, or mean something
    ///    different, under another), so the extension-layer dispatch
    ///    also matches on the revision.
    pub(crate) fn allows_syntax(&self, feature: SyntaxFeature) -> bool {
        // Stage 1: revision baseline. The match localizes where the
        // first revision bump needs to plug in its own baseline.
        let baseline_allows = match self.revision {
            ExprRevision::V2026_02 => Self::baseline_syntax_v2026_02(feature),
        };
        if baseline_allows {
            return true;
        }
        // Stage 2: per-revision extension layer. A given extension's
        // effect on the accepted syntax is revision-scoped, so this
        // second match is intentional and parallel to the first. Today
        // `ExprExtension` has no variants, so this function always
        // returns `false`; the structure is in place for the first
        // extension variant to plug in.
        match self.revision {
            ExprRevision::V2026_02 => self.extension_syntax_v2026_02(feature),
        }
    }

    /// Baseline syntax-feature acceptance for the 2026-02 revision.
    ///
    /// Extracted as an associated function (no `self`) to make the
    /// baseline self-contained and unambiguous — extension logic lives
    /// in [`Self::extension_syntax_v2026_02`].
    fn baseline_syntax_v2026_02(feature: SyntaxFeature) -> bool {
        // 2026-02 baseline: every optional syntax feature is rejected.
        // Exhaustive match so that adding a new `SyntaxFeature` variant
        // produces a compile error here rather than silently becoming
        // allowed.
        match feature {
            SyntaxFeature::Walrus
            | SyntaxFeature::Lambda
            | SyntaxFeature::TupleLiteral
            | SyntaxFeature::DictLiteral
            | SyntaxFeature::SetLiteral
            | SyntaxFeature::DictComprehension
            | SyntaxFeature::SetComprehension
            | SyntaxFeature::GeneratorExpression
            | SyntaxFeature::FString
            | SyntaxFeature::Ellipsis
            | SyntaxFeature::Starred
            | SyntaxFeature::Await
            | SyntaxFeature::UnicodeStringPrefix
            | SyntaxFeature::BytesLiteral
            | SyntaxFeature::BitwiseAnd
            | SyntaxFeature::BitwiseOr
            | SyntaxFeature::BitwiseXor
            | SyntaxFeature::BitwiseNot
            | SyntaxFeature::LeftShift
            | SyntaxFeature::RightShift
            | SyntaxFeature::MatMult
            | SyntaxFeature::IsOperator
            | SyntaxFeature::IsNotOperator
            | SyntaxFeature::KeywordArguments
            | SyntaxFeature::MultipleForClauses
            | SyntaxFeature::TupleUnpackingInComprehension
            | SyntaxFeature::MultipleIfClauses => false,
        }
    }

    /// Extension-layer syntax-feature acceptance for the 2026-02 revision.
    ///
    /// Iterates the profile's enabled extensions and asks each one
    /// whether it grants the feature under this revision. Today
    /// `ExprExtension` has no variants, so this function is defined as
    /// an empty iteration over `self.extensions` whose body would
    /// match on the extension variant. When the first variant is added,
    /// add a `match` arm here that returns `true` for each feature that
    /// variant contributes under V2026_02.
    #[allow(clippy::unused_self)] // placeholder: `self` is needed once extensions exist
    #[allow(clippy::never_loop)] // shape is preserved for when ExprExtension has variants
    fn extension_syntax_v2026_02(&self, feature: SyntaxFeature) -> bool {
        // With no `ExprExtension` variants today, the iteration body is
        // unreachable. The shape is kept so that adding a variant makes
        // it obvious where to plug in the grant logic.
        for ext in &self.extensions {
            // Exhaustive match: adding a new `ExprExtension` variant
            // produces a compile error here, forcing the contributor to
            // state which `SyntaxFeature`s (if any) that variant
            // enables under V2026_02.
            match *ext {
                // No variants today. When an extension is added that
                // enables a syntax feature, add a match arm like:
                //
                //     ExprExtension::DictLiteral => {
                //         if matches!(feature, SyntaxFeature::DictLiteral) {
                //             return true;
                //         }
                //     }
            }
        }
        let _ = feature; // silence unused warning until extensions exist
        false
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

    #[test]
    fn latest_enables_all_extensions() {
        let p = ExprProfile::latest();
        assert_eq!(p.revision(), ExprRevision::CURRENT);
        // Every extension in ALL must be present in the set.
        for ext in ExprExtension::ALL {
            assert!(
                p.has_extension(*ext),
                "ExprProfile::latest() must enable every extension in ExprExtension::ALL; missing {ext:?}"
            );
        }
        assert_eq!(p.extensions().len(), ExprExtension::ALL.len());
        assert!(matches!(p.host_context(), HostContext::None));
    }

    #[test]
    fn v2026_02_rejects_every_syntax_feature() {
        let p = ExprProfile::new(ExprRevision::V2026_02);
        // The full feature set must be rejected by the baseline 2026-02 profile.
        // If a future revision flips any of these to allowed, move it out of
        // this list and document the change.
        let all_features = [
            SyntaxFeature::Walrus,
            SyntaxFeature::Lambda,
            SyntaxFeature::TupleLiteral,
            SyntaxFeature::DictLiteral,
            SyntaxFeature::SetLiteral,
            SyntaxFeature::DictComprehension,
            SyntaxFeature::SetComprehension,
            SyntaxFeature::GeneratorExpression,
            SyntaxFeature::FString,
            SyntaxFeature::Ellipsis,
            SyntaxFeature::Starred,
            SyntaxFeature::Await,
            SyntaxFeature::UnicodeStringPrefix,
            SyntaxFeature::BytesLiteral,
            SyntaxFeature::BitwiseAnd,
            SyntaxFeature::BitwiseOr,
            SyntaxFeature::BitwiseXor,
            SyntaxFeature::BitwiseNot,
            SyntaxFeature::LeftShift,
            SyntaxFeature::RightShift,
            SyntaxFeature::MatMult,
            SyntaxFeature::IsOperator,
            SyntaxFeature::IsNotOperator,
            SyntaxFeature::KeywordArguments,
            SyntaxFeature::MultipleForClauses,
            SyntaxFeature::TupleUnpackingInComprehension,
            SyntaxFeature::MultipleIfClauses,
        ];
        for f in all_features {
            assert!(
                !p.allows_syntax(f),
                "Under V2026_02, SyntaxFeature::{f:?} must be rejected"
            );
        }
    }

    #[test]
    fn latest_rejects_same_features_as_current_for_v2026_02() {
        // With only one revision today, latest() and current() accept the
        // same syntax features. When a second revision ships this test
        // may need updating along with the feature gates.
        let cur = ExprProfile::current();
        let lat = ExprProfile::latest();
        assert!(!cur.allows_syntax(SyntaxFeature::Lambda));
        assert!(!lat.allows_syntax(SyntaxFeature::Lambda));
    }

    #[test]
    fn extension_layer_does_not_reject_baseline_allowed_features() {
        // Contract: extensions are additive. If the baseline accepts a
        // feature, no combination of extensions can cause
        // `allows_syntax` to return false. Today no SyntaxFeature is
        // baseline-allowed under V2026_02, so this test is vacuous on
        // the current revision set; it is kept as a guard for future
        // revisions that flip features into the baseline.
        let p_no_ext = ExprProfile::current();
        let p_all_ext = ExprProfile::latest();
        for f in [
            SyntaxFeature::Walrus,
            SyntaxFeature::Lambda,
            SyntaxFeature::DictLiteral,
            SyntaxFeature::SetLiteral,
            SyntaxFeature::FString,
            SyntaxFeature::KeywordArguments,
        ] {
            // If future baseline accepts f, extension-less and all-
            // extension profiles must both accept it (additivity).
            assert_eq!(p_no_ext.allows_syntax(f), p_all_ext.allows_syntax(f));
        }
    }
}
