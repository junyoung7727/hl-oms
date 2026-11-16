#!/usr/bin/env bash
# Fails when a tracked file looks like it holds a key, a wallet address, a
# journal or a log. This repository is public.
#
# It reads tracked files only (`git ls-files`). It is a pattern check: it
# does not prove that no secret exists, only that none has a known shape.
# A key split over lines, encoded in base64 or written as a mnemonic passes.
set -euo pipefail
cd "$(git rev-parse --show-toplevel)"

# Every run of exactly 40 or 64 hex digits (an address or a 32-byte key,
# with or without a 0x prefix) must be listed here as "<path> <value>".
ALLOW_FILE="tools/ci/secret_scan_allow.txt"
# Cargo.lock holds 64-digit package checksums. Only that length is exempt.
LOCK_FILE="Cargo.lock"

fail=0
report() {
  echo "secret-scan: $1"
  fail=1
}

declare -A allowed=()
while read -r path value _; do
  [[ -z "$path" || "$path" == \#* ]] && continue
  allowed["$path $value"]=1
done < "$ALLOW_FILE"

mapfile -t files < <(git ls-files)

# 1. File names that must never be tracked.
for f in "${files[@]}"; do
  if [[ "$f" =~ (^|/)\.env(\..*)?$ || "$f" =~ \.(log|pem|key|jsonl)$ || "$f" =~ (^|/)data/ ]]; then
    report "forbidden file is tracked: $f"
  fi
done

# 2. Credentials with a fixed shape.
TOKENS='-----BEGIN [A-Z ]*PRIVATE KEY-----|AKIA[0-9A-Z]{16}|gh[pousr]_[A-Za-z0-9]{36}|github_pat_[A-Za-z0-9_]{40,}|xox[baprs]-[A-Za-z0-9-]{10,}|sk-[A-Za-z0-9_-]{20,}'

for f in "${files[@]}"; do
  [[ -f "$f" && "$f" != "$ALLOW_FILE" ]] || continue
  # Skip binary files.
  grep -Iq . -- "$f" || continue

  # 3. Hex runs of exactly 40 or 64 digits, each one checked by value.
  while read -r value; do
    [[ -z "$value" ]] && continue
    if [[ "$f" == "$LOCK_FILE" && ${#value} -eq 64 ]]; then
      continue
    fi
    if [[ -z "${allowed["$f $value"]:-}" ]]; then
      line=$(grep -inF -- "$value" "$f" | head -1 | cut -d: -f1)
      report "${#value}-digit hex value not on the allow-list: $f:${line:-?}"
    fi
  done < <(tr -c '0-9a-fA-F' '\n' < "$f" | grep -xE '[0-9a-fA-F]{40}|[0-9a-fA-F]{64}' | tr 'A-F' 'a-f' | sort -u || true)

  if grep -IqE -e "$TOKENS" -- "$f"; then
    report "credential-shaped value in $f"
  fi
done

if [[ $fail -ne 0 ]]; then
  echo "secret-scan: FAILED"
  exit 1
fi
echo "secret-scan: ok (${#files[@]} tracked files, ${#allowed[@]} allow-listed values)"
