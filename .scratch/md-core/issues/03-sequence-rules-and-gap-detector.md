# 03: Sequence rules and gap detector

**What to build:** `SeqRule { PlusOne, PrevEquals, None }` and `GapDetector`, fed with snapshots and deltas of one stream, which emits a `GapEvent { key, last_good, next, recv_ns }` when a delta breaks the rule; a snapshot starts a new run; after a gap, no further gap is reported until the next snapshot. Spec: "Solution" 3.

**Blocked by:** 01

**Status:** ready-for-agent

- [ ] Test `plus_one_reports_a_missing_update_id_once` (seam A): 100 snapshot, 101, 103 → one gap (101 → 103); 104 → no second gap; snapshot 1 (venue restart) then 2 → no gap.
- [ ] Test `prev_equals_reports_a_wrong_prev` (seam A): Binance-style `pu`.
- [ ] Test `none_rule_never_reports_a_gap` (seam A).
- [ ] Test `delta_before_any_snapshot_is_a_gap` (seam A).
- [ ] Full local gate passes.
