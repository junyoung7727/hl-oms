# 06: disk floor

**What to build:** The free-space checker of spec D8.

**Blocked by:** 04

**Status:** ready-for-agent

- [ ] Test `disk_floor_drops_and_restores`: an injected free space below the floor stops the other feeds (their connections close, the traded ones stay), one `disk_floor` note per dropped topic; between floor and floor + margin nothing changes; above floor + margin the other topics are subscribed again on new connections.
