# 03: Trades and tickers

**What to build:** `parse` for `publicTrade` and `tickers`, and the additive md-core `Ticker::merge`. Spec: "Trades", "Tickers".

**Blocked by:** 01

**Status:** ready-for-agent

- [ ] Test `trades_keep_order_side_and_flags`: a message with three trades gives three `Trade`s in order, sides Buy/Sell, `BT=true` → bit 0, `RPI=true` → bit 1, `venue_ts_ns = T × 10^6`, `trade_id = i`, `venue_seq = seq` per element; `S` other than Buy/Sell is an error.
- [ ] Test `ticker_delta_carries_only_its_fields_and_merges`: a linear snapshot with all fields, then a delta with `lastPrice` and `bid1Size` only → the delta's other fields are `None`; `merge` gives the snapshot with those two fields changed; `fundingRate "0.0001"` → 10000; `markPrice "42000.15"` with tick 0.1 → 4200015 at `ref_px_exp` −3 while `lastPrice "42000.15"` is a `ParseError`; `nextFundingTime "1767340800000"` → ns.
- [ ] Test `spot_ticker_is_a_snapshot_without_book_fields`.
- [ ] md-core change: `Ticker.ref_px_exp` and `Ticker::merge` with a test in md-core.
