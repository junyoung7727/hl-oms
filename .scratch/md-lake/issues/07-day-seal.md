# 07: Day seal

**What to build:** `seal_days(world, venue)`: conditions, step 0 for crashed collector session files, rename, sidecar, manifest line with `sessions`, restart rule. Spec: "Day seal and verify".

**Blocked by:** 05

**Status:** done

- [x] Test `day_seal_waits_for_every_hour_and_every_lock`: a locked 14:00 file keeps the day `.partial` after hour 23; after release and the next run the day is sealed after hours 14–23.
- [x] Test `sidecar_and_manifest_line_validate`: sidecar format of 2.1; one manifest line per sealed file, valid against `xrepo/manifest_line.schema.json`, `sessions` with `first_rec_no` joining neighbouring runs of one session.
- [x] Test `step0_closes_a_crashed_session_file_once`: a crash between temp write and rename, then a rerun → exactly one `crash_truncate` note, the file ends with a frame end.
- [x] Test `day_seal_rerun_is_idempotent`: a torn last manifest line is cut; no duplicate line per `l0_file_id`; a sealed file is never modified (bytes and mtime unchanged).

## Comments

- Test-first (the report field `days_sealed` did not exist).
- Decision: the 24-hour condition counts the hours from `first_hour` on; earlier hours are out of the lake's scope and never sealed (F7).
- Decision (judgement, not a model): a normal run looks at the last 7 dates only (`LOOKBACK_DAYS`), so a run does not walk a year of directories.
- `first_recv_ns` / `last_recv_ns` are of the first and last record by position (golden manifest), not the minimum and maximum.
- The crash test of step 0 found a bug in the test file system (`SimFs::list` did not return subdirectories), not in the day seal.
- Mutations checked: no joining of neighbouring runs of one session, and no lock check of a collector's session file, each fail a test.
