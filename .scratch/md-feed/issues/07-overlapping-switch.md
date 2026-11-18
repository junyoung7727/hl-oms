# 07: Overlapping connection switch

**What to build:** `FeedHandle::switch(conn_index)`, the second connection with its own books, activation when every book topic has a snapshot, closing the old one, `SwitchDone`/`SwitchFailed`. Spec: "Overlapping switch".

**Blocked by:** 02

**Status:** ready-for-agent

- [ ] Test `switch_keeps_both_until_old_closed`: during the overlap the sink gets frames of both `conn_id`s; after `SwitchDone` none of the old one; `read_book`/`market_ref` come from the new connection.
- [ ] Test `switch_fails_without_snapshot`: the new connection never sends a snapshot → `SwitchFailed`, old connection still active and delivering.
