# 07: docs, deploy template, claims, full gate

**What to build:** Docs for the new config sections and metrics (`docs/operations/md-collector.md`), template changes (`Type=notify`, `WatchdogSec=30s`, `--telegram` for the seal), claims for the new tests, GLOSSARY entries, `cargo run -p xtask -- docs`, the full local gate.

**Blocked by:** 05, 06

**Status:** ready-for-agent

- [ ] Docs list every metric with labels and period, the alert texts, the repeat rule and the token file rules.
- [ ] Claims name only tests that exist; docs/generated regenerated.
- [ ] Full local gate (pr_plan U6 checks) passes.
