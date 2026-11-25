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
| `venues.toml` | 이 PC가 수집하는 거래소(`bybit`) |

아직 없는 파일: vmalert 규칙 파일, VictoriaMetrics 수집 설정(`scrape.json`), Alertmanager 설정, Grafana 설정과 대시보드. 단위의 명령줄은 그 설치 경로를 이미 가리킨다. 그 파일이 없는 동안 이 단위들은 시작하지 못한다(그래서 지금은 켜지 않는다).

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

**종료 코드와 어디서 보이나** (아래의 `Lake*` 알림 규칙 파일은 아직 없다, §1)

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

1. `cargo test --locked -p xtask --test deploy`. 실패하면 줄마다 `deploy: <파일>: <규칙>: <내용>`이 나온다. 규칙: P1(파일이 종류대로 읽힘), U1(서비스마다 `MemoryMax` 하나, 크기, `infinity` 아님), U2(oneshot 서비스 ↔ 같은 이름의 타이머, 타이머에 `Unit=` 없음), U3(수집기의 재시작·감시 설정과 보호 설정, 시작 제한의 옛 이름과 `[Service]` 쪽 키, `MemoryHigh` 없음), U4(슬라이스의 가중치와 예약), U5(세 타이머의 시각과 `Persistent`), U6(설계 상한), U7(아카이버와 감사의 일일 작업 잠금), U8(`127.0.0.1`에서만 듣고 Alertmanager 클러스터 포트 없음. 리스너 플래그는 어떤 철자로 써도 모두 셈), U9(저장소가 `/mnt/nvme/` 아래, 저장소마다 경로 플래그 한 번), U10(보관 1년, VM 메모리 한도), U11(vmalert 둘의 원본·알림·간격), U12(어느 배치 파일도 KR qdp 체크아웃을 이름하지 않음, 감사 스크립트의 `venue=$1`과 `--state "$STATE_DIRECTORY/audit-$venue.json"`, 감사 단위의 `StateDirectory=qdp-crypto-audit`. 스크립트는 CI에서 돌지 않으므로 다시 verify할 날짜와 종료 코드를 정하는 줄도 글자 그대로 고정: 9일 창, `-cnewer "$stamp"`, 지금보다 뒤인 도장 검사, 도장 → `sleep 1` → verify → `mv` 순서, 실패 시 옛 도장 유지, `exit 5`, `--vm-url http://127.0.0.1:8428`), U13(`venues.toml`의 거래소가 수집기와 아카이버 모두에 등록됨), U14(설치 지도), U15(스택은 `hlobs`, 어느 단위에도 `User=root`를 쓰지 않음), U16(`md-lake verify`나 감사 스크립트를 돌리는 단위가 읽기 전용 `l0` 아래의 `_verify`에, 감사는 `crypto/audit`에도 쓸 수 있음. 감사 스크립트를 돌리는 다른 단위도 `crypto/audit`에 쓸 수 있어야 함. 감사 단위의 두 경로 줄은 정확히 §4.1의 값이고 `LAKE=/mnt/nvme/lake` 한 번), U17(`Exec*` 줄에 `$(`, 백틱, `%i`·`%I`·`%n`·`%%`가 아닌 `%` 없음: systemd는 셸을 돌리지 않음), U18(스택 단위의 재시작·격리 설정, 쓰는 디렉터리, 다른 단위에 묶이지 않음), U19(봉인·아카이버·감사 실행의 시간 제한, 아카이버의 스레드 1개와 L1 삭제 꺼짐, `EnvironmentFile=` 없음).
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
   ```
5. 비밀 파일(토큰, 채팅 ID, Grafana 관리자 암호)은 `/etc/hl-oms/` 아래 권한 0640, 그룹 `hlobs`로 둔다. 저장소에는 경로만 있다. `md-lake seal`의 `/etc/hl-oms/telegram.json`은 그대로(0600).
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
