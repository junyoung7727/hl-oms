# 06: Stale cancel: resting plain orders are cancelled after 60 s without a report

**What to build:** When the link is up and no margin report has been accepted for more than 60 seconds, the tick requests one rule cancel, actor `Rule(MarginStale)`, for every resting plain order. Reduce-only orders stay. Nothing latches: the next accepted report restores normal acceptance. A node that never received a report measures the age from the moment its link session came up. Scenario row: S23. Claim: V-MARGIN-7.

**Blocked by:** 04

**Status:** claimed

- [x] Test `report_stale_past_the_cancel_limit_cancels_each_plain_order_once` (seam A, S23): fails when there is no cancel by `Rule(MarginStale)` after 60 s, a cancel at or before 60 s, a second cancel on the next tick, or a cancel of a reduce-only order.
- [x] Test `stale_age_starts_at_link_up_when_no_report_has_arrived` (seam A, S23): fails when a node that never got a report never cancels.
- [ ] Test `stale_cancel_is_silent_while_the_link_is_down_or_the_policy_is_off` (seam A, S23): fails when cancels are requested in either case.
- [ ] Test `stale_cancel_does_not_reissue_an_exhausted_cancel` (seam A, S23): fails when an order with a spent cancel gets a new request on a tick.
- [ ] Test `a_fresh_report_after_a_stale_episode_restores_acceptance_without_a_latch` (seam A, S23): fails when plain orders stay rejected, or REDUCING appears.
- [ ] Test `a_forward_clock_jump_reads_the_report_as_stale_and_cancels` (seam A, G5): fails when after the record time jumps forward by an hour a plain order is accepted on the old report, or the tick requests no cancel.
- [ ] Mutants: the stale limit comparison, the link-up baseline, and the "no cancel request yet" filter are each caught by a named test.
- [ ] Stated and not proven here: the position stays; nothing is cancelled while the link is down; a cancel can lose the race to a fill.
- [ ] Every test above was written first and seen red for the reason named, then made green (tdd skill); one commit per red→green cycle.
- [ ] The events and claims this ticket adds are listed under C-MARGIN in the registry; the contract's verification section names the real tests; a renamed test is also renamed in this file and in the scenario table of `spec-ko.md`.
- [ ] The existing tests are green and unchanged in meaning; with no margin policy the behaviour is the same as before this ticket.
- [ ] Full local gate passes: workspace tests, clippy with warnings denied, generated documents in step, extended verification run without the live-testnet suite with evidence committed, secret scan.
