# 관측 스택과 시세 단위 배치 (deploy/observability)

주인: 준영
검토일: 2026-11-25

이 PC(Linux, 단일 호스트)에서 VictoriaMetrics·VictoriaLogs·vmalert 둘·Alertmanager·Grafana와 시세 수집기·봉인·아카이버·레이크 감사를 systemd로 돌리는 방법이다. 결정과 대안은 [ADR-035](../adr/ADR-035-observability-deployment-files.md)에 있다. 판단(모델 아님)은 측정이나 모델 없이 정한 숫자다.

## 1. 무엇이 여기 있고 무엇은 설치하지 않았나

`deploy/observability/`에는 **파일만** 있다. 이 저장소는 아무것도 설치하거나 켜거나 시작하지 않는다. 설치는 소유자가 §8의 순서로 한다.

| 파일 | 내용 |
|---|---|
| `systemd/victoriametrics.service`, `victorialogs.service` | 지표·로그 저장소. 보관 1년, `127.0.0.1` |
| `systemd/vmalert-metrics.service`, `vmalert-logs.service` | 규칙 평가기 둘(지표 10초, 로그 30초) |
| `systemd/alertmanager.service`, `grafana.service` | 알림 전달, 화면 |
| `systemd/trading.slice` | 거래 서비스의 CPU 가중치와 메모리 예약 |
| `systemd/md-collector@.service` | 거래소마다 수집기 하나(인스턴스 = 거래소) |
| `systemd/md-lake-seal@.service`, `.timer` | 매시 5분 봉인 |
| `systemd/md-archiver@.service`, `.timer` | 매일 01:10 L1 만들기 |
| `systemd/crypto-lake-audit@.service`, `.timer` | 매일 02:30 레이크 감사: 최근 9일 중 바뀐 날짜의 verify, 그다음 qdp 감사기(§4.1) |
| `bin/crypto-lake-audit.sh` | 감사 단위가 돌리는 스크립트(POSIX `sh`, GNU `find`·`date` 필요) |
| `install.toml` | 저장소 파일 → 설치 경로와 권한(§8) |
| `venues.toml` | 이 PC가 수집하는 거래소(`bybit`). 거래소마다 생존 규칙이 있어야 한다(§9) |
| `vmalert/metrics/market-data.rules.json`, `lake.rules.json`, `system.rules.json` | 지표 알림 규칙(vmalert-metrics). 알림마다 등급과 할 일(§9) |
| `vmalert/logs/market-data-logs.rules.json` | 로그 알림 규칙(vmalert-logs, `type: vlogs`) |
| `vmalert/tests/*.test.json` | 지표 규칙 파일마다 하나인 `vmalert-tool unittest` 파일(설치하지 않음, §11) |
| `victoriametrics/scrape.json` | VictoriaMetrics가 15초마다 스택 다섯(VM, VL, vmalert 둘, Alertmanager)의 `/metrics`를 긁는 설정 |
| `metrics.toml` | 지표 목록: 규칙이 쓰는 지표 이름과 라벨, 그 이름을 내보내는 소스 파일(설치하지 않음, §7) |
| `alertmanager/alertmanager.json` | 알림 전달: 등급별 경로, 묶음, 억제 규칙, Telegram 수신자와 문자 모양(§10) |
| `grafana/grafana.ini` | Grafana 설정: 자리 표시 주소, 저장 경로, 끈 기능(§12) |
| `grafana/provisioning/datasources/hl-oms.yaml`, `grafana/provisioning/dashboards/hl-oms.yaml` | Grafana의 데이터 소스 둘과 대시보드 제공자(§12) |
| `grafana/dashboards/d2-market-data.json`, `d3-system.json` | 대시보드 D2(시세 수집), D3(시스템)(§12) |

모든 JSON 파일은 도구가 읽는 YAML이기도 하다(JSON은 YAML이다). 주석을 쓸 수 없으므로 알림마다 `annotations.summary`에 뜻과 할 일을 적었다.

Grafana는 `grafana.ini`의 자리 표시 주소 `192.0.2.1`을 이 PC의 Tailscale 주소로 바꾸기 전에는 시작하지 않는다(§12.1).

배치 검사(`cargo test --locked -p xtask --test deploy`)가 이 파일들을 읽고 규칙을 확인한다(§7). CI의 `cargo test --workspace`에도 들어간다.

## 2. 저장 위치

| 경로 | 무엇 | 장치 |
|---|---|---|
| `/mnt/nvme/lake` | 레이크(L0·L1·감사 결과) | 970 EVO Plus |
| `/mnt/nvme/observability/victoria-metrics` | 지표 | 같음 |
| `/mnt/nvme/observability/victoria-logs` | 로그 | 같음 |
| `/mnt/nvme/observability/alertmanager` | 침묵·알림 상태 | 같음 |
| `/mnt/nvme/observability/grafana` | Grafana 데이터와 플러그인 | 같음 |
| `/mnt/nvme/hl-oms-journal` | OMS 저널(이 문서의 단위는 쓰지 않음) | 같은 장치의 전용 파일 시스템 |

- 저장소마다 `/mnt/nvme` 아래 자기 디렉터리를 쓴다. 배치 검사가 `-storageDataPath`, `--storage.path`, `LAKE=`, `--lake` 값이 `/mnt/nvme/` 아래가 아니면 실패한다(`${LAKE}`는 단위의 `Environment=LAKE=` 값으로 바꿔 본다). 플래그는 도구가 읽는 모든 철자로 읽는다: 대시 하나나 둘, `이름=값`이나 `이름 값`, 따옴표. 저장소마다(VM, VL, Alertmanager, 봉인, 아카이버) 이 플래그가 꼭 한 번 있어야 한다: 없으면 도구가 상대 경로의 기본값에 쓴다.
- OMS 저널은 고정 크기(약 64 GB, 설계값) ext4 파일 시스템에 둔다. 예: 이미지 파일 `/mnt/nvme/hl-oms-journal.img`를 loop 장치로 `/mnt/nvme/hl-oms-journal`에 붙인다(운영 항목 X10). 레이크나 지표가 디스크를 채워도 저널이 쓸 자리가 남는다. 공간만 나누고 지연은 나누지 않는다(같은 장치).
- 저널을 `/`(980 500GB)에 두지 않는다. 그 장치는 지연 수용 기준을 넘었다(ADR-012: p99 27.2 ms, 기준 20 ms).

## 3. 단위와 메모리 상한

| 단위 | 사용자 | 듣는 곳 | MemoryMax | 다시 시작 |
|---|---|---|---|---|
| `victoriametrics` | `hlobs` | 127.0.0.1:8428 | 1G | 실패 시 5초 뒤 |
| `victorialogs` | `hlobs` | 127.0.0.1:9428 | 1G | 같음 |
| `vmalert-metrics` | `hlobs` | 127.0.0.1:8880 | 200M | 같음 |
| `vmalert-logs` | `hlobs` | 127.0.0.1:8881 | 200M | 같음 |
| `alertmanager` | `hlobs` | 127.0.0.1:9093, 클러스터 포트 없음 | 100M | 같음 |
| `grafana` | `hlobs` | `grafana.ini`의 주소(Tailscale) | 512M | 같음 |
| `md-collector@<거래소>` | root | — | 1G(Bybit) | 항상, 2초 뒤 |
| `md-lake-seal@<거래소>` | root | — | 1G | 타이머 |
| `md-archiver@<거래소>` | root | — | 4G | 타이머 |
| `crypto-lake-audit@<거래소>` | root | — | 4G | 타이머 |

- 상한은 설계 §8.5(TN 표 8-6)의 값이다. 배치 검사가 이 표와 다르면 실패한다(`MemoryMax`는 바이트로 비교, `1024M` = `1G`).
- VictoriaMetrics `-retentionPeriod=1y`, `-memory.allowedBytes=600MiB`(설계 §8.5 "약 0.6 GB"), VictoriaLogs도 1년과 600MiB(600MiB는 판단(모델 아님)). 기본 보관은 VM 1개월, VL 7일이다.
- vmalert는 `-rule.evalDelay` 기본값(30초)을 쓴다. 규칙은 30초 전의 저장소를 본다. vmalert 둘은 VM·VL·Alertmanager 뒤에 시작하되(`After=`) `Requires=`는 없다: VM을 다시 시작해도 vmalert가 멈추지 않는다.
- 스택 단위는 `NoNewPrivileges=yes`, `ProtectSystem=strict`, `ProtectHome=yes`, `PrivateTmp=yes`이고 자기 데이터 디렉터리만 쓴다(`ReadWritePaths=`, vmalert는 없음). 배치 검사가 이 설정과 `Restart=on-failure`, `RestartSec=5`, 그리고 `Requires=`·`Requisite=`·`BindsTo=`·`PartOf=`가 없음을 고정한다.
- 시세 단위(수집기, 봉인, 아카이버, 감사)는 지금처럼 root로 돈다(레이크의 소유자를 옮기지 않는 한. 소유자가 정할 것: ADR-035 §8).

### 3.1 수집기의 보호 설정 (TN 표 8-6a)

| 설정 | 뜻 |
|---|---|
| `Slice=trading.slice` | 거래 서비스 묶음에 들어간다 |
| `OOMScoreAdjust=-900` | 메모리가 모자라 커널이 프로세스를 죽일 때 거의 마지막에 고른다 |
| `MemorySwapMax=0` | 스왑으로 밀려나지 않는다(밀리면 받기가 늦어진다) |
| `MemoryMin=512M` | `MemoryMax`의 절반. 이만큼은 다른 프로세스 때문에 회수되지 않는다 |
| `IOWeight=1000` | 디스크 입출력 가중치. 이 PC의 스케줄러에서는 효과가 없을 것으로 본다(표 8-6a) |
| `Restart=always`, `RestartSec=2` | 죽으면(메모리 상한 초과 포함) 약 2초 뒤 다시 뜬다 |
| `StartLimitIntervalSec=0` | 시작 횟수 제한 없음. 기본(10초에 5번)이면 `RestartSec=2`에서 수집기가 영영 멈출 수 있다(판단(모델 아님)) |
| `WatchdogSec=30s`, `Type=notify` | 쓰기 루프의 박동이 30초 없으면 systemd가 죽이고 다시 띄운다 |
| `RestartPreventExitStatus=2` | 설정 오류(종료 코드 2)는 다시 시작해도 낫지 않으니 다시 띄우지 않는다 |

- **재시작 지연을 늘리지 않았다**(`RestartSteps=`, `RestartMaxDelaySec=` 없음, 배치 검사가 거부). systemd 문서는 재시작 횟수가 언제 0으로 돌아가는지 말하지 않는다. `Restart=always`에서 돌아가지 않는다면 수명 중 다섯 번째 충돌 뒤의 모든 재시작이 최대 지연을 기다리고 그만큼 원본 데이터를 잃는다(확인 안 함). 대가: 시작하자마자 죽는 수집기는 사람이 멈출 때까지 약 2초마다 거래소에 다시 접속한다(소유자가 정할 것: ADR-035 §8).
- 다른 거래소를 더하면 그 거래소의 `MemoryMax`(설계 0.5 GB)와 `MemoryMin`(그 절반)을 drop-in(`/etc/systemd/system/md-collector@<거래소>.service.d/*.conf`)에 쓰고, `trading.slice`의 `MemoryMin`을 그 거래소의 `MemoryMin`만큼 손으로 올린다. 배치 검사는 템플릿 파일 하나의 `MemoryMin`만 세고 인스턴스 수와 drop-in은 읽지 못한다.
- `trading.slice`: `CPUWeight=1000`, `MemoryMin=512M`. cgroup v2에서 자식의 실제 `memory.min`은 조상의 값으로 제한된다(커널 문서 `Documentation/admin-guide/cgroup-v2.rst`, memory.min). 그래서 슬라이스는 그 안 모든 단위의 `MemoryMin` 합 이상을 예약해야 한다. 단위를 슬라이스에 더하면 슬라이스의 `MemoryMin`도 올린다. 배치 검사가 합보다 작으면 실패한다.
- 수집기 설정의 `telemetry.slow_ms`(기본 10 000)는 20 000 이하로 둔다. 생존 알림 규칙(심박이 60초 없음)과 남은 일수 알림의 준비 시간(12시간)이 이 주기로 오는 값에 기댄다. 배치 검사는 `/etc/md-collector/<거래소>.toml`을 읽지 못하므로 이것은 사람이 지킨다.

## 4. 타이머와 하루 순서

| 타이머 | 시각(UTC) | 서비스 | 상한 |
|---|---|---|---|
| `md-lake-seal@.timer` | 매시 5분 | `md-lake seal`: 닫힌 시간 봉인, 날짜가 끝났으면 일별 봉인 | 1G, 시작 6시간 |
| `md-archiver@.timer` | 매일 01:10 | `md-archiver run`: 일별 봉인된 날짜 중 L1이 없는 것을 오래된 순으로 | 4G, 시작 6시간 |
| `crypto-lake-audit@.timer` | 매일 02:30 | `crypto-lake-audit.sh`: 최근 9일 중 바뀐 날짜 verify → qdp 감사기 | 4G, 시작 6시간 |

- 하루 순서: 00:05 23시 봉인과 일별 봉인 → 01:10 아카이버(여유 65분, 판단(모델 아님)) → 02:30 최근 9일 중 바뀐 날짜의 `md-lake verify` → 감사(02:30은 판단(모델 아님)).
- 모든 타이머는 `Persistent=true`다. PC가 꺼져 놓친 실행은 다음 부팅 때 한 번 돈다. 그러면 세 타이머가 부팅 때 함께 울린다. 감사 단위의 `After=md-archiver@%i.service`가 두 시작 작업이 함께 대기할 때 아카이버를 먼저 세운다. 그래도 감사가 먼저 돌면 L1이 없는 날짜는 `fail`이 아니라 `not_audited`이고 다음 날 감사된다. 배치 검사가 세 타이머의 `OnCalendar`와 `Persistent`를 고정한다.
- **일일 작업 잠금.** 아카이버와 감사의 명령은 `/usr/bin/flock /run/lock/hl-oms-daily.lock …`로 시작한다. 일일 작업(모든 거래소의 아카이버와 감사, 뒤에 더할 일일 작업)은 한 번에 하나만 돌아 4G 상한이 더해지지 않는다. `flock`은 기다리고, `TimeoutStartSec=6h`(판단(모델 아님))가 기다림을 포함한 한 번의 실행을 묶는다. 잠금을 쓰는 단위는 모두 root로 돈다: `/run/lock`은 누구나 쓰는 sticky 디렉터리라 `fs.protected_regular`이 켜져 있으면 다른 사용자가 root가 만든 잠금 파일을 `O_CREAT`로 열지 못한다.
- **봉인의 6시간 제한**은 걸린 봉인을 끝낼 뿐이다. systemd는 돌고 있는 oneshot을 두 번 시작하지 않고, 봉인에는 자기 잠금이 있다(종료 코드 3). 짧은 제한이면 부하에서 오래 걸리는 시간이 매시 죽고 다시 시작해 영영 봉인되지 않는다. 제한에 걸려 죽은 봉인은 다음 시간에 봉인 기록부터 이어 한다.
- **아카이버 4G의 여유.** 실측 2.57 GB / 320만 행(행당 약 800 B, [md-archiver.md](md-archiver.md) §5)이면 4 GB에 토픽 하루 약 500만 행, 실측 날짜의 1.6배다. 더 바쁜 날이 넘는지는 재지 않았다(판단(모델 아님). 소유자가 정할 것: ADR-035 §8). 넘으면 그 날짜에서 상한에 걸려 죽고, 아카이버는 가장 오래된 날짜부터 만들므로 뒤 날짜도 모두 막힌다. 손실은 없다(L0는 남고 `--retain-l1-days`는 꺼져 있음). 사람이 잠금을 잡고 더 높은 상한으로 한 번 돌린다: `systemd-run --wait -p MemoryMax=8G /usr/bin/flock /run/lock/hl-oms-daily.lock /usr/local/bin/md-archiver run --venue <v> --lake /mnt/nvme/lake --threads 1`(8G는 예, 판단(모델 아님)).
- 배치 검사가 봉인·아카이버·감사의 `TimeoutStartSec=6h`(그리고 `TimeoutSec=` 없음), 아카이버의 `--threads`가 1이고 `RETAIN_ARGS=`가 비어 있으며 `--retain-l1-days`가 명령에 없음, 어느 단위에도 `EnvironmentFile=`이 없음(검사가 그 파일의 값을 읽지 못함)을 고정한다. `oneshot` 서비스에는 기본 시작 제한 시간이 없어, 제한이 없으면 걸린 아카이버가 일일 작업 잠금을 끝없이 쥔다.
- **매일 확인**: `systemctl --failed 'md-*' 'crypto-lake-audit*'`. 실패한 아카이버 실행, 감사의 날짜 하나 쓰기 오류(종료 코드 3), verify 실패(5)는 어떤 알림도 내지 않는다(§4.1).
- `--retain-l1-days`(L1 지우기)는 꺼 둔다(`RETAIN_ARGS=` 비움). 켜기 전 두 조건: (1) 바로 앞에 `--full` 감사를 돌린다(qdp `docs/crypto/auditor.md`, "Run `--full` right before `md-archiver --retain-l1-days`"), (2) md-archiver가 `lake_l1_build_lag_days`를 내보내고 그 알림이 있다. 그 지표가 없으면 죽은 아카이버 실행이 뒤 날짜를 모두 막아도 `systemctl --failed`만 보여 준다.

### 4.1 레이크 감사 (`crypto-lake-audit@<거래소>`)

감사 단위는 잠금 아래서 `crypto-lake-audit.sh <거래소>` 하나를 돌린다. 스크립트는 이 순서로 한다:

1. `venue`(인자), `LAKE`, `STATE_DIRECTORY`, `QDP_CHECKOUT`, `UV`, `MD_LAKE` 중 빈 것이 있으면 stderr에 한 줄 쓰고 종료 코드 2로 멈춘다. 아무 파일도 쓰지 않는다.
2. 어제부터 9일 전까지 UTC 날짜마다(9는 판단(모델 아님)): 그 날짜에 봉인된 `*.qraw.zst` 파일이 `crypto/l0/venue=<v>/` 아래 하나라도 있고, 다음 중 하나이면 `md-lake verify`를 돌린다.
   - verify 결과 `crypto/l0/_verify/venue=<v>/date=<d>.json`이 없다.
   - 확인 도장 `/var/lib/qdp-crypto-audit/verify-<v>-<d>.stamp`가 없다.
   - 그 날짜의 봉인 파일 중 도장 뒤에 상태가 바뀐(ctime) 것이 있다(`find -cnewer`).
   - 도장의 시각이 지금(스크립트 시작 시각)보다 뒤다. 도장을 찍은 뒤 시계가 뒤로 갔다는 뜻이고, 그런 도장은 그 사이 봉인된 파일을 가리므로 없는 것으로 본다.

   봉인 파일이 없는 날짜(PC가 꺼진 날, 설치 전)는 건너뛴다. 빈 결과를 쓰지 않기 위해서다.
3. verify는 이렇게 한다: 도장 `….stamp.new`를 찍고 1초 쉰 뒤 verify를 돌린다. 성공하면 `….stamp.new`를 `….stamp`로 옮긴다(`mv`는 찍은 시각을 지킨다). 실패하면 stderr에 쓰고 옛 도장을 두고 다음 날짜로 간다.
   - 도장을 verify **전에** 찍는 이유: 일별 봉인은 일일 작업 잠금 밖에서 돈다. verify는 날짜의 파일을 한 번 나열하고 끝에 결과를 쓴다. 나열 뒤에 `.partial`에서 이름이 바뀐 파일은 결과에 없고, 그 ctime은 결과 파일보다 오래다. 결과 파일과 비교하면 그 날짜를 다시 고르지 않는다. 도장과 비교하면 그 파일이 더 새로우므로 다음 실행이 다시 verify한다.
   - ctime인 이유: 이름 바꾸기는 파일의 ctime을 바꾸고 mtime은 그대로 둔다. `-newer`(mtime)는 그런 파일을 놓친다. 1초 쉬는 이유: 커널 시각은 거칠어서 찍기와 같은 틱의 이름 바꾸기는 같은 시각이 되고, `-cnewer`는 "더 새로움"만 센다.
   - 도장과 파일의 ctime은 둘 다 벽시계 시각이다. 한 verify 실행 **도중** 시계가 뒤로 가면(운영자, 또는 재시작 뒤 chrony의 `makestep 1 3`) 그 실행 중 봉인된 파일의 ctime이 도장보다 앞설 수 있고, 그 날짜를 다시 고르지 않는다. 위의 "지금보다 뒤인 도장" 검사는 이 경우를 잡지 못한다. 02:30 UTC 무렵에 `clock_step`이 보이면 9개 날짜를 손으로 verify한다(아래 명령).
   - 도장은 거래소·날짜마다 빈 파일 하나이고 지우지 않는다(거래소마다 1년 약 365개, 판단(모델 아님)).
4. `cd /opt/qdp-crypto`(실패하면 종료 코드 2, 감사기를 돌리지 않음) 뒤 `uv run --no-sync python -m quant_data.crypto audit --lake /mnt/nvme/lake --venue <v> --state /var/lib/qdp-crypto-audit/audit-<v>.json --vm-url http://127.0.0.1:8428`. 상태 파일은 레이크 밖이다(감사기의 조건). 감사기는 근거가 있는 모든 날짜를 감사하고 날짜마다 verify 결과를 읽는다. verify 결과가 없는 날짜의 봉인 파일은 `lake_audit_not_audited_files`에 센다.

**9일인 이유.** 일별 봉인의 되돌아보기는 `last_closed`의 날부터 7일이다. N일 00:05 봉인은 N−8일을, 02:30 뒤의 봉인은 N−7일을 아직 봉인할 수 있다. 9일 창이면 N일 감사가 N−1…N−9, N+1일 감사가 N…N−8을 보므로, 보통의 봉인 실행이 닿는 날짜는 모두 어떤 다음 감사의 창에 든다.

**창보다 오래된 날짜는 손으로 verify한다.** 밀린 봉인이 한꺼번에 풀린 뒤(첫 봉인 시간부터 모든 날짜를 봉인함)나 `md-lake seal-days --from D`를 돌린 뒤, 9일보다 오래된 날짜마다 한 번:

```sh
md-lake verify --venue <v> --lake /mnt/nvme/lake --date <YYYY-MM-DD> --json
```

다음 감사가 그 결과를 읽는다. verify가 계속 실패하는 날짜(예: 읽을 수 없는 봉인 파일)도 같다: 스크립트는 그 날짜가 창 안에 있는 9일 동안만 매일 다시 verify하고 종료 코드 5를 낸다. 9일이 지나면 어떤 실행도 다시 시도하지 않고 단위는 다시 초록이 된다. 그 전에 원인을 고치고 손으로 verify한다.

**종료 코드와 어디서 보이나** (알림은 §9)

| 코드 | 뜻 | 어디서 보이나 |
|---|---|---|
| 0 | 감사 끝, verify 실패 없음 | — |
| 1 | 감사기가 `fail` 날짜를 찾음 | `LakeAuditFailed`(P2) |
| 1 | Python 자체의 예외(감사기가 잡지 않는 것)나 깨진 `.venv`(`--no-sync`는 고치지 않음). 아무것도 보내지 않음 | 36시간 뒤 `LakeAuditStale`(P2). `journalctl -u crypto-lake-audit@<v>`에 traceback이 있으면 이 경우, `audit: <날짜>` 줄이면 위의 경우 |
| 2 | 사용 거부: 스크립트의 빈 설정·없는 qdp 체크아웃, 또는 감사기의 사용·상태 거부 | 36시간 뒤 `LakeAuditStale` |
| 3 | 쓰기 오류나 버그. 지표를 보내기 전이면 | 36시간 뒤 `LakeAuditStale` |
| 3 | 날짜 하나의 쓰기 오류. 감사기는 `lake_audit_last_run_ts`를 보내고 그 날짜를 상태에서 지운다 | `systemctl --failed`만 |
| 4 | 지표 보내기 실패 | 36시간 뒤 `LakeAuditStale` |
| 5 | 감사기는 성공, verify 하나 이상이 돌지 못함(`md-lake verify`의 오류 종료, 나열·도장 쓰기 실패; 감사기는 5를 쓰지 않음). verify가 손상(예: `framing_errors`)을 찾으면 verify는 0으로 끝나고 손상은 감사기의 `fail`(코드 1)로 보인다 | `systemctl --failed`만. 다음 실행이 그 날짜를 다시 verify한다. 단 그 날짜가 9일 창 안에 있는 동안만이다(위의 "창보다 오래된 날짜") |
| 그 밖 | `uv` 자신의 오류 코드를 그대로 넘김 | 36시간 뒤 `LakeAuditStale` |

0이 아닌 코드는 모두 단위를 실패로 남긴다. 모두 실제로 볼 일이다.

- **손으로 돌리기**: `systemctl start crypto-lake-audit@<v>.service`. 스크립트를 systemd 밖에서 바로 돌리면 `STATE_DIRECTORY`가 비어 종료 코드 2로 멈춘다(도장과 상태 파일이 `/`로 가지 않게).
- **KR 파이프라인은 건드리지 않는다.** `/opt/qdp-crypto`는 qdp main의 전용 체크아웃이고 자기 `.venv`가 있다(`uv sync --frozen` 한 번). KR 파이프라인의 체크아웃(`/mnt/nvme/gitproject/` 아래)을 쓰지 않는다. 배치 검사가 그 경로를 이름한 배치 파일과 레이크 아래의 `--state`를 거부한다.
- **쓰는 곳.** 단위는 `crypto/l0`과 `crypto/l1`을 읽기만 하고 `crypto/l0/_verify`(verify)와 `crypto/audit`(감사기)에만 쓴다(`ReadOnlyPaths=`, `ReadWritePaths=`; systemd는 더 구체적인 경로를 따르므로 읽기 전용 `l0` 안의 `_verify`에 쓸 수 있다). 이 넷 중 하나라도 없으면 단위가 준비 단계에서 실패한다(§8).

## 5. 연구 작업

연구 작업(백테스트, 큰 질의)은 이렇게 돌린다:

```sh
systemd-run --user --slice=research.slice -p MemoryHigh=8G -p CPUWeight=50 <명령>
```

`MemoryHigh=8G`를 넘으면 그 작업이 먼저 회수·감속되고, `CPUWeight=50`이면 `trading.slice`(1000)와 다툴 때 CPU를 적게 받는다(설계 R5). 이 둘 없이 셸에서 바로 돌린 큰 작업은 수집기와 같은 무게로 경쟁한다.

## 6. 시계(chrony)

`/etc/chrony/chrony.conf`에 다음 한 줄을 둔다:

```text
makestep 1 3
```

chrony가 시작한 뒤 첫 세 번의 갱신에서만 1초가 넘는 차이를 한 번에 옮기고(step), 그 뒤로는 천천히 맞춘다(slew). 운영 중 시계가 뒤로 뛰면 수집기의 수신 시각이 거꾸로 가기 때문이다.

그래도 뒤로 뛰면 이렇게 남는다:

1. 수집기는 연결마다 수신 시각이 앞 프레임보다 작으면 세션 파일에 `clock_step` 노트를 쓴다(`detail.back_ns` = 거꾸로 간 나노초).
2. 아카이버는 그 노트를 L1 `crypto/l1/gap/venue=<v>/date=<d>/day.parquet`의 `kind = 'clock_step'` 행으로 옮긴다(`detail`은 JSON 문자열).

   ```sql
   -- DuckDB
   SELECT conn_id, start_recv_ns, json_extract(detail, '$.back_ns') AS back_ns
   FROM read_parquet('/mnt/nvme/lake/crypto/l1/gap/venue=bybit/date=2026-12-01/day.parquet')
   WHERE kind = 'clock_step';
   ```
3. 감사기(qdp)는 가장 새로 감사한 날짜의 종류별 gap 행 수를 `lake_audit_gap_rows{kind="clock_step"}`로 보낸다.

재생은 `rec_no` 순이라 호가창은 맞다. 수신 지연(`recv_ns` − 거래소 시각)과 거래소 사이의 선행·후행 분석에서는 그 구간을 뺀다.

## 7. 설치 전 확인

1. `cargo test --locked -p xtask --test deploy`. 실패하면 줄마다 `deploy: <파일>: <규칙>: <내용>`이 나온다. 규칙: P1(파일이 종류대로 읽힘), U1(서비스마다 `MemoryMax` 하나, 크기, `infinity` 아님), U2(oneshot 서비스 ↔ 같은 이름의 타이머, 타이머에 `Unit=` 없음), U3(수집기의 재시작·감시 설정과 보호 설정, 시작 제한의 옛 이름과 `[Service]` 쪽 키, `MemoryHigh` 없음), U4(슬라이스의 가중치와 예약), U5(세 타이머의 시각과 `Persistent`), U6(설계 상한), U7(아카이버와 감사의 일일 작업 잠금), U8(`127.0.0.1`에서만 듣고 Alertmanager 클러스터 포트 없음. 리스너 플래그는 어떤 철자로 써도 모두 셈), U9(저장소가 `/mnt/nvme/` 아래, 저장소마다 경로 플래그 한 번), U10(보관 1년, VM 메모리 한도), U11(vmalert 둘의 원본·알림·간격), U12(어느 배치 파일도 KR qdp 체크아웃을 이름하지 않음, 감사 스크립트의 `venue=$1`과 `--state "$STATE_DIRECTORY/audit-$venue.json"`, 감사 단위의 `StateDirectory=qdp-crypto-audit`. 스크립트는 CI에서 돌지 않으므로 다시 verify할 날짜와 종료 코드를 정하는 줄도 글자 그대로 고정: 9일 창, `-cnewer "$stamp"`, 지금보다 뒤인 도장 검사, 도장 → `sleep 1` → verify → `mv` 순서, 실패 시 옛 도장 유지, `exit 5`, `--vm-url http://127.0.0.1:8428`), U13(`venues.toml`의 거래소가 수집기와 아카이버 모두에 등록됨), U14(설치 지도), U15(스택은 `hlobs`, 어느 단위에도 `User=root`를 쓰지 않음), U16(`md-lake verify`나 감사 스크립트를 돌리는 단위가 읽기 전용 `l0` 아래의 `_verify`에, 감사는 `crypto/audit`에도 쓸 수 있음. 감사 스크립트를 돌리는 다른 단위도 `crypto/audit`에 쓸 수 있어야 함. 감사 단위의 두 경로 줄은 정확히 §4.1의 값이고 `LAKE=/mnt/nvme/lake` 한 번), U17(`Exec*` 줄에 `$(`, 백틱, `%i`·`%I`·`%n`·`%%`가 아닌 `%` 없음: systemd는 셸을 돌리지 않음), U18(스택 단위의 재시작·격리 설정, 쓰는 디렉터리, 다른 단위에 묶이지 않음), U19(봉인·아카이버·감사 실행의 시간 제한, 아카이버의 스레드 1개와 L1 삭제 꺼짐, `EnvironmentFile=` 없음). 규칙 파일과 지표 목록: R1(규칙의 모든 지표 이름이 목록에 있음: `metrics.toml`, 고정된 qdp `contracts/lake/xrepo/metrics.json`, 규칙 파일의 기록 규칙 이름), R2(`metrics.toml`의 `[[metric]]` 행마다 `source` 파일에 그 이름이 문자열 그대로 있음, `[[stack]]` 행은 구성 요소와 판이 있음), R2b(qdp가 hl-oms 몫으로 남긴 이름(`reserved`)마다 쓰는 쪽이 있음, 한 이름에 쓰는 쪽은 하나), R3(라벨이 목록의 어떤 지표의 라벨이거나 `job`·`instance`), R4(허용된 함수만, 검사기가 읽지 못하는 모양 없음), R5(알림마다 `severity` P1·P2·P3와 `summary`, `Watchdog`은 하나이고 `vector(1)`·`watchdog`, `for` 없음, `system.rules.json`의 그룹 `watchdog`), R6(`venues.toml`의 거래소마다 `MdCollectorDown`(P1)과 `MdLakeGaugesMissing`(P2)(그룹 `market-data`), `LakeAuditStale`(P2)(그룹 `lake-audit`)이 정확히 하나씩, 정해진 식과 그룹, `for` 없음, 목록에 없는 거래소의 것은 없음), R7(설계의 레이크·디스크 규칙: 남은 일수 기록 규칙, `LakeDiskDaysLeftLow`·`Critical`, `LakeL0WriteError`, `LakeSealLag`, `LakeAuditFailed`의 식·`for`·등급·그룹. R6·R7 규칙과 `Watchdog`이 든 그룹의 간격: `market-data` 10초, `lake-recording`·`lake-audit` 1분, `lake`·`watchdog`은 간격 없음(vmalert-metrics의 기본 10초, U11) 또는 `10s`), R8(`vmalert/logs/`의 그룹은 `type: vlogs`, `vmalert/metrics/`의 그룹은 `type` 없음), R9(`scrape.json`의 작업 다섯과 간격 15초, 작업마다 대상이 그 단위의 리스너, 규칙이 쓰는 `job` 값은 긁는 작업, `ObsMemoryNearCap`은 작업마다 `process_resident_memory_bytes{job="<작업>"} > 0.9 * <U6의 설계 상한>` 항 하나씩이고 다른 항은 없음. 항을 통째로 비교하므로 자릿수가 더 붙은 상한도 걸림), U8은 긁는 대상에도(`127.0.0.1`만). Alertmanager 설정: R10(경로 나무가 §10의 표 그대로: 기본 수신자 `null`은 아무 데도 보내지 않음, `group_by`에 `alertname`과 `venue`, 자식 경로 넷이 이 순서로 각각 맞춤 조건 하나: `Watchdog` → `deadman`, P1 → `telegram-p1`, `LakeAuditFailed` → `telegram-p2-noresolve`, P2 → `telegram-p2`. 자식 경로마다 묶음 대기·간격·반복 시간이 §10의 표 값 그대로 적혀 있음(빠지면 Alertmanager 기본값을 쓰므로 실패). 자식 경로에 `continue`·묶음·음소거·하위 경로 없음. 최상위 경로에는 수신자·묶음·자식 경로·시간 말고 다른 키 없음, `group_by`에 `...` 없음. `deadman`은 연동 없음(Q1 전까지), Telegram 수신자 셋은 `telegram_configs` 말고 다른 연동 없음. 경로가 이름한 수신자는 모두 있고 이름은 한 번씩. Telegram 수신자 셋은 설정 하나씩, 문자가 `[MON P1] `·`[MON P2] `로 시작, P2 둘은 소리 없음, P1은 소리 있음, `parse_mode` 빈 값, `LakeAuditFailed`의 수신자만 해소 문자 없음(키가 없으면 Alertmanager는 해소 문자를 보낸다). 억제 규칙은 정확히 하나: `MdCollectorDown`이 같은 거래소의 P2·P3 `Md.*|LakeSealLag|LakeDiskDaysLeft.*`를 막음. 이 이름 패턴에 걸리는 알림이 qdp 지표(`lake_audit_*`)를 읽으면 실패), R11(설정에 `bot_token`, `chat_id`, `api_url`, `http_config`, `telegram_api_url`, `url` 키 없음: 비밀과 웹훅 주소는 파일 경로로만, API는 기본값만. Telegram 설정마다 `bot_token_file`과 `chat_id_file`이 `/etc/` 아래 파일 경로. 어느 배치 파일에도 Telegram 토큰 모양(숫자 6개 이상`:`문자 30개 이상)이나 `chat` 키 옆의 6자리 이상 숫자가 없음. 찾아도 그 값을 출력하지 않는다. `*_file` 키에 토큰이나 채팅 ID를 붙여 넣어도 값은 출력하지 않는다). Grafana: G1(`[server] http_addr`가 한 번, `192.0.2.1`이나 100.64.0.0/10의 Tailscale 주소. `grafana.service`에 `GF_*` 환경 변수가 없고 `Exec` 줄은 `ExecStart=/opt/grafana/bin/grafana server --homepath /opt/grafana --config /etc/hl-oms-obs/grafana/grafana.ini` 하나뿐: 환경 변수, `cfg:` 인자, `--configOverrides`는 파일을 덮고 `--profile-addr`는 리스너를 연다), G2(`[unified_alerting]`과 `[auth.anonymous]`의 `enabled = false`가 한 번씩), G3(`admin_password`가 `$__file{/etc/…}` 한 번. 값을 출력하지 않음), G4(데이터 소스 둘이 §12의 값 그대로: uid·형식·루프백 주소·`access: proxy`·VictoriaMetrics가 기본·화면에서 고칠 수 없음·다른 키 없음), G5(파일 제공자 하나가 대시보드가 설치되는 디렉터리를 읽고 화면에서 지우거나 저장하지 못함. `[paths] provisioning`이 프로비저닝 파일의 설치 위치), G6(대시보드 uid가 하나씩, 패널마다 제목, 패널과 식이 데이터 소스 `vm`이나 `vl`(식은 패널과 같은 것), 식이 비지 않음, 글 패널과 행에는 식이 없음, 변수는 `vm`의 질의 변수, D3에 글 패널 `매일 확인: 실패한 일일 작업`이 하나이고 `systemctl --failed 'md-*' 'crypto-lake-audit*'`를 담음), G7(VictoriaMetrics 식과 `label_values(<지표>, <라벨>)` 변수가 R1·R3·R4를 통과하고 라벨은 그 지표의 라벨. VictoriaLogs 식은 읽지 않음), G8(D2·D3가 그 uid와 제목으로 하나씩. D3는 `LakeAuditFailed`의 식에서 ` > 0`을 뺀 값과, 긁는 구성 요소마다 `process_resident_memory_bytes{job="<작업>"} / <U6의 설계 상한>` 항을 정확히 보여 줌), G9(`[paths] data`와 `plugins`가 `/mnt/nvme/` 아래). 검사기는 MetricsQL의 부분 집합만 읽는다(파서가 아님). 문법은 §11의 `vmalert -dryRun`이 본다. LogsQL 식은 읽지 않는다.
2. `systemd-analyze verify`: 단위 사본의 실행 파일 경로(`/usr/local/bin/`, `/usr/local/lib/hl-oms-obs/`, `/opt/grafana/bin/`)를 내려받은 공식 바이너리와 `/bin/true` 링크가 있는 임시 디렉터리로 바꾸고, 템플릿은 인스턴스 링크(`md-collector@bybit.service` 등)로 검사한다. 모르는 키는 `Unknown key`로 나온다. 감사 스크립트는 `sh -n bin/crypto-lake-audit.sh`로 문법만 본다.

## 8. 설치 순서 (소유자, 운영 항목 X1)

1. 시스템 사용자 `hlobs`를 만든다(로그인 없음).
2. 디렉터리: §2의 다섯 곳(`/mnt/nvme/observability/*`는 `hlobs` 소유). 레이크의 `crypto/l0`, `crypto/l1`, `crypto/l0/_verify`, `crypto/audit`을 첫 실행 전에 만든다(아카이버와 감사의 `ReadOnlyPaths=`·`ReadWritePaths=`에 없는 경로가 있으면 단위가 시작 단계에서 실패한다). `l1`은 아카이버가 처음 돌 때 생기므로, 만들지 않고 설치 직후 감사를 손으로 시작하면 없는 경로 때문에 감사 단위가 실패한다.
3. 실행 파일: `/usr/local/bin/{victoria-metrics-prod,victoria-logs-prod,vmalert-prod,alertmanager,md-collector,md-lake,md-archiver,uv}`, Grafana는 `/opt/grafana`. 감사용 qdp: `/opt/qdp-crypto`에 qdp main을 따로 체크아웃하고 그 안에서 `uv sync --frozen`을 한 번 돌린다(§4.1).
4. `install.toml`의 각 줄을 설치한다(저장소의 `deploy/observability`에서):

   ```sh
   install -D -m 0644 systemd/victoriametrics.service /etc/systemd/system/victoriametrics.service
   install -D -m 0644 systemd/victorialogs.service    /etc/systemd/system/victorialogs.service
   install -D -m 0644 systemd/vmalert-metrics.service /etc/systemd/system/vmalert-metrics.service
   install -D -m 0644 systemd/vmalert-logs.service    /etc/systemd/system/vmalert-logs.service
   install -D -m 0644 systemd/alertmanager.service    /etc/systemd/system/alertmanager.service
   install -D -m 0644 systemd/grafana.service         /etc/systemd/system/grafana.service
   install -D -m 0644 systemd/trading.slice           /etc/systemd/system/trading.slice
   install -D -m 0644 systemd/md-collector@.service   /etc/systemd/system/md-collector@.service
   install -D -m 0644 systemd/md-lake-seal@.service   /etc/systemd/system/md-lake-seal@.service
   install -D -m 0644 systemd/md-lake-seal@.timer     /etc/systemd/system/md-lake-seal@.timer
   install -D -m 0644 systemd/md-archiver@.service    /etc/systemd/system/md-archiver@.service
   install -D -m 0644 systemd/md-archiver@.timer      /etc/systemd/system/md-archiver@.timer
   install -D -m 0644 systemd/crypto-lake-audit@.service /etc/systemd/system/crypto-lake-audit@.service
   install -D -m 0644 systemd/crypto-lake-audit@.timer   /etc/systemd/system/crypto-lake-audit@.timer
   install -D -m 0755 bin/crypto-lake-audit.sh        /usr/local/lib/hl-oms-obs/crypto-lake-audit.sh
   install -D -m 0644 victoriametrics/scrape.json     /etc/hl-oms-obs/victoriametrics/scrape.json
   install -D -m 0644 vmalert/metrics/market-data.rules.json /etc/hl-oms-obs/vmalert/metrics/market-data.rules.json
   install -D -m 0644 vmalert/metrics/lake.rules.json        /etc/hl-oms-obs/vmalert/metrics/lake.rules.json
   install -D -m 0644 vmalert/metrics/system.rules.json      /etc/hl-oms-obs/vmalert/metrics/system.rules.json
   install -D -m 0644 vmalert/logs/market-data-logs.rules.json /etc/hl-oms-obs/vmalert/logs/market-data-logs.rules.json
   install -D -m 0644 alertmanager/alertmanager.json  /etc/hl-oms-obs/alertmanager/alertmanager.json
   install -D -m 0644 grafana/grafana.ini             /etc/hl-oms-obs/grafana/grafana.ini
   install -D -m 0644 grafana/provisioning/datasources/hl-oms.yaml /etc/hl-oms-obs/grafana/provisioning/datasources/hl-oms.yaml
   install -D -m 0644 grafana/provisioning/dashboards/hl-oms.yaml  /etc/hl-oms-obs/grafana/provisioning/dashboards/hl-oms.yaml
   install -D -m 0644 grafana/dashboards/d2-market-data.json /etc/hl-oms-obs/grafana/dashboards/d2-market-data.json
   install -D -m 0644 grafana/dashboards/d3-system.json      /etc/hl-oms-obs/grafana/dashboards/d3-system.json
   ```

   Grafana는 켜기 전에 설치본 `/etc/hl-oms-obs/grafana/grafana.ini`의 `http_addr`를 이 PC의 Tailscale 주소로 바꾸고(§12.1), VictoriaLogs 플러그인을 깐다(§12.3).
5. 비밀 파일(토큰, 채팅 ID, Grafana 관리자 암호)은 `/etc/hl-oms/` 아래 권한 0640, 그룹 `hlobs`로 둔다. Alertmanager의 것은 `/etc/hl-oms/alertmanager-telegram-token`(봇 토큰 한 줄)과 `/etc/hl-oms/alertmanager-telegram-chat-id`(채팅 ID 한 줄)다(§10). Grafana 관리자 암호는 `/etc/hl-oms/grafana-admin-password`(암호 한 줄, §12.2). 저장소에는 경로만 있다. `md-lake seal`의 `/etc/hl-oms/telegram.json`은 그대로(0600).
6. `systemctl daemon-reload` 뒤 `systemctl enable --now`는 소유자가 X1에서 한다. **수집기를 vmalert보다 먼저 켠다.** 그러면 수집기 생존 알림 `MdCollectorDown`(P1)을 막을 필요가 없다. vmalert를 먼저 켰을 때만 이 한 줄로 10분(판단(모델 아님)) 막는다:

   ```sh
   amtool silence add alertname=MdCollectorDown --duration=10m --comment=first-install
   ```

   `MdCollectorDown`은 시작 자체보다 길게 막지 않는다. 막힌 동안 수집기가 죽으면 어디서도 알림이 오지 않고 원본 데이터를 잃는다.

   처음 켠 뒤 첫 감사가 올 때까지는 감사 지표가 없어 `LakeAuditStale`(P2)이 울린다. 이것은 따로 한 줄로 막는다(다른 알림에 36시간이 붙지 않게):

   ```sh
   amtool silence add alertname=LakeAuditStale --duration=36h --comment=first-install
   ```
7. 수집기가 12시간 넘게 멈췄다 돌아오면 디스크 남은 일수 알림은 최대 12시간 꺼져 있다(준비 시간). 그동안 디스크가 차는 것은 L0 쓰기 오류 알림과 수집기의 `[MD P1]` 디스크 하한이 알린다.

## 9. 알림 (vmalert 규칙)

등급(설계 §8.3): P1 = 원본 데이터를 잃거나 관측을 잃는 중(즉시, 소리). P2 = 저하(소리 없음). P3 = 추세(대시보드와 주간 검토만). `Watchdog`은 등급이 아니라 늘 울리는 신호다. Alertmanager의 경로와 억제는 §10에 둔다. 문턱값은 설계가 정한 것(60초·2분 생존, 7일·2일, 2시간 봉인 지연, 5 ms 시각 오프셋) 말고는 판단(모델 아님)이고 주간 검토에서 고친다(소유자 질문 Q5).

수집기 쪽 알림은 수집기가 VictoriaMetrics에 직접 넣는 값(`/api/v1/import`)으로 판단하고, 감사 알림은 qdp 감사기가 넣는 값으로, 스택 알림은 VictoriaMetrics가 긁는 값으로 판단한다. vmalert는 30초 전의 저장소를 보므로(`-rule.evalDelay` 기본값) 모든 알림은 조건이 생긴 뒤 30초 넘게 지나서 울린다. 수집기 생존 알림이 Alertmanager의 P1 수신자에 닿는 시간은 마지막 심박 뒤 약 90~170초다(판단(모델 아님), 설치 뒤 X1에서 잰다).

**시세 수집** (`market-data.rules.json`, 그룹 `market-data`, 10초)

| 이름 | 등급 | 조건 | 뜻 | 할 일 |
|---|---|---|---|---|
| `MdCollectorDown` | P1 | 거래소마다 하나. 심박(`md_collector_heartbeat_ts`)이 60초 넘게 낡았거나 2분 동안 값이 없음 | 수집기가 죽었거나 멈췄다. 원본 시세를 잃는 중이다. 죽은 수집기는 자기 `[MD P1]`을 보낼 수 없어서 이 규칙이 있다 | `systemctl status md-collector@<v>`, `journalctl -u md-collector@<v>`로 원인을 찾고 다시 띄운다 |
| `MdLakeGaugesMissing` | P2 | 거래소마다 하나. `lake_disk_free_bytes`나 `lake_seal_lag_hours`가 10분 동안 없음 | 수집기는 살아 있는데 디스크 여유(`statvfs` 실패)나 봉인 지연(`_open` 목록 실패) 값이 안 온다. 그동안 남은 일수와 봉인 지연 알림은 울릴 수 없다 | 수집기 로그에서 그 오류를 찾는다 |
| `MdCollectorRestarting` | P2 | 10분 안에 `lake_l0_bytes_total`이 한 번이라도 줄었음(재시작마다 0부터 다시 셈) | 수집기가 심박은 보내지만 몇 분마다 죽었다 다시 뜬다(예: 메모리 상한에서 강제 종료, panic). 재시작마다 그 사이의 원본 데이터와 호가창 상태를 잃는다. 운영자가 직접 재시작해도 한 번 울린다. 옛 프로세스가 마지막으로 보낸 값이 새 프로세스의 첫 값보다 클 때만 재시작을 본다(값을 보내기 전에 죽는 프로세스는 보지 못함) | `systemctl status md-collector@<v>`와 `journalctl -u md-collector@<v>`로 종료 원인(`oom-kill`, panic)을 찾는다 |
| `MdConnectionSilent` | P2 | 돌고 있는 연결 하나가 2분 동안 메시지 0개, 1분 지속 | 거래소 쪽 구독이 멈췄을 수 있다. 멈춘 연결은 값이 없으므로 여기 없고 `MdTopicsStale`·`MdReconnectsHigh`가 본다 | 그 연결의 재연결·구독 기록을 본다 |
| `MdTopicsStale` | P2 | 호가창 토픽의 10% 넘게가 60초 넘게 메시지 없음, 5분 지속 | 일부 종목이 비었다 | D2 대시보드에서 어느 종목인지 본다 |
| `MdRecvLagHigh` | P2 | 수신 지연 p99 > 1,000 ms, 5분 지속 | 이 PC가 느리거나 네트워크가 막혔다 | 연구 작업·디스크·시계 오프셋을 본다. 그 구간은 거래소 간 선후 분석에서 뺀다 |
| `MdReconnectsHigh` | P2 | 10분에 재연결 > 10 | 재연결마다 공백이 생긴다 | 거래소 장애 공지와 이 PC의 네트워크를 본다 |
| `MdGapsHigh` | P2 | 10분에 호가창 공백 > 20 | 공백 구간은 L1 gap 행으로 남는다 | 수신 지연·재연결 알림과 함께 본다 |
| `MdQueueHigh` | P2 | L0 쓰기 대기열 > 8 MiB(칸 상한 16 MiB의 절반), 1분 지속 | 디스크가 느리거나 막혔다. 상한에 닿으면 그 연결이 끊긴다 | 디스크 I/O와 연구 작업을 본다 |
| `MdObsPushDropping` | P2 | 10분에 `obs_push_dropped_total`이 늘었음 | 수집기가 VM·VL로 보내지 못한 줄을 버리고 있다(설계 §8.3 "VM/VL 쓰기 실패") | 두 저장소가 살아 있는지 본다 |
| `ClockOffsetHigh` | P2 | 시계 오프셋 절댓값 > 5 ms, 5분 지속 | 그 구간의 수신 지연과 선후 분석을 믿을 수 없다 | `chronyc tracking`(§6) |

**레이크** (`lake.rules.json`)

| 이름 | 등급 | 조건 | 뜻 | 할 일 |
|---|---|---|---|---|
| 기록 `lake_disk_days_left` | — | `lake_disk_free_bytes / clamp_min(-deriv(lake_disk_free_bytes[24h]) * 86400, 1)`, 1분마다(그룹 `lake-recording`) | 지난 24시간의 속도로 디스크가 차기까지 남은 일수. 저장된 값으로 계산하므로 수집기를 다시 시작해도 0부터 다시 세지 않는다. 디스크가 줄지 않으면 매우 큰 값 | — |
| `LakeDiskDaysLeftLow` | P2 | 남은 일수 < 7, 10분 지속. 지난 24시간의 가장 오래된 값이 12시간보다 오래됐을 때만 | 7일 안에 찬다 | 옮기거나 지울 계획을 세운다(L0는 지우지 않는다) |
| `LakeDiskDaysLeftCritical` | P1 | 남은 일수 < 2, 같은 조건 | 2일 안에 찬다. 차면 L0 쓰기가 멈춘다 | 지금 공간을 만든다 |
| `LakeL0WriteError` | P1 | `lake_l0_write_errors_total` > 0, 또는 10분 안에 늘었음(새 프로세스는 0부터) | L0 쓰기가 실패했다. 쓰기는 첫 오류에서 멈추므로 원본 시세를 잃는 중이다 | 오류를 보고 디스크를 확인한 뒤 수집기를 다시 띄운다 |
| `LakeSealLag` | P2 | 봉인 지연 > 2시간 | 봉인이 멈췄거나, 6시간 제한에 걸려 매시 다시 시작되고 있다 | `journalctl -u md-lake-seal@<v>`로 둘을 구분한다 |
| `LakeAuditFailed` | P2 | 감사 실패 카운터의 지금 값 − 26시간 전 값(그때 값이 없으면 0) > 0(그룹 `lake-audit`, 1분) | 감사가 새 실패 파일을 찾았다(설계 §8.3 "감사 실패") | `crypto/audit/venue=<v>/date=<d>.json`을 읽는다 |
| `LakeAuditStale` | P2 | 거래소마다 하나. 마지막 감사의 끝난 시각이 36시간보다 오래됐거나 36시간 동안 값이 없음 | 감사가 돌지 않았거나 보내기 전에 실패했다(종료 코드 1의 Python 예외, 2, 3, 4, §4.1) | `systemctl status crypto-lake-audit@<v>`, `journalctl -u crypto-lake-audit@<v>` |
| `LakeNotAudited` | P3 | verify 결과가 없는 봉인 L0 파일 > 0, 1시간 지속 | 9일 창 밖의 날짜일 수 있다 | §4.1의 손 verify |

- 남은 일수 알림의 준비 시간(12시간)은 처음 켠 날 아카이버의 첫 폭주가 페이지를 내지 않게 한다(판단(모델 아님)). 수집기가 12시간 넘게 멈췄다 돌아와도 같다(§8). 그동안 디스크가 차는 것은 `LakeL0WriteError`와 수집기의 `[MD P1]` 디스크 하한이 알린다. 수집기의 `telemetry.slow_ms`는 20 000 이하로 둔다(§3.1).
- 설계 §8.3의 "디스크 80%"는 9.4의 남은 일수로 대체됐고 규칙이 없다. 저널 디스크 규칙은 OMS 배치에서 더한다.
- `LakeAuditFailed`는 26시간 뒤 저절로 끝난다. 그것은 파일이 고쳐졌다는 뜻이 아니다. 아무도 고치지 않은 실패 파일은 감사기가 7일마다 다시 감사하므로(qdp `audit.py`의 `MAX_AGE_DAYS`) 이 알림이 7일마다 다시 온다. 감사 상태 파일을 지우거나 바꾼 뒤에는 카운터가 옛 값보다 작게 다시 시작해 26시간 동안 옛 값을 빼므로, 그 첫 전체 실행이 찾은 실패는 알림이 오지 않는다: 그 실행의 `crypto/audit/venue=<v>/date=<d>.json`을 손으로 읽는다.

**시스템** (`system.rules.json`)

| 이름 | 등급 | 조건 | 뜻 | 할 일 |
|---|---|---|---|---|
| `Watchdog` | watchdog | `vector(1)`, 늘 울림(그룹 `watchdog`) | 바깥 dead-man 수신자가 이것이 끊긴 것을 보고 스택이 죽었음을 안다. 바깥 서비스는 소유자가 정한다(Q1). 그때까지 이 층은 바깥으로 아무것도 알리지 않는다 | — |
| `ObsComponentDown` | P2 | `up{job=~"victorialogs|vmalert-logs|alertmanager"} == 0`, 2분 지속 | 그 구성 요소가 응답하지 않는다. 죽은 VictoriaMetrics나 vmalert-metrics는 자기 알림을 낼 수 없다: 그것은 `Watchdog`의 몫이다 | `systemctl status <job 이름>` |
| `VmalertIterationMissed` | P2 | 10분에 평가 주기를 놓쳤음 | 그동안 알림이 늦다 | 저장소 응답 시간과 vmalert 메모리를 본다 |
| `AlertmanagerTelegramFailing` | P2 | 10분에 Telegram 보내기 실패 | 알림이 Telegram으로 오지 않을 수 있다(이 알림 자신도) | 대시보드에서 보고 `journalctl -u alertmanager` |
| `ObsMemoryNearCap` | P3 | 구성 요소의 상주 메모리 > 단위 `MemoryMax`의 90%, 10분 지속(VM·VL 1G, vmalert 200M, Alertmanager 100M; Grafana는 긁지 않음) | 상한에 닿으면 강제 종료된다 | 주간 검토에서 상한이나 설정을 고친다 |

**로그** (`market-data-logs.rules.json`, 그룹 `md-logs`, `type: vlogs`, 30초)

| 이름 | 등급 | 조건 | 뜻 | 할 일 |
|---|---|---|---|---|
| `MdErrorLogs` | P2 | 지난 5분에 `service:="md-collector" level:=error` 줄이 거래소별로 1개 이상 | 수집기가 error 줄을 남겼다. 5분 창이라 줄 하나나 한 번의 폭주가 발생 하나·해소 하나가 된다(30초마다 한 쌍이 아님) | VictoriaLogs에서 그 줄을 읽는다 |

## 10. 알림 전달 (Alertmanager와 Telegram)

`alertmanager/alertmanager.json`(설치 경로 `/etc/hl-oms-obs/alertmanager/alertmanager.json`)이 vmalert 둘이 보낸 알림을 등급에 따라 보낸다. 묶음 대기·간격·반복 시간은 판단(모델 아님)이고 주간 검토에서 고친다. 배치 검사 R10이 아래 표의 값을 그대로 요구하므로 고칠 때는 검사의 경로 표(`crates/xtask/tests/deploy/alertmanager.rs`의 `ROUTES`)와 이 표를 함께 고친다.

| 순서 | 맞춤 조건 | 수신자 | 묶음 대기 / 간격 / 반복 | 보내는 곳 |
|---|---|---|---|---|
| 1 | `alertname="Watchdog"` | `deadman` | 0초 / 1분 / 1분 | 아무 데도(자리 표시, 아래) |
| 2 | `severity="P1"` | `telegram-p1` | 10초 / 1분 / 30분 | Telegram, 소리 있음, 해소 문자 보냄 |
| 3 | `alertname="LakeAuditFailed"` | `telegram-p2-noresolve` | 30초 / 5분 / 4시간 | Telegram, 소리 없음, 해소 문자 없음 |
| 4 | `severity="P2"` | `telegram-p2` | 30초 / 5분 / 4시간 | Telegram, 소리 없음, 해소 문자 보냄 |
| 그 밖 | P3와 나머지 | `null` | — | 아무 데도: 대시보드와 주간 검토만(설계 §8.3) |

- 알림은 `alertname`과 `venue`로 묶는다. 문자 하나는 한 알림 이름, 한 거래소다.
- 문자 모양(일반 글자, HTML 아님: 라벨 값이 서식으로 읽히지 않음). 첫 줄 `[MON P1] <알림 이름> <거래소> 발생 <N>건` 또는 `… 해소`, 그다음 알림마다 한 줄 `- <summary> (<MM-DD HH:MM:SS> UTC)`. 해소된 알림의 줄은 `(해소)`로 시작한다. 거래소 라벨이 없는 알림(예: `ObsComponentDown`)은 거래소 자리를 비운다. P2는 `[MON P2] `로 시작한다.
- 직접 알림(`[MD P1]`, `[WATCH P1]`)과 같은 봇이다(설계 Q18: 같은 봇, 머리말로 구분). Alertmanager는 `telegram.json`을 읽지 못하므로 토큰과 채팅 ID는 각각 한 줄짜리 파일이다: `/etc/hl-oms/alertmanager-telegram-token`, `/etc/hl-oms/alertmanager-telegram-chat-id`(0640, 그룹 `hlobs`, §8). 저장소에는 경로만 있다. 배치 검사(R11)는 설정 안의 토큰·채팅 ID 값과 다른 API 주소를 거부한다.
- **Telegram 도달성(운영 항목 X4).** 직접 알림은 `api.telegram.org`가 막히면 `149.154.167.220`으로 바로 간다. Alertmanager에는 그 우회가 없다: 설정한 이름으로만 보낸다. 이 PC에서 `api.telegram.org`가 막히면 `[MON …]` 문자는 오지 않는다. 그때 `AlertmanagerTelegramFailing`(P2)이 울리지만 그것도 Telegram으로는 오지 않으므로 대시보드에서만 보인다. 설치(X1) 때 경로마다 시험 알림을 하나씩 보내 확인하고, 막히면 로컬 중계나 차단 해소를 소유자가 정한다(추론, 측정 안 함).
- **dead-man(소유자 결정 D3, 질문 Q1).** `Watchdog`은 늘 울리고 1분마다 `deadman` 수신자로 간다. 지금 이 수신자에는 연동이 없다(자리 표시). 그래서 스택 전체가 죽어도 바깥에서 알려 주는 것이 없다. 바깥 서비스를 정하면 `deadman`에 `webhook_configs`의 `url_file`(주소는 `/etc/hl-oms/` 아래 파일)을 더한다. `url`이나 `http_config`를 쓰지 않는다(R11). 그때까지 배치 검사 R10은 `deadman`에 어떤 연동이 있어도 실패한다. 바깥 서비스를 더하는 변경이 이 규칙을 바꾼다.
- **억제 규칙.** 거래소의 `MdCollectorDown`(P1)이 울리는 동안 같은 거래소의 P2·P3 알림 중 이름이 `Md.*`, `LakeSealLag`, `LakeDiskDaysLeft.*`인 것은 보내지 않는다(설계 §8.3 B3 "파생 알림 억제"). 이것들은 수집기 자신이 넣는 값(`md_*`, `lake_seal_lag_hours`, `lake_disk_free_bytes`)과 그 로그 줄로 판단하므로 수집기가 죽으면 함께 울릴 뿐이다. 막지 않는 것: P1 알림 모두(`LakeL0WriteError`, `LakeDiskDaysLeftCritical`), qdp 감사기가 넣는 값의 알림(`LakeAuditFailed`, `LakeAuditStale`, `LakeNotAudited`), 스택 알림. 거래소 라벨이 없는 알림은 막지 않는다.
- **`LakeAuditFailed`는 해소 문자를 보내지 않는다.** 이 알림은 26시간 뒤 저절로 끝나지만 그것은 파일이 고쳐졌다는 뜻이 아니다(§9). 그래서 3번 경로가 P2 경로보다 먼저 이 알림을 받아 해소 문자가 없는 수신자로 보낸다. 끝난 뒤에도 `crypto/audit/venue=<v>/date=<d>.json`을 확인한다. 고쳐질 때만 끝나는 지표(현재 실패 수)는 qdp가 지표 이름을 정하므로 qdp 후속 작업이다.

## 11. 규칙 확인과 주간 소음 검토

규칙 파일을 고친 뒤, 바이너리가 있는 곳에서(CI에는 바이너리가 없다):

```sh
cd deploy/observability/vmalert
for f in metrics/*.json; do vmalert-prod -dryRun -rule="$f"; done
vmalert-prod -dryRun -rule.defaultRuleType=vlogs -rule=logs/market-data-logs.rules.json
vmalert-tool-prod unittest --files tests/market-data.test.json --files tests/lake.test.json --files tests/system.test.json
```

- `-dryRun`은 MetricsQL·LogsQL 문법을 본다. 나쁜 식이면 `fatal` 줄이 나온다.
- `vmalert-tool unittest`는 알림마다 울리는 경우와 울리지 않는 경우를 시계 없이 재생한다. 테스트 파일의 `exp_annotations`는 규칙의 `summary`와 글자까지 같아야 하므로 `summary`를 고치면 테스트 파일도 고친다. 로그 규칙에는 테스트가 없다(`-dryRun`만).
Alertmanager 설정을 고친 뒤(같은 디렉터리의 `amtool`, 네트워크 없음):

```sh
cd deploy/observability/alertmanager
amtool check-config alertmanager.json
amtool config routes test --config.file=alertmanager.json alertname=Watchdog severity=watchdog   # deadman
amtool config routes test --config.file=alertmanager.json alertname=MdCollectorDown severity=P1 venue=bybit   # telegram-p1
amtool config routes test --config.file=alertmanager.json alertname=LakeAuditFailed severity=P2 venue=bybit   # telegram-p2-noresolve
amtool config routes test --config.file=alertmanager.json alertname=MdGapsHigh severity=P2 venue=bybit   # telegram-p2
amtool config routes test --config.file=alertmanager.json alertname=LakeNotAudited severity=P3 venue=bybit   # null
```

- `check-config`는 토큰·채팅 ID 파일이 없어도 통과한다(파일은 보낼 때 읽는다). 문자 틀은 `amtool template render --template.glob=<빈 파일> --template.text="<message 값>"`으로 그려 본다(amtool의 예시 데이터라 알림 이름은 비어 나온다).
- `cargo test --locked -p xtask --test deploy`(§7)는 CI에서도 돈다. 규칙이 없는 지표를 쓰거나 등급이 없거나 설계의 식이 바뀌면 실패한다.

주간 소음 검토(설계 §8.3): 매주 알림마다 울린 횟수와 조치 여부를 보고, 조치가 없던 알림은 문턱값을 고치거나 지운다. 고친 문턱값은 규칙 파일과 이 표에 함께 적는다.

## 12. Grafana (화면 D2·D3)

Grafana는 보기만 한다. 알림은 vmalert와 Alertmanager가 낸다(`[unified_alerting] enabled = false`, 설계 표 8-5). 거래 안전 경로는 Grafana에 기대지 않는다(설계 §2). Grafana 자신은 긁지 않으므로 그 메모리와 생존은 D3에 나오지 않는다.

| 저장소 파일 | 설치 경로 | 내용 |
|---|---|---|
| `grafana/grafana.ini` | `/etc/hl-oms-obs/grafana/grafana.ini` | 주소, 포트 3000, 저장 경로(`/mnt/nvme/observability/grafana`, 플러그인은 그 아래 `plugins`), 프로비저닝 경로, 끈 기능 |
| `grafana/provisioning/datasources/hl-oms.yaml` | `/etc/hl-oms-obs/grafana/provisioning/datasources/hl-oms.yaml` | 데이터 소스 둘: `VictoriaMetrics`(uid `vm`, `prometheus` 형식, `http://127.0.0.1:8428`, 기본), `VictoriaLogs`(uid `vl`, `victoriametrics-logs-datasource`, `http://127.0.0.1:9428`). 둘 다 Grafana 서버가 읽는다(`access: proxy`: iPhone의 브라우저는 이 PC의 127.0.0.1에 닿지 못한다). 화면에서 고칠 수 없다 |
| `grafana/provisioning/dashboards/hl-oms.yaml` | `/etc/hl-oms-obs/grafana/provisioning/dashboards/hl-oms.yaml` | 파일 제공자 하나: `/etc/hl-oms-obs/grafana/dashboards`를 읽는다. 화면에서 지우거나 저장하지 못한다 |
| `grafana/dashboards/d2-market-data.json`, `d3-system.json` | `/etc/hl-oms-obs/grafana/dashboards/` | 대시보드 둘 |

모든 파일은 JSON으로 쓴 YAML이다(§1). 시작할 때 `provisioning/plugins`와 `provisioning/alerting` 디렉터리가 없다는 오류 줄이 나오지만 시작은 계속된다(2026-11-25 시험). 줄이 보기 싫으면 설치본 아래에 빈 디렉터리 둘을 만든다.

### 12.1 주소 (소유자 질문 Q3)

- 저장소의 `http_addr = 192.0.2.1`은 자리 표시다(RFC 5737 TEST-NET-1, 이 PC에 배정되지 않음). 이 값으로는 Grafana가 시작하지 않는다. 확인(2026-11-25, Grafana 13.3.2, 저장소 파일 그대로, 경로만 임시 디렉터리로): `failed to open listener on address 192.0.2.1:3000: listen tcp 192.0.2.1:3000: bind: cannot assign requested address`, 종료 코드 1.
- 값이 비었거나 줄이 없으면 Grafana는 모든 인터페이스에서 듣는다(설계 §8.4). 그래서 비워 두지 않고, 시작을 거부하는 주소를 두었다.
- 설치할 때 설치본만 이 PC의 Tailscale 주소(`tailscale ip -4`, 100.64.0.0/10)로 바꾼다. 저장소 파일은 자리 표시 그대로 둔다(공개 저장소에 이 PC의 주소를 올리지 않음). 다시 설치하면 다시 바꾼다. 배치 검사 G1은 `192.0.2.1`과 이 범위의 주소만 받는다.
- 설정은 `grafana.ini`에만 둔다. `GF_*` 환경 변수, `cfg:` 인자, `--configOverrides`는 파일을 덮고 `--profile-addr`는 따로 리스너를 연다. 배치 검사는 파일만 읽으므로 단위에 이것들이 있으면 G1이 실패한다.

### 12.2 관리자 암호와 접근

- `[security] admin_password = $__file{/etc/hl-oms/grafana-admin-password}`: 암호 한 줄짜리 파일(0640, 그룹 `hlobs`, §8). 저장소에는 경로만 있다. 파일이 없으면 Grafana는 시작하지 않는다(2026-11-25 시험: `got error while expanding security.admin_password with expander 'file': … no such file or directory`, 종료 코드 1).
- 이 값은 첫 시작 때 관리자 계정에 쓰인다(Grafana 13.3.2 `conf/defaults.ini`의 설명: "can be changed before first start of grafana, or in profile settings". 첫 시작 뒤에 파일만 바꾸면 어떻게 되는지는 시험하지 않음). 첫 시작 뒤에 파일을 바꾸면 이렇게 맞춘다(셸 기록에 암호가 남지 않게 표준 입력으로, `--password-from-stdin`은 이 판의 `grafana cli` 도움말에 있음):

  ```sh
  sudo -u hlobs /opt/grafana/bin/grafana cli --homepath /opt/grafana --config /etc/hl-oms-obs/grafana/grafana.ini \
    admin reset-admin-password --password-from-stdin < /etc/hl-oms/grafana-admin-password
  ```
- 익명 접근은 꺼져 있다(`[auth.anonymous] enabled = false`, G2). 시험에서 로그인 없는 `/api/search`는 401이었다.
- 바깥으로 나가는 호출은 끔(판단(모델 아님)): 사용 통계(`reporting_enabled`), 갱신 확인 둘, 뉴스, 미리 깔리는 플러그인(`preinstall_disabled`), 플러그인 서명 키 내려받기(`public_key_retrieval_disabled`: 첫 시험에서 Grafana가 시작하자마자 grafana.com에서 키를 받으려 한 줄이 있어 끔), 브라우저의 gravatar(`disable_gravatar`). 이 설정으로 돌린 두 번째 시험의 기록에는 바깥 주소로 가는 줄이 없었다.

### 12.3 VictoriaLogs 플러그인

`victoriametrics-logs-datasource`는 Grafana에 들어 있지 않다. 운영자가 한 번 깐다(VictoriaLogs v1.54.0과 맞는 판, 표 8-5):

```sh
sudo -u hlobs /opt/grafana/bin/grafana cli --homepath /opt/grafana --pluginsDir /mnt/nvme/observability/grafana/plugins \
  plugins install victoriametrics-logs-datasource
```

플러그인이 없어도 데이터 소스는 목록에 나온다(스펙 단계와 2026-11-25 시험). D2·D3는 VictoriaLogs를 읽지 않는다. 이 데이터 소스를 쓰는 화면(사건 타임라인)은 아직 없다.

### 12.4 D2 시세 수집: 어느 거래소·종목이 비거나 늦나

변수: `거래소`(`label_values(md_collector_heartbeat_ts, venue)`), `종목`(그 거래소의 `md_spread_bp`에 있는 종목). 숫자는 그 알림의 문턱값을 선으로 보여 준다.

| 패널 | 값 | 관련 알림(§9) |
|---|---|---|
| 초당 메시지, 초당 바이트(시장별) | `md_msgs`·`md_bytes`의 1분 합 / 60 | `MdConnectionSilent` |
| 10분 공백, 10분 재연결(시장·채널별) | `increase(md_gaps_total[10m])`, `increase(md_reconnects_total[10m])` | `MdGapsHigh`, `MdReconnectsHigh` |
| 수신 지연 p99(시장별) | `md_recv_lag_ms{quantile="0.99"}`, 선 1000 ms | `MdRecvLagHigh` |
| 멈춘 호가 주제 비율(채널별) | 마지막 메시지가 60초보다 오래된 주제 / 모든 주제, 선 10% | `MdTopicsStale` |
| 마지막 메시지 경과 상위 20 | `topk(20, md_last_msg_age_s)` 표 | — |
| 스프레드, 호가 수량(종목별) | `md_spread_bp`, `md_depth_qty` | — |
| 연결별 큐 바이트 | `md_queue_bytes`, 선 8 MiB | `MdQueueHigh` |

### 12.5 D3 시스템: 서비스가 살아 있고 자원이 남았나

| 패널 | 값 | 관련 알림(§9) |
|---|---|---|
| 수집기 심박 경과(거래소별) | `time() - md_collector_heartbeat_ts`, 선 60초 | `MdCollectorDown` |
| 스택 생존 | `up`(작업별) | `ObsComponentDown` |
| 메모리 / 단위 상한 | 구성 요소마다 상주 메모리 / 그 단위의 `MemoryMax`(§3), 선 0.9와 1. G8이 상한을 U6 표와 맞춘다 | `ObsMemoryNearCap` |
| 레이크 디스크 여유, 남은 일수 | `lake_disk_free_bytes`, `lake_disk_days_left`(2일 아래 빨강, 7일 아래 주황) | `LakeDiskDaysLeftLow`·`Critical` |
| 봉인 지연 | `lake_seal_lag_hours`, 선 2시간 | `LakeSealLag` |
| 시각 오프셋 | `node_clock_offset_ms` | `ClockOffsetHigh` |
| 지표 전송 버림(10분) | `increase(obs_push_dropped_total[10m])` | `MdObsPushDropping` |
| 마지막 감사 경과 | `time() - lake_audit_last_run_ts`(최근 3일), 선 36시간 | `LakeAuditStale` |
| 26시간 안의 새 감사 실패 | `LakeAuditFailed`의 식에서 ` > 0`을 뺀 값(G8이 같음을 확인). 0보다 크면 `crypto/audit/venue=<v>/date=<d>.json`을 읽는다 | `LakeAuditFailed` |
| 감사 공백 행: `clock_step`, 종류별 | `lake_audit_gap_rows`(최근 값). `clock_step`은 §6 | — |
| VictoriaMetrics·VictoriaLogs 디스크 여유 | `vm_free_disk_space_bytes`, `vl_free_disk_space_bytes` | — |
| 울리는 알림 | `ALERTS{alertstate="firing"}` 표. 보냈는지는 Alertmanager가 정한다(§10) | — |
| 매일 확인: 실패한 일일 작업 | 글: `systemctl --failed 'md-*' 'crypto-lake-audit*'`와, 알림이 없는 실패(감사 날짜 하나의 쓰기 오류 종료 코드 3, verify 실패 종료 코드 5, 아카이버 실패·강제 종료) | 없음(§4) |

수집기의 메모리와 일일 작업 단위의 상태는 값으로 보여 주지 않는다. 내보내는 것이 없다(ADR-035).

### 12.6 iPhone에서 보기 (운영 항목 X2)

iPhone에서 Tailscale을 켜고 같은 tailnet에서 `http://<이 PC의 Tailscale 주소>:3000`을 연다. 관리자 계정과 §12.2의 암호로 들어간다. 연결은 Tailscale(WireGuard)이 암호화하므로 Grafana는 HTTP로 둔다(판단(모델 아님)). 이 접속은 아직 확인하지 않았다(X2, 소유자).

### 12.7 고칠 때

- 대시보드는 화면에서 저장되지 않는다(`allowUiUpdates: false`, 시험에서 `Cannot save provisioned dashboard`). 저장소의 JSON을 고치고 다시 설치하면 제공자가 파일을 다시 읽는다.
- 패널 식은 규칙 파일과 같은 지표 목록으로 검사된다(G7). 없는 지표나 라벨, 허용되지 않은 함수를 쓰면 `cargo test --locked -p xtask --test deploy`가 실패한다.
- D3의 감사 실패 패널은 `LakeAuditFailed`의 식을 따라간다. 그 알림을 고치면 패널도 고친다(G8).
