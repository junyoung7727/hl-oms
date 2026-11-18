# 09: Memory bounds of the seal

**What to build:** A child-process seal runner for tests (Seam C) that reports `ru_maxrss`, and a synthetic hour generator. Spec: "Memory tests".

**Blocked by:** 04

**Status:** done

- [x] Test `seal_rss_does_not_grow_with_input`: 50 MB and 200 MB hours (200 topics, budget of 4 contexts) → peak RSS differs by ≤ 64 MB.
- [x] Test `seal_rss_minus_contexts_is_bounded` (F11): 4 topics × 100 MB → `ru_maxrss − max live context bytes ≤ 96 MB`.
- [x] `#[ignore]` test `seal_rss_2gb`: ≥ 2 GB, 200 topics, 0.5 GB budget → peak RSS ≤ 1 GB; run in release in this unit's checks; numbers in the PR.

**Result (Linux PC, release, 2026-11-19, head 94f77a3 after the review fixes):** `seal_rss_2gb`: 2,048 MB of records (200 topics, 0.5 GB budget) → peak RSS 78 MB, live contexts 58 MB, seal 63.6 s (writing the input 22.7 s). Before the fixes (579a817): 77 MB, 58 MB, 66.4 s.
