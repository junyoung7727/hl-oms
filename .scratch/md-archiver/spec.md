# Spec: md-archiver, L0 to L1 Parquet (unit U7)

Status: ready-for-agent

Design source: `RESEARCH/CRYPTO_TRADING_INTEGRATED_TN_20261118/10_DESIGN_DRAFT.md` (S8, checkpoints, trade de-dup, 9.4 retention) and `pr_plan.json` unit U7 (crypto integrated design TN v1.0, outside this repository). Normative source: the vendored lake contract `contracts/lake/` (qdp `docs/crypto/lake-contract.md` §2.2, §2.5, §3, §5.1, §5.3, §7, §8). Where this spec and the contract differ, the contract is right and this spec has a defect. Tickets: `issues/` next to this file.

## Problem Statement

The collector (U5b) and the sealer (U4) produce sealed L0 day files, session files and manifest lines. Nothing turns them into the L1 tables of the contract (`book_msg`, `trade`, `ticker`, `gap`, `instrument`), so research cannot query the lake, the auditor (U8) has no L1 to check, and retention (design 9.4: L1 for the last N days while the disk is small) cannot work.

## Solution

A new crate `crates/md-archiver` (library + binary `md-archiver`). One run, for one venue:

1. finds every **day-sealed** date of the venue that has no L1 yet (or the dates of `--rebuild`), oldest first;
2. builds each date from the sealed L0 files of that date and of the date before (carry-in), with the same md-bybit parser as the live path;
3. writes the L1 Parquet files of the date atomically, the gap day file last;
4. with `--retain-l1-days N`, deletes old L1 partitions whose L0 is sealed and audited clean.

It never writes, renames or deletes anything under `crypto/l0/`.

## User Stories

1. As a researcher, I want every order book message of every connection, every trade once, every ticker message and every instrument row in Parquet with integer prices, so that I can scan a day with DuckDB.
2. As a researcher, I want a checkpoint row at the start of each hour file, so that `book_at(t)` replays at most one hour.
3. As a researcher, I want one `gap` row for every sequence gap, reconnect, handover, restart, crash cut, overflow, disk-floor drop, clock step and collector outage, so that I know which stretches are not trustworthy.
4. As the operator, I want a run to catch up every sealed date that has no L1 (after two missed days, one run builds both), and to replace only derived files when it runs again.
5. As the operator, I want `--retain-l1-days N` to free disk only where L0 is sealed and audited clean, and never to touch L0.
6. As the parser owner, I want `--rebuild FROM..TO` to regenerate a date range after a `parser_ver` bump, with identical bytes for identical input.

## Implementation Decisions

### D1. Crate, dependencies, CLI

- `crates/md-archiver`: library (`build`, `input`, `replay`, `parquet_out`, `retain`, `registry`) and binary `md-archiver`. Depends on md-core, md-bybit, md-lake, serde_json, and **`parquet` 60.1 with `default-features = false, features = ["zstd"]`** (judgement: the low-level column writer, no arrow; 32 crates instead of 51 with arrow). The lock file gains parquet and its dependencies. `zstd-sys` must stay one copy (it has `links = "zstd"`): check `cargo tree -i zstd-sys` and record the result in the PR.
- `md-archiver run --lake <root> --venue <v> [--small-channels publicTrade,tickers] [--rebuild YYYY-MM-DD..YYYY-MM-DD] [--retain-l1-days N] [--disk-floor-bytes B] [--threads N] [--now-ns NS]`. `version` prints crate version and `parser_ver`.
  - Defaults: small channels `publicTrade,tickers` (contract 3.1: one `day.parquet` per date; `orderbook.*` get hour files); disk floor 20 GB (design 9.4); threads 4 (judgement).
  - `--now-ns` in the future is refused unless `MD_ARCHIVER_TEST_ALLOW_FUTURE_NOW=1` (as md-lake, U5b R-rule).
- Exit codes as md-collector: 0 done (also when there was nothing to build); 1 a failure a later run may cure (I/O error, disk below the floor, a date not day-sealed inside `--rebuild`); 2 a refusal it cannot cure (unknown venue, bad arguments, an L0 header or manifest line with `contract_ver` > 1, layout `by_connection` in a manifest, `--rebuild` with a date that has no L0).
- One run at a time per venue: an exclusive `flock` (`LOCK_EX | LOCK_NB`) on `crypto/l1/.md-archiver-<venue>.lock`; held → exit 1 with a message. Leftover `*.parquet.tmp` files under `crypto/l1/<table>/venue=<venue>/` are deleted at start, under the lock.

### D2. Which dates are built

- A date D of venue V is **day-sealed** when: `crypto/l0/_manifest/venue=V/date=D.jsonl` exists and every line is complete JSON; no `.partial` file exists under any folder of D (by_symbol, instruments, `_session`, `_conn`); `crypto/l0/_open/venue=V/date=D/` holds no file; every sealed file of D on disk has exactly one manifest line and its sidecar, and every manifest line's file exists with size = `bytes`. (No SHA-256 check: that is the auditor's job, contract 1.1.) A date that fails this is skipped with a log line; inside `--rebuild` it is exit 1.
- A date **has L1** when `crypto/l1/gap/venue=V/date=D/day.parquet` exists. The gap file is always written for a built date (also with zero rows) and is renamed last (D7), so it marks a finished build.
- A date is built only when D−1 is day-sealed or has no L0 at all (no manifest and no L0 folder of D−1): the carry-in must be final, so that a later run gives the same rows. Otherwise D waits (log line; inside `--rebuild` exit 1).
- Without `--rebuild`: every day-sealed date without L1, oldest first. With `--rebuild FROM..TO`: every date of the range that has L0, in order, built again even when it has L1.
- Retention (D9) runs after the builds.
- Layout `by_connection` (contract 2.1) is not supported by U7: a manifest line with `layout = "by_connection"` refuses the run (exit 2). The default layout is `by_symbol`.

### D3. Input model (`input`)

- From the manifest lines of D and of D−1 (carry-in, only when D−1 is day-sealed): the topic files (`by_symbol`, `instruments`), the session files. `contract_ver` > 1 in a manifest line or header → exit 2.
- **Connection of a record** (contract 2.2, 2.5, 3.1): a connection is `(collector, session_ns, conn_id)`. In a topic file the session comes from the manifest line's `sessions` by `rec_no`; in a collector's session file from the header. REST records (`conn_id = 0`) and notes belong to no connection. Unroutable records in the sealer's session file have no known session and are ignored for connections (judgement; they have no topic either).
- **Pass 1 (connections)**: one scan over every record of D−1 and D gives, per connection: first and last `recv_ns` over all its records (all topics and its control messages, contract 3.2), and the set of topics it carries with the first record of each topic. Notes are collected with their file and `rec_no`.
- **Instrument in effect** (contract gap: the contract does not say which response applies; judgement, documented in ADR-020): for a record of symbol S, the instrument row of S from the latest instruments response with `recv_ns` ≤ the record's `recv_ns`, over the instruments files of D−7 … D of that market; when none is at or before it, the earliest response of that window that names S (the collector fetches instruments before it subscribes, so this only covers a response that is a few ms late). No row in the window → the record is a `parse_error` (`no instrument row for S`). The window never looks after D, so the result of D does not depend on when the run happens.
- Parsing uses `BybitAdapter::new(market, &instruments)` and `route` + `parse` exactly as the md-bybit golden test does. A record of a topic file whose `route` is not that topic is a `parse_error`.

### D4. Replay per topic (`replay`)

One topic `(market, channel, symbol)` at a time; topics are independent (parallel over `--threads`). Records of the topic's files of D−1 then D, in file and `rec_no` order. D−1 is replayed only to build the state at 00:00:00 of D; it writes no row.

Per connection that carries the topic the replay keeps: whether it delivered a snapshot, and the last `update_id` since its last snapshot (sequence tracking stops at a break until the next snapshot). Per topic it keeps the **followed connection** and the **book state** (`OrderBook` of md-core + last applied record).

Order book topics (`is_book`):

- **Snapshot** on connection c:
  - c already delivered a snapshot of this topic → `resnapshot` row (evidence the snapshot; contract 3.5). This is per connection: a snapshot on the same `conn_id` of another session is another connection and never a `resnapshot`.
  - c is the followed connection, or there is none → the book starts again from the snapshot.
  - another connection f is followed and f **overlaps** c (f's last record, any topic, is at or after the snapshot's `recv_ns`) → `handover` row (from f to c, evidence the snapshot), c becomes followed, the book starts from the snapshot.
  - f does not overlap → c becomes followed, the book starts from the snapshot (the `reconnect` row comes from D5).
- **Delta** on connection c: if c delivered a snapshot and follows the sequence rule (Bybit `u` = previous `u` + 1) → tracked; if it breaks the rule → `seq_gap` row (`start_recv_ns` the last good delta of c, or the snapshot when there was none, `end_recv_ns` this delta, `last_good_update_id`, `next_update_id`) and tracking of c stops until its next snapshot. Only the followed connection's book applies its deltas; a break on it ends the book state.
- **Book state ends** (contract 3.2) at the earliest of: a `seq_gap` or `parse_error` on the followed connection; the followed connection's last record (any topic, from pass 1) when no other connection that carries the topic overlaps that time (a connection that carries only other topics does not count); the `start_recv_ns` of a `collector_down`, `disk_floor` or `stale` note that covers the topic; the followed connection's last record kept when a `crash_truncate` note names an open file of it and no topic-carrying connection overlaps. The end of a collector session ends its connections.
- Every snapshot and delta row is written to `book_msg` (nothing is removed across connections), applied or not.
- **Checkpoint rows**: at each hour start H of D (00:00 … 23:00), a topic whose book state is alive after every record and end with time < H gets one row: `kind = 2`, `recv_ns = H`, levels = the full book best price first, `venue_ts_ns`, `update_id`, `venue_seq`, exponents and lineage of the last applied record, `prev_update_id` and `first_update_id` of that record. A topic with book state at H gets the row also when it has no message in that hour. Small book channels get one checkpoint row at 00:00 in the day file.

Trade topics: the followed connection moves when a connection delivers its first record of the topic while the followed one overlaps it → `handover` row (evidence that first record). Rows are de-duplicated by `(venue, market, symbol, trade_id)`, keeping the earliest `recv_ns` (ties: smallest `(l0_file_id, rec_no)` — judgement: contract says smallest `rec_no`, which is ambiguous across files), over D−1 and D (a D row whose key was kept in D−1 is dropped). A dropped row is counted on the `handover` row whose `(from, to)` pair is the two connections of the kept and the dropped row; when no such row exists the drop is still made and the count goes to a log line (contract 7: "when no other drop reason applies"). `detail.dropped_trade_rows` is present on every trade-topic handover row (0 when none).

Ticker topics: one row per message (no merge, no de-dup). Instruments: one row per symbol of each response; `conn_id = 0`, `frame_no = 0` from the record.

### D5. Gap rows from connections and notes

- **reconnect** (per topic, from pass 1): connection A that carries the topic ends (its last record, any topic) and a later connection B that carries the topic starts, and no connection that carries the topic overlaps A. Evidence B's first record of the topic. `start_recv_ns` A's last record, `end_recv_ns` the evidence, `conn_id` B, `detail {from_conn_id, from_session_ns, to_conn_id, to_session_ns}`. Between two collector sessions the row is skipped when a `collector_down` note that covers the topic overlaps `[A last, B first]` (judgement for "covers the restart"). REST topics (`instruments`) have no connections and no reconnect or handover rows.
- **Note kinds**: one row per note and covered topic, `start_recv_ns`/`end_recv_ns`/`detail` from the note, `conn_id` the note's, evidence the note record. Covered topics: `topics` when given; null with `conn_id` → the topics that connection carries (the note's own session); null and no `conn_id` → every topic with records in the topic files of D or D−1 (contract 3.5 for `collector_down`; judgement for the others).
- **crash_truncate**: only notes whose `detail.file` names an open file (`crypto/l0/_open/…/<collector>-<session_ns>-c<conn_id>.qraw.zst.partial`); one row per topic that connection carries, `conn_id` from the file name. A note about a collector's session file gives no row (contract 3.5).
- **parse_error**: the record, `detail {"error": reason}`, the record's `conn_id`.
- A gap row belongs to the date of its evidence record's L0 file; rows whose evidence is in D−1 are not written by the build of D. `detail` is compact JSON with sorted keys.

### D6. Files and rows (`parquet_out`)

- One generic writer driven by the vendored table schema (`contracts/lake/l1/<table>.table.json`, read with `include_str!`): column names, order, types and nullability come from it. Types: `string` → BYTE_ARRAY (String), `int8`/`int16` → INT32 (Integer 8/16 signed), `int32` → INT32, `int64` → INT64, `bool` → BOOLEAN, `list<int64>` → three-level LIST of required INT64. Nullable → OPTIONAL, else REQUIRED.
- zstd level 9; key-value metadata `contract_ver`, `parser_ver`, `table`, and for `book_msg` `first_update_id` / `last_update_id`: the `update_id` of the first and the last snapshot or delta row of the file in sort order that has one, decimal, empty when none (the contract does not define them for a file of many symbols; judgement, reported to qdp as a contract question).
- Rows in the contract's sort order per file. Row groups are cut at about 64 MiB of buffered values (judgement), so a writer's memory is bounded.
- Paths and partitions as contract 3.1. A row's date is its L0 file's date (checkpoint: its own `recv_ns`); its hour is the UTC hour of `recv_ns`, clamped to `00` / `23` of the date. A `book_msg` hour file exists for every hour with a snapshot, delta or checkpoint row; `trade`, `ticker`, `instrument` files only when they have rows; the `gap` day file always.
- **Determinism**: no build time, no host name, no thread-order dependence: rows are sorted before writing and files are written in path order. The same L0 and `parser_ver` give byte-identical files.

### D7. Writing a date atomically

1. With `--rebuild`, the date's gap file is deleted first (the date counts as not built until step 4).
2. Every file is written to `<path>.tmp`, fsynced.
3. Every existing L1 file of the date and venue that the new build does not produce is deleted (stale hours); every `.tmp` is renamed to its final name, the gap file last; the parent directories are fsynced.
4. The date is built. A crash at any step leaves no gap file, so the next run builds the date again (all of it).

### D8. Disk floor

Before each date: `statvfs` (rustix, as md-collector) of the lake root. Free bytes below `--disk-floor-bytes` → no new build starts, log, exit 1. A build in progress finishes.

### D9. Retention

`--retain-l1-days N` (unset = keep forever): for each date D < (UTC date of now) − N that has L1: delete the `book_msg`, `trade` and `ticker` date folders of the venue, only when D is day-sealed (D2) and `crypto/audit/venue=V/date=D.json` exists, is complete JSON with `contract_ver` ≤ 1 and `status = "clean"`. The `gap` and `instrument` day files stay (judgement: they are small, and the gap file marks the date as built so it is not rebuilt; `--rebuild` brings the rest back). Never deletes or changes anything under `crypto/l0/` or `crypto/audit/`.

### D10. Logs

One line per date on stderr: date, files, rows per table, gap rows per kind, seconds. Warnings for skipped dates and unattributed trade drops. No metrics in U7 (U20a adds archiver metrics if wanted).

## Testing Decisions

- Tests build a lake tree in `OMS_TEST_DIR` (else `CARGO_TARGET_TMPDIR`) from the golden L0 segments: each segment compressed as one zstd frame and appended to its day file, manifest lines with `sessions` from `session_runs.json`, sidecars. One extra synthetic instruments response for 2026-01-01 (same body, `recv_ns` before 23:00) is added so the date before has an instrument row (D3); the test asserts its extra `instrument` file separately and compares every golden L1 file exactly.
- Golden: build 2026-01-01, 2026-01-02 and 2026-01-05 → every file of `golden/l1/*.json` has exactly those rows in that order; no other `book_msg`, `trade`, `ticker` or `gap` file exists. Rows read back from Parquet with the `parquet` reader.
- Schema read back equals the contract table JSON (names, order, types, nullability) and the metadata keys.
- Determinism and idempotence: a second run and a `--rebuild` give byte-identical files; L0 bytes (SHA-256 of every file under `crypto/l0/`) unchanged.
- Synthetic scenarios (built with md-lake writer + seal, or directly as frames): a single sequence gap gives one `seq_gap`; a `collector_down` note gives one row per topic of the date; overlapping connections give both connections' rows and one `handover`; a trade on two connections appears once and the handover counts 1; checkpoint rows equal a reference replay from the day's first snapshot; two missed days are built by one run, oldest first; a crash between steps of D7 (injected) leaves the date unbuilt and the next run builds it.
- Retention: only dates older than N with sealed L0 and a clean audit lose `book_msg`/`trade`/`ticker`; `not_audited`, `fail`, missing audit, a not day-sealed date keep everything; L0 unchanged.
- Refusals: unknown venue, `by_connection`, `contract_ver` 2, future `--now-ns` → exit 2 and nothing written; disk below floor → exit 1 and nothing written; a held lock → exit 1.
- Code written before its tests is checked by breaking each listed rule on purpose (as U4–U6).

## Out of Scope

- Layout `by_connection` input; Binance, OKX, Hyperliquid (U10, U11).
- A carry-in state cache (each build replays D−1 again; measure first).
- Metrics, timers enabled, the auditor (U8), DuckDB query helpers (U9).
- A topic whose book state lived across D−1 without any snapshot in D−1 (state from D−2): the build of D starts without it until the next snapshot. With the daily handover this needs a failed day change; documented as a limit.
