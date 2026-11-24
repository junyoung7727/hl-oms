# Weekly retro: turn repeated corrections into checks

Once a week (and after any owner correction), read what went wrong and
change the environment, not the prompt (ADR-033 part 4; Matt Pocock's
`retro` skill).

## Inputs

- The runner's reviews and fixes: `parts/<PART>/review*.md`,
  `report_impl.md` and `orch.log` of the runner (outside this repository).
- The owner's corrections in chat.
- CI failures of the week (`gh run list --status failure`).

## Rule

1. Classify each repeated finding.
   - **Mechanical** (a fixed pattern: a banned API, an import, a file
     location, a dependency direction, a string in the wrong crate): add a
     check (a clippy lint in a crate's `lib.rs`, a row in
     `tools/ci/layers.toml`, or a script in `tools/ci/`) and, in the same
     change, a planted case that goes red without it (`tools/ci/selftest.sh`
     for scripts, a unit test in `crates/xtask/src/layers.rs` for the
     table). Do not write a sentence for it.
   - **Judgement**: add or sharpen a rule in `CODING_STANDARDS.md`, with
     the evidence (count and period).
2. Start with what appeared twice or more.
3. A change to a check is an owner-review finding
   (`tools/ci/owner_review.sh`); the owner sees it before it merges.
4. Remove a rule from `CODING_STANDARDS.md` when a check now covers it.

## Measure

Count, per week, the findings that repeat a class already covered by a
check or a rule. The count should fall. Keep the counts in the retro's
change description.
