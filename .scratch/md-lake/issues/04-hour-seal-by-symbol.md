# 04: Hour seal, layout by_symbol

**What to build:** `seal_venue(world, venue, classifier, settings)` for closed hours in order: lock check, recovery of unlocked files, pass 1 counts, unroutable guard, begin entry, grouped k-way merge with session runs, frames appended per topic, sealer session-file frame, check, done entry, delete. Spec: "by_symbol grouping", "Merge", "Begin and done entries", "Unroutable guard", "Strict order", "collector_down".

**Blocked by:** 03

**Status:** ready-for-agent

- [ ] Test `lossless_move_and_session_runs`: two collector sessions and a reconnect inside one hour → every record exactly once in the day files with the same bytes; runs in `session_ns` order; ties by `conn_id`, `frame_no`; a connection whose `recv_ns` went backward keeps its order; done entry `sessions` add up.
- [ ] Test `golden_seal_logs_validate_and_match`: begin and done entries validate against `l0/seal_log_entry.schema.json` (test-only validator).
- [ ] Test `locked_file_is_never_touched` (Seam C child holds the 14:00 file): 14 and later hours are not sealed and the file is unchanged; after release the next run seals 14 and the later hours in order.
- [ ] Test `unroutable_guard`: 2 % unroutable → error, open files and targets unchanged, no seal-log entry; 0.5 % → sealed, the records in the sealer's session file, `unroutable_records` counted.
- [ ] Test `grouping_respects_the_budget_and_slots`: with a budget of 4 contexts, the live context sizes never pass it, no group holds topics of two slots, every pledged size equals the topic's counted bytes.
- [ ] Test `hour_without_open_files_and_session_gap`: an empty hour gets a zero-record done entry and one `collector_down` note; two sessions 30 s apart give one note with both session numbers.
- [ ] Test `recovery_of_an_unlocked_crashed_file`: an open file cut at random offsets → no record after the last complete block, no earlier record lost, one `crash_truncate` note.
