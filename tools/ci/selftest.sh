#!/usr/bin/env bash
# Proves that the two gate scripts go red on the cases they exist for and
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
cp "$src/secret_scan.sh" "$src/rustfmt_added.sh" tools/ci/
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
printf -- '-----BEGIN %s KEY-----\n' "RSA PRIVATE" > probe.txt
expect fail "PEM private key header" scan
reset
printf 'checksum = "%s"\n' "$(hex 3d 32)" >> Cargo.lock
expect ok "64-digit checksum in Cargo.lock" scan
reset
printf 'rev = "%s"\n' "$(hex 3d 20)" >> Cargo.lock
expect fail "40-digit hex in Cargo.lock" scan
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

if [[ $failures -ne 0 ]]; then
  echo "selftest: FAILED ($failures case(s))"
  exit 1
fi
echo "selftest: ok"
