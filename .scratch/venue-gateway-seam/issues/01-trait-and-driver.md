# 01 VenueGateway trait, driver, both gateways behind it

Status: ready-for-agent

Spec: `../spec.md` D1–D6.

## Acceptance

- [ ] fake-gateway tests (D5 1–3) written first and failing to build before the trait exists
- [ ] trait + `venue::start`; simnet and Hyperliquid call it; no protocol code changed
- [ ] ADR-021, decisions.toml, ADR index, architecture, gateway contract, GLOSSARY, module comment, claim V-GW-SEAM-1, docs/generated
- [ ] full gate passes; `git diff origin/main -- crates/*/tests` shows no change in existing tests
