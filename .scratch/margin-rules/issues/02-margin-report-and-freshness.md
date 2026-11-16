# 02: Margin report reaches the core; stale data rejects plain orders

**What to build:** With a margin policy configured, the node asks the venue for a margin report every 5 seconds, the core records it, the status shows the account value and its age, and a plain order is rejected with `MarginUnavailable` when no fresh report exists. A reduce-only order needs no report. A late, foreign or unreadable report never becomes the current one. Without a margin policy nothing changes, and mainnet is refused without one. The simulated venue answers the query and has a control for the account value; the Hyperliquid gateway answers nothing yet (safe side: stale). Scenario rows: S11, S13, S21, S25. Claims: V-MARGIN-2, V-MARGIN-11, V-REPLAY-1, V-CFG-1.

**Blocked by:** None (can start immediately)

**Status:** ready-for-agent

- [ ] Test `margin_report_of_the_current_session_is_recorded` (seam A): fails when a report of the current link session produces no `margin_reported` event.
- [ ] Test `margin_report_of_an_old_session_or_with_the_link_down_is_ignored` (seam A): fails when such a report is stored or judged.
- [ ] Test `late_margin_report_with_a_lower_sequence_is_ignored` (seam A, G6): fails when a report whose sequence number is not greater than the last accepted one replaces it or is judged.
- [ ] Test `invalid_margin_report_is_journaled_and_never_becomes_fresh` (seam A, S25): fails when an invalid report refreshes the age or is judged, or a zero maximum leverage reaches a rule, or no `margin_report_rejected` event carries the reason.
- [ ] Test `plain_order_is_rejected_margin_unavailable_without_a_fresh_report` (seam A, S11): fails when a plain order is accepted with no report or with one older than 30 s; exactly 30 s is fresh.
- [ ] Test `margin_report_dated_after_the_record_time_is_not_fresh` (seam A, G5): fails when a negative age counts as fresh.
- [ ] Test `reduce_only_order_needs_no_fresh_report` (seam A, S11): fails when a reduce-only order is rejected for staleness.
- [ ] Test `without_a_margin_policy_no_margin_check_runs` (seam A, S21): fails when with no margin policy an order is rejected for margin, or a report changes state.
- [ ] Test `margin_events_replay_to_the_same_state` (seam A, S13): fails when the state rebuilt from the journal differs in digest.
- [ ] Test `mainnet_is_refused_without_a_margin_policy_and_bad_margin_values_are_refused` (seam node unit, S21): fails when mainnet loads with no margin policy, or a zero limit, or a stale-cancel limit not greater than the freshness limit, loads.
- [ ] Test `node_polls_margin_and_status_shows_value_and_age` (seam C, S11): fails when the 5 s timer, the simulated venue answer, the sequence number, the core input or the status fields are not wired.
- [ ] Mutants: the session guard, the sequence guard and the freshness comparison (both ends) are each caught by a named test.
- [ ] Stated in the contract and not proven here: rules are not judged while reports are missing; whether the venue's account value includes isolated equity (T2).
- [ ] Every test above was written first and seen red for the reason named, then made green (tdd skill); one commit per red→green cycle.
- [ ] The events and claims this ticket adds are listed under C-MARGIN in the registry; the contract's verification section names the real tests; a renamed test is also renamed in this file and in the scenario table of `spec-ko.md`.
- [ ] The existing tests are green and unchanged in meaning; with no margin policy the behaviour is the same as before this ticket.
- [ ] Full local gate passes: workspace tests, clippy with warnings denied, generated documents in step, extended verification run without the live-testnet suite with evidence committed, secret scan.
