#!/usr/bin/env bash
# Lists the parts of a change that the owner reviews himself before it is
# merged (ADR-033): changes to the checks themselves. A change that weakens
# a check passes every check, so no check can catch it; only a person can.
#
# usage: owner_review.sh <base-commit> [paths-file]
#
# paths-file defaults to tools/ci/owner_review_paths.txt of the working
# tree. A merging runner runs the base branch's copy of this script with
# the base branch's list (`bash <(git show main:tools/ci/owner_review.sh)
# main <(git show main:tools/ci/owner_review_paths.txt)`), so that the
# change under review can neither edit the script nor shorten the list it
# is checked against.
#
# The change is <base-commit>...HEAD (from the merge base, as in a pull
# request). Four rules, one line per finding:
#   path:     a changed path (added, changed, deleted, or either side of a
#             move) is listed in tools/ci/owner_review_paths.txt (a name
#             such as clippy.toml or .cargo/ at any depth). That
#             file and this script are under tools/ci/, so changing the
#             list is a finding too.
#   manifest: a Cargo.toml changes what can turn a lint or a test off:
#             a [lints], [workspace.lints] or [profile] table (also as a
#             dotted, quoted or inline key), a [[test]] or [[bench]]
#             target, an `auto*` target key, a target's `test`, `harness`,
#             `doctest` or `bench` key; a new package manifest holds
#             anything but exactly `[lints] workspace = true` of these; or
#             the root manifest drops a [workspace] member. A crate can turn a lint off, or a
#             profile can drop overflow-checks, without a listed path.
#   lint:     a crate-level lint attribute (`#![allow|expect|warn|deny|
#             forbid(..)]`, or one inside `#![cfg_attr(..)]`), or a lint
#             attribute on a `mod` item (it covers the whole module),
#             changes in a file under crates/*/src/. Test files (tests/,
#             benches/, examples/) are not read: they start with
#             `#![allow(clippy::unwrap_used)]` by custom. Other item-level
#             `#[expect(.., reason = "..")]` is not a finding; the reason
#             is in the diff for the reviewer.
#   tests:    the change removes more `#[test]` / `#[tokio::test]`
#             attributes than it adds (one commented out counts as
#             removed), adds `#[ignore]` (also inside `cfg_attr`) to an
#             existing test, adds a `cfg` that can switch tests off,
#             removes a test target file, or removes the `mod` line of a
#             file that still holds tests.
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
test_attr='^[[:space:]]*#[[:space:]]*\[[[:space:]]*(tokio::)?test[[:space:]]*[](]'
finding() { echo "$prefix: $1"; findings=$((findings + 1)); }

# -z: raw paths. --no-renames: a move is a deletion plus an addition, so
# both names are seen.
mapfile -d '' -t changed < <(git diff -z --name-only --no-renames "$mb" HEAD)

# ---- path ------------------------------------------------------------------
# As in .gitignore: an entry with a "/" before its end is a path from the
# top (with a trailing "/" a directory prefix, else one exact path); an
# entry without one is a name at any depth (with a trailing "/" a
# directory name, else a file name). The list is read at HEAD: a change
# that removes an entry also changes the list, which is under tools/ci/.
mapfile -t entries < <(sed -e 's/#.*//' -e 's/[[:space:]]*$//' -e '/^$/d' "$list")
for f in "${changed[@]}"; do
  for e in "${entries[@]}"; do
    n="${e%/}"
    if [[ "$n" == */* ]]; then
      if [[ "$e" == */ ]]; then
        [[ "$f" == "$e"* ]] && { finding "path: $f"; break; }
      else
        [[ "$f" == "$e" ]] && { finding "path: $f"; break; }
      fi
    elif [[ "$e" == */ ]]; then
      [[ "$f" == "$e"* || "$f" == */"$e"* ]] && { finding "path: $f"; break; }
    else
      [[ "$f" == "$e" || "$f" == */"$e" ]] && { finding "path: $f"; break; }
    fi
  done
done

# ---- manifest --------------------------------------------------------------
# Prints what of a manifest read on standard input can turn a lint or a
# test off, without comments and blank lines: the lint and profile tables
# (also as dotted, quoted or inline keys), [[test]] and [[bench]] targets,
# the `auto*` target keys and the `test`/`harness`/`doctest`/`bench` keys
# of a target.
lint_tables() {
  awk '
    { sub(/[[:space:]]*#.*/, ""); sub(/[[:space:]]+$/, "") }
    /^$/ { next }
    /^\[/ {
      keep = ($0 ~ /^\[\[?[[:space:]]*["\x27]?(workspace["\x27]?\.["\x27]?)?(lints|profile)["\x27]?([].[:space:]])/ \
              || $0 ~ /^\[\[[[:space:]]*(test|bench)[[:space:]]*\]\]/)
      target = ($0 ~ /^\[\[?[[:space:]]*(lib|bin|example|test|bench)[[:space:]]*\]\]?/)
    }
    keep || /^[[:space:]]*["\x27]?(workspace\.)?(lints|profile)["\x27]?[[:space:]]*[.=]/ \
      || /^[[:space:]]*auto(tests|benches|bins|examples|lib)[[:space:]]*=/ \
      || (target && /^[[:space:]]*(test|harness|doctest|bench)[[:space:]]*=/) { print }
  '
}
# Prints the [workspace] members of a root manifest read on standard input,
# one per line.
members() {
  perl -0777 -ne 'if (/^\s*members\s*=\s*\[(.*?)\]/ms) { my $m = $1; print "$_\n" for $m =~ /"([^"]*)"/g }'
}
for f in "${changed[@]}"; do
  [[ "$f" == Cargo.toml || "$f" == */Cargo.toml ]] || continue
  old="$(git show "$mb:$f" 2> /dev/null | lint_tables)"
  new="$( (git show "HEAD:$f" 2> /dev/null || true) | lint_tables)"
  std=$'[lints]\nworkspace = true'
  # A new package manifest takes exactly the workspace lints: one without
  # them builds outside every workspace lint. A moved or removed one that
  # only took them is not a finding.
  if ! git cat-file -e "$mb:$f" 2> /dev/null \
    && git show "HEAD:$f" 2> /dev/null | grep -E '^[[:space:]]*\[[[:space:]]*package[[:space:]]*\]' > /dev/null; then
    [[ "$new" == "$std" ]] || finding "manifest: $f"
    continue
  fi
  [[ "$old" == "$std" && -z "$new" ]] && ! git cat-file -e "HEAD:$f" 2> /dev/null && continue
  [[ "$old" == "$new" ]] || finding "manifest: $f"
done
# A crate that leaves the workspace leaves `cargo test --workspace` and the
# lints. Adding a member is not a finding.
if printf '%s\n' "${changed[@]}" | grep -qx 'Cargo.toml'; then
  while IFS= read -r m; do
    finding "manifest: Cargo.toml: member removed: $m"
  done < <(comm -23 <(git show "$mb:Cargo.toml" 2> /dev/null | members | sort -u) \
    <( (git show "HEAD:Cargo.toml" 2> /dev/null || true) | members | sort -u))
fi

# ---- lint ------------------------------------------------------------------
# Prints the crate-level lint attributes of a Rust file read on standard
# input, and the lint attributes put on a `mod` item (they cover the whole
# module), one per line, with white space removed.
lint_attrs() {
  perl -0777 -ne '
    while (/#!\s*\[\s*((?:allow|expect|warn|deny|forbid|cfg_attr)\b[^\]]*)\]/g) {
      my $a = $1;
      next if $a =~ /^cfg_attr/ && $a !~ /\b(?:allow|expect|warn|deny|forbid)\s*\(/;
      $a =~ s/\s+//g;
      print "$a\n";
    }
    while (/#\s*\[\s*((?:allow|expect|warn|deny|forbid|cfg_attr)\b[^\]]*)\]((?:\s*#\s*\[[^\]]*\])*\s*(?:pub(?:\([^)]*\))?\s+)?mod\s+\w+)/g) {
      my ($a, $m) = ($1, $2);
      next if $a =~ /^cfg_attr/ && $a !~ /\b(?:allow|expect|warn|deny|forbid)\s*\(/;
      $m =~ s/.*mod\s+//s;
      $a =~ s/\s+//g;
      print "mod $m: $a\n";
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
# not a finding. Only an attribute at the start of a line counts, so that
# `// #[test]` is a removal. An #[ignore] (also inside `cfg_attr`) counts
# per file, beyond the test attributes that the same file adds: a new test
# may be ignored from the start (a measurement run by hand), an existing
# one may not. The same holds for a `cfg` added in a file under a tests/
# directory. Elsewhere an added `cfg` that holds `any()`, or names `test`
# inside `all(..)` or `not(..)`, can switch tests off without removing
# one, and is a finding; one that widens (`any(test, feature = "..")`) is
# not. A removed test target (tests/<name>.rs or
# tests/<dir>/main.rs that held a test), and a removed `mod <name>;` whose
# file still holds a test, are findings: cargo no longer builds those
# tests. A `cfg(feature = "..")` put above an unchanged `mod tests` line
# under src/ is not seen (text check).
while IFS=$'\t' read -r kind a b; do
  case "$kind" in
    tests) finding "tests: $a" ;;
    mod)
      # `mod b;` in file a: a/../b.rs or b/mod.rs, from the directory of a
      # lib.rs, main.rs or mod.rs, else from a directory named after a.
      case "${a##*/}" in
        lib.rs | main.rs | mod.rs) d="${a%/*}" ;;
        *) d="${a%.rs}" ;;
      esac
      for c in "$d/$b.rs" "$d/$b/mod.rs"; do
        if git show "HEAD:$c" 2> /dev/null | grep -E "$test_attr" > /dev/null; then
          finding "tests: $a: mod $b removed, $c still holds tests"
          break
        fi
      done
      ;;
  esac
done < <(git diff -U0 --no-renames --no-prefix "$mb" HEAD -- '*.rs' | awk '
  BEGIN { t = "^[-+][[:space:]]*#[[:space:]]*\\[[[:space:]]*(tokio::)?test[[:space:]]*[](]"
          ig = "^\\+[[:space:]]*#[[:space:]]*\\[[[:space:]]*ignore[[:space:]]*[]=(]"
          ca = "^\\+[[:space:]]*#[[:space:]]*\\[[[:space:]]*cfg_attr[[:space:]]*\\(.*[^[:alnum:]_]ignore([^[:alnum:]_]|$)"
          cf = "^\\+[[:space:]]*#!?[[:space:]]*\\[[[:space:]]*cfg(_attr)?[[:space:]]*\\("
          cp = "^\\+[[:space:]]*#!?[[:space:]]*\\[[[:space:]]*cfg[[:space:]]*\\("
          md = "^[-+][[:space:]]*(pub(\\([^)]*\\))?[[:space:]]+)?mod[[:space:]]+[[:alnum:]_]+[[:space:]]*;" }
  /^\+\+\+ / { f = substr($0, 5); next }
  /^--- / { next }
  /^-/ && $0 ~ t { removed++ }
  /^\+/ && $0 ~ t { added++; fa[f]++ }
  $0 ~ ig || $0 ~ ca { fi[f]++ }
  $0 ~ cf && ($0 ~ /any[[:space:]]*\([[:space:]]*\)/ \
    || ($0 ~ /test/ && ($0 ~ /all[[:space:]]*\(/ || $0 ~ /not[[:space:]]*\(/))) { cfg[f]++; next }
  $0 ~ cp && f ~ /(^|\/)tests\// { tcfg[f]++ }
  $0 ~ md && f != "/dev/null" {
    m = $0; sub(/^[-+][[:space:]]*(pub(\([^)]*\))?[[:space:]]+)?mod[[:space:]]+/, "", m); sub(/[^[:alnum:]_].*/, "", m)
    if ($0 ~ /^-/) mods[f "\t" m]--; else mods[f "\t" m]++
  }
  END {
    if (removed > added) printf "tests\t%d test attribute(s) removed, %d added\n", removed, added
    for (f in fi) if (fi[f] > fa[f]) printf "tests\t%s: %d #[ignore] added, %d test attribute(s) added\n", f, fi[f], fa[f] + 0
    for (f in cfg) printf "tests\t%s: %d cfg on tests added\n", f, cfg[f]
    for (f in tcfg) if (tcfg[f] > fa[f]) printf "tests\t%s: %d cfg added in a test file, %d test attribute(s) added\n", f, tcfg[f], fa[f] + 0
    for (k in mods) if (mods[k] < 0) printf "mod\t%s\n", k
  }')
# A test target that is gone. Moved below tests/<dir>/ (not main.rs), cargo
# no longer builds it; the attributes removed and added cancel out.
for f in "${changed[@]}"; do
  [[ "$f" =~ ^(crates/[^/]+/)?tests/([^/]+\.rs|[^/]+/main\.rs)$ ]] || continue
  git cat-file -e "HEAD:$f" 2> /dev/null && continue
  git show "$mb:$f" 2> /dev/null | grep -E "$test_attr" > /dev/null && finding "tests: test target removed: $f"
done

if (( findings == 0 )); then
  echo "$prefix: none"
  exit 0
fi
exit 1
