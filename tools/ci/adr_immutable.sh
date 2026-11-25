#!/usr/bin/env bash
# An accepted ADR is never edited (Nygard 2011). A change may add a new ADR
# and may change the `상태:` line of an existing one, nothing else: a
# reversal is a new ADR whose number goes on the old one's status line.
#
# usage: adr_immutable.sh <base-commit>
set -euo pipefail
cd "$(git rev-parse --show-toplevel)"
base="${1:?usage: adr_immutable.sh <base-commit>}"
# A process substitution hides a failing git diff from set -e: check the
# base first, so an unknown base cannot read as "nothing changed".
if ! git rev-parse --verify -q "$base^{commit}" > /dev/null; then
  echo "adr-immutable: cannot check: $base is not a commit here"
  exit 2
fi

bad=0
# Deleted or renamed ADRs: the record is kept.
while IFS= read -r -d '' f; do
  echo "adr-immutable: $f was deleted or renamed"
  bad=1
done < <(git diff -z --name-only --no-renames --diff-filter=D "$base"...HEAD -- 'docs/adr/ADR-*.md')
while IFS= read -r -d '' f; do
  # Changed lines other than the status line.
  other="$(git diff -U0 "$base"...HEAD -- "$f" | grep -E '^[-+]' | grep -vE '^(\+\+\+|---) ' | grep -vE '^[-+]상태:' || true)"
  if [[ -n "$other" ]]; then
    echo "adr-immutable: $f changed outside its 상태: line:"
    printf '%s\n' "$other" | sed 's/^/    /'
    bad=1
  fi
done < <(git diff -z --name-only --no-renames --diff-filter=M "$base"...HEAD -- 'docs/adr/ADR-*.md')
if [[ $bad -ne 0 ]]; then
  echo "adr-immutable: FAILED (write a new ADR instead; only the 상태: line of an old one may change)"
  exit 1
fi
echo "adr-immutable: ok"
