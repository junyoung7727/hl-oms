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

## U17b decisions

- `venue/bybit/trade.rs`: one WS Trade connection task (`/v5/trade`, IPv4, same connect rules as the private stream, 403 on the upgrade = ban). Auth (retCode 0 = ok; any other retCode = key rejected until restart, like the private stream), then up. Ping every `ping_interval_ms`; a missing pong, a closed socket, an unreadable frame or any block takes it down. Reconnect with the same backoff rules (reset only after the first pong).
- Request: `{"reqId", "header": {"X-BAPI-TIMESTAMP", "X-BAPI-RECV-WINDOW"}, "op": "order.create" | "order.cancel", "args": [body]}`, body = the REST body. `reqId` = `c` + wire id (create, 28 chars) or `x` + attempt (base 36) + wire id (cancel, ≤ 36). Never reused: one create per order, one cancel per attempt.
- Path: the caller hands the request to the connection task and waits. The task answers `NotWritten` if it has no authenticated connection or a block is set before it starts the write; the caller then uses REST (nothing left the host, so this is not a resend). Once the write starts, the request is in a pending map by `reqId`; the answer comes back classified by `answer::classify(200, text)`.
- In doubt: the pending request becomes `Unknown` when the connection goes down (pong missing, socket closed, read error, block) or when no answer comes within `request_timeout_ms`. A late answer for a request already reported is logged and dropped (the core resolves the order by the orderLinkId query). Nothing is sent again, over WS or REST.
- Budget and send-time rule: as U17a, taken once before the path is chosen (a fallback to REST does not take a second slot; the request was never written).
- Effects: retCode auth codes on an answer = key rejected. `10403` (IP limit, WS) = its class plus the ban pause (judgment, the safe side: it is the WS form of REST 10018; whether a ban follows is undocumented). `10429` = `Unknown`, no effect. `10019` (WS Trade restarting) = `Unknown` and the connection is closed and reopened (the doc says a new connection is routed to a normal service).
- Endpoints: `trade_ws` = `wss://stream.bybit.com/v5/trade` (mainnet), `wss://stream-testnet.bybit.com/v5/trade` (testnet); tests give loopback.
- Auth on WS Trade: the private stream's signature (`GET/realtime` + expires); the answer carries `retCode` (0 = ok). Any other answer = key rejected until restart. Pong: `op = "pong"`, or `op = "ping"` with `retMsg`/`ret_msg` = `pong`.
- The clock block (`create_block`) and every other block apply to both paths, checked after the budget wait as in U17a.
- Link: the WS Trade connection does not drive `Link` (the private stream does). Its ups and downs are logged; no status field in this unit.

## Testing Decisions (U17b)

Fake WS Trade server (answer, refuse, stay silent, drop after the request, stop answering pings). Prove: a create goes over WS when up and over REST when down, then back to WS; a silent server makes every unanswered request `Unknown` within ping + timeout and nothing is resent (count of creates on both fakes = 1 per order); a connection that drops after reading the request = `Unknown`; NotWritten falls back to REST only before the write; 20006 / 10019 = `Unknown`; auth refused = key rejected; the ninth create within a second waits across both paths.

## U17c decisions

- Endpoint: `public_ws` = `wss://stream.bybit.com/v5/public/linear` (mainnet), `wss://stream-testnet.bybit.com/v5/public/linear` (testnet), in `Endpoints` and `Wiring`; tests give loopback.
- `venue/bybit/ticker.rs`: one public connection task. Subscribes `tickers.<symbol>` for every traded symbol, at most 10 args per subscribe request (the spot limit, used for every market as in md-bybit); every subscribe answer must say `success: true`, else the connection is retried. IPv4 `connect` as the other two; 403 on the upgrade = ban. Ping every `ping_interval_ms`, the same liveness and backoff rules as the private stream (reset only after the first pong); both pong shapes (F20 2.10: the public linear pong is `op = "ping"`, `ret_msg = "pong"`). Connects only when the private stream would (no block, not quiet). Does not drive `Link`.
- Price: `markPrice` (judgment: the venue's own fair price, not moved by one trade; Hyperliquid uses the mid). Linear sends a snapshot, then deltas with the changed fields only: a delta without `markPrice` means the mark is unchanged as of its `ts`. A delta before the snapshot of this connection is ignored. The state is per connection and starts empty after a reconnect.
- Emission: `VenueInput::RefPrice { instrument, px = mark, venue_ts = frame ts (ms → µs) }` on a ticker frame, at most once per `ref_price_interval_ms` per symbol. No timer re-sends a held price: a symbol the venue stops sending goes stale in the core (`ref_price_max_age_us`), and creates with a price band are refused (the safe side).
- An unreadable ticker frame (not JSON, bad type, bad `markPrice`, bad `ts`) ends the connection (reconnect). A frame for a symbol not traded is ignored.
- Config: `ref_price_interval_ms` (default 5000) in the Bybit section. Must be ≥ 1000 and ≤ half of `core.policy.ref_price_max_age_us` (judgment: one lost update does not make the price stale).
- Node test hook: only in a build with the `failpoints` feature (the shipped binary has none), the node reads `OMS_TEST_BYBIT_WIRING` (JSON: rest, private_ws, trade_ws, public_ws, ip_echo_url) in place of the network's endpoints, and refuses it unless every host is `127.0.0.1`. The configuration file still names a network and an https echo URL.

## Testing Decisions (U17c)

Unit (fake public WS): snapshot then deltas give the mark; a delta without `markPrice` re-emits the held mark after the interval; a delta before any snapshot gives nothing; at most one emission per interval; unreadable frame = reconnect; subscribe refused = retried; missing pong = reconnect; 403 = ban; 10 args per request; config bounds.

Node (`tests/bybit_node.rs`, `--features failpoints`; the node is its own process with the real binary, the fake Bybit servers run in the test process, loopback only):
1. An order with a price band before any ticker is refused (`RefPriceUnavailable`) and no byte reaches the fakes; after a ticker snapshot the same order is accepted. When the fake WS Trade reads the `order.create`, the journal on disk already holds the record of that order (checked from the fake, by reading the journal files). One create in total.
2. The journal flush fails (failpoint) for the new order: the node stops, and no create reaches REST or WS Trade. Restarted without the failpoint, the order is resolved by the orderLinkId queries (realtime and history miss twice) as not placed; still no create.
3. WS Trade reads the create and does not answer: the order becomes in doubt, the realtime query by orderLinkId finds it open, the order is working. Creates over both paths = 1.

## Out of Scope

Amend (the core issues no amend), cancel-all (U18b), any real venue call, testnet orders.
