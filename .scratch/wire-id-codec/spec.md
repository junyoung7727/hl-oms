# Spec: per-venue wire id form (unit U13a)

Status: ready-for-agent

Design source: `RESEARCH/CRYPTO_TRADING_INTEGRATED_TN_20261118/10_DESIGN_DRAFT.md` §7.2 and §7.3 ("고객 주문번호"), and `pr_plan.json` unit U13 (crypto integrated design TN v1.0, outside this repository). U13 is split into four pull requests: U13a wire id form (this spec), U13b venue-neutral margin report and record schema 2, U13c funding and venue-made position changes, U13d absence rule and `market_ref`. Tickets: `issues/` next to this file.

## Problem Statement

The OMS writes its client order id for the venue in one form only: `0x` + 32 hex digits (`VenueClientId::to_hex`), which is Hyperliquid's cloid. Bybit's `orderLinkId` is at most 36 characters of digits, letters, `-` and `_` (Bybit v5 docs, `05_BYBIT_PRIVATE_API_DIGEST.md` §3), so the hex form (34 characters) would fit by length, but nothing in the code says which form a gateway uses, and the status API (`/api/orders/{id}`, `venue_client_id`) always prints hex. A Bybit gateway that picked its own form would make the status API show an id the venue does not know.

## Solution

1. `oms-core` gets `WireIdForm`, a closed list of the forms in which a `VenueClientId` is written on the wire, with `encode` and a strict `decode`:
   - `Hex128`: `0x` + 32 lower-case hex digits, exactly `to_hex` / `parse_hex` today (Hyperliquid, simulated venue).
   - `Base32Pair`: `instance_id` and `order_id`, each as 13 lower-case base32hex digits (`0-9a-v`, RFC 4648 §7), joined by `-`: 27 characters, for Bybit's `orderLinkId`.
2. `VenueGateway` gets `const WIRE_ID: WireIdForm`. The simulated venue and Hyperliquid declare `Hex128`.
3. `node.rs` puts the started gateway's `WIRE_ID` into `NodeInfo`, and the status API renders `venue_client_id` with it.

## User Stories

1. As the developer of the Bybit gateway, I want an `orderLinkId` form that the core can decode, so that a status or fill of our order is recognised as ours and an order of another data directory as foreign.
2. As the operator, I want `/api/orders/{id}` to show the id the venue knows, so that I can search for it at the venue.
3. As the owner, I want Hyperliquid's requests unchanged.

## Implementation Decisions

### D1. Fixed width, both halves

`Base32Pair` writes both halves at full width (13 digits cover 65 bits, a u64 needs 64). Fixed width makes the decoding strict and one-to-one: every `VenueClientId` has exactly one string, and every accepted string has exactly one `VenueClientId`. The design draft wrote "instance_id 12자 base32"; 12 digits cover only 60 bits, so it is 13 here (27 ≤ 36 characters).

### D2. Strict decoding

`decode` accepts only the exact form: length, separator, digit set and case. Upper case, a missing half, extra characters or a first digit that overflows 64 bits (only `0`–`f` are possible in the 13th-from-right position) return `None`. A foreign id therefore never decodes to one of our orders by accident of case folding.

### D3. Uniqueness

`order_id` is never reused within an instance (C-ID) and `instance_id` is fixed per data directory, so a one-to-one form keeps ids unique. Bybit's own uniqueness window for `orderLinkId` is undocumented (testnet item T1); the form does not depend on it.

### D4. Fill key

The fill key is already chosen by each gateway (C-GW G-8). The Bybit row (`<symbol>:<execId>`, at most 32 + 1 + 36 = 69 of the 96 characters `FillKey` allows) is written into C-GW now and implemented by the Bybit gateway unit. No code change here.

### D5. What does not change

`to_hex`, `parse_hex` and all their callers (both gateways, `simvenue`, `oms-sim`) stay. No journal record, event or test expectation changes. `NodeInfo` gains a field; it is built only in `node.rs`.

## Testing Decisions

- `ids::tests`: `Hex128.encode` equals `to_hex` and `Hex128.decode` equals `parse_hex` on edge values; `Base32Pair` round-trips edge values (0, 1, `u64::MAX` in each half), is at most 36 characters of `[0-9a-v-]`, is deterministic, and gives 10^6 distinct strings for 10^6 distinct ids (instance fixed, order 1..=10^6, plus a second instance); `decode` refuses upper case, wrong length, missing separator, digits outside `0-9a-v`, a 13-digit half above `u64::MAX`, and the other form.
- `venue::tests`: both gateways declare `Hex128`, so their wire form is unchanged.
- A claim in `registry/claims.toml` names the tests.

## Out of Scope

The Bybit gateway itself, amend, `cancel_all`, margin report, record schema, new venue inputs (U13b–U13d, U16–U18b).
