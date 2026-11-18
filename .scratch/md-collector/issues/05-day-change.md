# 05: day-change overlapping switch, staggered

**What to build:** The scheduler of spec D7 on the injected wall clock.

**Blocked by:** 04

**Status:** done

- [x] Test `day_change_times_default`: midnight + 5 s, + 7 s, + 9 s … for the defaults.
- [x] Test `day_change_switches_in_order`: with the clock a little before midnight, each slot gets a new connection in the configured order and stagger; each new connection connects before the old one closes; its snapshot and the deltas after it are in L0; the old connection's frames up to its close are in L0.
