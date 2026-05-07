#!/usr/bin/env bash
# Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
# Copyright by contributors to this project.
# SPDX-License-Identifier: (Apache-2.0 OR MIT)
#
# Focused variance study: measure HASH_UPLOAD throughput across multiple trials
# for each (runtime_flavor, max_workers) combination, plus s5cmd upload
# baseline. Emits a Markdown summary with min/median/max per cell.
#
# Requirements:
#   - OPENJD_TEST_S3_BUCKET env var (required)
#   - AWS_PROFILE / AWS_REGION as appropriate for your account
#   - s5cmd on $PATH or pointed to by $S5CMD
#   - openjd-snapshots-bench built in release mode (the script builds it)
#
# Configurable env vars:
#   TRIALS          trials per cell (default: 3)
#   RUN_ID          label for results dir (default: variance-<timestamp>)
#   DATASET_DIR     pre-generated dataset; if unset, one is created under /tmp
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

RUN_ID="${RUN_ID:-variance-$(date -u +%Y%m%d-%H%M%S)}"
BENCH_BIN="$WORKSPACE_DIR/target/release/openjd-snapshots-bench"
S5CMD="${S5CMD:-$(command -v s5cmd || echo "$HOME/.local/bin/s5cmd")}"
TRIALS="${TRIALS:-3}"
WORKERS_LIST=(10 50 100)
DATASET_DIR="${DATASET_DIR:-}"
RESULTS_DIR="${RESULTS_DIR:-$WORKSPACE_DIR/bench-results/$RUN_ID}"
mkdir -p "$RESULTS_DIR"

echo "=== openjd-snapshots variance study ==="
echo "Run ID:       $RUN_ID"
echo "Trials:       $TRIALS"
echo "Workers:      ${WORKERS_LIST[*]}"
echo "Results dir:  $RESULTS_DIR"
echo

# Build bench binary.
(cd "$WORKSPACE_DIR" && cargo build --release -p openjd-snapshots --features bench --bin openjd-snapshots-bench) 2>&1 | tail -1

# Generate dataset if needed.
if [[ -z "$DATASET_DIR" ]] || [[ ! -s "$DATASET_DIR/.generated" ]]; then
  DATASET_DIR="/tmp/openjd-bench-dataset-$RUN_ID"
  mkdir -p "$DATASET_DIR"
  rm -rf /tmp/snapshots_bench_data
  echo "Generating dataset in $DATASET_DIR ..."
  "$BENCH_BIN" \
      --preset tiny \
      --max-workers 1 \
      --keep-files \
      --skip-download --no-verify --no-hash-cache \
      --runtime-flavor multi_thread \
      > "$RESULTS_DIR/dataset-gen.log" 2>&1
  mv /tmp/snapshots_bench_data/* "$DATASET_DIR/"
  rmdir /tmp/snapshots_bench_data
  touch "$DATASET_DIR/.generated"
fi
DATA_SIZE_BYTES=$(du -sb "$DATASET_DIR" | awk '{print $1}')
DATA_SIZE_MB=$(( DATA_SIZE_BYTES / 1024 / 1024 ))
FILE_COUNT=$(find "$DATASET_DIR" -type f | wc -l)
echo "Dataset: $FILE_COUNT files, $DATA_SIZE_MB MB"
echo

# Declare result arrays indexed by "flavor-workers-trial" -> MB/s.
declare -A RESULTS_OPENJD
declare -A RESULTS_S5CMD

for TRIAL in $(seq 1 "$TRIALS"); do
  echo "=== Trial $TRIAL / $TRIALS ==="
  for FLAVOR in current_thread multi_thread; do
    for W in "${WORKERS_LIST[@]}"; do
      PREFIX="OpenJDSnapshotsBench/$RUN_ID/trial$TRIAL/openjd-$FLAVOR/w$W"
      OUT="$RESULTS_DIR/openjd-$FLAVOR-w$W-t$TRIAL.log"
      "$BENCH_BIN" \
          --source-dir "$DATASET_DIR" \
          --max-workers "$W" \
          --s3-bucket "$BUCKET" \
          --s3-prefix "$PREFIX" \
          --skip-download --no-verify --no-hash-cache \
          --runtime-flavor "$FLAVOR" \
          > "$OUT" 2>&1
      MB=$(grep -m1 "UPLOAD cold" "$OUT" | awk '{print $(NF-1)}')
      RESULTS_OPENJD["$FLAVOR-$W-$TRIAL"]="$MB"
      echo "  openjd $FLAVOR w=$W: ${MB} MB/s"
    done
  done
  for W in "${WORKERS_LIST[@]}"; do
    PREFIX="OpenJDSnapshotsBench/$RUN_ID/trial$TRIAL/s5cmd/w$W"
    OUT="$RESULTS_DIR/s5cmd-w$W-t$TRIAL.log"
    START_TIME=$(date +%s.%N)
    "$S5CMD" --numworkers "$W" cp \
        "$DATASET_DIR/" \
        "s3://$BUCKET/$PREFIX/" \
        > "$OUT" 2>&1
    END_TIME=$(date +%s.%N)
    TIME=$(awk -v s="$START_TIME" -v e="$END_TIME" 'BEGIN{printf "%.3f", e-s}')
    MB=$(awk -v sz="$DATA_SIZE_MB" -v t="$TIME" 'BEGIN{printf "%.2f", sz/t}')
    RESULTS_S5CMD["$W-$TRIAL"]="$MB"
    echo "  s5cmd w=$W:          ${MB} MB/s"
  done
  echo
done

# ----- Aggregate: compute min / median / max for each combination -----
summarize() {
  echo "$@" | tr ' ' '\n' | sort -n | awk '
    { arr[NR] = $1 }
    END {
      n = NR
      min = arr[1]
      max = arr[n]
      median = (n % 2 == 1) ? arr[(n+1)/2] : (arr[n/2] + arr[n/2+1]) / 2
      printf "%.1f %.1f %.1f\n", min, median, max
    }
  '
}

SUMMARY="$RESULTS_DIR/SUMMARY.md"
{
  echo "# openjd-snapshots variance study"
  echo
  echo "- Run ID: \`$RUN_ID\`"
  echo "- Dataset: $FILE_COUNT files, $DATA_SIZE_MB MB"
  echo "- Trials: $TRIALS per cell"
  echo "- Bucket: \`s3://$BUCKET\` ($AWS_REGION)"
  echo
  echo "All numbers are HASH_UPLOAD throughput in MB/s as reported by the bench binary;"
  echo "s5cmd throughput is dataset-size / wall-time."
  echo
  echo "## openjd-snapshots HASH_UPLOAD (min / median / max across $TRIALS trials)"
  echo
  echo "| Workers | current_thread (min / med / max) | multi_thread (min / med / max) |"
  echo "|--------:|-----------------------------------:|--------------------------------:|"
  for W in "${WORKERS_LIST[@]}"; do
    CT_VALS=""
    MT_VALS=""
    for T in $(seq 1 "$TRIALS"); do
      CT_VALS="$CT_VALS ${RESULTS_OPENJD[current_thread-$W-$T]}"
      MT_VALS="$MT_VALS ${RESULTS_OPENJD[multi_thread-$W-$T]}"
    done
    CT_SUMMARY=$(summarize $CT_VALS)
    MT_SUMMARY=$(summarize $MT_VALS)
    CT_STR=$(echo "$CT_SUMMARY" | awk '{printf "%s / %s / %s", $1, $2, $3}')
    MT_STR=$(echo "$MT_SUMMARY" | awk '{printf "%s / %s / %s", $1, $2, $3}')
    echo "| $W | $CT_STR | $MT_STR |"
  done
  echo
  echo "## s5cmd upload (min / median / max across $TRIALS trials)"
  echo
  echo "| numworkers | min / med / max (MB/s) |"
  echo "|-----------:|------------------------:|"
  for W in "${WORKERS_LIST[@]}"; do
    VALS=""
    for T in $(seq 1 "$TRIALS"); do
      VALS="$VALS ${RESULTS_S5CMD[$W-$T]}"
    done
    SUM=$(summarize $VALS)
    STR=$(echo "$SUM" | awk '{printf "%s / %s / %s", $1, $2, $3}')
    echo "| $W | $STR |"
  done
  echo
  echo "## Raw data"
  echo
  for W in "${WORKERS_LIST[@]}"; do
    echo "### Workers = $W"
    echo
    echo "| Trial | current_thread | multi_thread | s5cmd |"
    echo "|------:|---------------:|-------------:|------:|"
    for T in $(seq 1 "$TRIALS"); do
      echo "| $T | ${RESULTS_OPENJD[current_thread-$W-$T]} | ${RESULTS_OPENJD[multi_thread-$W-$T]} | ${RESULTS_S5CMD[$W-$T]} |"
    done
    echo
  done
} > "$SUMMARY"
echo "=== $SUMMARY ==="
cat "$SUMMARY"
