# 05: Operator clears REDUCING and rebases the day; latch survives kill and crash

**What to build:** The operator ends REDUCING with `clear-reduce --note` and resets the day base with `rebase-day --note`, from the operator socket only. A clear takes the current account value as the new day base, so a further 5 % loss latches again. The commands are refused on stale data or without a note. REDUCING and the day base survive a kill of the node process and a crash at each commit boundary; rule cancels that were not sent are sent after restart. The runbook gains the operator procedures (release, after a deposit or withdrawal). Scenario rows: S8, S13, S14, S15. Claims: V-MARGIN-6, V-MARGIN-11.

**Blocked by:** 04

**Status:** claimed

- [x] Test `clear_reduce_is_refused_when_stale_or_without_a_note_or_when_not_reducing` (seam A, S14): fails when any of the three is accepted, or the refusal reason is not `MarginUnavailable`, `NoteRequired`, `NotReducing`.
- [x] Test `clear_reduce_rebases_the_day_and_a_further_loss_latches_again` (seam A, S14): fails when after a clear at 94 a report at 94 or at 89.30 re-latches, or one at 89.29 does not.
- [x] Test `rebase_day_sets_the_base_and_leaves_the_latch_alone` (seam A, S15): fails when the base does not move, or the latch changes.
- [x] Test `a_withdrawal_without_rebase_reads_as_a_loss` (seam A, S15): fails when a drop in account value caused by a withdrawal stops latching. The unsafe side (a deposit hides a loss) is a stated non-guarantee.
- [x] Test `strategy_socket_cannot_clear_reducing_or_rebase` (seam C, S14): fails when either command works from the strategy socket.
- [ ] Test `omsctl_clear_reduce_and_rebase_day_work_end_to_end` (seam C, S14): fails when the commands, their replies or the status fields (state, rule, day base) are not wired.
- [ ] Test `reducing_and_day_base_survive_a_node_kill` (seam C, S13): fails when after a kill and restart the status shows no latch or a new base.
- [ ] Test `reducing_cancels_every_resting_plain_order_through_the_node` (seam C, S8, G7): fails when with many resting plain orders at the simulated venue one of them is still open after entry, or a reduce-only one is cancelled.
- [ ] Test `stopped_before_the_write_of_a_reducing_decision_the_next_report_decides_again` (seam C, crash point, G4): fails when after restart the latch or its cancels exist without a journal record, or the next breaching report does not latch.
- [ ] Test `stopped_after_the_reducing_decision_is_durable_the_cancels_are_sent_after_restart` (seam C, crash point, G4): fails when after restart the latch is missing, or a recorded rule cancel is never sent.
- [ ] Test `stopped_around_a_day_base_set_the_base_is_set_exactly_once` (seam C, crash point, G4): fails when after restart the base is missing, doubled, or differs from the report that set it.
- [ ] The crash-point mechanism fires on a margin-report batch; this is test-only code behind the existing feature.
- [ ] Mutants: each clear precondition and the rebase inside the clear are each caught by a named test.
- [ ] Stated and not proven here: that the operator judged well; disk loss and power failure.
- [ ] Every test above was written first and seen red for the reason named, then made green (tdd skill); one commit per red→green cycle.
- [ ] The events and claims this ticket adds are listed under C-MARGIN in the registry; the contract's verification section names the real tests; a renamed test is also renamed in this file and in the scenario table of `spec-ko.md`.
- [ ] The existing tests are green and unchanged in meaning; with no margin policy the behaviour is the same as before this ticket.
- [ ] Full local gate passes: workspace tests, clippy with warnings denied, generated documents in step, extended verification run without the live-testnet suite with evidence committed, secret scan.

## Comments

- Spec review r1 (reviewer claude-opus-5-5, head 37c6330), 2026-11-16. Finding 5 (nit, correct): copies of the rules differed. Resolution: ADR-018 §7 item 7 now names the NotReducing precondition. spec-ko.md says that `rebase-day` and `clear-reduce` take the report's day, also an earlier one. The contract's "the day never goes backwards" is narrowed to the report-driven roll, in the contract, spec.md and spec-ko.md.
