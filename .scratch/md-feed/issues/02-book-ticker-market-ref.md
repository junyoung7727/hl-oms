# 02: In-process book, ticker state, market_ref and reads

**What to build:** Parse every routed frame with the adapter, apply snapshots and deltas to the (connection, topic) `OrderBook` through the `GapDetector`, merge tickers, pass trades and tickers to the optional `MessageSink`, `read_book`/`market_ref`/`ticker`, `ParseError` events, `Resnapshot` events. Spec: "Book state and reads", "Messages", "Resnapshot".

**Blocked by:** 01

**Status:** ready-for-agent

- [ ] Test `book_matches_fake_after_snapshot_and_deltas`: Bybit snapshot + deltas → `read_book` levels equal the hand-computed book.
- [ ] Test `market_ref_is_last_applied_message`: fields equal the last delta's `u`, `ts`·10^6 and its `recv_ns` from the sink, venue `bybit`, configured market, channel and symbol.
- [ ] Test `ticker_merges_and_trades_reach_message_sink`.
- [ ] Test `resnapshot_event`: a second snapshot on the same connection gives one `Resnapshot` and restarts the book.
- [ ] Test `parse_error_event`: a malformed book delta gives `ParseError`, the frame is still in the sink, and the book is treated as a gap.
