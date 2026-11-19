# 03: md-feed book hook and the collector telemetry model

**What to build:** Spec D3 (additive `MessageSink::book`, registry `start_feed` passes the message sink) and D4 (per-topic stats, lag histogram, summary computation, 1 s / 10 s split) as a pure model with an injected clock and a book reader.

**Blocked by:** none

**Status:** done

- [x] Test (md-feed) `book_updates_reach_the_message_sink`: snapshot and delta of the active connection reach `book`; a standby connection's do not.
- [x] Test `summary_from_synthetic_book`: known levels → exact `md_spread_bp`, `md_mid_px`, `md_depth_qty` for 10 and 50 levels both sides with the instrument exponents; a stale or crossed book gives no spread/mid.
- [x] Test `lag_quantiles`: 100 samples 1..100 ms → p50 and p99 within the bucket error; no sample → no series; a negative lag counted.
- [x] Test `fast_slow_split`: two traded topics and three others; over 10 s of injected time the traded ones are emitted 10 times, the others once, never both.
- [x] Test `msgs_bytes_per_conn_channel` and `gaps_reconnects_totals` from synthetic frames and events.
- [x] Test `book_info_from_a_real_order_book`: spread, mid and 10/50-level depth from an md-core `OrderBook`.
- Note: the model was written before most of its tests; 7 rules were broken on purpose and each was caught (disclosed in the PR).
