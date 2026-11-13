// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! `job::template_diff` rendering over random template pairs: two sides with
//! different values never show the same text, no text is longer than 120
//! characters, and no text shows a value's serde form
//! (`specs/model/template-diff.md` § Rendering).

use openjd_model::job::diff::DiffOptions;
use openjd_model::job::template_diff::*;
use openjd_model::job::Job;

use super::test_template_diff::SERDE_INTERNALS;

/// Template pairs generated; each is diffed once by `diff_templates` and at
/// every depth, unlimited and 0 to 5, by `diff_built_jobs`.
const CASES: usize = 800;

/// SplitMix64, so the cases are the same on every run and platform.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }

    fn pick<'a, T>(&mut self, pool: &'a [T]) -> &'a T {
        &pool[self.below(pool.len())]
    }
}

/// The parts a template is made of. Each pool mixes values that render
/// alike (`4` and `4.0`, a range and the list it expands to, strings that
/// share a 150-character prefix) with ones that do not.
struct Pools {
    params: Vec<Option<(&'static str, String)>>,
    task_params: Vec<Option<String>>,
    descriptions: Vec<Option<String>>,
    dependencies: Vec<Option<&'static str>>,
    hosts: Vec<Option<&'static str>>,
    environments: Vec<Option<String>>,
    args: Vec<Option<String>>,
    files: Vec<Option<&'static str>>,
}

fn pools() -> Pools {
    let long = |c: char, end: &str| format!("{}{end}", c.to_string().repeat(150));
    let list = |n: i64| {
        (1..=n)
            .map(|i| i.to_string())
            .collect::<Vec<_>>()
            .join(", ")
    };
    Pools {
        params: vec![
            None,
            Some(("STRING", long('p', "1"))),
            Some(("STRING", long('p', "2"))),
            Some(("PATH", format!("//srv/{}", long('p', "1")))),
            Some(("PATH", format!("//srv/{}", long('p', "2")))),
            Some(("PATH", "//x/1".into())),
            Some(("STRING", "//x/1".into())),
            Some(("STRING", "4".into())),
            Some(("STRING", "4.0".into())),
            Some(("INT", "4".into())),
            Some(("FLOAT", "4".into())),
            Some(("FLOAT", "4.0".into())),
            Some(("FLOAT", "4.5".into())),
            Some(("STRING", "a\nb".into())),
            Some(("STRING", "a\r\nb".into())),
            Some(("STRING", " a".into())),
            Some(("STRING", "a -> b".into())),
            Some(("STRING", "".into())),
            Some(("BOOL", "true".into())),
            Some(("\"LIST[INT]\"", "[1, 2]".into())),
            Some(("RANGE_EXPR", "1-3".into())),
        ],
        task_params: vec![
            None,
            Some("{name: P, type: INT, range: \"1-60\"}".into()),
            Some(format!("{{name: P, type: INT, range: [{}]}}", list(60))),
            Some("{name: P, type: INT, range: \"1-2\"}".into()),
            Some("{name: P, type: INT, range: [1, 2]}".into()),
            Some("{name: P, type: INT, range: [2, 1]}".into()),
            Some(format!("{{name: P, type: STRING, range: [{}]}}", long('q', "1"))),
            Some(format!("{{name: P, type: STRING, range: [{}]}}", long('q', "2"))),
            Some("{name: P, type: STRING, range: [\"4\"]}".into()),
            Some("{name: P, type: STRING, range: [a]}".into()),
            Some("{name: P, type: PATH, range: [a]}".into()),
            Some("{name: P, type: FLOAT, range: [4]}".into()),
            Some("{name: P, type: FLOAT, range: [4.0]}".into()),
            Some("{name: P, type: FLOAT, range: [4.5]}".into()),
            Some("{name: P, type: \"CHUNK[INT]\", range: \"1-2\", chunks: {defaultTaskCount: 1, rangeConstraint: CONTIGUOUS}}".into()),
            Some("{name: P, type: \"CHUNK[INT]\", range: [1, 2], chunks: {defaultTaskCount: 1, rangeConstraint: CONTIGUOUS}}".into()),
        ],
        descriptions: vec![
            None,
            Some("d".into()),
            Some(long('d', "1")),
            Some(long('d', "2")),
        ],
        dependencies: vec![None, Some("[{dependsOn: A}]")],
        hosts: vec![
            None,
            Some("{amounts: [{name: amount.worker.vcpu, min: 4}]}"),
            Some("{amounts: [{name: amount.worker.vcpu, min: 4.0}]}"),
            Some("{amounts: [{name: amount.worker.vcpu, min: 4.5}]}"),
            Some("{attributes: [{name: attr.worker.os.family, anyOf: [linux]}]}"),
            Some("{attributes: [{name: attr.worker.os.family, anyOf: [windows]}]}"),
        ],
        environments: vec![
            None,
            Some(format!("{{name: E, variables: {{V: {}}}}}", long('v', "1"))),
            Some(format!("{{name: E, variables: {{V: {}}}}}", long('v', "2"))),
            Some(format!(
                "{{name: E, description: d, variables: {{V: {}}}}}",
                long('v', "1")
            )),
            Some("{name: E, variables: {V: \"4\"}}".into()),
        ],
        args: vec![None, Some(long('a', "1")), Some(long('a', "2")), Some("4".into())],
        files: vec![
            None,
            Some("{name: F, type: TEXT, data: hi}"),
            Some("{name: G, type: TEXT, data: hi}"),
            Some("{name: F, type: TEXT, data: ho}"),
        ],
    }
}

/// One side: an index into each pool.
#[derive(Clone)]
struct Side([usize; 8]);

impl Side {
    fn random(rng: &mut Rng, sizes: &[usize; 8]) -> Self {
        Self(sizes.map(|n| rng.below(n)))
    }

    /// The other side: each part kept with probability 1/2, else redrawn.
    fn vary(&self, rng: &mut Rng, sizes: &[usize; 8]) -> Self {
        let mut s = self.clone();
        for (i, n) in sizes.iter().enumerate() {
            if rng.below(2) == 0 {
                s.0[i] = rng.below(*n);
            }
        }
        s
    }

    fn input(&self, p: &Pools) -> TemplateInput {
        let [param, task, desc, deps, host, env, arg, file] = self.0;
        let mut t = String::from(
            "specificationVersion: jobtemplate-2023-09\nextensions: [EXPR, TASK_CHUNKING]\nname: J\n",
        );
        if let Some((ty, _)) = &p.params[param] {
            t += &format!("parameterDefinitions:\n  - {{name: X, type: {ty}}}\n");
        }
        if let Some(e) = &p.environments[env] {
            t += &format!("jobEnvironments:\n  - {e}\n");
        }
        t += "steps:\n  - name: A\n    script: {actions: {onRun: {command: a}}}\n  - name: S\n";
        if let Some(d) = &p.descriptions[desc] {
            t += &format!("    description: {d}\n");
        }
        if let Some(d) = p.dependencies[deps] {
            t += &format!("    dependencies: {d}\n");
        }
        if let Some(h) = p.hosts[host] {
            t += &format!("    hostRequirements: {h}\n");
        }
        let mut args = Vec::new();
        if let Some(def) = &p.task_params[task] {
            t +=
                &format!("    parameterSpace:\n      taskParameterDefinitions:\n        - {def}\n");
            args.push("\"{{Task.Param.P}}\"".to_string());
        }
        if let Some(a) = &p.args[arg] {
            args.push(format!("\"{a}\""));
        }
        t += &format!(
            "    script:\n      actions: {{onRun: {{command: a, args: [{}]}}}}\n",
            if args.is_empty() {
                "b".to_string()
            } else {
                args.join(", ")
            }
        );
        if let Some(f) = p.files[file] {
            t += &format!("      embeddedFiles: [{f}]\n");
        }
        let input = TemplateInput::new(TemplateSource::Text(t));
        match &p.params[param] {
            Some((_, v)) => input.param("X", v.as_str()),
            None => input,
        }
    }
}

/// Hand edits a `job_from_str` document can make that no template produces.
const EDITS: [(&str, &str); 3] = [
    ("\"chunkInt\"", "\"int\""),
    ("\"description\":\"d\"", "\"description\":null"),
    ("\"min\":4.0", "\"min\":4"),
];

fn check(d: &TemplateDiff, what: &dyn Fn() -> String) {
    for c in &d.changes {
        for s in [&c.before, &c.after].into_iter().flatten() {
            for bad in SERDE_INTERNALS {
                assert!(
                    !s.text.contains(bad),
                    "{bad} in {}\n{c}\n{}",
                    s.text,
                    what()
                );
            }
            assert!(
                s.text.chars().count() <= 120,
                "{} chars: {}\n{c}\n{}",
                s.text.chars().count(),
                s.text,
                what()
            );
        }
        if let (Some(b), Some(a)) = (&c.before, &c.after) {
            assert!(
                b.raw == a.raw || b.text != a.text,
                "different values, same text: {c}\n{}",
                what()
            );
        }
    }
}

fn diff_at_every_depth(old: &Job, new: &Job, what: &dyn Fn() -> String) {
    for depth in [None, Some(0), Some(1), Some(2), Some(3), Some(4), Some(5)] {
        let options = match depth {
            Some(n) => DiffOptions::default().with_max_depth(n),
            None => DiffOptions::default(),
        };
        let d = diff_built_jobs(old.clone(), new.clone(), &options);
        check(&d, &|| {
            format!("diff_built_jobs at depth {depth:?}\n{}", what())
        });
    }
}

#[test]
fn different_values_never_render_alike_and_no_text_is_too_long() {
    let p = pools();
    let sizes = [
        p.params.len(),
        p.task_params.len(),
        p.descriptions.len(),
        p.dependencies.len(),
        p.hosts.len(),
        p.environments.len(),
        p.args.len(),
        p.files.len(),
    ];
    let mut rng = Rng(0x0DD5_EED5);
    let (mut diffs, mut edited) = (0, 0);
    for case in 0..CASES {
        let old = Side::random(&mut rng, &sizes);
        let new = old.vary(&mut rng, &sizes);
        let (oi, ni) = (old.input(&p), new.input(&p));
        let what = || format!("case {case}: old {:?}, new {:?}", old.0, new.0);
        let depth = rng.below(7);
        let options = match depth {
            0 => DiffOptions::default(),
            n => DiffOptions::default().with_max_depth(n - 1),
        };
        let d = diff_templates(&oi, &ni, &options).unwrap_or_else(|e| panic!("{e}\n{}", what()));
        check(&d, &|| {
            format!("diff_templates, depth choice {depth}\n{}", what())
        });
        diff_at_every_depth(&d.old_job, &d.new_job, &what);
        diffs += 1 + 7;

        let doc = serde_json::to_string(&JobDocument(&d.new_job)).unwrap();
        let (from, to) = rng.pick(&EDITS);
        if doc.contains(from) {
            let job = job_from_str(&doc.replace(from, to))
                .unwrap_or_else(|e| panic!("edit {from} -> {to}: {e}\n{}", what()));
            diff_at_every_depth(&d.new_job, &job, &|| {
                format!("edit {from} -> {to}\n{}", what())
            });
            edited += 1;
            diffs += 7;
        }
    }
    // The pools reach the hand-edited paths, not only the template ones.
    assert!(edited > CASES / 10, "only {edited} edited documents");
    assert!(diffs > CASES * 8);
}
