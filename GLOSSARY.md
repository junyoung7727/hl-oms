# hl-oms

An order management system (OMS) for one Hyperliquid account that several strategies share. It decides which orders may go to the venue, tracks them to their end, and limits risk. Each entry gives the English term used in code, specs and tickets, and in brackets the Korean term used in the Korean documents.

## Language

### Parties and orders

**Account** (계좌):
The one venue account whose balance and positions all strategies share.
_Avoid_: wallet, user

**Strategy** (전략):
A program that asks the OMS to place and cancel orders and owns a share of the account's position.
_Avoid_: bot, client

**Operator** (운영자):
The person who runs the node and gives commands that a strategy may not give.
_Avoid_: admin, user

**Venue** (거래소):
The exchange that holds the orders and positions; here Hyperliquid or the simulated venue.
_Avoid_: exchange, HL (in rule text)

**Gateway** (게이트웨이):
The part of the OMS that talks to one venue. It implements the `VenueGateway` trait (send an order, send a cancel, answer a query, start its long-running tasks); one driver runs its command and query loops (ADR-021).
_Avoid_: connector, adapter (adapter is the market-data term)

**Tail** (꼬리 읽기):
A read-only follower of the journal for programs outside the OMS (projector, watcher). It never takes `LOCK` and never returns a record above the durable sequence its caller gives; `peek_beyond` shows later valid frames without moving, for a reader that knows the OMS is dead (ADR-025, C-TAIL).
_Avoid_: subscriber, stream

**Market ref** (시세 참조):
The order book state a strategy decided an order on: venue, market, symbol, channel, the last applied `update_id`, venue time and receive time. Carried by a new order, journaled with `OrderAccepted`, never read by the core and not part of idempotency (ADR-024).
_Avoid_: snapshot id, book ref

**Venue-made execution** (거래소가 만든 실행):
An execution the venue reports in the same stream as fills that is not a fill of an order of ours: a funding payment (cash only), a venue position change (auto-deleveraging, takeover of a liquidated position, delivery, settlement) or an execution the gateway could not classify. A trade of someone else's order is not one: it is an external fill (ADR-023).
_Avoid_: system trade, non-trade fill

**Wire id form** (고객 주문번호 형식):
How a gateway writes the client order id (`instance_id` with `order_id`) for its venue: `Hex128` (`0x` + 32 hex, Hyperliquid and the simulated venue) or `Base32Pair` (27 characters, Bybit `orderLinkId`). Each gateway declares one as `WIRE_ID`; decoding accepts only that exact form (ADR-022).
_Avoid_: cloid (Hyperliquid's word for one form), link id

**Instrument** (종목):
One perpetual contract that the configuration allows, for example BTC.
_Avoid_: coin, asset, symbol

**Reduce-only order** (감소 전용 주문):
An order that carries the venue's reduce-only flag, so the venue itself refuses to let it open or grow a position.
_Avoid_: reducing order, closing order

**Plain order** (일반 주문):
An order without the reduce-only flag, whatever its direction relative to the position.
_Avoid_: increasing order, normal order, non-reduce-only order

**Resting order** (미체결 주문):
An accepted order that is open at the venue, or may be, and is not yet fully filled or closed.
_Avoid_: pending order, working order, unfilled order

**Reservation** (예약):
The quantity of an open order that may still fill, kept in the position interval until terminal evidence arrives.
_Avoid_: lock, allocation

### Refusing orders

**Hold** (차단):
A condition computed from stored state that refuses new orders for as long as it is true.
_Avoid_: block, freeze, lock

**Reject reason** (거절 사유):
The code returned with one refused request; a hold is one kind, a failed check on that request is another.
_Avoid_: error

**Rule cancel** (규칙 취소):
A cancel request that a margin rule issues, not a strategy and not the operator.
_Avoid_: auto-cancel, forced cancel

### Margin

**Margin report** (증거금 보고):
One answer from the venue that gives the account value and, for each position, its value, isolated margin, leverage setting and maximum leverage, all computed by the venue at one moment.
_Avoid_: margin snapshot, clearinghouse state, account state

**Account value** (계좌 가치):
The venue's figure for what the whole account is worth, as given in a margin report.
_Avoid_: equity, balance, collateral

**Fresh** (신선한) / **Stale** (오래된):
A margin report is fresh while its age is within the freshness limit; otherwise it is stale.
_Avoid_: valid, expired

**Reference price** (참조 가격):
The latest mid price of an instrument that the OMS holds, used to value quantities.
_Avoid_: mark price, last price

**Exposure** (노출):
The largest notional the account can reach on an instrument if resting orders on one side all fill, summed over instruments for the account.
_Avoid_: notional, position size

**Account leverage** (계좌 레버리지):
Account exposure divided by account value.
_Avoid_: leverage (alone)

**Venue leverage** (거래소 레버리지):
The leverage number set at the venue for one instrument, which fixes how much margin a position needs there.
_Avoid_: leverage (alone), initial leverage

**Leverage gate** (레버리지 검사):
The check on one new plain order that refuses it when it would raise account leverage above the limit.
_Avoid_: leverage limit hold, margin check

**Isolated margin** (격리 증거금):
The margin the venue assigns to one position alone, so that a loss on it cannot take more than that amount.
_Avoid_: margin used, collateral

**Maintenance margin rate** (유지증거금률):
The fraction of a position's value below which the venue liquidates it.
_Avoid_: MMR (in owner-facing text), liquidation ratio

**MM buffer** (유지증거금 여유):
The distance, in percentage points of position value, that the OMS adds on top of the maintenance margin rate to act before liquidation.
_Avoid_: safety margin, cushion

**Account maintenance-margin rate** (계좌 유지증거금률):
For a cross-margin account (Bybit), the account's maintenance margin divided by its margin balance; the venue liquidates at 100 %. The OMS latches REDUCING at `account_mm_rate_max_bps` (U17d).
_Avoid_: account MM rate (the venue field `accountMMRate`, which the OMS does not read), margin ratio

**UTC day** (UTC 하루):
The period from 00:00 UTC to the next 00:00 UTC.
_Avoid_: trading day, session

**Day base** (기준 금액):
The account value against which the loss of the current UTC day is measured.
_Avoid_: start-of-day balance, high-water mark

**Daily loss** (일 손실):
Day base minus the account value of the latest margin report.
_Avoid_: drawdown, PnL

### States and operator actions

**REDUCING** (감소 전용 상태):
The account state in which only reduce-only orders are accepted, entered by a margin rule and left only by the operator.
_Avoid_: halt, kill switch, reduce-only mode

**NORMAL** (정상 상태):
The account state when REDUCING is not set.
_Avoid_: active, running

**Clear** (해제):
The operator action that ends REDUCING and takes the current account value as the new day base.
_Avoid_: resume, reset, force

**Rebase** (기준 재설정):
The operator action that takes the current account value as the new day base without touching REDUCING.
_Avoid_: reset

**Stale cancel** (보고 끊김 취소):
The rule cancel of every resting plain order when no margin report has been accepted for longer than the stale-cancel limit.
_Avoid_: timeout cancel, watchdog

**Kill latch** (킬 래치):
The hold `KillLatched`: every order is refused, reduce-only included, until the operator clears it. The clear is taken only after the gateway of the running process answered the kill (`confirmed` from the Bybit kill loop, or `unsupported` from a gateway without cancel-all: the simulated venue and Hyperliquid) and only while the OMS book has no open order (`kill_orders_open` otherwise: run cancel-all first) (C-HOLD).
_Avoid_: kill flag, emergency stop

**Kill loop** (킬 루프):
What a gateway does for a `Kill` command: cancel every order at the venue until zero is confirmed (Bybit: one cancel-all per round, a count after each, 30 s of checks after the first zero, C-KILL), or answer `unsupported` when it has no cancel-all. Creates stay home while it runs.
_Avoid_: panic cancel

**Stream-outage trigger** (스트림 끊김 트리거):
The Bybit gateway's kill trigger: the private stream down for longer than `kill_after_ms` after its last proof of life (subscribe confirmed, pong, or the first pass of the boot checks). Not armed before the boot checks pass; time waiting on the core queue is not an outage (C-KILL).
_Avoid_: heartbeat timeout, dead-man switch

**Emergency budget** (비상 예산):
The kill loop's own rate budget for cancel-all, 2 per 1 s on the loop's clock, apart from the order budget (C-KILL).
_Avoid_: kill rate limit

**Kill re-arm** (킬 다시 무장):
A new kill trigger after the kill loop of this process confirmed zero at the venue (not after `unsupported`): `kill_rearmed` voids the confirmation and a new loop runs; the latch keeps its first note (C-KILL).
_Avoid_: re-latch

**Kill marker** (킬 마커):
The file the watcher writes when it kills. Read once before the boot record; anything at its path, or a path that cannot be examined, latches the kill. Removed only after a durable `clear-hold kill`.
_Avoid_: kill file, lock file

**Hold release** (보류 해제):
The operator's `clear-hold` of one hold of C-HOLD (`kill`, `watcher_mismatch`, `venue_change`). It has no force option; each refusal has a stable code.
_Avoid_: unhold, override

**Watcher** (워처):
The separate program (crate and binary `watcher`) that checks the OMS against the venue with code of its own: its own journal reader built from ADR-011 and C-TAIL only, its own sets, its own Bybit key and client. It depends on no `oms-*` or `md-*` crate and on no other workspace crate; only the test crate `watcher-conformance` links it with OMS code, and `tools/ci/watcher_independent.sh` checks this on every change (ADR-031, C-WATCH). It keeps no state and replays the journal from record 1 at every start. U19a1 built the reader, U19a2 the journal state, U19a3 its own Bybit signer and client and its metrics export, U19a4 its config and credentials files, U19a4b its start checks (query-api, the public IP, the settle coin of every symbol) and the IP recheck as sans-IO steps, U19a5 the sans-IO check engine and W1 (open orders), U19a6 W2 (positions), U19a7 the links to the OMS (`/api/status`, `watcher_mismatch` on `admin.sock` with the resend rule, the heartbeat on `monitor.sock`) and the runtime and binary `watcher --config`, U19a8 W4 (executions), U19a9 W3 (wallet and margin), W2's use of W4's execution times and the budget measurement, U19b1a–c its own Telegram sender and the `[WATCH P1]` conditions, U19b2a its one signed POST (cancel-all), the `[kill]` key rules and the kill notice texts, U19b2b the sans-IO watcher kill loop.
_Avoid_: monitor (the monitoring gate is the OMS's), checker, reconciler (that is the OMS's own)

**Watcher journal state** (워처 저널 상태):
What the watcher builds from the journal with its own code (`watcher::state::JournalState`, C-WATCH part 2), one rule per row of the C-TAIL table: the order info from `order_accepted` until `purged` (kept after `order_closed`, so a late fill still has its side and instrument), the open set, the fill, venue-change and funding keys until the fill floor passes them, the position per instrument (booked fills, venue changes and adjustments; funding never), the fact time per order and the change time per instrument that W1 and W2 judge by, the holds, the day base and a fill floor that only goes up. A record that breaks a rule latches it until restart. A conformance test shows that it equals the core's sets after every record.
_Avoid_: watcher cache, shadow core (it is not the core's state and holds only what the checks compare)

**Check round** (검사 라운드):
One run of a watcher check (`watcher::engine`, C-WATCH part 5): the venue pages one after another, then one `/api/status` request of that round only, then the journal read up to that answer's durable sequence, then the comparison. Rounds of one check never overlap; a late round runs once. A round succeeds when every request answered, the journal reached the cap and no difference was deferred; its success time is the clock at the end of the comparison and goes into the heartbeat.
_Avoid_: poll, cycle, tick (the engine's `poll` starts rounds; one `poll` may start none)

**Difference (pending, deferred, confirmed)** (차이(보류 중, 미룸, 확정)):
What a check round sees where the venue and the journal disagree, with an identity `(check, kind, subject, value)`. Pending: first seen in this round (the round may still succeed). Deferred: seen in this and the previous completed round but younger than `min_age_ms` at this round (the round is not a success). Confirmed: seen in both and old enough at this round; counted once, logged once and sent as `watcher_mismatch` (C-WATCH part 5). A difference that a completed round does not see starts again at the first sighting.
_Avoid_: mismatch for an unconfirmed difference (a mismatch is a confirmed one), alarm

**Owed difference** (빚진 차이):
A confirmed difference of the watcher that no `watcher_mismatch` send carrying it has had answered `"held"` yet (C-WATCH part 7). Only `"held"` pays it: not `"already held"`, not a journal that shows the hold, not a failure. It is sent, with the detail of its confirmation, once the hold is off at a status cap sampled after the latest answer, even if no round sees it any more. Memory only: a restart forgets it.
_Avoid_: pending mismatch (pending is a first sighting), queued alert

**W4 sweep** (W4 훑기):
One read of the venue's executions by W4 (`watcher::w4`, C-WATCH part 8) over a window `[start, end]` whose `end` is the clock at its first page and stays fixed, so the cursor's rows do not move under new executions; `start` is the watermark minus `w4_overlap_ms`, not below the fill floor rounded up to ms, and the window is cut to 7 days. A round reads at most `w4_max_pages` pages; a longer sweep continues at the next round from its cursor, and only a completed sweep is compared and moves W4's success time. The two-round rule of W4 counts sweeps.
_Avoid_: round (a sweep may take several rounds), scan, page

**Watermark (W4)** (워터마크(W4)):
The `execTime` W4's next window starts from (less the overlap): after a completed sweep, the newest `execTime` at or below which every compared row agrees, is ignored or is confirmed; it never moves back. An unconfirmed difference, or an unconfirmed row without a readable `execTime`, holds it, so the next sweep reads that row again. It never passes the `end` of the sweep that read it: a row whose `execTime` is after that `end` is `unreadable_row`. Memory only: at start it is `now − w4_initial_lookback_ms` (C-WATCH part 8).
_Avoid_: cursor (the venue's page cursor is another thing), checkpoint (nothing is persisted)

**Request guard (watcher)** (요청 제한(워처)):
The watcher client's own limit: at most 60 requests in any 10 s window, all Bybit paths and the IP echo together (판단(모델 아님)). A sans-IO part on the injected clock (`watcher::rest::Guard::admit(now)`) that lets a request go or names the earliest instant it may go; a request beyond the limit waits, it is not dropped. It protects the IP limit that the OMS shares from a runaway loop of the watcher; it does not count the OMS's own requests (C-WATCH part 3).
_Avoid_: rate limiter (that is the venue's), budget (the 2.7 req/s design budget of the checks, U19a9)

**Reconciliation budget (watcher)** (대조 예산(워처)):
The design's request rate of the watcher's checks: W1 1 + W2 1 + W3 0.5 + W4 0.2 = 2.7 requests per second, when W1 and W2 read one page a round and W4 one page a sweep (design §7.5). It is measured (`crates/watcher/tests/budget.rs`), not enforced; the request guard is the hard bound. With 30 executions per 5 s it is met (162 requests a minute); with 100 per 5 s the scope's W4 rules (10 s overlap, every cursor) make it about 3.2 to 3.3 (owner question 1; C-WATCH part 9).
_Avoid_: rate limit, guard (that is the hard bound of 60 per 10 s)

**Watcher mismatch** (워처 불일치):
The hold `WatcherMismatch`, set when a check of the watcher (W1–W4) confirms a difference. Refuses plain orders until released.

**Venue change** (거래소 변경):
The hold `VenueChange`, set from the records of a position change the venue made or of an execution nobody could classify. Refuses plain orders until the mismatch is acknowledged and the hold released.

**Strategy liveness** (전략 연결 감시):
The node's watch over the trading connections of each strategy listed in `[liveness]`: the strategy's link is lost when its last trading connection that said `hello` closes, or when no request (ping included) comes on any of them for its liveness time. The loss is told to the core once until the strategy's next `hello` (C-LIVE).
_Avoid_: heartbeat, session timeout

**Strategy link hold** (전략 연결 보류):
The hold `StrategyLink` of one strategy, set by `strategy_link_lost`: that strategy's resting plain orders get a rule cancel and its new plain orders are refused; its reduce-only orders and other strategies' orders stay. Released by the strategy's own `hello` (`strategy_link_restored`), not by `clear-hold` (C-LIVE).
_Avoid_: strategy kill, disconnect halt

**Monitoring gate** (모니터링 게이트):
The core's rule, with `[core.policy.monitor]`, that trading needs a live watcher: the watcher's heartbeats on `monitor.sock` are journaled (`watcher_heartbeat`) and judged at record time; without a fresh one the account is unmonitored (C-LIVE part 2).
_Avoid_: watchdog, dead-man switch

**Unmonitored** (감시 안 됨):
The gate's verdict when no heartbeat came in this process, the last one is older than `heartbeat_max_age_us`, or in it W1 or W2 had no success within `check_max_age_us`. Stage 1: the hold `Unmonitored` refuses plain orders. Stage 2, after `unmonitored_cancel_us` more: resting plain orders get a rule cancel. Released by the first fresh heartbeat, not by `clear-hold`.
_Avoid_: watcher down, offline

**Watcher heartbeat** (워처 하트비트):
The watcher's line `heartbeat{watcher, checks}` on the monitor socket: per check `W1`..`W9` the time of its last success (ms on the wire, µs in the journal) or `null`. Answered `recorded` after it is durable.
_Avoid_: ping (a `ping` is not a heartbeat)

**`[WATCH P1]`** (워처 직접 알림):
The watcher's own direct alerts (C-WATCH part 10), sent by its own Telegram sender (not by the OMS, not through the observability stack): one per confirmation of a difference (at once, then at most one per check per 60 s with the rest collected; a resend after an operator's clear sends none), the lasting conditions `auth` (the watcher's key stopped) and `public_ip` under the repeat rule, and one at a start refusal before exit 2. A testnet watcher without `[telegram]` writes each as one `warn` line.
_Avoid_: watcher alert (the stack's alerts are `[MON P?]`), OMS alert (`[OMS P1]`)

**Watcher kill loop** (워처 킬 루프):
The watcher's own cancel-all confirmation loop (`watcher::kill::KillLoop`, sans-IO, C-WATCH part 11), with the watcher's key and for linear/USDT only: a cancel-all, then a count of the open orders after every cancel-all that was sent (also a failed one, unlike the OMS's kill loop), repeated every `round_ms` until a count says zero, then a watch of `watch_ms` with a count every `watch_every_ms`; only the zero of the count emitted at or after the watch's end ends it. Never two of its requests at once, at most 2 cancel-alls in any 1,000 ms, a deadline per request (per page for a count). Its notices say `워처 킬`: one start, one progress per `notice_ms` mark anchored to the start (no notice at a count change), one final, or one abort. Its count reads `/v5/order/realtime` by W1's paging rule, at most 20 pages, and counts every open order whoever placed it; its requests run as tasks of `kill::Inflight`, aborted at the loop's deadline. The test rig runs it against `http-fake` (U19b2c); the runtime runs it in a watcher kill (U19b3c).
_Avoid_: kill loop (that is the OMS's, C-KILL), watcher kill (the whole sequence: marker, halt or stop, then this loop; U19b3)

**W5** (OMS silence check, OMS 침묵 검사):
The watcher's check that the OMS still answers (`watcher::w5::W5`, sans-IO, C-WATCH part 12): with `[kill]` it is armed at the end of the start checks and probes `/api/status` every `probe_ms`; when no good answer (2xx JSON with a 16-hex `core.instance_id`) came for `silence_ms` and a probe sent since the last good answer failed, it fires once and the watcher kill starts. Time the watcher itself did not observe (a gap between two of its polls) is not silence. After a kill it waits for one good answer before it watches again. It sees a dead process or a silent HTTP task only, not a hung core behind an answering `/api/status`. It reads `clock::Steady`. Off without `[kill]` (testnet only).
_Avoid_: heartbeat (the watcher's line to the OMS, the other direction), W1–W4 (the checks against the venue)

**Watcher kill** (워처 킬):
The whole sequence the watcher runs when W5 fires (`watcher::kill::Kill`, C-WATCH part 12): K1 write the kill marker (deadline 2 s), K2 `watcher_kill` on `admin.sock`, K3 `systemctl stop` of the one OMS unit only when K2 did not latch, then the watcher kill loop with the watcher's own key; the first cancel-all comes only after K1 and K2 (and K3) ended or passed their deadlines. While the halt is owed the loop sends `watcher_kill` again every 5 s. While it runs the watcher's check engine is paused (no round, no heartbeat). Its notices say `워처 킬`. Run by the runtime from U19b3c.
_Avoid_: kill (the OMS's own latch, C-HOLD/C-KILL), watcher kill loop (only the cancel-all loop inside it)

**Venue setting** (거래소 설정):
The margin mode (isolated or cross) and venue leverage of one instrument, as the venue reports them.
_Avoid_: leverage config

**Account mode** (계좌 모드):
The venue's account-wide margin scheme, for example standard, unified account or portfolio margin.
_Avoid_: abstraction, account type

### Market data

**Stream key** (스트림 키):
The `(channel, symbol)` pair that names one topic of a venue, for example `orderbook.50` / `BTCUSDT`.
_Avoid_: feed, subscription

**Snapshot** (스냅샷):
An order book message that holds the whole book; it replaces both sides and starts a new sequence run.
_Avoid_: full update

**Delta** (변경분):
An order book message that sets levels; quantity 0 removes a level.
_Avoid_: diff, incremental update

**Sequence rule** (순번 규칙):
How a venue chains its order book messages: `PlusOne` (Bybit `u`), `PrevEquals` (Binance `pu`, OKX `prevSeqId`) or `None` (Hyperliquid).
_Avoid_: seq check

**Gap** (공백):
A delta that breaks the sequence rule; the book is wrong from there until the next snapshot.
_Avoid_: drop, missing message

**Market-data adapter** (시세 어댑터):
The per-venue code behind `md_core::adapter::MarketDataAdapter` that plans connections, routes and parses frames and chooses the resync after a gap.
_Avoid_: connector, driver

**Exponent** (지수):
`px_exp` or `qty_exp`: a price or quantity is an integer × 10^exp, with exp from the instrument's `tick_size` or `lot_size` (lake contract 1.3). Ticker mark and index prices use `ref_px_exp = px_exp - 2`, because the venue computes them finer than the tick.
_Avoid_: scale, precision

**Connection budget** (연결 예산):
The range of `conn_index` values one market's plan may use. Bybit counts 500 new connections per 5 minutes over every market of `stream.bybit.com`, so the budgets of all markets together stay ≤ 250.
_Avoid_: connection limit (that is Bybit's)

**Open file** (열린 파일):
The collector's file of one connection and one UTC hour under `crypto/l0/_open/`, one zstd level-3 stream, locked by its writer until the frame end and the last fsync.
_Avoid_: raw file, buffer file

**Market slot** (시장 슬롯):
A venue market's fixed position (Bybit: linear 0, inverse 1, spot 2); slot `k` owns `conn_id`s `[max(k << 28, 1), (k + 1) << 28)`, so the sealer reads a record's market from its `conn_id` (ADR-019).
_Avoid_: market id, shard

**Plan slot** (연결 자리):
One `ConnPlan` of a feed (`conn_index`): its topics keep their slot across reconnects and day-change switches; the collector's receive budget is counted per slot.
_Avoid_: connection (a slot has a new connection after each reconnect)

**Disk floor** (디스크 하한):
Free space of the lake below which the collector keeps only the traded topics (`traded_channels`) and writes a `disk_floor` note per dropped topic; it collects everything again above floor plus margin.
_Avoid_: quota

**Writer beat** (쓰기 박동):
The collector's writer loop marking itself alive once a second after a tick in which every file tick succeeded, also without frames: it sets `md_collector_heartbeat_ts` and sends `WATCHDOG=1`; a stuck or failed write stops it, a quiet venue does not.
_Avoid_: heartbeat of the venue, keepalive (that is the WebSocket ping)

**Direct alert** (직접 알림):
A Telegram message sent by the process itself (alert-direct), not through the observability stack, with the repeat rule: once at start, every 5 min while the condition lasts, once at clear; prefixes `[MD P1]` (md-collector), `[OMS P1]` (oms-node, C-ALERT), `[WATCH P1]` (the watcher, whose own sender rewrites the same rules without linking alert-direct, C-WATCH part 10). With a second channel configured, every direct alert also goes there.
_Avoid_: notification (the stack's alerts are `[MON P?]`)

**Second channel** (둘째 채널):
A push service besides Telegram for direct alerts (alert-direct `SecondChannel`, design §7.7): it gets a copy of every message through its own bounded outbox and thread, and the Telegram-down notice. Which service is not decided (D3); md-collector and the OMS run without one (the OMS accepts only `second_channel = "none"`; `alerts::start` takes one for tests).
_Avoid_: backup bot (it is the same bot that fails), fallback route (that is the pinned Telegram IP)

**Telegram-down notice** (텔레그램 장애 알림):
The direct alert, on the second channel only, that Telegram has failed for more than `second_after_ms` (60 s; `[alert] second_after_ms` in the OMS): counted from the first failed attempt since Telegram last answered (delivered or refused), with the repeat rule (start, every 5 min, clear when Telegram answers). Not raised while nothing is queued (no probe).
_Avoid_: Telegram outage (only failed sends are seen)

**Alert condition** (알림 조건):
One row of the C-ALERT table: a key (`hold:reducing`, `mismatch:<kind>`, `halt_strategy:<s>`, `margin_stale`, `gateway_block`, ...), a text and a grace, computed by the `oms-alert` thread from the published (durable) state only; `gateway_block` is the one read from gateway process state. Each key goes through the repeat rule of the direct alert.
_Avoid_: alert rule (that is vmalert's, U20b), hold (a hold is one source of conditions)

**Grace** (유예):
How long an alert condition must hold at every evaluation before its start is sent: 0 for latches and operator acts, `alert.grace_ms` (30 s) for states every reconnect or boot passes through. A condition that ends inside its grace sends nothing.
_Avoid_: debounce (the clear is not delayed), delay

**Hour seal** (시간별 봉인):
The sealer's move of a closed hour's open files into the day files, recorded by a begin and a done entry of the seal log; lossless and restartable at any step.
_Avoid_: compaction, flush

**Day seal** (일별 봉인):
The rename of a complete date's `.partial` files to their sealed names, with a sidecar and one manifest line each; a sealed file never changes again.
_Avoid_: finalize, close

**Session run** (세션 구간):
The records of one collector session inside one frame of a by_symbol day file; listed in the done entry and, joined, in the manifest line's `sessions`, so that `rec_no` gives a record's session.
_Avoid_: segment, batch

**Checkpoint row** (체크포인트 행):
A derived `book_msg` row (`kind = 2`) at the start of an L1 hour (or day) file: the full book of the connection followed then, with the lineage of the last record applied; written only while the symbol has book state.
_Avoid_: snapshot (that is the venue's message)

**Book state** (호가 상태):
What replay holds for a symbol between a snapshot row and the first break (a sequence gap, a parse error, the end of every connection that carries the topic, a `collector_down`, `disk_floor` or `stale` note); checkpoint rows exist only inside it.
_Avoid_: book (the in-memory structure), sync

**Followed connection** (따라가는 연결):
The connection whose messages replay applies to the book of a topic; it moves by a handover (overlapping connection) or a reconnect.
_Avoid_: active connection, primary

**Carry-in** (이어받기):
The replay of the date before, without rows, that gives the book state at 00:00:00 of the date being built.
_Avoid_: warm-up, backfill

**Seal log** (봉인 기록):
The JSON lines of one hour's seal: begin entries (sources and target offsets) and the done entry (offsets, counts, SHA-256, session runs).
_Avoid_: journal (that is the OMS's), WAL

