# 09: Memory bounds of the seal

**What to build:** A child-process seal runner for tests (Seam C) that reports `ru_maxrss`, and a synthetic hour generator. Spec: "Memory tests".

**Blocked by:** 04

**Status:** ready-for-agent

- [ ] Test `seal_rss_does_not_grow_with_input`: 50 MB and 200 MB hours (200 topics, budget of 4 contexts) → peak RSS differs by ≤ 64 MB.
- [ ] `#[ignore]` test `seal_rss_2gb`: ≥ 2 GB, 200 topics, 0.5 GB budget → peak RSS ≤ 1 GB; run in release in this unit's checks; numbers in the PR.
