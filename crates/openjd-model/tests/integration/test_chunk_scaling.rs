// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Chunking cost and overflow: `CHUNK[INT]` chunks are built from the range
//! expression without listing its values, and an adaptive chunk size of any
//! `usize` is clamped to the values that remain.
//!
//! The huge-range tests run under [`within_deadline`]: an implementation that
//! lists the values of `1-1000000000000` either aborts on the allocation or,
//! streaming them, never finishes, and only a deadline turns the second into
//! a failure.

use std::time::Duration;

use indexmap::IndexMap;
use openjd_expr::{ExprValue, RangeExpr};
use openjd_model::job;
use openjd_model::step_param_space::StepParameterSpaceIterator;
use openjd_model::template::RangeConstraint;
use openjd_model::types::{TaskParameterSet, TaskParameterType, TaskParameterValue};

/// Run `f` on its own thread and fail if it does not finish within `limit`.
/// The same helper as openjd-expr's `test_range_expr_canonical::within_deadline`;
/// integration test binaries cannot share code across crates.
fn within_deadline<F: FnOnce() + Send + 'static>(limit: Duration, f: F) {
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

const DEADLINE: Duration = Duration::from_secs(10);

/// One trillion values, written as text so no test lists them.
const TRILLION: &str = "1-1000000000000";

fn chunk_space(
    range: job::TaskParamRange<i64>,
    default_task_count: usize,
    target_runtime_seconds: Option<usize>,
    range_constraint: RangeConstraint,
) -> job::StepParameterSpace {
    let mut defs = IndexMap::new();
    defs.insert(
        "F".to_string(),
        job::TaskParameter::ChunkInt {
            range,
            chunks: job::ResolvedChunks {
                default_task_count,
                target_runtime_seconds,
                range_constraint,
            },
        },
    );
    job::StepParameterSpace {
        task_parameter_definitions: defs,
        combination: None,
    }
}

fn expr(text: &str) -> job::TaskParamRange<i64> {
    job::TaskParamRange::RangeExpr(text.parse::<RangeExpr>().unwrap())
}

/// The chunk in a task parameter set, with its contiguous flag shown by
/// `Display`.
fn chunk_of(set: &TaskParameterSet) -> String {
    match &set["F"].value {
        ExprValue::RangeExpr(r) => r.to_string(),
        other => panic!("expected a chunk, got {other:?}"),
    }
}

fn chunk_param(text: &str) -> TaskParameterSet {
    let mut set = TaskParameterSet::new();
    set.insert(
        "F".to_string(),
        TaskParameterValue {
            param_type: TaskParameterType::ChunkInt,
            value: ExprValue::RangeExpr(text.parse::<RangeExpr>().unwrap()),
        },
    );
    set
}

// ── Adaptive chunk size of usize::MAX ──

#[test]
fn adaptive_noncontiguous_chunk_size_of_usize_max_takes_the_rest_of_a_list() {
    let space = chunk_space(
        job::TaskParamRange::List((1..=10).collect()),
        2,
        Some(60),
        RangeConstraint::Noncontiguous,
    );
    let mut iter = StepParameterSpaceIterator::new(&space).unwrap();
    assert_eq!(chunk_of(&iter.next().unwrap()), "1,2");
    iter.set_chunks_default_task_count(usize::MAX);
    assert_eq!(chunk_of(&iter.next().unwrap()), "3-10");
    assert!(iter.next().is_none());
}

#[test]
fn adaptive_contiguous_chunk_size_of_usize_max_takes_the_rest_of_the_interval() {
    let space = chunk_space(expr("1-10,20-25"), 2, Some(60), RangeConstraint::Contiguous);
    let mut iter = StepParameterSpaceIterator::new(&space).unwrap();
    assert_eq!(chunk_of(&iter.next().unwrap()), "1-2");
    iter.set_chunks_default_task_count(usize::MAX);
    let rest: Vec<String> = iter.map(|s| chunk_of(&s)).collect();
    assert_eq!(rest, ["3-10", "20-25"]);
}

#[test]
fn adaptive_noncontiguous_chunk_size_of_usize_max_on_a_huge_range() {
    within_deadline(DEADLINE, || {
        let space = chunk_space(expr(TRILLION), 2, Some(60), RangeConstraint::Noncontiguous);
        let mut iter = StepParameterSpaceIterator::new(&space).unwrap();
        assert_eq!(chunk_of(&iter.next().unwrap()), "1,2");
        iter.set_chunks_default_task_count(usize::MAX);
        assert_eq!(chunk_of(&iter.next().unwrap()), "3-1000000000000");
        assert!(iter.next().is_none());
    });
}

// ── Huge ranges: no chunking path lists the values ──

#[test]
fn adaptive_noncontiguous_chunks_of_a_huge_range_are_built_lazily() {
    within_deadline(DEADLINE, || {
        let space = chunk_space(
            expr("1-1000000000000:3"),
            4,
            Some(60),
            RangeConstraint::Noncontiguous,
        );
        let mut iter = StepParameterSpaceIterator::new(&space).unwrap();
        assert_eq!(chunk_of(&iter.next().unwrap()), "1-10:3");
        iter.set_chunks_default_task_count(100_000_000_000);
        assert_eq!(chunk_of(&iter.next().unwrap()), "13-300000000010:3");
        // The containment check of a chunk is a subset test on the runs.
        assert!(iter.contains(&chunk_param("13-300000000010:3")));
        assert!(iter.contains(&chunk_param("1-999999999997:3")));
        assert!(!iter.contains(&chunk_param("1-999999999997:2")));
        assert!(!iter.contains(&chunk_param("1-1000000000003:3")));
    });
}

#[test]
fn adaptive_contiguous_chunks_of_a_huge_range_are_built_lazily() {
    within_deadline(DEADLINE, || {
        let space = chunk_space(expr(TRILLION), 3, Some(60), RangeConstraint::Contiguous);
        let mut iter = StepParameterSpaceIterator::new(&space).unwrap();
        assert_eq!(chunk_of(&iter.next().unwrap()), "1-3");
        iter.set_chunks_default_task_count(999_999_999_990);
        assert_eq!(chunk_of(&iter.next().unwrap()), "4-999999999993");
        assert_eq!(
            chunk_of(&iter.next().unwrap()),
            "999999999994-1000000000000"
        );
        assert!(iter.next().is_none());
        assert!(iter.contains(&chunk_param(TRILLION)));
    });
}

#[test]
fn static_noncontiguous_chunks_of_a_huge_range_are_built_lazily() {
    within_deadline(DEADLINE, || {
        let space = chunk_space(
            expr(TRILLION),
            500_000_000_000,
            None,
            RangeConstraint::Noncontiguous,
        );
        let iter = StepParameterSpaceIterator::new(&space).unwrap();
        assert_eq!(iter.len(), 2);
        assert_eq!(
            chunk_of(&iter.get(1).unwrap()),
            "500000000001-1000000000000"
        );
        let chunks: Vec<String> = iter.map(|s| chunk_of(&s)).collect();
        assert_eq!(chunks, ["1-500000000000", "500000000001-1000000000000"]);
    });
}

#[test]
fn static_noncontiguous_containment_of_a_huge_range_locates_the_chunk() {
    // A trillion chunks: checking a chunk must not compare it with each.
    within_deadline(DEADLINE, || {
        let space = chunk_space(expr(TRILLION), 1, None, RangeConstraint::Noncontiguous);
        let iter = StepParameterSpaceIterator::new(&space).unwrap();
        assert_eq!(iter.len(), 1_000_000_000_000);
        assert!(iter.contains(&chunk_param("999999999999")));
        assert!(!iter.contains(&chunk_param("999999999999-1000000000000")));
        assert_eq!(
            iter.validate_containment(&chunk_param("0")).unwrap_err(),
            "Parameter 'F' value '0' is not a valid chunk in the parameter space."
        );
    });
}

#[test]
fn static_contiguous_chunks_of_a_huge_stepped_range_are_counted_lazily() {
    // Every value of a stepped range is its own interval: 5e11 intervals.
    within_deadline(DEADLINE, || {
        let space = chunk_space(
            expr("1-1000000000000:2"),
            1,
            None,
            RangeConstraint::Contiguous,
        );
        let iter = StepParameterSpaceIterator::new(&space).unwrap();
        assert_eq!(iter.len(), 500_000_000_000);
        assert_eq!(chunk_of(&iter.get(0).unwrap()), "1-1");
        assert_eq!(
            chunk_of(&iter.get(499_999_999_999).unwrap()),
            "999999999999-999999999999"
        );
        assert!(iter.contains(&chunk_param("999999999999")));
        assert!(!iter.contains(&chunk_param("1000000000000")));
        let first: Vec<String> = iter.take(2).map(|s| chunk_of(&s)).collect();
        assert_eq!(first, ["1-1", "3-3"]);
    });
}

#[test]
fn static_contiguous_containment_of_a_huge_interval_locates_the_chunk() {
    within_deadline(DEADLINE, || {
        let space = chunk_space(expr(TRILLION), 3, None, RangeConstraint::Contiguous);
        let iter = StepParameterSpaceIterator::new(&space).unwrap();
        let last = iter.get(iter.len() - 1).unwrap();
        assert_eq!(chunk_of(&last), "999999999999-1000000000000");
        assert!(iter.contains(&last));
        assert!(!iter.contains(&chunk_param("999999999998-1000000000000")));
    });
}

// ── Differential test against the value-based construction ──
//
// Chunks used to be built by listing the range's values and rebuilding each
// chunk from them. The oracle below is that construction, over ranges small
// enough to list; the iterator must agree with it on every chunk string
// (with the contiguous flag, as `Display` writes it), the chunk count,
// random access, containment, and the run-cap behavior.

/// xorshift64*, so the test is deterministic without a dependency (as in
/// openjd-expr's `test_range_expr_canonical`).
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

/// Sorted distinct values whose gaps mix 1 (intervals), small steps
/// (stepped runs and their boundaries), and large jumps.
fn random_values(rng: &mut Rng) -> Vec<i64> {
    let n = 1 + rng.below(80) as usize;
    let mut v = -40 + rng.below(80) as i64;
    let mut values = Vec::with_capacity(n);
    while values.len() < n {
        values.push(v);
        let gap = match rng.below(10) {
            0..=4 => 1,
            5..=7 => 2 + rng.below(3) as i64,
            8 => 1 + rng.below(40) as i64,
            _ => 1_000 + rng.below(1_000_000) as i64,
        };
        // A stepped run: repeat the same gap a few times.
        let repeat = if rng.below(4) == 0 {
            1 + rng.below(6)
        } else {
            1
        };
        for _ in 1..repeat {
            if values.len() < n {
                v += gap;
                values.push(v);
            }
        }
        v += gap;
    }
    values
}

/// Static NONCONTIGUOUS chunks: `ceil(n / dtc)` windows of consecutive
/// values, the first `n % chunks` one value larger.
fn oracle_static_noncontiguous(values: &[i64], dtc: usize) -> Result<Vec<String>, String> {
    let chunks = values.len().div_ceil(dtc);
    let (small, leftovers) = (values.len() / chunks, values.len() % chunks);
    (0..chunks)
        .map(|i| {
            let size = small + usize::from(i < leftovers);
            let offset = i * small + i.min(leftovers);
            let window = &values[offset..offset + size];
            RangeExpr::from_values(window.to_vec())
                .map(|r| r.to_string())
                .map_err(|e| {
                    let (lo, hi) = (window.iter().min().unwrap(), window.iter().max().unwrap());
                    format!("Validation error: Task parameter 'F': chunk {i} (values {lo} to {hi}): {e}")
                })
        })
        .collect()
}

/// The contiguous intervals of a list, in list order, as value slices.
fn oracle_intervals(values: &[i64]) -> Vec<&[i64]> {
    let mut out = Vec::new();
    let mut start = 0;
    for i in 1..=values.len() {
        if i == values.len() || values[i] != values[i - 1] + 1 {
            out.push(&values[start..i]);
            start = i;
        }
    }
    out
}

/// Static CONTIGUOUS chunks, by the reference's `divide_chunk_sizes`:
/// `chunk_sizes[(i * chunk_count) // leftovers] += 1`.
fn oracle_static_contiguous(values: &[i64], dtc: usize) -> Vec<String> {
    let mut out = Vec::new();
    for interval in oracle_intervals(values) {
        let len = interval.len();
        let count = len.div_ceil(dtc);
        let (small, leftovers) = (len / count, len % count);
        let mut sizes = vec![small; count];
        for i in 0..leftovers {
            sizes[(i * count) / leftovers] += 1;
        }
        let mut offset = 0;
        for size in sizes {
            out.push(format!(
                "{}-{}",
                interval[offset],
                interval[offset + size - 1]
            ));
            offset += size;
        }
    }
    out
}

/// Adaptive chunks for a schedule of chunk sizes, one per chunk (the last
/// repeats). NONCONTIGUOUS: the next `size` values, shortened to
/// 50,000 values when their canonical form has too many runs. CONTIGUOUS:
/// up to `size` values from the current interval.
fn oracle_adaptive(
    values: &[i64],
    schedule: &[usize],
    constraint: &RangeConstraint,
) -> Vec<String> {
    let mut out = Vec::new();
    let mut cursor = 0;
    while cursor < values.len() {
        let size = schedule[out.len().min(schedule.len() - 1)];
        let end = cursor.saturating_add(size).min(values.len());
        match constraint {
            RangeConstraint::Noncontiguous => {
                // A list may repeat a value, so the window size, not the
                // chunk's length, is what the cursor advances by.
                let (chunk, stop) = match RangeExpr::from_values(values[cursor..end].to_vec()) {
                    Ok(chunk) => (chunk, end),
                    Err(_) => {
                        let stop = cursor + 50_000;
                        (
                            RangeExpr::from_values(values[cursor..stop].to_vec()).unwrap(),
                            stop,
                        )
                    }
                };
                cursor = stop;
                out.push(chunk.to_string());
            }
            RangeConstraint::Contiguous => {
                let mut stop = cursor + 1;
                while stop < end && values[stop] == values[stop - 1] + 1 {
                    stop += 1;
                }
                out.push(format!("{}-{}", values[cursor], values[stop - 1]));
                cursor = stop;
            }
        }
    }
    out
}

/// Walk an adaptive iterator, setting the chunk size before each chunk.
fn walk_adaptive(space: &job::StepParameterSpace, schedule: &[usize]) -> Vec<String> {
    let mut iter = StepParameterSpaceIterator::new(space).unwrap();
    assert!(iter.chunks_adaptive());
    let mut out = Vec::new();
    loop {
        iter.set_chunks_default_task_count(schedule[out.len().min(schedule.len() - 1)]);
        match iter.next() {
            Some(set) => out.push(chunk_of(&set)),
            None => return out,
        }
    }
}

/// Every chunk the iterator yields, plus a few sets of values that are not
/// chunks, checked for containment against `is_chunk`.
fn check_containment(
    iter: &StepParameterSpaceIterator,
    values: &[i64],
    chunks: &[String],
    is_chunk: impl Fn(&RangeExpr) -> bool,
    rng: &mut Rng,
) {
    let mut candidates: Vec<RangeExpr> = chunks.iter().map(|c| c.parse().unwrap()).collect();
    for _ in 0..4 {
        let a = rng.below(values.len() as u64) as usize;
        let b = a + 1 + rng.below((values.len() - a) as u64) as usize;
        candidates.push(RangeExpr::from_values(values[a..b].to_vec()).unwrap());
        let v = values[a] + rng.below(5) as i64 - 2;
        candidates.push(RangeExpr::from_values(vec![v]).unwrap());
        candidates.push(RangeExpr::from_values(vec![values[a], values[b - 1] + 1]).unwrap());
    }
    for candidate in candidates {
        let mut set = TaskParameterSet::new();
        set.insert(
            "F".to_string(),
            TaskParameterValue {
                param_type: TaskParameterType::ChunkInt,
                value: ExprValue::RangeExpr(candidate.clone()),
            },
        );
        assert_eq!(
            iter.contains(&set),
            is_chunk(&candidate),
            "containment of {candidate} in {values:?}"
        );
    }
}

#[test]
fn chunks_agree_with_the_value_based_construction() {
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    for case in 0..1_500 {
        let values = random_values(&mut rng);
        let text = RangeExpr::from_values(values.clone()).unwrap().to_string();
        let n = values.len();
        let dtc = 1 + rng.below(n as u64 + 3) as usize;
        let mut schedule: Vec<usize> = (0..4)
            .map(|_| match rng.below(6) {
                0 => usize::MAX,
                1 => 1,
                _ => 1 + rng.below(n as u64 + 3) as usize,
            })
            .collect();
        schedule.push(usize::MAX - rng.below(3) as usize);
        let ranges = [
            (expr(&text), values.clone()),
            (job::TaskParamRange::List(values.clone()), values.clone()),
        ];
        for (range, listed) in ranges {
            let ctx = format!("case {case}: {range:?} dtc={dtc} schedule={schedule:?}");

            // Static NONCONTIGUOUS.
            let space = chunk_space(range.clone(), dtc, None, RangeConstraint::Noncontiguous);
            let want = oracle_static_noncontiguous(&listed, dtc).unwrap();
            let iter = StepParameterSpaceIterator::new(&space).unwrap();
            assert_eq!(iter.len(), want.len(), "{ctx}");
            let got: Vec<String> = StepParameterSpaceIterator::new(&space)
                .unwrap()
                .map(|s| chunk_of(&s))
                .collect();
            assert_eq!(got, want, "{ctx}");
            let by_index: Vec<String> = (0..iter.len())
                .map(|i| chunk_of(&iter.get(i).unwrap()))
                .collect();
            assert_eq!(by_index, want, "{ctx}");
            let set: Vec<RangeExpr> = want.iter().map(|c| c.parse().unwrap()).collect();
            check_containment(&iter, &listed, &want, |r| set.contains(r), &mut rng);

            // Static CONTIGUOUS.
            let space = chunk_space(range.clone(), dtc, None, RangeConstraint::Contiguous);
            let want = oracle_static_contiguous(&listed, dtc);
            let iter = StepParameterSpaceIterator::new(&space).unwrap();
            assert_eq!(iter.len(), want.len(), "{ctx}");
            let got: Vec<String> = StepParameterSpaceIterator::new(&space)
                .unwrap()
                .map(|s| chunk_of(&s))
                .collect();
            assert_eq!(got, want, "{ctx}");
            let by_index: Vec<String> = (0..iter.len())
                .map(|i| chunk_of(&iter.get(i).unwrap()))
                .collect();
            assert_eq!(by_index, want, "{ctx}");
            let set: Vec<RangeExpr> = want.iter().map(|c| c.parse().unwrap()).collect();
            check_containment(&iter, &listed, &want, |r| set.contains(r), &mut rng);

            // Adaptive, both constraints; containment is a subset test.
            for constraint in [RangeConstraint::Noncontiguous, RangeConstraint::Contiguous] {
                let space = chunk_space(range.clone(), dtc, Some(60), constraint.clone());
                let want = oracle_adaptive(&listed, &schedule, &constraint);
                assert_eq!(
                    walk_adaptive(&space, &schedule),
                    want,
                    "{ctx} {constraint:?}"
                );
                let iter = StepParameterSpaceIterator::new(&space).unwrap();
                check_containment(
                    &iter,
                    &listed,
                    &want,
                    |r| r.iter().all(|v| listed.contains(&v)),
                    &mut rng,
                );
            }
        }
    }
}

#[test]
fn unsorted_list_chunks_agree_with_the_value_based_construction() {
    // A list is chunked in list order, so its intervals and windows follow
    // that order, and it may repeat a value.
    let mut rng = Rng(0xD1B5_4A32_D192_ED03);
    for case in 0..1_000 {
        let n = 1 + rng.below(40) as usize;
        let values: Vec<i64> = (0..n).map(|_| rng.below(30) as i64 - 10).collect();
        let dtc = 1 + rng.below(n as u64 + 2) as usize;
        let range = job::TaskParamRange::List(values.clone());
        let ctx = format!("case {case}: {values:?} dtc={dtc}");
        let walk = |constraint: RangeConstraint| -> Vec<String> {
            let space = chunk_space(range.clone(), dtc, None, constraint);
            StepParameterSpaceIterator::new(&space)
                .unwrap()
                .map(|s| chunk_of(&s))
                .collect()
        };
        assert_eq!(
            walk(RangeConstraint::Noncontiguous),
            oracle_static_noncontiguous(&values, dtc).unwrap(),
            "{ctx}"
        );
        assert_eq!(
            walk(RangeConstraint::Contiguous),
            oracle_static_contiguous(&values, dtc),
            "{ctx}"
        );
        for constraint in [RangeConstraint::Noncontiguous, RangeConstraint::Contiguous] {
            let schedule = [dtc, 1 + rng.below(n as u64) as usize, usize::MAX];
            let space = chunk_space(range.clone(), dtc, Some(60), constraint.clone());
            assert_eq!(
                walk_adaptive(&space, &schedule),
                oracle_adaptive(&values, &schedule, &constraint),
                "{ctx} {constraint:?}"
            );
        }
    }
}

/// 50,000 runs: `0-2`, then 49,998 isolated values, then a run of three far
/// away (about 50,004 values). Cutting a value off either end leaves two
/// values of a run of three, written as two runs.
fn cap_runs_values() -> Vec<i64> {
    let mut values = vec![0, 1, 2];
    values.extend((1..=49_998i64).map(|k| 100 + k * k));
    let far = values[values.len() - 1] + 1_000_000_000;
    values.extend([far, far + 1, far + 2]);
    values
}

#[test]
fn run_cap_behavior_agrees_with_the_value_based_construction() {
    let values = cap_runs_values();
    let n = values.len();
    let text = RangeExpr::from_values(values.clone()).unwrap().to_string();
    assert_eq!(text.parse::<RangeExpr>().unwrap().ranges().len(), 50_000);
    for range in [expr(&text), job::TaskParamRange::List(values.clone())] {
        // Static: a chunk over the cap fails construction, naming the chunk;
        // otherwise every chunk is built.
        for dtc in [n - 1, n - 2, n, 50_000, 50_001, 30_000, 25_002] {
            let space = chunk_space(range.clone(), dtc, None, RangeConstraint::Noncontiguous);
            let got = StepParameterSpaceIterator::new(&space)
                .map(|iter| iter.map(|s| chunk_of(&s)).collect::<Vec<_>>())
                .map_err(|e| e.to_string());
            assert_eq!(got, oracle_static_noncontiguous(&values, dtc), "dtc={dtc}");
        }
        // Adaptive: a chunk over the cap is shortened to 50,000 values.
        for schedule in [
            vec![n - 1],
            vec![1, n],
            vec![2, usize::MAX],
            vec![50_001],
            vec![50_000],
            vec![usize::MAX],
        ] {
            let space = chunk_space(range.clone(), 7, Some(60), RangeConstraint::Noncontiguous);
            assert_eq!(
                walk_adaptive(&space, &schedule),
                oracle_adaptive(&values, &schedule, &RangeConstraint::Noncontiguous),
                "schedule={schedule:?}"
            );
        }
    }
}
