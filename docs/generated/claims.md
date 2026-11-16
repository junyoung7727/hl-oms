# 검증 주장과 결과

> 자동 생성 문서다. 고치려면 `registry/*.toml`을 고치고 `cargo run -p xtask -- docs`를 실행한다.
> 소스 다이제스트 `bd9dec98dfee48a5`, 검증 결과의 다이제스트 `0a86a80a92f2313c`.

주장 61건: 통과 0, 실패 0, 미실행 4, 이전 버전 결과 57.

| ID | 주장 | 수준 | 상태 | 재현 |
|---|---|---|---|---|
| V-NUM-1 | 십진 문자열과 1e-8 정수 사이의 변환은 정확하며, 표현할 수 없는 입력은 반올림하지 않고 거부한다 | 단위 | 이전 버전 결과 | `cargo test -p oms-core --lib` |
| V-NUM-2 | 한도 비교용 금액은 정확한 곱 이상인 가장 작은 1e-8 단위 값이고, 넘치면 보고된다 | 속성 | 이전 버전 결과 | `cargo test -p oms-core --lib notional` |
| V-NEW-1 | 수용된 주문들이 어떤 순서로 체결되어도 계좌와 전략의 포지션은 설정한 구간을 벗어나지 않으며, 두 요청이 같은 여유를 쓰지 못한다 | 단위 | 이전 버전 결과 | `cargo test -p oms-core --test scenarios` |
| V-NEW-2 | 수량·가격 단위, 유효숫자, 최소·최대 금액, 가격대, 만료 시각, 종목·전략의 허용 여부, 용량을 검사하고 해당하는 사유를 모두 돌려준다. 거절 사유의 종류마다 시험이 있다 | 단위 | 이전 버전 결과 | `cargo test -p oms-core --test scenarios && cargo test -p oms-core --test gaps` |
| V-IDEM-1 | 같은 (전략, 키)로는 주문이 하나만 만들어지고 전송 명령도 한 번만 나온다. 다른 내용의 요청은 충돌로 거절된다. 키는 요청이 만료된 뒤에만 잊는다 | 단위 | 이전 버전 결과 | `cargo test -p oms-core --test scenarios` |
| V-IDEM-2 | 재전송, 충돌 요청, 재시작, 키 삭제가 임의로 섞여도 키당 수용 1회, 키당 전송 1회, 거래소의 클라이언트 ID당 접수 요청 1회가 유지된다 | 시뮬레이션 | 이전 버전 결과 | `OMS_SIM_SEEDS=400 cargo test --release -p oms-sim --test sim` |
| V-IDEM-3 | 실제 소켓으로 여러 연결이 같은 요청을 동시에 보내도, 노드를 강제 종료한 뒤에도, 주문은 하나이고 거래소 접수 요청도 하나다 | 프로세스 | 이전 버전 결과 | `cargo test -p oms-node --test e2e a_repeated_request` |
| V-DOUBT-1 | 전송 결과를 모르면 예약을 유지하고 신규 주문을 막으며 다시 보내지 않는다. 존재 증거가 오면 정상 주문이 된다 | 단위 | 이전 버전 결과 | `cargo test -p oms-core --test scenarios` |
| V-DOUBT-2 | '찾을 수 없음'은 전송 만료 + 여유가 지난 뒤의 조회일 때만 주문을 닫는다. 거래소가 만료를 강제하지 않는 설정에서는 절대 닫지 않는다 | 단위 | 이전 버전 결과 | `cargo test -p oms-core --test scenarios not_found && cargo test -p oms-node --test e2e a_lost_request` |
| V-DOUBT-3 | 부재 판정은 두 가정(거래소가 만료를 강제한다, 시계 차이가 여유보다 작다)에 의존하며, 가정을 깨면 시험이 위반을 잡는다 | 시뮬레이션 | 이전 버전 결과 | `cargo test --release -p oms-sim --test sim absence_rule -- --nocapture` |
| V-FILL-1 | 체결은 한 번만 기장되고, 접수 응답보다 먼저 와도, 주인을 아직 몰라도, 수량을 넘어도 버려지지 않는다 | 단위 | 이전 버전 결과 | `cargo test -p oms-core --test scenarios fill && cargo test -p oms-core --test gaps && cargo test -p oms-core --test review_core f2` |
| V-CANCEL-1 | 취소 요청은 예약을 풀지 않고, 취소 중 체결은 기장되며, 최종 수량은 종결 통보가 정한다. 취소는 차단 중에도 받고, 간격을 두고 상한까지 재시도한다. 상한에 이른 뒤 다시 요청하면 새 시도가 나가고, 결과가 오지 않는 시도는 시간 초과 뒤 불명으로 처리된다 | 단위 | 이전 버전 결과 | `cargo test -p oms-core --test scenarios cancel && cargo test -p oms-core --test review_core f1` |
| V-TERM-1 | 종결 통보가 체결보다 먼저 와도 아직 기장되지 않은 수량은 예약에 남고, 최종 수량을 모를 때는 체결 동기화가 종결 시각을 지난 뒤에만 닫는다 | 단위 | 이전 버전 결과 | `cargo test -p oms-core --test scenarios terminal` |
| V-RECON-1 | 링크가 돌아오면 미종결 주문의 증거, 체결, 열린 주문, 포지션을 현재 세션에서 다시 확인할 때까지 신규 주문을 막는다. 포지션은 예약으로 설명되는 범위로 검사하고, 오래된 스냅샷은 판정하지 않는다 | 단위 | 이전 버전 결과 | `cargo test -p oms-core --test scenarios` |
| V-RECON-2 | 노드가 죽어 있는 동안 거래소에서 일어난 체결, 거래소 주도 취소, 부분 체결을 재시작 뒤 거래소 조회로 모두 반영한다 | 프로세스 | 이전 버전 결과 | `cargo test -p oms-node --test e2e events_missed` |
| V-EXT-1 | OMS가 내지 않은 체결은 기장되고 불일치로 표시되어 신규 주문을 막으며, 운영자가 확인해야 풀린다 | 시뮬레이션 | 이전 버전 결과 | `cargo test --release -p oms-sim --test sim profile_external` |
| V-REPLAY-1 | 기록된 이벤트만 적용해 만든 상태는 살아 있는 상태와 같고, 기록된 입력을 다시 판단하면 기록된 이벤트가 나온다 | 단위 | 이전 버전 결과 | `cargo test -p oms-core --test scenarios && oms-node verify --data-dir <dir> --re-decide` |
| V-SIM-1 | 네트워크 장애, 통보 중복·지연, 링크 단절, 프로세스 재시작, 키 삭제가 임의로 섞인 실행의 매 단계에서: 구조 불변조건이 성립하고, 거래소의 실제 포지션이 OMS가 설명하는 범위 안에 있고, 한도를 넘지 않고, 같은 주문이 두 번 전송되지 않는다 | 시뮬레이션 | 이전 버전 결과 | `OMS_SIM_SEEDS=400 cargo test --release -p oms-sim --test sim -- --nocapture   (실패 시: OMS_SIM_PROFILE=<이름> OMS_SIM_SEED=<n>)` |
| V-SIM-2 | 장애가 멈추면 장부가 가상 거래소와 일치한다: 주문별 체결량과 상태, 포지션, 수수료 합. 차단 사유와 거짓 불일치가 남지 않는다 | 시뮬레이션 | 이전 버전 결과 | `V-SIM-1과 같다` |
| V-SIM-3 | 같은 seed는 바이트 단위로 같은 저널을 만든다 | 시뮬레이션 | 이전 버전 결과 | `cargo test --release -p oms-sim` |
| V-COMMIT-1 | flush가 실패하면 그 배치의 응답, 전송 명령, 읽기 모델 변경이 하나도 나가지 않고 프로세스가 입력을 닫는다. 성공한 경우 응답 시점에 읽기 모델은 이미 갱신돼 있다 | 프로세스 내부 | 이전 버전 결과 | `cargo test -p oms-node --features failpoints --test pipeline` |
| V-COMMIT-2 | 부하 중 임의 시점에 노드를 강제 종료하고 다시 띄우기를 반복해도, 대조가 끝난 뒤 주문별 체결량·상태와 포지션이 거래소와 일치하고 같은 주문이 두 번 접수 요청되지 않는다 | 프로세스 | 이전 버전 결과 | `OMS_KILL_ROUNDS=20 cargo test -p oms-node --test e2e random_kills -- --nocapture` |
| V-COMMIT-3 | 함께 도착한 입력은 flush를 공유하고, 응답은 판단 순서대로 나온다 | 프로세스 내부 | 이전 버전 결과 | `cargo test -p oms-node --features failpoints --test pipeline -- --nocapture` |
| V-JRN-1 | 프레임의 모든 절단과 모든 단일 비트 반전이 감지되고, 0으로 채워진 꼬리는 프레임으로 읽히지 않는다 | 단위 | 이전 버전 결과 | `cargo test -p oms-journal --lib` |
| V-JRN-2 | 저널의 끝이 불완전하면 유효한 접두부만 남기고 원본 바이트를 보존한 뒤 이어 쓸 수 있다. 중간이 손상됐거나 세그먼트가 빠졌거나 다른 인스턴스의 것이거나 스냅샷보다 짧으면 시작을 거부하고, 거부한 디렉터리의 파일은 건드리지 않는다 | 속성 | 이전 버전 결과 | `cargo test -p oms-journal && cargo test -p oms-node --test e2e journal_damage` |
| V-JRN-3 | 같은 데이터 디렉터리를 두 프로세스가 쓸 수 없고, 다른 인스턴스의 저널은 거부된다 | 프로세스 | 이전 버전 결과 | `cargo test -p oms-node --test e2e only_one_node` |
| V-JRN-4 | 스냅샷은 완전한 파일로만 나타나고, 손상된 스냅샷은 건너뛰며, 정상 종료 뒤 재시작은 스냅샷에서 같은 상태로 돌아온다 | 프로세스 | 이전 버전 결과 | `cargo test -p oms-journal --lib snapshot` |
| V-JRN-5 | 기록 스키마가 다른 빌드는 옛 세그먼트를 읽고 새 세그먼트를 시작하며, 읽을 수 없는 스키마를 만난 빌드는 시작을 거부한다 | 단위 | 이전 버전 결과 | `cargo test -p oms-journal a_schema_change` |
| V-BOOT-1 | 재시작은 결과 기록이 없는 전송을 불명으로 표시하고, 신규 주문을 다시 보내지 않으며, 요청돼 있던 취소만 다시 보낸다 | 프로세스 | 이전 버전 결과 | `cargo test -p oms-node --test e2e a_kill_during_send` |
| V-CFG-1 | 설정은 버전이 커야 활성화되고, 같은 버전에 다른 내용이면 시작을 거부한다. 노출이 남은 종목·전략은 제거할 수 없다. mainnet은 증거금 정책(policy.margin) 없이는 시작하지 않고, 정책의 값이 C-MARGIN 설정 표의 범위를 벗어나면 어느 거래소 설정에서도 시작하지 않는다 | 프로세스 | 이전 버전 결과 | `cargo test -p oms-node --test e2e configuration_changes && cargo test -p oms-node --lib config::tests` |
| V-OVL-1 | 입력 큐는 상한을 넘지 않고, 가득 찬 큐는 다른 큐에 영향을 주지 않으며, 배치마다 세 큐가 지분을 받는다 | 단위 | 이전 버전 결과 | `cargo test -p oms-node --lib queues` |
| V-OVL-2 | 큐보다 빠르게 온 요청은 즉시 거절되고 기록을 남기지 않으며, 그 직후에도 취소는 수용된다 | 프로세스 | 이전 버전 결과 | `cargo test -p oms-node --test e2e overload -- --nocapture` |
| V-OVL-3 | 저장 공간이 부족하면 신규 주문만 거절하고 체결과 취소는 계속 처리한다 | 단위 | 이전 버전 결과 | `cargo test -p oms-core --test scenarios storage_low` |
| V-API-1 | 전략 연결은 자기 주문만 보고 취소하며, 운영 명령은 운영 소켓에서만 받아들여진다. 알 수 없는 전략과 잘못된 요청은 거절된다 | 프로세스 | 이전 버전 결과 | `cargo test -p oms-node --test e2e a_strategy_sees` |
| V-API-2 | 문서의 요청 예가 그대로 해석되고, 수치에 JSON 숫자를 쓰면 거부된다 | 단위 | 이전 버전 결과 | `cargo test -p oms-api` |
| V-SLICE-1 | 신규 주문 → 영속 접수 → 거래소 접수 → 부분 체결 → 잔량 취소 → 종결이 실제 프로세스에서 이어지고, 구독자는 저널 순서로 이벤트를 받으며, 정상 종료 뒤 재시작해도 상태가 같고 재전송이 없다 | 프로세스 | 이전 버전 결과 | `cargo test -p oms-node --test e2e vertical_slice` |
| V-LINK-1 | 스트림이 끊기면 신규 주문을 막고, 그동안의 취소 요청을 기억했다가 복귀 뒤 보내며, 놓친 체결을 메운 뒤에야 거래를 재개한다 | 프로세스 | 이전 버전 결과 | `cargo test -p oms-node --test e2e link_loss` |
| V-HL-1 | L1 액션의 해시와 서명이 공식 Python SDK의 결과와 같고, 서명 키는 출력되지 않는다 | 단위 | 이전 버전 결과 | `cargo test -p oms-node --lib sign` |
| V-HL-2 | 문서에 실린 응답 예를 올바르게 해석한다. 문서에 없는 주문 응답 형태와 최상위 오류는 결과 불명으로 두고, 문서의 표에 없는 상태 이름에서는 아무 결론도 내지 않으며, 종결 상태에서 최종 수량을 추정하지 않는다. 체결 조회는 마지막 쪽임이 확실할 때만 완료를 선언한다 | 단위 | 이전 버전 결과 | `cargo test -p oms-node --lib hyperliquid` |
| V-HL-3 | 실제 testnet에 대해 게이트웨이가 메타를 읽어 종목 규칙을 통과시키고, 사용자 스트림을 구독해 링크를 올리고, 주문 조회(없음)·열린 주문·포지션·체결 조회·참조 가격을 해석한다 | 실제 거래소(읽기 전용) | 미실행 | `OMS_LIVE_TESTNET=1 cargo test -p oms-node --test hl_live -- --nocapture` |
| V-PURE-1 | 도메인 코어의 소스에 시계·파일·네트워크·스레드·난수·해시 컬렉션·부동소수점·unsafe가 없고 의존성은 serde와 serde_json뿐이다 | 소스 검사 | 이전 버전 결과 | `cargo run -p xtask -- verify --quick` |
| V-PERF-1 | 초당 200건의 open-loop 부하에서 로컬 응답 지연 p99가 20 ms 이하이고 오류와 미응답이 없다 | 측정 | 이전 버전 결과 | `cargo run --release -p xtask -- perf --dir <NVMe 디렉터리> --rates 20,200,2000 --seconds 60 --label nvme-final` |
| V-PERF-2 | 초당 2000건의 부하에서 (가) 수용 한도를 푼 설정에서는 큐가 상한 안에 머물고 미응답이 없으며, (나) 기본 한도에서는 초과분이 overloaded로 즉시 거절되고 저널에 기록을 남기지 않는다 | 측정 | 이전 버전 결과 | `xtask perf --label nvme-final --rates 20,200,2000 --seconds 60  /  xtask perf --label nvme-default-limits --rates 2000 --seconds 60 --default-limits` |
| V-PERF-3 | 스냅샷 이후 기록 5만 건의 재생이 5초 이내에 끝난다 | 측정 | 이전 버전 결과 | `cargo run --release -p xtask -- recovery-time --dir <디렉터리>` |
| V-MUT-1 | 핵심 보호 로직을 일부러 깨뜨리면 관련 시험이 실패한다 | 변이 | 이전 버전 결과 | `cargo run -p xtask -- mutants` |
| V-HIST-1 | 단계 지연 히스토그램이 보고하는 백분위는 실제 값 이상이고 12.5% 이내로 크다 | 단위 | 이전 버전 결과 | `cargo test -p oms-node --lib util` |
| V-COMMIT-4 | 커밋 순서의 경계마다 프로세스를 멈추면 결과가 경계별 장애 표와 같다: 쓰기 전에 멈추면 요청이 없었던 것이 되고, 쓴 뒤·flush 뒤·응답 뒤에 멈추면 주문은 남되 한 번도 전송되지 않고 결과 불명을 거쳐 "놓이지 않음"으로 닫히며, 같은 요청을 다시 보내면 같은 주문으로 답한다 | 프로세스 | 이전 버전 결과 | `cargo test -p oms-node --features failpoints --test crash_points` |
| V-COMMIT-5 | 실제 디스크에 둔 저널로 강제 종료를 60회 반복해도 V-COMMIT-2의 성질이 유지된다 | 프로세스 | 이전 버전 결과 | `cargo run --release -p xtask -- verify --extended <실제 디스크의 디렉터리>` |
| V-BOOT-2 | 저널이 여러 세그먼트에 걸쳐 있어도 강제 종료 뒤 스냅샷과 저널로 같은 상태로 돌아오고, `verify --re-decide`는 입력과 맞지 않는 기록을 잡아내며, 종료 중의 저널 실패는 정상 종료로 보고되지 않는다 | 프로세스 | 이전 버전 결과 | `cargo test -p oms-node --test review_node` |
| V-API-3 | 거래 소켓의 연결이 아무리 많아도 운영 소켓은 열리고, 끊긴 구독자는 연결 자리를 돌려주며, 한 연결은 구독을 하나만 가진다 | 프로세스 | 이전 버전 결과 | `cargo test -p oms-node --test review_node` |
| V-DASH-1 | 대시보드의 문서 열람 경로는 docs/, crates/, registry/ 아래의 .md, .rs, .toml만 읽는다 | 단위 | 이전 버전 결과 | `cargo test -p oms-node --lib http` |
| V-PERF-4 | 커밋 스레드 구성과 인라인 구성을 같은 조건에서 잰 결과가 보관돼 있다(ADR-012의 비교) | 측정 | 이전 버전 결과 | `xtask perf --label nvme-compare-thread --rates 200,2000,8000 --seconds 10  /  같은 명령에 --inline --label nvme-compare-inline` |
| V-PERF-5 | 저장 장치의 동기 쓰기 시간과 코어의 판단 시간을 잰 결과가 보관돼 있다 | 측정 | 이전 버전 결과 | `xtask fsync-probe --dir <디렉터리> --label nvme  /  xtask core-bench` |
| V-SIM-4 | 프로파일마다 seed 3000개로 V-SIM-1, V-SIM-2, V-EXT-1의 성질이 유지된다 | 시뮬레이션 | 이전 버전 결과 | `cargo run --release -p xtask -- verify --extended <실제 디스크의 디렉터리>` |
| V-FUZZ-1 | 가정을 지키지 않는 임의의 거래소 입력(순서 역전, 중복, 낡은 상태, 식별자 없는 체결, 부재 응답, 링크 변동, 재시작)에서도 코어는 결함으로 멈추지 않고, 불변조건과 재생 일치가 유지되고, 주문당 전송은 한 번이며, 차단 중 수용이 없고, 새 체결은 반드시 기장되거나 대기하고, 정직하게 정리된 뒤에는 차단이 남지 않는다 | 속성 | 이전 버전 결과 | `cargo test --release -p oms-core --test review_core examined_fuzz` |
| V-MIS-1 | 불일치 14종 각각을 일으키는 시험이 있고, 같은 대상의 반복은 확인되기 전까지 한 건으로 남는다 | 단위 | 이전 버전 결과 | `cargo test -p oms-core` |
| V-MARGIN-1 | 증거금 정책이 있으면 감소 전용 주문은 재확인, 결과 불명, 불일치 차단을 통과하고, 일반 주문은 모든 차단에서 거절된다. 정책 구역은 선택이고, 없으면 저널 기록에 바이트를 더하지 않는다 | 단위 | 이전 버전 결과 | `cargo test -p oms-core --test scenarios reduce_only && cargo test -p oms-core --test margin` |
| V-MARGIN-2 | 증거금 정책이 있으면 현재 링크 세션의 증거금 보고가 마지막 보고로 기록된다. 다른 세션의 보고, 링크가 내려간 동안의 보고, 순번이 크지 않은 보고는 버린다. 읽지 못한 보고와 최대 레버리지가 0인 보고는 사유와 함께 거부로 기록되고 마지막 보고를 바꾸지 않는다. 나이가 0 이상 30초 이하인 보고가 없으면 일반 주문은 MarginUnavailable로 거절된다. 감소 전용 주문은 보고가 없어도 받는다. 정책이 없으면 증거금 입력은 상태를 바꾸지 않고 기록되지 않는다 | 단위 | 이전 버전 결과 | `cargo test -p oms-core --test margin` |
| V-MARGIN-3 | 증거금 정책이 있으면 일반 주문은 받은 뒤의 계좌 노출이 한도 × 계좌 가치를 넘고 받기 전보다 커질 때 AccountLeverage로 거절된다. 정확히 한도와 같으면 받는다. 계좌 가치가 0 이하면 일반 주문을 모두 거절한다. 노출은 포지션과 모든 미체결 주문(감소 전용 포함)을 지정가와 참조 가격 중 높은 값으로 센 종목별 값의 합이다. 미체결 주문은 종결 증거가 올 때까지 노출에 남고 OMS는 그 주문을 고치거나 만료시키지 않는다. 가격이 움직여 한도를 넘어도 래치는 없다. 감소 전용 주문은 검사를 건너뛰지만 합에는 들어간다. 노출이 있는 종목에 신선한 참조 가격이 없으면 일반 주문은 MarginUnavailable로 거절된다 | 단위 | 미실행 | `cargo test -p oms-core --test margin` |
| V-MARGIN-11 | 증거금 정책이 있으면 노드는 정해진 간격으로 거래소에 증거금 보고를 묻고, 응답은 오르는 순번과 함께 코어 입력이 되며, status가 계좌 가치와 그 나이, 계좌 노출을 보여 준다. 정책이 없으면 묻지 않는다 | 프로세스 | 미실행 | `cargo test -p oms-node --test e2e node_polls_margin && cargo test -p oms-node --test e2e status_shows_account_exposure` |
| V-MARGIN-9 | 증거금 정책이 있으면, 종목의 거래소 설정(증거금 방식과 레버리지)을 이번 링크 세션에서 받지 못한 동안 그 종목의 일반 주문은 VenueSetting(종목) 차단으로 거절된다. 옛 세션의 응답은 버린다. 받은 설정의 증거금 방식이나 레버리지가 정책과 다르면 그 종목만 같은 차단에 걸리고 다른 종목은 걸리지 않는다. OMS는 설정을 읽기만 하고 거래소로 아무것도 보내지 않는다 | 단위 | 미실행 | `cargo test -p oms-core --test margin` |

## V-NUM-1 — 십진 문자열과 1e-8 정수 사이의 변환은 정확하며, 표현할 수 없는 입력은 반올림하지 않고 거부한다

- 상태: **이전 버전 결과**
- 왜 중요한가: 수치 오류는 모든 한도 검사를 무력화한다
- 수준: 단위
- 전제와 범위: 손으로 쓴 예제와, 모든 i64 값에 대한 왕복(property test, 기본 256회)
- 실제 구성요소: num.rs의 실제 함수
- 모의·시뮬레이션: 없음
- 주입한 장애: 잘못된 형식(지수, +, 쉼표, 소수 9자리, 범위 초과, 음수 수량)
- 판정 기준과 근거: 기대값은 손으로 계산해 시험에 적었다
- 재현: `cargo test -p oms-core --lib`
- 뒷받침하는 것: ADR-009의 표현 정확성
- 확인하지 않은 범위: 다른 로캘이나 유니코드 숫자. 입력은 ASCII로 가정한다

| 시험 | 결과 | 비고 |
|---|---|---|
| `core/num::tests::parses_business_examples` | 이전 버전 결과 | last result: passed |
| `core/num::tests::rejects_instead_of_rounding` | 이전 버전 결과 | last result: passed |
| `core/num::tests::formats_canonically` | 이전 버전 결과 | last result: passed |
| `core/num::tests::format_parse_roundtrip` | 이전 버전 결과 | last result: passed |
| `core/num::tests::serde_roundtrip_is_exact` | 이전 버전 결과 | last result: passed |
| `core/num::tests::significant_digits_examples` | 이전 버전 결과 | last result: passed |
| `core/ids::tests::rejects_invalid_identifiers` | 이전 버전 결과 | last result: passed |
| `core/ids::tests::venue_client_id_roundtrip` | 이전 버전 결과 | last result: passed |

## V-NUM-2 — 한도 비교용 금액은 정확한 곱 이상인 가장 작은 1e-8 단위 값이고, 넘치면 보고된다

- 상태: **이전 버전 결과**
- 왜 중요한가: 노출을 과소 계산하면 한도가 뚫린다
- 수준: 속성
- 전제와 범위: 가격 0~2e7, 수량 0~2e4 범위의 임의 값(property test)
- 실제 구성요소: notional_ceil
- 모의·시뮬레이션: 없음
- 주입한 장애: i64 범위를 넘는 곱, 음수
- 판정 기준과 근거: 나눗셈 없이 i128로 '결과 × 1e8 ≥ 곱, 차이 < 1e8'을 확인한다. 구현의 식을 복제하지 않는다
- 재현: `cargo test -p oms-core --lib notional`
- 뒷받침하는 것: ADR-009의 반올림 방향
- 확인하지 않은 범위: 범위 밖의 큰 값은 넘침 예제 하나로만 확인

| 시험 | 결과 | 비고 |
|---|---|---|
| `core/num::tests::notional_ceil_is_the_least_upper_bound` | 이전 버전 결과 | last result: passed |
| `core/num::tests::notional_rounds_in_the_documented_direction` | 이전 버전 결과 | last result: passed |
| `core/num::tests::notional_overflow_is_reported` | 이전 버전 결과 | last result: passed |

## V-NEW-1 — 수용된 주문들이 어떤 순서로 체결되어도 계좌와 전략의 포지션은 설정한 구간을 벗어나지 않으며, 두 요청이 같은 여유를 쓰지 못한다

- 상태: **이전 버전 결과**
- 왜 중요한가: 요구 R02. 한도 초과는 직접 손실 위험이다
- 수준: 단위
- 전제와 범위: 요구 문서의 실행 예와 반례 넷(동시 요청, 반대 방향 미체결, 전략 청산 대 계좌 한도). 한 종목, 두 전략
- 실제 구성요소: oms-core의 판단과 적용
- 모의·시뮬레이션: 거래소 통보는 시험이 직접 입력한다
- 주입한 장애: 없음(정상 순서)
- 판정 기준과 근거: 기대값은 docs/architecture/FIA-OMS-model.md §7.1의 표와 docs/research/oms-three-guarantees.md의 반례에서 가져왔다
- 재현: `cargo test -p oms-core --test scenarios`
- 뒷받침하는 것: I-QTY (조건부 불변조건)
- 확인하지 않은 범위: 여러 종목. 한도를 낮추는 설정 변경 뒤의 상태. 임의 순서는 V-SIM-1이 다룬다

| 시험 | 결과 | 비고 |
|---|---|---|
| `core/fia_7_1_reservation_follows_potential_fills` | 이전 버전 결과 | last result: passed |
| `core/concurrent_requests_cannot_double_spend_headroom` | 이전 버전 결과 | last result: passed |
| `core/open_orders_on_the_other_side_do_not_create_headroom` | 이전 버전 결과 | last result: passed |
| `core/strategy_reduction_is_not_account_reduction` | 이전 버전 결과 | last result: passed |
| `core/capacity_limits_bound_open_orders_and_outstanding_sends` | 이전 버전 결과 | last result: passed |

## V-NEW-2 — 수량·가격 단위, 유효숫자, 최소·최대 금액, 가격대, 만료 시각, 종목·전략의 허용 여부, 용량을 검사하고 해당하는 사유를 모두 돌려준다. 거절 사유의 종류마다 시험이 있다

- 상태: **이전 버전 결과**
- 왜 중요한가: 거래소가 거부할 주문을 미리 걸러 한도를 아끼고, 가격 입력 실수를 막는다
- 수준: 단위
- 전제와 범위: 경계값 예제. 가격대는 5%에서 정확히 경계 양쪽. 최소 금액은 내림, 최대 금액은 올림으로 비교한다
- 실제 구성요소: check_new_order
- 모의·시뮬레이션: 참조 가격은 시험이 입력한다
- 주입한 장애: 참조 가격 없음, 참조 가격 만료(15초 초과)
- 판정 기준과 근거: 거래소 문서의 예(123456은 유효, 12345.6은 무효)와 손으로 계산한 경계
- 재현: `cargo test -p oms-core --test scenarios && cargo test -p oms-core --test gaps`
- 뒷받침하는 것: C-NEW의 검사 목록
- 확인하지 않은 범위: 실제 거래소가 같은 주문을 받아들이는지

| 시험 | 결과 | 비고 |
|---|---|---|
| `core/field_validation_reasons` | 이전 버전 결과 | last result: passed |
| `core/significant_figure_rule_exempts_integers` | 이전 버전 결과 | last result: passed |
| `core/price_band_bounds_only_the_aggressive_side_and_needs_a_fresh_reference` | 이전 버전 결과 | last result: passed |
| `core/expiry_rules` | 이전 버전 결과 | last result: passed |
| `core/no_configuration_means_nothing_is_accepted` | 이전 버전 결과 | last result: passed |
| `core/every_remaining_reject_reason_has_a_case` | 이전 버전 결과 | last result: passed |
| `core/the_strategy_bound_binds_on_its_own` | 이전 버전 결과 | last result: passed |
| `core/memory_capacity_and_arithmetic_overflow_are_rejections_not_faults` | 이전 버전 결과 | last result: passed |
| `core/f9_min_notional_is_not_passed_by_rounding_up` | 이전 버전 결과 | last result: passed |

## V-IDEM-1 — 같은 (전략, 키)로는 주문이 하나만 만들어지고 전송 명령도 한 번만 나온다. 다른 내용의 요청은 충돌로 거절된다. 키는 요청이 만료된 뒤에만 잊는다

- 상태: **이전 버전 결과**
- 왜 중요한가: 재전송이 주문을 복제하면 안 된다
- 수준: 단위
- 전제와 범위: 같은 요청, 다른 요청, 다른 전략의 같은 키, 거절 뒤 재시도, 재시작 뒤, 삭제 전후
- 실제 구성요소: oms-core
- 모의·시뮬레이션: 재시작은 기록된 이벤트로 상태를 다시 만드는 것으로 흉내 낸다
- 주입한 장애: 응답 유실에 해당하는 재전송
- 판정 기준과 근거: 주문 수, 전송 명령 수, 기록 수를 직접 센다
- 재현: `cargo test -p oms-core --test scenarios`
- 뒷받침하는 것: I-KEY, C-IDEM
- 확인하지 않은 범위: 전략과 OMS의 시계가 다른 경우

| 시험 | 결과 | 비고 |
|---|---|---|
| `core/same_key_same_request_creates_one_order_and_one_send` | 이전 버전 결과 | last result: passed |
| `core/same_key_different_request_is_a_conflict_without_effect` | 이전 버전 결과 | last result: passed |
| `core/the_key_scope_is_per_strategy` | 이전 버전 결과 | last result: passed |
| `core/a_rejection_does_not_consume_the_key` | 이전 버전 결과 | last result: passed |
| `core/the_duplicate_answer_survives_restart` | 이전 버전 결과 | last result: passed |
| `core/a_key_is_forgotten_only_after_its_request_expired` | 이전 버전 결과 | last result: passed |

## V-IDEM-2 — 재전송, 충돌 요청, 재시작, 키 삭제가 임의로 섞여도 키당 수용 1회, 키당 전송 1회, 거래소의 클라이언트 ID당 접수 요청 1회가 유지된다

- 상태: **이전 버전 결과**
- 왜 중요한가: V-IDEM-1의 예제가 놓친 조합을 찾는다
- 수준: 시뮬레이션
- 전제와 범위: 프로파일 lossy, crashy, purge. 프로파일당 seed 수는 결과의 notes에 있다. purge는 보존 1초, 재전송 20%
- 실제 구성요소: oms-core
- 모의·시뮬레이션: 저널(내구 접두부를 가진 벡터), 게이트웨이, 네트워크, 거래소, 전략
- 주입한 장애: 응답 유실, 요청 유실·지연, 스트림 중복·지연, 링크 단절, 프로세스 재시작
- 판정 기준과 근거: 시뮬레이터가 수용 응답과 풀린 전송 명령을 키별로 세고, 가상 거래소가 접수 요청을 클라이언트 ID별로 센다
- 재현: `OMS_SIM_SEEDS=400 cargo test --release -p oms-sim --test sim`
- 뒷받침하는 것: I-KEY, I-ONCE
- 확인하지 않은 범위: 실제 저널과 실제 소켓(V-IDEM-3)

| 시험 | 결과 | 비고 |
|---|---|---|
| `sim/profile_lossy` | 이전 버전 결과 | last result: passed |
| `sim/profile_crashy` | 이전 버전 결과 | last result: passed |
| `sim/profile_purge` | 이전 버전 결과 | last result: passed |

## V-IDEM-3 — 실제 소켓으로 여러 연결이 같은 요청을 동시에 보내도, 노드를 강제 종료한 뒤에도, 주문은 하나이고 거래소 접수 요청도 하나다

- 상태: **이전 버전 결과**
- 왜 중요한가: 코어의 성질이 실제 프로세스 경계를 넘어 유지되는지 확인한다
- 수준: 프로세스
- 전제와 범위: 연결 8개의 동시 요청 1회, 강제 종료 1회
- 실제 구성요소: oms-node 프로세스, 소켓, 저널 파일
- 모의·시뮬레이션: 거래소(simvenue 프로세스)
- 주입한 장애: SIGKILL
- 판정 기준과 근거: 응답을 세고 가상 거래소의 접수 요청 계수기를 읽는다
- 재현: `cargo test -p oms-node --test e2e a_repeated_request`
- 뒷받침하는 것: C-IDEM
- 확인하지 않은 범위: 전원 장애

| 시험 | 결과 | 비고 |
|---|---|---|
| `e2e/a_repeated_request_never_creates_a_second_order` | 이전 버전 결과 | last result: passed |

## V-DOUBT-1 — 전송 결과를 모르면 예약을 유지하고 신규 주문을 막으며 다시 보내지 않는다. 존재 증거가 오면 정상 주문이 된다

- 상태: **이전 버전 결과**
- 왜 중요한가: 응답 유실을 실패로 읽으면 예약 없는 체결이나 중복 주문이 생긴다
- 수준: 단위
- 전제와 범위: 불명 → 차단, 조회로 발견, 확실한 실패(미전송, 거부)는 즉시 종결, 결과가 아예 없을 때의 시간 초과
- 실제 구성요소: oms-core
- 모의·시뮬레이션: 게이트웨이 결과는 시험이 입력한다
- 주입한 장애: 결과 불명, 결과 없음
- 판정 기준과 근거: 기존 계약 문서(hyperliquid-trading/docs/state-machine/02-recovery.md 사례 A)의 기대 동작
- 재현: `cargo test -p oms-core --test scenarios`
- 뒷받침하는 것: C-SEND, C-DOUBT
- 확인하지 않은 범위: 게이트웨이가 결과를 올바르게 분류하는지(C-GW G-2)는 게이트웨이 시험의 몫이다

| 시험 | 결과 | 비고 |
|---|---|---|
| `core/unknown_send_outcome_keeps_the_reservation_and_blocks_new_exposure` | 이전 버전 결과 | last result: passed |
| `core/in_doubt_order_found_by_query_becomes_working` | 이전 버전 결과 | last result: passed |
| `core/definite_gateway_failures_close_and_release` | 이전 버전 결과 | last result: passed |
| `core/a_send_without_any_result_becomes_in_doubt_after_the_timeout` | 이전 버전 결과 | last result: passed |
| `core/an_in_doubt_order_seen_in_the_open_order_sweep_is_acknowledged` | 이전 버전 결과 | last result: passed |

## V-DOUBT-2 — '찾을 수 없음'은 전송 만료 + 여유가 지난 뒤의 조회일 때만 주문을 닫는다. 거래소가 만료를 강제하지 않는 설정에서는 절대 닫지 않는다

- 상태: **이전 버전 결과**
- 왜 중요한가: 너무 일찍 닫으면 그 뒤에 놓인 주문이 예약 없이 체결된다
- 수준: 단위
- 전제와 범위: 여유 10초의 경계 양쪽. 실제 프로세스에서는 만료 1.5초 + 여유 0.3초 이전에 닫히지 않음을 시간으로 확인
- 실제 구성요소: oms-core. e2e에서는 oms-node 프로세스와 실제 시계
- 모의·시뮬레이션: e2e의 거래소는 simvenue이며 요청 1건을 버린다
- 주입한 장애: 요청 유실
- 판정 기준과 근거: 닫히는 시각이 만료 + 여유 이후인지, 예약이 그때까지 유지되는지
- 재현: `cargo test -p oms-core --test scenarios not_found && cargo test -p oms-node --test e2e a_lost_request`
- 뒷받침하는 것: ADR-005의 규칙 5
- 확인하지 않은 범위: 실제 거래소가 expiresAfter를 어떤 시계로 어떻게 강제하는지

| 시험 | 결과 | 비고 |
|---|---|---|
| `core/not_found_closes_an_in_doubt_order_only_after_expiry_plus_margin` | 이전 버전 결과 | last result: passed |
| `core/not_found_never_closes_when_the_venue_does_not_enforce_expiry` | 이전 버전 결과 | last result: passed |
| `core/not_found_is_ignored_while_the_send_is_simply_outstanding` | 이전 버전 결과 | last result: passed |
| `e2e/a_lost_request_is_closed_only_after_its_send_window` | 이전 버전 결과 | last result: passed |

## V-DOUBT-3 — 부재 판정은 두 가정(거래소가 만료를 강제한다, 시계 차이가 여유보다 작다)에 의존하며, 가정을 깨면 시험이 위반을 잡는다

- 상태: **이전 버전 결과**
- 왜 중요한가: 가정이 실제로 필요한지, 그리고 검사가 실제로 잡아내는지 확인한다
- 수준: 시뮬레이션
- 전제와 범위: 거래소가 만료를 무시하는 실행과 시계가 6초 어긋난 실행(여유 2초). 각각 seed 60개 안에서 위반을 찾는다
- 실제 구성요소: oms-core
- 모의·시뮬레이션: 나머지 전부
- 주입한 장애: 가정 위반을 일부러 주입
- 판정 기준과 근거: 위반이 주문·포지션에 관한 속성(bridge, venue-position-limit, position-bounds, final-orders, final-fills, final-position, no-false-mismatch) 중 하나로 잡혀야 통과. 다른 속성으로 실패하면 시험이 실패한다
- 재현: `cargo test --release -p oms-sim --test sim absence_rule -- --nocapture`
- 뒷받침하는 것: 가정 A-01, A-02의 필요성. I-BRIDGE 검사의 민감도
- 확인하지 않은 범위: 실제 거래소에서 이 가정이 성립하는지

| 시험 | 결과 | 비고 |
|---|---|---|
| `sim/absence_rule_depends_on_venue_side_expiry` | 이전 버전 결과 | last result: passed |
| `sim/absence_rule_depends_on_the_clock_margin` | 이전 버전 결과 | last result: passed |

## V-FILL-1 — 체결은 한 번만 기장되고, 접수 응답보다 먼저 와도, 주인을 아직 몰라도, 수량을 넘어도 버려지지 않는다

- 상태: **이전 버전 결과**
- 왜 중요한가: 체결 유실이나 이중 기장은 포지션 오차다
- 수준: 단위
- 전제와 범위: 중복, 순서 역전, 거래소 ID만 있는 체결의 대기와 해소(상태 통보로, 다른 체결로), 주인 없는 체결의 외부 기장, 초과 체결, 체결만으로 전량 체결 종결, 취소·부재·수동으로 닫은 주문의 체결, 메모리에서 지워진 주문의 체결, 설정에 없는 종목의 체결, 통계 합계가 넘치는 체결
- 실제 구성요소: oms-core
- 모의·시뮬레이션: 통보는 시험이 입력
- 주입한 장애: 중복 통보, 순서 역전, 식별 정보 부족
- 판정 기준과 근거: 포지션·누적량·예약·불일치 종류를 손으로 계산한 값과 비교
- 재현: `cargo test -p oms-core --test scenarios fill && cargo test -p oms-core --test gaps && cargo test -p oms-core --test review_core f2`
- 뒷받침하는 것: C-FILL, I-FILL
- 확인하지 않은 범위: 체결 정정. 코어가 기장하지 않는 체결 입력은 셋뿐이다(수량이 0 이하, 이미 기장했거나 대기 중인 키, 체결 바닥 시각보다 오래된 것). 그 셋의 시험이 `the_three_ways_a_fill_input_changes_nothing`이다

| 시험 | 결과 | 비고 |
|---|---|---|
| `core/duplicate_fill_is_booked_once` | 이전 버전 결과 | last result: passed |
| `core/fill_before_send_result_acknowledges_the_order` | 이전 버전 결과 | last result: passed |
| `core/fill_by_venue_id_before_the_id_is_known_is_parked_then_booked` | 이전 버전 결과 | last result: passed |
| `core/unidentified_fill_is_booked_as_external_after_the_wait_and_holds_trading` | 이전 버전 결과 | last result: passed |
| `core/a_parked_fill_is_external_once_no_order_can_own_it` | 이전 버전 결과 | last result: passed |
| `core/overfill_is_booked_in_full_and_flagged` | 이전 버전 결과 | last result: passed |
| `core/full_fill_by_fills_alone_closes_the_order` | 이전 버전 결과 | last result: passed |
| `core/f2_parked_fill_is_booked_to_its_order_when_another_fill_reveals_the_venue_id` | 이전 버전 결과 | last result: passed |
| `core/a_fill_after_any_kind_of_close_is_booked_and_flagged` | 이전 버전 결과 | last result: passed |
| `core/a_fill_for_a_purged_order_is_booked_as_external` | 이전 버전 결과 | last result: passed |
| `core/a_fill_on_an_instrument_that_is_not_configured_is_booked_as_external_without_a_fault` | 이전 버전 결과 | last result: passed |
| `core/the_three_ways_a_fill_input_changes_nothing` | 이전 버전 결과 | last result: passed |
| `core/f6_reporting_counter_overflow_does_not_fail_stop_fill_booking` | 이전 버전 결과 | last result: passed |

## V-CANCEL-1 — 취소 요청은 예약을 풀지 않고, 취소 중 체결은 기장되며, 최종 수량은 종결 통보가 정한다. 취소는 차단 중에도 받고, 간격을 두고 상한까지 재시도한다. 상한에 이른 뒤 다시 요청하면 새 시도가 나가고, 결과가 오지 않는 시도는 시간 초과 뒤 불명으로 처리된다

- 상태: **이전 버전 결과**
- 왜 중요한가: 취소했다는 이유로 위험을 0으로 보면 안 된다
- 수준: 단위
- 전제와 범위: 기존 계약 문서의 취소 경합 사례 4a, 4b, 4c. 소유권, 중복 취소, 미접수 주문의 취소, 재시도 상한, 상한 뒤의 재요청(전략과 운영자), 결과 불명 중에 쓴 시도, 거부된 취소, 결과 없는 취소, 중지 중 취소, 링크 복귀 시 전송
- 실제 구성요소: oms-core
- 모의·시뮬레이션: 통보는 시험이 입력
- 주입한 장애: 취소 결과 불명, 링크 단절
- 판정 기준과 근거: hyperliquid-trading/docs/state-machine/02-recovery.md 사례 B의 표
- 재현: `cargo test -p oms-core --test scenarios cancel && cargo test -p oms-core --test review_core f1`
- 뒷받침하는 것: C-CANCEL, 요구 R07, R09
- 확인하지 않은 범위: 실제 거래소의 취소 응답 문구와 시점

| 시험 | 결과 | 비고 |
|---|---|---|
| `core/cancel_race_final_status_decides` | 이전 버전 결과 | last result: passed |
| `core/final_quantity_below_booked_fills_is_flagged` | 이전 버전 결과 | last result: passed |
| `core/cancel_is_authorised_per_owner_and_idempotent` | 이전 버전 결과 | last result: passed |
| `core/cancel_of_an_unacknowledged_order_is_sent_once_the_order_is_acknowledged` | 이전 버전 결과 | last result: passed |
| `core/cancel_is_retried_with_backoff_and_then_flagged` | 이전 버전 결과 | last result: passed |
| `core/cancels_are_accepted_while_new_orders_are_held` | 이전 버전 결과 | last result: passed |
| `core/a_pending_cancel_is_sent_when_the_link_returns` | 이전 버전 결과 | last result: passed |
| `core/cancel_all_requests_every_open_order_in_scope` | 이전 버전 결과 | last result: passed |
| `core/holds_are_independent` | 이전 버전 결과 | last result: passed |
| `core/f1_cancel_can_be_reissued_after_transient_failures_used_up_the_attempts` | 이전 버전 결과 | last result: passed |
| `core/f1_acknowledging_cancel_stuck_releases_the_hold` | 이전 버전 결과 | last result: passed |
| `core/f1_cancel_attempts_spent_in_doubt_do_not_strand_the_order_once_it_is_found` | 이전 버전 결과 | last result: passed |
| `core/f1_a_rejected_cancel_is_retried_only_while_the_order_is_not_terminal` | 이전 버전 결과 | last result: passed |
| `core/f1_a_cancel_without_any_result_is_retried_after_a_timeout` | 이전 버전 결과 | last result: passed |

## V-TERM-1 — 종결 통보가 체결보다 먼저 와도 아직 기장되지 않은 수량은 예약에 남고, 최종 수량을 모를 때는 체결 동기화가 종결 시각을 지난 뒤에만 닫는다

- 상태: **이전 버전 결과**
- 왜 중요한가: 종결을 일찍 확정하면 뒤이은 체결이 예약 없이 기장된다
- 수준: 단위
- 전제와 범위: 최종 수량을 아는 경우와 모르는 경우. 체결이 끝내 오지 않는 경우
- 실제 구성요소: oms-core
- 모의·시뮬레이션: 통보는 시험이 입력
- 주입한 장애: 통보 순서 역전, 체결 누락
- 판정 기준과 근거: 예약·포지션·단계를 손으로 계산한 값과 비교
- 재현: `cargo test -p oms-core --test scenarios terminal`
- 뒷받침하는 것: C-TERM, ADR-007의 종결 규칙
- 확인하지 않은 범위: 게이트웨이가 체결 동기화를 올바른 시각으로 선언하는지(G-7)

| 시험 | 결과 | 비고 |
|---|---|---|
| `core/terminal_before_fills_keeps_the_unbooked_quantity_reserved` | 이전 버전 결과 | last result: passed |
| `core/terminal_without_final_quantity_waits_for_fill_sync` | 이전 버전 결과 | last result: passed |
| `core/missing_fills_after_sync_are_flagged_and_stay_reserved` | 이전 버전 결과 | last result: passed |

## V-RECON-1 — 링크가 돌아오면 미종결 주문의 증거, 체결, 열린 주문, 포지션을 현재 세션에서 다시 확인할 때까지 신규 주문을 막는다. 포지션은 예약으로 설명되는 범위로 검사하고, 오래된 스냅샷은 판정하지 않는다

- 상태: **이전 버전 결과**
- 왜 중요한가: 끊긴 동안의 변화를 모른 채 주문하면 안 되고, 거짓 불일치로 거래를 막아도 안 된다
- 수준: 단위
- 전제와 범위: 세션 전환, 이전 세션의 늦은 답(접수 증거 포함)과 늦은 단절 통보, 네 확인을 하나씩 뺀 경우, 범위 안/밖의 포지션, 조회 뒤 기장된 체결, 외부 주문과 고아 주문, 클라이언트 ID 없이 나열된 우리 주문, 불일치 확인의 독립성, 명시적 조정
- 실제 구성요소: oms-core
- 모의·시뮬레이션: 조회 결과는 시험이 입력
- 주입한 장애: 링크 단절, 늦은 답, 외부 주문
- 판정 기준과 근거: I-BRIDGE의 식과 손으로 계산한 구간
- 재현: `cargo test -p oms-core --test scenarios`
- 뒷받침하는 것: C-LINK, C-RECON, C-MISMATCH
- 확인하지 않은 범위: 실제 거래소의 조회 지연. 조회 응답이 조회 시각보다 오래된 상태를 돌려주면 거짓 불일치가 날 수 있다(가정 A-13)

| 시험 | 결과 | 비고 |
|---|---|---|
| `core/link_loss_blocks_new_orders_and_resync_is_required_after_it_returns` | 이전 버전 결과 | last result: passed |
| `core/stale_session_evidence_does_not_satisfy_resync` | 이전 버전 결과 | last result: passed |
| `core/position_check_uses_the_reachable_range` | 이전 버전 결과 | last result: passed |
| `core/a_position_snapshot_older_than_the_last_booked_fill_is_inconclusive` | 이전 버전 결과 | last result: passed |
| `core/foreign_and_orphan_open_orders_are_flagged` | 이전 버전 결과 | last result: passed |
| `core/acknowledging_one_mismatch_does_not_clear_another` | 이전 버전 결과 | last result: passed |
| `core/position_adjustment_is_explicit_and_keeps_the_ledgers_consistent` | 이전 버전 결과 | last result: passed |
| `core/each_part_of_the_re_confirmation_is_required` | 이전 버전 결과 | last result: passed |
| `core/f4_stale_session_evidence_that_acknowledges_an_order_does_not_satisfy_resync` | 이전 버전 결과 | last result: passed |
| `core/f5_stale_link_down_does_not_take_a_newer_session_down` | 이전 버전 결과 | last result: passed |
| `core/f8_own_unacknowledged_order_listed_without_client_id_is_not_flagged_foreign` | 이전 버전 결과 | last result: passed |

## V-RECON-2 — 노드가 죽어 있는 동안 거래소에서 일어난 체결, 거래소 주도 취소, 부분 체결을 재시작 뒤 거래소 조회로 모두 반영한다

- 상태: **이전 버전 결과**
- 왜 중요한가: 복구가 저널만이 아니라 거래소 상태까지 맞추는지 확인한다
- 수준: 프로세스
- 전제와 범위: 주문 3건(전량 체결, 부분 체결 후 거래소 취소, 부분 체결 후 잔존). 1회
- 실제 구성요소: oms-node 프로세스, 저널, 소켓, 대조기
- 모의·시뮬레이션: 거래소(simvenue)
- 주입한 장애: SIGKILL, 그 사이의 거래소 변화
- 판정 기준과 근거: 주문별 단계·누적량·예약과 포지션을 손으로 계산한 값(+1 −0.5 +1 = 1.5) 및 가상 거래소의 포지션과 비교
- 재현: `cargo test -p oms-node --test e2e events_missed`
- 뒷받침하는 것: C-RECON, C-BOOT
- 확인하지 않은 범위: 실제 거래소

| 시험 | 결과 | 비고 |
|---|---|---|
| `e2e/events_missed_while_down_are_recovered_from_the_venue` | 이전 버전 결과 | last result: passed |

## V-EXT-1 — OMS가 내지 않은 체결은 기장되고 불일치로 표시되어 신규 주문을 막으며, 운영자가 확인해야 풀린다

- 상태: **이전 버전 결과**
- 왜 중요한가: 외부 활동은 한도 계산의 전제를 깬다
- 수준: 시뮬레이션
- 전제와 범위: 코어 예제, 시뮬레이션 external 프로파일(외부 체결을 임의 시점에 주입), 실제 프로세스 1회
- 실제 구성요소: oms-core. e2e에서는 oms-node
- 모의·시뮬레이션: 거래소
- 주입한 장애: 외부 체결
- 판정 기준과 근거: 미귀속 포지션 = 가상 거래소가 기록한 외부 활동의 합, 불일치가 제기됨, 계좌 포지션 = 거래소 포지션
- 재현: `cargo test --release -p oms-sim --test sim profile_external`
- 뒷받침하는 것: 가정 A-04의 위반 탐지
- 확인하지 않은 범위: 외부 주문이 체결되기 전에 막는 것(탐지만 한다)

| 시험 | 결과 | 비고 |
|---|---|---|
| `core/unidentified_fill_is_booked_as_external_after_the_wait_and_holds_trading` | 이전 버전 결과 | last result: passed |
| `sim/profile_external` | 이전 버전 결과 | last result: passed |
| `e2e/external_activity_is_detected_and_needs_an_operator` | 이전 버전 결과 | last result: passed |

## V-REPLAY-1 — 기록된 이벤트만 적용해 만든 상태는 살아 있는 상태와 같고, 기록된 입력을 다시 판단하면 기록된 이벤트가 나온다

- 상태: **이전 버전 결과**
- 왜 중요한가: 복구의 정확성과 판단의 재현성
- 수준: 단위
- 전제와 범위: 시나리오 시험 전부(매 입력 뒤 그림자 상태 비교), 시뮬레이션의 최종 검사, 실제 저널에 대한 verify --re-decide
- 실제 구성요소: oms-core. e2e에서는 실제 저널 파일
- 모의·시뮬레이션: 시나리오에서는 저널이 메모리의 벡터
- 주입한 장애: 없음
- 판정 기준과 근거: 상태의 구조적 동등 비교와 이벤트 목록의 동등 비교
- 재현: `cargo test -p oms-core --test scenarios && oms-node verify --data-dir <dir> --re-decide`
- 뒷받침하는 것: I-REPLAY, I-DET
- 확인하지 않은 범위: 다른 빌드가 쓴 저널. 다른 CPU 구조

| 시험 | 결과 | 비고 |
|---|---|---|
| `core/recorded_inputs_re_decide_to_the_recorded_events` | 이전 버전 결과 | last result: passed |
| `core/restart_puts_outstanding_sends_in_doubt_and_never_resends` | 이전 버전 결과 | last result: passed |
| `sim/profile_crashy` | 이전 버전 결과 | last result: passed |
| `e2e/vertical_slice_new_partial_fill_cancel_restart` | 이전 버전 결과 | last result: passed |
| `e2e/random_kills_under_load_keep_books_consistent` | 이전 버전 결과 | last result: passed |
| `core/margin_events_replay_to_the_same_state` | 이전 버전 결과 | last result: passed |

## V-SIM-1 — 네트워크 장애, 통보 중복·지연, 링크 단절, 프로세스 재시작, 키 삭제가 임의로 섞인 실행의 매 단계에서: 구조 불변조건이 성립하고, 거래소의 실제 포지션이 OMS가 설명하는 범위 안에 있고, 한도를 넘지 않고, 같은 주문이 두 번 전송되지 않는다

- 상태: **이전 버전 결과**
- 왜 중요한가: 예제 시험이 생각하지 못한 순서를 찾는다. 실제로 결함 넷을 찾아 고쳤다
- 수준: 시뮬레이션
- 전제와 범위: 프로파일 6개(calm, lossy, crashy, weak_evidence, purge, skewed). 실행당 1500단계. seed 수는 결과의 notes에 있다. 한 종목, 두 전략, 가상 시계
- 실제 구성요소: oms-core의 판단과 상태
- 모의·시뮬레이션: 저널과 커밋 게이트(내구 접두부 모델), 게이트웨이, 네트워크, 거래소, 전략, 시계
- 주입한 장애: 요청 유실 4%, 요청 지연 6%(만료를 넘기기도), 응답 유실 8%, 미전송 3%, 스트림 중복 15%·지연 20%, 조회 유실 10%, 링크 단절, 재시작 1.2%/단계, 거래소 시계 1초 지연
- 판정 기준과 근거: 불변조건 I-RES, I-BOOK, I-ATTR, I-KEY, I-FILL, I-PHASE, I-QTY, I-BRIDGE, I-ONCE. 비교 대상은 가상 거래소가 따로 유지하는 실제 장부다
- 재현: `OMS_SIM_SEEDS=400 cargo test --release -p oms-sim --test sim -- --nocapture   (실패 시: OMS_SIM_PROFILE=<이름> OMS_SIM_SEED=<n>)`
- 뒷받침하는 것: I-QTY, I-BRIDGE, I-ONCE, 구조 불변조건
- 확인하지 않은 범위: 실제 저널·스레드·소켓. 여러 종목. 설정 변경. 운영자 조정. 장애가 계속되는 프로파일에서는 요청의 대부분이 차단으로 거절된다. 수용 비율은 결과의 notes에 있다

| 시험 | 결과 | 비고 |
|---|---|---|
| `sim/profile_calm` | 이전 버전 결과 | last result: passed |
| `sim/profile_lossy` | 이전 버전 결과 | last result: passed |
| `sim/profile_crashy` | 이전 버전 결과 | last result: passed |
| `sim/profile_weak_evidence` | 이전 버전 결과 | last result: passed |
| `sim/profile_purge` | 이전 버전 결과 | last result: passed |
| `sim/profile_skewed` | 이전 버전 결과 | last result: passed |

## V-SIM-2 — 장애가 멈추면 장부가 가상 거래소와 일치한다: 주문별 체결량과 상태, 포지션, 수수료 합. 차단 사유와 거짓 불일치가 남지 않는다

- 상태: **이전 버전 결과**
- 왜 중요한가: 안전 조건만으로는 '모두 거절'도 통과한다. 수렴을 따로 확인한다
- 수준: 시뮬레이션
- 전제와 범위: V-SIM-1과 같은 실행의 끝. 장애 주입을 멈추고 가상 시간 최대 90초를 준다
- 실제 구성요소: oms-core
- 모의·시뮬레이션: V-SIM-1과 같다
- 주입한 장애: 실행 중에는 V-SIM-1과 같고, 끝에서는 없음
- 판정 기준과 근거: 가상 거래소의 주문·체결·포지션·수수료와 직접 비교
- 재현: `V-SIM-1과 같다`
- 뒷받침하는 것: I-LIVE-2, I-LIVE-3
- 확인하지 않은 범위: 통신이 끝내 돌아오지 않는 경우

| 시험 | 결과 | 비고 |
|---|---|---|
| `sim/profile_calm` | 이전 버전 결과 | last result: passed |
| `sim/profile_lossy` | 이전 버전 결과 | last result: passed |
| `sim/profile_crashy` | 이전 버전 결과 | last result: passed |
| `sim/profile_weak_evidence` | 이전 버전 결과 | last result: passed |
| `sim/profile_purge` | 이전 버전 결과 | last result: passed |
| `sim/profile_skewed` | 이전 버전 결과 | last result: passed |

## V-SIM-3 — 같은 seed는 바이트 단위로 같은 저널을 만든다

- 상태: **이전 버전 결과**
- 왜 중요한가: 실패한 실행을 재현할 수 있어야 한다
- 수준: 시뮬레이션
- 전제와 범위: seed 3개, crashy 프로파일
- 실제 구성요소: oms-core
- 모의·시뮬레이션: 전부
- 주입한 장애: crashy
- 판정 기준과 근거: 통계와 저널 해시의 동등, 다른 seed와는 다름. 난수 생성기는 공개된 SplitMix64 기준값과 대조
- 재현: `cargo test --release -p oms-sim`
- 뒷받침하는 것: I-DET, 시뮬레이션의 재현성
- 확인하지 않은 범위: 다른 플랫폼에서의 동일성

| 시험 | 결과 | 비고 |
|---|---|---|
| `sim/a_seed_reproduces_the_same_run` | 이전 버전 결과 | last result: passed |
| `sim/rng::tests::matches_the_reference_sequence` | 이전 버전 결과 | last result: passed |

## V-COMMIT-1 — flush가 실패하면 그 배치의 응답, 전송 명령, 읽기 모델 변경이 하나도 나가지 않고 프로세스가 입력을 닫는다. 성공한 경우 응답 시점에 읽기 모델은 이미 갱신돼 있다

- 상태: **이전 버전 결과**
- 왜 중요한가: 커밋 게이트(I-WAL)의 핵심
- 수준: 프로세스 내부
- 전제와 범위: 커밋 스레드 구성과 인라인 구성 각각 1회. 주문 1건 성공 후 1건 실패
- 실제 구성요소: 코어 스레드, 커밋 스레드, 저널 파일, 채널
- 모의·시뮬레이션: 게이트웨이는 명령 채널의 수신단뿐. flush 실패는 주입(failpoints 빌드에서만 존재하는 훅)
- 주입한 장애: flush 오류 1회. 쓰기 전에 주입하므로 파일에는 아무것도 남지 않는다. 쓴 뒤에 실패하는 경우는 V-COMMIT-4
- 판정 기준과 근거: 응답 채널이 닫힘, 명령 채널이 비어 있음, 읽기 모델의 주문 수와 내구 순번이 그대로, 재복구한 상태가 실패 전과 같음(쓰기 전 주입이라 그렇다)
- 재현: `cargo test -p oms-node --features failpoints --test pipeline`
- 뒷받침하는 것: I-WAL, C-COMMIT
- 확인하지 않은 범위: 실제 장치 오류와 ENOSPC. 주입한 오류가 실제 fdatasync 오류와 같은 파일 상태를 남긴다는 보장은 없다

| 시험 | 결과 | 비고 |
|---|---|---|
| `pipeline/outputs_follow_durability_with_a_commit_thread` | 이전 버전 결과 | last result: passed |
| `pipeline/outputs_follow_durability_with_inline_commit` | 이전 버전 결과 | last result: passed |

## V-COMMIT-2 — 부하 중 임의 시점에 노드를 강제 종료하고 다시 띄우기를 반복해도, 대조가 끝난 뒤 주문별 체결량·상태와 포지션이 거래소와 일치하고 같은 주문이 두 번 접수 요청되지 않는다

- 상태: **이전 버전 결과**
- 왜 중요한가: 실제 스레드·저널·소켓에서 커밋 게이트와 복구가 함께 성립하는지
- 수준: 프로세스
- 전제와 범위: 회차 수는 결과의 notes에 있다(시험의 기본 12, 수용 시험 20). 두 전략이 주문과 취소를 계속 보낸다. 종료 시점은 60~460 ms 뒤 임의
- 실제 구성요소: oms-node 프로세스(스레드, 큐, 저널, 스냅샷, 소켓, 게이트웨이, 대조기)
- 모의·시뮬레이션: 거래소(simvenue 프로세스, 절반 자동 체결)
- 주입한 장애: SIGKILL. 스냅샷 주기를 150 기록으로 낮춰 스냅샷 경로도 지난다
- 판정 기준과 근거: 가상 거래소의 주문 목록·포지션·반복 접수 계수기와 비교. 마지막에 verify --re-decide
- 재현: `OMS_KILL_ROUNDS=20 cargo test -p oms-node --test e2e random_kills -- --nocapture`
- 뒷받침하는 것: C-COMMIT의 경계 표, C-BOOT
- 확인하지 않은 범위: 전원 장애, OS 충돌. SIGKILL은 커널이 받은 데이터를 잃지 않는다. 종료 시점은 무작위라 특정 경계를 겨냥하지 않는다(경계별 시험은 V-COMMIT-4). 실제 디스크에서의 더 긴 실행은 V-COMMIT-5

| 시험 | 결과 | 비고 |
|---|---|---|
| `e2e/random_kills_under_load_keep_books_consistent` | 이전 버전 결과 | last result: passed |
| `e2e/a_kill_during_send_does_not_duplicate_the_order` | 이전 버전 결과 | last result: passed |

## V-COMMIT-3 — 함께 도착한 입력은 flush를 공유하고, 응답은 판단 순서대로 나온다

- 상태: **이전 버전 결과**
- 왜 중요한가: 그룹 커밋이 실제로 일어나는지
- 수준: 프로세스 내부
- 전제와 범위: 요청 40건을 한꺼번에
- 실제 구성요소: 코어 스레드, 커밋 스레드, 저널 파일
- 모의·시뮬레이션: 게이트웨이 없음
- 주입한 장애: 없음
- 판정 기준과 근거: flush 횟수 < 40, 주문 ID 오름차순
- 재현: `cargo test -p oms-node --features failpoints --test pipeline -- --nocapture`
- 뒷받침하는 것: ADR-004의 그룹 커밋
- 확인하지 않은 범위: 배치 크기의 분포(성능 측정에서 본다)

| 시험 | 결과 | 비고 |
|---|---|---|
| `pipeline/inputs_that_arrive_together_share_a_flush` | 이전 버전 결과 | last result: passed |

## V-JRN-1 — 프레임의 모든 절단과 모든 단일 비트 반전이 감지되고, 0으로 채워진 꼬리는 프레임으로 읽히지 않는다

- 상태: **이전 버전 결과**
- 왜 중요한가: 손상 감지의 기초
- 수준: 단위
- 전제와 범위: 프레임 하나의 모든 절단 위치와 모든 비트
- 실제 구성요소: frame.rs
- 모의·시뮬레이션: 없음
- 주입한 장애: 절단, 비트 반전, 0 채움
- 판정 기준과 근거: CRC 기대값은 Python zlib.crc32로 따로 계산했다
- 재현: `cargo test -p oms-journal --lib`
- 뒷받침하는 것: ADR-011의 프레임
- 확인하지 않은 범위: 여러 비트의 동시 손상이 CRC를 통과할 확률(2^-32 수준)

| 시험 | 결과 | 비고 |
|---|---|---|
| `journal/frame::tests::roundtrip_and_layout` | 이전 버전 결과 | last result: passed |
| `journal/frame::tests::every_truncation_and_every_bit_flip_is_detected` | 이전 버전 결과 | last result: passed |
| `journal/frame::tests::zero_filled_tail_is_not_a_frame` | 이전 버전 결과 | last result: passed |
| `journal/frame::tests::finds_a_later_frame_behind_damage` | 이전 버전 결과 | last result: passed |

## V-JRN-2 — 저널의 끝이 불완전하면 유효한 접두부만 남기고 원본 바이트를 보존한 뒤 이어 쓸 수 있다. 중간이 손상됐거나 세그먼트가 빠졌거나 다른 인스턴스의 것이거나 스냅샷보다 짧으면 시작을 거부하고, 거부한 디렉터리의 파일은 건드리지 않는다

- 상태: **이전 버전 결과**
- 왜 중요한가: 손상을 건너뛰고 복구했다고 말하면 안 된다
- 수준: 속성
- 전제와 범위: 마지막 기록의 모든 절단 위치, 임의 크기·배치·절단 위치(property test 64회), 쓰레기·0 꼬리, 중간 비트 반전, 세그먼트 누락, 회전 중 중단. 실제 노드에서 꼬리와 중간 손상 각 1회
- 실제 구성요소: 저널 파일(파일을 직접 조작해 손상을 만든다)
- 모의·시뮬레이션: 장애 자체. 실제 전원 장애가 만드는 파일 모양이 아니다
- 주입한 장애: 절단, 덧붙은 쓰레기, 비트 반전, 파일 삭제
- 판정 기준과 근거: 복구된 기록 수 = 절단 지점 이전에 완전히 들어간 프레임 수. 중간 손상 뒤 파일 내용이 그대로
- 재현: `cargo test -p oms-journal && cargo test -p oms-node --test e2e journal_damage`
- 뒷받침하는 것: C-JOURNAL
- 확인하지 않은 범위: 전원 장애가 실제로 남기는 파일 모양. 마지막 기록만의 손상은 꼬리로 처리된다는 한계를 시험이 고정한다

| 시험 | 결과 | 비고 |
|---|---|---|
| `journal/every_truncation_of_the_last_record_recovers_the_prefix` | 이전 버전 결과 | last result: passed |
| `journal/recovery_returns_the_longest_valid_prefix` | 이전 버전 결과 | last result: passed |
| `journal/garbage_and_zero_tails_are_truncated` | 이전 버전 결과 | last result: passed |
| `journal/damage_in_the_middle_is_refused_and_nothing_is_modified` | 이전 버전 결과 | last result: passed |
| `journal/a_missing_middle_segment_is_a_gap` | 이전 버전 결과 | last result: passed |
| `journal/an_incomplete_newest_segment_is_removed_and_preserved` | 이전 버전 결과 | last result: passed |
| `journal/truncation_can_be_disabled` | 이전 버전 결과 | last result: passed |
| `journal/damage_in_the_last_record_is_treated_as_an_incomplete_tail` | 이전 버전 결과 | last result: passed |
| `journal/rotation_keeps_sequences_consecutive_across_segments` | 이전 버전 결과 | last result: passed |
| `journal/verify_all_reads_without_modifying` | 이전 버전 결과 | last result: passed |
| `journal/records_are_durable_only_after_sync` | 이전 버전 결과 | last result: passed |
| `journal/appends_must_be_consecutive` | 이전 버전 결과 | last result: passed |
| `e2e/journal_damage_is_handled_by_position` | 이전 버전 결과 | last result: passed |
| `journal/a_foreign_instance_journal_is_refused_without_being_modified` | 이전 버전 결과 | last result: passed |
| `journal/a_snapshot_newer_than_the_journal_is_refused_without_modifying_it` | 이전 버전 결과 | last result: passed |
| `journal/a_snapshot_without_any_segment_is_not_accepted_silently` | 이전 버전 결과 | last result: passed |
| `journal/header_only_newest_segment_is_clean` | 이전 버전 결과 | last result: passed |
| `journal/stale_and_out_of_order_frames_are_classified_conservatively` | 이전 버전 결과 | last result: passed |
| `journal/an_incomplete_newest_segment_after_a_snapshot_at_the_boundary_is_set_aside` | 이전 버전 결과 | last result: passed |

## V-JRN-3 — 같은 데이터 디렉터리를 두 프로세스가 쓸 수 없고, 다른 인스턴스의 저널은 거부된다

- 상태: **이전 버전 결과**
- 왜 중요한가: 두 writer는 순번과 주문 ID를 충돌시킨다
- 수준: 프로세스
- 전제와 범위: 같은 호스트
- 실제 구성요소: 파일 잠금, oms-node 프로세스 둘
- 모의·시뮬레이션: 없음
- 주입한 장애: 두 번째 프로세스 기동
- 판정 기준과 근거: 두 번째 프로세스의 종료 코드 3과 오류 문구. 첫 번째는 계속 동작
- 재현: `cargo test -p oms-node --test e2e only_one_node`
- 뒷받침하는 것: ADR-013
- 확인하지 않은 범위: 다른 호스트의 사본. 네트워크 파일 시스템

| 시험 | 결과 | 비고 |
|---|---|---|
| `journal/a_second_writer_is_refused` | 이전 버전 결과 | last result: passed |
| `journal/a_journal_of_another_instance_is_refused` | 이전 버전 결과 | last result: passed |
| `e2e/only_one_node_can_own_a_data_directory` | 이전 버전 결과 | last result: passed |

## V-JRN-4 — 스냅샷은 완전한 파일로만 나타나고, 손상된 스냅샷은 건너뛰며, 정상 종료 뒤 재시작은 스냅샷에서 같은 상태로 돌아온다

- 상태: **이전 버전 결과**
- 왜 중요한가: 스냅샷이 복구를 오염시키면 안 된다
- 수준: 프로세스
- 전제와 범위: 스냅샷 파일의 모든 절단 위치, 비트 반전, 임시 파일 잔존. 실제 노드의 정상 종료·재시작 1회
- 실제 구성요소: 스냅샷 파일, oms-node
- 모의·시뮬레이션: 손상은 파일 조작
- 주입한 장애: 절단, 비트 반전
- 판정 기준과 근거: 가장 새로운 유효 스냅샷이 선택됨. 재시작 뒤 주문·포지션이 같음
- 재현: `cargo test -p oms-journal --lib snapshot`
- 뒷받침하는 것: C-SNAP
- 확인하지 않은 범위: 스냅샷 쓰기 도중의 강제 종료를 겨냥한 시험

| 시험 | 결과 | 비고 |
|---|---|---|
| `journal/snapshot::tests::newest_valid_snapshot_wins_and_damaged_ones_are_skipped` | 이전 버전 결과 | last result: passed |
| `journal/snapshot::tests::truncated_snapshot_is_rejected` | 이전 버전 결과 | last result: passed |
| `e2e/vertical_slice_new_partial_fill_cancel_restart` | 이전 버전 결과 | last result: passed |
| `e2e/a_snapshot_whose_digest_does_not_match_is_skipped_not_fatal` | 이전 버전 결과 | last result: passed |

## V-JRN-5 — 기록 스키마가 다른 빌드는 옛 세그먼트를 읽고 새 세그먼트를 시작하며, 읽을 수 없는 스키마를 만난 빌드는 시작을 거부한다

- 상태: **이전 버전 결과**
- 왜 중요한가: 형식 변경이 조용한 오독으로 이어지면 안 된다
- 수준: 단위
- 전제와 범위: 스키마 1 → 2 전환 1회(저널 계층)
- 실제 구성요소: 저널 파일
- 모의·시뮬레이션: 스키마 2의 내용은 임의 바이트
- 주입한 장애: 없음
- 판정 기준과 근거: 세그먼트 수와 기록별 스키마 번호
- 재현: `cargo test -p oms-journal a_schema_change`
- 뒷받침하는 것: ADR-011의 버전 호환 표
- 확인하지 않은 범위: 실제 스키마 2가 없다. 옛 저널 파일을 새 빌드로 읽는 회귀 시험이 없다

| 시험 | 결과 | 비고 |
|---|---|---|
| `journal/a_schema_change_starts_a_new_segment` | 이전 버전 결과 | last result: passed |
| `journal/a_schema_change_right_after_a_rotation_can_still_write` | 이전 버전 결과 | last result: passed |

## V-BOOT-1 — 재시작은 결과 기록이 없는 전송을 불명으로 표시하고, 신규 주문을 다시 보내지 않으며, 요청돼 있던 취소만 다시 보낸다

- 상태: **이전 버전 결과**
- 왜 중요한가: 재시작이 주문을 복제하면 안 된다
- 수준: 프로세스
- 전제와 범위: 코어 예제 1건, 실제 프로세스에서 전송 중 강제 종료 1회(거래소가 400 ms 지연 중)
- 실제 구성요소: e2e에서는 oms-node, 저널, 소켓
- 모의·시뮬레이션: 거래소
- 주입한 장애: 전송 중 SIGKILL
- 판정 기준과 근거: 재시작 뒤 전송 명령 0건, 가상 거래소의 접수 요청 1건, 저널에 재시작 사유의 불명 기록
- 재현: `cargo test -p oms-node --test e2e a_kill_during_send`
- 뒷받침하는 것: C-BOOT, I-ONCE
- 확인하지 않은 범위: 내구화와 전송 사이의 각 경계는 V-COMMIT-4가 따로 겨냥한다

| 시험 | 결과 | 비고 |
|---|---|---|
| `core/restart_puts_outstanding_sends_in_doubt_and_never_resends` | 이전 버전 결과 | last result: passed |
| `e2e/a_kill_during_send_does_not_duplicate_the_order` | 이전 버전 결과 | last result: passed |

## V-CFG-1 — 설정은 버전이 커야 활성화되고, 같은 버전에 다른 내용이면 시작을 거부한다. 노출이 남은 종목·전략은 제거할 수 없다. mainnet은 증거금 정책(policy.margin) 없이는 시작하지 않고, 정책의 값이 C-MARGIN 설정 표의 범위를 벗어나면 어느 거래소 설정에서도 시작하지 않는다

- 상태: **이전 버전 결과**
- 왜 중요한가: 설정 변경이 추적되지 않으면 사고 분석이 불가능하다
- 수준: 프로세스
- 전제와 범위: 코어 예제, 실제 노드 1회, 노드 설정 검사의 단위 시험(증거금 정책)
- 실제 구성요소: oms-core, oms-node
- 모의·시뮬레이션: 없음
- 주입한 장애: 버전을 올리지 않은 변경. 정책 없는 mainnet 설정, 0인 한도, 신선도 한도보다 크지 않은 보고 끊김 취소 한도
- 판정 기준과 근거: 종료 코드 3과 오류 문구
- 재현: `cargo test -p oms-node --test e2e configuration_changes && cargo test -p oms-node --lib config::tests`
- 뒷받침하는 것: 요구 R13, C-BOOT
- 확인하지 않은 범위: 설정 파일의 그 밖의 검증 규칙. 조회 weight 예산 검사와 runtime.venue_setting_poll_ms(뒤 티켓). account_modes_allowed의 내용

| 시험 | 결과 | 비고 |
|---|---|---|
| `core/configuration_changes_are_validated_against_existing_exposure` | 이전 버전 결과 | last result: passed |
| `e2e/configuration_changes_need_a_new_version` | 이전 버전 결과 | last result: passed |
| `core/f10_config_validation_enforces_the_parked_fill_wait_rule` | 이전 버전 결과 | last result: passed |
| `node-unit/config::tests::mainnet_is_refused_without_a_margin_policy_and_bad_margin_values_are_refused` | 이전 버전 결과 | last result: passed |

## V-OVL-1 — 입력 큐는 상한을 넘지 않고, 가득 찬 큐는 다른 큐에 영향을 주지 않으며, 배치마다 세 큐가 지분을 받는다

- 상태: **이전 버전 결과**
- 왜 중요한가: 과부하가 메모리 고갈이나 취소 굶주림으로 바뀌면 안 된다
- 수준: 단위
- 전제와 범위: 큐 자료구조
- 실제 구성요소: queues.rs
- 모의·시뮬레이션: 없음
- 주입한 장애: 가득 참
- 판정 기준과 근거: 깊이와 거절 수를 직접 센다. 배치 8에서 4·2·2
- 재현: `cargo test -p oms-node --lib queues`
- 뒷받침하는 것: C-OVERLOAD
- 확인하지 않은 범위: 여러 스레드가 동시에 넣는 경합

| 시험 | 결과 | 비고 |
|---|---|---|
| `node-unit/queues::tests::a_full_lane_refuses_and_never_grows` | 이전 버전 결과 | last result: passed |
| `node-unit/queues::tests::every_lane_gets_a_share_of_each_batch` | 이전 버전 결과 | last result: passed |
| `node-unit/queues::tests::ticks_coalesce` | 이전 버전 결과 | last result: passed |
| `node-unit/queues::tests::close_drains_then_ends` | 이전 버전 결과 | last result: passed |

## V-OVL-2 — 큐보다 빠르게 온 요청은 즉시 거절되고 기록을 남기지 않으며, 그 직후에도 취소는 수용된다

- 상태: **이전 버전 결과**
- 왜 중요한가: 과부하 보호가 실제 프로세스에서 작동하는지
- 수준: 프로세스
- 전제와 범위: 주문 큐 8, 한 연결에서 600건을 응답을 읽지 않고 전송. 1회
- 실제 구성요소: oms-node 프로세스
- 모의·시뮬레이션: 거래소
- 주입한 장애: 요청 폭주
- 판정 기준과 근거: 큐 최고 수위 ≤ 8, 거절 수 = 진입 계수기, 코어의 수용·거절 계수(저널에서 복원되는 값)의 증가 = 코어가 답한 요청 수, 폭주 직후 취소 응답
- 재현: `cargo test -p oms-node --test e2e overload -- --nocapture`
- 뒷받침하는 것: C-OVERLOAD, 요구 R09
- 확인하지 않은 범위: 거래소 입력 큐가 가득 차는 경우. 여러 전략 사이의 공정성

| 시험 | 결과 | 비고 |
|---|---|---|
| `e2e/overload_is_refused_at_the_door_and_cancels_still_work` | 이전 버전 결과 | last result: passed |

## V-OVL-3 — 저장 공간이 부족하면 신규 주문만 거절하고 체결과 취소는 계속 처리한다

- 상태: **이전 버전 결과**
- 왜 중요한가: 저장 공간 부족이 체결 유실로 이어지면 안 된다
- 수준: 단위
- 전제와 범위: 코어 예제
- 실제 구성요소: oms-core
- 모의·시뮬레이션: 저장 공간 보고는 시험이 입력
- 주입한 장애: 저장 공간 부족 보고
- 판정 기준과 근거: 거절 사유와 포지션 변화
- 재현: `cargo test -p oms-core --test scenarios storage_low`
- 뒷받침하는 것: C-OVERLOAD
- 확인하지 않은 범위: 실제로 디스크를 채우는 시험. statvfs 측정 경로

| 시험 | 결과 | 비고 |
|---|---|---|
| `core/storage_low_blocks_new_orders_but_not_fills_or_cancels` | 이전 버전 결과 | last result: passed |

## V-API-1 — 전략 연결은 자기 주문만 보고 취소하며, 운영 명령은 운영 소켓에서만 받아들여진다. 알 수 없는 전략과 잘못된 요청은 거절된다

- 상태: **이전 버전 결과**
- 왜 중요한가: 권한 경계
- 수준: 프로세스
- 전제와 범위: 두 전략, 거래 소켓과 운영 소켓. 같은 OS 사용자
- 실제 구성요소: oms-node 프로세스와 소켓
- 모의·시뮬레이션: 거래소
- 주입한 장애: 권한 밖 요청, 잘못된 요청
- 판정 기준과 근거: 오류 코드(forbidden, unauthorized, bad_request, not_owner)
- 재현: `cargo test -p oms-node --test e2e a_strategy_sees`
- 뒷받침하는 것: ADR-015, C-API
- 확인하지 않은 범위: 다른 uid의 접속. 거대한 줄. 대시보드 HTTP는 V-DASH-1과 수동 확인

| 시험 | 결과 | 비고 |
|---|---|---|
| `e2e/a_strategy_sees_and_cancels_only_its_own_orders` | 이전 버전 결과 | last result: passed |
| `e2e/external_activity_is_detected_and_needs_an_operator` | 이전 버전 결과 | last result: passed |

## V-API-2 — 문서의 요청 예가 그대로 해석되고, 수치에 JSON 숫자를 쓰면 거부된다

- 상태: **이전 버전 결과**
- 왜 중요한가: 프로토콜 문서와 구현의 일치
- 수준: 단위
- 전제와 범위: 신규 주문 요청과 응답의 예
- 실제 구성요소: oms-api 타입
- 모의·시뮬레이션: 없음
- 주입한 장애: 수치를 숫자로 보냄
- 판정 기준과 근거: 문서에 적힌 JSON 문자열과의 정확한 일치
- 재현: `cargo test -p oms-api`
- 뒷받침하는 것: C-API
- 확인하지 않은 범위: 나머지 요청 종류의 예

| 시험 | 결과 | 비고 |
|---|---|---|
| `api/tests::wire_examples` | 이전 버전 결과 | last result: passed |

## V-SLICE-1 — 신규 주문 → 영속 접수 → 거래소 접수 → 부분 체결 → 잔량 취소 → 종결이 실제 프로세스에서 이어지고, 구독자는 저널 순서로 이벤트를 받으며, 정상 종료 뒤 재시작해도 상태가 같고 재전송이 없다

- 상태: **이전 버전 결과**
- 왜 중요한가: 첫 수직 구현의 완료 조건
- 수준: 프로세스
- 전제와 범위: 주문 1건, 1회
- 실제 구성요소: oms-node, 저널, 스냅샷, 소켓, 게이트웨이, 오프라인 도구(verify, trace)
- 모의·시뮬레이션: 거래소
- 주입한 장애: 없음(정상 경로와 정상 종료)
- 판정 기준과 근거: 단계별 주문 상태와 예약, 포지션 0.4, 가상 거래소의 접수 요청 1건, verify --re-decide 성공
- 재현: `cargo test -p oms-node --test e2e vertical_slice`
- 뒷받침하는 것: 전체 경로의 연결
- 확인하지 않은 범위: 실제 거래소

| 시험 | 결과 | 비고 |
|---|---|---|
| `e2e/vertical_slice_new_partial_fill_cancel_restart` | 이전 버전 결과 | last result: passed |
| `e2e/a_lost_response_is_covered_by_stream_evidence` | 이전 버전 결과 | last result: passed |

## V-LINK-1 — 스트림이 끊기면 신규 주문을 막고, 그동안의 취소 요청을 기억했다가 복귀 뒤 보내며, 놓친 체결을 메운 뒤에야 거래를 재개한다

- 상태: **이전 버전 결과**
- 왜 중요한가: 실제 소켓 단절에서의 재확인
- 수준: 프로세스
- 전제와 범위: 단절 1회, 그 사이 체결 1건과 취소 요청 1건
- 실제 구성요소: oms-node, 소켓, 대조기
- 모의·시뮬레이션: 거래소(스트림을 끊고 재구독을 거부하다 허용)
- 주입한 장애: 스트림 단절
- 판정 기준과 근거: 거절 사유, 복귀 뒤 주문의 누적량 0.25와 종결
- 재현: `cargo test -p oms-node --test e2e link_loss`
- 뒷받침하는 것: C-LINK, C-RECON
- 확인하지 않은 범위: 실제 거래소의 단절 양상

| 시험 | 결과 | 비고 |
|---|---|---|
| `e2e/link_loss_holds_new_orders_until_resynchronised` | 이전 버전 결과 | last result: passed |

## V-HL-1 — L1 액션의 해시와 서명이 공식 Python SDK의 결과와 같고, 서명 키는 출력되지 않는다

- 상태: **이전 버전 결과**
- 왜 중요한가: 서명이 한 바이트라도 다르면 모든 요청이 거부된다
- 수준: 단위
- 전제와 범위: SDK 시험 벡터 5종(액션 해시, 임의 액션, 주문, cloid 주문, 예약 취소 2종; mainnet과 testnet). expiresAfter는 바이트 배치만
- 실제 구성요소: sign.rs
- 모의·시뮬레이션: 없음
- 주입한 장애: 잘못된 키 형식
- 판정 기준과 근거: hyperliquid-python-sdk commit 2fdb18f의 tests/signing_test.py에 적힌 r, s, v 값. 주소는 널리 알려진 시험 키의 주소
- 재현: `cargo test -p oms-node --lib sign`
- 뒷받침하는 것: ADR-017의 서명
- 확인하지 않은 범위: 거래소가 실제로 수락하는지. expiresAfter가 붙은 서명의 SDK 벡터는 없다. 취소(cancelByCloid) 액션의 벡터도 없다

| 시험 | 결과 | 비고 |
|---|---|---|
| `node-unit/venue::hyperliquid::sign::tests::action_hash_matches_the_sdk_vector` | 이전 버전 결과 | last result: passed |
| `node-unit/venue::hyperliquid::sign::tests::dummy_action_signature_matches_the_sdk_vectors` | 이전 버전 결과 | last result: passed |
| `node-unit/venue::hyperliquid::sign::tests::order_signature_matches_the_sdk_vectors` | 이전 버전 결과 | last result: passed |
| `node-unit/venue::hyperliquid::sign::tests::order_with_cloid_signature_matches_the_sdk_vectors` | 이전 버전 결과 | last result: passed |
| `node-unit/venue::hyperliquid::sign::tests::schedule_cancel_signature_matches_the_sdk_vectors` | 이전 버전 결과 | last result: passed |
| `node-unit/venue::hyperliquid::sign::tests::expires_after_is_appended_as_in_the_sdk_source` | 이전 버전 결과 | last result: passed |
| `node-unit/venue::hyperliquid::sign::tests::the_address_is_derived_and_the_key_is_not_printed` | 이전 버전 결과 | last result: passed |

## V-HL-2 — 문서에 실린 응답 예를 올바르게 해석한다. 문서에 없는 주문 응답 형태와 최상위 오류는 결과 불명으로 두고, 문서의 표에 없는 상태 이름에서는 아무 결론도 내지 않으며, 종결 상태에서 최종 수량을 추정하지 않는다. 체결 조회는 마지막 쪽임이 확실할 때만 완료를 선언한다

- 상태: **이전 버전 결과**
- 왜 중요한가: 모르는 응답을 실패로 읽으면 예약 없는 주문이 생긴다
- 수준: 단위
- 전제와 범위: 문서의 주문·취소 응답 예, 상태 이름 표 전체(29개), 체결 필드, 체결 쪽 판정의 경계(499, 500, 2000, 겹침만 있는 쪽, 10000건)
- 실제 구성요소: wire.rs
- 모의·시뮬레이션: 응답은 문서의 예와 변형
- 주입한 장애: 빈 응답, 상태 2개, 문서에 없는 상태값, 주문 ID 없는 체결 응답
- 판정 기준과 근거: Hyperliquid 문서의 응답 예
- 재현: `cargo test -p oms-node --lib hyperliquid`
- 뒷받침하는 것: C-GW의 G-2, G-8, G-11
- 확인하지 않은 범위: 실제 응답. HTTP 오류와 시간 초과의 분류, 스트림 오류 채널과 구독 시간 제한, 예약 취소의 거부 처리는 코드 검토뿐이다(네트워크 모의 시험 없음)

| 시험 | 결과 | 비고 |
|---|---|---|
| `node-unit/venue::hyperliquid::wire::tests::documented_place_answers` | 이전 버전 결과 | last result: passed |
| `node-unit/venue::hyperliquid::wire::tests::anything_else_is_unclear_not_a_failure` | 이전 버전 결과 | last result: passed |
| `node-unit/venue::hyperliquid::wire::tests::documented_cancel_answers` | 이전 버전 결과 | last result: passed |
| `node-unit/venue::hyperliquid::wire::tests::status_mapping` | 이전 버전 결과 | last result: passed |
| `node-unit/venue::hyperliquid::wire::tests::fill_translation` | 이전 버전 결과 | last result: passed |
| `node-unit/venue::hyperliquid::wire::tests::order_wire_json` | 이전 버전 결과 | last result: passed |
| `node-unit/venue::hyperliquid::tests::the_budget_stops_orders_before_cancels` | 이전 버전 결과 | last result: passed |
| `node-unit/venue::hyperliquid::wire::tests::odd_coin_names_are_mapped_not_dropped` | 이전 버전 결과 | last result: passed |
| `node-unit/venue::hyperliquid::tests::a_fill_page_proves_completeness_only_when_it_is_certainly_the_last` | 이전 버전 결과 | last result: passed |
| `e2e/hyperliquid_fills_on_spot_coins_are_translatable` | 이전 버전 결과 | last result: passed |

## V-HL-3 — 실제 testnet에 대해 게이트웨이가 메타를 읽어 종목 규칙을 통과시키고, 사용자 스트림을 구독해 링크를 올리고, 주문 조회(없음)·열린 주문·포지션·체결 조회·참조 가격을 해석한다

- 상태: **미실행**
- 왜 중요한가: 게이트웨이가 파싱하는 형태가 실제 거래소의 응답과 맞는지
- 수준: 실제 거래소(읽기 전용)
- 전제와 범위: 계정이 없는 주소. 서명한 요청을 보내지 않는다. 1회
- 실제 구성요소: 게이트웨이 전체(HTTP, WebSocket, TLS), Hyperliquid testnet
- 모의·시뮬레이션: 코어 대신 입력 큐를 직접 읽는다
- 주입한 장애: 없음
- 판정 기준과 근거: 링크 올라옴, StatusNotFound, 빈 열린 주문·포지션, 과거 시각의 체결 동기화 선언, 양수의 BTC 참조 가격
- 재현: `OMS_LIVE_TESTNET=1 cargo test -p oms-node --test hl_live -- --nocapture`
- 뒷받침하는 것: 조회 경로와 스트림 연결
- 확인하지 않은 범위: 주문, 취소, 체결, 만료, 한도, 주문이 있는 계정의 응답 형태. 이 시험은 기본 수용 시험에 포함되지 않으며 `--live-testnet`으로 실행한다

| 시험 | 결과 | 비고 |
|---|---|---|
| `hl-live/the_gateway_reads_the_real_testnet` | 미실행 |  |

## V-PURE-1 — 도메인 코어의 소스에 시계·파일·네트워크·스레드·난수·해시 컬렉션·부동소수점·unsafe가 없고 의존성은 serde와 serde_json뿐이다

- 상태: **이전 버전 결과**
- 왜 중요한가: 결정적 실행의 전제
- 수준: 소스 검사
- 전제와 범위: crates/oms-core/src의 모든 줄(주석 제외)과 Cargo.toml
- 실제 구성요소: 소스 텍스트
- 모의·시뮬레이션: 없음
- 주입한 장애: 없음
- 판정 기준과 근거: 금지 문자열 목록과의 대조
- 재현: `cargo run -p xtask -- verify --quick`
- 뒷받침하는 것: ADR-002
- 확인하지 않은 범위: 문자열 대조이므로 매크로나 재수출을 통한 우회는 잡지 못한다. 의존 crate 내부는 보지 않는다

| 시험 | 결과 | 비고 |
|---|---|---|
| `source/core_purity` | 이전 버전 결과 | last result: passed |

## V-PERF-1 — 초당 200건의 open-loop 부하에서 로컬 응답 지연 p99가 20 ms 이하이고 오류와 미응답이 없다

- 상태: **이전 버전 결과**
- 왜 중요한가: 수용 기준 A-LAT-1, A-LAT-2
- 수준: 측정
- 전제와 범위: docs/performance/README.md의 조건. 한 호스트, 한 장치, 가상 거래소
- 실제 구성요소: oms-node, 저널(fdatasync), 소켓
- 모의·시뮬레이션: 거래소(자동 전량 체결, 같은 호스트)
- 주입한 장애: 없음
- 판정 기준과 근거: 지연은 의도한 전송 시각부터 잰다. 표본 전부를 보관해 정확한 백분위를 낸다
- 재현: `cargo run --release -p xtask -- perf --dir <NVMe 디렉터리> --rates 20,200,2000 --seconds 60 --label nvme-final`
- 뒷받침하는 것: A-LAT-1, A-LAT-2
- 확인하지 않은 범위: 실제 거래소 왕복. 다른 장치. 장시간 실행

| 시험 | 결과 | 비고 |
|---|---|---|
| `perf/nvme-final` | 이전 버전 결과 | 20/s: p99 4.06 ms, unanswered 0, error kinds 0; 200/s: p99 9.06 ms, unanswered 0, error kinds 0 |

## V-PERF-2 — 초당 2000건의 부하에서 (가) 수용 한도를 푼 설정에서는 큐가 상한 안에 머물고 미응답이 없으며, (나) 기본 한도에서는 초과분이 overloaded로 즉시 거절되고 저널에 기록을 남기지 않는다

- 상태: **이전 버전 결과**
- 왜 중요한가: 수용 기준 A-CAP-1
- 수준: 측정
- 전제와 범위: 초당 2000건, 60초. 설정 둘: 한도를 푼 것(nvme-final)과 기본 한도(nvme-default-limits). 결과 파일에 노드 설정 전문이 들어 있다
- 실제 구성요소: V-PERF-1과 같다
- 모의·시뮬레이션: V-PERF-1과 같다
- 주입한 장애: 과부하
- 판정 기준과 근거: (가) 큐 최고 수위 ≤ 상한, 미응답 0. (나) 미응답 0, 오류 종류가 overloaded뿐, 거절이 실제로 일어남, 코어의 수용·거절 계수 = 코어가 답한 요청 수
- 재현: `xtask perf --label nvme-final --rates 20,200,2000 --seconds 60  /  xtask perf --label nvme-default-limits --rates 2000 --seconds 60 --default-limits`
- 뒷받침하는 것: A-CAP-1, ADR-012의 비교
- 확인하지 않은 범위: (가)의 설정은 운영 설정이 아니다: 연결당 창 512, 전략 빈도 한도 초당 100만, 열린 주문 10만, 진행 중 전송 4096. 파이프라인이 감당하는 양을 재기 위한 것이다

| 시험 | 결과 | 비고 |
|---|---|---|
| `perf/nvme-final` | 이전 버전 결과 | 2000/s [commit_thread, limits lifted]: p99 19.61 ms, unanswered 0, queues within capacity: true, error kinds 0 |
| `perf/nvme-default-limits` | 이전 버전 결과 | 2000/s with shipped limits: 116901 refused as overloaded, 3099 decided by the core, 3099 decisions in the journal, unanswered 0, queues within capacity: true |

## V-PERF-3 — 스냅샷 이후 기록 5만 건의 재생이 5초 이내에 끝난다

- 상태: **이전 버전 결과**
- 왜 중요한가: 수용 기준 A-REC-2
- 수준: 측정
- 전제와 범위: 성능 측정으로 만든 저널을 스냅샷 없이 재시작
- 실제 구성요소: oms-node 복구 경로, 저널 파일
- 모의·시뮬레이션: 없음
- 주입한 장애: 없음
- 판정 기준과 근거: 복구 보고서의 elapsed_ms와 records_replayed
- 재현: `cargo run --release -p xtask -- recovery-time --dir <디렉터리>`
- 뒷받침하는 것: A-REC-2
- 확인하지 않은 범위: 열린 주문이 많은 상태. 다른 장치

| 시험 | 결과 | 비고 |
|---|---|---|
| `perf/recovery` | 이전 버전 결과 | 54023 records replayed in 135 ms |

## V-MUT-1 — 핵심 보호 로직을 일부러 깨뜨리면 관련 시험이 실패한다

- 상태: **이전 버전 결과**
- 왜 중요한가: 시험이 실제로 그 로직을 지키고 있는지
- 수준: 변이
- 전제와 범위: 선별한 변이 목록(docs/verification/README.md). 각 변이는 소스의 한 곳을 바꾼다
- 실제 구성요소: 작업 공간의 복사본을 고쳐 해당 시험을 돌린다
- 모의·시뮬레이션: 없음
- 주입한 장애: 보호 로직 제거
- 판정 기준과 근거: 변이마다 지정한 시험이 실패해야 한다. 통과하면 그 변이는 '살아남음'으로 보고한다
- 재현: `cargo run -p xtask -- mutants`
- 뒷받침하는 것: 시험의 민감도
- 확인하지 않은 범위: 목록에 없는 변이. 전수 변이 시험이 아니다

| 시험 | 결과 | 비고 |
|---|---|---|
| `mutants/all` | 이전 버전 결과 | 31 mutants, survived: [] |

## V-HIST-1 — 단계 지연 히스토그램이 보고하는 백분위는 실제 값 이상이고 12.5% 이내로 크다

- 상태: **이전 버전 결과**
- 왜 중요한가: 대시보드의 지연 수치를 얼마나 믿을 수 있는지
- 수준: 단위
- 전제와 범위: 0~5000과 큰 값 몇 개
- 실제 구성요소: util.rs의 Hist
- 모의·시뮬레이션: 없음
- 주입한 장애: 없음
- 판정 기준과 근거: 버킷 상한과 값의 관계를 직접 계산
- 재현: `cargo test -p oms-node --lib util`
- 뒷받침하는 것: 운영 화면의 지연 표시
- 확인하지 않은 범위: 부하 생성기의 지연은 이 히스토그램을 쓰지 않는다(정확한 값)

| 시험 | 결과 | 비고 |
|---|---|---|
| `node-unit/util::tests::histogram_bounds_hold_for_every_value` | 이전 버전 결과 | last result: passed |
| `node-unit/util::tests::quantiles_of_a_known_distribution` | 이전 버전 결과 | last result: passed |

## V-COMMIT-4 — 커밋 순서의 경계마다 프로세스를 멈추면 결과가 경계별 장애 표와 같다: 쓰기 전에 멈추면 요청이 없었던 것이 되고, 쓴 뒤·flush 뒤·응답 뒤에 멈추면 주문은 남되 한 번도 전송되지 않고 결과 불명을 거쳐 "놓이지 않음"으로 닫히며, 같은 요청을 다시 보내면 같은 주문으로 답한다

- 상태: **이전 버전 결과**
- 왜 중요한가: C-COMMIT의 표가 설계에 그치지 않고 실제 프로세스에서 성립하는지. 간격 검토에서 이 표에 대응하는 시험이 없다는 지적을 받았다
- 수준: 프로세스
- 전제와 범위: 경계 넷(쓰기 전, 쓴 뒤 flush 전, flush 뒤 공개 전, 응답 뒤 전송 명령 전)과 flush 오류 뒤 프로세스가 살아 있는 경우 하나. 각 1회, 주문 1건
- 실제 구성요소: oms-node 프로세스, 저널 파일, 소켓, 대조기, 재시작
- 모의·시뮬레이션: 거래소(simvenue). 멈춤은 failpoints 빌드에만 있는 지점에서 abort()로 만든다
- 주입한 장애: 지정한 경계에서의 프로세스 중단. flush 오류(쓴 뒤에 주입)
- 판정 기준과 근거: docs/contracts/commit.md의 경계별 장애 결과 표의 각 행: 전략이 본 응답, 가상 거래소의 접수 요청 수(0), 재시작 뒤 주문의 존재와 종결 사유, 같은 요청 재전송의 응답
- 재현: `cargo test -p oms-node --features failpoints --test crash_points`
- 뒷받침하는 것: C-COMMIT의 경계 표, I-WAL, C-BOOT
- 확인하지 않은 범위: 전원 장애: abort()는 OS가 받은 쓰기를 잃지 않으므로 "쓴 뒤 flush 전" 행에서 기록이 항상 남는다. 기록이 사라지는 쪽은 저널 crate의 꼬리 절단 시험이 다룬다. 게이트웨이 큐에 든 뒤와 전송 중의 경계는 V-BOOT-1의 전송 중 강제 종료 시험이 다룬다. 여러 주문이 한 배치에 든 경우

| 시험 | 결과 | 비고 |
|---|---|---|
| `pipeline/stopped_before_the_write_the_request_never_happened` | 이전 버전 결과 | last result: passed |
| `pipeline/stopped_after_the_write_the_order_exists_and_is_resolved_without_a_send` | 이전 버전 결과 | last result: passed |
| `pipeline/stopped_after_the_flush_the_order_is_in_doubt_and_never_sent` | 이전 버전 결과 | last result: passed |
| `pipeline/stopped_after_the_reply_the_accepted_order_is_never_sent_and_closes_as_not_placed` | 이전 버전 결과 | last result: passed |
| `pipeline/a_flush_error_after_the_write_stops_the_node_and_the_record_may_survive` | 이전 버전 결과 | last result: passed |

## V-COMMIT-5 — 실제 디스크에 둔 저널로 강제 종료를 60회 반복해도 V-COMMIT-2의 성질이 유지된다

- 상태: **이전 버전 결과**
- 왜 중요한가: 문서에 인용하는 더 긴 실행의 수치를 보관된 결과로 뒷받침한다
- 수준: 프로세스
- 전제와 범위: 60회. 저널은 `--extended`에 준 디렉터리(실제 장치)
- 실제 구성요소: V-COMMIT-2와 같다
- 모의·시뮬레이션: 거래소
- 주입한 장애: SIGKILL
- 판정 기준과 근거: V-COMMIT-2와 같다
- 재현: `cargo run --release -p xtask -- verify --extended <실제 디스크의 디렉터리>`
- 뒷받침하는 것: V-COMMIT-2의 확장
- 확인하지 않은 범위: V-COMMIT-2와 같다

| 시험 | 결과 | 비고 |
|---|---|---|
| `kills-extended/random_kills_under_load_keep_books_consistent` | 이전 버전 결과 | last result: passed |

## V-BOOT-2 — 저널이 여러 세그먼트에 걸쳐 있어도 강제 종료 뒤 스냅샷과 저널로 같은 상태로 돌아오고, `verify --re-decide`는 입력과 맞지 않는 기록을 잡아내며, 종료 중의 저널 실패는 정상 종료로 보고되지 않는다

- 상태: **이전 버전 결과**
- 왜 중요한가: 간격 검토가 찾은 결함(종료 중 실패의 종료 코드)과, 검토자가 깨려다 실패한 것의 기록
- 수준: 프로세스
- 전제와 범위: 4 KiB 세그먼트와 25건 주기 스냅샷으로 주문 60건. 체크섬이 맞는 기록 하나의 이벤트 조작. 데이터 디렉터리를 읽기 전용으로 만든 뒤 종료
- 실제 구성요소: oms-node 프로세스, 저널, 스냅샷
- 모의·시뮬레이션: 거래소
- 주입한 장애: SIGKILL, 기록 조작, 쓰기 불가
- 판정 기준과 근거: 가상 거래소의 접수 요청 수 60(주문당 1), 재결정 검사의 실패, 종료 코드 2
- 재현: `cargo test -p oms-node --test review_node`
- 뒷받침하는 것: C-JOURNAL, C-BOOT
- 확인하지 않은 범위: 전원 장애. 쓰기 불가는 디렉터리 권한으로 만든 것이라 실제 장치 오류와 다르다

| 시험 | 결과 | 비고 |
|---|---|---|
| `e2e/many_small_segments_survive_a_kill_and_a_clean_restart` | 이전 버전 결과 | last result: passed |
| `e2e/re_decide_detects_a_record_whose_events_differ_from_the_decision` | 이전 버전 결과 | last result: passed |
| `e2e/a_journal_failure_during_the_shutdown_drain_is_not_reported_as_a_clean_stop` | 이전 버전 결과 | last result: passed |

## V-API-3 — 거래 소켓의 연결이 아무리 많아도 운영 소켓은 열리고, 끊긴 구독자는 연결 자리를 돌려주며, 한 연결은 구독을 하나만 가진다

- 상태: **이전 버전 결과**
- 왜 중요한가: 간격 검토가 찾은 결함: 구독하고 떠난 연결 넷이 운영자까지 막을 수 있었다
- 수준: 프로세스
- 전제와 범위: 연결 상한 4에서 침묵하는 연결 4개, 구독 후 끊은 연결 4개, 한 연결의 구독 3000회
- 실제 구성요소: oms-node 프로세스와 소켓
- 모의·시뮬레이션: 거래소
- 주입한 장애: 침묵하는 연결, 끊긴 구독자, 구독 남용
- 판정 기준과 근거: 운영 소켓의 status 응답, 주문 하나에 대한 이벤트 전달 수
- 재현: `cargo test -p oms-node --test review_node`
- 뒷받침하는 것: C-API, C-OVERLOAD, ADR-015
- 확인하지 않은 범위: hello 시간 제한(10초) 자체. 다른 uid의 접속. 느린 구독자의 끊김

| 시험 | 결과 | 비고 |
|---|---|---|
| `e2e/idle_trading_connections_do_not_lock_the_operator_out` | 이전 버전 결과 | last result: passed |
| `e2e/a_departed_subscriber_gives_its_connection_slot_back` | 이전 버전 결과 | last result: passed |
| `e2e/one_connection_cannot_register_thousands_of_subscriptions` | 이전 버전 결과 | last result: passed |

## V-DASH-1 — 대시보드의 문서 열람 경로는 docs/, crates/, registry/ 아래의 .md, .rs, .toml만 읽는다

- 상태: **이전 버전 결과**
- 왜 중요한가: 읽기 전용 화면이 설정 파일이나 데이터 디렉터리를 내보내면 안 된다
- 수준: 단위
- 전제와 범위: 허용되는 경로 4개와 거부돼야 하는 경로 14개
- 실제 구성요소: 경로 검사 함수
- 모의·시뮬레이션: 없음
- 주입한 장애: 상위 디렉터리 참조, 다른 디렉터리, 숨김 파일, 다른 확장자, 인코딩된 점
- 판정 기준과 근거: 함수의 반환값
- 재현: `cargo test -p oms-node --lib http`
- 뒷받침하는 것: ADR-016, ADR-015
- 확인하지 않은 범위: HTTP 서버 전체. GET 외 메서드 거부, Host 검사, 응답 헤더는 수동으로 확인했다(docs/verification/README.md §6). 화면의 자동 시험은 없다

| 시험 | 결과 | 비고 |
|---|---|---|
| `node-unit/http::tests::the_document_viewer_cannot_leave_its_directories` | 이전 버전 결과 | last result: passed |

## V-PERF-4 — 커밋 스레드 구성과 인라인 구성을 같은 조건에서 잰 결과가 보관돼 있다(ADR-012의 비교)

- 상태: **이전 버전 결과**
- 왜 중요한가: 구성 선택의 근거가 측정이라는 것을 결과 파일로 남긴다
- 수준: 측정
- 전제와 범위: 초당 200, 2000, 8000건을 10초씩. 수용 한도를 푼 설정
- 실제 구성요소: V-PERF-1과 같다
- 모의·시뮬레이션: V-PERF-1과 같다
- 주입한 장애: 과부하(8000건)
- 판정 기준과 근거: 판정 기준이 없는 측정이다. 두 결과 파일이 현재 소스에서 만들어졌으면 통과로 센다. 수치의 해석은 ADR-012에 있다
- 재현: `xtask perf --label nvme-compare-thread --rates 200,2000,8000 --seconds 10  /  같은 명령에 --inline --label nvme-compare-inline`
- 뒷받침하는 것: ADR-012
- 확인하지 않은 범위: 10초 실행이라 꼬리 지연의 신뢰도가 낮다. 실행 사이의 변동을 여러 번 재지 않았다

| 시험 | 결과 | 비고 |
|---|---|---|
| `perf/nvme-compare-thread` | 이전 버전 결과 | 200/s [commit_thread]: p99 8.60 ms, unanswered 0, error kinds 0; 2000/s [commit_thread]: p99 15.05 ms, unanswered 0, error kinds 0; 8000/s [commit_thread]: p99 16.88 ms, unanswered 0, error kinds 0 |
| `perf/nvme-compare-inline` | 이전 버전 결과 | 200/s [inline]: p99 8.44 ms, unanswered 0, error kinds 0; 2000/s [inline]: p99 11.29 ms, unanswered 0, error kinds 0; 8000/s [inline]: p99 1045.90 ms, unanswered 0, error kinds 1 |

## V-PERF-5 — 저장 장치의 동기 쓰기 시간과 코어의 판단 시간을 잰 결과가 보관돼 있다

- 상태: **이전 버전 결과**
- 왜 중요한가: 응답 지연의 하한(장치)과 코어의 몫을 구분하는 근거
- 수준: 측정
- 전제와 범위: 300바이트 append + fdatasync 300회. 주문 20만 건의 수명을 메모리에서 처리
- 실제 구성요소: 대상 장치의 파일, oms-core
- 모의·시뮬레이션: 없음
- 주입한 장애: 없음
- 판정 기준과 근거: 판정 기준이 없는 측정이다. 결과 파일이 현재 소스에서 만들어졌으면 통과로 센다
- 재현: `xtask fsync-probe --dir <디렉터리> --label nvme  /  xtask core-bench`
- 뒷받침하는 것: ADR-004, ADR-012, P-PERF-03
- 확인하지 않은 범위: 다른 장치. 장치가 flush 요청을 실제로 지키는지

| 시험 | 결과 | 비고 |
|---|---|---|
| `perf/fsync-nvme` | 이전 버전 결과 | write+fdatasync: p50 2505 p99 2897 us; fdatasync: p50 2494 p99 2877 us |
| `perf/core-bench` | 이전 버전 결과 | handle(new_order): p50 22557 p99 30614 ns; handle(send_result placed): p50 864 p99 1138 ns; handle(fill closing the order): p50 1251 p99 1640 ns; encode record (serde_json): p50 527 p99 1635 ns; record size: p50 467 p99 550 bytes |

## V-SIM-4 — 프로파일마다 seed 3000개로 V-SIM-1, V-SIM-2, V-EXT-1의 성질이 유지된다

- 상태: **이전 버전 결과**
- 왜 중요한가: 문서에 인용하는 더 긴 실행의 수치를 보관된 결과로 뒷받침한다
- 수준: 시뮬레이션
- 전제와 범위: 프로파일 7개 × seed 3000개 × 1500단계
- 실제 구성요소: oms-core
- 모의·시뮬레이션: V-SIM-1과 같다
- 주입한 장애: V-SIM-1과 같다
- 판정 기준과 근거: V-SIM-1, V-SIM-2와 같다
- 재현: `cargo run --release -p xtask -- verify --extended <실제 디스크의 디렉터리>`
- 뒷받침하는 것: V-SIM-1의 확장
- 확인하지 않은 범위: V-SIM-1과 같다

| 시험 | 결과 | 비고 |
|---|---|---|
| `sim-extended/profile_calm` | 이전 버전 결과 | last result: passed |
| `sim-extended/profile_lossy` | 이전 버전 결과 | last result: passed |
| `sim-extended/profile_crashy` | 이전 버전 결과 | last result: passed |
| `sim-extended/profile_weak_evidence` | 이전 버전 결과 | last result: passed |
| `sim-extended/profile_purge` | 이전 버전 결과 | last result: passed |
| `sim-extended/profile_skewed` | 이전 버전 결과 | last result: passed |
| `sim-extended/profile_external` | 이전 버전 결과 | last result: passed |

## V-FUZZ-1 — 가정을 지키지 않는 임의의 거래소 입력(순서 역전, 중복, 낡은 상태, 식별자 없는 체결, 부재 응답, 링크 변동, 재시작)에서도 코어는 결함으로 멈추지 않고, 불변조건과 재생 일치가 유지되고, 주문당 전송은 한 번이며, 차단 중 수용이 없고, 새 체결은 반드시 기장되거나 대기하고, 정직하게 정리된 뒤에는 차단이 남지 않는다

- 상태: **이전 버전 결과**
- 왜 중요한가: 간격 검토에서 검토자가 코어를 깨려고 쓴 시험이다. 깨지지 않은 범위를 기록으로 남긴다
- 수준: 속성
- 전제와 범위: seed 3000개 × 800단계. 한 종목, 두 전략. 거래소 주문 ID와 체결 키는 일관되게 유지한다
- 실제 구성요소: oms-core
- 모의·시뮬레이션: 입력 전부
- 주입한 장애: 가정 위반 입력
- 판정 기준과 근거: 시험 안의 검사 아홉 가지와 끝의 정리 뒤 차단 없음
- 재현: `cargo test --release -p oms-core --test review_core examined_fuzz`
- 뒷받침하는 것: 구조 불변조건, I-ONCE, V-REPLAY-1
- 확인하지 않은 범위: 한 거래소 주문 ID가 두 주문에 붙는 입력(따로 시험이 있다: V-MIS-1). 수치의 극단값

| 시험 | 결과 | 비고 |
|---|---|---|
| `core/examined_fuzz_arbitrary_venue_input_keeps_invariants_and_liveness` | 이전 버전 결과 | last result: passed |

## V-MIS-1 — 불일치 14종 각각을 일으키는 시험이 있고, 같은 대상의 반복은 확인되기 전까지 한 건으로 남는다

- 상태: **이전 버전 결과**
- 왜 중요한가: 탐지 수단으로 문서에 적은 불일치가 실제로 제기되는지. 간격 검토에서 시험 없는 종류 다섯이 지적됐다
- 수준: 단위
- 전제와 범위: Overfill, FillAfterClose, CumExceedsFinal, MissingFills, ExternalFill, FillFieldConflict, OrphanOrder, ForeignOrder, OpenAfterClose, PositionOutOfRange, ForeignPosition, StatusConflict, CancelStuck, ParkedFillOverflow. 외부 체결 300건
- 실제 구성요소: oms-core
- 모의·시뮬레이션: 입력은 시험이 넣는다
- 주입한 장애: 각 불일치의 원인
- 판정 기준과 근거: 제기된 불일치의 종류와 수, 장부의 값
- 재현: `cargo test -p oms-core`
- 뒷받침하는 것: C-MISMATCH, 가정들의 탐지 칸
- 확인하지 않은 범위: 실제 거래소에서 이 조건들이 어떤 통보로 나타나는지

| 시험 | 결과 | 비고 |
|---|---|---|
| `core/overfill_is_booked_in_full_and_flagged` | 이전 버전 결과 | last result: passed |
| `core/a_fill_after_any_kind_of_close_is_booked_and_flagged` | 이전 버전 결과 | last result: passed |
| `core/final_quantity_below_booked_fills_is_flagged` | 이전 버전 결과 | last result: passed |
| `core/missing_fills_after_sync_are_flagged_and_stay_reserved` | 이전 버전 결과 | last result: passed |
| `core/unidentified_fill_is_booked_as_external_after_the_wait_and_holds_trading` | 이전 버전 결과 | last result: passed |
| `core/a_fill_that_contradicts_its_order_is_booked_to_the_account_and_flagged` | 이전 버전 결과 | last result: passed |
| `core/foreign_and_orphan_open_orders_are_flagged` | 이전 버전 결과 | last result: passed |
| `core/f3_placed_evidence_for_an_order_closed_as_not_placed_is_flagged` | 이전 버전 결과 | last result: passed |
| `core/f3_a_reappearing_order_is_reported_as_open_after_close` | 이전 버전 결과 | last result: passed |
| `core/position_check_uses_the_reachable_range` | 이전 버전 결과 | last result: passed |
| `core/a_position_on_an_instrument_that_is_not_configured_is_flagged` | 이전 버전 결과 | last result: passed |
| `core/an_acknowledged_order_the_venue_no_longer_knows_is_reported_not_closed` | 이전 버전 결과 | last result: passed |
| `core/f6_a_venue_order_id_reported_for_two_orders_keeps_the_state_recoverable` | 이전 버전 결과 | last result: passed |
| `core/cancel_is_retried_with_backoff_and_then_flagged` | 이전 버전 결과 | last result: passed |
| `core/too_many_unidentified_fills_are_booked_as_external_instead_of_waiting` | 이전 버전 결과 | last result: passed |
| `core/f7_external_fills_do_not_grow_the_mismatch_list_without_bound` | 이전 버전 결과 | last result: passed |

## V-MARGIN-1 — 증거금 정책이 있으면 감소 전용 주문은 재확인, 결과 불명, 불일치 차단을 통과하고, 일반 주문은 모든 차단에서 거절된다. 정책 구역은 선택이고, 없으면 저널 기록에 바이트를 더하지 않는다

- 상태: **이전 버전 결과**
- 왜 중요한가: 모든 차단이 줄이는 주문까지 막으면 '줄이기만 허용'하는 상태를 만들 수 없다. 정책이 없는 실행의 저널은 바뀌면 안 된다
- 수준: 단위
- 전제와 범위: 차단 표(ADR-008 §7 개정)의 사유별 시험, 정책 구역의 직렬화와 기본값
- 실제 구성요소: oms-core
- 모의·시뮬레이션: 통보는 시험이 입력
- 주입한 장애: 링크 단절 뒤 재접속
- 판정 기준과 근거: C-MARGIN의 차단 표와 설정 표. 기대값은 손으로 적었다
- 재현: `cargo test -p oms-core --test scenarios reduce_only && cargo test -p oms-core --test margin`
- 뒷받침하는 것: C-MARGIN, ADR-018 §7의 8
- 확인하지 않은 범위: 거래소가 감소 전용 주문의 포지션 확대를 실제로 막는지(T5). `Reducing`, `VenueSetting`, `AccountMode` 차단은 뒤 티켓에서 생긴다. 정책 값을 읽는 규칙은 아직 없다

| 시험 | 결과 | 비고 |
|---|---|---|
| `core/absent_margin_policy_adds_no_bytes_to_a_journal_record` | 이전 버전 결과 | last result: passed |
| `core/empty_margin_section_takes_the_contract_defaults` | 이전 버전 결과 | last result: passed |
| `core/reduce_only_order_is_accepted_during_resync` | 이전 버전 결과 | last result: passed |
| `core/reduce_only_order_is_accepted_while_another_order_is_in_doubt` | 이전 버전 결과 | last result: passed |
| `core/reduce_only_order_is_accepted_under_an_unacknowledged_mismatch` | 이전 버전 결과 | last result: passed |
| `core/reduce_only_order_is_rejected_by_every_block_all_hold` | 이전 버전 결과 | last result: passed |
| `core/reduce_only_order_is_still_bound_by_the_strategy_position_limit` | 이전 버전 결과 | last result: passed |
| `core/without_a_margin_policy_every_hold_still_refuses_a_reduce_only_order` | 이전 버전 결과 | last result: passed |

## V-MARGIN-2 — 증거금 정책이 있으면 현재 링크 세션의 증거금 보고가 마지막 보고로 기록된다. 다른 세션의 보고, 링크가 내려간 동안의 보고, 순번이 크지 않은 보고는 버린다. 읽지 못한 보고와 최대 레버리지가 0인 보고는 사유와 함께 거부로 기록되고 마지막 보고를 바꾸지 않는다. 나이가 0 이상 30초 이하인 보고가 없으면 일반 주문은 MarginUnavailable로 거절된다. 감소 전용 주문은 보고가 없어도 받는다. 정책이 없으면 증거금 입력은 상태를 바꾸지 않고 기록되지 않는다

- 상태: **이전 버전 결과**
- 왜 중요한가: 규칙이 읽는 계좌 가치는 거래소 보고에서만 온다. 늦거나 다른 세션의 보고, 읽지 못한 보고가 마지막 보고가 되면 규칙이 틀린 값으로 판정한다
- 수준: 단위
- 전제와 범위: 증거금 보고 입력 둘(MarginReport, MarginReportInvalid)과 이벤트 둘(margin_reported, margin_report_rejected)
- 실제 구성요소: oms-core
- 모의·시뮬레이션: 보고는 시험이 입력
- 주입한 장애: 링크 재접속, 늦게 온 응답, 중복 응답, 읽지 못한 응답, 최대 레버리지 0
- 판정 기준과 근거: C-MARGIN의 '입력, 상태, 이벤트'. 기대값은 손으로 적었다
- 재현: `cargo test -p oms-core --test margin`
- 뒷받침하는 것: C-MARGIN
- 확인하지 않은 범위: 실제 거래소 응답의 모양과 값의 뜻(T1, T2). 새 보고를 받지 못하는 동안에는 규칙을 판정하지 않는다

| 시험 | 결과 | 비고 |
|---|---|---|
| `core/margin_report_of_the_current_session_is_recorded` | 이전 버전 결과 | last result: passed |
| `core/margin_report_of_an_old_session_or_with_the_link_down_is_ignored` | 이전 버전 결과 | last result: passed |
| `core/late_margin_report_with_a_lower_sequence_is_ignored` | 이전 버전 결과 | last result: passed |
| `core/invalid_margin_report_is_journaled_and_never_becomes_fresh` | 이전 버전 결과 | last result: passed |
| `core/plain_order_is_rejected_margin_unavailable_without_a_fresh_report` | 이전 버전 결과 | last result: passed |
| `core/margin_report_dated_after_the_record_time_is_not_fresh` | 이전 버전 결과 | last result: passed |
| `core/reduce_only_order_needs_no_fresh_report` | 이전 버전 결과 | last result: passed |
| `core/without_a_margin_policy_no_margin_check_runs` | 이전 버전 결과 | last result: passed |
| `core/margin_events_replay_to_the_same_state` | 이전 버전 결과 | last result: passed |

## V-MARGIN-3 — 증거금 정책이 있으면 일반 주문은 받은 뒤의 계좌 노출이 한도 × 계좌 가치를 넘고 받기 전보다 커질 때 AccountLeverage로 거절된다. 정확히 한도와 같으면 받는다. 계좌 가치가 0 이하면 일반 주문을 모두 거절한다. 노출은 포지션과 모든 미체결 주문(감소 전용 포함)을 지정가와 참조 가격 중 높은 값으로 센 종목별 값의 합이다. 미체결 주문은 종결 증거가 올 때까지 노출에 남고 OMS는 그 주문을 고치거나 만료시키지 않는다. 가격이 움직여 한도를 넘어도 래치는 없다. 감소 전용 주문은 검사를 건너뛰지만 합에는 들어간다. 노출이 있는 종목에 신선한 참조 가격이 없으면 일반 주문은 MarginUnavailable로 거절된다

- 상태: **미실행**
- 왜 중요한가: 수량 한도만으로는 계좌 가치의 몇 배가 되는 포지션을 막지 못한다. 아직 체결되지 않은 주문도 나중에 체결되면 한도를 넘길 수 있다
- 수준: 단위
- 전제와 범위: 신규 주문 검사 안의 레버리지 검사. 계좌 가치 100, 한도 2.0, 참조 가격 100의 손 계산 값. 성질 시험 셋(proptest 256건씩): 주문을 더하면 노출이 줄지 않는다, 롱과 숏을 뒤집어도 같다, 가장 큰 가격과 수량에서 정확하다
- 실제 구성요소: oms-core
- 모의·시뮬레이션: 증거금 보고와 참조 가격은 시험이 입력
- 주입한 장애: 취소 요청, 거래소의 주문 거절, 가격 이동으로 한도 초과, 오래된 참조 가격, 계좌 가치 0 이하, i128에 들어가지 않는 합
- 판정 기준과 근거: C-MARGIN의 '계좌 레버리지 검사'와 '신선도'의 참조 가격 조건. 기대값은 손으로 적었다. 극단값 성질 시험의 경계는 시험이 따로 계산한 i128 곱에서 나온다
- 재현: `cargo test -p oms-core --test margin`
- 뒷받침하는 것: C-MARGIN
- 확인하지 않은 범위: 주문 사이에 가격이 움직인 뒤의 노출은 지켜보지 않는다. 참조 가격(mid)은 mark가 아니다. 검사가 받은 주문을 거래소가 거절할 수 있다(경계의 수수료). 계좌 가치가 격리 포지션의 자산을 포함하는지(T2)

| 시험 | 결과 | 비고 |
|---|---|---|
| `core/order_past_twice_the_account_value_is_rejected_account_leverage` | 미실행 |  |
| `core/order_exactly_at_the_limit_is_accepted` | 미실행 |  |
| `core/an_unfilled_resting_order_keeps_its_exposure_and_is_never_repriced_or_expired_by_the_oms` | 미실행 |  |
| `core/resting_order_counts_at_the_higher_of_limit_and_reference` | 미실행 |  |
| `core/reduce_only_and_plain_buy_on_a_short_count_at_full_reach` | 미실행 |  |
| `core/exposure_is_the_sum_over_instruments` | 미실행 |  |
| `core/partial_fill_then_cancel_frees_exposure_only_on_terminal_evidence` | 미실행 |  |
| `core/a_venue_rejection_of_an_accepted_order_frees_its_exposure` | 미실행 |  |
| `core/after_a_passive_breach_a_plain_reducing_order_passes_and_an_increasing_one_does_not` | 미실행 |  |
| `core/reduce_only_order_skips_the_leverage_gate` | 미실행 |  |
| `core/plain_order_is_rejected_when_an_exposed_instrument_has_no_fresh_reference_price` | 미실행 |  |
| `core/account_value_zero_or_negative_rejects` | 미실행 |  |
| `core/adding_an_order_never_lowers_exposure` | 미실행 |  |
| `core/mirrored_books_have_equal_exposure_and_equal_decisions` | 미실행 |  |
| `core/exposure_is_exact_at_extreme_price_and_quantity` | 미실행 |  |

## V-MARGIN-11 — 증거금 정책이 있으면 노드는 정해진 간격으로 거래소에 증거금 보고를 묻고, 응답은 오르는 순번과 함께 코어 입력이 되며, status가 계좌 가치와 그 나이, 계좌 노출을 보여 준다. 정책이 없으면 묻지 않는다

- 상태: **미실행**
- 왜 중요한가: 코어의 규칙이 옳아도 조회가 연결되지 않으면 보고가 없어 모든 일반 주문이 거절되거나, 운영자가 왜 거절되는지 볼 수 없다
- 수준: 프로세스
- 전제와 범위: 실제 oms-node와 simvenue 프로세스. 조회 간격 100 ms로 줄인 설정 1회, 정책 없는 설정 1회
- 실제 구성요소: oms-node(재조정 태스크의 타이머, 게이트웨이, 입력 큐, 저널, status), 소켓
- 모의·시뮬레이션: 거래소(simvenue). 계좌 가치는 시험이 정한다
- 주입한 장애: 없음
- 판정 기준과 근거: status의 core.margin(account_value, age_us, seq, fresh, account_exposure)과 simvenue가 센 조회 수
- 재현: `cargo test -p oms-node --test e2e node_polls_margin && cargo test -p oms-node --test e2e status_shows_account_exposure`
- 뒷받침하는 것: C-MARGIN
- 확인하지 않은 범위: 기본 간격 5초 그대로의 실행. Hyperliquid 게이트웨이(아직 이 조회에 답하지 않는다: 티켓 09). 운영자 명령과 재시작(뒤 티켓). 정확한 시간

| 시험 | 결과 | 비고 |
|---|---|---|
| `e2e/node_polls_margin_and_status_shows_value_and_age` | 이전 버전 결과 | last result: passed |
| `e2e/status_shows_account_exposure` | 미실행 |  |

## V-MARGIN-9 — 증거금 정책이 있으면, 종목의 거래소 설정(증거금 방식과 레버리지)을 이번 링크 세션에서 받지 못한 동안 그 종목의 일반 주문은 VenueSetting(종목) 차단으로 거절된다. 옛 세션의 응답은 버린다. 받은 설정의 증거금 방식이나 레버리지가 정책과 다르면 그 종목만 같은 차단에 걸리고 다른 종목은 걸리지 않는다. OMS는 설정을 읽기만 하고 거래소로 아무것도 보내지 않는다

- 상태: **미실행**
- 왜 중요한가: 거래소의 레버리지나 증거금 방식이 계획과 다르면 주문이 계획하지 않은 증거금을 쓴다. 확인하지 못한 설정을 맞다고 가정하면 그 차이를 아무도 모른다
- 수준: 단위
- 전제와 범위: 코어의 차단 계산과 신규 주문 검사. 정책 기본값(격리, 레버리지 2), 종목 하나 또는 둘
- 실제 구성요소: oms-core
- 모의·시뮬레이션: 거래소 설정 응답은 시험이 입력
- 주입한 장애: 설정을 받지 못한 링크 세션, 옛 세션의 늦은 응답, 교차 증거금, 레버리지 3과 1
- 판정 기준과 근거: C-MARGIN의 '거래소 설정과 계좌 모드'. 기대값은 손으로 적었다
- 재현: `cargo test -p oms-core --test margin`
- 뒷받침하는 것: C-MARGIN
- 확인하지 않은 범위: 포지션이 없을 때 거래소가 설정을 알려 주는지(T11). 거래소가 걸려 있는 주문에 레버리지를 적용하는지(T8)

| 시험 | 결과 | 비고 |
|---|---|---|
| `core/instrument_with_unknown_venue_setting_holds_plain_orders` | 미실행 |  |
| `core/a_setting_that_differs_from_config_holds_only_that_instrument` | 미실행 |  |

