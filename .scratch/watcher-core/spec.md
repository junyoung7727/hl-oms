# Spec: the independent watcher, part 1: own journal reader and sets, own Bybit client, checks W1–W4, heartbeat, independence check (unit U19a, PRs U19a1–U19a5)

Status: ready-for-agent

Design source: `RESEARCH/CRYPTO_TRADING_INTEGRATED_TN_20261118/10_DESIGN_DRAFT.md` §7.5 (table of W1–W5, budget 1 + 1 + 0.5 + 0.2 = 2.7 req/s, independence, heartbeat), §7.4 (own watcher key, IP bound), §7.6 (the watcher's `watcher_mismatch` row), amendments R1–R5 (execType classification MUST 1, W2 rule SHOULD 2, PositionAdjusted SHOULD 7, W4 paging SHOULD 8, settle coin SHOULD 7, public IP and key expiry SHOULD 4, watcher split SHOULD 10, conformance crate SHOULD 3); `pr_plan.json` unit U19a (title, scope, checks, done criteria: binding). Bybit facts: `RESEARCH/BYBIT_V5_GATEWAY_FACTS_20261120.md` Q4 (query-api), Q8 (instruments-info), Q9 (execution/list), Q10 (order/realtime), Q12 (position/list), Q13 (wallet-balance). Handoffs: `.scratch/kill-switch/spec.md` "Admin inputs (sent by the watcher, U19a/U19b)" and "Socket and heartbeat format"; U18b reports in `.scratch/crypto_dev2/parts/U18b1..4/report_impl.md` (outside the repo).

Market data and venue are Bybit only. Units U10/U11 (Binance, OKX, Hyperliquid) are deferred by the owner; nothing here needs them.

Numbers marked 판단(모델 아님) are judgements without a model or a measurement behind them. The design's own numbers (1 s, 2 s, 5 s, 10 s, 2 rounds, 2.7 req/s, 10 pages, 60 s) are design values (also judgements there, "testnet에서 조정"); they are cited as design values. Every validation bound in the config tables (the ranges, not the defaults) is 판단(모델 아님).

## Problem Statement

The OMS checks itself: its gateway reconciles open orders, fills and positions against Bybit, and since U18b it can hold plain orders on a watcher's word (`watcher_mismatch`) and stop trading when no watcher reports (the monitoring gate). But nothing yet plays the watcher. A bug in the OMS's journal encoding, its reconciliation, or its Bybit parsing would be checked by the same code that made it. Mainnet is refused without the monitoring gate (`docs/contracts/liveness.md` part 2, U18b4), so without a watcher the OMS cannot trade at all.

U19a adds the watcher: a separate binary that shares no code with the OMS. It reads the OMS journal with its own reader, built only from the documented frame format and the C-TAIL field table. It builds its own open-order, fill and position sets. It queries Bybit with its own key, signer and client. It runs the checks W1 (open orders), W2 (positions), W3 (wallet and margin) and W4 (executions). On a confirmed difference it sends `watcher_mismatch` (not `halt`) to `admin.sock`. Every 5 s it sends a heartbeat with each check's last success time to `monitor.sock`. A CI script proves that it depends on no `oms-*` or `md-*` crate. It sends no alert and has no kill path: W5, the kill marker, stopping the OMS, cancel-all and `[WATCH P1]` are U19b.

## Facts from the existing code

Journal format (what the watcher must reimplement, not reuse):
- Segment file `journal-<first seq, 20 digits>.oj`; header `"OMSJRNL\x01" | u32 LE length | JSON {format, record_schema, instance_id, first_seq, created_at_us, writer} | u32 LE CRC-32 of the JSON` (`docs/adr/ADR-011-file-format.md:69-76`; code `crates/oms-journal/src/segment.rs:28-44`, `:109-143`). `FORMAT = 1` (`segment.rs:44`); header JSON at most 64 KiB (`segment.rs:29`).
- Frame: `u32 LE payload length | u32 LE CRC-32 (IEEE) over bytes 8.. (sequence + payload) | u64 LE sequence | payload` (`crates/oms-journal/src/frame.rs:1-13`, encode `:19-29`, decode `:47-85`). Payload at most 16 MiB; a larger length is damage (`frame.rs:15-17`, `:58-62`). Within a segment the sequence must continue (`segment.rs:210-218`).
- Reading rules for programs outside the OMS (C-TAIL, `docs/contracts/tail.md:7-17`): no `LOCK`, no write; never a record above the durable cap (`journal.durable_seq` of `/api/status`); segment rotation; a header-only segment re-read for a new schema; damage at or below the cap is an error and the position does not move; the reader keeps its own cursor.
- Record payload `{"seq", "at" (µs), "input", "events": [{"<name>": {…}}]}`; only `events` matter; decimals are strings with at most 8 places and no exponent; record schemas 1 and 2; an unknown event name stops the reader (`tail.md:21-28`).
- The sets the watcher must build, event by event (`tail.md:32-52`): order info from `order_accepted` until `purged` (not removed at `order_closed`: a `fill_booked` may come after the close, `tail.md:34`); open orders = accepted without `order_closed`; fills from `fill_booked` (side and instrument from the order info) and `external_fill_booked`; position = Σ `fill_booked` + `external_fill_booked` + `venue_position_changed` + `position_adjusted` quantities (`tail.md:52`); `funding_booked` never changes a quantity (`tail.md:46`). The field table with every event and field is `tail.md:58-267`; it is checked against the encoder by `event_fields::the_field_table_lists_every_field_the_encoder_writes` (`tail.md:56`).
- The OMS core's own sets, for the conformance test: `Core::open: BTreeSet<OrderId>` (`crates/oms-core/src/core.rs:44`), `account: BTreeMap<InstrumentId, Book>` with `Book::pos` (`core.rs:49`, `crates/oms-core/src/model.rs:236-245`), `parked_fills` (`core.rs:52`), `seen_fills: BTreeMap<FillKey, Ts>` (`core.rs:56`; funding and venue-change keys share it, `docs/adr/ADR-023-venue-made-executions.md:60`), `fill_floor` (`core.rs:59`).
- Journals are generated in tests from records the core decided, through the `H` harness of `crates/oms-core/tests/common/mod.rs` (`H::ready` `:131`, `inp` `:265`, `venue` `:288`) and `oms_journal::{recover, Writer}`; model: `crates/oms-projector/tests/projector.rs:7-56` (includes the harness by `#[path]`, writes and syncs every record).

Identifiers:
- Bybit `orderLinkId` = `Base32Pair`: `instance_id` 13 digits `-` `order_id` 13 digits, lower-case base32hex `0-9a-v`, 27 characters, strict decoding (`docs/contracts/identifiers.md:15`, `docs/adr/ADR-022-wire-id-form.md:55-56`; code `crates/oms-core/src/ids.rs:213-245`). Vector: instance `0x0123456789abcdef`, order 42 → `028q5cu4qnjff-000000000001a` (`ids.rs:302-304`; recomputed independently with Python for this spec).
- Fill key of the Bybit gateway: `<symbol>:<execId>` (`docs/contracts/gateway.md:97`, `:117`; code `crates/oms-node/src/venue/bybit/exec.rs:28-33`). Funding and venue changes use the same key (`ADR-023:75`).
- Execution classification of the gateway: `Trade` → fill (external if the `orderLinkId` is not ours), `Funding` → funding, `AdlTrade`/`BustTrade`/`Delivery`/`Settle` → venue position change `adl`/`takeover`/`delivery`/`settle`, anything else → unclassified (`gateway.md:103`; `exec.rs:92-140`).

The OMS interfaces the watcher talks to:
- `/api/status` on the loopback dashboard (`crates/oms-node/src/http.rs:265`); the `Host` header must be `127.0.0.1:<port>`, `localhost:<port>` or `[::1]:<port>` and only GET is served (`http.rs:46-62`); the listener binds loopback only (`http.rs:254-257`) and is off when `[http] listen` is empty (`crates/oms-node/src/config.rs:157`, `:610-618`). Fields used: `core.instance_id` (16 hex digits, `crates/oms-node/src/status.rs:122`), `journal.durable_seq` (`status.rs:155`).
- Sockets: one JSON line per request, at most 64 KiB, `hello` first, replies `{"id", "ok", "result"}` or `{"id", "ok": false, "error": {"code", "message"}}` (`docs/contracts/api.md:7-11`; `crates/oms-api/src/lib.rs:162-193`). `admin.sock` and `monitor.sock` accept the node's own uid only (`crates/oms-node/src/server.rs:188-191`); pools: admin 8, monitor 2 (`api.md:110`).
- `watcher_mismatch {check: W1..W4, detail: non-empty, ≤ 1024 bytes}` on `admin.sock` → `"held"` / `"already held"`; anything else `bad_request`, nothing recorded (`api.md:32`; wire example `oms-api/src/lib.rs:261`). The hold refuses plain orders until `omsctl clear-hold --reason watcher_mismatch`; the first note stays (`docs/contracts/holds.md`, `api.md:100-101`). In the journal: `hold_set {hold, source, note}` and `hold_cleared {hold, note}` (`tail.md:241-245`).
- `heartbeat {watcher: 1–64 chars, checks: W1..W9 → epoch ms or null, W1 and W2 required}` on `monitor.sock` → `"recorded"` after durable; at most 2 per second per connection; a check time more than 1 s after the record time is `bad_request` (`api.md:34`; wire example `oms-api/src/lib.rs:295-316`; `HEARTBEAT_AHEAD_MAX_US = 1_000_000`, `crates/oms-core/src/input.rs:156`). The socket exists only with `[core.policy.monitor]` (`api.md:7`). The gate: no heartbeat for 30 s, or W1 or W2 older than 10 s at the heartbeat's record time → unmonitored; W3..W9 are recorded, not judged (`crates/oms-core/src/config.rs:117-131`, `core.rs:274-290`; U18b4 report "Judgment calls" 1).

The OMS's Bybit code (read only to know the API; the watcher must not reuse it):
- Signing: `hex(HMAC_SHA256(secret, timestamp + api_key + recv_window + payload))`; GET payload = the query string sorted by key, values sent as they are, a value with a character outside `[A-Za-z0-9-_.~%:,]` refused (`crates/oms-node/src/venue/bybit/sign.rs:1-70`); headers `X-BAPI-API-KEY`, `X-BAPI-TIMESTAMP`, `X-BAPI-RECV-WINDOW`, `X-BAPI-SIGN`, `X-BAPI-SIGN-TYPE: 2` (`venue/bybit/rest.rs:251-265`). Independent HMAC values for a made-up secret are in `sign.rs:110-140` and allow-listed in `tools/ci/secret_scan_allow.txt` (the scan fails on any hex run of 40 or more digits that is not allow-listed).
- Answer classification: HTTP 401 and retCodes `10003, 10004, 10005, 10007, 10010, 33004` = key rejected, never retried until restart; `10006`/HTTP 429 rate limited; HTTP 403 = IP ban, every request stops for 600 s; `10002` clock; `10016`/5xx/unreadable = no answer (`rest.rs:97-111`, `:158-195`; `gateway.md:86`, `:89`). IPv4-only local address so the echo sees the address Bybit sees (`rest.rs:103-107`); no redirects, no proxy (`rest.rs:219-221`).
- Start checks of the OMS key: `GET /v5/user/query-api` (`isMaster = false`, `userID`, `readOnly`, `permissions`, `ips`), a key bound to no IP (`[]` or `*`) is refused (`venue/bybit/checks.rs:186-245`); the public IP from an `https` echo URL whose body is the address (`checks.rs:331-366`; config rule `oms-node/src/config.rs:738-741`), rechecked every `ip_check_secs` (60 s, `gateway.md:94`); every traded symbol must have `settleCoin = USDT` in `/v5/market/instruments-info?category=linear&symbol=…` (`checks.rs:173-176`, `:270-293`).
- Queries: `linear()` = `category=linear&settleCoin=USDT` (`venue/bybit/query.rs:98-103`); open orders `/v5/order/realtime` 50 per page, at most 20 pages, open = `New`, `PartiallyFilled`, `Untriggered` (`query.rs:71`, `:91`, `:355-372`); positions 200 per page (`query.rs:92`); executions `/v5/execution/list` 100 per page with `startTime`/`endTime`/`cursor` (`query.rs:90`, `:670-700`); wallet `/v5/account/wallet-balance?accountType=UNIFIED`, fields `totalEquity`, `totalMaintenanceMargin`, `totalMarginBalance` (`query.rs:512-530`; `docs/contracts/margin.md:257-259`).
- Bybit facts: `execution/list` is sorted by `execTime` descending, `limit` 1..100 (default 50), a window of at most 7 days, `startTime` only → `startTime..startTime + 7 days` (gateway facts Q9.3, Q9.7, Q9.8); that an empty `nextPageCursor` means the last page is not documented for these endpoints (Q9.9); `position/list` has `updatedTime` "Position updated timestamp (ms)" (Q12.7); query-api `expiredAt`/`deadlineDay` are "Only for those api key with no IP bound or the password has been changed", and an IP-bound key's example is `"deadlineDay": -2, "expiredAt": "1970-01-01T00:00:00Z"` (Q4, line 97 of the facts file).
- Margin rule values of the OMS: `daily_loss_bps` 500 (`margin.md:35`), `account_mm_rate_max_bps` for Bybit 5000 (`margin.md:42`, 판단 there), the account-rate formula `maintenance × 10 000 ≥ max_bps × margin_balance` with `maintenance > 0` (`margin.md:125`), the daily-loss formula `(base − value) × 10 000 > daily_loss_bps × base` (`margin.md:104`); `day_base_set {day (UTC day number), set_by, value}` (`tail.md:208-210`).

Metrics export (what exists; the watcher writes its own minimal client):
- VictoriaMetrics `/api/v1/import`, one JSON line per sample `{"metric":{"__name__":…, labels}, "values":[v], "timestamps":[ms]}` (`crates/obs-push/src/lib.rs:8-10`). Every series of the projector carries `venue` (`docs/operations/oms-projector.md:29`); `host` defaults to `/etc/hostname` (`oms-projector.md:16`). The projector and obs-push depend on `oms-core`/`oms-journal` (`crates/oms-projector/Cargo.toml`), so the watcher may not use them.

Repository mechanics:
- Workspace members are listed in `Cargo.toml:3`. Lints: `unsafe_code = "forbid"`, `unwrap_used = "warn"` (`Cargo.toml:21-27`); release `panic = "abort"`.
- `http-fake` depends only on tokio (`crates/http-fake/Cargo.toml`); it answers per path from a script (`Answer::{Reply, Hang, Close}`, `route`, `on_request`, `requests`; `crates/http-fake/src/lib.rs:14-21`, `:120-170`). It has no state per request beyond the script.
- CI order: secret scan, toolchain, `tools/ci/selftest.sh`, cache, build, test, failpoints, clippy, docs, rustfmt of added files (`.github/workflows/ci.yml:33-80`). `selftest.sh` copies the gate scripts into a throwaway git repository and plants cases (`tools/ci/selftest.sh:9-40`). `jq` and `python3` exist on the dev host (`/usr/bin/jq`, `/usr/bin/python3`).
- Claims name tests as `<suite>/<test fn>`; the suites known to xtask are fixed in `crates/xtask/src/registry.rs:92-105`; suites `perf`, `mutants`, `source` are not resolved (`registry.rs:462-464`). The verify runner's suites are in `crates/xtask/src/verify.rs:28-90`. The last mutant is M181 (`crates/xtask/src/mutants.rs:1664-1672`); the last ADR is ADR-030 (`registry/decisions.toml:273-276`). The watcher writes no journal event, so `registry/contracts.toml` gets a contract with `events = []`, as C-COMMIT has (`registry/contracts.toml:4-12`).

## Split into PRs (merge order)

| Part | Branch | Content | Size (판단(모델 아님)) |
|---|---|---|---|
| U19a1 | `watcher-journal-sets` | Crates `watcher` (lib + empty-run binary) and `watcher-conformance`; own journal reader (frames, segments, cap, rotation, damage), own decimal and `Base32Pair` code, journal state (orders, open set, fills, venue changes, funding keys, positions, per-symbol and per-order change times, holds, day base, fill floor); `tools/ci/watcher_independent.sh` + selftest cases + CI step; conformance tests against `oms-core` on journals with every event kind; xtask suite `watcher`; ADR-031, C-WATCH part 1 | ≈ 1,400 lines |
| U19a2 | `watcher-bybit-client` | Own signer, REST client and answer classification (never-retried codes, 403 pause, request accounting, request guard), config file and credentials file, start checks (query-api, public IP, `watcher_api_key_days_left`, `watcher_public_ip_ok`, settle coin), the 60 s IP recheck, metrics registry and VM import client; C-WATCH part 2 | ≈ 1,300 lines |
| U19a3 | `watcher-w1-w2-checks` | The sans-IO check engine with an injected clock: scheduler, the two-round rule, W1 and W2, deferral, success times, confirmed differences and the `watcher_mismatch` action, in-process model venue and test journal writer for tests; C-WATCH part 3 | ≈ 1,400 lines |
| U19a4 | `watcher-oms-links-runtime` | `/api/status` client, `admin.sock` client (`watcher_mismatch`, resend rule), `monitor.sock` heartbeat client, the tokio runtime that executes the engine's actions, the binary `watcher --config`, boot and shutdown, end-to-end test on loopback fakes; ops doc, runbook; C-WATCH part 4 | ≈ 1,200 lines |
| U19a5 | `watcher-w3-w4-budget` | W3 (wallet and margin) and W4 (executions: sweeps, paging, watermark, execType table), the W2 fallback to W4's last confirmed execTime, the budget measurement; C-WATCH part 5, remaining claims | ≈ 1,300 lines |

Each part leaves the gate green and the watcher usable to the extent built: after U19a3 the checks are tested but nothing runs; after U19a4 the binary runs W1/W2 and heartbeats; after U19a5 all four checks run. No part starts an always-on service (deployment is U20b).

## Common decisions (all parts)

### Crates and independence
- `crates/watcher`: package `watcher`, a library plus binary `watcher` (`src/main.rs` only parses flags and calls the library). Normal dependencies only from the registry (already in `Cargo.lock` where possible): `serde`, `serde_json`, `toml`, `tokio`, `reqwest` (rustls, no default features), `hmac`, `sha2`, `hex`, `crc32fast`, `rustix` (fs, for owner and mode checks). Dev dependency allowed from the workspace: `http-fake` only. No dependency on any `oms-*` or `md-*` crate and no other workspace crate (`obs-push`, `alert-direct`, … are refused too: the watcher "shares no code with the OMS").
- `crates/watcher-conformance`: package `watcher-conformance`, `publish = false`, an empty `src/lib.rs` and the tests. Dev dependencies: `watcher`, `oms-core`, `oms-journal`, `serde_json`. It is the only package that links the watcher with OMS code.
- `tools/ci/watcher_independent.sh [manifest]` (default `./Cargo.toml`): runs `cargo metadata --format-version 1 --locked --offline --manifest-path <manifest>` and uses `jq`. It fails (exit 1, one line per finding) when:
  1. the package `watcher` is missing;
  2. a declared dependency of `watcher` of any kind (normal, `build`, `dev`; any `target`) is named `oms-*` or `md-*`;
  3. a declared dependency of `watcher` is a workspace path package other than `http-fake` with kind `dev`;
  4. the resolved graph reachable from `watcher` through normal and build edges contains a package named `oms-*` or `md-*`, or any workspace member (transitive reuse);
  5. a workspace package other than `watcher-conformance` depends on `watcher` (the OMS must not link the watcher either).
  It prints `watcher_independent: ok` otherwise. CI: a step "Watcher depends on no OMS or market-data crate" right after "Build" (the registry is fetched by then, so `--offline` resolves).
- `selftest.sh` copies the script into its throwaway repository and builds a synthetic workspace there (stub crates with empty `lib.rs`, no registry dependency, so `cargo metadata --offline` works): `watcher`, `watcher-conformance`, `oms-core`, `md-core`, `oms-journal`, `oms-api`, `obs-push`, `http-fake`, `helper`. Cases: real shape (only `watcher-conformance` depends on `oms-core` and `watcher`; `watcher` dev-depends on `http-fake`) → ok; planted normal dependency `oms-core` → fail; planted `[build-dependencies] md-core` → fail; planted `[dev-dependencies] oms-journal` → fail; planted `[target.'cfg(unix)'.dependencies] oms-core` → fail; `watcher` → `helper` → `oms-api` (transitive) → fail; normal dependency `obs-push` → fail; `oms-api` depends on `watcher` → fail.

### Clock
- Every timer of the watcher reads one injected clock, `trait Clock { fn now_ms(&self) -> u64 }` (wall time, ms since the epoch). The runtime uses `SystemTime`; tests use a manual clock that only moves when the test moves it. Success times on the wire are this clock (the OMS compares them with its own record time on the same host, `api.md:34`).
- A clock that steps back: elapsed times are `saturating_sub`, so timers hold until the clock passes the old value again (as the OMS gate, U18b4 review M1). Known limit, documented in C-WATCH.

### Errors at start and in a round
- Start refusals exit with code 2 and one line on stderr naming the reason. Nothing at start writes a file.
- In a round, an error ends that round without a success and without a comparison. The next round is the retry. There is no retry inside a round (the interval is the backoff; this keeps the request rate within the design budget).

### Metric names (all with labels `venue="bybit"`, `host`, `watcher=<name>`)
| Series | Type | Part | Meaning |
|---|---|---|---|
| `watcher_up` | gauge 1 | U19a2 | sent with every push (absence = the watcher or its push is gone) |
| `watcher_api_key_days_left` | gauge | U19a2 | days to `expiredAt`, rounded down; absent when `expiredAt` does not apply |
| `watcher_public_ip_ok` | gauge 0/1 | U19a2 | the echo's public IP is in the key's `ips` |
| `watcher_auth_rejected` | gauge 0/1 | U19a2 | a never-retried answer stopped all Bybit requests until restart |
| `watcher_requests_total{endpoint}` | counter | U19a2 | requests sent per path; every page counts |
| `watcher_request_errors_total{endpoint,class}` | counter | U19a2 | `not_sent`, `no_answer`, `rate_limited`, `ip_banned`, `clock`, `refused`, `auth`, `unreadable` |
| `watcher_journal_seq` | gauge | U19a3 | last applied record |
| `watcher_journal_error` | gauge 0/1 | U19a3 | the reader stopped (damage, unknown event, instance, sequence went back) |
| `watcher_check_age_s{check}` | gauge | U19a3 | seconds since the check's last success (since the watcher's start before the first one) |
| `watcher_differences{check}` | gauge | U19a3 | differences seen and not yet confirmed (pending or deferred) |
| `watcher_mismatches_total{check}` | counter | U19a3 | differences confirmed |
| `watcher_heartbeat_ok_ts` | gauge | U19a4 | epoch seconds of the last `"recorded"` reply |

No other `watcher_*` series is exported in U19a (W2 deferral shows only in `watcher_check_age_s{check="W2"}`, per scope).

## U19a1 decisions: journal reader, sets, independence, conformance

### Reader (`watcher::journal`)
- `Tail::open(data_dir) -> Tail`, `Tail::read(&mut self, cap: u64, max_records: usize) -> Result<Vec<Record>, JournalError>`. A `Record` is `{seq, at, record_schema, events: Vec<(String, serde_json::Value)>}`; `input` is not parsed.
- It lists `journal-<20 digits>.oj`, keeps (segment first seq, byte offset, next seq) and reads from the offset to the end of the file at each call (no whole-segment load; a frame is at most 16 MiB). It opens files read-only, never opens `LOCK`, never creates, renames or writes anything.
- Header: magic, length ≤ 64 KiB, CRC, `format == 1`, `record_schema ∈ {1, 2}`, `first_seq` equals the expected next seq, the same `instance_id` in every segment. A segment whose frames were not yet read has its header read again at each call (C-TAIL `tail.md:13`).
- Frame: length ≤ 16 MiB, CRC over sequence + payload, sequence = expected, payload JSON with `seq` equal to the frame's and `at` not below the previous record's (C-JOURNAL step 3, `docs/contracts/recovery.md:21`).
- Cap: a frame whose sequence is above `cap` is not decoded or returned (only its 16-byte header is looked at). Damage at or below the cap (short frame, short header, bad CRC, wrong sequence, missing segment, wrong instance) returns `Err` after the good records before it have been returned by an earlier call; the position does not move, and the same call gives the same error until the files change (C-TAIL `tail.md:14`).
- First segment: the watcher has no persisted state and replays from record 1 at every start. A journal whose first segment does not start at 1 is refused (`JournalError::NoStart`), the start waits and logs (judgment, owner question 9).
- `max_records` per call: 10,000 (판단(모델 아님)), so the boot replay of a long journal is many calls and a round in U19a3+ is never stuck behind it.
- Errors: `Damaged {seq, why}`, `UnknownEvent {seq, name}`, `Schema {segment, schema}`, `Instance {expected, found}`, `NoStart`, `Io(kind)`. All stop the reader; `watcher_journal_error = 1` (U19a3).

### Known events and decimals
- `watcher::events::KNOWN` lists every event name of the C-TAIL field table (`tail.md:58-267`). An event outside it stops the reader (`tail.md:28`). Unit test `events::the_known_events_are_the_rows_of_the_tail_table` reads `docs/contracts/tail.md` at test time (path from `CARGO_MANIFEST_DIR`) and compares the name sets in both directions, so a new OMS event makes this test fail until the watcher is taught it.
- `watcher::decimal::Dec(i128)` scaled by 10^8: optional `-`, digits, optional `.` and 1–8 digits; anything else (exponent, `+`, more places, empty) is refused. Arithmetic is exact and checked; overflow is an error.

### `Base32Pair` (`watcher::wire_id`)
- `encode(instance_id: u64, order_id: u64) -> String` and strict `decode(&str) -> Option<(u64, u64)>` from `identifiers.md:15` and ADR-022 §7: 13 digits each side, lower-case `0-9a-v` only, `-` between, the first digit at most `f` (a 65-bit value above `u64::MAX` is refused).

### Journal state (`watcher::state::JournalState`)
`apply(&mut self, rec: &Record) -> Result<(), JournalError>`; every rule is a row of `tail.md:36-50`:

| Event | Change |
|---|---|
| `order_accepted` | order info `{strategy, instrument, side, qty, client_order_id, venue_oid: None, filled: 0, open: true, acked: false, in_doubt: false, last_at}`, added to the open set |
| `order_acked` | `acked = true`, `in_doubt = false`, `venue_oid` if not null |
| `venue_oid_learned` | `venue_oid` |
| `order_in_doubt` | `in_doubt = true` |
| `fill_booked` | order's `filled += qty` (the order may be closed); fill `key → {qty, side, instrument, venue_ts}` from the order info; position += signed qty. Order info missing → `Err` (stop: the side cannot be known) |
| `external_fill_booked` | fill `fill.key → {fill.qty, fill.side, fill.instrument, fill.venue_ts}`; position += signed `fill.qty` |
| `order_closed` | removed from the open set; the order info stays |
| `venue_position_changed` | venue change `change.key → {instrument, qty, kind, venue_ts}`; position += `change.qty` |
| `position_adjusted` | position += `delta` |
| `funding_booked` | funding key `funding.key → {instrument, venue_ts}`; no quantity |
| `purged` | order info of `order_ids` removed (they are closed; an open one → `Err`); fill, venue-change and funding keys with `venue_ts < fill_floor` forgotten; `fill_floor` updated |
| `booted` | `fill_floor` |
| `hold_set` / `hold_cleared` | `holds.watcher_mismatch`, `holds.kill` (`true`/`false`) |
| `day_base_set` | `day_base = {day, value}` |
| any other known event | nothing |

Per order, `last_at` = the `at` of the latest record with any event whose `order_id` (or `fill.order_id`) is that order. Per instrument, `last_change_at` = the `at` of the latest `fill_booked`, `external_fill_booked`, `venue_position_changed` or `position_adjusted` for it (W2 rule, scope). The state also keeps `instance_id` (from the headers), `last_seq`, `last_at`.

Memory: the order info and keys are bounded by the OMS's own purge (`purged`); the watcher forgets what the OMS forgets. No other bound (판단(모델 아님): the OMS's in-memory state has the same size).

### Conformance (`crates/watcher-conformance/tests/conformance.rs`)
- Includes the core harness with `#[path = "../../oms-core/tests/common/mod.rs"] mod common;` (as `oms-projector/tests/projector.rs:7-8`) and writes the records of `H::log` with `oms_journal::Writer` (every record synced), into `OMS_TEST_DIR`.
- Scenarios, together covering every event name of the field table: plain fills and closes, partial fill and cancel, fill after close, external fills, parked and unparked fills, in-doubt and recheck, venue changes (each kind), position adjustment, funding, unclassified execution, mismatch and ack, operator halt/resume, storage low, reference price, margin report, rejected report, day base, reducing and clear, venue setting, account mode, purge, holds (kill, watcher mismatch, venue change, clear), kill loop end and rearm, strategy link loss and restore, monitor heartbeat and change, booted, config. The test asserts the union of event names written equals the table's names (read from `tail.md`) and fails naming the missing ones.
- After each scenario (and after every record of the small scenarios), the watcher reads the journal through `Tail` with `cap = last seq` and must equal the core: open set = `core.open`; for each open order: instrument, side, qty, strategy, client order id, venue order id = `core.orders[id]`; position per instrument = `core.account[i].pos` (instruments with 0 compare as absent); fill ∪ venue-change ∪ funding keys with `venue_ts ≥ core.fill_floor` = `core.seen_fills` keys with the same bound.
- `the_watcher_base32_pair_equals_the_core_form`: for edge ids (0, 1, `u64::MAX`, the vector) and 10,000 seeded random pairs (fixed seed, own xorshift; no `rand` dependency needed), `watcher::wire_id::encode` = `oms_core::WireIdForm::Base32Pair.encode`, and both decoders accept and refuse the same strings (a list of malformed ones).
- `the_watcher_reads_while_the_oms_writer_holds_the_lock`: the `Writer` stays alive (it holds the lock) while the watcher reads; the directory listing and every file's length and mtime are the same before and after.

### Status and boot in U19a1
The binary exists but only prints its version and exits 0 for `--version`, and exits 2 with "not built yet: the checks arrive in U19a3/U19a4" otherwise (no network, no files). This keeps `cargo build --workspace --all-targets` and the CI script meaningful from the first part.

## U19a2 decisions: Bybit client, config, start checks, metrics export

### Config file (`watcher --config <path>`, TOML, unknown keys refused)
| Field | Default | Validation (판단(모델 아님) for the ranges) |
|---|---|---|
| `name` | required | 1–64 chars, no control characters (heartbeat label, `api.md:34`) |
| `network` | required | `testnet` or `mainnet`; REST base `https://api-testnet.bybit.com` / `https://api.bybit.com` (the same hosts as the OMS, `venue/bybit/mod.rs:46-49`) |
| `mainnet_confirmed` | `false` | mainnet refused unless `true` (as the OMS, `oms-node/src/config.rs:715-719`) |
| `credentials_file` | required | absolute, no `..`, outside `data_dir`; at load: a regular file (not a symlink), owner = this process's uid, mode `0600` or `0400`; TOML `api_key`, `api_secret`, both non-empty; `Debug` never prints the secret, an error never quotes the file |
| `sub_account_uid` | required | > 0; the key's `userID` must equal it (the same sub-account as the OMS) |
| `symbols` | required | 1–50 names, each 1–32 of `[A-Z0-9]` |
| `ip_echo_url` | required | `https` (a forged answer would pass the IP check, as `oms-node/src/config.rs:738-741`) |
| `data_dir` | required | absolute; the OMS data directory, read only |
| `status_url` | required | `http://127.0.0.1:<port>/api/status` or `[::1]`; any other host refused (the dashboard is loopback only, `http.rs:254-257`) |
| `admin_socket`, `monitor_socket` | `<data_dir>/admin.sock`, `<data_dir>/monitor.sock` | absolute, different |
| `host` | `/etc/hostname` | label |
| `[intervals]` `w1_ms`, `w2_ms` | 1000 (design §7.5) | 500..=10,000 |
| `w3_ms` | 2000 (design §7.5) | 1000..=30,000 |
| `w4_ms` | 5000 (design §7.5) | 1000..=60,000 |
| `heartbeat_ms` | 5000 (design §7.5) | 1000..=10,000 (well inside the OMS's 30 s) |
| `ip_check_ms` | 60,000 (scope) | 10,000..=3,600,000 |
| `export_ms` | 10,000 (판단(모델 아님)) | 1000..=60,000 |
| `[rules]` `min_age_ms` | 2000 (design §7.5, scope) | 500..=10,000 |
| `w4_overlap_ms` | 10,000 (scope) | 0..=60,000 |
| `w4_max_pages` | 10 (scope) | 1..=50 |
| `w4_initial_lookback_ms` | 600,000 (판단(모델 아님)) | 0..=86,400,000 |
| `mismatch_resend_ms` | 10,000 (판단(모델 아님)) | 1000..=600,000 |
| `[w3]` `account_mm_rate_max_bps` | 5000 (the OMS's Bybit value, `margin.md:42`, 판단 there) | 1..=9999 |
| `daily_loss_bps` | 500 (the OMS default, `margin.md:35`) | 1..=10,000 |
| `[http]` `recv_window_ms` | 5000 (the OMS default) | 1000..=20,000 (`oms-node/src/config.rs:745`) |
| `request_timeout_ms` | 2000 (판단(모델 아님)) | 200..=10,000 |
| `[telemetry]` `vm_url` | none (nothing exported) | `http://` loopback only |

The confirmation count (2 rounds) is a constant, not config: the scope fixes it. "All intervals in config" (scope) is met by `[intervals]` and `[rules]`.

### Signer and client (`watcher::sign`, `watcher::rest`)
- Own HMAC-SHA256 and query string per `gateway.md:93` ("서명" row): GET only in U19a (POST signing is written and unit-tested for U19b's cancel-all, not used). Sorted keys; values sent as they are; a value with a character outside `[A-Za-z0-9-_.~%:,]` is refused before anything is sent.
- `Client::get(path, params) -> Result<Value, RestError>`: IPv4 local address, connect timeout 3 s, `request_timeout_ms`, no redirects, no proxy, the five headers. `RestError` classes and their effect:

| Answer | Class | Effect |
|---|---|---|
| connect failed, nothing sent | `not_sent` | round fails |
| timeout, HTTP 5xx, unreadable body, no retCode, retCode 10016 | `no_answer` | round fails |
| HTTP 401; retCode 10003, 10004, 10005, 10007, 10010, 33004 | `auth` | **never retried**: every Bybit request stops until the process restarts; `watcher_auth_rejected = 1`; one error line with the code |
| HTTP 403 | `ip_banned` | every Bybit request (and the echo) stops for 600 s (F18 L186, as the OMS, `rest.rs:108-111`) |
| HTTP 429, retCode 10006 | `rate_limited` | round fails |
| retCode 10002 | `clock` | round fails |
| other non-zero retCode | `refused` | round fails |
| retCode 0 but the rows cannot be read | `unreadable` | round fails (counted by the check, not the client) |

  The scope names 10010, 10003 and 33004; the other three codes and HTTP 401 are added because the OMS treats them as not healing (`rest.rs:97-101`) and stopping is the safer side (owner question 3).
- Request guard: at most 60 requests in any 10 s window, all Bybit paths together (판단(모델 아님); see U19a5 for the worst case of 47); a request beyond waits for the window, it is not dropped. This protects the IP limit the OMS shares (600 per 5 s per IP, design §7.3) from a runaway loop.
- Every request is counted in `watcher_requests_total{endpoint}` before it is sent.

### Start checks (`watcher::start`), in order
1. Config and credentials (above). Refusal → exit 2.
2. `GET /v5/user/query-api`: `isMaster == false`, `userID == sub_account_uid`, `ips` is a list of strings with at least one address and no `*` (a key bound to no IP is refused, as the OMS `checks.rs:186-192`; owner question 2). Refusal → exit 2. An `auth` answer → no exit: the watcher stays up with `watcher_auth_rejected = 1` and never sends another Bybit request (its checks never succeed, so the OMS gate closes; this is visible, a restart loop is not).
3. `watcher_api_key_days_left`: from `expiredAt`, RFC 3339 UTC `YYYY-MM-DDTHH:MM:SSZ` (own parser; fractional seconds accepted and dropped). Not exported when the field is absent, unreadable (one warn line), or at or before `1970-01-02T00:00:00Z` (the documented "does not apply" example is the epoch). Otherwise `floor((expiredAt − now) / 86,400 s)`, which is negative once expired.
4. Public IP: GET `ip_echo_url` (IPv4, timeout `request_timeout_ms`, body = the address); `watcher_public_ip_ok = 1` if it is in `ips` (IPv4-mapped IPv6 compares as IPv4), else 0 and one warn line. Not a refusal: Bybit will answer 10010 and the watcher stops itself (row 2 of the table).
5. Every symbol: `GET /v5/market/instruments-info?category=linear&symbol=<s>`. The row for that symbol must exist and have `settleCoin == "USDT"`; otherwise exit 2 ("<s> settles in <coin>; the watcher's scope is linear/USDT"). Transport errors (`not_sent`, `no_answer`, `rate_limited`, `refused`) retry every 5 s (판단(모델 아님)) and log; the checks do not start until every symbol is confirmed.
6. Every `ip_check_ms` after that: query-api again (the IP list may change) and the echo → `watcher_public_ip_ok`, `watcher_api_key_days_left` refreshed. These requests are outside the 2.7 req/s reconciliation budget (1 per minute) and inside the guard.

### Metrics export (`watcher::metrics`)
- A registry of the series above (gauges and counters, labels sorted). Every `export_ms` the current values go in one POST to `<vm_url>/api/v1/import`, one JSON line per sample with the export time. No queue: a failed push is dropped and logged once per minute (gauges are re-sent fresh; counters are cumulative). At shutdown one last push (best effort, `request_timeout_ms`).

## U19a3 decisions: the check engine, W1 and W2

### Engine (`watcher::engine`, sans-IO)
- `Engine::new(cfg, journal_state, start_ms)`; `poll(now) -> Vec<Action>`; `on_venue(now, req_id, Result<Value, RestError>) -> Vec<Action>`; `on_status(now, Result<Status, String>) -> Vec<Action>`; `on_admin(now, check, Result<String, String>) -> Vec<Action>`. `Action` = `Venue {req_id, path, params}`, `Status`, `Mismatch {check, detail}`, `Heartbeat {checks}`, `Export`. The engine reads the journal itself through `Tail` (synchronous, bounded by `max_records`).
- Scheduler: each check has `next_due`. At `poll(now)` a due check starts a round unless its previous round is still open (no overlap, no pile-up); `next_due = round start + interval`. A late round runs once, not several times.
- A round: (1) the venue requests, pages one after another; (2) one `Status` request after the last answer (`journal.durable_seq`, `core.instance_id`); (3) the journal is read up to that cap; (4) the comparison. The venue is read **before** the journal: the OMS sends nothing before its record is durable (C-COMMIT), so anything the venue shows that the OMS made is already in the journal the watcher reads after it.
- `Status` answer refused when: not JSON; no `journal.durable_seq` (u64) or `core.instance_id` (16 hex); `instance_id` differs from the journal headers (→ journal error "instance", the reader stops); `durable_seq` below the watcher's applied `last_seq` (→ journal error "durable sequence went back": a different journal). An unreachable OMS fails the round (no judgment without the journal; owner question 8).
- Success: a round succeeds when every request answered, the journal reached the cap, and every subject was judged (agree, or a difference that counts). Its success time is the clock at the end of the comparison. A failed or deferred round leaves the old success time; `watcher_check_age_s{check}` grows.

### The two-round rule (W1, W2; reused by W3 and W4 in U19a5)
- A difference has an identity `(check, subject, kind, value)`. It is **confirmed** when the same identity was present in the previous completed round of that check and in this one, and at this round the age condition holds (W1: the order's `last_at` older than `min_age_ms`; for an order unknown to the journal, the first round it was seen is older than `min_age_ms`; W2: below). A difference present in one round only never alarms. The first round may be fresh: the age is judged at the confirming round (this is what lets W2 confirm within 3 s, see the done criteria).
- A difference whose identity changes (another value, another kind) starts again at 1.
- On confirmation: `watcher_mismatches_total{check} += 1`, one error log line, and the engine emits `Mismatch {check, detail}` subject to the resend rule (U19a4). A confirmed difference stays listed (log and `watcher_differences` excluded) until a round no longer sees it.
- `detail`: `"<check> <kind> <subject>: venue <…>, journal <…>; seen 2 rounds, journal entry <a> s old"`, with `"; and N more"` when the round confirmed several; cut to 1024 bytes at a char boundary (`api.md:32`).

### W1 open orders (every `w1_ms`)
- Venue: `GET /v5/order/realtime?category=linear&settleCoin=USDT&limit=50` and `cursor` pages while `nextPageCursor` is not empty, at most 20 pages (판단(모델 아님), as the OMS `query.rs:71`); more pages → the round fails ("more than 1000 open orders"). Open = `orderStatus ∈ {New, PartiallyFilled, Untriggered}`; other rows are ignored. A row must have `orderLinkId` (string, may be empty), `orderId`, `symbol`, `side`, `qty`; else `unreadable`.
- Journal: the open set and order info.
- Differences:

| Kind | Condition | Subject |
|---|---|---|
| `foreign` | open at the venue; `orderLinkId` does not decode as `Base32Pair` with the journal's instance, or decodes to an order id the journal never accepted | venue `orderId` |
| `closed_in_journal` | open at the venue; the order is closed in the journal | `orderLinkId` |
| `details` | open in both; symbol or side differ | `orderLinkId` |
| `missing_at_venue` | open in the journal, `acked` and not `in_doubt`; not open at the venue | `orderLinkId` (encoded) |

  An open journal order that is not acked, or in doubt, and not at the venue is not compared: it may be unsent or its result unknown, and the OMS resolves it itself (C-GW G-1/G-5; owner question 5). Quantity is not compared (amends and partial fills change it; W4 and W2 cover fills).

### W2 positions (every `w2_ms`)
- Venue: `GET /v5/position/list?category=linear&settleCoin=USDT&limit=200` and cursor pages, at most 5 pages (판단(모델 아님)); more → round fails. Per row `symbol`, `side` (`Buy` +, `Sell` −, `""` or `None` 0), `size` (decimal), `updatedTime` (ms string, optional). Several rows for one symbol (hedge mode) → `unreadable` (the OMS refuses hedge mode, `checks.rs:313-326`).
- Journal: position per instrument = Σ fills + external fills + venue changes + adjustments (`tail.md:52`). Parked fills are not counted (scope wording; owner question 4).
- Per symbol in either set: `d = venue − journal`. `d == 0` → agree. `d ≠ 0`:
  - if the previous round had the same `d` for the symbol, and the symbol's `last_change_at` is older than `min_age_ms`, and the venue's latest change time is older than `min_age_ms` → confirmed;
  - else if either time is younger than `min_age_ms` → **deferred**: no count of its own beyond the candidate value, and the round is not a success (fills are still arriving; a deferral longer than 10 s closes the OMS gate, design R5);
  - else → first sighting (pending), the round is a success.
- Venue's latest change time: the row's `updatedTime` if present and readable, else (from U19a5) the last `execTime` W4 confirmed for the symbol, else "unknown", taken as old (owner question 10: an unknown time must not block a confirmation).

## U19a4 decisions: OMS links, runtime, binary

### `/api/status` client
- `GET status_url` with `Host: 127.0.0.1:<port>` (or `[::1]:<port>`), timeout `request_timeout_ms`, no redirects. Only `journal.durable_seq` and `core.instance_id` are read.

### `admin.sock` client and the resend rule
- One connection per send: connect, `{"id":1,"op":"hello","client":"watcher/<name>"}`, then `{"id":2,"op":"watcher_mismatch","check":"W<n>","detail":"…"}`, read both replies (timeout `request_timeout_ms`), close.
- Send when a difference is confirmed and the journal does not show the hold (`holds.watcher_mismatch == false`). While a confirmed difference stays and the hold is not in the journal, send again at most every `mismatch_resend_ms`, so an operator's `clear-hold` while the difference persists brings the hold back (owner question 7). `"held"`/`"already held"` → done until the journal shows the hold cleared. `bad_request` → one error line (a watcher bug), the same resend pace. Unreachable → the same resend pace.

### `monitor.sock` heartbeat client
- One persistent connection: `hello` without a strategy, then every `heartbeat_ms` `{"id":n,"op":"heartbeat","watcher":"<name>","checks":{"W1":<ms|null>,"W2":…,"W3":…,"W4":…}}` (W3 and W4 are `null` until U19a5 runs them). Reply `"recorded"` → `watcher_heartbeat_ok_ts`. A closed or failed connection is reopened at the next heartbeat (the pace stays one per `heartbeat_ms`, far below 2 per second per connection, `api.md:34`). `bad_request` (for example a check time ahead of the OMS's clock) → one error line per minute.
- The heartbeat is sent even while every check fails: the gate then sees stale W1/W2 and closes within 10 s (that is the intended effect).

### Runtime and binary
- `watcher --config <path>` (and `--version`). Boot: config and credentials → start checks (U19a2) → wait for `/api/status` (retry every 1 s, a log line every 10 s, 판단(모델 아님)) → replay the journal from record 1 to the cap → start the engine. Heartbeats start with the engine (all `null` until a success).
- The runtime is a single-threaded tokio loop: it calls `engine.poll(now)` every 100 ms (판단(모델 아님)), runs each action on its own task (requests in flight at most one per check, plus status, admin, heartbeat and export), and feeds answers back.
- Shutdown on SIGTERM or SIGINT: no new rounds; in-flight requests get at most `request_timeout_ms`; one last metrics push; the monitor connection closes; exit 0. Nothing is persisted.
- The binary has no network override: the REST base comes only from `network`. Tests call the library's `run(cfg, Endpoints, clock, shutdown)` with loopback endpoints.

## U19a5 decisions: W3, W4, budget

### W3 wallet and margin (every `w3_ms`)
- `GET /v5/account/wallet-balance?accountType=UNIFIED`; the `UNIFIED` row's `totalEquity`, `totalMaintenanceMargin`, `totalMarginBalance` (decimals); else `unreadable`.
- Violations (identity = kind only, value not part of it):
  - `account_mm_rate`: `maintenance > 0` and `maintenance × 10 000 ≥ account_mm_rate_max_bps × margin_balance` (the OMS's formula, `margin.md:125`);
  - `daily_loss`: the journal's `day_base_set` has `day` = today's UTC day number and `(base − totalEquity) × 10 000 > daily_loss_bps × base` (`margin.md:104`). Without today's base this part is not judged (owner question 6).
- Confirmed on 2 consecutive rounds (design §7.5 "한도 위반 2회 연속"); no journal-age rule.

### W4 executions (every `w4_ms`)
- A **sweep** reads the window `[start, end]` with `GET /v5/execution/list?category=linear&settleCoin=USDT&limit=100&startTime=<start>&endTime=<end>` and its cursor pages. `end` = the clock at the sweep's first page and stays fixed for the sweep, so the cursor's row set does not move under new executions. `start = max(watermark − w4_overlap_ms, fill_floor in ms)`; a window longer than 7 days is cut to `end − 7 days + 1 ms` with one warn line (the venue's limit, facts Q9.7).
- Pages: at most `w4_max_pages` per round. If the cursor is still not empty, the next round continues the same sweep with the same `start`, `end` and the cursor; the sweep's rows are compared when the cursor is empty. The success time moves only at a completed sweep, so `watcher_check_age_s{check="W4"}` rises during a multi-round sweep (scope). Every page is one request in `watcher_requests_total`.
- Initial watermark at start: `now − w4_initial_lookback_ms`, not below the journal's `fill_floor`.
- Rows: `execId`, `symbol`, `execType`, `execTime` (ms), `execQty`, `side`; key `<symbol>:<execId>` (`gateway.md:97`). Comparison per `execType` (scope, `gateway.md:103`):

| `execType` | Compared with | Difference |
|---|---|---|
| `Trade` | journal fills (`fill_booked`, `external_fill_booked`) by key | key missing (`missing_fill`); qty or side differ (`fill_details`) |
| `AdlTrade`, `BustTrade`, `Delivery`, `Settle` | journal venue changes by key, kind `adl`/`takeover`/`delivery`/`settle` | key missing (`missing_venue_change`); kind or signed qty differ (`venue_change_details`) |
| `Funding` | nothing | ignored (funding never changes quantity) |
| anything else | — | `unknown_exec_type`: **confirmed at once**, no second sweep (scope) |

- `missing_*` and `*_details` follow the two-round rule across sweeps, with the age condition "the row's `execTime` older than `min_age_ms`". The watermark after a completed sweep = the newest `execTime` such that every compared row at or below it agrees, is ignored, or is already confirmed. So an unconfirmed difference is read again by the next sweep, and a confirmed one no longer holds the window back.
- W4 also records, per symbol, the newest `execTime` of an agreeing `Trade` or venue-change row: W2's fallback for a missing `updatedTime`.

### Budget
- Design budget: W1 1 + W2 1 + W3 0.5 + W4 0.2 = 2.7 requests per second (§7.5), when W1 and W2 take one page and W4 one page per round. The guard (60 per 10 s) is the hard bound; the budget is measured, not enforced.
- **Conflict between the scope and a done criterion** (followed: the scope; owner question 1). The scope's W4 rules (start 10 s before the last confirmed `execTime`, follow every cursor) re-read 10 s of executions every round. With 100 executions every 5 s spread evenly (20 per second) a sweep window holds about 300 rows = 3 pages, so the rate is (60 + 60 + 30 + 36) / 60 ≈ 3.1 requests per second, not ≤ 2.7. With one page per sweep (at most 99 rows in a 15 s window, i.e. at most 33 executions per 5 s) it is (60 + 60 + 30 + 12) / 60 = 2.7. The parts test both: the done criterion at a load inside "at most 100 executions per 5 s" for which it holds, and the even 100-per-5-s load, which is reported as ≈ 3.1 (2.6 % of the venue's 120 per second per IP) with W4 keeping up.
- Worst case for the guard (판단(모델 아님)): in 10 s, W1 10 + W2 10 + W3 5 + W4 2 sweeps × 10 pages + IP check 2 = 47 ≤ 60.

## Testing Decisions

Rules for every part: loopback fakes only (`http-fake` for HTTP, a test-owned `UnixListener` for the sockets, an in-process model venue for the engine); journals generated at test time under `OMS_TEST_DIR`; no real host; a manual clock wherever time matters; every loop in a test on the manual clock has a bound (a step count). Real-time tests (U19a4 end-to-end) run 3 times and the report names the result of each run.

### Done criteria → tests
| Done criterion (pr_plan U19a) | Test | Part |
|---|---|---|
| `watcher_independent.sh` fails on a planted normal, build or dev dependency of the watcher package and passes on the real tree, in which only `crates/watcher-conformance` depends on `oms-core` (selftest planted cases) | `tools/ci/selftest.sh` cases listed in "Crates and independence"; CI step on the real tree | U19a1 |
| in `crates/watcher-conformance`, the watcher's open-order and fill sets and its journal position equal the OMS core's on journals generated at test time with every event kind | `conformance::the_watcher_sets_equal_the_core_on_journals_with_every_event_kind` | U19a1 |
| an `/v5/execution/list` response that mixes Funding rows with known fills produces no mismatch while a position is held | `w4::funding_rows_beside_known_fills_are_no_mismatch_while_a_position_is_held` (W2 and W4 both run; position ≠ 0; funding rows with `execQty` present) | U19a5 |
| an AdlTrade row that the journal does not have is a mismatch | `w4::an_adl_trade_the_journal_lacks_is_a_mismatch` | U19a5 |
| a traded symbol whose settle coin is not USDT makes start fail | `start::a_traded_symbol_that_does_not_settle_in_usdt_refuses_the_start` (http-fake; exit reason and no check request after it) | U19a2 |
| a one-check difference does not alarm, a two-check difference older than 2 s does, a fresh journal entry younger than 2 s does not | `w1::a_one_round_difference_does_not_alarm`, `w1::a_two_round_difference_older_than_2_s_alarms`, `w1::a_journal_entry_younger_than_2_s_does_not_alarm` | U19a3 |
| with a fake that fills 5 times per second for 60 s, W2 reports zero mismatches, and a position difference planted after the fills stop is confirmed within 3 s | `w2::five_fills_per_second_for_60_s_give_no_mismatch` (the model venue fills every 200 ms; the test journal books each fill 300 ms later, 판단(모델 아님); asserts 0 `Mismatch` actions and that `watcher_check_age_s{W2}` rose); `w2::a_difference_planted_after_the_fills_stop_is_confirmed_within_3_s` (after the last fill, the venue position changes by one step with `updatedTime` = that moment and the journal does not follow; asserts the `Mismatch` comes at most 3,000 ms later on the manual clock, for every phase of the 1 s round in steps of 100 ms) | U19a3 |
| a key that expires in 10 days gives `watcher_api_key_days_left = 10` | `start::a_key_that_expires_in_10_days_gives_days_left_10` (`expiredAt` = now + 10 d exactly, and + 10 d − 1 h gives 9) | U19a2 |
| heartbeats carry per-check success times | `heartbeat::heartbeats_carry_the_success_time_of_each_check` (fake `monitor.sock` records the lines: W1/W2 = the engine's success times, `null` before the first; after a failed W2 round its time stays) — W3/W4 added in U19a5 by `heartbeat::w3_and_w4_success_times_reach_the_heartbeat` | U19a4, U19a5 |
| a round with 250 executions is compared in full over 3 pages | `w4::a_round_of_250_executions_is_compared_in_full_over_3_pages` (one missing row among 250 is confirmed on the next sweep; requests = 3 pages) | U19a5 |
| 1,200 executions in one round are compared over two rounds (10 pages, then 2) and `watcher_check_age_s` for W4 rises in between | `w4::executions_1200_are_compared_over_two_rounds_and_the_w4_age_rises_between` | U19a5 |
| the measured request rate of the reconciliation checks in a simulated minute with at most 100 executions per 5 s stays at or below 2.7 req/s | `budget::the_reconciliation_rate_in_a_simulated_minute_stays_at_or_below_2_7` (load 33 executions per 5 s, counted requests in [0, 60 s) ≤ 162); and `budget::at_100_executions_per_5_s_the_rate_is_about_3_1_and_w4_keeps_up` (documents the conflict above) | U19a5 |
| full local gate passes | the runner's gate at the part's HEAD | every part |

### Other tests per part
- U19a1: `journal::frames_and_headers_follow_adr_011`, `journal::the_reader_never_returns_a_record_above_the_durable_cap`, `journal::the_reader_follows_segment_rotation_and_a_schema_change`, `journal::a_header_only_segment_rewritten_with_a_new_schema_is_read_again`, `journal::a_damaged_frame_at_or_below_the_cap_stops_without_moving` (short frame, bit flip, wrong sequence, missing segment), `journal::an_unknown_event_stops_the_reader`, `journal::schema_1_and_2_are_read`, `state::the_sets_follow_the_field_table`, `state::a_fill_after_close_uses_the_kept_order_info`, `state::purged_forgets_closed_orders_and_old_keys`, `state::funding_never_changes_a_position`, `events::the_known_events_are_the_rows_of_the_tail_table`, `wire_id::base32_pair_matches_the_documented_vector`, `decimal::decimals_parse_exactly_and_refuse_other_forms`, `conformance::the_watcher_base32_pair_equals_the_core_form`, `conformance::the_watcher_reads_while_the_oms_writer_holds_the_lock`. The watcher's own tests write journals with a test writer in `crates/watcher/tests/common/` built from ADR-011 (not from oms-journal).
- U19a2: `sign::the_strings_to_sign_and_signatures_match_independent_values` (the venue's example strings and RFC 4231 case 2; HMAC values computed with Python `hmac` for a made-up secret, allow-listed in `tools/ci/secret_scan_allow.txt` with the reason), `sign::query_strings_are_sorted_and_unsafe_values_refused`, `client::every_request_carries_the_five_headers_and_the_signed_query`, `client::answers_are_classified`, `client::auth_answers_are_never_retried` (for each of 10010, 10003, 33004, 10004, 10005, 10007 and HTTP 401: after the answer no further request reaches the fake on any path while the manual clock moves 10 min), `client::http_403_pauses_every_request_for_600_s`, `client::the_guard_holds_requests_beyond_60_in_10_s`, `config::the_config_is_validated_and_mainnet_needs_confirmation`, `creds::the_credentials_file_must_be_owner_only_and_outside_the_data_dir` (returns early when run as root, as the existing permission tests), `start::a_symbol_missing_from_instruments_info_refuses_the_start`, `start::a_master_key_another_uid_or_a_key_bound_to_no_ip_refuses_the_start`, `start::the_documented_epoch_expiry_exports_no_days_left`, `start::the_public_ip_is_rechecked_every_60_s`, `metrics::the_import_lines_are_victoria_metrics_json`, `metrics::a_failed_push_is_dropped`.
- U19a3: `w1::an_unacked_order_missing_at_the_venue_is_not_compared`, `w1::a_foreign_order_at_the_venue_is_a_mismatch`, `w1::an_order_closed_in_the_journal_and_open_at_the_venue_is_a_mismatch`, `w1::the_venue_is_read_before_the_journal`, `w2::a_changing_difference_restarts_the_count`, `w2::a_deferred_round_is_not_a_success`, `engine::a_late_round_runs_once_and_rounds_do_not_overlap`, `engine::a_status_of_another_instance_or_a_shorter_journal_stops_the_reader`, `engine::the_detail_is_at_most_1024_bytes`.
- U19a4: `admin::a_confirmed_mismatch_is_sent_once_and_again_after_a_clear` (journal `hold_set`/`hold_cleared`), `heartbeat::the_heartbeat_says_hello_first_and_reconnects`, `runtime::the_watcher_runs_against_loopback_fakes_end_to_end` (http-fake Bybit routes and status, fake sockets, a test journal; real time, intervals shortened by config; ×3), `runtime::without_an_answering_oms_nothing_is_judged`, `runtime::sigterm_ends_cleanly`.
- U19a5: `w4::an_unknown_exec_type_is_a_mismatch_at_once`, `w4::the_sweep_starts_10_s_before_the_last_confirmed_exec_time`, `w4::a_row_loaded_late_inside_the_overlap_is_compared`, `w4::a_missing_trade_needs_two_sweeps`, `w2::a_missing_updated_time_falls_back_to_w4s_last_exec_time`, `w3::a_margin_rate_at_the_limit_on_two_rounds_alarms`, `w3::a_daily_loss_beyond_todays_base_on_two_rounds_alarms`, `w3::without_todays_base_only_the_margin_rate_is_judged`.

### Deliberate defects (each becomes an xtask mutant, next free number from M182; each must be killed)
- U19a1: (1) the reader returns a record whose sequence equals cap + 1; (2) the CRC check is skipped; (3) an unknown event is skipped instead of stopping; (4) `order_closed` removes the order info (fill after close); (5) `venue_position_changed` is not added to the position; (6) `purged` forgets keys at or above the floor. The script's own defects (dev or build edges ignored, transitive check skipped) are caught by the selftest cases, not by xtask (xtask mutants run cargo tests).
- U19a2: (7) 10010 is retried at the next round; (8) the settle-coin check accepts any coin; (9) days left rounded up; (10) the query string is not sorted; (11) the 403 pause is 60 s; (12) mainnet starts without `mainnet_confirmed`.
- U19a3: (13) one round confirms; (14) the age is judged at the first round instead of the confirming one (the 3 s test fails) or not at all (the fresh-entry test fails); (15) W2 confirms two different values; (16) a deferred round counts as a success; (17) unacked journal orders are compared.
- U19a4: (18) the heartbeat sends W2's time as W1; (19) the mismatch is not sent again after a journal `hold_cleared`; (20) `hello` is not sent first on `monitor.sock`.
- U19a5: (21) Funding rows are compared as Trade; (22) AdlTrade rows are matched against fills; (23) an unknown execType waits for a second sweep; (24) the page cap is ignored (all 12 pages in one round); (25) the overlap is not subtracted; (26) a continued sweep restarts from page 1; (27) the watermark passes an unconfirmed row; (28) W3 confirms on one round.

## Documents and registry per part

- U19a1:
  - ADR-031 `docs/adr/ADR-031-watcher.md` (independence and its check, own reader from C-TAIL, replay at every start without persisted state, the conformance crate as the only bridge; alternatives: reuse `oms-journal::Tail`, persist a cursor); `registry/decisions.toml` entry.
  - Contract C-WATCH `docs/contracts/watcher.md` part 1 (reading and sets; refusals of the reader), `registry/contracts.toml` `C-WATCH` (`events = []`, adr ADR-031, code `crates/watcher/src`, claims), `docs/contracts/README.md` row.
  - C-TAIL `tail.md`: the watcher's implementation path and the conformance tests in 검증.
  - xtask: suite `watcher` in `verify.rs` (`cargo test -p watcher -p watcher-conformance`) and in `registry.rs` `suite_dirs` (`crates/watcher/src`, `crates/watcher/tests`, `crates/watcher-conformance/tests`).
  - Claims: V-WATCH-1 (independence; tests named in the `source` suite: the script and the selftest), V-WATCH-2 (reader and sets equal the core).
  - GLOSSARY: Watcher (워처) as a crate; Watcher journal state. README status line. `docs/generated` regenerated.
- U19a2: C-WATCH part 2 (client classification, never-retried codes, 403 pause, guard, start checks, IP recheck, metrics export), ADR-031 §, `secret_scan_allow.txt` lines with the reason, claims V-WATCH-3 (client), V-WATCH-4 (start checks), `docs/operations/watcher.md` (config table, start refusals, metrics), GLOSSARY "Request guard (watcher)".
- U19a3: C-WATCH part 3 (engine, two-round rule, W1, W2, success, deferral), ADR-031 §, claims V-WATCH-5 (two-round rule and W1/W2), GLOSSARY "Check round", "Difference (pending, deferred, confirmed)".
- U19a4: C-WATCH part 4 (status, admin, heartbeat, boot, shutdown), C-LIVE part 2 and C-HOLD: one line naming the watcher as the sender, runbook 7.13 (what each check's `detail` means and what to look at) and a new 7.17 (the watcher's own state: `watcher_auth_rejected`, `watcher_public_ip_ok = 0`, `watcher_api_key_days_left`, `watcher_journal_error`, a rising `watcher_check_age_s`), `docs/operations/watcher.md` run and situations, claims V-WATCH-6 (heartbeat and mismatch sending), README.
- U19a5: C-WATCH part 5 (W3, W4, budget and the conflict), ADR-031 §, claims V-WATCH-7 (W4), V-WATCH-8 (W3), V-WATCH-9 (budget measurement, with the 3.1 figure in `not_covered`), ops doc metrics rows, runbook 7.13 rows for W3/W4, GLOSSARY "W4 sweep", "Watermark (W4)".

Every number without a source in these documents is marked 판단(모델 아님).

## Out of Scope

- W5 (OMS silent → cancel-all), writing the kill marker, `watcher_kill`, stopping the OMS, the confirmation loop and the watcher's own Telegram sender `[WATCH P1]` and its alerts for `auth_rejected` or key expiry: U19b (scope: "No alert sending and no kill path in this unit").
- systemd unit, `trading.slice`, vmalert rules (`watcher_api_key_days_left < 14` = P2, check ages), dashboards: U20b.
- Any run against Bybit testnet or mainnet (no key exists; owner approval U-01 needed); real venue data in fixtures (Bybit terms 6.7).
- Binance, OKX, Hyperliquid (U10/U11 deferred): the watcher is Bybit only; a Hyperliquid OMS has no watcher.
- The reverse W4 direction (a journal fill the venue does not list): W2 compares the positions instead; not in the design table.
- A persisted watcher cursor or state file; a status endpoint of the watcher (metrics and log lines only).
- Changing the OMS: no OMS code changes in U19a (the U18b contracts are used as they are).
- The KR-equity pipeline: untouched.

## Owner questions (the parts use the default; the runner does not wait)

1. **Budget at the largest load.** With the scope's W4 rules (10 s overlap, every cursor), 100 executions every 5 s cost ≈ 3.1 requests per second, above the 2.7 of the done criterion. Default: keep the overlap and every page (no execution goes unread); the 2.7 test uses a load of ≤ 33 executions per 5 s and the 100-per-5-s case is reported as ≈ 3.1. Alternatives: cap at 2.7 and let W4 fall behind, or a shorter overlap.
2. **A watcher key bound to no IP.** Default: refuse the start (as the OMS does; such a key expires in 90 days and the design wants IP binding, §7.4).
3. **Codes that are never retried.** Default: 10010, 10003, 33004 (scope) and also 10004, 10005, 10007 and HTTP 401 (the OMS's set; they do not heal).
4. **Parked fills.** Default: not counted (W2 sums booked quantities only; W4 matches booked fills only). A fill parked for more than about 3 s can therefore hold plain orders.
5. **Open journal orders that are not acked or are in doubt** and absent at the venue. Default: not compared (the OMS resolves them; W2 and W4 catch any fill).
6. **W3 limits.** Default: the watcher's own config with the OMS's values (5000 bps account rate, 500 bps daily loss); the daily loss is judged only when the journal holds today's UTC base.
7. **After an operator clears the hold while the difference persists.** Default: the watcher sends `watcher_mismatch` again (at most every 10 s), so the hold returns.
8. **OMS not answering `/api/status`.** Default: nothing is judged; the checks' ages grow and the OMS's gate closes by itself when it comes back without fresh checks.
9. **Replay at every start.** Default: the watcher keeps no state and reads the journal from record 1 each start; a journal whose first segment does not start at 1 is not read (the start waits).
10. **Venue change time unknown** (`updatedTime` absent and no W4 time yet). Default: taken as old, so a stable difference can still be confirmed.
