# 03: Gap detection and Resubscribe

**What to build:** md-core additive trait method `resubscribe_frames` (default empty), md-bybit implementation (unsubscribe + subscribe of the topic), gap → one `Gap` event, `resyncing`, resubscribe on the same connection, resync timeout → reconnect, empty `resubscribe_frames` → reconnect. Spec: "Adapter trait, additive only", "Gap handling".

**Blocked by:** 02

**Status:** ready-for-agent

- [ ] Test `missing_u_gives_one_gap_and_resync`: fake skips one `u` then sends more deltas; exactly one `Gap` event (with `Resubscribe`); the fake receives the unsubscribe and subscribe frames; after its new snapshot + deltas the book equals the fake's book.
- [ ] Test `resync_timeout_reconnects`: fake ignores the resubscribe; after `resync_timeout` the connection is replaced (new `conn_id`).
- [ ] md-bybit unit test of `resubscribe_frames`.
