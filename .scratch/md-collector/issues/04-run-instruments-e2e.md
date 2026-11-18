# 04: collector run, instruments, end to end with the seal

**What to build:** `Collector::start` / `md-collector run` (spec D5, D6): instrument fetch and refresh, feeds per group, the event task, `FeedHandle::stop` (md-feed, additive), shutdown; the fake Bybit (`tests/common/`).

**Blocked by:** 02, 03

**Status:** ready-for-agent

- [ ] Test `end_to_end_seal`: the collector runs against the fakes (2 markets, several symbols), stops; `md-lake seal --venue bybit --now-ns` (child) seals; a by_symbol day file exists for every topic the fake sent, its records decode to exactly the frames the fake sent (byte equality), no market-data frame is in any session file, the instrument bodies are in the instruments day files.
- [ ] Test `open_files_locked_while_running`: while the collector runs, `try_lock` on each open file fails, and `md-lake seal` with a later `--now-ns` leaves every open file byte-identical.
- [ ] Test `forced_disconnect_resubscribes`: the fake drops a connection; the collector's new connection subscribes again (fake log), its subscribe answer is in the session file, and both connections' frames are in L0 under their own `conn_id`.
- [ ] Test `instruments_failure_refuses_start`: a failing instrument answer at start makes `start` return an error; its body is in L0.
- [ ] Test (md-feed) `stop_waits_for_tasks`: after `stop().await` the sink receives no frame.
