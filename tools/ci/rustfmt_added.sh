#!/usr/bin/env bash
# Checks the formatting of the Rust files a change ADDS, relative to a base
# commit. Files that already exist are exempt: the tree was imported
# unformatted and is not reformatted as a whole.
#
# - Each added file is formatted on its own through standard input. Given a
#   path, rustfmt also formats every `mod x;` child of that file, which
#   would pull existing, exempt files (for example tests/common/mod.rs)
#   into the check.
# - A moved or renamed file counts as added (`--no-renames`), so new code
#   cannot avoid the check by arriving in a renamed file. Moving an old file
#   therefore means formatting that one file.
# - Code added to a file that stays in place is not checked.
#
# usage: rustfmt_added.sh <base-commit>
set -euo pipefail
cd "$(git rev-parse --show-toplevel)"
base="${1:?usage: rustfmt_added.sh <base-commit>}"

mapfile -t added < <(git diff --name-only --no-renames --diff-filter=A "$base"...HEAD -- '*.rs')
if [[ ${#added[@]} -eq 0 ]]; then
  echo "rustfmt-added: no Rust file added since $base"
  exit 0
fi
echo "rustfmt-added: checking ${#added[@]} added file(s)"
bad=0
for f in "${added[@]}"; do
  # `rustfmt --check` on standard input exits 0 for unformatted input, so
  # the formatted text is compared with the file.
  if ! formatted="$(rustfmt --edition 2024 --emit stdout < "$f")"; then
    echo "rustfmt-added: rustfmt could not parse $f"
    bad=1
  elif ! diff -u --label "$f" --label "$f (formatted)" "$f" <(printf '%s\n' "$formatted"); then
    bad=1
  fi
done
if [[ $bad -ne 0 ]]; then
  echo "rustfmt-added: FAILED"
  exit 1
fi
echo "rustfmt-added: ok"
