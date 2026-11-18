# 05: Hour-seal restart rules and crash injection

**What to build:** The restart table of 2.5 (redo with `attempt` + 1 after cutting targets back, delete-only, stop on an unknown source) and the torn last seal-log line. Spec: "Injected world".

**Blocked by:** 04

**Status:** done

- [x] `SimFs` (F10): durable vs volatile state, torn writes at a crash.
- [x] Test `crash_at_every_step_ends_with_every_record_once` (by_symbol): on `SimFs`, a crash at call N for every N of a full seal, with random torn writes, (after begin, mid-append, after append, after done, mid-delete); a second run without faults ends with every record exactly once and no open file.
- [x] Test `done_with_an_unknown_source_stops`: an open file not named in `sources` → error, nothing changed.
- [x] Test `torn_last_seal_log_line_is_cut`.

## Comments

- The restart rules were written in ticket 04; their tests came here and passed at once. They were checked against five mutations: a redo that does not cut targets back, a reseal after a done entry, no fsync of the seal log, no fsync of a day file before the done entry, and no directory fsync after creating a day file. Each fails a test.
- `SimFs` keeps durable and volatile state; a reboot keeps a random prefix of each file's unsynced appended bytes. The scenario has about this many steps per seal: see `steps` in the test (asserted > 20).
