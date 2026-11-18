# 01: md-core crate, normalized types and the vendored lake contract

**What to build:** A workspace crate `md-core` with the normalized message types (`StreamKey`, `Level`, `BookSnapshot`, `BookDelta`, `Trade`, `Ticker`, `Control`, `Normalized`), exact decimal-string-to-integer conversion, and the vendored lake contract under `contracts/lake/` with `VENDOR.toml` and its check. Prefactoring ticket: later tickets use these types. Spec: "Solution" 1 and 6, "Implementation Decisions".

**Blocked by:** None (needs the qdp main commit of U1)

**Status:** done

- [x] Test `decimal_string_converts_exactly_or_fails` (seam A): `"42000.1"` with exp −1 gives 420001; `"0.010"` with exp −3 gives 10; `"42000.15"` with exp −1, a value beyond `i64`, an empty string and `"1e3"` are errors.
- [x] Test `exponent_is_minus_the_decimal_places_after_trailing_zeros` (seam A): `"0.10"` → −1, `"5"` → 0, `"0.001"` → −3.
- [x] Test `vendored_contract_files_match_the_index_and_the_record` (seam B): every file listed in `contracts/lake/index.json` has its SHA-256; `index.json` has the SHA-256 in `VENDOR.toml`; no file under `contracts/lake/` is missing from the index (except `index.json`, `.gitattributes`, `VENDOR.toml`).
- [x] Test `vendored_contract_check_fails_when_one_byte_changes` (seam B): on a temp copy, flipping one byte of one file makes the check report that file.
- [x] `md-core` has no dependency outside the standard library (dev-dependencies only); no `oms-*` crate changes.
- [x] Full local gate passes.
