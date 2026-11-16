# 08: Venue setting and account mode holds

**What to build:** With a margin policy configured, an instrument whose venue setting (isolated mode, venue leverage 2) is unknown in this link session or differs from the policy refuses plain orders on that instrument only. An account whose mode is unknown, is unified or portfolio, or is not on the allow-list refuses plain orders on the whole account. Reduce-only orders pass both holds. The node asks the venue for the settings at link-up and every 60 seconds for instruments without a position; the simulated venue answers and has controls for both; the status shows them. The OMS never changes a venue setting. Scenario rows: S17, S18. Claims: V-MARGIN-9, V-MARGIN-1.

**Blocked by:** 01, 02

**Status:** claimed

- [ ] Test `instrument_with_unknown_venue_setting_holds_plain_orders` (seam A, S17): fails when a plain order is accepted before the setting is known in this link session.
- [ ] Test `a_setting_that_differs_from_config_holds_only_that_instrument` (seam A, S17): fails when cross mode or leverage 3 passes, or another instrument is held too.
- [ ] Test `a_matching_setting_releases_the_hold_and_link_loss_makes_it_unknown_again` (seam A, S17): fails when the hold stays after a match, or the setting counts as known in a new session.
- [ ] Test `a_cross_position_in_a_margin_report_raises_the_hold` (seam A, S17): fails when the position's mode in a margin report is not used as a source.
- [ ] Test `venue_setting_hold_names_its_instrument_in_holds` (seam A, S17): fails when the hold query does not show the instrument.
- [ ] Test `unified_or_portfolio_account_mode_holds_the_account` (seam A, S18): fails when either mode lets a plain order through, also when it is on the allow-list.
- [ ] Test `an_account_mode_off_the_allow_list_or_unknown_holds_the_account` (seam A, S18): fails when an unlisted value, or no value in this link session, passes.
- [ ] Test `reduce_only_order_passes_the_venue_setting_and_account_mode_holds` (seam A, S17, S18): fails when it is rejected by either hold.
- [ ] Test `node_reads_venue_setting_and_account_mode_and_status_shows_them` (seam C, S17, S18): fails when the queries, the simulated venue answers, the inputs or the status fields are not wired.
- [ ] Mutants: the mode comparison, the leverage comparison, the session reset and the two never-allowed modes are each caught by a named test.
- [ ] Stated and not proven here: which mode string means standard (T10); whether the venue enforces the leverage on resting orders (T8).
- [ ] Every test above was written first and seen red for the reason named, then made green (tdd skill); one commit per red→green cycle.
- [ ] The events and claims this ticket adds are listed under C-MARGIN in the registry; the contract's verification section names the real tests; a renamed test is also renamed in this file and in the scenario table of `spec-ko.md`.
- [ ] The existing tests are green and unchanged in meaning; with no margin policy the behaviour is the same as before this ticket.
- [ ] Full local gate passes: workspace tests, clippy with warnings denied, generated documents in step, extended verification run without the live-testnet suite with evidence committed, secret scan.

## Comments

- Spec review r1 (reviewer claude-opus-5-5, head 37c6330), 2026-11-16. Finding 5 (nit, correct): ADR-018 §7 item 9 left out the case "account mode not received in this link session". Resolution: copied from the contract. This ticket's tests already cover the unknown mode.
- Spec review r2 (reviewer claude-opus-5-5, head 1a78dfb), 2026-11-16. Finding 3 (nit, correct): two tests of this ticket tagged S17 and S18 were missing from those rows of spec-ko.md. Resolution: `reduce_only_order_passes_the_venue_setting_and_account_mode_holds` is now in rows S17 and S18, and `node_reads_venue_setting_and_account_mode_and_status_shows_them` is now also in row S18. No change to this ticket's criteria.
