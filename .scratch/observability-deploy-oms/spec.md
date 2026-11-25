# Spec: observability deployment part 2 — OMS, watcher, projector and export units, the stop permission, oms_*/projector_*/journal rules, watcher_* rules with the trading and event dashboards (unit U20b, PRs U20b1, U20b2, U20b3)

Status: ready-for-agent

Design source: `RESEARCH/CRYPTO_TRADING_INTEGRATED_TN_20261118/10_DESIGN_DRAFT.md` §5.4.5 (metric names, VictoriaLogs fields), §7.5–7.7 (watcher, kill switch, direct alerts), §8.2 (projector), §8.3 (alert paths, severities, `oms_down` inhibition), §8.4 (dashboards D1 and D4), §8.5 (memory caps), amendments R2 (S7: `oms-export` daily timer; SHOULD 5: kill marker, `Restart=no`), R4 (SHOULD 2: `journal_disk_days_left`, series absent 5 min = P2), R5 (SHOULD 1: holds and `clear-hold`; SHOULD 3: resource isolation; SHOULD 4: `watcher_api_key_days_left` < 14 = P2); TN PDF table 7-7 (eight holds and their release), table 8-6 (memory caps), table 8-6a (protection settings), table 9-4 (G1 journal file system, G3 days left); `pr_plan.json` unit U20b (scope, checks, done_criteria). Model of format: `.scratch/observability-deploy-md/spec.md` (U20a), whose decisions D0–D11 stay in force; this spec names only what it adds or changes.

Numbers marked 판단(모델 아님) are judgements without a model or a measurement behind them. The design's own numbers (0.5 GB OMS, 0.1 GB projector and watcher, 4 GB daily jobs, OOMScoreAdjust −900, IOWeight 1000, days left 7 and 2, absent 5 min, key days left 14) are design values and are cited as such; the design marks most of them as judgements too.

Nothing in this unit is installed, enabled or started on any machine. The parts add files under `deploy/observability/`, tests under `crates/xtask/tests/deploy/`, mutants and documents. No code under `crates/` other than the deploy check and mutant entries changes. Market data is Bybit only (U10/U11 deferred by the owner).

## Problem Statement

The OMS (`oms-node`), the watcher, the projector and the journal export exist as binaries with configuration files, but nothing describes how they run on this PC. The runbook's only OMS unit is an untried example with `Restart=on-failure` and an `EnvironmentFile` (`docs/operations/runbook.md:115–139`), which the design replaced with `Restart=no` (R2 SHOULD 5). The watcher's kill step K3 (`systemctl stop <oms_unit>`) fails today because no unit grants it, and the runbook says so (`runbook.md:542`, "U20b가 그 권한을 주기 전에는 K3가 늘 실패한다"). Without units the kill marker has no fixed persistent home, the projector's journal free-space series (`journal_disk_free_bytes`) feeds no days-left rule, the `oms_*`, `projector_*` and `watcher_*` series reach VictoriaMetrics and nobody alerts on them, and the trading and event dashboards of design §8.4 do not exist. Without a check, an edit can give the OMS unit an automatic restart (an OMS that a watcher killed would come back without a person), drop the OOM protection of the OMS or the watcher, let the watcher stop any unit, or make a rule name a metric that no process exports.

## Solution

Files only, plus tests of the deployment check:

1. **U20b1** — systemd units `oms-node.service` (`Restart=no`, no `[Install]`), `watcher.service`, `oms-projector.service`, `oms-export.service` + `oms-export.timer`; `trading.slice` reserves their memory; a polkit rule that lets the user `hloms` stop `oms-node.service` and do nothing else; install entries; the unit rules of the check (U20–U24, extensions of U3–U7, U14, U19); ADR-036; runbook section 13 (identities, start and stop order, the kill marker and `clear-hold` under systemd, the release commands of table 7-7), and the runbook/watcher-doc corrections.
2. **U20b2** — the metric catalogue gains every series of the projector (U14); a `trading` venue list; rule file `oms.rules.json` (the journal days-left recording rule, the design's journal and OMS liveness rules, the hold classes), the `MdObsPushDropping` filter, `vmalert-tool` tests, the `ProjectorDown` inhibit rule, and the rule part of the check for these (R6, R7, R10 extensions, new R12 hold classes, R13 trading venues); runbook §9 and §10 rows for them.
3. **U20b3** — the catalogue rows of the watcher (U19a, U19b) and `ALERTS`; rule file `watcher.rules.json` (with the key days-left rule and `WatcherDown`), its tests, the `OmsDown` inhibit rule (its targets are watcher alerts), the R6/R7/R10 rows for them; dashboards D1 (trading) and D4 (event timeline) as JSON, their install entries, the dashboard part of the check (G8 extension, new G10 event-kind partition); runbook §9, §10 and §12 rows. (Split moved after spec review 1, S6: the old U20b2 was at its line budget.)

## Facts from the existing code (origin/main 2a67ab2)

### Binaries, their flags and exit codes

- `oms-node run --config <file>` (`crates/oms-node/src/main.rs:1–9`). Exit 2 = configuration error (`main.rs:45–49`) or fatal journal/core condition; 3 = start-up failure after recovery (`crates/oms-node/src/node.rs:15–23`, `:51–57`); 0 = SIGTERM/SIGINT shutdown after drain and final snapshot (`node.rs:15–18`). A panic aborts (SIGABRT, `node.rs:20–21`). Before an exit the direct alerts wait up to `exit_flush_ms`, default 15 000, range 1 000–30 000 (`crates/oms-node/src/config.rs:210–212`, `:249`). The OMS sends no `sd_notify` (searched `crates/oms-node/src`, `crates/watcher/src`, `crates/oms-projector/src` for `sd_notify`, `NOTIFY_SOCKET`, `WATCHDOG`: none).
- The OMS's direct alerts keep no state file "because every start is made by a person" (`crates/oms-node/src/alerts.rs:605–608`).
- `watcher --config <path>` | `--version` (`crates/watcher/src/main.rs:1–4`, `:13`); exit 2 on usage, config or key file and start rules (`main.rs:21–31`; `docs/operations/watcher.md:230`), 0 after SIGTERM/SIGINT (`main.rs:44–58`). The runtime is shut down with `SHUTDOWN_MS` = 1 000 after the run (`main.rs:61–65`, `crates/watcher/src/runtime.rs:66`).
- `oms-projector --config <file>` (`crates/oms-projector/src/main.rs:1–3`); any start error exits 1 (`main.rs:8–16`). A tail error keeps the process up and pushes `projector_tail_error` = 1 (`main.rs:63–69`).
- `oms-export --config <file>`: exit 0 done, 3 no read limit (the OMS did not answer; nothing written), 1 any other error (`crates/oms-export/src/main.rs:1–3`, `:47–51`). It holds its own lock `crypto/trading/.oms-export.lock` and writes under `<lake_root>/crypto/trading/` (`docs/operations/oms-export.md:20–25`). It needs a running OMS (`oms-export.md:21`).

### Identities, sockets and files

- `admin.sock` and `monitor.sock` are bound with mode 0600 (`node.rs:365`, `:374`) and accept a peer only when its uid equals the OMS's own uid; other uids are accepted on the trading socket only (`crates/oms-node/src/server.rs:188–192`). So the watcher must run as the OMS's user (`docs/operations/watcher.md:222`). The same rule applies to an operator's `omsctl` on `admin.sock`.
- Journal files are created with the process's default mode (no explicit mode: `crates/oms-journal/src/segment.rs:157–162`, `:254–258`); the directory lock is `<data_dir>/LOCK` (`segment.rs:155`). Credentials files must be mode 0600 or the OMS refuses them (`crates/oms-node/src/venue/bybit/rest.rs:90`).
- The projector does not take the lock; it reads `/proc/locks` for a lock on the inode of `<data_dir>/LOCK` (`crates/oms-projector/src/probe.rs:114–130`) and `statvfs` of the data directory (`probe.rs:172`).
- Kill marker: `[kill] marker` of the OMS must be absolute, without `..`, outside `data_dir`, and its directory must exist at load (`config.rs:80–113`). The OMS reads it once before the boot record (`node.rs:122–146`). After a durable `clear_hold kill` the OMS removes it (`server.rs:762–766`, `:810–818`). The watcher's `[kill]` has `marker` (same rule, `crates/watcher/src/config.rs:573–583`), `oms_unit` and `systemd` = `system` | `user` (`config.rs:85–98`, `:114–122`). The watcher writes the marker and a temporary file `.<name>.watcher-<pid>.tmp` in the marker's directory, which the watcher's uid must be able to write (`docs/operations/watcher.md:79`). The documented marker path is `/var/lib/hl-oms/kill.marker` (`docs/operations/runbook.md:554`, `watcher.md:40`).
- The watcher's stop runs exactly `/usr/bin/systemctl [--user] --no-ask-password stop -- <oms_unit>` (`crates/watcher/src/stop.rs:1–7`, `:27–44`). The watcher doc's example names `oms_unit = "hl-oms.service"` (`watcher.md:41`); this unit's name is `oms-node.service`.
- The watcher kills its `systemctl` client at `[kill] stop_wait_ms` (default 10 000, `crates/watcher/src/config.rs:94–112`) and then reports the stop as unconfirmed; systemd's stop job may still stop the OMS later (`stop.rs:14–17`).
- `unmonitored` is a passing state: with a monitoring gate every start holds it until the watcher's first fresh heartbeat (`crates/oms-core/src/core.rs:262–266`, `crates/oms-node/src/alerts.rs:278–279`, `Grace::Passing`); after `unmonitored_cancel_us` (default 60 s, `crates/oms-core/src/config.rs:132`) the OMS cancels resting plain orders itself. The watcher's `heartbeat_ms` is 1 000–10 000 (`watcher/src/config.rs:435`).
- Config bounds the rules depend on: the projector's `poll_ms` (default 1 000) has a lower bound of 100 and no upper bound (`crates/oms-projector/src/config.rs:57–58`, `:103–108`); the watcher's `export_ms` (default 1 000) may be 1 000–60 000 (`watcher/src/config.rs:437`).
- W5 arms when the start checks end, "whether or not the OMS ever answered in this process" (`crates/watcher/src/w5.rs:165–172`); then `silence_ms` (default 10 s) without a good `/api/status` answer starts a kill (`docs/operations/runbook.md:528`). Start order "OMS first, wait for `/api/status`, then the watcher" and "stop the watcher first" (`runbook.md:546`).

### Metrics actually exported (U14, U19a, U19b)

- Projector (all with `venue`; pushed through `obs-push` with self labels `service="oms-projector"`, `venue`, `crates/oms-projector/src/main.rs:29–33`): `oms_up`, `oms_heartbeat_ts`, `oms_journal_durable_seq`, `projector_lag_records`, `projector_cursor_seq`, `projector_pending_lines`, `projector_unconfirmed_records`, `projector_tail_error`, `projector_caught_up`, `projector_rejected_lines` (`crates/oms-projector/src/metrics.rs:118–133`, every step); after the first catch-up only: `oms_halt`, `oms_holds{reason}`, `oms_open_orders`, `oms_in_doubt_orders`, `oms_parked_fills` (`:138–154`), `oms_position_qty{symbol}`, `oms_realized_pnl{symbol}`, `oms_upnl{symbol}` (`:171–183`), `oms_account_value`, `oms_margin_ratio{symbol}` (`:187–192`), `oms_limit{kind[,symbol]}`, `oms_limit_distance{kind[,symbol]}` (`:204–205`; kinds `open_orders`, `position_max`, `position_min`, `leverage`, `daily_loss`, `mm_buffer`, `:220–304`). `journal_disk_free_bytes{venue}` every `disk_every_ms` (default 10 000), only when `statvfs` succeeded (`crates/oms-projector/src/projector.rs:640–656`, `config.rs:63–65`). No `oms_rate_tokens` (`metrics.rs:30`).
- `oms_holds{reason}`: the snake_case names of `HoldReason` (`crates/oms-core/src/model.rs:294–296`) returned by `Core::holds(None)` (`crates/oms-core/src/core.rs:201–266`; `venue_setting` is an object whose key is the name, `metrics.rs:92–99`). `operator_strategy` and `strategy_link` are added only for a given strategy (`core.rs:220–224`, `:257–261`), so the projector never pushes them.
- Watcher (every series has `venue="bybit"`, `host`, `watcher`; `crates/watcher/src/metrics.rs:5`, `:38–41`; own client, no `obs-push`): `watcher_up` (`metrics.rs:75`), `watcher_differences{check}`, `watcher_mismatch_active{check}` (`crates/watcher/src/engine.rs:318–319`, `:596`, `:835`), `watcher_check_age_s{check}` (`engine.rs:409`, `runtime.rs:791`), `watcher_request_errors_total{endpoint,class}` (`engine.rs:445`, `crates/watcher/src/rest.rs:500`, `crates/watcher/src/start.rs:423`), `watcher_journal_error`, `watcher_journal_seq` (`engine.rs:715`, `:725`), `watcher_mismatches_total{check}` (`engine.rs:828`), `watcher_heartbeat_ok_ts` (`crates/watcher/src/oms.rs:321`), `watcher_auth_rejected`, `watcher_requests_total{endpoint}` (`rest.rs:377`, `:450`), `watcher_alerts_queued`, `watcher_alerts_delivered_total`, `watcher_alerts_dropped_total`, `watcher_alert_failures_total` (`crates/watcher/src/alert.rs:449–452`), `watcher_api_key_days_left`, `watcher_public_ip_ok` (`start.rs:39–40`), `watcher_w5_armed`, `watcher_kill_active`, `watcher_kills_total`, `watcher_kill_cancel_all_total`, `watcher_kill_halt_owed` (`runtime.rs:454–462`; the four kill series exist only with `[kill]`), `watcher_kill_step{step}` (`runtime.rs:1540–1550`), `watcher_kill_round`, `watcher_kill_remaining` (`runtime.rs:1554–1557`), `watcher_oms_silence_s` (`runtime.rs:1528`).
- `oms-node` itself pushes nothing (no `obs_push` in `crates/oms-node/src`). The OMS's liveness in VictoriaMetrics is the projector's `oms_up`.
- The existing rule `MdObsPushDropping` sums `obs_push_dropped_total` over every service (`deploy/observability/vmalert/metrics/market-data.rules.json:104–105`), so a projector drop is reported as a market-data alert and held back by `MdCollectorDown`.
- `ALERTS` (written by vmalert) is named only in a comment of `deploy/observability/metrics.toml` (line 9): no `[[stack]]` row, and no entry has the label `alertstate` or `alertname`. G7 reports a name or label that no entry has (`crates/xtask/tests/deploy/grafana.rs:586–600`, `catalogue.rs:36–46`).
- VictoriaLogs lines: one per journal event with `service="oms"` (`crates/oms-projector/src/lines.rs:234`), fields `seq`, `event_index`, `kind`, `order_id`, `wire_id`, `symbol`, `side`, `qty`, `px`, `phase`, `reason`, `fill_key`, `recv_ns`, `venue_ts_ns`, `unconfirmed` (`lines.rs:8–22`); the projector's own lines have `service="oms-projector"` and a `level` field (`projector.rs:470–490`). Stream fields `service,host,venue` (`crates/obs-push/src/lib.rs:290`). Event names: the 51 arms of `Event::name` (`crates/oms-core/src/event.rs:462–514`, `Event::… => "<name>"`; the `impl Input` that follows at `:516` has its own names, which are not event kinds). The watcher writes no line to VictoriaLogs (stderr only, `watcher.md:231`).

### The U20a check this unit extends

- `crates/xtask/tests/deploy/`: `units.rs` (`PROTECTED` `:16`, `BOUNDED` `:65`, `MEMORY_MAX` `:73`, `TIMERS` `:87`, `DAILY` `:94`, `COLLECTOR` `:158`, `PROTECTION` `:176`; U3 `:423–441`, U4 `:443–469`), `rules.rs` (`PER_VENUE` `:32`, `DESIGN` `:66`, `GROUP_INTERVALS` `:118`), `alertmanager.rs` (inhibit constants `:69–76`), `grafana.rs` (G8 dashboards `:120–131`), `tree.rs` (`ANCHORS` `:17–24`), `catalogue.rs`. `deploy/observability/metrics.toml` has 20 `[[metric]]` rows, all md/lake.
- `trading.slice` has `MemoryMin=512M` (the collector's only, `deploy/observability/systemd/trading.slice`).
- Next free numbers at 2a67ab2: ADR-036 (`docs/adr/` and `registry/decisions.toml:324` end at ADR-035); mutant M362 (`crates/xtask/src/mutants.rs:3276` is M361). `crates/xtask/tests/deploy/**` and `crates/xtask/src/mutants.rs` are not owner-reviewed paths (`tools/ci/owner_review_paths.txt`); `main.rs`, `registry.rs`, `.github/` are.

### Checked at the spec stage (2026-11-25, this PC, files in `units/U20b/tmp/proto/`)

- `systemd-analyze verify` (systemd 259) passes on prototype units equal to D1–D4 below (paths to binaries replaced by `/bin/true` links): `AssertPathIsMountPoint=`, `RequiresMountsFor=`, `StateDirectory=hl-oms` in two units, `SupplementaryGroups=`, `UMask=`, `Restart=no`, a unit without `[Install]`, `trading.slice` with `MemoryMin=818M`. Output: only the host's unrelated `astxdaemon.service` warning; exit 0. A planted `Restart=bogus` is only a warning ("Failed to parse Restart=bogus, ignoring"): systemd does not refuse a bad restart value, so the check must.
- `vmalert -dryRun` (v1.157.0) accepts a prototype `oms.rules.json` with the recording rule and the alerts of D6; `vmalert-tool unittest` on five prototype cases: SUCCESS (journal days left 64 GB losing 10 GB/day: no P2 at 6 h (warm-up), P2 at 40 h, no P1; 20 GB losing 10 GB/day: P1 at 20 h; `journal_disk_free_bytes` stopping: no alert at 4 min, P2 with `venue="bybit"` at 11 min; `oms_up` 1 then 0: no alert at 1 min, P1 at 3 min; key days left 30 then 13: P2 only after 13). Ten-minute samples did not work for the days-left rule in `vmalert-tool` (no alert): tests use 10 s samples, the projector's default.
- `amtool check-config` (0.35.1) accepts the U20a `alertmanager.json` plus the two inhibit rules of D7 (3 inhibit rules, 5 receivers); `amtool config routes test` sends `OmsDown`/P1 to `telegram-p1` and a P2 watcher alert to `telegram-p2`.
- The prototype polkit rule (D2) passes `node --check` (syntax only). `man polkit` on this PC documents `action.lookup(key)` for "the polkit variables passed from the mechanism". That systemd passes `unit` and `verb` for `StopUnit` is not documented in `org.freedesktop.systemd1(5)` here; the strings `verb`, `polkit.message`, `polkit.gettext_domain` sit together in `libsystemd-core-259.so` (weak evidence). Not tested: polkit rules cannot be loaded without root (assumption A-22, D2).
- Daily lock under `ProtectSystem=strict` (spec review 1, M1): `bwrap --ro-bind / / --dev /dev --proc /proc flock /run/lock/<new>.lock true` → `Read-only file system`, exit 73; the same with a writable directory bound on `/run/lock` → exit 0 and the lock file is created. `/run/lock` on this PC is a directory of the `/run` tmpfs (mode 1777, no own mount), so the lock file is gone after every boot. The U20a daily jobs that share the lock (`md-archiver@.service`, `crypto-lake-audit@.service`) have no `ProtectSystem=`. `systemd-analyze verify` passes on the prototype `oms-export.service` with `ReadWritePaths=/mnt/nvme/lake/crypto/trading /run/lock` (exit 0).
- The `<GATE>` class (spec review 1, S2): `vmalert-tool unittest` (v1.157.0, `units/U20b/tmp/proto/gate/`) on `OmsUnmonitored` (`for` 2m) and `OmsHoldLatched` without `unmonitored`: `unmonitored` 1 for 90 s then 0 → no alert at 90 s or 4 min; 1 throughout → none at 1 min 50 s, `OmsUnmonitored{venue="bybit",reason="unmonitored",severity="P1"}` at 3 min, no `OmsHoldLatched`: SUCCESS.
- A Unix socket on a read-only bind mount still accepts `connect()` (bwrap `--ro-bind`, this PC, kernel 7.0): `ReadOnlyPaths=` on the data directory does not keep the watcher from `admin.sock` and `monitor.sock`.

## Split into PRs (merge order)

| Part | Branch | Contents | Size (판단(모델 아님)) |
|---|---|---|---|
| U20b1 | `observability-oms-units` (continues from the spec branch) | four units, the timer, the slice change, the polkit rule; install entries; check rules U20–U24 and the extensions of U3, U5, U6, U7, U14, U19, P1; mutants (1)–(9); ADR-036; `registry/decisions.toml`, `registry/assumptions.toml` A-22, A-23; runbook `observability.md` §13 and the `runbook.md`/`watcher.md` corrections; GLOSSARY; README | Rust ≈ 780 incl. tests and mutant entries; unit files ≈ 200; docs ≈ 420 |
| U20b2 | `observability-oms-alert-rules` | projector catalogue rows, `venues.toml` `trading`, `oms.rules.json`, the `MdObsPushDropping` filter and `ProjectorObsPushDropping`, `oms.test.json`, the `ProjectorDown` inhibit rule; check: R6 (three per-venue OMS rules), R7 (journal rules), R10 (two inhibit rules), R12, R13; mutants (10)–(13); runbook §9 and §10 rows of these rules | Rust ≈ 450; rule JSON ≈ 220; catalogue ≈ 120; docs ≈ 150; test JSON ≈ 450 (data) |
| U20b3 | `observability-trading-dashboards` | watcher and `ALERTS` catalogue rows, `watcher.rules.json`, `watcher.test.json`, the `OmsDown` inhibit rule; check: R6 (`WatcherDown`), R7 (key rule), R10 (third inhibit rule); `d1-trading.json`, `d4-events.json`, install entries, check G8 extension and G10; mutants (14)–(17); runbook §9 and §10 rows of the watcher rules, §12.8, §12.9 | Rust ≈ 500; rule JSON ≈ 150; catalogue ≈ 170; docs ≈ 200; dashboard JSON ≈ 900 and test JSON ≈ 300 (data) |

Line budget: as U20a (non-data lines ≤ 1,500 per part, measured with `git diff --stat origin/main` and reported). Rule files (`*.rules.json`), `metrics.toml`, `venues.toml`, `install.toml` and unit files count as non-data; only `vmalert/tests/*.test.json`, dashboard JSON and `docs/generated` count as data (판단(모델 아님), the stricter reading). Estimated non-data totals: U20b1 ≈ 1,400, U20b2 ≈ 940, U20b3 ≈ 1,020 (판단(모델 아님)). Fallbacks if a part is over at its last commit: U20b1 moves its runbook §13.3–§13.5 text to U20b2; U20b2 moves `ProjectorObsPushDropping` with the `MdObsPushDropping` filter and their tests to U20b3; U20b3 writes §12.8–§12.9 as tables only. The part report names the move. Each part keeps `the_deploy_tree_passes` green at its own HEAD: a rule of a later part does not run before its files exist; U20b1 adds no rule file; U20b2's R6 and R10 pin only the rules and inhibit rule that U20b2 adds, and U20b3 extends both tables.

## Decisions

### D0. Layout additions

```
deploy/observability/
  systemd/  oms-node.service watcher.service oms-projector.service
            oms-export.service oms-export.timer                      (U20b1)
  polkit/   50-hl-oms-watcher-stop.rules                             (U20b1)
  vmalert/metrics/oms.rules.json (U20b2) watcher.rules.json (U20b3)
  vmalert/tests/oms.test.json (U20b2) watcher.test.json (U20b3)      (not installed)
  grafana/dashboards/d1-trading.json d4-events.json                  (U20b3)
```

- Install map (`install.toml`): units → `/etc/systemd/system/`, the polkit rule → `/etc/polkit-1/rules.d/50-hl-oms-watcher-stop.rules` (mode 0644, a new branch of `install_target`, rule U14), rule files and dashboards as in U20a.
- Operator files outside the repository (placeholders, created by the operator, never in `deploy/`): binaries `/usr/local/bin/{oms-node,watcher,oms-projector,oms-export,omsctl}`; configs `/etc/hl-oms/{oms-node,watcher,oms-projector,oms-export}.toml`; secrets `/etc/hl-oms/oms-bybit.toml`, `/etc/hl-oms/watcher-bybit.toml`, `/etc/hl-oms/telegram.json`, `/etc/hl-oms/watcher-telegram.json` (owner `hloms`, mode 0600, as the binaries require). The configs are not deploy files: their keys depend on the account (sub-account UID, symbols) and stay with the operator. The runbook (§13.2) lists the values each must hold so that it matches the units; the check pins the unit side only.
- New syntax: `.rules` (polkit JavaScript) is not parsed; rule P1 accepts the extension only under `polkit/`, and U22 pins the text.

### D1. Identities and the journal file system (U20b1)

| Unit | User / Group | Why |
|---|---|---|
| `oms-node.service` | `hloms` / `hloms`, `UMask=0027` | a system user the operator creates (판단(모델 아님)); the umask makes new journal files group-readable for the projector and closed to others (판단(모델 아님)) |
| `watcher.service` | `hloms` / `hloms`, `UMask=0077` | forced: `admin.sock` and `monitor.sock` accept only the OMS's uid (`server.rs:188–192`). Cost: the watcher can read the OMS's credentials and the OMS the watcher's (`watcher.md:222`); owner question Q3 |
| `oms-projector.service` | `hlobs` / `hlobs`, `SupplementaryGroups=hloms` | an observability component (U20a's stack user); reads the journal through the group, cannot open `admin.sock` (0600 and the uid check) and cannot read the 0600 credentials |
| `oms-export.service` | root (no `User=`) | it holds the daily lock `/run/lock/hl-oms-daily.lock`, whose users all run as root (U20a D4, ADR-035), and writes the root-owned lake |

- Journal file system: `/mnt/nvme/hl-oms-journal` (U20a D1, X10), data directory `/mnt/nvme/hl-oms-journal/data` (owner `hloms:hloms`, mode 0750, made by the operator). All four units have `RequiresMountsFor=/mnt/nvme/hl-oms-journal` and `AssertPathIsMountPoint=/mnt/nvme/hl-oms-journal`. Reason: when the loop mount is missing, the path is a plain directory on the lake's file system; an OMS started there would make a new, empty journal beside the real one (a new instance without the journal's kill latch, `runbook.md:550`), and a watcher would read an empty journal. `RequiresMountsFor=` alone does nothing when no mount unit exists for the path (it only adds dependencies), so the assert is the guard: the start fails loudly. (`Condition…` would skip silently.)
- Kill marker home: `StateDirectory=hl-oms` and `StateDirectoryMode=0700` in `oms-node.service` and `watcher.service` → `/var/lib/hl-oms` (persistent, survives reboots, owned by `hloms`, writable under `ProtectSystem=strict`). Marker `/var/lib/hl-oms/kill.marker` in both configs, the path the docs already use (`runbook.md:554`). Never a `RuntimeDirectory` (tmpfs): a kill would vanish at the next boot. The marker is on `/`, not on the journal file system, so a full journal file system does not stop the marker write (판단(모델 아님)).
- Projector cursor: `StateDirectory=oms-projector` → `cursor_file = "/var/lib/oms-projector/projector.cursor"`.
- Existing journal files from an earlier run may be 0600: the runbook (§13.1) gives `chgrp -R hloms` and `chmod -R g+rX,o-rwx` on the data directory, before the first projector start.

### D2. The stop permission (U20b1)

`deploy/observability/polkit/50-hl-oms-watcher-stop.rules`, text exactly (whitespace-normalised by U22):

```js
// The watcher (user hloms) may stop the OMS unit and nothing else (ADR-036).
polkit.addRule(function(action, subject) {
    if (action.id == "org.freedesktop.systemd1.manage-units" &&
        subject.user == "hloms" &&
        action.lookup("unit") == "oms-node.service" &&
        action.lookup("verb") == "stop") {
        return polkit.Result.YES;
    }
});
```

- It allows exactly `StopUnit` of `oms-node.service` for processes of `hloms` (the watcher, and the OMS itself, which never calls it). Start, restart, kill and every other unit stay refused (polkit's default for a non-admin user). The watcher's config holds `oms_unit = "oms-node.service"`, `systemd = "system"` (runbook §13.2).
- Assumption A-22 (new, unverified): systemd 259 passes the polkit details `unit` (the unit name) and `verb` (`stop`) for `StopUnit`. If it does not, the rule never matches: K3 fails, the watcher's notice says `OMS 정지 실패` and `watcher_kill_step{step="stop"}` = 0 (`watcher.md:274`); K2 (`watcher_kill` on `admin.sock`) still latches a live OMS. Detection: X1 install step (runbook §13.6) runs `sudo -u hloms systemctl --no-ask-password stop oms-node.service` (must succeed) and `sudo -u hloms systemctl --no-ask-password stop grafana.service` and `… start oms-node.service` (must be refused).
- Not chosen: running the watcher as root (no polkit needed, but the watcher then may stop any unit and the scope asks for "only the oms-node unit"); a user unit (`systemctl --user`, needs lingering and moves the OMS out of the system slice); a sudoers line (the watcher calls `systemctl`, not `sudo`, `stop.rs:27`).

### D3. Units (U20b1)

Common to all four: `NoNewPrivileges=yes`, `ProtectSystem=strict`, `ProtectHome=yes`, `PrivateTmp=yes`, `KillSignal=SIGTERM`, the journal lines of D1, `[Unit] After=network-online.target` and `Wants=network-online.target` (projector and export without `Wants=`). `MemoryMax` per design §8.5 / TN table 8-6.

**`oms-node.service`**

```
[Unit]
Description=hl-oms OMS node (started by a person only: ADR-036)
After=network-online.target
Wants=network-online.target
RequiresMountsFor=/mnt/nvme/hl-oms-journal
AssertPathIsMountPoint=/mnt/nvme/hl-oms-journal
[Service]
Type=simple
User=hloms
Group=hloms
UMask=0027
ExecStart=/usr/local/bin/oms-node run --config /etc/hl-oms/oms-node.toml
Restart=no
StateDirectory=hl-oms
StateDirectoryMode=0700
ReadWritePaths=/mnt/nvme/hl-oms-journal/data
NoNewPrivileges=yes ProtectSystem=strict ProtectHome=yes PrivateTmp=yes   (one key per line in the file)
LimitCORE=0
MemoryMax=512M
Slice=trading.slice
OOMScoreAdjust=-900
MemorySwapMax=0
MemoryMin=256M
IOWeight=1000
KillSignal=SIGTERM
TimeoutStopSec=60
```

- `Restart=no` (R2 SHOULD 5): after a crash, a fatal exit or a watcher kill, a person starts the OMS. No `RestartSec`, `RestartPreventExitStatus`, `RestartForceExitStatus`, `RestartSteps`, `RestartMaxDelaySec`, `OnFailure=`, `OnSuccess=`, `Upholds=` (each could bring it back). **No `[Install]` section**: the unit cannot be enabled, so a reboot does not start it either (판단(모델 아님); "every start is made by a person", `alerts.rs:605–608`; owner question Q1). No other deploy unit may name `oms-node.service` in any key but `After=`/`Before=` (rule U20), so nothing pulls it in.
- `TimeoutStopSec=60` (판단(모델 아님)): drain, final snapshot and up to 30 s of alert flush (`exit_flush_ms` max). After it systemd sends SIGKILL; the journal is durable up to the last commit, the next start recovers (`node.rs:51–57`).
- `TimeoutStopSec=60` and the watcher's `stop_wait_ms = 10000` are not equal on purpose (spec review 1, S4). The watcher's K3 waits 10 s for its `systemctl stop` client, then reports `OMS 정지 미확인` and goes on to the cancel-all loop, which starts only after K3 (`runbook.md:528`). A larger `stop_wait_ms` would delay the cancel-all of a hung OMS's orders by up to 50 s; a shorter `TimeoutStopSec` would cut the OMS's own drain and alert flush. Safer for money: keep both; the cancel-all goes first, systemd's stop job keeps running and ends the OMS by SIGKILL at the latest 60 s after K3. §13.2 lists `stop_wait_ms = 10000`, and §13.6 gives the rule for the operator: after `OMS 정지 미확인`, wait 60 s and run `systemctl is-active oms-node`; `inactive`/`failed` = stopped, then `clear-hold` as for any kill; `active`/`deactivating` after 60 s = stop it by hand (`systemctl kill -s SIGKILL oms-node`, as an admin).
- `LimitCORE=0` (판단(모델 아님)): the process holds the Bybit API secret; a core dump of an abort would write it to disk. Same for the watcher.
- `Type=simple`: no `sd_notify` exists (Facts). No `WatchdogSec`.

**`watcher.service`**

```
[Unit]
Description=hl-oms watcher (independent reconciliation, W5, kill)
After=network-online.target oms-node.service
Wants=network-online.target
RequiresMountsFor=/mnt/nvme/hl-oms-journal
AssertPathIsMountPoint=/mnt/nvme/hl-oms-journal
[Service]
Type=simple
User=hloms
Group=hloms
UMask=0077
ExecStart=/usr/local/bin/watcher --config /etc/hl-oms/watcher.toml
Restart=on-failure
RestartSec=5
RestartPreventExitStatus=2
StateDirectory=hl-oms
StateDirectoryMode=0700
ReadOnlyPaths=/mnt/nvme/hl-oms-journal/data
(sandbox lines) LimitCORE=0
MemoryMax=100M
Slice=trading.slice
OOMScoreAdjust=-900
MemorySwapMax=0
MemoryMin=50M
IOWeight=1000
KillSignal=SIGTERM
TimeoutStopSec=30
[Install]
WantedBy=multi-user.target
```

- `Restart=on-failure`, `RestartSec=5` (판단(모델 아님)), `RestartPreventExitStatus=2` (`watcher.md:227`: a refused start is not repeated with a P1 each time). A crashed watcher returns; with `RestartSec=5` the default start limit (5 in 10 s) is never reached.
- `After=oms-node.service`, and no other dependency on it either way (rule U20): stopping the OMS must not stop the watcher (the watcher's kill loop runs after it stops the OMS), and at shutdown systemd stops the watcher first (reverse order), which is the documented maintenance order (`runbook.md:546`).
- **Enabled at boot** (`WantedBy=multi-user.target`, the operator runs `systemctl enable watcher`). At boot the OMS is not running (D3 above), the watcher's start checks end, W5 arms (`w5.rs:165–172`) and after `silence_ms` it kills: marker, `systemctl stop` of the inactive OMS, cancel-all with its own key until zero open orders. The orders the dead OMS left at Bybit are cancelled, and the next OMS start is latched until `clear-hold --reason kill`. This is the safer default for money: when the journal file system is mounted, a reboot does not leave unmanaged resting orders (판단(모델 아님); owner question Q2). The runbook (§13.4) gives the start sequence after a reboot.
- **Limit of the boot kill (spec review 1, S3):** the watcher has the mount assert of D1 and `ReadOnlyPaths=` on the data directory. If the journal's loop mount is missing after a reboot, the assert fails the start job (a missing `ReadOnlyPaths=` entry would too), and `Restart=on-failure` does not retry a unit that never started. Then no marker is written and no cancel-all is sent: the orders the OMS left at Bybit stay. The only signals are `WatcherDown` (P1, the stack) and `systemctl --failed`. Kept as designed: a watcher without its journal cannot check anything, and a boot kill without the journal is a different unit design (no assert on the watcher, `ReadOnlyPaths=-…`) that the owner decides (Q2), not this spec. §13.4's first step after every reboot: `findmnt /mnt/nvme/hl-oms-journal` and `systemctl is-active watcher`; if either fails, mount the journal and start the watcher before anything else, and check Bybit's open orders by hand until the watcher's kill has ended.
- `ReadOnlyPaths=` the data directory: the watcher reads the journal and connects to the sockets there (a socket on a read-only bind still connects, Facts). It writes only `/var/lib/hl-oms` (the marker).

**`oms-projector.service`**

```
[Unit]
After=network-online.target oms-node.service victoriametrics.service victorialogs.service
RequiresMountsFor=… AssertPathIsMountPoint=…   (D1)
[Service]
Type=simple
User=hlobs
Group=hlobs
SupplementaryGroups=hloms
ExecStart=/usr/local/bin/oms-projector --config /etc/hl-oms/oms-projector.toml
Restart=on-failure
RestartSec=10
StateDirectory=oms-projector
ReadOnlyPaths=/mnt/nvme/hl-oms-journal/data
(sandbox lines)
MemoryMax=100M
Slice=trading.slice
[Install]
WantedBy=multi-user.target
```

- In `trading.slice` (table 8-6a: CPU weight) but without the 8-6a memory protections (the scope gives them to the OMS and the watcher only). No `MemoryMin`, no `OOMScoreAdjust`.
- `RestartSec=10` (판단(모델 아님)): a config error exits 1 (`main.rs:8–16`) and repeats every 10 s; `ProjectorDown` (D6) reports it.
- No `ProcSubset=` (it would hide `/proc/locks`, which the projector reads, `probe.rs:114–130`); rule U23.

**`oms-export.service` and `oms-export.timer`**

```
[Unit]
After=oms-node.service
RequiresMountsFor=/mnt/nvme/hl-oms-journal /mnt/nvme/lake
AssertPathIsMountPoint=/mnt/nvme/hl-oms-journal
[Service]
Type=oneshot
ExecStart=/usr/bin/flock /run/lock/hl-oms-daily.lock /usr/local/bin/oms-export --config /etc/hl-oms/oms-export.toml
ReadOnlyPaths=/mnt/nvme/hl-oms-journal/data
ReadWritePaths=/mnt/nvme/lake/crypto/trading /run/lock
(sandbox lines)
MemoryMax=4G
TimeoutStartSec=6h
Nice=10
```
Timer: `OnCalendar=*-*-* 03:30:00 UTC`, `Persistent=true`, `[Install] WantedBy=timers.target`.

- Plain names, not a template: one OMS per PC, and the export reads one data directory (scope names `oms-export.service`). Under the U20a daily lock (rules U7, U19), so its 4 GB never adds to the archiver's or the audit's.
- 03:30 UTC (판단(모델 아님)): after the audit's 02:30 start; yesterday's file needs a durable record of today (`oms-export.md:24`). The lock makes the order safe in any case.
- Exit 3 (the OMS did not answer) leaves the unit failed. Not `SuccessExitStatus=3`: a day without export must stay visible in `systemctl --failed` (the OMS is then also down, `OmsDown` P1). The next run writes every completed date (`oms-export.md:23`). Owner question Q8.
- Not in `trading.slice`.
- `/mnt/nvme/lake/crypto/trading` must exist before the first run (a missing `ReadWritePaths` entry fails the unit at set-up; runbook §13.1).
- `/run/lock` in `ReadWritePaths` (spec review 1, M1): `ProtectSystem=strict` makes `/run` read-only, and `flock` creates the lock file when it does not exist. `/run/lock` is a tmpfs directory, so after every boot the file is missing; without the entry the first daily job after a boot that is the export fails with exit 73 before `oms-export` starts (reproduced, Facts). The U20a daily jobs have no `ProtectSystem=` and need nothing. `/run/lock` is made at every boot by the tmpfiles entry `d /run/lock 1777 root root` (`/usr/lib/tmpfiles.d/debian.conf:13` on this PC), so the entry does not fail the unit at set-up. Kept `ProtectSystem=strict` on this root job rather than dropping it as the other two daily jobs do: the export only reads the journal and writes one lake subtree.

**`trading.slice`**: `MemoryMin=818M` = 512M (collector) + 256M (OMS) + 50M (watcher). U4 keeps it ≥ the sum.

**Boot and shutdown.** At boot: projector (enabled) and watcher (enabled) start; the OMS does not. At shutdown systemd stops the watcher before the OMS (After= reversed) and sends SIGTERM to each; the OMS leaves its orders at the venue (`runbook.md:160–163`) and the runbook's planned shutdown first halts and cancels (§13.5).

### D4. Unit rules of the check (U20b1)

Constants: `PROTECTED` = `md-collector@.service`, `oms-node.service`, `watcher.service` (U3 protected group: `Slice=trading.slice`, `OOMScoreAdjust=-900`, `MemorySwapMax=0`, `MemoryMin` = half `MemoryMax`, `IOWeight=1000`, no `MemoryHigh`). `MEMORY_MAX` += `oms-node.service` 512M, `watcher.service` 100M, `oms-projector.service` 100M, `oms-export.service` 4G (U6). `TIMERS` += `oms-export.timer` `*-*-* 03:30:00 UTC` (U5). `DAILY` += `oms-export.service` (U7). `BOUNDED` += `oms-export.service` (U19). P1: `.rules` under `polkit/` only.

| Rule | Check |
|---|---|
| U20 | `oms-node.service`: exactly one `Restart=` line in `[Service]` and its value is `no` (a missing line, a second line or any other value is a finding); none of `RestartSec`, `RestartPreventExitStatus`, `RestartForceExitStatus`, `RestartSteps`, `RestartMaxDelaySec`, `OnFailure`, `OnSuccess`, `Upholds`, `WatchdogSec`; no `[Install]` section; `Type=simple`. Every other unit file under `deploy/` (`.service`, `.timer`, `.slice`, `.path`, `.socket`) that names `oms-node.service` in a value does so only in `After=` or `Before=` (the install map, the runbook and the polkit rule are not unit files). The same for `watcher.service` |
| U21 | `watcher.service`: `Restart=on-failure`, `RestartSec=5`, `RestartPreventExitStatus=2` (each once), `After=` holds `oms-node.service`, `[Install] WantedBy=multi-user.target`; no `Requires`, `Requisite`, `BindsTo`, `PartOf`, `Upholds`, `Conflicts`, `PropagatesStopTo`, `StopPropagatedFrom`, `JoinsNamespaceOf` |
| U22 | `oms-node.service` and `watcher.service` both `User=hloms`, `Group=hloms` (the same uid, `server.rs:188–192`); `oms-node.service` `UMask=0027`; `oms-projector.service` `User=hlobs`, `SupplementaryGroups=hloms`; `oms-export.service` has no `User=`; exactly one file under `deploy/observability/polkit/`, its text (whitespace-normalised) equals D2, and its install `dst` is under `/etc/polkit-1/rules.d/` |
| U23 | journal: the four units have `AssertPathIsMountPoint=/mnt/nvme/hl-oms-journal` and `RequiresMountsFor=` holding it; `oms-node.service` `ReadWritePaths=/mnt/nvme/hl-oms-journal/data` exactly; the other three `ReadOnlyPaths=/mnt/nvme/hl-oms-journal/data` exactly and no `ReadWritePaths` covering it; `oms-export.service` `ReadWritePaths` exactly the two entries `/mnt/nvme/lake/crypto/trading` and `/run/lock` (in any order, one line); every deploy unit whose `ExecStart=` runs `/usr/bin/flock /run/lock/…` and that has `ProtectSystem=strict` has `/run/lock` in `ReadWritePaths` (M1); `oms-node.service` and `watcher.service` `StateDirectory=hl-oms` and `StateDirectoryMode=0700`, and no unit has `RuntimeDirectory=hl-oms`; `oms-projector.service` `StateDirectory=oms-projector` and no `ProcSubset=` |
| U24 | the four units: `NoNewPrivileges=yes`, `ProtectSystem=strict`, `ProtectHome=yes`, `PrivateTmp=yes`; `oms-node.service` and `watcher.service` `LimitCORE=0`; `oms-projector.service` `Slice=trading.slice`; `oms-export.service` no `Slice=trading.slice`; `TimeoutStopSec` 60 (OMS) and 30 (watcher) |

All new rules read keys through the existing `Ini` reader (repeated keys kept, `[Unit]`/`[Service]` sections as written). A finding names the unit and the rule, never a secret.

### D5. Catalogue and venues (U20b2, U20b3)

- `metrics.toml` gains one `[[metric]]` row per name in Facts "Metrics actually exported". U20b2: projector rows with `source` = `crates/oms-projector/src/metrics.rs` (and `crates/oms-projector/src/projector.rs` for `journal_disk_free_bytes`), labels `venue` plus `symbol`, `reason`, `kind` where the code adds them. U20b3: watcher rows with `source` = the file that holds the literal (Facts), labels `venue`, `host`, `watcher` plus `check`, `endpoint`, `class`, `step` where the code adds them. Rule R2 then anchors each name to its literal. The anchor files join `tree.rs` `ANCHORS` in the part that first reads them: U20b2 `crates/oms-projector/src/metrics.rs`, `projector.rs`, `crates/oms-core/src/model.rs` (R12), `crates/watcher/src/metrics.rs` (R13); U20b3 the other seven watcher files and `crates/oms-core/src/event.rs` (G10).
- U20b3 also adds one `[[stack]]` row (spec review 1, S1): `name = "ALERTS"`, `labels = ["alertname", "alertstate", "severity", "venue"]`, `component = "vmalert"`, `version = "v1.157.0"` (as the other vmalert rows). Without it D1's and D4's `ALERTS{alertstate="firing"}` targets fail G7 (Facts). The `metrics.toml` comment on line 9 stays.
- `venues.toml` gains `trading = ["bybit"]`: the venues an OMS on this PC trades. Rule R13: each trading venue V appears as the literal `("venue", "V")` in `crates/watcher/src/metrics.rs` (the watcher labels Bybit only, `metrics.rs:40`); no other value is accepted.

### D6. Rule files (`oms.rules.json` U20b2, `watcher.rules.json` U20b3)

Thresholds and windows are 판단(모델 아님) unless the Source column cites the design; each alert has a Korean `annotations.summary` that says what to do and names the runbook section. Per-venue rules (R6, V ∈ `trading`) carry `labels.venue = V`.

`vmalert/metrics/oms.rules.json`:

| Group (interval) | Alert / record | Expr | for | Sev | Source |
|---|---|---|---|---|---|
| `journal-recording` (1m) | record `journal_disk_days_left` | `journal_disk_free_bytes / clamp_min(-deriv(journal_disk_free_bytes[24h]) * 86400, 1)` | — | — | scope, R4 SHOULD 2 (R7) |
| `oms` (10s) | `JournalDiskDaysLeftLow` | `journal_disk_days_left < 7 and on(venue) (time() - tfirst_over_time(journal_disk_free_bytes[24h])) > 43200` | 10m | P2 | scope; warm-up as the lake rule (U20a D7) (R7) |
| | `JournalDiskDaysLeftCritical` | same with `< 2` | 10m | P1 | scope (R7) |
| | `JournalDiskGaugeMissing` (per V) | `absent_over_time(journal_disk_free_bytes{venue="V"}[5m])` | — | P2 | scope "absent 5 min" (R6) |
| | `OmsDown` (per V) | `max by (venue) (max_over_time(oms_up{venue="V"}[30s])) == 0` | — | P1 | design §8.3 B3 `oms_down` (R6); 30 s > W5's 10 s so the watcher acts first |
| | `ProjectorDown` (per V) | `absent_over_time(projector_caught_up{venue="V"}[2m])` | — | P2 | (R6) the trading view is blind; the OMS's own alerts still run (design §8.1) |
| | `OmsHoldLatched` | `max by (venue, reason) (oms_holds{reason=~"<LATCHED>"}) > 0` | — | P1 | design §8.3 P1 "주문 정지, 불일치, 증거금 위반, 킬 스위치" (R12) |
| | `OmsUnmonitored` | `max by (venue, reason) (oms_holds{reason=~"<GATE>"}) > 0` | 2m | P1 | design §8.3 P1 "모니터링 상실" (R12); `for` 2m (판단(모델 아님)): every start holds `unmonitored` until the watcher's first fresh heartbeat (Facts), which with the documented start order comes seconds after the watcher starts; 2 min leaves room for the watcher's start checks and still pages within about one minute of the OMS's own cancel of resting orders (`unmonitored_cancel_us` 60 s) |
| | `OmsHoldLasting` | `max by (venue, reason) (oms_holds{reason=~"<PASSING>"}) > 0` | 5m | P1 | a passing hold that lasts 5 min stops trading (R12) |
| | `OmsOperatorHalt` | `max by (venue, reason) (oms_holds{reason=~"<OPERATOR>"}) > 0` | — | P3 | a person did it; dashboard only (R12) |
| | `OmsLimitPassed` | `max by (venue, kind, symbol) (oms_limit_distance) < 0` | — | P1 | D1 question "한도까지 얼마나" |
| | `OmsLimitNear` | `oms_limit_distance < 0.1 * abs(oms_limit) and oms_limit_distance >= 0` | 1m | P2 | 10% |
| | `OmsParkedFills` | `max by (venue) (oms_parked_fills) > 0` | 5m | P2 | |
| `projector` (10s) | `ProjectorTailError` | `max by (venue) (projector_tail_error) > 0` | — | P2 | design §8.2 P2 "CRC 오류 → P2" |
| | `ProjectorLagHigh` | `max by (venue) (projector_lag_records) > 10000` | 5m | P2 | 10 steps of `max_records` 1 000 |
| | `ProjectorNotCaughtUp` | `max by (venue) (projector_caught_up) == 0` | 10m | P2 | the `oms_*` state series are absent until then (`metrics.rs:24–28`) |
| | `ProjectorLogsRejected` | `max by (venue) (increase(projector_rejected_lines[10m])) > 0` | — | P2 | design §8.3 P2 "VM/VL 쓰기 실패" |
| | `ProjectorObsPushDropping` | `sum by (venue, endpoint) (increase(obs_push_dropped_total{service="oms-projector"}[10m])) > 0` | — | P2 | same |

`<LATCHED>` = `kill_latched|watcher_mismatch|venue_change|reducing|mismatch|storage_low`; `<GATE>` = `unmonitored`; `<PASSING>` = `link_down|resync|in_doubt|no_config|venue_setting|account_mode`; `<OPERATOR>` = `operator_account`. (`unmonitored` was in `<LATCHED>` before spec review 1, S2: it paged P1 at every normal start.) Rule R12: the four lists are disjoint and their union is exactly the snake_case names of the `HoldReason` variants read from `crates/oms-core/src/model.rs` (the identifiers that begin a line inside `pub enum HoldReason { … }` and are followed by `,` or `(`, CamelCase → snake_case as `rename_all = "snake_case"`, `model.rs:295`), minus `operator_strategy` and `strategy_link` (never in `holds(None)`, Facts), and `<GATE>` is exactly `unmonitored`. A new hold variant without a class is then a finding.

`vmalert/metrics/watcher.rules.json` (U20b3), group `watcher` (10s):

| Alert | Expr | for | Sev | Source |
|---|---|---|---|---|
| `WatcherDown` (per V) | `absent_over_time(watcher_up{venue="V"}[1m])` | — | P1 | design §8.3 P1 "모니터링 상실" (R6) |
| `WatcherMismatchActive` | `max by (venue, check) (watcher_mismatch_active) > 0` | — | P1 | P1 "불일치" |
| `WatcherKillActive` | `max by (venue) (watcher_kill_active) > 0` | — | P1 | P1 "킬 스위치 실행" |
| `WatcherKillHaltOwed` | `max by (venue) (watcher_kill_halt_owed) > 0` | — | P1 | the OMS may trade without a latch (`watcher.md:278`) |
| `WatcherAuthRejected` | `max by (venue) (watcher_auth_rejected) > 0` | — | P1 | every Bybit request stopped: no reconciliation, no cancel-all (`watcher.md:213`) |
| `WatcherApiKeyExpiring` | `min by (venue) (watcher_api_key_days_left) < 14` | — | P2 | scope, R5 SHOULD 4 (R7) |
| `WatcherPublicIpNotOk` | `min by (venue) (watcher_public_ip_ok) == 0` | 2m | P2 | |
| `WatcherCheckStale` | `max by (venue, check) (watcher_check_age_s) > 30` | 1m | P2 | W1/W2 over 10 s already close the OMS's gate (`unmonitored`, `OmsUnmonitored` P1 after 2 min) |
| `WatcherHeartbeatStale` | `time() - max by (venue) (watcher_heartbeat_ok_ts) > 30` | 1m | P2 | |
| `WatcherW5Unarmed` | `max by (venue) (watcher_w5_armed) == 0 and on(venue) max by (venue) (watcher_kill_active) == 0` | 5m | P2 | the kill series exist only with `[kill]` (`runtime.rs:455–462`): no alert for a testnet watcher without `[kill]` |
| `WatcherJournalError` | `max by (venue) (watcher_journal_error) > 0` | — | P2 | |
| `WatcherAlertsFailing` | `max by (venue) (increase(watcher_alert_failures_total[10m])) > 0` | — | P2 | the watcher's own Telegram path |
| `WatcherAlertsDropped` | `max by (venue) (increase(watcher_alerts_dropped_total[10m])) > 0` | — | P2 | |

- `market-data.rules.json`: `MdObsPushDropping` becomes `sum by (venue, endpoint) (increase(obs_push_dropped_total{service="md-collector"}[10m])) > 0` (it now sees the collector only; the projector's drops are `ProjectorObsPushDropping`). A spec-vs-U20a change, reported.
- R7 pins: the recording rule, `JournalDiskDaysLeftLow`, `…Critical` (U20b2), `WatcherApiKeyExpiring` (U20b3) (exact expression, `for`, severity, file, group). R6 pins the per-venue rules for every `trading` venue, none for another: `JournalDiskGaugeMissing`, `OmsDown`, `ProjectorDown` (U20b2), `WatcherDown` (U20b3). `GROUP_INTERVALS` += `oms.rules.json` `journal-recording` `1m`, `oms` none or `10s`, `projector` none or `10s` (U20b2); `watcher.rules.json` `watcher` none or `10s` (U20b3).
- Config values these rules need (spec review 1, S7; 판단(모델 아님)): projector `poll_ms` ≤ 10 000, so the 30 s window of `OmsDown` holds at least three samples (`poll_ms` has no upper bound in the code, and at 31 s or more `max_over_time(oms_up[30s]) == 0` is often empty: `OmsDown` would not fire); watcher `[intervals] export_ms` ≤ 20 000, so `WatcherDown`'s 1 min window holds at least three exports (the code accepts up to 60 000, and at 60 000 `WatcherDown` P1 flaps). Runbook §13.2 lists both with the defaults (1 000 each). The check cannot see them (Further Notes).
- The direct alerts (`[OMS P1]`, `[WATCH P1]`) already page most P1 states above. The `[MON P1]` copies are a second, independent path (a dead direct path or a full outbox is then still seen); duplicates are reviewed weekly (design §8.3). Owner question Q7.
- `vmalert/tests/oms.test.json` (U20b2) and `watcher.test.json` (U20b3) (data, not installed, not in CI): per alert one firing and one non-firing case with 10 s samples; the five spec-stage cases (the key case in U20b3); R12 classes: `oms_holds{reason="link_down"}` 1 for 4 min → no `OmsHoldLasting`, for 6 min → fires; `kill_latched` 1 → `OmsHoldLatched` at once; `unmonitored` 1 for 90 s then 0 → no alert (a normal start), 1 for 3 min → `OmsUnmonitored` only; `operator_account` 1 → only `OmsOperatorHalt`; `WatcherW5Unarmed` with and without `watcher_kill_active` series.

### D7. Alertmanager (U20b2, U20b3)

Two inhibit rules after the U20a one, exactly, in this order:

1. (U20b2) source `alertname="ProjectorDown"` → targets `severity=~"P2|P3"`, `alertname=~"JournalDiskGaugeMissing"`, `equal: ["venue"]` (the projector is that series' only writer).
2. (U20b3) source `alertname="OmsDown"` → targets `severity=~"P2|P3"`, `alertname=~"WatcherCheckStale|WatcherHeartbeatStale|WatcherW5Unarmed"`, `equal: ["venue"]`. These follow from a dead OMS (W1 rounds need `/api/status`; no OMS to beat to; W5 rearms only on an answer). Design §8.3 B3 "oms_down이면 파생 알림 억제". It is in U20b3 because its targets are alerts of `watcher.rules.json` (R10: no stale name).

- While the projector is down the stack has no OMS liveness: `oms_up` is absent, so `OmsDown` cannot fire, and every `oms_*` P1 goes quiet behind one P2 `ProjectorDown` (spec review 1, N6). This follows design §8.1 (the OMS's and the watcher's direct alerts do not depend on the stack); runbook §9 says so in the `ProjectorDown` row (U20b2).

- **Market-data alerts are not inhibited while the OMS is down** (spec-vs-scope deviation, reported): the scope asks for "inhibition of market-data alerts while the OMS is down", but the collector does not depend on the OMS (it writes the lake by itself, `md-collector` units of U20a), and with `Restart=no` an OMS outage lasts until a person acts, possibly hours. Holding back `Md*` alerts then would hide raw-data loss. Safer for data: no such inhibition. Owner question Q4; if the owner wants it, the change is one more inhibit rule that the check's table pins.
- Routes do not change: P1 → `telegram-p1`, P2 → `telegram-p2`, P3 → `null`.
- R10 extension: exactly two inhibit rules at U20b2 (U20a's and 1.) and three at U20b3, with these matchers; every name in a target regex is an alert of the rule files (no stale name); the U20a regex `Md.*|LakeSealLag|LakeDiskDaysLeft.*` matches no alert whose expression reads an `oms_`, `projector_`, `watcher_` or `journal_` metric.

### D8. Dashboards (U20b3)

- `d1-trading.json`: uid `d1-trading`, title `D1 거래: 지금 한도까지 얼마나 남았나`, variable `venue` = `label_values(oms_up, venue)`. Panels (datasource `vm`): OMS 생존 (`max by (venue) (oms_up{venue=~"$venue"})`), 정지 사유 (`max by (reason) (oms_holds{venue=~"$venue"}) > 0`), 포지션 (`oms_position_qty{venue=~"$venue"}`), **한도와 거리** (table, two targets exactly `oms_limit{venue=~"$venue"}` and `oms_limit_distance{venue=~"$venue"}`, by `kind`, `symbol`), 손익 (`oms_realized_pnl`, `oms_upnl`, `oms_account_value`), 증거금 (`oms_margin_ratio{venue=~"$venue"}` and `oms_limit_distance{venue=~"$venue", kind="mm_buffer"}`), 미체결과 결과 불명 (`oms_open_orders`, `oms_in_doubt_orders`, `oms_parked_fills`), 워처 (`watcher_check_age_s`, `watcher_mismatch_active`, `watcher_kill_active`, `watcher_api_key_days_left`), 프로젝터 (`projector_lag_records`, `time() - oms_heartbeat_ts`), **열린 알림** (`ALERTS{alertstate="firing"}`, table). The open alerts come from vmalert's `ALERTS` in VictoriaMetrics, not from Alertmanager (G4 allows two datasources only); they include silenced and inhibited alerts, and the panel description says so.
- `d4-events.json`: uid `d4-events`, title `D4 사건 타임라인: 무슨 일이 어떤 순서로 있었나`, variable `venue` (single value) = `label_values(oms_up, venue)`. Panels: 주문·체결 (datasource `vl`): `_stream:{service="oms"} venue:="$venue" kind:in(<ORDER_KINDS>) | fields _time, seq, event_index, kind, order_id, wire_id, symbol, side, qty, px, phase, reason, unconfirmed | sort by (_time desc) | limit 1000`; 정지·킬·불일치 (`vl`): the same with `kind:in(<HOLD_KINDS>)`; 프로젝터 로그 (`vl`): `_stream:{service="oms-projector"} venue:="$venue" level:in(warn, error)`; 알림 (`vm`, state timeline): `ALERTS{alertstate="firing"}`. `<ORDER_KINDS>` (19) = `order_accepted, request_rejected, order_acked, order_in_doubt, order_recheck, absence_noted, fill_booked, fill_parked, fill_unparked, external_fill_booked, missing_fills_flagged, fills_synced, cancel_requested, cancel_sent, cancel_outcome_noted, venue_terminal, order_closed, funding_booked, position_adjusted`; `<HOLD_KINDS>` (23) = `config_activated, booted, link_changed, mismatch_raised, mismatch_acked, operator_halted, operator_resumed, storage_low_changed, reducing_entered, reducing_cleared, venue_setting_noted, account_mode_noted, venue_position_changed, unclassified_execution_noted, venue_change_settled, margin_report_rejected, hold_set, hold_cleared, kill_loop_ended, kill_rearmed, strategy_link_lost, strategy_link_restored, monitor_changed`; `<LEFT_OUT>` (9) = `watcher_heartbeat, ref_price_updated, margin_reported, open_orders_swept, positions_verified, status_noted, venue_oid_learned, purged, day_base_set` (frequent or periodic, or bookkeeping without a state a person acts on; 판단(모델 아님)). 19 + 23 + 9 = the 51 names of `Event::name` (Facts). The fill, mismatch and hold-cause kinds that the first draft left out (`fill_parked`, `fill_unparked`, `missing_fills_flagged`, `fills_synced`, `order_recheck`, `absence_noted`, `venue_setting_noted`, `account_mode_noted`, `config_activated`, `margin_report_rejected`) are now in a panel (spec review 1, S5). `<LEFT_OUT>` is written in the test module `crates/xtask/tests/deploy/grafana.rs` as a constant with the reason, not in the dashboard. Lines can repeat after a projector restart (at-least-once, design §5.4.5); the panel shows `seq` and `event_index`, and the description says to read a repeat as one event. The exact LogsQL is confirmed against a throwaway VictoriaLogs in D9 before it is pinned.
- Rules: G8 += `d1-trading` and `d4-events` with their titles; D1's 한도와 거리 targets exactly as above; D4's two event panels' queries exactly. G10 (new, a partition as R12): the names read from the arms of `Event::name` in `crates/oms-core/src/event.rs` (the `"<name>"` string literals after `=>` between `pub fn name(&self)` of `impl Event` and the end of that function; not `impl Input`) are each in exactly one of D4's `<ORDER_KINDS>`, D4's `<HOLD_KINDS>` and the test's `<LEFT_OUT>`, and those three hold no other name. A new event kind without a place, a name in two lists, or a misspelt kind is a finding. G6/G7 as U20a (VictoriaLogs targets are not scanned as MetricsQL; variables on `vm`).

### D9. Validation each part runs and pastes into its report (not in CI)

As U20a D11 (binaries from `units/U20a/tmp/bin/` read-only, `ss -ltn` before any bind, a busy port = skip, every process killed, hashes only in the report):
- U20b1: `systemd-analyze verify` on copies of the four units, the timer and the slice with binaries replaced; the daily lock on a read-only root, as at the spec stage (`bwrap --ro-bind / / --dev /dev --proc /proc flock /run/lock/<new>.lock true` → exit 73, and with a tmp directory bound writable on `/run/lock` → exit 0), pasted with the unit's `ReadWritePaths` line; `node --check` on a `.js` copy of the polkit rule (if node exists; else say so); `systemctl list-unit-files 'oms-*' 'watcher*' 'trading.slice'` (system and `--user`) before and after: nothing new.
- U20b2: `vmalert -dryRun` on `oms.rules.json` and the changed market-data file; `vmalert-tool unittest` on `oms.test.json` and the existing three; `amtool check-config` and `amtool config routes test` for one alert of each new severity; a throwaway Alertmanager on 127.0.0.1 with a tmp copy whose receivers have no integration, `amtool alert add` of `ProjectorDown{venue="bybit",severity="P2"}` and `JournalDiskGaugeMissing{venue="bybit",severity="P2"}`, then `amtool alert query --inhibited` shows the second inhibited; killed.
- U20b3: `vmalert -dryRun` on `watcher.rules.json`; `vmalert-tool unittest` on `watcher.test.json` and all earlier ones; the throwaway Alertmanager as in U20b2 with `OmsDown{venue="bybit",severity="P1"}` and `WatcherCheckStale{venue="bybit",severity="P2"}` → the second inhibited; a throwaway VictoriaLogs on 127.0.0.1 with data in tmp, synthetic lines pushed over loopback in the projector's shape (two copies of one `seq`), the D4 queries run through `/select/logsql/query`; a throwaway Grafana that loads D1 and D4 (`/api/dashboards/uid/…`); killed.

## Testing Decisions

All tests are in `crates/xtask/tests/deploy/`, pure functions over the in-memory tree: no network, no clock, no temp files; every loop is over the tree's files. None is timing-sensitive.

### Done criteria → tests

| Done criterion | Test | Part |
|---|---|---|
| fails on a planted unknown `oms_*` metric name | `rules::a_rule_with_an_unknown_oms_metric_fails` (plant `oms_position_value` in `oms.rules.json` → R1; and in D1 → G7 in `grafana::a_dashboard_with_an_unknown_metric_fails`; `ALERTS` row removed → G7) | U20b2, U20b3 |
| fails on an oms-node unit with any `Restart=` value other than `no` | `units::an_oms_node_unit_that_restarts_fails` (each of `on-failure`, `always`, `on-abnormal`, `on-abort`, `on-watchdog`, `on-success`, `yes`; a missing line; a second `Restart=no` line; `RestartSec=5` added; `OnFailure=x.service` added; an `[Install]` section → U20) | U20b1 |
| fails on an oms-node or watcher unit without `OOMScoreAdjust` or without `MemorySwapMax=0` | `units::an_oms_or_watcher_unit_without_oom_score_adjust_fails`, `units::an_oms_or_watcher_unit_without_memory_swap_max_zero_fails` (missing, and `=1G`, each unit) | U20b1 |
| all files parse | `tree::every_file_parses_by_its_kind` (a `.rules` file outside `polkit/` → P1) and `the_deploy_tree_passes` | U20b1 |
| no secret is committed | R11 runs over every deploy file (new units and the polkit rule included); `tools/ci/secret_scan.sh` in the gate | each |
| full local gate passes | the plan's `checks` line at each part's HEAD | each |
| nothing is installed or enabled | report: no `systemctl`/`install`/`sudo`/`pkexec` was run; the D9 `list-unit-files` before/after | each |

Further tests: U20b1 — `units::no_unit_can_start_the_oms_node` (a `Wants=oms-node.service`, a `Requires=`, a timer `Unit=oms-node.service` planted in another unit → U20), `units::the_watcher_restarts_but_never_follows_the_oms` (`BindsTo=`, `PartOf=` → U21; `Restart=always` → U21), `units::the_oms_and_watcher_share_one_user` (watcher `User=hlwatch` → U22), `units::the_stop_permission_names_only_the_oms_unit_and_stop` (verb `start`, unit `grafana.service`, a second rule file, the `subject.user` line dropped → U22), `units::the_oms_units_refuse_an_unmounted_journal` (assert dropped, `RequiresMountsFor` dropped, data dir `ReadWritePaths` on the watcher → U23), `units::the_kill_marker_directory_is_persistent` (`StateDirectory` dropped, `RuntimeDirectory=hl-oms` added → U23), `units::the_projector_can_read_proc_locks` (`ProcSubset=pid` → U23), `units::the_oms_units_are_sandboxed` (`LimitCORE` dropped, `ProtectSystem=full` → U24), `units::the_export_can_take_the_daily_lock` (`/run/lock` dropped from `oms-export.service`'s `ReadWritePaths` → U23; a planted unit with `ProtectSystem=strict` and `ExecStart=/usr/bin/flock /run/lock/x.lock …` without the entry → U23), and the new rows of `memory_max_follows_the_design_table`, `the_seal_archiver_and_audit_timers_keep_their_schedule` (export timer), `a_daily_job_without_the_lock_fails` (export), `the_trading_slice_reserves_its_units_memory` (818M − 1 → U4), `the_install_map_covers_every_file_and_path` (polkit `dst` under `/etc/hl-oms-obs/` → U14). U20b2 — `catalogue::every_projector_and_watcher_series_is_catalogued` (projector rows; a row removed → R1 on the rule that uses it), `rules::every_hold_reason_has_an_alert_class` (a variant added to the in-memory `model.rs` → R12; a reason in two lists → R12; `unmonitored` moved back to `<LATCHED>` → R12 by the pinned `<GATE>` list), `rules::every_trading_venue_has_its_liveness_rules` (a second trading venue with its watcher literal in memory but no `OmsDown` → R6; `trading = ["okx"]` → R13), `rules::the_design_journal_and_key_rules_are_present` (`< 7` → `< 8` → R7), `alertmanager::the_oms_inhibit_rules_hold_back_only_derived_alerts` (target regex `.*`, a third rule at U20b2, a stale name, `Md.*` widened to `.*` → R10). U20b3 — the same four tests extended (watcher rows; `WatcherDown` missing for a trading venue → R6; `< 14` → `< 7` → R7; the `OmsDown` rule's target regex `.*`, a fourth rule → R10), `grafana::the_trading_and_event_dashboards_exist`, `grafana::the_trading_dashboard_shows_limits_and_distances` (a limit target dropped → G8), `grafana::the_event_dashboard_names_only_journal_event_kinds` (`kind:in(order_acepted)` → G10; a 52nd `Event::name` arm added to the in-memory `event.rs` → G10; `fill_parked` in both lists → G10; `boot` from `impl Input` put in `<LEFT_OUT>` → G10).

### Deliberate defects (xtask mutants; ids from M362 at 2a67ab2, next free at implementation time)

- U20b1: (1) rule U20 accepts `Restart=on-failure` → `units::an_oms_node_unit_that_restarts_fails`; (2) delete the `Restart=no` line from `oms-node.service` → `the_deploy_tree_passes`; (3) `PROTECTED` drops `watcher.service` → `units::an_oms_or_watcher_unit_without_oom_score_adjust_fails`; (4) U20's other-unit scan skips `Wants=` → `units::no_unit_can_start_the_oms_node`; (5) U22 skips the `verb` line of the polkit text → `units::the_stop_permission_names_only_the_oms_unit_and_stop`; (6) U23 skips `AssertPathIsMountPoint` → `units::the_oms_units_refuse_an_unmounted_journal`; (7) `watcher.service` `RestartPreventExitStatus=2` → `3` → `the_deploy_tree_passes`; (8) U22 accepts different users for the OMS and the watcher → `units::the_oms_and_watcher_share_one_user`; (9) U23 skips the `/run/lock` entry of daily jobs under `ProtectSystem=strict` → `units::the_export_can_take_the_daily_lock`.
- U20b2: (10) R12 skips the `<LATCHED>` list → `rules::every_hold_reason_has_an_alert_class`; (11) the catalogue accepts any name with the `oms_` prefix → `rules::a_rule_with_an_unknown_oms_metric_fails`; (12) R6 checks only `OmsDown` for trading venues → `rules::every_trading_venue_has_its_liveness_rules`; (13) R10 accepts any target regex on the `ProjectorDown` rule → `alertmanager::the_oms_inhibit_rules_hold_back_only_derived_alerts`.
- U20b3: (14) `WatcherApiKeyExpiring` threshold `< 14` → `< 7` in the rule file → `the_deploy_tree_passes`; (15) R10 accepts any target regex on the `OmsDown` rule → `alertmanager::the_oms_inhibit_rules_hold_back_only_derived_alerts`; (16) G8 skips D1's limit panel → `grafana::the_trading_dashboard_shows_limits_and_distances`; (17) G10 checks only that a listed name exists (no partition) → `grafana::the_event_dashboard_names_only_journal_event_kinds`.

Mutant `test` arguments: `["-p", "xtask", "--test", "deploy", "<module>::<test>"]`; `guards`: ADR-036. The mutant run already copies `deploy` and `contracts` (U20a); every new anchor file is under `crates/`.

## Documents and registry per part

- **ADR-036** (next free at 2a67ab2; if taken, the next free number, and the part report says so) `docs/adr/ADR-036-oms-deployment-files.md`, in U20b1: identities (one user for OMS and watcher, forced by the socket uid check; projector in the OMS group; export as root), `Restart=no` and no `[Install]` for the OMS, the watcher enabled at boot and why (a reboot with the journal mounted cancels resting orders) and the limit of that (no boot kill when the journal mount is missing, D3), the polkit stop permission and A-22, the mount assert, the kill marker's home, `/run/lock` writable for the sandboxed export (M1), `TimeoutStopSec=60` against `stop_wait_ms` (D3), `LimitCORE=0`; U20b2 amends it with the hold classes (four, `unmonitored` in its own), the `ProjectorDown` inhibit rule and the market-data deviation (D7); U20b3 with the `OmsDown` inhibit rule and the `ALERTS` source of the open-alert panel. Alternatives: root watcher, user units, sudoers, `Restart=on-failure` with a start limit (the runbook's old example), OMS enabled at boot, watcher not enabled at boot, inhibiting market-data alerts. `registry/decisions.toml` gets the ADR-036 entry (Korean, like the others).
- **assumptions.toml** (U20b1): A-22 (polkit details `unit`/`verb` for `StopUnit`; not checked; if broken K3 fails and `OMS 정지 실패` is sent; detection: the X1 install step of runbook §13.6); A-23 (a Unix socket on a read-only bind mount accepts `connect`; checked with bwrap on this PC on 2026-11-25).
- **contracts.toml**: no new journal event, no change. **claims.toml**: no new claim (same reason as U20a: verify suites live in the owner-reviewed `registry.rs`).
- **Runbook** `docs/operations/observability.md` §13 (U20b1, Korean): 13.1 사용자 `hloms`, 그룹, 데이터 디렉터리 권한(`chgrp`/`chmod`), `/mnt/nvme/lake/crypto/trading`, polkit 규칙 설치, 그리고 `/etc/hl-oms`의 소유자와 모드 표(N5: 디렉터리 `root:hloms` 0750 — 프로젝터는 보조 그룹 `hloms`로 들어감; `oms-node.toml`·`watcher.toml`·`oms-projector.toml`·`oms-export.toml`은 `root:hloms` 0640; 비밀 파일 넷은 `hloms:hloms` 0600; 판단(모델 아님)); 13.2 설정 파일에 들어갈 값 표(`data_dir`, `[kill] marker = "/var/lib/hl-oms/kill.marker"`, `oms_unit = "oms-node.service"`, `systemd = "system"`, `stop_wait_ms = 10000`(기본값; `TimeoutStopSec=60`과 다른 이유는 D3), OMS `[http] listen`의 포트와 같은 `status_url`(프로젝터·워처·내보내기), `cursor_file`, `vm_url`/`vl_url`, 그리고 규칙이 기대는 값: 프로젝터 `poll_ms` ≤ 10000, 워처 `[intervals] export_ms` ≤ 20000(둘 다 기본 1000; 넘으면 `OmsDown`이 안 울리거나 `WatcherDown`이 깜박임, S7)); 13.3 시작 순서(저널 마운트 확인 → `systemctl start oms-node` → `/api/status` 답 확인 → `systemctl start watcher`; 프로젝터는 언제든; OMS가 답한 뒤 2분 안에 워처의 첫 하트비트가 없으면 `OmsUnmonitored` P1이 온다 — 정상 시작에서 `unmonitored`는 몇 초만 걸림); 13.4 재부팅 뒤(첫 단계: `findmnt /mnt/nvme/hl-oms-journal`과 `systemctl is-active watcher` — 저널이 마운트되지 않았으면 워처는 뜨지 않고 킬도 일괄 취소도 없다: 마운트하고 워처를 띄우고, 워처의 킬이 끝날 때까지 Bybit의 걸린 주문을 손으로 확인(S3); 마운트돼 있으면 워처가 먼저 떠 킬함: 일괄 취소, 마커, OMS는 래치된 채 시작; `sudo -u hloms omsctl --socket /mnt/nvme/hl-oms-journal/data/admin.sock clear-hold --reason kill --note …`; `admin.sock`은 `hloms`의 uid만 받으므로 `sudo -u hloms`); 13.5 계획된 정지(워처를 먼저 멈춤 → `halt` → `cancel-all` → 0건 확인 → `systemctl stop oms-node`); 13.6 킬 뒤 다시 띄우기와 해제 명령 표(TN 표 7-7의 여덟 사유와 해제 명령을 systemd 맥락으로: `kill_latched`·워처 킬 → 사람이 `systemctl start oms-node` 뒤 `clear-hold --reason kill`(마커 삭제), `watcher_mismatch` → `clear-hold --reason watcher_mismatch`, `venue_change` → `ack` + `clear-hold --reason venue_change`, `unmonitored`·`strategy_link` → 자동, `reducing` → `clear-reduce`, 운영자 → `resume`; 그리고 행 `OMS 정지 미확인`: 60초 기다린 뒤 `systemctl is-active oms-node` — `inactive`/`failed`면 멈춘 것, `active`/`deactivating`이면 관리자 권한으로 `systemctl kill -s SIGKILL oms-node`(D3, S4)), 그리고 X1 설치 확인(먼저 `pkaction --version`이 0.106 이상인지: JavaScript 규칙은 그 뒤 버전만 읽음(N1); polkit: `sudo -u hloms systemctl --no-ask-password stop oms-node.service`는 되고 `… stop grafana.service`, `… start oms-node.service`는 거절돼야 함, A-22); 13.7 내보내기 타이머(종료 코드 3은 OMS가 꺼진 날; 03:30을 놓친 재부팅마다 `Persistent=true`로 부팅 때 돌아 OMS가 없으니 실패로 남음, N4). U20b2: §9 알림 표에 `oms.rules.json`의 행(`ProjectorDown` 행에: 프로젝터가 꺼진 동안 스택에는 OMS 생존 정보가 없고 `oms_*` P1이 모두 조용해진다 — OMS와 워처의 직접 알림이 그 경로다, N6), §10에 `ProjectorDown` 억제 규칙과 시세 알림을 막지 않는 이유. U20b3: §9에 `watcher.rules.json`의 행, §10에 `OmsDown` 억제 규칙, §12.8 D1, §12.9 D4.
- `docs/operations/runbook.md`: §4 (the untried example) is replaced by a pointer to `deploy/observability/systemd/oms-node.service` and §13, saying `Restart=no` and no `[Install]`; the `systemd 단위` status row (`runbook.md:18`) names the OMS units; 7.18 rows that say "U20b가 그 권한을 주기 전에는 K3가 늘 실패한다" (`runbook.md:542`) and step 7 (`:544`)'s "`Restart=on-failure`, 4장; 설계는 `Restart=no`, U20b" are rewritten for the merged units (U20b1). `docs/operations/watcher.md`: the example `oms_unit = "hl-oms.service"` becomes `"oms-node.service"`, and the systemd paragraph (`watcher.md:227`) points to §13 (U20b1). `docs/operations/oms-export.md:5` and `oms-projector.md` name their units (U20b1).
- **GLOSSARY.md**: (U20b1) Stop permission (정지 권한; _Avoid_: "kill permission"), Journal mount assert (저널 마운트 확인); (U20b2) Trading venue list (거래 거래소 목록), Hold alert class (보류 알림 등급).
- **README.md**: the structure table's xtask line count and the `deploy/` row (each part).
- **docs/generated**: regenerated in every part.

## Out of Scope

- Inhibiting market-data alerts while the OMS is down: not done, see D7 and Q4 (the scope's line). Moved to an owner decision; no unit takes it unless the owner asks.
- Binance, OKX, Hyperliquid trading or market data: U10/U11 deferred; `trading = ["bybit"]`.
- Strategy processes, their units and `allowed_uids` on the trading socket: outside the design (table 8-6 "전략 프로세스 … 이 설계 밖").
- Separate uids for the OMS and the watcher: needs a code change (`server.rs:188–192` accepts only the OMS's uid on `admin.sock`/`monitor.sock`); a follow-up code unit, not planned (Q3).
- The OMS's config files, the watcher's and the projector's: the operator's (D0); the runbook lists their unit-facing values.
- Watcher logs in VictoriaLogs (the watcher writes stderr only): D4 shows the OMS's journal events and the projector's own lines; watcher lines stay in `journalctl -u watcher`.
- `oms_rate_tokens` (not exported until U17, `metrics.rs:30`).
- Installing, enabling or starting anything; creating `hloms`; the journal image (X10); the polkit rule in `/etc`; X1, X2, X4.
- A CI step for `vmalert`, `amtool` or `systemd-analyze` (needs the binaries at CI time and a `.github/` change, as U20a).
- Measuring the 100M caps of the watcher and the projector (both replay the whole journal into memory at start): pilot (Q6).

## Owner questions (the parts use the default; the runner does not wait)

1. **Q1 OMS at boot.** Default: `oms-node.service` has no `[Install]`; a reboot does not start the OMS, a person does (as after a crash or a kill).
2. **Q2 Watcher at boot.** Default: enabled. After every reboot with the journal file system mounted it kills the not-running OMS: cancel-all of resting orders with its key, kill marker written, the next OMS start is latched until `clear-hold --reason kill`. Cost: one `[WATCH P1]` kill sequence and one release command per reboot. Limit: when the journal mount is missing the watcher does not start, so there is no boot kill and the resting orders stay (D3); only `WatcherDown` P1 and `systemctl --failed` show it, and §13.4 makes the mount check the first step after a reboot. A boot kill without the journal needs a different watcher unit (owner's choice, not taken).
3. **Q3 One user for OMS and watcher.** Default: both run as `hloms` (forced by the socket uid check); each can read the other's API key file. Separating them needs a code change.
4. **Q4 Market-data alerts during an OMS outage.** Default: not inhibited (the scope asked for it); only the watcher alerts that follow from a dead OMS are held back.
5. **Q5 Thresholds** of D6 (30 s OMS down, 2 min projector absent, 5 min passing hold, 2 min `unmonitored`, 10% limit distance, 10 000 records lag, 10 min not caught up, 30 s check age and heartbeat age, 1 min watcher absent, 5 min W5 unarmed, 2 min public IP) and the config bounds they need (`poll_ms` ≤ 10 000, `export_ms` ≤ 20 000). Default: as listed, reviewed weekly. Expected noise: `OmsLimitNear` (P2) fires for as long as a strategy sits at a position limit (distance 0, limit ≠ 0); that is by design, not a fault.
6. **Q6 Memory caps of watcher and projector (100M each, design).** Default: design values; a cap hit kills the process (the watcher is restarted after 5 s and replays the journal again). Pilot measures.
7. **Q7 Duplicate P1s.** Default: the stack sends `[MON P1]` for states the OMS and the watcher already send directly (holds, kill, mismatch): two independent paths, more messages.
8. **Q8 Export on a day the OMS is down.** Default: `oms-export` exit 3 leaves the unit failed (seen in `systemctl --failed`); the next successful run writes the missed dates. Cost: the timer is `Persistent=true` and the OMS never runs at boot, so every reboot that missed 03:30 UTC also leaves the export unit failed with exit 3 until the next day's run.

Decided here and reported, not asked: plain unit names for the OMS units (one OMS per PC); the watcher's `Restart=on-failure` (the scope sets only the OMS's restart); the projector as `hlobs` in the `hloms` group; `LimitCORE=0`; the mount assert; `/run/lock` writable in the sandboxed export unit; `unmonitored` in its own hold class with `for` 2m; `stop_wait_ms = 10000` kept under `TimeoutStopSec=60`; `MdObsPushDropping` filtered to the collector (a U20a rule changed); `ALERTS` as the open-alert source of D1 (D8).

## Further Notes

- The check covers files in this repository only. Whether the operator's `/etc/hl-oms/*.toml` hold the values of §13.2 is not checked by anything; a mismatch (for example a watcher `oms_unit` that is not `oms-node.service`) shows only at the first kill (`OMS 정지 실패`). §13.2 says to compare them at install time.
- The watcher's boot kill (Q2) depends on the OMS's `[kill] marker` and the watcher's being the same path; the watcher refuses to start when the OMS reports another marker path (`runbook.md:523`).
- Not checked by anything (spec review 1, S7): the rules need projector `poll_ms` ≤ 10 000 and watcher `export_ms` ≤ 20 000, but both values live in the operator's `/etc/hl-oms/*.toml`, which are not deploy files (D0). §13.2 lists them; a larger value is seen only as an `OmsDown` that does not fire or a `WatcherDown` that flaps.
- A-22 (spec review 1, N1): the reviewer recalls that systemd has passed `unit` and `verb` to polkit for `manage-units` since v226 (systemd NEWS). Not looked up here (no network in this stage; the local man page does not say it). A-22 stays "unverified" until X1; the X1 step also checks `pkaction --version` ≥ 0.106 (JavaScript rules).
- Spec review 1 findings: M1, S1–S7 and N1–N6 were all accepted; none was judged wrong. The split changed (S6): the watcher rules moved from U20b2 to U20b3.
