# 06: Layout by_connection and measure_layouts

**What to build:** `by_connection` seal (each open file whole into one level-9 frame of its connection's day file) and the `measure_layouts` option. Spec: "by_connection".

**Blocked by:** 05

**Status:** done

- [x] Test `by_connection_is_lossless`: records and bytes per connection unchanged; one header, at the start of the day file only (F2); `sessions` null.
- [x] Test `crash_at_every_step_by_connection` (as in ticket 05).
- [x] Test `measure_layouts_records_both_sizes` and writes no extra file.

## Comments

- Test-first: the by_connection tests failed before the branch existed. by_connection does not classify (2.5 step 4), so it has no unroutable guard and its sealer frame holds notes only.
- `measure` gives the layout used as the bytes it appended and the other layout as the compressed bytes of a counting sink (no file). Mutation checked: a header in every frame fails the lossless test.
