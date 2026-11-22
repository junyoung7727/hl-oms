# 동작 계약

계약은 "중요한 상태 전이와 경계" 단위로 쓴다. 각 계약은 입력과 전제, 읽는 상태, 상태 소유자, 변경 내용, 확정 시점, 외부 효과, 자원 상한을 적고,
중복 입력·순서 변경·시간 초과·취소·프로세스 중단·재실행에서 동작이 어떻게 달라지는지를 표로 적는다.

| 계약 | 문서 | 답하는 질문 |
|---|---|---|
| C-COMMIT | [commit.md](commit.md) | 여기서 중단되면 무엇이 남는가. 누가 언제 이 변경을 볼 수 있는가 |
| C-NEW, C-SEND, C-DOUBT, C-FILL, C-CANCEL, C-TERM | [order-lifecycle.md](order-lifecycle.md) | 주문은 어떤 증거로 어떤 단계가 되는가. 시스템은 외부 결과를 실제로 아는가 |
| C-IDEM | [idempotency.md](idempotency.md) | 같은 입력이 두 번 오면 어떻게 되는가 |
| C-JOURNAL, C-SNAP, C-BOOT | [recovery.md](recovery.md) | 복구 후 무엇을 다시 실행하며 무엇을 반복하면 안 되는가 |
| C-LINK, C-RECON, C-MISMATCH | [reconciliation.md](reconciliation.md) | 끊긴 동안의 변화를 어떻게 알아내는가. 차이가 나면 어떻게 하는가 |
| C-OVERLOAD | [overload.md](overload.md) | 감당할 수 없는 양이 오면 무엇이 거절되고 무엇이 기다리는가 |
| C-GW | [gateway.md](gateway.md) | 거래소와 통신하는 부분이 지켜야 하는 것 |
| C-API, C-OPS | [api.md](api.md) | 전략과 운영자가 보는 인터페이스 |
| C-TAIL | [tail.md](tail.md) | OMS 밖의 프로그램이 저널을 어떻게 읽는가. 기록의 필드는 무엇이고 어떤 상태를 바꾸는가 |
| C-MARGIN(구현됨. 실제 거래소 미확인) | [margin.md](margin.md) | 계좌 가치에 비해 얼마나 큰 주문까지 받는가. 손실이 커지면 무엇이 멈추고 누가 푸는가 |
| C-ALERT(구현됨. 실제 텔레그램 미확인) | [alerts.md](alerts.md) | 무엇이 언제 사람에게 알려지는가. 내구화되지 않은 것이 알려질 수 있는가. 무엇을 놓칠 수 있는가 |
| C-HOLD(구현됨, U18b1. 실제 거래소 미확인) | [holds.md](holds.md) | 사람만 푸는 보류(킬 래치, 워처 불일치, 거래소 변경)는 무엇을 막고 누가 언제 푸는가. 킬 마커가 있으면 시작이 어떻게 되는가 |
| C-KILL(구현됨, U18b2. 실제 거래소 미확인) | [kill.md](kill.md) | Bybit에서 개인 스트림을 잃으면 언제 킬이 걸리고, 걸린 주문을 어떻게 0건까지 취소하고 확인하는가. 사람은 무엇을 언제 받는가 |
| C-LIVE(1부 U18b3, 2부 U18b4 구현됨. 실제 거래소 미확인) | [liveness.md](liveness.md) | 전략 프로세스가 죽거나 멈추면 그 전략의 걸린 주문은 어떻게 되고, 무엇이 언제 다시 풀리는가. 워처가 없어지거나 검사가 실패하면 언제 주문이 막히고 언제 걸린 주문이 취소되는가 |
| C-WATCH(1~3부 구현됨, U19a1~U19a3. 검사 W1~W4는 아직 없음) | [watcher.md](watcher.md) | 워처는 OMS 코드 없이 저널을 어떻게 읽고, 그 독립성은 무엇으로 매번 확인하는가. 저널이 손상되거나 모르는 이벤트가 나오면 무엇을 하는가. 저널에서 열린 주문, 체결, 포지션을 어떻게 세우고, 그것이 코어와 같다는 것은 무엇으로 아는가. 자기 Bybit 클라이언트는 키가 거절되거나 IP가 막히면 무엇을 하고, 요청을 얼마나 보내는가 |
| — | [identifiers.md](identifiers.md) | 식별자의 범위와 수명 |

계약과 이벤트·코드·시험의 연결표는 [generated/traceability.md](../generated/traceability.md)에 있다.

## 생략한 것과 그 근거

내부 구현을 바꿔도 결과와 실패 의미와 자원 상한이 유지되는 부분은 계약에 쓰지 않았다. 대신 의존하는 보장을 적는다.

| 생략한 부분 | 의존하는 보장 | 그 보장의 출처 |
|---|---|---|
| `BTreeMap`·`VecDeque`의 내부 | 정렬된 순회, 삽입 순서 유지 | Rust 표준 라이브러리 문서 |
| `std::sync::Mutex`, `Condvar`, `RwLock`, `mpsc::sync_channel` | 상호 배제, 상한이 있는 대기, 송신단이 사라지면 수신이 끝남 | 표준 라이브러리 문서 |
| `tokio::sync::mpsc`, `oneshot`, `Semaphore` | 상한, 순서 유지, 닫힘 통지 | tokio 문서 |
| `serde_json` | 정수와 문자열의 왕복 보존, 구조체 필드 순서대로 직렬화 | serde_json 문서. 상태 다이제스트가 직렬화 순서에 기댄다 |
| `crc32fast` | CRC-32(IEEE) 계산 | 기대값을 Python `zlib.crc32`로 대조 |
| 파일 시스템 | [ADR-011](../adr/ADR-011-file-format.md) §3의 표 | man page, 연구 |
| `reqwest`, `tokio-tungstenite`, `rustls` | TLS 검증, 연결 오류의 분류 | 가정 A-08. 자세히 확인하지 않았다 |
| `k256`, `sha3`, `rmp-serde` | secp256k1 서명, Keccak-256, msgpack 인코딩 | SDK 시험 벡터와의 일치로 간접 확인 |

직접 작성한 `unsafe`, lock-free 구조는 없다. 원자 변수의 용도는 검토 항목 D-CONC-01에 적었다.
