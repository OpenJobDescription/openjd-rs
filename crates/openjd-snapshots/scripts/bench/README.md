# Benchmark scripts

Operational shell scripts that drive end-to-end benchmarks of
`openjd-snapshots` against real S3 and [`s5cmd`](https://github.com/peak/s5cmd).

These are **not** Cargo microbenchmarks (they don't run under `cargo bench`).
They wrap the `openjd-snapshots-bench` binary — which already exercises the
full pipeline (COLLECT → HASH_UPLOAD → DOWNLOAD → DIFF) — and add:

- Runs across multiple `max_workers` values to reveal scaling characteristics.
- Runs across both `current_thread` and `multi_thread` tokio runtime flavors.
- Parallel runs of `s5cmd` on the same dataset for a baseline.
- Aggregated Markdown summaries under `bench-results/<run-id>/`.

## Prerequisites

- AWS credentials with S3 read/write access to a bucket in the region you
  want to benchmark against.
- `s5cmd` installed and on your `$PATH`, or the path passed via `$S5CMD`.
- The `openjd-snapshots-bench` binary, built in release mode with the
  `bench` feature. The scripts build it automatically the first time.

```bash
# Install s5cmd (x86_64 Linux)
curl -fsSL https://github.com/peak/s5cmd/releases/download/v2.3.0/s5cmd_2.3.0_Linux-64bit.tar.gz \
  | tar xz -C ~/.local/bin s5cmd
```

## Required env vars

| Variable | Purpose |
|---|---|
| `OPENJD_TEST_S3_BUCKET` | S3 bucket for test uploads (required) |
| `AWS_PROFILE` | AWS profile (optional) |
| `AWS_REGION` | AWS region (default: `us-west-2`) |

## Scripts

### `s3.sh` — full pipeline suite

End-to-end comparison: upload + download via `openjd-snapshots` (both runtime
flavors × a worker sweep) vs `s5cmd` (matching worker counts). Generates a
Markdown summary with upload and download throughput tables.

```bash
OPENJD_TEST_S3_BUCKET=my-bucket \
crates/openjd-snapshots/scripts/bench/s3.sh
```

Runtime: ~15–20 min for a ~4 GB dataset.

Customization:

| Env var | Default | Meaning |
|---|---|---|
| `PRESET` | `tiny` | Dataset preset (`tiny`, `small`, `medium`, `large`) |
| `RUN_ID` | `<timestamp>` | Label for the run; controls results dir name |
| `WORKERS_LIST` | `1,10,50,100` | Worker counts for openjd-snapshots (comma-separated) |
| `S5CMD_NUMWORKERS_LIST` | `10 50 100 256` | Worker counts for s5cmd (space-separated) |
| `DATASET_DIR` | unset | If set, reuse a pre-generated dataset at that path (skips dataset generation). If unset, the script generates a fresh dataset under `/tmp`. |

### `variance.sh` — focused variance study

Runs the same upload workload multiple times per cell and reports
min/median/max. Useful when investigating the stability of a specific
performance claim (e.g., "current_thread is consistently slower than
multi_thread").

```bash
OPENJD_TEST_S3_BUCKET=my-bucket TRIALS=5 \
crates/openjd-snapshots/scripts/bench/variance.sh
```

Runtime: ~3–5 min with the default 3 trials.

## Output

Each invocation writes to `bench-results/<run-id>/`:

- `SUMMARY.md` — aggregated tables
- `openjd-<flavor>[-w<N>-t<trial>].log` — raw per-run bench output
- `s5cmd-upload-w<N>.log` / `s5cmd-download-w<N>.log` — s5cmd output
- `*.time` — elapsed-time records used for s5cmd throughput computation

## See also

- [`specs/snapshots-benchmarking-plan.md`](../../../../specs/snapshots-benchmarking-plan.md) —
  methodology and dataset design rationale for the full benchmark matrix.
- [`reports/snapshots-async-runtime-flavor-bench.md`](../../../../reports/snapshots-async-runtime-flavor-bench.md) —
  results comparing `current_thread` vs `multi_thread` for the async
  `hash_upload_abs_manifest`/`download_abs_manifest` interface.
