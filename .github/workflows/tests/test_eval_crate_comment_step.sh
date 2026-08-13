#!/bin/bash
# Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
# Copyright by contributors to this project.
# SPDX-License-Identifier: (Apache-2.0 OR MIT)
#
# Exercise the PR-comment step's shell against the failure modes that static
# checks missed. Extracts the real script out of the workflow, stubs `gh`, and
# runs it under `bash -e` -- the shell Actions actually uses -- so a regression
# in any of these reproduces here rather than in a live run.
#
# Cases:
#   1. diff > 64KB        (was: SIGPIPE -> 141 -> -e aborts, no comment posted)
#                         The trigger is the pipe buffer, not a line count: a
#                         41KB diff exits 0, a 168KB diff exits 141.
#   2. malformed summary  (was: jq non-zero -> -e aborts, no comment posted)
#   3. missing summary    (must degrade gracefully)
#   4. no baseline        (untracked report must not read as "no change")
#   5. healthy case       (all fields render)
set -uo pipefail
WF="$(cd "$(dirname "$0")/../.." && pwd)/workflows/eval_crate.yml"
R=/tmp/comment-step-test.txt
: > "$R"
say() { echo "$@" >> "$R"; }
fails=0

# Pull the `run:` block of the "Comment on the PR" step out of the YAML.
SCRIPT=$(python3 - "$WF" <<'PY'
import sys, yaml
d = yaml.safe_load(open(sys.argv[1]))
for s in d["jobs"]["eval"]["steps"]:
    if s.get("name", "").startswith("Comment on the PR"):
        sys.stdout.write(s["run"]); break
PY
)
[ -n "$SCRIPT" ] || { echo "could not extract the step" >&2; exit 2; }

run_case() {  # name, report_lines, summary_content, track_baseline
  local name="$1" lines="$2" summary="$3" track="$4"
  local wd; wd=$(mktemp -d)
  (
    cd "$wd" || exit 9
    git init -q .; git config user.email t@t; git config user.name t
    mkdir -p reports bin
    # Stub `gh` so we can observe whether the comment was posted.
    cat > bin/gh <<'EOF'
#!/bin/bash
for a in "$@"; do case "$a" in -X) shift; echo "GH_CALL:$1" >> "$GH_LOG"; exit 0;; esac; done
# `gh api --paginate --slurp ...` (the lookup): return two empty pages.
echo "[[],[]]"
EOF
    chmod +x bin/gh
    export PATH="$wd/bin:$PATH" GH_LOG="$wd/gh.log"
    : > "$GH_LOG"

    # A report with `lines` changed lines, optionally with a committed baseline.
    pad=$(printf 'x%.0s' $(seq 1 40))
    seq 1 "$lines" | sed "s/\$/ original $pad/" > reports/x-quality-evaluation-report.md
    if [ "$track" = yes ]; then
      git add reports/x-quality-evaluation-report.md
      git commit -qm base
      seq 1 "$lines" | sed "s/\$/ CHANGED $pad/" > reports/x-quality-evaluation-report.md
    fi
    [ -n "$summary" ] && printf '%s' "$summary" > reports/x-eval-summary.json

    export CRATE=x PR=1 RUN_URL=http://run GH_TOKEN=t GITHUB_REPOSITORY=o/r
    # bash -e is what Actions uses; the step must survive it.
    printf '%s' "$SCRIPT" > step.sh
    bash -e step.sh >/dev/null 2>&1
    echo "exit=$?" > result
    echo "posted=$(grep -c GH_CALL "$GH_LOG")" >> result
    cat result
  ) > "$wd/out" 2>&1
  local code posted
  code=$(grep -o 'exit=[0-9]*' "$wd/out" | cut -d= -f2)
  posted=$(grep -o 'posted=[0-9]*' "$wd/out" | cut -d= -f2)
  if [ "${code:-1}" = 0 ] && [ "${posted:-0}" -ge 1 ]; then
    say "  PASS  $name (exit 0, comment posted)"
  else
    say "  FAIL  $name (exit=${code:-?}, posted=${posted:-0})"
    fails=$((fails + 1))
  fi
  rm -rf "$wd"
}

GOOD='{"headline":"ok","findings":{"high":1,"medium":2,"low":3},"withdrawn":0,"build_clean":true,"tests_pass":false,"sections_incomplete":[]}'

say "PR-comment step, run under bash -e as Actions does:"
run_case "diff over the 64KB pipe buffer (SIGPIPE)"  3000 "$GOOD"                    yes
run_case "diff of 10 lines"                          10 "$GOOD"                    yes
run_case "malformed summary (truncated JSON)"         10 '{"headline":"oops",'      yes
run_case "summary missing findings key"               10 '{"headline":"h"}'         yes
run_case "sections_incomplete is a string not array"  10 '{"headline":"h","sections_incomplete":"api"}' yes
run_case "no summary at all"                          10 ""                         yes
run_case "no committed baseline"                      10 "$GOOD"                    no

say ""
if [ "$fails" -eq 0 ]; then
  say "all cases posted a comment and exited 0"
else
  say "$fails case(s) would have produced NO comment and a red job"
fi
cat "$R"
exit "$fails"
