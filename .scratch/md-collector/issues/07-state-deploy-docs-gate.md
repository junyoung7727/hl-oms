# 07: state, deploy templates, docs, full gate

**What to build:** `CollectorHandle::state()` (spec D10), `deploy/systemd/` templates (D11), `docs/operations/md-collector.md` (config schema, commands, runbook notes), `docs/03-architecture.md` crate rows (md-feed, md-collector), GLOSSARY entries, `cargo run -p xtask -- docs`, the full local gate of the plan entry.

**Blocked by:** 05, 06

**Status:** ready-for-agent

- [ ] Test `state_counts`: after the end-to-end run the snapshot shows frames written per connection, instrument fetches, and zero overflows.
- [ ] Test `no_non_loopback_socket`: every test config sets `loopback_only` (a test greps the test sources' configs, as a guard).
- [ ] Full gate of the plan entry passes.
