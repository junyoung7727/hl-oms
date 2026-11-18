# 06: Layout by_connection and measure_layouts

**What to build:** `by_connection` seal (each open file whole into one level-9 frame of its connection's day file) and the `measure_layouts` option. Spec: "by_connection".

**Blocked by:** 05

**Status:** ready-for-agent

- [ ] Test `by_connection_is_lossless`: records and bytes per connection unchanged; `sessions` null.
- [ ] Test `crash_at_every_step_by_connection` (as in ticket 05).
- [ ] Test `measure_layouts_records_both_sizes` and writes no extra file.
