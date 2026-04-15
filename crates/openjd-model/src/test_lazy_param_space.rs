// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Tests for the lazy `StepParameterSpaceIterator`.

use std::collections::HashMap;

use crate::job::{ResolvedChunks, StepParameterSpace, TaskParamRange, TaskParameter};
use crate::step_param_space::StepParameterSpaceIterator;
use crate::template::RangeConstraint;
use crate::types::{TaskParameterSet, TaskParameterType, TaskParameterValue};
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
        range: vals.to_vec(),
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

fn get_int(set: &HashMap<String, TaskParameterValue>, name: &str) -> i64 {
    match &set[name].value {
        ExprValue::Int(i) => *i,
        other => panic!("expected Int for {name}, got {other:?}"),
    }
}

fn get_float(set: &HashMap<String, TaskParameterValue>, name: &str) -> f64 {
    match &set[name].value {
        ExprValue::Float(f) => f.value(),
        other => panic!("expected Float for {name}, got {other:?}"),
    }
}

fn get_string(set: &HashMap<String, TaskParameterValue>, name: &str) -> String {
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

// ── Containment ──

#[test]
fn test_contains_positive() {
    let space = make_space(vec![("A", int_list(&[1, 2, 3]))], None);
    let iter = StepParameterSpaceIterator::new(&space).unwrap();
    let mut needle = HashMap::new();
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
    let mut needle = HashMap::new();
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
    // Each value should be a RangeExpr
    for i in 0..iter.len() {
        let set = iter.get(i).unwrap();
        assert_eq!(set["C"].param_type, TaskParameterType::ChunkInt);
        match &set["C"].value {
            ExprValue::RangeExpr(_) => {}
            other => panic!("expected RangeExpr, got {other:?}"),
        }
    }
    // Verify the chunks cover all 10 values
    let mut all_vals: Vec<i64> = Vec::new();
    for i in 0..iter.len() {
        let set = iter.get(i).unwrap();
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

    let mut hit = std::collections::HashMap::new();
    hit.insert(
        "Frame".to_string(),
        TaskParameterValue {
            param_type: TaskParameterType::Int,
            value: ExprValue::Int(500_000),
        },
    );

    let mut miss = std::collections::HashMap::new();
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
    let mut matching = std::collections::HashMap::new();
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
    let mut non_matching = std::collections::HashMap::new();
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
    let mut non_matching2 = std::collections::HashMap::new();
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
            value: openjd_expr::ExprValue::Path {
                value: "/tmp/b".to_string(),
                format: openjd_expr::PathFormat::Posix,
            },
        },
    );
    assert!(
        iter.validate_containment(&params).is_ok(),
        "validate_containment should accept ExprValue::Path for PATH param"
    );
}
