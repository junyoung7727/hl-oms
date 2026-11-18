# Spec: md-lake, the L0 writer, hour seal, day seal and verify library (unit U4)

Status: ready-for-agent

Design source: `RESEARCH/CRYPTO_TRADING_INTEGRATED_TN_20261118/10_DESIGN_DRAFT.md` §5.4.3 and §6.2 (S4, S7), and the unit entry U4 of `pr_plan.json` in the same folder (outside this repository). Contract: `contracts/lake/` (vendored) and its document `docs/crypto/lake-contract.md` in quant-data-platform, §2 (L0) and §5.1–5.2 (manifest line, verify result). Tickets: `issues/` next to this file.

## Problem Statement

The collector (U5b) must write everything it receives to disk at once, and the lake must end up with one sealed file per topic and UTC date, losslessly, even when the collector or the sealer crashes at any byte. The contract fixes the files, the lock rule, the hour seal, the seal log, the restart rules and the day seal (§2). Nothing in this repository writes or reads those files yet.

## Solution

A library crate `md-lake` with no venue knowledge and no executable (the `md-lake seal|verify` binary is a target of md-collector, U5b):

1. **QRAW1 codec and paths**: records and headers (contract 2.2), path building and parsing from `l0/paths.json` (2.1).
2. **zstd layer**: a frame writer with block flush, a frame scanner that finds the last complete zstd block and the frame end (RFC 8878), and a stream reader over concatenated frames.
3. **Stage 1 writer** for the collector: one open file per connection and UTC hour and one session file per UTC date, under the lock rule (2.4).
4. **Hour seal** (2.5, 2.6): `by_symbol` (default) and `by_connection`, recovery of unlocked files, strict hour order, the unroutable guard, bounded memory, the restart rules.
5. **Day seal** (2.7): step 0, rename, sidecar, manifest line with `sessions`, restart rule.
6. **Verify** (5.2): decode sealed files and report counts, framing errors and first/last `recv_ns`.

## User Stories

1. As the collector developer, I want a writer that loses at most about 1 s of data on a process crash and 5 s on a power loss, so that the lake holds what we received.
2. As the operator, I want the sealer to never touch a file its collector still holds, and to seal hours strictly in order, so that a slow or crashed collector cannot cause a partial or reordered day.
3. As the operator, I want a crash of the sealer at any step to end, after a restart, with every record exactly once, so that I never repair the lake by hand except in the one case the contract names.
4. As the researcher, I want one file per topic and day whose records I can assign to their connection by `rec_no`, so that replay matches the contract (3.2).
5. As the owner, I want the seal's memory not to grow with the hour's size, so that it fits next to the OMS on this PC.

## Implementation Decisions

- **Dependencies.** `zstd` (with `zstd-safe`; libzstd 1.5.x, built by `zstd-sys`), `serde_json`, `sha2`, `hex`. File locks use the standard library (`File::try_lock`, `File::lock`, flock on Linux). Dev: `regex` for a test-only JSON-schema subset validator, `libc` for `getrusage` in the memory test. No `oms-*` dependency; md-core is not needed (the classifier is a function argument).
- **Classifier.** Seal functions take `&dyn Fn(&Record) -> Option<Topic>` where `Topic = {market, channel, symbol}`. Notes (`src = 2`) are never classified; they go to the session file. REST instrument bodies are classified by the caller like any record.
- **Injected world.** A `Clock` trait (now in ns) and an `Fs` trait over the file operations the seal uses (create, open, append, set_len, rename, remove, fsync of file and directory, try_lock, read_dir). The real `Fs` is a thin wrapper. A test `FaultFs` fails the Nth call (a simulated crash: the call returns an error and the run ends), so a crash can be injected at every step. All paths are under the configured lake root; `Fs` refuses a path outside it.
- **Test directories** are under `OMS_TEST_DIR`, else `CARGO_TARGET_TMPDIR`, as in `oms-node/tests/common/mod.rs`; never `std::env::temp_dir()` (`/tmp` on this PC is tmpfs).
- **Stage 1.** `OpenWriter::create` writes `<name>.tmp`, takes the exclusive lock, renames to the open-file name, then writes the magic and the header. Level 3, one compression context per connection (counted through an injected codec factory). `flush` at 1 s (`ZSTD_e_flush`), `fsync` at 5 s, both settings, driven by the injected clock through `tick(now_ns)`; rotation at each UTC hour; `close` ends the frame, fsyncs and only then drops the lock. The session file of a collector: one zstd stream per UTC date with the same rules; a record goes to the date of its `recv_ns`, or to the newest started date when that date is already closed.
- **Recovery** (sealer only, unlocked files only): an open file without a frame end is cut to its last complete zstd block, then the reader keeps complete records only; one `crash_truncate` note per such file per attempt (2.5). A `.tmp` file the sealer can lock is deleted.
- **by_symbol grouping.** Pass 1 reads each open file of the hour once and counts records and bytes per topic. Topics are grouped inside one connection slot only (the topics of one open file; a topic seen in several files of the hour is merged from all of them and belongs to the slot of its first file). A group is closed when the sum of its level-9 context sizes would pass the budget (default 0.5 GB). The context size of a topic is `ZSTD_estimateCStreamSize_usingCParams` for level 9 with its pledged size (the counted bytes), so a small topic gets a small context. The largest number of live contexts goes to the done entry (`live_contexts_max`).
- **Merge.** For each group: a k-way merge by `recv_ns` over the files that hold the group's topics; one session run per collector session in `session_ns` order (ties: collector); inside a run ties by `conn_id`, then `frame_no`; a connection's records keep their file order. Each topic's frame is written with `pledged size = counted bytes` and appended to its day file.
- **Begin and done entries** follow 2.6 exactly; `sha256` is over the appended uncompressed record bytes in appended order; counts and digests are checked against pass 1 before the done entry. The sealer's session file (`<sealer>-<start_ns>` of the run's first start that date) is a target of every hour that adds to it.
- **Unroutable guard.** If unroutable market-data records (`src` 0 or 1) are more than the setting (default 1 %) of the hour's records, the run stops before the begin entry, changes nothing and returns an error.
- **Strict order.** Hours of a venue are sealed oldest first; an hour with a locked or failed open file stops the run for every later hour.
- **collector_down.** An hour with no open file gets a begin and a done entry with zero records and a `collector_down` note; a gap of more than 10 s (setting) between the last record of one collector session and the first record of the next gives one `collector_down` note with both session numbers in `detail`.
- **by_connection** recompresses each open file whole into one level-9 frame of its connection's day file. `measure_layouts` also compresses the hour in the other layout into a counting sink and writes `measure` to the done entry.
- **Day seal and verify** follow 2.7 and 5.2. `verify` reads frames, counts records, reports framing errors and first/last `recv_ns`; it does not parse payloads.
- **Out of scope here**: the binary, systemd units and metrics (U5b, U20a); the archiver (U7).

### Decisions from the spec review (2026-11-18, findings F1–F15)

- **F1 Pledged size** = the frame's uncompressed bytes: the topic's record bytes, plus `10 + header_len` when the frame is the first of its file (`offset_before = 0`), because that frame also holds magic and header.
- **F2 by_connection** appends the open file's records only, as one level-9 frame; the first frame of the day file starts with a `connection` header with the day's date. Never the open file's own header again.
- **F3 Sealer session file.** One per venue and UTC date: the `.partial` file of the sealer's writer name for that date if it exists, else `<sealer>-<now_ns of this run>` created when the first record must go in. It is a target of **every** hour seal (begin and done), also with nothing to add: then no frame is written, no file is created, `offset_after = offset_before`, `records = 0`, `sha256` of the empty input (golden `hour=02`). Its date is the sealed hour's date (golden `hour=20`).
- **F4 Check.** Pass 1 keeps, per topic, the count and an order-independent digest (wrapping sum of the per-record SHA-256 as four `u64` lanes). After the append, the sealer reads every appended frame back from the target, recomputes the ordered SHA-256 of 2.6 and the per-topic digest, and compares both with what it wrote and with pass 1. Any difference stops the run before the done entry.
- **F5 Rotation** is forward only (an hour is never reopened when the clock steps back; records go to the current file and a `clock_step` note is the collector's job, U5b). The `.tmp` → open-file step uses `hard_link` + remove of the `.tmp` name, which fails when the name exists (no replace); the lock moves with the inode.
- **F6 Session gap.** For hour H, the sessions with records are taken in `session_ns` order. For each session whose first record lies in H, the previous session is the one before it in H, or else the last session of the latest earlier hour with records (from its done entry `sessions`). Its last `recv_ns` is read from H's open files or from the frames that done entry appended (offsets in the seal log). A gap > 10 s (setting) gives one `collector_down` note in H's seal: `start_recv_ns` = that last record, `end_recv_ns` = the next session's first record, `detail` = `{prev_session_ns, next_session_ns}` (golden hour `01`). The inputs are sealed files, so a redo writes the same note.
- **F7 Start hour.** A sealer setting `first_hour` (UTC). Hours before it are never sealed; from it on, every closed hour is sealed, empty or not (golden `hour=20` of 2026-01-01).
- **F8** The collector's session file is rolled by `tick` at UTC midnight, also when no record arrives.
- **F9 Note records** (pinned to the golden bytes): payload keys in the order `note, writer, conn_id, topics, start_recv_ns, end_recv_ns, detail`, compact; record `recv_ns` = the sealer's clock when it writes the note, `src = 2`, record `conn_id` = the note's `conn_id` or 0, `frame_no = 0`. `crash_truncate` of an open file: `conn_id` = the file's, `start_recv_ns` = the last record kept (the file's hour start when none), `detail.file` = the open file's path. Order inside the sealer's frame: `crash_truncate` notes, then `collector_down` notes, then unroutable records in merge order. Step 0 writes a header from the path with `collector_ver` and `host` = `"unknown"` when no complete header is left.
- **F10 Simulated file system.** Crash tests run on `SimFs`, an in-memory `Fs` that keeps durable and volatile state: file bytes become durable at the file's fsync, directory entries (create, rename, link, remove) at the directory's fsync. A crash keeps the durable state plus, for each file, a random prefix of its volatile appended bytes (a torn write). Lock and child-process tests use the real `Fs`.
- **F11 Memory.** Besides the 50 MB / 200 MB comparison, a test with 4 topics of 100 MB each checks `ru_maxrss − max live context bytes ≤ 96 MB`, which fails for a design that holds a topic or a group in memory.
- **F12** The lock test records, at the rename call, that the lock is already held.
- **F13** Ticket 04 rebuilds the open files of the golden restart hour (2026-01-05 09) from the golden segments, seals them, and compares records, SHA-256, `sessions` and the note bytes with the golden seal log and `09.qraw`.
- **F14** A record across a frame boundary is refused (ticket 02, done); `verify` reports it (ticket 08). Ticket 04 counts read calls on a locked file (must be 0).
- **F15 Seal lock.** A run takes `LOCK_EX | LOCK_NB` on the directory `crypto/l0/_seal/venue=<venue>/` for its whole length (no extra file). A second run exits at once. Each open file's lock is taken at the check and held until the file is deleted or the run ends.

## Testing Decisions

- **Seams.** Seam A: the public API of `md-lake` (writer, seal, day seal, verify, reader) in `crates/md-lake/tests/`, with the injected clock and `FaultFs`. Seam B: the vendored golden files (golden L0 segments, seal logs, manifest, verify result) and the contract schemas, read by tests. Seam C: a child process (the test binary itself, run with an environment variable) that holds a lock.
- **Tests prove** the done criteria of the U4 plan entry. The tickets list them.
- **Memory tests** measure `ru_maxrss` of a child process (Seam C): two synthetic hours of 50 MB and 200 MB (200 topics) with a 4-context budget differ by at most 64 MB. The `#[ignore]` test `seal_rss_2gb` (≥ 2 GB, 0.5 GB budget, peak ≤ 1 GB) runs only in this unit's checks, in release mode; its numbers go into the PR.

## Out of Scope

The `md-lake` binary and its timer (U5b), metrics (U6), L1 (U7), the qdp auditor (U8), any change to `oms-*` crates, `.github/`, the journal or registry contracts.

## Further Notes

- Built directly by Claude Opus 5.5 (2026-11-18, owner: "이제 개발을 너가 직접해"); lean spec (owner: "검증 사소한 것까지 하지마").
