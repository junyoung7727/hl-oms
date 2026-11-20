# Spec: journal tail reader and the per-event field table (unit U14a)

Status: ready-for-agent

Design source: `RESEARCH/CRYPTO_TRADING_INTEGRATED_TN_20261118/10_DESIGN_DRAFT.md` §5.4.5, §7.5, §8.2, and `pr_plan.json` unit U14 (outside this repository). U14 is split: U14a this spec (library and contract), U14b the `oms-projector` binary (metrics, log lines, cursor file, dead-OMS mode, ADR on observability replacing ADR-016 §8).

## Problem Statement

The projector and the watcher must read the journal while the OMS writes it, without taking `LOCK`, without exporting a record that is not durable yet, and (the watcher) without depending on OMS code. Only `recover` (takes the lock) and `verify_all` (reads everything once) read journals today, and nothing documents every event field.

## Solution

- `oms_journal::Tail`: `new(dir, after_seq)`, `read(durable_seq, max) -> Vec<TailRecord>`, `peek_beyond(max)`, `next_seq()`. `TailRecord { seq, record_schema, payload }`. Follows rotation; re-reads the header of a segment it has read no frame from; a damaged frame at or below the cap is an error after the good records before it, and the position does not move; `peek_beyond` never moves and stops without error at the first invalid frame.
- `docs/contracts/tail.md` (C-TAIL): the tail rules, the record format, a table of which events change the open-order, fill and position sets, and the per-event field table between markers.
- Tests that keep the table equal to the encoder: every field path of every event kind (all optional fields set) is in the table, and every row is written.

## Decisions

- ADR number 025 (021–024 are taken). The observability ADR of U14b is ADR-026.
- `config_activated.config` and enum values are one row each ("object …" / "enum …"); the check does not look inside them.
- The watcher's position sum is fills + external fills + venue position changes + adjustments (ADR-023); funding changes no quantity.

## Testing Decisions

`crates/oms-journal/tests/tail.rs` (5), `crates/oms-core/tests/event_fields.rs` (2), both new and rustfmt-clean. Claims V-TAIL-1, V-TAIL-2; contract C-TAIL in `registry/contracts.toml`.

## Out of Scope

The projector binary, VictoriaMetrics/VictoriaLogs output, the cursor file, `/proc/locks` liveness (U14b).
