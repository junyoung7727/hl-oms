# 11: Simulation soak over several UTC days

**What to build:** A simulation profile runs the margin rules over virtual time that crosses at least three UTC day boundaries, with restarts in between and with losses, clears and rebases. Every invariant holds at every step, the day base rolls once per day after judging against the old base, REDUCING survives every restart and every day roll, and the number of margin records in the journal stays within one per poll. Scenario rows: S12, S13. Claim: V-MARGIN-12.

**Blocked by:** 10

**Status:** ready-for-agent

- [ ] Test `profile_margin_days_crosses_three_utc_days_with_restarts` (seam D, S12, S13, G8): fails when over the seeds of the standard run an invariant breaks, a day roll absorbs a loss, the base rolls more than once per day, or a restart or a day roll clears REDUCING.
- [ ] Test `margin_journal_records_stay_within_one_per_poll` (seam D, G8): fails when the run journals more margin records than polls made (17 280 per virtual day at 5 s).
- [ ] The profile is part of the standard verification run and of the extended run.
- [ ] Stated and not proven here: a wall-clock soak of the node process over real days; disk bytes (the journal is not pruned, U-04).
- [ ] Every test above was written first and seen red for the reason named, then made green (tdd skill); one commit per red→green cycle.
- [ ] The events and claims this ticket adds are listed under C-MARGIN in the registry; the contract's verification section names the real tests; a renamed test is also renamed in this file and in the scenario table of `spec-ko.md`.
- [ ] The existing tests are green and unchanged in meaning; with no margin policy the behaviour is the same as before this ticket.
- [ ] Full local gate passes: workspace tests, clippy with warnings denied, generated documents in step, extended verification run without the live-testnet suite with evidence committed, secret scan.
