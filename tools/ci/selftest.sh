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

if [[ $failures -ne 0 ]]; then
  echo "selftest: FAILED ($failures case(s))"
  exit 1
fi
echo "selftest: ok"
