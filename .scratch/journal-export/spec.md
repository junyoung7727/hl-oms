# Spec: oms-export (unit U15)

Status: ready-for-agent

Design source: `RESEARCH/CRYPTO_TRADING_INTEGRATED_TN_20261118/10_DESIGN_DRAFT.md` and `pr_plan.json` unit U15 (outside this repository). Lake contract section 4 (vendored under `contracts/lake/`, tables `trading/*.table.json`). C-TAIL for the journal side. Decision record: ADR-027.

## Problem Statement

Research cannot join our own orders and fills to the market data in the lake. The journal holds them, but only as JSON frames that need the OMS crates to read. The lake contract defines three trading tables (`orders`, `fills`, `events`) and nothing writes them.

## Solution

Crate `crates/oms-export` (library + binary `oms-export --config <file>`). One run:

1. Ask `/api/status` for the read limit, like the projector (`oms_projector::probe`): use only an answer whose `core.instance_id` equals the journal's `INSTANCE`. No usable answer → write nothing, exit 3 ("no read limit"). The OMS must run (or be started once) for the last records to be exported.
2. Read the journal from record 1 to the limit with `Tail` (no lock, never writes), schemas 1 and 2.
3. Map each record to rows (below). Keep the order info (`order_accepted`) for later fills and events.
4. A UTC date is complete when the limit record's `at` is on a later date. For each complete date, build the three files in memory. Write a file only when its bytes differ from the file on disk: temp file → fsync → rename → dir fsync. A date with no rows for a table gets no file for that table.
5. Print one JSON summary line (dates written, unchanged, rows per table, limit seq).

Re-running gives the same bytes (deterministic writer, no time or host in the file). The journal is never touched.

## Mapping (contract 4.1–4.3)

- `venue` and `market` come from the config (one OMS = one venue; Bybit `linear`). `symbol` = the journal `instrument`.
- `orders`: one row per `order_accepted`. `order_id` = decimal string of the u64. `side` buy +1, sell −1. `tif` gtc → `GTC`, ioc → `IOC`, alo → `PostOnly`. `px` = `request.px` (never null: the OMS has no market orders). `wire_id` = `WireIdForm::encode(VenueClientId{instance_id, order_id})` with the form from the status answer (`node.wire_id_form`); null when the answer has no form. `market_ref_*` from `request.market_ref`, all null when absent. `record_schema` = the record's schema.
- `fills`: one row per `fill_booked` (the order is known from its `order_accepted`). `exec_id` = `key`. `side` and `symbol` from the order. `venue_ts_ns` = `venue_ts` × 1000. `is_maker` and `recv_ns` are null: the journal does not hold them (U16 adds them to `FillIn` and `FillBooked`). `external_fill_booked`, `fill_parked` and venue position changes are not fills of a known order; they appear only in `events`.
- `events`: one row per event, every kind. `kind` = `Event::name()`. `order_id` = `Event::order_id()`. `symbol` = the event's own instrument field if it has one, else the order's instrument; `venue` is set exactly when `symbol` is. `payload` = the event's field object as JSON, keys sorted at every level, no spaces.
- `at_us` = the record `at` for every row of the record. Date = UTC date of `at_us`.

## Contract change (qdp, U15a, before this unit)

The vendored contract (draft version 1; no real L0 file exists yet, so draft rules apply) needs:
- `fills.is_maker` and `fills.recv_ns` nullable (journals before U16 do not have them).
- Golden trading rows that a real journal produces: real event names, numeric order ids, real wire ids, `at_us` of a fill equal to its record's (the current golden fill rows have `…300500` while their record's events have `…300000`).
The golden rows are produced by this unit's fixture journal, checked by hand against the contract text, committed to qdp with a change-list entry, then re-vendored here. The test here compares the export of that fixture to the vendored golden rows.

## Decisions

- Read limit only from the matching OMS status answer (same rule and code as the projector). Sealed-segment-only export was rejected: segments are 64 MiB, so export would lag by days.
- Full re-scan from record 1 each run (needed for order info of fills in later days). Small journal (judgment); re-check when a run exceeds 60 s.
- Complete dates only. The open date is not written, so a reader never sees a partial day under the final path.
- Write only changed files; an `export_ver` rise rewrites every date by itself.
- Parquet writer reused from `md-archiver` (`parquet_out`), extended with a constructor that takes any contract table schema and the metadata list. No second writer, one zstd build.
- `export_ver` = `oms-export-1`.

## Testing Decisions

`crates/oms-export/tests/export.rs`: fixture journal → export → read the Parquet back → equals the vendored golden rows (all three tables); metadata `contract_ver`, `export_ver`, `table`; rerun leaves bytes and mtimes unchanged; open date not written; nothing past the read limit; another instance's answer refused (exit 3, no files); `alo` → `PostOnly`; schema-1 and schema-2 records; fill of an order accepted on an earlier date; external fill only in events; the journal directory unchanged (bytes) and its lock never taken. Writer unit tests in md-archiver for the new constructor.

## Out of Scope

`is_maker`/`recv_ns` capture (U16); systemd timer (U20); any venue call or order; DuckDB views over the trading tables (qdp, later).
