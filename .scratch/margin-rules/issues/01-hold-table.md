# 01: Hold table: reduce-only orders pass the partial holds

**What to build:** With a margin policy configured, a strategy can send a reduce-only order while the account is in resync, has an in-doubt order, or has an unacknowledged mismatch, and the OMS accepts it. Plain orders stay refused under every hold, and a reduce-only order stays refused under the holds that refuse everything. Without a margin policy nothing changes: every hold refuses every new order, reduce-only included, so a journal recorded without a policy re-decides to the same result. This is the prefactoring that makes a state "only reduce" possible, so it lands alone. It uses the policy section that ticket 02 adds. ADR-008 §7 is replaced by the amendment text in ADR-018, including its sentence on the margin policy. Scenario rows: S3, S10, S21, S22. Claim: V-MARGIN-1. Every test below except the no-policy one runs with a margin policy set.

**Blocked by:** 02

**Status:** ready-for-agent

- [ ] Test `reduce_only_order_is_accepted_during_resync` (seam A, S3): fails when the resync hold still rejects a reduce-only order.
- [ ] Test `reduce_only_order_is_accepted_while_another_order_is_in_doubt` (seam A, S3): fails when the in-doubt hold still rejects it.
- [ ] Test `reduce_only_order_is_accepted_under_an_unacknowledged_mismatch` (seam A, S10): fails when a mismatch (for example a stuck cancel) still rejects it.
- [ ] Test `reduce_only_order_is_rejected_by_every_block_all_hold` (seam A): fails when any of no-config, link-down, operator account halt, operator strategy halt or storage-low lets a reduce-only order through.
- [ ] Test `reduce_only_order_is_still_bound_by_the_strategy_position_limit` (seam A, S22): fails when the reduce-only flag skips the position interval check. Does not prove that an unattributed position can be reduced through the OMS; it cannot.
- [ ] Test `without_a_margin_policy_every_hold_still_refuses_a_reduce_only_order` (seam A, S21): fails when, with no margin policy, a reduce-only order is accepted under resync, an in-doubt order or an unacknowledged mismatch. It pins that re-deciding a journal recorded before this work gives the recorded result.
- [ ] The existing tests `holds_are_independent` and `cancels_are_accepted_while_new_orders_are_held` stay green: a plain order is refused under every hold.
- [ ] Mutants: the existing mutant on the hold push is rewritten for the table; new mutants "a partial hold also refuses reduce-only", "a block-all hold lets reduce-only pass" and "the table applies without a margin policy" are each caught by a named test.
- [ ] ADR-008 §7 carries the amended text and table from ADR-018; the hold rows `Reducing`, `VenueSetting` and `AccountMode` are named there as arriving with later tickets.
- [ ] Every test above was written first and seen red for the reason named, then made green (tdd skill); one commit per red→green cycle.
- [ ] The events and claims this ticket adds are listed under C-MARGIN in the registry; the contract's verification section names the real tests; a renamed test is also renamed in this file and in the scenario table of `spec-ko.md`.
- [ ] The existing tests are green and unchanged in meaning; with no margin policy the behaviour is the same as before this ticket.
- [ ] Full local gate passes: workspace tests, clippy with warnings denied, generated documents in step, extended verification run without the live-testnet suite with evidence committed, secret scan.

## Comments

- Spec review r1 (reviewer claude-opus-5-5, head 37c6330), 2026-11-16. Finding 1 (blocking, correct): the hold table was unconditional and contradicted "no margin policy = behaviour unchanged". Resolution: option (a). The table applies only when a margin policy is set. This ticket now waits for ticket 02 (policy section), gains the test `without_a_margin_policy_every_hold_still_refuses_a_reduce_only_order` and one mutant, and names S21. The same rule is written in ADR-018 §4, §7 item 8 and the ADR-008 amendment text, the contract (설정, 차단 표), spec.md (Solution 7, Hold table, Further Notes) and spec-ko.md (§2, 차단 표, §7, S21, §14). Re-deciding a journal recorded without a policy is unchanged.
