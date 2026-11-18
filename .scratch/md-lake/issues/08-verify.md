# 08: Verify

**What to build:** `verify(world, venue, date)` producing the verify result of 5.2. Spec: "Day seal and verify".

**Blocked by:** 07

**Status:** done

- [x] Test `verify_matches_the_golden_counts`: for files built from the golden segments, the JSON validates against `xrepo/verify_result.schema.json` and the counts and first/last `recv_ns` match the golden verify result.
- [x] Test `verify_reports_framing_errors`: a corrupted frame and a truncated file are reported, not hidden.

## Comments

- Test-first (the module did not exist). The golden files of 2026-01-02 are rebuilt as zstd files (one frame per golden segment); frames, records, first/last recv_ns, framing_errors and error match the golden verify result; bytes and sha256 are of our files. Mutation checked: not reporting a file that ends inside a frame fails a test.
