# 02: obs-push crate

**What to build:** Spec D2: VM and VL line formats, bounded drop-oldest queues, batching, retry with backoff, the sender thread, self metrics.

**Blocked by:** none

**Status:** done

- [x] Test `vm_line_matches_documented_example`: the V17 example (`up`, two labels, values, timestamps) is reproduced byte for byte by our encoder for one sample; labels sorted; NaN/inf not pushed and counted `invalid`.
- [x] Test `vl_line_and_stream_fields`: the line has `_time` (RFC 3339, ns, `Z`), `_msg`, `service`, `host`, `venue` and the fields; the request query has `_stream_fields=service,host,venue`.
- [x] Test `queue_drops_oldest_and_counts`: queue 10, push 15 with the store dead → the 5 oldest dropped, `dropped = 5`; store back → the 10 newest arrive in order, and `obs_push_dropped_total` reaches the store.
- [x] Test `retry_keeps_batch`: fake answers 500 twice then 204 → every line arrives once, in order.
- [x] Test `push_never_blocks`: with a fake that never answers, 100,000 pushes return within 200 ms in total.
- [x] (review R11) Test `permanent_reject_drops_the_batch` (400 drops the batch, counted `rejected`, later lines still go) and `priority_goes_first`.
- [x] Test `reserved_field_names_do_not_overwrite`: a field named `_msg` or `venue` is written as `fields._msg` (judgement, spec D2).
