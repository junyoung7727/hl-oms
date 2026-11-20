# Spec: Bybit gateway, part 2 (unit U17, PRs U17a–U17d)

Design source: `RESEARCH/CRYPTO_TRADING_INTEGRATED_TN_20261118/10_DESIGN_DRAFT.md` O3–O5 and the rate section, `pr_plan.json` U17. Facts: `RESEARCH/BYBIT_V5_PRIVATE_API_FACTS_20261118.md` §2–§4, §7, `RESEARCH/BYBIT_V5_GATEWAY_FACTS_20261120.md`, the Bybit error page (B/v5/error, read 2026-11-21: HTTP 6 rows, WS OE 8, UTA 203).

## Problem Statement

The Bybit gateway answers every order and cancel with `NotSent` (U16d). The OMS cannot trade on Bybit until the gateway sends them, classifies every answer conservatively (G-2), never resends (G-3), keeps the venue's request limit, and gets a reference price.

## Split

- U17a: rate budget (oms-core), answer-code table, REST create and cancel, G-4 send-time rule, tick and step boot check.
- U17b: WS Trade connection, primary path; REST only while WS Trade is down. Unanswered requests on a dropped connection become `Unknown`.
- U17c: reference price (public ticker stream) and a node end-to-end test against a fake Bybit process, including the failpoints flush case.
- U17d: cross-margin maintenance-margin rule (owner question of 11-20, default = add, X = 50 %, 판단).

## U17a decisions

- Budget: a window, not a token bucket. A bucket of capacity 8 refilled at 8/s lets 16 requests into one second. A request counts from taking its slot until one second after its answer (the venue counts arrivals, which lie in between; review). Orders may go out while fewer than 8 requests (orders and cancels together) count; cancels while fewer than 10. A request of which no byte left gives its slot back at once. All cancels may use the reserve: there is no field that marks a cancel as an emergency, and a cancel only lowers risk. Pure type in oms-core, time as input; the gateway holds a request until the window allows it (it waits, nothing is dropped). WS and REST share one budget (T8 unknown, conservative). Queries are not counted (separate venue limit, 50/s; reconciliation ≈ 2.7/s).
- Answer table (`venue/bybit/answer.rs`): retCode 0 = placed (create) or confirmed (cancel). A listed code = `Rejected`. Every other answer = `Unknown`: unlisted or undocumented codes, HTTP non-2xx, timeout, unreadable body. `Unknown` codes that are documented: 10000, 10014, 10016, 10019, 10429, 429, 20006, 40004, 110030, 110072, 110079, 3400214 (server trouble, duplicates, "processing"). The test holds every documented code number (no descriptions) and checks each is in exactly one class. HTTP 403 stays `Unknown` and starts the ban pause; an auth retCode is `Rejected` and blocks the key (as for queries).
- Placed status: the ack only says accepted (asynchronous, F18 §3.1) → `Placed { status: Open, cum: None }`. A Working order that never appears is re-asked by the open-orders sweep (`OrderRecheck`) and closed by the absence rule.
- G-4: the gateway takes the request time once, signs with it, and sends only if `time + recv_window + 1 s ≤ expires_at`; otherwise `NotSent`. Checked again after any wait for the budget.
- `NotSent` also when a block is set (key rejected, IP ban, account, unchecked) or the connection was not established. No retry anywhere; redirects already off.
- Cancel: by `orderLinkId` (the wire id); `orderId` when known is not needed. Codes 110001/110008/110010 = `Rejected` (the core asks for the status).
- Boot check: the configured `px_tick` and `qty_step` must be multiples of the venue's `tickSize` and `qtyStep` (else `account` block): what the venue does with an order off its steps is undocumented.
- Clock (review): `/v5/market/time` before the boot checks and on the IP timer; a possible local lead over 500 ms (round trip counted as lead) blocks (`clock`). G-4 relies on it.
- Clock (re-review): a round trip over 250 ms is not judged (`Unchecked`); the clock block keeps creates home only (G-4 is a create rule), not cancels or the link; rechecked every `boot_retry` while it blocks; a ban met by the clock check ends the round.
- Budget (re-review): a request sent with no answer counts until `ts + recv_window + 1 s` (its last possible arrival, local clock) plus the window.
- 10018 (IP rate limit) = `Rejected` plus the ban pause (review).

## Testing Decisions (U17a)

Fake HTTP (`http-fake`) only. Prove: 9th order in one second waits while a cancel goes out; window frees after 1 s (deterministic unit tests with time input); every documented code in one class; a dropped or delayed answer = `Unknown` and no second create reaches the fake; expiry too close = no request; a block = no request; 403 pauses; signed body contents (category, symbol, side, orderType Limit, qty, price, timeInForce, orderLinkId, reduceOnly, positionIdx 0).

## Out of Scope

Amend (the core issues no amend), cancel-all (U18b), any real venue call, testnet orders.
