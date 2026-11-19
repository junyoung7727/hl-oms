# 04: writer heartbeat and the systemd watchdog

**What to build:** Spec D5.

**Blocked by:** none

**Status:** done

- [x] Test `heartbeat_and_watchdog_on_quiet_venue`: no frames for 3 s (wall) → the heartbeat value advances every second and a temp-dir `NOTIFY_SOCKET` receives `WATCHDOG=1` about once a second; `READY=1` once after start.
- [x] Test `stalled_writer_stops_beats`: the injected `Fs` blocks inside an append while the fake keeps sending → the heartbeat value stops advancing and no further `WATCHDOG=1` arrives; frames are still accepted until the queue cap (then the U5b overflow rule).
- [x] Test `failed_write_stops_beats`: an injected write error → no beat after the error.
- [x] Test `abstract_socket_address`: `@name` is sent to the abstract namespace.
- [x] (R5) `quiet_venue_keeps_heartbeat_60s`: the collector against a quiet fake venue for 60 s; `READY=1` once, `WATCHDOG=1` and the value keep advancing.
- [x] (R10) `full_socket_never_blocks`.
- Note: written body-first; 5 rules broken on purpose: 3 caught as test failures, 2 (beating after a write error, a blocking socket) as tests that hang instead of passing.
