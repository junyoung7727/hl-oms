# Spec: OMS kill switch, operator-released holds, strategy liveness and the monitoring gate (unit U18b, PRs U18b1–U18b4)

Status: ready-for-agent

Design source: `RESEARCH/CRYPTO_TRADING_INTEGRATED_TN_20261118/10_DESIGN_DRAFT.md` §7.5 (watcher, monitoring gate), §7.6 (kill switch), R2–R5 (kill-loop P1 rule, kill marker, hold reasons, liveness rule); TN chapter text `note/05_trading.html` §7.5–§7.6 with table 7-7 (eight holds, reduce-only column, release commands) and table 7-8 (confirmation loop C1–C5); `pr_plan.json` unit U18b (scope and done criteria, binding). Owner decisions: grill Q22 (block new orders on a reconciliation mismatch) and Q23 (own kill switch in the OMS and the watcher, 10 s trigger, 8 + 2 request split), `01_GRILL_DECISIONS.md` rows Q22–Q23, message f52aa410. Bybit facts: `RESEARCH/BYBIT_V5_GATEWAY_FACTS_20261120.md` Q10–Q11 and `RESEARCH/BYBIT_V5_PRIVATE_API_FACTS_20261118.md` §1.4, §4. Handoff from U18a: `.scratch/oms-direct-alerts/spec.md` "Handoff to U18b".

Numbers marked 판단(모델 아님) are judgements without a model or a measurement behind them. The design's own numbers (10 s, 1 s, 5 s, 30 s, 60 s, 2/s, ≤ 12 P1) are also judgements there ("testnet에서 조정"); they are cited as design values. Every validation bound in the config tables below (the ranges, not the defaults) is 판단(모델 아님); each default names its source or is marked.

## Problem Statement

Bybit offers no venue-side dead-man for this account (DCP is for institutional clients only, private facts §4). If the private stream is lost, the OMS no longer sees fills or cancels, its resting orders stay at the venue, and today nothing cancels them: the link goes down, new orders are refused, and that is all. A watcher (U19) that finds a mismatch, or finds the OMS silent, has no input that blocks orders without also blocking the reduce-only orders that close a position (only `halt` exists). A strategy process that dies leaves its quotes resting; the OMS drops the replies and cancels nothing (`docs/contracts/api.md:109`). And nothing stops trading when the watcher itself is gone. U18b adds: the kill latch with a cancel-all confirmation loop, four operator-released or self-clearing hold reasons of table 7-7, the kill marker read at boot, strategy liveness, and the monitoring gate fed by the watcher's heartbeat.

## Facts from the existing code

Core (`crates/oms-core`):
- Holds are derived from state on every call, never stored: `Core::holds` (`core.rs:177-224`). `OperatorStrategy` appears only for `holds(Some(s))` (`core.rs:196-200`). The C-MARGIN holds exist only with a margin policy (`core.rs:204-221`).
- `HoldReason` (`model.rs:293-325`) and its table `refuses_only_plain_orders` (`model.rs:332-339`), a `match` without a wildcard arm. The new-order check skips a plain-only hold for a reduce-only order only when a margin policy is set (`decide.rs:222-236`); without one every hold refuses every order.
- Operator commands: `OperatorCmd` (`input.rs:76-98`), handled in `Tx::operator` (`decide.rs:469-588`). `halt`/`resume` change only `halt_account`/`halt_strategy` (`decide.rs:471-498`, applied `core.rs:758-771`); `resume` refuses a scope that is not operator-halted (`decide.rs:496`). `clear_reduce` is the model for `clear_hold`: a refusal function returning a stable code (`decide.rs:594-630`, codes `model.rs:515-534`), then one event (`ReducingCleared`, `decide.rs:647-657`, `event.rs:236-243`); refusals reply `Err(text)` and write nothing.
- Rule cancels: `enter_reducing` latches once and cancels open plain orders in the same input (`decide.rs:893-903`); `rule_cancel_plain_orders` asks once per order with `Actor::Rule(rule)` (`decide.rs:909-926`); `stale_cancel` repeats on every `Tick` and asks only orders without a request (`decide.rs:1781-1804`, called from `tick`, `decide.rs:1806-1841`). `Actor::Rule(RuleId)` (`model.rs:112-118`), `RuleId` (`model.rs:469-481`).
- Venue changes today raise only a `Mismatch` (cleared by `ack_mismatch`): booked change `decide.rs:1397-1425` (events `venue_position_changed`, `mismatch_raised`), overflow path raises only (`decide.rs:1409-1415`), unclassified execution `decide.rs:1428-1453`. The existing test asserts the exact event list `["venue_position_changed", "mismatch_raised"]` (`tests/venue_changes.rs:113-116`) and `["unclassified_execution_noted", "mismatch_raised"]` (`:160-163`).
- `Input` (`input.rs:10-25`): `Boot { binary }` is the first input of every process (`decide.rs:91-126`); commands come only from live processing (`event.rs:313-315`), derived per event by `command_for` (`core.rs:272`). `VenueCommand` has `SendOrder` and `SendCancel` only (`event.rs:318-338`). An input that emits no event writes no record (`pipeline.rs:213-223`).
- `RateBudget` is a pure sliding window with time as input (`rate.rs:38-121`); `RateBudget::bybit()` = 8 orders, 10 with cancels, 1 s (`rate.rs:66-68`). State fields that are empty are not serialised so old state keeps its bytes (`core.rs:70-85`, `Policy::margin` `config.rs:98-103`). New optional fields and new event kinds did not raise `RECORD_SCHEMA` (stays 2: `oms-node/tests/record_schema.rs:173-177`, `:265`).

Node (`crates/oms-node`):
- Commit gate: `writer.sync()` then step 1 (read model) then step 2 releases replies, gateway commands and subscriber events (`pipeline.rs:165-172`, `:182-236`, `:238-274`). A command is therefore sent only after its record is durable.
- Boot order: `Boot` and `Config` are pushed and awaited before the gateway starts (`node.rs:118-145`); gateway spawn per venue (`node.rs:161-224`); tick task every `runtime.tick_ms` (default 250, `config.rs:235`; `node.rs:232-243`); sockets bound after that (`node.rs:290-305`).
- Sockets: trading `oms.sock` and admin `admin.sock` only (`config.rs:62-64`); there is no monitor socket. Admin needs the node's own uid (`server.rs:137-142`); operator ops need the admin socket (`server.rs:365-367`). `ping` is allowed before `hello` (`server.rs:362`, `:373`); `hello` names a strategy on the trading socket and is answered at once without a journal record (`server.rs:374-399`). A closed connection ends without any core input (`server.rs:300-309`). `submit` answers after the record is durable (`server.rs:314-353`). `clear_reduce` op `server.rs:556-559`, omsctl `clear-reduce` `oms-api/src/ctl.rs:125-127`, `Op::ClearReduce` `oms-api/src/lib.rs:111-113`.
- Gateway trait: `send_order`, `send_cancel`, `query`, `spawn_tasks` (`venue/mod.rs:76-99`); `start` dispatches each command on its own task under a semaphore (`venue/mod.rs:106-125`).
- Status: built from the published state and counters (`status.rs:28-120`); the Bybit block view is process state (`gateway.rs:854`, `status.rs:82-85`).
- Alerts (U18a): `hold_condition` is a `match` over `HoldReason` without a wildcard (`alerts.rs:86-214`); `conditions` adds `halt_strategy:<s>` and mismatch keys (`alerts.rs:220-305`); `Alerts` has no public one-off message method today (`alerts.rs:494-640`); it is started before the gateway (`node.rs:89-97`).

Bybit gateway (`crates/oms-node/src/venue/bybit`):
- Liveness (U16): a ping every `ping_interval_ms`; the stream is down when the socket closes, a read fails, or no pong arrives within the same interval of a ping (`gateway.rs:543-575`); time spent waiting on the core's queue moves the pong deadline (`gateway.rs:541-545`, `:597-599`). Link up after auth and subscribe (`gateway.rs:526-535`), link down sent at the end (`gateway.rs:604-610`). Every reconnect is a new session and the core re-reads before trading (`gateway.rs:6-10`). `ping_interval_ms` 1,000..=20,000, default 5,000 (`config.rs:364-366`, `:576-577`). Nothing measures "down for 10 s" today.
- Blocks: `Health::block` (auth, ban, boot, IP) keeps every request home; the clock slot only creates (`checks.rs:67-101`). `allowed()` / `create_allowed()` (`gateway.rs:660-680`); creates ask `create_allowed` after the budget wait (`gateway.rs:724`). The boot checks verify the sub-account, modes and the USDT settle coin before the block lifts (`gateway.rs:333-399`, `checks.rs:173-176`).
- `cancel_all(rest)` exists, posts `{"category":"linear","settleCoin":"USDT"}`, fails unless `success = "1"`, flags `maybe_more` at the 500 cap; "The kill switch (U18b) calls it; nothing else does" (`mod.rs:60-102`). The venue cancels every kind of order when `orderFilter` is not passed, at most 500 per call, picked at random, asynchronously; cancel-all has its own rate-limit row, linear 10/s (gateway facts Q11.2–Q11.6).
- Open orders: `/v5/order/realtime` all pages, 50 per page, at most 20 pages, open = `New`, `PartiallyFilled`, `Untriggered` (`query.rs:71`, `:91`, `:249-277`, `:354-392`). Positions: `/v5/position/list` rows (`query.rs:396-401`); the row has `markPrice`, `unrealisedPnl`, `liqPrice` (`BYBIT_V5_PRIVATE_API_FACTS_20261118.md:91`; cross `liqPrice` is "an **estimated** price", `:249`). Today only `size`, `side`, `positionValue`, `positionIM`, `leverage` are read (`query.rs:118-131`, `:540-551`).
- Order budget: one `RateBudget::bybit()` with a monotonic clock in `Orders` (`orders.rs:139-200`). Send-time rule: a create is processed by `ts + recv_window + 1 s` or not at all (`orders.rs:64-72`), `recv_window_ms` default 5,000 (`config.rs:372-374`).
- Sim and Hyperliquid have no cancel-all and no block view (`node.rs:163-190`).

## Split

| PR | Scope | Size (판단(모델 아님)) | Depends on |
|---|---|---|---|
| U18b1 | Core and node, venue-neutral: hold reasons `KillLatched`, `WatcherMismatch`, `VenueChange`; events `hold_set`, `hold_cleared`, `kill_loop_ended`; `clear_hold` with refusals; admin ops `watcher_mismatch`, `watcher_kill`, `clear_hold`; omsctl `clear-hold`; kill marker at boot and its removal; `VenueCommand::Kill` / `KillStop` with the sim and Hyperliquid answer `Unsupported`; alert arms; status; ADR-030, contract C-HOLD, registry, docs | ≈ 1,100 lines of code + tests | U18a (merged) |
| U18b2 | Bybit: the stream-outage trigger, the kill loop (pure state machine + driver with injected clock), emergency budget, position summary in every loop P1, creates refused while the loop runs, `status.gateway.kill_loop`, config; contract C-KILL; node test | ≈ 1,300 | U18b1 |
| U18b3 | Strategy liveness: node liveness table, `Input::Session`, `strategy_link_lost` / `strategy_link_restored`, per-strategy rule cancel, hello waits for the restore, `[liveness]` config, alert condition, status; contract C-LIVE (part 1) | ≈ 700 | U18b1 |
| U18b4 | Monitoring gate: `monitor.sock`, `heartbeat` op, `Input::Watcher(Heartbeat)`, `[core.policy.monitor]`, `watcher_heartbeat` / `monitor_changed`, the two stages, boot rule, alert arm, status; contract C-LIVE (part 2) | ≈ 900 | U18b1 |

Order: U18b1, U18b2, U18b3, U18b4 (U18b3 and U18b4 touch disjoint files except `holds()`, `alerts.rs` and `status.rs`; whichever merges second rebases). Each PR regenerates `docs/generated/`, lists its events in `registry/contracts.toml` and adds the claims that name its tests. Each PR adds its own `HoldReason` variants and their alert arms (the compile-enforced `match` makes a missing arm a build error).

## U18b1 decisions: holds, release, marker

### Hold reasons (table 7-7)

New variants, appended after `AccountMode` (serde snake_case); `holds()` pushes them after the existing ones so existing exact-order assertions stay true.

| Reason | Set by (event) | Refuses | Cleared by | Replay |
|---|---|---|---|---|
| `KillLatched` | `hold_set{hold: kill}` from: `VenueInput::StreamLost` (U18b2), admin `watcher_kill`, the kill marker at boot | every order, reduce-only included (`refuses_only_plain_orders = false`): the loop must confirm zero, and a new order would keep it from ending (TN table 7-7 text) | `clear_hold{reason: kill}` only | `core.kill` from `hold_set` / `hold_cleared`; confirmation from `kill_loop_ended`, reset by `booted` |
| `WatcherMismatch` | `hold_set{hold: watcher_mismatch}` from admin `watcher_mismatch` | plain orders only (`true`) | `clear_hold{reason: watcher_mismatch}` | `core.watcher_mismatch` |
| `VenueChange` | derived in `apply` from the existing events `venue_position_changed`, `unclassified_execution_noted`, and `mismatch_raised` with kind `venue_position_change` (overflow path) — no new event | plain orders only (`true`) | `clear_hold{reason: venue_change}` | `core.venue_change` |
| `StrategyLink` (U18b3) | `strategy_link_lost` | that strategy's plain orders only | `strategy_link_restored` (hello) | `core.strategy_link` |
| `Unmonitored` (U18b4) | `monitor_changed{monitored: false}`, and `booted` / `config_activated` with a gate | plain orders only | `monitor_changed{monitored: true}` | `core.monitor` |

- State: `core.kill: Option<KillLatch { since, source: KillSource (stream | watcher | marker), note, confirmed_at: Option<Ts> }>`, `core.watcher_mismatch: Option<HoldNote { since, source, note }>`, `core.venue_change: Option<HoldNote>`; each `#[serde(default, skip_serializing_if = "Option::is_none")]` so a run without them keeps its bytes and digest. Each latches once: a second trigger while held emits nothing and the first note stays (as `enter_reducing`, `decide.rs:895-897`).
- `VenueChange` without a new event: an added `hold_set` in the same input would change the event lists that `tests/venue_changes.rs:113-116` and `:160-163` assert. Consequence: a journal written before U18b that contains one of the three events boots with `VenueChange` latched. This is the safe side, and no real-venue journal exists (README: no order ever sent to a real venue). The note is the event's detail.
- The Mismatch entries stay as today (`ack_mismatch` per entry). Releasing a venue change needs both `ack_mismatch` and `clear_hold venue_change` (owner question 3).
- Operator `halt`/`resume` are unchanged (api.md:25, :87-88): `resume` clears only operator halts; under a kill and an operator halt, `resume` leaves `KillLatched`.

### Events

| Event | Fields | Contract |
|---|---|---|
| `hold_set` | `hold` (`kill` \| `watcher_mismatch`), `source` (`stream` \| `watcher` \| `marker`, or the check name `W1`..`W4` for a mismatch), `note` | C-HOLD |
| `hold_cleared` | `hold` (`kill` \| `watcher_mismatch` \| `venue_change`), `note` | C-HOLD |
| `kill_loop_ended` | `outcome` (`confirmed` \| `unsupported`), `rounds`, `cancel_all_calls`, `detail` | C-KILL (U18b1 adds the kind for the `unsupported` answer) |

All three get rows in the tail contract's field table (`docs/contracts/tail.md` §이벤트 필드 표) and instances in `tests/event_fields.rs::every_event`; none changes the open-order, fill or position sets (no row in §이벤트가 바꾸는 상태; U19a ignores them).

### Admin inputs (sent by the watcher, U19a/U19b)

`Input::Watcher(WatcherInput)`, new input kind, recorded like any input:
- `watcher_mismatch {check: "W1".."W4", detail}` on admin.sock → `hold_set{watcher_mismatch, source: check, note: detail}` if not held. Reply `"held"` or `"already held"`.
- `watcher_kill {detail}` on admin.sock → `hold_set{kill, source: watcher, note: detail}` if not held. Reply `"latched"` or `"already latched"`. Answered after durable, so the watcher (U19b) learns that the OMS will send nothing more before it starts its own loop.
- `check` outside `W1`..`W4` or an empty `detail` → `bad_request`, nothing recorded.

### `clear_hold {reason, note}` (admin), omsctl `clear-hold --reason kill|watcher_mismatch|venue_change --note TEXT`

`OperatorCmd::ClearHold { reason: HoldKind, note }`, shaped like `clear_reduce`: a refusal function returning a stable code, else one `hold_cleared` event. Codes (reply text starts with the code, `bad_request`):

| Code | When |
|---|---|
| `not_held` | that hold is not set |
| `note_required` | note empty or whitespace |
| `kill_unconfirmed` | reason `kill` and `core.kill.confirmed_at` is `None`: no `kill_loop_ended` since the latch or since this boot. Text adds "the kill loop has not confirmed zero open orders in this process" |
| `venue_change_pending` | reason `venue_change` and `core.venue_change_wait` is not empty: the venue's position after the change has not been read yet (U13b) |

There is no force option, as for `clear_reduce` (`decide.rs:604-606`) (owner question 2). `booted` sets `kill.confirmed_at = None`: every process confirms zero again before a kill can be cleared.

### Kill commands

- `VenueCommand::Kill { since, source, note }`: `command_for(hold_set{kill})`, and `command_for(booted)` when `core.kill` is set before the boot (a latch replayed from the journal). Released after durable (step 2): the latch is on disk before the first cancel-all leaves.
- `VenueCommand::KillStop`: `command_for(hold_cleared{kill})`. The gateway ends its loop.
- `VenueGateway` gets `fn kill(&self, cmd)`; `start` dispatches both commands (`venue/mod.rs:118-121`). It must return at once (the loop runs on its own task, not under a command permit).
- Sim and Hyperliquid: `kill` sends `VenueInput::KillLoopEnded { outcome: Unsupported, detail: "this gateway has no cancel-all; cancel with omsctl cancel-all" }` and logs `warn`. Core: `kill_loop_ended{unsupported}` sets `confirmed_at`, so `clear_hold kill` works there. Orders resting on those venues are left to the operator (Out of Scope). In U18b1 the Bybit gateway answers the same way; U18b2 replaces that with the loop, so no Bybit build with a real loop ever confirms without one.
- `VenueInput::KillLoopEnded` while not latched, or already confirmed: no event.

### Kill marker

- Config: `[kill] marker = "/abs/path/kill.marker"` (node section). Absolute; its resolved path is not inside `data_dir` (the `resolved()` helper, `config.rs:390-422`); the parent directory exists at load. Absent: no marker check (one `warn` at start). Mainnet without it is refused (owner question 1).
- Boot: before `Input::Boot`, the node does `symlink_metadata(marker)`:
  - `NotFound` → no marker.
  - exists (any file type, symlink included) → `Boot { binary, kill_marker: Some(text) }`, `text` = the first 512 bytes, lossy UTF-8, control characters replaced, prefixed `marker:`. 512 판단(모델 아님).
  - any other error (permission, I/O) → latched, `text = "marker unreadable: <io kind>"` (fail-safe; a path the OMS cannot look at cannot prove that the watcher did not kill).
  `Boot.kill_marker` is `#[serde(default, skip_serializing_if = "Option::is_none")]` (no record-schema rise). In `boot()`, after `booted`: if a marker is given and `core.kill` is `None` → `hold_set{kill, source: marker, note: text}` in the same record. Already latched → nothing more.
- Removal only through `clear_hold kill`: the admin handler waits for the durable `Ok`, then `unlink(marker)` (`NotFound` counts as removed). Reply `"cleared; kill marker removed"` or `"cleared; kill marker NOT removed (<io kind>): the next start latches again"`. Durable first, unlink second: a crash between the two re-latches at the next boot (safe side), the opposite order could lose the latch. A refused clear never touches the file.
- The marker is read only at boot (Out of Scope: polling while running; it would race the removal).

### Alerts (U18a pick-up)

`hold_condition` arms, grace 0 (the handoff's default):
- `hold:kill` — `킬 래치({source}, {since KST}). 모든 주문 거절(감소 전용 포함). 포지션은 닫지 않음. 운영자만 해제: omsctl clear-hold --reason kill. 메모: {note}` plus `확인: 킬 루프가 0건을 확인함 {KST}` or `확인: 킬 루프가 아직 0건을 확인하지 않음`.
- `hold:watcher_mismatch` — `워처가 불일치를 확정함({check}, {since}). 일반 주문 거절, 감소 전용은 받음. 운영자 해제: clear-hold --reason watcher_mismatch. 내용: {note}`.
- `hold:venue_change` — `거래소가 포지션을 바꿈 또는 분류표에 없는 체결({since}). 일반 주문 거절, 감소 전용은 받음. 해제: ack + clear-hold --reason venue_change. 내용: {note}`.
- `Alerts::notice(&self, text)` (new, pub): one `once` with the prefix, for the kill loop (U18b2). Without `[alert]` the gateway gets no notifier and logs each notice at `warn`.

### Status

`core.kill` (since, source, note, confirmed_at), `core.watcher_mismatch`, `core.venue_change` (all null when not held); `kill_marker: {configured, path, present_at_boot}` (node info, process state). Holds lists show the new reasons.

## U18b2 decisions: the Bybit kill switch

### Trigger: the stream outage

- The gateway keeps `last_proof: Option<tokio Instant>`, set at link up (subscribe confirmed) and at every pong, and moved on by the time spent waiting on the core's queue, exactly as the pong deadline is (`gateway.rs:597-599`), so a slow core never reads as an outage. It is also set once when the boot checks first pass (`booted = true`, `gateway.rs:378-383`), which arms the trigger before the first link up.
- Not armed before the boot checks pass: the account is not verified as the configured sub-account until then, and a cancel-all must never go to an unverified account (`checks.rs:20-27`).
- A watchdog task (`health_poll`, 250 ms) fires once per outage when `now − last_proof > kill_after` and the link is not up: it sends `VenueInput::StreamLost { since: <wall µs of last proof>, detail }`, `detail` = the stream's last end reason (`no pong within …`, `closed by the venue`, a block). A new proof re-arms it. While the link is up the stream loop itself ends at a missing pong (≤ 2 × `ping` after the last pong), so the watchdog decides only for a down link.
- Effect: socket closed at `t_c` after the last pong `t_p` → kill at `t_p + kill_after` unless a reconnect is subscribed before; half-open → the stream goes down at ≈ `t_p + 2 × ping` and the kill follows at `t_p + kill_after`; a quiet stream that answers pings never fires. With the defaults (ping 5 s, kill_after 10 s) this is the plan's "socket closed or no pong for 10 s".
- Key rejected, IP ban, IP mismatch while up: the link goes down, the kill fires 10 s later, and the loop reports that it cannot send (below). That is wanted: a person must know.
- Core: `StreamLost` → `hold_set{kill, source: stream, note: "private stream down since {since}: {detail}"}` unless latched → `Kill` command after durable.

### The kill loop

`venue/bybit/kill.rs`: a pure state machine `KillLoop` (no I/O, time as input, like `RateBudget`) and a driver task that performs its actions. The driver owns an injected `KillClock` (`now_ms()`, `async sleep_until(ms)`; production = tokio time; tests = a manual clock whose `sleep_until` parks until the test advances it and exposes `parked()`).

Machine: `start(now)`, `cancel_done(now, Ok(CancelAll) | Err(why))`, `count_done(now, Ok(n) | Err(why))`, `wake(now)`, `stop()` → actions `CancelAll`, `Count`, `Notice(kind)`, `SleepUntil(t)`, `Ended(report)`.

| State | Entered | Does | Next |
|---|---|---|---|
| C1 (core) | `hold_set{kill}` durable | the latch refuses every order; the gateway marks the loop active, so `create_allowed()` refuses with `kill switch active` (a create released before the latch stays home: `NotSent`) | C2 |
| C2 cancel | round `r` starts at `t_r` | if `allowed()` fails (block, pause): no request, the round fails with the block text. Else take a slot of the emergency budget (wait for it), `POST /v5/order/cancel-all` (`mod.rs:79`) | Ok → C3; Err → C4 |
| C3 count | after C2 Ok | `/v5/order/realtime` kill scope, all pages; `n` = open rows (any orderLinkId: foreign orders count). A query is never sent during the pause (`quiet()`) | `n = 0` → C5; `n > 0` → C4; Err → C4 (a failed count is never zero) |
| C4 repeat | `n > 0` or a failure | sleep until `t_r + round_ms` | C2 (round `r + 1`) |
| C5 watch | first `n = 0` at `z` | every `watch_every_ms` C3 again | `n > 0` → C2 at once; count Err → C4; `now ≥ z + watch_ms` with every check zero → End |
| End | — | final notice; `VenueInput::KillLoopEnded{confirmed, rounds, cancel_all_calls, detail}` | idle (latch stays) |
| Stopped | `KillStop` | ends at the next await; no notice, no input | idle |

- One cancel-all per round and `round_ms ≥ 1,000` keep the OMS at ≤ 1 cancel-all per second. The emergency budget `RateBudget::new(2, 2, 1 s)` with `RateClass::Cancel`, on the loop's clock, is an outer guard: the loop never has more than 2 cancel-alls counted in any 1 s (a slot counts until 1 s after its answer, `rate.rs:110-115`). It is separate from the order budget (`orders.rs:166`) because cancel-all is its own venue rate-limit row (facts Q11.6); the watcher's own loop (U19b) adds ≤ 1/s on the same UID, ≤ 2/s together (TN table 7-8 C4).
- The count is read right after the cancel-all answer (asynchronous cancels show up in the next round, 1 s later). 판단(모델 아님): no extra wait.
- Why 30 s of watch suffices: a create that left before the latch is processed by `ts + recv_window + 1 s` (`orders.rs:64-72`) = 6 s with the default, well inside 30 s; creates after the latch stay home.
- More than 1,000 open orders (20 pages × 50) is a count error (`query.rs:276`) and the rounds go on; each cancel-all removes up to 500, so the count converges.
- A second `Kill` while a loop runs is logged and ignored; `KillStop` while idle is ignored.
- The process stopping mid-loop ends it; the latch is in the journal, the next boot's `Kill` command runs a new loop (after the boot checks pass; until then every round fails with `unchecked`).

### P1 notices (C4 rule)

- Kinds: **start** (at `start`); **count change** (a known count `n > 0` that differs from the last count shown); **repeat** (when `notice_ms` passed since the last notice of any kind); **final** (at End: `n = 0` confirmed and kept for `watch_ms`). An unknown count (failed C2 or C3) is not a change. The transition to 0 has no notice of its own: the final notice is the "zero confirmed" message, `watch_ms` later (one message, so that 1 + 10 + 1 = 12 holds in 10 min, TN table 7-8 C4).
- Bound: in any window of length T the loop sends ≤ 2 + count changes + ⌊T / notice_ms⌋; with a 10-minute query failure (no known counts) that is ≤ 1 + 10 + 1 = 12.
- Every notice contains the tag `킬 루프`, the round, the remaining count (`모름: <reason>` when unknown), the cancel-all calls so far, and a position block read just before it from `/v5/position/list` (query path, not the order budget; ≤ 12 reads in 10 min):
  ```
  포지션(거래소 /v5/position/list, {KST}):
  - BTCUSDT 매수 0.010, 미실현손익 -1.23, 마크 70000.0, 마크에서 강제청산가까지 12.34 %
  - ETHUSDT 매도 0.5, 미실현손익 2.00, 마크 3000.0, 마크에서 강제청산가까지 모름(거래소가 청산가를 주지 않음)
  ```
  Rows with size 0 are left out; none → `포지션 없음`; the query fails → `포지션 조회 실패({kind}). 수량·손익 모름`. Distance = |mark − liq| / mark × 100, rounded down to 0.01 %; `liqPrice` empty, zero or unreadable, or mark ≤ 0 → `모름`. Cross margin's liquidation price is the venue's estimate (`BYBIT_V5_PRIVATE_API_FACTS_20261118.md:249`); the text says `추정`. The kill cancels orders only and leaves positions open until a person acts (TN §7.6).
- Texts: start `킬 루프 시작({source}: {note}). 모든 주문 거절, 미체결 0건 확인까지 1초마다 일괄 취소. 포지션은 닫지 않음`; progress `킬 루프 {r}회차: 남은 미체결 {n}건, 일괄 취소 {k}회. {last_error}`; final `킬 루프 정리 완료: 미체결 0건 확인 뒤 30초 동안 늦은 접수 없음({r}회차, 일괄 취소 {k}회). 해제는 운영자만: omsctl clear-hold --reason kill --note …`. Sent with `Alerts::notice`; they are after durable because the loop starts from a released command.

### Config (`[venue]`, Bybit)

| Field | Default | Validation |
|---|---|---|
| `kill_after_ms` | 10,000 (design §7.6) | `2 × ping_interval_ms ..= 60,000` (a kill before the liveness rule can say down is refused) |
| `kill_round_ms` | 1,000 (table 7-8 C4) | 1,000..=10,000 (≥ 1 s keeps the OMS at ≤ 1 cancel-all/s) |
| `kill_watch_ms` | 30,000 (C5) | 10,000..=300,000 |
| `kill_watch_every_ms` | 5,000 (C5) | 1,000..=`kill_watch_ms` |
| `kill_notice_ms` | 60,000 (C4) | 60,000..=600,000 (≥ 60 s keeps the 12-in-10-min bound) |

### Status

`gateway.kill_loop` (process state, like the block view): `phase` (`idle` \| `rounds` \| `watch` \| `done` \| `stopped`), `round`, `remaining` (number or null), `cancel_all_calls`, `last_error`, `started_at`, `zero_since`, `notices`; `gateway.kill_armed` and `gateway.last_proof_age_ms`; `source: "gateway process state"`.

## U18b3 decisions: strategy liveness (StrategyLink)

- Keys: the strategy id. Only trading-socket connections count; admin connections never do.
- Node: `Liveness` table in `ApiCtx`, per listed strategy: open trading connections that said `hello` for it, `last_request` (monotonic), `lost_reported`. Every parsed request line on such a connection after `hello` updates `last_request` (`ping` included; a line that is not a valid request does not). A timer (250 ms, 판단(모델 아님)) checks `now − last_request > liveness_ms`.
- Lost (`Input::Session(Lost { strategy, why })`, Lane::Control), submitted once until the next hello: when the strategy's last connection closes (`why: "last trading connection closed"`), or no request for `liveness_ms` on any of its connections (`why: "no request for {ms} ms"`). A full queue: retried on the next timer tick (never dropped).
- At process start the timer is armed only for listed strategies that have open plain orders in the published core (they need protecting; a strategy without orders is armed by its first hello). 판단(모델 아님): avoids a P1 per strategy at every start.
- Core: `Lost` for a configured strategy not already held → `strategy_link_lost{strategy, why}` and, in the same input, a cancel request (`Actor::Rule(RuleId::StrategyLink)`) for every open plain order of that strategy without one (`rule_cancel_plain_orders` with a strategy filter). Its reduce-only orders and other strategies' orders are untouched. `holds(Some(s))` gets `StrategyLink`; `holds(None)` does not (like `OperatorStrategy`).
- Restore: `hello` for strategy `s` submits `Input::Session(Hello { strategy })` and answers the hello only after it is durable whenever the node reported a loss since the last restore or the published core holds `s` (a hold replayed from before a restart). Both inputs go on Lane::Control, so a pending `Lost` is applied before the `Hello` (FIFO). Core: held → `strategy_link_restored{strategy}`; else no event.
- Events: `strategy_link_lost {strategy, why}`, `strategy_link_restored {strategy}` (contract C-LIVE).
- Config: node section `[liveness] strategies = { a = 5000, b = 3000 }` (ms per strategy; design default 5,000; 1,000..=60,000; names must be strategies of `[core]`). A strategy not listed has no liveness, so `omsctl … new` (one short connection) keeps working and the existing tests keep their expectations (owner question 4). Mainnet refuses an enabled strategy that is not listed (owner question 1).
- Alert: condition `strategy_link:<s>`, grace 0, `전략 {s} 연결 상실({why}, {since}). 그 전략의 걸린 일반 주문 취소 요청, 그 전략의 신규 일반 주문 거절. 감소 전용 주문은 유지. 그 전략이 다시 hello하면 풀림`. `hold_condition` returns `None` for `StrategyLink` (covered per strategy).
- Status: `strategies.<s>.link: {liveness_ms, connections, last_request_age_ms, held, since, why}`.

## U18b4 decisions: the monitoring gate

### Socket and heartbeat format (C-API)

- `api.monitor_socket`, default `monitor.sock` under `data_dir`, mode 0600, own uid only, its own pool of 2 connections (판단(모델 아님)). Bound only when `[core.policy.monitor]` is set. Same line protocol as the other sockets: `hello` (no strategy), `ping`, `heartbeat`; every other op → `forbidden`; `heartbeat` on another socket → `forbidden`. At most 2 heartbeats per second per connection (판단(모델 아님); a runaway watcher cannot flood the journal), the rest → `overloaded`, nothing recorded.
- Line (the watcher builds it without oms-api, U19a, so the contract text is the spec):
  ```json
  {"id": 7, "op": "heartbeat", "watcher": "watcher-1", "checks": {"W1": 1763700000123, "W2": 1763700000456, "W3": 1763699999000, "W4": null}}
  ```
  `watcher`: label ≤ 64 chars (판단(모델 아님)); `checks`: keys `W1`..`W9`, value = last success, ms since the epoch on the watcher's clock (same host), or `null` = no success yet; `W1` and `W2` must be present. Reply `"recorded"` after durable.
- Core input `Input::Watcher(Heartbeat { watcher, checks })` (µs). Refused (reply error, no event, no record) if any check time is more than 1 s after the record time (판단(모델 아님): a clock that went back must not make a stale check look fresh). Accepted → `watcher_heartbeat {watcher, checks}`, stored as `core.monitor.last`.

### Freshness and the two stages

`[core.policy.monitor]` (`MonitorPolicy`, in `Policy`, `Option`, not serialised when absent):

| Field | Default | Validation |
|---|---|---|
| `heartbeat_max_age_us` | 30,000,000 (design §7.5) | 10 s..=300 s (≥ 2 heartbeat periods of 5 s) |
| `check_max_age_us` | 10,000,000 (design R2: W1/W2 > 10 s) | 2 s..=60 s |
| `unmonitored_cancel_us` | 60,000,000 (design §7.5, "60 s more") | 10 s..=600 s |

- Verdict at record time `at` (on every heartbeat and every `Tick`): unmonitored if no heartbeat in this boot; or `at − last.at > heartbeat_max_age`; or W1 or W2 is `null` or older than `check_max_age` at `at`. W3/W4 are recorded and shown, not gated (plan: W1, W2).
- A verdict that differs from `core.monitor.monitored` → `monitor_changed {monitored, reason}`; `unmonitored_since` = that record time. A different reason while still unmonitored emits nothing (the since stays).
- Stage 1 (unmonitored): hold `Unmonitored`, plain-only. With a margin policy a reduce-only order passes; without one every hold refuses every order (`decide.rs:222-236`), so a gated config without a margin policy refuses reduce-only too (stated in C-LIVE).
- Stage 2 (`at − unmonitored_since > unmonitored_cancel_us`): on every `Tick`, while the link is up, every open plain order without a cancel request gets one (`Actor::Rule(RuleId::Unmonitored)`, `ExhaustedCancel::Leave`), as `stale_cancel` (`decide.rs:1781-1804`). No event of its own; the cancel requests are the record. With missing heartbeats: unmonitored at 30 s, cancels at 90 s after the last heartbeat (done criteria).
- Back to monitored: the first heartbeat with W1 and W2 fresh → `monitor_changed{true}`; stage 2 stops (no new requests; requests already made stand).
- Boot: `booted` with a gate resets `core.monitor` to `{monitored: false, since: at, reason: "no heartbeat since the start", last: None}`: until the watcher's first fresh heartbeat no plain order leaves, and 60 s after the start resting plain orders are cancelled (design §7.5 "의도된 동작"). `config_activated` that turns the gate on does the same with the activation time; one that turns it off clears `core.monitor`.
- Events: `watcher_heartbeat` (one record per 5 s ≈ 17,280 per day, a few MB, 판단(모델 아님)) and `monitor_changed` (contract C-LIVE). Alternative considered: the node judges freshness and journals only transitions; rejected because the 30 s / 10 s / 60 s rules would then live in two layers and the core could not replay the stage-2 timing.
- Alert: `hold:unmonitored`, grace = the passing grace (30 s): every boot passes through it until the watcher's first heartbeat (U18a rule for passing states). Text: `모니터링 상실({reason}, {since}). 포지션을 늘리는 일반 주문 거절, 감소 전용은 받음. {n}초 뒤 걸린 일반 주문 취소` / `… 걸린 일반 주문 취소 중`.
- Status: `core.monitor: {gate, monitored, since, reason, watcher, last_heartbeat_age_ms, checks: {W1: age_ms, …}, cancel_stage}`.
- Mainnet without `[core.policy.monitor]` is refused (owner question 1).

## Testing Decisions

Loopback fakes only (`http-fake`, the fake WS servers of `gateway.rs` tests and `tests/bybit_node.rs`, simvenue, fake Telegram); manual clocks (core record time; `KillClock`); test directories under `OMS_TEST_DIR` / `CARGO_TARGET_TMPDIR`. No live or testnet request.

### Done criteria → tests

| Done criterion (pr_plan U18b) | Test(s) | PR |
|---|---|---|
| A 10 s private-stream outage latches KillLatched that survives restart (replay) until `clear_hold` reason kill | `bybit_node::a_ten_second_stream_outage_latches_kill_through_a_restart_until_clear_hold_kill` (process, failpoints wiring, `ping_interval_ms = 1000`, `kill_after_ms = 10000`: the fake stops answering pings; no `hold_set` 9 s after the last pong, one by 11 s; the fake's first cancel-all finds the record already in the journal files; restart → `holds` has `kill_latched`, the boot loop runs; `clear-hold` refused `kill_unconfirmed` before the loop ends, accepted after; then a plain order is accepted). Core: `holds::the_kill_latch_and_its_confirmation_survive_replay_and_a_boot_asks_for_a_new_loop`. Gateway: `gateway::the_kill_fires_kill_after_the_last_pong_and_not_before`, `a_quiet_stream_that_answers_pings_never_fires_the_kill`, `a_reconnect_before_the_deadline_does_not_fire`, `the_kill_is_not_armed_before_the_boot_checks_pass`, `time_waiting_on_the_core_queue_does_not_fire_the_kill` | U18b1, U18b2 |
| The fake leaves one order after the first cancel-all, or returns 5xx: repeat until zero confirmed, then 30 s of checks, never > 2 order requests per second | `kill::one_order_left_after_the_first_cancel_all_repeats_until_zero_then_watches_30_s`, `kill::a_5xx_cancel_all_repeats_until_zero_and_never_exceeds_two_per_second` (gateway driver against `http-fake` routes for cancel-all, realtime and position/list; manual clock advanced in 100 ms steps; after each step, once the driver is parked, every 1 s window of loop time holds ≤ 2 order-path POSTs at the fake, and no create or single cancel at all; watch checks at +5 … +30 s; `KillLoopEnded{confirmed}` only after +30 s) | U18b2 |
| During a simulated 10-minute query failure the loop sends at most 12 P1 | `kill::ten_minutes_of_failed_queries_send_at_most_twelve_notices` (pure machine, exact count and kinds) and `kill::ten_minutes_of_failed_queries_reach_the_fake_telegram_at_most_twelve_times` (driver + `Alerts` with a loopback fake Telegram: realtime answers 500 for 600 s of loop time, then `[]`; requests whose text holds `킬 루프` ≤ 12) | U18b2 |
| Every kill-loop P1 carries, per symbol with a position, qty, uPnL, mark and distance to liquidation in %, unknown without a liquidation price | `kill::every_notice_lists_each_position_with_its_liquidation_distance_or_unknown` (two symbols, one with `liqPrice: ""`, one with size 0 left out; also a failed position query) and the unit test of the formatter (rounding, mark ≤ 0) | U18b2 |
| With the kill marker the OMS boots with KillLatched and accepts no order, reduce-only included, until `clear_hold` kill, which removes the marker | `holds_node::a_kill_marker_at_boot_refuses_every_order_until_clear_hold_kill_removes_it` (sim processes, margin policy set: plain and reduce-only refused with `Hold(KillLatched)`; sim answers `unsupported`; clear → file gone; next boot unlatched; an order accepted). Also `holds_node::an_unremovable_marker_is_reported_and_latches_the_next_boot`, `holds_node::a_marker_path_that_cannot_be_examined_latches`, core `holds::the_kill_marker_latches_in_the_boot_record` | U18b1 |
| Under WatcherMismatch, VenueChange and unmonitored a reduce-only order passes and a plain one is refused; under KillLatched and the operator halt both are refused | `holds::the_hold_table_of_table_7_7` (one row per reason, margin policy set; U18b4 adds the `Unmonitored` row, U18b3 the `StrategyLink` row) and `holds_node::watcher_mismatch_over_the_admin_socket_refuses_plain_orders_only` | U18b1, U18b3, U18b4 |
| Existing tests pass with unchanged expectations | the whole suite; in particular `venue_changes::*` event lists, `margin::*` exact hold lists, `e2e::*` (no liveness section, no gate, no marker) and `record_schema::*` (`RECORD_SCHEMA` stays 2, new test `record_schema::kill_holds_liveness_and_heartbeats_recover_to_the_live_state`) | all |
| Closing a strategy's connection, or 5 s without any request, sends cancel requests for that strategy's plain orders only | `liveness::closing_the_last_connection_cancels_that_strategys_plain_orders_only`, `liveness::five_seconds_without_a_request_cancel_and_a_ping_keeps_it_alive` (simvenue; strategies `a` listed at 5,000 ms, `b` listed; `a` plain + reduce-only, `b` plain: cancels at simvenue only for `a`'s plain order; `b` untouched); `liveness::a_second_connection_keeps_the_strategy_alive`, `liveness::hello_after_a_loss_is_answered_after_the_restore_is_durable`; core `strategy_link::*` | U18b3 |
| Missing heartbeats, and heartbeats with W1 or W2 older than 10 s, move the gate through the states at 30 s and 90 s with an injected clock | `monitor::missing_heartbeats_unmonitor_at_30_s_and_cancel_plain_orders_at_90_s`, `monitor::a_heartbeat_with_w1_or_w2_older_than_10_s_unmonitors_and_cancels_60_s_later` (core, record time in 250 ms ticks: monitored at 29.75 s, unmonitored at 30.25 s; no cancel at 89.75 s, cancels at 90.25 s; reduce-only resting order kept), `monitor::the_gate_starts_unmonitored_at_boot_until_the_first_fresh_heartbeat`, `monitor::a_fresh_heartbeat_monitors_again_and_stops_the_cancels`, `monitor::a_heartbeat_from_the_future_is_refused`; node `monitor_node::heartbeats_over_monitor_sock_open_the_gate_and_other_ops_are_forbidden` | U18b4 |
| Full local gate | the unit's `checks` command | all |

Further tests holding the decisions: `holds::clear_hold_refusals` (each code; nothing written on a refusal), `holds::resume_leaves_kill_and_watcher_mismatch`, `holds::a_venue_change_latches_without_a_new_event`, `holds::a_journal_with_a_venue_change_latches_on_replay`, `holds::a_second_trigger_keeps_the_first_note`; `gateway::no_create_leaves_while_the_kill_loop_runs`, `gateway::a_blocked_gateway_sends_no_cancel_all_and_says_why`, `gateway::kill_stop_ends_the_loop_and_creates_flow_again`; `kill::a_failed_count_is_never_zero`, `kill::a_late_order_in_the_watch_restarts_the_rounds`; config bounds for every new field, mainnet refusals; `alerts::tests` extended (every new reason has a text and the table's grace); `ctl` parse of `clear-hold`; `api::tests::wire_examples` for the new ops. Suites: `crates/xtask/src/verify.rs:57` (`--test` list) gets `holds_node`, `liveness`, `monitor_node`; `bybit_node` stays in the failpoints suite (`verify.rs:70`).

### Deliberate defects (xtask mutants, M125 onward; each must be caught)

1. `KillLatched.refuses_only_plain_orders()` returns true.
2. `clear_hold kill` skips the `kill_unconfirmed` check; `booted` does not reset `confirmed_at`.
3. The marker check treats a stat error as "no marker"; the marker is removed before the clear is durable.
4. The kill trigger measures from the link-down time instead of the last proof; the queue-wait adjustment of `last_proof` removed; the trigger armed before the boot checks.
5. A failed count read as zero; C5 skipped (End at the first zero); a late order in C5 does not return to C2.
6. The emergency budget not taken (two rounds without waiting); the repeat notice ignores `notice_ms`; an unknown count treated as a change.
7. The `Kill` command released from the core thread instead of after durable (derived outside `command_for`).
8. `StrategyLink` cancels every strategy's plain orders; a strategy's reduce-only order cancelled; `ping` not counted as a request; hello answered before the restore is durable.
9. The gate reads W3 in place of W2; stage 2 at `unmonitored_since + 0`; a stale-W2 heartbeat clears the gate; a future check time accepted.

## Documents and registry

- ADR-030 "Kill switch, operator-released holds, strategy liveness and the monitoring gate": chosen = journaled latches released only by `clear_hold`, a Bybit confirmation loop started by a durable command, node-side liveness with core-side holds, core-side gate fed by journaled heartbeats; alternatives = the watcher's `halt` (blocks reduce-only, design R5), cancel-all once without confirmation (asynchronous, 500 cap), node-side gate with transitions only (above), cancel-all of every strategy on a strategy loss. Amends ADR-008 §7 hold table, ADR-028 (trigger, kill_scope use).
- Contracts: `docs/contracts/holds.md` C-HOLD (table 7-7, `hold_set`, `hold_cleared`, watcher inputs, `clear_hold`, marker), `docs/contracts/kill.md` C-KILL (trigger, loop, budget, notice rule; `kill_loop_ended`), `docs/contracts/liveness.md` C-LIVE (strategy liveness; the gate; heartbeat format; `strategy_link_lost`, `strategy_link_restored`, `watcher_heartbeat`, `monitor_changed`); rows in `api.md` (ops, monitor socket), `gateway.md` (`Kill`, `KillStop`, `StreamLost`, `KillLoopEnded`), `tail.md` field table. `registry/contracts.toml` entries with their events; claims `V-HOLD-1..`, `V-KILL-1..`, `V-LIVE-1..`, `V-GATE-1..` naming the tests above.
- Runbook: rows for kill latched (what the P1s mean, wait for `정리 완료`, then `omsctl clear-hold --reason kill --note …`; X8 key revocation if the loop keeps failing), the marker (written by the watcher; removed only by clear-hold), watcher mismatch, venue change (ack each mismatch, then clear-hold), unmonitored (start the watcher; it clears itself), strategy link (reconnect the strategy). U-06 note: a restart with the marker or a journaled latch does not resume by itself. GLOSSARY: kill latch, kill loop, kill marker, emergency budget, hold release, strategy liveness, monitoring gate, unmonitored. README status paragraph.
- `cargo run -p xtask -- docs` and commit `docs/generated/` in every PR.

## Out of Scope

- The watcher itself: W1–W4 and its heartbeat sender (U19a); W5, writing the marker, its own loop and `[WATCH P1]` (U19b). U18b only defines what the OMS accepts on `admin.sock` and `monitor.sock` and documents it.
- Deployment (U20b): systemd units (`Restart=no`), vmalert rules, dashboards, runbook deployment steps.
- A kill loop for Hyperliquid and the simulated venue (they answer `unsupported`; the operator's existing `cancel_all` cancels per order). Polling the marker while running. Closing positions after a kill (TN §7.6: no automatic reduce-only close; a separate decision). Changing `halt`/`resume`. Any live or testnet request; any committed key, journal, snapshot or log.

## Open owner questions

1. **Mainnet without the new safety inputs.** Refuse to start when mainnet has no `[kill] marker`, no `[core.policy.monitor]`, or an enabled strategy missing from `[liveness]`? Default if unanswered: refuse all three (as for the margin policy and `[alert]`, `config.rs:500-546`). Testnet and sim: optional, one `warn` each.
2. **No force for `clear_hold kill`.** The latch can be cleared only after the kill loop of this process confirmed zero open orders and watched 30 s. If the venue cannot be reached, the hold stays until it can (restart with a new key after an X8 revocation runs a new loop). Default: no force option, as `clear_reduce`.
3. **Venue-change release in two steps.** Keep `ack_mismatch` per entry and add `clear-hold --reason venue_change` (refused until the venue's position after the change was read), or let `clear-hold venue_change` also acknowledge those mismatches? Default: two separate steps (existing tests and the mismatch contract unchanged).
4. **Strategy liveness default.** The plan says "configured time (default 5 s)". Default used: a strategy has liveness only when listed in `[liveness]` (value default 5,000 ms in the examples), because a strategy that connects per request (as `omsctl new` does) would have its order cancelled on every close. Should listed-by-default (all enabled strategies at 5 s unless opted out) apply instead?
5. **Every restart under a latch runs the loop again**, so a kill marker or a journaled latch makes the OMS send its own cancel-all at the next start (after the boot checks). Default: yes (it confirms zero before a person can clear).
