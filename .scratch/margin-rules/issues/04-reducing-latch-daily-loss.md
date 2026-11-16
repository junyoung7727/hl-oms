# 04: Daily loss enters REDUCING and cancels resting plain orders

**What to build:** When a margin report shows the account more than 5 % below the day base, the account enters REDUCING: every resting plain order gets a rule cancel, new plain orders are rejected with `Hold(Reducing)`, reduce-only orders are accepted and kept. The day base is journal state: the first report sets it, a later UTC day rolls it after the report was judged against the old base, a restart keeps it. A new day does not clear REDUCING. The status shows the state, the rule and the day base. Rule cancels carry the actor `Rule(DailyLoss)`. Scenario rows: S4, S8, S9, S10, S12, S13, S19, S24. Claims: V-MARGIN-4, V-MARGIN-5, V-REPLAY-1. Worked numbers: base 100; 95.00 does not trip, 94.99 does.

**Blocked by:** 01, 02

**Status:** ready-for-agent

- [ ] Test `first_report_sets_the_day_base` (seam A): fails when the first report emits no `day_base_set`.
- [ ] Test `loss_greater_than_five_percent_of_the_base_enters_reducing` (seam A, S8): fails when 94.99 does not latch, or 95.00 does.
- [ ] Test `entering_reducing_cancels_plain_orders_and_keeps_reduce_only_ones` (seam A, S8, S24): fails when a resting plain order (an unflagged take-profit included) gets no cancel by `Rule(DailyLoss)`, or a reduce-only order gets one.
- [ ] Test `under_reducing_a_plain_order_is_rejected_and_a_reduce_only_one_is_accepted` (seam A, S8): fails when either half is wrong, also for a plain order that would reduce the position.
- [ ] Test `an_order_pending_send_at_entry_is_cancelled_once_acknowledged` (seam A, S9): fails when the cancel is lost, or is sent before the acknowledgement.
- [ ] Test `a_rule_cancel_that_races_a_fill_books_the_fill` (seam A, S4): fails when a fill that arrives after the rule cancel is not booked, or the final status does not decide.
- [ ] Test `a_stuck_rule_cancel_does_not_block_reduce_only_orders` (seam A, S10): fails when a stuck cancel after a rule cancel rejects a reduce-only order.
- [ ] Test `reducing_is_entered_once` (seam A, S8): fails when a second breaching report emits a second entry event or new cancels.
- [ ] Test `first_report_after_midnight_is_judged_against_the_old_base_then_rolls_the_day` (seam A, S12): fails when a loss taken across midnight is absorbed into the new base; also for a gap of three days, which rolls once.
- [ ] Test `a_new_day_does_not_clear_reducing` (seam A, S12): fails when the latch is gone after the roll.
- [ ] Test `a_report_of_an_earlier_day_does_not_change_the_day_base` (seam A, G5): fails when a report dated a day before the base re-bases.
- [ ] Test `a_loss_between_two_reports_enters_reducing_on_the_next_report` (seam A, G9): fails when reports at 100 and then 88 do not latch on the second report. It documents that 12 % was already lost.
- [ ] Test `an_external_fill_raises_a_mismatch_and_a_loss_report_enters_reducing` (seam A, S19): fails when after an external fill and a loss report the account is not REDUCING with a mismatch open, or a reduce-only order is then refused.
- [ ] Test `latch_and_day_base_replay_to_the_same_state` (seam A, S13): fails when replay loses the latch or re-bases the day.
- [ ] Mutants: the loss comparison, the order "judge, then roll", the entry cancel filter (reduce-only kept), the single-entry guard and the `Reducing` row of the hold table are each caught by a named test.
- [ ] Stated and not proven here: a rule cancel can lose the race to a fill; losses between two reports and losses while reports are missing are not stopped; the shape of a real liquidation fill (T6).
- [ ] Every test above was written first and seen red for the reason named, then made green (tdd skill); one commit per red→green cycle.
- [ ] The events and claims this ticket adds are listed under C-MARGIN in the registry; the contract's verification section names the real tests; a renamed test is also renamed in this file and in the scenario table of `spec-ko.md`.
- [ ] The existing tests are green and unchanged in meaning; with no margin policy the behaviour is the same as before this ticket.
- [ ] Full local gate passes: workspace tests, clippy with warnings denied, generated documents in step, extended verification run without the live-testnet suite with evidence committed, secret scan.
