# Spec: direct alerts, observability push and collector telemetry (unit U6)

Status: ready-for-agent

Design source: `RESEARCH/CRYPTO_TRADING_INTEGRATED_TN_20261118/10_DESIGN_DRAFT.md` §5.4.5 (metric names), §7.7 (direct alerts), §8.3 (alert paths, severities) and `pr_plan.json` unit U6 (crypto integrated design TN v1.0, outside this repository). Delivery rules: draft PR #3 (`tools/alert/hl_alert.py` on branch `telegram-alert`). Store formats: VictoriaMetrics JSON line import and VictoriaLogs JSON stream API (`RESEARCH/OBSERVABILITY_STACK_FACTS_20261118.md` V17, L12, L13). Tickets: `issues/` next to this file.

## Problem Statement

The collector (U5b) writes L0 and keeps counters in process, but nothing leaves the process. Nobody learns that an L0 write failed, that the disk is at the floor, that the collector died, or how the books and the feed look. The observability stack (VictoriaMetrics, VictoriaLogs, vmalert, U20a) is not installed yet, and the design says that the alerts that protect the lake must not depend on it.

## Solution

Two new library crates and the wiring in md-collector:

1. **`alert-direct`**: a Telegram sender with the PR #3 delivery rules and the §7.7 repeat rule. It runs on its own thread; the caller never waits.
2. **`obs-push`**: a bounded, non-blocking client for VictoriaMetrics `/api/v1/import` and VictoriaLogs `/insert/jsonline`. It runs on its own thread; a full queue drops the oldest lines and counts them.
3. **md-collector telemetry**: summaries every 1 s for traded topics and every 10 s for the rest, lake and process metrics, a writer heartbeat and the systemd watchdog, JSON logs to VictoriaLogs and stderr, and `[MD P1]` alerts for an L0 write failure and for the disk floor.
4. **`md-lake seal`**: one `[MD P1]` per run when the run writes a `collector_down` note for a whole hour.

## User Stories

1. As the operator, I want a Telegram message when L0 writing fails or the disk floor is reached, even when the observability stack is down, so that I can act before data is lost.
2. As the operator, I want a dead collector reported by the seal run, which runs from a timer and does not need the collector or the stack.
3. As the operator, I want systemd to restart a collector whose writer is stuck, but not one whose venue is merely quiet.
4. As the researcher, I want spread, mid, depth, message counts, receive lag, gaps and reconnects per topic in VictoriaMetrics, at 1 s for the traded topics.
5. As the strategy owner, I want the metric and log pushes never to slow frame ingestion, even when VictoriaMetrics is dead or slow.

## Implementation Decisions

### D1. alert-direct (crate `crates/alert-direct`)

- **Config file** (PR #3 format): JSON `{"token": "<digits>:<chars>", "chat_id": "..."}`. `load(path)` refuses: not a regular file (symlinks refused, `lstat`), any group or other permission bit (`mode & 0o077 != 0`), a path with an ancestor directory that contains `.git` (the token must live outside any repository; judgement), a token that does not match `^[0-9]+:[A-Za-z0-9_-]+$`, a missing or empty `chat_id`. No error text contains the token.
- **Message**: `[MD P1] <venue> <MM-DD HH:MM:SS KST>\n<body>`. Prefix and venue are given by the caller (`[MD P1]` here; `[OMS P1]` and `[WATCH P1]` later, U18a/U19b).
- **Split** (PR #3 `split_message`): parts of at most 3,500 UTF-16 units, cut at line ends; one overlong line is cut by characters.
- **Outbox**: at most 50 parts. Adding beyond 50 drops the oldest and counts them. After a flush that delivered everything, if the count is non-zero, one message `[MD P1] 전송 실패로 알림 N개를 버림` is sent and the count reset (PR #3).
- **Send outcome** (PR #3): HTTP answer with `ok: true` → delivered. HTTP 400 or 413 → refused for good: drop the part, count it. Any other answer (401, 403, 429, 5xx) or a network error or a timeout (10 s per request) → keep the part and stop this flush; retry on the next flush. A timeout after delivery can send twice: a duplicate is better than a lost alert (PR #3).
- **Routes**: by default (1) `https://api.telegram.org` by DNS, (2) the same URL with `api.telegram.org` resolved to `149.154.167.220:443` (reqwest `resolve`, so SNI and certificate are checked for `api.telegram.org`). The route that worked last goes first. Routes are injectable for tests (`Route { base: Url, resolve: Option<(host, SocketAddr)> }`).
- **Repeat rule** (§7.7): conditions keyed by a string. `set(key, Some(text))` while the condition holds, `set(key, None)` when it does not. A message is queued at the first `Some` (start), then every 5 min while it lasts (the latest text), and once at the first `None` after it (clear, body `해소: <text of start>`). `once(text)` queues one message without a key. The clock is injected.
- **Runtime**: `AlertDirect::start(cfg, clock) -> AlertHandle`. One thread named `alert-direct` owns a current-thread tokio runtime and the reqwest client. `set` and `once` only take a mutex and notify; they never wait for the network. The thread flushes on notify and every 5 s. `AlertHandle::flush_for(Duration)` waits until the outbox is empty or the time is up (used before exit). `AlertHandle::state()` returns `{queued, delivered, dropped_total, failures}`.
- Dependencies: only crates already in `Cargo.lock` (reqwest with `rustls`, tokio, serde_json, url). No new crate in the lock file.

### D2. obs-push (crate `crates/obs-push`)

- **VictoriaMetrics line**: `{"metric":{"__name__":"<name>","<label>":"<value>",...},"values":[<f64>],"timestamps":[<ms>]}` (V17). One sample per line. Labels in a `BTreeMap` (sorted, stable output). Non-finite values are not pushed (counted as `invalid`).
- **VictoriaLogs line**: `{"_time":"<RFC 3339 UTC, ns>","_msg":"<text>","service":"...","host":"...","venue":"...",<fields>}`. The request URL carries `_stream_fields=service,host,venue` (L12). Field values are strings or numbers; nested objects are flattened as `a.b` keys (VictoriaLogs does this itself; we keep the JSON as it is and let the server flatten it — judgement: no own flattening).
- **Queue**: one bounded queue per endpoint (default 100,000 lines each). `push` never blocks: when full it drops the oldest line and counts it (`dropped`). The sender thread takes up to 10,000 lines (judgement) per request, POSTs them joined by `\n`, timeout 5 s. 2xx → done. Anything else → the batch goes back to the front of the queue (still bounded: overflow drops the oldest), retry after a backoff of 1 s doubling to 30 s.
- **Runtime**: as D1, one thread `obs-push` with its own current-thread runtime. `ObsPush::start(cfg) -> ObsHandle` with `metric(name, labels, value, ts_ms)`, `metrics(Vec<Sample>)`, `log(LogLine)`, `state() -> {queued, sent, dropped, failed_requests, invalid}` per endpoint, and `flush_for(Duration)`.
- **Self metrics**: each push cycle also pushes `obs_push_dropped_total{endpoint}` and `obs_push_queue_lines{endpoint}` (judgement; so drops are visible once the store is back).
- A missing endpoint (no URL) makes that kind of push a no-op that counts nothing.

### D3. md-feed: book updates to the message sink (additive)

- `MessageSink` gets a default method `fn book_msg(&self, conn_id: u32, key: &StreamKey, venue_ts_ns: i64, recv_ns: i64) {}`, called for every book snapshot or delta that arrives on an active connection (same `active` rule as `forward`), **before** the book's lock is taken (R6; as built in ticket 03: the lag is of the message received, applied or not). Existing implementors are unchanged. The collector uses it only for `md_recv_lag_ms` of book topics (frames are counted in the frame sink).
- `Venue::start_feed` in the registry gets a `msgs: Option<Arc<dyn MessageSink>>` argument (Bybit passes it to `Feed::start_with`).

### D4. Collector telemetry model (`crates/md-collector/src/telemetry.rs`)

Per topic `(market, symbol, channel)` the collector keeps, behind one mutex per topic stripe (judgement: a `Mutex<BTreeMap>` sharded by topic hash, 16 shards):

- frame count and payload bytes since the last summary, the last frame `recv_ns` (from the frame sink: `RawFrame.topic`, market from the conn slot);
- a receive-lag histogram since the last summary: `recv_ns - venue_ts_ns` in ms, from the message sink (books, trades, tickers). p50 and p99 by nearest rank over a fixed log-bucket histogram (buckets: 0, then ×1.25 from 0.1 ms to 60 s; judgement: about 4 % relative error, constant memory per topic). Negative lags (clock skew) go to bucket 0 and are counted in `md_recv_lag_negative_total` (judgement);
- gap and reconnect totals from feed events (`Gap.key`; `Connected` of a conn_index counts one reconnect for every topic of that plan, the first connect of a slot does not).

Every **fast** period (default 1 s) the collector emits the summary of every **traded** topic (`traded_channels`); every **slow** period (default 10 s) the summary of every other topic. A traded topic is never also emitted at 10 s. Series (names exactly §5.4.5, labels `venue, market, symbol, channel` unless said):

| metric | value |
|---|---|
| `md_spread_bp`, `md_mid_px` | from the active connection's book (`read_book`), skipped when the topic has no book or the book is stale or crossed (judgement) |
| `md_depth_qty{side=bid\|ask, levels=10\|50}` | `depth_sum(side, n)` as a float with the instrument's qty exponent |
| `md_msgs`, `md_bytes` | labels `venue, market, channel, conn`: sum over the period of frames and payload bytes on that connection and channel |
| `md_recv_lag_ms{quantile=0.5\|0.99}` | from the histogram; skipped when no sample in the period |
| `md_gaps_total`, `md_reconnects_total` | running totals (counters) |
| `md_last_msg_age_s` | now − last frame recv of the topic; skipped before the first frame |

Every 10 s (venue labels only): `lake_l0_open_files` (open writers held by the writer thread), `lake_l0_bytes_total` (bytes written to L0 since start), `lake_l0_write_errors_total` (0 or 1: the writer stops at the first error), `lake_disk_free_bytes` (the disk checker's last reading, injected `FreeSpace`), `md_queue_bytes{conn}` (in-flight bytes per plan slot; `conn` = slot index), `md_collector_heartbeat_ts` (D5), `node_clock_offset_ms{host}` (D6). Every 60 s: `lake_seal_lag_hours` (D7). The collector computes no days-left value (vmalert does, U20a).

`md_msgs` and `md_bytes` are per connection, the others per symbol: the cardinality rule of §5.4.5 (no order ids in labels) holds.

### D5. Writer heartbeat and the systemd watchdog

- The writer loop (U5b `ingest::Writer::run`) marks a **beat** once per 1 s cycle: after a tick in which every writer's tick returned `Ok`, at most once per second, also when no frame arrived. A failed write or flush makes the thread stop (U5b), so no beat follows. A stall inside a write (the injected `Fs` blocks) gives no beat.
- A beat stores the wall time in seconds in the shared state (`md_collector_heartbeat_ts{venue}`, pushed every 10 s by the telemetry task) and sends `WATCHDOG=1` to `$NOTIFY_SOCKET` (a `std::os::unix::net::UnixDatagram`; a leading `@` is the Linux abstract namespace). `READY=1` is sent once after `Collector::start` returns `Ok`. Without `NOTIFY_SOCKET` nothing is sent. The notifier is injected (`Notifier` trait) so tests use a socket in a temp dir.
- So a quiet venue or a network outage keeps the beat (the loop still ticks every 100 ms), and a stuck or failed writer stops it. `WatchdogSec` is set in U20a; this unit adds `Type=notify` and `WatchdogSec=30s` (judgement) to the template `md-collector@.service` only as a comment-free documented default.

### D6. Clock offset

`ClockOffset` trait; the real reader runs `chronyc -c tracking` (CSV; field 5 is the "System time" offset in seconds; measured on the Linux PC 2026-11-19: CSV `0.000465` = text "0.000465 seconds slow of NTP time", so positive = the system clock is behind NTP). The metric is that value in ms, same sign with a 2 s timeout, at most every 10 s. No chronyc, an error or a parse failure → no sample and one warn log per hour (judgement). Tests inject a fixed value.

### D7. Seal lag

md-lake gets an additive `pub fn unsealed_hours(fs, venue, now_ns, lookback_hours) -> io::Result<Vec<i64>>`: closed hours (before the current hour) within the lookback whose `_open` hour folder is not empty. The collector pushes `lake_seal_lag_hours = (hour_start(now) - oldest) / 1 h`, `0` when none. Lookback 168 h (judgement: the seal's 7-day day-seal lookback).

### D8. Direct alerts from the collector

Config (optional section; absent → no direct alerts, one warn log at start):

```toml
[alert]
telegram = "/home/user/.config/hl-oms/telegram.json"   # D1 file
# api_base = "http://127.0.0.1:18080"                  # tests only: honoured only with loopback_only = true
```

- **L0 write failure**: when the writer stops on an error (`IngestSnapshot.failed`), the collector sends one `[MD P1]` `L0 쓰기 실패: <error kind, e.g. ENOSPC/EIO/other> — 수집기가 종료되고 systemd가 다시 시작함` with `once` (the process exits; a restart that fails again sends again), then `flush_for(10 s)`, then exits 1 as in U5b. The error text is the io error kind and message (no paths outside the lake are added).
- **Disk floor**: the condition `disk_floor` is `set(Some(...))` while the checker is in the floor state (start, every 5 min, clear). Text: free bytes and floor.
- The alerts use alert-direct only, never obs-push: they reach Telegram when VictoriaMetrics is down.

### D9. md-lake seal alert

- `SealReport` gets `collector_down_hours: Vec<(String, u8)>`: hours of this run for which the sealer wrote the "no open file in the hour" note (the whole-hour case only; the session-gap `collector_down` note is not a whole hour).
- `md-lake seal` and `seal-days` take `--telegram <file>` (optional) and `--telegram-api <url>` (tests only, refused unless the URL host is a loopback address). When the run wrote whole-hour notes, it sends **one** message per run listing the hours (judgement: a long outage gives one message, not one per hour), then `flush_for(10 s)`. A failed send does not change the exit code; it is logged to stderr.
- The systemd seal unit template passes `--telegram %h/.config/hl-oms/telegram.json`.

### D10. Logs

`collector::log` (stderr JSON line) also pushes a VictoriaLogs line: `_time` = now, `_msg` = msg, `service=md-collector`, `host`, `venue`, `level`, and the fields object. The writer thread's `eprintln!` lines are changed to the same `log` call. stderr output is unchanged in shape (journald keeps it).

### D11. Config additions

```toml
[telemetry]                         # optional; absent → no push (metrics and logs)
vm_url = "http://127.0.0.1:8428"    # optional
vl_url = "http://127.0.0.1:9428"    # optional
fast_ms = 1000
slow_ms = 10000
queue_lines = 100000
```

Unknown keys refused as in U5b. `fast_ms` ≥ 100, `slow_ms` ≥ `fast_ms`. A non-loopback `vm_url`/`vl_url` is refused when `loopback_only = true`.

## Testing Decisions

- Every network peer is a loopback fake: a small HTTP/1.1 server on `127.0.0.1:0` (hyper is in the lock file through reqwest; the fake uses tokio `TcpListener` and hand-written HTTP to keep it dependency-free — judgement) that records requests and answers with a scripted status, or never answers (slow), or is not listening (dead port).
- Format tests compare exact lines with the documented examples (V17, L12), with the labels and fields of this design.
- Summary values are checked from a synthetic book (known levels → known spread, mid, depth).
- Time-dependent rules (repeat rule, 1 s / 10 s split, heartbeat) use injected clocks or short configured periods; no test sleeps longer than 3 s (judgement), the 60 s quiet-venue criterion is shown with a 60 s period of an injected clock, not wall time.
- The "ingest unchanged when the store is dead or slow" test runs the collector against the fake venue twice (store absent vs store slow) and requires every sent frame in L0 both times and the sink's frame call latency p99 below 1 ms (judgement).
- Each ticket: write the test, see it fail, then the code. Where a body is written before its test, break the rule on purpose and check that the test catches it (disclosed in the PR).

## Out of Scope

- vmalert rules, Alertmanager, Grafana, unit files beyond the md-collector and seal templates (U20a).
- OMS and watcher alerts (U18a, U19b); the projector (U14).
- Installing or starting any service; any network call outside loopback in tests; any order.

## Further Notes

- The heartbeat is pushed every 10 s by the telemetry task, but its value is set only by the writer loop: a stuck writer freezes the value while pushes continue, which is what vmalert needs (`time() - md_collector_heartbeat_ts > 30`).
- The design note "summaries to VM" (§5.4.5 resolution column) says 10 s for gaps, reconnects and last-message age; here they follow the topic's period (1 s for traded topics). That is finer, not coarser, and keeps one emission path (judgement).

## Review decisions (spec review, 2026-11-19)

One Opus reviewer, 6 MUST and 6 SHOULD; all accepted. These override the sections above where they differ.

- **R1 (token in logs).** Every send error is written with reqwest `without_url()` (the URL holds the token); tests check `state().last_errors` and the error texts. Done in ticket 01.
- **R2 (blocked first route).** Each route has a connect timeout of 3 s, separate from the 10 s request timeout. Exit paths flush for 20 s (D8, D9), which covers a blocked first route plus the pinned one. The collector's watchdog (30 s) is not reached: detection ≤ 1 s + 20 s.
- **R3 (repeat rule across restarts).** alert-direct can keep the repeat state in a JSON file (`AlertConfig.state_file`), written after every change (write temp, fsync, rename). The collector uses `[alert] state_file`, default `$RUNTIME_DIRECTORY/alert-state.json` when that variable is set; the template sets `RuntimeDirectory=md-collector-%i` and `RuntimeDirectoryPreserve=restart` (not on the lake's file system). The L0 write failure becomes the condition `l0_write` (set at failure; cleared by the first beat of the next session), so a restart loop under ENOSPC sends start, every 5 min, and clear — not one message per restart. Without a state file the rule holds only within one process (warn log at start).
- **R4 (token file path, refused file).** Templates use an absolute path (`/etc/hl-oms/telegram.json`, `Environment=TELEGRAM=`), never `%h`. A configured file that cannot be loaded (mode, symlink, repository, format) is a configuration error: `md-collector check`/`run` exit 2, `md-lake seal` exits 2 before touching the lake.
- **R5 (60 s quiet venue).** One real 60 s test (`quiet_venue_keeps_heartbeat_60s`) is allowed as the single exception to the 3 s rule; it runs in parallel with the other tests.
- **R6 (lock order).** `MessageSink::book` is called after the topic's book lock is released. The summary copies what it needs from the telemetry shard, releases it, then calls `read_book`; it never holds a shard lock while reading a book, and the sink never holds a book lock while taking a shard lock.
- **R7 (frame topic, error kind).** Per-topic frame counts use the `Route::Stream(key)` that `Ingest::dest` computes (WebSocket `RawFrame.topic` is `None`). The writer keeps the OS error code with its error (`ENOSPC`, `EIO`, `EDQUOT`, else the io kind) for the alert text.
- **R8 (reconnects).** `md_reconnects_total` counts the end of a slot's **active** connection. As built (ticket 03, because md-feed emits a failed candidate's `Disconnected` before its `SwitchFailed`): per slot, the first `Connected` while no connection is active makes it active, `SwitchDone` moves it to `to_conn_id`, and a `Disconnected` of the active one counts one reconnect for every topic of the slot and leaves none active. Candidates and failed connect attempts are never active, so a day-change switch counts nothing and an outage with many retries counts one.
- **R9 (conn label, first message).** The `conn` label of `md_msgs`, `md_bytes` and `md_queue_bytes` is the plan slot (`conn_index`), stable across reconnects. `md_last_msg_age_s` before the first frame is the age since the topic's group started.
- **R10 (notify socket).** The notify socket is non-blocking; `WouldBlock` and other send errors are ignored (counted).
- **R11 (permanent rejects, heartbeat first).** obs-push treats 400, 413 and 422 as permanent: the batch is dropped and counted `rejected`, one stderr line. A small priority queue (1,000 lines) is sent before the main queue; the heartbeat and the self metrics use it.
- **R12 (seal error path).** md-lake gets `Sealer::seal_into(&mut SealReport)`; `seal` calls it. The binary alerts on the hours in the report even when the run then fails, before exiting 1. Ticket 05 adds a dead port (connection refused) to the "store never blocks ingest" test.

## Final review decisions (code review, 2026-11-19)

One Opus reviewer: 1 MUST, 9 SHOULD. No token leak, no nested lock on the frame path.

- **F1 (MUST, flood).** `l0_write` cleared at the first beat, before the first flush or fsync: a disk failing at fsync sent a clear and a new start on every restart. Now it clears only after the session has beaten for `fsync_ms + 5 s` with no failure (`obs::l0_conditions`, test `l0_conditions_clear_late_and_report_stalls`).
- **F2.** A stuck write had no direct alert. New condition `writer_stall`: beat older than 15 s and no failure (same test).
- **F3.** A start message that was queued but not delivered before exit counted as sent: `AlertHandle::forget` removes it from the state file after a failed exit flush; `shutdown` flushes for 5 s (test `forget_makes_the_next_start_send_again`).
- **F4 (not fixed).** The seal's `collector_down` alert is not retried when Telegram is unreachable during that run; documented as a known limit in the operations doc.
- **F5.** The alert and lake paths copied the per-connection map every second under the frame-path lock: new `Ingest::health()` with a running byte total. The map itself still grows by one small entry per connection (U5b handoff; ≈ 64 B each, judgement: a few MB a year).
- **F6.** Fast/slow intervals use `MissedTickBehavior::Delay`; the heartbeat and lake metrics go before the summary; chronyc and the seal-lag listing run in their own tasks.
- **F7.** The disk-floor alert is set before the feeds are stopped; above the floor with a feed that did not restart, the text says so.
- **F8.** `lake_l0_bytes_total` is payload bytes before zstd; documented, name kept (design §5.4.5).
- **F9.** Topics stopped at the disk floor leave the summaries (`Telemetry::stop_slot`, test `stopped_slot_leaves_the_summaries`).
- **F10.** `node_clock_offset_ms{host}` only; `md_recv_lag_negative_total{venue}` one series; `md_gaps_total` only for book topics.
