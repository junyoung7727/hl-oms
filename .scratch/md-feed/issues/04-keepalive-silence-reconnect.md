# 04: Keepalive, silence timeout, reconnect and backoff

**What to build:** Keepalive frame at the adapter's interval, silence timeout, reconnect with backoff (reset after an applied snapshot, `conn_index` spread), `Resync::Reconnect`. Spec: "Silence and keepalive".

**Blocked by:** 01

**Status:** done

- [x] Test `keepalive_frames_sent` (short interval in config override for tests).
- [x] Test `silence_reconnects`: fake goes silent; after `silence_timeout` a new connection with a new `conn_id` and `frame_no` from 0.
- [x] Test `server_close_reconnects_with_backoff`: fake closes at once twice; the third attempt comes after ≥ min + 2×min.
