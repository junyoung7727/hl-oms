# 04: MarketDataAdapter trait and conformance harness

**What to build:** The `MarketDataAdapter` trait of design §6.4 with its plan, route, keepalive, resync and instrument types, and `conformance::run(&adapter, &Fixture) -> Vec<Failure>` that checks: snapshot applied, delta applied, zero-quantity delete, gap detected with the adapter's sequence rule, resync chosen for that gap, `venue_ts_ns` converted, `route` gives the fixture's stream key and never fails on garbage. A fake adapter in the tests (a tiny made-up JSON format) passes; two broken fakes fail. Spec: "Solution" 4 and 5.

**Blocked by:** 02, 03

**Status:** done

- [x] Test `fake_adapter_passes_the_conformance_harness` (seam A).
- [x] Test `harness_fails_an_adapter_that_drops_zero_quantity_levels` (seam A).
- [x] Test `harness_fails_an_adapter_that_ignores_the_sequence_field` (seam A).
- [x] Test `route_never_fails_on_garbage_frames` (seam A): empty bytes, non-UTF-8, JSON without a topic → `Route::Control`.
- [x] Docs: `docs/03-architecture.md` 3.1 crate table gains `md-core`; `GLOSSARY.md` gains the market-data terms used in code. `cargo run -p xtask -- docs` regenerated.
- [x] Full local gate passes (the unit's checks in `pr_plan.json`).
