# 06: REST snapshot then replay

**What to build:** md-core additive `snapshot_request` and `parse_snapshot` (defaults), `RestClient` (reqwest, loopback guard, body to the sink as `src` rest, `conn_id` 0), buffer deltas while syncing, the bridge rule, retry once then reconnect, initial sync without a gap event. A test-only adapter (`SeqRule::PrevEquals`, `RestSnapshotThenReplay`). Spec: "Adapter trait, additive only" (bridging), "Gap handling".

**Blocked by:** 03

**Status:** done

- [x] Test `initial_rest_sync_no_gap_event`: deltas before any snapshot start a REST sync; no `Gap` event; buffered deltas after the snapshot are replayed; book equals the fake's book.
- [x] Test `gap_rest_resync_book_equals_fake`: a `pu` break gives one `Gap` (RestSnapshotThenReplay), the REST body is in the sink (`src` rest, `conn_id` 0), the book after replay equals the fake's.
- [x] Test `non_bridging_snapshot_retries_then_reconnects`.
- [x] Test `gap_inside_the_buffer_keeps_the_later_deltas` (found two bugs: the delta that showed the gap, and the deltas after a gap inside the buffer, were dropped).
