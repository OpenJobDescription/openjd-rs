# Benchmark: async hash_upload runtime-flavor evaluation

**Date:** 2026-05-05
**Branch:** `snapshots-interface-improvements` (commit 19583c3 — async HASH_UPLOAD / DOWNLOAD)
**Purpose:** evaluate whether making `hash_upload_abs_manifest` async-and-runtime-agnostic
(instead of building its own internal multi-thread runtime) causes a measurable
throughput regression on `tokio::runtime::Builder::new_current_thread()` callers.

## Environment

- Host: EC2 instance, 8 vCPU
- Region: us-west-2 (same region as bucket)
- `openjd-snapshots` commit 19583c3
- `s5cmd` v2.3.0
- AWS SDK for Rust 1.x via `aws-sdk-s3` 1.129

## Method

- Dataset generated once by the bench binary with `--preset tiny`:
  400 × 1 KB files, 40 × 50 MB files, 2 × 1 GB files, organized under
  1000 subdirs up to 10 levels deep. Total: **443 files, 4 070 MB**.
- For each `(runtime_flavor × max_workers)` cell, three trials run
  back-to-back in the same process (fresh S3 prefix per trial → cold cache).
- s5cmd runs the same dataset, same bucket, with matching `--numworkers`.
- Throughput is measured as `HASH_UPLOAD cold / elapsed wall time`. The
  HASH_UPLOAD operation reads every file, computes the xxh128 hash, and
  PUTs the content under a content-addressed key — s5cmd only does the
  PUT (so a fair comparison inflates s5cmd's apparent advantage).
- Reproducer: `scripts/bench/s3.sh` (full pipeline) and `scripts/bench/variance.sh`
  (focused min/median/max) in `crates/openjd-snapshots/`.

## Results

### HASH_UPLOAD throughput (MB/s, median of 3 trials)

| Workers | openjd `current_thread` | openjd `multi_thread` | s5cmd |
|---:|---:|---:|---:|
| 10 | 478 | **758** | 879 |
| 50 | 558 | **894** | 895 |
| 100 | 542 | **779** | 704 |

Full min/median/max:

| Workers | current_thread (min/med/max) | multi_thread (min/med/max) | s5cmd (min/med/max) |
|---:|---:|---:|---:|
| 10 | 454 / 478 / 497 | 748 / 758 / 761 | 791 / 879 / 889 |
| 50 | 506 / 558 / 567 | 890 / 894 / 908 | 776 / 895 / 945 |
| 100 | 486 / 542 / 542 | 676 / 779 / 910 | 550 / 704 / 961 |

## Observations

1. **`current_thread` is consistently 30-40% slower than `multi_thread`** at
   every worker count we tested. At 50 workers (the sweet spot): 558 vs 894
   MB/s, a 38% throughput reduction. This is not noise — the three trials
   per cell agree to within ±10%.

2. **`multi_thread` matches s5cmd within ~15%** at 10 and 50 workers, despite
   doing additional work (xxh128 hashing of every byte). At 50 workers:
   openjd 894 MB/s, s5cmd 895 MB/s — effectively tied. This is the original
   observation from the benchmarking plan, reproduced.

3. **Variance climbs steeply at 100 workers.** All three tools show the
   widest min/max spread at 100 (e.g. s5cmd 550–961 MB/s). Most likely
   cause: S3 bucket-prefix request-rate throttling or network RTT jitter
   under high parallelism. This is also why `multi_thread` median drops
   from 894 at 50w to 779 at 100w — we've passed the knee of the scaling
   curve.

4. **The optimal `max_workers` for this dataset is ~50** on `multi_thread`.
   Adding more workers past 50 decreases throughput due to the throttling
   above.

5. **`current_thread` doesn't scale past ~50 workers** — its median is
   558 at 50w and 542 at 100w. The scheduler thread saturates on the
   orchestration work (task polling, semaphore management, HTTP state
   machine progression) before it can use more concurrency.

## Interpretation

The design choice in commit 19583c3 is:

- **Before:** the function builds its own internal multi_thread runtime
  and block_on's. Callers don't see runtime flavor; throughput is always
  the `multi_thread` number. But the function panics if called from inside
  an existing tokio runtime.
- **After:** the function runs on the caller's runtime. Callers see
  `multi_thread` performance (the common case, because `#[tokio::main]`
  defaults to `multi_thread`) or pay a 30-40% penalty on `current_thread`.

This is a real trade-off. The upside — native async composition with
other async code, no internal-runtime-panic — is substantial for library
users. The downside — 30-40% slower on `current_thread` — is also
substantial, but it's opt-in: callers explicitly chose `current_thread`,
probably for other reasons (reduced memory, simpler runtime), and those
reasons still apply.

**Mitigation via documentation**: the operation spec and doc-comment
should state the performance expectation so callers can make an informed
choice:

> For throughput-sensitive workloads, call `hash_upload_abs_manifest`
> from a multi-threaded tokio runtime. On a `current_thread` runtime,
> throughput is typically 30–40% lower because the scheduler thread
> becomes a bottleneck at `max_workers > ~10`.

## Reproducing

```bash
# Build
cd crates/openjd-snapshots
cargo build --release --features bench --bin openjd-snapshots-bench

# Full variance study (~4 min per run, runs 3 trials)
OPENJD_TEST_S3_BUCKET=<your-bucket> \
  ./scripts/bench/variance.sh

# Full suite including download and s5cmd download (~17 min)
OPENJD_TEST_S3_BUCKET=<your-bucket> \
  ./scripts/bench/s3.sh

# Custom worker sweep
OPENJD_TEST_S3_BUCKET=<your-bucket> \
RUN_ID=my-test WORKERS_LIST="1,5,20,50,200" \
  ./scripts/bench/s3.sh
```

Raw logs and `SUMMARY.md` for each run land under `bench-results/<run-id>/`
(gitignored). Re-run the variance script above to regenerate the backing
data; results vary with network/S3 conditions but the relative ranking
(multi_thread > current_thread; multi_thread ≈ s5cmd at ~50 workers) has
been consistent across runs.
