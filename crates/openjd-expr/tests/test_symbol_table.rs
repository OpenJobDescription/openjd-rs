#![allow(clippy::approx_constant)]
// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Tests ported from Python test_symbol_table.py

use openjd_expr::value::Float64;
use openjd_expr::{ExprType, ExprValue, ExpressionError, PathFormat, SymbolTable};

// ══════════════════════════════════════════════════════════════
// TestSymbolTable
// ══════════════════════════════════════════════════════════════

#[test]
fn construct_empty() {
    let st = SymbolTable::new();
    assert!(!st.contains("Param"));
}

#[test]
fn construct_from_pairs() {
    let st = SymbolTable::from_pairs(vec![
        ("Param.Frame", ExprValue::Int(42)),
        ("Param.Name", ExprValue::String("test".into())),
    ])
    .unwrap();
    assert!(st.contains("Param"));
    assert_eq!(st.get_value("Param.Frame"), Some(&ExprValue::Int(42)));
    assert_eq!(
        st.get_value("Param.Name"),
        Some(&ExprValue::String("test".into()))
    );
}

#[test]
fn construct_with_path() {
    let st = SymbolTable::from_pairs(vec![(
        "Param.InputFile",
        ExprValue::new_path("/projects/render.exr", PathFormat::Posix),
    )])
    .unwrap();
    assert_eq!(
        st.get_value("Param.InputFile").unwrap().to_display_string(),
        "/projects/render.exr"
    );
}

#[test]
fn construct_nested_subtable() {
    let mut inner = SymbolTable::new();
    inner.set("Frame", ExprValue::Int(100)).unwrap();
    inner
        .set("Name", ExprValue::String("nested".into()))
        .unwrap();
    let mut st = SymbolTable::new();
    st.set_table("Param", inner);
    assert_eq!(st.get_value("Param.Frame"), Some(&ExprValue::Int(100)));
    assert_eq!(
        st.get_value("Param.Name"),
        Some(&ExprValue::String("nested".into()))
    );
}

#[test]
fn construct_from_clone() {
    let original = SymbolTable::from_pairs(vec![("Param.Frame", ExprValue::Int(42))]).unwrap();
    let copy = original.clone();
    assert_eq!(copy.get_value("Param.Frame"), Some(&ExprValue::Int(42)));
}

#[test]
fn set_dotted_path() {
    let mut st = SymbolTable::new();
    st.set("Task.Param.Index", ExprValue::Int(5)).unwrap();
    assert!(st.get_table("Task").is_some());
    assert!(st.get_table("Task").unwrap().get_table("Param").is_some());
    assert_eq!(st.get_value("Task.Param.Index"), Some(&ExprValue::Int(5)));
}

#[test]
fn set_creates_intermediate_tables() {
    let mut st = SymbolTable::new();
    st.set("A.B.C.D", ExprValue::String("deep".into())).unwrap();
    assert!(st.get_table("A").is_some());
    assert!(st.get_table("A").unwrap().get_table("B").is_some());
    assert_eq!(
        st.get_value("A.B.C.D"),
        Some(&ExprValue::String("deep".into()))
    );
}

#[test]
fn set_various_types() {
    let mut st = SymbolTable::new();
    st.set("b", ExprValue::Bool(true)).unwrap();
    st.set("i", ExprValue::Int(42)).unwrap();
    st.set("f", ExprValue::Float(Float64::new(1.2345).unwrap()))
        .unwrap();
    st.set("s", ExprValue::String("hello".into())).unwrap();
    st.set("n", ExprValue::Null).unwrap();
    assert!(matches!(st.get_value("b"), Some(ExprValue::Bool(true))));
    assert!(matches!(st.get_value("i"), Some(ExprValue::Int(42))));
    assert!(matches!(st.get_value("f"), Some(ExprValue::Float(_))));
    assert!(matches!(st.get_value("s"), Some(ExprValue::String(_))));
    assert!(matches!(st.get_value("n"), Some(ExprValue::Null)));
}

#[test]
fn set_expr_value_passthrough() {
    let mut st = SymbolTable::new();
    st.set("Test", ExprValue::Int(999)).unwrap();
    assert_eq!(st.get_value("Test"), Some(&ExprValue::Int(999)));
}

#[test]
fn get_existing() {
    let st = SymbolTable::from_pairs(vec![("Param.X", ExprValue::Int(1))]).unwrap();
    assert!(st.get("Param").is_some());
    assert!(st.get("Missing").is_none());
}

// ══════════════════════════════════════════════════════════════
// TestDottedPathLookup
// ══════════════════════════════════════════════════════════════

#[test]
fn getitem_dotted() {
    let st = SymbolTable::from_pairs(vec![("Param.X", ExprValue::Int(42))]).unwrap();
    assert_eq!(st.get_value("Param.X"), Some(&ExprValue::Int(42)));
}

#[test]
fn getitem_dotted_deep() {
    let st = SymbolTable::from_pairs(vec![("A.B.C", ExprValue::String("hello".into()))]).unwrap();
    assert_eq!(
        st.get_value("A.B.C"),
        Some(&ExprValue::String("hello".into()))
    );
}

#[test]
fn getitem_dotted_missing() {
    let st = SymbolTable::from_pairs(vec![("Param.X", ExprValue::Int(42))]).unwrap();
    assert!(st.get_value("Param.Y").is_none());
}

#[test]
fn contains_dotted() {
    let st = SymbolTable::from_pairs(vec![
        ("Param.X", ExprValue::Int(42)),
        ("Param.Y", ExprValue::String("hi".into())),
    ])
    .unwrap();
    assert!(st.contains("Param.X"));
    assert!(st.contains("Param.Y"));
    assert!(!st.contains("Param.Z"));
}

#[test]
fn get_dotted() {
    let st = SymbolTable::from_pairs(vec![("Param.X", ExprValue::Int(42))]).unwrap();
    assert!(st.get("Param.X").is_some());
    assert!(st.get("Param.Y").is_none());
}

#[test]
fn simple_key_works() {
    let st = SymbolTable::from_pairs(vec![("X", ExprValue::Int(42))]).unwrap();
    assert!(st.contains("X"));
    assert_eq!(st.get_value("X"), Some(&ExprValue::Int(42)));
}

#[test]
fn get_returns_subtable() {
    let st = SymbolTable::from_pairs(vec![
        ("Param.X", ExprValue::Int(42)),
        ("Param.Y", ExprValue::String("hi".into())),
    ])
    .unwrap();
    assert!(st.get_table("Param").is_some());
}

#[test]
fn contains_namespace() {
    let st = SymbolTable::from_pairs(vec![("Param.X", ExprValue::Int(42))]).unwrap();
    assert!(st.contains("Param"));
    assert!(st.contains("Param.X"));
    assert!(!st.contains("Other"));
}

// ══════════════════════════════════════════════════════════════
// Integration: SymbolTable with evaluate_expression
// ══════════════════════════════════════════════════════════════

#[test]
fn eval_with_symtab() {
    let st = SymbolTable::from_pairs(vec![
        ("Param.Frame", ExprValue::Int(42)),
        ("Param.Name", ExprValue::String("test".into())),
    ])
    .unwrap();
    let r = openjd_expr::evaluate_expression("Param.Frame + 1", &st).unwrap();
    assert_eq!(r.to_display_string(), "43");
}

#[test]
fn eval_with_path() {
    let st = SymbolTable::from_pairs(vec![(
        "P",
        ExprValue::new_path("/a/b/file.txt", PathFormat::Posix),
    )])
    .unwrap();
    let parsed = openjd_expr::ParsedExpression::new("P.name").unwrap();
    let symtabs = [&st];
    let r = parsed
        .with_path_format(PathFormat::Posix)
        .evaluate(&symtabs)
        .unwrap();
    assert_eq!(r.to_display_string(), "file.txt");
}

#[test]
fn eval_with_list() {
    let st = SymbolTable::from_pairs(vec![(
        "Items",
        ExprValue::make_list(
            vec![ExprValue::Int(10), ExprValue::Int(20)],
            openjd_expr::ExprType::INT,
        )
        .unwrap(),
    )])
    .unwrap();
    let r = openjd_expr::evaluate_expression("Items[0] + Items[1]", &st).unwrap();
    assert_eq!(r.to_display_string(), "30");
}

// ══════════════════════════════════════════════════════════════
// Generic set() with Into<ExprValue> (B)
// ══════════════════════════════════════════════════════════════

#[test]
fn set_i32() {
    let mut st = SymbolTable::new();
    st.set("X", 42).unwrap();
    assert_eq!(st.get_value("X"), Some(&ExprValue::Int(42)));
}

#[test]
fn set_i64() {
    let mut st = SymbolTable::new();
    st.set("X", 42_i64).unwrap();
    assert_eq!(st.get_value("X"), Some(&ExprValue::Int(42)));
}

#[test]
fn set_f64() {
    let mut st = SymbolTable::new();
    st.set("X", ExprValue::Float(Float64::new(1.2345).unwrap()))
        .unwrap();
    assert!(matches!(st.get_value("X"), Some(ExprValue::Float(_))));
}

#[test]
fn set_bool() {
    let mut st = SymbolTable::new();
    st.set("X", true).unwrap();
    assert_eq!(st.get_value("X"), Some(&ExprValue::Bool(true)));
}

#[test]
fn set_str_ref() {
    let mut st = SymbolTable::new();
    st.set("X", "hello").unwrap();
    assert_eq!(st.get_value("X"), Some(&ExprValue::String("hello".into())));
}

#[test]
fn set_string_owned() {
    let mut st = SymbolTable::new();
    st.set("X", String::from("hello")).unwrap();
    assert_eq!(st.get_value("X"), Some(&ExprValue::String("hello".into())));
}

#[test]
fn set_expr_value_still_works() {
    let mut st = SymbolTable::new();
    st.set("X", ExprValue::Int(42)).unwrap();
    assert_eq!(st.get_value("X"), Some(&ExprValue::Int(42)));
}

#[test]
fn set_dotted_generic() {
    let mut st = SymbolTable::new();
    st.set("Param.Frame", 42).unwrap();
    st.set("Param.Name", "test").unwrap();
    assert_eq!(st.get_value("Param.Frame"), Some(&ExprValue::Int(42)));
    assert_eq!(
        st.get_value("Param.Name"),
        Some(&ExprValue::String("test".into()))
    );
}

// ══════════════════════════════════════════════════════════════
// From<ExprType> auto-wraps as unresolved (C)
// ══════════════════════════════════════════════════════════════

#[test]
fn set_expr_type_becomes_unresolved() {
    let mut st = SymbolTable::new();
    st.set("X", openjd_expr::ExprType::INT).unwrap();
    assert!(matches!(st.get_value("X"), Some(ExprValue::Unresolved(_))));
}

#[test]
fn set_expr_type_path_unresolved() {
    let mut st = SymbolTable::new();
    st.set("Session.Dir", openjd_expr::ExprType::PATH).unwrap();
    let val = st.get_value("Session.Dir").unwrap();
    assert!(val.is_unresolved());
    assert_eq!(val.expr_type().to_string(), "unresolved[path]");
}

// ══════════════════════════════════════════════════════════════
// symtab! macro (D)
// ══════════════════════════════════════════════════════════════

#[test]
fn symtab_macro_basic() {
    let st = openjd_expr::symtab! {
        "Param.Frame" => 42,
        "Param.Name" => "test",
    };
    assert_eq!(st.get_value("Param.Frame"), Some(&ExprValue::Int(42)));
    assert_eq!(
        st.get_value("Param.Name"),
        Some(&ExprValue::String("test".into()))
    );
}

#[test]
fn symtab_macro_mixed_types() {
    let st = openjd_expr::symtab! {
        "i" => 1,
        "f" => ExprValue::Float(Float64::new(2.5).unwrap()),
        "b" => true,
        "s" => "hi",
        "u" => openjd_expr::ExprType::STRING,
    };
    assert!(matches!(st.get_value("i"), Some(ExprValue::Int(1))));
    assert!(matches!(st.get_value("f"), Some(ExprValue::Float(_))));
    assert!(matches!(st.get_value("b"), Some(ExprValue::Bool(true))));
    assert!(matches!(st.get_value("s"), Some(ExprValue::String(_))));
    assert!(matches!(st.get_value("u"), Some(ExprValue::Unresolved(_))));
}

#[test]
fn symtab_macro_empty() {
    let st = openjd_expr::symtab! {};
    assert!(!st.contains("anything"));
}

#[test]
fn symtab_macro_trailing_comma() {
    let st = openjd_expr::symtab! {
        "X" => 1,
    };
    assert_eq!(st.get_value("X"), Some(&ExprValue::Int(1)));
}

#[test]
fn symtab_macro_eval() {
    let st = openjd_expr::symtab! {
        "Param.X" => 10,
        "Param.Y" => 20,
    };
    let r = openjd_expr::evaluate_expression("Param.X + Param.Y", &st).unwrap();
    assert_eq!(r.to_display_string(), "30");
}

// ══════════════════════════════════════════════════════════════
// FromIterator (E)
// ══════════════════════════════════════════════════════════════

#[test]
fn from_iterator() {
    let st: SymbolTable = [
        ("Param.Frame", ExprValue::from(42)),
        ("Param.Name", ExprValue::from("test")),
    ]
    .into_iter()
    .collect();
    assert_eq!(st.get_value("Param.Frame"), Some(&ExprValue::Int(42)));
    assert_eq!(
        st.get_value("Param.Name"),
        Some(&ExprValue::String("test".into()))
    );
}

#[test]
fn from_iterator_empty() {
    let st: SymbolTable = std::iter::empty::<(&str, ExprValue)>().collect();
    assert!(!st.contains("anything"));
}

#[test]
fn from_iterator_vec() {
    let pairs = vec![("A", ExprValue::Int(1)), ("B", ExprValue::Int(2))];
    let st: SymbolTable = pairs.into_iter().collect();
    assert_eq!(st.get_value("A"), Some(&ExprValue::Int(1)));
    assert_eq!(st.get_value("B"), Some(&ExprValue::Int(2)));
}

// ══════════════════════════════════════════════════════════════
// Error cases (SymbolTableError)
// ══════════════════════════════════════════════════════════════

#[test]
fn set_conflict_scalar_then_dotted() {
    // Setting "A.B" as scalar, then "A.B.C" should fail because "A.B" is not a table
    let mut st = SymbolTable::new();
    st.set("A.B", 42).unwrap();
    let err = st.set("A.B.C", "deep").unwrap_err();
    assert!(err.to_string().contains("is not a table"));
}

#[test]
fn set_overwrite_value() {
    let mut st = SymbolTable::new();
    st.set("X", 1).unwrap();
    st.set("X", 2).unwrap();
    assert_eq!(st.get_value("X"), Some(&ExprValue::Int(2)));
}

// ══════════════════════════════════════════════════════════════
// API coverage: keys, all_paths, get_string, set_string
// ══════════════════════════════════════════════════════════════

#[test]
fn keys_returns_top_level() {
    let st = SymbolTable::from_pairs(vec![
        ("Param.X", ExprValue::Int(1)),
        ("Task.Y", ExprValue::Int(2)),
    ])
    .unwrap();
    let mut keys: Vec<&str> = st.keys().collect();
    keys.sort();
    assert_eq!(keys, vec!["Param", "Task"]);
}

#[test]
fn all_paths_collects_leaves() {
    let st = SymbolTable::from_pairs(vec![
        ("Param.X", ExprValue::Int(1)),
        ("Param.Y", ExprValue::Int(2)),
        ("Task.Name", ExprValue::String("t".into())),
    ])
    .unwrap();
    let mut paths = st.all_paths("");
    paths.sort();
    assert_eq!(paths, vec!["Param.X", "Param.Y", "Task.Name"]);
}

#[test]
fn get_string_returns_str() {
    let mut st = SymbolTable::new();
    st.set("X", "hello").unwrap();
    assert_eq!(st.get_string("X"), Some("hello"));
    assert_eq!(st.get_string("Missing"), None);
}

#[test]
fn get_string_returns_path_value() {
    let mut st = SymbolTable::new();
    st.set("P", ExprValue::new_path("/a/b", PathFormat::Posix))
        .unwrap();
    assert_eq!(st.get_string("P"), Some("/a/b"));
}

#[test]
fn set_string_convenience() {
    let mut st = SymbolTable::new();
    st.set_string("Param.Name", "test").unwrap();
    assert_eq!(
        st.get_value("Param.Name"),
        Some(&ExprValue::String("test".into()))
    );
}

#[test]
fn set_value_over_existing_table_errors() {
    let mut st = SymbolTable::new();
    st.set("A.B.C", ExprValue::Int(1)).unwrap();
    // A.B is a table containing C. Setting A.B to a scalar should error.
    let err = st.set("A.B", ExprValue::Int(2)).unwrap_err();
    assert!(
        err.to_string().contains("A.B"),
        "Error should mention the conflicting path: {err}"
    );
}

// ══════════════════════════════════════════════════════════════
// SerializedSymbolTable round-trip
// ══════════════════════════════════════════════════════════════

fn round_trip(symtab: &SymbolTable, path_format: PathFormat) -> SymbolTable {
    let json = serde_json::to_string(symtab).unwrap();
    let serialized: openjd_expr::SerializedSymbolTable = serde_json::from_str(&json).unwrap();
    serialized.to_symtab(path_format).unwrap()
}

#[test]
fn round_trip_int() {
    let mut st = SymbolTable::new();
    st.set("x", ExprValue::Int(42)).unwrap();
    let rt = round_trip(&st, PathFormat::Posix);
    assert_eq!(rt.get_value("x"), Some(&ExprValue::Int(42)));
}

#[test]
fn round_trip_float() {
    let mut st = SymbolTable::new();
    st.set("x", ExprValue::Float(Float64::new(3.14).unwrap()))
        .unwrap();
    let rt = round_trip(&st, PathFormat::Posix);
    assert!(matches!(rt.get_value("x"), Some(ExprValue::Float(f)) if f.0 == 3.14));
}

#[test]
fn round_trip_string() {
    let mut st = SymbolTable::new();
    st.set("x", ExprValue::String("hello".into())).unwrap();
    let rt = round_trip(&st, PathFormat::Posix);
    assert_eq!(rt.get_value("x"), Some(&ExprValue::String("hello".into())));
}

#[test]
fn round_trip_bool() {
    let mut st = SymbolTable::new();
    st.set("x", ExprValue::Bool(true)).unwrap();
    let rt = round_trip(&st, PathFormat::Posix);
    assert_eq!(rt.get_value("x"), Some(&ExprValue::Bool(true)));
}

#[test]
fn round_trip_null() {
    let mut st = SymbolTable::new();
    st.set("x", ExprValue::Null).unwrap();
    let rt = round_trip(&st, PathFormat::Posix);
    assert_eq!(rt.get_value("x"), Some(&ExprValue::Null));
}

#[test]
fn round_trip_path_posix() {
    let mut st = SymbolTable::new();
    st.set("x", ExprValue::new_path("/tmp/out", PathFormat::Posix))
        .unwrap();
    let rt = round_trip(&st, PathFormat::Posix);
    assert_eq!(
        rt.get_value("x"),
        Some(&ExprValue::new_path("/tmp/out", PathFormat::Posix))
    );
}

#[test]
fn round_trip_path_windows() {
    let mut st = SymbolTable::new();
    st.set("x", ExprValue::new_path("C:\\out", PathFormat::Windows))
        .unwrap();
    // Deserialize with a different format — the value is preserved but format comes from the receiver
    let rt = round_trip(&st, PathFormat::Windows);
    assert_eq!(
        rt.get_value("x"),
        Some(&ExprValue::new_path("C:\\out", PathFormat::Windows))
    );
}

#[test]
fn round_trip_path_format_changes_on_deserialize() {
    let mut st = SymbolTable::new();
    st.set("x", ExprValue::new_path("/tmp/out", PathFormat::Posix))
        .unwrap();
    // Deserialize with Windows format — path separators are normalized to the receiver's format
    let rt = round_trip(&st, PathFormat::Windows);
    let v = rt.get_value("x").unwrap();
    assert_eq!(v.to_display_string(), "\\tmp\\out");
    assert_eq!(v.expr_type().to_string(), "path");
}

#[test]
fn round_trip_list_int() {
    let mut st = SymbolTable::new();
    st.set(
        "x",
        ExprValue::make_list(vec![ExprValue::Int(1), ExprValue::Int(2)], ExprType::INT).unwrap(),
    )
    .unwrap();
    let rt = round_trip(&st, PathFormat::Posix);
    let v = rt.get_value("x").unwrap();
    assert_eq!(v.list_len(), Some(2));
}

#[test]
fn round_trip_list_string() {
    let mut st = SymbolTable::new();
    st.set(
        "x",
        ExprValue::make_list(
            vec![ExprValue::from("a"), ExprValue::from("b")],
            ExprType::STRING,
        )
        .unwrap(),
    )
    .unwrap();
    let rt = round_trip(&st, PathFormat::Posix);
    let v = rt.get_value("x").unwrap();
    assert_eq!(v.list_len(), Some(2));
    assert_eq!(v.expr_type().to_string(), "list[string]");
}

#[test]
fn round_trip_list_list() {
    let inner1 =
        ExprValue::make_list(vec![ExprValue::Int(1), ExprValue::Int(2)], ExprType::INT).unwrap();
    let inner2 = ExprValue::make_list(vec![ExprValue::Int(3)], ExprType::INT).unwrap();
    let outer = ExprValue::make_list(vec![inner1, inner2], ExprType::list(ExprType::INT)).unwrap();
    let mut st = SymbolTable::new();
    st.set("x", outer).unwrap();
    let rt = round_trip(&st, PathFormat::Posix);
    let v = rt.get_value("x").unwrap();
    assert_eq!(v.expr_type().to_string(), "list[list[int]]");
    assert_eq!(v.list_len(), Some(2));
}

#[test]
fn round_trip_dotted_paths() {
    let mut st = SymbolTable::new();
    st.set("Param.Frame", ExprValue::Int(42)).unwrap();
    st.set("Param.Name", ExprValue::from("shot_01")).unwrap();
    let rt = round_trip(&st, PathFormat::Posix);
    assert_eq!(rt.get_value("Param.Frame"), Some(&ExprValue::Int(42)));
    assert_eq!(
        rt.get_value("Param.Name"),
        Some(&ExprValue::String("shot_01".into()))
    );
}

#[test]
fn round_trip_skips_unresolved() {
    let mut st = SymbolTable::new();
    st.set("x", ExprValue::Int(1)).unwrap();
    st.set("y", ExprValue::unresolved(ExprType::STRING))
        .unwrap();
    let rt = round_trip(&st, PathFormat::Posix);
    assert_eq!(rt.get_value("x"), Some(&ExprValue::Int(1)));
    // Unresolved values are skipped during serialization
    assert_eq!(rt.get_value("y"), None);
}

#[test]
fn round_trip_empty_list() {
    let mut st = SymbolTable::new();
    st.set("x", ExprValue::make_list(vec![], ExprType::INT).unwrap())
        .unwrap();
    let rt = round_trip(&st, PathFormat::Posix);
    let v = rt.get_value("x").unwrap();
    assert_eq!(v.list_len(), Some(0));
}

// ── Refactor coverage: all_paths return value, from_pairs IntoIterator ──

#[test]
fn all_paths_returns_owned_vec() {
    let st = SymbolTable::from_pairs(vec![
        ("Param.X", ExprValue::Int(1)),
        ("Param.Y", ExprValue::Int(2)),
    ])
    .unwrap();
    // Consume the returned Vec directly (no out-param plumbing).
    let mut paths = st.all_paths("");
    paths.sort();
    assert_eq!(paths, vec!["Param.X", "Param.Y"]);
}

#[test]
fn all_paths_with_prefix() {
    let st = SymbolTable::from_pairs(vec![("A", ExprValue::Int(1))]).unwrap();
    assert_eq!(st.all_paths("Task"), vec!["Task.A"]);
}

#[test]
fn all_paths_empty_table() {
    let st = SymbolTable::new();
    assert!(st.all_paths("").is_empty());
}

#[test]
fn from_pairs_accepts_array() {
    // Arrays implement IntoIterator, so no `vec!` macro needed.
    let st = SymbolTable::from_pairs([("X", ExprValue::Int(1)), ("Y", ExprValue::Int(2))]).unwrap();
    assert_eq!(st.get_value("X"), Some(&ExprValue::Int(1)));
    assert_eq!(st.get_value("Y"), Some(&ExprValue::Int(2)));
}

#[test]
fn from_pairs_accepts_iterator_chain() {
    // Any IntoIterator — e.g., map() — works directly, no collect() needed.
    let st = SymbolTable::from_pairs(
        ["x", "y", "z"]
            .iter()
            .enumerate()
            .map(|(i, &k)| (k, ExprValue::Int(i as i64))),
    )
    .unwrap();
    assert_eq!(st.get_value("x"), Some(&ExprValue::Int(0)));
    assert_eq!(st.get_value("y"), Some(&ExprValue::Int(1)));
    assert_eq!(st.get_value("z"), Some(&ExprValue::Int(2)));
}

#[test]
fn from_pairs_accepts_empty_iter() {
    let st = SymbolTable::from_pairs(std::iter::empty::<(&str, ExprValue)>()).unwrap();
    assert!(st.keys().next().is_none());
}

#[test]
fn symbol_table_error_converts_to_expression_error() {
    // The From<SymbolTableError> for ExpressionError impl preserves the
    // original message verbatim, so no context is lost when bubbling
    // through `?` in evaluator code paths.
    let mut st = SymbolTable::new();
    st.set("A.B", 42).unwrap();
    let sym_err = st.set("A.B.C", "deep").unwrap_err();
    let sym_msg = sym_err.to_string();

    let expr_err: ExpressionError = sym_err.into();
    let expr_msg = expr_err.to_string();
    assert_eq!(expr_msg, sym_msg);
}
