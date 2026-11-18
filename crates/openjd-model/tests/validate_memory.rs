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

/// One step's shape: its `let` block, and the format strings its host
/// requirements, parameterSpace range, and `onRun` args evaluate.
#[derive(Clone, Copy, Default)]
struct Step<'a> {
    lets: &'a [&'a str],
    hr_min: Option<&'a str>,
    range: Option<&'a str>,
    arg: Option<&'a str>,
}

/// A job template with `steps`, and a job environment with `job_env_lets`
/// when given.
fn template(steps: &[Step], job_env_lets: Option<&[&str]>) -> serde_json::Value {
    let one = |(i, step): (usize, &Step)| {
        let mut s = serde_json::json!({
            "name": format!("S{i}"),
            "script": {"actions": {"onRun": {"command": "echo", "args": step.arg.into_iter().collect::<Vec<_>>()}}}
        });
        if !step.lets.is_empty() {
            s["let"] = serde_json::json!(step.lets);
        }
        if let Some(min) = step.hr_min {
            s["hostRequirements"] =
                serde_json::json!({"amounts": [{"name": "amount.worker.vcpu", "min": min}]});
        }
        if let Some(range) = step.range {
            s["parameterSpace"] = serde_json::json!({
                "taskParameterDefinitions": [{"name": "F", "type": "INT", "range": range}]
            });
        }
        s
    };
    let mut t = serde_json::json!({
        "specificationVersion": "jobtemplate-2023-09",
        "extensions": ["EXPR", "FEATURE_BUNDLE_1"],
        "name": "M",
        "steps": steps.iter().enumerate().map(one).collect::<Vec<_>>(),
    });
    if let Some(env_lets) = job_env_lets {
        t["jobEnvironments"] = serde_json::json!([{
            "name": "J",
            "script": {
                "let": env_lets,
                "actions": {"onEnter": {"command": "echo", "args": ["{{ w }}"]}}
            }
        }]);
    }
    t
}

/// `steps` steps with literal host requirements and the `let` block `lets`,
/// over `params` STRING job parameters.
fn hr_template(steps: usize, params: usize, expr: bool, lets: &[String]) -> serde_json::Value {
    let params: Vec<_> = (0..params)
        .map(|k| serde_json::json!({"name": format!("P{k}"), "type": "STRING", "default": "v"}))
        .collect();
    let steps: Vec<_> = (0..steps)
        .map(|i| {
            let mut s = serde_json::json!({
                "name": format!("S{i}"),
                "hostRequirements": {"amounts": [{"name": "amount.worker.vcpu", "min": 1}]},
                "script": {"actions": {"onRun": {"command": "echo"}}}
            });
            if !lets.is_empty() {
                s["let"] = serde_json::json!(lets);
            }
            s
        })
        .collect();
    let mut t = serde_json::json!({
        "specificationVersion": "jobtemplate-2023-09",
        "name": "M",
        "parameterDefinitions": params,
        "steps": steps,
    });
    if expr {
        t["extensions"] = serde_json::json!(["EXPR", "FEATURE_BUNDLE_1"]);
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

/// Peak heap use of template validation stays within fixed multiples of the
/// large values its templates evaluate, for valid and invalid `let` blocks
/// and for large values at every site that evaluates one.
#[test]
fn validation_peak_memory_stays_within_its_bounds() {
    let x = format!("x = 'A' * {V}");
    let big = format!("len('C' * {})", 10 * V);
    let long_name = format!("{} = 1", "a".repeat(V));
    let no_eq = "b".repeat(V);
    let env_z = format!("z = 'B' * {}", 3 * V);
    let hr_x = Some("{{ len(x) % 4 + 1 }}");
    let range_x = Some("1-{{ len(x) % 3 + 1 }}");
    let range_big = format!("1-{{{{ {big} % 3 + 1 }}}}");
    let hr_big = format!("{{{{ {big} % 4 + 1 }}}}");
    let arg_big = format!("{{{{ {big} }}}}");
    let hr_huge = format!("{{{{ len('C' * {}) % 4 + 1 }}}}", 20 * V);
    let fails = [x.as_str(), "y = int('nope')"];
    let fails_large = [x.as_str(), "y = int(x)"];
    let small_lets: Vec<String> = (0..50).map(|k| format!("v{k} = {k}")).collect();

    // (name, template, limit in multiples of V). Each comment gives the
    // measured peak, which each limit exceeds by about 0.5, and what would
    // raise it, with the peak that caused.
    let cases = [
        // 3.09: origin/main's 2.09 plus one 4 MiB scope kept within
        // RETAINED_SCOPE_BUDGET. Keeping every step's values alive: 31.10.
        (
            "30 steps",
            template(
                &[Step {
                    lets: &[&x],
                    hr_min: hr_x,
                    range: range_x,
                    ..Default::default()
                }; 30],
                None,
            ),
            3.6,
        ),
        // 2.01. Two copies of a failing step's values: 3.00.
        (
            "3 steps, a binding fails",
            template(
                &[Step {
                    lets: &fails,
                    hr_min: hr_x,
                    range: range_x,
                    ..Default::default()
                }; 3],
                None,
            ),
            2.5,
        ),
        // 17.01. Step errors alive while the job environment evaluates: 18.00.
        (
            "3 steps, large errors, large job environment",
            template(
                &[Step {
                    lets: &fails_large,
                    hr_min: hr_x,
                    range: range_x,
                    ..Default::default()
                }; 3],
                Some(&[&env_z, "w = len(z)"]),
            ),
            17.5,
        ),
        // 11.00. Keeping a not-clean scope through the ranges: 14.00.
        (
            "long binding name, large range temporary",
            template(
                &[Step {
                    lets: &[&long_name],
                    range: Some(&range_big),
                    ..Default::default()
                }; 1],
                None,
            ),
            11.5,
        ),
        // 10.00. Keeping a not-clean scope through the ranges: 12.00.
        (
            "binding without '=', large range temporary",
            template(
                &[Step {
                    lets: &[&no_eq],
                    range: Some(&range_big),
                    ..Default::default()
                }; 1],
                None,
            ),
            10.5,
        ),
        // 15.00. Keeping a failed scope through the body's evaluation: 17.00.
        (
            "large errors, range, large body temporary",
            template(
                &[Step {
                    lets: &fails_large,
                    range: Some("1-3"),
                    arg: Some(&arg_big),
                    ..Default::default()
                }; 1],
                None,
            ),
            15.5,
        ),
        // 12.00. A second copy of the errors at host requirements: 13.00.
        (
            "large errors, large host-requirement temporary",
            template(
                &[Step {
                    lets: &fails_large,
                    hr_min: Some(&hr_big),
                    ..Default::default()
                }; 1],
                None,
            ),
            12.5,
        ),
        // 13.00. A step with only a body.
        (
            "large errors, large body temporary",
            template(
                &[Step {
                    lets: &fails_large,
                    arg: Some(&arg_big),
                    ..Default::default()
                }; 1],
                None,
            ),
            13.5,
        ),
        // 14.00. Keeping the binding names through the body: 15.00.
        (
            "long binding name, large body temporary",
            template(
                &[Step {
                    lets: &[&long_name],
                    arg: Some(&arg_big),
                    ..Default::default()
                }],
                None,
            ),
            14.5,
        ),
        // 20.00. Job-environment errors alive while host requirements
        // evaluate: 23.00.
        (
            "large job-environment errors, large host-requirement temporary",
            template(
                &[Step {
                    hr_min: Some(&hr_huge),
                    ..Default::default()
                }],
                Some(&[&env_z, "w = int(z)"]),
            ),
            20.5,
        ),
        // 20.00. An earlier step's errors alive while a later step's host
        // requirements evaluate: 23.00.
        (
            "large errors, then a later step's large host-requirement temporary",
            template(
                &[
                    Step {
                        lets: &fails_large,
                        range: Some("1-3"),
                        ..Default::default()
                    },
                    Step {
                        hr_min: Some(&hr_huge),
                        ..Default::default()
                    },
                ],
                None,
            ),
            20.5,
        ),
        // 0.61. Keeping each step's whole symtab (Param.* entries
        // included) for its body: 3.57.
        (
            "1000 host-requirement steps, 50 parameters, no let",
            hr_template(1000, 50, false, &[]),
            0.9,
        ),
        // 1.47, origin/main 0.52: the kept bindings, within
        // RETAINED_SCOPE_BUDGET (1.05V). Keeping whole symtabs: 2.54.
        (
            "1000 host-requirement steps, 50 small bindings each",
            hr_template(1000, 0, true, &small_lets),
            1.7,
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
