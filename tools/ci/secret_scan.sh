#!/usr/bin/env bash
# Fails when a tracked file looks like it holds a key, a wallet address, a
# journal, a snapshot or a log. This repository is public.
#
# What it checks, on tracked files only (`git ls-files -z`):
#   1. names: .env files, logs, key files, data/ directories, and this
#      project's journal, snapshot and torn-tail files, in any letter case;
#      no symlink may be tracked (its target is not file content);
#   2. binary files: none may be tracked unless listed in ALLOW_BINARY;
#   3. hex: every run of 40 or more hex digits (an address, a 32-byte key,
#      or several of them joined) must be on the allow-list, by value. In
#      the allow-list file itself, comments included, a run passes only if
#      it is the value of a valid entry. In a path, no run passes;
#   4. credentials with a fixed shape (PEM, AWS, GitHub, Slack, sk-), in
#      file content and in paths.
#
# What it does not check: it is a pattern check and does not prove that no
# secret exists. A key split over lines, written with separators, encoded
# in base64 or given as a mnemonic passes. Untracked files are not read.
set -euo pipefail
cd "$(git rev-parse --show-toplevel)"

# "<path> <value>" lines. See the header of that file.
ALLOW_FILE="tools/ci/secret_scan_allow.txt"
# Cargo.lock holds 64-digit package checksums. Only that length is exempt.
LOCK_FILE="Cargo.lock"
# Tracked binary files. Empty on purpose: a binary file cannot be read by
# a reviewer or by this scan. Add a path only with a reason in the PR.
ALLOW_BINARY=()

fail=0
report() {
  echo "secret-scan: $1"
  fail=1
}

hex_runs_in() { # prints each distinct hex run of 40+ digits on stdin, lower case
  tr -c '0-9a-fA-F' '\n' | grep -xE '[0-9a-fA-F]{40,}' | tr 'A-F' 'a-f' | sort -u || true
}
hex_runs() { hex_runs_in < "$1"; } # the same, for file $1

TOKENS='-----BEGIN [A-Z ]*PRIVATE KEY-----|AKIA[0-9A-Z]{16}|gh[pousr]_[A-Za-z0-9]{36}|github_pat_[A-Za-z0-9_]{40,}|xox[baprs]-[A-Za-z0-9-]{10,}|sk-[A-Za-z0-9_-]{20,}'

# -z: raw paths. Without it git prints a path that holds a non-ASCII byte, a
# tab, a quote or a backslash as a quoted string, and no check would read
# that file.
mapfile -d '' -t files < <(git ls-files -z)

# 1. File names that must never be tracked. The journal crate writes
#    journal-<seq>.oj, snapshot-<seq>.snap, <name>.at-<offset>.<n>.bin and
#    .tmp files. The match is on the lower-case path, so .PEM or .ENV
#    does not pass. A path is public text too, so it gets the hex and the
#    credential checks, with no allow-list.
for f in "${files[@]}"; do
  lower="${f,,}"
  if [[ "$lower" =~ (^|/)\.env(\..*)?$ || "$lower" =~ \.(log|pem|key|jsonl|oj|snap|bin|tmp)$ || "$lower" =~ (^|/)data/ ]]; then
    report "forbidden file is tracked: $f"
  fi
  if [[ -L "$f" ]]; then
    report "symlink is tracked: $f"
  fi
  if [[ -n "$(printf '%s\n' "$f" | hex_runs_in)" ]]; then
    report "hex value of 40 or more digits in a path: ${f:0:24}…"
  fi
  if printf '%s\n' "$f" | grep -qE -e "$TOKENS"; then
    report "credential-shaped value in a path: ${f:0:24}…"
  fi
done

# The allow-list: every line must name a tracked file that still holds the value.
declare -A allowed=()
declare -A allowed_values=() # the value of every valid entry, in any file
while read -r path value extra; do
  [[ -z "$path" || "$path" == \#* ]] && continue
  if [[ -n "$extra" || ! "$value" =~ ^[0-9a-f]{40,}$ ]]; then
    report "allow-list line is not '<path> <lower-case hex of 40+ digits>': $path"
    continue
  fi
  if [[ ! -f "$path" ]] || ! hex_runs "$path" | grep -qxF -- "$value"; then
    report "allow-list entry is stale, the value is not in the file: $path ${value:0:8}…"
    continue
  fi
  allowed["$path $value"]=1
  allowed_values["$value"]=1
done < "$ALLOW_FILE"

for f in "${files[@]}"; do
  [[ -f "$f" && -s "$f" ]] || continue

  # 2. Binary files are not skipped: they fail. Binary means: holds a NUL
  #    byte. (`grep -I` would also call a file of empty lines binary.)
  if ! LC_ALL=C tr -d '\000' < "$f" | cmp -s - "$f"; then
    listed=0
    for a in "${ALLOW_BINARY[@]}"; do [[ "$f" == "$a" ]] && listed=1; done
    [[ $listed -eq 1 ]] || report "binary file is tracked: $f"
    continue
  fi

  # 3. Hex runs, each one checked by value. The allow-list file is not
  #    exempt: it is free text outside its entries (comments), so a run in
  #    it passes only if it is the value of a valid entry, and those values
  #    were checked against their files above.
  while read -r value; do
    [[ -z "$value" ]] && continue
    [[ "$f" == "$LOCK_FILE" && ${#value} -eq 64 ]] && continue
    if [[ "$f" == "$ALLOW_FILE" ]]; then
      [[ -n "${allowed_values["$value"]:-}" ]] && continue
      line=$(grep -inF -- "$value" "$f" | head -1 | cut -d: -f1)
      report "${#value}-digit hex value in the allow-list file is not the value of a valid entry: $f:${line:-?}"
    elif [[ -z "${allowed["$f $value"]:-}" ]]; then
      line=$(grep -inF -- "$value" "$f" | head -1 | cut -d: -f1)
      report "${#value}-digit hex value not on the allow-list: $f:${line:-?}"
    fi
  done < <(hex_runs "$f")

  # 4. Credentials with a fixed shape. The allow-list file is not exempt.
  if grep -qE -e "$TOKENS" -- "$f"; then
    report "credential-shaped value in $f"
  fi
done

if [[ $fail -ne 0 ]]; then
  echo "secret-scan: FAILED"
  exit 1
fi
echo "secret-scan: ok (${#files[@]} tracked files, ${#allowed[@]} allow-listed values)"
