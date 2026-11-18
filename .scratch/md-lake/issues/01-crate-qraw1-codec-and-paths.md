# 01: md-lake crate, QRAW1 codec and paths

**What to build:** Crate `md-lake`: `Record`, header (three forms), encoder and decoder of QRAW1 content (contract 2.2; records never cross a segment, truncated tail reported, `src` > 2 and `len` > 16,777,216 are framing errors), path building and parsing for every kind of 2.1 with the field patterns and integer limits of `l0/paths.json`, and the test-dir helper (`OMS_TEST_DIR`, else `CARGO_TARGET_TMPDIR`). Spec: "Solution" 1.

**Blocked by:** None

**Status:** done

- [x] Test `golden_segments_round_trip_byte_exact`: every golden L0 file decodes; re-encoding its header and records gives the same bytes, segment by segment.
- [x] Test `framing_errors_and_truncated_tail`: a record across a segment end, `src` 3, `len` over the maximum and a header longer than the segment are errors; a cut inside the last record gives the complete records plus `truncated_tail`.
- [x] Test `paths_build_and_parse_every_kind`: each kind of `paths.json` builds and parses back; `conn_id` 2^32, `session_ns` 2^63, a `..`, a `-` in a collector name, a leading `_` in a symbol and 2026-02-30 are refused.
- [x] Test `golden_paths_parse`: every golden L0 `l0_file_id` parses to its kind and fields.
