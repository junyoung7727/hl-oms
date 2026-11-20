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
A Telegram message sent by the process itself (alert-direct), not through the observability stack, with the repeat rule: once at start, every 5 min while the condition lasts, once at clear; prefixes `[MD P1]`, later `[OMS P1]`, `[WATCH P1]`.
_Avoid_: notification (the stack's alerts are `[MON P?]`)

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

