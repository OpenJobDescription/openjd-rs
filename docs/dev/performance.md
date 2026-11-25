# Measuring a performance change

How to produce the evidence that [quality-criteria Q14](quality-criteria.md)
asks of a performance change: unchanged output, counted work, confirming wall
time, and peak memory. The `implement-change` skill's "Performance guideline"
says when to use these. They compile and run both sides, so use them for
changes whose purpose is performance.

The scripts are in [`scripts/perf/`](../../scripts/perf). They need Python
3.9 or later; `difftest.py` also needs PyYAML, which `uv run` installs from
the script's header. Profiling needs valgrind, which runs on Linux (install
the `valgrind` package; on macOS or Windows, use a Linux container).

## 1. Build both sides

Build the base and the change as release binaries in separate worktrees, so
each keeps its own `target/`. The commands below expect a checkout of
[openjd-specifications](https://github.com/OpenJobDescription/openjd-specifications)
next to this repository, as the conformance suite does:

```bash
git worktree add --detach ../openjd-base origin/main
(cd ../openjd-base && cargo build --release -p openjd-cli)
cargo build --release -p openjd-cli
OLD=../openjd-base/target/release/openjd
NEW=target/release/openjd
```

Give both the workloads the change is about, plus a minimal one. Every CLI
call pays a fixed per-process cost, which only the minimal workload shows:

```bash
MIN=../openjd-specifications/conformance-tests/2023-09/base/job_templates/1.1--minimal-job-template.yaml
```

## 2. Output is unchanged

```bash
uv run scripts/perf/difftest.py "$OLD" "$NEW" \
    ../openjd-specifications/conformance-tests/2023-09 \
    --extensions '' --extensions EXPR,FEATURE_BUNDLE_1,TASK_CHUNKING
```

It runs `check` (and `summary` for job templates) on every template, once per
extension set, and compares exit code, stdout and stderr. Add a directory of
your own edge cases for the paths the change touches: valid and invalid
templates, errors at each site, each state of any input the change
special-cases. The result to quote is `N invocations, 0 differences`.

Once, check that the harness can see a difference: run it against a binary
from an older commit that is known to print something different.

The CLI fixes `CallerLimits`, and it does not reach `openjd-snapshots`. For
those, write a library-level probe (section 6).

## 3. Count work

```bash
scripts/perf/profile.py run /tmp/old.cg -- "$OLD" check "$MIN"
scripts/perf/profile.py run /tmp/new.cg -- "$NEW" check "$MIN"
scripts/perf/profile.py diff /tmp/old.cg /tmp/new.cg
```

`diff` prints the total change in instructions and the functions whose
inclusive cost moved most. Instruction counts do not depend on load or
layout, so this is the number a speed claim cites. They still vary by about
0.1% between runs of the same binary (randomly seeded hash maps, worker
threads): profile each side twice and claim only differences well above that
spread. To find work worth
removing, start from one profile:

```bash
scripts/perf/profile.py report /tmp/old.cg              # areas, top functions, allocation sites
scripts/perf/profile.py callers /tmp/old.cg '::path_field$'   # who calls it, how often
```

## 4. Confirm with time

```bash
scripts/perf/bench.py -n 40 old="$OLD" new="$NEW" -- check "$MIN"
scripts/perf/bench.py -n 40 old="$OLD" new="$NEW" -- --help
```

The binaries run interleaved, and the first is also timed from a copy at
another path (`old-copy`): that is the null test, the measurement's noise
floor. The `--help` run is the no-op control: if it moves, the difference is
binary layout, not the change. Report medians with the null test's
difference. Check that time agrees with the count: a change that counts less
work can still run slower.

## 5. Peak memory

Add a counting-allocator test, in its own test binary with a single `#[test]`
so nothing else allocates while it measures. Copy the allocator from
[`crates/openjd-snapshots/tests/decode_memory.rs`](../../crates/openjd-snapshots/tests/decode_memory.rs).
For each case, run the operation inside `peak_during`, and assert the base's
measured peak plus any budget the change states. Write the test on the change,
copy it into the base worktree, and record the base's peaks in the limits.

The cases that find regressions keep something large alive while a later step
does something large: a large value or a large error message (`y = int(x)`
where `x` is large) from one site, and a large temporary at every site that
runs after it. Show that the test fails on the regression it guards.

## 6. Library-level probes

For behaviour the CLI cannot reach, write a scratch integration test that
prints the result of each input under each setting to a file, build it in both
worktrees, and compare the files. Remove it before committing.

```rust
// crates/openjd-model/tests/perf_probe.rs (scratch; do not commit)
use openjd_model::template::parse::document_string_to_object;
use openjd_model::{decode_job_template, CallerLimits, DocumentType};
use std::fmt::Write;

#[test]
fn probe() {
    let dir = std::env::var("PROBE_DIR").expect("PROBE_DIR");
    let mut out = String::new();
    let mut paths: Vec<_> = std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap().path()).collect();
    paths.sort();
    for path in paths {
        let text = std::fs::read_to_string(&path).unwrap();
        for (mem, ops) in [(None, None), (Some(64), None), (None, Some(5))] {
            let limits = CallerLimits {
                max_eval_memory_bytes: mem,
                max_eval_operations: ops,
                ..CallerLimits::default()
            };
            let result = document_string_to_object(&text, DocumentType::Yaml, &limits)
                .and_then(|v| decode_job_template(v, Some(&["EXPR", "FEATURE_BUNDLE_1"]), &limits));
            writeln!(out, "{} {mem:?} {ops:?}\n{:?}", path.display(), result.map(|_| ())).unwrap();
        }
    }
    std::fs::write(std::env::var("PROBE_OUT").expect("PROBE_OUT"), out).unwrap();
}
```

```bash
PROBE_DIR=/tmp/cases PROBE_OUT=/tmp/new.txt cargo test -p openjd-model --test perf_probe
(cd ../openjd-base && PROBE_DIR=/tmp/cases PROBE_OUT=/tmp/old.txt cargo test -p openjd-model --test perf_probe)
cmp /tmp/old.txt /tmp/new.txt
```

Some errors come out in hash-map order. If the base disagrees with itself
across two runs, compare those lines as multisets.
