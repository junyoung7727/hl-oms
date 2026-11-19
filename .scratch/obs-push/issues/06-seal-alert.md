# 06: md-lake seal alert for a whole-hour collector_down

**What to build:** Spec D9.

**Blocked by:** 01

**Status:** done

- [x] Test (md-lake) `report_lists_whole_hour_collector_down`: an hour with no open file is in `collector_down_hours`; a session-gap note is not.
- [x] Test (binary) `seal_sends_one_md_p1_for_down_hours`: two hours without open files, fake VM down (not used), fake Telegram up → exactly one message naming both hours; exit code 0.
- [x] Test `seal_alert_failure_keeps_exit_code`: fake Telegram dead → exit 0, stderr names the failure; `--telegram-api` with a non-loopback host → exit 2, nothing touched.
- Built: the whole-hour list is asserted in the existing md-lake test `hour_without_open_files_and_session_gap` (not a new test). `seal_alert_config_refused_before_the_lake` also refuses a non-loopback `--telegram-api` and `--telegram-api` without `--telegram`.
- Not tested: R12's error path (an alert sent before exit 1 when a later hour fails); the code sends from the report filled by `seal_into` before the result is matched.
- Mutation checks: 3 rules broken on purpose, all caught.
