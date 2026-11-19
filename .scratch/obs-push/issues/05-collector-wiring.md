# 05: collector wiring: push, logs, lake metrics, clock offset, direct alerts

**What to build:** Spec D4 emission, D6, D7, D8, D10, D11 in md-collector; md-lake `unsealed_hours`.

**Blocked by:** 01, 02, 03, 04

**Status:** done

- [x] Test `config_telemetry_and_alert_sections`: defaults, unknown keys, `fast_ms`/`slow_ms` bounds, non-loopback URLs refused with `loopback_only`, `api_base` refused without `loopback_only`.
- [x] Test `collector_pushes_metrics_and_logs` (e2e, fake venue + fake VM + fake VL): every §5.4.5 collector metric name arrives with its labels; `lake_disk_free_bytes` equals the injected free space; `node_clock_offset_ms` equals the injected offset; logs arrive with the stream fields.
- [x] Test `dead_or_slow_store_never_blocks_ingest`: store absent vs store never answering → every sent frame in L0 both times; frame-call p99 < 1 ms; drops counted.
- [x] Test `write_error_sends_one_md_p1`: injected L0 write error with the fake VM down → exactly one `[MD P1]` at the fake Telegram, then the collector reports failed.
- [x] Test `disk_floor_sends_one_md_p1`: injected free space below the floor with the fake VM down → exactly one `[MD P1]` at entry (repeat rule covered in 01), one `해소:` on leaving.
- [x] Test (md-lake) `unsealed_hours_lists_closed_hours_with_open_files` and the collector's `lake_seal_lag_hours` value.
- [x] (R4) `refused_token_file_refuses_start`; `chrony_csv_field_5`.
- Deviation: "frame-call p99 < 1 ms" is not measured; `dead_or_slow_store_never_blocks_ingest` checks instead that every frame the fake sent (1,604) is in L0 with nothing refused, for a store that refuses connections and one that never answers, and obs-push's `push_never_blocks` bounds the push call itself.
- Also fixed: U5b `end_to_end_seal` could pass its wait on subscribe answers before the last data frame (failed once in 6 full runs); it now waits for the fake's sends and a stable count.
- Mutation checks: 7 rules broken on purpose, all caught (one only after the test was strengthened: frames were not counted per topic and the test did not notice).
