// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Tests for the lazy `StepParameterSpaceIterator`.

use crate::job::{ResolvedChunks, StepParameterSpace, TaskParamRange, TaskParameter};
use crate::step_param_space::StepParameterSpaceIterator;
use crate::template::RangeConstraint;
use crate::types::{TaskParameterSet, TaskParameterType, TaskParameterValue};
use openjd_expr::value::Float64;
use openjd_expr::{ExprValue, RangeExpr};

fn make_space(params: Vec<(&str, TaskParameter)>, combination: Option<&str>) -> StepParameterSpace {
    StepParameterSpace {
        task_parameter_definitions: params
            .into_iter()
            .map(|(n, p)| (n.to_string(), p))
            .collect(),
        combination: combination.map(|s| s.to_string()),
    }
}

fn int_list(vals: &[i64]) -> TaskParameter {
    TaskParameter::Int {
        range: TaskParamRange::List(vals.to_vec()),
        chunks: None,
    }
}

fn float_list(vals: &[f64]) -> TaskParameter {
    TaskParameter::Float {
        range: vals.iter().map(|v| Float64::new(*v).unwrap()).collect(),
    }
}

fn string_list(vals: &[&str]) -> TaskParameter {
    TaskParameter::String {
        range: vals.iter().map(|s| s.to_string()).collect(),
    }
}

fn int_range_expr(expr: &str) -> TaskParameter {
    TaskParameter::Int {
        range: TaskParamRange::RangeExpr(expr.parse::<RangeExpr>().unwrap()),
        chunks: None,
    }
}

fn get_int(set: &TaskParameterSet, name: &str) -> i64 {
    match &set[name].value {
        ExprValue::Int(i) => *i,
        other => panic!("expected Int for {name}, got {other:?}"),
    }
}

fn get_float(set: &TaskParameterSet, name: &str) -> f64 {
    match &set[name].value {
        ExprValue::Float(f) => f.value(),
        other => panic!("expected Float for {name}, got {other:?}"),
    }
}

fn get_string(set: &TaskParameterSet, name: &str) -> String {
    match &set[name].value {
        ExprValue::String(s) => s.clone(),
        other => panic!("expected String for {name}, got {other:?}"),
    }
}

// ── Basic functionality ──

#[test]
fn test_empty_space() {
    let space = make_space(vec![], None);
    let iter = StepParameterSpaceIterator::new(&space).unwrap();
    assert_eq!(iter.len(), 1);
    let set = iter.get(0).unwrap();
    assert!(set.is_empty());
}

#[test]
fn test_single_int_list() {
    let space = make_space(vec![("A", int_list(&[1, 2, 3]))], None);
    let iter = StepParameterSpaceIterator::new(&space).unwrap();
    assert_eq!(iter.len(), 3);
    let vals: Vec<i64> = (0..3)
        .map(|i| get_int(&iter.get(i).unwrap(), "A"))
        .collect();
    assert_eq!(vals, vec![1, 2, 3]);
}

#[test]
fn test_single_range_expr() {
    let space = make_space(vec![("A", int_range_expr("1-1000000"))], None);
    let iter = StepParameterSpaceIterator::new(&space).unwrap();
    // Verify len without collecting
    assert_eq!(iter.len(), 1_000_000);
    assert_eq!(get_int(&iter.get(0).unwrap(), "A"), 1);
    assert_eq!(get_int(&iter.get(999_999).unwrap(), "A"), 1_000_000);
}

#[test]
fn test_single_float_list() {
    let space = make_space(vec![("F", float_list(&[1.5, 2.5]))], None);
    let iter = StepParameterSpaceIterator::new(&space).unwrap();
    assert_eq!(iter.len(), 2);
    assert!((get_float(&iter.get(0).unwrap(), "F") - 1.5).abs() < f64::EPSILON);
    assert!((get_float(&iter.get(1).unwrap(), "F") - 2.5).abs() < f64::EPSILON);
}

#[test]
fn test_single_string_list() {
    let space = make_space(vec![("S", string_list(&["a", "b", "c"]))], None);
    let iter = StepParameterSpaceIterator::new(&space).unwrap();
    assert_eq!(iter.len(), 3);
    let vals: Vec<String> = (0..3)
        .map(|i| get_string(&iter.get(i).unwrap(), "S"))
        .collect();
    assert_eq!(vals, vec!["a", "b", "c"]);
}

// ── Combination expressions ──

#[test]
fn test_product_two_params() {
    let space = make_space(
        vec![
            ("A", int_list(&[1, 2])),
            ("B", string_list(&["x", "y", "z"])),
        ],
        Some("A * B"),
    );
    let iter = StepParameterSpaceIterator::new(&space).unwrap();
    assert_eq!(iter.len(), 6);
    let first = iter.get(0).unwrap();
    assert_eq!(get_int(&first, "A"), 1);
    assert_eq!(get_string(&first, "B"), "x");
    let last = iter.get(5).unwrap();
    assert_eq!(get_int(&last, "A"), 2);
    assert_eq!(get_string(&last, "B"), "z");
}

#[test]
fn test_association_two_params() {
    let space = make_space(
        vec![
            ("A", int_list(&[1, 2, 3])),
            ("B", string_list(&["x", "y", "z"])),
        ],
        Some("(A, B)"),
    );
    let iter = StepParameterSpaceIterator::new(&space).unwrap();
    assert_eq!(iter.len(), 3);
    for i in 0..3 {
        let set = iter.get(i).unwrap();
        assert_eq!(get_int(&set, "A"), (i + 1) as i64);
        assert_eq!(get_string(&set, "B"), ["x", "y", "z"][i]);
    }
}

#[test]
fn test_association_length_mismatch() {
    let space = make_space(
        vec![
            ("A", int_list(&[1, 2])),
            ("B", string_list(&["x", "y", "z"])),
        ],
        Some("(A, B)"),
    );
    let result = StepParameterSpaceIterator::new(&space);
    let msg = match result {
        Err(e) => e.to_string(),
        Ok(_) => panic!("expected error for mismatched association lengths"),
    };
    assert!(
        msg.contains("same number of values"),
        "Expected association mismatch error, got: {msg}"
    );
}

#[test]
fn test_product_association_mixed() {
    let space = make_space(
        vec![
            ("A", int_list(&[1, 2])),
            ("B", string_list(&["x", "y"])),
            ("C", int_list(&[10, 20])),
        ],
        Some("A * (B, C)"),
    );
    let iter = StepParameterSpaceIterator::new(&space).unwrap();
    assert_eq!(iter.len(), 4); // 2 * 2
}

// ── Random access ──

#[test]
fn test_get_matches_iteration() {
    let space = make_space(
        vec![
            ("A", int_list(&[1, 2])),
            ("B", string_list(&["x", "y", "z"])),
        ],
        Some("A * B"),
    );
    let iter = StepParameterSpaceIterator::new(&space).unwrap();
    let collected: Vec<_> = iter.collect();
    let iter2 = StepParameterSpaceIterator::new(&space).unwrap();
    for (i, set) in collected.iter().enumerate() {
        let got = iter2.get(i).unwrap();
        assert_eq!(set.len(), got.len());
        for (k, v) in set {
            let gv = &got[k];
            assert_eq!(v.param_type, gv.param_type);
            match (&v.value, &gv.value) {
                (ExprValue::Int(a), ExprValue::Int(b)) => assert_eq!(a, b),
                (ExprValue::String(a), ExprValue::String(b)) => assert_eq!(a, b),
                (ExprValue::Float(a), ExprValue::Float(b)) => {
                    assert!((a.value() - b.value()).abs() < f64::EPSILON)
                }
                _ => panic!("mismatched value types"),
            }
        }
    }
}

// ── Random access with complex combinations ──

#[test]
fn test_get_product_three_params() {
    // Product of 3 params: A(2) * B(3) * C(2) = 12 elements
    let space = make_space(
        vec![
            ("A", int_list(&[1, 2])),
            ("B", string_list(&["a", "b", "c"])),
            ("C", int_list(&[-1, -2])),
        ],
        Some("A * B * C"),
    );
    let iter = StepParameterSpaceIterator::new(&space).unwrap();
    assert_eq!(iter.len(), 12);

    // Verify get() matches iteration order
    let iter2 = StepParameterSpaceIterator::new(&space).unwrap();
    let collected: Vec<_> = iter2.collect();
    for (i, expected) in collected.iter().enumerate() {
        let got = iter.get(i).unwrap();
        assert_eq!(get_int(&got, "A"), get_int(expected, "A"), "index {i} A");
        assert_eq!(
            get_string(&got, "B"),
            get_string(expected, "B"),
            "index {i} B"
        );
        assert_eq!(get_int(&got, "C"), get_int(expected, "C"), "index {i} C");
    }
    assert!(iter.get(12).is_none());
}

#[test]
fn test_get_association_three_params() {
    // Association of 3 params: (A, B, C) with 4 elements each
    let space = make_space(
        vec![
            ("A", int_list(&[1, 2, 3, 4])),
            ("B", string_list(&["a", "b", "c", "d"])),
            ("C", int_list(&[-1, -2, -3, -4])),
        ],
        Some("(A, B, C)"),
    );
    let iter = StepParameterSpaceIterator::new(&space).unwrap();
    assert_eq!(iter.len(), 4);

    for i in 0..4 {
        let got = iter.get(i).unwrap();
        assert_eq!(get_int(&got, "A"), (i + 1) as i64);
        assert_eq!(get_string(&got, "B"), ["a", "b", "c", "d"][i]);
        assert_eq!(get_int(&got, "C"), -(i as i64 + 1));
    }
    assert!(iter.get(4).is_none());
}

#[test]
fn test_get_nested_product_association() {
    // A * (B, C * D): A=[1,2], B=["a","b"], C=[10,11], D=[20,21]
    // (B, C*D) is association of len 2 (B has 2, C*D has 2*2=4... wait, association needs same len)
    // Let's use: A=[1,2], B=["a","b","c","d"], C=[10,11], D=[20,21]
    // C*D = 4 elements, B has 4 elements, so (B, C*D) = 4 elements
    // A * (B, C*D) = 2 * 4 = 8
    let space = make_space(
        vec![
            ("A", int_list(&[1, 2])),
            ("B", string_list(&["a", "b", "c", "d"])),
            ("C", int_range_expr("10-11")),
            ("D", int_list(&[20, 21])),
        ],
        Some("A * (B, C * D)"),
    );
    let iter = StepParameterSpaceIterator::new(&space).unwrap();
    assert_eq!(iter.len(), 8);

    // Verify get() matches iteration
    let iter2 = StepParameterSpaceIterator::new(&space).unwrap();
    let collected: Vec<_> = iter2.collect();
    for (i, expected) in collected.iter().enumerate() {
        let got = iter.get(i).unwrap();
        assert_eq!(get_int(&got, "A"), get_int(expected, "A"), "index {i} A");
        assert_eq!(
            get_string(&got, "B"),
            get_string(expected, "B"),
            "index {i} B"
        );
        assert_eq!(get_int(&got, "C"), get_int(expected, "C"), "index {i} C");
        assert_eq!(get_int(&got, "D"), get_int(expected, "D"), "index {i} D");
    }
    assert!(iter.get(8).is_none());
}

#[test]
fn test_get_product_with_range_expr() {
    // Product with range expression: A(range 1-5) * B(["x","y"]) = 10 elements
    let space = make_space(
        vec![
            ("A", int_range_expr("1-5")),
            ("B", string_list(&["x", "y"])),
        ],
        Some("A * B"),
    );
    let iter = StepParameterSpaceIterator::new(&space).unwrap();
    assert_eq!(iter.len(), 10);

    // Spot check specific indices
    let e0 = iter.get(0).unwrap();
    assert_eq!(get_int(&e0, "A"), 1);
    assert_eq!(get_string(&e0, "B"), "x");

    let e1 = iter.get(1).unwrap();
    assert_eq!(get_int(&e1, "A"), 1);
    assert_eq!(get_string(&e1, "B"), "y");

    let e9 = iter.get(9).unwrap();
    assert_eq!(get_int(&e9, "A"), 5);
    assert_eq!(get_string(&e9, "B"), "y");

    assert!(iter.get(10).is_none());
}

// ── Containment ──

#[test]
fn test_contains_positive() {
    let space = make_space(vec![("A", int_list(&[1, 2, 3]))], None);
    let iter = StepParameterSpaceIterator::new(&space).unwrap();
    let mut needle = TaskParameterSet::new();
    needle.insert(
        "A".to_string(),
        TaskParameterValue {
            param_type: TaskParameterType::Int,
            value: ExprValue::Int(2),
        },
    );
    assert!(iter.contains(&needle));
}

#[test]
fn test_contains_negative() {
    let space = make_space(vec![("A", int_list(&[1, 2, 3]))], None);
    let iter = StepParameterSpaceIterator::new(&space).unwrap();
    let mut needle = TaskParameterSet::new();
    needle.insert(
        "A".to_string(),
        TaskParameterValue {
            param_type: TaskParameterType::Int,
            value: ExprValue::Int(99),
        },
    );
    assert!(!iter.contains(&needle));
}

// ── Lazy behavior ──

#[test]
fn test_large_range_expr_no_oom() {
    let space = make_space(vec![("A", int_range_expr("1-10000000"))], None);
    let iter = StepParameterSpaceIterator::new(&space).unwrap();
    assert_eq!(iter.len(), 10_000_000);
    assert_eq!(get_int(&iter.get(5_000_000).unwrap(), "A"), 5_000_001);
}

// ── Static chunking ──

#[test]
fn test_chunk_int_static() {
    let space = make_space(
        vec![(
            "C",
            TaskParameter::ChunkInt {
                range: TaskParamRange::RangeExpr("1-10".parse::<RangeExpr>().unwrap()),
                chunks: ResolvedChunks {
                    default_task_count: 3,
                    target_runtime_seconds: None,
                    range_constraint: RangeConstraint::Contiguous,
                },
            },
        )],
        None,
    );
    let iter = StepParameterSpaceIterator::new(&space).unwrap();
    // 10 items / 3 per task → ceil(10/3) = 4 chunks
    assert_eq!(iter.len(), 4);
    // Collect all chunks via iteration
    let chunks: Vec<_> = iter.collect();
    assert_eq!(chunks.len(), 4);
    for set in &chunks {
        assert_eq!(set["C"].param_type, TaskParameterType::ChunkInt);
        match &set["C"].value {
            ExprValue::RangeExpr(_) => {}
            other => panic!("expected RangeExpr, got {other:?}"),
        }
    }
    // Verify the chunks cover all 10 values
    let mut all_vals: Vec<i64> = Vec::new();
    for set in &chunks {
        if let ExprValue::RangeExpr(r) = &set["C"].value {
            all_vals.extend(r.iter());
        }
    }
    all_vals.sort();
    assert_eq!(all_vals, (1..=10).collect::<Vec<_>>());
}

#[test]
fn test_truly_lazy_trillion_element_space() {
    // Two parameters with 1M values each → 10^12 element product space.
    // If anything materializes eagerly, this will OOM or timeout.
    use std::time::Instant;

    let space = make_space(
        vec![
            (
                "Frame",
                TaskParameter::Int {
                    range: TaskParamRange::RangeExpr("1-1000000".parse::<RangeExpr>().unwrap()),
                    chunks: None,
                },
            ),
            (
                "Tile",
                TaskParameter::Int {
                    range: TaskParamRange::RangeExpr("1-2000000:2".parse::<RangeExpr>().unwrap()),
                    chunks: None,
                },
            ),
        ],
        Some("Frame * Tile"),
    );

    let start = Instant::now();
    let mut iter = StepParameterSpaceIterator::new(&space).unwrap();

    // len() should return 10^12 instantly (pure multiplication, no expansion)
    assert_eq!(iter.len(), 1_000_000 * 1_000_000);

    // First element: both parameters at their first value
    let first = iter.next().unwrap();
    assert_eq!(first["Frame"].value, ExprValue::Int(1));
    assert_eq!(first["Tile"].value, ExprValue::Int(1));
    drop(iter);

    // Random access: verify specific elements are computable instantly
    let iter2 = StepParameterSpaceIterator::new(&space).unwrap();

    // Element 0: first of both
    let e0 = iter2.get(0).unwrap();
    assert_eq!(e0["Frame"].value, ExprValue::Int(1));
    assert_eq!(e0["Tile"].value, ExprValue::Int(1));

    // Last element
    let last = iter2.get(iter2.len() - 1).unwrap();
    assert_eq!(last["Frame"].value, ExprValue::Int(1_000_000));
    assert_eq!(last["Tile"].value, ExprValue::Int(1_999_999)); // last value of 1-2000000:2

    let elapsed = start.elapsed();
    assert!(
        elapsed.as_millis() < 100,
        "Should complete in <100ms, took {}ms",
        elapsed.as_millis()
    );
}

// (end of tests)

#[test]
fn test_lazy_association_with_large_product() {
    // (A * B * C, D) where A, B, C are 1000-element string lists and D is a billion-element RangeExpr.
    // A*B*C = 10^9 elements. D must also be 10^9 for the association to match.
    // Total space = 10^9 (association zips, doesn't multiply).
    // The product A*B*C uses divmod over three RangeListNodes — no materialization of the 10^9 product.
    // D uses RangeExprNode — no materialization of the billion integers.
    use std::time::Instant;

    let a_vals: Vec<String> = (0..1000).map(|i| format!("a{i}")).collect();
    let b_vals: Vec<String> = (0..1000).map(|i| format!("b{i}")).collect();
    let c_vals: Vec<String> = (0..1000).map(|i| format!("c{i}")).collect();

    let space = make_space(
        vec![
            ("A", TaskParameter::String { range: a_vals }),
            ("B", TaskParameter::String { range: b_vals }),
            ("C", TaskParameter::String { range: c_vals }),
            (
                "D",
                TaskParameter::Int {
                    range: TaskParamRange::RangeExpr("1-1000000000".parse::<RangeExpr>().unwrap()),
                    chunks: None,
                },
            ),
        ],
        Some("(A * B * C, D)"),
    );

    let start = Instant::now();
    let mut iter = StepParameterSpaceIterator::new(&space).unwrap();

    // Association: len = len(A*B*C) = len(D) = 10^9
    assert_eq!(iter.len(), 1_000_000_000);

    // First element: A[0], B[0], C[0], D[0]
    let first = iter.next().unwrap();
    assert_eq!(first["A"].value, ExprValue::String("a0".to_string()));
    assert_eq!(first["B"].value, ExprValue::String("b0".to_string()));
    assert_eq!(first["C"].value, ExprValue::String("c0".to_string()));
    assert_eq!(first["D"].value, ExprValue::Int(1));

    // Second element: A*B*C product advances rightmost (C), D advances by 1
    let second = iter.next().unwrap();
    assert_eq!(second["A"].value, ExprValue::String("a0".to_string()));
    assert_eq!(second["B"].value, ExprValue::String("b0".to_string()));
    assert_eq!(second["C"].value, ExprValue::String("c1".to_string()));
    assert_eq!(second["D"].value, ExprValue::Int(2));
    drop(iter);

    // Random access near the end
    let iter2 = StepParameterSpaceIterator::new(&space).unwrap();
    let last = iter2.get(iter2.len() - 1).unwrap();
    // A*B*C index 999_999_999: A=999_999_999/(1000*1000)=999, B=(999_999_999/1000)%1000=999, C=999_999_999%1000=999
    assert_eq!(last["A"].value, ExprValue::String("a999".to_string()));
    assert_eq!(last["B"].value, ExprValue::String("b999".to_string()));
    assert_eq!(last["C"].value, ExprValue::String("c999".to_string()));
    assert_eq!(last["D"].value, ExprValue::Int(1_000_000_000));

    // Spot check in the middle: element 500_000_000
    let mid = iter2.get(500_000_000).unwrap();
    // A*B*C index 500_000_000: A=500_000_000/(1000*1000)=500, B=(500_000_000/1000)%1000=0, C=500_000_000%1000=0
    assert_eq!(mid["A"].value, ExprValue::String("a500".to_string()));
    assert_eq!(mid["B"].value, ExprValue::String("b0".to_string()));
    assert_eq!(mid["C"].value, ExprValue::String("c0".to_string()));
    assert_eq!(mid["D"].value, ExprValue::Int(500_000_001));

    let elapsed = start.elapsed();
    assert!(
        elapsed.as_millis() < 100,
        "Should complete in <100ms, took {}ms",
        elapsed.as_millis()
    );
}

#[test]
fn test_chunk_int_contiguous_two_element_chunk_displays_as_range() {
    // Regression: a 2-element contiguous chunk like 7-8 must display as "7-8" not "7,8"
    let space = make_space(
        vec![(
            "Frame",
            TaskParameter::ChunkInt {
                range: TaskParamRange::RangeExpr("1-8".parse::<RangeExpr>().unwrap()),
                chunks: ResolvedChunks {
                    default_task_count: 3,
                    target_runtime_seconds: None,
                    range_constraint: RangeConstraint::Contiguous,
                },
            },
        )],
        None,
    );
    let iter = StepParameterSpaceIterator::new(&space).unwrap();
    assert_eq!(iter.len(), 3);
    let chunks: Vec<_> = iter.collect();
    // Each chunk's Frame value is a RangeExpr — check their string representations
    let chunk_strs: Vec<String> = chunks
        .iter()
        .map(|c| match &c["Frame"].value {
            ExprValue::RangeExpr(r) => r.to_string(),
            other => panic!("Expected RangeExpr, got {:?}", other),
        })
        .collect();
    assert_eq!(chunk_strs, vec!["1-3", "4-6", "7-8"]);
}

// ── Feature 1: Per-node contains() optimization ──

#[test]
fn test_contains_optimized_range_expr() {
    // RangeExpr 1-1000000: contains uses RangeExpr::contains(i64) directly,
    // not linear scan over 1M elements.
    use std::time::Instant;

    let space = make_space(vec![("Frame", int_range_expr("1-1000000"))], None);
    let iter = StepParameterSpaceIterator::new(&space).unwrap();

    let mut hit = TaskParameterSet::new();
    hit.insert(
        "Frame".to_string(),
        TaskParameterValue {
            param_type: TaskParameterType::Int,
            value: ExprValue::Int(500_000),
        },
    );

    let mut miss = TaskParameterSet::new();
    miss.insert(
        "Frame".to_string(),
        TaskParameterValue {
            param_type: TaskParameterType::Int,
            value: ExprValue::Int(2_000_000),
        },
    );

    let start = Instant::now();
    assert!(iter.contains(&hit));
    assert!(!iter.contains(&miss));
    let elapsed = start.elapsed();
    // Must be sub-millisecond (O(log n) not O(n))
    assert!(
        elapsed.as_millis() < 10,
        "contains() took {}ms, expected <10ms",
        elapsed.as_millis()
    );
}

#[test]
fn test_contains_product_node() {
    let space = make_space(
        vec![("A", int_list(&[1, 2, 3])), ("B", string_list(&["x", "y"]))],
        Some("A * B"),
    );
    let iter = StepParameterSpaceIterator::new(&space).unwrap();

    // Matching set: A=2, B="y"
    let mut matching = TaskParameterSet::new();
    matching.insert(
        "A".to_string(),
        TaskParameterValue {
            param_type: TaskParameterType::Int,
            value: ExprValue::Int(2),
        },
    );
    matching.insert(
        "B".to_string(),
        TaskParameterValue {
            param_type: TaskParameterType::String,
            value: ExprValue::String("y".to_string()),
        },
    );
    assert!(iter.contains(&matching));

    // Non-matching: A=2, B="z" (z not in range)
    let mut non_matching = TaskParameterSet::new();
    non_matching.insert(
        "A".to_string(),
        TaskParameterValue {
            param_type: TaskParameterType::Int,
            value: ExprValue::Int(2),
        },
    );
    non_matching.insert(
        "B".to_string(),
        TaskParameterValue {
            param_type: TaskParameterType::String,
            value: ExprValue::String("z".to_string()),
        },
    );
    assert!(!iter.contains(&non_matching));

    // Non-matching: A=99, B="x" (99 not in range)
    let mut non_matching2 = TaskParameterSet::new();
    non_matching2.insert(
        "A".to_string(),
        TaskParameterValue {
            param_type: TaskParameterType::Int,
            value: ExprValue::Int(99),
        },
    );
    non_matching2.insert(
        "B".to_string(),
        TaskParameterValue {
            param_type: TaskParameterType::String,
            value: ExprValue::String("x".to_string()),
        },
    );
    assert!(!iter.contains(&non_matching2));
}

// ── Feature 2: Adaptive chunking ──

#[test]
fn test_adaptive_chunking_basic() {
    let space = make_space(
        vec![(
            "C",
            TaskParameter::ChunkInt {
                range: TaskParamRange::RangeExpr("1-10".parse::<RangeExpr>().unwrap()),
                chunks: ResolvedChunks {
                    default_task_count: 3,
                    target_runtime_seconds: Some(10),
                    range_constraint: RangeConstraint::Contiguous,
                },
            },
        )],
        None,
    );
    let iter = StepParameterSpaceIterator::new(&space).unwrap();

    assert!(iter.chunks_adaptive());
    assert_eq!(iter.chunks_parameter_name(), Some("C"));
    assert_eq!(iter.chunks_default_task_count(), Some(3));

    // Iterate and collect all chunks
    let mut chunks = Vec::new();
    for set in iter {
        match &set["C"].value {
            ExprValue::RangeExpr(r) => chunks.push(r.to_string()),
            other => panic!("expected RangeExpr, got {other:?}"),
        }
    }
    // 1-3, 4-6, 7-9, 10-10 (contiguous: breaks at gaps, chunks of up to 3)
    assert_eq!(chunks, vec!["1-3", "4-6", "7-9", "10-10"]);
}

#[test]
fn test_adaptive_chunking_change_size() {
    let space = make_space(
        vec![(
            "C",
            TaskParameter::ChunkInt {
                range: TaskParamRange::RangeExpr("1-12".parse::<RangeExpr>().unwrap()),
                chunks: ResolvedChunks {
                    default_task_count: 3,
                    target_runtime_seconds: Some(10),
                    range_constraint: RangeConstraint::Contiguous,
                },
            },
        )],
        None,
    );
    let mut iter = StepParameterSpaceIterator::new(&space).unwrap();

    // Get first chunk with size 3
    let first = iter.next().unwrap();
    let first_str = match &first["C"].value {
        ExprValue::RangeExpr(r) => r.to_string(),
        other => panic!("expected RangeExpr, got {other:?}"),
    };
    assert_eq!(first_str, "1-3");

    // Change chunk size to 5 — takes effect on next iteration without resetting position
    iter.set_chunks_default_task_count(5);
    assert_eq!(iter.chunks_default_task_count(), Some(5));

    // Now iterate with new size, continuing from where we left off (after 1-3)
    let mut chunks = Vec::new();
    for set in iter {
        match &set["C"].value {
            ExprValue::RangeExpr(r) => chunks.push(r.to_string()),
            other => panic!("expected RangeExpr, got {other:?}"),
        }
    }
    // 4-8, 9-12 (contiguous chunks of up to 5, continuing from position 4)
    assert_eq!(chunks, vec!["4-8", "9-12"]);
}

#[test]
fn test_adaptive_chunking_len_returns_zero() {
    let space = make_space(
        vec![(
            "C",
            TaskParameter::ChunkInt {
                range: TaskParamRange::RangeExpr("1-10".parse::<RangeExpr>().unwrap()),
                chunks: ResolvedChunks {
                    default_task_count: 3,
                    target_runtime_seconds: Some(10),
                    range_constraint: RangeConstraint::Contiguous,
                },
            },
        )],
        None,
    );
    let iter = StepParameterSpaceIterator::new(&space).unwrap();
    assert_eq!(iter.len(), 0, "adaptive chunking len() should return 0");
}

// ══════════════════════════════════════════════════════════════
// PATH parameter contains
// ══════════════════════════════════════════════════════════════

#[test]
fn path_parameter_contains() {
    fn path_list(vals: &[&str]) -> TaskParameter {
        TaskParameter::Path {
            range: vals.iter().map(|s| s.to_string()).collect(),
        }
    }
    let space = make_space(vec![("File", path_list(&["/a/b", "/c/d", "/e/f"]))], None);
    let iter = StepParameterSpaceIterator::new(&space).unwrap();
    for task in iter {
        assert!(StepParameterSpaceIterator::new(&space)
            .unwrap()
            .contains(&task));
    }
}

// ══════════════════════════════════════════════════════════════
// PATH parameter containment via ExprValue::String vs ExprValue::Path
// ══════════════════════════════════════════════════════════════

fn make_path_space(paths: Vec<&str>) -> StepParameterSpace {
    let mut defs = indexmap::IndexMap::new();
    defs.insert(
        "Dir".to_string(),
        TaskParameter::Path {
            range: paths.iter().map(|s| s.to_string()).collect(),
        },
    );
    StepParameterSpace {
        task_parameter_definitions: defs,
        combination: None,
    }
}

#[test]
fn validate_containment_path_as_string_works() {
    let space = make_path_space(vec!["/tmp/a", "/tmp/b", "/tmp/c"]);
    let iter = StepParameterSpaceIterator::new(&space).unwrap();

    let mut params = TaskParameterSet::new();
    params.insert(
        "Dir".into(),
        TaskParameterValue {
            param_type: TaskParameterType::Path,
            value: openjd_expr::ExprValue::String("/tmp/b".to_string()),
        },
    );
    assert!(
        iter.validate_containment(&params).is_ok(),
        "validate_containment should accept ExprValue::String for PATH param"
    );
}

#[test]
fn validate_containment_path_as_path_value() {
    let space = make_path_space(vec!["/tmp/a", "/tmp/b", "/tmp/c"]);
    let iter = StepParameterSpaceIterator::new(&space).unwrap();

    let mut params = TaskParameterSet::new();
    params.insert(
        "Dir".into(),
        TaskParameterValue {
            param_type: TaskParameterType::Path,
            value: openjd_expr::ExprValue::new_path(
                "/tmp/b".to_string(),
                openjd_expr::PathFormat::Posix,
            ),
        },
    );
    assert!(
        iter.validate_containment(&params).is_ok(),
        "validate_containment should accept ExprValue::Path for PATH param"
    );
}

#[test]
fn test_chunk_int_contiguous_with_noncontiguous_range() {
    // Contiguous chunking must respect gaps in the source range.
    // Range "1,3-4,5-7,10-20" has values [1, 3,4, 5,6,7, 10..20].
    // Contiguous intervals: [1], [3,4,5,6,7], [10..20].
    // With default_task_count=20, each interval fits in one chunk:
    //   "1-1", "3-7", "10-20"
    // But 3-4 and 5-7 are adjacent (4+1=5), so it's actually [1], [3..7], [10..20].
    let space = make_space(
        vec![(
            "Frame",
            TaskParameter::ChunkInt {
                range: TaskParamRange::RangeExpr("1,3-4,5-7,10-20".parse::<RangeExpr>().unwrap()),
                chunks: ResolvedChunks {
                    default_task_count: 20,
                    target_runtime_seconds: None,
                    range_constraint: RangeConstraint::Contiguous,
                },
            },
        )],
        None,
    );
    let iter = StepParameterSpaceIterator::new(&space).unwrap();
    let chunk_strs: Vec<String> = iter
        .map(|c| match &c["Frame"].value {
            ExprValue::RangeExpr(r) => r.to_string(),
            other => panic!("Expected RangeExpr, got {:?}", other),
        })
        .collect();
    assert_eq!(chunk_strs, vec!["1-1", "3-7", "10-20"]);
}

// ── Tests ported from Python test_step_param_space_iter_with_chunks.py ──

/// Helper: build a single-param chunked space and collect iteration results as strings.
fn collect_chunk_strs(param: TaskParameter, combination: Option<&str>) -> Vec<String> {
    let space = make_space(vec![("P", param)], combination);
    let iter = StepParameterSpaceIterator::new(&space).unwrap();
    iter.map(|s| match &s["P"].value {
        ExprValue::RangeExpr(r) => r.to_string(),
        other => panic!("Expected RangeExpr, got {:?}", other),
    })
    .collect()
}

fn chunk_int_list(vals: &[i64], dtc: usize, constraint: RangeConstraint) -> TaskParameter {
    TaskParameter::ChunkInt {
        range: TaskParamRange::List(vals.to_vec()),
        chunks: ResolvedChunks {
            default_task_count: dtc,
            target_runtime_seconds: None,
            range_constraint: constraint,
        },
    }
}

fn chunk_int_range(expr: &str, dtc: usize, constraint: RangeConstraint) -> TaskParameter {
    TaskParameter::ChunkInt {
        range: TaskParamRange::RangeExpr(expr.parse::<RangeExpr>().unwrap()),
        chunks: ResolvedChunks {
            default_task_count: dtc,
            target_runtime_seconds: None,
            range_constraint: constraint,
        },
    }
}

fn chunk_int_range_adaptive(
    expr: &str,
    dtc: usize,
    trs: usize,
    constraint: RangeConstraint,
) -> TaskParameter {
    TaskParameter::ChunkInt {
        range: TaskParamRange::RangeExpr(expr.parse::<RangeExpr>().unwrap()),
        chunks: ResolvedChunks {
            default_task_count: dtc,
            target_runtime_seconds: Some(trs),
            range_constraint: constraint,
        },
    }
}

// -- Single-param contiguous chunking --

#[test]
fn test_py_contig_chunksize1_short_list() {
    let r = collect_chunk_strs(
        chunk_int_list(&[1, 2], 1, RangeConstraint::Contiguous),
        None,
    );
    assert_eq!(r, vec!["1-1", "2-2"]);
}

#[test]
fn test_py_contig_chunksize2_short_list() {
    let r = collect_chunk_strs(
        chunk_int_list(&[1, 2], 2, RangeConstraint::Contiguous),
        None,
    );
    assert_eq!(r, vec!["1-2"]);
}

#[test]
fn test_py_contig_chunksize1_short_range() {
    let r = collect_chunk_strs(chunk_int_range("1-2", 1, RangeConstraint::Contiguous), None);
    assert_eq!(r, vec!["1-1", "2-2"]);
}

#[test]
fn test_py_contig_chunksize2_short_range() {
    let r = collect_chunk_strs(chunk_int_range("1-2", 2, RangeConstraint::Contiguous), None);
    assert_eq!(r, vec!["1-2"]);
}

#[test]
fn test_py_contig_chunksize100_noncontig_range() {
    // Each isolated value becomes its own chunk
    let r = collect_chunk_strs(
        chunk_int_range("1,3,5", 100, RangeConstraint::Contiguous),
        None,
    );
    assert_eq!(r, vec!["1-1", "3-3", "5-5"]);
}

#[test]
fn test_py_contig_chunksize10_range_1_35() {
    // Non-adaptive spreads chunks evenly: 35/4 chunks → sizes 9,9,9,8
    let r = collect_chunk_strs(
        chunk_int_range("1-35", 10, RangeConstraint::Contiguous),
        None,
    );
    assert_eq!(r, vec!["1-9", "10-18", "19-27", "28-35"]);
}

#[test]
fn test_py_contig_chunksize5_negative_frames() {
    let r = collect_chunk_strs(
        chunk_int_range("-20--5", 5, RangeConstraint::Contiguous),
        None,
    );
    assert_eq!(r, vec!["-20--17", "-16--13", "-12--9", "-8--5"]);
}

// -- Single-param noncontiguous chunking --

#[test]
fn test_py_noncontig_chunksize1_short_list() {
    let r = collect_chunk_strs(
        chunk_int_list(&[1, 2], 1, RangeConstraint::Noncontiguous),
        None,
    );
    assert_eq!(r, vec!["1", "2"]);
}

#[test]
fn test_py_noncontig_chunksize2_short_list() {
    let r = collect_chunk_strs(
        chunk_int_list(&[1, 2], 2, RangeConstraint::Noncontiguous),
        None,
    );
    assert_eq!(r, vec!["1,2"]);
}

#[test]
fn test_py_noncontig_chunksize1_short_range() {
    let r = collect_chunk_strs(
        chunk_int_range("1-2", 1, RangeConstraint::Noncontiguous),
        None,
    );
    assert_eq!(r, vec!["1", "2"]);
}

#[test]
fn test_py_noncontig_chunksize2_short_range() {
    let r = collect_chunk_strs(
        chunk_int_range("1-2", 2, RangeConstraint::Noncontiguous),
        None,
    );
    assert_eq!(r, vec!["1,2"]);
}

#[test]
fn test_py_noncontig_chunksize100_noncontig_range() {
    let r = collect_chunk_strs(
        chunk_int_range("1,3,5", 100, RangeConstraint::Noncontiguous),
        None,
    );
    assert_eq!(r, vec!["1-5:2"]);
}

// -- Single-param adaptive chunking --

#[test]
fn test_py_adaptive_noncontig_chunksize10_range_1_35() {
    // Adaptive makes chunks as big as possible, last chunk smaller
    let r = collect_chunk_strs(
        chunk_int_range_adaptive("1-35", 10, 20, RangeConstraint::Noncontiguous),
        None,
    );
    assert_eq!(r, vec!["1-10", "11-20", "21-30", "31-35"]);
}

#[test]
fn test_py_adaptive_contig_chunksize5_negative() {
    let r = collect_chunk_strs(
        chunk_int_range_adaptive("-20--5", 5, 20, RangeConstraint::Contiguous),
        None,
    );
    assert_eq!(r, vec!["-20--16", "-15--11", "-10--6", "-5--5"]);
}

#[test]
fn test_py_adaptive_noncontig_chunksize5_negative() {
    let r = collect_chunk_strs(
        chunk_int_range_adaptive("-20--5", 5, 20, RangeConstraint::Noncontiguous),
        None,
    );
    assert_eq!(r, vec!["-20--16", "-15--11", "-10--6", "-5"]);
}

// -- Multi-param chunked iteration --

fn chunk_int_list_adaptive(
    vals: &[i64],
    dtc: usize,
    trs: usize,
    constraint: RangeConstraint,
) -> TaskParameter {
    TaskParameter::ChunkInt {
        range: TaskParamRange::List(vals.to_vec()),
        chunks: ResolvedChunks {
            default_task_count: dtc,
            target_runtime_seconds: Some(trs),
            range_constraint: constraint,
        },
    }
}

/// Helper: build a 2-param space (chunk + string) and collect as (chunk_str, string_val) tuples.
fn collect_chunk_product(
    chunk_param: TaskParameter,
    string_vals: &[&str],
    chunk_override: Option<usize>,
) -> Vec<(String, String)> {
    let iter = make_chunk_product_iter(chunk_param, string_vals, chunk_override);
    extract_chunk_product(iter)
}

fn make_chunk_product_iter(
    chunk_param: TaskParameter,
    string_vals: &[&str],
    chunk_override: Option<usize>,
) -> StepParameterSpaceIterator {
    use crate::job;
    use indexmap::IndexMap;

    let mut defs = IndexMap::new();
    defs.insert("P1".to_string(), chunk_param);
    defs.insert(
        "P2".to_string(),
        TaskParameter::String {
            range: string_vals.iter().map(|s| s.to_string()).collect(),
        },
    );
    let space = job::StepParameterSpace {
        task_parameter_definitions: defs,
        combination: None,
    };
    match chunk_override {
        Some(n) => StepParameterSpaceIterator::new_with_chunk_override(&space, Some(n)).unwrap(),
        None => StepParameterSpaceIterator::new(&space).unwrap(),
    }
}

fn extract_chunk_product(iter: StepParameterSpaceIterator) -> Vec<(String, String)> {
    iter.map(|s| {
        let p1 = match &s["P1"].value {
            ExprValue::RangeExpr(r) => r.to_string(),
            other => panic!("Expected RangeExpr for P1, got {:?}", other),
        };
        let p2 = match &s["P2"].value {
            ExprValue::String(s) => s.clone(),
            other => panic!("Expected String for P2, got {:?}", other),
        };
        (p1, p2)
    })
    .collect()
}

fn next_chunk_product(iter: &mut StepParameterSpaceIterator) -> (String, String) {
    let s = iter.next().expect("expected another item");
    let p1 = match &s["P1"].value {
        ExprValue::RangeExpr(r) => r.to_string(),
        other => panic!("Expected RangeExpr for P1, got {:?}", other),
    };
    let p2 = match &s["P2"].value {
        ExprValue::String(s) => s.clone(),
        other => panic!("Expected String for P2, got {:?}", other),
    };
    (p1, p2)
}

#[test]
fn test_py_multi_contig_chunksize1_adaptive() {
    // Adaptive dimension should be innermost (varies fastest)
    let r = collect_chunk_product(
        chunk_int_list_adaptive(&[1, 2], 1, 20, RangeConstraint::Contiguous),
        &["A", "B"],
        None,
    );
    assert_eq!(
        r,
        vec![
            ("1-1".into(), "A".into()),
            ("2-2".into(), "A".into()),
            ("1-1".into(), "B".into()),
            ("2-2".into(), "B".into()),
        ]
    );
}

#[test]
fn test_py_multi_contig_chunksize2_adaptive() {
    let r = collect_chunk_product(
        chunk_int_list_adaptive(&[1, 2], 2, 20, RangeConstraint::Contiguous),
        &["A", "B"],
        None,
    );
    assert_eq!(
        r,
        vec![("1-2".into(), "A".into()), ("1-2".into(), "B".into())]
    );
}

#[test]
fn test_py_multi_noncontig_chunksize2_adaptive_override1() {
    // Override=1 turns off adaptive, uses chunksize 1
    let r = collect_chunk_product(
        chunk_int_list_adaptive(&[1, 2], 2, 20, RangeConstraint::Noncontiguous),
        &["A", "B"],
        Some(1),
    );
    assert_eq!(
        r,
        vec![
            ("1".into(), "A".into()),
            ("1".into(), "B".into()),
            ("2".into(), "A".into()),
            ("2".into(), "B".into()),
        ]
    );
}

#[test]
fn test_py_adaptive_contiguous_mid_iteration_size_change() {
    // Port of test_adaptive_contiguous_chunked_iteration
    let mut iter = make_chunk_product_iter(
        chunk_int_range_adaptive("1-20", 2, 20, RangeConstraint::Contiguous),
        &["A", "B"],
        None,
    );
    assert_eq!(next_chunk_product(&mut iter), ("1-2".into(), "A".into()));
    assert_eq!(next_chunk_product(&mut iter), ("3-4".into(), "A".into()));
    iter.set_chunks_default_task_count(10);
    assert_eq!(next_chunk_product(&mut iter), ("5-14".into(), "A".into()));
    assert_eq!(next_chunk_product(&mut iter), ("15-20".into(), "A".into()));
    assert_eq!(next_chunk_product(&mut iter), ("1-10".into(), "B".into()));
    iter.set_chunks_default_task_count(4);
    assert_eq!(next_chunk_product(&mut iter), ("11-14".into(), "B".into()));
    assert_eq!(next_chunk_product(&mut iter), ("15-18".into(), "B".into()));
    iter.set_chunks_default_task_count(1);
    assert_eq!(next_chunk_product(&mut iter), ("19-19".into(), "B".into()));
    assert_eq!(next_chunk_product(&mut iter), ("20-20".into(), "B".into()));
    assert!(iter.next().is_none());
    assert_eq!(iter.chunks_parameter_name(), Some("P1"));
}

#[test]
fn test_py_adaptive_noncontiguous_mid_iteration_size_change() {
    // Port of test_adaptive_noncontiguous_chunked_iteration
    let mut iter = make_chunk_product_iter(
        chunk_int_range_adaptive(
            "1-10,12,15,18,20-23,1000",
            2,
            20,
            RangeConstraint::Noncontiguous,
        ),
        &["A", "B"],
        None,
    );
    assert_eq!(next_chunk_product(&mut iter), ("1,2".into(), "A".into()));
    assert_eq!(next_chunk_product(&mut iter), ("3,4".into(), "A".into()));
    iter.set_chunks_default_task_count(10);
    assert_eq!(
        next_chunk_product(&mut iter),
        ("5-10,12-18:3,20".into(), "A".into())
    );
    assert_eq!(
        next_chunk_product(&mut iter),
        ("21-23,1000".into(), "A".into())
    );
    assert_eq!(next_chunk_product(&mut iter), ("1-10".into(), "B".into()));
    iter.set_chunks_default_task_count(4);
    assert_eq!(
        next_chunk_product(&mut iter),
        ("12-18:3,20".into(), "B".into())
    );
    assert_eq!(
        next_chunk_product(&mut iter),
        ("21-23,1000".into(), "B".into())
    );
    assert!(iter.next().is_none());
    assert_eq!(iter.chunks_parameter_name(), Some("P1"));
}

#[test]
fn test_py_multi_contig_chunksize1() {
    let r = collect_chunk_product(
        chunk_int_list(&[1, 2], 1, RangeConstraint::Contiguous),
        &["A", "B"],
        None,
    );
    assert_eq!(
        r,
        vec![
            ("1-1".into(), "A".into()),
            ("1-1".into(), "B".into()),
            ("2-2".into(), "A".into()),
            ("2-2".into(), "B".into()),
        ]
    );
}

#[test]
fn test_py_multi_contig_chunksize2() {
    let r = collect_chunk_product(
        chunk_int_list(&[1, 2], 2, RangeConstraint::Contiguous),
        &["A", "B"],
        None,
    );
    assert_eq!(
        r,
        vec![("1-2".into(), "A".into()), ("1-2".into(), "B".into())]
    );
}

#[test]
fn test_py_multi_contig_chunksize1_override5() {
    // Override chunk size to 5 → single chunk "1-2"
    let r = collect_chunk_product(
        chunk_int_list(&[1, 2], 1, RangeConstraint::Contiguous),
        &["A", "B"],
        Some(5),
    );
    assert_eq!(
        r,
        vec![("1-2".into(), "A".into()), ("1-2".into(), "B".into())]
    );
}

#[test]
fn test_py_multi_contig_chunksize2_override1() {
    // Override chunk size to 1 → two chunks
    let r = collect_chunk_product(
        chunk_int_list(&[1, 2], 2, RangeConstraint::Contiguous),
        &["A", "B"],
        Some(1),
    );
    assert_eq!(
        r,
        vec![
            ("1-1".into(), "A".into()),
            ("1-1".into(), "B".into()),
            ("2-2".into(), "A".into()),
            ("2-2".into(), "B".into()),
        ]
    );
}

// ── NONCONTIGUOUS chunks and the range expression run cap ──
//
// A NONCONTIGUOUS chunk is a `RangeExpr`, which holds at most
// `MAX_RANGE_EXPR_CHUNKS` runs, and its canonical form can have up to two
// more runs than the runs of the range it intersects (see
// specs/model/parameter-space.md, Noncontiguous Static Chunking).

const CAP: usize = openjd_expr::MAX_RANGE_EXPR_CHUNKS;

/// A range of `CAP` runs: `0-(head-1)`, then `CAP - 2` isolated values,
/// then a run of three far away. Splitting it into two chunks that cut the
/// first run two values from its end gives a second chunk of `CAP + 1`
/// runs: the last two values of the first run are written as two runs.
fn cap_runs_range(head: i64) -> RangeExpr {
    let mut values: Vec<i64> = (0..head).collect();
    values.extend((1..=CAP as i64 - 2).map(|k| head + 100 + k * k));
    let far = values[values.len() - 1] + 1_000_000_000;
    values.extend([far, far + 1, far + 2]);
    let r = RangeExpr::from_values(values).unwrap();
    assert_eq!(r.ranges().len(), CAP);
    r
}

fn chunk_int_of(range: TaskParamRange<i64>, dtc: usize, trs: Option<usize>) -> TaskParameter {
    TaskParameter::ChunkInt {
        range,
        chunks: ResolvedChunks {
            default_task_count: dtc,
            target_runtime_seconds: trs,
            range_constraint: RangeConstraint::Noncontiguous,
        },
    }
}

fn chunk_value(set: &TaskParameterSet) -> RangeExpr {
    match &set["F"].value {
        ExprValue::RangeExpr(r) => r.clone(),
        other => panic!("expected a RangeExpr chunk, got {other:?}"),
    }
}

#[test]
fn test_noncontiguous_static_chunk_beyond_the_run_cap_is_an_error() {
    // 50,006 + 49,998 + 3 = 100,007 values in two chunks of 50,004 and
    // 50,003: the second starts two values before the end of the first run.
    let range = cap_runs_range(50_006);
    assert_eq!(range.len(), 100_007);
    let space = make_space(
        vec![(
            "F",
            chunk_int_of(TaskParamRange::RangeExpr(range), 60_000, None),
        )],
        None,
    );
    let err = StepParameterSpaceIterator::new(&space).err().unwrap();
    assert_eq!(
        err.to_string(),
        "Validation error: Task parameter 'F': chunk 1 (values 50004 to 3499850112): Range expression has too many sub-ranges in canonical form (50001); maximum is 50000"
    );
}

#[test]
fn test_noncontiguous_static_chunks_of_a_cap_run_range_that_fit() {
    // The same range in one chunk is the range itself; cut three values
    // from the end of the first run instead of two, both chunks fit.
    for (head, dtc, chunks) in [(50_006, 100_007, 1), (50_007, 60_000, 2)] {
        let range = cap_runs_range(head);
        let space = make_space(
            vec![(
                "F",
                chunk_int_of(TaskParamRange::RangeExpr(range.clone()), dtc, None),
            )],
            None,
        );
        let iter = StepParameterSpaceIterator::new(&space).unwrap();
        let got: Vec<RangeExpr> = iter.map(|set| chunk_value(&set)).collect();
        assert_eq!(got.len(), chunks, "head {head}");
        let total: usize = got.iter().map(RangeExpr::len).sum();
        assert_eq!(total, range.len());
        assert!(got.iter().all(|c| c.ranges().len() <= CAP));
    }
}

#[test]
fn test_noncontiguous_static_list_chunk_beyond_the_run_cap_is_an_error() {
    // A list range from the public API (a template caps lists at 1,024
    // values): 100,001 squares in chunks of 50,001 and 50,000 values, each
    // value a run.
    let squares: Vec<i64> = (0..100_001i64).map(|i| i * i).collect();
    let space = make_space(
        vec![(
            "F",
            chunk_int_of(TaskParamRange::List(squares), 60_000, None),
        )],
        None,
    );
    let err = StepParameterSpaceIterator::new(&space).err().unwrap();
    assert_eq!(
        err.to_string(),
        "Validation error: Task parameter 'F': chunk 0 (values 0 to 2500000000): Range expression has too many sub-ranges in canonical form (50001); maximum is 50000"
    );
}

#[test]
fn test_noncontiguous_static_unsorted_list_chunk_error_names_its_smallest_and_largest_values() {
    // A public-API list is chunked in list order; the error names the
    // chunk's smallest and largest values, whatever their order.
    let squares: Vec<i64> = (0..100_001i64).rev().map(|i| i * i).collect();
    let space = make_space(
        vec![(
            "F",
            chunk_int_of(TaskParamRange::List(squares), 60_000, None),
        )],
        None,
    );
    let err = StepParameterSpaceIterator::new(&space).err().unwrap();
    assert_eq!(
        err.to_string(),
        "Validation error: Task parameter 'F': chunk 0 (values 2500000000 to 10000000000): Range expression has too many sub-ranges in canonical form (50001); maximum is 50000"
    );
}

#[test]
fn test_noncontiguous_adaptive_chunk_beyond_the_run_cap_is_shortened() {
    // An adaptive chunk size can grow past the cap at run time, where
    // iteration cannot fail. A chunk whose canonical form would have more
    // than CAP runs takes CAP values instead.
    let range = cap_runs_range(10);
    let len = range.len();
    let space = make_space(
        vec![(
            "F",
            chunk_int_of(TaskParamRange::RangeExpr(range.clone()), 2, Some(60)),
        )],
        None,
    );
    let mut iter = StepParameterSpaceIterator::new(&space).unwrap();
    assert!(iter.chunks_adaptive());
    // All but the last value: CAP + 1 runs, so CAP values are taken.
    iter.set_chunks_default_task_count(len - 1);
    let first = chunk_value(&iter.next().unwrap());
    assert_eq!(first.len(), CAP);
    assert_eq!(first.to_vec(), range.to_vec()[..CAP]);
    let rest = chunk_value(&iter.next().unwrap());
    assert_eq!(rest.to_vec(), range.to_vec()[CAP..]);
    assert!(iter.next().is_none());
    // A chunk that fits is not shortened: the whole range is CAP runs.
    iter.reset();
    iter.set_chunks_default_task_count(len);
    assert_eq!(chunk_value(&iter.next().unwrap()), range);
    assert!(iter.next().is_none());
}

/// A chunk layout for `check_noncontiguous_chunks_fit`: `chunks` chunks, the
/// first `larger` of `small + 1` values and the rest of `small`, with chunk
/// `failing` the only one beyond the run cap.
///
/// The range is written as text, so it is never expanded: an optional
/// isolated value far below (`lead`), then one run that ends two values
/// into chunk `failing`, then `isolated` isolated values, then one run that
/// starts two values before the end of chunk `failing`. Chunk `failing`
/// has `2 + isolated + 2` values, each its own run, and the range has
/// `isolated + 2` runs (`+ 1` with `lead`).
struct ChunkLayout {
    chunks: usize,
    small: usize,
    larger: usize,
    failing: usize,
    lead: bool,
}

impl ChunkLayout {
    fn size(&self, i: usize) -> usize {
        self.small + usize::from(i < self.larger)
    }

    /// The range, the `defaultTaskCount` that gives this layout, the
    /// number of runs, and the failing chunk's first and last values.
    fn build(&self) -> (RangeExpr, usize, usize, i64, i64) {
        let failing_size = self.size(self.failing);
        let isolated = failing_size - 4;
        let before: usize = (0..self.failing).map(|i| self.size(i)).sum();
        let after: usize = (self.failing + 1..self.chunks).map(|i| self.size(i)).sum();
        let mut elements = Vec::new();
        let head_start: i64 = if self.lead {
            elements.push("-1000".to_string());
            0
        } else {
            0
        };
        let head_len = (before + 2 - usize::from(self.lead)) as i64;
        let head_end = head_start + head_len - 1;
        elements.push(format!("{head_start}-{head_end}"));
        let mut last = head_end;
        for k in 1..=isolated as i64 {
            last = head_end + 100 + k * k;
            elements.push(last.to_string());
        }
        let tail_start = last + 1_000_000_000;
        let tail_end = tail_start + (after + 2) as i64 - 1;
        elements.push(format!("{tail_start}-{tail_end}"));
        let range: RangeExpr = elements.join(",").parse().unwrap();
        let total = range.len();
        assert_eq!(total, self.chunks * self.small + self.larger);
        // ceil(total / dtc) == chunks, with the remainder spread as above.
        let dtc = if self.larger == 0 {
            self.small
        } else {
            self.small + 1
        };
        assert_eq!(total.div_ceil(dtc), self.chunks);
        let runs = range.ranges().len();
        (range, dtc, runs, head_end - 1, tail_start + 1)
    }
}

fn static_noncontiguous_error(range: RangeExpr, dtc: usize) -> Option<String> {
    let space = make_space(
        vec![(
            "F",
            chunk_int_of(TaskParamRange::RangeExpr(range), dtc, None),
        )],
        None,
    );
    StepParameterSpaceIterator::new(&space)
        .err()
        .map(|e| e.to_string())
}

#[test]
fn test_noncontiguous_static_check_finds_the_failing_chunk_anywhere() {
    // The check examines one chunk, found by index arithmetic: cover a
    // failing chunk in the middle among the larger chunks, the last larger
    // chunk, among the smaller chunks, a failing chunk that misses the
    // range's first run (`lead`), and both 49,999 and 50,000 runs. (A
    // failing first or last chunk is tested above.) Each row: chunks,
    // small, larger, failing, lead, expected runs.
    let rows = [
        (6, 50_001, 4, 2, false, 50_000),
        (6, 50_001, 4, 3, false, 50_000),
        (6, 50_002, 2, 4, false, 50_000),
        (6, 50_001, 0, 3, false, 49_999),
        (6, 50_000, 3, 1, false, 49_999),
        (6, 50_001, 0, 3, true, 50_000),
        (6, 50_000, 5, 2, true, 50_000),
    ];
    for (chunks, small, larger, failing, lead, expected_runs) in rows {
        let layout = ChunkLayout {
            chunks,
            small,
            larger,
            failing,
            lead,
        };
        let (range, dtc, runs, first, last) = layout.build();
        assert_eq!(runs, expected_runs, "{chunks} {small} {larger} {failing}");
        let failing_runs = layout.size(failing);
        // The failing chunk is the only one beyond the cap.
        for i in 0..chunks {
            let offset: usize = (0..i).map(|j| layout.size(j)).sum();
            let chunk = range
                .slice(offset as i64, (offset + layout.size(i)) as i64, 1)
                .map(|c| c.ranges().len());
            if i == failing {
                assert!(chunk.is_err(), "chunk {i}");
            } else {
                assert!(
                    chunk.unwrap() <= openjd_expr::MAX_RANGE_EXPR_CHUNKS,
                    "chunk {i}"
                );
            }
        }
        assert_eq!(
            static_noncontiguous_error(range, dtc).as_deref(),
            Some(
                format!(
                    "Validation error: Task parameter 'F': chunk {failing} (values {first} to {last}): Range expression has too many sub-ranges in canonical form ({failing_runs}); maximum is 50000"
                )
                .as_str()
            ),
            "{chunks} {small} {larger} {failing} {lead}"
        );
    }
}

#[test]
fn test_noncontiguous_static_check_with_many_leftovers() {
    // 50,001 chunks, the first 50,000 of 50,002 values and the last of
    // 50,001: the last larger chunk fails. Locating it depends on the span
    // of the larger chunks, `larger * (small + 1)`.
    let layout = ChunkLayout {
        chunks: 50_001,
        small: 50_001,
        larger: 50_000,
        failing: 49_999,
        lead: false,
    };
    let (range, dtc, runs, first, last) = layout.build();
    assert_eq!(runs, 50_000);
    assert_eq!(
        static_noncontiguous_error(range, dtc).as_deref(),
        Some(
            format!(
                "Validation error: Task parameter 'F': chunk 49999 (values {first} to {last}): Range expression has too many sub-ranges in canonical form (50002); maximum is 50000"
            )
            .as_str()
        )
    );
}

/// The first chunk beyond the run cap, found by building every chunk, with
/// the error the check should report for it.
fn first_chunk_beyond_the_cap(range: &RangeExpr, dtc: usize) -> Option<(usize, String)> {
    let total = range.len();
    let chunks = total.div_ceil(dtc);
    let (small, larger) = (total / chunks, total % chunks);
    let mut offset = 0;
    (0..chunks).find_map(|i| {
        let size = small + usize::from(i < larger);
        let (start, stop) = (offset as i64, (offset + size) as i64);
        offset += size;
        range.slice(start, stop, 1).err().map(|e| {
            let (first, last) = (range.get(start).unwrap(), range.get(stop - 1).unwrap());
            (
                i,
                format!(
                    "Validation error: Task parameter 'F': chunk {i} (values {first} to {last}): {e}"
                ),
            )
        })
    })
}

#[test]
fn test_noncontiguous_static_check_agrees_with_building_every_chunk() {
    // Shift the chunk boundaries of failing layouts by dropping values from
    // either end, so the cut falls zero to five values before the end of a
    // run, and compare the check with building every chunk. Some shifts
    // fail and some fit; the check must agree on both, and on which chunk.
    let mut outcomes = (0, 0);
    for (chunks, small, larger, failing, lead) in [
        (3, 50_001, 0, 1, false),
        (4, 50_000, 3, 2, false),
        (4, 50_001, 2, 2, true),
    ] {
        let (range, _, _, _, _) = ChunkLayout {
            chunks,
            small,
            larger,
            failing,
            lead,
        }
        .build();
        let len = range.len() as i64;
        for (cut_front, cut_back) in [(0, 0), (1, 0), (2, 0), (3, 0), (0, 1), (0, 3), (2, 2)] {
            let shifted = range.slice(cut_front, len - cut_back, 1).unwrap();
            for dtc in [
                shifted.len().div_ceil(chunks),
                shifted.len().div_ceil(chunks) + 1,
            ] {
                let want = first_chunk_beyond_the_cap(&shifted, dtc);
                let got = static_noncontiguous_error(shifted.clone(), dtc);
                match want {
                    Some((i, message)) => {
                        outcomes.0 += 1;
                        let got = got.unwrap_or_else(|| panic!("chunk {i} missed"));
                        assert_eq!(got, message);
                    }
                    None => {
                        outcomes.1 += 1;
                        assert_eq!(got, None);
                    }
                }
            }
        }
    }
    // Both outcomes occur, so the comparison tests the check both ways.
    assert!(outcomes.0 > 0 && outcomes.1 > 0, "{outcomes:?}");
}

#[test]
fn test_noncontiguous_static_check_passes_when_no_chunk_can_fail() {
    // 49,998 runs: no window has more than 50,000 runs, so the check does
    // not look at any chunk, even with chunks of more than 50,000 values.
    let layout = ChunkLayout {
        chunks: 3,
        small: 50_000,
        larger: 1,
        failing: 1,
        lead: false,
    };
    let (range, dtc, runs, _, _) = layout.build();
    assert_eq!(runs, 49_998);
    assert_eq!(first_chunk_beyond_the_cap(&range, dtc), None);
    assert_eq!(static_noncontiguous_error(range, dtc), None);
}
