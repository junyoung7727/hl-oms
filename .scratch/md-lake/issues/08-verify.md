# 08: Verify

**What to build:** `verify(world, venue, date)` producing the verify result of 5.2. Spec: "Day seal and verify".

**Blocked by:** 07

**Status:** ready-for-agent

- [ ] Test `verify_matches_the_golden_counts`: for files built from the golden segments, the JSON validates against `xrepo/verify_result.schema.json` and the counts and first/last `recv_ns` match the golden verify result.
- [ ] Test `verify_reports_framing_errors`: a corrupted frame and a truncated file are reported, not hidden.
