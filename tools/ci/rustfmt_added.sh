#!/usr/bin/env bash
# Checks the formatting of the Rust files a change ADDS, relative to a base
# commit. Files that already exist are exempt: the tree was imported
# unformatted and is not reformatted as a whole.
#
# usage: rustfmt_added.sh <base-commit>
set -euo pipefail
cd "$(git rev-parse --show-toplevel)"
base="${1:?usage: rustfmt_added.sh <base-commit>}"

mapfile -t added < <(git diff --name-only --diff-filter=A "$base"...HEAD -- '*.rs')
if [[ ${#added[@]} -eq 0 ]]; then
  echo "rustfmt-added: no Rust file added since $base"
  exit 0
fi
echo "rustfmt-added: checking ${#added[@]} added file(s)"
rustfmt --check --edition 2024 -- "${added[@]}"
