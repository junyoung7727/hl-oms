# Spec: md-feed, the venue connection and resync driver (unit U5a)

Status: ready-for-agent

Design source: `RESEARCH/CRYPTO_TRADING_INTEGRATED_TN_20261118/10_DESIGN_DRAFT.md` §6.2–6.4, §6.7, R2 "Strategy feed (SHOULD 9)" and `pr_plan.json` unit U5a (crypto integrated design TN v1.0, outside this repository). Lake contract: `contracts/lake/` (vendored) and its document `docs/crypto/lake-contract.md` in quant-data-platform (§2.2 record fields, §3.5 gap kinds). Tickets: `issues/` next to this file.

## Problem Statement

`md-core` (U2) and `md-bybit` (U3) turn venue frames into messages, but nothing connects to a venue. The collector (U5b) and the trading strategy both need the same connection, subscription, keepalive, reconnect and resync logic, not only the same parser. If each wrote its own, the book the strategy trades on could differ from the book the lake replays, and a gap could be handled one way in research and another way in trading.

## Solution

A new workspace library crate `md-feed` (tokio) that runs the connections of one venue market for an adapter:

1. **Driver.** For each `ConnPlan` of the adapter's plan: connect, send the subscribe frames, send the keepalive frame at its interval, reconnect with exponential backoff when the connection ends or is silent too long. Every received data frame is stamped with `recv_ns` (CLOCK_REALTIME, ns) at once and handed, with its `conn_id` and `frame_no`, to an optional **frame sink** (the collector's L0 writer is one; the strategy passes none). REST responses the feed makes go to the same sink as `src = rest`.
2. **Book.** Per (connection, topic) an `md-core` `OrderBook`, fed by the adapter's parser and the adapter's sequence rule through `md-core`'s `GapDetector`. The latest merged `Ticker` per topic is kept too.
3. **Resync.** On a gap the feed asks the adapter (`on_gap`) and does it: `Resubscribe` (unsubscribe and subscribe that topic on the same connection), `Reconnect` (close and reconnect the connection) or `RestSnapshotThenReplay` (buffer deltas, fetch the REST snapshot, apply it, replay the buffered deltas that follow it).
4. **Stale flag** per book: stale before its first snapshot, while a resync runs, and when the time since the topic's last applied message exceeds the configured value; cleared by the next applied snapshot or delta.
5. **`market_ref()`** of a book: the lake contract fields `{venue, market, symbol, channel, update_id, venue_ts_ns, recv_ns}` of the last applied message, read in the same lock as the book (the OMS stores it with each order, U13).
6. **Overlapping switch.** Open a new connection for the topics of one plan slot, keep both delivering frames, make the new one active when it has a snapshot of every book topic, then close the old one (used at the UTC day change by U5b).
7. **Events** on a channel: connected, disconnected (with reason), gap (with the chosen resync), resnapshot, stale raised and cleared, parse error, switch done. U5b turns them into L0 notes and metrics.

## User Stories

1. As the strategy developer, I want to read a book, its stale flag and its `market_ref` in one call, so that the reference I send with an order is the state I decided on.
2. As the strategy developer, I want the book to be stale while a resync runs and when the topic goes silent, so that the rule "no order while the book is stale" can be enforced.
3. As the collector developer, I want every received frame, with increasing `frame_no` per connection and a `conn_id` that is unique in my session over all markets, so that L0 holds exactly what each connection received.
4. As the collector developer, I want the day-change switch to keep both connections' frames until the old one closes, so that L1 can write a `handover` row from overlapping data.
5. As the owner, I want tests that never open a non-loopback socket, so that no test talks to a venue.

## Implementation Decisions

- **Crate.** `crates/md-feed`, deps: `md-core`, `tokio` (rt, net, time, sync, macros), `tokio-tungstenite` with `rustls-tls-webpki-roots` (as oms-node), `futures-util`, `reqwest` with `rustls` (as oms-node), `serde_json`. All are already in `Cargo.lock` at the versions oms-node uses. Dev-deps: `md-bybit`, `tokio` test features. No `oms-*` change.
- **Adapter trait, additive only** (md-core): three default methods, so U3 code and other adapters still compile:
  - `resubscribe_frames(&self, plan: &ConnPlan, key: &StreamKey) -> Vec<String>`; default empty. Empty means "cannot resubscribe one topic": the feed reconnects instead. md-bybit implements it as one unsubscribe frame and one subscribe frame for the topic (`op` `unsubscribe` / `subscribe`, documented v5 ops).
  - `snapshot_request(&self, key: &StreamKey, depth: u32) -> Option<RestRequest>`; default `None`. `None` with `RestSnapshotThenReplay` means the feed reconnects.
  - `parse_snapshot(&self, key: &StreamKey, body: &[u8], recv_ns: i64) -> Result<BookUpdate, ParseError>`; default `ParseError`.
  - Bridging after a REST snapshot with `update_id` S (the `PrevEquals` note in `seq.rs`, open for U10): the feed drops buffered deltas with `update_id <= S`; the first remaining delta bridges when `first_update_id <= S + 1 <= update_id` if it has `first_update_id` (Binance `U`), else when it follows S by the stream's rule (`PlusOne`: `update_id == S + 1`; `PrevEquals`: `prev_update_id == S`). It is then applied and the detector is restarted at its `update_id` (`GapDetector::snapshot`). A first delta that does not bridge is a failed resync (retry once, then reconnect). U10 may override this with an adapter method if Binance's rule differs; that is additive.
- **Feed construction.** `Feed::start(adapter: Arc<A>, cfg: FeedConfig, plans: Vec<ConnPlan>, sink: Option<Arc<dyn FrameSink>>) -> FeedHandle` on the caller's tokio runtime. `FeedConfig { venue_market: market name for market_ref, ws_url, rest_base: Option, conn_ids: Arc<ConnIds>, stale_after: Duration (one value, per-channel override map), keepalive from the adapter, silence_timeout, backoff_min, backoff_max, resync_timeout, loopback_only: bool }`. `ConnIds` is a shared allocator (an `AtomicU32` starting at 1) so that every connection of a collector session, over all markets, gets its own `conn_id`; `0` is reserved for REST and notes (contract 2.2).
- **conn_id and frame_no.** A reconnect is a new connection (contract 2.2 and 3.5 `reconnect`): it gets a new `conn_id`, and `frame_no` counts again from 0. Every received text or binary data message counts (`frame_no` is its position in the connection); WebSocket ping, pong and close frames are not data and are not counted or sunk. REST bodies get `conn_id` 0 and a feed-wide counter for `frame_no` (contract: "`0` for REST"; the counter is my reading of "position inside its connection" for the REST pseudo-connection — judgement). The sink sees `RawFrame { recv_ns, src: Ws | Rest, conn_id, frame_no, conn_index, payload }`, the fields of md-lake `Record` plus `conn_index` (the plan slot, for routing files). `FrameSink::frame(&self, f: RawFrame)` is synchronous and must not block (U5b puts a bounded queue behind it).
- **recv_ns** is read with `SystemTime::now()` immediately after the read returns, before parsing. A value that went backward inside one connection is passed on unchanged (U5b writes the `clock_step` note).
- **Book state and reads.** Each connection task owns the `BookState` of its topics behind one `std::sync::RwLock` per (connection, topic): `{ book: OrderBook, last_ref: Option<MarketRef>, last_applied: Option<Instant>, resyncing: bool, has_book: bool }`. A plan slot has an active connection; reads go to the active connection's state. `FeedHandle::read_book(&key, |view: BookView| -> R) -> Option<R>` gives the book, `market_ref` and `stale` under one read lock. `market_ref(&key)`, `is_stale(&key)` and `ticker(&key)` are shorthands. `MarketRef { venue, market, symbol, channel, update_id: Option<i64>, venue_ts_ns, recv_ns }`.
- **Messages.** Parsed trades and tickers are applied (tickers merged with `Ticker::merge`) and also passed to an optional `MessageSink` (the strategy may want trades). Control frames go only to the frame sink.
- **Gap handling.** A delta that the detector reports with `last_good = Some(..)` is a gap: one `Gap` event, the book becomes stale (`resyncing`), and the resync starts. Deltas while resyncing give no further event (the detector already stays silent). A delta before any snapshot on a `Resubscribe`/`Reconnect` venue is also a gap (`last_good = None`); on a `RestSnapshotThenReplay` venue it is the start of the initial sync, not a gap event. A resync that does not end with an applied snapshot within `resync_timeout` reconnects the connection.
- **Resnapshot.** A snapshot on a (connection, topic) that already had a book gives a `Resnapshot` event (counted for metrics; U3 note). The book restarts from it.
- **Stale.** `stale = !has_book || resyncing || now − last_applied > stale_after(channel)`, computed on read with `Instant` (monotonic). A per-feed checker (every `stale_after / 4`, at least 50 ms) sends `StaleRaised { key, conn_id, since_recv_ns }` and `StaleCleared { key, conn_id, recv_ns }` events on transitions. Applies to book topics only; trades and tickers are not stale-checked in U5a (judgement: a quiet symbol has no trades).
- **Silence and keepalive.** The adapter's keepalive frame is sent every `interval_ms`. When nothing is received for `silence_timeout` (default 2 × keepalive interval + 5 s, judgement) the connection is closed and reconnected. Backoff: `backoff_min` 500 ms doubling to `backoff_max` 30 s; reset when a connection has applied a snapshot. Reconnects of one feed are spread by `conn_index × 100 ms` (judgement; no randomness).
- **Overlapping switch.** `FeedHandle::switch(conn_index) -> SwitchTicket`: opens a new connection with the same `ConnPlan` (new `conn_id`). Both connections send frames to the sink and keep their own books. When the new one has an applied snapshot for every book topic of the plan (or the plan has no book topics and the new one has received a frame of each topic), the slot's active connection becomes the new one, the old one is closed, and `SwitchDone { conn_index, from_conn_id, to_conn_id }` is sent. If that does not happen within `resync_timeout × 2`, the new connection is closed and `SwitchFailed` is sent; the old one stays.
- **Loopback guard.** With `loopback_only` the feed resolves the host and refuses to connect unless every address is loopback (WS and REST). All tests set it. A test proves a non-loopback URL is refused before any connect.
- **Errors.** No panics on venue input. A parse error gives a `ParseError` event (and the frame is already in the sink); the book of that topic is treated as a gap if the frame was a book delta of it (the book may now be wrong).
- **No new journal events, no registry entries.** md-feed writes no journal.

## Testing Decisions

- **Seam A**: the public API of `md-feed` (`Feed::start`, `FeedHandle`, `FrameSink`, events), driven from integration tests in `crates/md-feed/tests/`. **Seam B**: loopback fake servers in `crates/md-feed/tests/common/`: a WebSocket server on `127.0.0.1:0` (tokio-tungstenite `accept_async`) whose script per accepted connection is a list of steps (send frame, wait for a client frame matching X, pause, close), and a REST server that answers fixed bodies (hyper is in the lock through reqwest; a minimal `TcpListener` HTTP/1.1 responder is enough and avoids a new dependency).
- **Adapters in tests**: the real `BybitAdapter` with synthetic instruments and synthetic frames (documented shapes) for the WebSocket path; a test-only adapter in `tests/common/` for `RestSnapshotThenReplay` and for `Reconnect`.
- **Tests prove** (done criteria): every frame reaches the sink, with `frame_no` 0, 1, 2… per connection and a new `conn_id` after reconnect; a missing `u` gives exactly one `Gap` event and a resync, and the book after the resync equals the fake's book; with REST resync the book after snapshot + replay equals the fake's book; the stale flag rises when the fake stops sending for longer than `stale_after` and during a resync, and clears on the next applied message; `market_ref()` returns the fields of the last applied message; the overlapping switch keeps delivering frames from both connections until the old one is closed and reads go to the new one after `SwitchDone`; keepalive frames are sent; silence triggers a reconnect; a non-loopback URL is refused with `loopback_only`.
- **Timing.** Tests use real time with short settings (`stale_after` 200 ms and the like) and wait for events with a timeout, never fixed sleeps to assert absence shorter than 3× the setting (flakiness guard).
- **Fixtures are synthetic.** No captured venue data (public repository; Bybit API terms 6.7).
- **Prior art.** `crates/md-bybit/tests/`, the HL stream loop in `crates/oms-node/src/venue/hyperliquid/mod.rs`.

## Out of Scope

- Instrument fetching and paging (the caller builds the adapter; `RestClient::get` is public so U5b can fetch with the same loopback guard).
- L0 writing, notes, bounded queues, the disk floor, day-change scheduling (U5b), metrics export (U6).
- Binance, OKX and Hyperliquid adapters (U10, U11). Binance's server ping needs tungstenite's automatic pong to be flushed; the driver flushes after every read when a pong is queued — tested in U10 with its adapter.
- Any change to `oms-*` crates, `.github/`, the journal or registry contracts.

## Further Notes

- Built directly by Claude Opus 5.5 (2026-11-19), lean spec (owner: "검증 사소한 것까지 하지마").
