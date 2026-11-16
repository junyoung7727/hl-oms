# 12: Final verification: performance, mutants, documents

**What to build:** Before the final gate the whole feature is measured and its documents state the truth. The performance claims are rerun with the leverage gate in the hot path, including a core benchmark case with the margin policy on and 500 resting orders. The full mutant run covers the rule code. The README status, the scope document, the verification documents, the runbook, the contract's verification section and the scenario table of the Korean spec say what is implemented, by which test, and what is not verified. Scenario rows: all, as a cross-check of S1–S25 against real test names. Claims: V-PERF-1 to V-PERF-5, V-MUT-1.

**Blocked by:** 01, 02, 03, 04, 05, 06, 07, 08, 09, 10, 11

**Status:** ready-for-agent

- [ ] The core benchmark has a case with the margin policy on and about 500 resting orders; its median and p99 decision times are recorded as evidence and written beside the earlier figures (median 22 µs, p99 29 µs at 500 open orders without the gate).
- [ ] Claims V-PERF-1 to V-PERF-5 are rerun on the current source and are `passed`, not `stale`; a claim that no longer meets its criterion is a finding written in this file under `## Comments` with the numbers.
- [ ] The full mutant run is executed on the current source; every mutant on the margin, latch, stale, MM-buffer, hold-table and venue-setting code is caught; a surviving mutant is fixed with a new failing-first test or written under `## Comments` with the reason. Claim V-MUT-1 is `passed`.
- [ ] Every row S1–S25 of the scenario table in `spec-ko.md` names a test that exists under that exact name, and the marker "planned" is removed from rows whose test exists.
- [ ] The contract's verification section lists the real tests per rule and repeats what is not verified (T1–T11).
- [ ] The README status paragraph, the scope document, the verification overview and the runbook state: margin rules implemented; not run against any real venue; the testnet rehearsal and T1–T11 are required before mainnet and need the owner's approval; claim V-HL-3 is not run and why.
- [ ] Full local gate passes: workspace tests, clippy with warnings denied, generated documents in step, extended verification run without the live-testnet suite with evidence committed, secret scan.
