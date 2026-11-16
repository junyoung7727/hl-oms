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
printf 'fn main() {}\n' > main.rs
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
printf 'token gh%s_%s\n' p "$(hex Ab 18)" >> tools/ci/secret_scan_allow.txt
expect fail "credential inside the allow-list file" scan
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
expect fail "unformatted code in a renamed file" fmt
undo
printf 'pub fn  more( ){}\n' >> tests/common/mod.rs
expect ok "unformatted code added to an existing file (exempt by design)" fmt
undo

if [[ $failures -ne 0 ]]; then
  echo "selftest: FAILED ($failures case(s))"
  exit 1
fi
echo "selftest: ok"
