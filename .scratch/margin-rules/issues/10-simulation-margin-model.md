# 10: Simulation: venue margin model and margin invariants

**What to build:** The simulation world answers margin, venue-setting and account-mode queries from its own venue model: account value follows fills, price, fees and funding. New profiles run the margin rules under random interleavings of fills, dropped and delayed reports, and restarts, and new invariants are checked at every step. Existing profiles give the same run when the margin policy is off. Scenario rows: S3, S5, S8, S11, S14, S21, S23. Claims: V-MARGIN-12, V-SIM-1.

**Blocked by:** 03, 05, 06, 07, 08

**Status:** ready-for-agent

- [ ] Test `world_account_value_follows_fills_price_fees_and_funding` (seam D, model unit, G10): fails when the model's account value differs from a hand-worked example with one fill, one price move, one fee and one funding charge. Does not prove the venue computes it this way.
- [ ] Test `a_loss_made_only_of_fees_and_funding_enters_reducing` (seam D, G10): fails when fees and funding that take the account more than 5 % below the base do not latch.
- [ ] Test `world_refuses_a_reduce_only_order_that_would_grow_the_position` (seam D, S3): fails when the model accepts it. This encodes the documented venue contract as an assumption (T5).
- [ ] Test `profile_margin_calm` (seam D, S5): fails when on some interleaving a plain order was accepted with account exposure above the limit at acceptance.
- [ ] Test `profile_margin_calm_reducing_invariant` (seam D, S8): fails when while REDUCING a plain order is accepted, or a plain order open at entry has no cancel request.
- [ ] Test `profile_margin_calm_release_invariant` (seam D, S14): fails when REDUCING ends by a restart, a new day or a recovery and not by an operator input.
- [ ] Test `profile_margin_lossy` (seam D, S11, S23): fails when an invariant breaks when reports are dropped or delayed or the node restarts.
- [ ] Test `existing_profiles_give_the_same_run_with_margin_off` (seam D, S21): for a fixed list of seeds of each existing profile, the journal hash of the run (the `trace_hash` of the run statistics, a hash of every journal record's bytes) is compared with a literal recorded by running the same seed on main at 84db0f9. Fails when any hash differs with no margin policy. It never compares two runs of the same build. The state digest is not compared, because the state gains fields in this work.
- [ ] Stated and not proven here: anything about the real venue; fills that beat a cancel; exposure after price moves.
- [ ] Every test above was written first and seen red for the reason named, then made green (tdd skill); one commit per red→green cycle.
- [ ] The events and claims this ticket adds are listed under C-MARGIN in the registry; the contract's verification section names the real tests; a renamed test is also renamed in this file and in the scenario table of `spec-ko.md`.
- [ ] The existing tests are green and unchanged in meaning; with no margin policy the behaviour is the same as before this ticket.
- [ ] Full local gate passes: workspace tests, clippy with warnings denied, generated documents in step, extended verification run without the live-testnet suite with evidence committed, secret scan.

## Comments

- Spec review r1 (reviewer claude-opus-5-5, head 37c6330), 2026-11-16. Finding 8 (nit, correct): `existing_profiles_give_the_same_run_with_margin_off` did not say what it compares with. Resolution: it compares the journal hash of fixed seeds with literals recorded on main at 84db0f9. The fix text asked for final digests; the state digest is a hash of the serialised state (`Core::digest`), which gains fields in this work, so the journal hash (`trace_hash`) is used instead.
