# 09: Hyperliquid gateway: margin report, venue setting and account mode

**What to build:** Against Hyperliquid the node sends the documented queries for the margin report, the per-instrument venue setting and the account mode, and turns the answers into the core's inputs. An answer with a missing, zero or unreadable number is rejected as a whole with a reason; it never becomes a partial report. A position on a coin that is not configured is kept. The shipped example configuration has a margin policy and loads. The steady-state query weight is computed from the configuration and refused above 600 per minute. Nothing here contacts the venue: the fixtures are the documented examples. Scenario rows: S17, S18, S21, S22, S25. Claims: V-MARGIN-10, V-CFG-1.

**Blocked by:** 02, 08

**Status:** claimed

- [x] Test `documented_clearinghouse_state_parses_to_a_margin_report` (seam B, S25): fails when any field of the documented example maps to a wrong value. Does not prove the live answer equals the documented one.
- [x] Test `missing_zero_or_unparseable_max_leverage_rejects_the_whole_report` (seam B, S25): fails when a partial report is returned.
- [x] Test `any_unparseable_number_rejects_the_whole_report` (seam B, S25): fails when a bad account value, size, position value or margin is defaulted to zero.
- [ ] Test `a_position_on_an_unconfigured_coin_is_kept` (seam B, S22): fails when it is dropped, so the MM-buffer rule cannot see a manual position.
- [ ] Test `documented_active_asset_data_parses_leverage_and_mode` (seam B, S17): fails when the mapping is wrong. Does not prove it answers with no position (T11).
- [ ] Test `user_abstraction_values_parse_and_an_unknown_value_stays_unknown` (seam B, S18): fails when an unknown string maps to an allowed mode.
- [ ] Test `margin_and_setting_request_json` (seam B): fails when a request body differs from the documented one.
- [ ] Test `example_config_with_a_margin_policy_loads` (seam node unit, S21): fails when the shipped example configuration fails validation.
- [ ] Test `steady_state_query_weight_is_computed_from_config_and_a_budget_above_600_is_refused` (seam node unit, G7): fails when the weight for the shipped configuration is not the hand-worked literal (216 + 20 per instrument), or a configuration above 600 loads.
- [ ] Test `a_full_cancel_burst_leaves_the_margin_query_waiting_for_a_known_time` (seam node unit, G7): fails when the local request budget after 500 cancels, or the wait until the next margin query may go, differs from the hand-worked literals. The numbers go into the contract.
- [ ] Mutants: the whole-report rejection and the mode mapping are each caught by a named test.
- [ ] Stated and not proven here: everything about the live venue (T1, T2, T10, T11); behaviour on HTTP 429.
- [ ] Every test above was written first and seen red for the reason named, then made green (tdd skill); one commit per red→green cycle.
- [ ] The events and claims this ticket adds are listed under C-MARGIN in the registry; the contract's verification section names the real tests; a renamed test is also renamed in this file and in the scenario table of `spec-ko.md`.
- [ ] The existing tests are green and unchanged in meaning; with no margin policy the behaviour is the same as before this ticket.
- [ ] Full local gate passes: workspace tests, clippy with warnings denied, generated documents in step, extended verification run without the live-testnet suite with evidence committed, secret scan.
