# Spec: observability deployment part 1 — stack, collector, seal, archiver and audit units, lake and disk rules, market-data and system dashboards (unit U20a, PRs U20a1–U20a3)

Status: ready-for-agent

Design source: `RESEARCH/CRYPTO_TRADING_INTEGRATED_TN_20261118/10_DESIGN_DRAFT.md` §5.4.5 (metric names), §5.5 (retention), §8.3 (alert paths and severities), §8.4 (dashboards), §8.5 (memory caps), amendments R1 (9.4 disk policy), R2 (G1 journal placement, S7 seal timer at minute 5), R3 (rule home, chrony `makestep 1 3`), R4 (collector death rule, days-left recording rule), R5 (resource isolation); TN PDF table 8-6 (memory caps), table 8-6a (protection settings), table 9-4 (G1–G4), table 8-5 (Grafana settings); `pr_plan.json` unit U20a (scope, checks, done_criteria). qdp auditor: `quant-data-platform` origin/main `d6fe094`, `docs/crypto/auditor.md`.

Numbers marked 판단(모델 아님) are judgements without a model or a measurement behind them. The design's own numbers (10 s and 30 s evaluation, 60 s and 2 min liveness, 7 and 2 days, 2 hours seal lag, the 8.5 caps, OOMScoreAdjust −900, CPUWeight 1000, MemoryHigh 8G) are design values and are cited as such; the design marks most of them as judgements too.

Nothing in this unit is installed, enabled or started on any machine. The parts add files under `deploy/observability/`, a test under `crates/xtask/tests/deploy/`, mutants and documents. Market data is Bybit only (U10/U11 deferred by the owner).

## Problem Statement

The collector (U5b, U6), the seal (`md-lake seal`) and the archiver (U7) exist as code and three template units, and the qdp auditor (U8) exists in qdp. Nothing describes how the observability stack (VictoriaMetrics, VictoriaLogs, two vmalert instances, Alertmanager, Grafana) runs on this PC, where it keeps its data, how much memory each service may use, or which rules raise which alert. Without rules, a dead collector, a full lake disk, a failed L0 write or a stuck seal is seen only by the direct `[MD P1]` path or not at all, and nobody looks at the market-data numbers the collector already pushes. Without a check, a rule can name a metric that no process exports and stay silent forever, and a collector unit can lose `Restart=always` or its OOM protection in an edit without anyone noticing.

## Solution

Files only, plus one test:

1. **U20a1** — systemd units for the stack, the collector, the seal, the archiver and the qdp audit, a `trading.slice`, timers with fixed schedules, an install map, and the first half of the deployment check (units, timers, slices, paths). Runbook `docs/operations/observability.md` with the storage layout, the protection settings, the research-job rule and chrony.
2. **U20a2** — vmalert rule files (market data, lake, system; one log rule file), the metric catalogue, the Alertmanager config with Telegram (token and chat id from files only) and a placeholder dead-man receiver, rule tests for `vmalert-tool`, and the rule half of the check (every metric name exists, every alert has a severity, one liveness rule per configured venue, the design's lake and disk rules).
3. **U20a3** — Grafana: `grafana.ini`, provisioning for the VictoriaMetrics and VictoriaLogs datasources, dashboards D2 (market-data collection) and D3 (system), and the dashboard half of the check.

## Facts from the existing code (origin/main 0806fcc)

### Existing unit templates (moved and changed by U20a1)

- `deploy/systemd/md-collector@.service`: Type=notify, `WatchdogSec=30s`, `NotifyAccess=main` (lines 12–14), `RuntimeDirectory=md-collector-%i` and `RuntimeDirectoryPreserve=restart` for the alert state (17–18), `ExecStart=/usr/local/bin/md-collector run --config /etc/md-collector/%i.toml` (19), `Restart=always` (20), `RestartSec=5` (21), `RestartPreventExitStatus=2` (23), `MemoryMax=1G` (25), `KillSignal=SIGTERM`, `TimeoutStopSec=30` (27–28). No `User=` (runs as root), no slice, no OOM or swap settings.
- `deploy/systemd/md-lake-seal@.service`: oneshot, `SuccessExitStatus=3` (lock held by another seal run, 9–10), `LAKE=/mnt/nvme/lake` (11), `FIRST_HOUR` (12), `TELEGRAM=/etc/hl-oms/telegram.json` (16), `ExecStart=/usr/local/bin/md-lake seal --venue %i --lake ${LAKE} --first-hour ${FIRST_HOUR} --telegram ${TELEGRAM}` (17). No `MemoryMax`.
- `deploy/systemd/md-lake-seal@.timer:6`: `OnCalendar=*-*-* *:02:00 UTC` — the design moved the seal to minute 5 (R2 S7, TN table 8-6 "매시 5분").
- `deploy/systemd/md-archiver@.service`: oneshot (8), `ReadOnlyPaths=/mnt/nvme/lake/crypto/l0 /mnt/nvme/lake/crypto/audit` (11), `THREADS=1` (14), `RETAIN_ARGS=` empty (17), `MemoryMax=8G` (18), `Nice=10` (19), `ExecStart=/usr/local/bin/md-archiver run --venue %i --lake ${LAKE} --threads ${THREADS} $RETAIN_ARGS` (20).
- `deploy/systemd/md-archiver@.timer:8`: `OnCalendar=*-*-* 01:10:00 UTC`, `Persistent=true` (9). Its comment (6–7) assumes the hour-23 seal runs at 00:02.
- Measured archiver memory: `docs/operations/md-archiver.md:65` — peak RSS 2.57 GB for a 3.2 M-row book topic-day with `--threads 1` (2026-11-19, release). The design cap for daily jobs is 4 GB (TN table 8-6, R1 "daily jobs memory cap 4 GB").
- Nothing in the code or the tests references `deploy/` (searched `*.rs`, `*.toml`, `*.sh`, `*.yml` under the worktree; only `docs/operations/md-collector.md:5`, `docs/operations/md-archiver.md:5` and `.scratch/` notes name `deploy/systemd/`).

### Binaries and their flags

- `md-collector run --config <file>`; exit 2 = configuration refused (`docs/operations/md-collector.md:10–24`).
- `md-lake seal --venue V --lake DIR --first-hour YYYY-MM-DDTHH [--layout ..] [--telegram FILE]`, `md-lake verify --venue V --lake DIR --date YYYY-MM-DD --json` (`crates/md-collector/src/bin/md-lake.rs:7`, `:34`). `verify` prints the result and writes `crypto/l0/_verify/venue=<v>/date=<d>.json` (`crates/md-lake/src/verify.rs:126`, `:148`); nothing runs `verify` today (no caller outside the CLI in `crates/`).
- `md-archiver run --lake DIR --venue V [--small-channels a,b] [--rebuild FROM..TO] [--retain-l1-days N] [--disk-floor-bytes B] [--threads N]` (`crates/md-archiver/src/bin/md-archiver.rs:5–7`, `:36`); `--venue` is required (`:68`).
- qdp auditor (qdp origin/main `d6fe094`, `docs/crypto/auditor.md`): `uv run --no-sync python -m quant_data.crypto audit --lake L --venue V --state FILE [--vm-url URL]`; one venue per run and per state file; `--state` must be outside the lake's `crypto/`; exit 0 clean, 1 some date `fail`, 2 usage/state refused, 3 write error, 4 metrics push failed; "Run one auditor per state file at a time (a systemd oneshot). There is no lock file." Metrics are POSTed to `URL/api/v1/import/prometheus`.
- Registered venues: `crates/md-collector/src/registry.rs:130` (`"bybit" => Some(&Bybit)`), `crates/md-archiver/src/registry.rs:100` (same). Only Bybit.

### Metrics actually exported (the U6 list)

All pushed through `obs-push` to VictoriaMetrics `/api/v1/import` (no scraping):

| Name | Labels | Source |
|---|---|---|
| `lake_l0_open_files`, `lake_l0_bytes_total`, `lake_l0_write_errors_total`, `lake_disk_free_bytes` | venue | `crates/md-collector/src/obs.rs:207`, `:209`, `:211`, `:219` |
| `md_queue_bytes` | venue, conn | `obs.rs:227` |
| `md_collector_heartbeat_ts` (Unix seconds, priority queue, every `slow_ms`) | venue | `obs.rs:235`, `:264–266` |
| `node_clock_offset_ms` | host | `obs.rs:293` |
| `lake_seal_lag_hours` (every 60 s) | venue | `obs.rs:316` |
| `md_spread_bp`, `md_mid_px` | venue, market, symbol, channel | `crates/md-collector/src/telemetry.rs:433`, `:437` |
| `md_depth_qty` | + side, levels | `telemetry.rs:453` |
| `md_recv_lag_ms` | + quantile (`0.5`, `0.99`) | `telemetry.rs:461` |
| `md_gaps_total`, `md_reconnects_total`, `md_last_msg_age_s` | venue, market, symbol, channel | `telemetry.rs:466`, `:469`, `:475` |
| `md_msgs`, `md_bytes` (sum over the period, reset after each push) | venue, market, channel, conn | `telemetry.rs:491–494` |
| `md_recv_lag_negative_total` | venue | `telemetry.rs:499` |
| `obs_push_dropped_total`, `obs_push_queue_lines` | service, venue, endpoint (`vm`, `vm_priority`, `vl`) | `crates/obs-push/src/lib.rs:586–597`; self labels `service`, `venue` set at `crates/md-collector/src/collector.rs:82` |

- `lake_l0_write_errors_total` is 0 or 1: the writer stops at the first error (`obs.rs:210–216`). After a restart the new process starts at 0.
- The collector computes no days-left value (`docs/operations/md-collector.md:126`, "vmalert, U20a").
- Log lines to VictoriaLogs carry `service="md-collector"`, `host`, `venue`, `level` (`warn`/`error`/…) and the fields (`obs.rs:24`, `:45–73`), with `_stream_fields=service,host,venue` (`crates/obs-push/src/lib.rs:290`).
- `oms_*`, `projector_*`, `journal_disk_free_bytes` (`crates/oms-projector/src/metrics.rs:118–205`, `projector.rs:646`) and `watcher_*` (`crates/watcher/src/metrics.rs:75`) are U20b's rules; U20a's rules do not use them.

### The pinned qdp metric list

- `contracts/lake/xrepo/metrics.json` already exists in the vendored lake contract: `metrics` = `lake_audit_fail_total`, `lake_audit_clean_total` (counters), `lake_audit_not_audited_files`, `lake_audit_last_run_ts`, `lake_audit_gap_rows{kind}` (gauges), all with label `venue` (lines 3–45); `reserved` = `lake_l0_open_files`, `lake_l0_bytes_total`, `lake_l0_write_errors_total`, `lake_seal_lag_hours`, `lake_disk_free_bytes`, `lake_disk_days_left` (46–53) — names hl-oms owns.
- It is vendored with its qdp commit and sha256: `contracts/lake/VENDOR.toml:3–6` (source commit `d6fe0942…`, `index_sha256`), `contracts/lake/index.json:237` lists `xrepo/metrics.json`, and `crates/md-core/tests/vendored.rs:38–64` checks the copy against the index and the index against `VENDOR.toml`. U20a reads this copy; it vendors nothing new.

### The check's home and the owner gate

- `.github/workflows/ci.yml:68` runs `cargo test --workspace --locked`; `:65` runs `xtask layers`. A new CI step would change `.github/`.
- `tools/ci/owner_review_paths.txt` lists `.github/`, `tools/ci/`, `crates/xtask/src/layers.rs`, `crates/xtask/src/registry.rs`, `crates/xtask/src/main.rs`: a change to any of them stops the runner's merge for the owner (ADR-033 §part 1, `docs/adr/ADR-033-repository-standards.md:54–58`). A new xtask subcommand needs `main.rs`; a new verify suite for a claim needs `registry.rs:92–106` (`suite_dirs`) and claim tests are checked against those suites (`registry.rs:461–480`).
- `crates/xtask/src/mutants.rs` is not on that list. A mutant is `{id, what, file, find, replace, test, guards}` (`mutants.rs:15–24`); the last id at 0806fcc is M323 (`mutants.rs:2862`). The mutant run copies only `crates`, `config`, `Cargo.toml`, `Cargo.lock` into its work directory (`mutants.rs:2975`) and clears only `work/crates` (`:2972`).
- xtask's normal dependencies are `serde`, `serde_json`, `toml` (`crates/xtask/Cargo.toml`); an integration test may use them. No YAML crate is in `Cargo.lock` (searched `serde_yaml`, `serde_yml`, `yaml-rust`, `yaml-rust2`, `saphyr`, `serde_norway`), and none is in the local cargo cache.
- Test files start with `#![allow(clippy::unwrap_used)]` (`crates/watcher/tests/metrics.rs:4`; ADR-033 part 3 note).

### Tools checked at the spec stage (2026-11-25, this PC, binaries in the unit tmp dir, checksums equal to the release checksum files)

- VictoriaMetrics, vmalert, vmalert-tool v1.157.0; VictoriaLogs v1.54.0; Alertmanager/amtool 0.35.1; Grafana OSS 13.3.2; systemd 259.
- `vmalert -dryRun` accepts a rule file written in JSON syntax and rejects a bad MetricsQL expression with a `fatal` line ("bad MetricsQL expr"; exit status not recorded). A `type: vlogs` group with a LogsQL `stats` expression passes `-dryRun`.
- `vmalert-tool unittest` on prototype rules (JSON syntax): the liveness rule `time() - max by (venue) (md_collector_heartbeat_ts{venue="bybit"}) > 60 or absent_over_time(md_collector_heartbeat_ts{venue="bybit"}[2m])` does not fire while beats arrive, fires 70 s after the last beat, fires when only another venue's series exists, and the absent branch carries `venue="bybit"`; `max by (venue) (lake_l0_write_errors_total) > 0 or max by (venue) (increase(lake_l0_write_errors_total[10m])) > 0` fires on 0→1 and still fires after the series restarts at 0; `lake_disk_free_bytes / clamp_min(-deriv(lake_disk_free_bytes[24h]) * 86400, 1)` gives 1.0 for a series that loses its whole value in a day; a daily-sampled `lake_audit_last_run_ts` with `last_over_time(..[3d])` and `absent_over_time(..[36h])` does not fire 2 h after a run and fires 42 h after the last one; `increase(lake_audit_fail_total[26h])` sees a daily 0→2 step. All four test files: SUCCESS.
- `amtool check-config`: a JSON config with `telegram_configs` using `bot_token_file` and `chat_id_file` passes; `chat_id: 0` fails ("missing chat_id or chat_id_file"). `amtool config routes test` sends `alertname=Watchdog` to the dead-man receiver, `severity=P1` to the P1 receiver, `severity=P3` to the null receiver.
- Grafana 13.3.2 bound to 127.0.0.1 with data in tmp: provisioning files written in JSON syntax with a `.yaml` name load; both datasources (`prometheus` type for VM, `victoriametrics-logs-datasource` for VL, plugin not installed) are listed read-only, the file-provisioned dashboard is found by `/api/search`.
- A throwaway VM with `-promscrape.config` (JSON) scraping VM, VL, vmalert and Alertmanager on 127.0.0.1 stored `up{job,instance}` for all four; their `/metrics` expose `process_resident_memory_bytes`, `vm_free_disk_space_bytes`, `vl_free_disk_space_bytes`, `vmalert_iteration_missed_total`, `vmalert_execution_errors_total`, `vmalert_alerts_send_errors_total`, `alertmanager_notifications_total`, `alertmanager_notifications_failed_total`. vmalert delivered `Watchdog` and `MdCollectorDown` to Alertmanager. Every process was killed before the stage ended (checked: no process, port closed).
- `systemd-analyze verify` passes on prototype units with `RestartSteps=`, `RestartMaxDelaySec=`, `MemorySwapMax=0`, `MemoryMin=`, `OOMScoreAdjust=-900`, `IOWeight=`, `Slice=trading.slice`, a slice with `CPUWeight=`/`MemoryMin=`, and a template instance; it reports a missing `ExecStart` binary as "is not executable" and does not report an unknown `User=`.

## Split into PRs (merge order)

| Part | Branch | Contents | Size (판단(모델 아님)) |
|---|---|---|---|
| U20a1 | `observability-deploy-units` (continues from the spec branch) | units, slice, timers, install map, venues list, check part 1 (tree, parsers, unit rules), runbook first half, ADR-035, mutant copy list | Rust ≈ 900 incl. tests; unit files ≈ 350; docs ≈ 300 |
| U20a2 | `observability-alert-rules` | vmalert rule files, `vmalert-tool` tests, metric catalogue, scrape config, Alertmanager config, check part 2 (expression scanner, catalogue, rule rules, Alertmanager rules), runbook alert sections | Rust ≈ 900; JSON ≈ 600; docs ≈ 200 |
| U20a3 | `observability-grafana-dashboards` | `grafana.ini`, provisioning, D2 and D3 dashboards, check part 3, runbook Grafana section | Rust ≈ 350; dashboard JSON ≈ 900 (data); docs ≈ 120 |

Each part keeps `the_deploy_tree_passes` green on its own HEAD: a rule of a later part is not checked before its files exist.

## Decisions (all parts)

### D0. Layout, file syntax and the install map

- Everything lives under `deploy/observability/`. U20a1 moves the three existing templates there with `git mv` (`deploy/systemd/` → `deploy/observability/systemd/`), then edits them in a separate commit, and removes the empty `deploy/systemd/`. U20b adds its units to the same directory; the check covers every unit under `deploy/` wherever it is.
- Tree (final, after U20a3):
  ```
  deploy/observability/
    README.md                     three lines: not installed; see docs/operations/observability.md
    install.toml                  repo file -> install path, mode
    venues.toml                   venues = ["bybit"]
    metrics.toml                  the metric catalogue (U20a2)
    systemd/  victoriametrics.service victorialogs.service vmalert-metrics.service vmalert-logs.service
              alertmanager.service grafana.service trading.slice
              md-collector@.service md-lake-seal@.service md-lake-seal@.timer
              md-archiver@.service md-archiver@.timer crypto-lake-audit@.service crypto-lake-audit@.timer
    victoriametrics/scrape.json   (U20a2)
    vmalert/metrics/market-data.rules.json lake.rules.json system.rules.json   (U20a2)
    vmalert/logs/market-data-logs.rules.json                                   (U20a2)
    vmalert/tests/*.test.json     vmalert-tool unittest files, not installed (U20a2)
    alertmanager/alertmanager.json                                             (U20a2)
    grafana/grafana.ini  grafana/provisioning/datasources/hl-oms.yaml
    grafana/provisioning/dashboards/hl-oms.yaml  grafana/dashboards/d2-market-data.json d3-system.json   (U20a3)
  ```
- **JSON syntax for every YAML consumer.** vmalert rules, the scrape config, the Alertmanager config, Grafana provisioning and the vmalert-tool tests are JSON documents (JSON is valid YAML; checked above with the real tools). Rule and Alertmanager files are named `*.json`; Grafana provisioning keeps the `.yaml` name it requires. Reason: the check then parses exactly what the tools parse, with `serde_json` (already a dependency), and no YAML crate enters `Cargo.lock` (no network fetch, no new supply-chain surface). Cost: no comments in those files; every alert carries an `annotations.summary`, and the runbook explains each file. Alternative rejected: a hand-written YAML subset parser in the test (it could accept what vmalert rejects).
- **Install map** `install.toml`: `[[file]] src = "<path under deploy/observability>"`, `dst = "<absolute path>"`, `mode = "0644"` (or `0755` for none; no executables are shipped). Install prefixes (판단(모델 아님)): units → `/etc/systemd/system/`; configs → `/etc/hl-oms-obs/<same sub-path>`; Grafana dashboards → `/etc/hl-oms-obs/grafana/dashboards/`. `vmalert/tests/`, `README.md`, `install.toml`, `venues.toml`, `metrics.toml` are not installed. The runbook's install section is a list of `install -D -m <mode> <src> <dst>` lines generated by hand from this map (the operator runs them; this repository does not).
- **Install-time paths outside the repository** (all placeholders, created by the operator): binaries `/usr/local/bin/{victoria-metrics-prod,victoria-logs-prod,vmalert-prod,alertmanager,md-collector,md-lake,md-archiver}`, Grafana tree `/opt/grafana`; secrets `/etc/hl-oms/alertmanager-telegram-token`, `/etc/hl-oms/alertmanager-telegram-chat-id`, `/etc/hl-oms/grafana-admin-password` (mode 0640, group `hlobs`), and the existing `/etc/hl-oms/telegram.json` of `md-lake seal`; qdp checkout `/opt/qdp-crypto` (D5).

### D1. Storage layout (runbook only; G1, R2, table 9-4)

- Journal: `/mnt/nvme/hl-oms-journal`, a fixed-size ext4 file system of about 64 GB (design value, R2 G1) on the 970 EVO Plus, e.g. an image `/mnt/nvme/hl-oms-journal.img` mounted on a loop device (operations item X10). Never on `/` (980 500GB: A-LAT-1 failed there, hl-oms ADR-012, 27.2 ms vs 20 ms). It separates space only, not latency (R4 SHOULD 6); pilot P6 re-measures. U20a writes no journal unit (U20b) and no disk rule for the journal (U20b).
- `/mnt/nvme/lake` (lake), `/mnt/nvme/observability/victoria-metrics`, `/mnt/nvme/observability/victoria-logs`, `/mnt/nvme/observability/alertmanager`, `/mnt/nvme/observability/grafana` (data and plugins). Each a directory of its own on `/mnt/nvme`.
- The check (rule U9) refuses a storage flag of a unit or a Grafana data path that does not start with `/mnt/nvme/`.

### D2. Stack units (U20a1)

All stack units: `Type=simple`, `User=hlobs`, `Group=hlobs` (a system user the operator creates, 판단(모델 아님)), `Restart=on-failure`, `RestartSec=5` (판단(모델 아님)), `NoNewPrivileges=yes`, `ProtectSystem=strict`, `ProtectHome=yes`, `PrivateTmp=yes`, `ReadWritePaths=<its data dir>`, `[Install] WantedBy=multi-user.target`. Every listener binds `127.0.0.1` except Grafana (D9). Every unit carries the `MemoryMax` of design §8.5 / TN table 8-6:

| Unit | ExecStart flags (beyond the binary) | MemoryMax |
|---|---|---|
| `victoriametrics.service` | `-storageDataPath=/mnt/nvme/observability/victoria-metrics -retentionPeriod=1y -memory.allowedBytes=600MiB -httpListenAddr=127.0.0.1:8428 -promscrape.config=/etc/hl-oms-obs/victoriametrics/scrape.json` | 1G |
| `victorialogs.service` | `-storageDataPath=/mnt/nvme/observability/victoria-logs -retentionPeriod=1y -memory.allowedBytes=600MiB -httpListenAddr=127.0.0.1:9428` | 1G |
| `vmalert-metrics.service` | `-rule=/etc/hl-oms-obs/vmalert/metrics/*.json -datasource.url=http://127.0.0.1:8428 -remoteWrite.url=http://127.0.0.1:8428 -remoteRead.url=http://127.0.0.1:8428 -notifier.url=http://127.0.0.1:9093 -evaluationInterval=10s -httpListenAddr=127.0.0.1:8880` | 200M |
| `vmalert-logs.service` | `-rule=/etc/hl-oms-obs/vmalert/logs/*.json -rule.defaultRuleType=vlogs -datasource.url=http://127.0.0.1:9428 -remoteWrite.url=http://127.0.0.1:8428 -remoteRead.url=http://127.0.0.1:8428 -notifier.url=http://127.0.0.1:9093 -evaluationInterval=30s -httpListenAddr=127.0.0.1:8881` | 200M |
| `alertmanager.service` | `--config.file=/etc/hl-oms-obs/alertmanager/alertmanager.json --storage.path=/mnt/nvme/observability/alertmanager --web.listen-address=127.0.0.1:9093 --cluster.listen-address=` (empty: no gossip port, 판단(모델 아님)) | 100M |
| `grafana.service` | `/opt/grafana/bin/grafana server --homepath /opt/grafana --config /etc/hl-oms-obs/grafana/grafana.ini`, `WorkingDirectory=/opt/grafana` | 512M |

- `-retentionPeriod=1y` for VM and VL: design §5.5 (defaults 1 month and 7 days). `-memory.allowedBytes=600MiB`: design §8.5 "약 0.6 GB" for VM (판단 there); the same for VL is 판단(모델 아님). vmalert's `-rule.evalDelay` keeps its default (30 s), so a rule sees the store as of 30 s ago: the collector-liveness alert fires about 90–100 s after the last beat (60 s threshold + 30 s delay + up to 10 s evaluation, 판단(모델 아님); measured in X1).
- Ports 8428, 9428, 9093 are the tools' defaults; 8880/8881 are vmalert's default and default + 1.
- `Wants=`/`After=`: vmalert-* `After=victoriametrics.service victorialogs.service alertmanager.service`; no `Requires=` (a restart of VM must not stop vmalert).
- At boot nothing starts until the operator enables the units (none is enabled by this repository). At shutdown systemd sends SIGTERM; defaults otherwise.

### D3. `trading.slice` and the collector unit (U20a1; R4, R5, TN table 8-6a)

- `trading.slice`: `[Slice] CPUWeight=1000`, `MemoryMin=512M`. cgroup v2 limits a child's effective `memory.min` by its ancestors' (kernel `Documentation/admin-guide/cgroup-v2.rst`, memory.min: "the effective min boundary is limited by memory.min values of all ancestor cgroups"), so the slice must reserve at least the sum of its units' `MemoryMin`. U20b raises it when it adds oms-node and the watcher. Check rule U4.
- `md-collector@.service` = the existing template plus: `[Unit] StartLimitIntervalSec=0` (no start limit: the default 5 starts in 10 s with `RestartSec=2` could stop the collector for good, 판단(모델 아님)); `RestartSec=2` (design R4; was 5); `RestartSteps=5`, `RestartMaxDelaySec=60s` (a crash loop backs off from 2 s to 60 s instead of hitting the venue every 2 s, 판단(모델 아님); systemd ≥ 254, this PC has 259); `Slice=trading.slice`; `OOMScoreAdjust=-900`; `MemorySwapMax=0`; `MemoryMin=512M` (half of `MemoryMax=1G`); `IOWeight=1000` (expected to have no effect on this PC, table 8-6a). Kept: Type=notify, `WatchdogSec=30s`, `NotifyAccess=main`, `Restart=always`, `RestartPreventExitStatus=2`, the runtime directory lines, `MemoryMax=1G` (Bybit; other venues 0.5 GB later, design §8.5), `TimeoutStopSec=30`. A comment says the cap and `MemoryMin` of a second venue go in a drop-in.
- Identity: unchanged (no `User=`, as merged in U5b/U6); see owner question Q2.

### D4. Seal, archiver and the daily chain (U20a1)

Schedules (UTC; the check pins them, rule U5):

| Timer | `OnCalendar` | Service | MemoryMax | Why |
|---|---|---|---|---|
| `md-lake-seal@.timer` | `*-*-* *:05:00 UTC` | `md-lake-seal@%i.service` | 1G | R2 S7 "hourly at minute 5", TN table 8-6 |
| `md-archiver@.timer` | `*-*-* 01:10:00 UTC` | `md-archiver@%i.service` | 4G | after the hour-23 seal at 00:05 (65 min margin, 판단(모델 아님)); unchanged time |
| `crypto-lake-audit@.timer` | `*-*-* 02:30:00 UTC` | `crypto-lake-audit@%i.service` | 4G | after the archiver; 판단(모델 아님) |

- Every timer: `Persistent=true` (a missed run fires at the next boot), `[Install] WantedBy=timers.target`.
- **Templates, not plain names.** The scope names `md-archiver.service` and `crypto-lake-audit.service`; this spec keeps per-venue templates (`@`) because both binaries take exactly one `--venue` (`md-archiver.rs:68`, qdp `auditor.md`), a failing venue must not skip the others (several `ExecStart=` lines of one oneshot stop at the first failure), and the existing archiver unit is already a template. The design's "one daily job at a time" (TN table 8-6) is kept by the lock below. Spec-vs-scope deviation, reported.
- **Daily-job lock.** Every daily job's `ExecStart` is `/usr/bin/flock /run/lock/hl-oms-daily.lock <command>`: archiver and audit runs of all venues (and U20b's `oms-export`) run one at a time, so their 4 GB caps never add up. `flock` waits; `TimeoutStartSec=6h` (판단(모델 아님)) bounds a run including the wait. Check rule U7.
- `md-lake-seal@.service`: existing lines plus `MemoryMax=1G` and `TimeoutStartSec=50min` (a seal must end before the next hour's run; P4 target is 15 min per hour, R4 SHOULD 5; 판단(모델 아님)). A seal killed by the timeout restarts from its seal log next hour (U4). `md-lake-seal@.timer` comment and the archiver timer comment change from :02 to :05.
- `md-archiver@.service`: existing lines with `MemoryMax=4G` (was 8G; 2.57 GB measured with one thread, `md-archiver.md:65`), the lock, `TimeoutStartSec=6h`. `THREADS=1` stays (memory is one topic-day per thread).
- `crypto-lake-audit@.service` (new): `Type=oneshot`, `Environment=QDP_CHECKOUT=/opt/qdp-crypto`, `Environment=UV=/usr/local/bin/uv`, `Environment=LAKE=/mnt/nvme/lake`, `Environment=PYTHONDONTWRITEBYTECODE=1`, `WorkingDirectory=/opt/qdp-crypto` (the same value; `WorkingDirectory=` cannot expand variables), `StateDirectory=qdp-crypto-audit` (state file `/var/lib/qdp-crypto-audit/audit-%i.json`, outside the lake as the auditor requires), `CacheDirectory=qdp-crypto-audit` with `UV_CACHE_DIR=/var/cache/qdp-crypto-audit`, `ReadOnlyPaths=/mnt/nvme/lake/crypto/l0 /mnt/nvme/lake/crypto/l1` (the auditor writes only `crypto/audit/`; 판단(모델 아님), the implementer confirms against qdp `audit.py` which directories it writes and widens only to `crypto/audit` and `crypto/l0/_verify`), `MemoryMax=4G`, `TimeoutStartSec=6h`, `Nice=10`.
  - `ExecStartPre=/usr/bin/flock /run/lock/hl-oms-daily.lock /bin/sh -c 'exec /usr/local/bin/md-lake verify --venue %i --lake ${LAKE} --date "$(date -u -d yesterday +%%F)" --json > /dev/null'` — the auditor reads the verify result of each date (`verify.rs:148`) and nothing else writes it. Only yesterday (UTC) is verified (verify decodes every sealed file of a date; a full rescan each day is too slow, 판단(모델 아님)); a missed day shows as `lake_audit_not_audited_files` (P3) and the runbook gives the manual command.
  - `ExecStart=/usr/bin/flock /run/lock/hl-oms-daily.lock ${UV} run --no-sync python -m quant_data.crypto audit --lake ${LAKE} --venue %i --state ${STATE_DIRECTORY}/audit-%i.json --vm-url http://127.0.0.1:8428`.
  - Exit 1 (a `fail` date) and 4 (push failed) leave the unit failed: both are real findings; the alert rules read the metrics.
  - The KR pipeline is never touched: `QDP_CHECKOUT` must be a dedicated checkout of qdp main; the check (rule U12) refuses any deploy file that names `/mnt/nvme/gitproject/quant-data-platform` (prefix, so also `-official`) and refuses a `--state` under the lake.
- Order inside a day (UTC): 00:05 seal of hour 23 + day seal → 01:10 archiver (L1 of every day-sealed date without L1) → 02:30 verify yesterday + audit. `--retain-l1-days` stays off (`RETAIN_ARGS=` empty); the runbook says to enable it only together with an audit `--full` right before the archiver (qdp `auditor.md`, "Run `--full` right before `md-archiver --retain-l1-days`").

### D5. Venues

- `deploy/observability/venues.toml`: `venues = ["bybit"]`. Each venue must be a registered collector venue: the check finds the literal `"<venue>" => Some(` in `crates/md-collector/src/registry.rs` and in `crates/md-archiver/src/registry.rs` (rule U13). One liveness rule per listed venue (rule R6); no liveness rule for an unlisted venue.

### D6. The deployment check (`crates/xtask/tests/deploy/`, all parts)

- An integration test of the xtask package (`crates/xtask/tests/deploy/main.rs` with modules `tree.rs`, `ini.rs`, `units.rs` in U20a1; `expr.rs`, `catalogue.rs`, `rules.rs`, `alertmanager.rs` in U20a2; `grafana.rs` in U20a3). It runs under `cargo test --workspace` (so in CI), needs no network, no temp directory and no clock, and touches neither `main.rs` nor `registry.rs` (owner-reviewed paths). The operator runs it before an install with `cargo test --locked -p xtask --test deploy`.
- `Tree`: every file under `deploy/` plus `contracts/lake/xrepo/metrics.json` and the anchor source files, read once from `CARGO_MANIFEST_DIR/../..` into a `BTreeMap<String, String>`. `check(&Tree) -> Vec<Finding>` is pure; `Finding { file, rule, text }` prints as `deploy: <file>: <rule>: <text>`. The real-tree test asserts an empty list; every planted-defect test clones the real tree, changes one file in memory, and asserts that the expected rule appears (and that no unrelated rule does, to keep each test about one rule).
- `ini.rs`: systemd and Grafana INI. Sections `[Name]`; `Key=Value` (systemd) or `key = value` (Grafana); `#`/`;` comments; a trailing `\` joins the next line (systemd); repeated keys keep all values in order; a line that is neither is a parse finding.
- Parse rule P1: every file under `deploy/` parses by its extension: `.service`/`.timer`/`.slice` INI, `.json`/`.yaml` JSON, `.toml` TOML, `.ini` INI, `.md` not parsed; any other extension is a finding.

Unit rules (U20a1):

| Rule | Check |
|---|---|
| U1 | every `.service` has exactly one `MemoryMax=` with a size (`K`/`M`/`G` suffix or bytes), not `infinity` |
| U2 | every `Type=oneshot` service has a `.timer` of the same stem (`x@.service` ↔ `x@.timer`) and every timer has its service (no `Unit=`) |
| U3 | `md-collector@.service`: `Type=notify`, `Restart=always`, `RestartSec=2`, `WatchdogSec=30s`, `NotifyAccess=main`, `RestartPreventExitStatus=2`, `[Unit] StartLimitIntervalSec=0`, `Slice=trading.slice`, `OOMScoreAdjust=-900`, `MemorySwapMax=0`, `MemoryMin` = half of `MemoryMax` (bytes), `IOWeight=1000` — the protected-unit list is a constant `PROTECTED = ["md-collector@.service"]` (U20b appends oms-node and the watcher) and all but the first six apply to every protected unit |
| U4 | `trading.slice` exists with `CPUWeight=1000` and `MemoryMin` ≥ the sum of `MemoryMin` of every unit with `Slice=trading.slice` |
| U5 | the three timers exist with exactly the `OnCalendar` of D4 and `Persistent=true` |
| U6 | the `MemoryMax` of each unit named in D2–D4 equals the table value (1G, 1G, 200M, 200M, 100M, 512M, 1G, 1G, 4G, 4G) |
| U7 | the `ExecStart` (and any `ExecStartPre`) of `md-archiver@` and `crypto-lake-audit@` begins with `/usr/bin/flock /run/lock/hl-oms-daily.lock ` |
| U8 | every `-httpListenAddr=` / `--web.listen-address=` value is `127.0.0.1:<port>`; Alertmanager has `--cluster.listen-address=` with an empty value |
| U9 | every `-storageDataPath=`, `--storage.path=`, `LAKE=` and `--lake` value starts with `/mnt/nvme/` |
| U10 | VM and VL have `-retentionPeriod=1y`; VM has `-memory.allowedBytes=` |
| U11 | `vmalert-metrics` has `-datasource.url=http://127.0.0.1:8428`, `-notifier.url=`, `-remoteWrite.url=`, `-evaluationInterval=10s`; `vmalert-logs` has `-datasource.url=http://127.0.0.1:9428`, `-rule.defaultRuleType=vlogs`, `-notifier.url=`, `-evaluationInterval=30s` |
| U12 | no deploy file contains `/mnt/nvme/gitproject/quant-data-platform`; the audit's `--state` is not under `${LAKE}` or `/mnt/nvme/lake` |
| U13 | every venue in `venues.toml` is registered in both registries (D5) |
| U14 | install map: every `src` exists, `dst` values are unique and absolute, units go to `/etc/systemd/system/`, every other file to `/etc/hl-oms-obs/`; every file under `deploy/observability/` except the not-installed ones (D0) is a `src`; every `/etc/hl-oms-obs/…` path or glob in a unit's command line matches at least one `dst` |
| U15 | no unit has `User=root` written out, and no stack unit (D2) lacks `User=hlobs` |

Rule rules (U20a2), Alertmanager rules (U20a2), Grafana rules (U20a3): D7, D8, D9.

### D7. Metric catalogue, rule files and the expression scanner (U20a2)

**Catalogue** = `deploy/observability/metrics.toml` ∪ the pinned `metrics.json` `metrics` ∪ the `record` names of the rule files.

- `metrics.toml`: `[[metric]] name, labels = [...], source = "<repo path>"` for hl-oms metrics (every row of the U6 table above, source = the file named there), and `[[stack]] name, labels, component, version` for the stack's own metrics confirmed at the spec stage (`up{job,instance}`, `process_resident_memory_bytes{job,instance}`, `vm_free_disk_space_bytes{job,instance,path}`, `vl_free_disk_space_bytes{job,instance,path}`, `vmalert_iteration_missed_total{job,instance,group,file}`, `vmalert_execution_errors_total{job,instance}`, `vmalert_alerts_send_errors_total{job,instance,addr}`, `alertmanager_notifications_total{job,instance,integration}`, `alertmanager_notifications_failed_total{job,instance,integration,reason}`, `ALERTS{alertname,alertstate,severity,venue,alertgroup}`; versions VM/vmalert v1.157.0, VL v1.54.0, Alertmanager 0.35.1). The label lists of stack rows are the ones the files use; the implementer re-reads them from a throwaway `/metrics` (D11) and records what it saw in the PR.
- Rule R2 (anchor): the file named by each `[[metric]] source` contains the literal `"<name>"`; a `[[stack]]` row has a non-empty component and version.
- Rule R2b: every name in `metrics.json` `reserved` is a `[[metric]]` row or a `record` name (so `lake_disk_days_left` must be produced by the recording rule); every `metrics.json` metric name is not also a `[[metric]]` row (one writer per name).
- **Scanner** (`expr.rs`), MetricsQL subset used by these files: skips string literals and numbers/durations; inside `{…}` collects label names before `=`, `!=`, `=~`, `!~`; inside `[…]` skips; after `by`, `without`, `on`, `ignoring`, `group_left`, `group_right` collects the parenthesised label list; an identifier followed by `(` is a function and must be in the allowlist (`time`, `vector`, `absent`, `absent_over_time`, `last_over_time`, `max_over_time`, `min_over_time`, `sum_over_time`, `count_over_time`, `increase`, `rate`, `deriv`, `clamp_min`, `abs`, `round`, `max`, `min`, `sum`, `count`, `avg`, `topk`, `label_values` (Grafana templating only)); keywords `and`, `or`, `unless`, `bool`, `offset` are skipped; `$name`, `${name}` and `$__…` (Grafana variables) are skipped; any other identifier is a metric name. Rule R1: every metric name is in the catalogue. Rule R3: every collected label name is a label of some catalogue entry or `job`/`instance`. Rule R4: every function is in the allowlist. The scanner is not a parser: syntax is checked by `vmalert -dryRun` (D11).
- Rule files (`vmalert/metrics/*.rules.json`): `groups[]` with `name`, `interval` (optional), `rules[]` of `{alert|record, expr, for?, labels?, annotations?}`. Rule R5: every alert has `labels.severity` ∈ {`P1`, `P2`, `P3`} and a non-empty `annotations.summary`, except the one alert named `Watchdog`, which has `expr` exactly `vector(1)` and `severity` = `watchdog`; exactly one `Watchdog` exists. Rule R8: groups under `vmalert/logs/` have `"type": "vlogs"`; groups under `vmalert/metrics/` have no `type`. LogsQL expressions are not scanned (no metric names; `-dryRun` checks them).
- Rule R6 (liveness, per venue V of `venues.toml`): an alert `MdCollectorDown` with `labels.severity = "P1"`, `labels.venue = V` and `expr` equal to `time() - max by (venue) (md_collector_heartbeat_ts{venue="V"}) > 60 or absent_over_time(md_collector_heartbeat_ts{venue="V"}[2m])` (whitespace-normalised); no `for`. No `MdCollectorDown` names a venue outside the list.
- Rule R7 (design rules present, whitespace-normalised exact expressions):

| Group (file) | Alert / record | Expr | for | Severity |
|---|---|---|---|---|
| `lake-recording` (lake, interval 1m 판단(모델 아님)) | record `lake_disk_days_left` | `lake_disk_free_bytes / clamp_min(-deriv(lake_disk_free_bytes[24h]) * 86400, 1)` | — | — |
| `lake` (lake) | `LakeDiskDaysLeftLow` | `lake_disk_days_left < 7 and on(venue) count_over_time(lake_disk_free_bytes[24h]) > 4320` | 10m | P2 |
| | `LakeDiskDaysLeftCritical` | `lake_disk_days_left < 2 and on(venue) count_over_time(lake_disk_free_bytes[24h]) > 4320` | 10m | P1 |
| | `LakeL0WriteError` | `max by (venue) (lake_l0_write_errors_total) > 0 or max by (venue) (increase(lake_l0_write_errors_total[10m])) > 0` | — | P1 |
| | `LakeSealLag` | `max by (venue) (lake_seal_lag_hours) > 2` | — | P2 |
| `watchdog` (system) | `Watchdog` | `vector(1)` | — | watchdog |

  `count_over_time(…) > 4320` = at least 12 h of 10 s samples before a days-left alert (warm-up, so the archiver's first burst on day one does not page; 판단(모델 아님)); during warm-up a filling disk is still reported by `LakeL0WriteError` and the collector's `[MD P1]` disk floor. `for: 10m` on days left: 판단(모델 아님). Days left uses the stored series, so a collector restart does not reset it (R4 SHOULD 2). The journal days-left rule is U20b's.

- Other rules (thresholds 판단(모델 아님) unless cited; each with a Korean `annotations.summary` that says what to do):

| File / group | Alert | Expr (sketch) | for | Sev |
|---|---|---|---|---|
| market-data / `market-data` (10 s) | `MdConnectionSilent` | `sum by (venue, market, channel, conn) (sum_over_time(md_msgs[2m])) == 0` | 1m | P2 |
| | `MdTopicsStale` | `count by (venue, market, channel) (md_last_msg_age_s{channel=~"orderbook.*"} > 60) / count by (venue, market, channel) (md_last_msg_age_s{channel=~"orderbook.*"}) > 0.1` | 5m | P2 |
| | `MdRecvLagHigh` | `max by (venue, market) (md_recv_lag_ms{quantile="0.99"}) > 1000` | 5m | P2 |
| | `MdReconnectsHigh` | `sum by (venue, market) (increase(md_reconnects_total[10m])) > 10` | — | P2 |
| | `MdGapsHigh` | `sum by (venue, market) (increase(md_gaps_total[10m])) > 20` | — | P2 |
| | `MdQueueHigh` | `max by (venue, conn) (md_queue_bytes) > 8388608` (half of the 16 MiB slot cap, `md-collector.md:37`) | 1m | P2 |
| | `MdObsPushDropping` | `sum by (venue, endpoint) (increase(obs_push_dropped_total[10m])) > 0` | — | P2 (design §8.3 "VM/VL 쓰기 실패") |
| | `ClockOffsetHigh` | `abs(node_clock_offset_ms) > 5` | 5m | P2 (design §8.3 "시각 오프셋 5 ms") |
| lake / `lake-audit` (1m) | `LakeAuditFailed` | `max by (venue) (increase(lake_audit_fail_total[26h])) > 0` | — | P2 (design §8.3 "감사 실패") |
| | `LakeAuditStale` (per venue V) | `time() - last_over_time(lake_audit_last_run_ts{venue="V"}[3d]) > 129600 or absent_over_time(lake_audit_last_run_ts{venue="V"}[36h])` | — | P2 |
| | `LakeNotAudited` | `max by (venue) (last_over_time(lake_audit_not_audited_files[3d])) > 0` | 1h | P3 |
| system / `system` (10 s) | `ObsComponentDown` | `up{job=~"victorialogs|vmalert-logs|alertmanager"} == 0` | 2m | P2 |
| | `VmalertIterationMissed` | `increase(vmalert_iteration_missed_total[10m]) > 0` | — | P2 |
| | `AlertmanagerTelegramFailing` | `increase(alertmanager_notifications_failed_total{integration="telegram"}[10m]) > 0` | — | P2 |
| | `ObsMemoryNearCap` | `process_resident_memory_bytes{job="victoriametrics"} > 0.9 * 1073741824` and the same for each stack job with its D2 cap | 10m | P3 |

  The days-left and liveness rules cover the lake disk; design §8.3's "디스크 80%" is superseded by 9.4 (R1 MUST 3) and has no rule. A dead VM or vmalert-metrics cannot raise its own alert: that is the Watchdog's job (D8).
- Log rules (`vmalert/logs/market-data-logs.rules.json`, group `md-logs`, `type: vlogs`, 30 s): `MdErrorLogs`: `service:="md-collector" level:=error | stats by (venue) count() as errors | filter errors:>0`, P2. 판단(모델 아님).
- Scrape config `victoriametrics/scrape.json`: `scrape_interval` 15s (판단(모델 아님)); jobs `victoriametrics` (127.0.0.1:8428), `victorialogs` (:9428), `vmalert-metrics` (:8880), `vmalert-logs` (:8881), `alertmanager` (:9093). Grafana is not scraped (its listener is the Tailscale address). Rule U8 extends to scrape targets: all `127.0.0.1`.
- `vmalert/tests/*.test.json`: one `vmalert-tool unittest` file per rule file with the cases of the spec-stage prototypes plus one per alert above (fires / does not fire). They run only where the binary exists (D11), never in CI.

### D8. Alertmanager (U20a2)

- `alertmanager/alertmanager.json`: `route.receiver = "null"` (no integrations), `group_by = ["alertname", "venue"]`; child routes in order: (1) `alertname="Watchdog"` → `deadman`, `group_wait 0s`, `group_interval 1m`, `repeat_interval 1m`; (2) `severity="P1"` → `telegram-p1`, `group_wait 10s`, `group_interval 1m`, `repeat_interval 30m`; (3) `severity="P2"` → `telegram-p2`, `group_wait 30s`, `group_interval 5m`, `repeat_interval 4h`; P3 and anything else falls to `null` (dashboard and weekly review only, design §8.3). Timings 판단(모델 아님).
- `inhibit_rules`: `alertname="MdCollectorDown"` inhibits `severity=~"P2|P3"` with `equal: ["venue"]` (design §8.3 B3 "파생 알림 억제").
- `telegram_configs`: `bot_token_file: /etc/hl-oms/alertmanager-telegram-token`, `chat_id_file: /etc/hl-oms/alertmanager-telegram-chat-id`, `parse_mode: ""` (plain text: no HTML from label values), `send_resolved: true`; P2 has `disable_notifications: true` (silent, design §8.3). `message`: `[MON P1] ` or `[MON P2] ` then alertname, venue, `발생 N건` or `해소`, then one line per alert with its summary and `StartsAt` as `MM-DD HH:MM:SS UTC`. The implementer renders the template with `amtool template render` and puts the output in the PR. Same bot as the direct alerts (design Q18: "같은 봇·머리말로 구분"); the token is a raw file because Alertmanager does not read `telegram.json`.
- `deadman` receiver: no integration (placeholder). The owner's D3 decision adds a `webhook_configs` entry with `url_file` later (owner question Q1). Until then the Watchdog layer reports nothing outside (design §8.3 B4 "D3 미정이면 이 층 없음").
- Rule R10: the route tree above (first child matches only `alertname="Watchdog"` and goes to a receiver named `deadman`; P1 and P2 receivers' `message` start with `[MON P1] ` / `[MON P2] `; P2 has `disable_notifications: true`; `group_by` holds `alertname` and `venue`; the default receiver has no integration); every receiver named by a route exists. Rule R11 (secrets): no key `bot_token`, `chat_id`, `api_url` or `http_config` anywhere in the file; `bot_token_file` and `chat_id_file` are absolute paths under `/etc/`; no string in any deploy file matches `[0-9]{6,}:[A-Za-z0-9_-]{30,}` (a Telegram token shape) or is an all-digit value of 6 or more digits next to a `chat` key. `tools/ci/secret_scan.sh` still runs.

### D9. Grafana (U20a3)

- `grafana/grafana.ini`: `[server] http_addr = TAILSCALE_ADDRESS` (a placeholder Grafana cannot bind, so it refuses to start until the operator writes this PC's Tailscale address: refuse rather than open on all interfaces, table 8-5), `http_port = 3000`; `[paths] data = /mnt/nvme/observability/grafana`, `plugins = /mnt/nvme/observability/grafana/plugins`, `provisioning = /etc/hl-oms-obs/grafana/provisioning`; `[unified_alerting] enabled = false` (table 8-5); `[alerting] enabled = false`; `[auth.anonymous] enabled = false`; `[security] admin_password = $__file{/etc/hl-oms/grafana-admin-password}`; `[analytics] reporting_enabled = false`, `check_for_updates = false`, `check_for_plugin_updates = false`; `[news] news_feed_enabled = false`; `[plugins] preinstall_disabled = true`; `[log] mode = console`. The outbound-call settings are 판단(모델 아님) (no call home).
- `provisioning/datasources/hl-oms.yaml` (JSON): `VictoriaMetrics` uid `vm`, type `prometheus`, url `http://127.0.0.1:8428`, default; `VictoriaLogs` uid `vl`, type `victoriametrics-logs-datasource`, url `http://127.0.0.1:9428`; both `editable: false`. The VL plugin is installed by the operator (`grafana cli --pluginsDir /mnt/nvme/observability/grafana/plugins plugins install victoriametrics-logs-datasource`, VL ≥ v1.54.0 per table 8-5); nothing in U20a reads VL from Grafana yet (D4 events dashboard is U20b).
- `provisioning/dashboards/hl-oms.yaml` (JSON): one file provider, path `/etc/hl-oms-obs/grafana/dashboards`, `disableDeletion: true`, `allowUiUpdates: false`.
- `dashboards/d2-market-data.json` (uid `d2-market-data`, title `D2 시세 수집: 어느 거래소·종목이 비거나 늦나`), variable `venue` = `label_values(md_collector_heartbeat_ts, venue)`, panels: messages per second by market (`sum by (market) (sum_over_time(md_msgs{venue=~"$venue"}[1m])) / 60`), bytes per second, gaps and reconnects in 10 min, receive-lag p99 by market, top 20 `md_last_msg_age_s`, stale-topic share by channel, spread and depth for the traded symbols (variable `symbol`), queue bytes per connection.
- `dashboards/d3-system.json` (uid `d3-system`, title `D3 시스템: 서비스가 살아 있고 자원이 남았나`): collector heartbeat age per venue, `up` per stack job, `process_resident_memory_bytes` per job with the D2 cap as a threshold line, `lake_disk_free_bytes`, `lake_disk_days_left`, `lake_seal_lag_hours`, `node_clock_offset_ms`, `obs_push_dropped_total` increase, `lake_audit_last_run_ts` age, `lake_audit_fail_total` increase, `lake_audit_gap_rows{kind="clock_step"}` and the other kinds, `vm_free_disk_space_bytes`, `vl_free_disk_space_bytes`, firing alerts from `ALERTS{alertstate="firing"}`. The collector's own memory against its cap is not shown (no exporter; Out of Scope).
- Rules: G1 `http_addr` is `TAILSCALE_ADDRESS` or an address in 100.64.0.0/10, never empty, `0.0.0.0`, `::` or `*`; G2 `[unified_alerting] enabled = false` and `[auth.anonymous] enabled = false`; G3 `admin_password` is a `$__file{/etc/…}` reference; G4 the two datasources exactly as above; G5 the dashboard provider path is a `dst` of the install map's dashboard files and `allowUiUpdates` is false; G6 every dashboard has a unique `uid`, every panel a `title`, every panel and target a datasource uid in {`vm`, `vl`}, every target a non-empty `expr`; G7 every `expr` and every templating `query` passes R1/R3/R4 (`label_values(metric, label)`: metric in the catalogue, label one of its labels); G8 `d2-market-data` and `d3-system` exist; G9 `[paths] data` starts with `/mnt/nvme/` (U9).

### D10. Runbook `docs/operations/observability.md` (Korean, grows per part)

U20a1: §1 무엇이 여기 있고 무엇은 설치하지 않았나; §2 저장 위치(D1, 저널 전용 파일 시스템 G1, X10); §3 단위와 메모리 상한(D2–D4 표, 8-6a 보호 설정과 그 뜻, `trading.slice`의 MemoryMin 규칙); §4 타이머와 하루 순서(봉인 → 아카이버 → verify·감사, 잠금, `--retain-l1-days`를 켤 때의 조건); §5 연구 작업은 `systemd-run --user --slice=research.slice -p MemoryHigh=8G -p CPUWeight=50 <명령>`으로(R5); §6 chrony: `/etc/chrony/chrony.conf`에 `makestep 1 3`(시작 뒤 첫 세 번의 갱신에서만 1초 넘는 차이를 한 번에 옮기고 그 뒤에는 천천히 맞춤), `clock_step` 읽는 법(세션 파일 노트 → L1 `crypto/l1/gap/venue=<v>/date=<d>/`의 `kind = 'clock_step'` 행, `detail.back_ns`; DuckDB 질의 예; `lake_audit_gap_rows{kind="clock_step"}`; 재생은 `rec_no` 순이라 호가창은 맞지만 수신 지연과 거래소 간 선행·후행 분석에서 그 구간을 뺌); §7 설치 전 확인(`cargo test -p xtask --test deploy`, 바이너리 경로를 임시 사본으로 바꾼 `systemd-analyze verify`); §8 설치 순서(사용자 `hlobs`, 디렉터리, `install -D` 줄, 비밀 파일 0640, `systemctl enable --now`는 소유자가 X1에서). U20a2: §9 알림 표(이름, 등급, 조건, 뜻, 할 일), §10 Alertmanager와 Telegram 파일, X4(Alertmanager는 IP 우회를 못 함), dead-man(D3 미정), §11 규칙 확인(`vmalert -dryRun`, `vmalert-tool unittest`, `amtool check-config`), 주간 소음 검토(설계 §8.3). U20a3: §12 Grafana(플러그인, `http_addr`, 관리자 암호 파일, D2·D3의 질문과 패널, iPhone 접속 X2).

### D11. Validation each part runs and pastes into its report (not in CI)

- U20a1: `systemd-analyze verify` on copies of every unit in the part's tmp dir with `/usr/local/bin/` and `/opt/grafana/bin/` replaced by a tmp `bin/` holding the downloaded release binaries and `/bin/true` links for ours; template units verified through an instance link (`x@bybit.service`); output = no finding except the host's unrelated warnings.
- U20a2: `vmalert -dryRun -rule=<file>` for every rule file (metrics files; logs file with `-rule.defaultRuleType=vlogs`), `vmalert-tool unittest --files deploy/observability/vmalert/tests/*.test.json`, `amtool check-config`, `amtool config routes test` for Watchdog/P1/P2/P3, `amtool template render` of both messages; a throwaway VM + VL + vmalert + Alertmanager on 127.0.0.1 with data in tmp reading the real `scrape.json`, curl of each `/metrics` for the `[[stack]]` names; every process killed before the stage ends.
- U20a3: a throwaway Grafana on 127.0.0.1 (the real `grafana.ini` with `http_addr` and the paths overridden through `GF_SERVER_HTTP_ADDR`, `GF_PATHS_*` environment variables) that loads the real provisioning and dashboards; `/api/datasources`, `/api/search` and `/api/dashboards/uid/<uid>` outputs; killed before the stage ends.
- Downloads only from github.com/VictoriaMetrics/VictoriaMetrics, github.com/VictoriaMetrics/VictoriaLogs, github.com/prometheus/alertmanager, dl.grafana.com, into the part's tmp dir, checksums compared with the release checksum files.

## Testing Decisions

All tests are in `crates/xtask/tests/deploy/`, pure functions over an in-memory tree: no network, no clock, no temp files, no loops without a bound (each loop is over the files of the tree). None is timing-sensitive.

### Done criteria → tests

| Done criterion | Test | Part |
|---|---|---|
| fails on a planted unknown metric name | `rules::a_rule_with_an_unknown_metric_fails`, `grafana::a_dashboard_with_an_unknown_metric_fails` | U20a2, U20a3 |
| fails on a name in neither the U6 list, the recording rules nor the pinned metrics.json | `catalogue::a_name_in_no_part_of_the_catalogue_fails` (plant `lake_audit_runs_total`, which is in none), `catalogue::a_removed_recording_rule_makes_its_name_unknown`, `catalogue::a_catalogue_row_without_its_source_literal_fails`, `catalogue::a_reserved_lake_name_nobody_produces_fails` | U20a2 |
| fails on an md-collector unit without Restart=always or WatchdogSec | `units::a_collector_unit_without_restart_always_fails`, `units::a_collector_unit_without_watchdog_sec_fails` | U20a1 |
| fails on a unit without MemoryMax | `units::a_service_without_memory_max_fails` (a plain and a template unit), `units::memory_max_follows_the_design_table` | U20a1 |
| fails on a oneshot service without a timer | `units::a_oneshot_without_a_timer_fails`, `units::a_timer_without_its_service_fails` | U20a1 |
| fails on an md-collector unit without OOMScoreAdjust or without MemorySwapMax=0 | `units::a_collector_unit_without_oom_score_adjust_fails`, `units::a_collector_unit_without_memory_swap_max_zero_fails` (missing, and `=1G`) | U20a1 |
| the collector liveness rule exists for every configured venue | `rules::every_configured_venue_has_a_collector_liveness_rule` (adds a venue to `venues.toml` and to both registry copies in memory → finding; drops the `absent_over_time` branch → finding; a rule for an unlisted venue → finding) | U20a2 |
| seal, archiver and audit timers exist with the documented schedule | `units::the_seal_archiver_and_audit_timers_keep_their_schedule` (each timer removed, `OnCalendar` changed, `Persistent` removed) | U20a1 |
| all files parse | `tree::every_file_parses_by_its_kind` (a broken JSON, a bad INI line, an unknown extension), and `the_deploy_tree_passes` | U20a1 |
| no secret or real chat id committed | `alertmanager::an_inline_token_or_chat_id_fails` (`bot_token`, `chat_id`, a token-shaped string in any deploy file), `alertmanager::secret_files_are_absolute_paths_under_etc`, plus `tools/ci/secret_scan.sh` in the gate | U20a2 |
| full local gate passes | the plan entry's `checks` line at each part's HEAD | each |
| nothing installed or enabled on the PC | report: no `systemctl`/`install`/`sudo` was run; `systemctl list-unit-files 'victoria*' 'vmalert*' 'alertmanager*' 'grafana*' 'md-*' 'crypto-lake-audit*' 'trading.slice'` (system and `--user`) shows nothing new, before and after | each |

Further tests: `units::a_collector_unit_with_a_start_limit_fails`, `units::a_collector_unit_with_wrong_memory_min_or_io_weight_fails`, `units::the_trading_slice_reserves_its_units_memory`, `units::a_daily_job_without_the_lock_fails`, `units::a_listener_off_loopback_fails`, `units::a_store_off_the_lake_disk_fails`, `units::the_audit_never_points_at_the_kr_checkouts`, `units::the_install_map_covers_every_file_and_path`, `units::vmalert_units_carry_their_sources_and_intervals`, `units::an_unregistered_venue_fails`, `ini::the_ini_reader_keeps_repeats_and_joins_lines` (U20a1); `expr::the_scanner_finds_names_labels_and_functions` (table of expressions → names, labels, functions; `$var`, strings, durations, `by`/`on` lists), `rules::an_unknown_label_or_function_fails`, `rules::an_alert_without_a_severity_or_summary_fails`, `rules::the_design_lake_and_disk_rules_are_present`, `rules::exactly_one_watchdog_exists`, `rules::log_rules_are_vlogs_and_metric_rules_are_not`, `alertmanager::the_routes_send_watchdog_p1_p2_where_they_belong` (U20a2); `grafana::grafana_listens_on_the_tailscale_placeholder_only`, `grafana::grafana_alerting_and_anonymous_access_are_off`, `grafana::the_datasources_are_vm_and_vl_on_loopback`, `grafana::a_dashboard_panel_with_an_unknown_datasource_fails`, `grafana::the_market_data_and_system_dashboards_exist` (U20a3). Every part also keeps `the_deploy_tree_passes` (in `main.rs`).

### Deliberate defects (xtask mutants; ids = the next free M number at implementation time, M324 at 0806fcc)

U20a1 first adds `"deploy"` and `"contracts"` to the mutant run's copy list (`mutants.rs:2975`) and clears `work/deploy` and `work/contracts` like `work/crates` (`:2972`), so a mutant can change a deploy file and the tree test reads the copy.

- U20a1: (1) remove `Restart=always` from `md-collector@.service` → `the_deploy_tree_passes`; (2) the check skips the `WatchdogSec` line of rule U3 → `a_collector_unit_without_watchdog_sec_fails`; (3) rule U2 checks only timers → services → `a_oneshot_without_a_timer_fails`; (4) rule U1 skips template units → `a_service_without_memory_max_fails`; (5) rule U3 accepts any `OOMScoreAdjust` value → `a_collector_unit_without_oom_score_adjust_fails`; (6) rule U3 ignores `MemorySwapMax` → `a_collector_unit_without_memory_swap_max_zero_fails`; (7) rule U4 compares the slice's `MemoryMin` with the largest child, not the sum → `the_trading_slice_reserves_its_units_memory`; (8) change the seal timer to `:02` → `the_deploy_tree_passes`.
- U20a2: (9) the scanner skips identifiers after `or` → `a_rule_with_an_unknown_metric_fails`; (10) the catalogue accepts any name with the `lake_` prefix → `a_name_in_no_part_of_the_catalogue_fails`; (11) rule R5 accepts an alert without `severity` → `an_alert_without_a_severity_or_summary_fails`; (12) rule R6 checks only the first venue → `every_configured_venue_has_a_collector_liveness_rule`; (13) rule R11 accepts an inline `chat_id` → `an_inline_token_or_chat_id_fails`; (14) delete the `absent_over_time` branch from the market-data rule file → `the_deploy_tree_passes`; (15) rule R2 skips the source-literal anchor → `a_catalogue_row_without_its_source_literal_fails`.
- U20a3: (16) rule G1 accepts `0.0.0.0` → `grafana_listens_on_the_tailscale_placeholder_only`; (17) the dashboard check does not scan target expressions → `a_dashboard_with_an_unknown_metric_fails`; (18) rule G2 does not read `[unified_alerting]` → `grafana_alerting_and_anonymous_access_are_off`.

Mutant `test` arguments: `["-p", "xtask", "--test", "deploy", "<module>::<test>"]`; `guards`: `ADR-035`.

## Documents and registry per part

- **ADR-035** (next free: `docs/adr/` and `registry/decisions.toml` end at ADR-034), `docs/adr/ADR-035-observability-deployment-files.md`, in U20a1, covering D0 (layout, JSON syntax, install map), D3 (protections and the slice's `MemoryMin`, restart back-off, no start limit), D4 (templates instead of plain names, daily-job lock, verify before audit, schedules), D6 (check as an xtask integration test, not a subcommand or CI step, because `main.rs`, `registry.rs` and `.github/` are owner-reviewed), D7 (catalogue anchored to source literals and the pinned `metrics.json`), D8 (files for secrets, placeholder dead-man), D9 (placeholder address that refuses). Alternatives: plain-name units with one `ExecStart` per venue; a YAML crate; a YAML-subset parser; a `deploy-check` subcommand; a CI step; Grafana alerting (Q18 chose B); scraping instead of push. U20a2 and U20a3 amend it only if they change a decision. `registry/decisions.toml` gets the ADR-035 entry (`problem`, `alternatives`, `chosen`, `gives_up`, `revisit`, Korean like the others) in U20a1.
- **contracts.toml**: no new journal event, no change.
- **claims.toml**: no new claim. A claim needs a verify suite, and suites live in `crates/xtask/src/registry.rs` (owner-reviewed); the check runs in CI through `cargo test --workspace` anyway. Not an owner question: an owner who wants one adds the suite in an owner-reviewed change.
- **Runbook rows**: `docs/operations/observability.md` (D10, new); `docs/operations/runbook.md:15` row "systemd 단위" points to it (U20a1); `docs/operations/md-collector.md:5` and `docs/operations/md-archiver.md:5` name the new path, and their unit facts change (RestartSec 2, protections, seal at :05, archiver MemoryMax 4 GB and the lock) (U20a1).
- **GLOSSARY.md** (U20a1): Trading slice, Daily-job lock, Install map; (U20a2): Metric catalogue, Collector liveness rule, Days left, Watchdog alert, Dead-man receiver; each with `_Avoid_` lines where a near word exists (e.g. "alert rule" vs "direct alert").
- **README.md**: the documents table gets a row for `docs/operations/observability.md` (U20a1); the structure table gets a `deploy/` row and the xtask line count (each part updates it).
- **docs/generated**: regenerate with `cargo run --locked -q -p xtask -- docs` in every part (ADR-035 appears in the decision list from U20a1).

## Out of Scope

- OMS, projector, watcher and `oms-export` units, the trading (D1) and event-timeline (D4) dashboards, `oms_*`/`watcher_*` rules and the journal disk rule: U20b (depends on U14, U15, U18a, U18b, U19b, U20a).
- Binance, OKX and Hyperliquid collectors, their memory caps (0.5 GB each) and their liveness rules: U10/U11, deferred by the owner. `venues.toml` lists Bybit only.
- Installing, enabling or starting anything, creating `hlobs`, the journal image, `/etc` files, the Grafana plugin, Tailscale access: operations items X1, X2, X10 (owner).
- The outside dead-man service (D3) and a second P1 channel: owner decision; the placeholder receiver is in U20a2.
- Telegram reachability from Alertmanager (it cannot use the direct alerts' IP route): X4.
- A memory or liveness exporter for our own services (md-collector RSS against its cap, systemd unit state, failed oneshot units): no exporter is in scope and none may be installed; the seal is covered by `lake_seal_lag_hours`, the audit by `LakeAuditStale`, the archiver only by `systemctl --failed` and the audit's findings. A follow-up code unit could export `lake_l1_build_lag_days` from md-archiver (not planned).
- A CI step for `vmalert -dryRun`/`amtool`: it would need the binaries at CI time (network) and a `.github/` change (owner).
- Pilot measurements P1–P6 (detection delay, MemoryMax fits, I/O under research load).

## Owner questions (the parts use the default; the runner does not wait)

1. **Q1 Outside dead-man (design D3).** Default: the Watchdog alert goes to a `deadman` receiver with no integration; nothing leaves the PC, so a dead stack is not reported from outside.
2. **Q2 Service identity.** Default: stack units run as a new system user `hlobs`; `md-collector@`, `md-lake-seal@`, `md-archiver@` and `crypto-lake-audit@` keep running as root as merged (a change needs the lake's owner moved).
3. **Q3 Grafana address.** Default: `http_addr = TAILSCALE_ADDRESS`, a placeholder Grafana cannot bind, so it does not start until the operator writes this PC's Tailscale address (never all interfaces).
4. **Q4 qdp checkout for the audit.** Default: `/opt/qdp-crypto`, a dedicated checkout of qdp main with its own `.venv` (`uv sync --frozen` once); never the KR checkouts under `/mnt/nvme/gitproject/quant-data-platform*`.
5. **Q5 P2/P3 thresholds** of D7's second table (2 min silent connection, 10% stale topics, 1,000 ms p99 lag, 10 reconnects or 20 gaps in 10 min, 8 MiB queue, 36 h audit age). Default: as listed, reviewed in the weekly noise review.

Decided here and reported, not asked: per-venue templates with the daily-job lock instead of the plan's plain unit names (D4); no verify claim for the check (Documents and registry).
