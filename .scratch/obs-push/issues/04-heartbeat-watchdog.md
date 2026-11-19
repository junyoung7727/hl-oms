# 04: writer heartbeat and the systemd watchdog

**What to build:** Spec D5.

**Blocked by:** none

**Status:** ready-for-agent

- [ ] Test `heartbeat_and_watchdog_on_quiet_venue`: no frames for 3 s (wall) → the heartbeat value advances every second and a temp-dir `NOTIFY_SOCKET` receives `WATCHDOG=1` about once a second; `READY=1` once after start.
- [ ] Test `stalled_writer_stops_beats`: the injected `Fs` blocks inside an append while the fake keeps sending → the heartbeat value stops advancing and no further `WATCHDOG=1` arrives; frames are still accepted until the queue cap (then the U5b overflow rule).
- [ ] Test `failed_write_stops_beats`: an injected write error → no beat after the error.
- [ ] Test `abstract_socket_address`: `@name` is sent to the abstract namespace.
