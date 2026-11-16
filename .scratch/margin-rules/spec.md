# Spec: margin rules (user decision U-02)

Status: ready-for-agent

Vocabulary: `GLOSSARY.md`. Decision record: ADR-018. Behaviour contract: C-MARGIN (`docs/contracts/margin.md`). Tickets: `issues/` next to this file. Korean version for the owner: `spec-ko.md` next to this file.

## Problem Statement

The owner wants to run Hyperliquid perpetual strategies through this OMS, starting with 100 USDC. Today the OMS limits only quantities. The owner cannot rely on it for the things that lose an account:

- An order can be accepted that makes the account's possible position many times larger than the account value, because resting orders and account value are not looked at.
- The OMS keeps accepting new orders on a day when the account has already lost a large part of its value.
- The OMS does not see that an isolated position is close to liquidation.
- The venue's leverage or margin mode for an instrument can differ from what the owner planned, and nothing notices.
- Every hold refuses every order, also an order that can only reduce a position. So a state "only reduce" cannot exist today: its own stuck cancel would stop the reducing orders.
- The owner asked how an unfilled order is managed and who decides to cancel or reprice it. The documents do not answer that in one place.

## Solution

When a margin policy is configured, the OMS reads a margin report from the venue every 5 seconds and applies these rules. Without a margin policy the OMS behaves exactly as before.

1. **Leverage gate.** A new plain order is refused when it would raise account exposure above 2 times the account value. Exposure counts the position and every resting order at its worst case. Nothing latches.
2. **Daily loss.** When the account value falls more than 5 % below the day base, the account enters REDUCING.
3. **MM buffer.** When the isolated margin of a position falls below (maintenance margin rate + 0.25 percentage points) of its value, the account enters REDUCING.
4. **REDUCING.** Plain orders are refused, resting plain orders are cancelled, reduce-only orders are accepted and kept. The OMS never closes a position. Only the operator clears REDUCING; the clear takes the current account value as the new day base.
5. **Stale data.** With no fresh margin report (older than 30 s) plain orders are refused. With no accepted report for more than 60 s, resting plain orders are cancelled. Nothing latches.
6. **Venue setting and account mode.** The OMS reads them and never writes them. A wrong or unknown venue setting holds plain orders on that instrument. A wrong or unknown account mode holds plain orders on the account.
7. **Hold table.** Holds for resync, in-doubt orders, mismatches, REDUCING, venue setting and account mode refuse only plain orders. Reduce-only orders pass them. This also applies only when a margin policy is set; without one every hold refuses every new order, as today.
8. **Unfilled orders.** The strategy decides when to cancel or re-send. The OMS tracks a resting order, counts it in the leverage gate, and cancels it on REDUCING or on a stale cancel if it is a plain order.

The owner gets, before development, a list of 25 scenarios (S1–S25). Each has the expected behaviour, the planned test that proves it, what that test guarantees and what it does not.

Values set by the owner: account leverage limit 2x, MM buffer +0.25 percentage points, daily loss 5 %, UTC day, operator-only release, start capital 100 USDC, one account with isolated margin per instrument.

Values decided as "Fable·Claude 합의 (추천안 적용, 사장님이 바꿀 수 있음)":

| # | Decision | Alternative |
|---|---|---|
| O1 | Venue leverage is 2x isolated, the same number as the account limit | 3x |
| O2 | The MM buffer stays at +0.25 percentage points as a last tripwire; it fires 0.13–0.37 % of price before liquidation (BTC, venue 2x) | A larger buffer |
| O3 | The strategy cancels and re-sends unfilled orders; the OMS adds no auto-cancel timer | A maximum rest time in the OMS |
| O4 | A clear takes the current account value as the new day base | No release until the next UTC day |
| O5 | No deposit or withdrawal while the node runs; the operator runs `rebase-day` after one | Automatic detection of transfers |
| O6 | The venue account stays in standard mode | Unified account or portfolio margin |

## User Stories

1. As the owner, I want a new order refused when it would take the account above 2x leverage, so that a strategy error cannot build a position the account cannot carry.
2. As the owner, I want resting orders counted at their worst-case fill in that check, so that orders which have not filled yet cannot take the account past the limit later.
3. As the owner, I want the leverage limit to be a configuration value, so that I can change it without a code change.
4. As the owner, I want an order that lands exactly on the limit accepted, so that the rule is predictable at the boundary.
5. As the owner, I want exposure summed over all instruments, so that two instruments cannot each use the whole limit.
6. As the owner, I want a sell far above the market counted at its limit price, so that the check covers the price at which it would really fill.
7. As the owner, I want trading to stop adding risk when the account has lost more than 5 % in a UTC day, so that one bad day cannot empty the account.
8. As the owner, I want the daily loss measured against a day base stored in the journal, so that a restart cannot erase a loss.
9. As the owner, I want the first report after midnight judged against yesterday's base before it becomes today's base, so that a loss taken during a gap is not absorbed.
10. As the owner, I want a new UTC day to leave REDUCING in place, so that only a person decides to trade again.
11. As the owner, I want fees and funding to count as loss, so that the 5 % rule looks at what the account is really worth.
12. As the owner, I want the OMS to enter REDUCING when an isolated position is close to liquidation, so that nothing is added to an account in trouble.
13. As the owner, I want an MM-buffer trigger on one instrument to stop plain orders on all instruments, so that the whole account is protected.
14. As the owner, I want the OMS never to close a position by itself, so that no automatic market order is sent in my name.
15. As the owner, I want to know that the realised daily loss can be larger than 5 %, so that I do not read the rule as a guarantee.
16. As the owner, I want a list of important scenarios with the proving test for each, so that I can control what was built.
17. As the owner, I want to read what each important test guarantees and does not guarantee, so that I do not trust a green test for more than it shows.
18. As the owner, I want every fact that only the testnet can confirm named as "not verified", so that I know what stands between this work and real money.
19. As the owner, I want mainnet refused without a margin policy, so that the rules cannot be forgotten.
20. As the owner, I want the rules off when no margin policy is set, so that existing behaviour, tests and journals stay valid.
21. As the operator, I want a `clear-reduce` command that needs a note, so that every release is recorded with a reason.
22. As the operator, I want the clear to take the current account value as the new day base, so that the stop does not trigger again 5 seconds later.
23. As the operator, I want a further 5 % loss after a clear to stop trading again, so that a release is not a licence to lose without limit.
24. As the operator, I want the clear refused while the margin report is stale, so that I do not release on old numbers.
25. As the operator, I want the clear refused while the MM-buffer condition is true, so that a position near liquidation cannot be released into more risk.
26. As the operator, I want a `rebase-day` command, so that a deposit or withdrawal does not distort the daily loss.
27. As the operator, I want the status to show the state (NORMAL or REDUCING), the rule that tripped, the day base, the account value and its age, and the account exposure, so that I can see why orders are refused.
28. As the operator, I want the status to show each instrument's venue setting and the account mode, so that I can fix a mismatch at the venue.
29. As the operator, I want the status to show why the last margin report was rejected, so that I can tell a venue problem from a network problem.
30. As the operator, I want REDUCING and the day base to survive a kill and restart, so that a crash does not release the account.
31. As the operator, I want a crash between a rule decision and its journal write to leave a consistent state, so that a restart needs no manual repair.
32. As the operator, I want the strategy socket unable to clear REDUCING or rebase, so that a strategy cannot release itself.
33. As the operator, I want resting plain orders cancelled when margin reports stop for more than 60 s, so that the account does not keep filling orders while blind.
34. As the operator, I want an instrument held when its venue leverage or margin mode differs from the configuration, so that an order does not use margin I did not plan.
35. As the operator, I want the account held when it is in unified or portfolio mode, so that the rules do not read numbers that have no meaning there.
36. As the operator, I want the query load checked against the venue's request limit at start, so that adding instruments cannot starve the margin report.
37. As the operator, I want a list of what I must do (flag settings at the venue, rebase after transfers, close unattributed positions at the venue), so that the rules work as described.
38. As a strategy developer, I want my reduce-only order accepted during REDUCING, so that I can close my position.
39. As a strategy developer, I want my reduce-only order accepted during a resync, an in-doubt order or an unacknowledged mismatch, so that an operational problem does not trap my position.
40. As a strategy developer, I want a clear reject reason (`AccountLeverage`, `MarginUnavailable`, a named hold), so that my code can react correctly.
41. As a strategy developer, I want to know that an unflagged take-profit is cancelled on REDUCING and on a stale cancel, so that I flag every reducing order.
42. As a strategy developer, I want to know that the OMS never reprices or expires a resting order by itself, so that I own the cancel and re-send logic.
43. As a strategy developer, I want a plain order that reduces exposure accepted after a price move took the account above the limit, so that I can lower risk without the flag.
44. As a strategy developer, I want a reduce-only order to need no fresh margin report, so that I can reduce while the venue queries fail.
45. As a strategy developer, I want a reduce-only order still checked against my position limit, so that the quantity invariant of the OMS stays true.
46. As a strategy developer, I want a cancelled order to keep its exposure until terminal evidence arrives, so that the gate never assumes a cancel worked.
47. As a strategy developer, I want an order that the venue rejects to free its exposure, so that a venue refusal does not use up my limit.
48. As a reviewer, I want every scenario row to name a test that fails when the rule is broken, so that I can check a guarantee by breaking the rule.
49. As a reviewer, I want expected values in tests written as literals worked out by hand, so that a test does not repeat the rule's own formula.
50. As a reviewer, I want mutants on the rule code run before the final gate, so that a rule line no test protects is found.
51. As a reviewer, I want the performance claims rerun after the gate lands, so that the hot-path cost of the exposure sum is measured and not assumed.
52. As a reviewer, I want a simulated run over several UTC days with restarts, so that day rolls and the latch are tested together over time.
53. As a developer, I want every margin event listed under contract C-MARGIN in the registry, so that the traceability documents stay complete.
54. As a developer, I want each ticket to carry its own tests, claims, mutants and contract text, so that the integration branch is verifiable after every merge.
55. As a developer, I want a late margin report ignored when a newer one was accepted, so that slow venue answers cannot move the state backwards.
56. As a developer, I want an unreadable margin report rejected as a whole and journaled with its reason, so that a half-parsed report never drives a rule.

## Implementation Decisions

Source of every decision: the closed design agreement between Fable 5.2 and Claude (2026-11-15/16), the owner's decisions, and the grill of test gaps G1–G11 in this stage. Decisions that a skill asks to confirm with the user were delegated by the owner (2026-11-16) and are marked **[delegated]** with the alternative.

### Principle

- A rule that latches reads only the venue's margin report. A rule that reads the OMS books only rejects one order. So an OMS bookkeeping error cannot switch off daily loss or MM buffer, and the worst failure of the leverage gate is one wrong accept or reject.

### Modules

- **Core** (deterministic decision module): new policy section, new inputs, new events, the leverage gate inside the new-order check, the two latch rules, the REDUCING state, the hold table, the stale cancel inside the tick, the operator commands. All state changes go through journal events and are restored by replay.
- **Configuration**: `policy.margin` is optional. Absent means rules off. Node validation refuses mainnet without it, refuses bad values, and refuses a query-weight budget above half of the venue limit.
- **Reconciler task** (node): a separate margin query every 5 s while the link is up, and venue-setting and account-mode queries at link-up and every 60 s for instruments without a position. The existing 15 s sweep and the position input are not changed. Each margin query gets a sequence number that rises within a link session.
- **Simulated venue**: answers the margin, venue-setting and account-mode queries, with control knobs for account value, margin fields, setting and mode. Its account value follows fills, price, fees and funding.
- **Hyperliquid gateway**: request bodies and pure parse functions for the margin report, the per-instrument venue setting and the account mode. An unreadable answer becomes a rejected report with a reason, never a partial report.
- **API and operator tool**: `clear-reduce --note` and `rebase-day --note` on the operator socket only. Status fields for the state, rule, day base, account value and age, account exposure, venue settings, account mode and the last report-rejection reason.
- **Simulation world**: a venue margin model, margin profiles, new invariants, a multi-day profile.
- **Verification tooling**: mutants for the rule code, a core benchmark case with the margin policy on, the registry rows.

### Names fixed in this stage

- The venue answer is a **margin report** (`MarginReport`), not a "snapshot". "Snapshot" already means the journal snapshot in this repository.
- Inputs: `MarginReport`, `MarginReportInvalid`, `VenueSetting`, `AccountMode`, operator `ClearReduce`, operator `RebaseDay`.
- Events: `margin_reported`, `margin_report_rejected`, `day_base_set`, `reducing_entered`, `reducing_cleared`, `venue_setting_noted`, `account_mode_noted`. Rule cancels use the existing `cancel_requested` with a new actor.
- Reject reasons: `AccountLeverage`, `MarginUnavailable`; holds `Reducing`, `VenueSetting(instrument)`, `AccountMode`; command refusals `NotReducing`, `MmBufferActive`, `NoteRequired`.
- Actor: `Rule(RuleId)` with `DailyLoss`, `MmBuffer`, `MarginStale`. The API can build only strategy and operator actors.

### Policy values

| Value | Default | Validation |
|---|---|---|
| `max_account_leverage` | 2.0, stored as a fixed-point integer | greater than 0 |
| `daily_loss_bps` | 500 | greater than 0 |
| `mm_buffer_bps` | 25 | 0 or more |
| `margin_max_age_us` | 30 000 000 | greater than 0 |
| `stale_cancel_us` | 60 000 000 | greater than `margin_max_age_us` |
| `venue_leverage` | 2 | 1 or more |
| `isolated` | true | — |
| `account_modes_allowed` | empty | `unifiedAccount` and `portfolioMargin` are never allowed, also when listed |
| runtime `margin_poll_ms` | 5 000 | greater than 0 |
| runtime `venue_setting_poll_ms` | 60 000 | greater than 0 |

### Margin report intake

- Accepted only when the link is up, the report's session is the current link session, and its sequence number is greater than that of the last report accepted in this session. Any other report is dropped: not stored, not judged.
- Ordering uses the sequence number, not the query time, because the wall clock can step backwards.
- A report with any unreadable number, or with a missing or zero maximum leverage, is rejected as a whole. The rejection and its reason are journaled. The last good report stays and ages. The core itself also rejects a zero maximum leverage.
- A position on an instrument that is not configured is kept in the report.
- Processing order for one accepted report, inside one input: record → venue settings from its positions → MM buffer → daily loss → day roll.
- Rules are judged on every accepted report, also one that is already older than 30 s when it arrives.
- Link change makes venue settings and account mode unknown. The last report, the day base and REDUCING stay.

### Freshness

- Age = record time − query time, both node clock values.
- Fresh means 0 ≤ age ≤ 30 s. Exactly 30 s is fresh. A report whose query time is after the record time is not fresh.
- Without a fresh report a plain order is rejected with `MarginUnavailable`. A reduce-only order needs no fresh report.
- A plain order is also rejected with `MarginUnavailable` when an instrument with non-zero exposure has no fresh reference price.
- Staleness is a reject reason and not a hold, because holds are computed from state without a time input.

### Leverage gate

- Per instrument, with `pos` the account book position, `ref` the reference price, and every resting order (reduce-only included) at its remaining reserved quantity:
  - `N_up = max(0, pos × ref + Σ buys reserved × max(limit, ref))`
  - `N_down = max(0, −pos × ref + Σ sells reserved × max(limit, ref))`
  - instrument exposure = `max(N_up, N_down)`; account exposure = sum over instruments.
- A plain order is rejected with `AccountLeverage` when exposure after > limit × account value **and** exposure after > exposure before. Equal to the limit is accepted. Account value ≤ 0 rejects.
- A reduce-only order skips the gate. It still counts in the sums for later orders.
- Pre-trade only. A price move above the limit latches nothing and cancels nothing.
- Quantity comes from the OMS book, never from the margin report. Price is the existing reference price (mid). There is no mark feed.
- A cancel request does not lower exposure. Terminal evidence does.
- Integer arithmetic in `i128`, no division, no overflow at the largest configured price × quantity.

### Daily loss and day base

- State: day base = (UTC day number of the query time, value, who set it), or none. It changes only by `day_base_set`.
- On every accepted report, in this order: (1) if a base exists and `(base − account value) × 10 000 > daily_loss_bps × base`, enter REDUCING with rule `DailyLoss`; (2) if no base exists or the report's UTC day is later than the base's day, set the base to this report's account value.
- Base 100: 95.00 does not trip, 94.99 does.
- A report whose UTC day is earlier than the base's day never changes the base: a report-driven roll never moves the day backwards. Only the operator's `rebase-day` or `clear-reduce` can set an earlier day (see Clear and rebase).
- No rebase at boot. A new day does not clear REDUCING.
- Fees and funding are inside account value, so they count as loss. A withdrawal reads as a loss. A deposit hides a loss of its size until a rebase or the next UTC day.

### MM buffer

- Judged for every isolated position with position value > 0 in an accepted report.
- Condition: `margin_used × 2 × max_leverage × 10 000 < position_value × (10 000 + 2 × max_leverage × mm_buffer_bps)`. True → enter REDUCING with rule `MmBuffer`.
- Maximum leverage 40, buffer 25 bps, position value 200: isolated margin 2.99 trips, 3.00 does not.
- The maintenance margin rate is 1 / (2 × maximum leverage) and comes from the report, not from configuration.

### REDUCING

- Stored like the operator halt: set by `reducing_entered`, cleared by `reducing_cleared`; the hold `Reducing` is derived from it.
- Entry, inside the input that trips the rule: `reducing_entered`, then a rule cancel for every open order that is not reduce-only and whose cancel is absent or exhausted.
- An order still waiting for its send result gets the cancel recorded; the cancel is sent after the venue acknowledges the order.
- While REDUCING: a plain order is rejected with `Hold(Reducing)`, also one that would reduce the position. A reduce-only order is accepted. Resting reduce-only orders are kept.
- Entered once: a second trigger emits nothing, and the first rule stays recorded.
- One latch for the whole account.

### Clear and rebase

- `clear-reduce --note` is accepted only when: the account is REDUCING, the report is fresh, the MM-buffer condition is false on it, and the note is not empty. Refusals: `NotReducing`, `MarginUnavailable`, `MmBufferActive`, `NoteRequired`.
- On success: `day_base_set` by the operator with the current account value and the report's UTC day (also when that day is earlier), then `reducing_cleared` with the note, the old base and the new base. After a clear at 94, a report at 89.30 does not trip and 89.29 does.
- `rebase-day --note` needs a fresh report and a note, sets the base to the current account value and the report's UTC day (also when that day is earlier), and leaves REDUCING alone.
- There is no force option.

### Stale cancel

- Condition: margin policy set, link up, and record time − max(query time of the last accepted report, time the current link session came up) > 60 s.
- Action in the tick: a rule cancel (`MarginStale`) for every open order that is not reduce-only and has no cancel request. One request per order. An exhausted cancel is not re-issued.
- No latch and no hold. A newly accepted report ends it.
- The time the link session came up is stored state, set by the link-up event.

### Hold table

| Refuses every new order | Refuses only plain orders |
|---|---|
| `NoConfig`, `LinkDown`, `OperatorAccount`, `OperatorStrategy`, `StorageLow` | `Resync`, `InDoubt`, `Mismatch`, `Reducing`, `VenueSetting(instrument)`, `AccountMode` |

- The table applies only when a margin policy is set. Without one, every hold refuses every new order, reduce-only included, exactly as before this work. So a journal recorded without a margin policy gives the same result when it is re-decided (`verify --re-decide`). A planned test in ticket 01 pins this.
- "Reduce-only" means the venue flag on the order, not a direction computed by the OMS.
- A reduce-only order skips only the right-hand holds and the leverage gate. It passes every other existing check, including the strategy position limit and the price band.
- This amends ADR-008 §7. The amendment text is in ADR-018 and is applied to ADR-008 by the ticket that implements the table.

### Venue setting and account mode

- Sources: positions in the margin report, and the per-instrument setting query.
- `VenueSetting(instrument)` hold: the setting was not received in this link session, or its mode or leverage differs from the policy.
- `AccountMode` hold: the mode was not received in this link session, or it is `unifiedAccount` or `portfolioMargin`, or it is not on the allow-list.
- The OMS never sends a venue-setting change.

### Clock

- The node's record time never goes backwards (existing behaviour: it waits for the wall clock to catch up). While it waits, time-based behaviour (freshness, stale cancel, order expiry) is judged with the frozen time.
- After a backward step, new reports carry query times before the record time, read as old, and plain orders are rejected until the clock catches up. This is the safe side.

### Request budget

- Steady-state query weight per minute, from the configuration: margin report 24 (weight 2, every 5 s), reference prices 24, sweep 168 (weight 42, every 15 s), venue setting 20 per instrument (every 60 s, worst case no positions). Account mode adds 20 once per link-up.
- Validation refuses a configuration whose steady-state weight is above 600 per minute (half of the documented 1200 per minute per IP).
- Existing fact, kept: the gateway's local request budget never delays a cancel. A REDUCING entry can send up to the open-order limit (500) of cancels at once, which can delay the next queries until the budget refills at 20 per second.

### Registry and documents

- The contract row C-MARGIN exists from this stage with empty event and claim lists. Each ticket adds its events, claims (V-MARGIN-1 to V-MARGIN-12) and tests in the same change, because the document check refuses a claim without an existing test and an event without a contract.
- Claims: V-MARGIN-1 hold table; -2 report intake; -3 leverage gate; -4 REDUCING; -5 daily loss; -6 clear and rebase; -7 stale cancel; -8 MM buffer; -9 venue setting and account mode; -10 Hyperliquid parsing; -11 the node process level (seam C): poll wiring and status, operator commands, restart; -12 simulation invariants. Existing claims V-REPLAY-1, V-SIM-1, V-CFG-1, V-PERF-1..5 and V-MUT-1 gain tests or are rerun.
- Every change that touches Rust source regenerates the generated documents and commits new evidence from the extended verification run without the live-testnet suite. The live-testnet suite is not run; claim V-HL-3 stays "not run" with that reason.

## Testing Decisions

### What makes a good test here

- It uses only the public entry point of its seam and reads results from outputs, events, holds, status or replay. It never reads a private field.
- One behaviour per test. The test name says the behaviour.
- Expected values are literals worked out by hand. A test does not recompute a rule with the rule's formula.
- It can fail: breaking the rule with a one-line change turns it red. The reviewer checks this for every scenario row.
- Each test is written first, seen red for the expected reason, then made green with the smallest change (tdd skill), one commit per cycle.

### Seams **[delegated]**

Four existing seams are used. No new seam is added.

| Seam | Entry point | Catches | Misses |
|---|---|---|---|
| A. Core | The core's single input handler; results read from its output events and commands, the hold query, the state digest and replay | Every rule, the hold table, the latch, rule cancels, boundaries, replay equality | Wire format, real timing, whether venue numbers mean what we derived (T1, T2) |
| B. Gateway parse | Pure functions from a JSON answer to a typed input or a reason | Field mapping, decimals, missing or zero maximum leverage, unknown enum values, request bodies | Whether the live answer has these fields; anything the venue does |
| C. Node process | Operator tool → admin socket → node process with the simulated venue process; results read from status. The crash-point variant aborts the process at a named commit boundary | Commands end to end, status, poll wiring, latch and day base across a real kill | The real venue; exact timing |
| D. Simulation | A world run by seed and profile, judged by its always-checks and final checks | Random interleavings of fills, drops, restarts and reports against invariants | The real venue; anything the world model does not generate |

Configuration validation and the request-budget arithmetic are tested as unit tests of the node library (existing suite), because they are pure functions with no higher seam.

Alternative considered: seam A only. Rejected because the operator commands, the restart and the Hyperliquid field mapping cannot be reached from the core.

### Modules tested and prior art

- Core: the existing scenario tests with their harness (helpers for inputs, venue inputs, ticks, operator commands, accept, reject, rebuild from journal) are the model. New margin tests form one new core test file; hold-table tests sit beside the existing hold tests (`holds_are_independent`, `cancels_are_accepted_while_new_orders_are_held`).
- Gateway parse: the existing parse tests of order answers, with the documented examples of the protocol research note as fixtures.
- Node process: the existing end-to-end tests that start a node and the simulated venue, kill the node and read status; and the existing crash-point tests that abort at `before_write`, `after_write`, `after_sync`, `after_reply`.
- Simulation: the existing seeded profiles and invariant checks.
- Property tests: the existing `proptest` properties of the numeric module.
- Mutants: the existing hand-written mutant list of the verification tool.
- Performance: the existing core benchmark (about 500 open orders) and the open-loop load runs behind claims V-PERF-1..5.

### Scenario coverage

Every scenario S1–S25 has at least one planned test in a ticket. The ticket files list the planned names. S4 and S20 get margin-specific tests on top of the existing ones. S19 is covered by composition (external fill plus loss report); the shape of a real liquidation fill (T6) is not verified.

### Test gaps G1–G11: decisions

| Gap | Decision | Where |
|---|---|---|
| G1 Performance | Accepted. The gate runs on every new order and sums over all resting orders. The core benchmark gets a case with the margin policy on and 500 resting orders; claims V-PERF-1..5 are rerun after the gate lands. Perf evidence is not left stale | Ticket 12 |
| G2 Mutation testing | Accepted. Every rule ticket adds mutants for its rule lines (leverage comparison, exposure sum, daily-loss comparison, evaluate-then-roll order, MM comparison, latch entry, hold table rows, stale limits, venue-setting comparison, sequence guard). The full mutant run before the final gate must catch all; a survivor is a finding that is fixed or written down | Tickets 01–09, run in 12 |
| G3 Property tests | Accepted. Through seam A: adding an order never lowers exposure; mirroring long and short gives the same exposure and the same decision; account exposure is the sum over instruments; no overflow and no rounding flip at extreme price × quantity, with expected values from independent `i128` multiplication on literals | Ticket 03 (gate), 07 (MM) |
| G4 Crash points for the latch | Accepted. The crash-point mechanism is extended to fire on a margin-report batch. Crash before the journal write of the REDUCING decision; crash after the write and before the cancels are sent; crash around a day-base set. After restart the latch, base and cancel state are consistent | Ticket 05 |
| G5 Wall clock jumps | Accepted. Backward: a report dated after the record time is not fresh; an earlier UTC day does not re-base. Forward: the stored report reads stale, the stale cancel fires, a multi-day roll judges against the old base and rolls once | Tickets 02, 04, 06 |
| G6 Out-of-order reports | Accepted. A sequence number per query; a report with a sequence not greater than the last accepted one is dropped, not stored and not judged | Ticket 02 |
| G7 Request weight | Accepted. Deterministic test of the weight computed from configuration against the 600 per minute budget. For the cancel burst: a test pins what the budget does after 500 cancels and how long the next margin query can wait; the result is written in the contract | Ticket 09; burst through the node in 05 |
| G8 Multi-day soak | Accepted at the simulation seam: a profile whose virtual time crosses at least 3 UTC day rolls with restarts; all invariants hold; margin journal records stay within one per poll | Ticket 11 |
| G9 Gap larger than 5 % | Accepted. A test where two consecutive reports are 100 and 88: REDUCING is entered on the second report with a 12 % loss already taken. The non-guarantee is stated here and in the Korean spec | Ticket 04 |
| G10 Fees and funding | Accepted for the model: the world's account value charges fees and funding, and a test shows a loss made only of fees and funding trips the rule. Which balance they are paid from stays unverified (T3) | Ticket 10 |
| G11 Testnet rehearsal | Accepted as a statement: out of this work, required before mainnet, needs the owner's approval | Out of Scope |

### What the important tests guarantee and do not guarantee

| Tests | Guarantee | Do not guarantee |
|---|---|---|
| Leverage gate (seam A, properties) | With the numbers the OMS holds, no plain order is accepted that takes account exposure above the limit at acceptance, at the boundary and at extreme values | Exposure after a price move; that mid equals mark; that the venue accepts the order; that account value includes isolated equity (T2) |
| Daily loss (seam A) | A report more than 5 % below the base enters REDUCING; the base survives replay; a gap across midnight is judged against the old base | Losses between two reports; losses while no new report is accepted; a deposit hiding a loss |
| REDUCING entry (seam A, D) | Every resting plain order gets a cancel request in the same input; plain orders are refused; reduce-only orders pass | That a cancel beats a fill; that the position is closed; behaviour while the link is down |
| MM buffer (seam A, property) | The comparison is exact at the boundary and at extreme values | That `marginUsed` is the isolated equity (T1). If T1 is wrong the rule can be wrong in both directions |
| Clear and rebase (seam A, C) | A clear is refused when stale, without a note, when not REDUCING, or while the MM condition holds; a strategy cannot clear | That the operator judged well; a way out of a false MM latch |
| Stale cancel (seam A, D) | After 60 s without an accepted report each resting plain order gets one cancel request | The position; reduce-only orders; anything while the link is down |
| Restart and crash points (seam C) | The latch and the base survive a kill at each commit boundary; unsent rule cancels are sent after restart | Disk loss; power failure (not tested in this repository) |
| Gateway parse (seam B) | The documented example maps to the right fields; an unreadable answer is rejected as a whole | That the live answer equals the documented one |
| Simulation (seam D) | The invariants hold over random interleavings, restarts and several virtual days | Anything about the real venue; the venue margin model is our own model |

## Out of Scope

- **Running a node against testnet or mainnet, and sending any order.** "Deploy" in this work is a merged pull request.
- **Testnet rehearsal.** Real orders on testnet (a REDUCING drill, a stale-cancel drill, kill and restart with open orders) is the standard step before mainnet. It is not part of this work. It is required before mainnet and needs the owner's approval (U-01, U-09). The same holds for facts T1–T11, which only the testnet can confirm: T1 isolated `marginUsed` is equity; T2 account value includes isolated equity; T3 where fees and funding are paid from; T4 leverage change with an open position; T5 oversize or multiple reduce-only orders; T6 shape of a liquidation fill; T7 minimum notional for a reduce-only close; T8 margin reserved by resting isolated orders; T9 maximum leverage and margin tiers; T10 which account-mode value means standard; T11 venue setting query with no position.
- The read-only live-testnet suite. It is not run; claim V-HL-3 stays "not run".
- A mark price feed. A ledger poll that detects deposits and withdrawals. A maximum rest time for orders. Cancel-on-disconnect for a dead strategy. An atomic position transfer between ledgers. Alerts (U-11). Sub-accounts. Per-address request budget. Writing venue settings.
- Closing positions automatically. Trigger (stop) orders.
- Pruning the journal (U-04).

## Further Notes

- **Deviation kept in the next stage (implement-spec).** implement-spec asks for maximum concurrency. Here at most two implementer subagents run at a time, and tickets merge into the integration branch one at a time: each merge must pass the full local gate (tests, clippy, generated documents, extended verification with committed evidence, secret scan) before the next merge starts. Reason: one shared machine, and the evidence files are keyed by a digest of the whole source tree, so two merges cannot produce them in parallel. After the last ticket an adversarial reviewer that did not write the code approves a named commit before the integration branch is merged to main. That reviewer breaks each claimed rule in its own checkout and sees the named test go red.
- **Decisions in this stage that change the design agreement.**
  - D1.5 and D1.7 (rules judged "in a fresh snapshot"; "while the snapshot is stale, daily loss and MM buffer are not evaluated"): changed. Every accepted report is judged once, also one that is already older than 30 s when it arrives. Reason: its values were computed by the venue and are the newest the OMS has, and entering REDUCING is the safe side. Freshness gates only what releases risk: accepting a plain order, `clear-reduce` and `rebase-day`. The non-guarantee is now "no rule is judged while no new report is accepted". Planned tests in tickets 04 and 07 pin this.
  - D3.1 (hold table) against D1.8 (no margin policy = behaviour unchanged): D3.1 is narrowed. The table applies only when a margin policy is set, so D1.8 holds with no exception. Ticket 01 therefore waits for ticket 02, which adds the policy section. Alternative refused: the table always applies (it would change the result of re-deciding journals recorded before this work).
  - D9.1 (a fresh adversarial reviewer with mutation checks on every PR): changed. The work is one integration branch with one pull request, not one pull request per slice. A ticket merge does not get its own adversarial reviewer. It gets the full local gate, and its acceptance criteria name the mutants that its tests must catch. The adversarial reviewer with mutation checks runs at two points: on this spec stage, and on the whole integration branch after the last ticket and the code-review skill. Its approval names a commit; a push after it voids it; the merge to main is a merge commit. Reason: the owner chose the to-spec, to-tickets, implement-spec flow with local tickets (2026-11-16).
- **Delegated decisions.** The owner delegated every "confirm with the user" point of the skills to the Fable·Claude grill (2026-11-16). Recorded here with the alternative:
  - Tracker: local markdown (alternative: GitHub Issues; refused by the owner's instruction "티켓은 md에만 기록").
  - Agent instructions file: neither `CLAUDE.md` nor `AGENTS.md` existed; `AGENTS.md` was created (alternative: `CLAUDE.md`).
  - Seams: the four existing seams (alternative: core only; see Testing Decisions).
  - Ticket breakdown: 12 tickets (alternative considered: the earlier 7-part table by rule; refused because two of its parts held 18 and 22 tests and do not fit one context window).
- **Non-guarantees the owner must know.** The realised daily loss can exceed 5 %: the rule acts on the next report after the loss. Example: reports at 100 and then 88 five seconds later; REDUCING is entered at 88, with 12 % already lost. A rule cancel can lose the race to a fill. Nothing is cancelled while the link is down. A price move above 2x latches nothing. The MM buffer fires 0.13–0.37 % of price before liquidation (BTC, venue 2x) and a 5 s poll can step over it. A false MM-buffer latch cannot be cleared while its condition reads true. A deposit without a rebase hides a loss of its size for that day. An unflagged take-profit is cancelled by REDUCING and by a stale cancel. A strategy cannot reduce an unattributed position through the OMS; the operator closes it at the venue and corrects the ledgers. While a wall clock stepped backwards has not caught up, time-based behaviour stands still.
- **Test gaps accepted only in part, with the reason.**
  - G7, cancel burst "must respect the existing send limits": the existing gateway never delays a cancel by its local budget, on purpose (a cancel lowers risk). This is kept. The test pins the consequence (queries wait for the refill) and does not add a limit on cancels.
  - G8, soak: done in virtual time at the simulation seam. A wall-clock soak of the node process over several real days is not done; no node is run for days in this work. "Journal growth bounded" is tested as a bound on the number of margin records, not on disk bytes; the journal is not pruned (U-04).
  - G10, fees and funding: the simulation charges them to account value. Which balance the venue takes them from (T3) is not verified and can reopen O1.
  - G11: not a test in this work; see Out of Scope.
- **Staged behaviour.** Until ticket 09 lands, the Hyperliquid gateway answers the new queries with nothing. With a margin policy set this reads as stale or unknown and refuses plain orders, the safe side. With no margin policy nothing changes.
- **Planned test names** in the tickets may be renamed by an implementer. The ticket file and the Korean scenario table are then updated in the same change.
- **Documents not changed in this stage**: ADR-008 §7 (amended by ticket 01), the README status paragraph, the scope and verification documents, the runbook (updated by the tickets that make the behaviour real).
