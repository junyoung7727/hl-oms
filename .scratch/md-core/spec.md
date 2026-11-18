# Spec: md-core, the venue-neutral market-data core (unit U2)

Status: ready-for-agent

Design source: `RESEARCH/CRYPTO_TRADING_INTEGRATED_TN_20261118/10_DESIGN_DRAFT.md` §6.2–6.7 (crypto integrated design TN v1.0, outside this repository). Lake contract: junyoung7727/quant-data-platform `quant_data/crypto/contract/` (unit U1), vendored here. Tickets: `issues/` next to this file. Korean version for the owner: `spec-ko.md`.

## Problem Statement

The OMS will trade on Bybit, and later venues, with market data it receives itself. The collector (U5b), the archiver (U7) and the strategy (§6.7) must read venue messages the same way: one parser per venue, one order book, one sequence rule. Today the repository has no market-data code at all, so every later unit would invent its own types, and the L1 tables the archiver writes could differ from the book the strategy sees.

## Solution

A new workspace crate `md-core` with no I/O. It holds:

1. **Normalized messages**: `BookSnapshot`, `BookDelta` (price and quantity lists, any depth), `Trade`, `Ticker`, `Control`, with integer prices and quantities and per-instrument exponents, the two timestamps (`venue_ts_ns`, `recv_ns`) and the venue's sequence fields. Field names follow the L1 tables of the lake contract.
2. **Order book**: applies snapshots and deltas, deletes a level whose quantity is 0, keeps an optional maximum depth, and gives best bid and ask, spread in basis points, mid, and the quantity sums of the top N levels.
3. **Sequence rules** `PlusOne` (Bybit `u`), `PrevEquals` (Binance `pu`, OKX `prevSeqId`) and `None` (Hyperliquid), and a gap detector that emits a gap event when a delta breaks the rule. A snapshot starts a new run.
4. **The `MarketDataAdapter` trait** of design §6.4: `venue`, `plan`, `subscribe_frames`, `keepalive`, `route`, `parse`, `seq_rule`, `on_gap`, `instruments_request`, `parse_instruments`, `venue_ts_field`.
5. **A conformance harness** that any adapter runs with its own fixture: snapshot applied, delta applied, zero-quantity delete, gap detected, resync chosen, time converted.
6. **The vendored lake contract** under `contracts/lake/`, with the source commit and the SHA-256 of `index.json` recorded in `contracts/lake/VENDOR.toml`, and a test that every vendored file matches the index and the index matches the record.

## User Stories

1. As the developer of a venue adapter (U3, U10, U11), I want one trait and one set of message types, so that the collector and the archiver work with every venue without venue code of their own.
2. As the developer of the collector, I want `route` to never fail, so that an unknown frame still goes to the session file and nothing is lost.
3. As the developer of the archiver, I want the normalized fields to have the names and units of the L1 tables, so that building a row is a copy.
4. As the strategy developer, I want the same order book and gap rule as the archiver, so that what I trade on is what research later replays.
5. As a reviewer, I want each adapter to pass the same conformance tests, so that a new venue cannot skip zero-quantity deletes, gap detection or time conversion.
6. As the owner, I want the vendored contract to be checked byte for byte, so that hl-oms cannot drift from the contract quant-data-platform tests.

## Implementation Decisions

- **Crate boundary.** `md-core` depends on nothing outside the standard library at run time (dev-dependencies `sha2` and `hex`, already in `Cargo.lock`, for the vendored-contract test). It has no network, file, clock or randomness access, like `oms-core` (ADR-002). No `oms-*` crate depends on it in this unit, and no `oms-*` file changes.
- **Integers.** Prices and quantities are `i64` with `px_exp`/`qty_exp` (`i8`), as in the lake contract 1.3, not the OMS's 1e-8 fixed point (ADR-009): market data keeps the venue's tick, and the conversion happens where an order is built. Decimal-string conversion is exact or an error, never rounded. `spread_bp` and `mid` are `f64` because they are monitoring values, not order inputs; the exact form (`bid + ask` as `i128`) is also exposed.
- **Book sides** are ordered maps keyed by price. A max depth keeps the best N levels per side after each update. A crossed book after a delta is not repaired; it is reported by `is_crossed()`.
- **Gap detector** state is per stream: the last update id of the current run. `PlusOne`: a delta with `u != last + 1` is a gap. `PrevEquals`: a delta with `prev != last` is a gap. `None`: never a gap. A delta before any snapshot is a gap (no run). After a gap the run is broken until the next snapshot; further deltas are not reported again.
- **Resync** choices: `Resubscribe`, `RestSnapshotThenReplay`, `Reconnect` (design §6.4).
- **`route`** returns `Route::Stream(StreamKey)` or `Route::Control`; it reads only the topic and never returns an error.
- **Conformance harness** is a public module (`md_core::conformance`) with a fixture struct the adapter fills (frames, expected book, expected gap, expected resync, expected `venue_ts_ns`). It returns a list of failures; adapter crates call it from their own tests.
- **Vendored contract.** `contracts/lake/` holds a byte copy of qdp `quant_data/crypto/contract/` at the merge commit of U1 into qdp main. `VENDOR.toml` records `source_repo`, `source_commit`, `index_sha256`. The test recomputes the SHA-256 of every file listed in `index.json` and of `index.json` itself. Updating the copy is a manual step with a commit that changes `VENDOR.toml`.

## Testing Decisions

- **Seams.** Seam A: the public Rust API of `md-core` (types, `OrderBook`, `GapDetector`, `conformance::run`), called from integration tests in `crates/md-core/tests/`. Seam B: the files under `contracts/lake/`, read by a test.
- **Tests prove** (done criteria): snapshot + delta application; zero-size deletion; max-depth truncation; best price, spread, mid and depth-sum values against hand-computed literals; gap detection for each sequence rule; the conformance harness passes a fake adapter and fails one that skips a zero delete or a gap; the vendored-contract check fails when one byte changes (run on a temp copy).
- **Prior art.** `crates/oms-core/tests/` style: integration tests through the public API, literals as expected values.

## Out of Scope

- Any venue adapter (U3 Bybit, U10 Binance, U11 OKX and Hyperliquid), the network feed (U5a), L0 writing (U4), the archiver (U7).
- Any change to `oms-*` crates, `.github/`, the journal, or registry contracts (no journal event is added).

## Further Notes

- Built directly by Claude Opus 5.5 (2026-11-18, owner instruction "이제 개발을 너가 직접해"); the spec and tickets are lean (owner: "검증 사소한 것까지 하지마").
- The vendored copy is taken from qdp main after PR #28 merges; until then ticket 01 waits for that commit.
