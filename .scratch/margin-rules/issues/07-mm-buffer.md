# 07: MM buffer: a position near liquidation enters REDUCING

**What to build:** When a margin report shows an isolated position whose isolated margin is below (maintenance margin rate + 0.25 percentage points) of its value, the whole account enters REDUCING with rule `MmBuffer`. The maintenance margin rate comes from the position's maximum leverage in the same report. The operator cannot clear REDUCING while the condition holds. Scenario rows: S11, S14, S16. Claims: V-MARGIN-8, V-MARGIN-6, V-MARGIN-4. Worked numbers: maximum leverage 40, buffer 25 bps, position value 200; isolated margin 2.99 trips, 3.00 does not.

**Blocked by:** 05

**Status:** ready-for-merge

- [x] Test `isolated_margin_below_mmr_plus_buffer_enters_reducing` (seam A, S16): fails when 2.99 does not latch with `Rule(MmBuffer)`, or 3.00 does. Does not prove that the venue's margin field is the isolated equity (T1).
- [x] Test `mmr_follows_the_max_leverage_of_each_position` (seam A, S16): fails when a position with maximum leverage 25 is judged at 1.25 % and not at 2 %.
- [x] Test `a_trigger_on_one_instrument_blocks_plain_orders_on_all` (seam A, S16): fails when a plain order on another instrument is accepted.
- [x] Test `a_cross_position_is_not_judged_by_the_mm_rule` (seam A): fails when a cross-margin position trips the rule (it is the venue-setting hold's case).
- [x] Test `clear_reduce_is_refused_while_the_mm_condition_holds` (seam A, S14): fails when the clear succeeds, or the refusal reason is not `MmBufferActive`. There is no way out of a false latch; that is a stated non-guarantee.
- [x] Test `reducing_records_the_rule_that_tripped_first` (seam A, S16): fails when a later MM trigger overwrites `DailyLoss` in the latch, or the reverse.
- [x] Test `an_old_report_on_arrival_is_still_judged_by_the_mm_rule` (seam A, S11): an accepted report whose query time is 40 s before the record time carries isolated margin 2.99 on position value 200 at maximum leverage 40. Fails when the account does not enter REDUCING with `Rule(MmBuffer)` on that report.
- [x] Test `mm_comparison_is_exact_at_extreme_values` (seam A, property, G3): fails when an overflow or a rounding flips the result near the boundary; expected values come from independent `i128` multiplication on literals.
- [x] Mutants: an MM judgement that runs only on a fresh report, the MM comparison direction, the buffer term and the factor 2 × maximum leverage are each caught by a named test.
- [x] Stated and not proven here: margin tiers (T9); the rule fires 0.13–0.37 % of price before liquidation and a 5 s poll can step over it.
- [x] Every test above was written first and seen red for the reason named, then made green (tdd skill); one commit per red→green cycle.
- [x] The events and claims this ticket adds are listed under C-MARGIN in the registry; the contract's verification section names the real tests; a renamed test is also renamed in this file and in the scenario table of `spec-ko.md`.
- [x] The existing tests are green and unchanged in meaning; with no margin policy the behaviour is the same as before this ticket.
- [ ] Full local gate passes: workspace tests, clippy with warnings denied, generated documents in step, extended verification run without the live-testnet suite with evidence committed, secret scan.

## Comments

- Spec review r1 (reviewer claude-opus-5-5, head 37c6330), 2026-11-16. Finding 2: this ticket gains the test `an_old_report_on_arrival_is_still_judged_by_the_mm_rule`, one mutant and scenario row S11. See ticket 04 for the resolution.
- Implementer (claude-fable-5-2), 2026-11-16, branch `t07-mm-buffer`. Built:
  - `RuleId::MmBuffer`. `judge_mm_buffer` runs on every recorded report, after the record and before the daily loss (`decide.rs`). It judges isolated positions with a position value above zero, also on instruments that are not configured, and enters REDUCING through the existing `enter_reducing` (so the rule cancels carry `Rule(MmBuffer)`). The `detail` names the first position of the report that is under its line.
  - `MarginRefusal::MmBufferActive` (`mm_buffer_active: ...`). `clear-reduce` reads the condition from the last recorded report with the same function as the rule. `rebase-day` does not look at it.
  - Eight tests in `crates/oms-core/tests/margin.rs` (names as planned, none renamed), claim V-MARGIN-8 (new), tests added to V-MARGIN-4 and V-MARGIN-6, no new event. Mutants M100–M108 (nine: the four of the checklist, plus a comparison that includes the line, a cross position judged, an overflowing line read as enough margin, a clear while the condition holds, the daily loss judged first). Each killed with `xtask mutants --only M10`; M50–M59 and M60–M66 run again after the change and the merge, all killed. The merger renumbers M100–M108 (mutant list, `docs/contracts/margin.md` verification rows, `docs/verification/README.md` rows and the paragraph under them).
  - Documents: C-MARGIN contract, runbook 7.8 (`mm_buffer`, `mm_buffer_active`), mutant table, registry.
- Red first: tests 1, 2, 4, 5 and 8 failed on their own before the code. Tests 3, 6 and 7 pin behaviour that already held (the account-wide latch and the entered-once guard of ticket 04, no freshness condition in the judgement); each was seen red by a temporary, uncommitted mutation, named in its commit body. The filter "position value above zero" was written in the first cycle without its own red test; test 8 pins it and its lines were seen red by a mutation (commit body of that cycle).
- Choices where the spec says nothing (smallest reasonable; the owner can change them):
  - Order of the refusal codes of `clear-reduce`: `not_reducing`, `margin_unavailable`, `mm_buffer_active`, `note_required` (the order of the contract's table). So an empty note on a position under its line answers `mm_buffer_active`.
  - Overflow: the left side of the comparison always fits in an `i128`. The right side can pass it only with a buffer of billions of bps (the node's validation has no upper bound on `mm_buffer_bps`); the position is then read as under the line (exact, because the line is then above every margin that can be written). No fault, no rejected report.
  - A negative margin is under every line. A position value of zero or below is not judged.
- No code was changed because it conflicted with the spec. Merge of `margin-rules` at 640a2aa (ticket 06): both kept; `RuleId` is `DailyLoss`, `MmBuffer`, `MarginStale`.
- Gate on the branch tip: workspace tests 265 passed, 0 failed; clippy clean; generated documents in step; secret scan ok; rustfmt-added ok. Extended verification and evidence are left to the merger.
- Not guaranteed: that the venue's `marginUsed` of an isolated position is its equity (T1; if not, the rule can be wrong in both directions and a false latch cannot be cleared while it reads true); margin tiers (T9); the rule fires 0.13–0.37 % of price before liquidation and a 5 s poll can step over it; nothing is judged while no new report is accepted. There is no process-level test of this rule: the simulated venue reports no positions (ticket 10), so `status` showing `mm_buffer` and `omsctl` printing `mm_buffer_active` are untested paths that pass the core's own serialisation and text through. When the venue stops listing the position (closed or liquidated), the condition reads false and the clear is allowed.
- Merger (claude-fable-5-2), 2026-11-16. Merge of `margin-rules` at e6f02d9 (tickets 08 and 09) into `t07-mm-buffer`:
  - Mutants M100–M108 are now M80–M88, in the same order, after ticket 09's M67–M79 (`crates/xtask/src/mutants.rs`, `docs/contracts/margin.md` verification rows, `docs/verification/README.md` rows and paragraph). The numbers M100–M108 in the implementer's comment above are the old ones. Each of M80–M88 was run with `xtask mutants --only M8` on the merged branch: nine killed.
  - Conflicts were in the mutant list, the claim list (V-MARGIN-8 and V-MARGIN-10 both kept), the contract entry of C-MARGIN (code list of ticket 09, claims of both), the contract's "what is in the code" list and the mutant table. No conflict in Rust code outside the mutant list. The hand-written mutant total is 88.
  - Contract, "not yet in the code": the MM buffer (this ticket), the Hyperliquid queries and the query weight check (ticket 09) are no longer listed. What is left is the simulated venue reporting positions (ticket 10).
  - No test needed a changed setup: no test of ticket 07, 08 or 09 was edited. Workspace tests on the merged branch: 275 passed, 0 failed, 0 ignored. Clippy with warnings denied is clean.
