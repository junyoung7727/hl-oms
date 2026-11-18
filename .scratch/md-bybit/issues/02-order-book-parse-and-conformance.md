# 02: Order book parsing, sequence rule and the conformance harness

**What to build:** `parse` for `orderbook.*` topics, `seq_rule` (`PlusOne` for books, `None` otherwise) and `on_gap` (`Resubscribe`). Spec: "Solution" 2–3, "Book parse", "Snapshots rebase".

**Blocked by:** 01

**Status:** done

- [x] Test `bybit_adapter_passes_the_conformance_harness`: a synthetic `orderbook.50.BTCUSDT` fixture (snapshot, deltas with a zero delete, a delta skipping one `u`).
- [x] Test `missing_u_gives_one_gap_and_resubscribe`: snapshot u=100, deltas 101, 103, 104 → exactly one gap (101 → 103), `on_gap` = `Resubscribe`.
- [x] Test `every_snapshot_starts_a_new_run`: after deltas up to u=500, a snapshot with u=1 then a delta u=2 gives no gap; after deltas up to u=500, a snapshot with u=480 then a delta u=481 gives no gap; the book equals the new snapshot plus its deltas.
- [x] Test `inexact_or_mismatched_book_frames_are_refused`: a price with more decimals than the tick, a quantity beyond i64, a `data.s` that differs from the topic, an unknown symbol → `ParseError`.
