# 01 WireIdForm and the gateway's WIRE_ID

Status: ready-for-agent

Spec: `../spec.md`.

## What to build

`WireIdForm { Hex128, Base32Pair }` in `crates/oms-core/src/ids.rs` with `encode` and strict `decode`; `const WIRE_ID: WireIdForm` on `VenueGateway` (both gateways `Hex128`); `NodeInfo.wire_id`, used by the status API for `venue_client_id`. ADR-022, C-ID and C-GW rows, glossary, claim.

## Acceptance criteria

- [ ] Tests listed in the spec pass; no existing test edited.
- [ ] Full local gate passes.
