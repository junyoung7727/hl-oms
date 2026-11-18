# 03: Stage 1 writer: open files and the collector's session file

**What to build:** `OpenWriter` (per connection) and `SessionWriter` (per collector) with the lock rule, level 3, flush and fsync driven by `tick(now_ns)`, hourly rotation and date change. Spec: "Stage 1".

**Blocked by:** 02

**Status:** ready-for-agent

- [ ] Test `open_file_is_created_locked_under_a_tmp_name`: before the rename the file is `.tmp`; after it the open-file name exists and a second handle cannot take the lock.
- [ ] Test `flush_every_second_and_fsync_every_five`: with a fake clock and a counting `Fs`, 12 s of records give 12 flushes and 2 fsyncs (settings changed: 2 s / 3 s give 6 and 4).
- [ ] Test `rotation_at_the_utc_hour`: records at 13:59:59.9 and 14:00:00.0 go to two files; the first is finished (frame end), fsynced and unlocked.
- [ ] Test `one_level3_context_per_connection`: 3 connections over 2 hours → never more than 3 live contexts.
- [ ] Test `session_file_by_date_and_backward_clock`: a record whose date's session file is already closed goes to the newest started date.
