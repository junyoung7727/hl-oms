#!/usr/bin/env bash
# Every relative link in a living Markdown document points at a file that
# exists. ADRs are records and are not edited (adr_immutable.sh), so a link
# in an old ADR may name a document that was removed later; they are left
# out, and so are the working notes under .scratch/.
#
# usage: doc_links.sh
set -euo pipefail
cd "$(git rev-parse --show-toplevel)"

bad=0
while IFS= read -r -d '' f; do
  dir="$(dirname "$f")"
  # [text](target) with a relative target; the anchor (#...) is dropped.
  while IFS= read -r target; do
    path="${target%%#*}"
    [[ -z "$path" ]] && continue
    if [[ ! -e "$dir/$path" ]]; then
      echo "doc-links: $f links to $target, which does not exist"
      bad=1
    fi
  done < <(grep -oE '\]\([^)[:space:]]+\)' "$f" | sed -E 's/^\]\((.*)\)$/\1/' | grep -vE '^(https?:|mailto:|#|/)' || true)
  # A target in <...> is not read above; refuse it rather than let a dead
  # link pass unseen. (A space ends a target: "[x](2차 자료)" is prose.)
  if grep -nE '\]\(<' "$f" > /dev/null; then
    echo "doc-links: $f has a link target in <...>; write it without"
    bad=1
  fi
done < <(git ls-files -z -- '*.md' ':!:docs/adr/ADR-*.md' ':!:.scratch/**')
if [[ $bad -ne 0 ]]; then
  echo "doc-links: FAILED"
  exit 1
fi
echo "doc-links: ok"
