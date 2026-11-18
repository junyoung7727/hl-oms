# 08: Docs and final gate

**What to build:** Crate docs, `GLOSSARY.md` terms if new (market_ref, stale), `cargo run -p xtask -- docs`, the full U5a checks from `pr_plan.json`, PR.

**Blocked by:** 01–07

**Status:** ready-for-agent

- [ ] Full gate passes (secret scan, selftest, build, tests, failpoints, clippy, docs diff, rustfmt on added files).
- [ ] No test opens a non-loopback socket (all test URLs are `127.0.0.1`; `loopback_only` set in every test config).
