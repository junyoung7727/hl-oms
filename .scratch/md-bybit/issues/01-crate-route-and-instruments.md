# 01: md-bybit crate, topic routing and instruments

**What to build:** Workspace crate `md-bybit` with `Market` (`Linear`, `Inverse`, `Spot`, each with its WebSocket URL constant), `BybitAdapter::new(market, instruments)`, `route`, `venue`, `venue_ts_field`, `instruments_request`, `instruments_request_page`, `parse_instruments` and `next_page_cursor`. The other trait methods may return placeholders until their tickets. Spec: "Solution" 1 and 5.

**Blocked by:** None

**Status:** done

- [x] Test `route_maps_each_topic_and_never_fails`: `orderbook.50.BTCUSDT`, `orderbook.1000.ETHUSDT`, `publicTrade.BTCUSDT`, `tickers.BTCUSDT` give their stream keys; empty bytes, non-UTF-8, a pong, a subscribe answer, `orderbook.7.BTCUSDT`, `kline.1.BTCUSDT` and JSON without a topic give `Route::Control`.
- [x] Test `instruments_give_exponents_for_linear_and_spot`: a linear body (`tickSize "0.10"`, `qtyStep "0.001"`) gives px_exp −1, qty_exp −3, contract type, settle coin and `venue_ts_ns = time × 10^6`; a spot body (`tickSize "0.01"`, `basePrecision "0.000001"`) gives −2 and −6; `retCode` ≠ 0 is an error.
- [x] Test `instruments_request_pages_linear_but_not_spot`: linear has `category=linear` and `limit=1000`; with a cursor it adds `cursor`; spot has only `category=spot`; `next_page_cursor` reads `nextPageCursor` and gives `None` for an empty one.
- [x] Runtime dependencies are `md-core` and `serde_json` only; no `oms-*` change.
