# Template diff samples

Inputs for `openjd diff` and the `job::template_diff` tests. They are the cases in
`specs/model/template-diff.md` § Examples. `Scene` has no default, so pass it,
e.g. `-p Scene=/proj/shot010.blend`.

| case | command |
|---|---|
| 1. parameter set | `openjd diff render_v1.yaml --old-parameter Frames=1-100 --new-parameter Frames=1-200 -p Scene=/s` |
| 2. template adds `right` to `Eye` | `openjd diff render_v1.yaml render_v2.yaml -p Scene=/s` |
| 3. both | `openjd diff render_v1.yaml render_v2.yaml --new-parameter Frames=1-200 -p Scene=/s` |
| 4. a new `Eye` dimension | `openjd diff render_no_eye.yaml render_v1.yaml -p Scene=/s` |
| 5. only `Scene` changes | `openjd diff render_v1.yaml --old-parameter Scene=//srv/a --new-parameter Scene=//srv/b` |
| A4. template default changes | `openjd diff render_v1.yaml render_default_1_200.yaml -p Scene=/s` |
