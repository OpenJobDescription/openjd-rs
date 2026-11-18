// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Peak heap use of template validation, measured with a counting global
//! allocator.
//!
//! This is its own test binary so the allocator sees nothing but these
//! decodes, and it holds a single `#[test]` so no other test allocates
//! concurrently.

use openjd_model::{decode_job_template, CallerLimits};
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};

struct Counting;

static CURRENT: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

fn grew(by: usize) {
    let now = CURRENT.fetch_add(by, Relaxed) + by;
    PEAK.fetch_max(now, Relaxed);
}

// SAFETY: every method forwards to `System` with the caller's arguments, and
// only updates counters besides.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // SAFETY: forwarded unchanged; the caller upholds `alloc`'s contract.
        let p = unsafe { System.alloc(layout) };
        if !p.is_null() {
            grew(layout.size());
        }
        p
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: forwarded unchanged; the caller upholds `dealloc`'s contract.
        unsafe { System.dealloc(ptr, layout) };
        CURRENT.fetch_sub(layout.size(), Relaxed);
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        // SAFETY: forwarded unchanged; the caller upholds `realloc`'s contract.
        let p = unsafe { System.realloc(ptr, layout, new_size) };
        if !p.is_null() {
            if new_size >= layout.size() {
                grew(new_size - layout.size());
            } else {
                CURRENT.fetch_sub(layout.size() - new_size, Relaxed);
            }
        }
        p
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

/// Peak bytes allocated while `f` runs, above what was live when it started.
fn peak_during<T>(f: impl FnOnce() -> T) -> usize {
    let base = CURRENT.load(Relaxed);
    PEAK.store(base, Relaxed);
    drop(f());
    PEAK.load(Relaxed) - base
}

/// Size of each large `let` value, in bytes.
const V: usize = 4_000_000;

/// A job template with `steps` steps whose `let` block is `lets`, each with
/// host requirements and a parameterSpace range that read `x`.
fn template(steps: usize, lets: &str, job_env_lets: Option<&str>) -> serde_json::Value {
    let step = |i: usize| {
        serde_json::json!({
            "name": format!("S{i}"),
            "let": serde_json::from_str::<serde_json::Value>(lets).unwrap(),
            "hostRequirements": {
                "amounts": [{"name": "amount.worker.vcpu", "min": "{{ len(x) % 4 + 1 }}"}]
            },
            "parameterSpace": {
                "taskParameterDefinitions": [
                    {"name": "F", "type": "INT", "range": "1-{{ len(x) % 3 + 1 }}"}
                ]
            },
            "script": {"actions": {"onRun": {"command": "echo"}}}
        })
    };
    let mut t = serde_json::json!({
        "specificationVersion": "jobtemplate-2023-09",
        "extensions": ["EXPR", "FEATURE_BUNDLE_1"],
        "name": "M",
        "steps": (0..steps).map(step).collect::<Vec<_>>(),
    });
    if let Some(env_lets) = job_env_lets {
        t["jobEnvironments"] = serde_json::json!([{
            "name": "J",
            "script": {
                "let": serde_json::from_str::<serde_json::Value>(env_lets).unwrap(),
                "actions": {"onEnter": {"command": "echo", "args": ["{{ w }}"]}}
            }
        }]);
    }
    t
}

fn validate(t: serde_json::Value) {
    let _ = decode_job_template(
        t,
        Some(&["EXPR", "FEATURE_BUNDLE_1"]),
        &CallerLimits::default(),
    );
}

/// Template validation holds one step's evaluated `let` values at a time,
/// whether the bindings evaluate or fail, and does not keep earlier steps'
/// errors alive while it evaluates the job environments.
#[test]
fn validation_peak_memory_stays_within_its_bounds() {
    // Measured with these templates, in multiples of V: 2.09, 2.01 and
    // 17.01. Keeping every step's values peaks at 30x in the first case, two
    // copies of a failing step's values at 3x in the second, and step errors
    // alongside the job environment's values at 18x in the third.
    let x = format!(r#""x = 'A' * {V}""#);
    let cases = [
        ("30 steps", template(30, &format!("[{x}]"), None), 2.5),
        (
            "3 steps, a binding fails",
            template(3, &format!(r#"[{x}, "y = int('nope')"]"#), None),
            2.5,
        ),
        (
            "3 steps, large errors, large job environment",
            template(
                3,
                &format!(r#"[{x}, "y = int(x)"]"#),
                Some(&format!(r#"["z = 'B' * {}", "w = len(z)"]"#, 3 * V)),
            ),
            17.5,
        ),
    ];
    let mut report = Vec::new();
    let mut over = Vec::new();
    for (name, t, limit) in cases {
        let peak = peak_during(|| validate(t)) as f64 / V as f64;
        let line = format!("{name}: peak {peak:.2}x the value size (limit {limit}x)");
        if peak > limit {
            over.push(line.clone());
        }
        report.push(line);
    }
    println!("{}", report.join("\n"));
    assert!(
        over.is_empty(),
        "over the limit:\n{}\n\nall cases:\n{}",
        over.join("\n"),
        report.join("\n")
    );
}
