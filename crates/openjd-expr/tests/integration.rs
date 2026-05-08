// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Consolidated integration-test binary.
//!
//! Each `tests/integration/test_*.rs` file is included as a module here so
//! cargo links one test executable for this crate instead of one per file.
//! This substantially cuts test link time, especially on Windows where every
//! test binary linked the full workspace + AWS SDK graph.

#[path = "integration/test_arithmetic.rs"]
mod test_arithmetic;
#[path = "integration/test_ast_validation.rs"]
mod test_ast_validation;
#[path = "integration/test_comparison.rs"]
mod test_comparison;
#[path = "integration/test_error_formatting.rs"]
mod test_error_formatting;
#[path = "integration/test_evaluation.rs"]
mod test_evaluation;
#[path = "integration/test_expr_value.rs"]
mod test_expr_value;
#[path = "integration/test_expression_depth.rs"]
mod test_expression_depth;
#[path = "integration/test_format_strings.rs"]
mod test_format_strings;
#[path = "integration/test_function_context.rs"]
mod test_function_context;
#[path = "integration/test_function_library.rs"]
mod test_function_library;
#[path = "integration/test_int64_bounds.rs"]
mod test_int64_bounds;
#[path = "integration/test_list_nesting.rs"]
mod test_list_nesting;
#[path = "integration/test_lists.rs"]
mod test_lists;
#[path = "integration/test_memory.rs"]
mod test_memory;
#[path = "integration/test_method_coercion.rs"]
mod test_method_coercion;
#[path = "integration/test_misc_builtins.rs"]
mod test_misc_builtins;
#[path = "integration/test_misc_getitem.rs"]
mod test_misc_getitem;
#[path = "integration/test_operation_limit.rs"]
mod test_operation_limit;
#[path = "integration/test_parse_expression.rs"]
mod test_parse_expression;
#[path = "integration/test_path_format_mismatch.rs"]
mod test_path_format_mismatch;
#[path = "integration/test_path_mapping.rs"]
mod test_path_mapping;
#[path = "integration/test_path_mapping_platform.rs"]
mod test_path_mapping_platform;
#[path = "integration/test_paths.rs"]
mod test_paths;
#[path = "integration/test_profile_threading.rs"]
mod test_profile_threading;
#[path = "integration/test_range_expr.rs"]
mod test_range_expr;
#[path = "integration/test_regex_validation.rs"]
mod test_regex_validation;
#[path = "integration/test_rfc_examples.rs"]
mod test_rfc_examples;
#[path = "integration/test_slicing.rs"]
mod test_slicing;
#[path = "integration/test_string_operation_counting.rs"]
mod test_string_operation_counting;
#[path = "integration/test_strings.rs"]
mod test_strings;
#[path = "integration/test_symbol_table.rs"]
mod test_symbol_table;
#[path = "integration/test_target_type_propagation.rs"]
mod test_target_type_propagation;
#[path = "integration/test_types.rs"]
mod test_types;
#[path = "integration/test_types_evaluate.rs"]
mod test_types_evaluate;
#[path = "integration/test_unicode_codepoint.rs"]
mod test_unicode_codepoint;
#[path = "integration/test_unresolved_eval.rs"]
mod test_unresolved_eval;
#[path = "integration/test_uri_paths.rs"]
mod test_uri_paths;
