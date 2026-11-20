# Spec: absence rule for Bybit and market_ref on new orders (unit U13c)

Status: ready-for-agent

Design source: `RESEARCH/CRYPTO_TRADING_INTEGRATED_TN_20261118/10_DESIGN_DRAFT.md` §7.3 (O1, O5), §5.4.4 (order ↔ market join key), and `pr_plan.json` unit U13 (outside this repository). U13 split: U13a wire id form (PR #13, merged), U13b venue-made executions and record schema 2, U13c this spec.

## Problem Statement

1. ADR-005 closes an in-doubt order as `NotPlaced` when a status query sent after `expires_at + absence_margin` finds nothing. That needs a venue that refuses a request after its expiry. Hyperliquid has `expiresAfter`. Bybit has no expiry field, but it refuses a request whose `X-BAPI-TIMESTAMP + X-BAPI-RECV-WINDOW` is earlier than its server time ("Bybit_server_time - X-BAPI-RECV-WINDOW <= X-BAPI-TIMESTAMP < Bybit_server_time + 1000", Bybit v5 docs; digest 05 §1). Nothing in the gateway contract says so, and with `venue_enforces_expiry = false` an in-doubt Bybit order would wait for the operator every time.
2. Bybit's `/v5/order/realtime` keeps only "a maximum of recent 500 closed status records" and clears them when Bybit restarts a service (digest 05 §2). One "not found" can therefore be an order that was placed, filled and fell out of the list. The gateway also asks order history, but one answer is still thin evidence for releasing a reservation.
3. The journal does not record which order book state a strategy decided on, so an order cannot be joined to the market data lake (design §5.4.4).

## Solution

1. Gateway contract (C-GW G-4, Bybit row): the request timestamp is the local time at send, and `timestamp + recv_window + 1 s ≤ expires_at`; otherwise the gateway answers `NotSent` before sending. Then the venue enforces the expiry and the Bybit configuration keeps `venue_enforces_expiry = true`. The 1 s covers a local clock up to 1 s ahead of Bybit's (it is refused beyond that); a slower local clock only makes the deadline earlier. `absence_margin_us` (default 10 s) covers the rest, as for Hyperliquid.
2. Policy `absence_not_found_needed` (default 1, the Hyperliquid behaviour; Bybit 2; 0 is a configuration error): the number of "not found" answers that close an in-doubt order. Only answers to queries sent after `expires_at + absence_margin_us` count, and each counted answer must have a `queried_at` at least `absence_margin_us` after the one before (two answers to one query, or to two queries a moment apart, may see one short-lived venue state; review 2026-11-20). The count defends only against such short-lived inconsistency: an order that fell out of the 500-record list stays absent, so the Bybit gateway must send `StatusNotFound` only when both the realtime query and order history miss it (a requirement on U16). With more than one needed, each counted answer is journaled as `AbsenceNoted { order_id, queried_at }` and the count lives on the order (`Order.absence`, not written while none); the last one closes the order in the same input. Evidence of the order moves it out of `PendingSend`, so the count is not used after that. With the default, nothing new is written.
3. `NewOrderReq.market_ref: Option<MarketRef>` with `MarketRef { venue, market, symbol, channel, update_id: Option<i64>, venue_ts_ns, recv_ns }` (the fields of `md_feed::MarketRef`). Not written when absent, so a record without it keeps its bytes. It is part of `OrderAccepted.request` and so of the journal; the core does not read it. The trading API (`oms-api` `Op::NewOrder`) accepts it as an optional field.

## Implementation Decisions

- `market_ref` is not compared for idempotency: a retry usually comes with a newer book, and comparing it would turn a retry into `conflict`. The first request's value stays.
- `market_ref` is not kept in the order state (memory); the export (U15) reads the journal.
- Names in `market_ref` are a string id `RefName` (1..=64 of `[A-Za-z0-9_.:-]`), so a bad value is refused when the API request is parsed and never reaches the core.
- Existing struct literals of `NewOrderReq` and `Op::NewOrder` in tests and tools get `market_ref: None`; no expectation changes.
- ADR-024 records the decision.

## Testing Decisions

`crates/oms-core/tests/absence_and_market_ref.rs` (new, rustfmt-clean): two answers needed; one answer by default with no new event; zero refused; default not serialised; `market_ref` round trip through a journal record; no key when absent; a retry with another `market_ref` is a duplicate; name validation. `oms-api`: a new-order line with `market_ref` parses and a bad name is refused. Claims V-ABSENCE-N-1 and V-MARKET-REF-1.

## Out of Scope

The Bybit gateway (U16, U17), which implements the G-4 check and the history fallback; the export join (U15).
