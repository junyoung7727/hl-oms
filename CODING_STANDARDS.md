# Coding standards (read by the reviewer)

The review agent applies these rules to a diff. The implementing agent does
not need to load this file. Only judgement calls live here: a mistake that
a fixed pattern can catch gets a check instead (ADR-033; see
`docs/agents/retro.md`). Each rule names the evidence that put it here.

## 1. Every rule the change states has a test that fails without it

A sentence in a contract, an ADR, a claim (`registry/claims.toml`) or a
comment that says the code does X must be backed by a test that fails when
X is removed. Name the test in the review; if none exists, it is a SHOULD.
A guard (`if`, a bound, a refusal) that survives its own removal is the
same finding.

Evidence: 34 of the 70 MUST/SHOULD findings in the runner's 29 reviews up
to 2026-11-24 (U18b–U19b) were "a rule without a test" or "survives
removal".

## 2. Numbers and claims in documents match the code and the tests

A number in `docs/` or `README.md` (a size, a bound, a count, a time) must
be the one the code uses or a test measures, at this commit. Recompute it;
do not trust the report of the implementing agent.

Evidence: 13 of the 70 findings were a document, a claim or a README line
that said more than, or other than, the code.

## 3. Time: name which clock, and what a backward step does

Code that reads time says whether it reads the wall clock or a steady
clock, and what happens when the wall clock steps back. The pure crates
take time as an input (ADR-002; enforced by the `disallowed_methods` lint).

Evidence: 7 of the 70 findings were about time and clocks.
