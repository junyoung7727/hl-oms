#!/usr/bin/env bash
# Proves that the gate scripts go red on the cases they exist for and
# stay green on the cases they must allow. Runs in a throwaway git
# repository; it does not touch this one.
#
# Every hex value below is built at run time from a repeated digit pair, so
# this file holds no long hex run itself.
set -euo pipefail
src="$(cd "$(dirname "$0")" && pwd)"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

failures=0
expect() { # expect <ok|fail> <label> <command...>
  local want="$1" label="$2" got=ok
  shift 2
  "$@" > "$tmp/out.txt" 2>&1 || got=fail
  if [[ "$got" == "$want" ]]; then
    echo "selftest: pass  ($want) $label"
  else
    echo "selftest: WRONG (wanted $want, got $got) $label"
    sed 's/^/    /' "$tmp/out.txt"
    failures=$((failures + 1))
  fi
}
hex() { # hex <pair> <count>: the pair repeated count times
  local out="" i
  for ((i = 0; i < $2; i++)); do out+="$1"; done
  printf '%s' "$out"
}

repo="$tmp/repo"
mkdir -p "$repo/tools/ci" "$repo/tests/common"
cd "$repo"
git init -q .
g() { git -c user.name=selftest -c user.email=selftest@invalid -c commit.gpgsign=false "$@"; }
cp "$src/secret_scan.sh" "$src/rustfmt_added.sh" "$src/watcher_independent.sh" tools/ci/
printf '# empty\n' > tools/ci/secret_scan_allow.txt
# An existing, unformatted file that the format check must leave alone.
printf 'pub fn  helper( ){}\n' > tests/common/mod.rs
# Long enough that git still sees a rename after one line is appended.
printf 'fn main() {\n    one();\n    two();\n}\n\nfn one() {}\n\nfn two() {}\n' > main.rs
printf '# This is a synthetic lock file.\nchecksum = "%s"\n' "$(hex 7e 32)" > Cargo.lock
g add -A
g commit -q -m base
base="$(git rev-parse HEAD)"

# ---- secret scan -----------------------------------------------------------
scan() { git add -A && tools/ci/secret_scan.sh; }
reset() { git reset -q --hard "$base" && git clean -fdxq; }

expect ok "clean tree" scan

printf 'a = "0x%s"\n' "$(hex ab 20)" > probe.toml
expect fail "address with 0x prefix" scan
reset
printf 'a = "%s"\n' "$(hex ef 20)" > probe.rs
expect fail "bare 40-digit hex" scan
reset
printf 'k = "%s"\n' "$(hex cd 32)" > probe.toml
expect fail "64-digit hex" scan
reset
printf 'sig 0x%s\n' "$(hex 9a 64)" > probe.md
expect fail "128-digit hex (two keys joined)" scan
reset
printf 'k = "%s"\n' "$(hex 1b 32)x" > probe.toml
printf 'probe.toml %s\n' "$(hex 1b 32)" >> tools/ci/secret_scan_allow.txt
expect ok "allow-listed value in the named file" scan
printf 'k = "%s"\n' "$(hex 1b 32)" > other.toml
expect fail "allow-listed value in another file" scan
reset
printf 'probe.toml %s\n' "$(hex 1b 32)" >> tools/ci/secret_scan_allow.txt
expect fail "stale allow-list entry" scan
reset
# The value is in the file, so only the line-format check can catch this.
printf 'k = "%s"\n' "$(hex 2f 32)" > probe.toml
printf 'probe.toml %s trailing-word\n' "$(hex 2f 32)" >> tools/ci/secret_scan_allow.txt
expect fail "malformed allow-list line" scan
reset
# On a comment line, so that only the credential check can catch it.
printf '# token gh%s_%s\n' p "$(hex Ab 18)" >> tools/ci/secret_scan_allow.txt
expect fail "credential inside the allow-list file" scan
reset
printf 'token = "gh%s_%s"\n' p "$(hex Ab 18)" > probe.toml
expect fail "GitHub token in an ordinary file" scan
reset
# The allow-list file is free text outside its entries. A value on a comment
# line is not an entry, so it must fail like a value in any other file.
printf '# %s\n' "$(hex 4c 32)" >> tools/ci/secret_scan_allow.txt
expect fail "64-digit hex on a comment line of the allow-list" scan
reset
printf '  # note %s\n' "$(hex 4c 32)" >> tools/ci/secret_scan_allow.txt
expect fail "64-digit hex on an indented comment line of the allow-list" scan
reset
# A comment may repeat a value that a valid entry of the file allows.
printf 'k = "%s"\n' "$(hex 1b 32)" > probe.toml
printf 'probe.toml %s\n# see %s\n' "$(hex 1b 32)" "$(hex 1b 32)" >> tools/ci/secret_scan_allow.txt
expect ok "comment that repeats the value of a valid allow-list entry" scan
reset
printf -- '-----BEGIN %s KEY-----\n' "RSA PRIVATE" > probe.txt
expect fail "PEM private key header" scan
reset
printf 'checksum = "%s"\n' "$(hex 3d 32)" >> Cargo.lock
expect ok "64-digit checksum in Cargo.lock" scan
reset
printf 'rev = "%s"\n' "$(hex 3d 20)" >> Cargo.lock
expect fail "40-digit hex in Cargo.lock" scan
reset
# The vendored lake contract: only its three exemptions, only under it.
mkdir -p contracts/lake/golden/l0/x contracts/lakeside
printf 'sha256 = "%s"\n' "$(hex 3d 32)" > contracts/lake/index.json
expect ok "64-digit digest in the vendored contract" scan
reset
mkdir -p contracts/lake
printf 'rev = "%s"\n' "$(hex 3d 20)" > contracts/lake/VENDOR.toml
expect fail "40-digit hex in the vendored contract" scan
reset
mkdir -p contracts/lake
printf 'sig %s\n' "$(hex 3d 64)" > contracts/lake/index.json
expect fail "128-digit hex in the vendored contract" scan
reset
mkdir -p contracts/lakeside
printf 'sha256 = "%s"\n' "$(hex 3d 32)" > contracts/lakeside/index.json
expect fail "64-digit hex in a directory that only starts like the contract" scan
reset
mkdir -p contracts/lake/golden/xrepo
printf '{}\n' > contracts/lake/golden/xrepo/manifest.jsonl
expect ok "golden manifest .jsonl in the vendored contract" scan
reset
mkdir -p contracts/lake/l1
printf '{}\n' > contracts/lake/l1/rows.jsonl
expect fail ".jsonl in the vendored contract outside golden/" scan
reset
mkdir -p contracts/lake/golden
printf 'text\n' > contracts/lake/golden/run.log
git add -f contracts/lake/golden/run.log
expect fail "log file in the vendored contract" tools/ci/secret_scan.sh
reset
mkdir -p contracts/lake/golden/l0/x
printf 'a\0b\n' > contracts/lake/golden/l0/x/00.qraw
expect ok "binary golden .qraw in the vendored contract" scan
reset
mkdir -p contracts/lake/golden/l0/x
printf 'a\0b k=%s\n' "$(hex cd 20)" > contracts/lake/golden/l0/x/00.qraw
expect fail "40-digit hex inside a golden .qraw" scan
reset
mkdir -p contracts/lake/golden/l0/x
printf 'a\0b token gh%s_%s\n' p "$(hex Ab 18)" > contracts/lake/golden/l0/x/00.qraw
expect fail "credential inside a golden .qraw" scan
reset
mkdir -p contracts/lake/l1
printf 'a\0b\n' > contracts/lake/l1/00.qraw
expect fail "binary .qraw in the vendored contract outside golden/l0/" scan
reset
mkdir -p contracts/lake/golden/l0/x
printf 'a\0b\n' > contracts/lake/golden/l0/x/00.dat
expect fail "binary file that is not .qraw under golden/l0/" scan
reset
printf 'a\0b\n' > 00.qraw
expect fail "binary .qraw outside the vendored contract" scan
reset
printf '\n\n' > blank.txt
expect ok "text file that holds only empty lines" scan
reset
printf 'a\0b\nkey %s\n' "$(hex 5c 32)" > probe.toml
expect fail "key in a file with a NUL byte" scan
reset
mkdir -p evidence/probe
printf '\0\1\2' > "evidence/probe/journal-$(printf '%020d' 1).oj"
expect fail "journal file" scan
reset
mkdir -p evidence/probe
printf 'text\n' > "evidence/probe/snapshot-$(printf '%020d' 10).snap"
expect fail "snapshot file (by name, even as text)" scan
reset
printf 'x\n' > .env
expect fail ".env file" scan
reset
printf 'x\n' > run.log
git add -f run.log
expect fail "log file" tools/ci/secret_scan.sh
reset
# One text file for each forbidden name, so that only the name rule can
# catch it.
for name in notes.pem id.key trades.jsonl "journal-$(printf '%020d' 2).oj" tail.at-8.1.bin snapshot.tmp .env.local data/readme.txt crates/x/data/readme.txt; do
  mkdir -p "$(dirname "$name")"
  printf 'text\n' > "$name"
  git add -f "$name"
  expect fail "forbidden name: $name" tools/ci/secret_scan.sh
  reset
done
# The name rule does not depend on letter case.
for name in probe.PEM run.LOG .ENV Id.Key Data/readme.txt; do
  mkdir -p "$(dirname "$name")"
  printf 'text\n' > "$name"
  git add -f "$name"
  expect fail "forbidden name in another letter case: $name" tools/ci/secret_scan.sh
  reset
done
# A path is text that the public can read, like file content.
printf 'text\n' > "k_$(hex 4c 32).txt"
expect fail "64-digit hex in a file name" scan
reset
mkdir -p "$(hex 4c 20)"
printf 'text\n' > "$(hex 4c 20)/readme.txt"
expect fail "40-digit hex in a directory name" scan
reset
printf 'text\n' > "$(printf 'gh%s_%s.txt' p "$(hex Ab 18)")"
expect fail "credential shape in a file name" scan
reset
# A symlink's target is stored in the repository but is not file content.
ln -s "$(hex 4c 32)" probe_link
expect fail "symlink whose target is a 64-digit hex value" scan
reset
ln -s main.rs probe_link
expect fail "symlink to an existing file (no symlink may be tracked)" scan
reset
# No hex and no forbidden name, so that only the binary check can catch it.
printf 'a\0b\n' > probe.dat
expect fail "binary file with no key and an ordinary name" scan
reset
# One sample for each credential shape. The filler has no hex run, and each
# sample is joined at run time, so this file holds no credential shape.
token() { # token <label> <first part> <second part> <filler>
  printf 'token = "%s%s%s"\n' "$2" "$3" "$4" > probe.toml
  expect fail "credential shape: $1" scan
  reset
}
token "AWS access key id" AK IA "$(hex ZQ 8)"
token "sk- key" s k- "$(hex Zq 10)"
token "Slack token" xo xb- "$(hex Zq 5)"
token "GitHub fine-grained token" github _pat_ "$(hex Zq 20)"
# File names that git prints in quoted form unless asked for raw paths.
printf 'k = "%s"\n' "$(hex 6d 32)" > '키.toml'
expect fail "key in a file with a non-ASCII name" scan
reset
mkdir -p '데이터'
printf 'k = "%s"\n' "$(hex 6d 32)" > '데이터/설정.toml'
expect fail "key under a directory with a non-ASCII name" scan
reset
printf 'k = "%s"\n' "$(hex 6d 32)" > "$(printf 'tab\tname.toml')"
expect fail "key in a file with a tab in its name" scan
reset
printf 'k = "%s"\n' "$(hex 6d 32)" > 'quote"back\slash.toml'
expect fail "key in a file with a quote and a backslash in its name" scan
reset
printf 'text\n' > '로그.log'
git add -f '로그.log'
expect fail "log file with a non-ASCII name" tools/ci/secret_scan.sh
reset
printf 'a\0b\n' > '자료.dat'
expect fail "binary file with a non-ASCII name" scan
reset
printf 'text\n' > '메모.md'
expect ok "clean text file with a non-ASCII name" scan
reset

# ---- format check of added files -------------------------------------------
fmt() { g add -A && g commit -q -m probe && tools/ci/rustfmt_added.sh "$base"; }
undo() { git reset -q --hard "$base" && git clean -fdxq; }

expect ok "no Rust file added" tools/ci/rustfmt_added.sh "$base"

printf 'mod common;\n\n#[test]\nfn probe() {\n    assert_eq!(1, 1);\n}\n' > tests/probe_slice.rs
expect ok "formatted new file whose child module is an unformatted old file" fmt
undo
printf 'mod common;\n\n#[test]\nfn  probe( ) {assert_eq!(1,1);}\n' > tests/probe_slice.rs
expect fail "unformatted new file" fmt
undo
git mv main.rs moved.rs
printf 'fn  added( ){}\n' >> moved.rs
g add -A && g commit -q -m probe
# The case is only worth something if git really reports a rename.
expect ok "git reports the move as a rename" bash -c 'git diff --name-status "$1"...HEAD | grep -qE "^R[0-9]+[[:space:]]main.rs[[:space:]]moved.rs$"' _ "$base"
expect fail "unformatted code in a renamed file" tools/ci/rustfmt_added.sh "$base"
undo
printf 'fn broken( {\n' > tests/probe_broken.rs
expect fail "new file that rustfmt cannot parse" fmt
undo
printf 'pub fn probe() {}\n' > 'tests/시험.rs'
expect ok "formatted new file with a non-ASCII name" fmt
undo
printf 'pub fn  probe( ){}\n' > 'tests/시험.rs'
expect fail "unformatted new file with a non-ASCII name" fmt
undo
printf 'pub fn  more( ){}\n' >> tests/common/mod.rs
expect ok "unformatted code added to an existing file (exempt by design)" fmt
undo

# ---- watcher independence --------------------------------------------------
# A synthetic workspace in ws/ (the fake Cargo.lock at the root of this
# repository is no lock of any workspace), with its own copy of the
# toolchain file. Every case starts from the base manifests, plants one
# change and regenerates the lock, so the result is the script's.
expect_exit() { # expect_exit <code> <label> <fixed text> <command...>
  local want="$1" label="$2" text="$3" got=0
  shift 3
  "$@" > "$tmp/out.txt" 2>&1 || got=$?
  if [[ "$got" == "$want" ]] && grep -qF -- "$text" "$tmp/out.txt" \
    && { [[ "$want" == 0 ]] || ! grep -qF "watcher_independent: ok" "$tmp/out.txt"; }; then
    echo "selftest: pass  (exit $want) $label"
  else
    echo "selftest: WRONG (wanted exit $want and \"$text\", got exit $got) $label"
    sed 's/^/    /' "$tmp/out.txt"
    failures=$((failures + 1))
  fi
}
expect_finding() { expect_exit 1 "$@"; }
ws_crates="watcher watcher-conformance oms-core md-core oms-journal oms-api obs-push http-fake helper"
ws_base() {
  rm -rf ws
  mkdir -p ws
  cp "$src/../../rust-toolchain.toml" ws/
  local c list=""
  for c in $ws_crates; do
    list+="\"$c\", "
    mkdir -p "ws/$c/src"
    : > "ws/$c/src/lib.rs"
    printf '[package]\nname = "%s"\nversion = "0.1.0"\nedition = "2024"\npublish = false\n' "$c" > "ws/$c/Cargo.toml"
  done
  printf '[workspace]\nresolver = "3"\nmembers = [%s]\n' "${list%, }" > ws/Cargo.toml
  printf '\n[dev-dependencies]\nhttp-fake = { path = "../http-fake" }\n' >> ws/watcher/Cargo.toml
  printf '\n[dev-dependencies]\nwatcher = { path = "../watcher" }\noms-core = { path = "../oms-core" }\n' >> ws/watcher-conformance/Cargo.toml
}
ws_lock() { (cd ws && cargo generate-lockfile --offline -q); }
indep() { tools/ci/watcher_independent.sh ws/Cargo.toml; }
dep() { # dep <crate> <section> <dependency>: appends a path dependency
  if [[ -n "$2" ]]; then printf '\n[%s]\n' "$2" >> "ws/$1/Cargo.toml"; fi
  printf '%s = { path = "../%s" }\n' "$3" "$3" >> "ws/$1/Cargo.toml"
}
if command -v cargo > /dev/null; then
  ws_base && ws_lock
  expect_exit 0 "watcher: the real shape (only watcher-conformance links oms-core)" "watcher_independent: ok" indep
  ws_base && dep watcher dependencies oms-core && ws_lock
  expect_finding "watcher: normal dependency oms-core" "declared: watcher -> oms-core (normal)" indep
  ws_base && dep watcher build-dependencies md-core && ws_lock
  expect_finding "watcher: build dependency md-core" "declared: watcher -> md-core (build)" indep
  # The base manifest ends in [dev-dependencies]; the line joins it.
  ws_base && dep watcher "" oms-journal && ws_lock
  expect_finding "watcher: dev dependency oms-journal" "declared: watcher -> oms-journal (dev)" indep
  ws_base && dep watcher "target.'cfg(unix)'.dependencies" oms-core && ws_lock
  expect_finding "watcher: unix target dependency oms-core" "declared: watcher -> oms-core (normal, target cfg(unix))" indep
  ws_base && dep watcher "target.'cfg(windows)'.dependencies" oms-core && ws_lock
  expect_finding "watcher: windows target dependency oms-core (seen on the host too)" "declared: watcher -> oms-core (normal, target cfg(windows))" indep
  ws_base && dep watcher dependencies helper && dep helper dependencies oms-api && ws_lock
  expect_finding "watcher: oms-api reached through helper" "reachable: watcher -> helper -> oms-api" indep
  ws_base && dep watcher dependencies obs-push && ws_lock
  expect_finding "watcher: another workspace crate (obs-push)" "path: watcher -> obs-push (normal)" indep
  ws_base && dep oms-api dependencies watcher && ws_lock
  expect_finding "watcher: oms-api depends on the watcher" "reverse: oms-api -> watcher (normal)" indep
  ws_base && ws_lock && printf '#[path = "../../oms-core/src/lib.rs"]\nmod x;\n' > ws/watcher/src/lib.rs
  expect_finding "watcher: #[path] to OMS source" "source: watcher/src/lib.rs:1" indep
  ws_base && ws_lock && mkdir -p ws/watcher/tests \
    && printf '\nconst _: &str = include_str!("../../oms-journal/src/lib.rs");\n' > ws/watcher/tests/probe.rs
  expect_finding "watcher: include_str! of OMS source in a test" "source: watcher/tests/probe.rs:2" indep
  ws_base && ws_lock && printf '#[path = "inner.rs"]\nmod inner;\n' > ws/watcher/src/lib.rs && : > ws/watcher/src/inner.rs
  expect_exit 0 "watcher: #[path] inside the package" "watcher_independent: ok" indep
  ws_base && ws_lock && printf '#[cfg_attr(unix, path = "../../oms-core/src/lib.rs")]\nmod x;\n' > ws/watcher/src/lib.rs
  expect_finding "watcher: path = in a cfg_attr" "source: watcher/src/lib.rs:1" indep
  ws_base && ws_lock && ln -s ../../oms-core/src/lib.rs ws/watcher/src/shared.rs
  expect_finding "watcher: symlink to OMS source" "symlink: watcher/src/shared.rs" indep
  # White space between # and [, and before the ! of a macro (review 1 S2).
  ws_base && ws_lock && printf '# [path = "../../oms-core/src/lib.rs"]\nmod x;\n' > ws/watcher/src/lib.rs
  expect_finding "watcher: # [path] with a space" "source: watcher/src/lib.rs:1" indep
  ws_base && ws_lock && printf 'const _: &str = include_str !("../../oms-core/src/lib.rs");\n' > ws/watcher/src/lib.rs
  expect_finding "watcher: include_str ! with a space" "source: watcher/src/lib.rs:1" indep
  # Rule 7: a target of the manifest whose source file lies outside the
  # package directory (review 1 S1).
  ws_base && printf '\n[lib]\npath = "../oms-core/src/lib.rs"\n' >> ws/watcher/Cargo.toml && ws_lock
  expect_finding "watcher: [lib] path to OMS source" "target: watcher lib watcher -> oms-core/src/lib.rs" indep
  ws_base && printf '\n[[test]]\nname = "t"\npath = "../oms-core/src/lib.rs"\n' >> ws/watcher/Cargo.toml && ws_lock
  expect_finding "watcher: [[test]] path to OMS source" "target: watcher test t -> oms-core/src/lib.rs" indep
  ws_base && sed -i 's|^publish = false$|publish = false\nbuild = "../oms-core/src/lib.rs"|' ws/watcher/Cargo.toml && ws_lock
  expect_finding "watcher: build script from OMS source" "target: watcher custom-build build-script-lib -> oms-core/src/lib.rs" indep
  ws_base && printf '\n[lib]\npath = "src/other.rs"\n' >> ws/watcher/Cargo.toml && : > ws/watcher/src/other.rs && ws_lock
  expect_exit 0 "watcher: [lib] path inside the package" "watcher_independent: ok" indep
  # The lock of the base case stays: --locked must refuse it.
  ws_base && ws_lock && dep watcher dependencies oms-core
  expect_exit 2 "watcher: a stale lock cannot be checked (not ok)" "watcher_independent: cannot check:" indep
  expect_exit 2 "watcher: a manifest that does not exist" "watcher_independent: cannot check:" \
    tools/ci/watcher_independent.sh ws/missing/Cargo.toml
  rm -rf ws
else
  echo "selftest: skip  watcher independence cases (no cargo on the path)"
fi

# ---- owner review ----------------------------------------------------------
# A second throwaway repository, or/, with the script, its list, a root
# manifest with lint and profile tables, and one crate whose lib.rs holds a
# crate-level deny block and a test. Every case starts from the base
# commit, plants one change, commits it and runs the script against the
# base, so a finding is the script's.
expect_owner() { # expect_owner <code> <label> <fixed text>
  expect_owner_run "$1" "$2" "$3" tools/ci/owner_review.sh "$obase"
}
expect_owner_run() { # expect_owner_run <code> <label> <fixed text> <command...>
  local want="$1" label="$2" text="$3" got=0
  shift 3
  "$@" > "$tmp/out.txt" 2>&1 || got=$?
  if [[ "$got" == "$want" ]] && grep -qF -- "$text" "$tmp/out.txt" \
    && { [[ "$want" == 0 ]] || ! grep -qF "owner_review: none" "$tmp/out.txt"; }; then
    echo "selftest: pass  (exit $want) $label"
  else
    echo "selftest: WRONG (wanted exit $want and \"$text\", got exit $got) $label"
    sed 's/^/    /' "$tmp/out.txt"
    failures=$((failures + 1))
  fi
}
mkdir -p "$tmp/or"
cd "$tmp/or"
git init -q .
mkdir -p tools/ci .github/workflows crates/a/src crates/a/tests
cp "$src/owner_review.sh" "$src/owner_review_paths.txt" tools/ci/
printf 'name: ci\n' > .github/workflows/ci.yml
printf '# empty\n' > tools/ci/secret_scan_allow.txt
printf 'echo gate\n' > tools/ci/gate.sh
printf '[workspace]\nmembers = ["crates/a"]\n\n[workspace.lints.clippy]\nunwrap_used = "warn"\n\n[profile.release]\noverflow-checks = true\npanic = "abort"\n' > Cargo.toml
printf '[package]\nname = "a"\n\n[lints]\nworkspace = true\n' > crates/a/Cargo.toml
printf '//! A.\n\n#![deny(\n    clippy::float_arithmetic,\n    clippy::expect_used\n)]\n\npub fn one() -> u8 {\n    1\n}\n\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn one_is_one() {\n        assert_eq!(super::one(), 1);\n    }\n\n    #[tokio::test]\n    async fn later() {}\n}\n' > crates/a/src/lib.rs
printf '#![allow(clippy::unwrap_used)]\n\n#[test]\nfn outside() {}\n' > crates/a/tests/t.rs
# A module file with a test, declared from lib.rs.
printf '\npub mod m;\n' >> crates/a/src/lib.rs
printf 'pub fn m() {}\n\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn m() {}\n}\n' > crates/a/src/m.rs
# A crate with a stricter lint table of its own.
mkdir -p crates/s/src; : > crates/s/src/lib.rs
printf '[package]\nname = "s"\n\n[lints.clippy]\nfloat_arithmetic = "deny"\n' > crates/s/Cargo.toml
g add -A
g commit -q -m base
obase="$(git rev-parse HEAD)"
oreset() { git reset -q --hard "$obase" && git clean -fdxq; }
plant() { git add -A && g commit -q -m plant; }

expect_owner 0 "owner: no change" "owner_review: none"
printf 'pub fn two() -> u8 {\n    2\n}\n' >> crates/a/src/lib.rs; plant
expect_owner 0 "owner: ordinary code change" "owner_review: none"
oreset; printf 'on: push\n' >> .github/workflows/ci.yml; plant
expect_owner 1 "owner: workflow change" "owner_review: path: .github/workflows/ci.yml"
oreset; printf 'x.toml %s\n' "$(hex 1b 32)" >> tools/ci/secret_scan_allow.txt; plant
expect_owner 1 "owner: allow-list entry added" "owner_review: path: tools/ci/secret_scan_allow.txt"
oreset; sed -i '/^\.github\/$/d' tools/ci/owner_review_paths.txt; printf 'on: push\n' >> .github/workflows/ci.yml; plant
expect_owner 1 "owner: an entry removed from the list (the list itself)" "owner_review: path: tools/ci/owner_review_paths.txt"
oreset; mkdir -p scripts; git mv tools/ci/gate.sh scripts/gate.sh; plant
expect_owner 1 "owner: a check moved out of tools/ci" "owner_review: path: tools/ci/gate.sh"
oreset; rm tools/ci/gate.sh; plant
expect_owner 1 "owner: a check deleted" "owner_review: path: tools/ci/gate.sh"
oreset; printf 'disallowed-methods = []\n' > clippy.toml; plant
expect_owner 1 "owner: clippy.toml added" "owner_review: path: clippy.toml"
oreset; printf 'x\n' > "tools/ci/caf$(printf '\xc3\xa9').txt"; plant
expect_owner 1 "owner: a file with a non-ASCII name under tools/ci" "owner_review: path: tools/ci/caf"
oreset; sed -i '/^overflow-checks = true$/d' Cargo.toml; plant
expect_owner 1 "owner: overflow-checks removed from the release profile" "owner_review: manifest: Cargo.toml"
oreset; sed -i 's/^unwrap_used = "warn"$/unwrap_used = "allow"/' Cargo.toml; plant
expect_owner 1 "owner: a workspace lint relaxed" "owner_review: manifest: Cargo.toml"
oreset; printf '[package]\nname = "a"\n' > crates/a/Cargo.toml; plant
expect_owner 1 "owner: a crate leaves the workspace lints" "owner_review: manifest: crates/a/Cargo.toml"
oreset; printf '\n[lints.clippy]\nfloat_arithmetic = "allow"\n' >> crates/a/Cargo.toml; plant
expect_owner 1 "owner: a crate lint table added" "owner_review: manifest: crates/a/Cargo.toml"
oreset; printf '[profile.dev.package.zstd-sys]\nopt-level = 3\n' >> Cargo.toml; plant
expect_owner 1 "owner: a profile table added" "owner_review: manifest: Cargo.toml"
oreset; sed -i 's/^members = \["crates\/a"\]$/members = ["crates\/a", "crates\/b"]/' Cargo.toml
mkdir -p crates/b/src; : > crates/b/src/lib.rs; printf '[package]\nname = "b"\n\n[lints]\nworkspace = true\n' > crates/b/Cargo.toml; plant
expect_owner 0 "owner: a new crate that takes the workspace lints" "owner_review: none"
oreset; mkdir -p crates/c/src; : > crates/c/src/lib.rs; printf '[package]\nname = "c"\n\n[lints.clippy]\nexpect_used = "allow"\n' > crates/c/Cargo.toml; plant
expect_owner 1 "owner: a new crate with its own lint table" "owner_review: manifest: crates/c/Cargo.toml"
oreset; printf '[package]\nname = "s"\n\n[lints]\nworkspace = true\n' > crates/s/Cargo.toml; plant
expect_owner 1 "owner: a crate swaps its stricter table for the workspace lints" "owner_review: manifest: crates/s/Cargo.toml"
oreset; sed -i '/^    clippy::expect_used$/d; s/^    clippy::float_arithmetic,$/    clippy::float_arithmetic/' crates/a/src/lib.rs; plant
expect_owner 1 "owner: a lint removed from a multi-line deny block" "owner_review: lint: crates/a/src/lib.rs"
oreset; sed -i 's|^//! A\.$|//! A.\n#![allow(clippy::expect_used)]|' crates/a/src/lib.rs; plant
expect_owner 1 "owner: a crate-level allow added" "owner_review: lint: crates/a/src/lib.rs"
oreset; sed -i 's|^//! A\.$|//! A.\n#![cfg_attr(test, allow(clippy::float_arithmetic))]|' crates/a/src/lib.rs; plant
expect_owner 1 "owner: a crate-level allow inside cfg_attr" "owner_review: lint: crates/a/src/lib.rs"
oreset; mkdir -p crates/a/src/m; printf '#![allow(clippy::float_arithmetic)]\n' > crates/a/src/m/mod.rs; plant
expect_owner 1 "owner: an inner allow at the top of a new module file" "owner_review: lint: crates/a/src/m/mod.rs"
oreset; printf '#[expect(clippy::float_arithmetic, reason = "monitoring only")]\npub fn half(x: f64) -> f64 {\n    x / 2.0\n}\n' >> crates/a/src/lib.rs; plant
expect_owner 0 "owner: an item-level expect with a reason" "owner_review: none"
oreset; printf '#![allow(clippy::unwrap_used)]\n\n#[test]\nfn more() {}\n' > crates/a/tests/t2.rs; plant
expect_owner 0 "owner: a new test file with the usual allow" "owner_review: none"
oreset; perl -0pi -e 's/    #\[test\]\n    fn one_is_one\(\) \{\n        assert_eq!\(super::one\(\), 1\);\n    \}\n\n//' crates/a/src/lib.rs; plant
expect_owner 1 "owner: a test removed" "owner_review: tests: 1 test attribute(s) removed, 0 added"
oreset; perl -0pi -e 's/    #\[tokio::test\]\n    async fn later\(\) \{\}\n//' crates/a/src/lib.rs; plant
expect_owner 1 "owner: a tokio test removed" "owner_review: tests: 1 test attribute(s) removed, 0 added"
oreset; perl -0pi -e 's/    #\[test\]\n    fn one_is_one\(\) \{\n        assert_eq!\(super::one\(\), 1\);\n    \}\n\n//' crates/a/src/lib.rs
printf '\n#[test]\nfn one_is_one() {\n    assert_eq!(a::one(), 1);\n}\n' >> crates/a/tests/t.rs; plant
expect_owner 0 "owner: a test moved to another file" "owner_review: none"
oreset; sed -i 's/^    #\[test\]$/    #[test]\n    #[ignore]/' crates/a/src/lib.rs; plant
expect_owner 1 "owner: an existing test ignored" "owner_review: tests: crates/a/src/lib.rs: 1 #[ignore] added, 0 test attribute(s) added"
oreset; printf '\n#[test]\n#[ignore = "measurement, run by hand"]\nfn slow() {}\n' >> crates/a/tests/t.rs; plant
expect_owner 0 "owner: a new test ignored from the start" "owner_review: none"
# Review 1 of ADR-033: ways to switch tests off without removing an attribute.
oreset; sed -i 's|^    #\[test\]$|    // #[test]|' crates/a/src/lib.rs; plant
expect_owner 1 "owner: a test attribute commented out" "owner_review: tests: 1 test attribute(s) removed, 0 added"
oreset; sed -i 's|^#\[cfg(test)\]$|#[cfg(all(test, any()))]|' crates/a/src/lib.rs; plant
expect_owner 1 "owner: the test module switched off by cfg" "owner_review: tests: crates/a/src/lib.rs: 1 cfg on tests added"
oreset; printf '\n#[cfg(any())]\n#[test]\nfn off() {}\n' >> crates/a/tests/t.rs; plant
expect_owner 1 "owner: a test behind cfg(any())" "owner_review: tests: crates/a/tests/t.rs: 1 cfg on tests added"
oreset; printf '\n#[cfg(test)]\nmod more {\n    #[test]\n    fn m() {}\n}\n' >> crates/a/src/lib.rs; plant
expect_owner 0 "owner: a new plain cfg(test) module" "owner_review: none"
oreset; printf '\n#[cfg(any(test, feature = "failpoints"))]\npub fn hook() {}\n' >> crates/a/src/lib.rs; plant
expect_owner 0 "owner: a widening cfg(any(test, ..)) on a test hook" "owner_review: none"
oreset; sed -i 's|^#\[cfg(test)\]$|#[cfg(not(test))]|' crates/a/src/lib.rs; plant
expect_owner 1 "owner: the test module put under cfg(not(test))" "owner_review: tests: crates/a/src/lib.rs: 1 cfg on tests added"
oreset; sed -i 's|^name = "a"$|name = "a"\nautotests = false|' crates/a/Cargo.toml; plant
expect_owner 1 "owner: autotests switched off" "owner_review: manifest: crates/a/Cargo.toml"
oreset; printf '\n[lib]\ntest = false\n' >> crates/a/Cargo.toml; plant
expect_owner 1 "owner: the lib target's tests switched off" "owner_review: manifest: crates/a/Cargo.toml"
oreset; printf '\n[[test]]\nname = "t"\nharness = false\n' >> crates/a/Cargo.toml; plant
expect_owner 1 "owner: a [[test]] target added" "owner_review: manifest: crates/a/Cargo.toml"
oreset; sed -i 's|^members = \["crates/a"\]$|members = []|' Cargo.toml; plant
expect_owner 1 "owner: a crate removed from the workspace" "owner_review: manifest: Cargo.toml: member removed: crates/a"
oreset; mkdir -p crates/z/src; : > crates/z/src/lib.rs; printf '[package]\nname = "z"\nlints = { clippy = { expect_used = "allow" } }\n' > crates/z/Cargo.toml; plant
expect_owner 1 "owner: a new crate with an inline lints key" "owner_review: manifest: crates/z/Cargo.toml"
oreset; printf '\n["lints".clippy]\nexpect_used = "allow"\n' >> crates/s/Cargo.toml; plant
expect_owner 1 "owner: a quoted lints table" "owner_review: manifest: crates/s/Cargo.toml"
oreset; sed -i 's|^pub fn one() -> u8 {$|#[expect(clippy::float_arithmetic, reason = "x")]\npub mod inner {}\n\npub fn one() -> u8 {|' crates/a/src/lib.rs; plant
expect_owner 1 "owner: an item-level expect on a mod item" "owner_review: lint: crates/a/src/lib.rs"
# Review 2 of ADR-033: a listed name without an inner "/" matches at any
# depth (clippy and rustfmt read the file nearest the crate, cargo every
# .cargo/ above the directory it runs in).
oreset; printf 'disallowed-methods = []\n' > crates/a/clippy.toml; plant
expect_owner 1 "owner: a nested clippy.toml" "owner_review: path: crates/a/clippy.toml"
oreset; printf 'nightly\n' > rust-toolchain; plant
expect_owner 1 "owner: a legacy rust-toolchain file" "owner_review: path: rust-toolchain"
oreset; printf 'edition = "2015"\n' > crates/a/.rustfmt.toml; plant
expect_owner 1 "owner: a nested .rustfmt.toml" "owner_review: path: crates/a/.rustfmt.toml"
oreset; mkdir -p crates/a/.cargo; printf '[build]\nrustflags = ["-Aclippy::all"]\n' > crates/a/.cargo/config.toml; plant
expect_owner 1 "owner: a nested .cargo/config.toml" "owner_review: path: crates/a/.cargo/config.toml"
oreset; printf 'x\n' > crates/a/myclippy.toml; mkdir -p docs/tools/ci; printf 'x\n' > docs/tools/ci/note.md; plant
expect_owner 0 "owner: a name that only ends in a listed name, a listed path below another directory" "owner_review: none"
# The layering and generated-docs checks are Rust code in xtask (review 2, S2).
oreset; mkdir -p crates/xtask/src; printf 'pub fn run() -> i32 {\n    0\n}\n' > crates/xtask/src/layers.rs; plant
expect_owner 1 "owner: the layering checker's code changed" "owner_review: path: crates/xtask/src/layers.rs"
oreset; mkdir -p crates/xtask/src; printf 'pub fn check() {}\n' > crates/xtask/src/registry.rs; plant
expect_owner 1 "owner: the generated-docs checker's code changed" "owner_review: path: crates/xtask/src/registry.rs"
oreset; mkdir -p crates/xtask/src; printf 'fn main() {}\n' > crates/xtask/src/main.rs; plant
expect_owner 1 "owner: the xtask command table changed" "owner_review: path: crates/xtask/src/main.rs"
oreset; mkdir -p crates/xtask/src; printf 'pub fn m() {}\n' > crates/xtask/src/mutants.rs; plant
expect_owner 0 "owner: another xtask file changed" "owner_review: none"
# A new crate that leaves out the workspace lints builds outside all of them.
oreset; mkdir -p crates/n/src; : > crates/n/src/lib.rs; printf '[package]\nname = "n"\n' > crates/n/Cargo.toml; plant
expect_owner 1 "owner: a new crate without [lints] workspace = true" "owner_review: manifest: crates/n/Cargo.toml"
oreset; mkdir -p crates/n/src; : > crates/n/src/lib.rs; printf '[package]\nname = "n"\n\n[lints]\nworkspace = false\n' > crates/n/Cargo.toml; plant
expect_owner 1 "owner: a new crate with workspace = false" "owner_review: manifest: crates/n/Cargo.toml"
oreset; mkdir -p crates/n/src; : > crates/n/src/lib.rs; printf "[package]\nname = \"n\"\n\n[lints]\nworkspace = true\n\n['lints'.clippy]\nunwrap_used = \"allow\"\n" > crates/n/Cargo.toml; plant
expect_owner 1 "owner: a new crate with a single-quoted lints table" "owner_review: manifest: crates/n/Cargo.toml"
oreset; mkdir -p fixtures/w; printf '[workspace]\nmembers = []\n' > fixtures/w/Cargo.toml; plant
expect_owner 0 "owner: a new manifest without [package]" "owner_review: none"
# Tests switched off without removing an attribute (review 2, S1).
oreset; sed -i 's/^    #\[test\]$/    #[test]\n    #[cfg_attr(unix, ignore)]/' crates/a/src/lib.rs; plant
expect_owner 1 "owner: an existing test ignored inside cfg_attr" "owner_review: tests: crates/a/src/lib.rs: 1 #[ignore] added, 0 test attribute(s) added"
oreset; printf '\n#[test]\n#[cfg_attr(miri, ignore)]\nfn slow() {}\n' >> crates/a/tests/t.rs; plant
expect_owner 0 "owner: a new test ignored inside cfg_attr from the start" "owner_review: none"
oreset; sed -i 's/^#\[test\]$/#[cfg(feature = "never")]\n#[test]/' crates/a/tests/t.rs; plant
expect_owner 1 "owner: cfg(feature) above an existing test in a test file" "owner_review: tests: crates/a/tests/t.rs: 1 cfg added in a test file, 0 test attribute(s) added"
oreset; sed -i '1s/^/#![cfg(feature = "never")]\n/' crates/a/tests/t.rs; plant
expect_owner 1 "owner: #![cfg(feature)] at the top of an existing test file" "owner_review: tests: crates/a/tests/t.rs: 1 cfg added in a test file, 0 test attribute(s) added"
oreset; printf '#![cfg(unix)]\n#![allow(clippy::unwrap_used)]\n\n#[test]\nfn on_unix() {}\n' > crates/a/tests/t3.rs; plant
expect_owner 0 "owner: a new test file under #![cfg(unix)]" "owner_review: none"
oreset; mkdir -p crates/a/tests/old; git mv crates/a/tests/t.rs crates/a/tests/old/t.rs; plant
expect_owner 1 "owner: a test file moved where cargo does not build it" "owner_review: tests: test target removed: crates/a/tests/t.rs"
oreset; sed -i '/^pub mod m;$/d' crates/a/src/lib.rs; plant
expect_owner 1 "owner: a mod declaration removed, its file with tests kept" "owner_review: tests: crates/a/src/lib.rs: mod m removed, crates/a/src/m.rs still holds tests"
oreset; sed -i 's/^pub mod m;$/mod m;/' crates/a/src/lib.rs; plant
expect_owner 0 "owner: a mod declaration made private" "owner_review: none"
# Ordinary changes that must not call the owner (review 2, S7).
oreset; printf '\n#[cfg(not(feature = "testnet"))]\npub fn mainnet_only() {}\n' >> crates/a/src/lib.rs; plant
expect_owner 0 "owner: cfg(not(feature = \"testnet\")) (the word test only)" "owner_review: none"
oreset; mkdir -p crates/n/src; printf '//! N.\n\n#![cfg_attr(test, allow(clippy::unwrap_used))]\n\npub fn n() {}\n' > crates/n/src/lib.rs
printf '[package]\nname = "n"\n\n[lints]\nworkspace = true\n' > crates/n/Cargo.toml; plant
expect_owner 0 "owner: a new crate whose lib.rs takes the usual test-only unwrap allow" "owner_review: none"
oreset; mkdir -p crates/n/src; printf '#![cfg_attr(test, allow(clippy::unwrap_used))]\n#![allow(clippy::expect_used)]\n' > crates/n/src/lib.rs
printf '[package]\nname = "n"\n\n[lints]\nworkspace = true\n' > crates/n/Cargo.toml; plant
expect_owner 1 "owner: a new crate's lib.rs with another allow" "owner_review: lint: crates/n/src/lib.rs"
oreset; sed -i 's|^//! A\.$|//! A.\n#![cfg_attr(test, allow(clippy::unwrap_used))]|' crates/a/src/lib.rs; plant
expect_owner 1 "owner: the usual test-only unwrap allow added to an existing crate" "owner_review: lint: crates/a/src/lib.rs"
oreset; mkdir -p crates/a/src/q; printf '#![cfg_attr(test, allow(clippy::unwrap_used))]\n' > crates/a/src/q/mod.rs; plant
expect_owner 1 "owner: the usual test-only unwrap allow in a new module of an existing crate" "owner_review: lint: crates/a/src/q/mod.rs"
# The paths file as the header shows it, from a pipe, and a relative one
# named from a subdirectory (review 2, S3).
oreset; printf 'on: push\n' >> .github/workflows/ci.yml; plant
expect_owner_run 1 "owner: script and list from git show, as the header shows" "owner_review: path: .github/workflows/ci.yml" \
  bash -c 'bash <(git show HEAD:tools/ci/owner_review.sh) "$1" <(git show HEAD:tools/ci/owner_review_paths.txt)' _ "$obase"
cp tools/ci/owner_review_paths.txt crates/a/list.txt
expect_owner_run 1 "owner: a relative paths file named from a subdirectory" "owner_review: path: .github/workflows/ci.yml" \
  bash -c 'cd crates/a && ../../tools/ci/owner_review.sh "$1" list.txt' _ "$obase"
expect_owner_run 2 "owner: an empty paths file from a pipe (a failed git show)" "owner_review: cannot check: no entry in paths file" \
  bash -c 'tools/ci/owner_review.sh "$1" <(git show HEAD:no/such/file 2> /dev/null)' _ "$obase"
oreset; cp tools/ci/owner_review_paths.txt "$tmp/base_paths.txt"; sed -i '/^\.github\/$/d' tools/ci/owner_review_paths.txt
printf 'on: push\n' >> .github/workflows/ci.yml; plant
expect_owner_with() { # the list from the base, as the runner passes it
  local got=0
  tools/ci/owner_review.sh "$obase" "$tmp/base_paths.txt" > "$tmp/out.txt" 2>&1 || got=$?
  if [[ "$got" == 1 ]] && grep -qF "owner_review: path: .github/workflows/ci.yml" "$tmp/out.txt"; then
    echo "selftest: pass  (exit 1) owner: the base list still sees what the change dropped from its own list"
  else
    echo "selftest: WRONG (wanted exit 1 and the workflow line, got exit $got) owner: base list"
    sed 's/^/    /' "$tmp/out.txt"
    failures=$((failures + 1))
  fi
}
expect_owner_with
rm tools/ci/owner_review_paths.txt
expect_owner 2 "owner: a paths file that does not exist" "owner_review: cannot check: no paths file"
# A failed read is "cannot check", never "none" (review 2, S4). An object
# that the change needs is taken out of the repository for one run and
# put back; a tool is left off the path.
oreset; printf 'pub fn two() -> u8 {\n    2\n}\n' >> crates/a/src/lib.rs; plant
without_object() { # without_object <rev:path> <code> <label> <text>: runs with that object gone
  local oid obj
  oid="$(git rev-parse "$1")"; obj=".git/objects/${oid:0:2}/${oid:2}"
  mv "$obj" "$tmp/object"
  expect_owner "$2" "$3" "$4"
  mv "$tmp/object" "$obj"
}
without_object HEAD:crates/a/src/lib.rs 2 "owner: a changed file whose content is missing" "owner_review: cannot check:"
without_object HEAD:crates/a/src 2 "owner: a changed directory whose tree is missing (git diff fails)" "owner_review: cannot check:"
for t in perl awk; do
  mkdir -p "$tmp/no-$t"
  for c in git sed awk perl comm sort grep; do
    [[ "$c" == "$t" ]] || ln -sf "$(command -v "$c")" "$tmp/no-$t/$c"
  done
  expect_owner_run 2 "owner: no $t on the path" "owner_review: cannot check: no $t" \
    env PATH="$tmp/no-$t" "$BASH" tools/ci/owner_review.sh "$obase"
done
oreset
obase=not-a-commit
expect_owner 2 "owner: a base that is no commit" "owner_review: cannot check:"
cd "$repo"

if [[ $failures -ne 0 ]]; then
  echo "selftest: FAILED ($failures case(s))"
  exit 1
fi
echo "selftest: ok"
