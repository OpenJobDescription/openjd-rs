// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Tests for EXPR parameter type check_value_constraints.
//! Asserts specific error messages per AGENTS.md test quality standard.

use crate::template::{
    FlexFloat, FlexInt, Identifier, JobBoolParameterDefinition, JobListBoolParameterDefinition,
    JobListFloatParameterDefinition, JobListIntParameterDefinition,
    JobListListIntParameterDefinition, JobListPathParameterDefinition,
    JobListStringParameterDefinition, JobRangeExprParameterDefinition, ListFloatItemConstraints,
    ListIntItemConstraints, ListListIntItemConstraints, ListStringItemConstraints,
};
use openjd_expr::{value::Float64, ExprType, ExprValue, PathFormat};

fn id(s: &str) -> Identifier {
    Identifier::new(s).unwrap()
}

// ============================================================
// BOOL
// ============================================================

#[test]
fn test_bool_accepts_true() {
    let p = JobBoolParameterDefinition {
        name: id("Flag"),
        description: None,
        default: None,
        user_interface: None,
    };
    assert!(p.check_value_constraints(&ExprValue::Bool(true)).is_ok());
}

#[test]
fn test_bool_rejects_string() {
    let p = JobBoolParameterDefinition {
        name: id("Flag"),
        description: None,
        default: None,
        user_interface: None,
    };
    let err = p
        .check_value_constraints(&ExprValue::String("invalid".into()))
        .unwrap_err();
    assert_eq!(err, "Parameter 'Flag': value 'invalid' is not a valid bool");
}

// ============================================================
// RANGE_EXPR
// ============================================================

#[test]
fn test_range_expr_accepts_valid() {
    let p = JobRangeExprParameterDefinition {
        name: id("Frames"),
        description: None,
        default: None,
        min_length: None,
        max_length: None,
        user_interface: None,
    };
    let r = "1-10".parse::<openjd_expr::RangeExpr>().unwrap();
    assert!(p.check_value_constraints(&ExprValue::RangeExpr(r)).is_ok());
}

#[test]
fn test_range_expr_rejects_too_long() {
    let p = JobRangeExprParameterDefinition {
        name: id("Frames"),
        description: None,
        default: None,
        min_length: None,
        max_length: Some(5),
        user_interface: None,
    };
    let r = "1-1000".parse::<openjd_expr::RangeExpr>().unwrap();
    let err = p
        .check_value_constraints(&ExprValue::RangeExpr(r))
        .unwrap_err();
    assert_eq!(err, "Parameter 'Frames': value length 6 exceeds maximum 5");
}

// ============================================================
// LIST[STRING]
// ============================================================

#[test]
fn test_list_string_accepts_valid() {
    let p = JobListStringParameterDefinition {
        name: id("Tags"),
        description: None,
        default: None,
        min_length: None,
        max_length: None,
        item: None,
        user_interface: None,
    };
    let v = ExprValue::ListString(vec!["a".into(), "b".into()], 0);
    assert!(p.check_value_constraints(&v).is_ok());
}

#[test]
fn test_list_string_rejects_too_many() {
    let p = JobListStringParameterDefinition {
        name: id("Tags"),
        description: None,
        default: None,
        min_length: None,
        max_length: Some(2),
        item: None,
        user_interface: None,
    };
    let v = ExprValue::ListString(vec!["a".into(), "b".into(), "c".into()], 0);
    let err = p.check_value_constraints(&v).unwrap_err();
    assert_eq!(err, "Parameter 'Tags': list length 3 exceeds maximum 2");
}

#[test]
fn test_list_string_item_too_long() {
    let p = JobListStringParameterDefinition {
        name: id("Tags"),
        description: None,
        default: None,
        min_length: None,
        max_length: None,
        item: Some(ListStringItemConstraints {
            allowed_values: None,
            min_length: None,
            max_length: Some(3),
        }),
        user_interface: None,
    };
    let v = ExprValue::ListString(vec!["toolong".into()], 0);
    let err = p.check_value_constraints(&v).unwrap_err();
    assert_eq!(err, "Parameter 'Tags': item[0] length 7 exceeds maximum 3");
}

// ============================================================
// LIST[INT]
// ============================================================

#[test]
fn test_list_int_accepts_valid() {
    let p = JobListIntParameterDefinition {
        name: id("Nums"),
        description: None,
        default: None,
        min_length: None,
        max_length: None,
        item: None,
        user_interface: None,
    };
    assert!(p
        .check_value_constraints(&ExprValue::ListInt(vec![1, 2, 3]))
        .is_ok());
}

#[test]
fn test_list_int_rejects_below_min() {
    let p = JobListIntParameterDefinition {
        name: id("Nums"),
        description: None,
        default: None,
        min_length: None,
        max_length: None,
        item: Some(ListIntItemConstraints {
            allowed_values: None,
            min_value: Some(crate::template::FlexInt(0)),
            max_value: None,
        }),
        user_interface: None,
    };
    let err = p
        .check_value_constraints(&ExprValue::ListInt(vec![-1]))
        .unwrap_err();
    assert_eq!(err, "Parameter 'Nums': item[0] -1 is less than minimum 0");
}

// ============================================================
// LIST[BOOL]
// ============================================================

#[test]
fn test_list_bool_accepts_valid() {
    let p = JobListBoolParameterDefinition {
        name: id("Flags"),
        description: None,
        default: None,
        min_length: None,
        max_length: None,
        user_interface: None,
    };
    assert!(p
        .check_value_constraints(&ExprValue::ListBool(vec![true, false]))
        .is_ok());
}

// ============================================================
// LIST[LIST[INT]]
// ============================================================

#[test]
fn test_list_list_int_inner_too_short() {
    let p = JobListListIntParameterDefinition {
        name: id("Matrix"),
        description: None,
        default: None,
        min_length: None,
        max_length: None,
        item: Some(ListListIntItemConstraints {
            min_length: Some(2),
            max_length: None,
            item: None,
        }),
        user_interface: None,
    };
    let v = ExprValue::ListList(
        vec![ExprValue::ListInt(vec![1])],
        ExprType::list(ExprType::INT),
        0,
    );
    let err = p.check_value_constraints(&v).unwrap_err();
    assert_eq!(
        err,
        "Parameter 'Matrix': item[0] length 1 is less than minimum 2"
    );
}

// ============================================================
// BOOL — additional tests
// ============================================================

#[test]
fn test_bool_accepts_false() {
    let p = JobBoolParameterDefinition {
        name: id("Flag"),
        description: None,
        default: None,
        user_interface: None,
    };
    assert!(p.check_value_constraints(&ExprValue::Bool(false)).is_ok());
}

#[test]
fn test_bool_rejects_float() {
    let p = JobBoolParameterDefinition {
        name: id("Flag"),
        description: None,
        default: None,
        user_interface: None,
    };
    assert!(p
        .check_value_constraints(&ExprValue::Float(Float64::new(0.5).unwrap()))
        .is_err());
}

// ============================================================
// RANGE_EXPR — additional tests
// ============================================================

#[test]
fn test_range_expr_rejects_too_short() {
    let p = JobRangeExprParameterDefinition {
        name: id("Frames"),
        description: None,
        default: None,
        min_length: Some(10),
        max_length: None,
        user_interface: None,
    };
    let r = "1-5".parse::<openjd_expr::RangeExpr>().unwrap();
    let err = p
        .check_value_constraints(&ExprValue::RangeExpr(r))
        .unwrap_err();
    assert!(err.contains("less than minimum"), "Got: {err}");
}

#[test]
fn test_range_expr_rejects_non_range() {
    let p = JobRangeExprParameterDefinition {
        name: id("Frames"),
        description: None,
        default: None,
        min_length: None,
        max_length: None,
        user_interface: None,
    };
    let err = p
        .check_value_constraints(&ExprValue::String("not a range".into()))
        .unwrap_err();
    assert!(err.contains("not a valid range"), "Got: {err}");
}

// ============================================================
// LIST[STRING] — additional tests
// ============================================================

#[test]
fn test_list_string_rejects_too_few() {
    let p = JobListStringParameterDefinition {
        name: id("Tags"),
        description: None,
        default: None,
        min_length: Some(2),
        max_length: None,
        item: None,
        user_interface: None,
    };
    let v = ExprValue::ListString(vec!["a".into()], 0);
    let err = p.check_value_constraints(&v).unwrap_err();
    assert!(err.contains("less than minimum"), "Got: {err}");
}

#[test]
fn test_list_string_item_not_in_allowed() {
    let p = JobListStringParameterDefinition {
        name: id("Tags"),
        description: None,
        default: None,
        min_length: None,
        max_length: None,
        item: Some(ListStringItemConstraints {
            allowed_values: Some(vec!["a".into(), "b".into()]),
            min_length: None,
            max_length: None,
        }),
        user_interface: None,
    };
    let v = ExprValue::ListString(vec!["c".into()], 0);
    let err = p.check_value_constraints(&v).unwrap_err();
    assert!(err.contains("not in allowed"), "Got: {err}");
}

// ============================================================
// LIST[INT] — additional tests
// ============================================================

#[test]
fn test_list_int_rejects_above_max() {
    let p = JobListIntParameterDefinition {
        name: id("Nums"),
        description: None,
        default: None,
        min_length: None,
        max_length: None,
        item: Some(ListIntItemConstraints {
            allowed_values: None,
            min_value: None,
            max_value: Some(crate::template::FlexInt(10)),
        }),
        user_interface: None,
    };
    let err = p
        .check_value_constraints(&ExprValue::ListInt(vec![11]))
        .unwrap_err();
    assert!(err.contains("exceeds maximum"), "Got: {err}");
}

#[test]
fn test_list_int_rejects_too_many() {
    let p = JobListIntParameterDefinition {
        name: id("Nums"),
        description: None,
        default: None,
        min_length: None,
        max_length: Some(2),
        item: None,
        user_interface: None,
    };
    let err = p
        .check_value_constraints(&ExprValue::ListInt(vec![1, 2, 3]))
        .unwrap_err();
    assert!(err.contains("exceeds maximum"), "Got: {err}");
}

// ============================================================
// LIST[BOOL] — additional tests
// ============================================================

#[test]
fn test_list_bool_rejects_too_many() {
    let p = JobListBoolParameterDefinition {
        name: id("Flags"),
        description: None,
        default: None,
        min_length: None,
        max_length: Some(1),
        user_interface: None,
    };
    let err = p
        .check_value_constraints(&ExprValue::ListBool(vec![true, false]))
        .unwrap_err();
    assert!(err.contains("exceeds maximum"), "Got: {err}");
}

// ============================================================
// BOOL — exhaustive accepted values per §2.9
// Spec: true/false, 0/1, "true"/"false"/"yes"/"no"/"on"/"off"/"1"/"0"
// ============================================================

fn bool_param() -> JobBoolParameterDefinition {
    JobBoolParameterDefinition {
        name: id("Flag"),
        description: None,
        default: None,
        user_interface: None,
    }
}

#[test]
fn test_bool_accepts_int_0() {
    assert!(bool_param()
        .check_value_constraints(&ExprValue::Int(0))
        .is_ok());
}

#[test]
fn test_bool_accepts_int_1() {
    assert!(bool_param()
        .check_value_constraints(&ExprValue::Int(1))
        .is_ok());
}

#[test]
fn test_bool_accepts_string_true() {
    assert!(bool_param()
        .check_value_constraints(&ExprValue::String("true".into()))
        .is_ok());
}

#[test]
fn test_bool_accepts_string_false() {
    assert!(bool_param()
        .check_value_constraints(&ExprValue::String("false".into()))
        .is_ok());
}

#[test]
fn test_bool_accepts_string_yes() {
    assert!(bool_param()
        .check_value_constraints(&ExprValue::String("yes".into()))
        .is_ok());
}

#[test]
fn test_bool_accepts_string_no() {
    assert!(bool_param()
        .check_value_constraints(&ExprValue::String("no".into()))
        .is_ok());
}

#[test]
fn test_bool_accepts_string_on() {
    assert!(bool_param()
        .check_value_constraints(&ExprValue::String("on".into()))
        .is_ok());
}

#[test]
fn test_bool_accepts_string_off() {
    assert!(bool_param()
        .check_value_constraints(&ExprValue::String("off".into()))
        .is_ok());
}

#[test]
fn test_bool_accepts_string_1() {
    assert!(bool_param()
        .check_value_constraints(&ExprValue::String("1".into()))
        .is_ok());
}

#[test]
fn test_bool_accepts_string_0() {
    assert!(bool_param()
        .check_value_constraints(&ExprValue::String("0".into()))
        .is_ok());
}

// Case-insensitive string acceptance
#[test]
fn test_bool_accepts_string_true_uppercase() {
    assert!(bool_param()
        .check_value_constraints(&ExprValue::String("TRUE".into()))
        .is_ok());
}

#[test]
fn test_bool_accepts_string_yes_mixed_case() {
    assert!(bool_param()
        .check_value_constraints(&ExprValue::String("Yes".into()))
        .is_ok());
}

// Boundary rejections — values just outside accepted set
#[test]
fn test_bool_rejects_int_2() {
    let err = bool_param()
        .check_value_constraints(&ExprValue::Int(2))
        .unwrap_err();
    assert_eq!(err, "Parameter 'Flag': expected bool, got int");
}

#[test]
fn test_bool_rejects_int_negative_1() {
    let err = bool_param()
        .check_value_constraints(&ExprValue::Int(-1))
        .unwrap_err();
    assert_eq!(err, "Parameter 'Flag': expected bool, got int");
}

#[test]
fn test_bool_rejects_string_maybe() {
    let err = bool_param()
        .check_value_constraints(&ExprValue::String("maybe".into()))
        .unwrap_err();
    assert_eq!(err, "Parameter 'Flag': value 'maybe' is not a valid bool");
}

#[test]
fn test_bool_rejects_string_2() {
    let err = bool_param()
        .check_value_constraints(&ExprValue::String("2".into()))
        .unwrap_err();
    assert_eq!(err, "Parameter 'Flag': value '2' is not a valid bool");
}

#[test]
fn test_bool_rejects_float_1_0() {
    // Float is not accepted per the runtime check_value_constraints (only deserialization accepts float)
    let err = bool_param()
        .check_value_constraints(&ExprValue::Float(Float64::new(1.0).unwrap()))
        .unwrap_err();
    assert_eq!(err, "Parameter 'Flag': expected bool, got float");
}

#[test]
fn test_bool_rejects_list() {
    let err = bool_param()
        .check_value_constraints(&ExprValue::ListBool(vec![true]))
        .unwrap_err();
    assert_eq!(err, "Parameter 'Flag': expected bool, got list");
}

#[test]
fn test_bool_rejects_null() {
    let err = bool_param()
        .check_value_constraints(&ExprValue::Null)
        .unwrap_err();
    assert_eq!(err, "Parameter 'Flag': expected bool, got null");
}

// ============================================================
// RANGE_EXPR — exhaustive per §2.10
// Accepts: RangeExpr value, String that parses as valid range expr
// Constraints: minLength/maxLength on string representation
// ============================================================

fn range_param() -> JobRangeExprParameterDefinition {
    JobRangeExprParameterDefinition {
        name: id("Frames"),
        description: None,
        default: None,
        min_length: None,
        max_length: None,
        user_interface: None,
    }
}

#[test]
fn test_range_expr_accepts_string_value() {
    assert!(range_param()
        .check_value_constraints(&ExprValue::String("1-10".into()))
        .is_ok());
}

#[test]
fn test_range_expr_accepts_string_with_step() {
    assert!(range_param()
        .check_value_constraints(&ExprValue::String("1-100:10".into()))
        .is_ok());
}

#[test]
fn test_range_expr_accepts_string_list() {
    assert!(range_param()
        .check_value_constraints(&ExprValue::String("1,3,5".into()))
        .is_ok());
}

#[test]
fn test_range_expr_rejects_invalid_string() {
    let err = range_param()
        .check_value_constraints(&ExprValue::String("not-a-range".into()))
        .unwrap_err();
    assert_eq!(
        err,
        "Parameter 'Frames': value 'not-a-range' is not a valid range expression"
    );
}

#[test]
fn test_range_expr_rejects_wrong_type_int() {
    let err = range_param()
        .check_value_constraints(&ExprValue::Int(5))
        .unwrap_err();
    assert_eq!(err, "Parameter 'Frames': expected range_expr, got int");
}

#[test]
fn test_range_expr_rejects_wrong_type_bool() {
    let err = range_param()
        .check_value_constraints(&ExprValue::Bool(true))
        .unwrap_err();
    assert_eq!(err, "Parameter 'Frames': expected range_expr, got bool");
}

#[test]
fn test_range_expr_rejects_wrong_type_float() {
    let err = range_param()
        .check_value_constraints(&ExprValue::Float(Float64::new(1.0).unwrap()))
        .unwrap_err();
    assert_eq!(err, "Parameter 'Frames': expected range_expr, got float");
}

// minLength boundary: exactly at min passes, one below fails
#[test]
fn test_range_expr_min_length_at_boundary() {
    let p = JobRangeExprParameterDefinition {
        name: id("Frames"),
        description: None,
        default: None,
        min_length: Some(3),
        max_length: None,
        user_interface: None,
    };
    let r = "1-5".parse::<openjd_expr::RangeExpr>().unwrap(); // "1-5" is 3 chars
    assert!(p.check_value_constraints(&ExprValue::RangeExpr(r)).is_ok());
}

#[test]
fn test_range_expr_min_length_one_below() {
    let p = JobRangeExprParameterDefinition {
        name: id("Frames"),
        description: None,
        default: None,
        min_length: Some(4),
        max_length: None,
        user_interface: None,
    };
    let r = "1-5".parse::<openjd_expr::RangeExpr>().unwrap(); // "1-5" is 3 chars
    let err = p
        .check_value_constraints(&ExprValue::RangeExpr(r))
        .unwrap_err();
    assert_eq!(
        err,
        "Parameter 'Frames': value length 3 is less than minimum 4"
    );
}

// maxLength boundary: exactly at max passes, one above fails
#[test]
fn test_range_expr_max_length_at_boundary() {
    let p = JobRangeExprParameterDefinition {
        name: id("Frames"),
        description: None,
        default: None,
        min_length: None,
        max_length: Some(3),
        user_interface: None,
    };
    let r = "1-5".parse::<openjd_expr::RangeExpr>().unwrap(); // "1-5" is 3 chars
    assert!(p.check_value_constraints(&ExprValue::RangeExpr(r)).is_ok());
}

#[test]
fn test_range_expr_max_length_one_above() {
    let p = JobRangeExprParameterDefinition {
        name: id("Frames"),
        description: None,
        default: None,
        min_length: None,
        max_length: Some(2),
        user_interface: None,
    };
    let r = "1-5".parse::<openjd_expr::RangeExpr>().unwrap(); // "1-5" is 3 chars
    let err = p
        .check_value_constraints(&ExprValue::RangeExpr(r))
        .unwrap_err();
    assert_eq!(err, "Parameter 'Frames': value length 3 exceeds maximum 2");
}

// String input also checks length constraints
#[test]
fn test_range_expr_string_min_length_check() {
    let p = JobRangeExprParameterDefinition {
        name: id("Frames"),
        description: None,
        default: None,
        min_length: Some(10),
        max_length: None,
        user_interface: None,
    };
    let err = p
        .check_value_constraints(&ExprValue::String("1-5".into()))
        .unwrap_err();
    assert_eq!(
        err,
        "Parameter 'Frames': value length 3 is less than minimum 10"
    );
}

#[test]
fn test_range_expr_string_max_length_check() {
    let p = JobRangeExprParameterDefinition {
        name: id("Frames"),
        description: None,
        default: None,
        min_length: None,
        max_length: Some(2),
        user_interface: None,
    };
    let err = p
        .check_value_constraints(&ExprValue::String("1-5".into()))
        .unwrap_err();
    assert_eq!(err, "Parameter 'Frames': value length 3 exceeds maximum 2");
}

// ============================================================
// LIST[STRING] — additional coverage per §2.11
// Accepts: ListString, ListPath (path is a subtype of string)
// Constraints: minLength/maxLength on list, item minLength/maxLength/allowedValues
// ============================================================

#[test]
fn test_list_string_accepts_list_path() {
    let p = JobListStringParameterDefinition {
        name: id("Tags"),
        description: None,
        default: None,
        min_length: None,
        max_length: None,
        item: None,
        user_interface: None,
    };
    let v = ExprValue::ListPath(vec!["/tmp/a".into()], PathFormat::Posix, 0);
    assert!(p.check_value_constraints(&v).is_ok());
}

#[test]
fn test_list_string_rejects_wrong_type() {
    let p = JobListStringParameterDefinition {
        name: id("Tags"),
        description: None,
        default: None,
        min_length: None,
        max_length: None,
        item: None,
        user_interface: None,
    };
    let err = p
        .check_value_constraints(&ExprValue::ListInt(vec![1]))
        .unwrap_err();
    assert_eq!(err, "Parameter 'Tags': expected list[string], got list");
}

#[test]
fn test_list_string_item_too_short() {
    let p = JobListStringParameterDefinition {
        name: id("Tags"),
        description: None,
        default: None,
        min_length: None,
        max_length: None,
        item: Some(ListStringItemConstraints {
            allowed_values: None,
            min_length: Some(3),
            max_length: None,
        }),
        user_interface: None,
    };
    let v = ExprValue::ListString(vec!["ab".into()], 0);
    let err = p.check_value_constraints(&v).unwrap_err();
    assert_eq!(
        err,
        "Parameter 'Tags': item[0] length 2 is less than minimum 3"
    );
}

// Boundary: item exactly at min_length passes
#[test]
fn test_list_string_item_at_min_length() {
    let p = JobListStringParameterDefinition {
        name: id("Tags"),
        description: None,
        default: None,
        min_length: None,
        max_length: None,
        item: Some(ListStringItemConstraints {
            allowed_values: None,
            min_length: Some(3),
            max_length: None,
        }),
        user_interface: None,
    };
    let v = ExprValue::ListString(vec!["abc".into()], 0);
    assert!(p.check_value_constraints(&v).is_ok());
}

// Boundary: item exactly at max_length passes
#[test]
fn test_list_string_item_at_max_length() {
    let p = JobListStringParameterDefinition {
        name: id("Tags"),
        description: None,
        default: None,
        min_length: None,
        max_length: None,
        item: Some(ListStringItemConstraints {
            allowed_values: None,
            min_length: None,
            max_length: Some(3),
        }),
        user_interface: None,
    };
    let v = ExprValue::ListString(vec!["abc".into()], 0);
    assert!(p.check_value_constraints(&v).is_ok());
}

// List length boundaries
#[test]
fn test_list_string_at_min_length() {
    let p = JobListStringParameterDefinition {
        name: id("Tags"),
        description: None,
        default: None,
        min_length: Some(2),
        max_length: None,
        item: None,
        user_interface: None,
    };
    let v = ExprValue::ListString(vec!["a".into(), "b".into()], 0);
    assert!(p.check_value_constraints(&v).is_ok());
}

#[test]
fn test_list_string_at_max_length() {
    let p = JobListStringParameterDefinition {
        name: id("Tags"),
        description: None,
        default: None,
        min_length: None,
        max_length: Some(2),
        item: None,
        user_interface: None,
    };
    let v = ExprValue::ListString(vec!["a".into(), "b".into()], 0);
    assert!(p.check_value_constraints(&v).is_ok());
}

// ============================================================
// LIST[PATH] — entirely new coverage per §2.12
// Accepts: ListString, ListPath
// Constraints: minLength/maxLength on list, item minLength/maxLength/allowedValues
// ============================================================

fn list_path_param() -> JobListPathParameterDefinition {
    JobListPathParameterDefinition {
        name: id("Paths"),
        description: None,
        object_type: None,
        data_flow: None,
        default: None,
        min_length: None,
        max_length: None,
        item: None,
        user_interface: None,
    }
}

#[test]
fn test_list_path_accepts_list_string() {
    let v = ExprValue::ListString(vec!["/tmp/a".into()], 0);
    assert!(list_path_param().check_value_constraints(&v).is_ok());
}

#[test]
fn test_list_path_accepts_list_path() {
    let v = ExprValue::ListPath(vec!["/tmp/a".into()], PathFormat::Posix, 0);
    assert!(list_path_param().check_value_constraints(&v).is_ok());
}

#[test]
fn test_list_path_accepts_empty() {
    let v = ExprValue::ListString(vec![], 0);
    assert!(list_path_param().check_value_constraints(&v).is_ok());
}

#[test]
fn test_list_path_rejects_wrong_type_int() {
    let err = list_path_param()
        .check_value_constraints(&ExprValue::ListInt(vec![1]))
        .unwrap_err();
    assert_eq!(err, "Parameter 'Paths': expected list[path], got list");
}

#[test]
fn test_list_path_rejects_wrong_type_bool() {
    let err = list_path_param()
        .check_value_constraints(&ExprValue::Bool(true))
        .unwrap_err();
    assert_eq!(err, "Parameter 'Paths': expected list[path], got bool");
}

// List length boundaries
#[test]
fn test_list_path_at_min_length() {
    let mut p = list_path_param();
    p.min_length = Some(2);
    let v = ExprValue::ListString(vec!["/a".into(), "/b".into()], 0);
    assert!(p.check_value_constraints(&v).is_ok());
}

#[test]
fn test_list_path_one_below_min_length() {
    let mut p = list_path_param();
    p.min_length = Some(2);
    let v = ExprValue::ListString(vec!["/a".into()], 0);
    let err = p.check_value_constraints(&v).unwrap_err();
    assert_eq!(
        err,
        "Parameter 'Paths': list length 1 is less than minimum 2"
    );
}

#[test]
fn test_list_path_at_max_length() {
    let mut p = list_path_param();
    p.max_length = Some(2);
    let v = ExprValue::ListString(vec!["/a".into(), "/b".into()], 0);
    assert!(p.check_value_constraints(&v).is_ok());
}

#[test]
fn test_list_path_one_above_max_length() {
    let mut p = list_path_param();
    p.max_length = Some(2);
    let v = ExprValue::ListString(vec!["/a".into(), "/b".into(), "/c".into()], 0);
    let err = p.check_value_constraints(&v).unwrap_err();
    assert_eq!(err, "Parameter 'Paths': list length 3 exceeds maximum 2");
}

// Item constraints
#[test]
fn test_list_path_item_at_min_length() {
    let mut p = list_path_param();
    p.item = Some(ListStringItemConstraints {
        allowed_values: None,
        min_length: Some(3),
        max_length: None,
    });
    let v = ExprValue::ListString(vec!["abc".into()], 0);
    assert!(p.check_value_constraints(&v).is_ok());
}

#[test]
fn test_list_path_item_below_min_length() {
    let mut p = list_path_param();
    p.item = Some(ListStringItemConstraints {
        allowed_values: None,
        min_length: Some(3),
        max_length: None,
    });
    let v = ExprValue::ListString(vec!["ab".into()], 0);
    let err = p.check_value_constraints(&v).unwrap_err();
    assert_eq!(
        err,
        "Parameter 'Paths': item[0] length 2 is less than minimum 3"
    );
}

#[test]
fn test_list_path_item_at_max_length() {
    let mut p = list_path_param();
    p.item = Some(ListStringItemConstraints {
        allowed_values: None,
        min_length: None,
        max_length: Some(3),
    });
    let v = ExprValue::ListString(vec!["abc".into()], 0);
    assert!(p.check_value_constraints(&v).is_ok());
}

#[test]
fn test_list_path_item_above_max_length() {
    let mut p = list_path_param();
    p.item = Some(ListStringItemConstraints {
        allowed_values: None,
        min_length: None,
        max_length: Some(3),
    });
    let v = ExprValue::ListString(vec!["abcd".into()], 0);
    let err = p.check_value_constraints(&v).unwrap_err();
    assert_eq!(err, "Parameter 'Paths': item[0] length 4 exceeds maximum 3");
}

#[test]
fn test_list_path_item_in_allowed() {
    let mut p = list_path_param();
    p.item = Some(ListStringItemConstraints {
        allowed_values: Some(vec!["/tmp/a".into(), "/tmp/b".into()]),
        min_length: None,
        max_length: None,
    });
    let v = ExprValue::ListString(vec!["/tmp/a".into()], 0);
    assert!(p.check_value_constraints(&v).is_ok());
}

#[test]
fn test_list_path_item_not_in_allowed() {
    let mut p = list_path_param();
    p.item = Some(ListStringItemConstraints {
        allowed_values: Some(vec!["/tmp/a".into(), "/tmp/b".into()]),
        min_length: None,
        max_length: None,
    });
    let v = ExprValue::ListString(vec!["/tmp/c".into()], 0);
    let err = p.check_value_constraints(&v).unwrap_err();
    assert_eq!(
        err,
        "Parameter 'Paths': item[0] '/tmp/c' is not in allowed values"
    );
}

// Error reports correct index for second item
#[test]
fn test_list_path_error_reports_correct_index() {
    let mut p = list_path_param();
    p.item = Some(ListStringItemConstraints {
        allowed_values: None,
        min_length: None,
        max_length: Some(3),
    });
    let v = ExprValue::ListString(vec!["ok".into(), "toolong".into()], 0);
    let err = p.check_value_constraints(&v).unwrap_err();
    assert_eq!(err, "Parameter 'Paths': item[1] length 7 exceeds maximum 3");
}

// ============================================================
// LIST[FLOAT] — entirely new coverage per §2.14
// Accepts: ListFloat
// Constraints: minLength/maxLength on list, item minValue/maxValue/allowedValues
// ============================================================

fn list_float_param() -> JobListFloatParameterDefinition {
    JobListFloatParameterDefinition {
        name: id("Weights"),
        description: None,
        default: None,
        min_length: None,
        max_length: None,
        item: None,
        user_interface: None,
    }
}

#[test]
fn test_list_float_accepts_valid() {
    let v = ExprValue::ListFloat(vec![Float64::new(1.0).unwrap(), Float64::new(2.5).unwrap()]);
    assert!(list_float_param().check_value_constraints(&v).is_ok());
}

#[test]
fn test_list_float_accepts_empty() {
    let v = ExprValue::ListFloat(vec![]);
    assert!(list_float_param().check_value_constraints(&v).is_ok());
}

#[test]
fn test_list_float_rejects_wrong_type_int() {
    let err = list_float_param()
        .check_value_constraints(&ExprValue::ListInt(vec![1]))
        .unwrap_err();
    assert_eq!(err, "Parameter 'Weights': expected list[float], got list");
}

#[test]
fn test_list_float_rejects_wrong_type_string() {
    let err = list_float_param()
        .check_value_constraints(&ExprValue::String("1.0".into()))
        .unwrap_err();
    assert_eq!(err, "Parameter 'Weights': expected list[float], got string");
}

// List length boundaries
#[test]
fn test_list_float_at_min_length() {
    let mut p = list_float_param();
    p.min_length = Some(2);
    let v = ExprValue::ListFloat(vec![Float64::new(1.0).unwrap(), Float64::new(2.0).unwrap()]);
    assert!(p.check_value_constraints(&v).is_ok());
}

#[test]
fn test_list_float_one_below_min_length() {
    let mut p = list_float_param();
    p.min_length = Some(2);
    let v = ExprValue::ListFloat(vec![Float64::new(1.0).unwrap()]);
    let err = p.check_value_constraints(&v).unwrap_err();
    assert_eq!(
        err,
        "Parameter 'Weights': list length 1 is less than minimum 2"
    );
}

#[test]
fn test_list_float_at_max_length() {
    let mut p = list_float_param();
    p.max_length = Some(2);
    let v = ExprValue::ListFloat(vec![Float64::new(1.0).unwrap(), Float64::new(2.0).unwrap()]);
    assert!(p.check_value_constraints(&v).is_ok());
}

#[test]
fn test_list_float_one_above_max_length() {
    let mut p = list_float_param();
    p.max_length = Some(2);
    let v = ExprValue::ListFloat(vec![
        Float64::new(1.0).unwrap(),
        Float64::new(2.0).unwrap(),
        Float64::new(3.0).unwrap(),
    ]);
    let err = p.check_value_constraints(&v).unwrap_err();
    assert_eq!(err, "Parameter 'Weights': list length 3 exceeds maximum 2");
}

// Item minValue boundary
#[test]
fn test_list_float_item_at_min_value() {
    let mut p = list_float_param();
    p.item = Some(ListFloatItemConstraints {
        allowed_values: None,
        min_value: Some(FlexFloat(0.0, None)),
        max_value: None,
    });
    let v = ExprValue::ListFloat(vec![Float64::new(0.0).unwrap()]);
    assert!(p.check_value_constraints(&v).is_ok());
}

#[test]
fn test_list_float_item_below_min_value() {
    let mut p = list_float_param();
    p.item = Some(ListFloatItemConstraints {
        allowed_values: None,
        min_value: Some(FlexFloat(0.0, None)),
        max_value: None,
    });
    let v = ExprValue::ListFloat(vec![Float64::new(-0.1).unwrap()]);
    let err = p.check_value_constraints(&v).unwrap_err();
    assert!(err.contains("is less than minimum"), "Got: {err}");
}

// Item maxValue boundary
#[test]
fn test_list_float_item_at_max_value() {
    let mut p = list_float_param();
    p.item = Some(ListFloatItemConstraints {
        allowed_values: None,
        min_value: None,
        max_value: Some(FlexFloat(10.0, None)),
    });
    let v = ExprValue::ListFloat(vec![Float64::new(10.0).unwrap()]);
    assert!(p.check_value_constraints(&v).is_ok());
}

#[test]
fn test_list_float_item_above_max_value() {
    let mut p = list_float_param();
    p.item = Some(ListFloatItemConstraints {
        allowed_values: None,
        min_value: None,
        max_value: Some(FlexFloat(10.0, None)),
    });
    let v = ExprValue::ListFloat(vec![Float64::new(10.1).unwrap()]);
    let err = p.check_value_constraints(&v).unwrap_err();
    assert!(err.contains("exceeds maximum"), "Got: {err}");
}

// Item allowedValues
#[test]
fn test_list_float_item_in_allowed() {
    let mut p = list_float_param();
    p.item = Some(ListFloatItemConstraints {
        allowed_values: Some(vec![FlexFloat(1.0, None), FlexFloat(2.0, None)]),
        min_value: None,
        max_value: None,
    });
    let v = ExprValue::ListFloat(vec![Float64::new(1.0).unwrap()]);
    assert!(p.check_value_constraints(&v).is_ok());
}

#[test]
fn test_list_float_item_not_in_allowed() {
    let mut p = list_float_param();
    p.item = Some(ListFloatItemConstraints {
        allowed_values: Some(vec![FlexFloat(1.0, None), FlexFloat(2.0, None)]),
        min_value: None,
        max_value: None,
    });
    let v = ExprValue::ListFloat(vec![Float64::new(3.0).unwrap()]);
    let err = p.check_value_constraints(&v).unwrap_err();
    assert!(err.contains("is not in allowed values"), "Got: {err}");
}

// Error reports correct index
#[test]
fn test_list_float_error_reports_correct_index() {
    let mut p = list_float_param();
    p.item = Some(ListFloatItemConstraints {
        allowed_values: None,
        min_value: None,
        max_value: Some(FlexFloat(5.0, None)),
    });
    let v = ExprValue::ListFloat(vec![
        Float64::new(1.0).unwrap(),
        Float64::new(10.0).unwrap(),
    ]);
    let err = p.check_value_constraints(&v).unwrap_err();
    assert!(err.contains("item[1]"), "Got: {err}");
}

// ============================================================
// LIST[INT] — additional coverage per §2.13
// ============================================================

#[test]
fn test_list_int_rejects_wrong_type() {
    let p = JobListIntParameterDefinition {
        name: id("Nums"),
        description: None,
        default: None,
        min_length: None,
        max_length: None,
        item: None,
        user_interface: None,
    };
    let err = p
        .check_value_constraints(&ExprValue::ListFloat(vec![Float64::new(1.0).unwrap()]))
        .unwrap_err();
    assert_eq!(err, "Parameter 'Nums': expected list[int], got list");
}

// Item at boundary
#[test]
fn test_list_int_item_at_min_value() {
    let p = JobListIntParameterDefinition {
        name: id("Nums"),
        description: None,
        default: None,
        min_length: None,
        max_length: None,
        item: Some(ListIntItemConstraints {
            allowed_values: None,
            min_value: Some(FlexInt(0)),
            max_value: None,
        }),
        user_interface: None,
    };
    assert!(p
        .check_value_constraints(&ExprValue::ListInt(vec![0]))
        .is_ok());
}

#[test]
fn test_list_int_item_at_max_value() {
    let p = JobListIntParameterDefinition {
        name: id("Nums"),
        description: None,
        default: None,
        min_length: None,
        max_length: None,
        item: Some(ListIntItemConstraints {
            allowed_values: None,
            min_value: None,
            max_value: Some(FlexInt(10)),
        }),
        user_interface: None,
    };
    assert!(p
        .check_value_constraints(&ExprValue::ListInt(vec![10]))
        .is_ok());
}

#[test]
fn test_list_int_item_in_allowed() {
    let p = JobListIntParameterDefinition {
        name: id("Nums"),
        description: None,
        default: None,
        min_length: None,
        max_length: None,
        item: Some(ListIntItemConstraints {
            allowed_values: Some(vec![FlexInt(1), FlexInt(2), FlexInt(3)]),
            min_value: None,
            max_value: None,
        }),
        user_interface: None,
    };
    assert!(p
        .check_value_constraints(&ExprValue::ListInt(vec![2]))
        .is_ok());
}

#[test]
fn test_list_int_item_not_in_allowed() {
    let p = JobListIntParameterDefinition {
        name: id("Nums"),
        description: None,
        default: None,
        min_length: None,
        max_length: None,
        item: Some(ListIntItemConstraints {
            allowed_values: Some(vec![FlexInt(1), FlexInt(2), FlexInt(3)]),
            min_value: None,
            max_value: None,
        }),
        user_interface: None,
    };
    let err = p
        .check_value_constraints(&ExprValue::ListInt(vec![4]))
        .unwrap_err();
    assert_eq!(err, "Parameter 'Nums': item[0] 4 is not in allowed values");
}

// List length boundaries
#[test]
fn test_list_int_at_min_length() {
    let p = JobListIntParameterDefinition {
        name: id("Nums"),
        description: None,
        default: None,
        min_length: Some(2),
        max_length: None,
        item: None,
        user_interface: None,
    };
    assert!(p
        .check_value_constraints(&ExprValue::ListInt(vec![1, 2]))
        .is_ok());
}

#[test]
fn test_list_int_one_below_min_length() {
    let p = JobListIntParameterDefinition {
        name: id("Nums"),
        description: None,
        default: None,
        min_length: Some(2),
        max_length: None,
        item: None,
        user_interface: None,
    };
    let err = p
        .check_value_constraints(&ExprValue::ListInt(vec![1]))
        .unwrap_err();
    assert_eq!(
        err,
        "Parameter 'Nums': list length 1 is less than minimum 2"
    );
}

// ============================================================
// LIST[BOOL] — additional coverage per §2.15
// Accepts: ListBool only
// Constraints: minLength/maxLength on list (no item constraints)
// ============================================================

#[test]
fn test_list_bool_rejects_wrong_type_int() {
    let p = JobListBoolParameterDefinition {
        name: id("Flags"),
        description: None,
        default: None,
        min_length: None,
        max_length: None,
        user_interface: None,
    };
    let err = p
        .check_value_constraints(&ExprValue::ListInt(vec![1]))
        .unwrap_err();
    assert_eq!(err, "Parameter 'Flags': expected list[bool], got list");
}

#[test]
fn test_list_bool_rejects_wrong_type_string() {
    let p = JobListBoolParameterDefinition {
        name: id("Flags"),
        description: None,
        default: None,
        min_length: None,
        max_length: None,
        user_interface: None,
    };
    let err = p
        .check_value_constraints(&ExprValue::String("true".into()))
        .unwrap_err();
    assert_eq!(err, "Parameter 'Flags': expected list[bool], got string");
}

#[test]
fn test_list_bool_accepts_empty() {
    let p = JobListBoolParameterDefinition {
        name: id("Flags"),
        description: None,
        default: None,
        min_length: None,
        max_length: None,
        user_interface: None,
    };
    assert!(p
        .check_value_constraints(&ExprValue::ListBool(vec![]))
        .is_ok());
}

#[test]
fn test_list_bool_at_min_length() {
    let p = JobListBoolParameterDefinition {
        name: id("Flags"),
        description: None,
        default: None,
        min_length: Some(2),
        max_length: None,
        user_interface: None,
    };
    assert!(p
        .check_value_constraints(&ExprValue::ListBool(vec![true, false]))
        .is_ok());
}

#[test]
fn test_list_bool_one_below_min_length() {
    let p = JobListBoolParameterDefinition {
        name: id("Flags"),
        description: None,
        default: None,
        min_length: Some(2),
        max_length: None,
        user_interface: None,
    };
    let err = p
        .check_value_constraints(&ExprValue::ListBool(vec![true]))
        .unwrap_err();
    assert_eq!(
        err,
        "Parameter 'Flags': list length 1 is less than minimum 2"
    );
}

// ============================================================
// LIST[LIST[INT]] — comprehensive coverage per §2.16
// Accepts: ListList where each inner element is ListInt
// Constraints: outer minLength/maxLength, inner item minLength/maxLength,
//              inner item.item minValue/maxValue/allowedValues
// ============================================================

fn list_list_int_param() -> JobListListIntParameterDefinition {
    JobListListIntParameterDefinition {
        name: id("Matrix"),
        description: None,
        default: None,
        min_length: None,
        max_length: None,
        item: None,
        user_interface: None,
    }
}

#[test]
fn test_list_list_int_accepts_valid() {
    let v = ExprValue::ListList(
        vec![ExprValue::ListInt(vec![1, 2]), ExprValue::ListInt(vec![3])],
        ExprType::list(ExprType::INT),
        0,
    );
    assert!(list_list_int_param().check_value_constraints(&v).is_ok());
}

#[test]
fn test_list_list_int_accepts_empty() {
    let v = ExprValue::ListList(vec![], ExprType::list(ExprType::INT), 0);
    assert!(list_list_int_param().check_value_constraints(&v).is_ok());
}

#[test]
fn test_list_list_int_accepts_empty_inner() {
    let v = ExprValue::ListList(
        vec![ExprValue::ListInt(vec![])],
        ExprType::list(ExprType::INT),
        0,
    );
    assert!(list_list_int_param().check_value_constraints(&v).is_ok());
}

#[test]
fn test_list_list_int_rejects_wrong_type() {
    let err = list_list_int_param()
        .check_value_constraints(&ExprValue::ListInt(vec![1]))
        .unwrap_err();
    assert_eq!(
        err,
        "Parameter 'Matrix': expected list[list[int]], got list"
    );
}

#[test]
fn test_list_list_int_rejects_wrong_type_scalar() {
    let err = list_list_int_param()
        .check_value_constraints(&ExprValue::Int(1))
        .unwrap_err();
    assert_eq!(err, "Parameter 'Matrix': expected list[list[int]], got int");
}

// Wrong inner type
#[test]
fn test_list_list_int_rejects_wrong_inner_type() {
    let mut p = list_list_int_param();
    p.item = Some(ListListIntItemConstraints {
        min_length: None,
        max_length: None,
        item: None,
    });
    let v = ExprValue::ListList(
        vec![ExprValue::ListString(vec!["a".into()], 0)],
        ExprType::list(ExprType::INT),
        0,
    );
    let err = p.check_value_constraints(&v).unwrap_err();
    assert_eq!(
        err,
        "Parameter 'Matrix': item[0] expected list[int], got list"
    );
}

// Outer list length boundaries
#[test]
fn test_list_list_int_at_min_length() {
    let mut p = list_list_int_param();
    p.min_length = Some(2);
    let v = ExprValue::ListList(
        vec![ExprValue::ListInt(vec![1]), ExprValue::ListInt(vec![2])],
        ExprType::list(ExprType::INT),
        0,
    );
    assert!(p.check_value_constraints(&v).is_ok());
}

#[test]
fn test_list_list_int_one_below_min_length() {
    let mut p = list_list_int_param();
    p.min_length = Some(2);
    let v = ExprValue::ListList(
        vec![ExprValue::ListInt(vec![1])],
        ExprType::list(ExprType::INT),
        0,
    );
    let err = p.check_value_constraints(&v).unwrap_err();
    assert_eq!(
        err,
        "Parameter 'Matrix': list length 1 is less than minimum 2"
    );
}

#[test]
fn test_list_list_int_at_max_length() {
    let mut p = list_list_int_param();
    p.max_length = Some(2);
    let v = ExprValue::ListList(
        vec![ExprValue::ListInt(vec![1]), ExprValue::ListInt(vec![2])],
        ExprType::list(ExprType::INT),
        0,
    );
    assert!(p.check_value_constraints(&v).is_ok());
}

#[test]
fn test_list_list_int_one_above_max_length() {
    let mut p = list_list_int_param();
    p.max_length = Some(2);
    let v = ExprValue::ListList(
        vec![
            ExprValue::ListInt(vec![1]),
            ExprValue::ListInt(vec![2]),
            ExprValue::ListInt(vec![3]),
        ],
        ExprType::list(ExprType::INT),
        0,
    );
    let err = p.check_value_constraints(&v).unwrap_err();
    assert_eq!(err, "Parameter 'Matrix': list length 3 exceeds maximum 2");
}

// Inner list length boundaries
#[test]
fn test_list_list_int_inner_at_min_length() {
    let mut p = list_list_int_param();
    p.item = Some(ListListIntItemConstraints {
        min_length: Some(2),
        max_length: None,
        item: None,
    });
    let v = ExprValue::ListList(
        vec![ExprValue::ListInt(vec![1, 2])],
        ExprType::list(ExprType::INT),
        0,
    );
    assert!(p.check_value_constraints(&v).is_ok());
}

#[test]
fn test_list_list_int_inner_too_long() {
    let mut p = list_list_int_param();
    p.item = Some(ListListIntItemConstraints {
        min_length: None,
        max_length: Some(2),
        item: None,
    });
    let v = ExprValue::ListList(
        vec![ExprValue::ListInt(vec![1, 2, 3])],
        ExprType::list(ExprType::INT),
        0,
    );
    let err = p.check_value_constraints(&v).unwrap_err();
    assert_eq!(
        err,
        "Parameter 'Matrix': item[0] length 3 exceeds maximum 2"
    );
}

#[test]
fn test_list_list_int_inner_at_max_length() {
    let mut p = list_list_int_param();
    p.item = Some(ListListIntItemConstraints {
        min_length: None,
        max_length: Some(2),
        item: None,
    });
    let v = ExprValue::ListList(
        vec![ExprValue::ListInt(vec![1, 2])],
        ExprType::list(ExprType::INT),
        0,
    );
    assert!(p.check_value_constraints(&v).is_ok());
}

// Inner item value constraints
#[test]
fn test_list_list_int_inner_item_at_min_value() {
    let mut p = list_list_int_param();
    p.item = Some(ListListIntItemConstraints {
        min_length: None,
        max_length: None,
        item: Some(ListIntItemConstraints {
            allowed_values: None,
            min_value: Some(FlexInt(0)),
            max_value: None,
        }),
    });
    let v = ExprValue::ListList(
        vec![ExprValue::ListInt(vec![0])],
        ExprType::list(ExprType::INT),
        0,
    );
    assert!(p.check_value_constraints(&v).is_ok());
}

#[test]
fn test_list_list_int_inner_item_below_min_value() {
    let mut p = list_list_int_param();
    p.item = Some(ListListIntItemConstraints {
        min_length: None,
        max_length: None,
        item: Some(ListIntItemConstraints {
            allowed_values: None,
            min_value: Some(FlexInt(0)),
            max_value: None,
        }),
    });
    let v = ExprValue::ListList(
        vec![ExprValue::ListInt(vec![-1])],
        ExprType::list(ExprType::INT),
        0,
    );
    let err = p.check_value_constraints(&v).unwrap_err();
    assert_eq!(
        err,
        "Parameter 'Matrix': item[0][0] -1 is less than minimum 0"
    );
}

#[test]
fn test_list_list_int_inner_item_at_max_value() {
    let mut p = list_list_int_param();
    p.item = Some(ListListIntItemConstraints {
        min_length: None,
        max_length: None,
        item: Some(ListIntItemConstraints {
            allowed_values: None,
            min_value: None,
            max_value: Some(FlexInt(10)),
        }),
    });
    let v = ExprValue::ListList(
        vec![ExprValue::ListInt(vec![10])],
        ExprType::list(ExprType::INT),
        0,
    );
    assert!(p.check_value_constraints(&v).is_ok());
}

#[test]
fn test_list_list_int_inner_item_above_max_value() {
    let mut p = list_list_int_param();
    p.item = Some(ListListIntItemConstraints {
        min_length: None,
        max_length: None,
        item: Some(ListIntItemConstraints {
            allowed_values: None,
            min_value: None,
            max_value: Some(FlexInt(10)),
        }),
    });
    let v = ExprValue::ListList(
        vec![ExprValue::ListInt(vec![11])],
        ExprType::list(ExprType::INT),
        0,
    );
    let err = p.check_value_constraints(&v).unwrap_err();
    assert_eq!(err, "Parameter 'Matrix': item[0][0] 11 exceeds maximum 10");
}

#[test]
fn test_list_list_int_inner_item_in_allowed() {
    let mut p = list_list_int_param();
    p.item = Some(ListListIntItemConstraints {
        min_length: None,
        max_length: None,
        item: Some(ListIntItemConstraints {
            allowed_values: Some(vec![FlexInt(1), FlexInt(2)]),
            min_value: None,
            max_value: None,
        }),
    });
    let v = ExprValue::ListList(
        vec![ExprValue::ListInt(vec![1, 2])],
        ExprType::list(ExprType::INT),
        0,
    );
    assert!(p.check_value_constraints(&v).is_ok());
}

#[test]
fn test_list_list_int_inner_item_not_in_allowed() {
    let mut p = list_list_int_param();
    p.item = Some(ListListIntItemConstraints {
        min_length: None,
        max_length: None,
        item: Some(ListIntItemConstraints {
            allowed_values: Some(vec![FlexInt(1), FlexInt(2)]),
            min_value: None,
            max_value: None,
        }),
    });
    let v = ExprValue::ListList(
        vec![ExprValue::ListInt(vec![3])],
        ExprType::list(ExprType::INT),
        0,
    );
    let err = p.check_value_constraints(&v).unwrap_err();
    assert_eq!(
        err,
        "Parameter 'Matrix': item[0][0] 3 is not in allowed values"
    );
}

// Error reports correct outer and inner indices
#[test]
fn test_list_list_int_error_reports_correct_indices() {
    let mut p = list_list_int_param();
    p.item = Some(ListListIntItemConstraints {
        min_length: None,
        max_length: None,
        item: Some(ListIntItemConstraints {
            allowed_values: None,
            min_value: None,
            max_value: Some(FlexInt(5)),
        }),
    });
    let v = ExprValue::ListList(
        vec![
            ExprValue::ListInt(vec![1, 2]),
            ExprValue::ListInt(vec![3, 99]),
        ],
        ExprType::list(ExprType::INT),
        0,
    );
    let err = p.check_value_constraints(&v).unwrap_err();
    assert_eq!(err, "Parameter 'Matrix': item[1][1] 99 exceeds maximum 5");
}
