# OrderManagementSystem

단일 계좌·다중 전략용 주문 관리 시스템(OMS). Rust. 첫 대상 거래소는 Hyperliquid perpetual(testnet 먼저)이고 코어는 거래소 중립이다.
LMAX의 원칙 가운데 **단일 writer의 결정적 코어 + 입력 저널**을 따르고, 복제 노드·Disruptor·busy-spin은 채택하지 않았다([ADR-001](docs/adr/ADR-001-lmax-scope.md)).

## 지금 상태를 한 문단으로

가상 거래소에 대해서는 접수·위험 예약·전송·체결·취소·종결·재시작 복구·대조·과부하 처리가 구현돼 있다. 무엇이 어떤 시험으로 확인됐는지는 주장별로 [generated/claims.md](docs/generated/claims.md)에 있고, 그 통과 여부는 보관된 결과 파일에서 계산한다.
구현과 분리된 검토에서 결함 20건이 나와 고쳤다([gap-review](docs/verification/gap-review.md)). 사람의 검토는 받지 않았다.
**실제 거래소에는 주문을 한 번도 보내지 않았다.** Hyperliquid 게이트웨이는 서명을 SDK 시험 벡터와 대조했고 testnet에 읽기 전용 조회만 했다.
**전원 장애는 시험하지 않았다.** 금액·증거금 한도, 누적 체결량 한도, 알림은 없다.
현재 소스(`0a86a80a92f2313c`)의 수용 시험은 testnet 조회 묶음(`hl-live`)을 빼고 실행했고, 실행한 220건은 모두 통과했다. 변이 점검은 31건을 모두 잡았다. 주장 59건 가운데 통과 53건, 미실행 1건(V-HL-3: testnet에 접속하지 않았다), 이전 버전 결과 5건(V-PERF-1~5: 성능은 다시 측정하지 않았고 결과는 소스 `c2addecd59bc4a5a`의 것이다)이다. 소스 `c2addecd59bc4a5a`에서는 그때의 주장 56건이 모두 통과였다([검증 문서](docs/verification/README.md)).
지연 수용 기준은 저널 장치에 달려 있다: 970 EVO Plus 2TB에서 만족했고(초당 200건 p99 9.1 ms), 거의 찬 500GB SSD에서는 넘었다([성능 문서](docs/performance/README.md)).

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
| `crates/xtask` | 수용 시험 실행기, 변이 점검, 부하 생성기, 문서 생성기 | 2,100 |
| `registry/` | 검토 항목, 결정, 계약, 주장, 가정, 사용자 결정의 원본(TOML) | |
| `docs/` | 문서. `docs/generated/`는 registry와 결과 파일에서 만든다 | |
| `dashboard/` | 화면(정적 파일)과 생성된 자료 | |
| `evidence/` | 시험·변이·성능 결과 | |
| `config/` | 설정 예 | |

작업 공간 전체에 `unsafe_code = "forbid"`가 걸려 있다.
