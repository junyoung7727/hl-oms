# 03: sink, bounded queues, writer thread, notes

**What to build:** The collector's `FrameSink`, per-connection in-flight accounting with `Overflow`, the writer thread (open files per `conn_id`, session file, ticks, close markers), `clock_step` and `overflow` notes (spec D4).

**Blocked by:** 01

**Status:** done

- [x] Test `routes_market_data_and_control`: synthetic frames into the sink; market data lands in the connection's open file, control frames in the session file, byte-equal.
- [x] Test `clock_step_note`: a frame whose `recv_ns` is lower than the previous of its connection gives exactly one `clock_step` note with `back_ns`, and both frames are kept.
- [x] Test `overflow_refuses_and_notes`: with a blocked writer (slow `Fs`), the sink returns `Overflow` once `queue_bytes` is reached, one `overflow` note is queued, and after the gate opens every accepted frame is in L0.
- [x] Test `close_marker_unlocks`: after the close marker the connection's open file is unlocked and ends with a zstd frame end.
- [x] (review R1) The reconnect of a slot inherits its backlog; a total cap holds over all slots (`total_cap_refuses`).
- [x] (review R2) md-lake `OpenWriter::set_min_hour`; a queued frame never opens a past hour (`a_queued_frame_never_opens_a_past_hour`, md-lake `a_new_writer_never_opens_an_hour_before_its_floor`).
- [x] (review R5) md-feed `FrameSink::conn_ended` after the connection's last frame (`conn_ended_follows_last_frame`).
