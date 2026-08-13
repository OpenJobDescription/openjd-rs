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
# Part 1, "must always post" -- a reporting step may never be fatal:
#   1. diff > 64KB        (was: SIGPIPE -> 141 -> -e aborts, no comment posted)
#                         The trigger is the pipe buffer, not a line count: a
#                         41KB diff exits 0, a 168KB diff exits 141.
#   2. malformed summary  (was: jq non-zero -> -e aborts, no comment posted)
#   3. missing summary    (must degrade gracefully)
#   4. no baseline        (untracked report must not read as "no change")
#   5. healthy case       (all fields render)
#
# Part 2, "must upsert" -- the comment lookup must find an existing comment and
# PATCH it, rather than stacking a new one every run. These cases exist because
# two separate bugs both silently degraded to "always POST", and the earlier
# version of this test could not see either: it stubbed the lookup as a fixed
# empty result, so the PATCH branch was never once executed.
#   6. match on page 1        -> PATCH
#   7. match only on page 2   -> PATCH  (a lookup without --paginate misses it)
#   8. match on both pages    -> PATCH exactly one id, not a newline-joined pair
#                                (--paginate applies --jq per page, so an
#                                 unreduced filter yields one id PER PAGE and
#                                 the PATCH URL 404s)
#   9. no match anywhere      -> POST   (negative control: do not over-correct
#                                 into always-PATCH)
set -uo pipefail
WF="$(cd "$(dirname "$0")/../.." && pwd)/workflows/eval_crate.yml"
R=/tmp/comment-step-test.txt
LAST_BODY=/tmp/comment-step-last-body.md
export LAST_BODY
: > "$R"
say() { echo "$@" >> "$R"; }
fails=0

command -v jq >/dev/null || { echo "jq is required" >&2; exit 2; }
python3 -c 'import yaml' 2>/dev/null \
    || { echo "python3 with pyyaml is required to extract the step" >&2; exit 2; }

# Pull the `run:` block of the "Comment on the PR" step out of the YAML.
SCRIPT=$(python3 - "$WF" <<'PY'
import sys, yaml
d = yaml.safe_load(open(sys.argv[1]))
for s in d["jobs"]["eval"]["steps"]:
    if s.get("name", "").startswith("Comment on the PR"):
        sys.stdout.write(s["run"]); break
PY
)
[ -n "$SCRIPT" ] || {
    echo "could not find a step named 'Comment on the PR…' in the eval job of" >&2
    echo "$WF -- this test locates the step by name, so renaming it silently" >&2
    echo "un-tests the script. Update the prefix above to match." >&2
    exit 2
}

# A `gh api` stub faithful in the three ways this step depends on:
#   * `--paginate` applies `--jq` to each page SEPARATELY and concatenates the
#     results -- it does not build one document. This is what makes an unreduced
#     filter emit one id per page.
#   * `--slurp` is rejected outright when combined with `--jq`, with gh's own
#     message and a non-zero exit. The step sends the lookup's stderr to
#     /dev/null, so an unsupported flag combination is invisible at run time and
#     merely leaves the result empty.
#   * `-X <METHOD>` calls are recorded with their endpoint, so a test can tell a
#     PATCH from a POST and check the URL the id was interpolated into.
write_gh_stub() {
  cat > "$1" <<'EOF'
#!/bin/bash
method=""; endpoint=""; filter=""; paginate=0; slurp=0
[ "${1:-}" = api ] && shift   # the subcommand, not the endpoint
while [ $# -gt 0 ]; do
  case "$1" in
    -X)         method="${2:-}"; shift 2 ;;
    --jq)       filter="${2:-}"; shift 2 ;;
    -F|-f|-H)
      # Keep the rendered comment so a test can assert on what a reader sees,
      # not merely that some request was made.
      case "${2:-}" in body=@*) cp "${2#body=@}" "$GH_BODY" 2>/dev/null ;; esac
      shift 2 ;;
    --paginate) paginate=1; shift ;;
    --slurp)    slurp=1; shift ;;
    -*)         shift ;;
    *)          [ -z "$endpoint" ] && endpoint="$1"; shift ;;
  esac
done
if [ -n "$method" ]; then
  # Render an embedded newline as a literal \n. An unreduced lookup puts two ids
  # in the URL, and the resulting 404 is far clearer as `.../111\n222` than as a
  # log record that silently wraps onto a second line.
  printf 'GH_CALL:%s:%s\n' "$method" "${endpoint//$'\n'/\\n}" >> "$GH_LOG"
  exit 0
fi
if [ "$slurp" = 1 ] && [ -n "$filter" ]; then
  echo 'the `--slurp` option is not supported with `--jq` or `--template`' >&2
  exit 1
fi
emit() { if [ -n "$filter" ]; then jq -r "$filter" < "$1"; else cat "$1"; fi; }
if [ "$paginate" = 1 ]; then
  for p in "$GH_PAGES_DIR"/page*.json; do [ -f "$p" ] && emit "$p"; done
else
  [ -f "$GH_PAGES_DIR/page1.json" ] && emit "$GH_PAGES_DIR/page1.json"
fi
exit 0
EOF
  chmod +x "$1"
}

# Build the two lookup pages. $1/$2 are the ids to mark as OUR comment on page
# 1 / page 2 ("" for none); every page also carries an unrelated comment so the
# filter has something it must reject.
write_pages() {
  local dir="$1" p1="$2" p2="$3" n=1
  for spec in "$p1" "$p2"; do
    {
      printf '[{"id":90%d,"body":"unrelated chatter"}' "$n"
      [ -n "$spec" ] && printf ',{"id":%s,"body":"<!-- eval-crate:x -->\\nprevious"}' "$spec"
      printf ']'
    } > "$dir/page$n.json"
    n=$((n + 1))
  done
}

# Core runner. Writes `exit=`, `posted=`, `method=`, `endpoint=` to stdout.
_run() {  # lines, summary, track_baseline, page1_id, page2_id
  local lines="$1" summary="$2" track="$3" p1="$4" p2="$5"
  local wd; wd=$(mktemp -d)
  (
    cd "$wd" || exit 9
    git init -q .; git config user.email t@t; git config user.name t
    mkdir -p reports bin pages
    write_gh_stub bin/gh
    write_pages "$wd/pages" "$p1" "$p2"
    export PATH="$wd/bin:$PATH" GH_LOG="$wd/gh.log" GH_PAGES_DIR="$wd/pages"
    export GH_BODY="$wd/body.md"
    : > "$GH_LOG"; : > "$GH_BODY"

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
    echo "exit=$?"
    # Hand the rendered comment back out; $wd is removed below.
    cp "$GH_BODY" "$LAST_BODY" 2>/dev/null
    echo "posted=$(grep -c GH_CALL "$GH_LOG")"
    echo "method=$(sed -n '1s/^GH_CALL:\([A-Z]*\):.*/\1/p' "$GH_LOG")"
    # Keep the endpoint on one line so an embedded newline is visible as a gap.
    echo "endpoint=$(sed -n '1s/^GH_CALL:[A-Z]*://p' "$GH_LOG" | tr '\n' ' ')"
  ) > "$wd/out" 2>&1
  cat "$wd/out"
  rm -rf "$wd"
}

field() { grep -o "^$2=.*" <<<"$1" | head -1 | cut -d= -f2- ; }

# Part 1: the step must exit 0 and post something, whatever the inputs.
run_case() {  # name, report_lines, summary_content, track_baseline
  local out; out=$(_run "$2" "$3" "$4" "" "")
  local code posted; code=$(field "$out" exit); posted=$(field "$out" posted)
  if [ "${code:-1}" = 0 ] && [ "${posted:-0}" -ge 1 ]; then
    say "  PASS  $1 (exit 0, comment posted)"
  else
    say "  FAIL  $1 (exit=${code:-?}, posted=${posted:-0})"
    fails=$((fails + 1))
  fi
}

# Part 1b: the rendered comment must say what the summary actually said.
run_render_case() {  # name, summary_content, expected substring...
  local name="$1" summary="$2"; shift 2
  local out; out=$(_run 10 "$summary" yes "" "")
  local code; code=$(field "$out" exit)
  local missing=""
  local want
  for want in "$@"; do
    grep -qF -- "$want" "$LAST_BODY" 2>/dev/null || missing="$missing '$want'"
  done
  if [ "${code:-1}" = 0 ] && [ -z "$missing" ]; then
    say "  PASS  $name"
  else
    say "  FAIL  $name (exit=${code:-?}, comment missing:$missing)"
    fails=$((fails + 1))
  fi
}

# Part 2: the step must reuse an existing comment when one exists.
run_upsert_case() {  # name, page1_id, page2_id, expect_method, expect_id
  local out; out=$(_run 10 "$GOOD" yes "$2" "$3")
  local code method endpoint
  code=$(field "$out" exit); method=$(field "$out" method)
  endpoint=$(field "$out" endpoint)
  local want_ep="repos/o/r/issues/comments/$5 "
  [ "$4" = POST ] && want_ep="repos/o/r/issues/1/comments "
  if [ "${code:-1}" = 0 ] && [ "$method" = "$4" ] && [ "$endpoint" = "$want_ep" ]; then
    say "  PASS  $1 ($4 $endpoint)"
  else
    say "  FAIL  $1 (exit=${code:-?}, want $4 '$want_ep', got ${method:-none} '${endpoint:-}')"
    fails=$((fails + 1))
  fi
}

GOOD='{"headline":"ok","findings":{"high":1,"medium":2,"low":3},"withdrawn":0,"build_clean":true,"tests_pass":false,"sections_incomplete":[]}'

say "PR-comment step, run under bash -e as Actions does:"
say " must always post a comment and exit 0:"
run_case "diff over the 64KB pipe buffer (SIGPIPE)"  3000 "$GOOD"                    yes
run_case "diff of 10 lines"                          10 "$GOOD"                    yes
run_case "malformed summary (truncated JSON)"         10 '{"headline":"oops",'      yes
run_case "summary missing findings key"               10 '{"headline":"h"}'         yes
run_case "sections_incomplete is a string not array"  10 '{"headline":"h","sections_incomplete":"api"}' yes
run_case "no summary at all"                          10 ""                         yes
run_case "no committed baseline"                      10 "$GOOD"                    no

say " must report the summary's real values, not placeholders:"
# jq's `//` takes its right-hand side for `false` as well as `null`, so a broken
# build would have rendered as "?" -- reporting the failure as unknown. These
# are the two fields where `false` is the whole point of the comment.
run_render_case "build_clean=false renders as false, not ?" \
  '{"headline":"broken","findings":{"high":0,"medium":0,"low":0},"withdrawn":0,"build_clean":false,"tests_pass":false,"sections_incomplete":[]}' \
  "build clean: false" "tests pass: false"
# Negative control: a genuine zero must not be mistaken for missing either, and
# real values must still come through when nothing is falsy.
run_render_case "zero counts and true booleans render literally" \
  '{"headline":"clean","findings":{"high":0,"medium":0,"low":0},"withdrawn":0,"build_clean":true,"tests_pass":true,"sections_incomplete":[]}' \
  "high 0, medium 0, low 0" "withdrawn by verification: 0" "build clean: true"
# A truly absent field is the only case that should show a placeholder.
run_render_case "absent booleans still fall back to ?" \
  '{"headline":"partial","findings":{"high":1,"medium":2,"low":3}}' \
  "build clean: ?" "tests pass: ?" "withdrawn by verification: n/a"

say " must edit the existing comment rather than stack a new one:"
run_upsert_case "match on page 1"          111 ""    PATCH 111
run_upsert_case "match only on page 2"     ""  222   PATCH 222
run_upsert_case "match on both pages"      111 222   PATCH 222
run_upsert_case "no match anywhere"        ""  ""    POST  ""

say ""
if [ "$fails" -eq 0 ]; then
  say "all cases exited 0, posted a comment, and upserted correctly"
else
  say "$fails case(s) would misbehave in a live run"
fi
cat "$R"
exit "$fails"
