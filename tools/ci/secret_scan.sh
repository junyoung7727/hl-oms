#!/usr/bin/env bash
# Fails when a tracked file looks like it holds a key, a wallet address, a
# journal or a log. This repository is public.
#
# It reads tracked files only (`git ls-files`). It is a pattern check: it
# does not prove that no secret exists, only that none has a known shape.
set -euo pipefail
cd "$(git rev-parse --show-toplevel)"

# Files that hold public SDK test vectors or documentation examples. A new
# entry needs a reason in the PR that adds it.
ALLOW_HEX=(
  "crates/oms-node/src/venue/hyperliquid/sign.rs"
  "crates/oms-node/tests/hl_live.rs"
  "config/hyperliquid-testnet.example.toml"
  "docs/research/hyperliquid-protocol.md"
)
# Cargo.lock holds 64-digit package checksums and nothing else of that shape.
ALLOW_BARE64=("Cargo.lock")

fail=0
report() {
  echo "secret-scan: $1"
  fail=1
}

allowed() {
  local file="$1"
  shift
  local a
  for a in "$@"; do
    [[ "$file" == "$a" ]] && return 0
  done
  return 1
}

mapfile -t files < <(git ls-files)

# 1. File names that must never be tracked.
for f in "${files[@]}"; do
  if [[ "$f" =~ (^|/)\.env(\..*)?$ || "$f" =~ \.(log|pem|key|jsonl)$ || "$f" =~ (^|/)data/ ]]; then
    report "forbidden file is tracked: $f"
  fi
done

# 2. A 20-byte address or a 32-byte key in hex.
ADDRESS='0x[0-9a-fA-F]{40}([^0-9a-fA-F]|$)'
KEY64='(^|[^0-9a-fA-F])[0-9a-fA-F]{64}([^0-9a-fA-F]|$)'
# 3. Credentials with a fixed shape.
TOKENS='-----BEGIN [A-Z ]*PRIVATE KEY-----|AKIA[0-9A-Z]{16}|gh[pousr]_[A-Za-z0-9]{36}|github_pat_[A-Za-z0-9_]{40,}|xox[baprs]-[A-Za-z0-9-]{10,}|sk-[A-Za-z0-9_-]{20,}'

for f in "${files[@]}"; do
  [[ -f "$f" ]] || continue
  if ! allowed "$f" "${ALLOW_HEX[@]}"; then
    if grep -IqE "$ADDRESS" -- "$f"; then
      report "address-shaped value in $f: $(grep -InE "$ADDRESS" -- "$f" | cut -d: -f1 | head -3 | tr '\n' ' ')(line numbers)"
    fi
    if ! allowed "$f" "${ALLOW_BARE64[@]}" && grep -IqE "$KEY64" -- "$f"; then
      report "64-digit hex value in $f: $(grep -InE "$KEY64" -- "$f" | cut -d: -f1 | head -3 | tr '\n' ' ')(line numbers)"
    fi
  fi
  if grep -IqE -e "$TOKENS" -- "$f"; then
    report "credential-shaped value in $f"
  fi
done

if [[ $fail -ne 0 ]]; then
  echo "secret-scan: FAILED"
  exit 1
fi
echo "secret-scan: ok (${#files[@]} tracked files)"
