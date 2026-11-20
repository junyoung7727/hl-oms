# Spec: Bybit gateway, part 1 (unit U16, PRs U16b–U16d)

Status: in progress (U16a merged 0181f6f: fill maker flag and receive time; U16b merged b10e994)

Design source: `pr_plan.json` U16 and the design TN (outside this repository). Facts: `RESEARCH/BYBIT_V5_PRIVATE_API_FACTS_20261118.md`, `RESEARCH/BYBIT_V5_GATEWAY_FACTS_20261120.md` (outside; cited as F18 §n, F20 Qn.m). Decision record: ADR-028.

## Problem Statement

The OMS has no Bybit gateway. Before any order is sent (U17), the OMS must read the Bybit account (orders, fills, positions, margin), follow the private stream, and refuse to trade when the key, the account or the network path is not the one configured.

## Split

- **U16b (merged)**: `VenueConfig::Bybit`, credentials file, v5 HMAC signing (REST and the private-stream auth payload), the REST client and its error classes, the boot checks, the public-IP check, cancel-all (client only), path routes in `http-fake`, ADR-028 (first part), C-GW Bybit column (auth and errors).
- **U16c (this PR)**: the seven queries (`QueryReq`), the execType table, the margin report mapping.
- **U16d**: the private WebSocket (auth, four topics, ping liveness, reconnect), node wiring (`node.venue = bybit:<network>`), status field for the gateway's block reason.

## U16b decisions

- Credentials: `credentials_file` (absolute path, owner-only mode 0600, outside the data directory), TOML `api_key`, `api_secret`. Never logged.
- Signing (F20 Q1.1–1.3): `hex(HMAC_SHA256(secret, timestamp + api_key + recv_window + payload))`; payload = the query string for GET, the body for POST. The query string is built once, sorted by key, and the same bytes go to the URL and the signature; the body likewise. Headers `X-BAPI-API-KEY`, `X-BAPI-TIMESTAMP`, `X-BAPI-SIGN`, `X-BAPI-RECV-WINDOW`, `X-BAPI-SIGN-TYPE: 2` (F20 Q1.5–1.6).
- Test vectors: the docs publish the string to sign but not the secret (F20 Q1.11). Tests: (1) the doc's strings to sign are reproduced exactly; (2) RFC 4231 case 2 through our HMAC; (3) our own vector computed with Python's `hmac` for a fixed secret.
- Error classes: retCode 0 ok; `10003, 10004, 10005, 10007, 10010, 33004` = auth rejected, never retried (scope names 10010, 10003, 33004; 10004/10005/10007 added: a bad signature, a missing permission or a failed authentication does not heal by retrying — judgment); `10006` and HTTP 429 = rate limited; HTTP 403 = IP banned (pause all requests 10 min, F18); `10002` = clock (time window); `10016` and HTTP 5xx = venue error; anything else = refused with the code.
- Boot checks, read-only, all must pass before the gateway may report the link up (U16d):
  - `GET /v5/user/query-api`: `isMaster == false`, `userID == sub_account_uid`, `readOnly == 0`, `permissions.ContractTrade` holds `Order` and `Position`, `ips` is a list of addresses that holds the echoed public IP (`["*"]` or empty = unbound: refused, F20 Q4).
  - `GET /v5/account/info`: `marginMode == REGULAR_MARGIN`, `unifiedMarginStatus` in 3–6 (a unified account; F18 §6, F20 Q6).
  - `GET /v5/position/list?category=linear&symbol=S` per configured symbol: every row `positionIdx == 0` (one-way; F20 Q7.2–7.3).
  - `GET /v5/market/instruments-info?category=linear&symbol=S`: `settleCoin == USDT` (F20 Q8.1).
  A failed check blocks the gateway with a reason (state `blocked`); it never changes a venue setting.
- Public IP: `GET <ip_echo_url>` (body = the address) at start and every `ip_check_secs` (60); not in the key's `ips` → `blocked` (auth), like an auth error.
- Cancel-all: `POST /v5/order/cancel-all {"category":"linear","settleCoin":"USDT"}` (kill scope). Client only; U18b calls it.
- Config: `network` testnet|mainnet; mainnet needs `mainnet_confirmed = true` and `[core.policy.margin]` (as Hyperliquid); the core policy must have `venue_enforces_expiry = true` and `absence_not_found_needed = 2` (ADR-024).
- Endpoints come from the network (F20 Q8, F18 L311); tests construct the gateway parts with loopback URLs. No config key overrides them.

## Testing Decisions (U16b)

Loopback `http-fake` only. Signing: doc strings, RFC 4231, Python vector, header set. REST: each auth code → `AuthRejected` with exactly one request; 10006 → rate limited; HTTP 500 → venue error; malformed JSON → venue error. Boot: each check refuses (master key, other UID, read-only, missing permission, unbound IPs, IP not bound, ISOLATED/PORTFOLIO margin, classic account, hedge row, USDC settle) and all pass together. IP loop: a later mismatch blocks. Config refusals. cancel-all body and signature.

## U16c decisions

- `venue/bybit/query.rs`: `Queries::answer(QueryReq, session) -> Answer { inputs, failed }`. U16d sends the inputs and acts on `failed` (auth, IP ban).
- `venue/bybit/exec.rs`: `classify(row, instance_id, recv_ns)`, the one execution table, also for the stream (U16d). Key `<symbol>:<execId>`.
- Last page only when the cursor is empty and the page is not full (F20 Q9.9: undocumented). Snapshots that cannot prove a last page give no input; a fill window is split in two.
- Status: realtime, then history, both `category=linear&settleCoin=USDT&orderLinkId=…`; not found only if both are empty; rows of other orders conclude nothing.
- Fills: windows of 7 days − 1 ms, each starting 1 ms early; `FillsSynced` per finished window; 30 pages per query.
- Margin: `totalEquity`, `positionIM`, account `marginMode` as every position's mode, whole leverage only, `maxLeverage` cached per symbol. Config refuses account modes other than `REGULAR_MARGIN` for Bybit.
- Details: ADR-028 §7b.

## Testing Decisions (U16c)

Loopback `http-fake` with per-path answers. Every exec type row; status table; realtime → history → not found; other-order rows; paging (two pages with a venue-style cursor, full page without cursor, stuck cursor, endless cursor, 10016 mid-way); positions sign and hedge row; margin mapping and each whole-report refusal; venue setting and account mode; fill windows, overlap, de-duplication, unreadable row, split, page budget; a source scan for order requests.

## Out of Scope

Sending orders (U17), the rate budget (U17), the kill switch and watcher (U18–U19), any real venue call.
