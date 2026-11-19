# 06: md-lake seal alert for a whole-hour collector_down

**What to build:** Spec D9.

**Blocked by:** 01

**Status:** ready-for-agent

- [ ] Test (md-lake) `report_lists_whole_hour_collector_down`: an hour with no open file is in `collector_down_hours`; a session-gap note is not.
- [ ] Test (binary) `seal_sends_one_md_p1_for_down_hours`: two hours without open files, fake VM down (not used), fake Telegram up → exactly one message naming both hours; exit code 0.
- [ ] Test `seal_alert_failure_keeps_exit_code`: fake Telegram dead → exit 0, stderr names the failure; `--telegram-api` with a non-loopback host → exit 2, nothing touched.
