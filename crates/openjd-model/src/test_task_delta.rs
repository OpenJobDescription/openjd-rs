// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Tests for `job::task_delta`, checked against brute-force enumeration.

use std::collections::BTreeSet;

use crate::job::task_delta::{task_delta, ProductDelta, TaskBox, TaskDelta};
use crate::job::{ResolvedChunks, StepParameterSpace, TaskParamRange, TaskParameter};
use crate::step_param_space::StepParameterSpaceIterator;
use crate::template::RangeConstraint;
use crate::types::TaskParameterSet;
use openjd_expr::RangeExpr;

fn space(params: Vec<(&str, TaskParameter)>, combination: Option<&str>) -> StepParameterSpace {
    StepParameterSpace {
        task_parameter_definitions: params
            .into_iter()
            .map(|(n, p)| (n.to_string(), p))
            .collect(),
        combination: combination.map(str::to_string),
    }
}

fn ints(expr: &str) -> TaskParameter {
    TaskParameter::Int {
        range: TaskParamRange::RangeExpr(expr.parse::<RangeExpr>().unwrap()),
        chunks: None,
    }
}

fn strs(v: &[&str]) -> TaskParameter {
    TaskParameter::String {
        range: v.iter().map(|s| (*s).to_string()).collect(),
    }
}

fn key(t: &TaskParameterSet) -> String {
    let mut kv: Vec<String> = t
        .iter()
        .map(|(k, v)| format!("{k}={}", serde_json::to_string(&v.value).unwrap()))
        .collect();
    kv.sort();
    kv.join(",")
}

fn all(s: &StepParameterSpace) -> Vec<TaskParameterSet> {
    StepParameterSpaceIterator::new(s).unwrap().collect()
}

fn keys_of(boxes: &[&TaskBox]) -> (BTreeSet<String>, usize) {
    let mut set = BTreeSet::new();
    let mut n = 0;
    for b in boxes {
        for t in b.tasks().unwrap() {
            set.insert(key(&t));
            n += 1;
        }
        assert_eq!(
            b.len(),
            u64::try_from(b.tasks().unwrap().count()).unwrap(),
            "len() matches iteration"
        );
    }
    (set, n)
}

/// Checks the decomposition against enumeration: kept/added/removed are
/// exactly the set differences, boxes are disjoint (no task counted twice),
/// and `old.index_of` reproduces every old task's iteration position.
fn check(old: &StepParameterSpace, new: &StepParameterSpace) -> TaskDelta {
    let d = task_delta(Some(old), Some(new));
    let TaskDelta::Product(pd) = &d else {
        panic!("expected Product, got {d:?}");
    };
    let (old_box, kept, added, removed) = (&pd.old, &pd.kept, &pd.added, &pd.removed);
    let o: BTreeSet<String> = all(old).iter().map(key).collect();
    let n: BTreeSet<String> = all(new).iter().map(key).collect();

    let (k, kn) = keys_of(&[kept]);
    assert_eq!(k, o.intersection(&n).cloned().collect());
    assert_eq!(kn, k.len());

    let (a, an) = keys_of(&added.iter().collect::<Vec<_>>());
    assert_eq!(a, n.difference(&o).cloned().collect());
    assert_eq!(an, a.len(), "added boxes are disjoint");

    let (r, rn) = keys_of(&removed.iter().collect::<Vec<_>>());
    assert_eq!(r, o.difference(&n).cloned().collect());
    assert_eq!(rn, r.len(), "removed boxes are disjoint");

    for (i, t) in all(old).iter().enumerate() {
        assert_eq!(old_box.index_of(t), Some(u64::try_from(i).unwrap()));
    }
    d
}

#[test]
fn equal_spaces_are_unchanged() {
    let s = space(vec![("Frame", ints("1-10"))], None);
    assert_eq!(task_delta(Some(&s), Some(&s)), TaskDelta::Unchanged);
    assert_eq!(task_delta(None, None), TaskDelta::Unchanged);
}

#[test]
fn extending_one_range_adds_one_box() {
    let d = check(
        &space(vec![("Frame", ints("1-10"))], None),
        &space(vec![("Frame", ints("1-12"))], None),
    );
    let TaskDelta::Product(pd) = d else {
        unreachable!()
    };
    let ProductDelta {
        kept,
        added,
        removed,
        ..
    } = *pd;
    assert_eq!(kept.len(), 10);
    assert_eq!(added.iter().map(TaskBox::len).collect::<Vec<_>>(), vec![2]);
    assert!(removed.is_empty());
}

#[test]
fn two_dimensions_both_changed() {
    // Frames shift 1-10 -> 5-14, cameras A,B -> B,C.
    let d = check(
        &space(
            vec![("Frame", ints("1-10")), ("Cam", strs(&["A", "B"]))],
            None,
        ),
        &space(
            vec![("Frame", ints("5-14")), ("Cam", strs(&["B", "C"]))],
            None,
        ),
    );
    let TaskDelta::Product(pd) = d else {
        unreachable!()
    };
    let ProductDelta {
        kept,
        added,
        removed,
        ..
    } = *pd;
    // kept = {5..10} x {B}; added = {11..14} x {B,C}  +  {5..10} x {C}
    assert_eq!(kept.len(), 6);
    assert_eq!(
        added.iter().map(TaskBox::len).collect::<Vec<_>>(),
        vec![8, 6]
    );
    assert_eq!(
        removed.iter().map(TaskBox::len).collect::<Vec<_>>(),
        vec![8, 6]
    );
}

#[test]
fn explicit_product_order_is_respected_for_old_indices() {
    // Same dimension order, written differently: still a product, and
    // index_of follows the combination, not the definition order.
    check(
        &space(
            vec![("Frame", ints("1-4")), ("Cam", strs(&["A", "B", "C"]))],
            Some("Cam * Frame"),
        ),
        &space(
            vec![("Frame", ints("1-6")), ("Cam", strs(&["A", "C"]))],
            Some(" Cam*Frame "),
        ),
    );
}

#[test]
fn a_different_dimension_order_falls_back() {
    // Every kept task moves, even when the task set is identical.
    let f = || ints("1-2");
    let g = || strs(&["a", "b"]);
    let none = space(vec![("F", f()), ("G", g())], None);
    let gf = space(vec![("F", f()), ("G", g())], Some("G * F"));
    let fg = space(vec![("F", f()), ("G", g())], Some("F * G"));
    assert!(is_fallback(&task_delta(Some(&none), Some(&gf))));
    assert!(is_fallback(&task_delta(Some(&fg), Some(&gf))));
    // `F * G` is the default order for these definitions: same task order.
    assert!(!is_fallback(&task_delta(Some(&none), Some(&fg))));
    let wider = space(vec![("F", ints("1-3")), ("G", g())], Some("G * F"));
    assert!(is_fallback(&task_delta(Some(&fg), Some(&wider))));
}

#[test]
fn huge_product_is_decomposed_without_enumeration() {
    // 10^5 frames x 10^3 tiles = 10^8 tasks; only the 1-D ranges are touched.
    let old = space(
        vec![("Frame", ints("1-100000")), ("Tile", ints("1-1000"))],
        None,
    );
    let new = space(
        vec![("Frame", ints("1-100001")), ("Tile", ints("1-1000"))],
        None,
    );
    let TaskDelta::Product(pd) = task_delta(Some(&old), Some(&new)) else {
        panic!("expected Product")
    };
    let ProductDelta {
        kept,
        added,
        removed,
        ..
    } = *pd;
    assert_eq!(kept.len(), 100_000_000);
    assert_eq!(added.iter().map(TaskBox::len).sum::<u64>(), 1000);
    assert!(removed.is_empty());
}

#[test]
fn association_chunking_and_new_parameters_fall_back() {
    let fb = |d: TaskDelta| assert!(matches!(d, TaskDelta::Fallback { .. }), "{d:?}");
    let a = space(vec![("A", ints("1-3")), ("B", ints("4-6"))], Some("(A, B)"));
    let b = space(vec![("A", ints("1-3")), ("B", ints("4-7"))], Some("(A, B)"));
    fb(task_delta(Some(&a), Some(&b)));

    let chunked = |r: &str| {
        space(
            vec![(
                "F",
                TaskParameter::ChunkInt {
                    range: TaskParamRange::RangeExpr(r.parse().unwrap()),
                    chunks: ResolvedChunks {
                        default_task_count: 3,
                        target_runtime_seconds: None,
                        range_constraint: RangeConstraint::Contiguous,
                    },
                },
            )],
            None,
        )
    };
    fb(task_delta(Some(&chunked("1-9")), Some(&chunked("1-12"))));

    let one = space(vec![("A", ints("1-3"))], None);
    let two = space(vec![("A", ints("1-3")), ("B", ints("1-2"))], None);
    fb(task_delta(Some(&one), Some(&two)));
    fb(task_delta(None, Some(&one)));
}

fn floats(v: &[f64]) -> TaskParameter {
    TaskParameter::Float {
        range: v
            .iter()
            .map(|f| openjd_expr::value::Float64::new(*f).unwrap())
            .collect(),
    }
}

fn paths(v: &[&str]) -> TaskParameter {
    TaskParameter::Path {
        range: v.iter().map(|s| (*s).to_string()).collect(),
    }
}

fn int_list(v: &[i64]) -> TaskParameter {
    TaskParameter::Int {
        range: TaskParamRange::List(v.to_vec()),
        chunks: None,
    }
}

fn is_fallback(d: &TaskDelta) -> bool {
    matches!(d, TaskDelta::Fallback { .. })
}

#[test]
fn float_and_path_ranges_decompose_and_index() {
    check(
        &space(
            vec![
                ("Scale", floats(&[0.5, 1.0, 2.0])),
                ("Out", paths(&["/a", "/b"])),
            ],
            None,
        ),
        &space(
            vec![
                ("Scale", floats(&[1.0, 2.0, 4.0])),
                ("Out", paths(&["/b", "/c"])),
            ],
            None,
        ),
    );
}

#[test]
fn repeated_range_values_fall_back() {
    // The algebra is over sets; a repeated value would make kept/added
    // counts and index_of wrong, so it is refused.
    let a = space(vec![("F", int_list(&[1, 1, 2]))], None);
    let b = space(vec![("F", int_list(&[1, 2]))], None);
    assert!(is_fallback(&task_delta(Some(&a), Some(&b))));
    assert!(is_fallback(&task_delta(Some(&b), Some(&a))));
    let s = space(vec![("C", strs(&["x", "x"]))], None);
    let t = space(vec![("C", strs(&["x"]))], None);
    assert!(is_fallback(&task_delta(Some(&s), Some(&t))));
    // The repeat is in a dimension that did not change.
    let u = space(vec![("C", strs(&["a", "a"])), ("G", ints("1-2"))], None);
    let v = space(vec![("C", strs(&["a", "a"])), ("G", ints("1"))], None);
    assert!(is_fallback(&task_delta(Some(&u), Some(&v))));
    assert!(is_fallback(&task_delta(Some(&v), Some(&u))));
}

#[test]
fn a_retyped_parameter_falls_back() {
    // STRING and PATH values can be spelled the same.
    let a = space(vec![("F", strs(&["a", "b"]))], None);
    let b = space(vec![("F", paths(&["a", "b", "c"]))], None);
    assert!(is_fallback(&task_delta(Some(&a), Some(&b))));
    assert!(is_fallback(&task_delta(Some(&b), Some(&a))));
}

#[test]
fn a_star_combination_reorder_falls_back() {
    let a = space(vec![("X", ints("1-2")), ("Y", ints("3-4"))], Some("*"));
    let b = space(vec![("Y", ints("3-4")), ("X", ints("1-2"))], Some(" * "));
    assert!(is_fallback(&task_delta(Some(&a), Some(&b))));
}

#[test]
fn combination_must_name_each_parameter_once_with_star_only() {
    let p = |c: Option<&str>, b: &str| space(vec![("A", ints("1-2")), ("B", ints(b))], c);
    let fb = |c: &str| {
        assert!(
            is_fallback(&task_delta(
                Some(&p(Some(c), "7-8")),
                Some(&p(Some(c), "7-9"))
            )),
            "{c:?} should fall back"
        );
    };
    fb("A * A");
    fb("");
    fb("(A, B)");
    fb("A * C");
    // A combination that leaves out a definition.
    fb("A");
    // Whitespace is fine.
    assert!(!is_fallback(&task_delta(
        Some(&p(Some(" A *B "), "7-8")),
        Some(&p(Some("A*B"), "7-9")),
    )));
    check(&p(Some(" A *B "), "7-8"), &p(Some("A*B"), "7-9"));
}

#[test]
fn task_count_overflow_falls_back() {
    // 2^16 to the 4th power is 2^64, one past usize::MAX on 64-bit.
    let big = || ints("1-65536");
    let a = space(
        vec![("W", big()), ("X", big()), ("Y", big()), ("Z", big())],
        None,
    );
    let b = space(
        vec![
            ("W", big()),
            ("X", big()),
            ("Y", big()),
            ("Z", ints("1-65537")),
        ],
        None,
    );
    assert!(is_fallback(&task_delta(Some(&a), Some(&b))));
}

#[test]
fn zero_parameters_is_one_task() {
    let a = space(vec![], None);
    let b = space(vec![("F", ints("1-2"))], None);
    assert_eq!(task_delta(Some(&a), Some(&a.clone())), TaskDelta::Unchanged);
    assert!(is_fallback(&task_delta(Some(&a), Some(&b))));
}

#[test]
fn index_of_accepts_path_values_as_well_as_strings() {
    use crate::types::{TaskParameterType, TaskParameterValue};
    let a = space(vec![("Out", paths(&["/a", "/b"]))], None);
    let b = space(vec![("Out", paths(&["/b", "/c"]))], None);
    let TaskDelta::Product(pd) = task_delta(Some(&a), Some(&b)) else {
        panic!("expected Product")
    };
    let mut t = TaskParameterSet::new();
    t.insert(
        "Out".into(),
        TaskParameterValue {
            param_type: TaskParameterType::Path,
            value: openjd_expr::ExprValue::new_path(
                "/b",
                openjd_expr::path_mapping::PathFormat::Posix,
            ),
        },
    );
    assert_eq!(pd.old.index_of(&t), Some(1));
}

#[test]
fn definition_reorder_without_combination_falls_back() {
    // Same task set, different iteration order: every index moves.
    let a = space(vec![("F", ints("1-2")), ("C", strs(&["l", "r"]))], None);
    let b = space(vec![("C", strs(&["l", "r"])), ("F", ints("1-2"))], None);
    assert_eq!(a, b, "IndexMap == ignores order");
    assert!(is_fallback(&task_delta(Some(&a), Some(&b))));
    // With an explicit combination the definition order does not matter.
    let a = space(
        vec![("F", ints("1-2")), ("C", strs(&["l", "r"]))],
        Some("F * C"),
    );
    let b = space(
        vec![("C", strs(&["l", "r"])), ("F", ints("1-2"))],
        Some("F * C"),
    );
    assert_eq!(task_delta(Some(&a), Some(&b)), TaskDelta::Unchanged);
}

#[test]
fn reordering_values_inside_a_range_falls_back() {
    let a = space(vec![("F", ints("1-2")), ("G", strs(&["b", "a"]))], None);
    let b = space(vec![("F", ints("1-2")), ("G", strs(&["a", "b"]))], None);
    assert!(is_fallback(&task_delta(Some(&a), Some(&b))));
    // Appending and removing values while keeping the shared order is fine.
    let c = space(
        vec![("F", ints("1-3")), ("G", strs(&["b", "c", "a"]))],
        None,
    );
    check(&a, &c);
    let d = space(vec![("F", int_list(&[2, 1]))], None);
    let e = space(vec![("F", int_list(&[1, 2]))], None);
    assert!(is_fallback(&task_delta(Some(&d), Some(&e))));
}
