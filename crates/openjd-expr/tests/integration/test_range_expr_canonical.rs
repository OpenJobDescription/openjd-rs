// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! The canonical string form of a range expression and range_expr value
//! equality (Template Schemas §3.4.1.1.1; Expression Language §1.2.3 and
//! §1.2.5).
//!
//! The canonical form depends only on the list of values: over the sorted
//! values, split from the left into runs; a run of three or more values
//! with a constant step is written `x-y` or `x-y:s`, and otherwise one
//! value is written alone.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::time::{Duration, Instant};

use openjd_expr::range_expr::IntRange;
use openjd_expr::{ExprValue, ParsedExpression, RangeExpr, SymbolTable};

fn eval(expr: &str) -> String {
    ParsedExpression::new(expr)
        .and_then(|p| p.evaluate(&SymbolTable::new()))
        .unwrap_or_else(|e| panic!("{expr}: {e}"))
        .to_display_string()
}

fn canonical(text: &str) -> String {
    text.parse::<RangeExpr>()
        .unwrap_or_else(|e| panic!("{text}: {e}"))
        .to_string()
}

/// Run `f` on its own thread and fail if it does not finish within
/// `limit`. An implementation that lists the values of a range with more
/// than 10^18 of them never finishes, and one that streams them allocates
/// nothing, so neither a time check after the work nor the memory limit
/// would stop it; this does. (The abandoned thread ends with the process.)
pub(crate) fn within_deadline<F: FnOnce() + Send + 'static>(limit: Duration, f: F) {
    let (done, finished) = std::sync::mpsc::channel();
    let worker = std::thread::spawn(move || {
        f();
        let _ = done.send(());
    });
    match finished.recv_timeout(limit) {
        Ok(()) => worker.join().unwrap(),
        // `f` panicked: report its panic.
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
            std::panic::resume_unwind(worker.join().unwrap_err())
        }
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
            panic!("did not finish within {limit:?}: is it expanding the values?")
        }
    }
}

fn hash_of<T: Hash>(v: &T) -> u64 {
    let mut h = DefaultHasher::new();
    v.hash(&mut h);
    h.finish()
}

/// The normative definition, over a sorted list of distinct small values.
/// Used as the oracle for the differential test below.
fn reference_canonical(values: &[i64]) -> String {
    let mut out = Vec::new();
    let mut i = 0;
    while i < values.len() {
        if i == values.len() - 1 {
            out.push(values[i].to_string());
            break;
        }
        let s = values[i + 1] - values[i];
        let mut j = i + 1;
        while j + 1 < values.len() && values[j + 1] - values[j] == s {
            j += 1;
        }
        if j - i + 1 >= 3 {
            out.push(if s == 1 {
                format!("{}-{}", values[i], values[j])
            } else {
                format!("{}-{}:{s}", values[i], values[j])
            });
            i = j + 1;
        } else {
            out.push(values[i].to_string());
            i += 1;
        }
    }
    out.join(",")
}

// ── The definition, from strings ──

#[test]
fn canonical_form_spec_table() {
    // Template Schemas §3.4.1.1.1, "List of values" / "Canonical form".
    let cases = [
        ("5", "5"),
        ("1,2", "1,2"),
        ("1,2,3", "1-3"),
        ("1,3,5,7,9", "1-9:2"),
        ("3,7", "3,7"),
        ("1,3,4,5", "1,3-5"),
        ("1,2,4", "1,2,4"),
        ("1,2,3,5,7,9", "1-3,5-9:2"),
        ("2,4,6,8,9,10", "2-8:2,9,10"),
        ("13,14,15,16,17,18,19", "13-19"),
        ("-3,-2,-1", "-3--1"),
        ("-2,-1,3,4,5,7,8,9,12,100", "-2,-1,3-5,7-9,12,100"),
        ("1-10:4", "1-9:4"),
        ("5-1:-1", "1-5"),
        ("19-18:-5,13-18", "13-19"),
        ("13-18,19", "13-19"),
    ];
    for (text, want) in cases {
        assert_eq!(canonical(text), want, "{text}");
    }
}

#[test]
fn canonical_form_conformance_strings() {
    // The string cases of the conformance test
    // expr1.2.3--range-expr-canonical-form.
    let cases = [
        ("1-2", "1,2"),
        ("5-5", "5"),
        ("3-7:4", "3,7"),
        (" 1 - 3 , 7 ", "1-3,7"),
        ("1,3,5-7", "1-5:2,6,7"),
        ("1-3,5,7,9", "1-3,5-9:2"),
        ("-3--1", "-3--1"),
        ("10-15:2,1-5", "1-5,10-14:2"),
    ];
    for (text, want) in cases {
        assert_eq!(canonical(text), want, "{text}");
    }
}

#[test]
fn elements_merge_and_split_across_boundaries() {
    // A run can continue from one element into the next, take only the
    // first value of an element, or leave an element's first value to the
    // run before it.
    let cases = [
        ("1-3,4-6", "1-6"),
        ("1-5:2,7-11:2", "1-11:2"),
        ("1-3,5-9:2", "1-3,5-9:2"),
        ("1-3,5", "1-3,5"),
        ("1-3,5,7", "1-3,5,7"),
        ("1,2,3-9:3", "1-3,6,9"),
        ("1-4,6-10:2", "1-4,6-10:2"),
        ("0-6:3,7,8", "0-6:3,7,8"),
        ("0-6:3,9,10", "0-9:3,10"),
        ("1,2", "1,2"),
        ("1,10-12", "1,10-12"),
    ];
    for (text, want) in cases {
        assert_eq!(canonical(text), want, "{text}");
        let values: Vec<i64> = text.parse::<RangeExpr>().unwrap().iter().collect();
        assert_eq!(reference_canonical(&values), want, "{text} (reference)");
    }
}

#[test]
fn runs_never_hold_two_values() {
    // The representation is the canonical runs: one value, or three or
    // more. "1-2" is two runs of one value.
    let r: RangeExpr = "1-2".parse().unwrap();
    assert_eq!(
        r.ranges(),
        &[
            IntRange::new(1, 1, 1).unwrap(),
            IntRange::new(2, 2, 1).unwrap()
        ]
    );
    let r: RangeExpr = "1,3,5,7,9".parse().unwrap();
    assert_eq!(r.ranges(), &[IntRange::new(1, 9, 2).unwrap()]);
    // A one-value element with a step stores step 1.
    let r: RangeExpr = "5-5:3".parse().unwrap();
    assert_eq!(r.ranges(), &[IntRange::new(5, 5, 1).unwrap()]);
}

// ── From lists, slices, and from_ranges ──

#[test]
fn canonical_form_from_values() {
    let cases: [(&[i64], &str); 7] = [
        (&[1, 3, 5, 7, 9], "1-9:2"),
        (&[1, 3, 4, 5], "1,3-5"),
        (&[1, 2, 3, 5, 7, 9], "1-3,5-9:2"),
        (&[2, 4, 6, 8, 9, 10], "2-8:2,9,10"),
        (&[9, 7, 5, 3, 1], "1-9:2"),
        (&[3, 7], "3,7"),
        (&[5, 5, 1, 1, 3], "1-5:2"),
    ];
    for (values, want) in cases {
        assert_eq!(
            RangeExpr::from_values(values.to_vec()).unwrap().to_string(),
            want,
            "{values:?}"
        );
    }
    assert_eq!(RangeExpr::from_values(Vec::new()).unwrap().to_string(), "");
}

#[test]
fn canonical_form_from_ranges_in_any_order() {
    let r = RangeExpr::from_ranges(vec![
        IntRange::new(13, 18, 1).unwrap(),
        IntRange::new(19, 18, -5).unwrap(),
    ])
    .unwrap();
    assert_eq!(r.to_string(), "13-19");
    assert_eq!(r.ranges(), &[IntRange::new(13, 19, 1).unwrap()]);
}

#[test]
fn slice_results_are_canonical() {
    // "1-10"[::2] selects 1,3,5,7,9 from one run.
    assert_eq!(eval("string(range_expr('1-10')[::2])"), "1-9:2");
    // Selecting two values gives two runs of one value.
    assert_eq!(eval("string(range_expr('1-10')[0:2])"), "1,2");
    // Values selected from different runs join one run.
    assert_eq!(eval("string(range_expr('1,2,3-9:3')[1:4])"), "2,3,6");
    assert_eq!(eval("string(range_expr('1-5,7-11:2')[::2])"), "1-5:2,9");
    assert_eq!(eval("string(range_expr('1-6,8-12:2')[3:])"), "4-6,8-12:2");
    assert_eq!(eval("string(range_expr('0-4,6-14:2')[::2])"), "0-4:2,8,12");
}

// ── Every path that writes a range_expr as a string ──

#[test]
fn every_string_path_uses_the_canonical_form() {
    assert_eq!(eval("string(range_expr('1,3,5,7,9'))"), "1-9:2");
    assert_eq!(eval("'f' + range_expr('1,2,3')"), "f1-3");
    assert_eq!(eval("range_expr('2,4,6,8,9,10') + 'x'"), "2-8:2,9,10x");
    assert_eq!(eval("repr_py(range_expr('13-18,19'))"), "'13-19'");
    assert_eq!(eval("repr_json(range_expr('1,3,5,7,9'))"), "\"1-9:2\"");
    assert_eq!(eval("repr_pwsh(range_expr('1,3,5,7,9'))"), "'1-9:2'");
    assert_eq!(eval("range_expr('19-18:-5,13-18')"), "13-19");
    assert_eq!(eval("string(range_expr([1, 3, 4, 5]))"), "1,3-5");
    // Inside a list, a range_expr element is written in canonical form too.
    assert_eq!(eval("repr_json([range_expr('1,2,3')])"), "[\"1-3\"]");
}

#[test]
fn format_string_interpolation_uses_the_canonical_form() {
    use openjd_expr::format_string::{FormatString, FormatStringOptions};
    let resolve = |text: &str| {
        FormatString::new(text)
            .unwrap()
            .resolve_string_with(&SymbolTable::new(), &FormatStringOptions::default())
            .unwrap()
    };
    assert_eq!(resolve("F={{ range_expr('1,3,5,7,9') }}"), "F=1-9:2");
    assert_eq!(resolve("{{ range_expr([1, 3, 4, 5]) }}"), "1,3-5");
}

#[test]
fn canonical_form_is_idempotent() {
    for text in [
        "1,3,5,7,9",
        "19-18:-5,13-18",
        "2,4,6,8,9,10",
        "1,3,4,5",
        "1-10:4",
        "1,3,5-7",
    ] {
        let once = canonical(text);
        assert_eq!(canonical(&once), once, "{text}");
    }
}

// ── Value equality and Hash ──

#[test]
fn range_expr_equality_compares_values() {
    // The conformance cases of expr1.2.5--range-expr-equality.
    let cases = [
        ("range_expr('1,2,3')", "range_expr('1-3')", true),
        ("range_expr('1,3,5,7,9')", "range_expr('1-9:2')", true),
        (
            "range_expr('13-18,19')",
            "range_expr('19-18:-5,13-18')",
            true,
        ),
        ("range_expr('1-10:4')", "range_expr('1-9:4')", true),
        ("range_expr('1-3')", "range_expr('1-4')", false),
        ("range_expr([5, 1, 3])", "range_expr('1-5:2')", true),
        ("range_expr('1-10')[::2]", "range_expr('1,3,5,7,9')", true),
    ];
    for (a, b, equal) in cases {
        assert_eq!(
            eval(&format!("{a} == {b}")),
            equal.to_string(),
            "{a} == {b}"
        );
        assert_eq!(
            eval(&format!("{a} != {b}")),
            (!equal).to_string(),
            "{a} != {b}"
        );
    }
}

#[test]
fn hash_agrees_with_value_equality() {
    let a: RangeExpr = "1,3,5,7,9".parse().unwrap();
    let b: RangeExpr = "1-9:2".parse().unwrap();
    let c = RangeExpr::from_values(vec![9, 1, 5, 3, 7]).unwrap();
    assert_eq!(a, b);
    assert_eq!(a, c);
    assert_eq!(hash_of(&a), hash_of(&b));
    assert_eq!(hash_of(&a), hash_of(&c));
    assert_eq!(
        hash_of(&ExprValue::RangeExpr(a.clone())),
        hash_of(&ExprValue::RangeExpr(b))
    );
    // The contiguous display flag is not part of the value.
    let flagged = "5-6".parse::<RangeExpr>().unwrap().with_contiguous(true);
    let plain: RangeExpr = "5,6".parse().unwrap();
    assert_eq!(flagged.to_string(), "5-6");
    assert_eq!(plain.to_string(), "5,6");
    assert_eq!(flagged, plain);
    assert_eq!(hash_of(&flagged), hash_of(&plain));
}

#[test]
fn a_value_keeps_memory_for_its_runs_only() {
    // A million consecutive values are one run, however they were built.
    let built = RangeExpr::from_values((0..1_000_000).collect()).unwrap();
    let parsed: RangeExpr = "0-999999".parse().unwrap();
    assert_eq!(built, parsed);
    assert_eq!(built.heap_size(), parsed.heap_size());
}

#[test]
fn hash_distinguishes_different_values() {
    // Hash covers the runs, so different values (almost always) hash
    // differently: a constant hash would agree with == but make hashed
    // collections of range expressions degrade.
    let values: Vec<RangeExpr> = (1..=200)
        .map(|n| format!("1-{n}").parse().unwrap())
        .chain((2..=100).map(|s| format!("1-1000:{s}").parse().unwrap()))
        .collect();
    let hashes: std::collections::HashSet<u64> = values.iter().map(hash_of).collect();
    assert_eq!(hashes.len(), values.len());
}

// ── The CONTIGUOUS chunk form ──

#[test]
fn contiguous_flag_writes_n_dash_m() {
    let r = |s: &str| s.parse::<RangeExpr>().unwrap().with_contiguous(true);
    assert_eq!(r("5").to_string(), "5-5");
    assert_eq!(r("5-6").to_string(), "5-6");
    assert_eq!(r("1-10").to_string(), "1-10");
    // Written in canonical form when the values are not contiguous.
    assert_eq!(r("1,3,5").to_string(), "1-5:2");
    assert_eq!(r("1,2,4").to_string(), "1,2,4");
    // Clearing the flag restores the canonical form.
    assert_eq!(r("5").with_contiguous(false).to_string(), "5");
    // An empty value is written empty, flag or not.
    assert_eq!(
        RangeExpr::from_values(vec![])
            .unwrap()
            .with_contiguous(true)
            .to_string(),
        ""
    );
}

#[test]
fn values_computed_from_a_contiguous_value_are_canonical() {
    let mut st = SymbolTable::new();
    st.set(
        "F",
        ExprValue::RangeExpr("5-6".parse::<RangeExpr>().unwrap().with_contiguous(true)),
    )
    .unwrap();
    let run = |e: &str| {
        ParsedExpression::new(e)
            .and_then(|p| p.evaluate(&st))
            .unwrap()
            .to_display_string()
    };
    assert_eq!(run("string(F)"), "5-6");
    assert_eq!(run("'f' + F"), "f5-6");
    assert_eq!(run("repr_py(F)"), "'5-6'");
    assert_eq!(run("string(F[0:2])"), "5,6");
    assert_eq!(run("string(range_expr(list(F)))"), "5,6");
    assert_eq!(run("string(range_expr(string(F)))"), "5,6");
    // Still the same value as its canonical spelling.
    assert_eq!(run("F == range_expr('5,6')"), "true");
}

// ── Efficiency: never expand the values ──

#[test]
fn huge_ranges_are_canonicalized_without_expanding() {
    // Each has more than 10^18 values. Expanding any of them would hang
    // or exhaust memory, so the deadline catches an implementation that
    // lists the values. These are the conformance cases of
    // expr1.2.3--range-expr-canonical-form-large.
    within_deadline(Duration::from_secs(10), huge_range_cases);
}

fn huge_range_cases() {
    let cases = [
        (
            "0-4000000000000000000:2,4000000000000000001",
            "0-4000000000000000000:2,4000000000000000001",
        ),
        (
            "1-3000000000000000000,3000000000000000001-6000000000000000000",
            "1-6000000000000000000",
        ),
        ("1,3,5-9000000000000000000:2", "1-8999999999999999999:2"),
        (
            "-9223372036854775807--1:2,2-9223372036854775806:2",
            "-9223372036854775807--1:2,2-9223372036854775806:2",
        ),
        (
            "-4000000000000000000-0,1-4000000000000000000",
            "-4000000000000000000-4000000000000000000",
        ),
    ];
    for (text, want) in cases {
        let r: RangeExpr = text.parse().unwrap();
        assert!(r.len() as u64 > 1_000_000_000_000_000_000, "{text}");
        assert_eq!(r.to_string(), want, "{text}");
        assert_eq!(canonical(want), want, "{text} (idempotent)");
    }
    // Equality and slicing are likewise symbolic.
    let a: RangeExpr = "1-3000000000000000000,3000000000000000001-6000000000000000000"
        .parse()
        .unwrap();
    let b: RangeExpr = "1-6000000000000000000".parse().unwrap();
    assert_eq!(a, b);
    assert_eq!(hash_of(&a), hash_of(&b));
    assert_eq!(
        eval("string(range_expr('1-6000000000000000000')[::2])"),
        "1-5999999999999999999:2"
    );
    assert_eq!(
        eval("range_expr('1-3000000000000000000,3000000000000000001-6000000000000000000') == range_expr('1-6000000000000000000')"),
        "true"
    );
}

#[test]
fn i64_endpoints_whose_difference_overflows_i64() {
    // The conformance cases B1 and B2 of
    // expr1.2.3--range-expr-canonical-form-large: the difference between
    // consecutive values is 2^64 - 2, beyond i64.
    assert_eq!(
        canonical("-9223372036854775807,9223372036854775807"),
        "-9223372036854775807,9223372036854775807"
    );
    assert_eq!(
        canonical(
            "-9223372036854775807--9223372036854775806,9223372036854775806-9223372036854775807"
        ),
        "-9223372036854775807,-9223372036854775806,9223372036854775806,9223372036854775807"
    );
    // Three equally spaced values have a step that fits i64.
    assert_eq!(
        canonical("-9223372036854775807,0,9223372036854775807"),
        "-9223372036854775807-9223372036854775807:9223372036854775807"
    );
    assert_eq!(
        RangeExpr::from_values(vec![i64::MAX, -i64::MAX])
            .unwrap()
            .to_string(),
        "-9223372036854775807,9223372036854775807"
    );
    assert_eq!(
        eval("range_expr('-9223372036854775807,9223372036854775807') == range_expr([9223372036854775807, -9223372036854775807])"),
        "true"
    );
}

#[test]
fn slice_of_one_value_whose_spacing_overflows_i64() {
    // Three values -i64::MAX, 0, i64::MAX; slice step 2 spaces the selected
    // values 2 * i64::MAX apart, beyond i64. Selecting one value of them
    // must give that value alone.
    let r: RangeExpr = "-9223372036854775807-9223372036854775807:9223372036854775807"
        .parse()
        .unwrap();
    let one = r.slice(0, 1, 2).unwrap();
    assert_eq!(one.to_vec(), vec![-i64::MAX]);
    assert_eq!(one.to_string(), "-9223372036854775807");
    let two = r.slice(0, 3, 2).unwrap();
    assert_eq!(two.to_vec(), vec![-i64::MAX, i64::MAX]);
    assert_eq!(two.to_string(), "-9223372036854775807,9223372036854775807");
}

#[test]
fn int_range_with_more_than_max_len_values_rejected() {
    // i64::MAX values is the largest count; one more is too many.
    assert_eq!(
        IntRange::new(1, i64::MAX, 1).unwrap().len() as u64,
        openjd_expr::MAX_RANGE_EXPR_LEN
    );
    assert_eq!(
        IntRange::new(0, i64::MAX, 1).unwrap_err().to_string(),
        "Range expression has too many values; the maximum is 9223372036854775807"
    );
    assert_eq!(
        IntRange::new(i64::MAX, 0, -1).unwrap_err().to_string(),
        "Range expression has too many values; the maximum is 9223372036854775807"
    );
}

#[test]
fn many_elements_canonicalize_in_linear_time() {
    // 10,000 elements (the element cap) forming one run: the work is
    // proportional to the number of elements.
    let text = (0..openjd_expr::MAX_RANGE_EXPR_CHUNKS as i64)
        .map(|i| {
            format!(
                "{}-{}",
                i * 1_000_000_000_000,
                i * 1_000_000_000_000 + 999_999_999_999
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    within_deadline(Duration::from_secs(10), move || {
        let started = Instant::now();
        let r: RangeExpr = text.parse().unwrap();
        assert_eq!(r.to_string(), "0-9999999999999999");
        assert_eq!(r.ranges().len(), 1);
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "took {:?}",
            started.elapsed()
        );
    });
}

// ── The element cap applies to the canonical form too ──

#[test]
fn canonical_form_within_the_element_cap_round_trips() {
    // 5,000 two-value elements are 10,000 runs in canonical form: the cap.
    let text = (0..5_000i64)
        .map(|i| format!("{}-{}", 3 * i, 3 * i + 1))
        .collect::<Vec<_>>()
        .join(",");
    let r: RangeExpr = text.parse().unwrap();
    assert_eq!(r.ranges().len(), openjd_expr::MAX_RANGE_EXPR_CHUNKS);
    let printed = r.to_string();
    assert!(printed.starts_with("0,1,3,4,6,7,"), "{}", &printed[..40]);
    let reparsed: RangeExpr = printed.parse().unwrap();
    assert_eq!(reparsed, r);
    assert_eq!(reparsed.to_string(), printed);
    let json = serde_json::to_string(&r).unwrap();
    let deserialized: RangeExpr = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized, r);
}

#[test]
fn canonical_form_beyond_the_element_cap_is_rejected() {
    // 5,001 two-value elements are within the element cap, but their
    // canonical form has 10,002 runs, which would not parse back.
    let text = (0..5_001i64)
        .map(|i| format!("{}-{}", 3 * i, 3 * i + 1))
        .collect::<Vec<_>>()
        .join(",");
    let err = text.parse::<RangeExpr>().unwrap_err().to_string();
    assert_eq!(
        err,
        "Range expression has too many sub-ranges in canonical form (10002); maximum is 10000"
    );
    let ranges = (0..5_001i64)
        .map(|i| IntRange::new(3 * i, 3 * i + 1, 1).unwrap())
        .collect();
    assert_eq!(
        RangeExpr::from_ranges(ranges).unwrap_err().to_string(),
        "Range expression has too many sub-ranges in canonical form (10002); maximum is 10000"
    );
    // So is the serialized form of such a value (built here from values,
    // which are not capped).
    let values = (0..5_001i64).flat_map(|i| [3 * i, 3 * i + 1]).collect();
    let computed = RangeExpr::from_values(values).unwrap();
    assert_eq!(computed.ranges().len(), 10_002);
    let json = serde_json::to_string(&computed).unwrap();
    let err = serde_json::from_str::<RangeExpr>(&json)
        .unwrap_err()
        .to_string();
    assert_eq!(
        err,
        "Range expression has too many sub-ranges (10002); maximum is 10000"
    );
}

// ── Differential test against the definition ──

/// xorshift64*, so the test is deterministic without a dependency.
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

/// A random spelling of a sorted list of distinct values: split it into
/// elements, each a single value or an arithmetic progression, written in
/// a random direction and shuffled.
fn random_spelling(values: &[i64], rng: &mut Rng) -> String {
    let mut elements = Vec::new();
    let mut i = 0;
    while i < values.len() {
        let mut j = i;
        if i + 1 < values.len() && rng.below(10) < 7 {
            let d = values[i + 1] - values[i];
            j = i + 1;
            let limit = i + 1 + rng.below(values.len() as u64) as usize;
            while j + 1 < values.len() && values[j + 1] - values[j] == d && j < limit {
                j += 1;
            }
        }
        let (a, b) = (values[i], values[j]);
        let element = if i == j {
            a.to_string()
        } else {
            let d = (b - a) / (j - i) as i64;
            // An overshooting end (like "1-10:4") spells the same values.
            let pad = rng.below(d as u64) as i64;
            match (rng.below(2), d) {
                (0, 1) => format!("{a}-{}", b),
                (0, _) => format!("{a}-{}:{d}", b + pad),
                (_, _) => format!("{b}-{}:-{d}", a - pad),
            }
        };
        elements.push(element);
        i = j + 1;
    }
    for k in (1..elements.len()).rev() {
        elements.swap(k, rng.below(k as u64 + 1) as usize);
    }
    elements.join(",")
}

#[test]
fn canonical_form_matches_the_definition_on_random_inputs() {
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    for _ in 0..20_000 {
        let n = 1 + rng.below(12) as usize;
        let mut values: Vec<i64> = (0..n).map(|_| rng.below(45) as i64 - 15).collect();
        values.sort_unstable();
        values.dedup();
        let want = reference_canonical(&values);

        let spelled = random_spelling(&values, &mut rng);
        let parsed: RangeExpr = spelled.parse().unwrap_or_else(|e| panic!("{spelled}: {e}"));
        assert_eq!(parsed.to_vec(), values, "{spelled}");
        assert_eq!(parsed.to_string(), want, "{spelled}");

        let mut shuffled = values.clone();
        for k in (1..shuffled.len()).rev() {
            shuffled.swap(k, rng.below(k as u64 + 1) as usize);
        }
        let from_list = RangeExpr::from_values(shuffled).unwrap();
        assert_eq!(from_list.to_string(), want, "{values:?}");
        assert_eq!(from_list, parsed, "{spelled}");
        assert_eq!(hash_of(&from_list), hash_of(&parsed), "{spelled}");

        // Idempotent: the canonical form re-parses to the same value.
        let reparsed: RangeExpr = want.parse().unwrap();
        assert_eq!(reparsed.to_string(), want);
        assert_eq!(reparsed, parsed);

        // A slice is canonical too.
        let (start, step) = (rng.below(4) as i64, 1 + rng.below(3) as i64);
        let sliced = parsed.slice(start, values.len() as i64, step).unwrap();
        let picked: Vec<i64> = values
            .iter()
            .copied()
            .skip(start as usize)
            .step_by(step as usize)
            .collect();
        assert_eq!(sliced.to_vec(), picked, "{spelled}[{start}::{step}]");
        assert_eq!(
            sliced.to_string(),
            reference_canonical(&picked),
            "{spelled}[{start}::{step}]"
        );
    }
}

#[test]
fn different_values_are_unequal_on_random_inputs() {
    let mut rng = Rng(0xD1B5_4A32_D192_ED03);
    for _ in 0..5_000 {
        let pick = |rng: &mut Rng| {
            let mut v: Vec<i64> = (0..1 + rng.below(6))
                .map(|_| rng.below(12) as i64)
                .collect();
            v.sort_unstable();
            v.dedup();
            v
        };
        let (a, b) = (pick(&mut rng), pick(&mut rng));
        let ra = RangeExpr::from_values(a.clone()).unwrap();
        let rb: RangeExpr = random_spelling(&b, &mut rng).parse().unwrap();
        assert_eq!(ra == rb, a == b, "{a:?} vs {b:?}");
    }
}
