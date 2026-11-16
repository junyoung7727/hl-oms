# 03: Leverage gate: a plain order cannot take the account above 2x

**What to build:** A plain order is rejected with `AccountLeverage` when it would raise account exposure above the limit times the account value. Exposure counts the position and every resting order, reduce-only included, at the higher of limit price and reference price, per instrument, summed over instruments. A resting order keeps its exposure until terminal evidence; the OMS never reprices or expires it. A price move above the limit latches nothing: a plain order that lowers exposure passes. The status shows the account exposure. Scenario rows: S1, S2, S5, S6, S7, S20. Claim: V-MARGIN-3. Worked numbers: account value 100, limit 2.0, reference price 100.

**Blocked by:** 02

**Status:** ready-for-agent

- [ ] Test `order_past_twice_the_account_value_is_rejected_account_leverage` (seam A, S5): fails when a buy of 2.01 at 100 is accepted.
- [ ] Test `order_exactly_at_the_limit_is_accepted` (seam A, S5): fails when a buy of 2.00 at 100 is rejected.
- [ ] Test `an_unfilled_resting_order_keeps_its_exposure_and_is_never_repriced_or_expired_by_the_oms` (seam A, S1): the test feeds a fresh margin report and a fresh reference price at every poll interval over hours of ticks (so the stale cancel of ticket 06 does not apply), and fails when the order then has a cancel, a new price, or no longer counts in the gate.
- [ ] Test `resting_order_counts_at_the_higher_of_limit_and_reference` (seam A, S1): fails when a sell at 150 counts at 100, or a buy at 50 counts at 50.
- [ ] Test `reduce_only_and_plain_buy_on_a_short_count_at_full_reach` (seam A, S6): fails when short 5 with a reduce-only buy 5 and a plain buy 8 counts as 5 and not 8.
- [ ] Test `exposure_is_the_sum_over_instruments` (seam A, S5): fails when two instruments at 1.5x each pass a 2x limit.
- [ ] Test `partial_fill_then_cancel_frees_exposure_only_on_terminal_evidence` (seam A, S2): fails when a cancel request alone frees room for the next order.
- [ ] Test `a_venue_rejection_of_an_accepted_order_frees_its_exposure` (seam A, S20): fails when an order the venue rejected (for example for margin) still counts, or the reject text is interpreted.
- [ ] Test `after_a_passive_breach_a_plain_reducing_order_passes_and_an_increasing_one_does_not` (seam A, S7): fails when a price move above 2x rejects a plain sell that lowers exposure, accepts one that raises it, or latches anything.
- [ ] Test `reduce_only_order_skips_the_leverage_gate` (seam A, S7): fails when a reduce-only order is rejected with `AccountLeverage`, or is left out of the sum for the next plain order.
- [ ] Test `plain_order_is_rejected_when_an_exposed_instrument_has_no_fresh_reference_price` (seam A): fails when exposure on an instrument without a fresh reference price counts as zero.
- [ ] Test `account_value_zero_or_negative_rejects` (seam A): fails when a plain order passes with account value 0 or below.
- [ ] Test `adding_an_order_never_lowers_exposure` (seam A, property, G3): fails when any generated accepted order lowers the published account exposure.
- [ ] Test `mirrored_books_have_equal_exposure_and_equal_decisions` (seam A, property, G3): fails when swapping every side and the sign of every position changes the exposure or a decision.
- [ ] Test `exposure_is_exact_at_extreme_price_and_quantity` (seam A, property, G3): fails when an overflow or a rounding flips a decision near the limit; expected values come from independent `i128` multiplication on literals.
- [ ] Test `status_shows_account_exposure` (seam C): fails when the exposure is missing from the status.
- [ ] Mutants: the limit comparison (`>` against `≥`), the "after greater than before" clause, the `max(limit, reference)` valuation, and the sum over instruments are each caught by a named test.
- [ ] Stated and not proven here: exposure after a price move is not watched between orders; mid is not mark; the venue may refuse an order the gate accepts (fees at the edge).
- [ ] Every test above was written first and seen red for the reason named, then made green (tdd skill); one commit per red→green cycle.
- [ ] The events and claims this ticket adds are listed under C-MARGIN in the registry; the contract's verification section names the real tests; a renamed test is also renamed in this file and in the scenario table of `spec-ko.md`.
- [ ] The existing tests are green and unchanged in meaning; with no margin policy the behaviour is the same as before this ticket.
- [ ] Full local gate passes: workspace tests, clippy with warnings denied, generated documents in step, extended verification run without the live-testnet suite with evidence committed, secret scan.

## Comments

- Spec review r1 (reviewer claude-opus-5-5, head 37c6330), 2026-11-16. Finding 7 (nit, correct): the S1 test ran hours of ticks without saying that reports keep arriving, so the stale cancel of ticket 06 would cancel the order. Resolution: the test feeds a fresh margin report and reference price at every poll interval. The S1 row of spec-ko.md is qualified the same way and points to S23.
