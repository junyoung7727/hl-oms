# 02: Margin report reaches the core; stale data rejects plain orders

**What to build:** With a margin policy configured, the node asks the venue for a margin report every 5 seconds, the core records it, the status shows the account value and its age, and a plain order is rejected with `MarginUnavailable` when no fresh report exists. A reduce-only order needs no report. A late, foreign or unreadable report never becomes the current one. Without a margin policy nothing changes, and mainnet is refused without one. The policy section itself comes from ticket 01; this ticket adds the node's checks of it and is the first to read its values. The simulated venue answers the query and has a control for the account value; the Hyperliquid gateway answers nothing yet (safe side: stale). Scenario rows: S11, S13, S21, S25. Claims: V-MARGIN-2, V-MARGIN-11, V-REPLAY-1, V-CFG-1.

**Blocked by:** 01

**Status:** claimed

- [x] Test `margin_report_of_the_current_session_is_recorded` (seam A): fails when a report of the current link session produces no `margin_reported` event.
- [x] Test `margin_report_of_an_old_session_or_with_the_link_down_is_ignored` (seam A): fails when such a report is stored or judged.
- [x] Test `late_margin_report_with_a_lower_sequence_is_ignored` (seam A, G6): fails when a report whose sequence number is not greater than the last accepted one replaces it or is judged.
- [x] Test `invalid_margin_report_is_journaled_and_never_becomes_fresh` (seam A, S25): fails when an invalid report refreshes the age or is judged, or a zero maximum leverage reaches a rule, or no `margin_report_rejected` event carries the reason.
- [x] Test `plain_order_is_rejected_margin_unavailable_without_a_fresh_report` (seam A, S11): fails when a plain order is accepted with no report or with one older than 30 s; exactly 30 s is fresh.
- [x] Test `margin_report_dated_after_the_record_time_is_not_fresh` (seam A, G5): fails when a negative age counts as fresh.
- [x] Test `reduce_only_order_needs_no_fresh_report` (seam A, S11): fails when a reduce-only order is rejected for staleness.
- [x] Test `without_a_margin_policy_no_margin_check_runs` (seam A, S21): fails when with no margin policy an order is rejected for margin, or a report changes state.
- [x] Test `margin_events_replay_to_the_same_state` (seam A, S13): fails when the state rebuilt from the journal differs in digest.
- [x] Test `mainnet_is_refused_without_a_margin_policy_and_bad_margin_values_are_refused` (seam node unit, S21): fails when mainnet loads with no margin policy, or a zero limit, or a stale-cancel limit not greater than the freshness limit, loads.
- [x] Test `node_polls_margin_and_status_shows_value_and_age` (seam C, S11): fails when the 5 s timer, the simulated venue answer, the sequence number, the core input or the status fields are not wired.
- [ ] An absent margin policy still adds no bytes to a journal record after this ticket (criterion of ticket 01; ticket 10 checks it against literals recorded on main).
- [ ] Mutants: the session guard, the sequence guard and the freshness comparison (both ends) are each caught by a named test.
- [ ] Stated in the contract and not proven here: rules are not judged while no new report is accepted; whether the venue's account value includes isolated equity (T2).
- [ ] Every test above was written first and seen red for the reason named, then made green (tdd skill); one commit per red→green cycle.
- [ ] The events and claims this ticket adds are listed under C-MARGIN in the registry; the contract's verification section names the real tests; a renamed test is also renamed in this file and in the scenario table of `spec-ko.md`.
- [ ] The existing tests are green and unchanged in meaning; with no margin policy the behaviour is the same as before this ticket.
- [ ] Full local gate passes: workspace tests, clippy with warnings denied, generated documents in step, extended verification run without the live-testnet suite with evidence committed, secret scan.

## Comments

- Spec review r1 (reviewer claude-opus-5-5, head 37c6330), 2026-11-16. Finding 1: ticket 01 now depends on this ticket for the policy section. No change to the scope here.
- Spec review r1 (reviewer claude-opus-5-5, head 37c6330), 2026-11-16. Finding 3 (blocking, correct): spec-ko.md defined a fresh report as under 30 s. Resolution: the definition now says 30 s or less, exactly 30 s included, as in the contract and this ticket's test.
- Spec review r1 (reviewer claude-opus-5-5, head 37c6330), 2026-11-16. Finding 8: a criterion was added here that an absent margin policy adds no bytes to a journal record, so the literal comparison of ticket 10 can hold.
- Spec review r1 (reviewer claude-opus-5-5, head 37c6330), 2026-11-16. Finding 9 (nit, correct): V-MARGIN-11 was defined as operator commands and restart only, but this ticket claims it for the poll-wiring test at seam C. Resolution: spec.md now defines V-MARGIN-11 as the node process level (poll wiring and status, operator commands, restart). `Status: done` is documented as a local closing state, not a triage role, in docs/agents/triage-labels.md and issue-tracker.md.
- Spec review r2 (reviewer claude-opus-5-5, head 1a78dfb), 2026-11-16. Finding 1 (blocking, correct): the ticket order. Resolution: ticket 01 now adds the optional policy section, so this ticket is blocked by 01 and no longer the first. The criterion that an absent policy adds no journal bytes moved to ticket 01; this ticket keeps it as a guard. This replaces the r1 note above that ticket 01 depends on this ticket.
