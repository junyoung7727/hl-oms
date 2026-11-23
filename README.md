# OrderManagementSystem

단일 계좌·다중 전략용 주문 관리 시스템(OMS). Rust. 첫 대상 거래소는 Hyperliquid perpetual(testnet 먼저)이고 코어는 거래소 중립이다.
LMAX의 원칙 가운데 **단일 writer의 결정적 코어 + 입력 저널**을 따르고, 복제 노드·Disruptor·busy-spin은 채택하지 않았다([ADR-001](docs/adr/ADR-001-lmax-scope.md)).

## 지금 상태를 한 문단으로

가상 거래소에 대해서는 접수·위험 예약·전송·체결·취소·종결·재시작 복구·대조·과부하 처리가 구현돼 있다. 무엇이 어떤 시험으로 확인됐는지는 주장별로 [generated/claims.md](docs/generated/claims.md)에 있고, 그 통과 여부는 보관된 결과 파일에서 계산한다.
**증거금 규칙이 구현돼 있다**(2026-11-16, [ADR-018](docs/adr/ADR-018-margin-rules.md), [계약 C-MARGIN](docs/contracts/margin.md)). 증거금 정책(`policy.margin`)을 설정하면: 일반 주문이 계좌 노출(포지션과 모든 미체결 주문)을 계좌 가치의 2배 위로 올리면 거절한다. 일 손실이 5 %를 넘거나 격리 포지션이 청산에 가까워지면 계좌가 감소 전용 상태가 되어 일반 주문을 거절하고 걸린 일반 주문을 취소하며, 이 상태는 운영자만 푼다. 증거금 보고가 끊기면 일반 주문을 거절하고(30초), 걸린 일반 주문을 취소한다(60초). 정책이 없으면 전과 같고, mainnet은 정책 없이 시작하지 않는다. OMS는 포지션을 스스로 닫지 않고, 손실이 5 %에서 멈춘다고 보장하지 않는다.
**증거금 규칙은 실제 거래소에서 한 번도 실행하지 않았다.** 가상 거래소와 시뮬레이션으로만 시험했다. 규칙이 읽는 거래소 숫자의 뜻은 문서에서 읽은 가정 열한 가지(T1~T11)이고 하나도 확인하지 않았다. testnet 예행(실주문으로 감소 전용 상태·보고 끊김 취소·강제 종료 훈련)과 T1~T11의 확인이 mainnet 전에 필요하고, 사용자의 승인이 필요하다(U-01, U-09). 스펙과 다르게 한 것과 보장하지 않는 것은 [검증 문서 §11](docs/verification/README.md)에 있다. 코드 검토(2026-11-16)에서 나온 것은 고쳤고(대조기가 모르는 거래소 설정을 1초마다 되묻던 것 포함), 미해결 사항은 지금 없다.
구현과 분리된 검토에서 결함 20건이 나와 고쳤다([gap-review](docs/verification/gap-review.md)). 사람의 검토는 받지 않았다.
**실제 거래소에는 주문을 한 번도 보내지 않았다.** Hyperliquid 게이트웨이는 서명을 SDK 시험 벡터와 대조했고, 2026-10-02에 testnet에 읽기 전용 조회만 했다(증거금 조회는 그때 없었고 그 뒤로도 보내지 않았다).
**운영자만 푸는 보류가 구현돼 있다**(U18b1, 2026-11-21, [ADR-030](docs/adr/ADR-030-kill-switch-holds.md), [계약 C-HOLD](docs/contracts/holds.md)). 킬 래치는 감소 전용을 포함한 모든 주문을 거절하고, 워처가 확정한 불일치와 거래소가 스스로 바꾼 포지션은 일반 주문을 거절한다. 셋 다 `omsctl clear-hold`로만 풀린다. 킬은 그 프로세스의 게이트웨이가 킬에 답한 뒤에만, 그리고 OMS 장부에 미체결 주문이 없을 때만 풀린다(`kill_orders_open`이면 먼저 `omsctl cancel-all`). 가상 거래소와 Hyperliquid는 "일괄 취소 없음"(`unsupported`)으로 답한다. 킬 마커(`[kill] marker`)가 있으면 시작이 래치를 걸고, 해제가 내구화된 뒤 노드가 마커를 지운다. mainnet은 `[kill]` 없이 시작하지 않는다. **Bybit 킬 스위치가 구현돼 있다**(U18b2, 2026-11-22, [계약 C-KILL](docs/contracts/kill.md)): 개인 스트림을 마지막 생존 증거 뒤 10초 잃으면 래치를 걸고, 래치가 디스크에 있은 뒤 1초마다 일괄 취소하며 거래소의 미체결을 세어 0건을 확인하고 30초 지켜본다(일괄 취소는 어느 1초에도 둘 이하, 루프 중 새 주문은 나가지 않음, P1은 10분에 12개 이하이고 포지션 묶음이 붙음). 확인 뒤 새 계기가 오면 다시 무장한다. loopback 가짜 Bybit와 수동 시계로만 시험했고 실제 거래소에서는 돌려 본 적이 없다. **전략 연결 감시가 구현돼 있다**(U18b3, 2026-11-22, [계약 C-LIVE](docs/contracts/liveness.md)): `[liveness]`에 적힌 전략의 마지막 거래 연결이 닫히거나 정한 시간(예 5초) 동안 요청(`ping` 포함)이 없으면, 그 전략의 걸린 일반 주문 취소를 요청하고 그 전략의 일반 주문만 거절한다(감소 전용 주문과 다른 전략은 그대로). 그 전략이 다시 `hello`하면 풀리고, 그 답은 복구가 내구화된 뒤에 간다. mainnet은 사용 중인 전략이 모두 적혀야 시작한다. 가상 거래소에 붙인 노드 프로세스로만 시험했다. **모니터링 게이트가 구현돼 있다**(U18b4, 2026-11-22, [계약 C-LIVE](docs/contracts/liveness.md) 2부): `[core.policy.monitor]`가 있으면 워처의 하트비트(`monitor.sock`)를 저널에 남기고 코어가 판단한다. 하트비트가 30초 없거나 워처의 W1·W2가 10초 넘게 성공하지 못했으면 일반 주문을 거절하고, 60초 더 가면 걸린 일반 주문 취소를 요청한다. 모든 시작은 워처의 첫 하트비트까지 이 상태다. mainnet은 게이트 없이 시작하지 않는다. 워처(U19a)는 아직 없어 시험의 클라이언트가 하트비트를 보냈고, 실제 거래소에서는 돌려 본 적이 없다. **독립 워처를 만드는 중이다**(U19a1, 2026-11-23, [ADR-031](docs/adr/ADR-031-watcher.md), [계약 C-WATCH](docs/contracts/watcher.md)): 별도 crate `watcher`가 OMS 코드 없이 문서(ADR-011, C-TAIL)만으로 쓴 저널 읽기를 갖췄고(잠금 없음, 내구화 번호까지, 손상과 모르는 이벤트에서 멈춤), `tools/ci/watcher_independent.sh`가 CI에서 워처가 OMS·시세 crate에 의존하지 않음을 매번 확인한다. U19a2(2026-11-23)에서 같은 방식으로 저널 상태(열린 주문, 체결, 포지션, 보류, 체결 바닥)를 세우고, 그것이 이벤트 51개를 모두 쓰는 코어 시나리오에서 기록마다 코어의 집합과 같음을 적합성 시험이 확인한다. 시작마다 저널을 처음부터 다시 읽는데, 기록 100만 건에 약 1.3초다. U19a3(2026-11-23)에서 OMS 코드 없이 자기 Bybit 서명과 클라이언트를 갖췄다: 키가 거절되면(10010 등) 재시작까지 Bybit에 아무것도 보내지 않고, HTTP 403이면 600초 멈추며, 어느 10초에도 요청을 60개 넘게 보내지 않는다. 자기 지표 내보내기도 있다. loopback 가짜로만 시험했고 실제 Bybit에는 보낸 적이 없다. U19a4(2026-11-23)에서 설정 파일(모르는 키는 거부, mainnet은 `mainnet_confirmed = true`가 있어야)과 키 파일(워처를 돌리는 사용자의 권한 0600·0400 일반 파일, 링크 아님, OMS 데이터 디렉터리 밖)을 읽는다([운영 문서](docs/operations/watcher.md)). U19a4b(2026-11-23)에서 시작 확인을 갖췄다: 마스터 키, 다른 하위 계정의 키, IP에 묶이지 않은 키, USDT로 정산되지 않는 기호면 시작하지 않고, 키가 거절되면 요청 없이 떠 있으며, 키 만료까지 남은 날과 공인 IP가 키의 IP 목록에 있는지를 내보내고 60초마다 다시 본다. U19a5(2026-11-23)에서 검사 엔진과 W1(열린 주문)을 갖췄다: 라운드마다 거래소를 먼저 읽고 그 뒤의 저널과 비교하며, 차이는 두 라운드 연속으로 보이고 저널 항목이 2초 넘게 지났을 때만 확정한다(한 번 본 차이나 막 기록된 항목은 울리지 않는다). 프로세스 안의 모델 거래소와 수동 시계로만 시험했다. U19a6(2026-11-23)에서 W2(포지션)를 갖췄다: 기호마다 거래소 크기와 저널의 체결·변경·조정 합을 비교하고, 그 종목의 저널 변경이나 거래소 변경이 2초보다 어리면 미룬다(초당 5번의 체결이 60초 이어져도 울리지 않고, 체결이 멈춘 뒤의 차이는 3초 안에 확정한다). U19a7(2026-11-23)에서 워처가 돈다(`watcher --config`): OMS의 `/api/status`를 기다려 저널을 처음부터 읽은 뒤 W1과 W2를 돌리고, 확정된 차이를 `admin.sock`에 보내 일반 주문을 보류시키며(OMS가 `"held"`로 답할 때까지 빚으로 들고, 운영자가 풀었는데 차이가 남아 있으면 다시 보낸다), 5초마다 `monitor.sock`에 검사별 마지막 성공 시각을 보낸다. loopback 가짜로만 시험했고 실제 OMS 프로세스나 실제 Bybit에 붙여 돌려 본 적이 없다. U19a8(2026-11-23)에서 W4(체결)를 갖췄다: 5초마다 마지막으로 맞춘 체결 시각 10초 전부터 거래소 체결 목록을 읽어(라운드마다 10쪽까지, 더 길면 다음 라운드가 이어 읽음) 체결은 저널의 체결과, ADL·청산 인수·인도·정산은 저널의 거래소 변경과 키로 비교하고, 펀딩은 비교하지 않으며, 모르는 체결 형은 바로 확정한다. 보류 메모가 이름한 W4 차이는 운영자가 푼 뒤 다시 보내지 않는다. 모델 거래소와 수동 시계로만 시험했다. U19a9(2026-11-23)에서 W3(지갑과 증거금)을 갖춰 네 검사가 모두 돈다: 2초마다 통합 계좌의 유지증거금률과 오늘의 일 손실을 OMS와 같은 공식으로 보고, 두 라운드 연속 한도를 넘으면 확정한다. W2는 W4가 맞춘 가장 새 체결 시각도 거래소의 변경 시각으로 쓴다. 대조 요청은 5초에 체결 30개면 초당 2.7이지만 100개면 약 3.2~3.3으로 완료 기준을 넘는다(체결을 놓치지 않는 W4 규칙을 지켰다, 소유자 질문). 열린 주문 500개에서도 W1·W2는 10초 안에 성공한다. 모델 거래소와 수동 시계로만 쟀다. U19b1a(2026-11-24, [ADR-032](docs/adr/ADR-032-watcher-kill-and-alerts.md))에서 워처 자신의 텔레그램 알림 `[WATCH P1]`의 규칙을 갖췄다: `[telegram]`의 토큰 파일(권한 0600·0400, 링크 아님, git 저장소 밖)을 시작 확인 전에 읽고, mainnet은 그것 없이 시작하지 않는다. 메시지 분할(3,500 UTF-16), 보관 50개, 응답의 결과(400·413은 버림, 나머지는 다시 보냄), 5분 반복 규칙은 단위 시험으로만 확인했고 아직 아무것도 보내지 않는다. U19b1b(2026-11-24)에서 송신 태스크를 갖췄다: DNS로 닿지 않으면 텔레그램의 고정 주소(149.154.167.220)로 가고, 남은 부분은 5초 뒤 다시 보내며, 종료 때 5초 안에 마지막으로 보낸다. U19b1c(2026-11-24)에서 워처가 `[WATCH P1]`을 보낸다: 차이를 확정하면 하나(검사마다 60초에 하나, 그 사이 것은 모아서; 운영자가 푼 뒤의 다시 보내기는 보내지 않음), 키가 거절되거나(10010 등) 공인 IP가 키의 목록 밖이면 시작과 5분마다, 시작 확인이 시작을 거부하면 종료 전에 하나. loopback 가짜 텔레그램으로만 시험했다.
**전원 장애는 시험하지 않았다.** 누적 체결량 한도는 없다. 거래 안전 알림(차단, 감소 전용, 불일치, 치명 종료)은 노드가 내구화된 상태에서 텔레그램으로 직접 보낸다([ADR-029](docs/adr/ADR-029-oms-direct-alerts.md), [계약 C-ALERT](docs/contracts/alerts.md)). mainnet은 `[alert]` 없이 시작하지 않는다. 실제 텔레그램에는 보낸 적이 없다(시험은 loopback 가짜만).
현재 소스(`762a4acf55af90fb`)의 수용 시험은 testnet 조회 묶음(`hl-live`)을 빼고 실행했고, 실행한 326건은 모두 통과했다. 같은 소스에서 변이 점검 122건을 시험이 모두 잡았고, 성능을 다시 쟀다. 주장 68건 가운데 통과 67건, 미실행 1건이다([검증 문서](docs/verification/README.md)).
주장 V-HL-3(testnet 읽기 전용 조회)은 "미실행"이다: 증거금 규칙의 작업은 실제 거래소에 어떤 요청도 보내지 않기로 했기 때문이다. 실패도 통과도 아니다.
지연 수용 기준은 저널 장치에 달려 있다: 970 EVO Plus 2TB에서 만족했고(초당 200건 p99 8.7 ms, 2026-11-16), 거의 찬 500GB SSD에서는 넘었다([성능 문서](docs/performance/README.md)). 이 부하 측정의 설정에는 증거금 정책이 없다. 정책을 켠 신규 주문 한 건의 판단 시간은 열린 주문 약 500건에서 중앙값 66 µs, p99 90 µs다(정책 없음 27 µs, 33 µs).

## 어디서부터 읽나

| 알고 싶은 것 | 문서 |
|---|---|
| 무엇을 만들고 무엇을 만들지 않았나, 수용 기준 | [docs/01-scope.md](docs/01-scope.md) |
| 무엇을 검토했나(질문 103개와 각각의 답·미해결, 출처 요소별 대응) | [docs/02-review-criteria.md](docs/02-review-criteria.md) → [generated/considerations.md](docs/generated/considerations.md), [generated/coverage.md](docs/generated/coverage.md) |
| 구조: 스레드, 데이터 흐름, crate 경계 | [docs/03-architecture.md](docs/03-architecture.md) |
| 항상 성립해야 하는 것 | [docs/04-invariants.md](docs/04-invariants.md) |
| 왜 이렇게 만들었나(대안 비교 포함) | [docs/adr/](docs/adr/README.md) |
| 정확히 어떻게 동작하나(중단·중복·순서 역전·복구에서) | [docs/contracts/](docs/contracts/README.md) |
| 정상·장애 시나리오를 따라가며 | [docs/scenarios.md](docs/scenarios.md) |
| 무엇으로 확인했고 무엇을 확인하지 않았나 | [docs/verification/](docs/verification/README.md) → [generated/claims.md](docs/generated/claims.md) |
| 성능 측정 조건과 결과 | [docs/performance/](docs/performance/README.md) |
| 배포, 복구, 버전 올리기, 상황별 절차 | [docs/operations/runbook.md](docs/operations/runbook.md) |
| 저널을 VictoriaMetrics·VictoriaLogs로 내보내는 프로젝터 | [docs/operations/oms-projector.md](docs/operations/oms-projector.md) |
| 저널을 레이크 거래 테이블(Parquet)로 내보내기 | [docs/operations/oms-export.md](docs/operations/oms-export.md) |
| 독립 워처의 설정, 키 파일, 시작 확인, 실행과 로그 줄 | [docs/operations/watcher.md](docs/operations/watcher.md) |
| 외부에 기대는 가정 | [generated/assumptions.md](docs/generated/assumptions.md) |
| 사용자가 정해야 하는 것 | [generated/user-decisions.md](docs/generated/user-decisions.md) |
| 질문 → 결정 → 계약 → 코드 → 시험의 연결표 | [generated/traceability.md](docs/generated/traceability.md) |
| 단계별 진행 기록, 구현과 분리된 검토 | [docs/progress/](docs/progress/README.md), [verification/gap-review.md](docs/verification/gap-review.md) |
| 원래의 지시문 | [docs/00-brief.md](docs/00-brief.md) |

같은 내용을 눌러 가며 보려면 대시보드를 쓴다(아래).

## 빌드와 실행

필요한 것: Rust 1.96 이상(edition 2024), Linux.

```sh
cargo build --release
T=$(cargo metadata --format-version 1 --no-deps | python3 -c 'import json,sys; print(json.load(sys.stdin)["target_directory"])')

# 문서와 검증 결과만 보는 화면 (노드 없음)
$T/release/oms-node dashboard            # http://127.0.0.1:8188/

# 가상 거래소에 붙여 실행
mkdir -p data/sim
$T/release/simvenue data/sim/venue.sock &
$T/release/oms-node run --config config/sim.toml &
$T/release/omsctl --socket data/sim/oms.sock --strategy demo new --key k1 --instrument BTC --side buy --qty 0.001 --px 70000 --ttl-ms 20000
$T/release/omsctl --socket data/sim/admin.sock status
```

Hyperliquid testnet 설정 예: [config/hyperliquid-testnet.example.toml](config/hyperliquid-testnet.example.toml). 처음 쓸 때의 순서는 [운영 절차 §10](docs/operations/runbook.md)에 있다.
비밀 키는 환경 변수로만 받는다. 자금을 가진 마스터 지갑의 키는 이 시스템에 넣지 않는다.

## 수용 시험

```sh
cargo run --release -p xtask -- verify      # 전체 시험 묶음. 결과: evidence/latest/  (--extended <디렉터리>, --live-testnet)
cargo run --release -p xtask -- mutants     # 고의로 고장 낸 코드를 시험이 잡는지
cargo run --release -p xtask -- perf --label nvme-final --seconds 60 --dir <실제 디스크의 디렉터리>
cargo run --release -p xtask -- core-bench   # 코어의 판단 시간. 증거금 정책이 없는 경우와 있는 경우
cargo run --release -p xtask -- recovery-time --dir <실제 디스크의 디렉터리>
cargo run --release -p xtask -- docs        # 결과를 주장과 묶어 docs/generated/ 와 dashboard/data/ 를 다시 만든다
```

결과에는 소스 다이제스트가 들어간다. 소스를 고친 뒤 다시 실행하지 않으면 문서와 화면이 그 결과를 "이전 버전 결과"로 표시한다.

## 대시보드

읽기 전용이다. 중지·재개·취소 같은 변경은 운영 소켓으로만 한다.

| 화면 | 내용 |
|---|---|
| 고려사항·결정 | 질문 103개, 설계 결정 17건, 외부 가정, 사용자 결정, 출처. 서로 연결돼 있다 |
| 동작 탐색 | 정상·오류 경로의 이벤트 순서. 이벤트 → 계약 → 설계 결정 → 코드 → 시험 |
| 주문 추적 | 실행 중인 노드의 주문별 저널 기록: 입력, 이벤트, 응답, 게이트웨이 명령, 단계별 지연 |
| 검증 현황 | 주장별 통과·실패·미실행·이전 버전 결과, 변이 점검, 성능 측정 |
| 운영 상태 | 차단 사유, 결과 불명 주문, 불일치, 저널, 큐, 장부 |

값마다 관측·추론·기대·미확인을 구분해 표시하고, 실행 종류(노드 없음 / 가상 거래소 / 실제 거래소)를 상단에 고정 표시한다.

## 저장소 구조

| 경로 | 내용 | 줄 수(시험 포함) |
|---|---|---|
| `crates/oms-core` | 결정적 코어. I/O·시계·스레드 없음. 의존성은 `serde`, `serde_json`뿐 | 7,200 |
| `crates/oms-journal` | 저널 세그먼트와 스냅샷 파일 | 1,600 |
| `crates/oms-sim` | 결정적 시뮬레이션과 가상 거래소 모델 | 1,700 |
| `crates/oms-api` | 로컬 프로토콜 타입, 클라이언트, `omsctl` | 500 |
| `crates/oms-node` | 실행 계층: 큐, 커밋 단계, 복구, 소켓 서버, HTTP, 게이트웨이(가상, Hyperliquid), `simvenue` | 7,300 |
| `crates/watcher` | 독립 워처(U19a): OMS 코드 없이 문서로만 쓴 저널 읽기와 저널 상태, 자기 Bybit 서명·클라이언트와 지표 내보내기, 설정과 키 파일 읽기, 시작 확인과 IP 재확인, 검사 엔진과 W1(열린 주문), W2(포지션), W3(지갑과 증거금), W4(체결), OMS 연결(`/api/status`, `admin.sock` 다시 보내기, `monitor.sock` 하트비트)과 실행, 바이너리 `watcher --config`, 예산 측정, 직접 알림의 토큰 파일과 전달 규칙(U19b1a, 아직 보내지 않음). 의존성은 레지스트리 crate뿐 | 16,600 |
| `crates/watcher-conformance` | 워처와 OMS 코드를 함께 링크하는 유일한 곳: 적합성 시험 | 900 |
| `crates/xtask` | 수용 시험 실행기, 변이 점검, 부하 생성기, 문서 생성기 | 2,100 |
| `registry/` | 검토 항목, 결정, 계약, 주장, 가정, 사용자 결정의 원본(TOML) | |
| `docs/` | 문서. `docs/generated/`는 registry와 결과 파일에서 만든다 | |
| `dashboard/` | 화면(정적 파일)과 생성된 자료 | |
| `evidence/` | 시험·변이·성능 결과 | |
| `config/` | 설정 예 | |

작업 공간 전체에 `unsafe_code = "forbid"`가 걸려 있다.
