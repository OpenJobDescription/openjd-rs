// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Default function library with all built-in signatures.
//!
//! Registers signatures for all operators, functions, and properties defined
//! in the expression language spec. Implementations are placeholders — the
//! evaluator's hardcoded match arms handle actual evaluation. The library
//! is used for static type checking via `derive_return_type`.

use crate::function_library::FunctionLibrary;
use crate::profile::{ExprProfile, ExprRevision, HostContext, HostKind, ProfileKey};
use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex};

pub(crate) static DEFAULT_LIBRARY: LazyLock<FunctionLibrary> = LazyLock::new(build_default_library);

/// Cache of per-profile libraries, keyed by the rules-independent portion
/// of an [`ExprProfile`].
///
/// Keeping the cache keyed on [`ProfileKey`] rather than on the full
/// profile means that callers can construct many libraries sharing the
/// same (revision, extensions, host kind) but different path-mapping
/// rules without thrashing the cache — the cached skeleton is cloned
/// once and the host-context registrations are applied on top.
static PROFILE_CACHE: LazyLock<Mutex<HashMap<ProfileKey, Arc<FunctionLibrary>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Build a library skeleton (no host context) for a given profile.
///
/// The skeleton is what goes into the profile cache. Host-context
/// functions are added on top, in `for_profile`, either as stubs
/// (`HostContext::Unresolved`) or as closures capturing rules
/// (`HostContext::WithRules`).
///
/// Revision selects the base library; the subsequent extension loop
/// merges in any expression-level extensions enabled on the profile.
/// Both the revision match and the per-extension match are
/// intentionally exhaustive-without-wildcard — adding a new
/// `ExprRevision` variant *or* a new `ExprExtension` variant produces
/// a compile error here, forcing an explicit decision about how the
/// new variant affects the library.
fn build_library_skeleton(profile: &ExprProfile) -> FunctionLibrary {
    let lib = match profile.revision() {
        ExprRevision::V2026_02 => {
            // Base library for 2026-02. Future revisions can diverge
            // here.
            build_default_library()
        } // Intentionally no wildcard: this match lives in the same
          // crate as `ExprRevision`, so adding a new revision variant
          // will produce a compile error here.
    };

    // Merge in expression-level extensions. `ExprExtension` has no
    // variants today, so this loop body is unreachable — but the
    // exhaustive match on `*ext` below is the forcing function that
    // makes adding the first variant produce a compile error at this
    // site, rather than silently inheriting the base library.
    //
    // `#[allow(clippy::never_loop)]` is required because the empty
    // match on an uninhabited-today enum diverges, so clippy's
    // `never_loop` lint flags the loop. That's exactly the property
    // we want preserved: when `ExprExtension` gains its first variant,
    // the empty match becomes a compile error and the lint stops
    // firing in one step.
    #[allow(clippy::never_loop)]
    for ext in profile.extensions() {
        match *ext {} // Intentionally empty — every future variant
                      // must add an arm describing how it modifies
                      // the library.
    }

    lib
}

impl FunctionLibrary {
    /// Construct or retrieve a cached function library matching the
    /// given expression profile.
    ///
    /// Libraries are cached keyed on the profile's
    /// (revision, extensions, host-kind) triple. Profiles that differ
    /// only in the specific `Arc<Vec<PathMappingRule>>` they carry share
    /// a cached skeleton; the rules are applied as a cheap clone-and-
    /// register on top for each call.
    ///
    /// # Examples
    ///
    /// ```
    /// use openjd_expr::{ExprProfile, FunctionLibrary, HostContext};
    ///
    /// // Default library for template validation — host functions available
    /// // as unresolved type-check stubs.
    /// let profile = ExprProfile::current().with_host_context(HostContext::Unresolved);
    /// let lib = FunctionLibrary::for_profile(&profile);
    /// assert!(lib.host_context_enabled);
    ///
    /// // Runtime library with real path mapping rules.
    /// let profile = ExprProfile::current()
    ///     .with_host_context(HostContext::with_rules(Vec::new()));
    /// let lib = FunctionLibrary::for_profile(&profile);
    /// assert!(lib.host_context_enabled);
    /// ```
    pub fn for_profile(profile: &ExprProfile) -> Arc<FunctionLibrary> {
        // Fast path: look up the skeleton (no-host or unresolved-host)
        // in the cache. Insert if missing.
        let key = profile.cache_key();
        let cached = {
            let mut cache = PROFILE_CACHE.lock().expect("profile cache mutex poisoned");
            Arc::clone(cache.entry(key.clone()).or_insert_with(|| {
                // Build a skeleton *without* any host-context
                // registrations. For the Unresolved bucket we then
                // register the stub inline; for WithRules we defer to
                // the caller below.
                let mut skeleton = build_library_skeleton(profile);
                if matches!(key.host_kind, HostKind::Unresolved) {
                    skeleton.host_context_enabled = true;
                    register_unresolved_host_context_functions(&mut skeleton);
                }
                Arc::new(skeleton)
            }))
        };

        // Slow path: WithRules always needs the rules applied on top of
        // a no-host skeleton. We cache the *no-host* skeleton, not the
        // WithRules variant, because the rules are per-call.
        if let HostContext::WithRules(rules) = profile.host_context() {
            // Pull the no-host skeleton (a separate cache entry, since
            // the key for this call is `WithRules` but we want the
            // `None` skeleton underneath). If it's absent, build it.
            let base_key = ProfileKey {
                host_kind: HostKind::None,
                ..key
            };
            let base = {
                let mut cache = PROFILE_CACHE.lock().expect("profile cache mutex poisoned");
                Arc::clone(
                    cache
                        .entry(base_key)
                        .or_insert_with(|| Arc::new(build_library_skeleton(profile))),
                )
            };
            // Clone the base library and register host context on the clone.
            // The clone is cheap — `FunctionLibrary` holds `Arc<dyn Fn>`
            // entries in its `HashMap`.
            let mut derived = (*base).clone();
            derived.host_context_enabled = true;
            register_host_context_functions(&mut derived, Arc::clone(rules));
            return Arc::new(derived);
        }

        cached
    }
}

/// Build the default library (called once by LazyLock).
fn build_default_library() -> FunctionLibrary {
    let lib = FunctionLibrary::new();
    lib.merge(arithmetic())
        .merge(string_ops())
        .merge(list_ops())
        .merge(comparison())
        .merge(math_ops())
        .merge(string_functions())
        .merge(list_functions())
        .merge(conversion())
        .merge(path_ops())
        .merge(repr_ops())
        .merge(regex_ops())
        .merge(misc())
}

fn arithmetic() -> FunctionLibrary {
    use crate::functions::arithmetic::*;
    let mut lib = FunctionLibrary::new();
    // int arithmetic
    lib.register_sig("__add__", "(int, int) -> int", add_int)
        .expect("bad builtin signature");
    lib.register_sig("__sub__", "(int, int) -> int", sub_int)
        .expect("bad builtin signature");
    lib.register_sig("__mul__", "(int, int) -> int", mul_int)
        .expect("bad builtin signature");
    lib.register_sig("__truediv__", "(int, int) -> float", truediv_int)
        .expect("bad builtin signature");
    lib.register_sig("__floordiv__", "(int, int) -> int", floordiv_int)
        .expect("bad builtin signature");
    lib.register_sig("__mod__", "(int, int) -> int", mod_int)
        .expect("bad builtin signature");
    lib.register_sig("__pow__", "(int, int) -> float | int", pow_int)
        .expect("bad builtin signature");
    lib.register_sig("__neg__", "(int) -> int", neg_int)
        .expect("bad builtin signature");
    lib.register_sig("__pos__", "(int) -> int", pos_int)
        .expect("bad builtin signature");
    // float arithmetic
    lib.register_sig("__add__", "(float, float) -> float", add_float)
        .expect("bad builtin signature");
    lib.register_sig("__sub__", "(float, float) -> float", sub_float)
        .expect("bad builtin signature");
    lib.register_sig("__mul__", "(float, float) -> float", mul_float)
        .expect("bad builtin signature");
    lib.register_sig("__truediv__", "(float, float) -> float", truediv_float)
        .expect("bad builtin signature");
    lib.register_sig("__floordiv__", "(float, float) -> int", floordiv_float)
        .expect("bad builtin signature");
    lib.register_sig("__mod__", "(float, float) -> float", mod_float)
        .expect("bad builtin signature");
    lib.register_sig("__pow__", "(float, float) -> float", pow_float)
        .expect("bad builtin signature");
    lib.register_sig("__neg__", "(float) -> float", neg_float)
        .expect("bad builtin signature");
    lib.register_sig("__pos__", "(float) -> float", pos_float)
        .expect("bad builtin signature");
    lib
}

fn string_ops() -> FunctionLibrary {
    use crate::functions::arithmetic::*;
    let mut lib = FunctionLibrary::new();
    lib.register_sig("__add__", "(string, string) -> string", add_string)
        .expect("bad builtin signature");
    lib.register_sig(
        "__add__",
        "(string, range_expr) -> string",
        add_string_range,
    )
    .expect("bad builtin signature");
    lib.register_sig(
        "__add__",
        "(range_expr, string) -> string",
        add_range_string,
    )
    .expect("bad builtin signature");
    lib.register_sig("__mul__", "(string, int) -> string", mul_string)
        .expect("bad builtin signature");
    lib
}

fn list_ops() -> FunctionLibrary {
    use crate::functions::arithmetic::*;
    let mut lib = FunctionLibrary::new();
    lib.register_sig("__add__", "(list[T1], list[T2]) -> list[T3]", add_list_list)
        .expect("bad builtin signature");
    lib.register_sig(
        "__add__",
        "(range_expr, list[T1]) -> list[T2]",
        add_range_list,
    )
    .expect("bad builtin signature");
    lib.register_sig(
        "__add__",
        "(list[T1], range_expr) -> list[T2]",
        add_list_range,
    )
    .expect("bad builtin signature");
    lib.register_sig(
        "__add__",
        "(range_expr, range_expr) -> list[int]",
        add_range_range,
    )
    .expect("bad builtin signature");
    lib.register_sig("__mul__", "(list[T1], int) -> list[T1]", mul_list)
        .expect("bad builtin signature");
    lib.register_sig(
        "__getitem__",
        "(list[T1], int) -> T1",
        crate::functions::misc::getitem_list,
    )
    .expect("bad builtin signature");
    lib.register_sig(
        "__getitem__",
        "(string, int) -> string",
        crate::functions::misc::getitem_string,
    )
    .expect("bad builtin signature");
    lib.register_sig(
        "__getitem__",
        "(range_expr, int) -> int",
        crate::functions::misc::getitem_range,
    )
    .expect("bad builtin signature");
    lib
}

fn comparison() -> FunctionLibrary {
    use crate::functions::arithmetic::not_bool;
    use crate::functions::comparison::*;
    let mut lib = FunctionLibrary::new();
    lib.register_sig("__not__", "(bool) -> bool", not_bool)
        .expect("bad builtin signature");
    // Equality / ordering — generic (T1, T2) -> bool
    lib.register_sig("__eq__", "(T1, T2) -> bool", eq_generic)
        .expect("bad builtin signature");
    lib.register_sig("__ne__", "(T1, T2) -> bool", ne_generic)
        .expect("bad builtin signature");
    lib.register_sig("__lt__", "(T1, T2) -> bool", lt_generic)
        .expect("bad builtin signature");
    lib.register_sig("__le__", "(T1, T2) -> bool", le_generic)
        .expect("bad builtin signature");
    lib.register_sig("__gt__", "(T1, T2) -> bool", gt_generic)
        .expect("bad builtin signature");
    lib.register_sig("__ge__", "(T1, T2) -> bool", ge_generic)
        .expect("bad builtin signature");
    // Containment — container first, item second
    lib.register_sig("__contains__", "(list[T1], T1) -> bool", contains_list)
        .expect("bad builtin signature");
    lib.register_sig("__contains__", "(range_expr, int) -> bool", contains_range)
        .expect("bad builtin signature");
    lib.register_sig("__contains__", "(string, string) -> bool", contains_string)
        .expect("bad builtin signature");
    lib.register_sig(
        "__not_contains__",
        "(list[T1], T1) -> bool",
        not_contains_list,
    )
    .expect("bad builtin signature");
    lib.register_sig(
        "__not_contains__",
        "(range_expr, int) -> bool",
        not_contains_range,
    )
    .expect("bad builtin signature");
    lib.register_sig(
        "__not_contains__",
        "(string, string) -> bool",
        not_contains_string,
    )
    .expect("bad builtin signature");
    // Slice — 4-arg __getitem__ overloads
    lib.register_sig(
        "__getitem__",
        "(list[T1], int | nulltype, int | nulltype, int | nulltype) -> list[T1]",
        slice_list,
    )
    .expect("bad builtin signature");
    lib.register_sig(
        "__getitem__",
        "(range_expr, int | nulltype, int | nulltype, int | nulltype) -> range_expr | list[int]",
        slice_range,
    )
    .expect("bad builtin signature");
    lib.register_sig(
        "__getitem__",
        "(string, int | nulltype, int | nulltype, int | nulltype) -> string",
        slice_string,
    )
    .expect("bad builtin signature");
    lib
}

fn math_ops() -> FunctionLibrary {
    use crate::functions::math::*;
    let mut lib = FunctionLibrary::new();
    lib.register_sig("min", "(int, int) -> int", min_fn)
        .expect("bad builtin signature");
    lib.register_sig("min", "(float, float) -> float", min_fn)
        .expect("bad builtin signature");
    lib.register_sig("min", "(int, int, int) -> int", min_fn)
        .expect("bad builtin signature");
    lib.register_sig("min", "(float, float, float) -> float", min_fn)
        .expect("bad builtin signature");
    lib.register_sig("min", "(list[int]) -> int", min_fn)
        .expect("bad builtin signature");
    lib.register_sig("min", "(list[float]) -> float", min_fn)
        .expect("bad builtin signature");
    lib.register_sig("min", "(range_expr) -> int", min_fn)
        .expect("bad builtin signature");
    lib.register_sig("min", "(list[nulltype]) -> noreturn", min_fn)
        .expect("bad builtin signature");
    lib.register_sig("max", "(int, int) -> int", max_fn)
        .expect("bad builtin signature");
    lib.register_sig("max", "(float, float) -> float", max_fn)
        .expect("bad builtin signature");
    lib.register_sig("max", "(int, int, int) -> int", max_fn)
        .expect("bad builtin signature");
    lib.register_sig("max", "(float, float, float) -> float", max_fn)
        .expect("bad builtin signature");
    lib.register_sig("max", "(list[int]) -> int", max_fn)
        .expect("bad builtin signature");
    lib.register_sig("max", "(list[float]) -> float", max_fn)
        .expect("bad builtin signature");
    lib.register_sig("max", "(range_expr) -> int", max_fn)
        .expect("bad builtin signature");
    lib.register_sig("max", "(list[nulltype]) -> noreturn", max_fn)
        .expect("bad builtin signature");
    lib.register_sig("floor", "(int) -> int", floor_int)
        .expect("bad builtin signature");
    lib.register_sig("floor", "(float) -> int", floor_float)
        .expect("bad builtin signature");
    lib.register_sig("ceil", "(int) -> int", ceil_int)
        .expect("bad builtin signature");
    lib.register_sig("ceil", "(float) -> int", ceil_float)
        .expect("bad builtin signature");
    lib.register_sig("round", "(float) -> int", round_fn)
        .expect("bad builtin signature");
    lib.register_sig("round", "(float, int) -> float | int", round_fn)
        .expect("bad builtin signature");
    lib.register_sig("round", "(int, int) -> int", round_fn)
        .expect("bad builtin signature");
    lib.register_sig("sum", "(list[int]) -> int", sum_list)
        .expect("bad builtin signature");
    lib.register_sig("sum", "(list[float]) -> float", sum_list)
        .expect("bad builtin signature");
    lib.register_sig("sum", "(list[nulltype]) -> int", sum_list)
        .expect("bad builtin signature");
    lib.register_sig("sum", "(range_expr) -> int", sum_list)
        .expect("bad builtin signature");
    lib
}

fn string_functions() -> FunctionLibrary {
    use crate::functions::string::*;
    let mut lib = FunctionLibrary::new();
    lib.register_sig("upper", "(string) -> string", upper_fn)
        .expect("bad builtin signature");
    lib.register_sig("lower", "(string) -> string", lower_fn)
        .expect("bad builtin signature");
    lib.register_sig("strip", "(string) -> string", strip_fn)
        .expect("bad builtin signature");
    lib.register_sig("strip", "(string, string) -> string", strip_fn)
        .expect("bad builtin signature");
    lib.register_sig("lstrip", "(string) -> string", lstrip_fn)
        .expect("bad builtin signature");
    lib.register_sig("lstrip", "(string, string) -> string", lstrip_fn)
        .expect("bad builtin signature");
    lib.register_sig("rstrip", "(string) -> string", rstrip_fn)
        .expect("bad builtin signature");
    lib.register_sig("rstrip", "(string, string) -> string", rstrip_fn)
        .expect("bad builtin signature");
    lib.register_sig("startswith", "(string, string) -> bool", startswith_fn)
        .expect("bad builtin signature");
    lib.register_sig("endswith", "(string, string) -> bool", endswith_fn)
        .expect("bad builtin signature");
    lib.register_sig("replace", "(string, string, string) -> string", replace_fn)
        .expect("bad builtin signature");
    lib.register_sig("split", "(string) -> list[string]", split_fn)
        .expect("bad builtin signature");
    lib.register_sig("split", "(string, string) -> list[string]", split_fn)
        .expect("bad builtin signature");
    lib.register_sig("split", "(string, string, int) -> list[string]", split_fn)
        .expect("bad builtin signature");
    lib.register_sig("rsplit", "(string) -> list[string]", rsplit_fn)
        .expect("bad builtin signature");
    lib.register_sig("rsplit", "(string, string) -> list[string]", rsplit_fn)
        .expect("bad builtin signature");
    lib.register_sig("rsplit", "(string, string, int) -> list[string]", rsplit_fn)
        .expect("bad builtin signature");
    lib.register_sig("find", "(string, string) -> int", find_fn)
        .expect("bad builtin signature");
    lib.register_sig("rfind", "(string, string) -> int", rfind_fn)
        .expect("bad builtin signature");
    lib.register_sig("index", "(string, string) -> int", index_fn)
        .expect("bad builtin signature");
    lib.register_sig("rindex", "(string, string) -> int", rindex_fn)
        .expect("bad builtin signature");
    lib.register_sig("count", "(string, string) -> int", count_fn)
        .expect("bad builtin signature");
    lib.register_sig(
        "removeprefix",
        "(string, string) -> string",
        removeprefix_fn,
    )
    .expect("bad builtin signature");
    lib.register_sig(
        "removesuffix",
        "(string, string) -> string",
        removesuffix_fn,
    )
    .expect("bad builtin signature");
    lib.register_sig("isdigit", "(string) -> bool", isdigit_fn)
        .expect("bad builtin signature");
    lib.register_sig("isalpha", "(string) -> bool", isalpha_fn)
        .expect("bad builtin signature");
    lib.register_sig("isalnum", "(string) -> bool", isalnum_fn)
        .expect("bad builtin signature");
    lib.register_sig("isspace", "(string) -> bool", isspace_fn)
        .expect("bad builtin signature");
    lib.register_sig("isupper", "(string) -> bool", isupper_fn)
        .expect("bad builtin signature");
    lib.register_sig("islower", "(string) -> bool", islower_fn)
        .expect("bad builtin signature");
    lib.register_sig("isascii", "(string) -> bool", isascii_fn)
        .expect("bad builtin signature");
    lib.register_sig("title", "(string) -> string", title_fn)
        .expect("bad builtin signature");
    lib.register_sig("capitalize", "(string) -> string", capitalize_fn)
        .expect("bad builtin signature");
    lib.register_sig("center", "(string, int) -> string", center_fn)
        .expect("bad builtin signature");
    lib.register_sig("ljust", "(string, int) -> string", ljust_fn)
        .expect("bad builtin signature");
    lib.register_sig("rjust", "(string, int) -> string", rjust_fn)
        .expect("bad builtin signature");
    lib
}

fn list_functions() -> FunctionLibrary {
    use crate::functions::list::*;
    let mut lib = FunctionLibrary::new();
    lib.register_sig("sorted", "(list[T1]) -> list[T1]", sorted_fn)
        .expect("bad builtin signature");
    lib.register_sig("reversed", "(list[T1]) -> list[T1]", reversed_fn)
        .expect("bad builtin signature");
    lib.register_sig("unique", "(list[T1]) -> list[T1]", unique_fn)
        .expect("bad builtin signature");
    lib.register_sig("flatten", "(list[list[T1]]) -> list[T1]", flatten_fn)
        .expect("bad builtin signature");
    lib.register_sig("flatten", "(list[T1]) -> list[T1]", flatten_fn)
        .expect("bad builtin signature");
    lib.register_sig("flatten", "(list[nulltype]) -> list[nulltype]", flatten_fn)
        .expect("bad builtin signature");
    lib.register_sig("join", "(list[string], string) -> string", join_fn)
        .expect("bad builtin signature");
    lib.register_sig("join", "(list[path], string) -> string", join_fn)
        .expect("bad builtin signature");
    lib.register_sig("join", "(list[nulltype], string) -> string", join_fn)
        .expect("bad builtin signature");
    lib.register_sig("range", "(int) -> list[int]", range_fn)
        .expect("bad builtin signature");
    lib.register_sig("range", "(int, int) -> list[int]", range_fn)
        .expect("bad builtin signature");
    lib.register_sig("range", "(int, int, int) -> list[int]", range_fn)
        .expect("bad builtin signature");
    lib
}

fn conversion() -> FunctionLibrary {
    use crate::functions::conversion::*;
    let mut lib = FunctionLibrary::new();
    lib.register_sig("int", "(int) -> int", int_from_int)
        .expect("bad builtin signature");
    lib.register_sig("int", "(float) -> int", int_from_float)
        .expect("bad builtin signature");
    lib.register_sig("int", "(string) -> int", int_from_string)
        .expect("bad builtin signature");
    lib.register_sig("float", "(float) -> float", float_from_float)
        .expect("bad builtin signature");
    lib.register_sig("float", "(int) -> float", float_from_int)
        .expect("bad builtin signature");
    lib.register_sig("float", "(string) -> float", float_from_string)
        .expect("bad builtin signature");
    lib.register_sig("string", "(int) -> string", string_fn)
        .expect("bad builtin signature");
    lib.register_sig("string", "(float) -> string", string_fn)
        .expect("bad builtin signature");
    lib.register_sig("string", "(bool) -> string", string_fn)
        .expect("bad builtin signature");
    lib.register_sig("string", "(string) -> string", string_fn)
        .expect("bad builtin signature");
    lib.register_sig("string", "(path) -> string", string_fn)
        .expect("bad builtin signature");
    lib.register_sig("string", "(nulltype) -> string", string_fn)
        .expect("bad builtin signature");
    lib.register_sig("string", "(list[T1]) -> string", string_fn)
        .expect("bad builtin signature");
    lib.register_sig("string", "(range_expr) -> string", string_fn)
        .expect("bad builtin signature");
    lib.register_sig("bool", "(bool) -> bool", bool_from_bool)
        .expect("bad builtin signature");
    lib.register_sig("bool", "(int) -> bool", bool_from_int)
        .expect("bad builtin signature");
    lib.register_sig("bool", "(float) -> bool", bool_from_float)
        .expect("bad builtin signature");
    lib.register_sig("bool", "(string) -> bool", bool_from_string)
        .expect("bad builtin signature");
    lib.register_sig("bool", "(nulltype) -> bool", bool_from_null)
        .expect("bad builtin signature");
    lib.register_sig("bool", "(path) -> noreturn", bool_from_path)
        .expect("bad builtin signature");
    lib.register_sig("bool", "(list[T]) -> noreturn", bool_from_list)
        .expect("bad builtin signature");
    lib.register_sig(
        "list",
        "(range_expr) -> list[int]",
        crate::functions::list::list_from_range,
    )
    .expect("bad builtin signature");
    lib.register_sig(
        "range_expr",
        "(string) -> range_expr",
        crate::functions::list::range_expr_from_string,
    )
    .expect("bad builtin signature");
    lib.register_sig(
        "range_expr",
        "(list[int]) -> range_expr",
        crate::functions::list::range_expr_from_list,
    )
    .expect("bad builtin signature");
    lib.register_sig(
        "range_expr",
        "(list[nulltype]) -> noreturn",
        crate::functions::list::range_expr_from_empty_list,
    )
    .expect("bad builtin signature");
    lib
}

fn path_ops() -> FunctionLibrary {
    use crate::functions::arithmetic::*;
    use crate::functions::path::*;
    let mut lib = FunctionLibrary::new();
    lib.register_sig("path", "(string) -> path", crate::functions::misc::path_fn)
        .expect("bad builtin signature");
    lib.register_sig(
        "path",
        "(list[string]) -> path",
        crate::functions::misc::path_fn,
    )
    .expect("bad builtin signature");
    lib.register_sig("__truediv__", "(path, string) -> path", path_div)
        .expect("bad builtin signature");
    lib.register_sig("__truediv__", "(path, path) -> path", path_div)
        .expect("bad builtin signature");
    lib.register_sig("__add__", "(path, string) -> path", add_path_string)
        .expect("bad builtin signature");
    lib.register_sig("as_posix", "(path) -> string", as_posix_fn)
        .expect("bad builtin signature");
    lib.register_sig("with_name", "(path, string) -> path", with_name_fn)
        .expect("bad builtin signature");
    lib.register_sig("with_stem", "(path, string) -> path", with_stem_fn)
        .expect("bad builtin signature");
    lib.register_sig("with_suffix", "(path, string) -> path", with_suffix_fn)
        .expect("bad builtin signature");
    lib.register_sig("with_number", "(path, int) -> path", with_number_fn)
        .expect("bad builtin signature");
    lib.register_sig("with_number", "(string, int) -> string", with_number_fn)
        .expect("bad builtin signature");
    lib.register_sig("is_absolute", "(path) -> bool", is_absolute_fn)
        .expect("bad builtin signature");
    lib.register_sig("is_relative_to", "(path, path) -> bool", is_relative_to_fn)
        .expect("bad builtin signature");
    lib.register_sig(
        "is_relative_to",
        "(path, string) -> bool",
        is_relative_to_fn,
    )
    .expect("bad builtin signature");
    lib.register_sig("relative_to", "(path, path) -> path", relative_to_fn)
        .expect("bad builtin signature");
    lib.register_sig("relative_to", "(path, string) -> path", relative_to_fn)
        .expect("bad builtin signature");
    // apply_path_mapping is host-context only — registered on a library
    // built via `FunctionLibrary::for_profile` with a host context of
    // `HostContext::WithRules` or `HostContext::Unresolved`.
    // Properties (handled by eval_attribute, registered for type checking)
    lib.register_sig("__property_name__", "(path) -> string", prop_name)
        .expect("bad builtin signature");
    lib.register_sig("__property_stem__", "(path) -> string", prop_stem)
        .expect("bad builtin signature");
    lib.register_sig("__property_suffix__", "(path) -> string", prop_suffix)
        .expect("bad builtin signature");
    lib.register_sig(
        "__property_suffixes__",
        "(path) -> list[string]",
        prop_suffixes,
    )
    .expect("bad builtin signature");
    lib.register_sig("__property_parent__", "(path) -> path", prop_parent)
        .expect("bad builtin signature");
    lib.register_sig("__property_parts__", "(path) -> list[string]", prop_parts)
        .expect("bad builtin signature");
    lib
}

fn repr_ops() -> FunctionLibrary {
    use crate::functions::repr::*;
    let mut lib = FunctionLibrary::new();
    for f in [
        (
            "repr_py",
            repr_py_fn
                as fn(
                    &mut dyn crate::function_library::EvalContext,
                    &[crate::value::ExprValue],
                )
                    -> Result<crate::value::ExprValue, crate::error::ExpressionError>,
        ),
        ("repr_json", repr_json_fn),
        ("repr_sh", repr_sh_fn),
        ("repr_cmd", repr_cmd_fn),
        ("repr_pwsh", repr_pwsh_fn),
    ] {
        lib.register_sig(f.0, "(int) -> string", f.1)
            .expect("bad builtin signature");
        lib.register_sig(f.0, "(float) -> string", f.1)
            .expect("bad builtin signature");
        lib.register_sig(f.0, "(string) -> string", f.1)
            .expect("bad builtin signature");
        lib.register_sig(f.0, "(bool) -> string", f.1)
            .expect("bad builtin signature");
        lib.register_sig(f.0, "(path) -> string", f.1)
            .expect("bad builtin signature");
        lib.register_sig(f.0, "(nulltype) -> string", f.1)
            .expect("bad builtin signature");
        lib.register_sig(f.0, "(list[T1]) -> string", f.1)
            .expect("bad builtin signature");
        lib.register_sig(f.0, "(range_expr) -> string", f.1)
            .expect("bad builtin signature");
    }
    lib
}

fn regex_ops() -> FunctionLibrary {
    use crate::functions::regex::*;
    let mut lib = FunctionLibrary::new();
    lib.register_sig("re_match", "(string, string) -> list[string]?", re_match_fn)
        .expect("bad builtin signature");
    lib.register_sig(
        "re_search",
        "(string, string) -> list[string]?",
        re_search_fn,
    )
    .expect("bad builtin signature");
    lib.register_sig(
        "re_findall",
        "(string, string) -> list[string]",
        re_findall_fn,
    )
    .expect("bad builtin signature");
    lib.register_sig(
        "re_findall",
        "(string, string) -> list[list[string]]",
        re_findall_fn,
    )
    .expect("bad builtin signature");
    lib.register_sig(
        "re_sub",
        "(string, string, string) -> string",
        re_replace_fn,
    )
    .expect("bad builtin signature");
    lib.register_sig("re_split", "(string, string) -> list[string]", re_split_fn)
        .expect("bad builtin signature");
    lib.register_sig(
        "re_split",
        "(string, string, int) -> list[string]",
        re_split_fn,
    )
    .expect("bad builtin signature");
    lib.register_sig("re_escape", "(string) -> string", re_escape_fn)
        .expect("bad builtin signature");
    lib
}

/// Register host-context-only functions (e.g. `apply_path_mapping`).
///
/// `rules` are captured by the registered closure and applied on every
/// `apply_path_mapping` call during evaluation.
pub fn register_host_context_functions(
    lib: &mut FunctionLibrary,
    rules: std::sync::Arc<Vec<crate::path_mapping::PathMappingRule>>,
) {
    lib.register_sig(
        "apply_path_mapping",
        "(string) -> path",
        crate::functions::path::make_apply_path_mapping_fn(rules),
    )
    .expect("bad builtin signature");
}

pub fn register_unresolved_host_context_functions(lib: &mut FunctionLibrary) {
    fn unresolved_apply_path_mapping(
        _ctx: &mut dyn crate::function_library::EvalContext,
        _a: &[crate::ExprValue],
    ) -> Result<crate::ExprValue, crate::ExpressionError> {
        Ok(crate::ExprValue::Unresolved(crate::ExprType::PATH))
    }
    lib.register_sig(
        "apply_path_mapping",
        "(string) -> path",
        unresolved_apply_path_mapping,
    )
    .expect("bad builtin signature");
}

fn misc() -> FunctionLibrary {
    use crate::functions::misc::*;
    let mut lib = FunctionLibrary::new();
    lib.register_sig("fail", "(string) -> noreturn", fail_fn)
        .expect("bad builtin signature");
    lib.register_sig("zfill", "(string, int) -> string", zfill_fn)
        .expect("bad builtin signature");
    lib.register_sig("zfill", "(int, int) -> string", zfill_fn)
        .expect("bad builtin signature");
    lib.register_sig("zfill", "(float, int) -> string", zfill_fn)
        .expect("bad builtin signature");
    lib.register_sig("any", "(list[bool]) -> bool", any_fn)
        .expect("bad builtin signature");
    lib.register_sig("any", "(list[nulltype]) -> bool", any_fn)
        .expect("bad builtin signature");
    lib.register_sig("all", "(list[bool]) -> bool", all_fn)
        .expect("bad builtin signature");
    lib.register_sig("all", "(list[nulltype]) -> bool", all_fn)
        .expect("bad builtin signature");
    lib.register_sig("abs", "(int) -> int", abs_int)
        .expect("bad builtin signature");
    lib.register_sig("abs", "(float) -> float", abs_float)
        .expect("bad builtin signature");
    lib.register_sig("len", "(string) -> int", len_string)
        .expect("bad builtin signature");
    lib.register_sig("len", "(path) -> int", len_path)
        .expect("bad builtin signature");
    lib.register_sig("len", "(list[T1]) -> int", len_list)
        .expect("bad builtin signature");
    lib.register_sig("len", "(range_expr) -> int", len_range)
        .expect("bad builtin signature");
    lib
}

#[cfg(test)]
mod tests {
    // Tests in this module exercise `DEFAULT_LIBRARY` directly because
    // they inspect the contents of the default skeleton; external callers
    // go through `FunctionLibrary::for_profile(&ExprProfile::current())`.
    use super::*;
    use crate::types::ExprType;

    #[test]
    fn default_library_has_all_categories() {
        let lib = &*DEFAULT_LIBRARY;
        // Spot check each category
        assert!(!lib.get_signatures("__add__").is_empty(), "arithmetic");
        assert!(!lib.get_signatures("upper").is_empty(), "string functions");
        assert!(!lib.get_signatures("sorted").is_empty(), "list functions");
        assert!(!lib.get_signatures("__not__").is_empty(), "not operator");
        assert!(!lib.get_signatures("abs").is_empty(), "math");
        assert!(!lib.get_signatures("int").is_empty(), "conversion");
        assert!(!lib.get_signatures("path").is_empty(), "path");
        assert!(!lib.get_signatures("repr_py").is_empty(), "repr");
        assert!(!lib.get_signatures("re_match").is_empty(), "regex");
        assert!(!lib.get_signatures("fail").is_empty(), "misc");
    }

    #[test]
    fn derive_return_type_add_int() {
        let lib = &*DEFAULT_LIBRARY;
        assert_eq!(
            lib.derive_return_type("__add__", &[ExprType::INT, ExprType::INT]),
            Some(ExprType::INT)
        );
    }

    #[test]
    fn derive_return_type_add_float_coercion() {
        let lib = &*DEFAULT_LIBRARY;
        assert_eq!(
            lib.derive_return_type("__add__", &[ExprType::INT, ExprType::FLOAT]),
            Some(ExprType::FLOAT)
        );
    }

    #[test]
    fn derive_return_type_getitem_generic() {
        let lib = &*DEFAULT_LIBRARY;
        assert_eq!(
            lib.derive_return_type(
                "__getitem__",
                &[ExprType::list(ExprType::STRING), ExprType::INT]
            ),
            Some(ExprType::STRING)
        );
    }

    #[test]
    fn derive_return_type_sorted_generic() {
        let lib = &*DEFAULT_LIBRARY;
        assert_eq!(
            lib.derive_return_type("sorted", &[ExprType::list(ExprType::INT)]),
            Some(ExprType::list(ExprType::INT))
        );
    }

    #[test]
    fn derive_return_type_comparison_operators() {
        let lib = &*DEFAULT_LIBRARY;
        assert_eq!(
            lib.derive_return_type("__eq__", &[ExprType::INT, ExprType::INT]),
            Some(ExprType::BOOL)
        );
        assert_eq!(
            lib.derive_return_type("__ne__", &[ExprType::STRING, ExprType::STRING]),
            Some(ExprType::BOOL)
        );
        assert_eq!(
            lib.derive_return_type("__lt__", &[ExprType::INT, ExprType::FLOAT]),
            Some(ExprType::BOOL)
        );
        assert_eq!(
            lib.derive_return_type("__ge__", &[ExprType::FLOAT, ExprType::INT]),
            Some(ExprType::BOOL)
        );
    }

    #[test]
    fn derive_return_type_contains_operators() {
        let lib = &*DEFAULT_LIBRARY;
        assert_eq!(
            lib.derive_return_type(
                "__contains__",
                &[ExprType::list(ExprType::INT), ExprType::INT]
            ),
            Some(ExprType::BOOL)
        );
        assert_eq!(
            lib.derive_return_type("__contains__", &[ExprType::STRING, ExprType::STRING]),
            Some(ExprType::BOOL)
        );
        assert_eq!(
            lib.derive_return_type(
                "__not_contains__",
                &[ExprType::list(ExprType::STRING), ExprType::STRING]
            ),
            Some(ExprType::BOOL)
        );
    }

    #[test]
    fn derive_return_type_slice_operators() {
        let lib = &*DEFAULT_LIBRARY;
        assert_eq!(
            lib.derive_return_type(
                "__getitem__",
                &[
                    ExprType::list(ExprType::INT),
                    ExprType::NULLTYPE,
                    ExprType::INT,
                    ExprType::NULLTYPE
                ]
            ),
            Some(ExprType::list(ExprType::INT))
        );
        assert_eq!(
            lib.derive_return_type(
                "__getitem__",
                &[
                    ExprType::STRING,
                    ExprType::INT,
                    ExprType::NULLTYPE,
                    ExprType::NULLTYPE
                ]
            ),
            Some(ExprType::STRING)
        );
    }

    #[test]
    fn get_property_type_path() {
        let lib = &*DEFAULT_LIBRARY;
        assert_eq!(
            lib.get_property_type(&ExprType::PATH, "name"),
            Some(ExprType::STRING)
        );
        assert_eq!(
            lib.get_property_type(&ExprType::PATH, "parent"),
            Some(ExprType::PATH)
        );
        assert_eq!(
            lib.get_property_type(&ExprType::PATH, "suffixes"),
            Some(ExprType::list(ExprType::STRING))
        );
        assert_eq!(lib.get_property_type(&ExprType::INT, "name"), None);
    }

    #[test]
    fn signature_count() {
        let lib = &*DEFAULT_LIBRARY;
        let total: usize = lib
            .function_names()
            .map(|n| lib.get_signatures(n).len())
            .sum();
        assert!(total >= 190, "total signatures: {total}");
    }

    #[test]
    fn all_signatures_have_real_implementations() {
        // Verify zero nyi — all signatures have real implementations
        let st = crate::SymbolTable::new();
        // Test a representative function from each category
        assert!(crate::ParsedExpression::new("1 + 2")
            .and_then(|p| p.evaluate(&st))
            .is_ok()); // arithmetic
        assert!(crate::ParsedExpression::new("'hello'.upper()")
            .and_then(|p| p.evaluate(&st))
            .is_ok()); // string
        assert!(crate::ParsedExpression::new("len([1,2,3])")
            .and_then(|p| p.evaluate(&st))
            .is_ok()); // list
        assert!(crate::ParsedExpression::new("abs(-5)")
            .and_then(|p| p.evaluate(&st))
            .is_ok()); // math
        assert!(crate::ParsedExpression::new("int('42')")
            .and_then(|p| p.evaluate(&st))
            .is_ok()); // conversion
        assert!(crate::ParsedExpression::new("repr_py(42)")
            .and_then(|p| p.evaluate(&st))
            .is_ok()); // repr
        assert!(crate::ParsedExpression::new("1 == 1")
            .and_then(|p| p.evaluate(&st))
            .is_ok()); // comparison
        assert!(crate::ParsedExpression::new("2 in [1,2,3]")
            .and_then(|p| p.evaluate(&st))
            .is_ok()); // contains
        assert!(crate::ParsedExpression::new("[1,2,3][0]")
            .and_then(|p| p.evaluate(&st))
            .is_ok()); // getitem
        assert!(crate::ParsedExpression::new("sorted([3,1,2])")
            .and_then(|p| p.evaluate(&st))
            .is_ok()); // list functions
        assert!(crate::ParsedExpression::new("range(5)")
            .and_then(|p| p.evaluate(&st))
            .is_ok()); // range
    }

    #[test]
    fn python_function_names_present() {
        let lib = &*DEFAULT_LIBRARY;
        // All function names from the Python implementation
        let expected = vec![
            "__add__",
            "__sub__",
            "__mul__",
            "__truediv__",
            "__floordiv__",
            "__mod__",
            "__pow__",
            "__neg__",
            "__pos__",
            "__not__",
            "__eq__",
            "__ne__",
            "__lt__",
            "__le__",
            "__gt__",
            "__ge__",
            "__contains__",
            "__not_contains__",
            "__getitem__",
            "__property_name__",
            "__property_stem__",
            "__property_suffix__",
            "__property_suffixes__",
            "__property_parent__",
            "__property_parts__",
            "abs",
            "all",
            "any",
            "as_posix",
            "bool",
            "capitalize",
            "ceil",
            "center",
            "count",
            "endswith",
            "fail",
            "find",
            "flatten",
            "float",
            "floor",
            "index",
            "int",
            "is_absolute",
            "is_relative_to",
            "isalnum",
            "isalpha",
            "isascii",
            "isdigit",
            "islower",
            "isspace",
            "isupper",
            "join",
            "len",
            "list",
            "ljust",
            "lower",
            "lstrip",
            "max",
            "min",
            "path",
            "range",
            "range_expr",
            "re_escape",
            "re_findall",
            "re_match",
            "re_search",
            "re_split",
            "re_sub",
            "relative_to",
            "removeprefix",
            "removesuffix",
            "replace",
            "repr_cmd",
            "repr_json",
            "repr_py",
            "repr_pwsh",
            "repr_sh",
            "reversed",
            "rfind",
            "rindex",
            "rjust",
            "round",
            "rsplit",
            "rstrip",
            "sorted",
            "split",
            "startswith",
            "string",
            "strip",
            "sum",
            "title",
            "unique",
            "upper",
            "with_name",
            "with_number",
            "with_stem",
            "with_suffix",
            "zfill",
        ];
        for name in &expected {
            assert!(
                !lib.get_signatures(name).is_empty(),
                "Missing function: {name}"
            );
        }
        // apply_path_mapping should NOT be in default library
        assert!(
            lib.get_signatures("apply_path_mapping").is_empty(),
            "apply_path_mapping should only be available with host context"
        );
    }

    #[test]
    fn host_context_has_apply_path_mapping() {
        let lib = FunctionLibrary::for_profile(&ExprProfile::current().with_host_context(
            HostContext::with_rules(Vec::<crate::path_mapping::PathMappingRule>::new()),
        ));
        assert!(!lib.get_signatures("apply_path_mapping").is_empty());
        assert!(lib.host_context_enabled);
    }
}

#[cfg(test)]
mod for_profile_tests {
    //! Tests that use only the non-deprecated `FunctionLibrary::for_profile`
    //! API. Kept out of the main `tests` module so they prove the new API
    //! works without relying on the deprecated surface.

    use crate::path_mapping::{PathFormat, PathMappingRule};
    use crate::profile::{ExprProfile, HostContext};
    use crate::FunctionLibrary;

    #[test]
    fn default_profile_has_builtins() {
        let lib = FunctionLibrary::for_profile(&ExprProfile::current());
        assert!(!lib.get_signatures("__add__").is_empty());
        assert!(!lib.get_signatures("upper").is_empty());
        // No host context → no apply_path_mapping.
        assert!(lib.get_signatures("apply_path_mapping").is_empty());
        assert!(!lib.host_context_enabled);
    }

    #[test]
    fn unresolved_host_context_registers_stub() {
        let profile = ExprProfile::current().with_host_context(HostContext::Unresolved);
        let lib = FunctionLibrary::for_profile(&profile);
        assert!(!lib.get_signatures("apply_path_mapping").is_empty());
        assert!(lib.host_context_enabled);
    }

    #[test]
    fn with_rules_host_context_registers_real_impl() {
        let rule = PathMappingRule {
            source_path_format: PathFormat::Posix,
            source_path: "/src".into(),
            destination_path: "/dst".into(),
        };
        let profile = ExprProfile::current().with_host_context(HostContext::with_rules(vec![rule]));
        let lib = FunctionLibrary::for_profile(&profile);
        assert!(!lib.get_signatures("apply_path_mapping").is_empty());
        assert!(lib.host_context_enabled);
    }

    #[test]
    fn cache_returns_same_arc_for_none_profile() {
        // Two requests with the same profile shape should share a cached
        // skeleton Arc.
        let profile = ExprProfile::current();
        let a = FunctionLibrary::for_profile(&profile);
        let b = FunctionLibrary::for_profile(&profile);
        assert!(std::sync::Arc::ptr_eq(&a, &b));
    }

    #[test]
    fn cache_returns_same_arc_for_unresolved_profile() {
        let profile = ExprProfile::current().with_host_context(HostContext::Unresolved);
        let a = FunctionLibrary::for_profile(&profile);
        let b = FunctionLibrary::for_profile(&profile);
        assert!(std::sync::Arc::ptr_eq(&a, &b));
    }

    #[test]
    fn with_rules_does_not_cache_rules_variant() {
        // Rules are per-call; the returned Arc must be fresh each time
        // so that different rule sets don't alias.
        let r1 = vec![PathMappingRule {
            source_path_format: PathFormat::Posix,
            source_path: "/a".into(),
            destination_path: "/b".into(),
        }];
        let r2 = vec![PathMappingRule {
            source_path_format: PathFormat::Posix,
            source_path: "/c".into(),
            destination_path: "/d".into(),
        }];
        let p1 = ExprProfile::current().with_host_context(HostContext::with_rules(r1));
        let p2 = ExprProfile::current().with_host_context(HostContext::with_rules(r2));
        let a = FunctionLibrary::for_profile(&p1);
        let b = FunctionLibrary::for_profile(&p2);
        assert!(!std::sync::Arc::ptr_eq(&a, &b));
    }

    #[test]
    fn evaluator_works_with_for_profile_library() {
        // Smoke test: evaluate a simple expression using a library from
        // for_profile, end-to-end.
        use crate::{ExprValue, ParsedExpression, SymbolTable};
        let lib = FunctionLibrary::for_profile(&ExprProfile::current());
        let parsed = ParsedExpression::new("1 + 2 * 3").unwrap();
        let st = SymbolTable::new();
        let v = parsed.with_library(&lib).evaluate(&[&st]).unwrap();
        assert_eq!(v, ExprValue::Int(7));
    }
}
