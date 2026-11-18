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

