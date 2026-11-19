# 03: md-feed book hook and the collector telemetry model

**What to build:** Spec D3 (additive `MessageSink::book`, registry `start_feed` passes the message sink) and D4 (per-topic stats, lag histogram, summary computation, 1 s / 10 s split) as a pure model with an injected clock and a book reader.

**Blocked by:** none

**Status:** ready-for-agent

- [ ] Test (md-feed) `book_updates_reach_the_message_sink`: snapshot and delta of the active connection reach `book`; a standby connection's do not.
- [ ] Test `summary_from_synthetic_book`: known levels → exact `md_spread_bp`, `md_mid_px`, `md_depth_qty` for 10 and 50 levels both sides with the instrument exponents; a stale or crossed book gives no spread/mid.
- [ ] Test `lag_quantiles`: 100 samples 1..100 ms → p50 and p99 within the bucket error; no sample → no series; a negative lag counted.
- [ ] Test `fast_slow_split`: two traded topics and three others; over 10 s of injected time the traded ones are emitted 10 times, the others once, never both.
- [ ] Test `msgs_bytes_per_conn_channel` and `gaps_reconnects_totals` from synthetic frames and events.
