#!/usr/bin/env bash
# Benchmark openjd-snapshots upload/download against s5cmd on the same dataset.
#
# Compares across the full matrix:
#   - openjd-snapshots tokio runtime flavor (current_thread vs multi_thread)
#   - openjd-snapshots max_workers sweep
#   - s5cmd at various --numworkers
#
# Requirements:
#   - AWS credentials for an S3 bucket you can write to. Set these via the env:
#         OPENJD_TEST_S3_BUCKET   (required)  S3 bucket for test uploads
#         AWS_PROFILE             (optional)  AWS profile to use
#         AWS_REGION              (optional)  AWS region (default us-west-2)
#   - s5cmd on $PATH, or pointed to by $S5CMD.
#   - `openjd-snapshots-bench` built in release mode (the script builds it
#     automatically at the start).
#
# Outputs a Markdown summary under ./bench-results/<RUN_ID>/SUMMARY.md along
# with per-invocation logs. Run from anywhere; the script resolves paths
# relative to its own location.
set -euo pipefail

if [[ -z "${OPENJD_TEST_S3_BUCKET:-}" ]]; then
  echo "error: OPENJD_TEST_S3_BUCKET must be set" >&2
  exit 2
fi
BUCKET="$OPENJD_TEST_S3_BUCKET"
export AWS_REGION="${AWS_REGION:-us-west-2}"

# Resolve workspace and crate roots from this script's location.
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CRATE_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"
WORKSPACE_DIR="$(cd "$CRATE_DIR/../.." && pwd)"

PRESET="${PRESET:-tiny}"
RUN_ID="${RUN_ID:-$(date -u +%Y%m%d-%H%M%S)}"
BENCH_BIN="$WORKSPACE_DIR/target/release/openjd-snapshots-bench"
S5CMD="${S5CMD:-$(command -v s5cmd || echo "$HOME/.local/bin/s5cmd")}"
WORKERS_LIST="${WORKERS_LIST:-1,10,50,100}"
RESULTS_DIR="${RESULTS_DIR:-$WORKSPACE_DIR/bench-results/$RUN_ID}"
S5CMD_NUMWORKERS_LIST="${S5CMD_NUMWORKERS_LIST:-10 50 100 256}"

mkdir -p "$RESULTS_DIR"

echo "=== openjd-snapshots benchmark suite ==="
echo "Run ID:       $RUN_ID"
echo "Bucket:       s3://$BUCKET"
echo "Preset:       $PRESET"
echo "Workers:      $WORKERS_LIST (openjd)"
echo "Workers:      $S5CMD_NUMWORKERS_LIST (s5cmd)"
echo "Results dir:  $RESULTS_DIR"
echo

# Ensure the bench binary is built in release.
(cd "$WORKSPACE_DIR" && cargo build --release -p openjd-snapshots --features bench --bin openjd-snapshots-bench) 2>&1 | tail -1

# ---- Generate dataset once, reuse for all runs ------------------------------
DATASET_DIR="${DATASET_DIR:-/tmp/openjd-bench-dataset-$RUN_ID}"
if [[ ! -s "$DATASET_DIR/.generated" ]]; then
  mkdir -p "$DATASET_DIR"
  echo "=== Generating test dataset in $DATASET_DIR ==="
  # --keep-files makes the bench binary write its generated test data to
  # /tmp/snapshots_bench_data. We move it to the stable location we want.
  rm -rf /tmp/snapshots_bench_data
  "$BENCH_BIN" \
      --preset "$PRESET" \
      --max-workers 1 \
      --keep-files \
      --skip-download --no-verify --no-hash-cache \
      --runtime-flavor multi_thread \
      > "$RESULTS_DIR/dataset-gen.log" 2>&1 || true
  if [[ -d /tmp/snapshots_bench_data ]] && [[ "$(ls /tmp/snapshots_bench_data | wc -l)" -gt 0 ]]; then
    rm -rf "$DATASET_DIR"
    mv /tmp/snapshots_bench_data "$DATASET_DIR"
    touch "$DATASET_DIR/.generated"
  else
    echo "ERROR: dataset generation failed; /tmp/snapshots_bench_data is empty" >&2
    tail -30 "$RESULTS_DIR/dataset-gen.log" >&2
    exit 1
  fi
fi
DATA_SIZE_BYTES=$(du -sb "$DATASET_DIR" | awk '{print $1}')
DATA_SIZE_MB=$(( DATA_SIZE_BYTES / 1024 / 1024 ))
FILE_COUNT=$(find "$DATASET_DIR" -type f | wc -l)
echo "Dataset: $FILE_COUNT files, $DATA_SIZE_MB MB"
echo

# ---- openjd-snapshots benchmarks --------------------------------------------
for FLAVOR in current_thread multi_thread; do
  echo "=== openjd-snapshots ($FLAVOR, workers=$WORKERS_LIST) ==="
  PREFIX="OpenJDSnapshotsBench/$RUN_ID/openjd/$FLAVOR"
  OUT="$RESULTS_DIR/openjd-$FLAVOR.log"
  "$BENCH_BIN" \
      --source-dir "$DATASET_DIR" \
      --max-workers "$WORKERS_LIST" \
      --s3-bucket "$BUCKET" \
      --s3-prefix "$PREFIX" \
      --no-verify --no-hash-cache \
      --runtime-flavor "$FLAVOR" \
      > "$OUT" 2>&1
  echo "  → $OUT"
  grep -A20 "SCALING TEST SUMMARY" "$OUT" | head -25 || true
  echo
done

# ---- s5cmd upload baseline --------------------------------------------------
echo "=== s5cmd upload baselines ==="
for NW in $S5CMD_NUMWORKERS_LIST; do
  PREFIX="OpenJDSnapshotsBench/$RUN_ID/s5cmd/w$NW"
  OUT="$RESULTS_DIR/s5cmd-upload-w$NW.log"
  echo "  s5cmd --numworkers $NW cp ..."
  START_TIME=$(date +%s.%N)
  "$S5CMD" --numworkers "$NW" cp \
      "$DATASET_DIR/" \
      "s3://$BUCKET/$PREFIX/" \
      > "$OUT" 2>&1
  END_TIME=$(date +%s.%N)
  TIME=$(awk -v s="$START_TIME" -v e="$END_TIME" 'BEGIN{printf "%.3f", e-s}')
  echo "$TIME" > "$OUT.time"
  THROUGHPUT=$(awk -v sz="$DATA_SIZE_MB" -v t="$TIME" 'BEGIN{printf "%.1f", sz/t}')
  echo "    elapsed: ${TIME}s, throughput: ${THROUGHPUT} MB/s"
done
echo

# ---- s5cmd download baseline -----------------------------------------------
echo "=== s5cmd download baselines (from one of openjd's upload prefixes) ==="
UPLOAD_PREFIX="OpenJDSnapshotsBench/$RUN_ID/openjd/multi_thread/w100"
for NW in $S5CMD_NUMWORKERS_LIST; do
  OUT="$RESULTS_DIR/s5cmd-download-w$NW.log"
  DEST=$(mktemp -d /tmp/s5cmd-dl-XXXXXX)
  echo "  s5cmd --numworkers $NW cp (download) ..."
  START_TIME=$(date +%s.%N)
  "$S5CMD" --numworkers "$NW" cp \
      "s3://$BUCKET/$UPLOAD_PREFIX/*" \
      "$DEST/" \
      > "$OUT" 2>&1
  END_TIME=$(date +%s.%N)
  TIME=$(awk -v s="$START_TIME" -v e="$END_TIME" 'BEGIN{printf "%.3f", e-s}')
  echo "$TIME" > "$OUT.time"
  DL_BYTES=$(du -sb "$DEST" | awk '{print $1}')
  DL_MB=$(( DL_BYTES / 1024 / 1024 ))
  if [[ "$TIME" != "0" ]] && [[ -n "$TIME" ]]; then
    THROUGHPUT=$(awk -v sz="$DL_MB" -v t="$TIME" 'BEGIN{printf "%.1f", sz/t}')
    echo "    elapsed: ${TIME}s, downloaded: ${DL_MB} MB, throughput: ${THROUGHPUT} MB/s"
  fi
  rm -rf "$DEST"
done
echo

# ---- Summary ---------------------------------------------------------------
SUMMARY="$RESULTS_DIR/SUMMARY.md"
{
  echo "# openjd-snapshots benchmark summary"
  echo
  echo "- Run ID: \`$RUN_ID\`"
  echo "- Bucket: \`s3://$BUCKET\`"
  echo "- Preset: \`$PRESET\`"
  echo "- Dataset: $FILE_COUNT files, $DATA_SIZE_MB MB"
  echo "- Workers tested (openjd): $WORKERS_LIST"
  echo "- Workers tested (s5cmd): $S5CMD_NUMWORKERS_LIST"
  echo
  echo "## HASH_UPLOAD cold throughput (MB/s)"
  echo
  echo "| Workers | openjd current_thread | openjd multi_thread | s5cmd (same numworkers) |"
  echo "|--------:|----------------------:|--------------------:|------------------------:|"
  for W in $(echo "$WORKERS_LIST" | tr ',' ' '); do
    CT_MB=$(grep -A1 "TEST: HASH_UPLOAD.*max_workers=$W)" "$RESULTS_DIR/openjd-current_thread.log" \
            -m1 -A30 | grep -m1 "Throughput:" | awk '{print $2}' || echo "-")
    MT_MB=$(grep -A1 "TEST: HASH_UPLOAD.*max_workers=$W)" "$RESULTS_DIR/openjd-multi_thread.log" \
            -m1 -A30 | grep -m1 "Throughput:" | awk '{print $2}' || echo "-")
    S5_TIME_FILE="$RESULTS_DIR/s5cmd-upload-w$W.log.time"
    if [[ -f "$S5_TIME_FILE" ]]; then
      S5_T=$(cat "$S5_TIME_FILE")
      S5_MB=$(awk -v sz="$DATA_SIZE_MB" -v t="$S5_T" 'BEGIN{printf "%.1f", sz/t}')
    else
      S5_MB="—"
    fi
    echo "| $W | ${CT_MB:-—} | ${MT_MB:-—} | $S5_MB |"
  done
  echo
  echo "## DOWNLOAD cold throughput (MB/s)"
  echo
  echo "| Workers | openjd current_thread | openjd multi_thread | s5cmd (same numworkers) |"
  echo "|--------:|----------------------:|--------------------:|------------------------:|"
  for W in $(echo "$WORKERS_LIST" | tr ',' ' '); do
    CT_MB=$(grep -A1 "TEST: DOWNLOAD.*max_workers=$W)" "$RESULTS_DIR/openjd-current_thread.log" \
            -m1 -A30 | grep -m1 "Throughput:" | awk '{print $2}' || echo "-")
    MT_MB=$(grep -A1 "TEST: DOWNLOAD.*max_workers=$W)" "$RESULTS_DIR/openjd-multi_thread.log" \
            -m1 -A30 | grep -m1 "Throughput:" | awk '{print $2}' || echo "-")
    S5_TIME_FILE="$RESULTS_DIR/s5cmd-download-w$W.log.time"
    if [[ -f "$S5_TIME_FILE" ]]; then
      S5_T=$(cat "$S5_TIME_FILE")
      S5_MB=$(awk -v sz="$DATA_SIZE_MB" -v t="$S5_T" 'BEGIN{printf "%.1f", sz/t}')
    else
      S5_MB="—"
    fi
    echo "| $W | ${CT_MB:-—} | ${MT_MB:-—} | $S5_MB |"
  done
  echo
} > "$SUMMARY"
echo "=== Summary: $SUMMARY ==="
cat "$SUMMARY"
