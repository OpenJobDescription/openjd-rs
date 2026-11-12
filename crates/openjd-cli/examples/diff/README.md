# `openjd diff` examples

Template pairs for the `openjd diff` example gallery,
[`specs/cli/diff-examples.md`](../../../../specs/cli/diff-examples.md). Each pair changes one
field: `<case>_v1.yaml` is the old side and `<case>_v2.yaml` the new. A "removed" case runs a
pair in reverse. `frames.yaml` and `env_param.yaml` are one template diffed under two
parameter sets.

`frames_1_100.json` and `frames_1_200.json` are `openjd build` output for the `--jobs` case:

```sh
openjd build crates/openjd-cli/examples/diff/frames.yaml -p Frames=1-100 > crates/openjd-cli/examples/diff/frames_1_100.json
```

The gallery test, `crates/openjd-cli/tests/diff_examples.rs`, runs every case from the
repository root and checks the gallery and the two jobs. It also fails when a file here is
not used by any case. Values must print the same on every host: a `PATH` value is written
`//srv/a`, which is absolute under both POSIX and Windows rules (`/a` is not absolute
under Windows rules, so it would print with the runner's drive). After changing a file
here, or the diff output, regenerate them:

```sh
UPDATE_DIFF_EXAMPLES=1 cargo test -p openjd-cli --test diff_examples
```
