# 03: sink, bounded queues, writer thread, notes

**What to build:** The collector's `FrameSink`, per-connection in-flight accounting with `Overflow`, the writer thread (open files per `conn_id`, session file, ticks, close markers), `clock_step` and `overflow` notes (spec D4).

**Blocked by:** 01

**Status:** ready-for-agent

- [ ] Test `routes_market_data_and_control`: synthetic frames into the sink; market data lands in the connection's open file, control frames in the session file, byte-equal.
- [ ] Test `clock_step_note`: a frame whose `recv_ns` is lower than the previous of its connection gives exactly one `clock_step` note with `back_ns`, and both frames are kept.
- [ ] Test `overflow_refuses_and_notes`: with a blocked writer (slow `Fs`), the sink returns `Overflow` once `queue_bytes` is reached, one `overflow` note is queued, and after the gate opens every accepted frame is in L0.
- [ ] Test `close_marker_unlocks`: after the close marker the connection's open file is unlocked and ends with a zstd frame end.
