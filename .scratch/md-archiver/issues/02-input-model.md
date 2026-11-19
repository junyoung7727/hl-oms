# 02: input model: day-sealed dates, connections, instruments in effect

**What to build:** Spec D2 (day-sealed check, has-L1 check, carry-in rule, by_connection refusal), D3 (manifest lines, sessions → connection, pass 1, instrument in effect), and the test helper that builds a sealed lake from the golden L0 segments.

**Blocked by:** 01

**Status:** ready-for-agent

- [ ] Test helper `golden_lake(dir)`: day files from golden segments (one zstd frame per segment), manifest lines with `sessions` from `session_runs.json`, sidecars, plus the synthetic 2026-01-01 instruments response.
- [ ] Test `day_sealed_rules`: a `.partial` file, an open file, a sealed file without a manifest line or sidecar, a size mismatch, a cut manifest line each make the date not day-sealed; the golden dates are day-sealed.
- [ ] Test `record_connections_from_sessions`: every record of the golden order book day file of 2026-01-02 gets the session of `session_runs.json`; the restart date gives three connections with `conn_id` 1.
- [ ] Test `pass1_lifetimes`: first/last `recv_ns` per connection include control records of the collector's session file; the trade-only connection of session 2 carries only `publicTrade`.
- [ ] Test `instrument_in_effect`: latest at or before; earliest of the window when none before; none in D−7…D → parse error; a response of D+1 is never used.
- [ ] Test `refusals`: `by_connection` manifest line and `contract_ver` 2 → refusal error.
