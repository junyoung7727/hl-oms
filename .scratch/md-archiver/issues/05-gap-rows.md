# 05: gap rows from connections and notes

**What to build:** Spec D5: `reconnect` from pass 1 (skipped when a `collector_down` covers the restart), note kinds with their covered topics, `crash_truncate` only for open files, gap rows only for evidence in D, sorted compact `detail`.

**Blocked by:** 03, 04

**Status:** ready-for-agent

- [ ] Test `golden_gap`: builds of 2026-01-02 and 2026-01-05 give exactly the golden `gap` files.
- [ ] Test `collector_down_one_row_per_topic`: a synthetic note gives one row per topic with records in D or D−1.
- [ ] Test `note_topics`: `topics` given, null with `conn_id`, null without; `clock_step`, `overflow`, `disk_floor`, `stale` rows; `disk_floor` and `stale` end the book state.
