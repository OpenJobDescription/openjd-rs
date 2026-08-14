#!/bin/bash
# Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
# Copyright by contributors to this project.
# SPDX-License-Identifier: (Apache-2.0 OR MIT)
#
# The publish job recovers the crate name and the run date from an artifact
# filename shaped `<crate>-quality-evaluation-report-<YYYY-MM-DD>.md`. The date
# contains dashes, so peeling it off with `${base%-*}` strips only `-14` and the
# crate never matches -- which silently skips every report and publishes nothing.
# That is not hypothetical: it is what the first version of this change did.
#
# The two expansions are read back out of the workflow rather than copied here,
# so editing them in the workflow breaks this test instead of leaving it green
# against a stale duplicate.
set -uo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
WF="$(cd "$HERE/../.." && pwd)/workflows/eval_crate.yml"
fails=0

python3 -c 'import yaml' 2>/dev/null \
    || { echo "python3 with pyyaml is required to extract the expansions" >&2; exit 2; }

EXPANSIONS=$(python3 "$HERE/extract_publish_expansions.py" "$WF") \
    || { echo "could not extract the expansions from $WF" >&2; exit 2; }
CRATE_EXPANSION=${EXPANSIONS%% *}
DATE_EXPANSION=${EXPANSIONS##* }
[ -n "$CRATE_EXPANSION" ] && [ -n "$DATE_EXPANSION" ] \
    || { echo "extraction produced nothing usable" >&2; exit 2; }

echo "publish-step filename parsing, expansions read from the workflow:"
echo "  crate: $CRATE_EXPANSION"
echo "  date:  $DATE_EXPANSION"

parse() {  # base -> "<crate> <date>", with UNPARSED for a half that did not match
    local base="$1" crate date
    crate=$(base="$base" eval "printf '%s' \"$CRATE_EXPANSION\"")
    date=$(base="$base" eval "printf '%s' \"$DATE_EXPANSION\"")
    [ "$crate" = "$base" ] && crate=UNPARSED
    [ "$date" = "$base" ] && date=UNPARSED
    echo "$crate $date"
}

check() {  # base, want_crate, want_date
    local got; got=$(parse "$1")
    if [ "$got" = "$2 $3" ]; then
        printf '  PASS  %-48s -> %s\n' "$1" "$got"
    else
        printf '  FAIL  %-48s -> %s (want "%s %s")\n' "$1" "$got" "$2" "$3"
        fails=$((fails + 1))
    fi
}

check "expr-quality-evaluation-report-2026-08-14"        expr       2026-08-14
check "model-quality-evaluation-report-2026-08-14"       model      2026-08-14
check "sessions-quality-evaluation-report-2026-01-01"    sessions   2026-01-01
check "cli-quality-evaluation-report-2099-12-31"         cli        2099-12-31
# A dashed crate name must survive, since the strip is anchored on the middle.
check "cross-user-quality-evaluation-report-2026-08-14"  cross-user 2026-08-14
# Reports already committed under reports/ that are not eval artifacts must not
# be mistaken for one. Both of these exist in this repo today.
check "expr-model-future-revision-readiness"             UNPARSED   UNPARSED
check "snapshots-async-runtime-flavor-bench"             UNPARSED   UNPARSED

echo
if [ "$fails" -eq 0 ]; then
    echo "all names parsed correctly"
else
    echo "$fails name(s) parsed wrongly; publish would skip or misfile them"
fi
exit "$fails"
