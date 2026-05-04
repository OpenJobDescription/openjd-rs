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
use std::sync::LazyLock;

static DEFAULT_LIBRARY: LazyLock<FunctionLibrary> = LazyLock::new(build_default_library);

/// Get the cached default function library (shared, immutable).
pub fn get_default_library() -> &'static FunctionLibrary {
    &DEFAULT_LIBRARY
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
    // apply_path_mapping is host-context only — registered via with_host_context()
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
    use super::*;
    use crate::types::ExprType;

    #[test]
    fn default_library_has_all_categories() {
        let lib = get_default_library();
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
        let lib = get_default_library();
        assert_eq!(
            lib.derive_return_type("__add__", &[ExprType::INT, ExprType::INT]),
            Some(ExprType::INT)
        );
    }

    #[test]
    fn derive_return_type_add_float_coercion() {
        let lib = get_default_library();
        assert_eq!(
            lib.derive_return_type("__add__", &[ExprType::INT, ExprType::FLOAT]),
            Some(ExprType::FLOAT)
        );
    }

    #[test]
    fn derive_return_type_getitem_generic() {
        let lib = get_default_library();
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
        let lib = get_default_library();
        assert_eq!(
            lib.derive_return_type("sorted", &[ExprType::list(ExprType::INT)]),
            Some(ExprType::list(ExprType::INT))
        );
    }

    #[test]
    fn derive_return_type_comparison_operators() {
        let lib = get_default_library();
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
        let lib = get_default_library();
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
        let lib = get_default_library();
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
        let lib = get_default_library();
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
        let lib = get_default_library();
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
        let lib = get_default_library();
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
        let lib = get_default_library()
            .clone()
            .with_host_context(Vec::<crate::path_mapping::PathMappingRule>::new());
        assert!(!lib.get_signatures("apply_path_mapping").is_empty());
        assert!(lib.host_context_enabled);
    }
}
