# `openjd diff` per-field test cases

Template pairs for the per-field cases in
[`specs/cli/diff-cases.md`](../../../../../specs/cli/diff-cases.md). Each pair changes one
field: `<case>_v1.yaml` is the old side and `<case>_v2.yaml` the new. A "removed" case runs a
pair in reverse. `frames.yaml` and `env_param.yaml` are one template diffed under two
parameter sets. These are test data. To learn the command, read the curated examples in
[`crates/openjd-cli/examples/diff/`](../../../examples/diff/README.md).

The test `crates/openjd-cli/tests/diff_cases.rs` runs every case from the repository root
and checks the generated file. It fails when a file here is not used by any case. Values
must print the same on every host: a `PATH` value is written `//srv/a`, which is absolute
under both POSIX and Windows rules (`/a` is not absolute under Windows rules, so it would
print with the runner's drive). After changing a file here, or the diff output, regenerate
it:

```sh
UPDATE_DIFF_CASES=1 cargo test -p openjd-cli --test diff_cases
```
