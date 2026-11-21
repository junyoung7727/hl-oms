# Spec: OMS direct alerts (unit U18a, PRs U18a1–U18a2)

Status: ready-for-agent

Design source: `RESEARCH/CRYPTO_TRADING_INTEGRATED_TN_20261118/10_DESIGN_DRAFT.md` §7.6 (kill switch), §7.7 (direct alerts), §8.3 (alert paths, severities), the TN chapter text `note/05_trading.html` §7.7 (repeat rule, second channel, fatal exit) and `note/02_map_state.html` §4.6 (PR #3 verdict, table 4-4), `pr_plan.json` unit U18a. Owner decisions: grill Q7 (two alert paths, the OMS sends trading-safety alerts itself; `01_GRILL_DECISIONS.md` row Q7, message d0d7d01d), Q18 (same bot, prefixes), D3 (second channel, not decided). Delivery rules: draft PR #3 (`tools/alert/hl_alert.py` on `origin/telegram-alert`, head c9c7337), as already built in `crates/alert-direct` (U6, `.scratch/obs-push/spec.md` D1, R1–R3, F1–F7).

Numbers marked 판단(모델 아님) are judgements without a model or a measurement behind them.

## Problem Statement

The OMS refuses orders, latches REDUCING, raises mismatches and stops on a journal failure, and nobody is told unless someone looks at the dashboard (U-11, `registry/user-decisions.toml:94-100`; ADR-016 §8, `docs/adr/ADR-016-observability.md:112`). The design (§7.7, Q7) has the OMS send these trading-safety alerts to Telegram itself, independent of the observability stack, and only after the event that causes the alert is durable (ADR-004). Draft PR #3 solved the same need with an external script that polls `/api/status` every 30 s; it misses conditions that start and clear between two polls, and the design supersedes it (`note/02_map_state.html` §4.6). The sender, its delivery rules and the repeat rule exist in `alert-direct` (U6). What is missing: the OMS conditions, their wiring behind the commit gate, the fatal-exit message, a second channel for when Telegram is unreachable, and the configuration.

## Split

| PR | Scope | Size (판단(모델 아님)) |
|---|---|---|
| U18a1 | `alert-direct` only: "Telegram failing since", the `SecondChannel` trait with its own bounded outbox and thread, the Telegram-down notice on the second channel. Additive; md-collector behaviour unchanged. | ≈ 250 lines of code + 250 of tests |
| U18a2 | `oms-node`: condition table, the `oms-alert` evaluator thread reading only the published (durable) state, fatal-exit message, start/stop notices, `[alert]` config, `status.alerts`, failpoints tests; docs (ADR-029, contract C-ALERT, runbook, U-11, ADR-016 §8, README line, glossary), registry claims, `docs/generated`. The PR description says "Supersedes #3 (not merged)". | ≈ 1,000 lines of code + tests, plus documents |

U18a2 depends on U18a1. U18b (kill switch, monitoring gate) builds on U18a2 (see "Handoff to U18b").

## Facts from the existing code

- Commit stage: records are written, then `writer.sync()`; on error `Committer::fail` trips the fatal latch and closes the queues, and the function returns before anything is released (`crates/oms-node/src/pipeline.rs:129-133`, `:165-172`). Only after the sync succeeded: step 1 updates the read model (`:182-236`), step 2 releases replies, gateway commands and subscriber events (`:238-274`). `shared.durable_seq` is stored after the sync and before step 1 (`:180`).
- The published read model "never runs ahead of the journal" (`crates/oms-node/src/published.rs:9-10`); its `durable_seq` is set per published record (`:215`). Status and the reconciler read only it (`status.rs:3-4`, `:29`).
- Holds are derived from state on every call (`crates/oms-core/src/core.rs:177-224`); reasons in `HoldReason` (`model.rs:296-327`). Strategy halts are not in `holds(None)` (`core.rs:196-200`); they are in `core.halt_strategy` (`core.rs:62`). Unacked mismatches (`core.rs:63`, `model.rs:367-376`, kinds `model.rs:344-365`, including `VenuePositionChange` and `UnclassifiedExecution` from U13b). REDUCING with its rule, detail and time (`model.rs:485-491`, rules `:467-481`). Margin freshness `margin_report_is_fresh(at)` (`core.rs:258-268`), default max age 30 s and stale cancel 60 s (`oms-core/src/config.rs:143-144`).
- The Bybit gateway's block view (`BlockView`, `gateway.rs:854`, built at `:902`) is `create_block()` = link block or clock block (`venue/bybit/checks.rs:95-97`). It is process state, not journaled; status shows it (`status.rs:82-85`). Sim and Hyperliquid have none (`node.rs:155`, `:164`).
- Fatal stop: `node::run` waits for `fatal.wait()` and takes code 2 (`node.rs:291-295`), closes the queues, joins the threads (`:303-313`), logs and returns (`:349-352`). A panic aborts (`node.rs:18`).
- alert-direct: message `"{prefix} {venue} {KST}\n{body}"` (`crates/alert-direct/src/lib.rs:726-733`); split at 3,500 UTF-16 units (`:24`, `:177-210`); outbox 50 parts, drop oldest and count, one notice after a full delivery (`:26`, `:256-265`, `:295-309`); 400/413 drop and count, everything else retry (`:942-954`); routes DNS then `api.telegram.org` resolved to 149.154.167.220 with SNI and certificate for the host (`:517-535`, `:644-646`); request timeout 10 s, connect 3 s, flush every 5 s (`:577-581`); repeat rule with start, every `repeat_ms`, clear `해소: <start text>` (`:376-402`); token file 0600, no symlink, not inside a repository (`:132-162`); `set`/`once` only take a mutex, but `set` with a `state_file` writes and fsyncs the file in the caller's thread (`:746-769`). No "failing since" and no second channel exist (`AlertState`, `:589-602`).
- Test seams: `fail_next_sync` (failpoints only, `pipeline.rs:73-76`, used by `tests/pipeline.rs:168`); process failpoints `OMS_CRASH_AT=sync_error_after_write` and `OMS_CRASH_ON=reducing_entered` (`pipeline.rs:84-103`, `tests/crash_points.rs:133-160`); loopback-only env hook pattern `OMS_TEST_BYBIT_WIRING` (`gateway.rs:151-153`); `http-fake` with `Reply`, `Hang`, `Close` and `dead_addr()` (`crates/http-fake/src/lib.rs:15-22`, `:181`).
- Config errors exit 2 (`main.rs:44-48`). Mainnet already refuses to start without a margin policy (`config.rs:354-355`, `:388`).

## U18a1 decisions (alert-direct)

- **Failing since.** The sender thread keeps `failing_since_ms: Option<i64>` (injected `Clock`, now also handed to `run`). Set at the first `Retry` outcome while it is `None`. Reset to `None` on `Delivered` or `Refused` (Telegram answered: it is reachable). An empty outbox starts no episode: an outage with nothing to send is not detected (no probe; see Out of Scope). `AlertState` gets `failing_since_ms`.
- **Second channel trait.**
  ```rust
  pub trait SecondChannel: Send + Sync {
      fn name(&self) -> &str;
      /// Blocking; called only on the `alert-second` thread. Err = keep the part and retry.
      fn send(&self, text: &str) -> Result<(), String>;
  }
  ```
  `AlertConfig.second: Option<Arc<dyn SecondChannel>>` (default `None`; `Debug` written by hand, never the token) and `second_after_ms: i64` (default 60,000, design §7.7 판단 there).
- **Tee.** Every message made by `once` or `set` is also added, whole, to a second `Outbox` (same type, same split and the same bound `outbox_limit`; 판단(모델 아님): one rule for both). Its own thread `alert-second` flushes on notify and every `flush_every`; `Err` keeps the part and ends the flush (no permanent-refusal class: the trait has none; the bound limits memory). Its drop notice is its own.
- **Telegram-down notice.** On every wake of the sender thread (notify or `flush_every`), if `failing_since_ms` is `Some(t)` and `now − t > second_after_ms`, the condition `telegram_down` holds on the second side's own `Repeat` (start, every `repeat_ms`, clear when `failing_since_ms` returns to `None`). Text: `Telegram 전송이 {s}초째 실패함. 대기 {queued}개. 마지막 오류: {last_error}` (errors are already token-free, R1). This notice goes to the second channel only. Detection is at most `second_after_ms + flush_every + one attempt` (≤ 60 + 5 + 13 s with the defaults).
- `flush_for(d)` waits until both outboxes are settled or `d` passes. `AlertState.second: Option<{name, queued, delivered, dropped_total, failures}>`.
- With `second = None` nothing changes for md-collector and `md-lake` (no thread, no tee).

## Testing Decisions (U18a1)

alert-direct `tests/alert.rs`, loopback `http-fake` only, manual clock:
1. `failing_since_set_and_reset`: 500 → set at the first attempt's clock time; 200 ok → reset; 400 → reset; empty outbox → stays `None`.
2. `second_channel_gets_every_message`: `once` and `set` start/repeat/clear each reach the fake second channel exactly once, also when Telegram delivers.
3. `second_outbox_bound`: fake second always `Err`; 60 messages → 50 queued, 10 counted; after it recovers, 50 parts then its drop notice.
4. `telegram_down_notice_after_60s`: Telegram 500, `flush_every` 20 ms; clock +59 s → the second fake holds only the P1 copy; +61 s → the notice within 1 s real time; +5 min → one repeat; Telegram 200 → `해소:` notice.
5. `flush_for_waits_for_both` and `second_off_by_default` (`state().second == None`, no `alert-second` thread).
6. The existing U6 tests pass unchanged.

## U18a2 decisions (oms-node)

### Conditions (the alert list)

A pure function `alerts::conditions(core: &Core, now_us: Ts, gateway_block: Option<&str>) -> Vec<Cond { key, text, grace_ms }>` over the published core. Each body ends with the line `근거: 저널 seq {published.durable_seq}`. Texts reuse PR #3's `HOLDS` table with numbers added.

| Key | Triggering durable state (start) | Clear | Grace |
|---|---|---|---|
| `hold:reducing` | `HoldReason::Reducing` (margin breach: daily loss, mm buffer, account mm rate; rule, detail, since) | `ReducingCleared` (operator `clear_reduce`) | 0 |
| `mismatch:<kind>` | ≥ 1 unacked mismatch of that `MismatchKind` (count, newest 3 subject: detail — 3 판단(모델 아님)); covers the reconciler's mismatches and `VenuePositionChange` / `UnclassifiedExecution` | no unacked mismatch of that kind | 0 |
| `hold:operator_account` | `halt_account` set (note) | resume | 0 |
| `halt_strategy:<s>` | `halt_strategy` holds `s` (note) | resume of `s` | 0 |
| `hold:storage_low` | `storage_low` (measured every 5 s; can flap near the threshold, review S2) | `StorageLowChanged{low:false}` | 30 s |
| `margin_stale` | margin policy set, link up, `!margin_report_is_fresh(now)` (age or "none yet"; stale-cancel time) | a fresh report | 30 s |
| `hold:link_down` | `LinkDown`; text adds the gateway block if any | link up | 30 s |
| `hold:resync` | `Resync` | confirmed | 30 s |
| `hold:in_doubt` | `InDoubt` (count, oldest `doubt.since`) | none in doubt | 30 s |
| `hold:venue_setting:<i>` | `VenueSetting(i)` | as configured | 30 s |
| `hold:account_mode` | `AccountMode` | allowed mode told | 30 s |
| `hold:no_config` | `NoConfig` | config active | 30 s |
| `gateway_block` | block view `Some(reason)` (key rejected, IP ban, account, unchecked, clock) | `None` | 30 s |

- The hold texts come from one `match` over `HoldReason` **without a wildcard arm**: a new variant does not compile until it has a text and a grace (this is how U18b's reasons get alerts). `Mismatch` maps to no condition (covered per kind); `OperatorStrategy` never appears in `holds(None)` (covered by `halt_strategy:<s>`).
- Grace: a condition must hold at every evaluation for `grace_ms` before its start is sent; one that ends inside the grace sends nothing. 0 for latches and operator acts, `alert.grace_ms` (default 30 s, 판단(모델 아님)) for states that every reconnect or boot passes through and for storage low, a measured state with no hysteresis in the core (review S2). A flapping state sends at most one start and one clear per grace period.
- Keys are bounded: hold kinds + instruments + strategies + mismatch kinds (16) + 2. So the outbox fills only through repeats during a long Telegram outage.
- `gateway_block` is the one condition not read from the journal: the block is gateway process state with no journal event. It reports that the gateway refuses to send and carries no journaled data, so it cannot contradict the journal (판단(모델 아님)). Every other condition is read only from the published core.

### Where the send is released, and why only after durable

- A thread `oms-alert` owns the evaluator. It never writes the journal or the core and is not on the commit path. Every `poll_ms` (100 ms, 판단(모델 아님)) it takes the published read lock; if `published.durable_seq` changed since the last evaluation, or `eval_ms` (1 s, 판단(모델 아님)) passed, it computes `conditions(...)` inside the lock, releases it, then calls `AlertHandle::set(key, Some/None)` for each key (repeat rule in alert-direct). The commit stage is not changed (C-COMMIT unchanged).
- Guarantee: the published core is updated only in step 1, after `writer.sync()` returned `Ok` (`pipeline.rs:165-172`, `:182-236`). A batch whose sync failed never reaches it (the function returns at `:171`), so no condition it would cause is ever seen, and nothing is sent for it. This is structural: the evaluator has no other source (except `gateway_block`, above).
- What can be missed: a grace-0 condition that starts and ends between two evaluations (normally < 100 ms; these are latches cleared by an operator act). Stated in C-ALERT.
- `AlertHandle` is created without `state_file` in the OMS, so `set` never fsyncs on the alert thread (`lib.rs:761-767`). Reason: oms-node runs with `Restart=no` (U20b), every start is made by a person, and a fresh start message per active condition after a restart is wanted (판단(모델 아님)).
- Notices: one `once` at the first evaluation after the configuration is active, `OMS 시작 (boot {n}, config {v}). 활성 조건 {k}개`; one at a clean stop (code 0), `OMS 정상 종료. 걸린 주문 {n}개는 거래소에 남음` (open orders from the published core). Both exercise the path (judgement; see owner question 4).
- Threading: `set`/`once` only take alert-direct's mutex; Telegram never blocks the commit stage or the core. The evaluator's read lock is held for the evaluation only (like `status`, `status.rs:29`).

### Journal flush failure (and any fatal stop) before exit

- The alert thread checks `fatal.is_tripped()` first on every poll. At the first trip it stops evaluating conditions (no clear or start after a fatal) and queues one `once`: `치명 종료: {reason}. 노드가 멈춤(exit 2). 마지막 내구 seq {durable_seq}. 걸린 주문은 거래소에 남아 있음, 워처와 운영자가 확인해야 함`. The text holds only the fatal reason (`commit failed: <io error>`, `pipeline.rs:130`; core fault with the input name only, `:469`) and the last durable seq: nothing of the failed batch.
- This message is the one exception to "after durable": it reports that durability failed. Delivery runs on the alert thread while `node::run` drains.
- `node::run`, after the drain and before `runtime.shutdown_timeout` (`node.rs:351`), calls `alerts.flush_for(exit_flush_ms)` (default 15 s = connect 3 s on the blocked route + 10 s request on the pinned one + 2 s, 판단(모델 아님)) on both exit paths (code 0 after the stop notice, code 2 after the fatal message). The drain-time fatal (`node.rs:313`) is handled by the same check.
- Best effort, stated: a failed exit flush loses the message (no state file); the second channel got its copy if configured; SIGKILL, abort or panic send nothing. Those cases are covered by the observability path (projector `oms_up` = 0, U14/U20b) and the watcher's W5 (U19b). During the exit flush `/api/status` still answers with `fatal` set (the HTTP task is not aborted, `node.rs:267-279`, `:296-297`).

### Token file

`[alert] telegram` must be an absolute path (판단(모델 아님): a relative path would resolve against the config directory, often a checkout). Loaded at config load with `TelegramConfig::load` (0600, regular file, no symlink, not inside any repository, token form; error texts never hold the token, `lib.rs:132-162`). A configured file that cannot be loaded is a configuration error (exit 2, `main.rs:47`), as in md-collector (U6 R4).

### Delivery rules

Unchanged from alert-direct / PR #3: parts ≤ 3,500 UTF-16 units; outbox 50 parts, overflow drops the oldest and counts, then `[OMS P1] 전송 실패로 알림 N개를 버림`; 400/413 drop and count; 401, 403, 429, 5xx, network error, timeout keep and retry every 5 s; routes DNS then 149.154.167.220 with the certificate for `api.telegram.org`, last good first; a timeout after delivery may send twice. Prefix `[OMS P1]`, venue label = the node's venue name (`sim`, `hyperliquid:<network>`, `bybit:<network>`, as `status.node.venue`), computed from the config so that the sender starts before the gateway.

### Second channel

`alerts::start(..., second: Option<Arc<dyn SecondChannel>>)`. The config accepts `second_channel = "none"` (default) only, until D3 names a service; any other value is refused. Tests inject a fake through the library function. With a channel, every `[OMS P1]` message and the Telegram-down notice (> `second_after_ms`, default 60 s) reach it (U18a1).

### Config (`[alert]`, optional section)

| Field | Default | Validation |
|---|---|---|
| `telegram` | required in the section | absolute path; loads (above) |
| `grace_ms` | 30,000 (판단(모델 아님)) | 0..=600,000 |
| `eval_ms` | 1,000 (판단(모델 아님)) | 100..=10,000 |
| `repeat_ms` | 300,000 (design §7.7) | 60,000..=3,600,000 |
| `exit_flush_ms` | 15,000 (판단(모델 아님)) | 1,000..=30,000 |
| `second_channel` | `"none"` | only `"none"` until D3 |
| `second_after_ms` | 60,000 (design §7.7) | 10,000..=600,000 |

- Section absent: no alerts, one `warn` log at start, `status.alerts = {"configured": false}`.
- Mainnet (`mainnet_confirmed = true`, Bybit or Hyperliquid) without `[alert]` is refused (as the margin policy rule, `config.rs:354-355`, `:388`); owner question 1.
- Test hook, only in a `failpoints` build (as `OMS_TEST_BYBIT_WIRING`): env `OMS_TEST_TELEGRAM_API` = an `http://127.0.0.1:<port>` base, refused for any other host; with it the routes are that one URL and the repository rule of the token file is skipped (the test directory may be inside a checkout). The shipped binary ignores the variable.

### Status

`status` gets `"alerts"`: `configured`, `queued`, `delivered`, `dropped_total`, `failures`, `failing_since_ms`, `last_errors` (token-free), `active` (keys), `second` (name and counters or null), with `source: "process counters"`. Neither the token nor the file path is shown.

### Documents and registry

- ADR-029 "OMS direct alerts": chosen = in-node evaluator over the published state; alternatives = PR #3 external polling (misses short states, U-11 text), hook in the commit stage (puts the alert mutex and its work on the commit thread), journaling alerts (puts I/O outcomes into the journal). Supersedes the ADR-016 §8 line "알림이 없다(U-11)".
- Contract `docs/contracts/alerts.md` (C-ALERT): the condition table, the release rule, what can be missed, the fatal exception, the gateway-block exception; `registry/contracts.toml` entry with `events = []` (no journal event is added).
- U-11 (`registry/user-decisions.toml:94-100`): recorded as decided by grill Q7 (2026-11-17, message d0d7d01d): the node sends them itself (the alternative "노드에 알림 기능을 내장"). README line "누적 체결량 한도와 알림은 없다" updated. Runbook: the token file, a test message (start notice), what each alert means and which command clears it. GLOSSARY: alert condition, grace, second channel, Telegram-down notice.
- Claims `V-ALERT-1..5` naming the tests below; `crates/xtask/src/verify.rs:57` node-unit suite gets `--test alerts`; the failpoints tests live in the existing `pipeline` suite files. `cargo run -p xtask -- docs` and commit `docs/generated/`.

## Testing Decisions (U18a2)

Loopback `http-fake` as Telegram only; manual clock (`alert_direct::Clock`); test directories under `OMS_TEST_DIR` / `CARGO_TARGET_TMPDIR`, never `/tmp`.

| Done criterion | Test |
|---|---|
| No Telegram request for an event whose batch failed to fsync (failpoints bundle) | `tests/pipeline.rs::no_alert_for_a_batch_that_failed_to_flush` (in-process: real core thread, commit thread, journal, alert thread; account halt committed normally → halt message arrives; then `fail_next_sync` + a second account halt on a resumed account → fatal; after 2 s the fake holds exactly one new request, the fatal message, and no request contains the halt text). `tests/crash_points.rs::a_flush_failure_sends_one_fatal_alert_and_nothing_of_the_batch` (process: `OMS_CRASH_AT=sync_error_after_write`, `OMS_CRASH_ON=reducing_entered`, two resting plain orders and a breaching margin report; exit 2; the fake got the start notice and one fatal message, nothing with REDUCING; the node exited within `exit_flush_ms` + 5 s) |
| The outbox bound holds and drops are counted | `tests/alerts.rs::outbox_bound_holds_and_drops_are_counted` (fake answers 500; one account halt and 59 clock steps of 5 min = 60 messages; `status.alerts.queued = 50`, `dropped_total = 10`; fake answers 200 → 50 parts, then one `알림 10개를 버림`) |
| A lasting condition: start, every 5 min with an injected clock, once at clear | `tests/alerts.rs::repeat_rule_with_an_injected_clock` (pipeline rig + alert thread: halt → 1; +4 min → 0; +5 min → 1 with the latest text; +10 min → 1; resume → 1 `해소:`; +5 min → 0) |
| Telegram failing > 60 s → the second-channel fake gets a notice | `tests/alerts.rs::telegram_failing_over_60s_notifies_the_second_channel` (fake 500; fake second channel gets the halt copy at once; +59 s → no notice; +61 s → notice within 1 s; fake 200 → clear notice) |
| Full local gate | the `checks` command of the unit |

Further tests (not in the done criteria, they hold the decisions):
- `alerts::tests` (lib): every `HoldReason` has a non-empty text and the grace of the table; `conditions` on cores built with `Core::handle` gives the table's keys; `mismatch:<kind>` lists the count and the newest 3; the fatal text holds the reason and the seq only.
- `tests/alerts.rs::grace_hides_a_short_link_down_and_reports_a_long_one` (link down 20 s → nothing; 31 s → start; up → clear).
- `tests/alerts.rs::a_hung_telegram_never_slows_the_commit_stage` (fake `Hang`; 1,000 halt/resume records commit within 5 s, 판단(모델 아님)).
- Config: absent section = no alerts and `configured: false`; relative path, mode 0640 file, symlink, file in a repository → error; each bound; `second_channel = "pushover"` refused; mainnet without `[alert]` refused; `OMS_TEST_TELEGRAM_API` with a non-loopback host refused.
- Mutants (xtask): grace check removed; fatal check moved after the evaluation. Both must be caught.

## Handoff to U18b

- New hold reasons (`KillLatched`, `WatcherMismatch`, `VenueChange`, `StrategyLink`) and the unmonitored gate get their arms in the hold-text `match` (compile-enforced) with grace 0 (판단(모델 아님)); `StrategyLink` keyed per strategy. "Mismatch reported by the watcher", "unmonitored" and "kill switch executed" then alert through the same path with no other change.
- The kill loop's progress messages ("executed or failing", start / count change / at most every 60 s / zero) use `Alerts::once` from the task that runs the released cancel-all command; they are after durable because the loop starts from a command released in step 2. Its "at most 12 P1 in 10 min" test counts requests at the fake Telegram.
- U19b: a non-null `fatal` in `/api/status` during the exit flush means the OMS is stopping.

## Out of Scope

- The kill loop, the monitoring gate and their hold reasons (U18b); watcher alerts `[WATCH P1]` (U19b, own sender); vmalert rules and Alertmanager (U20b).
- Choosing or building a concrete second channel (D3); an outside dead-man for the stack (D3, X5).
- A Telegram health probe (`getMe`) when nothing is queued; recovery-failure alerts (exit 3 before the sender starts: a person started the node and sees it); snapshot-error alerts (they cost recovery time, not correctness, `pipeline.rs:336-337`; P2 for vmalert); alert metrics in the projector.
- Merging PR #3. Closing it on GitHub is an outward action for the owner or with the owner's approval; the U18a2 PR description only states that it is superseded.
- Any live or testnet request, any committed token, journal or log.

## Open owner questions

1. **Mainnet without `[alert]`.** Refuse to start, or start with a warning? Default if unanswered: refuse (as for the margin policy).
2. **D3, the second channel.** Which push service, if any? Default: none; only the interface ships, `second_channel = "none"`.
3. **Grace for passing states (link down, resync, in doubt, venue setting, account mode, margin stale, gateway block).** Default 30 s (판단(모델 아님)); the owner may want link down sooner (the U18b kill switch acts at 10 s anyway).
4. **Start and clean-stop notices** (one message each per process start and stop). Default: send them; they also show that the path works after each start.
