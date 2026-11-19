# 06: the run: date selection, atomic write, rebuild, CLI

**What to build:** Spec D1 (CLI, exit codes, lock, `.tmp` cleanup), D2 (dates to build, oldest first, carry-in rule), D7 (atomic write, stale-file removal, rebuild), D8 (disk floor), D10 (logs).

**Blocked by:** 05

**Status:** ready-for-agent

- [ ] Test `two_missed_days_one_run`: two sealed dates without L1 are built by one run, oldest first.
- [ ] Test `second_run_and_rebuild_are_identical`: byte-identical files; SHA-256 of every L0 file unchanged.
- [ ] Test `crash_mid_write_rebuilds`: a failure injected after some renames (before the gap file) leaves no gap file; the next run builds the date and removes stale files.
- [ ] Test `waits_for_the_date_before`: D−1 with L0 but not day-sealed → D is not built.
- [ ] Test `cli_refusals`: unknown venue, bad range, future `--now-ns`, `by_connection` → exit 2, nothing written; disk below floor and a held lock → exit 1, nothing written.
