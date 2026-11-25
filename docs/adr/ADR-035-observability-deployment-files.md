# ADR-035 관측 스택과 시세 단위의 배치 파일, 그리고 그 검사

상태: 채택 (2026-11-25).

운영 절차: [docs/operations/observability.md](../operations/observability.md). 설계 출처: 통합 설계 원장(`RESEARCH/CRYPTO_TRADING_INTEGRATED_TN_20261118/10_DESIGN_DRAFT.md`, 저장소 밖) §5.4.5, §5.5, §8.3~§8.5와 개정 R1~R5, TN 표 8-5·8-6·8-6a·9-4.

## 1. 문제와 발생 가능한 실패

- 수집기, 봉인(`md-lake seal`), 아카이버는 코드와 단위 틀 셋이 있었지만, 관측 스택(VictoriaMetrics, VictoriaLogs, vmalert 둘, Alertmanager, Grafana)이 이 PC에서 어떻게 돌고, 데이터를 어디 두고, 서비스마다 메모리를 얼마까지 쓰며, 어떤 규칙이 어떤 알림을 내는지는 어디에도 없었다.
- 규칙이 없으면 죽은 수집기, 찬 레이크 디스크, 실패한 L0 쓰기, 멈춘 봉인은 수집기 자신의 `[MD P1]`으로만 보이거나 아예 보이지 않는다. 죽은 수집기는 자기 알림을 보내지 못한다.
- 검사가 없으면 아무 프로세스도 내보내지 않는 지표 이름을 쓴 규칙이 영원히 조용하고, 수집기 단위가 편집 한 번에 `Restart=always`나 메모리 보호를 잃어도 아무도 모른다.
- 메모리 상한이 없거나 너무 크면 큰 일일 작업(아카이버, 감사)이나 연구 작업이 커널의 OOM 처리를 불러 수집기가 죽고 원본 데이터를 잃는다.

## 2. 일반적으로 만족해야 하는 성질

- 이 저장소는 아무것도 설치·시작하지 않는다. 파일과 그 검사만 있다.
- 검사는 도구가 읽는 것과 같은 것을 읽는다. 네트워크, 임시 파일, 시계 없이 CI에서 돈다.
- 수집기는 죽으면 곧 다시 뜨고, 메모리 압박에서 마지막에 죽는다.
- 큰 일일 작업은 한 번에 하나만 돌아 상한이 더해지지 않는다.
- 모든 리스너는 루프백에만 묶인다(Grafana만 Tailscale 주소). 비밀은 파일 경로로만 저장소에 있다.
- 설치하기 전에 없는 파일을 가리키는 단위, 없는 지표를 쓰는 규칙은 검사에서 실패한다.

## 3. 제약

| 사실 | 출처 |
|---|---|
| 아카이버 최대 RSS 2.57 GB(합성 호가 320만 행, `--threads 1`) | [md-archiver.md](../operations/md-archiver.md) §5 |
| 일일 작업 상한 4 GB, 봉인 1 GB, 스택 상한(1G, 1G, 200M, 200M, 100M, 512M), 수집기 1 GB(Bybit) | 설계 §8.5, TN 표 8-6 |
| 봉인은 매시 5분 | 설계 R2 S7, TN 표 8-6 |
| 보호 설정 `OOMScoreAdjust=-900`, `MemorySwapMax=0`, `MemoryMin` = `MemoryMax`의 절반, `IOWeight=1000`, 슬라이스 `CPUWeight=1000` | TN 표 8-6a, 설계 R4·R5 |
| cgroup v2: 자식의 실제 `memory.min`은 조상의 값으로 제한된다 | 커널 `Documentation/admin-guide/cgroup-v2.rst`, memory.min |
| `md-archiver`와 qdp 감사기는 `--venue` 하나만 받는다 | `crates/md-archiver/src/bin/md-archiver.rs`, qdp `docs/crypto/auditor.md` |
| 감사기에는 잠금 파일이 없고 한 상태 파일에 하나씩 돌려야 한다. 종료 코드는 0~4 | qdp `docs/crypto/auditor.md`, `audit.py` |
| `tools/ci/`, `.github/`, xtask의 `main.rs`·`registry.rs`·`layers.rs`, 그리고 `Cargo.toml`의 `[[test]]`·`auto*`·lint 표의 변경은 소유자가 머지한다 | `tools/ci/owner_review_paths.txt`, `tools/ci/owner_review.sh`, ADR-033 |
| `Cargo.lock`에 YAML crate도 `regex`도 없다. xtask의 의존성은 `serde`, `serde_json`, `toml` | `Cargo.lock`, `crates/xtask/Cargo.toml` |
| vmalert, Alertmanager, Grafana 프로비저닝은 JSON으로 쓴 YAML 파일을 읽는다 | 스펙 단계 실측(vmalert v1.157.0, Alertmanager 0.35.1, Grafana 13.3.2) |
| `systemd-analyze verify`는 `RestartSteps=`, `MemorySwapMax=0`, `OOMScoreAdjust=-900`, 슬라이스, 템플릿 인스턴스를 받고 모르는 키를 알린다 | systemd 259, 이 PC |

## 4. 필수 조건과 선택 조건

필수: 파일만(설치 없음), 설계 상한과 시각, 수집기의 재시작·감시·보호, 일일 작업 하나씩, 루프백 리스너, 비밀은 파일로만, 네트워크 없이 CI에서 도는 검사, 새 패키지 없음, 소유자 확인 경로를 건드리지 않음. 선택: 설치 스크립트, 규칙 파일의 CI 문법 검사(바이너리가 필요해 범위 밖).

## 5. 대안

- A. 계획의 이름 그대로 평범한 단위(`md-archiver.service`, `crypto-lake-audit.service`)에 거래소마다 `ExecStart=` 한 줄.
- B. 거래소마다 템플릿 인스턴스(`md-archiver@bybit.service`)와 일일 작업 잠금 하나.
- C. YAML 파일과 YAML crate.
- D. YAML 파일과 손으로 쓴 YAML 부분 파서.
- E. JSON 문법으로 쓴 YAML 파일(규칙·Alertmanager는 `*.json`, Grafana 프로비저닝은 요구대로 `.yaml` 이름)을 `serde_json`으로 읽음.
- F. 검사를 xtask 하위 명령(`deploy-check`)으로.
- G. 검사를 CI 단계로(vmalert·amtool 바이너리 내려받기).
- H. 검사를 xtask의 통합 시험(`crates/xtask/tests/deploy/`)으로.
- I. Grafana 알림. J. vmalert + Alertmanager.
- K. 지표를 긁어 오기(scrape). L. 프로세스가 보냄(push, 지금의 `obs-push`).

## 6. 비교

| | 장점 | 실패 모드 | 비용 |
|---|---|---|---|
| A | 계획의 이름 | 한 oneshot의 여러 `ExecStart=`는 첫 실패에서 멈춰 한 거래소가 다른 거래소를 건너뛰게 함 | 없음 |
| B | 거래소마다 독립 실패, 기존 아카이버 단위가 이미 템플릿 | 잠금이 없으면 상한이 더해짐 | 잠금 한 줄 |
| C | 주석 가능 | 새 패키지(공급망, 네트워크 받기) | `Cargo.lock` 변경 |
| D | 새 패키지 없음 | 도구가 거부하는 것을 받을 수 있음 | 파서 유지 |
| E | 도구와 같은 것을 읽음, 새 패키지 없음 | 주석 없음(요약 주석과 운영 문서로 대신) | 없음 |
| F | 운영자가 부르기 쉬움 | `main.rs` 변경 = 소유자 확인 | 머지가 멈춤 |
| G | 도구 자체의 검사 | CI에서 네트워크, `.github/` 변경 | 소유자 확인 |
| H | `cargo test --workspace`로 CI에서 돌고 소유자 경로를 건드리지 않음 | 문법은 도구가 아닌 손 검사 | 없음 |
| I | 한 도구 | 설계 Q18이 B(vmalert)를 고름 | — |
| K | 표준 | 수집기·워처·프로젝터는 이미 push | 수집기 코드 변경 |

## 7. 선택과 이유

B, E, H, J, L(스택 자신의 지표만 VictoriaMetrics가 루프백에서 긁는다).

- D0 배치: 모두 `deploy/observability/` 아래. 설치 지도 `install.toml`의 `[[file]] src, dst, mode`: 단위 → `/etc/systemd/system/<이름>`, `bin/` → `/usr/local/lib/hl-oms-obs/<이름>` 0755, 나머지 설정 → `/etc/hl-oms-obs/<같은 하위 경로>` 0644(경로는 판단). `vmalert/tests/`, `README.md`, `install.toml`, `venues.toml`, `metrics.toml`은 설치하지 않는다. 설치 줄(`install -D -m`)은 운영자가 이 지도대로 친다.
- D3 보호: `md-collector@.service`는 `Restart=always`, `RestartSec=2`, `StartLimitIntervalSec=0`, `WatchdogSec=30s`, `Type=notify`, `RestartPreventExitStatus=2`, `Slice=trading.slice`, `OOMScoreAdjust=-900`, `MemorySwapMax=0`, `MemoryMin=512M`(1G의 절반), `IOWeight=1000`. 슬라이스는 `CPUWeight=1000`이고 그 안 단위의 `MemoryMin` 합 이상을 예약한다(지금 512M). **재시작 지연을 늘리지 않는다**(`RestartSteps=`, `RestartMaxDelaySec=` 금지): systemd 문서는 재시작 횟수가 언제 0이 되는지 말하지 않고, `Restart=always`에서 0이 되지 않는다면 다섯 번째 충돌 뒤의 모든 재시작이 최대 지연만큼 원본 데이터를 잃는다(확인 안 함). 시작 횟수 제한을 끈 이유: 기본(10초에 5번)이면 2초 재시작이 수집기를 영영 멈출 수 있다.
- D4 일일 작업: 거래소마다 템플릿. 모든 일일 작업(아카이버, 감사, 뒤의 `oms-export`)은 `/usr/bin/flock /run/lock/hl-oms-daily.lock <명령>`으로 시작하고 `TimeoutStartSec=6h`(판단)로 묶인다. 잠금을 쓰는 단위는 모두 root로 돈다: `/run/lock`은 sticky이고 `fs.protected_regular`이 켜지면 다른 사용자가 root의 잠금 파일을 `O_CREAT`로 열지 못한다(`oms-export`도 root로 돌거나 잠금을 자기용 디렉터리로 옮긴다). 시각(UTC): 봉인 매시 5분(`MemoryMax=1G`, `TimeoutStartSec=6h`: 걸린 봉인만 끝냄, 다음 시간이 봉인 기록부터 이음), 아카이버 01:10(`MemoryMax=4G`, 8G에서 낮춤), 감사 02:30(`MemoryMax=4G`, 아카이버 뒤 `After=`). 모든 타이머 `Persistent=true`.
  - 감사 단위(`crypto-lake-audit@.service`)는 스크립트 `bin/crypto-lake-audit.sh <거래소>` 하나를 잠금 아래서 돌린다: (0) `venue`, `LAKE`, `STATE_DIRECTORY`, `QDP_CHECKOUT`, `UV`, `MD_LAKE` 중 빈 것이 있으면 종료 코드 2; (1) 어제부터 9일 전까지 날짜마다, 봉인된 L0 파일이 있고 verify 결과가 없거나, 확인 도장(stamp)이 없거나, 도장 뒤에 상태가 바뀐(ctime) 봉인 파일이 있으면 도장을 먼저 찍고 1초 쉰 뒤 `md-lake verify`를 돌리며, 성공하면 도장을 바꾸고 실패하면 옛 도장을 둔다; (2) qdp 체크아웃(`/opt/qdp-crypto`, KR 파이프라인의 체크아웃이 아님)에서 감사기를 돌린다(상태 파일은 `StateDirectory` 아래, 레이크 밖); (3) 종료 코드는 0단계의 2, 아니면 0이 아닌 감사기의 코드, 아니면 verify 실패의 5, 아니면 0. 9일인 이유: 일별 봉인의 되돌아보기가 `last_closed`의 날부터 7일이라 다음 날 감사까지 닿는 날짜가 모두 창 안에 든다. 경로 줄은 정확히 `ReadOnlyPaths=…/crypto/l0 …/crypto/l1`, `ReadWritePaths=…/crypto/l0/_verify …/crypto/audit`이다(verify는 `_verify`에, 감사기는 `audit`에만 쓴다).
- D6 검사: xtask의 통합 시험 `crates/xtask/tests/deploy/`(Cargo가 `[[test]]` 없이 찾는다). `deploy/`의 모든 파일과 고정된 qdp 지표 목록, 기준이 되는 소스 파일을 한 번 읽어 메모리의 나무로 만들고, 순수 함수 `check`가 발견 목록을 돌려준다. 시험은 실제 나무에서 발견이 없음을, 파일 하나를 메모리에서 바꾼 나무에서 그 규칙만 나옴을 확인한다. 하위 명령이나 CI 단계가 아닌 이유: `main.rs`, `registry.rs`, `.github/`는 소유자 확인 경로다. 주장(claim)은 두지 않는다: 주장은 검증 묶음이 필요하고 묶음 목록은 `registry.rs`에 있다. 검사는 어차피 `cargo test --workspace`로 CI에서 돈다.
- D7 지표 목록: 규칙이 쓰는 모든 지표 이름은 목록(`metrics.toml`의 행마다 그 이름이 문자 그대로 있는 소스 파일, 고정된 qdp `contracts/lake/xrepo/metrics.json`, 규칙 파일의 기록 규칙 이름)에 있어야 한다. 한 이름에 쓰는 쪽은 하나다.
- D8 비밀과 dead-man: Alertmanager의 텔레그램 토큰과 채팅 ID는 `/etc/hl-oms/` 아래 파일 경로로만 둔다(인라인 값은 검사가 거부). Watchdog 알림은 연동이 없는 `deadman` 수신자로 간다(바깥 dead-man 서비스는 소유자가 정함).
- D9 Grafana: `http_addr`는 자리 표시 `192.0.2.1`(RFC 5737 TEST-NET-1)이다. 이 PC가 묶을 수 없는 주소라 운영자가 Tailscale 주소를 쓰기 전까지 Grafana는 시작을 거부한다. 모든 인터페이스에 여는 것보다 거부를 골랐다.
- 하나 더: `xtask::digest::source_digest`는 `crates/**/*.rs`, `crates/**/Cargo.toml`, `Cargo.lock`만 센다. 배치 파일만 바뀌면 `evidence/mutants.json`의 다이제스트는 그대로다. 그대로 둔다: 다이제스트는 `crates/oms-node/build.rs`가 바이너리에 넣는 값과 같아야 하고, 배치 파일은 바이너리의 일부가 아니다.

## 8. 포기한 장점, 남은 위험, 소유자가 정할 것

- 포기: JSON에는 주석이 없다. 알림마다 `annotations.summary`와 운영 문서가 대신한다. 검사는 문법을 손으로 본다. 도구의 문법 검사(`vmalert -dryRun`, `amtool check-config`, `systemd-analyze verify`)는 바이너리가 있는 곳에서 사람이 돌린다.
- 위험: 재시작 지연이 없어서 시작하자마자 죽는 수집기는 약 2초마다 거래소에 다시 접속한다. Bybit의 IP당 연결 제한은 확인하지 않았다.
- 위험: 아카이버 4G는 토픽 하루 약 500만 행까지다(실측 1.6배, 판단). 더 바쁜 날은 상한에서 죽고 뒤 날짜를 모두 막는다(L0는 남는다).
- 위험: 우리 서비스의 메모리와 단위 상태를 내보내는 것이 없다. 실패한 아카이버 실행, 감사의 날짜별 쓰기 오류(종료 코드 3), verify 실패(5)는 `systemctl --failed`로만 보인다.
- 소유자가 정할 것(기본값으로 진행): (1) 바깥 dead-man(기본: 연동 없는 수신자), (2) 서비스 신원(기본: 스택은 `hlobs`, 시세 단위는 root 그대로), (3) Grafana 주소(기본: 거부하는 자리 표시), (4) 감사용 qdp 체크아웃(기본: `/opt/qdp-crypto`), (5) P2·P3 임계값(기본: 스펙의 값, 주간 소음 검토), (6) 수집기 충돌 반복(기본: 2초, 지연 늘림 없음, 시작 횟수 제한 없음), (7) 아카이버 상한(기본: 4G).

## 9. 근거

- 시험: `crates/xtask/tests/deploy/`의 `the_deploy_tree_passes`, `tree::every_file_parses_by_its_kind`, `ini::the_ini_reader_keeps_repeats_and_joins_lines`, `units::*`(수집기의 재시작·감시·보호, 상한, 타이머, 슬라이스, 잠금, 리스너, 저장 위치, 플래그, 거래소, 설치 지도, 신원).
- 변이: M324~M332(xtask, `cargo run -p xtask -- mutants --only M3xx`).
- 도구: `systemd-analyze verify`(systemd 259)를 단위 사본에 돌렸다(바이너리 경로만 바꿈).

## 10. 재검토 조건

- 재시작 횟수가 언제 0이 되는지 systemd가 문서로 밝히거나 측정했을 때(지연 늘림을 다시 본다).
- 아카이버가 4G를 넘는 날이 나오거나 `lake_l1_build_lag_days`가 생길 때.
- 우리 서비스의 메모리·단위 상태 내보내기가 생길 때.
- 둘째 거래소를 더할 때(상한, 슬라이스 예약, 생존 규칙).
