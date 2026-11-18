# 10: Documents and final gate

**What to build:** `docs/03-architecture.md` crate table gains `md-lake`; `GLOSSARY.md` gains the L0 terms used in code (open file, hour seal, day seal, session run, seal log); `cargo run -p xtask -- docs` regenerated.

**Blocked by:** 06, 07, 08, 09

**Status:** done

- [x] No file is written outside the lake root in any test (the real `Fs` refuses it; a test checks the refusal).
- [x] Full local gate passes, plus `cargo test --release -p md-lake -- --ignored seal_rss_2gb`.
