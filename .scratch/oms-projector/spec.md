# Spec: oms-projector (unit U14b)

Status: ready-for-agent

Design source: `RESEARCH/CRYPTO_TRADING_INTEGRATED_TN_20261118/10_DESIGN_DRAFT.md` §5.4.5, §8.2, and `pr_plan.json` unit U14 (outside this repository). U14a (PR #16) gave the `Tail` and C-TAIL. Decision record: ADR-026 (replaces ADR-016 §8 "no external collector").

## Problem Statement

Nothing outside the OMS turns the journal into metrics and event logs. The operator cannot see positions, PnL, limit distances or the order/fill timeline in VictoriaMetrics/VictoriaLogs, and nobody can see what was on disk when the OMS died.

## Solution

Crate `crates/oms-projector` (library + binary `oms-projector --config <file>`). Each step: poll `/api/status`, `Tail::read` up to `journal.durable_seq`, `Core::replay`, push metrics and one VL line per event through obs-push, move an fsynced cursor file when the VL endpoint has processed the record's lines. Dead OMS (no answer ≥ `dead_after_ms` and no holder of `LOCK` in `/proc/locks`): lines past the durable sequence with `unconfirmed=true`, emitted again without the flag once durable.

## Decisions

- Cursor follows log lines only; metrics are gauges recomputed each step.
- Only a status answer whose `core.instance_id` equals the journal's `INSTANCE` is used (review MUST, 2026-11-20).
- Processed = `sent + rejected + dropped` of the VL endpoint (only projector lines go there). `rejected` is permanent: counted as processed, logged, `projector_rejected_lines`.
- Read records wait in memory; they are pushed in order while pushed-not-taken lines ≤ `max_pending_lines` (one record alone may pass); no reading while waiting lines ≥ it. Config requires `queue_lines ≥ 2 × max_pending_lines`.
- A rise in `dropped`: confirm nothing, push nothing until the queue is empty, then push every pending record again within the same limit.
- Replay from record 1 at start (snapshot payload is an oms-node type), up to the cursor for 0.3 s per step; no `oms_*` state series until the replay first reaches the read limit (`projector_caught_up`).
- PnL by average cost in the projector (OMS books have no cost basis).
- `/proc/locks` matched by inode only (btrfs device numbers differ; a false "held" only hides unconfirmed lines).
- `oms_rate_tokens` left out (U17).
- No change to oms-node or oms-core.

## Testing Decisions

`crates/oms-projector/tests/projector.rs` (17): exact VL and VM line shapes against loopback fakes; nothing past the durable cap while the lock is held; dead OMS unconfirmed lines then confirmed re-emission across rotated segments; crash resume from the cursor with the same seqs; refusing store keeps the cursor; CRC damage stops without moving the cursor; the projector never takes the lock (a writer can lock while it runs; real `/proc/locks` probe); `/proc/locks` parsing; HTTP status source; foreign cursor refused; config refusals; dropped lines re-sent with one rewind; another instance's answer ignored; no state series before catch-up; replay to the cursor in one step; limit distances with an open order and a margin report; venue change and adjustment in the average cost. `src/pnl.rs` (2). Eleven deliberate faults each caught by a test (ADR-026 §9).

## Out of Scope

vmalert rules, Grafana dashboards, systemd units (U20); watcher (U19a); rate tokens (U17); any venue call or order.
