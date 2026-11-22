#!/usr/bin/env bash
# Proves that the watcher shares no code with the OMS (ADR-031, C-WATCH
# part 1): the package `watcher` depends on no oms-* or md-* crate and on
# no other workspace crate, nothing in the workspace but
# watcher-conformance depends on it, and neither its sources nor its
# manifest's targets pull in a file from outside its own directory.
#
# usage: watcher_independent.sh [manifest]   (default ./Cargo.toml)
#
# exit 0: no finding, prints "watcher_independent: ok"
# exit 1: one line per finding, never "ok"
# exit 2: it could not check ("watcher_independent: cannot check: <reason>")
#
# Rules 2, 3 and 5 read the declared dependencies of every target
# (packages[].dependencies: a cfg(windows) dependency is seen on Linux too).
# Rule 4 walks resolve.nodes, the resolved graph of the host platform only
# (--filter-platform, so that --offline needs only the packages a host
# build fetched): a package reachable only through a dependency of another
# platform is not walked; rule 2 still sees it when the watcher declares it.
# Rule 6 is a plain text search: a match in a comment is a finding too.
# Rule 7 reads the targets of the watcher's manifest (packages[].targets).
set -uo pipefail

prefix="watcher_independent"
cannot() { # cannot <reason> [details file]
  echo "$prefix: cannot check: $1"
  if [[ -n "${2:-}" && -s "$2" ]]; then sed 's/^/  /' "$2"; fi
  exit 2
}

manifest="${1:-./Cargo.toml}"
command -v jq > /dev/null || cannot "jq is not on the path"
command -v cargo > /dev/null || cannot "cargo is not on the path"
[[ -f "$manifest" ]] || cannot "no manifest at $manifest"
mdir="$(cd "$(dirname "$manifest")" && pwd)" || cannot "cannot enter the directory of $manifest"
mabs="$mdir/$(basename "$manifest")"

work="$(mktemp -d)" || cannot "mktemp failed"
trap 'rm -rf "$work"' EXIT

# rustc and cargo run in the manifest's directory, so that its
# rust-toolchain.toml selects the toolchain.
host="$(cd "$mdir" && rustc -vV 2> "$work/rustc.err" | sed -n 's/^host: //p')"
[[ -n "$host" ]] || cannot "rustc -vV gave no host: line" "$work/rustc.err"
if ! (cd "$mdir" && cargo metadata --format-version 1 --locked --offline --filter-platform "$host" \
  --manifest-path "$mabs" > "$work/meta.json" 2> "$work/cargo.err"); then
  cannot "cargo metadata failed" "$work/cargo.err"
fi
jq -e 'type == "object" and (.packages | type == "array") and (.workspace_members | type == "array")
  and (.resolve.nodes | type == "array") and (.workspace_root | type == "string")' "$work/meta.json" > /dev/null 2>&1 \
  || cannot "cargo metadata did not give the expected JSON"

# Rules 1 to 5, one line per finding.
if ! jq -r '
  def kind: .kind // "normal";
  def where: kind + (if .target then ", target " + .target else "" end);
  def banned: test("^(oms|md)-");
  . as $m
  | ($m.workspace_members) as $members
  | ([$m.packages[] | select(.id as $id | $members | index($id))]) as $ws
  | ([$ws[] | select(.name == "watcher")] | first) as $w
  | if $w == null then "missing: package watcher"
    else
      ( [$w.dependencies[] | select(.name | banned) | "declared: watcher -> \(.name) (\(where))"] | sort[] ),
      ( [$w.dependencies[] | select(.path != null) | select((.name == "http-fake" and kind == "dev") | not)
          | "path: watcher -> \(.name) (\(where))"] | sort[] ),
      ( ($m.packages | map({key: .id, value: .name}) | from_entries) as $name
        | ($m.resolve.nodes | map({key: .id, value: .}) | from_entries) as $node
        | {queue: [$w.id], seen: {($w.id): [$w.id]}}
        | until(.queue | length == 0;
            .queue[0] as $cur
            | .queue |= .[1:]
            | reduce ($node[$cur].deps[]?
                | select(any(.dep_kinds[]; .kind == null or .kind == "build")) | .pkg) as $n (.;
                if .seen[$n] then . else .seen[$n] = (.seen[$cur] + [$n]) | .queue += [$n] end))
        | [ .seen | to_entries[] | select(.key != $w.id)
            | select(($name[.key] | banned) or (.key as $k | $members | index($k)))
            | "reachable: " + (.value | map($name[.]) | join(" -> ")) ] | sort[] ),
      ( [$ws[] | select(.name != "watcher" and .name != "watcher-conformance") | .name as $p
          | .dependencies[] | select(.name == "watcher") | "reverse: \($p) -> watcher (\(where))"] | sort[] )
    end
' "$work/meta.json" > "$work/findings.txt" 2> "$work/jq.err"; then
  cannot "jq failed" "$work/jq.err"
fi

root="$(jq -r '.workspace_root' "$work/meta.json")"
wmanifest="$(jq -r '. as $m | [$m.packages[] | select(.name == "watcher" and (.id as $id | $m.workspace_members | index($id)))][0].manifest_path // ""' "$work/meta.json")"

pkg=""
[[ -n "$wmanifest" ]] && pkg="$(dirname "$wmanifest")"
rel() { realpath -m -s --relative-to="$root" "$1"; }
inside() { # inside <path>: the lexically normalised path stays under $pkg
  local p
  p="$(realpath -m -s "$1")"
  [[ "$p" == "$pkg" || "$p" == "$pkg"/* ]]
}

# Rule 6: source pulled in by path. Every *.rs file under the watcher's
# directory; #[path, include!, include_str!, include_bytes! and `path =`
# inside an attribute, with any white space between # ! [ and before the
# macro's ! (`# [path = …]`, `include_str !(…)`). The argument must be one
# plain string literal, relative, and stay inside the package directory.
# Every symlink under the directory is a finding, whatever it points at.
attr='#[[:space:]]*!?[[:space:]]*\['
inc='include(_str|_bytes)?[[:space:]]*!'
if [[ -n "$wmanifest" ]]; then
  while IFS= read -r -d '' link; do
    echo "symlink: $(rel "$link")"
  done < <(find "$pkg" -path "$pkg/target" -prune -o -type l -print0) >> "$work/findings.txt"
  while IFS= read -r -d '' f; do
    dir="$(dirname "$f")"
    while IFS= read -r hit; do
      line="${hit%%:*}"
      text="${hit#*:}"
      ok=1
      inc_count="$(grep -oE "$inc" <<< "$text" | wc -l)"
      path_count="$(grep -oE "$attr[[:space:]]*path|$attr[^]]*\\bpath[[:space:]]*=" <<< "$text" | wc -l)"
      if (( inc_count + path_count != 1 )); then
        ok=0
      elif (( inc_count == 1 )); then
        arg="$(sed -nE 's/.*include(_str|_bytes)?[[:space:]]*![[:space:]]*\([[:space:]]*"([^"\\]*)"[[:space:]]*\).*/\2/p' <<< "$text")"
        [[ -n "$arg" ]] || ok=0
      else
        arg="$(sed -nE 's/.*\bpath[[:space:]]*=[[:space:]]*"([^"\\]*)"[[:space:]]*[]),].*/\1/p' <<< "$text")"
        [[ -n "$arg" ]] || ok=0
      fi
      if (( ok == 1 )) && { [[ "$arg" == /* ]] || ! inside "$dir/$arg"; }; then
        ok=0
      fi
      if (( ok == 0 )); then
        echo "source: $(rel "$f"):$line"
      fi
    done < <(grep -nE "$attr[[:space:]]*path|$inc|$attr[^]]*\\bpath[[:space:]]*=" "$f")
  done < <(find "$pkg" -path "$pkg/target" -prune -o -type f -name '*.rs' -print0) >> "$work/findings.txt"
fi

# Rule 7: every target of the watcher's manifest (lib, bins, tests,
# benches, examples, the build script) has its source file inside the
# package directory. `[lib] path = "../oms-core/src/lib.rs"` or
# `build = "…"` would compile OMS code with no dependency edge and no
# #[path] (review 1 S1). A file that the target's own module tree pulls in
# is rule 6's.
if [[ -n "$pkg" ]]; then
  jq -er --arg m "$wmanifest" '[.packages[] | select(.manifest_path == $m) | .targets[]
    | [(.kind | join(",")), .name, .src_path] | @tsv] | if length > 0 then .[] else error("no target") end' \
    "$work/meta.json" > "$work/targets.tsv" 2> "$work/jq.err" || cannot "jq failed on the targets" "$work/jq.err"
  while IFS=$'\t' read -r kind name src; do
    inside "$src" || echo "target: watcher $kind $name -> $(rel "$src")"
  done < "$work/targets.tsv" >> "$work/findings.txt"
fi

if [[ -s "$work/findings.txt" ]]; then
  sed "s/^/$prefix: /" "$work/findings.txt"
  exit 1
fi
echo "$prefix: ok"
exit 0
