# 07: MM buffer: a position near liquidation enters REDUCING

**What to build:** When a margin report shows an isolated position whose isolated margin is below (maintenance margin rate + 0.25 percentage points) of its value, the whole account enters REDUCING with rule `MmBuffer`. The maintenance margin rate comes from the position's maximum leverage in the same report. The operator cannot clear REDUCING while the condition holds. Scenario rows: S11, S14, S16. Claims: V-MARGIN-8, V-MARGIN-6, V-MARGIN-4. Worked numbers: maximum leverage 40, buffer 25 bps, position value 200; isolated margin 2.99 trips, 3.00 does not.

**Blocked by:** 05

**Status:** claimed

- [x] Test `isolated_margin_below_mmr_plus_buffer_enters_reducing` (seam A, S16): fails when 2.99 does not latch with `Rule(MmBuffer)`, or 3.00 does. Does not prove that the venue's margin field is the isolated equity (T1).
- [x] Test `mmr_follows_the_max_leverage_of_each_position` (seam A, S16): fails when a position with maximum leverage 25 is judged at 1.25 % and not at 2 %.
- [x] Test `a_trigger_on_one_instrument_blocks_plain_orders_on_all` (seam A, S16): fails when a plain order on another instrument is accepted.
- [ ] Test `a_cross_position_is_not_judged_by_the_mm_rule` (seam A): fails when a cross-margin position trips the rule (it is the venue-setting hold's case).
- [ ] Test `clear_reduce_is_refused_while_the_mm_condition_holds` (seam A, S14): fails when the clear succeeds, or the refusal reason is not `MmBufferActive`. There is no way out of a false latch; that is a stated non-guarantee.
- [ ] Test `reducing_records_the_rule_that_tripped_first` (seam A, S16): fails when a later MM trigger overwrites `DailyLoss` in the latch, or the reverse.
- [ ] Test `an_old_report_on_arrival_is_still_judged_by_the_mm_rule` (seam A, S11): an accepted report whose query time is 40 s before the record time carries isolated margin 2.99 on position value 200 at maximum leverage 40. Fails when the account does not enter REDUCING with `Rule(MmBuffer)` on that report.
- [ ] Test `mm_comparison_is_exact_at_extreme_values` (seam A, property, G3): fails when an overflow or a rounding flips the result near the boundary; expected values come from independent `i128` multiplication on literals.
- [ ] Mutants: an MM judgement that runs only on a fresh report, the MM comparison direction, the buffer term and the factor 2 × maximum leverage are each caught by a named test.
- [ ] Stated and not proven here: margin tiers (T9); the rule fires 0.13–0.37 % of price before liquidation and a 5 s poll can step over it.
- [ ] Every test above was written first and seen red for the reason named, then made green (tdd skill); one commit per red→green cycle.
- [ ] The events and claims this ticket adds are listed under C-MARGIN in the registry; the contract's verification section names the real tests; a renamed test is also renamed in this file and in the scenario table of `spec-ko.md`.
- [ ] The existing tests are green and unchanged in meaning; with no margin policy the behaviour is the same as before this ticket.
- [ ] Full local gate passes: workspace tests, clippy with warnings denied, generated documents in step, extended verification run without the live-testnet suite with evidence committed, secret scan.

## Comments

- Spec review r1 (reviewer claude-opus-5-5, head 37c6330), 2026-11-16. Finding 2: this ticket gains the test `an_old_report_on_arrival_is_still_judged_by_the_mm_rule`, one mutant and scenario row S11. See ticket 04 for the resolution.
