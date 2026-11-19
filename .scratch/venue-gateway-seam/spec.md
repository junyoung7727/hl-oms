# Spec: the VenueGateway trait, no behaviour change (unit U12)

Status: ready-for-agent

Design source: `RESEARCH/CRYPTO_TRADING_INTEGRATED_TN_20261118/10_DESIGN_DRAFT.md` §6.5 and §7.2, and `pr_plan.json` unit U12 (crypto integrated design TN v1.0, outside this repository). Tickets: `issues/` next to this file.

## Problem Statement

The OMS has two order gateways, the simulated venue (`venue/simnet.rs`) and Hyperliquid (`venue/hyperliquid/`). Each one copies the same wiring: a command loop that runs each `SendOrder` or `SendCancel` as its own task under a semaphore, a query loop under a second semaphore, and its long-running tasks (event stream; for Hyperliquid also the dead-man switch). `docs/03-architecture.md` says "게이트웨이에는 trait이 없다". A third gateway (Bybit, U16–U17) would copy the wiring a third time, and nothing in the code states what a gateway must provide.

## Solution

A trait `VenueGateway` in `crates/oms-node/src/venue/mod.rs` and one driver `venue::start(gw, ports, max_concurrent)` that owns the command and query loops. Both gateways implement the trait and call the driver. Their protocol code is not changed.

## User Stories

1. As the developer of the Bybit gateway, I want one trait to implement, so that I do not copy the command and query loops.
2. As a reviewer, I want the wiring in one place, so that the concurrency limits and the order of startup are checked once.
3. As the owner, I want this change to leave every journal record, event, test expectation and Hyperliquid request unchanged.

## Implementation Decisions

### D1. The trait mirrors the C-GW message contract

```rust
pub trait VenueGateway: Send + Sync + 'static {
    /// Queries that run at the same time (simulated venue 8, Hyperliquid 4: the values today).
    const QUERY_CONCURRENCY: usize;
    /// Handles one `VenueCommand::SendOrder`. Must deliver exactly one `SendResult` (G-1).
    fn send_order(&self, cmd: VenueCommand) -> impl Future<Output = ()> + Send;
    /// Handles one `VenueCommand::SendCancel`. Must deliver exactly one `CancelResult` (G-5).
    fn send_cancel(&self, cmd: VenueCommand) -> impl Future<Output = ()> + Send;
    /// Answers one query with venue inputs, or not at all.
    fn query(&self, q: QueryReq) -> impl Future<Output = ()> + Send;
    /// Starts the gateway's long-running tasks (event stream and any other). Called once, before the first command.
    fn spawn_tasks(self: &Arc<Self>, boot: u32);
}
```

`query` stays one method over `QueryReq`. The exhaustive `match` in each gateway already forces every query kind to be handled; seven methods would move code without adding a check.

### D2. Methods the design lists that U12 does not add

§6.5 lists `amend`, `cancel_all`, `query_order(wire_id)`, `account_report`, `rate_classes`, `absence_rule` and a wire id codec. No command or query carries them today, so adding them now would force the simulated and Hyperliquid gateways to implement unused methods without tests. They arrive with the units that use them: wire id codec, absence rule and venue-neutral margin report with U13; rate budget with U17 (Bybit); `cancel_all` with U18b (kill switch). Amend has no unit yet. ADR-021 records this.

### D3. Static dispatch

`venue::start<G: VenueGateway>(gw: Arc<G>, ports: GatewayPorts, max_concurrent: usize)`. A trait with `impl Future` methods is not usable as `dyn`, and boxing each command future would add an allocation per command on the order path. `node.rs` keeps one `match` on `VenueConfig` because the gateways are built from different settings; after construction every arm goes through `start`.

### D4. Nothing observable changes

- Startup order stays: long-running tasks, then the command loop, then the query loop.
- Each command and each query still runs as its own task; command tasks are bounded by `max_concurrent` (unchanged sizing in `node.rs`), query tasks by `QUERY_CONCURRENCY`.
- When the semaphore is closed the loop returns, as today.
- `simnet::spawn`, `hyperliquid::spawn` and `hyperliquid::spawn_with_key` keep their signatures (`tests/hl_live.rs` uses `spawn_with_key`).
- No existing test is edited.

### D5. Tests

Unit tests in `venue/mod.rs` with a fake gateway that records calls:

1. every `SendOrder` reaches `send_order` and every `SendCancel` reaches `send_cancel`, each exactly once; every query reaches `query`;
2. `spawn_tasks` runs exactly once, with the boot number, before the first command is handled;
3. at most `max_concurrent` commands and at most `QUERY_CONCURRENCY` queries run at the same time, and the rest wait instead of being dropped.

Claim `V-GW-SEAM-1` under contract C-GW names these tests. The existing e2e suite (simulated venue) shows that the simulated gateway behaves as before through the driver.

### D6. Documents

ADR-021 (Korean, the ten sections) and its `registry/decisions.toml` entry and ADR index line; `docs/03-architecture.md` (the "no trait" sentence); `docs/contracts/gateway.md` (implementation line, a short trait section); the module comment in `venue/mod.rs`; a `GLOSSARY.md` entry; `docs/generated/` regenerated.

## Testing Decisions

Full local gate from `pr_plan.json` U12 `checks`. Done when every existing test passes with no edited assertion, the new tests pass, and the gate passes.

## Out of Scope

Venue-neutral types (U13), Bybit gateway (U16–U17), kill switch (U18b). No network calls, no orders, no always-on services.
