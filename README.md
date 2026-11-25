# OrderManagementSystem

단일 계좌·다중 전략용 주문 관리 시스템(OMS). Rust. 거래소는 Bybit USDT 무기한(먼저 테스트넷)이고, 코어는 거래소 중립이다. Hyperliquid 게이트웨이와 가상 거래소도 있다.
LMAX의 원칙 가운데 **단일 writer의 결정적 코어 + 입력 저널**을 따르고, 복제 노드·Disruptor·busy-spin은 채택하지 않았다([ADR-001](docs/adr/ADR-001-lmax-scope.md)).

## 무엇을 읽나

- 상황별로 어떻게 동작하나(응답이 안 오는 주문, 프로세스가 죽음, 연결이 끊김, 킬 …): [docs/explanation.md](docs/explanation.md)
- 요구 R01~R15가 무엇으로 확인됐고 무엇이 미실행인가: [docs/generated/requirements.md](docs/generated/requirements.md)
- 왜 이렇게 만들었나: [docs/adr/](docs/adr/README.md)
- 운영 절차: [docs/operations/runbook.md](docs/operations/runbook.md)
- 어디에 무엇이 있나: [AGENTS.md](AGENTS.md). 작업 규칙: [CONTRIBUTING.md](CONTRIBUTING.md)

진행 상황은 PR에만 적는다.

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
| 고려사항·결정 | 검토 항목, 설계 결정, 외부 가정, 사용자 결정, 출처. 서로 연결돼 있다 |
| 동작 탐색 | 정상·오류 경로의 이벤트 순서. 이벤트 → 계약 → 설계 결정 → 코드 → 시험 |
| 주문 추적 | 실행 중인 노드의 주문별 저널 기록: 입력, 이벤트, 응답, 게이트웨이 명령, 단계별 지연 |
| 검증 현황 | 주장별 통과·실패·미실행(현재 소스의 결과만), 변이 점검, 성능 측정 |
| 운영 상태 | 차단 사유, 결과 불명 주문, 불일치, 저널, 큐, 장부 |

값마다 관측·추론·기대·미확인을 구분해 표시하고, 실행 종류(노드 없음 / 가상 거래소 / 실제 거래소)를 상단에 고정 표시한다.

작업 공간 전체에 `unsafe_code = "forbid"`가 걸려 있다.
