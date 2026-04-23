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
