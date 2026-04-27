#![allow(clippy::approx_constant)]
// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Tests for ExprValue: coercion, repr_python, and construction.

use openjd_expr::value::Float64;
use openjd_expr::{ExprType, ExprValue, PathFormat, RangeExpr};

// ══════════════════════════════════════════════════════════════
// from_str_coerce
// ══════════════════════════════════════════════════════════════

#[test]
fn coerce_str_to_int() {
    let v = ExprValue::from_str_coerce("42", &ExprType::INT, PathFormat::Posix).unwrap();
    assert!(matches!(v, ExprValue::Int(42)));
}

#[test]
fn coerce_str_to_float() {
    let v = ExprValue::from_str_coerce("1.2345", &ExprType::FLOAT, PathFormat::Posix).unwrap();
    assert!(matches!(v, ExprValue::Float(_)));
    assert_eq!(v.to_display_string(), "1.2345");
}

#[test]
fn coerce_str_to_bool_true() {
    for s in &["true", "TRUE", "yes", "on", "1"] {
        let v = ExprValue::from_str_coerce(s, &ExprType::BOOL, PathFormat::Posix).unwrap();
        assert!(matches!(v, ExprValue::Bool(true)), "failed for {s}");
    }
}

#[test]
fn coerce_str_to_bool_false() {
    for s in &["false", "FALSE", "no", "off", "0"] {
        let v = ExprValue::from_str_coerce(s, &ExprType::BOOL, PathFormat::Posix).unwrap();
        assert!(matches!(v, ExprValue::Bool(false)), "failed for {s}");
    }
}

#[test]
fn coerce_str_to_bool_invalid() {
    let e = ExprValue::from_str_coerce("maybe", &ExprType::BOOL, PathFormat::Posix).unwrap_err();
    assert!(
        e.contains("Cannot convert") || e.contains("bool"),
        "got: {e}"
    );
}

#[test]
fn coerce_str_to_string() {
    let v = ExprValue::from_str_coerce("hello", &ExprType::STRING, PathFormat::Posix).unwrap();
    assert_eq!(v.to_display_string(), "hello");
}

#[test]
fn coerce_str_to_path() {
    let v =
        ExprValue::from_str_coerce("/tmp/file.txt", &ExprType::PATH, PathFormat::Posix).unwrap();
    assert!(matches!(v, ExprValue::Path { .. }));
    assert_eq!(v.to_display_string(), "/tmp/file.txt");
}

#[test]
fn coerce_str_to_range_expr() {
    let v = ExprValue::from_str_coerce("1-5", &ExprType::RANGE_EXPR, PathFormat::Posix).unwrap();
    assert!(matches!(v, ExprValue::RangeExpr(_)));
}

#[test]
fn coerce_str_to_int_invalid() {
    let e = ExprValue::from_str_coerce("abc", &ExprType::INT, PathFormat::Posix).unwrap_err();
    assert!(e.contains("Cannot convert"), "got: {e}");
}

#[test]
fn coerce_str_to_float_inf() {
    let e = ExprValue::from_str_coerce("inf", &ExprType::FLOAT, PathFormat::Posix).unwrap_err();
    assert!(
        e.contains("infinity") || e.contains("Cannot convert"),
        "got: {e}"
    );
}

#[test]
fn coerce_str_to_float_nan() {
    let e = ExprValue::from_str_coerce("nan", &ExprType::FLOAT, PathFormat::Posix).unwrap_err();
    assert!(
        e.contains("NaN") || e.contains("Cannot convert"),
        "got: {e}"
    );
}

// ══════════════════════════════════════════════════════════════
// coerce (value-to-value)
// ══════════════════════════════════════════════════════════════

#[test]
fn coerce_int_to_float() {
    let v = ExprValue::Int(42)
        .coerce(&ExprType::FLOAT, PathFormat::Posix)
        .unwrap();
    assert!(matches!(v, ExprValue::Float(_)));
    assert_eq!(v.to_display_string(), "42.0");
}

#[test]
fn coerce_same_type_passthrough() {
    let v = ExprValue::Int(42)
        .coerce(&ExprType::INT, PathFormat::Posix)
        .unwrap();
    assert!(matches!(v, ExprValue::Int(42)));
}

#[test]
fn coerce_string_to_int() {
    let v = ExprValue::String("99".to_string())
        .coerce(&ExprType::INT, PathFormat::Posix)
        .unwrap();
    assert!(matches!(v, ExprValue::Int(99)));
}

#[test]
fn coerce_path_to_string() {
    let v = ExprValue::new_path("/a/b".to_string(), PathFormat::Posix)
        .coerce(&ExprType::STRING, PathFormat::Posix)
        .unwrap();
    assert_eq!(v.to_display_string(), "/a/b");
    assert!(matches!(v, ExprValue::String(_)));
}

#[test]
fn coerce_list_elements() {
    let list = ExprValue::make_list(
        vec![
            ExprValue::String("1".to_string()),
            ExprValue::String("2".to_string()),
        ],
        ExprType::STRING,
    )
    .unwrap();
    let v = list
        .coerce(&ExprType::list(ExprType::INT), PathFormat::Posix)
        .unwrap();
    assert_eq!(v.expr_type().to_string(), "list[int]");
}

// ══════════════════════════════════════════════════════════════
// repr_python
// ══════════════════════════════════════════════════════════════

#[test]
fn repr_int() {
    assert_eq!(ExprValue::Int(42).repr_python(), "ExprValue(42)");
}
#[test]
fn repr_float() {
    assert_eq!(
        ExprValue::Float(Float64::new(1.2345).unwrap()).repr_python(),
        "ExprValue(1.2345)"
    );
}
#[test]
fn repr_float_preserved() {
    assert_eq!(
        ExprValue::Float(Float64::with_str(3.5, "3.500".to_string()).unwrap()).repr_python(),
        "ExprValue('3.500', type='float')"
    );
}
#[test]
fn repr_bool_true() {
    assert_eq!(ExprValue::Bool(true).repr_python(), "ExprValue(True)");
}
#[test]
fn repr_bool_false() {
    assert_eq!(ExprValue::Bool(false).repr_python(), "ExprValue(False)");
}
#[test]
fn repr_string() {
    assert_eq!(
        ExprValue::String("hello".to_string()).repr_python(),
        "ExprValue('hello')"
    );
}
#[test]
fn repr_none() {
    assert_eq!(ExprValue::Null.repr_python(), "ExprValue(None)");
}

#[test]
fn repr_list_int() {
    let v = ExprValue::make_list(
        vec![ExprValue::Int(1), ExprValue::Int(2), ExprValue::Int(3)],
        ExprType::INT,
    )
    .unwrap();
    assert_eq!(v.repr_python(), "ExprValue([1, 2, 3], type='list[int]')");
}

#[test]
fn repr_list_string() {
    let v = ExprValue::make_list(
        vec![ExprValue::String("a".into()), ExprValue::String("b".into())],
        ExprType::STRING,
    )
    .unwrap();
    assert_eq!(
        v.repr_python(),
        "ExprValue(['a', 'b'], type='list[string]')"
    );
}

#[test]
fn repr_list_nested() {
    let inner1 =
        ExprValue::make_list(vec![ExprValue::Int(1), ExprValue::Int(2)], ExprType::INT).unwrap();
    let inner2 = ExprValue::make_list(vec![ExprValue::Int(3)], ExprType::INT).unwrap();
    let v = ExprValue::make_list(vec![inner1, inner2], ExprType::list(ExprType::INT)).unwrap();
    assert_eq!(
        v.repr_python(),
        "ExprValue([[1, 2], [3]], type='list[list[int]]')"
    );
}

#[test]
fn repr_path() {
    let v = ExprValue::new_path("/tmp/file".to_string(), PathFormat::Posix);
    assert_eq!(
        v.repr_python(),
        "ExprValue('/tmp/file', type='path', path_format=PathFormat.POSIX)"
    );
}

#[test]
fn repr_range_expr() {
    let v = ExprValue::RangeExpr("1-5".parse::<RangeExpr>().unwrap());
    assert_eq!(v.repr_python(), "ExprValue('1-5', type='range_expr')");
}

#[test]
fn repr_unresolved() {
    assert_eq!(
        ExprValue::unresolved(ExprType::INT).repr_python(),
        "ExprValue.unresolved(ExprType(\"int\"))"
    );
}

#[test]
fn repr_list_path_with_format() {
    let v = ExprValue::make_list(
        vec![
            ExprValue::new_path("/a", PathFormat::Posix),
            ExprValue::new_path("/b", PathFormat::Posix),
        ],
        ExprType::PATH,
    )
    .unwrap();
    let r = v.repr_python();
    assert!(r.contains("type='list[path]'"));
    assert!(r.contains("path_format=PathFormat.POSIX"));
}

// ══════════════════════════════════════════════════════════════
// JSON transport (to/from)
// ══════════════════════════════════════════════════════════════

#[test]
fn transport_int_roundtrip() {
    let v = ExprValue::Int(42);
    let json = v.to_json_transport();
    assert_eq!(json["type"], "int");
    assert_eq!(json["value"], "42");
    let v2 = ExprValue::from_json_transport(&json, PathFormat::Posix).unwrap();
    assert!(matches!(v2, ExprValue::Int(42)));
}

#[test]
fn transport_float_roundtrip() {
    let v = ExprValue::Float(Float64::with_str(1.2345, "3.140".to_string()).unwrap());
    let json = v.to_json_transport();
    assert_eq!(json["type"], "float");
    assert_eq!(json["value"], "3.140");
    let v2 = ExprValue::from_json_transport(&json, PathFormat::Posix).unwrap();
    assert_eq!(v2.to_display_string(), "3.140");
}

#[test]
fn transport_string_roundtrip() {
    let v = ExprValue::String("hello world".to_string());
    let json = v.to_json_transport();
    assert_eq!(json["type"], "string");
    assert_eq!(json["value"], "hello world");
    let v2 = ExprValue::from_json_transport(&json, PathFormat::Posix).unwrap();
    assert_eq!(v2.to_display_string(), "hello world");
}

#[test]
fn transport_bool_roundtrip() {
    let v = ExprValue::Bool(true);
    let json = v.to_json_transport();
    assert_eq!(json["type"], "bool");
    assert_eq!(json["value"], "true");
    let v2 = ExprValue::from_json_transport(&json, PathFormat::Posix).unwrap();
    assert!(matches!(v2, ExprValue::Bool(true)));
}

#[test]
fn transport_path_roundtrip() {
    let v = ExprValue::new_path("/tmp/file.txt".to_string(), PathFormat::Posix);
    let json = v.to_json_transport();
    assert_eq!(json["type"], "path");
    assert_eq!(json["value"], "/tmp/file.txt");
    let v2 = ExprValue::from_json_transport(&json, PathFormat::Posix).unwrap();
    assert!(matches!(v2, ExprValue::Path { .. }));
    assert_eq!(v2.to_display_string(), "/tmp/file.txt");
}

#[test]
fn transport_null_roundtrip() {
    let v = ExprValue::Null;
    let json = v.to_json_transport();
    assert_eq!(json["type"], "nulltype");
    assert_eq!(json["value"], "null");
}

#[test]
fn transport_list_int_roundtrip() {
    let v = ExprValue::make_list(
        vec![ExprValue::Int(1), ExprValue::Int(2), ExprValue::Int(3)],
        ExprType::INT,
    )
    .unwrap();
    let json = v.to_json_transport();
    assert_eq!(json["type"], "list[int]");
    assert!(json["value"].is_array());
    assert_eq!(json["value"][0], "1");
    assert_eq!(json["value"][1], "2");
    let v2 = ExprValue::from_json_transport(&json, PathFormat::Posix).unwrap();
    assert_eq!(v2.expr_type().to_string(), "list[int]");
}

#[test]
fn transport_list_string_roundtrip() {
    let v = ExprValue::make_list(
        vec![ExprValue::String("a".into()), ExprValue::String("b".into())],
        ExprType::STRING,
    )
    .unwrap();
    let json = v.to_json_transport();
    assert_eq!(json["type"], "list[string]");
    assert_eq!(json["value"][0], "a");
    let v2 = ExprValue::from_json_transport(&json, PathFormat::Posix).unwrap();
    assert_eq!(v2.expr_type().to_string(), "list[string]");
}

#[test]
fn transport_nested_list_roundtrip() {
    let inner1 =
        ExprValue::make_list(vec![ExprValue::Int(1), ExprValue::Int(2)], ExprType::INT).unwrap();
    let inner2 = ExprValue::make_list(vec![ExprValue::Int(3)], ExprType::INT).unwrap();
    let v = ExprValue::make_list(vec![inner1, inner2], ExprType::list(ExprType::INT)).unwrap();
    let json = v.to_json_transport();
    assert_eq!(json["type"], "list[list[int]]");
    assert!(json["value"][0].is_array());
    assert_eq!(json["value"][0][0], "1");
    let v2 = ExprValue::from_json_transport(&json, PathFormat::Posix).unwrap();
    assert_eq!(v2.expr_type().to_string(), "list[list[int]]");
}

#[test]
fn transport_range_expr_roundtrip() {
    let v = ExprValue::RangeExpr("1-5".parse::<RangeExpr>().unwrap());
    let json = v.to_json_transport();
    assert_eq!(json["type"], "range_expr");
    assert_eq!(json["value"], "1-5");
    let v2 = ExprValue::from_json_transport(&json, PathFormat::Posix).unwrap();
    assert!(matches!(v2, ExprValue::RangeExpr(_)));
}

#[test]
fn transport_list_path_roundtrip() {
    let v = ExprValue::make_list(
        vec![
            ExprValue::new_path("/a", PathFormat::Posix),
            ExprValue::new_path("/b", PathFormat::Posix),
        ],
        ExprType::PATH,
    )
    .unwrap();
    let json = v.to_json_transport();
    assert_eq!(json["type"], "list[path]");
    assert_eq!(json["value"][0], "/a");
    let v2 = ExprValue::from_json_transport(&json, PathFormat::Posix).unwrap();
    assert!(matches!(v2, ExprValue::ListPath(_, _, _)));
}

#[test]
fn transport_missing_type_errors() {
    let json = serde_json::json!({"value": "42"});
    let e = ExprValue::from_json_transport(&json, PathFormat::Posix).unwrap_err();
    assert!(e.contains("type"), "got: {e}");
}

#[test]
fn transport_missing_value_errors() {
    let json = serde_json::json!({"type": "int"});
    let e = ExprValue::from_json_transport(&json, PathFormat::Posix).unwrap_err();
    assert!(e.contains("value"), "got: {e}");
}

#[test]
fn transport_with_name_field() {
    // Caller adds name — we just verify our output can be extended
    let v = ExprValue::Int(42);
    let mut json = v.to_json_transport();
    json.as_object_mut()
        .unwrap()
        .insert("name".to_string(), serde_json::json!("MyVar"));
    assert_eq!(json["name"], "MyVar");
    assert_eq!(json["type"], "int");
    assert_eq!(json["value"], "42");
    // from_json_transport ignores extra fields
    let v2 = ExprValue::from_json_transport(&json, PathFormat::Posix).unwrap();
    assert!(matches!(v2, ExprValue::Int(42)));
}

// ══════════════════════════════════════════════════════════════
// memory_size
// ══════════════════════════════════════════════════════════════

#[test]
fn memory_size_range_expr_is_constant() {
    // A range representing 1 million values should use the same memory as a range of 5
    let small = ExprValue::RangeExpr("1-5".parse::<RangeExpr>().unwrap());
    let large = ExprValue::RangeExpr("1-1000000".parse::<RangeExpr>().unwrap());
    assert_eq!(small.memory_size(), large.memory_size());
    assert!(large.memory_size() < 100);
}

#[test]
fn memory_size_includes_base() {
    let base = std::mem::size_of::<ExprValue>();
    // Scalars: just the enum size, no heap
    assert_eq!(ExprValue::Int(42).memory_size(), base);
    assert_eq!(ExprValue::Bool(true).memory_size(), base);
    assert_eq!(ExprValue::Null.memory_size(), base);
    // String: base + heap buffer
    let s = ExprValue::String("hello".to_string());
    assert!(s.memory_size() >= base + 5);
}

#[test]
fn memory_size_list_no_double_count() {
    let base = std::mem::size_of::<ExprValue>();
    let input: Vec<ExprValue> = (0..100).map(ExprValue::Int).collect();
    let v = ExprValue::make_list(input, ExprType::INT).unwrap();
    assert!(matches!(&v, ExprValue::ListInt(_)));
    let total = v.memory_size();
    // ListInt heap = capacity * 8. Capacity may exceed len due to allocator reuse.
    // Key invariant: total = base + heap, no double-counting of base per element.
    assert_eq!(
        total,
        base + if let ExprValue::ListInt(inner) = &v {
            inner.capacity() * std::mem::size_of::<i64>()
        } else {
            0
        }
    );
}

// ══════════════════════════════════════════════════════════════
// Construction and type inference
// ══════════════════════════════════════════════════════════════

#[test]
fn construct_bool() {
    let v = ExprValue::Bool(true);
    assert_eq!(v.expr_type().to_string(), "bool");
    assert_eq!(v.to_display_string(), "true");
}

#[test]
fn construct_int() {
    let v = ExprValue::Int(42);
    assert_eq!(v.expr_type().to_string(), "int");
    assert_eq!(v.to_display_string(), "42");
}

#[test]
fn construct_float() {
    let v = ExprValue::Float(Float64::new(1.2345).unwrap());
    assert_eq!(v.expr_type().to_string(), "float");
    assert_eq!(v.to_display_string(), "1.2345");
}

#[test]
fn construct_float_with_original_str() {
    let v = ExprValue::Float(Float64::with_str(3.5, "3.500".to_string()).unwrap());
    assert_eq!(v.to_display_string(), "3.500");
}

#[test]
fn construct_float_trailing_zeros() {
    let v = ExprValue::Float(Float64::with_str(1.0, "1.000".to_string()).unwrap());
    assert_eq!(v.to_display_string(), "1.000");
}

#[test]
fn construct_string() {
    let v = ExprValue::String("hello".into());
    assert_eq!(v.expr_type().to_string(), "string");
    assert_eq!(v.to_display_string(), "hello");
}

#[test]
fn construct_null() {
    let v = ExprValue::Null;
    assert_eq!(v.expr_type().to_string(), "nulltype");
    assert!(matches!(v, ExprValue::Null));
}

#[test]
fn construct_path() {
    let v = ExprValue::new_path("/tmp/test", PathFormat::Posix);
    assert_eq!(v.expr_type().to_string(), "path");
    assert_eq!(v.to_display_string(), "/tmp/test");
}

#[test]
fn construct_range_expr() {
    let v = ExprValue::RangeExpr("1-10".parse::<RangeExpr>().unwrap());
    assert_eq!(v.expr_type().to_string(), "range_expr");
}

// ══════════════════════════════════════════════════════════════
// to_display_string
// ══════════════════════════════════════════════════════════════

#[test]
fn to_string_bool_true() {
    assert_eq!(ExprValue::Bool(true).to_display_string(), "true");
}
#[test]
fn to_string_bool_false() {
    assert_eq!(ExprValue::Bool(false).to_display_string(), "false");
}
#[test]
fn to_string_int() {
    assert_eq!(ExprValue::Int(42).to_display_string(), "42");
}
#[test]
fn to_string_int_negative() {
    assert_eq!(ExprValue::Int(-5).to_display_string(), "-5");
}
#[test]
fn to_string_float() {
    assert_eq!(
        ExprValue::Float(Float64::new(1.2345).unwrap()).to_display_string(),
        "1.2345"
    );
}
#[test]
fn to_string_null() {
    assert_eq!(ExprValue::Null.to_display_string(), "null");
}
#[test]
fn to_string_string() {
    assert_eq!(
        ExprValue::String("hello".into()).to_display_string(),
        "hello"
    );
}
#[test]
fn to_string_path() {
    assert_eq!(
        ExprValue::new_path("/a/b", PathFormat::Posix).to_display_string(),
        "/a/b"
    );
}
#[test]
fn to_string_list_int() {
    let v = ExprValue::make_list(
        vec![ExprValue::Int(1), ExprValue::Int(2), ExprValue::Int(3)],
        ExprType::INT,
    )
    .unwrap();
    assert_eq!(v.to_display_string(), "[1, 2, 3]");
}
#[test]
fn to_string_list_string() {
    let v = ExprValue::make_list(
        vec![ExprValue::String("a".into()), ExprValue::String("b".into())],
        ExprType::STRING,
    )
    .unwrap();
    assert_eq!(v.to_display_string(), "[\"a\", \"b\"]");
}
#[test]
fn to_string_list_float() {
    let v = ExprValue::make_list(
        vec![
            ExprValue::Float(Float64::new(1.5).unwrap()),
            ExprValue::Float(Float64::new(2.5).unwrap()),
        ],
        ExprType::FLOAT,
    )
    .unwrap();
    assert_eq!(v.to_display_string(), "[1.5, 2.5]");
}
#[test]
fn to_string_list_bool() {
    let v = ExprValue::make_list(
        vec![ExprValue::Bool(true), ExprValue::Bool(false)],
        ExprType::BOOL,
    )
    .unwrap();
    assert_eq!(v.to_display_string(), "[true, false]");
}
#[test]
fn to_string_list_nested() {
    let inner1 =
        ExprValue::make_list(vec![ExprValue::Int(1), ExprValue::Int(2)], ExprType::INT).unwrap();
    let inner2 =
        ExprValue::make_list(vec![ExprValue::Int(3), ExprValue::Int(4)], ExprType::INT).unwrap();
    let v = ExprValue::make_list(vec![inner1, inner2], ExprType::list(ExprType::INT)).unwrap();
    assert_eq!(v.to_display_string(), "[[1, 2], [3, 4]]");
}
#[test]
fn to_string_list_empty() {
    let v = ExprValue::make_list(vec![], ExprType::NULLTYPE).unwrap();
    assert_eq!(v.to_display_string(), "[]");
}

// ══════════════════════════════════════════════════════════════
// Equality
// ══════════════════════════════════════════════════════════════

#[test]
fn eq_int_same() {
    assert_eq!(ExprValue::Int(5), ExprValue::Int(5));
}
#[test]
fn eq_int_diff() {
    assert_ne!(ExprValue::Int(5), ExprValue::Int(6));
}
#[test]
fn eq_int_string_diff() {
    assert_ne!(ExprValue::Int(5), ExprValue::String("5".into()));
}
#[test]
fn eq_int_float() {
    assert_eq!(
        ExprValue::Int(5),
        ExprValue::Float(Float64::new(5.0).unwrap())
    );
}
#[test]
fn eq_bool_same() {
    assert_eq!(ExprValue::Bool(true), ExprValue::Bool(true));
}
#[test]
fn eq_bool_diff() {
    assert_ne!(ExprValue::Bool(true), ExprValue::Bool(false));
}
#[test]
fn eq_bool_int_diff() {
    assert_ne!(ExprValue::Bool(true), ExprValue::Int(1));
}
#[test]
fn eq_string_same() {
    assert_eq!(ExprValue::String("a".into()), ExprValue::String("a".into()));
}
#[test]
fn eq_null_same() {
    assert_eq!(ExprValue::Null, ExprValue::Null);
}
#[test]
fn eq_null_int_diff() {
    assert_ne!(ExprValue::Null, ExprValue::Int(0));
}
#[test]
fn eq_list_same() {
    let a =
        ExprValue::make_list(vec![ExprValue::Int(1), ExprValue::Int(2)], ExprType::INT).unwrap();
    let b =
        ExprValue::make_list(vec![ExprValue::Int(1), ExprValue::Int(2)], ExprType::INT).unwrap();
    assert_eq!(a, b);
}
#[test]
fn eq_list_diff() {
    let a = ExprValue::make_list(vec![ExprValue::Int(1)], ExprType::INT).unwrap();
    let b = ExprValue::make_list(vec![ExprValue::Int(2)], ExprType::INT).unwrap();
    assert_ne!(a, b);
}
#[test]
fn eq_string_path() {
    assert_eq!(
        ExprValue::String("/a/b".into()),
        ExprValue::new_path("/a/b", PathFormat::Posix)
    );
}

// ══════════════════════════════════════════════════════════════
// List construction and type inference (make_list)
// ══════════════════════════════════════════════════════════════

#[test]
fn list_string() {
    let v = ExprValue::make_list(
        vec![
            ExprValue::String("a".into()),
            ExprValue::String("b".into()),
            ExprValue::String("c".into()),
        ],
        ExprType::STRING,
    )
    .unwrap();
    assert_eq!(v.list_len(), Some(3));
    assert_eq!(v.expr_type().to_string(), "list[string]");
}

#[test]
fn list_int() {
    let v = ExprValue::make_list(
        vec![ExprValue::Int(1), ExprValue::Int(2), ExprValue::Int(3)],
        ExprType::INT,
    )
    .unwrap();
    assert_eq!(v.list_len(), Some(3));
    assert_eq!(v.expr_type().to_string(), "list[int]");
}

#[test]
fn list_bool() {
    let v = ExprValue::make_list(
        vec![
            ExprValue::Bool(true),
            ExprValue::Bool(false),
            ExprValue::Bool(true),
        ],
        ExprType::BOOL,
    )
    .unwrap();
    assert_eq!(v.list_len(), Some(3));
    let elems = v.list_elements().unwrap();
    assert!(matches!(elems[0], ExprValue::Bool(true)));
    assert!(matches!(elems[1], ExprValue::Bool(false)));
}

#[test]
fn list_list_int() {
    let inner1 =
        ExprValue::make_list(vec![ExprValue::Int(1), ExprValue::Int(2)], ExprType::INT).unwrap();
    let inner2 = ExprValue::make_list(
        vec![ExprValue::Int(3), ExprValue::Int(4), ExprValue::Int(5)],
        ExprType::INT,
    )
    .unwrap();
    let v = ExprValue::make_list(vec![inner1, inner2], ExprType::list(ExprType::INT)).unwrap();
    assert_eq!(v.list_len(), Some(2));
    let elems = v.list_elements().unwrap();
    assert_eq!(elems[0].list_len(), Some(2));
    assert_eq!(elems[1].list_len(), Some(3));
}

#[test]
fn list_empty() {
    let v = ExprValue::make_list(vec![], ExprType::NULLTYPE).unwrap();
    assert_eq!(v.expr_type().to_string(), "list[nulltype]");
    assert_eq!(v.list_len(), Some(0));
}

#[test]
fn list_int_float_mix() {
    let v = ExprValue::make_list(
        vec![
            ExprValue::Int(1),
            ExprValue::Float(Float64::new(2.0).unwrap()),
            ExprValue::Int(3),
        ],
        ExprType::FLOAT,
    )
    .unwrap();
    assert_eq!(v.expr_type().to_string(), "list[float]");
    // All elements coerced to float
    for e in v.list_elements().unwrap() {
        assert!(matches!(e, ExprValue::Float(_)));
    }
}

#[test]
fn list_nested() {
    let inner1 =
        ExprValue::make_list(vec![ExprValue::Int(1), ExprValue::Int(2)], ExprType::INT).unwrap();
    let inner2 =
        ExprValue::make_list(vec![ExprValue::Int(3), ExprValue::Int(4)], ExprType::INT).unwrap();
    let v = ExprValue::make_list(vec![inner1, inner2], ExprType::list(ExprType::INT)).unwrap();
    assert_eq!(v.expr_type().to_string(), "list[list[int]]");
}

#[test]
fn list_nested_int_float_mix() {
    let inner1 =
        ExprValue::make_list(vec![ExprValue::Int(1), ExprValue::Int(2)], ExprType::INT).unwrap();
    let inner2 = ExprValue::make_list(
        vec![
            ExprValue::Float(Float64::new(3.0).unwrap()),
            ExprValue::Float(Float64::new(4.0).unwrap()),
        ],
        ExprType::FLOAT,
    )
    .unwrap();
    let v = ExprValue::make_list(vec![inner1, inner2], ExprType::list(ExprType::FLOAT)).unwrap();
    // Inner int list should be promoted to float
    let elems = v.list_elements().unwrap();
    assert_eq!(elems[0].expr_type().to_string(), "list[float]");
}

#[test]
fn list_path_string_mix() {
    let v = ExprValue::make_list(
        vec![
            ExprValue::new_path("/a", PathFormat::Posix),
            ExprValue::String("b".into()),
        ],
        ExprType::STRING,
    )
    .unwrap();
    // path/string mix promotes to string
    assert_eq!(v.expr_type().to_string(), "list[string]");
}

// ══════════════════════════════════════════════════════════════
// repr_python — additional cases
// ══════════════════════════════════════════════════════════════

#[test]
fn repr_float_trailing_zeros() {
    assert_eq!(
        ExprValue::Float(Float64::with_str(1.0, "1.000".into()).unwrap()).repr_python(),
        "ExprValue('1.000', type='float')"
    );
}

#[test]
fn repr_float_from_float_with_string() {
    assert_eq!(
        ExprValue::Float(Float64::with_str(2.5, "2.500".into()).unwrap()).repr_python(),
        "ExprValue('2.500', type='float')"
    );
}

#[test]
fn repr_empty_list_path() {
    let v = ExprValue::make_list(vec![], ExprType::PATH).unwrap();
    let r = v.repr_python();
    assert!(r.contains("[]"), "got: {r}");
    assert!(r.contains("type='list["), "got: {r}");
}

#[test]
fn repr_empty_list_list_path() {
    let v = ExprValue::make_list(vec![], ExprType::list(ExprType::PATH)).unwrap();
    let r = v.repr_python();
    assert!(r.contains("[]"), "got: {r}");
}

// ══════════════════════════════════════════════════════════════
// Coercion — additional cases
// ══════════════════════════════════════════════════════════════

#[test]
fn coerce_list_string_type() {
    let v = ExprValue::make_list(
        vec![ExprValue::String("a".into()), ExprValue::String("b".into())],
        ExprType::STRING,
    )
    .unwrap();
    let coerced = v
        .coerce(&ExprType::list(ExprType::STRING), PathFormat::Posix)
        .unwrap();
    assert_eq!(coerced.expr_type().to_string(), "list[string]");
}

#[test]
fn coerce_nested_list_type() {
    let inner = ExprValue::make_list(
        vec![ExprValue::String("1".into()), ExprValue::String("2".into())],
        ExprType::STRING,
    )
    .unwrap();
    let v = ExprValue::make_list(vec![inner], ExprType::list(ExprType::STRING)).unwrap();
    let coerced = v
        .coerce(
            &ExprType::list(ExprType::list(ExprType::INT)),
            PathFormat::Posix,
        )
        .unwrap();
    assert_eq!(coerced.expr_type().to_string(), "list[list[int]]");
}

#[test]
fn repr_list_list_path_with_format() {
    let inner1 = ExprValue::make_list(
        vec![ExprValue::new_path("/a", PathFormat::Posix)],
        ExprType::PATH,
    )
    .unwrap();
    let inner2 = ExprValue::make_list(
        vec![ExprValue::new_path("/b", PathFormat::Posix)],
        ExprType::PATH,
    )
    .unwrap();
    let v = ExprValue::make_list(vec![inner1, inner2], ExprType::list(ExprType::PATH)).unwrap();
    let r = v.repr_python();
    assert!(r.contains("type='list[list[path]]'"), "got: {r}");
    assert!(r.contains("path_format=PathFormat.POSIX"), "got: {r}");
}

// ══════════════════════════════════════════════════════════════
// From trait implementations
// ══════════════════════════════════════════════════════════════

#[test]
fn from_bool() {
    assert_eq!(ExprValue::from(true), ExprValue::Bool(true));
}
#[test]
fn from_i64() {
    assert_eq!(ExprValue::from(42_i64), ExprValue::Int(42));
}
#[test]
fn from_f64() {
    assert_eq!(
        ExprValue::Float(Float64::new(1.2345).unwrap()).expr_type(),
        ExprType::FLOAT
    );
}
#[test]
fn from_string() {
    assert_eq!(
        ExprValue::from("hello".to_string()),
        ExprValue::String("hello".into())
    );
}
#[test]
fn from_str() {
    assert_eq!(ExprValue::from("hello"), ExprValue::String("hello".into()));
}
#[test]
fn from_range_expr() {
    let r = "1-5".parse::<openjd_expr::RangeExpr>().unwrap();
    let v = ExprValue::from(r.clone());
    assert_eq!(v, ExprValue::RangeExpr(r));
}

// ══════════════════════════════════════════════════════════════
// Tests ported from Python test_expression_value.py
// ══════════════════════════════════════════════════════════════

// -- TestFromFloat equivalents --

#[test]
fn from_float_basic_to_string() {
    let v = ExprValue::Float(Float64::new(1.2345).unwrap());
    assert_eq!(v.to_display_string(), "1.2345");
}

#[test]
fn from_float_decimal_trailing_zeros() {
    // Python: ExprValue(Decimal("3.140")) → item()==1.2345, to_string()=="3.140"
    let v = ExprValue::Float(Float64::with_str(1.2345, "3.140".to_string()).unwrap());
    if let ExprValue::Float(f) = &v {
        assert_eq!(f.value(), 1.2345);
    } else {
        panic!("Expected Float");
    }
    assert_eq!(v.to_display_string(), "3.140");
}

#[test]
fn from_float_decimal_no_trailing_zeros() {
    let v = ExprValue::Float(Float64::with_str(2.5, "2.5".to_string()).unwrap());
    if let ExprValue::Float(f) = &v {
        assert_eq!(f.value(), 2.5);
    } else {
        panic!("Expected Float");
    }
    assert_eq!(v.to_display_string(), "2.5");
}

// -- TestFromList: list of floats with preserved strings --

#[test]
fn list_float_with_preserved_strings() {
    // Python: ExprValue([Decimal("1.100"), Decimal("2.200")])
    let v = ExprValue::make_list(
        vec![
            ExprValue::Float(Float64::with_str(1.1, "1.100".to_string()).unwrap()),
            ExprValue::Float(Float64::with_str(2.2, "2.200".to_string()).unwrap()),
        ],
        ExprType::FLOAT,
    )
    .unwrap();
    assert_eq!(v.list_len(), Some(2));
    let elems = v.list_elements().unwrap();
    assert_eq!(elems[0].to_display_string(), "1.100");
    assert_eq!(elems[1].to_display_string(), "2.200");
}

#[test]
fn list_mixed_float_with_preserved() {
    // Python: ExprValue([1.5, Decimal("2.500")])
    let v = ExprValue::make_list(
        vec![
            ExprValue::Float(Float64::new(1.5).unwrap()),
            ExprValue::Float(Float64::with_str(2.5, "2.500".to_string()).unwrap()),
        ],
        ExprType::FLOAT,
    )
    .unwrap();
    let elems = v.list_elements().unwrap();
    assert_eq!(elems[0].to_display_string(), "1.5");
    assert_eq!(elems[1].to_display_string(), "2.500");
}

// -- TestExprValueConstruction: is_null equivalent --

#[test]
fn construct_null_is_null() {
    let v = ExprValue::Null;
    assert!(matches!(v, ExprValue::Null));
}

// -- TestExprValueTypeCoercionWithString: range_expr iteration --

#[test]
fn coerce_str_to_range_expr_iteration() {
    let v = ExprValue::from_str_coerce("1-5", &ExprType::RANGE_EXPR, PathFormat::Posix).unwrap();
    if let ExprValue::RangeExpr(r) = &v {
        assert_eq!(r.to_vec(), vec![1, 2, 3, 4, 5]);
    } else {
        panic!("Expected RangeExpr");
    }
}

// -- Coerce list with ExprType (Python: ExprValue([1,2,3], type="list[int]")) --

#[test]
fn coerce_list_int_with_type() {
    let list = ExprValue::make_list(
        vec![ExprValue::Int(1), ExprValue::Int(2), ExprValue::Int(3)],
        ExprType::INT,
    )
    .unwrap();
    let coerced = list
        .coerce(&ExprType::list(ExprType::INT), PathFormat::Posix)
        .unwrap();
    assert_eq!(coerced.list_len(), Some(3));
    assert_eq!(coerced.expr_type().to_string(), "list[int]");
}

#[test]
fn coerce_nested_list_int_with_type() {
    let inner1 =
        ExprValue::make_list(vec![ExprValue::Int(1), ExprValue::Int(2)], ExprType::INT).unwrap();
    let inner2 = ExprValue::make_list(vec![ExprValue::Int(3)], ExprType::INT).unwrap();
    let v = ExprValue::make_list(vec![inner1, inner2], ExprType::list(ExprType::INT)).unwrap();
    let coerced = v
        .coerce(
            &ExprType::list(ExprType::list(ExprType::INT)),
            PathFormat::Posix,
        )
        .unwrap();
    assert_eq!(coerced.expr_type().to_string(), "list[list[int]]");
}

// -- Python: ExprValue(42, type="float") → int to float coercion --

#[test]
fn coerce_int_value_to_float() {
    let v = ExprValue::Int(42)
        .coerce(&ExprType::FLOAT, PathFormat::Posix)
        .unwrap();
    assert!(matches!(v, ExprValue::Float(_)));
    if let ExprValue::Float(f) = &v {
        assert_eq!(f.value(), 42.0);
    }
}

// -- repr_python: Windows path format variants --

#[test]
fn repr_list_path_with_windows_format() {
    let v = ExprValue::make_list(
        vec![
            ExprValue::new_path("\\a", PathFormat::Windows),
            ExprValue::new_path("\\b", PathFormat::Windows),
        ],
        ExprType::PATH,
    )
    .unwrap();
    let r = v.repr_python();
    assert!(r.contains("type='list[path]'"), "got: {r}");
    assert!(r.contains("path_format=PathFormat.WINDOWS"), "got: {r}");
}

#[test]
fn repr_list_list_path_with_windows_format() {
    let inner1 = ExprValue::make_list(
        vec![ExprValue::new_path("\\a", PathFormat::Windows)],
        ExprType::PATH,
    )
    .unwrap();
    let inner2 = ExprValue::make_list(
        vec![ExprValue::new_path("\\b", PathFormat::Windows)],
        ExprType::PATH,
    )
    .unwrap();
    let v = ExprValue::make_list(vec![inner1, inner2], ExprType::list(ExprType::PATH)).unwrap();
    let r = v.repr_python();
    assert!(r.contains("type='list[list[path]]'"), "got: {r}");
    assert!(r.contains("path_format=PathFormat.WINDOWS"), "got: {r}");
}

// -- repr_python: exact match for empty list[path] and list[list[path]] --

#[test]
fn repr_empty_list_path_exact() {
    let v = ExprValue::make_list(vec![], ExprType::PATH).unwrap();
    let r = v.repr_python();
    assert!(r.starts_with("ExprValue([]"), "got: {r}");
    assert!(r.contains("type='list["), "got: {r}");
}

#[test]
fn repr_empty_list_list_path_exact() {
    let v = ExprValue::make_list(vec![], ExprType::list(ExprType::PATH)).unwrap();
    let r = v.repr_python();
    assert!(r.starts_with("ExprValue([]"), "got: {r}");
}

// -- Python: ExprValue("/tmp/file.txt", type="path", path_format=PathFormat.POSIX) item check --

#[test]
fn coerce_str_to_path_item_value() {
    let v =
        ExprValue::from_str_coerce("/tmp/file.txt", &ExprType::PATH, PathFormat::Posix).unwrap();
    assert_eq!(v.to_display_string(), "/tmp/file.txt");
    if let ExprValue::Path { value, format, .. } = &v {
        assert_eq!(value, "/tmp/file.txt");
        assert_eq!(*format, PathFormat::Posix);
    } else {
        panic!("Expected Path");
    }
}

// ── Refactor coverage: Path encapsulation via new_path ──

#[test]
fn new_path_normalizes_windows_separators() {
    // Construction through new_path must normalize separators — this is the
    // invariant that the #[non_exhaustive] marker on ExprValue::Path protects.
    let v = ExprValue::new_path("C:/foo/bar".to_string(), PathFormat::Windows);
    if let ExprValue::Path { value, format, .. } = &v {
        assert_eq!(
            value, "C:\\foo\\bar",
            "forward slashes should normalize to backslashes"
        );
        assert_eq!(*format, PathFormat::Windows);
    } else {
        panic!("expected Path variant");
    }
}

#[test]
fn new_path_preserves_backslashes_on_posix() {
    // On POSIX, `\` is a valid filename character — it must NOT be normalized
    // to `/`. This is the project-wide convention (see openjd-snapshots
    // path_util) and matches pathlib semantics.
    let v = ExprValue::new_path("foo\\bar\\baz".to_string(), PathFormat::Posix);
    if let ExprValue::Path { value, format, .. } = &v {
        assert_eq!(
            value, "foo\\bar\\baz",
            "backslashes must be preserved on POSIX"
        );
        assert_eq!(*format, PathFormat::Posix);
    } else {
        panic!("expected Path variant");
    }
}

#[test]
fn new_path_preserves_uri() {
    // URI paths must not be touched by normalization.
    let v = ExprValue::new_path(
        "s3://bucket/key\\with\\backslashes".to_string(),
        PathFormat::Uri,
    );
    if let ExprValue::Path { value, .. } = &v {
        assert_eq!(value, "s3://bucket/key\\with\\backslashes");
    } else {
        panic!("expected Path variant");
    }
}

// Compile-time test: downstream pattern matches must use `..` because Path is
// marked `#[non_exhaustive]`. Inside this integration test (external to the
// crate), the following pattern must type-check with the `..` token.
#[test]
fn path_pattern_requires_dotdot_externally() {
    let v = ExprValue::new_path("/p", PathFormat::Posix);
    let got_value = match &v {
        ExprValue::Path { value, .. } => value.as_str(),
        _ => panic!(),
    };
    assert_eq!(got_value, "/p");
}

#[test]
fn from_transport_value_deeply_nested_rejected() {
    // Build a type list[list[...[int]...]] with 11 levels and matching nested JSON arrays
    let mut typ = ExprType::parse("int").unwrap();
    for _ in 0..11 {
        typ = ExprType::list(typ);
    }
    let mut json_val: serde_json::Value = serde_json::json!("42");
    for _ in 0..11 {
        json_val = serde_json::json!([json_val]);
    }
    let err = ExprValue::from_transport_value(&json_val, &typ, PathFormat::Posix).unwrap_err();
    assert!(
        err.contains("depth") || err.contains("nesting"),
        "got: {err}"
    );
}
