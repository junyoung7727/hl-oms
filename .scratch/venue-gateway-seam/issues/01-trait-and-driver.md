# 01 VenueGateway trait, driver, both gateways behind it

Status: done

Spec: `../spec.md` D1–D6.

## Acceptance

- [x] fake-gateway tests (D5 1–3) written first and failing to build before the trait exists
- [x] trait + `venue::start`; simnet and Hyperliquid call it; no protocol code changed
- [x] ADR-021, decisions.toml, ADR index, architecture, gateway contract, GLOSSARY, module comment, claim V-GW-SEAM-1, docs/generated
- [x] full gate passes; `git diff origin/main -- crates/*/tests` shows no change in existing tests

## Comments

- Done on branch venue-gateway-seam; final review 1 MUST (claim test ids) + 2 SHOULD fixed. Merge commit recorded in the PR.
