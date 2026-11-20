# Spec: funding, venue-made position changes, unclassified executions, record schema 2 (unit U13b)

Status: ready-for-agent

Design source: `RESEARCH/CRYPTO_TRADING_INTEGRATED_TN_20261118/10_DESIGN_DRAFT.md` §7.2, §7.5 (W2), amendments R2 (execType) and R4 (PositionAdjusted), and `pr_plan.json` unit U13 (outside this repository). U13 is split: U13a wire id form (PR #13), U13b this spec, U13c absence rule and `market_ref`. The venue-neutral margin report of the plan is not done: the only core rule that uses the Hyperliquid formula judges isolated positions, and the Bybit account runs cross margin, so the change would edit many tests for no Bybit function (proposal to the owner, 2026-11-20, DM event bd8d7b04; the owner can override).

## Problem Statement

On Bybit, executions that are not fills of our orders change the account: funding payments (cash only), auto-deleveraging (`AdlTrade`), takeover of a liquidated position (`BustTrade`), `Delivery` and `Settle`. Bybit sends them in the same execution stream and `/v5/execution/list` as our fills, told apart by `execType` (14 documented values). The core today knows only `Fill`: a funding row would have to be dropped or booked as a trade, and a position change made by the venue would show only as a position mismatch on the next sweep (`PositionOutOfRange`), with no record of what happened. An `execType` value the gateway does not know must stop trading, not pass silently.

## Solution

Three new venue inputs and their events, all journaled:

- `VenueInput::Funding(FundingIn { key, instrument, amount, rate, venue_ts })`: a cash flow. `amount` positive is paid by the account (the sign of `FillIn.fee`). It changes no position, no order and no reservation and is not a fill. Event `FundingBooked`. De-duplicated by `key` in the same set as fills (`seen_fills`, fill floor), because a REST resync re-delivers it.
- `VenueInput::PositionChange(VenuePositionChangeIn { key, kind, instrument, qty, px, venue_ts })` with `kind` one of `adl`, `takeover`, `delivery`, `settle`: a signed quantity the venue moved in or out of the account. Event `VenuePositionChanged` adds `qty` to the account position and to the unattributed ledger (like an external fill), records the key, and marks the instrument "waiting for the venue's position". The core raises a mismatch of the new kind `VenuePositionChange` (subject `instrument:<i>`), which holds plain orders until the operator acknowledges it (existing `ack-mismatch`).
- `VenueInput::UnclassifiedExecution { key, instrument, exec_type, detail, venue_ts }`: an execution the gateway cannot classify. Event `UnclassifiedExecutionNoted` (key recorded) and a mismatch of the new kind `UnclassifiedExecution` (same hold).

Position of record after a venue change: the next `Positions` answer of the current session whose `queried_at` is after the change sets the account position of each waiting instrument to the venue's quantity. If they differ, the core journals `PositionAdjusted { instrument, ledger: unattributed, delta, note, reason: snapshot_after_venue_change }`; if they are equal it journals `VenueChangeSettled { instrument }`. Either event ends the wait. A reader of the journal alone therefore gets the position as the sum of booked fills, external fills, `VenuePositionChanged` and `PositionAdjusted` quantities.

Record schema 2: `RECORD_SCHEMA = 2`; recovery and `verify` read schemas 1 and 2. Every schema-1 record parses unchanged, because the change only adds enum variants and one defaulted field. `STATE_SCHEMA` stays 1: the new state fields (`venue_change_wait`, two statistics) are not written while empty or zero, so an older snapshot keeps its bytes and its digest (decided while implementing; the first draft said 2).

## User Stories

1. As the owner, I want a funding payment journaled as cash, not as a trade, so that the position and the fills stay exact.
2. As the owner, I want an ADL or a takeover journaled with its quantity, and trading of plain orders held until I look, so that a position the OMS did not make cannot grow silently.
3. As the owner, I want an execution type nobody classified to stop plain orders, so that a new Bybit value cannot pass as a fill.
4. As the developer of the watcher (U19a), I want the journal alone to give the position, so that W2 compares the venue with the journal sum.
5. As the operator, I want journals written before this change to replay to the same state.

## Implementation Decisions

### D1. Foreign trades stay external fills

R2 maps a `Trade` row with an unknown `orderLinkId` to `VenuePositionChange(foreign_order)`. The core already books such a row as `ExternalFillBooked` (account and unattributed ledger, mismatch `ExternalFill`, the same hold). A second path for the same fact would split one concept in two. So the Bybit gateway sends it as a `Fill` with `order_id: None`, as Hyperliquid does, and there is no `foreign_order` kind. W2 counts external fills as fills.

### D2. The hold is the mismatch hold

R5 names a hold `VenueChange` that refuses plain orders only and is cleared by `clear_hold{reason: venue_change}`. The existing mismatch hold already refuses plain orders only (with a margin policy; the Bybit configuration has one), survives replay, shows in the status as an unacknowledged mismatch (U14 turns those into `[OMS P1]`) and is cleared by `ack-mismatch` with a note. U13b uses it with two new kinds; `clear_hold` and the other R5 holds come with the units that need them (U18b, U19b).

### D3. `PositionAdjusted` gets a reason

The event exists for the operator's `adjust-position`. It gains `reason: AdjustReason { operator, snapshot_after_venue_change }`, default `operator`, not written when it is `operator`, so an operator adjustment is byte-identical to schema 1. `boot_reconcile` (R4) has no caller yet and is not added.

### D4. Which snapshot counts

The core keeps, per waiting instrument, the record time of its last venue change. A `Positions` answer counts only if it belongs to the current link session, `queried_at` is later than that time, (as today) no fill was booked at or after `queried_at`, and the instrument has no open quantity (`buy_open`, `sell_open`) and no parked fill: otherwise a fill of ours still on its way would be counted twice (review 2026-11-20). While it waits, the instrument is range-checked as today; once set, the range check passes because the position equals the venue's.

### D5. Keys

Funding and position changes use the gateway's fill key (C-GW G-8) and the same de-duplication as fills: a key already seen, or a venue time below the fill floor, is ignored. Unclassified executions keep their keys in a separate set (`seen_unclassified`), so that the same key delivered later as a fill (a gateway that learned the kind) is still booked (review 2026-11-20). All keys are purged with fills.

### D6. Quantities

`VenuePositionChangeIn.qty` is signed (`SQty`), positive adds to a long. Zero is ignored. A change that would overflow the booked position is not booked; the core raises the `VenuePositionChange` mismatch instead (no `CoreFault`, which would stop the node on a venue input).

## Testing Decisions

Core tests (`crates/oms-core/tests/venue_changes.rs`, new file, rustfmt-clean):

- a funding input changes no position, no open order, no reservation, creates no fill and is journaled once (second delivery ignored);
- a position change adds its signed quantity to the account and the unattributed ledger, raises one `VenuePositionChange` mismatch, refuses a plain order and accepts a reduce-only one, and the state after replay of the records equals the live state;
- an unclassified execution raises the `UnclassifiedExecution` mismatch and holds plain orders; replay keeps the hold;
- after a change whose quantity differs from the next positions answer, exactly one `PositionAdjusted` with reason `snapshot_after_venue_change` makes the account equal the venue; replay gives the same position; journal sum of quantities equals the venue;
- a positions answer equal to the sum journals `VenueChangeSettled` and no `PositionAdjusted`;
- a positions answer queried before the change, or of an old session, does not end the wait;
- an operator `PositionAdjusted` serialises without `reason` (schema-1 bytes).
- (review 2026-11-20) the wait does not end while an order of ours on the instrument is open, so a fill still on its way is not counted twice;
- (review 2026-11-20) an unclassified key does not block a later fill with the same key.

Node test: a journal written by this build with record schema 1 (`Options.record_schema = 1`) from inputs that produce only schema-1 events recovers with this build to the same state digest as the live core.

Registry: the five new events are listed in `registry/contracts.toml` (C-FILL or C-MISMATCH) with a claim naming these tests.

## Out of Scope

Bybit gateway and its `execType` table (U16), `clear_hold` and the R5 hold list (U18b, U19b), the watcher (U19a), alerts (U14), the venue-neutral margin report (proposal above).
