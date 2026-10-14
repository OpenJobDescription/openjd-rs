#!/usr/bin/env bash
# Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
# Copyright by contributors to this project.
# SPDX-License-Identifier: (Apache-2.0 OR MIT)
#
# Posts (or edits) the sticky eval-crate comment on a PR. Called by the
# `Comment on the PR` step of eval_crate.yml, from the openjd-rs checkout.
#
# Reads: CRATE, RUN_DATE, PR, RUN_URL, GITHUB_REPOSITORY, GH_TOKEN.
#
# This reports on a run that may itself have gone wrong, so nothing here may be
# fatal: `set +e` is deliberate. With `-e` active, one non-zero exit inside the
# body-building block aborts the script mid-write and the PATCH/POST never runs,
# turning a successful eval into a red job with no output. Every fragile command
# is additionally guarded.
set +e
set -uo pipefail

: "${CRATE:?}" "${RUN_DATE:?}" "${PR:?}" "${RUN_URL:?}" "${GITHUB_REPOSITORY:?}"

# The run writes to a dated path so the agent's `Write` always creates rather
# than overwrites; the committed baseline it is compared against is the undated
# one.
report="reports/$CRATE-quality-evaluation-report-$RUN_DATE.md"
summary="reports/$CRATE-eval-summary-$RUN_DATE.json"
previous="reports/$CRATE-quality-evaluation-report.md"

# Capture the diff ONCE, in a variable. Piping `git diff | head -200` gives
# head's early exit -> SIGPIPE -> git exits 141 -> pipefail propagates it, which
# under `-e` killed this step for any diff over 200 lines -- i.e. the common case
# for a 20-50 KB report.
#
# `--no-index` because the two sides are different paths and the new one is
# untracked; it exits 1 whenever the files differ, which is the normal case here
# and must not read as failure.
diff_body=""
diff_lines=0
if [ ! -f "$report" ]; then
    # No report at all is a different thing from a report with no changes.
    # Without this case an empty diff against the previous committed report reads
    # as "no change", announcing a run that produced nothing as a run that found
    # nothing new.
    baseline="missing"
elif [ -f "$previous" ] && git ls-files --error-unmatch "$previous" >/dev/null 2>&1; then
    # Tracked AND present: a PR that deletes the baseline leaves it tracked at
    # HEAD but absent from the worktree, and diffing against a missing file
    # yields nothing -- which would read as "no change".
    baseline="tracked"
    diff_body=$(git diff --no-index --unified=0 -- "$previous" "$report")
    diff_lines=$(printf '%s' "$diff_body" | grep -c '' || true)
else
    baseline="none"
fi

# The summary is written freehand by the agent under a wall-clock squeeze, so
# treat it as untrusted: truncated JSON, a missing `findings` key, or a string
# where an object belongs would each make jq exit non-zero.
summary_md=""
if [ -f "$summary" ] && jq -e . "$summary" >/dev/null 2>&1; then
    # `orelse` rather than jq's `//`: `//` takes its right-hand side for `false`
    # as well as `null`, so `build_clean: false` -- the single most important
    # thing this comment can say -- would render as "?".
    summary_md=$(jq -r '
      def orelse(alt): if . == null then alt else . end;
      def triple: "high \(.high | orelse("?")), medium \(.medium | orelse("?")), low \(.low | orelse("?"))";
      "**\(.headline | orelse("no headline"))**\n\n"
      # Keep the CONFIRMED/PLAUSIBLE split the skill requires: a flat total reads
      # as though every finding had been verified. Still render a flat `findings`
      # if the agent wrote one, rather than dropping the numbers entirely.
      + (if (.findings.confirmed // .findings.plausible) != null
         then "- confirmed: \((.findings.confirmed | orelse({})) | triple)\n"
            + "- plausible: \((.findings.plausible | orelse({})) | triple)\n"
         else "- findings (unsplit): \((.findings | orelse({})) | triple)\n" end)
      + "- withdrawn by verification: \(.withdrawn | orelse("n/a"))\n"
      + "- build clean: \(.build_clean | orelse("?")) | tests pass: \(.tests_pass | orelse("?"))"
      + (try ((.sections_incomplete | orelse([])) | if length > 0 then "\n\n> Incomplete sections: " + join(", ") else "" end) catch "")
    ' "$summary" 2>/dev/null)
fi
if [ -z "$summary_md" ]; then
    if [ -f "$summary" ]; then
        summary_md="_The run produced an unreadable summary; see the log._"
    else
        summary_md="_The run did not produce a summary; see the log._"
    fi
fi

body=$(mktemp)
{
    echo "<!-- eval-crate:$CRATE -->"
    echo "## eval-crate: \`openjd-$CRATE\`"
    echo
    printf '%s\n' "$summary_md"
    echo
    # What THIS PR changed relative to the committed baseline is the reviewable
    # signal; the absolute report is large and mostly stable.
    if [ "$baseline" = "missing" ]; then
        echo "_This run produced no report at \`$report\`; see the log._"
    elif [ "$baseline" = "none" ]; then
        echo "_New report; no committed baseline to diff against._"
    elif [ "$diff_lines" -eq 0 ]; then
        echo "_No change vs the committed baseline report._"
    else
        echo "<details><summary>Diff vs the committed baseline report</summary>"
        echo
        echo '```diff'
        printf '%s\n' "$diff_body" | head -200
        echo '```'
        if [ "$diff_lines" -gt 200 ]; then
            echo
            echo "_Diff truncated at 200 of $diff_lines lines; see the artifact for the full report._"
        fi
        echo "</details>"
    fi
    echo
    echo "[Full report artifact]($RUN_URL) · advisory only, not a merge gate."
} >"$body"

# Upsert so repeated runs edit one comment instead of stacking, and paginate
# since the default page is 30 and a re-labelled PR exceeds it. `--paginate`
# applies --jq per page and concatenates, so the filter emits one id per line
# across all pages; `tail -1` reduces that to the most recent (the API returns
# comments oldest-first). Do NOT add `--slurp` to get a single document -- gh
# rejects it outright with "the --slurp option is not supported with --jq",
# which under the `2>/dev/null` would silently leave $existing empty and POST a
# new comment every run.
existing=$(gh api --paginate "repos/$GITHUB_REPOSITORY/issues/$PR/comments" \
    --jq ".[] | select(.body | startswith(\"<!-- eval-crate:$CRATE -->\")) | .id" \
    2>/dev/null | tail -1)
if [ -n "$existing" ]; then
    gh api -X PATCH "repos/$GITHUB_REPOSITORY/issues/comments/$existing" -F body=@"$body"
else
    gh api -X POST "repos/$GITHUB_REPOSITORY/issues/$PR/comments" -F body=@"$body"
fi
