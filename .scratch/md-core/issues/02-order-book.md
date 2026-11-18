# 02: Depth-agnostic order book

**What to build:** `OrderBook` applies a `BookSnapshot` (replaces both sides) and a `BookDelta` (sets or deletes levels; quantity 0 deletes), keeps an optional max depth per side, and exposes `best_bid`, `best_ask`, `spread_bp`, `mid`, `mid_x2` (exact `bid + ask`), `depth_sum(side, n)`, `levels(side)` (best first) and `is_crossed`. Spec: "Solution" 2.

**Blocked by:** 01

**Status:** done

- [x] Test `snapshot_then_deltas_give_the_hand_computed_book` (seam A): a 3-level snapshot and two deltas give the literal levels, best first.
- [x] Test `zero_quantity_deletes_a_level_and_an_unknown_zero_is_ignored` (seam A).
- [x] Test `max_depth_keeps_the_best_levels_only` (seam A): max depth 2 after a snapshot of 4 levels and after a delta that adds a better level.
- [x] Test `best_spread_mid_and_depth_sums_match_literals` (seam A): bid 420000, ask 420002 → spread_bp = 2/420001×10⁴, mid 420001, mid_x2 840002; depth_sum of 2 levels; empty side gives `None`.
- [x] Test `a_new_snapshot_replaces_the_whole_book` (seam A).
- [x] Full local gate passes.
