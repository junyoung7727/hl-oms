# 04: collector run, instruments, end to end with the seal

**What to build:** `Collector::start` / `md-collector run` (spec D5, D6): instrument fetch and refresh, feeds per group, the event task, `FeedHandle::stop` (md-feed, additive), shutdown; the fake Bybit (`tests/common/`).

**Blocked by:** 02, 03

**Status:** ready-for-agent

- [ ] Test `end_to_end_seal`: the collector runs against the fakes (2 markets, several symbols), stops; `md-lake seal --venue bybit --now-ns` (child) seals; a by_symbol day file exists for every topic the fake sent, its records decode to exactly the frames the fake sent (byte equality), no market-data frame is in any session file, the instrument bodies are in the instruments day files.
- [ ] Test `open_files_locked_while_running`: while the collector runs, `try_lock` on each open file fails, and `md-lake seal` with a later `--now-ns` leaves every open file byte-identical.
- [ ] Test `forced_disconnect_resubscribes`: the fake drops a connection; the collector's new connection subscribes again (fake log), its subscribe answer is in the session file, and both connections' frames are in L0 under their own `conn_id`.
- [ ] Test `instruments_failure_refuses_start`: a failing instrument answer at start makes `start` return an error; its body is in L0.
- [ ] Test `slow_disk_overflow_reconnects` (review R3): fake Bybit + gated `Fs`: `Disconnected` reason `overflow`, a new connection subscribes again, the `overflow` note is in the session file, every accepted frame is in L0.
- [ ] Test `traded_topic_not_planned_refuses_start` (review R4): with `symbols = "all"`, a traded symbol the venue does not list as trading refuses the start.
- [ ] Test `feed_stop_gate` (review R5): after a feed's gate closes and the feed is dropped, its connections' open files are closed and later frames of the feed are refused.
