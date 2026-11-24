#!/usr/bin/env bash
# Lists the parts of a change that the owner reviews himself before it is
# merged (ADR-033): changes to the checks themselves. A change that weakens
# a check passes every check, so no check can catch it; only a person can.
#
# usage: owner_review.sh <base-commit> [paths-file]
#
# paths-file defaults to tools/ci/owner_review_paths.txt of the working
# tree. A merging runner passes the copy from the base branch, so that the
# change under review cannot shorten the list it is checked against.
#
# The change is <base-commit>...HEAD (from the merge base, as in a pull
# request). Four rules, one line per finding:
#   path:     a changed path (added, changed, deleted, or either side of a
#             move) is listed in tools/ci/owner_review_paths.txt. That
#             file and this script are under tools/ci/, so changing the
#             list is a finding too.
#   manifest: a Cargo.toml changes a [lints], [workspace.lints] or
#             [profile] section (or a `lints.` / `profile.` dotted key):
#             a crate can turn a lint off, or a profile can drop
#             overflow-checks, without touching a listed path.
#   lint:     a crate-level lint attribute (`#![allow|expect|warn|deny|
#             forbid(..)]`, or one inside `#![cfg_attr(..)]`) changes in a
#             file under crates/*/src/. Test files (tests/, benches/,
#             examples/) are not read: they start with
#             `#![allow(clippy::unwrap_used)]` by custom. Item-level
#             `#[expect(.., reason = "..")]` is not a finding; the reason
#             is in the diff for the reviewer.
#   tests:    the change removes more `#[test]` / `#[tokio::test]`
#             attributes than it adds, or adds `#[ignore]`.
#
# exit 0: no finding, prints "owner_review: none"
# exit 1: one line per finding ("owner_review: <rule>: ..."), never "none"
# exit 2: it could not check ("owner_review: cannot check: <reason>")
#
# These are text checks. A lint attribute whose reason string holds `]`
# is read up to that `]`; a test attribute written over two lines is not
# counted.
set -uo pipefail

prefix="owner_review"
cannot() { echo "$prefix: cannot check: $1"; exit 2; }

top="$(git rev-parse --show-toplevel 2> /dev/null)" || cannot "not in a git repository"
cd "$top" || cannot "cannot enter $top"
base="${1:-}"
[[ -n "$base" ]] || cannot "usage: owner_review.sh <base-commit> [paths-file]"
git rev-parse -q --verify "$base^{commit}" > /dev/null || cannot "no commit $base"
mb="$(git merge-base "$base" HEAD 2> /dev/null)" || cannot "no merge base of $base and HEAD"
list="${2:-tools/ci/owner_review_paths.txt}"
[[ -f "$list" ]] || cannot "no paths file $list"

findings=0
finding() { echo "$prefix: $1"; findings=$((findings + 1)); }

# -z: raw paths. --no-renames: a move is a deletion plus an addition, so
# both names are seen.
mapfile -d '' -t changed < <(git diff -z --name-only --no-renames "$mb" HEAD)

# ---- path ------------------------------------------------------------------
# An entry that ends with "/" is a directory prefix, any other entry is one
# exact path. The list is read at HEAD: a change that removes an entry
# also changes the list, which is under tools/ci/.
mapfile -t entries < <(sed -e 's/#.*//' -e 's/[[:space:]]*$//' -e '/^$/d' "$list")
for f in "${changed[@]}"; do
  for e in "${entries[@]}"; do
    if [[ "$e" == */ ]]; then
      [[ "$f" == "$e"* ]] && { finding "path: $f"; break; }
    else
      [[ "$f" == "$e" ]] && { finding "path: $f"; break; }
    fi
  done
done

# ---- manifest --------------------------------------------------------------
# Prints the lint and profile tables of a manifest read on standard input,
# without comments and blank lines.
lint_tables() {
  awk '
    { sub(/[[:space:]]*#.*/, ""); sub(/[[:space:]]+$/, "") }
    /^$/ { next }
    /^\[/ { keep = ($0 ~ /^\[\[?[[:space:]]*(workspace\.)?(lints|profile)([].[:space:]])/) }
    keep || /^[[:space:]]*(workspace\.)?(lints|profile)\./ { print }
  '
}
for f in "${changed[@]}"; do
  [[ "$f" == Cargo.toml || "$f" == */Cargo.toml ]] || continue
  old="$(git show "$mb:$f" 2> /dev/null | lint_tables)"
  new="$( (git show "HEAD:$f" 2> /dev/null || true) | lint_tables)"
  # A new manifest that only takes the workspace lints is not a finding.
  [[ -z "$old" && "$new" == $'[lints]\nworkspace = true' ]] && continue
  [[ "$old" == "$new" ]] || finding "manifest: $f"
done

# ---- lint ------------------------------------------------------------------
# Prints the crate-level lint attributes of a Rust file read on standard
# input, one per line, with white space removed.
lint_attrs() {
  perl -0777 -ne '
    while (/#!\s*\[\s*((?:allow|expect|warn|deny|forbid|cfg_attr)\b[^\]]*)\]/g) {
      my $a = $1;
      next if $a =~ /^cfg_attr/ && $a !~ /\b(?:allow|expect|warn|deny|forbid)\s*\(/;
      $a =~ s/\s+//g;
      print "$a\n";
    }'
}
for f in "${changed[@]}"; do
  [[ "$f" =~ ^crates/[^/]+/src/.*\.rs$ ]] || continue
  old="$(git show "$mb:$f" 2> /dev/null | lint_attrs)"
  new="$( (git show "HEAD:$f" 2> /dev/null || true) | lint_attrs)"
  [[ "$old" == "$new" ]] || finding "lint: $f"
done

# ---- tests -----------------------------------------------------------------
# Counted over the whole change, so that a test moved to another file is
# not a finding. An #[ignore] counts per file, beyond the test attributes
# that the same file adds: a new test may be ignored from the start (a
# measurement run by hand), an existing one may not.
while IFS=$'\t' read -r kind detail; do
  finding "tests: $detail"
done < <(git diff -U0 --no-renames --no-prefix "$mb" HEAD -- '*.rs' | awk '
  BEGIN { t = "#[[:space:]]*\\[[[:space:]]*(tokio::)?test[[:space:]]*[](]"
          ig = "#[[:space:]]*\\[[[:space:]]*ignore[[:space:]]*[]=(]" }
  /^\+\+\+ / { f = substr($0, 5); next }
  /^--- / { next }
  /^-/ && $0 ~ t { removed++ }
  /^\+/ && $0 ~ t { added++; fa[f]++ }
  /^\+/ && $0 ~ ig { fi[f]++ }
  END {
    if (removed > added) printf "tests\t%d test attribute(s) removed, %d added\n", removed, added
    for (f in fi) if (fi[f] > fa[f]) printf "tests\t%s: %d #[ignore] added, %d test attribute(s) added\n", f, fi[f], fa[f] + 0
  }')

if (( findings == 0 )); then
  echo "$prefix: none"
  exit 0
fi
exit 1
