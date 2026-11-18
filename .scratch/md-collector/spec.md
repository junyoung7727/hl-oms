# Spec: md-collector part 1, the collector and the md-lake binary (unit U5b)

Status: ready-for-agent

Design source: `RESEARCH/CRYPTO_TRADING_INTEGRATED_TN_20261118/10_DESIGN_DRAFT.md` §5.4.3, §6.2, §8.5, R2–R5 and `pr_plan.json` unit U5b (crypto integrated design TN v1.0, outside this repository). Lake contract: `contracts/lake/` (vendored) and its document `docs/crypto/lake-contract.md` in quant-data-platform, §2.2 (records), §2.3 (notes), §2.4 (stage 1), §2.5 (the sealer). Tickets: `issues/` next to this file.

## Problem Statement

`md-feed` (U5a) connects to a venue and hands every received frame to a sink, and `md-lake` (U4) can write stage-1 files and seal them, but no program joins the two. Nothing fetches the instrument list, writes frames to L0, survives a slow disk or a full disk without losing frames silently, takes the daily snapshot on a new connection, or runs the seal. The seal also cannot run: it needs a classifier that names the topic of every record, and only venue code can give one.

One gap the earlier units did not show: **a Bybit frame does not say its market.** `orderbook.50.BTCUSDT` is the same text on linear and on spot. The U4 tests used a classifier that always answers `linear`. The sealer sees only the record (`recv_ns, src, conn_id, frame_no, payload`), so the market must be recoverable from these fields.

## Solution

A new workspace crate `crates/md-collector` (library + two binaries):

1. **`md-collector run --config <file>`**: one process per venue. It loads and checks a TOML config, fetches the instrument list of every market (all pages) and records each body verbatim, plans the connections, runs one md-feed `Feed` per (market, group), and writes every frame to L0 through a bounded queue per connection and one writer thread. Market data goes to the connection's stage-1 open file, control frames and notes to the collector's session file. Each UTC day it switches every connection to a new overlapping one, staggered. When the lake's free space falls below a floor it keeps only the traded topics. It exposes its state to U6 in-process.
2. **`md-lake seal|seal-days|verify`**: the stage-2 commands of the lake contract, using the registry's classifier for the venue.
3. **Registry**: venue name → its markets, its adapter factory, its `conn_id` ranges and its classifier. Bybit is registered; U10/U11 add theirs.

## User Stories

1. As the operator, I want one command per venue that writes everything the venue sends to L0 and never drops a frame without a note, so that the lake holds what we received.
2. As the operator, I want `md-lake seal --venue bybit` to seal closed hours with the right market for every record, and to refuse an unknown venue before touching any file.
3. As the researcher, I want each UTC day's files to start from a fresh snapshot on a new connection, with the overlap kept, so that a day file can be read alone.
4. As the strategy owner, I want the traded topics on their own connections that stay up when the disk is nearly full, so that trading data is the last to go.
5. As the owner, I want tests that never open a non-loopback socket, and no real venue data in the repository.

## Implementation Decisions

### D1. Market from `conn_id` (the gap above)

- Each market of a venue owns a fixed range of `conn_id`: market slot `k` gets `[max(k << 28, 1), (k + 1) << 28)`. Bybit slots are pinned forever: `linear` 0, `inverse` 1, `spot` 2 (a test pins them; changing them would misroute old files). The contract allows it: `conn_id` is "the connection number inside the collector session", not a count.
- md-feed change, additive: `ConnIds::range(&self, start, end) -> Arc<ConnIds>` returns an allocator for `[start, end)` that **shares the REST `frame_no` counter** of its parent (so `conn_id 0` stays one pseudo-connection per session), and `ConnIds::next_rest_frame_no` becomes public (the collector numbers its instrument bodies with it).
- Classifier (`registry::classify(venue)`): `src = 0` → market from `conn_id >> 28`, then the adapter's `route(payload)`; `Route::Control` → `None`. `src = 1` → the body's JSON `result.category` names the market (Bybit instrument answers carry it), topic `{market, "instruments", "_all"}`; anything else `None`. Notes → `None`.
- The collector applies the same classifier before writing a REST body: routable → the REST open file (`conn_id 0`), not routable (a failed answer without `category`) → the session file. So the seal's 1 % unroutable guard does not fire in normal operation.

### D2. Config (TOML, schema in the crate docs and `docs/operations/md-collector.md`)

```toml
venue = "bybit"
collector = "md_collector"        # writer name, ^[a-z0-9][a-z0-9_]*$
lake_root = "/mnt/nvme/lake"
host = "linux-pc"                 # optional; default: /etc/hostname, else "unknown"

[write]                           # all optional
flush_ms = 1000
fsync_ms = 5000
queue_bytes = 16777216            # per connection

[disk]
floor_bytes = 20_000_000_000
margin_bytes = 20_000_000_000
check_ms = 10000

[day_change]
start_offset_ms = 5000            # first switch at 00:00:05 UTC
stagger_ms = 2000

[instruments]
refresh_ms = 3600000

[[markets]]
name = "linear"                   # a market the venue registers
symbols = "all"                   # or ["BTCUSDT", ...]; "all" = instruments with status Trading at start
channels = ["orderbook.50", "publicTrade", "tickers"]
conn_budget = 100                 # connections this market may hold
max_topics_per_conn = 200         # optional
ws_url = "..."                    # optional override (tests: loopback fakes)
rest_base = "..."                 # optional override

[[markets.extra]]                 # extra topics for some symbols
channel = "orderbook.1000"
symbols = ["BTCUSDT"]

[[traded_channels]]               # what a strategy reads; must be collected
market = "linear"
symbol = "BTCUSDT"
channel = "orderbook.1000"
```

- `deny_unknown_fields` everywhere. Checks at load, before any file or network: venue registered; each market registered, listed once; `conn_budget` sum ≤ the venue's limit (Bybit 250, the md-bybit note); every `traded_channels` entry is a collected (market, channel) and, for a symbol list, a listed symbol. With `symbols = "all"` the symbol part is checked again after the instrument fetch; a traded symbol that is not trading refuses the start. A test-only key `loopback_only = true` (top level) is passed to md-feed and the REST client.
- New symbols listed after the start are collected from the next process start (judgement: the plan is fixed for a session; the hourly instrument bodies are still recorded). Recorded in Out of Scope.

### D3. Plan groups

- Topics of a market: `symbols × channels` plus each `extra`'s `symbols × {channel}`. They are split into **traded** (in `traded_channels`) and **other**. Each set is planned separately into its own connections, so that the disk floor can close the other connections and keep the traded ones.
- md-bybit `plan(symbols, channels, depth)` takes one depth per call, so a set is cut into groups: per symbol, its non-book channels plus its first book channel form one channel set, each further book channel its own set; symbols with the same channel set form one `plan` call (depth = the book channel's depth, 50 when none). Groups are planned in a fixed order (traded first, then other; inside by channel set) with consecutive `conn_index` ranges inside the market's `conn_budget` (`with_conn_budget(first, remaining)`). Each group is one `Feed`.

### D4. Ingest: sink → bounded queues → writer thread

- The collector's `FrameSink` (called on md-feed's tasks, must not block): routes the frame (`src = 0`: adapter `route`; `Control` → session file; `src = 1`: D1), counts it against its connection's in-flight bytes, and sends it on one unbounded channel to the writer thread. When in-flight bytes + this frame would pass `queue_bytes`, it returns `Err(Overflow)` (md-feed then closes the connection, reason `overflow`, and reconnects); once per connection it first enqueues an `overflow` note (`conn_id` = the connection, `topics` null, `start_recv_ns` = the refused frame's `recv_ns`, `end_recv_ns` null, `detail.text` = "receive queue full"). Frames accepted before it are written. In-flight bytes go down when the writer has written the frame. Memory bound: `queue_bytes` × connections (24 × 16 MiB ≈ 0.4 GB).
- Notes and connection-close markers bypass the cap (they are small and must not be lost).
- **Writer thread** (one, std): owns one `md_lake::writer::OpenWriter` per `conn_id` (created at its first frame; `conn_id 0` is the REST open file) and the `SessionWriter`. It ticks every writer every 100 ms (flush 1 s, fsync 5 s, hour and date rotation are md-lake's). A close marker for a `conn_id` (sent after md-feed's `Disconnected`, or after a feed stop) closes its open file (frame end, fsync, unlock). A frame for a `conn_id` already closed (should not happen after D6's stop) goes to the session file, with a log line: it is kept, never dropped.
- **clock_step**: per `conn_id ≥ 1`, when a frame's `recv_ns` is lower than the previous one of that connection, the writer first writes a `clock_step` note: `conn_id`, `topics` null, `start_recv_ns` = the new value, `end_recv_ns` null, `detail.back_ns` = previous − new. The frame is written as received (md-lake keeps file order).
- Notes are md-lake note records (`src = 2`, record `conn_id` = the note's or 0, `frame_no` 0, record `recv_ns` = the writer's wall clock), payload keys in the contract order (as md-lake's `note_payload`), `writer` = the collector name.
- A write error is fatal: the collector stops (exit 1) and systemd restarts it into a new session; md-lake seal recovers the files (crash_truncate).
- The collector never runs the stage-2 seal and never opens a file of an earlier session (every file is created new with the session's `session_ns`).

### D5. Instruments

- At start, for each market, every page (`instruments_request_page` + `next_page_cursor`) through md-feed's `RestClient` (same loopback guard), with `get_any`, so a failed answer's body is recorded too. Each body is a REST record (`conn_id 0`, `frame_no` from the shared counter, `recv_ns` from the client). A start without a complete, parsable list of every market exits 1 (after writing the bodies it got).
- Every `refresh_ms` the same fetch runs again and records its bodies; the parsed list is used only to log a changed symbol set (D2).

### D6. Feeds, events, stop

- One `Feed` per group, `FeedConfig` from the market (`ws_url`, the market's `ConnIds` range, `loopback_only`). Events of every feed go to one task: `Connected` → remember `conn_id` of the group; `Disconnected` → close marker for that `conn_id`; every event is logged as one JSON line on stderr (as oms-node). U6 turns them into metrics.
- md-feed change, additive: `FeedHandle::stop(self) -> impl Future<Output = bool>`: asks every connection to stop, then waits (at most 5 s) until no task of the feed is alive, so that no frame reaches the sink afterwards; `true` when all ended. Used by the disk floor and by shutdown, followed by close markers of the feed's connections.
- Shutdown (SIGTERM, Ctrl-C, or the handle in tests): stop every feed, then the writer closes every file (frame end, fsync, unlock) and exits.

### D7. Day change

- A scheduler on an injected wall clock: at each UTC midnight + `start_offset_ms`, it calls `switch(conn_index)` for every slot of every running feed, in a fixed order (market order of the config, then group order, then `conn_index`), one every `stagger_ms`. md-feed opens the new connection, keeps both until the new one has every snapshot, then closes the old one (U5a). Frames of both go to L0 as received. `SwitchFailed` is logged; the old connection stays (it is retried the next day; judgement).
- The schedule is a pure function `day_change_times(midnight_ns, n, offset, stagger) -> Vec<i64>` (unit-tested with the defaults: 00:00:05, 00:00:07, …).
- Slots of a feed dropped by the disk floor are skipped.

### D8. Disk floor

- A checker reads free space of `lake_root` every `check_ms` (`rustix::fs::statvfs`, as oms-node; injected `FreeSpace` trait in tests). Below `floor_bytes`: stop every **other** feed (D6), write one `disk_floor` note per dropped topic (`conn_id` null, `topics` [that topic], `start_recv_ns` = now, `end_recv_ns` null), keep the traded feeds. At or above `floor_bytes + margin_bytes`: start the other feeds again (new connections, new `conn_id`s). Between the two: no change.

### D9. md-lake binary

```text
md-lake seal      --venue V --lake DIR --first-hour YYYY-MM-DDTHH [--layout by_symbol|by_connection] [--now-ns N]
md-lake seal-days --venue V --lake DIR --first-hour YYYY-MM-DDTHH --from YYYY-MM-DD [--now-ns N]
md-lake verify    --venue V --lake DIR --date YYYY-MM-DD --json [--now-ns N]
```

- The venue is looked up first: unknown → exit 2 with a message, before the lake root is opened (no file is created or touched). Sealer name `md_lake`, `host` as D2.
- `seal` prints the `SealReport` as JSON. Exit 0 when the run ended normally (hours may wait on locked files: reported, not an error), 3 when another run holds the seal lock, 1 on any other seal error (`Unroutable`, `UnknownSource`, `Check`, I/O).
- `--now-ns` replaces the wall clock (tests, and a person re-running a past state). It cannot break the lock rule: a file its collector holds is never read.

### D10. State for U6

- `CollectorHandle::state() -> StateSnapshot`: per connection in-flight bytes, frames and bytes written, overflows; clock steps; disk floor active and the last free-space value; instrument fetches (ok, failed, last `recv_ns`); feed events counted by kind; switches done and failed. Plain counters behind atomics/mutex; no metrics export here.

### D11. Deploy files

- `deploy/systemd/md-collector@.service` (`MemoryMax=1G`, `Restart=always`), `deploy/systemd/md-lake-seal@.service` + `.timer` (hourly at :02), templates only, with a comment that they are not installed. No service is started by this unit.

## Testing Decisions

- **Seam A**: the library API `Collector::start(cfg, deps) -> CollectorHandle` with injected `Deps { fs: Arc<dyn md_lake::fs::Fs>, free_space: Arc<dyn FreeSpace>, clock: Arc<dyn WallClock> }`, driven from `crates/md-collector/tests/`. **Seam B**: a loopback fake Bybit in `tests/common/`: a WebSocket server that answers the subscribe frames of each connection with a subscribe answer and then, per subscribed topic, a synthetic snapshot and deltas (documented v5 shapes), answers `ping` with `pong`, can drop a connection after N frames, and records every frame it sent per connection; and a minimal HTTP/1.1 server for the instrument pages. **Seam C**: the `md-lake` binary as a child process (`CARGO_BIN_EXE_md-lake`) with `--now-ns`.
- **Ingest unit tests** (clock_step, queue accounting) drive the sink directly with synthetic `RawFrame`s.
- **Slow disk**: an `Fs` wrapper over `RealFs` whose file writes block while a gate is closed.
- Test directories under `OMS_TEST_DIR`, else `CARGO_TARGET_TMPDIR` (as md-lake). Real time with short settings; waits poll with a 5–10 s limit.
- Fixtures are synthetic. No captured venue data (public repository; Bybit API terms 6.7). No test opens a non-loopback socket (every config sets `loopback_only`).
- **Tests prove** the done criteria of the U5b plan entry; the tickets list them.

## Out of Scope

- Metrics export, VictoriaMetrics, the 1 s summaries (U6); L1 (U7); Binance, OKX, Hyperliquid (U10, U11; they register in the registry and bring their own `conn_id` slots).
- New symbols during a session (picked up at the next start; D2).
- Installing or enabling the systemd units; any order; any network call to a venue.
- Any change to `oms-*` crates, `.github/`, the journal or the registry contracts. md-collector writes no journal, so no `registry/contracts.toml` entry.
- From U4: a power loss can leave garbage inside a block whose length is complete; the seal then stops for a person. Goes into the runbook section of `docs/operations/md-collector.md`.

## Further Notes

- Built directly by Claude Opus 5.5 (2026-11-19, owner: "이제 개발을 너가 직접해"); lean spec (owner: "검증 사소한 것까지 하지마").
- D1 was told to the owner on 2026-11-19 (Buzz DM) with the invitation to override.
