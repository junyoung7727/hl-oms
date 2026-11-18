# 06: REST snapshot then replay

**What to build:** md-core additive `snapshot_request` and `parse_snapshot` (defaults), `RestClient` (reqwest, loopback guard, body to the sink as `src` rest, `conn_id` 0), buffer deltas while syncing, the bridge rule, retry once then reconnect, initial sync without a gap event. A test-only adapter (`SeqRule::PrevEquals`, `RestSnapshotThenReplay`). Spec: "Adapter trait, additive only" (bridging), "Gap handling".

**Blocked by:** 03

**Status:** ready-for-agent

- [ ] Test `initial_rest_sync_no_gap_event`: deltas before any snapshot start a REST sync; no `Gap` event; buffered deltas after the snapshot are replayed; book equals the fake's book.
- [ ] Test `gap_rest_resync_book_equals_fake`: a `pu` break gives one `Gap` (RestSnapshotThenReplay), the REST body is in the sink (`src` rest, `conn_id` 0), the book after replay equals the fake's.
- [ ] Test `non_bridging_snapshot_retries_then_reconnects`.
