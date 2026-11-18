# 01: md-feed crate, connection driver and frame sink

**What to build:** Crate `md-feed` with `Feed::start`, `FeedConfig`, `ConnIds`, `FrameSink`/`RawFrame`, the per-connection task (connect, subscribe frames, read loop stamping `recv_ns`, `frame_no` from 0, events `Connected`/`Disconnected`), the loopback guard, and the test fakes (`tests/common/`: scripted WebSocket server, minimal HTTP responder). Spec: "Solution" 1, "conn_id and frame_no", "Loopback guard".

**Blocked by:** None

**Status:** done

- [x] Test `frames_reach_sink_in_order`: two plan slots, each fake connection sends N frames; the sink gets all, `frame_no` 0..N per `conn_id`, distinct `conn_id`s ≥ 1, `src` ws, `recv_ns` non-decreasing per connection in this test.
- [x] Test `subscribe_frames_are_sent`: the fake receives exactly the adapter's subscribe frames, in order, before it sends data.
- [x] Test `conn_ids_shared_across_feeds`: two feeds with one `ConnIds` never repeat a `conn_id`.
- [x] Test `non_loopback_refused`: `loopback_only` with `ws://203.0.113.1:9/` gives `Disconnected` with a refusal reason and no connect attempt (a listener count stays 0 is not observable; assert the reason text and that it is returned within 1 s).
