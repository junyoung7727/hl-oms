#!/usr/bin/env bash
# Every runbook under docs/operations/ names its owner and the date it was
# last reviewed, on two lines near the top:
#
#   주인: <name>
#   검토일: YYYY-MM-DD
#
# A runbook without them fails. One reviewed more than MAX_AGE_DAYS ago
# only warns (SWE at Google ch.10: freshness dates), because an old date
# asks for a review, not for a stop.
#
# usage: doc_freshness.sh [today YYYY-MM-DD]   (the argument is for the selftest)
set -euo pipefail
cd "$(git rev-parse --show-toplevel)"
today="${1:-$(date -u +%F)}"
max_age_days=90
now="$(date -u -d "$today" +%s)"

bad=0
for f in docs/operations/*.md; do
  [[ -e "$f" ]] || continue
  owner="$(head -n 10 "$f" | sed -n 's/^주인: *//p' | head -n 1)"
  reviewed="$(head -n 10 "$f" | sed -n 's/^검토일: *//p' | head -n 1)"
  if [[ -z "$owner" ]]; then
    echo "doc-freshness: $f has no '주인:' line in its first 10 lines"
    bad=1
  fi
  if ! [[ "$reviewed" =~ ^[0-9]{4}-[0-9]{2}-[0-9]{2}$ ]] || ! reviewed_s="$(date -u -d "$reviewed" +%s 2>/dev/null)"; then
    echo "doc-freshness: $f has no valid '검토일: YYYY-MM-DD' line in its first 10 lines"
    bad=1
    continue
  fi
  age=$(((now - reviewed_s) / 86400))
  if ((age > max_age_days)); then
    echo "::warning file=$f::reviewed $age days ago ($reviewed), more than $max_age_days; review it and update 검토일"
  fi
done
if [[ $bad -ne 0 ]]; then
  echo "doc-freshness: FAILED"
  exit 1
fi
echo "doc-freshness: ok"
