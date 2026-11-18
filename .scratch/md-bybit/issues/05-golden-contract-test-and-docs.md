# 05: Golden contract test and documents

**What to build:** A test-only QRAW1 reader (`tests/common/`) and a test that parses every Bybit record of the vendored golden L0 files and compares the result with the golden L1 rows. Docs: architecture crate table and glossary. Spec: "Testing Decisions", seam B.

**Blocked by:** 02, 03, and qdp PR #29 merged (re-vendor `contracts/lake/` from its main commit first)

**Status:** ready-for-agent

- [ ] Test `golden_l0_frames_give_the_golden_l1_rows`: for every row of `golden/l1/book_msg.json` with kind 0 or 1, `trade.json`, `ticker.json` and `instrument.json`, the record `(l0_file_id, rec_no)` parsed by md-bybit gives a message equal on every md-core field (trades by `msg_idx`); lineage columns are not compared. Each parsed record gives exactly as many messages as there are golden rows naming it (trade records whose rows were dropped as duplicates at a handover still give their full trade count). The golden gap table has no `parse_error` row, so every WebSocket and REST record of the golden topic files must parse without error.
- [ ] Test `golden_reader_refuses_a_torn_record` (the test-only reader must not hide a short file).
- [ ] Docs: `docs/03-architecture.md` crate table gains `md-bybit`; `GLOSSARY.md` gains the Bybit terms used in code; `cargo run -p xtask -- docs` regenerated.
- [ ] Full local gate passes (the unit's checks in `pr_plan.json`).
