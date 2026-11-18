# Spec: md-bybit, the Bybit public market-data adapter (unit U3)

Status: ready-for-agent

Design source: `RESEARCH/CRYPTO_TRADING_INTEGRATED_TN_20261118/10_DESIGN_DRAFT.md` §6.2–6.4, §6.6 and the venue facts `04_VENUE_MARKET_DATA_FACTS.md` §1.1 (crypto integrated design TN v1.0, outside this repository). Lake contract: `contracts/lake/` (vendored, U2) and its document `docs/crypto/lake-contract.md` in quant-data-platform (§1.2, §1.3, §3.2–3.6). Tickets: `issues/` next to this file.

## Problem Statement

The collector (U5b), the archiver (U7) and the strategy must read Bybit public WebSocket messages the same way. `md-core` (U2) gives the trait, the types, the order book and the gap detector, but nothing yet turns a Bybit frame into those types, splits thousands of topics over connections within Bybit's limits, or derives each instrument's exponents. If each later unit did this itself, the L1 rows the archiver writes could differ from the book the strategy trades on.

## Solution

A new workspace crate `md-bybit` with no I/O that implements `MarketDataAdapter` for Bybit v5 public streams of the markets `linear`, `inverse` and `spot`:

1. **Topics**: `orderbook.{1,50,200,1000}.{symbol}`, `publicTrade.{symbol}`, `tickers.{symbol}`. `route` reads only the `topic` field and maps it to `StreamKey { channel, symbol }` (for example `("orderbook.50", "BTCUSDT")`); anything else is `Route::Control`.
2. **Parsing** into md-core types with the instrument's exponents: book snapshot and delta (`u` → `update_id`, `seq` → `venue_seq`, `ts` → `venue_ts_ns`), trades (one `Trade` per element, `T` → `venue_ts_ns`, `i` → `trade_id`, `S` → side, `seq` → `venue_seq`, `BT`/`RPI` → flags bits 0/1), tickers (snapshot and delta; a delta carries only the fields it has).
3. **Sequence rule** `PlusOne` for order book topics, `None` for trades and tickers. On a gap: `Resubscribe`.
4. **Connection plan** within Bybit's documented limits, with subscribe frames and a 20 s `{"op":"ping"}` keepalive.
5. **Instruments**: the `instruments-info` request and its parsing into `Instrument` rows with `px_exp`/`qty_exp` by contract rule 1.3.
6. **Tests**: the md-core conformance harness with a synthetic Bybit fixture, and a golden test that parses every Bybit record of the vendored golden L0 files and compares the result with the golden L1 rows.

## User Stories

1. As the developer of the collector, I want each market's plan to fit a connection budget I give it, with connection numbers that do not collide, so that linear, spot and inverse together stay within Bybit's 500 connections per 5 minutes on `stream.bybit.com` and the collector cannot be refused or banned at start.
2. As the developer of the archiver, I want the parser to give exactly the golden L1 rows for the golden L0 frames, so that the archiver's rows match the lake contract.
3. As the strategy developer, I want a missing `u` to give one gap and a `Resubscribe`, and a new snapshot (also `u = 1`, also a `u` below the last applied delta) to start a new run, so that I never trade on a broken book.
4. As the owner, I want prices and quantities converted exactly or refused, so that no row holds a price Bybit never quoted.

## Implementation Decisions

- **Crate boundary.** `md-bybit` depends on `md-core` and `serde_json` (both in the workspace). No network, file, clock or randomness access. No `oms-*` crate changes.
- **Construction.** `BybitAdapter::new(market, instruments)` with `market` one of `Linear`, `Inverse`, `Spot` and the instrument rows from `parse_instruments`. `parse` of a symbol without an instrument row is a `ParseError` (the exponents are unknown). The WebSocket URL per market is a public constant for the feed; the adapter never connects.
- **Route.** Only the `topic` value is read. The channel is the known prefix (`orderbook.1`, `orderbook.50`, `orderbook.200`, `orderbook.1000`, `publicTrade`, `tickers`); the symbol is the rest after the dot. Any other topic, a frame without a topic, non-JSON and non-UTF-8 give `Route::Control`.
- **Book parse.** `type` `snapshot` → `BookSnapshot`, `delta` → `BookDelta`; levels from `data.b` / `data.a` in the order received; `data.s` must equal the key's symbol and `topic` the key's topic, else `ParseError`. `venue_ts_ns = ts × 10^6`. `cts` stays only in L0 (contract 1.2). A price or quantity that is not exact at the instrument's exponent, or does not fit `i64`, is a `ParseError` (contract 1.3: no row, a `parse_error` gap).
- **Snapshots rebase.** Every snapshot starts a new run in the gap detector, whatever its `u` (`u = 1` after a Bybit service restart, or a `u` below the last applied delta after a same-connection resubscribe, measured 4 of 4). Writing the `resnapshot` gap row is the archiver's job (contract 3.5, U7); counting it for metrics is the feed's (U5a). The adapter only parses.
- **Trades.** One message may hold up to 1,024 trades; each becomes one `Trade` in message order (`msg_idx` is its position). `S` `Buy` → `Side::Buy`, `Sell` → `Side::Sell`, anything else is a `ParseError`. `L` (tick direction) is not in L1.
- **Tickers.** Linear and inverse: snapshot and delta; spot: snapshot only (documented). Fields: `lastPrice` (px_exp), `markPrice`, `indexPrice` (`ref_px_exp = px_exp - 2`, contract 1.3 and 3.4 as amended by qdp PR #29: these computed prices were measured up to one decimal finer than the tick), `openInterest` (qty_exp), `fundingRate` (units of 1e-8), `nextFundingTime` (ms string → ns), `bid1Price`/`bid1Size`/`ask1Price`/`ask1Size`. A field the message does not carry is `None`. md-core gains `Ticker.ref_px_exp` and `Ticker::merge(&mut self, delta: &Ticker)` that overwrites only the fields the delta carries. Spot `usdIndexPrice` is not stored (not an L1 column).
- **Plan.** Topics are grouped symbol by symbol (all channels of one symbol on one connection, so one reconnect touches one symbol's streams together). A connection takes topics while (a) the sum of the topic strings' lengths plus 3 characters each (quotes and comma) stays ≤ 21,000 (Bybit's args limit; read as per connection, the stricter reading) and (b) the number of topics stays ≤ `max_topics_per_conn` (default 200 — judgement, not a model: 24 connections × ≈212 topics ran 5 minutes with no reconnect, `04_VENUE_MARKET_DATA_FACTS.md`). The limit of 500 new connections per 5 minutes counts every connection to `stream.bybit.com`, all markets together (`RESEARCH/BYBIT_WS_DOCS_TERMS_NOTES_20261117.md` §2, outside this repository). So the adapter takes a budget: `with_conn_budget(first_conn_index, max_conns)`; a plan that needs more than `max_conns` is a `PlanError`, and `conn_index` runs from `first_conn_index`. The default is `(0, 100)`. The collector (U5b) gives each market a disjoint range and keeps the sum of all markets' budgets ≤ 250, so that a full reconnect of everything plus the start still stays under 500 in 5 minutes (judgement, not a model). An unknown channel, a depth Bybit does not offer, or `orderbook.*` with depth ≠ the channel's depth is a `PlanError`.
- **Subscribe frames.** `{"op":"subscribe","req_id":"c{conn_index}-{n}","args":[...]}` with at most 10 args per frame (the spot per-request limit, used for every market so that one code path is tested).
- **Instruments.** `GET /v5/market/instruments-info` with `category` and, for linear and inverse, `limit=1000`. Spot has no paging (documented: "Spot does not support pagination"). md-bybit adds `next_page_cursor(body) -> Option<String>` and `instruments_request_page(cursor)`; the feed pages. `px_exp` from `priceFilter.tickSize`; `qty_exp` from `lotSizeFilter.qtyStep` (linear, inverse) or `lotSizeFilter.basePrecision` (spot); `venue_ts_ns = time × 10^6`; `contract_type`, `settle_coin` when present. `retCode ≠ 0` is a `ParseError`.
- **Vendored contract.** U3 re-vendors `contracts/lake/` from the qdp main commit of PR #29 and updates `VENDOR.toml` (and the secret-scan allow-list entry of the source commit).
- **venue_ts_field**: `ts` for order book and tickers, `T` for trades (contract 1.2).

## Testing Decisions

- **Seam A**: the public API of `md-bybit` (`BybitAdapter` through `MarketDataAdapter`, plus `next_page_cursor`), called from integration tests in `crates/md-bybit/tests/`. **Seam B**: the vendored golden files under `contracts/lake/golden/`, read by a test with a small test-only QRAW1 reader (the golden L0 files are uncompressed QRAW1 segments, contract 2.2).
- **Tests prove** (done criteria): the conformance harness passes with a synthetic Bybit fixture; snapshot then deltas give the hand-computed book; a missing `u` gives exactly one gap event and `Resubscribe`; a snapshot with `u = 1` and a snapshot with a `u` below the last delta both start a new run (the next `+1` delta gives no gap); trade parsing with side and flags; ticker snapshot + delta merge; instrument exponents for linear and spot; plans for the measured market sizes (linear 932, spot 509, inverse 26 symbols × 3 channels) with disjoint budgets that never exceed the limits and whose `conn_index` ranges do not overlap, and refusal when a plan would exceed its budget; `route` never fails on unknown frames; the golden test (every expected `book_msg` snapshot/delta row, `trade` row, `ticker` row and `instrument` row equals the parse of its record, compared on the md-core fields; lineage columns are not compared; and each record gives exactly as many messages as there are golden rows naming it, except trade records whose rows were dropped as duplicates at a handover, which must still give their full trade count).
- **Fixtures are synthetic**, built from the documented message shapes. No captured venue data (the repository is public; Bybit API terms 6.7).
- **Prior art.** `crates/md-core/tests/`.

## Out of Scope

- `orderbook.full` and the REST `full_orderbook` snapshot: the REST response shape is not in our checked facts. Add it when the full depth stage (§6.6) starts.
- `orderbook.rpi`, options, `allLiquidation`.
- The network feed and resync driver (U5a), L0 writing (U4), the archiver and the `resnapshot` row (U7).
- Any change to `oms-*` crates, `.github/`, the journal or registry contracts.

## Further Notes

- Built directly by Claude Opus 5.5 (2026-11-18, owner instruction "이제 개발을 너가 직접해"); lean spec (owner: "검증 사소한 것까지 하지마").
- `orderbook.1` resends a snapshot with the same `u` after 3 s without change (documented). By contract 3.5 each would be a `resnapshot` row. We do not subscribe depth 1 now (grill decision: 50 for all, 1000 for traded symbols); revisit with U7 if depth 1 is ever used.
