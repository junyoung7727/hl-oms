# 05: Stale flag and events

**What to build:** `is_stale` and `BookView.stale` by the spec rule, per-channel `stale_after`, the checker task with `StaleRaised`/`StaleCleared`. Spec: "Stale".

**Blocked by:** 02, 03

**Status:** ready-for-agent

- [ ] Test `stale_before_first_snapshot`.
- [ ] Test `stale_on_silence_and_cleared_by_next_message`: fake stops for > `stale_after`; `StaleRaised`; next delta → `StaleCleared`, `is_stale` false.
- [ ] Test `stale_during_resync`: during a resubscribe the book is stale even with fresh deltas, until the snapshot applies.
