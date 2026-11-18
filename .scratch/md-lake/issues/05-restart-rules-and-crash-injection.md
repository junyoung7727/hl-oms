# 05: Hour-seal restart rules and crash injection

**What to build:** The restart table of 2.5 (redo with `attempt` + 1 after cutting targets back, delete-only, stop on an unknown source) and the torn last seal-log line. Spec: "Injected world".

**Blocked by:** 04

**Status:** ready-for-agent

- [ ] `SimFs` (F10): durable vs volatile state, torn writes at a crash.
- [ ] Test `crash_at_every_step_ends_with_every_record_once` (by_symbol): on `SimFs`, a crash at call N for every N of a full seal, with random torn writes, (after begin, mid-append, after append, after done, mid-delete); a second run without faults ends with every record exactly once and no open file.
- [ ] Test `done_with_an_unknown_source_stops`: an open file not named in `sources` → error, nothing changed.
- [ ] Test `torn_last_seal_log_line_is_cut`.
