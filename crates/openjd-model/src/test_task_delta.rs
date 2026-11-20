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
    check_host_order(pd, old, new);
    d
}

/// `appended()` and `removed()` against enumeration. Appended: the new
/// tasks not in the old space, grouped by the first dimension whose value
/// the old space lacks, each group in new iteration order. Removed: the old
/// tasks not in the new space, with their old index, ascending.
fn check_host_order(pd: &ProductDelta, old: &StepParameterSpace, new: &StepParameterSpace) {
    let o: BTreeSet<String> = all(old).iter().map(key).collect();
    let n: BTreeSet<String> = all(new).iter().map(key).collect();
    let dims: Vec<&String> = pd.old.space().task_parameter_definitions.keys().collect();
    let value = |t: &TaskParameterSet, d: &str| serde_json::to_string(&t[d].value).unwrap();
    let old_values: Vec<BTreeSet<String>> = dims
        .iter()
        .map(|d| all(old).iter().map(|t| value(t, d)).collect())
        .collect();
    let mut want: Vec<(usize, String)> = all(new)
        .iter()
        .filter(|t| !o.contains(&key(t)))
        .map(|t| {
            let k = dims
                .iter()
                .zip(&old_values)
                .position(|(d, olds)| !olds.contains(&value(t, d)))
                .unwrap();
            (k, key(t))
        })
        .collect();
    want.sort_by_key(|w| w.0);
    let want: Vec<String> = want.into_iter().map(|w| w.1).collect();
    let got: Vec<String> = pd.appended().unwrap().map(|t| key(&t)).collect();
    assert_eq!(got, want, "appended order");
    assert_eq!(pd.appended_count(), u64::try_from(want.len()).unwrap());
    let TaskDelta::Product(again) = task_delta(Some(old), Some(new)) else {
        panic!("a second delta of the same spaces is a product too");
    };
    let again: Vec<String> = again.appended().unwrap().map(|t| key(&t)).collect();
    assert_eq!(again, got, "a second delta yields the same order");

    let want: Vec<(u64, String)> = all(old)
        .iter()
        .enumerate()
        .filter(|(_, t)| !n.contains(&key(t)))
        .map(|(i, t)| (u64::try_from(i).unwrap(), key(t)))
        .collect();
    let got: Vec<(u64, String)> = pd.removed().unwrap().map(|(i, t)| (i, key(&t))).collect();
    assert_eq!(got, want, "removed, ascending old index");
    assert_eq!(pd.removed_count(), u64::try_from(want.len()).unwrap());
}

#[test]
fn appended_and_removed_follow_the_documented_order() {
    // Appended: box 0 (F = 3, any new G), then box 1 (kept F, G = c). The new
    // space iterates (1,c) before (3,b); the host order puts it after.
    let d = check(
        &space(vec![("F", ints("1-2")), ("G", strs(&["a", "b"]))], None),
        &space(vec![("F", ints("1-3")), ("G", strs(&["b", "c"]))], None),
    );
    let TaskDelta::Product(pd) = d else {
        panic!("expected Product");
    };
    let appended: Vec<String> = pd.appended().unwrap().map(|t| key(&t)).collect();
    assert_eq!(
        appended,
        vec![
            r#"F={"Int":3},G={"String":"b"}"#,
            r#"F={"Int":3},G={"String":"c"}"#,
            r#"F={"Int":1},G={"String":"c"}"#,
            r#"F={"Int":2},G={"String":"c"}"#,
        ]
    );
    // Removed: box 0 (F = 2) holds old indices 2 and 3, box 1 (F = 1,
    // G = a) holds 0; the merge returns them ascending.
    let d = check(
        &space(vec![("F", ints("1-2")), ("G", strs(&["a", "b"]))], None),
        &space(vec![("F", ints("1,3")), ("G", strs(&["b"]))], None),
    );
    let TaskDelta::Product(pd) = d else {
        panic!("expected Product");
    };
    let removed: Vec<(u64, String)> = pd.removed().unwrap().map(|(i, t)| (i, key(&t))).collect();
    assert_eq!(
        removed,
        vec![
            (0, r#"F={"Int":1},G={"String":"a"}"#.to_string()),
            (2, r#"F={"Int":2},G={"String":"a"}"#.to_string()),
            (3, r#"F={"Int":2},G={"String":"b"}"#.to_string()),
        ]
    );
    assert_eq!(pd.removed.len(), 2, "two boxes, interleaved by index");
}

#[test]
fn removed_follows_a_combination_that_reorders_dimensions() {
    // Iteration order G then F on both sides: old index is G-major.
    let d = check(
        &space(
            vec![("F", ints("1-3")), ("G", strs(&["a", "b"]))],
            Some("G * F"),
        ),
        &space(vec![("F", ints("2-4")), ("G", strs(&["b"]))], Some("G * F")),
    );
    let TaskDelta::Product(pd) = d else {
        panic!("expected Product");
    };
    let removed: Vec<u64> = pd.removed().unwrap().map(|(i, _)| i).collect();
    assert_eq!(removed, vec![0, 1, 2, 3]);
}

#[test]
fn a_task_parameter_set_document_is_sorted_text() {
    let s = space(
        vec![
            ("Z", ints("7")),
            ("A", strs(&["x"])),
            (
                "M",
                TaskParameter::Float {
                    range: vec![openjd_expr::value::Float64::with_str(1.5, "1.50".into()).unwrap()],
                },
            ),
        ],
        None,
    );
    let t = all(&s).remove(0);
    assert_eq!(
        crate::job::task_delta::task_parameter_set_document(&t).to_string(),
        r#"{"A":{"type":"STRING","value":"x"},"M":{"type":"FLOAT","value":"1.50"},"Z":{"type":"INT","value":"7"}}"#
    );
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

#[test]
fn removed_indices_use_every_dimensions_stride() {
    // Three dimensions: F's stride is 2 * 3 = 6, so removing F = 2 removes
    // old indices 6 to 11. Two dimensions cannot tell `strides[d + 1]` from
    // `strides[d]`.
    let rest = || vec![("G", strs(&["a", "b"])), ("H", strs(&["p", "q", "r"]))];
    let with = |f: &str| {
        let mut v = vec![("F", ints(f))];
        v.extend(rest());
        space(v, None)
    };
    let d = check(&with("1-2"), &with("1"));
    let TaskDelta::Product(pd) = d else {
        panic!("expected Product");
    };
    let removed: Vec<(u64, String)> = pd.removed().unwrap().map(|(i, t)| (i, key(&t))).collect();
    let task =
        |g: &str, h: &str| format!(r#"F={{"Int":2}},G={{"String":"{g}"}},H={{"String":"{h}"}}"#);
    assert_eq!(
        removed,
        vec![
            (6, task("a", "p")),
            (7, task("a", "q")),
            (8, task("a", "r")),
            (9, task("b", "p")),
            (10, task("b", "q")),
            (11, task("b", "r")),
        ]
    );
}
