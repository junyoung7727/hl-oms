# 검증 주장과 결과

> 자동 생성 문서다. 고치려면 `registry/*.toml`을 고치고 `cargo run -p xtask -- docs`를 실행한다.
> 소스 다이제스트 `98d152eed1f5de52`, 검증 결과의 다이제스트 `762a4acf55af90fb`.

주장 121건: 통과 0, 실패 0, 미실행 54, 이전 버전 결과 67.

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
| V-CFG-1 | 설정은 버전이 커야 활성화되고, 같은 버전에 다른 내용이면 시작을 거부한다. 노출이 남은 종목·전략은 제거할 수 없다. mainnet은 증거금 정책(policy.margin) 없이는 시작하지 않고, 정책의 값이 C-MARGIN 설정 표의 범위를 벗어나면 어느 거래소 설정에서도 시작하지 않는다. 저장소의 Hyperliquid 예제 설정은 증거금 정책(소유자가 정한 값)을 담고 검사를 통과한다. Hyperliquid 설정에 증거금 정책이 있으면 설정에서 계산한 분당 조회 weight(216 + 종목마다 20, 기본 간격)가 600을 넘을 때 시작하지 않는다 | 프로세스 | 이전 버전 결과 | `cargo test -p oms-node --test e2e configuration_changes && cargo test -p oms-node --lib config::tests` |
| V-OVL-1 | 입력 큐는 상한을 넘지 않고, 가득 찬 큐는 다른 큐에 영향을 주지 않으며, 배치마다 세 큐가 지분을 받는다 | 단위 | 이전 버전 결과 | `cargo test -p oms-node --lib queues` |
| V-OVL-2 | 큐보다 빠르게 온 요청은 즉시 거절되고 기록을 남기지 않으며, 그 직후에도 취소는 수용된다 | 프로세스 | 이전 버전 결과 | `cargo test -p oms-node --test e2e overload -- --nocapture` |
| V-OVL-3 | 저장 공간이 부족하면 신규 주문만 거절하고 체결과 취소는 계속 처리한다 | 단위 | 이전 버전 결과 | `cargo test -p oms-core --test scenarios storage_low` |
| V-API-1 | 전략 연결은 자기 주문만 보고 취소하며, 운영 명령은 운영 소켓에서만 받아들여진다. 알 수 없는 전략과 잘못된 요청은 거절된다 | 프로세스 | 이전 버전 결과 | `cargo test -p oms-node --test e2e a_strategy_sees` |
| V-API-2 | 문서의 요청 예가 그대로 해석되고, 수치에 JSON 숫자를 쓰면 거부된다 | 단위 | 이전 버전 결과 | `cargo test -p oms-api` |
| V-SLICE-1 | 신규 주문 → 영속 접수 → 거래소 접수 → 부분 체결 → 잔량 취소 → 종결이 실제 프로세스에서 이어지고, 구독자는 저널 순서로 이벤트를 받으며, 정상 종료 뒤 재시작해도 상태가 같고 재전송이 없다 | 프로세스 | 이전 버전 결과 | `cargo test -p oms-node --test e2e vertical_slice` |
| V-GW-SEAM-1 | VenueGateway를 구현한 세 번째(가짜) 게이트웨이가 oms-core를 고치지 않고 구동기에 꽂히며, 구동기는 상시 작업 시작(spawn_tasks)을 명령 루프를 띄우기 전에 한 번 동기적으로 부르고, 주문 명령은 send_order로, 취소 명령은 send_cancel로, 조회는 query로 각각 한 번씩 간다. 동시에 도는 명령은 max_concurrent, 조회는 QUERY_CONCURRENCY를 넘지 않고 넘치는 것은 기다렸다가 모두 처리된다 | 단위 | 미실행 | `cargo test -p oms-node --lib venue::tests` |
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
| V-MARGIN-3 | 증거금 정책이 있으면 일반 주문은 받은 뒤의 계좌 노출이 한도 × 계좌 가치를 넘고 받기 전보다 커질 때 AccountLeverage로 거절된다. 정확히 한도와 같으면 받는다. 계좌 가치가 0 이하면 일반 주문을 모두 거절한다. 노출은 포지션과 모든 미체결 주문(감소 전용 포함)을 지정가와 참조 가격 중 높은 값으로 센 종목별 값의 합이다. 미체결 주문은 종결 증거가 올 때까지 노출에 남고 OMS는 그 주문을 고치거나 만료시키지 않는다. 가격이 움직여 한도를 넘어도 래치는 없다. 감소 전용 주문은 검사를 건너뛰지만 합에는 들어간다. 신선한 증거금 보고가 없으면 검사는 오래된 계좌 가치로 판정하지 않고 주문은 MarginUnavailable 하나로 거절된다. 노출이 있는 종목에 신선한 참조 가격이 없으면 일반 주문은 MarginUnavailable로 거절된다 | 단위 | 이전 버전 결과 | `cargo test -p oms-core --test margin` |
| V-MARGIN-11 | 증거금 정책이 있으면 노드는 정해진 간격으로 거래소에 증거금 보고를 묻고, 응답은 오르는 순번과 함께 코어 입력이 되며, status가 계좌 가치와 그 나이, 계좌 노출을 보여 준다. 링크가 올라오면 종목마다 거래소 설정을, 그리고 계좌 모드를 묻고, 포지션이 없는 종목의 설정은 정해진 간격으로 다시 묻는다. status는 종목별 거래소 설정과 계좌 모드, 그것이 정책과 맞는지를 보여 준다. 정책이 없으면 묻지 않는다. 조회한 계좌 가치가 기준 금액보다 5 %를 넘게 낮아지면 status가 감소 전용 상태와 그 규칙, 기준 금액을 보여 주고 일반 주문이 Hold(Reducing)으로 거절된다. 해제(clear-reduce)와 기준 재설정(rebase-day)은 전략 소켓에서 forbidden으로 거절되고 코어에 닿지 않는다. 운영자 도구(omsctl)의 clear-reduce --note와 rebase-day --note는 운영자 소켓으로 실행 중인 노드의 코어에 닿고, 응답과 status(상태, 규칙, 기준 금액)가 그 결과를 보여 준다. 감소 전용 상태에서 노드 프로세스를 강제 종료(SIGKILL)하고 다시 띄우면 status가 같은 상태(규칙, 내용, 진입 시각)와 같은 기준 금액을 보여 주고, 해제한 뒤의 강제 종료도 해제와 운영자의 기준을 그대로 남긴다. 가상 거래소에 일반 주문 300건과 감소 전용 주문 3건이 걸린 채로 감소 전용 상태에 들어가면 일반 주문은 모두 거래소에서 취소되고 감소 전용 주문 3건은 열린 채로 남으며, 그 뒤에도 증거금 조회가 이어진다. 감소 전용 상태를 결정한 기록이 쓰이기 전에 노드가 멈추면 재시작 뒤에 상태도 취소 요청도 그 기록도 없고 기준 금액은 그대로이며, 다음에 한도를 넘는 보고가 한 번 다시 결정한다. 그 기록이 파일에 닿은 뒤(쓰기 뒤, 내구화 뒤, 읽기 모델 반영 뒤)에 취소가 나가기 전에 멈추면 재시작 뒤에 상태와 취소 요청이 저널에서 복원되고, 보내지 못한 취소가 거래소로 나가며, 감소 전용 주문은 남는다. 기준 금액을 정하는 기록의 각 경계에서 멈춰도 저널의 day_base_set은 정확히 하나이고, 그 날짜와 값은 같은 기록의 보고의 것이다(쓰기 전에 멈추면 다음 보고가 정한다). 가상 거래소 프로세스의 증거금 보고는 포지션(수량, 가치, 증거금, 증거금 방식, 레버리지, 최대 레버리지)을 담고 status가 그대로 보여 주며, 포지션이 있는 종목의 거래소 설정이 바뀌면 보고로 코어에 닿아 그 종목이 막힌다 | 프로세스 | 이전 버전 결과 | `cargo test -p oms-node --test e2e node_polls_margin && cargo test -p oms-node --test e2e status_shows_account_exposure && cargo test -p oms-node --test e2e status_shows_reducing && cargo test -p oms-node --test e2e node_reads_venue_setting && cargo test -p oms-node --test e2e -- strategy_socket_cannot omsctl_clear reducing_ && cargo test -p oms-node --test e2e margin_report_of_the_venue && cargo test -p oms-node --features failpoints --test crash_points -- reducing day_base` |
| V-MARGIN-5 | 증거금 정책이 있으면 첫 증거금 보고가 그 계좌 가치를 조회 시각의 UTC 날짜와 함께 기준 금액으로 정한다. 같은 날의 뒤 보고는 기준을 바꾸지 않는다. 받은 보고의 계좌 가치가 기준 금액보다 5 %를 넘게 낮으면 계좌가 감소 전용 상태로 들어간다(기준 100에서 95.00은 아니고 94.99는 들어간다). 뒤 UTC 날짜의 첫 보고는 옛 기준으로 먼저 판정된 뒤에 새 기준이 되고, 여러 날의 공백 뒤에도 기준은 한 번만 넘어간다 | 단위 | 이전 버전 결과 | `cargo test -p oms-core --test margin` |
| V-MARGIN-4 | 감소 전용 상태로 들어가는 입력 안에서, 감소 전용 표시가 없고 취소 요청이 없거나 취소 시도가 다 된 모든 열린 주문에 규칙 이름으로 취소 요청이 나간다. 표시 없는 익절 주문도 취소된다. 감소 전용 표시가 있는 주문은 그대로 둔다. 감소 전용 상태에서 일반 주문은 포지션을 줄이는 방향이어도 Hold(Reducing)으로 거절되고, 감소 전용 주문은 받는다. 유지증거금 여유 규칙으로 들어갈 때도 같다(취소의 이름은 Rule(MmBuffer)) | 단위 | 이전 버전 결과 | `cargo test -p oms-core --test margin` |
| V-MARGIN-9 | 증거금 정책이 있으면, 종목의 거래소 설정(증거금 방식과 레버리지)을 이번 링크 세션에서 받지 못한 동안 그 종목의 일반 주문은 VenueSetting(종목) 차단으로 거절된다. 옛 세션의 응답은 버린다. 받은 설정의 증거금 방식이나 레버리지가 정책과 다르면 그 종목만 같은 차단에 걸리고 다른 종목은 걸리지 않는다. 맞는 설정을 받으면 바로 풀리고, 링크가 바뀌면 다시 모르는 것이 된다. 증거금 보고의 포지션에 적힌 방식과 레버리지도 설정의 출처다. 차단 조회는 걸린 종목마다 그 종목 이름과 함께 차단을 보여 준다. 계좌 모드가 unifiedAccount나 portfolioMargin이면 허용 목록에 적혀 있어도 계좌의 모든 종목의 일반 주문이 AccountMode 차단으로 거절된다. 허용 목록에 없는 모드, 빈 응답, 이번 링크 세션에서 받지 못한 모드도 같다. 기본 허용 목록은 비어 있어서 운영자가 모드를 적기 전에는 일반 주문이 나가지 않는다. 감소 전용 주문은 두 차단을 모두 통과한다. OMS는 설정을 읽기만 하고 거래소로 아무것도 보내지 않는다 | 단위 | 이전 버전 결과 | `cargo test -p oms-core --test margin` |
| V-MARGIN-6 | 운영자의 해제(clear-reduce)는 계좌가 감소 전용 상태가 아니면 NotReducing으로, 증거금 보고가 30초보다 오래됐으면 MarginUnavailable로, 사유가 비었으면 NoteRequired로 거절되고, 거절은 아무것도 기록하지 않는다. 받아들인 해제는 한 입력 안에서 마지막 보고의 계좌 가치를 새 기준 금액으로 정하고(day_base_set, set_by: operator) 감소 전용 상태를 끝낸다(reducing_cleared). 정확히 30초 된 보고는 신선하다. 그 뒤 새 기준 94에서 94와 89.30은 걸리지 않고 89.29는 다시 걸린다. 두 번째 해제는 NotReducing이다. 기준 재설정(rebase-day)은 신선한 보고와 사유가 있을 때 마지막 보고의 계좌 가치와 그 날짜(기준보다 앞선 날짜라도)를 기준 금액으로 정하고, 감소 전용 상태는 건드리지 않는다. 기준 재설정 없이 출금하면 그만큼이 손실로 읽힌다(100에서 6을 출금하면 걸린다). 마지막 보고에서 유지증거금 여유 조건이 참이면(어느 규칙으로 들어왔든) 해제는 MmBufferActive로 거절되고, 이 조건은 사유보다 먼저 본다. 포지션이 선 위로 돌아온 보고(가치 200, 최대 레버리지 40에서 3.00) 뒤에는 해제된다 | 단위 | 이전 버전 결과 | `cargo test -p oms-core --test margin` |
| V-MARGIN-7 | 증거금 정책이 있고 링크가 올라와 있을 때, 마지막으로 받은 증거금 보고의 조회 시각에서 60초가 넘게 지나면 틱이 감소 전용 표시가 없고 취소 요청이 없는 모든 열린 주문에 Rule(MarginStale) 이름으로 취소 요청을 한 번 낸다. 정확히 60초에는 내지 않는다. 감소 전용 표시가 있는 주문은 그대로 둔다. 아무 상태도 걸리지 않는다 | 단위 | 이전 버전 결과 | `cargo test -p oms-core --test margin` |
| V-MARGIN-8 | 증거금 정책이 있으면, 받은 증거금 보고의 포지션 가운데 포지션 가치가 0보다 큰 것의 격리 증거금이 (유지증거금률 + 0.25 %p) × 포지션 가치보다 작으면 계좌가 감소 전용 상태로 들어가고 규칙은 MmBuffer로 남는다. 최대 레버리지 40, 포지션 가치 200에서 3.00은 걸리지 않고 2.99는 걸린다. 유지증거금률은 그 포지션의 최대 레버리지(같은 보고의 값)로 1 / (2 × 최대 레버리지)이다: 최대 레버리지 25, 가치 200에서 4.50은 걸리지 않고 4.49는 걸리며, 최대 레버리지 50에서는 2.50이 걸리지 않는다. 한 종목의 포지션이 걸려도 래치는 계좌 전체에 하나다: 다른 종목에 걸려 있는 일반 주문이 Rule(MmBuffer) 이름으로 취소되고, 모든 종목의 새 일반 주문이 Hold(Reducing)으로 거절되며, 감소 전용 주문은 받는다. 교차 증거금 포지션은 판정하지 않는다(그 종목은 VenueSetting 차단에 걸린다). 래치에는 먼저 건 규칙이 남는다: 일 손실로 들어간 뒤의 유지증거금 여유 판정도, 그 반대도 아무것도 내지 않고 바꾸지 않는다. 한 보고가 둘 다 걸면 유지증거금 여유를 먼저 판정하므로 MmBuffer가 남는다. 받은 보고는 도착했을 때 이미 30초보다 오래됐어도 판정한다(조회 뒤 40초에 기록된 보고의 2.99도 건다). 비교는 나눗셈 없는 정확한 곱으로 한다: 가장 큰 금액·최대 레버리지·여유에서도 넘침이 결과를 뒤집지 않고(곱이 i128에 들어가지 않으면 선 아래로 본다), 선 바로 옆에서 반올림이 결과를 뒤집지 않는다. 포지션 가치가 0 이하인 포지션은 판정하지 않는다 | 단위 | 이전 버전 결과 | `cargo test -p oms-core --test margin` |
| V-MARGIN-13 | 증거금 정책에 account_mm_rate_max_bps가 있으면, 받은 증거금 보고의 교차 계좌 유지증거금이 0보다 크고 유지증거금 × 10 000 ≥ 한도 × 증거금 잔고일 때 계좌가 감소 전용 상태로 들어가고 규칙은 AccountMm(status의 account_mm)으로 남으며, 같은 입력에서 열린 일반 주문마다 Rule(AccountMm) 취소를 낸다(감소 전용 주문은 남는다). 한도 5000에서 잔고 1000: 499.99는 걸리지 않고 500은 건다. 한도 1에서 잔고 100: 0.00999999는 아니고 0.01은 건다. 잔고 0 이하에 유지증거금이 있으면 어느 한도에서도 건다. 유지증거금 0은 잔고가 얼마든(0, 음수 포함) 걸지 않는다. 가장 큰 금액에서도 나눗셈 없이 정확히 비교한다. 한도가 있는데 account_mm이 없거나 유지증거금이 음수인 보고는 통째로 MarginReportRejected이고 마지막 정상 보고가 나이를 먹는다. 한도가 없으면 같은 보고를 기록하고 숫자를 판정하지 않는다. 래치에는 먼저 건 규칙이 남는다(일 손실 뒤의 계좌 규칙도, 그 반대도 아무것도 내지 않는다). 한 보고가 여럿을 걸면 유지증거금 여유, 계좌 유지증거금률, 일 손실 순이다. clear-reduce는 마지막 보고에서 조건이 참이면 어느 규칙이 걸었든 account_mm_active로 거절된다. status는 마지막 보고의 두 숫자를 보이고 없으면 키가 없다. account_mm 없는 보고와 한도 없는 정책은 저널에 바이트를 남기지 않고(기록 스키마 2 그대로), 이 필드 전의 기록은 없음으로 읽히며, 두 종류의 저널 모두 재생·재결정·디스크 복구(verify --re-decide)에서 실시간 상태와 같다. 한도는 1..=9999다. 노드 설정은 Bybit에서 증거금 정책이 있으면 한도를 요구하고 Hyperliquid와 가상 거래소에서는 거부한다. Bybit 게이트웨이는 totalMaintenanceMargin과 totalMarginBalance를 보고에 싣고, 둘 중 하나라도 없거나 문자열 소수가 아니면 응답 전체가 MarginReportInvalid이며, accountMMRate는 무슨 값이든(없어도) 보고를 바꾸지 않는다 | 단위 | 미실행 | `cargo test -p oms-core --test account_mm; cargo test -p oms-node --test record_schema; cargo test -p oms-node --lib -- venue::bybit::query config::` |
| V-MARGIN-12 | 시뮬레이션의 가상 거래소는 계좌 가치를 자기 모형으로 계산한다: 현금 + 포지션 × 기준 가격. 체결은 낸 값과 수수료만큼, 펀딩은 그 금액만큼 현금을 줄이고, 가격 변화는 포지션 가치를 바꾼다. 손으로 푼 예(입금 100, 100에 0.5 매수와 수수료 0.01, 가격 104, 펀딩 0.1 %)에서 계좌 가치는 99.99, 101.99, 101.938이다. 가격이 움직이지 않아도 수수료와 펀딩만으로 계좌가 기준 금액보다 5 %를 넘게 내려가면 감소 전용 상태로 들어간다(100에서 95.00은 아니고 94.99는 들어간다). 가상 거래소는 포지션을 키우거나 반대쪽으로 넘기는 감소 전용 주문을 거절하고, 걸려 있는 감소 전용 주문은 남은 포지션만큼만 체결한 뒤 나머지를 취소한다. 증거금 정책을 켠 임의 순서의 실행(체결, 가격 변화, 펀딩, 운영자 해제, 그리고 margin_lossy에서는 보고 유실·지연·무응답, 링크 단절, 재시작)의 매 입력 뒤에: 받아들인 일반 주문은 신선한 보고가 있었고 노출이 계좌 가치의 2배 이내이거나 노출을 올리지 않았다. 감소 전용 상태에 들어가면 열려 있는 일반 주문마다 취소 요청이 있고, 그 상태인 동안 일반 주문은 받아들여지지 않는다. 감소 전용 상태는 증거금 보고로만 들어가고 운영자의 해제로만 끝난다(재시작, 새 UTC 날, 회복된 계좌 가치는 끝내지 않는다). 보고가 한도보다 오래 끊긴 뒤의 틱이 끝나면 열려 있는 일반 주문마다 취소 요청이 있다. 증거금 정책이 없는 기존 프로파일 일곱 개는 seed 다섯 개씩 main(84db0f9)에서 기록한 것과 같은 저널을 쓴다. 가상 시간 50시간짜리 실행(margin_days: UTC 자정 1시간 전에 시작해 자정을 세 번 넘고, 그 사이에 재시작, 링크 단절, 손실, 출금과 입금, 운영자의 해제와 기준 재설정이 섞인다)에서도 위 검사가 매 입력 뒤에 성립하고, 기록된 보고가 그 전의 기준 금액보다 5 %를 넘게 낮으면 그 보고가 새 날의 기준을 정하는 보고여도 계좌는 감소 전용 상태가 되고, 저널의 기준 금액은 실행마다 자정을 정확히 세 번 넘고, 재시작과 새 날은 감소 전용 상태를 끝내지 않는다. 정해진 순서 하나(자정을 넘는 20초의 조회 끊김 동안 100에서 6 출금, 재시작, 자정 두 번 더)에서 새 날의 첫 보고 94는 상태를 걸고 나서 기준이 되고, 저널에는 날마다 기준이 하나다(100, 94, 100, 99). 저널의 증거금 기록(입력이 증거금 보고인 기록)은 조회 한 번에 하나를 넘지 않는다: 조용한 하루는 조회 17 280번에 기록 17 280건이고, 50시간 실행에서 기록 수는 조회 수 이하이고 같은 조회의 답은 두 번 기록되지 않는다 | 시뮬레이션 | 이전 버전 결과 | `cargo test -p oms-sim --test sim -- world_ a_loss_made profile_margin margin_journal existing_profiles --nocapture   (실패 시: OMS_SIM_PROFILE=<이름> OMS_SIM_SEED=<n>)` |
| V-MARGIN-10 | Hyperliquid 게이트웨이는 문서의 clearinghouseState 응답 예를 증거금 보고 입력으로 옮긴다: 계좌 가치는 marginSummary.accountValue, 포지션마다 종목, 부호 있는 수량, 포지션 가치, 사용 증거금, 증거금 방식, 레버리지, 최대 레버리지. 어느 포지션이든 maxLeverage가 없거나 0이거나 정수가 아니면 보고 전체를 사유와 함께 거부 입력(MarginReportInvalid)으로 바꾸고, 멀쩡한 다른 포지션도 보고하지 않는다. 계좌 가치, 수량, 포지션 가치, 사용 증거금, 레버리지 가운데 하나라도 없거나 읽을 수 없을 때, 증거금 방식이 isolated도 cross도 아닐 때, 포지션 목록이 없을 때도 같다. 어떤 값도 0이나 기본값으로 채우지 않고 어떤 포지션도 건너뛰지 않는다. 설정에 없는 종목의 포지션도 보고에 남는다(번역 함수는 설정된 종목을 받지 않는다). activeAssetData 응답 예는 그 종목의 거래소 설정 입력(증거금 방식, 레버리지)이 된다. 방식이 isolated도 cross도 아니거나, 레버리지가 정수가 아니거나, 응답이 다른 종목의 것이면 입력을 만들지 않는다(설정은 모름으로 남아 그 종목이 막힌다). userAbstraction 응답(문자열 하나)은 글자 그대로 계좌 모드 입력이 된다. 문서에 없는 문자열을 문서에 있는 값으로 바꾸지 않고, 문자열이 아니거나 64자를 넘으면 입력을 만들지 않는다. 세 조회의 요청 본문은 문서의 것과 같다(clearinghouseState, activeAssetData, userAbstraction). 게이트웨이의 요청 예산은 취소를 막지 않으므로 취소 500건 뒤에는 조회가 기다린다: 가득 찬 예산(1200)에서는 700이 남아 증거금 조회가 바로 나가고, 시작 직후(600)에는 100이 남아 10.1초, 빈 예산에서는 15.1초를 기다린다 | 단위 | 이전 버전 결과 | `cargo test -p oms-node --lib venue::hyperliquid` |
| V-WIRE-ID-1 | 고객 주문번호 형식 Base32Pair는 VenueClientId와 일대일이고 27자(36자 이하, 0-9a-v와 -)이며 같은 입력에 같은 문자열을 내고, 서로 다른 100만 개 주문에 서로 다른 100만 개 문자열을 낸다. 해석은 그 형식이 쓴 문자열만 받는다(대문자·다른 길이·다른 구분자·64비트 초과·16진수 형식은 None). Hex128은 to_hex와 같은 문자열을 내고 소문자 16진수만 해석하며, 가상 거래소와 Hyperliquid 게이트웨이는 Hex128을 선언하고 보내는 번호를 그 형식으로 만든다 | 단위 | 미실행 | `cargo test -p oms-core --lib ids::tests && cargo test -p oms-node --lib venue::tests` |
| V-VENUE-EXEC-1 | 펀딩 입력은 포지션·주문·예약·체결 수를 바꾸지 않고 한 번만 기록된다. 거래소 포지션 변경은 부호 있는 수량을 계좌와 미귀속 장부에 더하고 VenuePositionChange 불일치로 일반 주문을 막으며(감소 전용은 통과) 재생 뒤에도 남는다. 분류 못 한 실행은 기장 없이 UnclassifiedExecution 불일치로 막는다. 변경 뒤 그보다 늦게 물은 현재 세션의 포지션 응답 중 그 종목에 열린 주문과 대기 체결이 없을 때의 첫 응답이 다르면 PositionAdjusted(snapshot_after_venue_change) 한 건으로 맞추고 같으면 VenueChangeSettled만 남기며(열린 주문이 있는 동안은 맞추지 않아 오는 중인 체결을 두 번 세지 않는다), 저널만으로 계산한 포지션이 거래소 값과 같다. 분류 못 한 실행의 키는 같은 키로 나중에 온 체결의 기장을 막지 않는다. 운영자 조정은 reason 없이 스키마 1과 같은 바이트로 쓰인다 | 단위 | 미실행 | `cargo test -p oms-core --test venue_changes` |
| V-SCHEMA-2-1 | 이 빌드는 기록 스키마 2를 쓰고 1과 2를 읽는다. 기록 스키마 1로 쓴 저널(체결, 운영자 조정, 정지 포함)을 복구하면 실시간 코어와 같은 상태(다이제스트 포함)가 되고, 그 뒤에 스키마 2로 이어 쓴 기록은 새 세그먼트에 들어가며, 두 세그먼트를 함께 복구해도 실시간 상태와 같고 --re-decide 검사도 통과한다 | 통합 | 미실행 | `cargo test -p oms-node --test record_schema` |
| V-ABSENCE-N-1 | absence_not_found_needed가 2이면 결과 불명 주문은 expires_at + absence_margin 뒤에 보낸 조회의 "없음"을 absence_margin 이상 떨어진 두 조회로 받아야 NotPlaced로 닫히고, 첫 번째는 AbsenceNoted로만 기록되며 예약은 그대로다. 만료 + 여유 전의 "없음", 같은 조회의 두 번째 답, 앞에 센 답보다 여유 시간 안쪽의 답은 세지 않고, 셈은 재생 뒤에도 같다. 기본값 1에서는 AbsenceNoted 없이 예전처럼 한 번에 닫히고, 기본값은 직렬화하지 않으며, 0은 설정 오류다 | 단위 | 미실행 | `cargo test -p oms-core --test absence_and_market_ref` |
| V-MARKET-REF-1 | 새 주문은 판단에 쓴 호가창 상태(market_ref)를 실을 수 있고, 그 값은 OrderAccepted.request로 저널 기록에 들어가 직렬화와 해석을 거쳐도 같다. market_ref가 없는 요청은 예전과 같은 바이트로 쓰인다. 같은 요청이 다른 market_ref로 다시 오면 duplicate다. 이름은 64자 이하의 [A-Za-z0-9_.:-]만 받고 거래 API는 형식이 틀린 market_ref를 해석 단계에서 거절한다 | 단위 | 미실행 | `cargo test -p oms-core --test absence_and_market_ref && cargo test -p oms-api --lib` |
| V-TAIL-1 | 꼬리 읽기는 디스크에 더 많은 기록이 있어도 호출자가 준 내구화 번호보다 큰 기록을 돌려주지 않고, 작은 세그먼트로 회전한 저널을 끊김 없이 순서대로 읽으며, 커서 다음 번호부터 다시 시작한다. 쓰는 쪽이 잠금을 쥔 동안에도 읽고, 읽는 동안 새 쓰는 쪽이 잠금을 잡을 수 있다(잠금을 잡지 않는다). 상한 이하의 손상된 프레임이나 번호가 튀는 프레임에서는 그 앞 기록을 돌려준 뒤 오류를 내고 위치를 움직이지 않으며, 커서 앞의 손상도 오류로 알린다(끝에서 쓰는 중인 프레임은 기다린다). 기록 스키마가 바뀌어 열린 새 세그먼트와, 헤더만 있다가 새 스키마 헤더로 다시 쓰인 세그먼트를 따라가고 기록마다 스키마를 알려 준다. 엿보기는 상한 너머의 유효한 프레임을 위치를 움직이지 않고 보여 주며 잘린 마지막 프레임에서 오류 없이 멈춘다 | 단위 | 미실행 | `cargo test -p oms-journal --test tail` |
| V-TAIL-2 | 모든 이벤트 종류(43가지)의 인스턴스를 선택 필드까지 채워 직렬화하면, 인코더가 쓰는 모든 필드 경로가 docs/contracts/tail.md의 필드 표에 있고, 표의 모든 행은 실제로 쓰이는 필드이며 타입 열이 실제 JSON 값의 종류(문자열, 정수, 불, 목록, 열거형, 객체, null 허용)와 맞는다. 통째로 읽는 행은 설정 하나와 열거형 값뿐이다. 이벤트 종류가 늘면 시험이 컴파일되지 않는다 | 소스 검사 | 미실행 | `cargo test -p oms-core --test event_fields` |
| V-BYBIT-READ-1 | Bybit 게이트웨이의 조회 일곱 가지는 읽기만 하고, 통째로 읽을 수 있는 응답만 입력으로 넘긴다. 쪽 넘김은 커서가 비고 쪽이 덜 찼을 때만 끝으로 보고, 다 찬 쪽에 커서가 없거나 커서가 그대로이거나 쪽이 20을 넘으면 스냅숏(열린 주문, 포지션, 증거금 보고)을 넘기지 않는다. 주문 상태는 우리 orderLinkId의 행만 보고 realtime, 기간 없는 주문 내역, 코어가 주문을 받은 시각 − 1분부터 7일의 주문 내역 셋 다에 없을 때만 StatusNotFound(첫 조회 전 시각)를 낸다. 체결은 since − 2분부터 7일 이하의 창으로 읽고(창 사이 1ms 겹침, 중복 키는 한 번), 한 번에 30쪽까지 읽고 다 읽은 창까지만 선언한 채 멈추며(오류 아님), 창을 다 읽은 뒤에만 그 끝 직전까지 FillsSynced를 내며, 끝을 알 수 없는 창은 반으로 나누고, 읽을 수 없는 행에서 멈춘다. 실행 행은 execType 표 하나로 Fill(우리 주문이면 order_id, 아니면 외부 체결), Funding, PositionChange(adl, takeover, delivery, settle), UnclassifiedExecution이 된다. 증거금 보고는 totalEquity, totalMaintenanceMargin, totalMarginBalance(U17d), positionIM, 계좌 증거금 모드, 정수 레버리지, maxLeverage로 만들고 한 값이라도 못 읽으면 MarginReportInvalid다. accountMMRate는 보고를 바꾸지 않는다. 이 코드에는 주문 생성·정정·단건 취소 요청이 없다. 설정은 Bybit에서 REGULAR_MARGIN 밖의 계좌 모드 허용을 거절한다 | 단위 | 미실행 | `cargo test -p oms-node --lib venue::bybit` |
| V-BYBIT-STREAM-1 | Bybit 게이트웨이는 시작 확인이 통과하고 막힘이 없을 때만 비공개 스트림에 접속하고, auth(GET/realtime + 만료 시각의 서명, 만료 = 지금 + recv_window)가 확인된 뒤 order.linear·execution.linear를 구독하며, 구독이 확인된 뒤에만 Link를 올린다. 접속마다 세션 번호가 커진다. ping 뒤 ping 간격 안에 pong이 없거나(코어 입력 큐를 기다린 시간은 빼고), 소켓이 닫히거나, 읽을 수 없는 행이나 symbol 없는 행이 오거나, 연결 중 막힘(IP 변경, 키 거부, IP 차단)이 생기면 Link를 내리고, 키 거부와 IP 변경이 아니면 다시 붙는다. 인증 거부, 조회의 키 거부 코드, IP 변경 뒤에는 재시작까지 접속도 요청도 하지 않는다. HTTP 403(REST든 스트림 업그레이드든) 뒤에는 멈춤 시간 동안 REST 요청과 스트림 접속이 없고, 끝나면 다시 붙는다. 구독 거부와 응답 없는 인증은 키 거부가 아니라 다시 시도하고, 인증 응답 전에는 구독을 보내지 않는다. 연결이 올라가 첫 pong을 받았으면 재연결 기다림은 처음 값으로 돌아가고, pong 전에 끊기면 기다림이 계속 는다. 설정한 종목의 주문 행은 OrderStatus(Stream), 실행 행은 실행 분류표를 거쳐 받은 시각이 실린 입력이 되고 다른 종목의 행은 넘기지 않는다. 주문과 취소는 V-BYBIT-ORDERS-1 | 통합 | 미실행 | `cargo test -p oms-node --lib venue::bybit` |
| V-BYBIT-ORDERS-1 | Bybit 게이트웨이는 주문과 취소마다 결과 하나를 돌려주고 REST 요청을 많아야 하나 보내며 다시 보내지 않는다. 생성은 orderLinkId = 우리 고객 주문번호, category linear, Limit, 단방향(positionIdx 0)으로 보내고 보낸 본문에 서명한다. 요청 시각 + recv_window + 1초가 expires_at보다 늦거나, 막힘이 있거나, 거래하지 않는 종목이거나, 접속이 되지 않으면 보내지 않고 NotSent다. retCode 0은 생성이면 orderId가 있을 때만 Placed(Open, 누적 수량 없음), 취소면 Confirmed. 오류 문서의 0 아닌 코드는 UNKNOWN_CODES가 아니면 Rejected, 그 밖의 모든 응답(문서에 없는 코드, 2xx가 아닌 HTTP, 시간 초과, 끊김, 읽을 수 없는 본문)은 Unknown이다. HTTP 403과 retCode 10018은 IP 차단 멈춤을, HTTP 401과 키 거부 retCode는 키 거부를 건다. 요청은 자리를 잡은 때부터 응답 + 1초까지(응답이 없으면 요청 시각 + recv_window + 1초 + 1초까지) 세므로 보낸 때와 응답(또는 마지막 도착 가능 시각) 사이 어디에 도착하든 어느 1초에도 주문은 8개, 주문과 취소는 합쳐 10개를 넘지 않고, 주문이 8개를 다 써도 취소는 바로 나가며, 차면 기다린다(버리지 않는다). 아무 바이트도 나가지 않은 요청은 자리를 바로 돌려준다. 시작 확인은 설정의 px_tick·qty_step이 거래소 tickSize·qtyStep의 배수가 아니면 막고, 로컬 시계가 거래소보다 500ms 넘게 앞설 수 있으면(시작 확인 전, IP 확인 주기마다, 막힌 동안 10초마다, 왕복 250ms 넘는 측정은 판정 안 함) 생성만 막고 취소와 스트림은 막지 않는다 | 통합 | 미실행 | `cargo test -p oms-core --lib rate && cargo test -p oms-node --lib venue::bybit` |
| V-BYBIT-TRADE-1 | Bybit 게이트웨이는 주문과 취소를 WS Trade(/v5/trade)로 보내고, REST로는 WS Trade 연결 작업이 그 요청의 아무 바이트도 쓰지 않았을 때(인증된 연결 없음, 쓰기 전 막힘)만, 막힘을 다시 본 뒤 같은 한도 자리와 같은 요청 시각으로 보낸다. 요청은 두 경로를 합쳐 많아야 한 번 나가고 한도 자리는 경로와 상관없이 하나다(1초 안의 9번째 생성은 경로가 섞여도 기다린다). 요청은 reqId(생성 c + 고객 주문번호, 취소 x + 36진 시도 번호 + 고객 주문번호, 36자 이하), header의 X-BAPI-TIMESTAMP·X-BAPI-RECV-WINDOW, op, REST 본문 하나인 args로 되어 있다. 응답은 REST와 같은 표로 분류하고, 20006·10019·10429는 결과 불명, 10019는 연결을 닫고 다시 열며, 10403과 10018은 IP 차단 멈춤을, 키 거부 코드는 키 거부를 건다. 쓴 요청에 request_timeout_ms 안에 응답이 없거나 그 사이 연결이 내려가면(pong 없음, 끊김, 막힘) 결과 불명이고 어느 경로로도 다시 보내지 않으며, 한도는 요청 시각 + recv_window + 1초 + 1초까지 센다. 늦은 응답은 버린다. 같은 reqId가 기다리는 중이면 다시 쓰지 않는다. WS Trade 연결은 막힘이 없을 때만 IPv4로 접속하고, auth(비공개 스트림의 서명)의 retCode가 0일 때만 올라가며, 0이 아니면 재시작까지 키 거부, 업그레이드의 HTTP 403은 IP 차단 멈춤이다. ping 뒤 간격 안에 pong이 없으면 내려가고, 재연결 기다림은 첫 pong을 받은 연결 뒤에만 처음 값으로 돌아간다. WS Trade는 Link를 움직이지 않는다 | 통합 | 미실행 | `cargo test -p oms-node --lib venue::bybit` |
| V-BYBIT-REFPX-1 | Bybit 게이트웨이는 공개 linear 스트림에 거래하는 종목마다 tickers.<symbol>을 구독하고(구독 요청 하나에 토픽 10개까지, 모든 응답이 success: true일 때만 구독됨, 거부되거나 handshake 시간 안에 답이 없는 요청이 하나라도 있으면 pong이 와도 끊고 다시 붙음), 종목마다 그 연결의 스냅숏의 markPrice를 mark로 삼아 RefPrice(px = mark, venue_ts = 메시지 ts의 µs)로 코어에 보낸다. 스냅숏 전의 델타는 버리고, markPrice가 있는 델타는 mark를 바꾸며, markPrice 없는 델타는 든 mark를 그 델타의 ts로 다시 보낸다. 내보냄은 종목마다 ref_price_interval_ms에 많아야 한 번이고(재연결을 넘어 센다) 메시지 없이 다시 보내지 않는다. mark는 연결마다 비운다. 읽을 수 없는 티커 메시지(JSON 아님, type·data·symbol·markPrice·ts 이상, markPrice 없는 스냅숏, 소수 9자리, 0 이하)는 연결을 끊고 다시 붙으며, 거래하지 않는 종목의 메시지는 읽지 않고 버린다. 연결은 막힘이 없을 때만 IPv4로 접속하고, 연결 중 막힘이 생기면 끊고 막힌 동안 다시 붙지 않으며, 업그레이드의 HTTP 403은 IP 차단 멈춤이다. ping 뒤 간격 안에 pong(op = pong, 또는 공개 linear의 op = ping + ret_msg = pong)이 없으면 끊는다. 첫 pong 전에 끊긴 연결은 재연결 대기를 늘리고, 첫 pong 뒤에 끊긴 연결만 대기를 처음으로 되돌린다. 공개 스트림은 Link를 움직이지 않는다. 설정의 ref_price_interval_ms는 1000 이상, ref_price_max_age_us의 절반 이하만 받는다. 시험용 연결 주소 OMS_TEST_BYBIT_WIRING은 다섯 주소의 호스트가 모두 127.0.0.1일 때만 받는다 | 통합 | 미실행 | `cargo test -p oms-node --lib -- venue::bybit config::tests` |
| V-BYBIT-NODE-1 | kind = bybit인 oms-node 프로세스(failpoints 빌드, 시험용 연결 주소로 가짜 Bybit에 연결)는 참조 가격이 오기 전 가격 범위가 있는 주문을 RefPriceUnavailable로 거절하고 아무 요청도 보내지 않으며, 공개 스트림의 티커 스냅숏 뒤에는 같은 요청을 받아 WS Trade로 한 번 보내고, 가짜 WS Trade가 그 order.create를 읽을 때 디스크의 저널 파일에 그 주문의 수락 기록이 있다(이 확인은 순서의 증명이 아니다: 기록이 네트워크 왕복보다 먼저 파일에 닿으므로 쓰기 전에 보내도 통과한다. 기록이 내구화된 뒤에만 보낸다는 것은 다음 flush 실패 시험이 증명한다). 새 주문의 저널 flush가 실패하면(OMS_CRASH_AT=sync_error_after_write) 노드는 상태 2로 멈추고 REST와 WS Trade 어느 쪽에도 생성이 닿지 않으며, failpoint 없이 다시 시작하면 그 기록을 찾고 orderLinkId 조회(realtime과 history 두 번이 모두 비었음)로 주문을 '놓이지 않음'으로 닫고, 그래도 생성은 없다. WS Trade가 생성을 읽고 답하지 않으면 주문은 결과 불명(Transport)이 되고, orderLinkId로 묻는 realtime 조회가 열린 주문을 찾아 Working이 되며, 두 경로를 합친 생성은 하나다 | 프로세스 | 미실행 | `cargo test -p oms-node --features failpoints --test bybit_node` |
| V-ALERT-DIRECT-1 | alert-direct의 송신 스레드는 텔레그램 전송이 처음 다시 보내기(Retry)로 끝난 시도의 시각을 '실패 시작'으로 두고, 텔레그램이 답하면(전달 또는 400·413 거절) 지운다. 보낼 것이 없으면 실패 시작은 생기지 않는다. 둘째 채널을 설정하면 once와 set이 만든 모든 메시지가 같은 분할·같은 한도(50개)의 둘째 보관함에도 통째로 들어가 자기 스레드(alert-second)로 한 번씩 나가고, 넘치면 오래된 것부터 버리고 세어 자기 버림 알림을 보내며, 오류는 그 조각을 두고 그 전송을 끝낸다. 실패 시작에서 second_after_ms(기본 60초)를 넘게 지나면 둘째 채널에만 'Telegram 전송이 N초째 실패함' 알림이 시작 1번, repeat_ms마다 1번, 텔레그램이 다시 답하면 해소 1번 나간다. flush_for는 두 보관함을 모두 기다린다. 둘째 채널이 없으면 스레드도 복사도 없다 | 통합 | 미실행 | `cargo test -p alert-direct` |
| V-ALERT-1 | OMS의 알림 스레드(oms-alert)는 공개된 읽기 모델만 읽으므로, 알림은 그 원인이 된 기록이 내구화된 뒤에만 나간다. 코어가 결정했지만 커밋 단계가 아직 내구화하지 않은 계좌 중지는 보내지 않고, 내구화되면 보낸다. flush에 실패한 묶음의 계좌 중지는 어떤 요청에도 나오지 않고, 그 뒤의 새 요청은 치명 알림 하나뿐이다. 치명 알림은 치명 사유와 마지막 내구 seq만 담고(실패한 묶음의 내용 없음) 치명 래치를 처음 본 때 한 번 나가며, 그 뒤에는 조건을 평가하지 않는다: 유예가 지난 연결 끊김도 시작하지 않고, 읽기 모델에서 사라진 중지도 해소를 보내지 않는다. 평가가 읽기 모델을 읽은 뒤 래치가 걸려 있으면 그 평가는 시작 알림도 set도 하지 않는다. 종료는 래치가 걸려 있으면 받은 종료 코드와 상관없이 치명 알림을 넣고 전송을 기다린다. 프로세스에서: 감소 전용 상태로 들어가는 기록의 flush가 실패하면 노드는 종료 코드 2로 exit_flush_ms + 5초 안에 끝나고, 가짜 텔레그램은 시작 알림과 치명 알림만 받으며(첫 시도 500 뒤 종료 전 전송에서 다시) REDUCING에 관한 것은 없다. 시작 뒤 첫 flush(Boot 기록)가 실패하면 설정이 활성화되기 전이어도 노드는 종료 코드 2로 끝나고, 가짜 텔레그램은 치명 알림 하나만 받는다(종료 전 전송) | 통합 | 미실행 | `cargo test -p oms-node --features failpoints --test pipeline --test crash_points; cargo test -p oms-node --test alerts; cargo test -p oms-node --lib alerts::` |
| V-ALERT-2 | 텔레그램이 계속 500으로 답하는 동안 시작 알림, 계좌 중지, 5분씩 시계를 58번 움직인 반복이 만든 60개 메시지 가운데 50개가 보관되고 10개가 버려져 status.alerts에 queued 50, dropped_total 10으로 보인다(failures와 failing_since_ms도 보이고, 토큰은 없다). 텔레그램이 다시 200으로 답하면 남은 50개가 나간 뒤 '[OMS P1] 전송 실패로 알림 10개를 버림' 하나가 나가고, 전달 51, 대기 0, failing_since_ms null이 된다 | 통합 | 미실행 | `cargo test -p oms-node --test alerts outbox_bound` |
| V-ALERT-3 | 알림 조건은 C-ALERT 표의 키와 유예를 따른다: 차단 사유마다 비어 있지 않은 문장(Mismatch와 OperatorStrategy는 조건 없음), 래치와 운영자 행위는 유예 0, 지나가는 상태와 저장 공간 부족은 grace_ms. Core::handle로 만든 상태에서 hold:link_down, gateway_block, hold:resync, hold:operator_account, hold:storage_low, halt_strategy:<s>, mismatch:<kind>, hold:venue_setting:<i>, hold:account_mode, margin_stale, hold:reducing이 나오고 문장 끝은 '근거: 저널 seq n'이다. 불일치는 종류마다 키 하나에 개수와 최근 3개(subject: detail)를 담고 확인한 것은 빠진다. 계좌 중지는 시작 1번, +4분 0번, +5분에 그때의 메모로 1번, +10분 1번, 재개에 '해소: <시작 문장>' 1번, 그 뒤 +5분 0번이다. 연결 끊김은 20초 만에 다시 붙으면 아무것도 보내지 않고, 29.999초에는 아니고 정확히 30초에 시작을, 31초에는 더 보내지 않고, 다시 붙으면 해소를 보낸다. 저장 공간 부족이 20초 낮음·5초 정상을 열 번 되풀이하면 아무것도 보내지 않고, 30초를 넘게 계속되면 시작 1번(29.999초에는 아니고 30초에, 35초에 더 없음), 풀리면 해소 1번이다 | 통합 | 미실행 | `cargo test -p oms-node --test alerts; cargo test -p oms-node --lib alerts::` |
| V-ALERT-4 | OMS 알림에 둘째 채널을 주면(alerts::start) 모든 메시지의 사본이 바로 그쪽에 가고, 텔레그램이 500으로 답한 첫 시도부터 59초에는 장애 알림이 없고 61초가 되면 1초 안에 'Telegram 전송이 61초째 실패함'이 둘째 채널에만 가며(텔레그램 요청에는 없음), 텔레그램이 다시 답하면 해소가 간다. 텔레그램이 답하지 않아도(연결은 받고 답 없음) 계좌 중지·재개 1,000건이 5초 안에 내구화되고 알림 스레드는 계속 평가한다 | 통합 | 미실행 | `cargo test -p oms-node --test alerts -- telegram_ a_hung` |
| V-ALERT-5 | [alert] 설정은 절대 경로의 토큰 파일을 요구하고 그 파일을 설정을 읽을 때 alert-direct 규칙으로 읽는다: 0640, 심볼릭 링크, 저장소 안, 없는 파일은 '[alert] telegram: '로 시작하는 설정 오류이고 그 문장과 설정의 Debug에 토큰이 없다. 각 값(grace_ms, eval_ms, repeat_ms, exit_flush_ms, second_after_ms)은 양 끝에서 받고 바로 바깥에서 거부하며, second_channel은 "none"만 받는다. 표가 없으면 알림이 없고 status.alerts는 {"configured": false}다. mainnet(Hyperliquid, Bybit)은 표 없이 시작하지 않고, testnet과 가상 거래소에서는 선택이다. OMS_TEST_TELEGRAM_API는 loopback http만 받고, 배포하는 바이너리(failpoints 없음)는 그 변수를 무시해 저장소 안의 토큰 파일을 여전히 거부하고 종료 코드 2로 끝난다. 시작 알림은 그 평가에서 시작이 나간 조건과 유예 안의 조건을 따로 센다: 계좌 중지와 연결 끊김이면 '활성 조건 1개(유예 중 1개)'다. 프로세스에서: 시작 알림 'OMS 시작 (boot 1, config 1)'이 나가고, SIGTERM으로 정상 종료하면 'OMS 정상 종료. 걸린 주문 1개는 거래소에 남음'이 종료 전에 나간다. 설정이 활성화된 뒤 거래 소켓을 bind하지 못하면 종료 코드 3으로 끝나고 'OMS 시작 실패(exit 3): trading_socket' 하나가 종료 전에 나가며, 어떤 요청에도 소켓 경로·토큰·토큰 파일 이름이 없다 | 통합 | 미실행 | `cargo test -p oms-node --lib config::; cargo test -p oms-node --test alerts the_; cargo test -p oms-node --features failpoints --test crash_points -- a_clean_stop an_exit_3` |
| V-HOLD-1 | 증거금 정책이 있으면 워처 불일치와 거래소 변경 아래에서 감소 전용 주문은 통과하고 일반 주문은 거절되며, 킬 래치와 운영자 중지 아래에서는 둘 다 거절된다(TN 표 7-7). 정책이 없으면 모든 보류가 모든 주문을 거절한다. 새 사유 셋은 기존 사유 뒤에 붙어 기존 사유의 순서가 바뀌지 않는다. 프로세스에서: 운영 소켓의 watcher_mismatch 뒤 일반 주문은 Hold(WatcherMismatch)로 거절되고 감소 전용 주문은 받으며, watcher_kill 뒤에는 감소 전용도 Hold(KillLatched)로 거절된다. 워처 입력의 detail은 비지 않고 1 KiB(1024바이트) 이하여야 한다: 노드와 코어가 각각 거절하고 아무것도 기록하지 않는다 | 단위, 통합 | 미실행 | `cargo test -p oms-core --test holds; cargo test -p oms-node --test holds_node watcher` |
| V-HOLD-2 | clear_hold는 고정 코드로 거절한다: 걸려 있지 않음(not_held), 메모 없음(note_required), 이 프로세스의 킬 루프가 답하지 않음(kill_unconfirmed), 코어의 장부에 미체결이 있음(kill_orders_open, 킬 루프의 답이 confirmed든 unsupported든, kill_unconfirmed 다음에 본다), 거래소 변경 뒤 포지션을 아직 읽지 않음(venue_change_pending). 거절은 기록도 명령도 남기지 않는다. 래치 아래에서도 cancel-all은 받고, 거래소가 취소를 확인해 장부가 비면 킬의 해제를 받는다(코어와 가상 거래소 프로세스). 받으면 hold_cleared 하나와, 킬이면 KillStop 하나. 거래소 변경의 해제와 불일치 확인은 따로다. resume은 킬과 워처 불일치를 풀지 않는다. omsctl clear-hold는 세 사유와 메모를 읽는다 | 단위 | 미실행 | `cargo test -p oms-core --test holds clear_hold resume a_kill_is_not; cargo test -p oms-node --test holds_node watcher; cargo test -p oms-api` |
| V-HOLD-3 | 킬 래치는 hold_set에서, 그 확인은 kill_loop_ended에서 재실행으로 같은 상태가 되고, 모든 boot가 확인을 지우고 래치의 필드 그대로 Kill 명령을 다시 요청한다. 래치가 없거나 이미 확인된 뒤의 KillLoopEnded는 기록되지 않는다. 모든 게이트웨이 명령은 기록된 이벤트에 command_for를 적용한 것과 같다. 커밋 단계는 Kill을 래치가 내구화된 뒤에만 넘기고, flush가 실패하면 Kill도 응답도 나가지 않으며 다시 시작한 노드에 래치가 없다 | 단위, 통합 | 미실행 | `cargo test -p oms-core --test holds; cargo test -p oms-node --features failpoints --test pipeline a_kill_command` |
| V-HOLD-4 | 킬 마커가 있으면 OMS는 시작할 때 boot 기록 안에서 KillLatched를 걸고(booted, hold_set), 감소 전용을 포함한 어떤 주문도 받지 않으며, 가상 거래소가 unsupported로 답한 뒤 clear_hold kill이 마커를 지운다. 거절된 해제(빈 메모, 다른 사유, not_held, 답을 붙잡아 둔 동안 메모가 있는 kill_unconfirmed)는 마커를 건드리지 않고, 해제 기록의 flush가 실패하면 응답이 없고 노드가 멈추며 마커가 남는다. 그 둘 뒤의 다음 시작은 다시 걸린다. 다음 시작은 걸리지 않고 주문을 받는다. 지우지 못한 마커는 응답이 말하고 다음 시작이 다시 건다. 볼 수 없는 마커 경로(권한)도 건다. 마커 파일은 처음 512바이트를 제어 문자를 바꿔 읽고, 디렉터리·심볼릭 링크도 마커다 | 통합, 단위 | 미실행 | `cargo test -p oms-node --test holds_node; cargo test -p oms-node --features failpoints --test crash_points a_clear_; cargo test -p oms-node --lib node::tests; cargo test -p oms-core --test holds marker` |
| V-HOLD-5 | 거래소 변경 보류는 새 이벤트 없이 venue_position_changed, unclassified_execution_noted, 종류가 venue_position_change인 mismatch_raised(넘침 경로)에서 apply가 건다. 그래서 U13b의 이벤트 목록은 그대로이고, U18b 전에 쓴 저널에 이 셋 중 하나가 있으면 재실행한 상태에 보류가 있다. 다른 종류의 불일치는 걸지 않는다. 불일치를 확인해도 보류는 남는다. 각 보류는 한 번만 걸리고 두 번째 원인은 첫 메모를 바꾸지 않는다 | 단위 | 미실행 | `cargo test -p oms-core --test holds venue second` |
| V-HOLD-6 | 킬 래치·워처 불일치·거래소 변경의 상태 필드, Boot의 kill_marker, 워처 입력과 새 이벤트 셋은 기록 스키마를 올리지 않는다: 비어 있으면 쓰지 않아 옛 상태의 바이트가 그대로이고, 이들이 든 저널은 라이브 상태로 복구되며 verify --re-decide를 통과한다. 상태 스키마는 2로 올랐다: U18b 전의 스냅샷(상태 스키마 1, venue_change 없음)은 사유와 함께 건너뛰고 저널을 처음부터 재생해, 거래소 변경 보류가 스냅샷의 유무에 따라 달라지지 않는다. 이벤트 필드 표는 새 이벤트의 모든 필드를 적는다. 알림 조건 hold:kill_latched·hold:watcher_mismatch·hold:venue_change는 유예 0이고, 킬 문장은 출처·메모·확인 상태를 담으며, 워처 불일치와 거래소 변경의 문장은 증거금 정책이 있을 때만 감소 전용을 받는다고 말하며, Alerts::notice는 접두어를 붙여 한 번 보낸다. [kill] marker는 절대 경로, 데이터 디렉터리 밖, 읽을 때 디렉터리가 있어야 하고, mainnet은 그것 없이 시작하지 않는다 | 단위, 통합 | 미실행 | `cargo test -p oms-node --test record_schema --test alerts; cargo test -p oms-node --lib alerts:: config::; cargo test -p oms-core --test event_fields` |
| V-KILL-1 | Bybit 게이트웨이는 개인 스트림의 마지막 생존 증거(구독 확인, pong, 시작 확인의 첫 통과) 뒤 kill_after_ms가 지나도록 연결이 내려가 있으면 끊김 하나에 한 번 StreamLost를 보내고, 코어는 래치가 없으면 hold_set{kill, source: stream}과 Kill 명령을 한 기록으로 낸다. 마지막 pong 전에는 걸리지 않고, ping에 답하는 조용한 스트림, 기한 전에 다시 구독된 연결, 시작 확인 전의 끊김, 코어 큐를 기다린 시간은 킬을 부르지 않는다. 프로세스에서: 반쯤 열린 개인 스트림(ping 1초, kill_after 10초)은 마지막 pong 9초 뒤에는 디스크에 hold_set이 없고 11초까지 하나 있으며, 가짜 거래소의 첫 일괄 취소는 그 기록이 이미 저널 파일에 있을 때 온다. 다시 시작해도 래치가 남고, 새 프로세스의 루프가 끝나기 전 clear-hold는 kill_unconfirmed로 거절되며 끝난 뒤 받아들여지고 일반 주문이 다시 받아진다 | 단위, 통합 | 미실행 | `cargo test -p oms-node --lib venue::bybit::gateway::tests::; cargo test -p oms-core --test holds stream; cargo test -p oms-node --features failpoints --test bybit_node a_ten_second` |
| V-KILL-2 | 킬 루프는 일괄 취소 뒤 거래소의 미체결을 세어 0이 될 때까지 회차의 시작부터 kill_round_ms마다 일괄 취소를 되풀이하고, 첫 0 뒤 kill_watch_every_ms마다 kill_watch_ms 동안 다시 세어 모두 0일 때만 KillLoopEnded{confirmed}를 보낸다. 실패한 셈은 0이 아니고, 감시 중의 늦은 주문은 곧바로 회차로 돌아간다. 거래소가 첫 일괄 취소 뒤 주문 하나를 남기거나 5xx로 답해도 0을 확인할 때까지 되풀이하고, 어느 1초 창에도 일괄 취소는 둘을 넘지 않으며(비상 예산 2/1초, 회차가 100 ms여도), 생성이나 단건 취소는 하나도 나가지 않는다. 막힘(IP 차단 등)에는 요청을 보내지 않고 막힘 문장으로 회차가 실패하며, 예산을 기다리는 동안 생긴 막힘도 그 요청을 집에 둔다. 셈은 모든 쪽의 열린 행(New, PartiallyFilled, Untriggered)을 orderLinkId와 상관없이 세고, 상태를 읽을 수 없는 행, 실패한 쪽, 20쪽 넘음은 오류다. 정지(IP 차단 뒤, 키 거부) 중에는 셈을 보내지 않는다. 감시는 첫 0 + kill_watch_ms에 나간 셈으로만 끝난다(그 전에 나가 늦게 답한 셈으로는 끝나지 않는다) | 단위, 통합(드라이버) | 미실행 | `cargo test -p oms-node --lib venue::bybit::kill::tests:: venue::bybit::gateway::tests::a_blocked venue::bybit::query::tests::the_kill_count` |
| V-KILL-3 | 킬 루프의 P1은 시작, 알려진 수의 바뀜, kill_notice_ms마다의 반복, 마지막(0건 확인 뒤 감시를 마침)뿐이고 모르는 수는 바뀜이 아니다. 10분 동안 셈이 실패하면 정확히 시작 1 + 반복 10 + 마지막 1 = 12개이고, 드라이버와 Alerts를 loopback 가짜 텔레그램에 붙여도 '킬 루프'가 든 요청은 12개 이하다. 모든 알림은 태그, 회차, 남은 수(모르면 이유), 일괄 취소 수와 바로 전에 읽은 포지션 묶음(심볼마다 수량, 미실현손익, 마크, 마크에서 강제청산가까지의 거리 %, 0.01 %에서 내림, 청산가가 없으면 모름, 크기 0은 뺌, 조회 실패는 실패라고)을 담는다. 알림은 루프 옆에서 순서대로 나가서, 포지션 조회가 답하지 않아도 첫 일괄 취소와 감시 중 늦은 주문의 일괄 취소는 기다리지 않는다. 느린 조회 뒤에서 기다리는 알림은 하나이고 가장 새 수의 것이 남으며, 마지막 알림은 KillLoopEnded보다 먼저 나간다. status의 notices는 보낸 알림 수다. 일괄 취소가 나갈 수 없는 동안(정지, 막힘)은 포지션을 읽지 않고 그렇다고 말한다 | 단위, 통합(드라이버) | 미실행 | `cargo test -p oms-node --lib venue::bybit::kill::tests::` |
| V-KILL-4 | Bybit 게이트웨이는 Kill을 받으면 곧바로 킬 활성을 켜서 루프가 도는 동안 어떤 생성도 내보내지 않고(래치 전에 풀린 생성도 NotSent), 루프가 도는 동안의 두 번째 Kill은 무시하며, KillStop은 다음 대기에서 알림도 입력도 없이 루프를 끝내고 생성이 다시 나간다 | 단위, 통합(게이트웨이·드라이버) | 미실행 | `cargo test -p oms-node --lib venue::bybit::gateway::tests::no_create venue::bybit::gateway::tests::kill_stop venue::bybit::kill::tests::a_stop` |
| V-KILL-5 | 이 프로세스의 루프가 거래소에서 0건을 확인한 뒤(confirmed) 새 킬 계기(StreamLost, watcher_kill)가 오면 코어는 kill_rearmed{source, note}로 확인(confirmed_at, outcome)을 지우고 새 Kill을 부르며, 래치의 첫 메모는 그대로다. 새 루프가 확인하기 전 clear-hold는 kill_unconfirmed로 거절되고, 재실행이 같은 상태를 만든다. 확인 전의 두 번째 계기와 unsupported 답 뒤의 계기는 아무것도 쓰지 않는다. [venue]의 킬 필드 다섯은 설계 기본값으로 읽히고 각 경계 밖 한 걸음은 거절된다(kill_after_ms는 2 × ping 이상) | 단위 | 미실행 | `cargo test -p oms-core --test holds a_new_trigger second; cargo test -p oms-node --lib config::tests::the_kill_loop; cargo test -p oms-core --test event_fields` |
| V-LIVE-1 | [liveness]에 적힌 전략의 마지막 거래 연결이 닫히거나, 그 전략의 어느 연결에서도 liveness_ms 동안 요청이 없으면(ping도 요청이다), 노드는 다음 hello까지 한 번 Input::Session(Lost)를 넣고, 코어는 strategy_link_lost와 같은 기록에서 그 전략의 걸린 일반 주문 가운데 취소 요청이 없는 것마다 규칙 strategy_link의 취소를 요청한다. 그 전략의 감소 전용 주문과 다른 전략의 주문은 취소하지 않는다. 프로세스에서: a(5,000 ms)와 b가 적혔을 때 a의 연결을 닫으면 거래소에서 a의 일반 주문만 취소되고 a의 감소 전용과 b의 일반 주문은 남는다. ping만 8초 보내면 잃음이 없고, 멈추면 마지막 요청 5초 뒤에서 7초 사이에 잃음이 기록된다. 두 연결 가운데 하나가 닫혀도 잃음이 없고 마지막이 닫히면 있다. 운영 소켓의 hello는 세지 않는다. 한 연결이 다른 전략으로 다시 hello하면 앞 전략에서는 닫힌 연결이다: 그것이 a의 마지막 연결이면 곧바로 last trading connection closed로 잃음이 되고 a의 일반 주문이 취소된다. 노드의 표에서: 정확히 liveness_ms의 침묵은 잃음이 아니고 1 ms 넘으면 잃음이며, 침묵은 마지막 요청이나 마지막 hello부터 잰다 | 단위, 통합 | 미실행 | `cargo test -p oms-node --test liveness; cargo test -p oms-node --lib liveness::; cargo test -p oms-core --test strategy_link` |
| V-LIVE-2 | StrategyLink는 그 전략의 일반 주문만 거절한다: holds(Some(s))에만 있고 holds(None)과 다른 전략의 목록에는 없다. 증거금 정책이 있으면 그 전략의 감소 전용 주문은 받고, 없으면 모든 보류처럼 감소 전용도 거절한다. 걸린 동안의 잃음은 기록하지 않고 첫 이유와 시각이 남으며, 설정에 없는 전략의 잃음은 bad_request로 기록하지 않는다. 그 전략이 없는 설정이 활성화되면 보류가 사라진다. 표 7-7의 시험에서 StrategyLink 줄은 일반 주문 거절, 감소 전용 받음이다 | 단위 | 미실행 | `cargo test -p oms-core --test strategy_link --test holds` |
| V-LIVE-3 | 잃음 뒤(또는 내구화된 코어가 그 전략을 보류 중일 때) 거래 소켓의 hello는 Input::Session(Hello)를 control 레인에 넣고, strategy_link_restored가 내구화된 뒤에 답한다: 답이 온 순간 같은 연결의 status(내구화된 기록만)에 보류가 없고 저널 파일에 복구가 있다. 복구가 아직 답을 받지 않은 동안 같은 전략의 hello(다른 연결이든 같은 연결이든)도 복구를 넣으므로 그 답도 앞 복구가 내구화된 뒤에 온다(코어에서는 기록 없는 not held, 커밋 단계가 순서대로 답한다). 큐를 기다리던 잃음은 복구보다 먼저 넣는다(표의 hello가 넣는 순서: lost, hello). 큐가 가득이면 잃음은 버리지 않고 틱마다 다시 넣는다. 재시작 뒤 보류는 저널에서 복원되고 다음 hello가 푼다. 시작할 때는 걸린 일반 주문이 있는 적힌 전략만 무장한다: 프로세스에서 강제 종료(SIGKILL) 뒤 다시 붙지 않으면, 일반·감소 전용 주문이 걸린 a는 시작 5초 뒤 no request for 5000 ms로 잃음이 되고 일반 주문만 취소되며, 감소 전용 주문만 걸린 b는 무장하지 않는다. 걸리지 않은 전략의 hello는 기록하지 않는다 | 단위, 통합 | 미실행 | `cargo test -p oms-node --test liveness hello; cargo test -p oms-node --lib liveness::; cargo test -p oms-core --test strategy_link` |
| V-LIVE-4 | Input::Session, strategy_link_lost, strategy_link_restored, core.strategy_link는 기록 스키마를 올리지 않는다(2 그대로): 비어 있으면 쓰지 않아 옛 상태의 바이트가 그대로이고, 잃음·복구·잃음이 든 저널은 라이브 상태로 복구되며 verify --re-decide를 통과한다. 이벤트 필드 표가 두 이벤트의 모든 필드를 적는다. 알림 strategy_link:<s>는 유예 0이고 이유·시각과 증거금 정책에 따른 감소 전용 문장을 담으며 hello로 해소된다. [liveness]의 값은 1,000..=60,000 ms, 이름은 [core]의 전략이어야 하고, mainnet은 사용 중인 전략이 모두 적혀야 시작한다. 적히지 않은 전략은 감시하지 않는다 | 단위, 통합 | 미실행 | `cargo test -p oms-node --test record_schema; cargo test -p oms-node --lib alerts:: config:: liveness::; cargo test -p oms-core --test event_fields` |
| V-GATE-1 | 모니터링 게이트는 기록 시각으로 판단한다(250 ms 틱, 엄격한 >): 하트비트가 0초에 오고 끊기면 30.0초까지 감시됨, 30.25초에 monitor_changed{false}, 그보다 60초 넘은 첫 틱(90.5초)에 걸린 일반 주문의 취소를 한 번 요청하고(규칙 unmonitored) 그 뒤 틱은 더 묻지 않는다. 5초마다 오는 하트비트에서 W1이나 W2가 0초에 멈추면 10초의 하트비트(정확히 10초)는 감시됨, 15초의 하트비트가 닫고, 하트비트가 계속 와도 취소는 닫은 기록보다 60초 넘은 첫 틱에 온다. 검사의 나이는 그것을 알린 하트비트의 시각에서 잰다. W3·W4는 기록할 뿐 판정하지 않는다. 알려진 한계(리뷰 1 M1): 기록 시각이 멈춘 동안(호스트 시계가 뒤로 감) 틱 2,400개에도 monitor_changed와 취소가 없고, 옛 시계로 찍은 W1·W2의 하트비트는 받아들여져 게이트가 열린 채이며, 새 시계로 590초 뒤에 찍힌 하트비트는 게이트를 닫는다. 감소 전용 주문은 취소하지 않고, 연결이 내려가 있는 동안은 묻지 않으며, 다한 취소는 다시 묻지 않는다. 프로세스에서: W2가 11초 전인 하트비트가 곧바로 닫고, 거래소의 일반 주문은 10초(unmonitored_cancel_us) 전에는 남고 그 뒤 규칙으로 취소되며 감소 전용 주문은 남는다 | 단위, 통합 | 미실행 | `cargo test -p oms-core --test monitor; cargo test -p oms-node --test monitor_node a_stale_heartbeat` |
| V-GATE-2 | 게이트가 있는 설정의 booted는 감시 안 됨(no heartbeat since the start, 하트비트 없음)으로 시작한다: 재시작 전의 하트비트는 세지 않고, 첫 신선한 하트비트까지 일반 주문은 Hold(Unmonitored)로 거절되며, 하트비트가 없으면 booted의 기록 시각보다 60초 넘은 첫 틱에 걸린 일반 주문의 취소를 요청한다. 이미 감시 안 됨일 때의 부팅도 since를 부팅 시각으로 다시 정해, 그보다 60초 전에는 취소하지 않는다. 게이트를 켜는 config_activated도 활성화 시각부터 같다(no heartbeat since the gate came on), 끄는 설정은 상태와 보류를 지우고, 값만 바꾸는 설정은 상태를 그대로 둔다. W1·W2가 신선한 첫 하트비트가 monitor_changed{true}로 풀고, 그 뒤 2단계는 새 요청을 하지 않으며 이미 한 요청은 남는다. 게이트 상태는 저널에서 같은 상태로 재실행된다. 표 7-7의 시험에서 Unmonitored 줄은 일반 주문 거절, 감소 전용 받음이다(증거금 정책 있음). 프로세스에서: 재시작한 노드는 직전에 감시됨이었어도 no heartbeat since the start로 감시 안 됨이다 | 단위, 통합 | 미실행 | `cargo test -p oms-core --test monitor; cargo test -p oms-core --test holds the_hold_table` |
| V-GATE-3 | monitor.sock은 게이트가 있을 때만 열리고(모드 0600) 자기 풀이 연결 둘이다: 셋째 연결은 거절된다. 그 소켓은 전략 없는 hello, ping, heartbeat만 받는다: status, list_open, new_order, halt, watcher_kill, snapshot은 forbidden, 전략을 단 hello는 unauthorized. heartbeat는 거래·운영 소켓에서 forbidden이고 기록되지 않는다. hello 전의 heartbeat는 unauthorized이고 기록되지 않는다. 계약의 줄(epoch ms, null 포함)은 해석되고, 기록이 내구화된 뒤 recorded로 답한다(답이 온 때 저널에 하트비트가 있다). 노드는 ms를 µs로 바꾸고(넘침은 거절), 연결마다 지난 1초에 둘을 받았으면 그 뒤를 overloaded로 거절한다(한꺼번에 보낸 넷 가운데 둘 기록). 코어는 검사 시각이 기록 시각보다 1초 넘게 뒤인 하트비트, W1·W2가 없거나 키가 W1..W9 밖이거나 이름이 1~64자가 아닌 하트비트, 게이트가 없을 때의 하트비트를 bad_request로 거절하고 아무것도 기록하지 않는다 | 단위, 통합 | 미실행 | `cargo test -p oms-node --test monitor_node; cargo test -p oms-node --lib monitor::; cargo test -p oms-core --test monitor; cargo test -p oms-api wire_example_of_the_heartbeat` |
| V-GATE-4 | Input::Watcher(Heartbeat), watcher_heartbeat, monitor_changed, policy.monitor, core.monitor는 기록 스키마를 올리지 않는다(2 그대로): 게이트가 없으면 쓰지 않아 옛 상태와 설정의 바이트가 그대로이고, 게이트를 켜는 설정·신선한 하트비트·오래된 하트비트·2단계의 틱이 든 저널은 라이브 상태로 복구되며 verify --re-decide를 통과한다. 이벤트 필드 표가 두 이벤트의 모든 필드를 적는다(W1..W9 포함). [core.policy.monitor]의 값은 heartbeat_max_age_us 10..=300초, check_max_age_us 2..=60초, unmonitored_cancel_us 10..=600초이고, 모르는 키는 거부되며, mainnet은 표 없이 시작하지 않고 testnet과 가상 거래소는 선택이다. 표가 있으면 runtime.tick_ms는 1,000 이하다(1,001은 거부, 표가 없으면 60,000도 받음). 세 소켓 경로는 서로 달라야 한다. 알림 hold:unmonitored는 지나가는 상태의 유예(30초)를 받고 이유·시각·2단계까지 남은 초(또는 취소 중)와 증거금 정책에 따른 감소 전용 문장을 담으며, 신선한 하트비트로 해소된다. status.core.monitor는 게이트가 없으면 gate: false다 | 단위, 통합 | 미실행 | `cargo test -p oms-node --test record_schema; cargo test -p oms-node --lib alerts:: config::; cargo test -p oms-core --test event_fields; cargo test -p oms-core --test monitor the_gate_values` |
| V-WATCH-1 | 워처 crate는 OMS와 코드를 나누지 않는다: oms-*·md-* crate를 어떤 종류(정상, 빌드, 개발)와 target으로도 선언하지 않고, 개발 의존성 http-fake 말고는 작업 공간 crate를 쓰지 않으며, 그 정상·빌드 그래프에서 OMS·시세·작업 공간 crate에 닿지 않고, watcher-conformance 말고는 아무 작업 공간 crate도 워처에 의존하지 않으며, 소스가 #[path]·include!·속성 안의 path =·심볼릭 링크로, 매니페스트의 target이 소스 경로로 패키지 밖의 파일을 가져오지 않는다. 검사 스크립트는 그 각각을 심으면 그 결과 줄과 종료 코드 1을 내고, 확인할 수 없을 때(맞지 않는 잠금, 없는 매니페스트)는 ok가 아니라 종료 코드 2를 낸다 | 소스 검사 | 미실행 | `tools/ci/watcher_independent.sh && tools/ci/selftest.sh` |
| V-WATCH-2 | 워처의 독자 읽기와 저널 상태가 코어와 같은 집합을 세운다: 코어가 결정하고 oms-journal이 쓴 저널을 기록마다 읽어, 열린 주문 집합, 열린 주문마다 종목·방향·수량·전략·클라이언트 ID·거래소 주문 번호·누적 체결·결과 불명·거래소 종결, 종목마다 포지션, 체결·거래소 변경·펀딩 키와 그 거래소 시각, 체결 바닥, 킬 보류와 워처 불일치 보류, 일 기준값이 같은 기록을 재생한 코어와 같다. 시나리오들이 C-TAIL 표의 이벤트를 모두 쓴다. 닫힌 주문의 정보는 purged까지 남아 늦은 체결의 방향과 종목을 주고, 펀딩은 포지션을 바꾸지 않으며, 체결 바닥은 내려가지 않고 booted는 키를 잊지 않으며, 규칙에 맞지 않는 기록(모르는 주문의 체결, 열린 주문의 purged, 형식이 틀린 필드, 순서가 어긋난 기록)은 래치되는 오류다 | 단위, 통합 | 미실행 | `cargo test -p watcher --test state && cargo test -p watcher-conformance` |
| V-WATCH-3 | 워처의 자기 Bybit 클라이언트는 GET을 거래소 문서대로 서명하고(정렬된 질의, 보낼 수 없는 값은 보내기 전에 거절, 머리글 다섯, 보내는 순간의 시계), 응답을 부류로 나누며(not_sent, no_answer, rate_limited, clock, refused, auth, ip_banned; unreadable은 검사가 만든다), 키 거절(HTTP 401, retCode 10003·10004·10005·10007·10010·33004) 뒤에는 프로세스가 다시 시작할 때까지 Bybit에 아무 요청도 보내지 않고, HTTP 403 뒤에는 응답 순간부터 600초 동안 모든 요청과 IP 반향을 보내지 않으며, 어느 10초 창에서도 60개를 넘겨 보내지 않고 넘는 요청은 버리지 않고 기다린다. 요청은 보내기 전에 세고, 지표는 VictoriaMetrics 가져오기 JSON 한 줄씩(watcher_up 포함) 큐 없이 밀어 넣는다 | 단위, 통합(loopback) | 미실행 | `cargo test -p watcher --test sign --test client --test metrics` |
| V-WATCH-4 | 워처의 설정 파일은 스펙 표의 모든 필드를 그 기본값과 검사대로 읽는다: 모르는 키와 표는 거부하고, mainnet은 mainnet_confirmed = true일 때만 받으며, 범위는 양 끝을 포함하고, status_url과 vm_url은 loopback만, ip_echo_url은 https만, 자격 파일은 절대 경로이고 ..이 없으며 링크를 따라 봐도 데이터 디렉터리 밖이다. 자격 파일은 링크를 따르지 않고 FIFO를 기다리지 않는 한 핸들로, 이 프로세스의 uid가 소유하고 권한이 정확히 0600 또는 0400인 일반 파일일 때만, api_key와 api_secret 둘만 비어 있지 않게 읽는다. 모든 거부는 한 줄이고, 자격 파일의 오류는 내용을 인용하지 않으며 비밀은 어디에도 찍히지 않는다. 시작 확인(U19a4b)은 query-api의 마스터 키, 다른 userID, IP에 묶이지 않은 키([]·*)와 행이 없거나 USDT로 정산되지 않는 기호를 종료 2의 이유 한 줄로 거부하고, 키 거절(auth)이면 끝내지 않고 요청 없이 떠 있으며, 그 밖의 답은 답의 시각 + 5초(403이면 + 600초)에 다시 묻는다. watcher_api_key_days_left는 expiredAt까지의 날을 내림하고 해당 없음(없음, null, 1970-01-02 이하)이면 내보내지 않는다. watcher_public_ip_ok는 반향의 주소가 마지막으로 통과한 ips에 있을 때만 1이다. IP 재확인은 지난 재확인의 마지막 답 + ip_check_ms마다 돌고, 실패하면 0으로 두고 남은 날을 지키며, 시작 규칙에 더는 맞지 않는 키는 재시작까지 모든 Bybit 요청을 멈추고 0으로 둔다. 재확인이나 5단계에서 키가 거절되면 Bybit에는 묻지 않고 반향만 ip_check_ms마다 이어 간다. 요청이 답을 기다리지 않을 때 온 답은 아무것도 바꾸지 않는다 | 단위 | 미실행 | `cargo test -p watcher --test config --test creds --test start` |
| V-WATCH-5 | 워처의 검사 엔진은 첫 라운드와 첫 하트비트를 start_ms에 내고, 한 검사의 라운드를 겹치지 않으며 늦은 라운드는 한 번만 돌린다. 라운드는 거래소 쪽을 모두 읽은 뒤에만 그 라운드만의 상태 요청을 내고, 그 답은 물은 라운드만 쓰며, 그 상한까지 저널을 읽은 뒤 비교한다. 다른 인스턴스의 상태와 그 요청을 낼 때 읽은 기록보다 낮은 상한은 읽기를 재시작까지 래치하고 그 뒤 라운드는 아무것도 묻지 않는다. 다시 읽는 저널 오류는 다음 라운드에 낫고 watcher_journal_error를 0으로 돌린다. 기한(낸 시각 + request_timeout_ms + 11초)까지 답이 없는 요청은 라운드를 실패로 닫고 늦은 답은 버린다. 차이는 지난 완료 라운드와 이번 라운드에 같은 정체로 있고 이번 라운드에서 min_age_ms 이상 늙었을 때만 확정되며(실패한 라운드는 지난 완료 라운드의 sighting을 바꾸지 않는다), 두 번 보았지만 어린 차이는 라운드를 성공으로 치지 않는다. 확정은 정체마다 한 번 세고(한 라운드에 두 번 실린 정체도 하나) 1024바이트 이하의 detail을 실은 Mismatch 하나를 내며, watcher_mismatch_active는 라운드가 그것을 보지 않으면 내려간다. W1은 OMS의 쪽 규칙대로 열린 주문을 읽고(커서 없는 가득 찬 쪽, 같은 커서, 필드 없는 행은 unreadable, 20쪽 한도), 모르는 주문(foreign), 저널에서 닫힘, 거래소 종결 뒤에도 열림, 종목·방향 다름, 확인된 주문의 거래소 부재를 차이로 보고, 확인되지 않았거나 결과 불명이거나 거래소 종결된 주문의 부재는 비교하지 않는다. 나이는 주문의 fact_at(다른 이벤트로 움직이지 않음) 또는 첫 sighting이다 | 단위(sans-IO, 수동 시계) | 미실행 | `cargo test -p watcher --test w1 --test engine` |
| V-WATCH-6 | 워처의 W2는 position/list(category=linear, settleCoin=USDT, 200개씩 5쪽까지)를 W1의 쪽 규칙으로 읽고(커서 없는 가득 찬 쪽, 같은 커서, 한 기호의 두 행(헤지 모드), 모르는 방향, 소수가 아닌 크기, 크기 있는 빈·없는 방향, 음수 크기, 기호 없는 행은 unreadable, 6쪽째는 실패), 기호마다 d = 거래소 − 저널(체결 + 외부 체결 + 거래소 변경 + 조정)을 차이 (W2, position, 기호, d)로 본다(범위를 넘은 d는 값 overflow인 차이). 다른 d는 처음부터 센다. 저널의 마지막 변경 시각과 거래소의 최근 변경 시각(updatedTime, 없으면 그 크기를 처음 본 라운드 seen_at) 중 알려진 것이 min_age_ms보다 어리면 첫 sighting이어도 미루고 라운드는 성공이 아니며, 모르는 시각은 늙음이라 저널이 바꾼 적 없는 기호의 거래소 포지션은 두 번째 라운드에 확정된다. 행이 없는 기호는 크기 0으로 보아 막 닫힌 포지션도 처음 0으로 본 라운드부터 2초 미루고, 첫 완전한 읽기의 크기에는 seen_at이 없으며, 모든 쪽을 읽은 뒤 실패한 라운드의 크기는 마지막 거래소 답의 시각으로 남고, 쪽 사이에서 실패한 라운드는 아무것도 남기지 않는다. 초당 5번의 체결이 60초 이어지는 동안 Mismatch가 없고, 체결이 멈춘 뒤 심은 차이는 라운드의 어느 위상에서도 3초 안에 확정된다 | 단위(sans-IO, 수동 시계) | 미실행 | `cargo test -p watcher --test w2` |
| V-WATCH-7 | 워처의 엔진은 확정된 차이를 그것을 실은 보내기가 "held"로 답할 때까지 빚으로 두고("already held", 보류를 보이는 저널, 실패는 갚지 않고, 라운드가 더 보지 않아도 확정 때의 detail로 보낸다), 저널 상태가 그 검사의 최근 held/already held 답 뒤에 낸 상태 요청의 상한에서 보류 꺼짐을 보일 때, 지난 보내기 시작부터 mismatch_resend_ms가 지났을 때, 그 검사의 보내기가 진행 중이 아닐 때만 보낸다(보고가 없는 보내기는 낸 시각 + 2 × request_timeout_ms + 1초에 실패하고, 그 뒤에 온 늦은 보고는 번호가 달라 아무것도 갚지 않는다). W1·W2는 라운드가 아직 보는 확정된 차이를 해제 뒤 다시 보낸다. admin.sock 클라이언트는 hello 다음 watcher_mismatch를 보내고 두 답을 읽는다. monitor.sock 클라이언트는 연결 하나를 유지하고 hello를 먼저, 그다음 W1~W4의 성공 시각(없으면 null)을 보내며, 끊김·답 없음 뒤 다음 하트비트에 다시 연다. 실행은 설정을 읽은 때부터 지표를 밀어 넣고, 시작 확인, /api/status 기다림, 재생 동안에도 IP를 다시 확인하고, 재확인에서 시작 규칙에 맞지 않는 키는 모든 Bybit 요청을 멈추며, OMS가 답하지 않으면 아무것도 판단하지 않고, 시작 거부는 코드 2, shutdown 입력은 새 라운드 없이 마지막 밀어 넣기와 모니터 연결 닫음 뒤 0으로 끝난다 | 단위(sans-IO, 수동 시계)와 통합(loopback 가짜, 실제 시간 하나) | 미실행 | `cargo test -p watcher --test admin --test heartbeat --test runtime --test binary` |
| V-WATCH-8 | 워처의 W4는 execution/list를 고정된 끝(첫 쪽의 시계)과 시작 max(워터마크 − w4_overlap_ms, ⌈체결 바닥/1000⌉)(7일로 자름)의 창으로 훑고, 라운드마다 w4_max_pages쪽까지 읽어 더 긴 훑기는 다음 라운드가 같은 창과 커서로 이으며(성공 시각은 끝난 훑기에서만), 빈 쪽이나 빈 커서에서 끝내고, 바뀌지 않은 커서와 읽을 수 없는 쪽은 라운드를 실패시켜 같은 커서로 다시 읽되 연달아 3번이나 50라운드면 같은 시작과 새 끝으로 다시 시작한다. 행은 execType으로 먼저 나눠 Trade는 저널 체결과, AdlTrade·BustTrade·Delivery·Settle은 저널 거래소 변경과 키 <symbol>:<execId>로 비교하고, Funding은 비교하지 않으며, 모르는 형은 끝난 훑기에서 바로 확정하고, 형이 필요로 하는 필드가 없는 행과 execTime이 훑기의 끝보다 뒤인 행은 unreadable_row, 바닥 아래 행과 수량 0인 Trade·거래소 변경 행은 무시한다. 차이는 두 훑기와 execTime 나이로 확정하고 어린 차이는 훑기를 실패시키지 않는다. 워터마크는 확정되지 않은 가장 오래된 행(execTime 없는 행은 모든 행보다 오래됨) 아래의 가장 새 행으로만, 뒤로 가지 않게 움직인다. held 답(already held는 갚지 않는다)으로 갚고 그 답 뒤에 낸 상한에서 보류 꺼짐을 본 W4 정체는 거두어 다시 보내지도 세지도 않는다 | 단위(sans-IO, 수동 시계)와 통합(loopback 가짜, 실제 시간 하나) | 미실행 | `cargo test -p watcher --test w4 --test runtime` |
| V-WATCH-9 | 워처의 W3은 w3_ms마다 wallet-balance?accountType=UNIFIED의 UNIFIED 계좌 하나(정확히 하나, 세 필드가 저널 소수, 유지증거금이 음수가 아님; 아니면 unreadable로 라운드 실패)를 OMS의 공식으로 판단한다: account_mm_rate는 maintenance > 0이고 maintenance × 10 000 ≥ account_mm_rate_max_bps × margin_balance(같으면 건다), daily_loss는 저널의 일 기준값이 오늘의 UTC 날이고 (base − totalEquity) × 10 000 > daily_loss_bps × base(오늘의 기준이 없으면 판단하지 않는다). 정체는 종류뿐이고 두 라운드 연속에서 나이 없이 확정하며, 위반이 남아 있으면 해제 뒤 다시 보낸다. W2는 W4가 마지막으로 맞춘 그 기호의 execTime이 updatedTime이나 seen_at보다 새면 그것을 거래소의 변경 시각으로 쓴다. 하트비트의 W3·W4는 그 검사의 마지막 성공 시각이고, 실행은 W2(W4의 시각 지도와 함께), W3, W4를 돌리며 엔진 전에는 네 검사의 나이를 boot_ms부터 내보낸다 | 단위(sans-IO, 수동 시계)와 통합(loopback 가짜, 실제 시간 하나) | 미실행 | `cargo test -p watcher --test w3 --test w2 --test heartbeat --test runtime` |
| V-WATCH-10 | W1·W2·W3·W4를 설계 간격(1초, 1초, 2초, 5초)으로 돌린 모의 1분(수동 시계, 실제 rest::Guard를 거친 모델 거래소, 요청 지연 0)에서 5초에 체결 30개를 고르게 주면 대조 요청은 W1 60 + W2 60 + W3 30 + W4 12 = 162개, 초당 2.7 이하이고 차이도 실패한 라운드도 없다. 5초에 100개면 192개(초당 3.2; 정상 상태는 훑기당 4쪽, 초당 3.3)로 설계 예산을 넘지만 W4는 12 훑기를 모두 자기 라운드에서 끝내고 정상 상태의 창은 15초다. 열린 주문 500개(W1 라운드 11쪽)에서는 가드가 10초에 60을 지키며 W1 라운드를 늘리지만 하트비트마다 W1·W2의 나이는 10초 아래다 | 단위(sans-IO 엔진과 sans-IO 가드, 수동 시계) | 미실행 | `cargo test -p watcher --test budget -- --nocapture` |
| V-WATCH-11 | 워처의 [telegram]은 token_file 하나만 받고(모르는 키 거부) mainnet은 그것 없이 시작하지 않는다(testnet은 warn 한 줄). 토큰 파일은 설정의 경로 규칙과 자격 파일의 파일 규칙(링크 아님, 일반 파일, 실효 uid 소유, 정확히 0600·0400)을 여는 핸들에 적용하고, 적힌 경로나 가리키는 곳이 git 저장소 안이면 읽기 전에 거부하며, {"token": "<digits>:<[A-Za-z0-9_-]+>", "chat_id": 비지 않은 문자열 또는 정수} 말고는 거부한다. 거부는 내용과 토큰을 인용하지 않고, 실행은 그것을 시작 확인 전에 종료 코드 2로 끝내며 요청을 보내지 않는다. 읽은 값과 설정의 Debug는 토큰을 보이지 않는다. 메시지는 [WATCH P1] bybit {name} {MM-DD HH:MM:SS KST}로 시작하고, split은 각 부분을 3,500 UTF-16 단위 이하로 줄 끝에서(긴 줄은 문자 단위로, 이모지를 쪼개지 않고) 자르며 글자를 잃지 않는다(부분 경계와 맨 앞의 빈 줄은 사라질 수 있고, 빈 부분은 만들지 않는다). Outbox는 50개를 넘으면 가장 오래된 부분을 버리고 세고, 큐가 빈 뒤 알림 하나로 그 수를 알리며, 보내는 중 버려진 부분을 두 번 지우거나 세지 않는다. outcome은 2xx와 ok true만 Delivered, 400·413은 Refused(버리고 셈), 나머지는 Retry(두고 흘려보내기를 끝냄)다. Repeat는 시작 한 번, 300,000 ms마다 최신 문구 한 번, 해소: <시작 문구> 한 번을 낸다. 송신 태스크(alert::spawn, U19b1b)는 경로를 순서대로 써서 네트워크 오류면 다음 경로(DNS 다음 고정 주소)로 가고, HTTP 답이 결정하며, 답한 경로가 다음부터 맨 앞이다. 부분을 순서대로 하나씩 보내고 Retry에서 그 부분을 두고 흘려보내기를 끝낸다. 새 메시지가 Notify로 바로 깨우고, 그 밖에는 흘려보내기가 끝난 주입된 시계 + 5,000 ms에 실행의 틱(Handle::tick)만 깨운다(자기 타이머 없음). 답이 오지 않는 요청은 실제 시간 10,000 ms의 요청 한도에서 실패로 세고 그 부분을 둔다. 로그 줄은 경로와 원인을 적고 토큰과 URL을 적지 않는다. finish(5,000)은 마지막 흘려보내기를 실제 시간 5,000 ms 안에서 하고 남은 부분을 한 줄로 센다. 네 지표(queued, delivered, dropped, failures)가 이것을 따른다. P1 조건(U19b1c): 엔진은 라운드마다 새로 확정한 식별자의 내용을 Action::Confirmed 하나로 그 라운드의 Mismatch보다 앞에 내고 다시 보내기는 내지 않는다. 불일치 P1은 확정마다이고 검사마다 첫 것은 바로, 그 뒤 마지막 메시지에서 60,000 ms에 하나이며 그 사이 확정은 모아(내용 다섯, 각 600 UTF-16 단위, 나머지는 수) 다음 메시지로 간다. Client::auth_stop은 래치를 건 첫 답(retMsg, HTTP 401의 본문)이나 규칙 멈춤의 이유를 바꾸지 않고 들며, 실행은 틱마다 그것과 start::PublicIp(게이지가 0인 이유)를 반복 규칙에 줘서 다시 보내지 않는 답(10010·10003·33004·401 포함)마다 답 뒤의 첫 틱에 하나, 5분마다 하나, 공인 IP는 해소 하나를 보낸다. 송신기는 시작 확인 전에 뜨고(2단계의 키 거부도 알림), 시작 거부는 P1 하나를 보내고 5,000 ms 안에 흘려보낸 뒤 종료 코드 2, 종료는 60초 간격 안에 모인 불일치 메시지를 간격과 상관없이 넣고 U19a 단계 뒤 흘려보낸다. 시작 거부의 이유는 200자로 자른다. 실행의 틱이 송신기의 다시 보내기를 깨운다. [telegram]이 없으면 모든 P1(불일치, 키 거부, 시작 거부)은 warn 한 줄이다 | 단위(순수 함수, 파일 시스템)와 통합(runtime::run과 송신 태스크, loopback 가짜, 실제 시간) | 미실행 | `cargo test -p watcher --test alert --test config --test creds --test runtime --test engine --test client` |
| V-WATCH-12 | 워처의 서명된 POST는 Client::cancel_all 하나뿐이다: 경로 /v5/order/cancel-all과 본문 {"category":"linear","settleCoin":"USDT"}는 상수이고, 서명은 timestamp + api_key + recv_window + 보내는 본문의 바이트 위에 하며(거래소의 POST 예와 독립 HMAC 값), 머리글 다섯과 Content-Type application/json을 달고, 요청 제한·403 멈춤·키 래치를 지나며 보내기 전에 watcher_requests_total에 센다. 답은 success가 없거나 "1"이면 result.list의 길이, "0"이면 refused, 목록이 아니거나 다른 값이면 unreadable이다. crates/watcher/src의 글자에는 .post(가 rest.rs·metrics.rs·alert.rs에 하나씩만 있고 Method::·.request(·.put(·.patch(·.delete(는 없다. [kill]은 round_ms·watch_ms·watch_every_ms·notice_ms를 OMS의 kill_* 기본값과 범위로 받고, 그것이 있으면 시작 2단계와 재확인은 readOnly 0(정수), ContractTrade의 Order, 어느 목록에도 Withdraw 없음을 요구한다(시작은 종료 2, 재확인은 재시작까지 모든 Bybit 요청과 일괄 취소를 멈춤). 읽히는 규칙 하나가 맞지 않으면 다른 필드나 목록을 읽을 수 없어도 거부하고, 읽히는 규칙이 모두 맞는데 읽을 수 없는 것이 있으면 시작은 5초 뒤, 재확인은 ip_check_ms 뒤 다시 묻는다(재확인 실패, 멈춤 아님). [kill]이 없으면 그것을 읽지 않는다. Dec::percent_floor는 0.01 %로 내리고 넘치면 None이다. 킬 알림의 포지션 묶음은 기호마다 방향·수량·미실현손익·마크·마크에서 강제청산가까지의 거리(내림)를 적고, 청산가가 비었거나 0이거나 마크가 0 이하면 모름, 크기 0인 행은 빼고 크기를 읽을 수 없는 행은 수량 모름으로 보이며(포지션 없음이라 하지 않음) 20행 뒤는 수로 센다. 포지션 읽기는 5쪽까지, 읽기 시작 + request_timeout_ms + 11,000 ms의 기한 때나 뒤에 답한 쪽에서 끝내고 다음 쪽을 보내지 않으며, 래치나 멈춤이면 묻지 않는다. 알림 문구는 워처 킬로 시작하고, 마지막 알림의 줄은 StopOutcome 다섯을 따르며 래치와 경로를 확인한 마커의 멈춤만 clear-hold를 말하고, 키 래치와 403 멈춤의 진행 문구는 정한 줄이며, 중단 알림은 포지션 조회 안 함(워처 종료 중)으로 끝난다. sans-IO KillLoop(U19b2b)는 보낸 모든 일괄 취소 뒤(실패해도) 세고, 보내지 않은 것 뒤에는 세지 않고 다음 라운드를 기다리며, 세기 오류는 0이 아니고, n > 0이면 t_r + round_ms까지 기다리며, 첫 0의 z부터 watch_every_ms마다 세어 z + watch_ms 때나 뒤에 낸 세기의 0에서만 끝난다(지켜봄의 늦은 주문은 곧바로 일괄 취소, C5에 들어올 때마다 새 z, watch_ms가 watch_every_ms의 배수가 아니어도 마지막 세기는 z + watch_ms, 지켜봄의 늦은 주문이 연 라운드는 그 세기의 시각에 시작). 두 번째 start는 아무것도 하지 않는다. 요청 둘이 함께 열리지 않고, 어느 [t, t + 1,000) ms에도 일괄 취소를 둘 넘게 내지 않으며(설정에 맞는 타이밍에서는 예산이 잡지 않고 두 일괄 취소가 1,000 ms보다 가깝지 않다), 요청의 기한은 낸 시각 + request_timeout_ms + 11,000 ms이고 세기는 기한 전에 답한 쪽마다 그것을 옮기며, 기한 때나 뒤의 답·쪽은 기한 뒤(세기 오류, 일괄 취소 Failed)이고 그 뒤의 같은 req_id 답과 쪽은 버리며(버린 세기의 쪽은 지금 열린 세기의 기한을 옮기지 않는다), next_wake는 기한·라운드·지켜봄·예산의 시각과 다음 진행 표시 중 가장 이른 것이다. 알림은 시작, 시작에 묶인 notice_ms 표시마다 진행 하나(건수가 같아도; 바뀐 건수의 알림은 없다), 끝에서 곧바로 마지막, 중단 하나와 끝이고, 10분 동안 조회가 실패해도 600초 창마다 12개 이하(시작 1 + 진행 10 + 마지막 1)다. U19b2c의 세기는 /v5/order/realtime linear USDT의 모든 쪽을 W1의 쪽 규칙으로 20쪽까지 읽고(20쪽째에도 커서가 있으면 세기 오류), orderStatus가 New·PartiallyFilled·Untriggered인 행을 orderLinkId와 상관없이 세며(남의 주문도), orderStatus가 없거나 읽을 수 없는 쪽은 세기 오류다. 세기 태스크는 루프가 그 세기를 낸 시각(첫 실행이 아님), 그 뒤 쪽마다 답한 시각 + request_timeout_ms + 11,000 ms의 기한을 스스로 지켜 기한 때나 뒤에 답한 쪽 뒤에는 묻지 않는다. 일괄 취소가 나갔는지는 보내기 전 상태로 가린다(래치 NotSent(Latched), 403 멈춤 동안 NotSent(Paused)이고 멈춤이 끝나면 다시 보냄, 연결 못 함 NotSent(Local), 그 밖은 Failed). kill::Inflight는 루프의 요청마다 태스크 하나를 돌리고 루프의 기한(open_request) 때나 뒤에, 또는 루프가 버린 요청이면 곧바로 끊고 그 태스크가 보낸 것을 루프에 주지 않으며, 새 요청 전에 산 태스크를 끊고, Inflight가 사라질 때 모두 끊는다. http-fake와 수동 시계의 시험 장치에서 일괄 취소 뒤 한 건이 남거나 5xx가 와도 루프는 0을 확인할 때까지 되풀이하고 어느 [t, t + 1,000) ms에도 일괄 취소가 거래소에 둘 넘게 닿지 않는다 | 단위(순수 함수)와 통합(실제 Client, loopback 가짜, 수동 시계); 루프는 sans-IO 상태 기계(손으로 준 시각, 시드 무작위 답 순서); 세기·Inflight·시험 장치는 통합(실제 Client와 tokio 태스크, loopback 가짜, 수동 시계, 실제 시간으로 기다린 뒤에만 시계를 옮김) | 미실행 | `cargo test -p watcher --lib --test sign --test client --test config --test start --test kill` |
| V-WATCH-13 | [w5]은 probe_ms 1,000(500~5,000)과 silence_ms 10,000(3,000~60,000)을 받고 [kill]이 있으면 silence_ms ≥ 3 × probe_ms + 3 × request_timeout_ms + 1,000이어야 한다(잃은 탐침 하나도, 틈 한계보다 짧은 워처의 멈춤 하나도 건강한 OMS를 킬하지 않게). [kill]은 marker(OMS의 규칙: 절대 경로, .. 없음, 가리키는 곳이 data_dir 밖, 디렉터리가 있음)와 oms_unit([A-Za-z0-9:_.@-] 1~255자, .service로 끝남, -로 시작하지 않음)이 필수이고 systemd system\|user와 stop_wait_ms 10,000(1,000~120,000)을 받으며, mainnet은 [kill] 없이 시작하지 않고 testnet은 warn 한 줄을 남긴다. sans-IO W5는 [kill]이 없으면 Off, arm 전에는 탐침도 쏨도 없고, Armed와 Rearming에서 앞 탐침을 낸 때부터 probe_ms마다 탐침 하나를 내되 둘을 함께 열지 않으며, 낸 시각 + request_timeout_ms + 1,000 ms 때나 뒤에 답 없는 탐침은 실패이고, 탐침의 답은 온 때 받으며(기한 정각이라도 poll이 적용하기 전이면), 열리지 않은 탐침의 답은 버리고, 2xx JSON에 16자리 16진수 core.instance_id인 답만 정상이다. Armed에서 now − last_good_ms ≥ silence_ms이고 last_good_ms 때나 뒤에 낸 탐침이 실패했을 때만 Fire를 한 번 내고 Killing에서는 탐침이 없다. 두 시각이 2 × probe_ms + request_timeout_ms보다 멀면 W5가 가진 시각을 모두 그 틈만큼 옮기고 warn 한 줄을 남긴다(보지 못한 시간은 침묵이 아니다). kill_ended 뒤에는 정상 응답 하나가 Armed로 돌리고, 정지를 빚졌으면 그 응답이 OwedHalt 하나를 내며(진행 중에는 둘째가 없음) latched나 already latched로 시작하는 답이면 늦은 정지 P1과 Armed, 그 밖에는 빚이 남고 첫 실패에만 늦은 정지 실패 P1, 실패 뒤의 래치는 해소 줄을 단다. clock::Steady는 안쪽 시계가 뒤로 가도 앞서 준 값보다 작은 값을 주지 않고 그 뒤의 진행은 그대로 더하며 앞으로 뛴 걸음은 지나가게 하고 모든 읽는 쪽에 한 시간선을 준다. oms::admin_kill은 연결 하나에 hello 다음 watcher_kill {detail}을 보내 답마다 timeout_ms, 전체 2 × timeout_ms 안에 result나 이유를 준다. stop::Stopper는 정확히 <systemctl> [--user] --no-ask-password stop -- <oms_unit>을 stdin·stdout null, stderr 비차단 파이프로 띄우고, 틱마다 try_wait만 하며 끝난 뒤에만 stderr를 4 KiB까지 기다리지 않고 읽어, 0이면 Stopped, 다른 끝은 exit <code>: <첫 줄>이나 signal <n>, 주입된 시계의 기한 때나 뒤에 아직 돌면 죽이고 Unconfirmed, 그 뒤의 틱이 거둔다. crates/watcher/src의 글자에 Command::new는 stop.rs에 하나뿐이다. 킬 순서 kill::Kill(U19b3b, sans-IO)은 K1 WriteMarker(기한 2,000 ms, 실패나 기한에도 계속, 늦은 결과는 버림) → K2 Halt(기한 2 × request_timeout_ms + 1,000 ms; latched·already latched·재무장 답만 정지; 열린 시도의 답은 기한 정각이라도 poll 전이면 받고 닫힌 시도의 답은 버림) → K2가 정지하지 못했을 때만 K3 StopUnit(now + stop_wait_ms; 지나면 Unconfirmed, 늦은 결과는 버림) → KillLoop::start의 순서로 내고, 첫 CancelAll과 시작 알림은 그 뒤에만 나온다. 머리는 W5의 침묵, 원래 시계의 마지막 정상 응답 KST(없으면 이 프로세스에서 없음), 킬 마커(씀·이미 있음, 경로를 확인하지 않았으면 그 말, 쓰지 못함(<kind>), 시간 초과), OMS 정지(watcher_kill → <답>, 또는 watcher_kill 실패(<이유>) → systemctl stop <unit>: 정지됨·실패·정지 확인 못 함)를 말하고 K2가 실패하고 멈춤이 확인되지 않으면 X8 줄을 달며, 자유 문구는 글자 경계에서 200 UTF-16 단위로 잘리고(BMP 밖 글자는 2단위) 제어 문자가 없어 머리는 침묵 10초에서 가장 길 때 1,029 단위이고 침묵의 초 문구 글자 하나마다 1단위 늘어 가장 길 때 1,048 단위다(래치한 답도 같이 자름). 루프를 시작할 때 halt_owed = !halted && !(stopped && marker_in_place)이고 marker_in_place는 Written·AlreadyPresent와 marker_checked다; StopOutcome은 Latched·StoppedMarked·StoppedUncheckedMarker·StoppedUnmarked·Unconfirmed이고 마지막 알림의 줄과 Ended의 halt_owed가 같다. 빚이 있는 동안 루프에서 앞 시도가 끝난 때(답이 온 때나 기한, 답 없는 K2의 기한 포함)부터 5,000 ms마다 Halt를 다시 내되 둘을 함께 열지 않고(워처 키가 래치되어도), 루프가 끝날 때 열린 시도는 닫으며(halt_open 없음, 늦은 답은 버림, Ended의 halt_owed는 참), 래치한 답은 LateHalt 알림 하나, 빚 없음, 마지막 줄 Latched와 재시도 끝을 준다. Kill::abort는 루프 전이면 그때까지의 머리(끝나지 않은 단계는 진행 중)와 일괄 취소 전 줄, 포지션 조회 안 함 줄의 알림 하나와 Ended(aborted, halt_owed)를, 루프 안이면 루프의 중단을 준다. clock::Steady 위의 Kill은 원래 시계가 킬 전과 루프 중에 60초 뒤로 가도 일괄 취소·재시도·진행 알림을 진행 기준으로 제때 내고, 마커 문구와 머리의 시각은 원래 시계다. kill::write_marker는 무엇이든 있으면 AlreadyPresent로 건드리지 않고, 없으면 자기 임시 이름(옛 것은 먼저 지움)에 0600으로 써 sync_all, hard_link, 임시 이름 지움, 디렉터리 sync_all 뒤 Written, 오류는 Failed(<io 종류>); 문구는 512바이트, detail은 1,024바이트 이하로 글자 경계에서 자르고 OMS의 read_kill_marker는 그 파일에서 marker: <문구>를 읽는다. 실행(U19b3c)은 시작 확인이 끝난 때 W5를 무장하고 그 뒤 모든 단계에서 틱마다 돌린다. [kill]이 있으면 node.kill_marker가 워처의 마커와 같은 파일(가리키는 곳으로 비교)을 가리키지 않는 상태 답(configured false 포함)에 시작을 거부한다(종료 코드 2, 시작 거부 P1, 같은 파일을 가리키는 링크는 됨). W5가 쏘면 마커를 쓰고, watcher_kill을 보내고(답 없는 시도는 K2 기한에 태스크를 끊어 연결을 닫음), 그것이 래치하지 못했을 때만 systemctl stop을 돌린 뒤에야 첫 일괄 취소를 보내며, 루프를 Inflight로 돌려 남은 주문·5xx 일괄 취소·5xx 세기에도 0건 확인까지 round_ms마다(어느 1초에도 둘 이하, 보낸 일괄 취소마다 세기 하나) 되풀이한다. 빚진 정지는 루프 중 5초마다 다시 보내 돌아온 OMS를 킬 동안 래치하고(watcher_kill 하나, 늦은 정지 알림 하나, 마지막 알림 OMS는 래치됨, 워처 키가 10010으로 래치되어 일괄 취소가 없어도), 킬 동안 엔진은 라운드도 하트비트도 내지 않고 검사 나이는 계속 오르며, 끝나면 엔진이 다시 돌고 정상 응답 하나 뒤 W5가 다시 무장한다. 시계가 루프 중 60초 뒤로 가도 다음 일괄 취소는 round_ms + 한 걸음 안에 나간다. OMS가 내려간 채 시작한 워처도 침묵 뒤 킬하고 시작 알림은 이 프로세스에서 없음과 경로 확인 못 함을 말하며, 실행이 남긴 마커는 0600 일반 파일이고 임시 파일이 남지 않는다. 킬 알림마다 포지션 묶음이 붙는다. 루프 뒤 정지를 빚진 킬 다음 OMS가 다시 답하면 watcher_kill 하나를 보내고(K2의 기한, 답이 없으면 늦은 정지 실패 P1 하나, 래치하면 늦은 정지 P1과 해소, W5가 다시 지킴), 시작 뒤 탐침 답의 마커 경로가 다르면 marker_path P1과 함께 마커를 제자리로 세지 않아 그때의 킬이 정지를 빚으며, 킬 중의 종료는 중단 알림 하나로 끝나고 그 뒤 일괄 취소가 없고 systemctl 클라이언트를 죽이지 않으며, 킬이 끝난 바로 뒤의 종료도 포지션 읽기를 기다리던 마지막 알림을 조회 안 함 줄과 함께 보내고, 킬 중의 시작 거부는 중단 알림 다음 거부 P1과 종료 코드 2이고 정지를 빚졌으면(루프 전 K1~K3 중이면 빚으로 봄) 그 전에 watcher_kill 하나(나가 있는 것이 있으면 그것)를 보내 결과를 거부 P1에 적는다. 600초 동안 조회가 실패해도 [s, s + 600 s]의 메시지는 12개 이하이고, 킬의 모든 메시지가 포지션 묶음이나 조회 실패나 조회 안 함 줄을 가지며, 기한 뒤에 답한 포지션 읽기는 시간 초과이고 다음 쪽을 묻지 않는다 | 단위(sans-IO W5, 손으로 준 시각과 Steady 위의 수동 시계; 설정의 순수 검증; Steady의 두 스레드)와 통합(oms::admin_kill과 시험의 UnixListener, w5::good_answer와 http-fake의 /api/status, stop::Stopper와 OMS_TEST_DIR 아래의 셸 스크립트, 실제 프로세스와 실제 시간); 단위(sans-IO Kill, 손으로 준 시각과 Steady 위의 수동 시계)와 통합(kill::write_marker와 OMS_TEST_DIR 아래의 실제 파일, spawn_blocking; watcher-conformance에서 OMS 자신의 read_kill_marker); 통합(runtime::run과 loopback http-fake의 Bybit·/api/status·텔레그램·VM, 시험의 admin.sock·monitor.sock, OMS_TEST_DIR 아래의 가짜 systemctl, 시험 디렉터리의 마커, 수동 시계와 실제 시간의 walk_kill) | 미실행 | `cargo test -p watcher --test config --test clock --test w5 --test admin --test stop --test runtime --test kill_sequence && cargo test -p watcher-conformance --test marker` |

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
| `core/latch_and_day_base_replay_to_the_same_state` | 이전 버전 결과 | last result: passed |

## V-SIM-1 — 네트워크 장애, 통보 중복·지연, 링크 단절, 프로세스 재시작, 키 삭제가 임의로 섞인 실행의 매 단계에서: 구조 불변조건이 성립하고, 거래소의 실제 포지션이 OMS가 설명하는 범위 안에 있고, 한도를 넘지 않고, 같은 주문이 두 번 전송되지 않는다

- 상태: **이전 버전 결과**
- 왜 중요한가: 예제 시험이 생각하지 못한 순서를 찾는다. 실제로 결함 넷을 찾아 고쳤다
- 수준: 시뮬레이션
- 전제와 범위: 프로파일 6개(calm, lossy, crashy, weak_evidence, purge, skewed)와 증거금 정책을 켠 3개(margin_calm, margin_lossy, margin_days: 증거금 검사는 V-MARGIN-12). 실행당 1500단계(margin_days는 가상 시간 50시간, 약 15 000단계이고 seed는 10분의 1). seed 수는 결과의 notes에 있다. 한 종목, 두 전략, 가상 시계
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
| `sim/profile_margin_calm` | 이전 버전 결과 | last result: passed |
| `sim/profile_margin_lossy` | 이전 버전 결과 | last result: passed |
| `sim/profile_margin_days_crosses_three_utc_days_with_restarts` | 이전 버전 결과 | last result: passed |

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

## V-CFG-1 — 설정은 버전이 커야 활성화되고, 같은 버전에 다른 내용이면 시작을 거부한다. 노출이 남은 종목·전략은 제거할 수 없다. mainnet은 증거금 정책(policy.margin) 없이는 시작하지 않고, 정책의 값이 C-MARGIN 설정 표의 범위를 벗어나면 어느 거래소 설정에서도 시작하지 않는다. 저장소의 Hyperliquid 예제 설정은 증거금 정책(소유자가 정한 값)을 담고 검사를 통과한다. Hyperliquid 설정에 증거금 정책이 있으면 설정에서 계산한 분당 조회 weight(216 + 종목마다 20, 기본 간격)가 600을 넘을 때 시작하지 않는다

- 상태: **이전 버전 결과**
- 왜 중요한가: 설정 변경이 추적되지 않으면 사고 분석이 불가능하다
- 수준: 프로세스
- 전제와 범위: 코어 예제, 실제 노드 1회, 노드 설정 검사의 단위 시험(증거금 정책)
- 실제 구성요소: oms-core, oms-node
- 모의·시뮬레이션: 없음
- 주입한 장애: 버전을 올리지 않은 변경. 정책 없는 mainnet 설정, 0인 한도, 신선도 한도보다 크지 않은 보고 끊김 취소 한도, 종목 20개, 짧은 조회 간격
- 판정 기준과 근거: 종료 코드 3과 오류 문구
- 재현: `cargo test -p oms-node --test e2e configuration_changes && cargo test -p oms-node --lib config::tests`
- 뒷받침하는 것: 요구 R13, C-BOOT
- 확인하지 않은 범위: 설정 파일의 그 밖의 검증 규칙. 조회 weight는 문서의 weight 표로 계산한 값이고 거래소가 실제로 세는 값과 대조하지 않았다. 정상 상태가 아닌 조회(답을 받지 못한 동안 간격마다 되묻는 계좌 모드 조회, 링크 세션이 시작할 때 포지션 있는 종목의 설정 조회, 주문 상태 조회)와 주문·취소는 예산 계산에 없다. 답이 없는 거래소 설정 조회는 종목마다 간격에 한 번이라 계산 안에 있다(V-MARGIN-11의 대조기 시험). 증거금 정책이 없는 설정에는 예산 검사를 하지 않는다. account_modes_allowed의 내용(목록에 무엇을 적든 설정은 읽힌다. 허용 여부는 코어가 판정한다: V-MARGIN-9)

| 시험 | 결과 | 비고 |
|---|---|---|
| `core/configuration_changes_are_validated_against_existing_exposure` | 이전 버전 결과 | last result: passed |
| `e2e/configuration_changes_need_a_new_version` | 이전 버전 결과 | last result: passed |
| `core/f10_config_validation_enforces_the_parked_fill_wait_rule` | 이전 버전 결과 | last result: passed |
| `node-unit/config::tests::mainnet_is_refused_without_a_margin_policy_and_bad_margin_values_are_refused` | 이전 버전 결과 | last result: passed |
| `node-unit/config::tests::example_config_with_a_margin_policy_loads` | 이전 버전 결과 | last result: passed |
| `node-unit/config::tests::steady_state_query_weight_is_computed_from_config_and_a_budget_above_600_is_refused` | 이전 버전 결과 | last result: passed |

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

## V-GW-SEAM-1 — VenueGateway를 구현한 세 번째(가짜) 게이트웨이가 oms-core를 고치지 않고 구동기에 꽂히며, 구동기는 상시 작업 시작(spawn_tasks)을 명령 루프를 띄우기 전에 한 번 동기적으로 부르고, 주문 명령은 send_order로, 취소 명령은 send_cancel로, 조회는 query로 각각 한 번씩 간다. 동시에 도는 명령은 max_concurrent, 조회는 QUERY_CONCURRENCY를 넘지 않고 넘치는 것은 기다렸다가 모두 처리된다

- 상태: **미실행**
- 왜 중요한가: Bybit 게이트웨이가 배선을 복사하지 않고 같은 상한과 시작 순서를 물려받는 전제
- 수준: 단위
- 전제와 범위: 명령 13건, 조회 8건
- 실제 구성요소: 구동기 venue::start, 채널, 세마포어
- 모의·시뮬레이션: 게이트웨이(기록하는 가짜)
- 주입한 장애: 없음. 가짜가 열어 줄 때까지 모든 작업이 멈춰 있어 상한에 걸린 상태를 만든다. 순서 시험은 명령을 시작 전에 큐에 넣고, 다중 스레드 런타임에서 가짜의 spawn_tasks가 20 ms 늦게 기록한다
- 판정 기준과 근거: 가짜가 남긴 호출 기록(첫 줄이 상시 작업 시작, 나머지가 명령·조회 각 한 번), 동시 실행 최대값 3과 2, 처리 수 16
- 재현: `cargo test -p oms-node --lib venue::tests`
- 뒷받침하는 것: C-GW
- 확인하지 않은 범위: 가상 거래소와 Hyperliquid 게이트웨이가 구동기를 거쳐도 전과 같다는 것은 이 시험이 아니라 기존 e2e(가상 거래소)가 본다. Hyperliquid 경로는 실제 거래소 시험 없이 컴파일과 단위 시험만 확인했다

| 시험 | 결과 | 비고 |
|---|---|---|
| `node-unit/venue::tests::a_gateway_plugged_in_through_the_trait_gets_each_command_and_query_once` | 미실행 |  |
| `node-unit/venue::tests::the_driver_bounds_concurrent_commands_and_queries_and_drops_none` | 미실행 |  |

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
| `perf/nvme-final` | 이전 버전 결과 | 20/s: p99 5.91 ms, unanswered 0, error kinds 0; 200/s: p99 8.65 ms, unanswered 0, error kinds 0 |

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
| `perf/nvme-final` | 이전 버전 결과 | 2000/s [commit_thread, limits lifted]: p99 18.00 ms, unanswered 0, queues within capacity: true, error kinds 0 |
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
| `perf/recovery` | 이전 버전 결과 | 54023 records replayed in 154 ms |

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
| `mutants/all` | 이전 버전 결과 | 122 mutants, survived: [] |

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
| `perf/nvme-compare-thread` | 이전 버전 결과 | 200/s [commit_thread]: p99 9.30 ms, unanswered 0, error kinds 0; 2000/s [commit_thread]: p99 15.88 ms, unanswered 0, error kinds 0; 8000/s [commit_thread]: p99 16.37 ms, unanswered 0, error kinds 0 |
| `perf/nvme-compare-inline` | 이전 버전 결과 | 200/s [inline]: p99 8.71 ms, unanswered 0, error kinds 0; 2000/s [inline]: p99 12.09 ms, unanswered 0, error kinds 0; 8000/s [inline]: p99 882.20 ms, unanswered 0, error kinds 1 |

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
| `perf/fsync-nvme` | 이전 버전 결과 | write+fdatasync: p50 2406 p99 2875 us; fdatasync: p50 2404 p99 2874 us |
| `perf/core-bench` | 이전 버전 결과 | handle(new_order): p50 27356 p99 33446 ns; handle(send_result placed): p50 920 p99 1240 ns; handle(fill closing the order): p50 1310 p99 1809 ns; encode record (serde_json): p50 539 p99 1750 ns; record size: p50 467 p99 550 bytes; handle(new_order), margin policy on: p50 65857 p99 89769 ns; handle(send_result placed), margin policy on: p50 1007 p99 1472 ns; handle(fill closing the order), margin policy on: p50 1424 p99 2313 ns |

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
- 확인하지 않은 범위: 거래소가 감소 전용 주문의 포지션 확대를 실제로 막는지(T5). `Reducing` 차단은 뒤 티켓에서 생긴다(`VenueSetting`, `AccountMode` 차단의 통과는 V-MARGIN-9)

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
| `core/a_recorded_report_ends_the_last_rejection` | 이전 버전 결과 | last result: passed |
| `node-unit/venue::simproto::tests::a_margin_answer_without_its_position_list_is_not_read` | 이전 버전 결과 | last result: passed |

## V-MARGIN-3 — 증거금 정책이 있으면 일반 주문은 받은 뒤의 계좌 노출이 한도 × 계좌 가치를 넘고 받기 전보다 커질 때 AccountLeverage로 거절된다. 정확히 한도와 같으면 받는다. 계좌 가치가 0 이하면 일반 주문을 모두 거절한다. 노출은 포지션과 모든 미체결 주문(감소 전용 포함)을 지정가와 참조 가격 중 높은 값으로 센 종목별 값의 합이다. 미체결 주문은 종결 증거가 올 때까지 노출에 남고 OMS는 그 주문을 고치거나 만료시키지 않는다. 가격이 움직여 한도를 넘어도 래치는 없다. 감소 전용 주문은 검사를 건너뛰지만 합에는 들어간다. 신선한 증거금 보고가 없으면 검사는 오래된 계좌 가치로 판정하지 않고 주문은 MarginUnavailable 하나로 거절된다. 노출이 있는 종목에 신선한 참조 가격이 없으면 일반 주문은 MarginUnavailable로 거절된다

- 상태: **이전 버전 결과**
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
| `core/order_past_twice_the_account_value_is_rejected_account_leverage` | 이전 버전 결과 | last result: passed |
| `core/order_exactly_at_the_limit_is_accepted` | 이전 버전 결과 | last result: passed |
| `core/without_a_fresh_report_an_order_past_the_limit_is_rejected_margin_unavailable_only` | 이전 버전 결과 | last result: passed |
| `core/an_unfilled_resting_order_keeps_its_exposure_and_is_never_repriced_or_expired_by_the_oms` | 이전 버전 결과 | last result: passed |
| `core/resting_order_counts_at_the_higher_of_limit_and_reference` | 이전 버전 결과 | last result: passed |
| `core/reduce_only_and_plain_buy_on_a_short_count_at_full_reach` | 이전 버전 결과 | last result: passed |
| `core/exposure_is_the_sum_over_instruments` | 이전 버전 결과 | last result: passed |
| `core/partial_fill_then_cancel_frees_exposure_only_on_terminal_evidence` | 이전 버전 결과 | last result: passed |
| `core/a_venue_rejection_of_an_accepted_order_frees_its_exposure` | 이전 버전 결과 | last result: passed |
| `core/after_a_passive_breach_a_plain_reducing_order_passes_and_an_increasing_one_does_not` | 이전 버전 결과 | last result: passed |
| `core/reduce_only_order_skips_the_leverage_gate` | 이전 버전 결과 | last result: passed |
| `core/plain_order_is_rejected_when_an_exposed_instrument_has_no_fresh_reference_price` | 이전 버전 결과 | last result: passed |
| `core/account_value_zero_or_negative_rejects` | 이전 버전 결과 | last result: passed |
| `core/adding_an_order_never_lowers_exposure` | 이전 버전 결과 | last result: passed |
| `core/mirrored_books_have_equal_exposure_and_equal_decisions` | 이전 버전 결과 | last result: passed |
| `core/exposure_is_exact_at_extreme_price_and_quantity` | 이전 버전 결과 | last result: passed |

## V-MARGIN-11 — 증거금 정책이 있으면 노드는 정해진 간격으로 거래소에 증거금 보고를 묻고, 응답은 오르는 순번과 함께 코어 입력이 되며, status가 계좌 가치와 그 나이, 계좌 노출을 보여 준다. 링크가 올라오면 종목마다 거래소 설정을, 그리고 계좌 모드를 묻고, 포지션이 없는 종목의 설정은 정해진 간격으로 다시 묻는다. status는 종목별 거래소 설정과 계좌 모드, 그것이 정책과 맞는지를 보여 준다. 정책이 없으면 묻지 않는다. 조회한 계좌 가치가 기준 금액보다 5 %를 넘게 낮아지면 status가 감소 전용 상태와 그 규칙, 기준 금액을 보여 주고 일반 주문이 Hold(Reducing)으로 거절된다. 해제(clear-reduce)와 기준 재설정(rebase-day)은 전략 소켓에서 forbidden으로 거절되고 코어에 닿지 않는다. 운영자 도구(omsctl)의 clear-reduce --note와 rebase-day --note는 운영자 소켓으로 실행 중인 노드의 코어에 닿고, 응답과 status(상태, 규칙, 기준 금액)가 그 결과를 보여 준다. 감소 전용 상태에서 노드 프로세스를 강제 종료(SIGKILL)하고 다시 띄우면 status가 같은 상태(규칙, 내용, 진입 시각)와 같은 기준 금액을 보여 주고, 해제한 뒤의 강제 종료도 해제와 운영자의 기준을 그대로 남긴다. 가상 거래소에 일반 주문 300건과 감소 전용 주문 3건이 걸린 채로 감소 전용 상태에 들어가면 일반 주문은 모두 거래소에서 취소되고 감소 전용 주문 3건은 열린 채로 남으며, 그 뒤에도 증거금 조회가 이어진다. 감소 전용 상태를 결정한 기록이 쓰이기 전에 노드가 멈추면 재시작 뒤에 상태도 취소 요청도 그 기록도 없고 기준 금액은 그대로이며, 다음에 한도를 넘는 보고가 한 번 다시 결정한다. 그 기록이 파일에 닿은 뒤(쓰기 뒤, 내구화 뒤, 읽기 모델 반영 뒤)에 취소가 나가기 전에 멈추면 재시작 뒤에 상태와 취소 요청이 저널에서 복원되고, 보내지 못한 취소가 거래소로 나가며, 감소 전용 주문은 남는다. 기준 금액을 정하는 기록의 각 경계에서 멈춰도 저널의 day_base_set은 정확히 하나이고, 그 날짜와 값은 같은 기록의 보고의 것이다(쓰기 전에 멈추면 다음 보고가 정한다). 가상 거래소 프로세스의 증거금 보고는 포지션(수량, 가치, 증거금, 증거금 방식, 레버리지, 최대 레버리지)을 담고 status가 그대로 보여 주며, 포지션이 있는 종목의 거래소 설정이 바뀌면 보고로 코어에 닿아 그 종목이 막힌다

- 상태: **이전 버전 결과**
- 왜 중요한가: 코어의 규칙이 옳아도 조회가 연결되지 않으면 보고가 없어 모든 일반 주문이 거절되거나, 운영자가 왜 거절되는지 볼 수 없다
- 수준: 프로세스
- 전제와 범위: 실제 oms-node와 simvenue 프로세스. 조회 간격 100 ms(거래소 설정은 200 ms)로 줄인 설정 3회(계좌 가치 따라가기, 일 손실 진입, 거래소 설정과 계좌 모드), 정책 없는 설정 2회. 티켓 05: 운영자 명령, 강제 종료 2회, 주문 303건의 감소 전용 상태 진입. 중단 지점 시험은 failpoints 기능으로 빌드한 노드. 코드 검토 뒤(2026-11-16): simvenue의 제어 명령 margin_used로 증거금을 정해 유지증거금 여유 규칙으로 들어가는 실행 1회, 그리고 프로세스 없이 대조기 태스크의 두 포트(공개된 상태, 조회 채널)에서 조회 간격을 보는 시험 2건(간격 10초에서 2.5초 동안 설정 조회와 모드 조회가 한 번씩, 큐에 자리가 없던 증거금 조회가 다음 바퀴에 순번 1로 나간다)
- 실제 구성요소: oms-node(재조정 태스크의 타이머, 게이트웨이, 입력 큐, 저널, status), 소켓
- 모의·시뮬레이션: 거래소(simvenue). 계좌 가치, 종목의 거래소 설정, 계좌 모드는 시험이 정한다
- 주입한 장애: 거래소에서 바뀐 종목 설정(교차, 레버리지 3), 바뀐 계좌 모드(unifiedAccount), 스트림 끊김으로 생긴 새 링크 세션. SIGKILL. 감소 전용 상태 진입 기록과 기준 금액 기록의 커밋 경계(before_write, after_write, after_sync, after_reply)에서 abort
- 판정 기준과 근거: status의 core.margin(account_value, age_us, seq, fresh, account_exposure, reducing, day_base, venue_settings, account_mode), core.holds, simvenue가 센 조회 수와 simvenue의 열린 주문 목록, 운영 명령의 응답, 저널의 이벤트 수
- 재현: `cargo test -p oms-node --test e2e node_polls_margin && cargo test -p oms-node --test e2e status_shows_account_exposure && cargo test -p oms-node --test e2e status_shows_reducing && cargo test -p oms-node --test e2e node_reads_venue_setting && cargo test -p oms-node --test e2e -- strategy_socket_cannot omsctl_clear reducing_ && cargo test -p oms-node --test e2e margin_report_of_the_venue && cargo test -p oms-node --features failpoints --test crash_points -- reducing day_base`
- 뒷받침하는 것: C-MARGIN
- 확인하지 않은 범위: 기본 간격 5초(거래소 설정은 60초) 그대로의 실행. Hyperliquid 게이트웨이가 조회를 보내는 경로(응답의 번역만 V-MARGIN-10이 본다). Hyperliquid 게이트웨이의 요청 예산이 실제 거래소 한도와 맞는지(예산의 산술은 V-MARGIN-10). 디스크 유실과 전원 장애(중단은 프로세스 abort와 SIGKILL이다). omsctl 실행 파일의 main. 정확한 시간

| 시험 | 결과 | 비고 |
|---|---|---|
| `e2e/node_polls_margin_and_status_shows_value_and_age` | 이전 버전 결과 | last result: passed |
| `e2e/status_shows_account_exposure` | 이전 버전 결과 | last result: passed |
| `e2e/status_shows_reducing_its_rule_and_the_day_base` | 이전 버전 결과 | last result: passed |
| `e2e/node_reads_venue_setting_and_account_mode_and_status_shows_them` | 이전 버전 결과 | last result: passed |
| `e2e/strategy_socket_cannot_clear_reducing_or_rebase` | 이전 버전 결과 | last result: passed |
| `e2e/omsctl_clear_reduce_and_rebase_day_work_end_to_end` | 이전 버전 결과 | last result: passed |
| `e2e/reducing_and_day_base_survive_a_node_kill` | 이전 버전 결과 | last result: passed |
| `e2e/reducing_cancels_every_resting_plain_order_through_the_node` | 이전 버전 결과 | last result: passed |
| `pipeline/stopped_before_the_write_of_a_reducing_decision_the_next_report_decides_again` | 이전 버전 결과 | last result: passed |
| `pipeline/stopped_after_the_reducing_decision_is_durable_the_cancels_are_sent_after_restart` | 이전 버전 결과 | last result: passed |
| `pipeline/stopped_around_a_day_base_set_the_base_is_set_exactly_once` | 이전 버전 결과 | last result: passed |
| `e2e/margin_report_of_the_venue_process_carries_its_positions` | 이전 버전 결과 | last result: passed |
| `e2e/venue_process_states_the_margin_a_control_sets_and_the_mm_rule_latches_on_it` | 이전 버전 결과 | last result: passed |
| `node-unit/a_margin_query_the_gateway_had_no_room_for_goes_out_on_the_next_round` | 이전 버전 결과 | last result: passed |
| `node-unit/an_unanswered_setting_or_mode_query_is_repeated_at_the_setting_period_only` | 이전 버전 결과 | last result: passed |

## V-MARGIN-5 — 증거금 정책이 있으면 첫 증거금 보고가 그 계좌 가치를 조회 시각의 UTC 날짜와 함께 기준 금액으로 정한다. 같은 날의 뒤 보고는 기준을 바꾸지 않는다. 받은 보고의 계좌 가치가 기준 금액보다 5 %를 넘게 낮으면 계좌가 감소 전용 상태로 들어간다(기준 100에서 95.00은 아니고 94.99는 들어간다). 뒤 UTC 날짜의 첫 보고는 옛 기준으로 먼저 판정된 뒤에 새 기준이 되고, 여러 날의 공백 뒤에도 기준은 한 번만 넘어간다

- 상태: **이전 버전 결과**
- 왜 중요한가: 일 손실은 기준 금액에 대해 잰다. 기준이 저널 상태가 아니면 재시작이 손실을 지운다
- 수준: 단위
- 전제와 범위: 이벤트 day_base_set, reducing_entered와 상태 day_base, reducing
- 실제 구성요소: oms-core
- 모의·시뮬레이션: 보고는 시험이 입력
- 주입한 장애: 없음
- 판정 기준과 근거: C-MARGIN의 '일 손실과 기준 금액'. 기대값은 손으로 적었다
- 재현: `cargo test -p oms-core --test margin`
- 뒷받침하는 것: C-MARGIN
- 확인하지 않은 범위: 실제 거래소의 계좌 가치가 뜻하는 것(T2). 입금과 출금(운영자의 기준 재설정은 티켓 05)

| 시험 | 결과 | 비고 |
|---|---|---|
| `core/first_report_sets_the_day_base` | 이전 버전 결과 | last result: passed |
| `core/loss_greater_than_five_percent_of_the_base_enters_reducing` | 이전 버전 결과 | last result: passed |
| `core/first_report_after_midnight_is_judged_against_the_old_base_then_rolls_the_day` | 이전 버전 결과 | last result: passed |
| `core/a_report_of_an_earlier_day_does_not_change_the_day_base` | 이전 버전 결과 | last result: passed |
| `core/a_loss_between_two_reports_enters_reducing_on_the_next_report` | 이전 버전 결과 | last result: passed |
| `core/a_report_older_than_the_freshness_limit_on_arrival_is_still_judged` | 이전 버전 결과 | last result: passed |
| `core/latch_and_day_base_replay_to_the_same_state` | 이전 버전 결과 | last result: passed |

## V-MARGIN-4 — 감소 전용 상태로 들어가는 입력 안에서, 감소 전용 표시가 없고 취소 요청이 없거나 취소 시도가 다 된 모든 열린 주문에 규칙 이름으로 취소 요청이 나간다. 표시 없는 익절 주문도 취소된다. 감소 전용 표시가 있는 주문은 그대로 둔다. 감소 전용 상태에서 일반 주문은 포지션을 줄이는 방향이어도 Hold(Reducing)으로 거절되고, 감소 전용 주문은 받는다. 유지증거금 여유 규칙으로 들어갈 때도 같다(취소의 이름은 Rule(MmBuffer))

- 상태: **이전 버전 결과**
- 왜 중요한가: 손실 한도를 넘은 계좌에 걸려 있는 일반 주문이 체결되면 위험이 더 는다. 줄이는 주문까지 취소하면 포지션을 줄일 길이 없다
- 수준: 단위
- 전제와 범위: 이벤트 reducing_entered와 그 입력 안의 cancel_requested(by: Rule(DailyLoss) 또는 Rule(MmBuffer)), cancel_sent
- 실제 구성요소: oms-core
- 모의·시뮬레이션: 보고와 거래소 통보는 시험이 입력
- 주입한 장애: 취소 시도가 다 된 주문
- 판정 기준과 근거: C-MARGIN의 '감소 전용 상태'. 기대값은 손으로 적었다
- 재현: `cargo test -p oms-core --test margin`
- 뒷받침하는 것: C-MARGIN
- 확인하지 않은 범위: 취소가 체결보다 먼저 닿는다는 것. 포지션이 닫힌다는 것(OMS는 닫지 않는다). 링크가 내려가 있는 동안의 동작

| 시험 | 결과 | 비고 |
|---|---|---|
| `core/entering_reducing_cancels_plain_orders_and_keeps_reduce_only_ones` | 이전 버전 결과 | last result: passed |
| `core/under_reducing_a_plain_order_is_rejected_and_a_reduce_only_one_is_accepted` | 이전 버전 결과 | last result: passed |
| `core/an_order_pending_send_at_entry_is_cancelled_once_acknowledged` | 이전 버전 결과 | last result: passed |
| `core/a_rule_cancel_that_races_a_fill_books_the_fill` | 이전 버전 결과 | last result: passed |
| `core/a_stuck_rule_cancel_does_not_block_reduce_only_orders` | 이전 버전 결과 | last result: passed |
| `core/reducing_is_entered_once` | 이전 버전 결과 | last result: passed |
| `core/a_new_day_does_not_clear_reducing` | 이전 버전 결과 | last result: passed |
| `core/an_external_fill_raises_a_mismatch_and_a_loss_report_enters_reducing` | 이전 버전 결과 | last result: passed |
| `core/latch_and_day_base_replay_to_the_same_state` | 이전 버전 결과 | last result: passed |
| `core/a_trigger_on_one_instrument_blocks_plain_orders_on_all` | 이전 버전 결과 | last result: passed |
| `core/reducing_records_the_rule_that_tripped_first` | 이전 버전 결과 | last result: passed |
| `core/without_a_margin_policy_the_reducing_latch_refuses_no_order_and_returns_with_the_policy` | 이전 버전 결과 | last result: passed |

## V-MARGIN-9 — 증거금 정책이 있으면, 종목의 거래소 설정(증거금 방식과 레버리지)을 이번 링크 세션에서 받지 못한 동안 그 종목의 일반 주문은 VenueSetting(종목) 차단으로 거절된다. 옛 세션의 응답은 버린다. 받은 설정의 증거금 방식이나 레버리지가 정책과 다르면 그 종목만 같은 차단에 걸리고 다른 종목은 걸리지 않는다. 맞는 설정을 받으면 바로 풀리고, 링크가 바뀌면 다시 모르는 것이 된다. 증거금 보고의 포지션에 적힌 방식과 레버리지도 설정의 출처다. 차단 조회는 걸린 종목마다 그 종목 이름과 함께 차단을 보여 준다. 계좌 모드가 unifiedAccount나 portfolioMargin이면 허용 목록에 적혀 있어도 계좌의 모든 종목의 일반 주문이 AccountMode 차단으로 거절된다. 허용 목록에 없는 모드, 빈 응답, 이번 링크 세션에서 받지 못한 모드도 같다. 기본 허용 목록은 비어 있어서 운영자가 모드를 적기 전에는 일반 주문이 나가지 않는다. 감소 전용 주문은 두 차단을 모두 통과한다. OMS는 설정을 읽기만 하고 거래소로 아무것도 보내지 않는다

- 상태: **이전 버전 결과**
- 왜 중요한가: 거래소의 레버리지나 증거금 방식이 계획과 다르면 주문이 계획하지 않은 증거금을 쓴다. 확인하지 못한 설정을 맞다고 가정하면 그 차이를 아무도 모른다
- 수준: 단위
- 전제와 범위: 코어의 차단 계산과 신규 주문 검사. 정책 기본값(격리, 레버리지 2), 종목 하나 또는 둘
- 실제 구성요소: oms-core
- 모의·시뮬레이션: 거래소 설정 응답과 증거금 보고는 시험이 입력
- 주입한 장애: 설정을 받지 못한 링크 세션, 옛 세션의 늦은 응답, 교차 증거금, 레버리지 3과 1, 허용 목록에 잘못 적힌 통합 계좌와 포트폴리오 증거금
- 판정 기준과 근거: C-MARGIN의 '거래소 설정과 계좌 모드'. 기대값은 손으로 적었다. 변이 M43~M50
- 재현: `cargo test -p oms-core --test margin`
- 뒷받침하는 것: C-MARGIN
- 확인하지 않은 범위: 어느 문자열이 표준 모드인지(T10): 시험의 standard는 임의의 이름이다. 포지션이 없을 때 거래소가 설정을 알려 주는지(T11). 거래소가 걸려 있는 주문에 레버리지를 적용하는지(T8)

| 시험 | 결과 | 비고 |
|---|---|---|
| `core/instrument_with_unknown_venue_setting_holds_plain_orders` | 이전 버전 결과 | last result: passed |
| `core/a_setting_that_differs_from_config_holds_only_that_instrument` | 이전 버전 결과 | last result: passed |
| `core/a_matching_setting_releases_the_hold_and_link_loss_makes_it_unknown_again` | 이전 버전 결과 | last result: passed |
| `core/a_cross_position_in_a_margin_report_raises_the_hold` | 이전 버전 결과 | last result: passed |
| `core/venue_setting_hold_names_its_instrument_in_holds` | 이전 버전 결과 | last result: passed |
| `core/unified_or_portfolio_account_mode_holds_the_account` | 이전 버전 결과 | last result: passed |
| `core/an_account_mode_off_the_allow_list_or_unknown_holds_the_account` | 이전 버전 결과 | last result: passed |
| `core/reduce_only_order_passes_the_venue_setting_and_account_mode_holds` | 이전 버전 결과 | last result: passed |
| `core/a_report_position_on_an_unconfigured_instrument_leaves_no_venue_setting` | 이전 버전 결과 | last result: passed |

## V-MARGIN-6 — 운영자의 해제(clear-reduce)는 계좌가 감소 전용 상태가 아니면 NotReducing으로, 증거금 보고가 30초보다 오래됐으면 MarginUnavailable로, 사유가 비었으면 NoteRequired로 거절되고, 거절은 아무것도 기록하지 않는다. 받아들인 해제는 한 입력 안에서 마지막 보고의 계좌 가치를 새 기준 금액으로 정하고(day_base_set, set_by: operator) 감소 전용 상태를 끝낸다(reducing_cleared). 정확히 30초 된 보고는 신선하다. 그 뒤 새 기준 94에서 94와 89.30은 걸리지 않고 89.29는 다시 걸린다. 두 번째 해제는 NotReducing이다. 기준 재설정(rebase-day)은 신선한 보고와 사유가 있을 때 마지막 보고의 계좌 가치와 그 날짜(기준보다 앞선 날짜라도)를 기준 금액으로 정하고, 감소 전용 상태는 건드리지 않는다. 기준 재설정 없이 출금하면 그만큼이 손실로 읽힌다(100에서 6을 출금하면 걸린다). 마지막 보고에서 유지증거금 여유 조건이 참이면(어느 규칙으로 들어왔든) 해제는 MmBufferActive로 거절되고, 이 조건은 사유보다 먼저 본다. 포지션이 선 위로 돌아온 보고(가치 200, 최대 레버리지 40에서 3.00) 뒤에는 해제된다

- 상태: **이전 버전 결과**
- 왜 중요한가: 해제는 위험을 다시 여는 유일한 길이다. 오래된 숫자로, 또는 사유 없이 풀리면 손실 한도가 뜻을 잃는다
- 수준: 단위
- 전제와 범위: 운영자 입력 ClearReduce와 RebaseDay, 그 거절 사유, 이벤트 day_base_set(운영자)과 reducing_cleared
- 실제 구성요소: oms-core
- 모의·시뮬레이션: 보고는 시험이 입력
- 주입한 장애: 없음
- 판정 기준과 근거: C-MARGIN의 '해제와 기준 재설정'. 기대값은 손으로 적었다
- 재현: `cargo test -p oms-core --test margin`
- 뒷받침하는 것: C-MARGIN
- 확인하지 않은 범위: 운영자의 판단이 옳은 것. 입금이 가린 손실(100, 입금 10, 손실 14는 걸리지 않는다: 시험이 그대로 보여 준다). 잘못 걸린 유지증거금 여유 래치에서 나가는 길(없다: 조건이 참으로 읽히는 동안에는 해제할 수 없고 강제 옵션이 없다)

| 시험 | 결과 | 비고 |
|---|---|---|
| `core/clear_reduce_is_refused_when_stale_or_without_a_note_or_when_not_reducing` | 이전 버전 결과 | last result: passed |
| `core/clear_reduce_rebases_the_day_and_a_further_loss_latches_again` | 이전 버전 결과 | last result: passed |
| `core/rebase_day_sets_the_base_and_leaves_the_latch_alone` | 이전 버전 결과 | last result: passed |
| `core/a_withdrawal_without_rebase_reads_as_a_loss` | 이전 버전 결과 | last result: passed |
| `core/clear_reduce_is_refused_while_the_mm_condition_holds` | 이전 버전 결과 | last result: passed |

## V-MARGIN-7 — 증거금 정책이 있고 링크가 올라와 있을 때, 마지막으로 받은 증거금 보고의 조회 시각에서 60초가 넘게 지나면 틱이 감소 전용 표시가 없고 취소 요청이 없는 모든 열린 주문에 Rule(MarginStale) 이름으로 취소 요청을 한 번 낸다. 정확히 60초에는 내지 않는다. 감소 전용 표시가 있는 주문은 그대로 둔다. 아무 상태도 걸리지 않는다

- 상태: **이전 버전 결과**
- 왜 중요한가: 증거금 보고를 못 받는 동안에는 일 손실도 유지증거금 여유도 판정하지 못한다. 그동안 걸려 있는 일반 주문이 체결되면 보지 못하는 채로 위험이 는다
- 수준: 단위
- 전제와 범위: 틱 안의 cancel_requested(by: Rule(MarginStale)), cancel_sent
- 실제 구성요소: oms-core
- 모의·시뮬레이션: 보고와 거래소 통보, 시각은 시험이 입력
- 주입한 장애: 보고가 끊긴 링크
- 판정 기준과 근거: C-MARGIN의 '보고 끊김 취소'. 기대값은 손으로 적었다
- 재현: `cargo test -p oms-core --test margin`
- 뒷받침하는 것: C-MARGIN
- 확인하지 않은 범위: 포지션(그대로 남는다). 감소 전용 주문. 링크가 내려가 있는 동안(아무것도 취소하지 않는다). 취소가 체결보다 먼저 닿는다는 것

| 시험 | 결과 | 비고 |
|---|---|---|
| `core/report_stale_past_the_cancel_limit_cancels_each_plain_order_once` | 이전 버전 결과 | last result: passed |
| `core/stale_age_starts_at_link_up_when_no_report_has_arrived` | 이전 버전 결과 | last result: passed |
| `core/stale_cancel_is_silent_while_the_link_is_down_or_the_policy_is_off` | 이전 버전 결과 | last result: passed |
| `core/stale_cancel_does_not_reissue_an_exhausted_cancel` | 이전 버전 결과 | last result: passed |
| `core/a_fresh_report_after_a_stale_episode_restores_acceptance_without_a_latch` | 이전 버전 결과 | last result: passed |
| `core/a_forward_clock_jump_reads_the_report_as_stale_and_cancels` | 이전 버전 결과 | last result: passed |
| `core/stale_age_starts_at_policy_activation_when_the_link_is_already_up` | 이전 버전 결과 | last result: passed |

## V-MARGIN-8 — 증거금 정책이 있으면, 받은 증거금 보고의 포지션 가운데 포지션 가치가 0보다 큰 것의 격리 증거금이 (유지증거금률 + 0.25 %p) × 포지션 가치보다 작으면 계좌가 감소 전용 상태로 들어가고 규칙은 MmBuffer로 남는다. 최대 레버리지 40, 포지션 가치 200에서 3.00은 걸리지 않고 2.99는 걸린다. 유지증거금률은 그 포지션의 최대 레버리지(같은 보고의 값)로 1 / (2 × 최대 레버리지)이다: 최대 레버리지 25, 가치 200에서 4.50은 걸리지 않고 4.49는 걸리며, 최대 레버리지 50에서는 2.50이 걸리지 않는다. 한 종목의 포지션이 걸려도 래치는 계좌 전체에 하나다: 다른 종목에 걸려 있는 일반 주문이 Rule(MmBuffer) 이름으로 취소되고, 모든 종목의 새 일반 주문이 Hold(Reducing)으로 거절되며, 감소 전용 주문은 받는다. 교차 증거금 포지션은 판정하지 않는다(그 종목은 VenueSetting 차단에 걸린다). 래치에는 먼저 건 규칙이 남는다: 일 손실로 들어간 뒤의 유지증거금 여유 판정도, 그 반대도 아무것도 내지 않고 바꾸지 않는다. 한 보고가 둘 다 걸면 유지증거금 여유를 먼저 판정하므로 MmBuffer가 남는다. 받은 보고는 도착했을 때 이미 30초보다 오래됐어도 판정한다(조회 뒤 40초에 기록된 보고의 2.99도 건다). 비교는 나눗셈 없는 정확한 곱으로 한다: 가장 큰 금액·최대 레버리지·여유에서도 넘침이 결과를 뒤집지 않고(곱이 i128에 들어가지 않으면 선 아래로 본다), 선 바로 옆에서 반올림이 결과를 뒤집지 않는다. 포지션 가치가 0 이하인 포지션은 판정하지 않는다

- 상태: **이전 버전 결과**
- 왜 중요한가: 격리 포지션이 청산에 가까운 계좌에 일반 주문을 더 받으면 위험이 는다. 판정은 거래소가 준 보고만 읽으므로 OMS 장부의 오류가 이 규칙을 끄지 못한다
- 수준: 단위
- 전제와 범위: 이벤트 reducing_entered(rule: mm_buffer)와 상태 reducing
- 실제 구성요소: oms-core
- 모의·시뮬레이션: 보고는 시험이 입력
- 주입한 장애: 없음
- 판정 기준과 근거: C-MARGIN의 '유지증거금 여유'. 기대값은 손으로 적었다
- 재현: `cargo test -p oms-core --test margin`
- 뒷받침하는 것: C-MARGIN
- 확인하지 않은 범위: 거래소의 격리 포지션 marginUsed가 그 포지션의 자산이라는 것(T1. 틀리면 규칙이 양쪽으로 틀릴 수 있다). 증거금 구간(T9: 유지증거금률은 보고의 최대 레버리지 하나로만 계산한다). 규칙은 청산 가격의 0.13~0.37 % 앞에서 걸리고(BTC, 거래소 2배) 5초 간격의 조회가 그 사이를 건너뛸 수 있다. 새 보고를 받지 못하는 동안에는 판정하지 않는다

| 시험 | 결과 | 비고 |
|---|---|---|
| `core/isolated_margin_below_mmr_plus_buffer_enters_reducing` | 이전 버전 결과 | last result: passed |
| `core/mmr_follows_the_max_leverage_of_each_position` | 이전 버전 결과 | last result: passed |
| `core/a_trigger_on_one_instrument_blocks_plain_orders_on_all` | 이전 버전 결과 | last result: passed |
| `core/a_cross_position_is_not_judged_by_the_mm_rule` | 이전 버전 결과 | last result: passed |
| `core/reducing_records_the_rule_that_tripped_first` | 이전 버전 결과 | last result: passed |
| `core/an_old_report_on_arrival_is_still_judged_by_the_mm_rule` | 이전 버전 결과 | last result: passed |
| `core/mm_comparison_is_exact_at_extreme_values` | 이전 버전 결과 | last result: passed |

## V-MARGIN-13 — 증거금 정책에 account_mm_rate_max_bps가 있으면, 받은 증거금 보고의 교차 계좌 유지증거금이 0보다 크고 유지증거금 × 10 000 ≥ 한도 × 증거금 잔고일 때 계좌가 감소 전용 상태로 들어가고 규칙은 AccountMm(status의 account_mm)으로 남으며, 같은 입력에서 열린 일반 주문마다 Rule(AccountMm) 취소를 낸다(감소 전용 주문은 남는다). 한도 5000에서 잔고 1000: 499.99는 걸리지 않고 500은 건다. 한도 1에서 잔고 100: 0.00999999는 아니고 0.01은 건다. 잔고 0 이하에 유지증거금이 있으면 어느 한도에서도 건다. 유지증거금 0은 잔고가 얼마든(0, 음수 포함) 걸지 않는다. 가장 큰 금액에서도 나눗셈 없이 정확히 비교한다. 한도가 있는데 account_mm이 없거나 유지증거금이 음수인 보고는 통째로 MarginReportRejected이고 마지막 정상 보고가 나이를 먹는다. 한도가 없으면 같은 보고를 기록하고 숫자를 판정하지 않는다. 래치에는 먼저 건 규칙이 남는다(일 손실 뒤의 계좌 규칙도, 그 반대도 아무것도 내지 않는다). 한 보고가 여럿을 걸면 유지증거금 여유, 계좌 유지증거금률, 일 손실 순이다. clear-reduce는 마지막 보고에서 조건이 참이면 어느 규칙이 걸었든 account_mm_active로 거절된다. status는 마지막 보고의 두 숫자를 보이고 없으면 키가 없다. account_mm 없는 보고와 한도 없는 정책은 저널에 바이트를 남기지 않고(기록 스키마 2 그대로), 이 필드 전의 기록은 없음으로 읽히며, 두 종류의 저널 모두 재생·재결정·디스크 복구(verify --re-decide)에서 실시간 상태와 같다. 한도는 1..=9999다. 노드 설정은 Bybit에서 증거금 정책이 있으면 한도를 요구하고 Hyperliquid와 가상 거래소에서는 거부한다. Bybit 게이트웨이는 totalMaintenanceMargin과 totalMarginBalance를 보고에 싣고, 둘 중 하나라도 없거나 문자열 소수가 아니면 응답 전체가 MarginReportInvalid이며, accountMMRate는 무슨 값이든(없어도) 보고를 바꾸지 않는다

- 상태: **미실행**
- 왜 중요한가: 교차 계좌에는 격리 포지션이 없어 유지증거금 여유 규칙이 걸리지 않는다. 계좌가 청산에 가까울 때 일반 주문을 더 받지 않고 걸려 있는 일반 주문을 거둔다는 전제(U17d, 소유자 질문의 기본값 50 %)
- 수준: 단위
- 전제와 범위: oms-core 시험 11개, 디스크 저널 복구 시험 1개, loopback 가짜 HTTP 서버(http-fake)의 wallet-balance 응답 시험 3개, 설정 시험 1개
- 실제 구성요소: oms-core 판단·적용·재생·설정 검증, oms-journal 쓰기와 oms-node recovery::recover·verify, venue::bybit::query, oms-node 설정 검증
- 모의·시뮬레이션: 증거금 보고(시험이 입력), Bybit REST 응답(문서의 필드 이름으로 손으로 쓴 JSON)
- 주입한 장애: 숫자 없는 보고, 음수 유지증거금, 잔고 0·음수, 가장 큰 금액, 필드 없음·빈 문자열·JSON 숫자·숫자 아닌 문자열, accountMMRate의 여러 값과 없음, 한도 0·10000
- 판정 기준과 근거: C-MARGIN의 '계좌 유지증거금률'. 기대값은 손으로 적었다. 변이 20가지를 넣어 모두 잡았다(ADR-028 §9)
- 재현: `cargo test -p oms-core --test account_mm; cargo test -p oms-node --test record_schema; cargo test -p oms-node --lib -- venue::bybit::query config::`
- 뒷받침하는 것: C-MARGIN, ADR-018 §7의 11, ADR-028 §7g
- 확인하지 않은 범위: 실제 Bybit 응답의 두 필드와 accountMMRate의 형식(testnet은 U-01 허가 뒤). totalMaintenanceMargin이 거래소의 청산 판정과 같은 숫자라는 것. 5000 bps가 적당하다는 것(판단). 5초 간격의 조회가 건너뛰는 급한 가격 변화, 새 보고를 받지 못하는 동안의 판정. 노드 프로세스를 거친 진입과 omsctl의 account_mm_active 응답(Bybit 노드 시험은 증거금 정책 없이 돈다). 잘못 걸린 래치는 조건이 참인 동안 해제로 풀 수 없다

| 시험 | 결과 | 비고 |
|---|---|---|
| `core/rate_just_below_the_limit_does_not_latch_and_at_the_limit_latches_with_the_cancels` | 미실행 |  |
| `core/the_limit_is_judged_exactly_at_one_basis_point` | 미실행 |  |
| `core/a_margin_balance_of_zero_or_below_with_maintenance_latches` | 미실행 |  |
| `core/no_maintenance_margin_latches_nothing_whatever_the_balance` | 미실행 |  |
| `core/the_comparison_is_exact_at_the_largest_amounts` | 미실행 |  |
| `core/a_report_without_account_mm_is_rejected_under_the_limit_and_recorded_without_it` | 미실행 |  |
| `core/the_latch_is_not_replaced_by_a_later_rule` | 미실행 |  |
| `core/clear_reduce_is_refused_while_the_account_rate_is_at_or_above_the_limit` | 미실행 |  |
| `core/status_shows_the_account_numbers_and_the_rule` | 미실행 |  |
| `core/journals_with_and_without_account_mm_replay_to_the_live_state` | 미실행 |  |
| `core/the_limit_is_one_to_9999_bps` | 미실행 |  |
| `node-unit/margin_reports_with_and_without_account_mm_recover_to_the_live_state` | 미실행 |  |
| `node-unit/venue::bybit::query::tests::the_margin_report_maps_the_unified_account` | 미실행 |  |
| `node-unit/venue::bybit::query::tests::a_margin_answer_that_cannot_be_read_as_a_whole_is_refused_as_a_whole` | 미실행 |  |
| `node-unit/venue::bybit::query::tests::the_account_mm_rate_field_is_not_used_for_the_judgment` | 미실행 |  |
| `node-unit/config::tests::the_account_mm_limit_is_required_on_bybit_and_refused_elsewhere` | 미실행 |  |

## V-MARGIN-12 — 시뮬레이션의 가상 거래소는 계좌 가치를 자기 모형으로 계산한다: 현금 + 포지션 × 기준 가격. 체결은 낸 값과 수수료만큼, 펀딩은 그 금액만큼 현금을 줄이고, 가격 변화는 포지션 가치를 바꾼다. 손으로 푼 예(입금 100, 100에 0.5 매수와 수수료 0.01, 가격 104, 펀딩 0.1 %)에서 계좌 가치는 99.99, 101.99, 101.938이다. 가격이 움직이지 않아도 수수료와 펀딩만으로 계좌가 기준 금액보다 5 %를 넘게 내려가면 감소 전용 상태로 들어간다(100에서 95.00은 아니고 94.99는 들어간다). 가상 거래소는 포지션을 키우거나 반대쪽으로 넘기는 감소 전용 주문을 거절하고, 걸려 있는 감소 전용 주문은 남은 포지션만큼만 체결한 뒤 나머지를 취소한다. 증거금 정책을 켠 임의 순서의 실행(체결, 가격 변화, 펀딩, 운영자 해제, 그리고 margin_lossy에서는 보고 유실·지연·무응답, 링크 단절, 재시작)의 매 입력 뒤에: 받아들인 일반 주문은 신선한 보고가 있었고 노출이 계좌 가치의 2배 이내이거나 노출을 올리지 않았다. 감소 전용 상태에 들어가면 열려 있는 일반 주문마다 취소 요청이 있고, 그 상태인 동안 일반 주문은 받아들여지지 않는다. 감소 전용 상태는 증거금 보고로만 들어가고 운영자의 해제로만 끝난다(재시작, 새 UTC 날, 회복된 계좌 가치는 끝내지 않는다). 보고가 한도보다 오래 끊긴 뒤의 틱이 끝나면 열려 있는 일반 주문마다 취소 요청이 있다. 증거금 정책이 없는 기존 프로파일 일곱 개는 seed 다섯 개씩 main(84db0f9)에서 기록한 것과 같은 저널을 쓴다. 가상 시간 50시간짜리 실행(margin_days: UTC 자정 1시간 전에 시작해 자정을 세 번 넘고, 그 사이에 재시작, 링크 단절, 손실, 출금과 입금, 운영자의 해제와 기준 재설정이 섞인다)에서도 위 검사가 매 입력 뒤에 성립하고, 기록된 보고가 그 전의 기준 금액보다 5 %를 넘게 낮으면 그 보고가 새 날의 기준을 정하는 보고여도 계좌는 감소 전용 상태가 되고, 저널의 기준 금액은 실행마다 자정을 정확히 세 번 넘고, 재시작과 새 날은 감소 전용 상태를 끝내지 않는다. 정해진 순서 하나(자정을 넘는 20초의 조회 끊김 동안 100에서 6 출금, 재시작, 자정 두 번 더)에서 새 날의 첫 보고 94는 상태를 걸고 나서 기준이 되고, 저널에는 날마다 기준이 하나다(100, 94, 100, 99). 저널의 증거금 기록(입력이 증거금 보고인 기록)은 조회 한 번에 하나를 넘지 않는다: 조용한 하루는 조회 17 280번에 기록 17 280건이고, 50시간 실행에서 기록 수는 조회 수 이하이고 같은 조회의 답은 두 번 기록되지 않는다

- 상태: **이전 버전 결과**
- 왜 중요한가: 일 손실 규칙은 거래소가 말한 계좌 가치만 읽는다. 시뮬레이션이 그 값을 체결·가격·수수료·펀딩에서 스스로 만들어야 규칙을 임의 순서의 실행에서 볼 수 있다. 예제 시험이 생각하지 못한 순서(취소 중의 진입, 재시작 직후의 보고, 앞지른 늦은 보고)는 여기서만 나온다
- 수준: 시뮬레이션
- 전제와 범위: 가상 거래소의 증거금 모형(단위 3건). 프로파일 margin_calm, margin_lossy: 실행당 1500단계, 한 종목, 두 전략, 계좌 100, 증거금 시계는 계약 기본값의 50분의 1(조회 100 ms, 신선도 600 ms, 끊김 취소 1.2초), UTC 자정을 한 번 넘는다. 프로파일 margin_days: 가상 시간 50시간, 증거금 시계는 계약 기본값(조회 5초, 신선도 30초, 끊김 취소 60초), UTC 자정을 세 번 넘는다. 활동은 묶음으로 오고 그 사이 5초~10분은 타이머만 돈다. seed는 표준 실행의 10분의 1(최소 2개). 기존 프로파일 7개 × seed 5개의 저널 해시
- 실제 구성요소: oms-core의 판단과 상태
- 모의·시뮬레이션: 거래소와 그 증거금 모형(우리 모형이고 거래소의 문서화된 산술이 아니다), 저널과 커밋 게이트, 게이트웨이, 네트워크, 전략, 운영자, 시계
- 주입한 장애: margin_calm: 없음. margin_lossy: lossy의 네트워크 장애, 링크 단절 0.3 %/단계, 재시작 0.2 %/단계, 증거금 조회 무응답 0.5~2.5초, 늦은 응답, 포지션 증거금을 0으로 말하는 거래소. margin_days: margin_lossy의 장애(링크 단절 0.2 %/단계, 재시작 0.1 %/단계, 실행당 약 15 000단계), 증거금 조회 무응답 1분~1시간과 자정 둘 중 하나를 넘는 무응답, 두 번 도착하는 증거금 응답 5 %, 출금과 입금, 운영자의 기준 재설정
- 판정 기준과 근거: 모형은 손으로 푼 예의 값. 실행은 검사 leverage-gate, margin-fresh, reducing-cancel, reducing-accept, reducing-entry, reducing-release, stale-cancel, daily-loss(노출과 손실은 세계가 코어의 장부와 보고에서 따로 계산한다)와 V-SIM-1, V-SIM-2의 검사. 프로파일이 각 규칙에 닿았는지는 seed 40개부터 확인한다(margin_days는 흔한 것은 4개부터, 자정에 관한 것은 40개부터). 저널 해시는 main에서 기록한 상수와 비교한다. 여러 날 실행의 기대값(기준 금액, 17 280)은 손으로 적었다. 변이 M89~M100
- 재현: `cargo test -p oms-sim --test sim -- world_ a_loss_made profile_margin margin_journal existing_profiles --nocapture   (실패 시: OMS_SIM_PROFILE=<이름> OMS_SIM_SEED=<n>)`
- 뒷받침하는 것: C-MARGIN
- 확인하지 않은 범위: 실제 거래소에 관한 것 전부: 계좌 가치와 증거금의 계산(T1, T2), 수수료와 펀딩이 어느 잔고에서 나가는지(T3), 감소 전용 주문의 처리(T5, 문서 인용을 가정으로 적었다). 취소가 체결보다 먼저 닿는 것. 주문을 받은 뒤 가격이 움직여 넘은 노출. 청산(모형에 없다). 증거금 구간. 여러 종목. 노드 프로세스를 실제로 며칠 돌리는 것(여러 날 실행은 가상 시간이다). 저널의 디스크 바이트(건수만 센다. 저널은 지워지지 않는다, U-04)와 증거금 기록이 아닌 기록의 수. margin_days의 seed는 표준 실행의 10분의 1이다

| 시험 | 결과 | 비고 |
|---|---|---|
| `sim/world_account_value_follows_fills_price_fees_and_funding` | 이전 버전 결과 | last result: passed |
| `sim/a_loss_made_only_of_fees_and_funding_enters_reducing` | 이전 버전 결과 | last result: passed |
| `sim/world_refuses_a_reduce_only_order_that_would_grow_the_position` | 이전 버전 결과 | last result: passed |
| `sim/profile_margin_calm` | 이전 버전 결과 | last result: passed |
| `sim/profile_margin_calm_reducing_invariant` | 이전 버전 결과 | last result: passed |
| `sim/profile_margin_calm_release_invariant` | 이전 버전 결과 | last result: passed |
| `sim/profile_margin_lossy` | 이전 버전 결과 | last result: passed |
| `sim/existing_profiles_give_the_same_run_with_margin_off` | 이전 버전 결과 | last result: passed |
| `sim/profile_margin_days_crosses_three_utc_days_with_restarts` | 이전 버전 결과 | last result: passed |
| `sim/margin_journal_records_stay_within_one_per_poll` | 이전 버전 결과 | last result: passed |

## V-MARGIN-10 — Hyperliquid 게이트웨이는 문서의 clearinghouseState 응답 예를 증거금 보고 입력으로 옮긴다: 계좌 가치는 marginSummary.accountValue, 포지션마다 종목, 부호 있는 수량, 포지션 가치, 사용 증거금, 증거금 방식, 레버리지, 최대 레버리지. 어느 포지션이든 maxLeverage가 없거나 0이거나 정수가 아니면 보고 전체를 사유와 함께 거부 입력(MarginReportInvalid)으로 바꾸고, 멀쩡한 다른 포지션도 보고하지 않는다. 계좌 가치, 수량, 포지션 가치, 사용 증거금, 레버리지 가운데 하나라도 없거나 읽을 수 없을 때, 증거금 방식이 isolated도 cross도 아닐 때, 포지션 목록이 없을 때도 같다. 어떤 값도 0이나 기본값으로 채우지 않고 어떤 포지션도 건너뛰지 않는다. 설정에 없는 종목의 포지션도 보고에 남는다(번역 함수는 설정된 종목을 받지 않는다). activeAssetData 응답 예는 그 종목의 거래소 설정 입력(증거금 방식, 레버리지)이 된다. 방식이 isolated도 cross도 아니거나, 레버리지가 정수가 아니거나, 응답이 다른 종목의 것이면 입력을 만들지 않는다(설정은 모름으로 남아 그 종목이 막힌다). userAbstraction 응답(문자열 하나)은 글자 그대로 계좌 모드 입력이 된다. 문서에 없는 문자열을 문서에 있는 값으로 바꾸지 않고, 문자열이 아니거나 64자를 넘으면 입력을 만들지 않는다. 세 조회의 요청 본문은 문서의 것과 같다(clearinghouseState, activeAssetData, userAbstraction). 게이트웨이의 요청 예산은 취소를 막지 않으므로 취소 500건 뒤에는 조회가 기다린다: 가득 찬 예산(1200)에서는 700이 남아 증거금 조회가 바로 나가고, 시작 직후(600)에는 100이 남아 10.1초, 빈 예산에서는 15.1초를 기다린다

- 상태: **이전 버전 결과**
- 왜 중요한가: 증거금 규칙이 읽는 숫자는 모두 이 번역을 거친다. 필드를 하나 잘못 옮기면 규칙이 틀린 값으로 판정한다
- 수준: 단위
- 전제와 범위: 게이트웨이의 순수 번역 함수(JSON 응답 → 코어 입력)와 요청 본문 함수. 문서의 응답 예가 입력. 요청 예산은 시각을 시험이 주는 단위 시험
- 실제 구성요소: oms-node(venue/hyperliquid/wire.rs, venue/hyperliquid/mod.rs의 Budget)
- 모의·시뮬레이션: 응답은 시험이 입력. 거래소에 묻지 않는다
- 주입한 장애: maxLeverage 없음, 0, 문자열, null, 음수, 소수, u32 초과, 배열. 소수 필드 없음, 숫자가 아닌 문자열, 빈 문자열, JSON 숫자, null, 지수 표기, NaN, 소수 9자리. 모르는 증거금 방식. 종목 이름 없음. 포지션 목록 없음
- 판정 기준과 근거: C-MARGIN의 'Hyperliquid 조회'. 기대값은 문서 예의 소수를 손으로 1e-8 단위 정수로 적었다. 변이 M67~M76, M79
- 재현: `cargo test -p oms-node --lib venue::hyperliquid`
- 뒷받침하는 것: C-MARGIN
- 확인하지 않은 범위: 실제 응답이 문서의 예와 같은지, 필드의 뜻이 유도한 것과 같은지(T1, T2). 어느 userAbstraction 값이 표준 모드인지(T10), 포지션이 없을 때 activeAssetData가 레버리지를 주는지(T11). activeAssetData의 응답 예는 저장소의 조사 문서에 없고 문서와 다시 대조하지 않았다. 게이트웨이가 조회를 실제로 보내는 경로(HTTP)는 시험하지 않는다. HTTP 429일 때의 동작. 거래소가 실제로 세는 weight. 요청 예산을 여러 조회가 다투는 순서와 조회 간격만큼 더 길어지는 대기

| 시험 | 결과 | 비고 |
|---|---|---|
| `node-unit/venue::hyperliquid::wire::tests::documented_clearinghouse_state_parses_to_a_margin_report` | 이전 버전 결과 | last result: passed |
| `node-unit/venue::hyperliquid::wire::tests::missing_zero_or_unparseable_max_leverage_rejects_the_whole_report` | 이전 버전 결과 | last result: passed |
| `node-unit/venue::hyperliquid::wire::tests::any_unparseable_number_rejects_the_whole_report` | 이전 버전 결과 | last result: passed |
| `node-unit/venue::hyperliquid::wire::tests::a_position_on_an_unconfigured_coin_is_kept` | 이전 버전 결과 | last result: passed |
| `node-unit/venue::hyperliquid::wire::tests::documented_active_asset_data_parses_leverage_and_mode` | 이전 버전 결과 | last result: passed |
| `node-unit/venue::hyperliquid::wire::tests::user_abstraction_values_parse_and_an_unknown_value_stays_unknown` | 이전 버전 결과 | last result: passed |
| `node-unit/venue::hyperliquid::wire::tests::margin_and_setting_request_json` | 이전 버전 결과 | last result: passed |
| `node-unit/venue::hyperliquid::tests::a_full_cancel_burst_leaves_the_margin_query_waiting_for_a_known_time` | 이전 버전 결과 | last result: passed |

## V-WIRE-ID-1 — 고객 주문번호 형식 Base32Pair는 VenueClientId와 일대일이고 27자(36자 이하, 0-9a-v와 -)이며 같은 입력에 같은 문자열을 내고, 서로 다른 100만 개 주문에 서로 다른 100만 개 문자열을 낸다. 해석은 그 형식이 쓴 문자열만 받는다(대문자·다른 길이·다른 구분자·64비트 초과·16진수 형식은 None). Hex128은 to_hex와 같은 문자열을 내고 소문자 16진수만 해석하며, 가상 거래소와 Hyperliquid 게이트웨이는 Hex128을 선언하고 보내는 번호를 그 형식으로 만든다

- 상태: **미실행**
- 왜 중요한가: Bybit orderLinkId로 낸 주문을 우리 것으로 알아보고 남의 주문을 우리 것으로 잘못 읽지 않으며, 기존 두 거래소의 요청은 바뀌지 않는다는 전제
- 수준: 단위
- 전제와 범위: 경계값 36쌍, 100만 개 주문(인스턴스 2개 × 50만), 잘못된 문자열 Base32Pair 11개·Hex128 4개
- 실제 구성요소: WireIdForm::encode, WireIdForm::decode, 두 게이트웨이의 WIRE_ID 상수
- 모의·시뮬레이션: 없음
- 주입한 장애: 없음
- 판정 기준과 근거: 왕복 일치, 길이 27, 문자 집합, 중복 0, 잘못된 문자열 전부 None, 두 상수가 Hex128
- 재현: `cargo test -p oms-core --lib ids::tests && cargo test -p oms-node --lib venue::tests`
- 뒷받침하는 것: C-GW
- 확인하지 않은 범위: Bybit가 이 형식을 실제로 받는지, orderLinkId 재사용·유일성 범위(설계 TN의 testnet 항목 T1, ADR-018의 T1과 다름). Bybit 게이트웨이는 아직 없다. 상태 API의 venue_client_id가 Base32Pair로 출력되는 경로는 Bybit 게이트웨이가 생길 때 시험한다(지금은 두 게이트웨이 모두 Hex128이라 출력이 전과 같다). 게이트웨이가 거래소 응답을 읽을 때 쓰는 parse_hex는 대문자도 받는다(바꾸지 않음)

| 시험 | 결과 | 비고 |
|---|---|---|
| `core/ids::tests::hex128_is_the_hex_form_of_today` | 미실행 |  |
| `core/ids::tests::base32_pair_roundtrips_and_fits_an_order_link_id` | 미실행 |  |
| `core/ids::tests::base32_pair_gives_a_million_distinct_ids` | 미실행 |  |
| `core/ids::tests::base32_pair_decoding_is_strict` | 미실행 |  |
| `node-unit/venue::tests::the_gateways_of_today_write_the_hex_form` | 미실행 |  |

## V-VENUE-EXEC-1 — 펀딩 입력은 포지션·주문·예약·체결 수를 바꾸지 않고 한 번만 기록된다. 거래소 포지션 변경은 부호 있는 수량을 계좌와 미귀속 장부에 더하고 VenuePositionChange 불일치로 일반 주문을 막으며(감소 전용은 통과) 재생 뒤에도 남는다. 분류 못 한 실행은 기장 없이 UnclassifiedExecution 불일치로 막는다. 변경 뒤 그보다 늦게 물은 현재 세션의 포지션 응답 중 그 종목에 열린 주문과 대기 체결이 없을 때의 첫 응답이 다르면 PositionAdjusted(snapshot_after_venue_change) 한 건으로 맞추고 같으면 VenueChangeSettled만 남기며(열린 주문이 있는 동안은 맞추지 않아 오는 중인 체결을 두 번 세지 않는다), 저널만으로 계산한 포지션이 거래소 값과 같다. 분류 못 한 실행의 키는 같은 키로 나중에 온 체결의 기장을 막지 않는다. 운영자 조정은 reason 없이 스키마 1과 같은 바이트로 쓰인다

- 상태: **미실행**
- 왜 중요한가: Bybit의 펀딩·ADL·청산 인수가 포지션이나 체결을 틀어지게 하지 않고, 사람이 볼 때까지 노출을 늘리지 않으며, 감시자 W2가 저널 합만으로 포지션을 비교할 수 있다는 전제
- 수준: 단위
- 전제와 범위: 종목 하나, 전략 하나, 시험 9개
- 실제 구성요소: oms-core 판단(decide)과 적용(apply), 재생
- 모의·시뮬레이션: 거래소 입력(시험이 직접 만든 VenueInput)
- 주입한 장애: 같은 실행의 재전달, 변경보다 먼저 물은 포지션 응답, 지난 세션의 응답, 아직 도착하지 않은 우리 체결, 분류 못 한 실행 뒤 같은 키의 체결
- 판정 기준과 근거: 이벤트 이름 목록, 계좌·미귀속·전략 포지션, 미확인 불일치 종류, 차단과 주문 거절 사유, 재생한 코어와 실시간 코어의 같음, 저널 합 = 거래소 포지션, 직렬화 문자열
- 재현: `cargo test -p oms-core --test venue_changes`
- 뒷받침하는 것: C-FILL, C-MISMATCH
- 확인하지 않은 범위: Bybit 실행 행의 실제 모양(testnet T15). 게이트웨이의 execType 분류표는 V-BYBIT-READ-1이 보고, 분류한 입력을 이 코어에 넣는 끝에서 끝까지의 시험은 없다(두 시험이 같은 VenueInput 모양에서 만난다). 펀딩 합계는 어떤 한도에도 들어가지 않는다. 불일치가 [OMS P1]로 가는 경로(U14). 거래소가 응답 시각 기준으로 일관된다는 가정

| 시험 | 결과 | 비고 |
|---|---|---|
| `core/a_funding_payment_is_cash_only_and_booked_once` | 미실행 |  |
| `core/a_venue_position_change_is_booked_flagged_and_holds_plain_orders` | 미실행 |  |
| `core/an_unclassified_execution_holds_plain_orders` | 미실행 |  |
| `core/the_next_positions_answer_after_a_change_is_the_position_of_record` | 미실행 |  |
| `core/a_positions_answer_equal_to_the_journal_settles_without_an_adjustment` | 미실행 |  |
| `core/a_positions_answer_of_an_old_session_does_not_end_the_wait` | 미실행 |  |
| `core/an_operator_adjustment_is_written_as_in_schema_1` | 미실행 |  |
| `core/the_wait_does_not_end_while_an_order_of_ours_can_still_fill` | 미실행 |  |
| `core/an_unclassified_key_does_not_block_a_later_fill_with_the_same_key` | 미실행 |  |

## V-SCHEMA-2-1 — 이 빌드는 기록 스키마 2를 쓰고 1과 2를 읽는다. 기록 스키마 1로 쓴 저널(체결, 운영자 조정, 정지 포함)을 복구하면 실시간 코어와 같은 상태(다이제스트 포함)가 되고, 그 뒤에 스키마 2로 이어 쓴 기록은 새 세그먼트에 들어가며, 두 세그먼트를 함께 복구해도 실시간 상태와 같고 --re-decide 검사도 통과한다

- 상태: **미실행**
- 왜 중요한가: 기록 형식을 올려도 그 전에 쓴 저널이 그대로 다시 읽힌다는 전제
- 수준: 통합
- 전제와 범위: 기록 스키마 1 세그먼트 하나와 스키마 2 세그먼트 하나, 시험 때 만든 저널(커밋하지 않음)
- 실제 구성요소: oms-journal 쓰기·복구, oms-node recovery::recover와 verify, oms-core
- 모의·시뮬레이션: 입력(시험이 직접 만든 Input)
- 주입한 장애: 없음
- 판정 기준과 근거: 복구한 코어 = 실시간 코어, 다이제스트 같음, 스키마 1 기록을 두 번 나눠 써도 세그먼트 1개이고 스키마 2를 쓰면 2개, verify 기록 수
- 재현: `cargo test -p oms-node --test record_schema`
- 뒷받침하는 것: C-JOURNAL
- 확인하지 않은 범위: 스키마 1 저널은 이 빌드가 스키마 번호 1로 쓴 것이다. main의 옛 바이너리가 쓴 저널을 직접 읽어 보지는 않았다(옛 기록과 바이트가 같다는 것은 직렬화 시험과 코드 검토로만 본다). 옛 바이너리로 되돌리기는 안 된다(runbook §9)

| 시험 | 결과 | 비고 |
|---|---|---|
| `node-unit/this_build_writes_schema_2_and_reads_1_and_2` | 미실행 |  |
| `node-unit/a_schema_1_journal_and_a_schema_2_tail_replay_to_the_live_state` | 미실행 |  |

## V-ABSENCE-N-1 — absence_not_found_needed가 2이면 결과 불명 주문은 expires_at + absence_margin 뒤에 보낸 조회의 "없음"을 absence_margin 이상 떨어진 두 조회로 받아야 NotPlaced로 닫히고, 첫 번째는 AbsenceNoted로만 기록되며 예약은 그대로다. 만료 + 여유 전의 "없음", 같은 조회의 두 번째 답, 앞에 센 답보다 여유 시간 안쪽의 답은 세지 않고, 셈은 재생 뒤에도 같다. 기본값 1에서는 AbsenceNoted 없이 예전처럼 한 번에 닫히고, 기본값은 직렬화하지 않으며, 0은 설정 오류다

- 상태: **미실행**
- 왜 중요한가: Bybit 조회 사본의 짧은 불일치로 한 번 "없음"이 나와도 예약을 풀지 않는다는 전제. 500건 한도·서비스 재시작은 이 횟수가 아니라 게이트웨이의 주문 내역 확인이 막는다. Hyperliquid 동작은 그대로다
- 수준: 단위
- 전제와 범위: 결과 불명 주문 하나, 시험 4개
- 실제 구성요소: oms-core 판단과 적용, 설정 검증, 재생
- 모의·시뮬레이션: 거래소 입력(시험이 직접 만든 StatusNotFound)
- 주입한 장애: 만료 전 조회, 같은 조회의 중복 답, 1마이크로초와 여유 시간 직전의 두 번째 조회
- 판정 기준과 근거: 이벤트 이름 목록, 주문 단계와 종결 사유, 계좌 매수 예약, 재생한 코어와 실시간 코어의 같음, 설정 검증 오류, 직렬화 문자열
- 재현: `cargo test -p oms-core --test absence_and_market_ref`
- 뒷받침하는 것: C-DOUBT
- 확인하지 않은 범위: Bybit가 recv window 밖의 요청을 실제로 거절하는지, 게이트웨이의 요청 시각 + recv_window + 1초 ≤ expires_at 검사(Bybit 게이트웨이가 아직 없다, U16·U17), 주문 내역 조회도 닫힌 주문을 잃는지. 게이트웨이가 realtime과 주문 내역 둘 다 본 뒤에만 없음을 보낸다는 것은 V-BYBIT-READ-1(가짜 서버)

| 시험 | 결과 | 비고 |
|---|---|---|
| `core/two_answers_after_expiry_plus_margin_are_needed_when_the_policy_asks_for_two` | 미실행 |  |
| `core/one_answer_is_enough_by_default_and_writes_no_new_event` | 미실행 |  |
| `core/the_policy_refuses_zero_answers` | 미실행 |  |
| `core/a_policy_at_its_default_writes_no_new_key` | 미실행 |  |

## V-MARKET-REF-1 — 새 주문은 판단에 쓴 호가창 상태(market_ref)를 실을 수 있고, 그 값은 OrderAccepted.request로 저널 기록에 들어가 직렬화와 해석을 거쳐도 같다. market_ref가 없는 요청은 예전과 같은 바이트로 쓰인다. 같은 요청이 다른 market_ref로 다시 오면 duplicate다. 이름은 64자 이하의 [A-Za-z0-9_.:-]만 받고 거래 API는 형식이 틀린 market_ref를 해석 단계에서 거절한다

- 상태: **미실행**
- 왜 중요한가: 주문을 시세 레이크의 호가창 상태와 이을 수 있고, 재시도가 conflict로 바뀌지 않는다는 전제
- 수준: 단위
- 전제와 범위: 시험 5개
- 실제 구성요소: oms-core 판단, Record 직렬화, oms-api 요청 해석
- 모의·시뮬레이션: 없음
- 주입한 장애: 다른 market_ref의 재시도, 형식이 틀린 이름
- 판정 기준과 근거: 다시 읽은 기록의 market_ref, 직렬화 문자열, 응답 종류, 해석 오류
- 재현: `cargo test -p oms-core --test absence_and_market_ref && cargo test -p oms-api --lib`
- 뒷받침하는 것: C-NEW
- 확인하지 않은 범위: 전략이 md-feed의 MarketRef를 실제로 채워 보내는 경로(전략 쪽 코드는 이 저장소에 없다), 시세 레이크와의 결합(U15)

| 시험 | 결과 | 비고 |
|---|---|---|
| `core/market_ref_round_trips_through_the_journal` | 미실행 |  |
| `core/a_request_without_market_ref_keeps_its_bytes` | 미실행 |  |
| `core/a_repeated_request_with_another_market_ref_is_a_duplicate` | 미실행 |  |
| `core/market_ref_names_are_validated` | 미실행 |  |
| `api/tests::a_new_order_line_may_carry_a_market_ref` | 미실행 |  |

## V-TAIL-1 — 꼬리 읽기는 디스크에 더 많은 기록이 있어도 호출자가 준 내구화 번호보다 큰 기록을 돌려주지 않고, 작은 세그먼트로 회전한 저널을 끊김 없이 순서대로 읽으며, 커서 다음 번호부터 다시 시작한다. 쓰는 쪽이 잠금을 쥔 동안에도 읽고, 읽는 동안 새 쓰는 쪽이 잠금을 잡을 수 있다(잠금을 잡지 않는다). 상한 이하의 손상된 프레임이나 번호가 튀는 프레임에서는 그 앞 기록을 돌려준 뒤 오류를 내고 위치를 움직이지 않으며, 커서 앞의 손상도 오류로 알린다(끝에서 쓰는 중인 프레임은 기다린다). 기록 스키마가 바뀌어 열린 새 세그먼트와, 헤더만 있다가 새 스키마 헤더로 다시 쓰인 세그먼트를 따라가고 기록마다 스키마를 알려 준다. 엿보기는 상한 너머의 유효한 프레임을 위치를 움직이지 않고 보여 주며 잘린 마지막 프레임에서 오류 없이 멈춘다

- 상태: **미실행**
- 왜 중요한가: 프로젝터와 감시자가 OMS를 방해하지 않고, 아직 내구화되지 않아 복구가 다시 쓸 수 있는 기록을 확정된 것처럼 내보내지 않는다는 전제
- 수준: 단위
- 전제와 범위: 기록 최대 90개, 세그먼트 256바이트(회전 3개 이상)와 1 MiB, 스키마 1→2, 시험 9개
- 실제 구성요소: oms-journal Writer와 Tail, 실제 파일
- 모의·시뮬레이션: 손상(바이트 뒤집기), 잘린 꼬리(파일 길이 줄이기)
- 주입한 장애: 체크섬 손상, 잘린 마지막 프레임, 번호가 튀는 유효 프레임, 커서 앞 손상, 쓰는 쪽 교체, 스키마 변경, 헤더만 있는 세그먼트
- 판정 기준과 근거: 돌려준 번호 목록과 payload, 오류 종류, next_seq, 세그먼트 수
- 재현: `cargo test -p oms-journal --test tail`
- 뒷받침하는 것: C-TAIL
- 확인하지 않은 범위: 실행 중인 OMS 프로세스와 함께 읽는 것(프로세스 수준은 U14b 프로젝터 시험), 헤더를 다시 쓰는 rename 순간의 경합, 네트워크 파일 시스템, 복구가 마지막 세그먼트를 fsync한다는 것(페이지 캐시 유실은 흉내 낼 수 없다)

| 시험 | 결과 | 비고 |
|---|---|---|
| `journal/the_tail_never_returns_a_record_above_the_durable_cap` | 미실행 |  |
| `journal/the_tail_follows_segment_rotation_and_starts_after_a_cursor` | 미실행 |  |
| `journal/the_tail_never_takes_the_lock` | 미실행 |  |
| `journal/a_damaged_durable_frame_stops_the_tail_without_moving_it` | 미실행 |  |
| `journal/peek_beyond_shows_valid_frames_past_the_cap_without_moving` | 미실행 |  |
| `journal/a_frame_with_the_wrong_sequence_comes_after_the_good_records` | 미실행 |  |
| `journal/damage_before_the_cursor_is_an_error` | 미실행 |  |
| `journal/the_tail_follows_a_schema_change_to_a_new_segment` | 미실행 |  |
| `journal/the_tail_reads_a_header_only_segment_rewritten_with_a_new_schema` | 미실행 |  |

## V-TAIL-2 — 모든 이벤트 종류(43가지)의 인스턴스를 선택 필드까지 채워 직렬화하면, 인코더가 쓰는 모든 필드 경로가 docs/contracts/tail.md의 필드 표에 있고, 표의 모든 행은 실제로 쓰이는 필드이며 타입 열이 실제 JSON 값의 종류(문자열, 정수, 불, 목록, 열거형, 객체, null 허용)와 맞는다. 통째로 읽는 행은 설정 하나와 열거형 값뿐이다. 이벤트 종류가 늘면 시험이 컴파일되지 않는다

- 상태: **미실행**
- 왜 중요한가: 감시자(U19a)를 OMS 코드 없이 이 표만으로 만들 수 있다는 전제
- 수준: 소스 검사
- 전제와 범위: 이벤트 43종, 필드 경로 172개(설정은 통째로 한 행)
- 실제 구성요소: oms-core 이벤트 직렬화, 문서 파일
- 모의·시뮬레이션: 없음
- 주입한 장애: 없음
- 판정 기준과 근거: 표에 없는 필드 0개, 쓰이지 않는 행 0개, 이벤트 종류 43
- 재현: `cargo test -p oms-core --test event_fields`
- 뒷받침하는 것: C-TAIL
- 확인하지 않은 범위: 뜻 열의 정확성은 사람이 읽어 확인한다. 열거형 값의 목록(통째로 읽는 행)은 표에 적은 것과 코드가 같은지 시험하지 않는다. 입력(input) 쪽 필드는 표에 없다(읽는 쪽은 events만 해석한다)

| 시험 | 결과 | 비고 |
|---|---|---|
| `core/the_field_table_lists_every_field_the_encoder_writes` | 미실행 |  |
| `core/every_row_of_the_table_is_a_field_the_encoder_writes` | 미실행 |  |
| `core/only_the_configuration_and_enums_are_read_as_a_whole` | 미실행 |  |

## V-BYBIT-READ-1 — Bybit 게이트웨이의 조회 일곱 가지는 읽기만 하고, 통째로 읽을 수 있는 응답만 입력으로 넘긴다. 쪽 넘김은 커서가 비고 쪽이 덜 찼을 때만 끝으로 보고, 다 찬 쪽에 커서가 없거나 커서가 그대로이거나 쪽이 20을 넘으면 스냅숏(열린 주문, 포지션, 증거금 보고)을 넘기지 않는다. 주문 상태는 우리 orderLinkId의 행만 보고 realtime, 기간 없는 주문 내역, 코어가 주문을 받은 시각 − 1분부터 7일의 주문 내역 셋 다에 없을 때만 StatusNotFound(첫 조회 전 시각)를 낸다. 체결은 since − 2분부터 7일 이하의 창으로 읽고(창 사이 1ms 겹침, 중복 키는 한 번), 한 번에 30쪽까지 읽고 다 읽은 창까지만 선언한 채 멈추며(오류 아님), 창을 다 읽은 뒤에만 그 끝 직전까지 FillsSynced를 내며, 끝을 알 수 없는 창은 반으로 나누고, 읽을 수 없는 행에서 멈춘다. 실행 행은 execType 표 하나로 Fill(우리 주문이면 order_id, 아니면 외부 체결), Funding, PositionChange(adl, takeover, delivery, settle), UnclassifiedExecution이 된다. 증거금 보고는 totalEquity, totalMaintenanceMargin, totalMarginBalance(U17d), positionIM, 계좌 증거금 모드, 정수 레버리지, maxLeverage로 만들고 한 값이라도 못 읽으면 MarginReportInvalid다. accountMMRate는 보고를 바꾸지 않는다. 이 코드에는 주문 생성·정정·단건 취소 요청이 없다. 설정은 Bybit에서 REGULAR_MARGIN 밖의 계좌 모드 허용을 거절한다

- 상태: **미실행**
- 왜 중요한가: Bybit 계좌를 대조할 때 반쯤 읽은 응답으로 포지션·열린 주문을 틀리게 보거나, 닫힌 주문을 500건 한도·서비스 재시작 때문에 없다고 보거나, 펀딩·ADL을 체결로 기장하지 않는다는 전제
- 수준: 단위
- 전제와 범위: loopback 가짜 HTTP 서버(http-fake)에 문서 모양의 응답을 경로별로 준비, 시험 24개
- 실제 구성요소: venue::bybit의 query, exec, rest(서명한 GET), config 검증
- 모의·시뮬레이션: Bybit REST 응답(문서 예시를 본뜬 손으로 쓴 JSON)
- 주입한 장애: 남의 주문 행, 조건부 주문 상태, 다 찬 마지막 쪽, 움직이지 않는 커서, 끝없는 커서, 10016·HTTP 500·502, 읽을 수 없는 행·숫자·소수 9자리, 소수 레버리지, PORTFOLIO_MARGIN, maxLeverage 0, 계좌 유지증거금·증거금 잔고 없음과 못 읽음, 헤지 모드 행, 겹치는 창의 같은 실행
- 판정 기준과 근거: 만든 VenueInput 전체(값까지), 보낸 요청의 경로·질의·순서·개수, 창의 시작·끝 시각, 소스에 주문 요청 경로가 없음
- 재현: `cargo test -p oms-node --lib venue::bybit`
- 뒷받침하는 것: C-GW(Bybit 열), C-MARGIN, ADR-028
- 확인하지 않은 범위: 실제 Bybit 응답(문서 예시만 본떴다, testnet은 U-01 허가 뒤). 빈 커서가 마지막 쪽이라는 것, startTime·endTime이 끝값을 포함하는지, maker 체결의 orderLinkId, execFee의 maker 리베이트 부호, 펀딩·ADL 행이 실행 목록에 실리는지(T15)는 문서에 없다. 조회의 요청 한도(U17). 스트림과 노드 연결은 V-BYBIT-STREAM-1. maxLeverage는 프로세스당 한 번만 읽는다(교차 계좌에서는 어떤 규칙도 쓰지 않는다). 주문 내역 기간이 생성 시각과 갱신 시각 중 무엇을 거르는지, 2분보다 늦게 실리는 실행 행, 변경 직후의 늦은 포지션 응답(ADR-028 §8)

| 시험 | 결과 | 비고 |
|---|---|---|
| `node-unit/venue::bybit::exec::tests::a_trade_of_our_order_is_a_fill_of_that_order` | 미실행 |  |
| `node-unit/venue::bybit::exec::tests::a_trade_of_an_order_that_is_not_ours_is_an_external_fill` | 미실행 |  |
| `node-unit/venue::bybit::exec::tests::a_funding_row_is_cash_and_never_a_fill` | 미실행 |  |
| `node-unit/venue::bybit::exec::tests::each_venue_made_execution_type_is_a_position_change_of_its_kind` | 미실행 |  |
| `node-unit/venue::bybit::exec::tests::every_other_execution_type_is_unclassified` | 미실행 |  |
| `node-unit/venue::bybit::exec::tests::a_row_that_cannot_be_read_is_an_error_not_an_input` | 미실행 |  |
| `node-unit/venue::bybit::query::tests::the_status_of_our_order_is_read_from_the_open_order_view` | 미실행 |  |
| `node-unit/venue::bybit::query::tests::an_order_missing_from_the_open_order_view_is_looked_up_in_the_history` | 미실행 |  |
| `node-unit/venue::bybit::query::tests::an_order_is_not_found_only_when_neither_view_has_it` | 미실행 |  |
| `node-unit/venue::bybit::query::tests::a_status_answer_about_another_order_or_of_a_kind_we_never_send_concludes_nothing` | 미실행 |  |
| `node-unit/venue::bybit::query::tests::the_status_table` | 미실행 |  |
| `node-unit/venue::bybit::query::tests::open_orders_are_read_through_every_page` | 미실행 |  |
| `node-unit/venue::bybit::query::tests::a_snapshot_whose_last_page_cannot_be_told_gives_no_input` | 미실행 |  |
| `node-unit/venue::bybit::query::tests::positions_are_signed_by_side` | 미실행 |  |
| `node-unit/venue::bybit::query::tests::the_margin_report_maps_the_unified_account` | 미실행 |  |
| `node-unit/venue::bybit::query::tests::a_margin_answer_that_cannot_be_read_as_a_whole_is_refused_as_a_whole` | 미실행 |  |
| `node-unit/venue::bybit::query::tests::the_account_mm_rate_field_is_not_used_for_the_judgment` | 미실행 |  |
| `node-unit/venue::bybit::query::tests::the_venue_setting_and_the_account_mode_are_read_as_they_are` | 미실행 |  |
| `node-unit/venue::bybit::query::tests::fills_are_read_in_windows_of_at_most_seven_days_each_declared_synced` | 미실행 |  |
| `node-unit/venue::bybit::query::tests::a_row_that_cannot_be_read_stops_the_sync_before_its_window_is_declared` | 미실행 |  |
| `node-unit/venue::bybit::query::tests::fills_follow_the_cursor_and_a_window_whose_last_page_cannot_be_told_is_split` | 미실행 |  |
| `node-unit/venue::bybit::query::tests::a_long_gap_is_synced_over_several_queries_within_the_page_budget` | 미실행 |  |
| `node-unit/venue::bybit::query::tests::the_gateway_has_no_order_request` | 미실행 |  |
| `node-unit/config::tests::a_bybit_configuration_loads_and_each_unsafe_value_is_refused` | 미실행 |  |

## V-BYBIT-STREAM-1 — Bybit 게이트웨이는 시작 확인이 통과하고 막힘이 없을 때만 비공개 스트림에 접속하고, auth(GET/realtime + 만료 시각의 서명, 만료 = 지금 + recv_window)가 확인된 뒤 order.linear·execution.linear를 구독하며, 구독이 확인된 뒤에만 Link를 올린다. 접속마다 세션 번호가 커진다. ping 뒤 ping 간격 안에 pong이 없거나(코어 입력 큐를 기다린 시간은 빼고), 소켓이 닫히거나, 읽을 수 없는 행이나 symbol 없는 행이 오거나, 연결 중 막힘(IP 변경, 키 거부, IP 차단)이 생기면 Link를 내리고, 키 거부와 IP 변경이 아니면 다시 붙는다. 인증 거부, 조회의 키 거부 코드, IP 변경 뒤에는 재시작까지 접속도 요청도 하지 않는다. HTTP 403(REST든 스트림 업그레이드든) 뒤에는 멈춤 시간 동안 REST 요청과 스트림 접속이 없고, 끝나면 다시 붙는다. 구독 거부와 응답 없는 인증은 키 거부가 아니라 다시 시도하고, 인증 응답 전에는 구독을 보내지 않는다. 연결이 올라가 첫 pong을 받았으면 재연결 기다림은 처음 값으로 돌아가고, pong 전에 끊기면 기다림이 계속 는다. 설정한 종목의 주문 행은 OrderStatus(Stream), 실행 행은 실행 분류표를 거쳐 받은 시각이 실린 입력이 되고 다른 종목의 행은 넘기지 않는다. 주문과 취소는 V-BYBIT-ORDERS-1

- 상태: **미실행**
- 왜 중요한가: Bybit 연결이 확인되지 않은 키·계좌·구독으로 올라오지 않고, 조용히 죽은 연결을 살아 있다고 보지 않으며, 거부된 키나 차단된 IP로 요청을 계속 보내지 않는다는 전제
- 수준: 통합
- 전제와 범위: loopback 가짜 웹소켓 서버(auth·subscribe·ping 응답을 시험이 고른다)와 가짜 HTTP 서버(REST, IP 확인), 짧게 줄인 시간, 코어 쪽 소비를 멈출 수 있는 입력 큐, 시험 18개
- 실제 구성요소: venue::bybit::gateway(시작 확인·IP 반복, 스트림 반복, 조회, 멈춤), stream, checks, query, exec, rest, venue::start(명령·조회 반복), InputQueues
- 모의·시뮬레이션: Bybit 비공개 스트림과 REST(문서 모양의 손으로 쓴 JSON), 공인 IP 확인 서비스, 시간(ping 150ms, 멈춤 600ms)
- 주입한 장애: 격리 계좌, pong 없음, 읽을 수 없는 실행 행, symbol 없는 행, 소켓 끊김, 인증 거부, 응답 없는 인증, 구독 거부, 연결 중 IP 변경, 조회의 HTTP 403, 스트림 업그레이드의 HTTP 403, 조회의 33004, 거래하지 않는 종목의 행, 가득 찬 코어 입력 큐, 거듭된 재연결 실패, 구독 직후 매번 오는 읽을 수 없는 메시지
- 판정 기준과 근거: 코어에 간 입력 전체(Link 순서·세션 번호·사유, 상태·체결 값), 가짜 서버가 받은 메시지와 요청의 순서·개수, 접속 수, 막힘 이유
- 재현: `cargo test -p oms-node --lib venue::bybit`
- 뒷받침하는 것: C-GW(Bybit 열), C-LINK, ADR-028
- 확인하지 않은 범위: 실제 Bybit 스트림(testnet은 U-01 허가 뒤), TLS 접속(시험은 ws://), 인증 실패 응답의 실제 모양(문서에 없다), 재연결 뒤 놓친 메시지를 거래소가 다시 주는지(T4), 비거래 실행이 스트림에 실리는지(T15), 노드 바이너리를 kind = bybit로 띄우는 끝에서 끝까지의 시험(V-BYBIT-NODE-1, failpoints 빌드의 시험용 연결 주소로), 상태 API의 gateway.block 출력(코드 검토만), 참조 가격(V-BYBIT-REFPX-1), 주문 전송(V-BYBIT-ORDERS-1), 정리 스위치의 10초 규칙(U18b)

| 시험 | 결과 | 비고 |
|---|---|---|
| `node-unit/venue::bybit::stream::tests::the_sent_messages_have_the_documented_shapes` | 미실행 |  |
| `node-unit/venue::bybit::stream::tests::frames_are_read_by_their_documented_fields` | 미실행 |  |
| `node-unit/venue::bybit::stream::tests::an_order_row_becomes_a_stream_status_of_our_order_or_of_a_foreign_one` | 미실행 |  |
| `node-unit/venue::bybit::gateway::tests::the_link_comes_up_after_the_checks_auth_and_subscribe_and_stream_rows_become_inputs` | 미실행 |  |
| `node-unit/venue::bybit::gateway::tests::the_stream_does_not_connect_while_a_boot_check_fails` | 미실행 |  |
| `node-unit/venue::bybit::gateway::tests::a_missing_pong_takes_the_link_down_and_the_reconnect_is_a_new_session` | 미실행 |  |
| `node-unit/venue::bybit::gateway::tests::an_unreadable_row_or_a_closed_socket_takes_the_link_down_and_it_comes_back` | 미실행 |  |
| `node-unit/venue::bybit::gateway::tests::a_refused_stream_auth_blocks_until_the_restart_and_nothing_is_sent_after` | 미실행 |  |
| `node-unit/venue::bybit::gateway::tests::a_refused_subscription_never_brings_the_link_up_and_is_retried` | 미실행 |  |
| `node-unit/venue::bybit::gateway::tests::an_ip_change_while_up_takes_the_link_down_for_good` | 미실행 |  |
| `node-unit/venue::bybit::gateway::tests::an_ip_ban_stops_every_request_for_the_pause_then_the_link_comes_back` | 미실행 |  |
| `node-unit/venue::bybit::gateway::tests::a_key_rejected_by_a_query_takes_the_link_down_for_good` | 미실행 |  |
| `node-unit/venue::bybit::gateway::tests::a_403_on_the_stream_upgrade_pauses_every_request_then_the_link_comes_up` | 미실행 |  |
| `node-unit/venue::bybit::gateway::tests::no_subscription_goes_out_before_the_auth_answer_and_a_silent_auth_is_retried` | 미실행 |  |
| `node-unit/venue::bybit::gateway::tests::a_row_without_a_symbol_takes_the_link_down` | 미실행 |  |
| `node-unit/venue::bybit::gateway::tests::time_spent_waiting_on_a_full_core_queue_does_not_count_against_the_pong` | 미실행 |  |
| `node-unit/venue::bybit::gateway::tests::the_reconnect_waits_start_over_after_a_link_that_came_up` | 미실행 |  |
| `node-unit/venue::bybit::gateway::tests::a_link_that_fails_before_its_first_pong_keeps_the_waits_growing` | 미실행 |  |

## V-BYBIT-ORDERS-1 — Bybit 게이트웨이는 주문과 취소마다 결과 하나를 돌려주고 REST 요청을 많아야 하나 보내며 다시 보내지 않는다. 생성은 orderLinkId = 우리 고객 주문번호, category linear, Limit, 단방향(positionIdx 0)으로 보내고 보낸 본문에 서명한다. 요청 시각 + recv_window + 1초가 expires_at보다 늦거나, 막힘이 있거나, 거래하지 않는 종목이거나, 접속이 되지 않으면 보내지 않고 NotSent다. retCode 0은 생성이면 orderId가 있을 때만 Placed(Open, 누적 수량 없음), 취소면 Confirmed. 오류 문서의 0 아닌 코드는 UNKNOWN_CODES가 아니면 Rejected, 그 밖의 모든 응답(문서에 없는 코드, 2xx가 아닌 HTTP, 시간 초과, 끊김, 읽을 수 없는 본문)은 Unknown이다. HTTP 403과 retCode 10018은 IP 차단 멈춤을, HTTP 401과 키 거부 retCode는 키 거부를 건다. 요청은 자리를 잡은 때부터 응답 + 1초까지(응답이 없으면 요청 시각 + recv_window + 1초 + 1초까지) 세므로 보낸 때와 응답(또는 마지막 도착 가능 시각) 사이 어디에 도착하든 어느 1초에도 주문은 8개, 주문과 취소는 합쳐 10개를 넘지 않고, 주문이 8개를 다 써도 취소는 바로 나가며, 차면 기다린다(버리지 않는다). 아무 바이트도 나가지 않은 요청은 자리를 바로 돌려준다. 시작 확인은 설정의 px_tick·qty_step이 거래소 tickSize·qtyStep의 배수가 아니면 막고, 로컬 시계가 거래소보다 500ms 넘게 앞설 수 있으면(시작 확인 전, IP 확인 주기마다, 막힌 동안 10초마다, 왕복 250ms 넘는 측정은 판정 안 함) 생성만 막고 취소와 스트림은 막지 않는다

- 상태: **미실행**
- 왜 중요한가: 결과를 모르는 주문을 거부로 보거나 다시 보내 두 번 놓지 않고, 만료 뒤에 처리될 수 있는 주문을 보내지 않으며, 거래소 요청 한도를 넘어 IP 차단을 부르지 않는다는 전제
- 수준: 통합
- 전제와 범위: loopback 가짜 HTTP 서버(경로별 응답: 답, 끊김, 무응답, 오류 상태), 고정한 시계, 오류 문서의 코드 번호 목록(2026-11-21), 시각을 입력으로 받는 한도 규칙, 시험 27개
- 실제 구성요소: oms-core::rate, venue::bybit::{answer, orders, rest, checks, gateway}, venue::start(명령 반복)
- 모의·시뮬레이션: Bybit REST(문서 모양의 손으로 쓴 JSON), 시간(창 시험은 시각 입력, 9번째 주문 시험은 실제 1초)
- 주입한 장애: 응답 없음(무응답, 끊김, 502, 읽을 수 없는 본문), 접속 거부, 만료가 1µs 모자람, 기다린 뒤 생긴 막힘, 잔고 부족 거부, orderLinkId 중복, IP 불일치 코드, HTTP 401, HTTP 403, orderId 없는 접수, 취소 대상 없음, 거래하지 않는 종목, 1초에 9번째 주문, 0.3초 걸린 응답 8개, 접속 거부 20번, 거꾸로 간 시계, 앞선 로컬 시계, 느린 시계 응답(252ms), 시계 막힘 중의 취소, 시계 확인의 403, 읽을 수 없는 거래소 시각, 10018, 거래소 단위보다 촘촘한 설정 단위, 읽을 수 없는 단위
- 판정 기준과 근거: 코어에 간 결과(종류, 이유, venue_oid, 상태), 가짜 서버가 받은 요청의 경로·개수·본문·서명 머리글, 막힘 이유, 한도 규칙이 돌려준 시각, 지연(0–0.4초)과 임의 도착의 일정 2만 번에서 도착 기준 어느 1초든 주문 ≤ 8·전체 ≤ 10
- 재현: `cargo test -p oms-core --lib rate && cargo test -p oms-node --lib venue::bybit`
- 뒷받침하는 것: C-GW(Bybit 열: G-1, G-2, G-3, G-4, G-5, 요청 한도), ADR-028 §7d
- 확인하지 않은 범위: 실제 Bybit 주문 응답과 코드의 실제 뜻(T6, testnet은 U-01 허가 뒤), WS Trade와 REST의 한도 공유(T8), WS Trade 경로(V-BYBIT-TRADE-1), 참조 가격(V-BYBIT-REFPX-1)과 노드 끝에서 끝까지(V-BYBIT-NODE-1), 저널 flush 실패 때 요청 0(V-BYBIT-NODE-1, 커밋 단계가 명령을 내구화 뒤에만 넘기는 것은 V-COMMIT 계열이 가상 거래소로 보인다), 감시자·다른 프로세스가 같은 UID로 보내는 요청(세지 않는다), 접속 단계 오류가 바이트를 내보내지 않았다는 가정(A-08), 접수된 취소가 적용되지 않는 경우(U18b·U19), 운영 중 틱·수량 단위 변경

| 시험 | 결과 | 비고 |
|---|---|---|
| `core/rate::tests::the_ninth_order_in_a_second_waits_while_two_cancels_still_go_out` | 미실행 |  |
| `core/rate::tests::the_window_slides_with_each_request` | 미실행 |  |
| `core/rate::tests::cancels_may_take_the_reserve_and_then_block_orders_too` | 미실행 |  |
| `core/rate::tests::a_clock_that_goes_back_does_not_free_a_slot` | 미실행 |  |
| `core/rate::tests::a_request_counts_until_one_window_after_it_finished` | 미실행 |  |
| `core/rate::tests::no_rolling_second_of_arrivals_holds_more_than_the_limits` | 미실행 |  |
| `node-unit/venue::bybit::answer::tests::every_documented_code_has_exactly_one_class` | 미실행 |  |
| `node-unit/venue::bybit::answer::tests::anything_but_a_documented_refusal_is_unknown` | 미실행 |  |
| `node-unit/venue::bybit::answer::tests::code_zero_is_accepted_with_its_object_from_rest_or_ws` | 미실행 |  |
| `node-unit/venue::bybit::orders::tests::a_create_is_one_signed_limit_order_and_its_ack_is_an_open_order` | 미실행 |  |
| `node-unit/venue::bybit::orders::tests::an_expiry_inside_the_window_plus_one_second_sends_nothing` | 미실행 |  |
| `node-unit/venue::bybit::orders::tests::a_block_after_the_wait_sends_nothing` | 미실행 |  |
| `node-unit/venue::bybit::orders::tests::no_answer_is_unknown_and_nothing_is_sent_twice` | 미실행 |  |
| `node-unit/venue::bybit::orders::tests::a_dead_venue_is_not_sent` | 미실행 |  |
| `node-unit/venue::bybit::orders::tests::refusals_are_rejected_and_the_key_and_ip_answers_have_their_effects` | 미실행 |  |
| `node-unit/venue::bybit::orders::tests::a_cancel_goes_by_client_id_and_has_the_same_classes` | 미실행 |  |
| `node-unit/venue::bybit::orders::tests::the_ninth_create_in_a_second_waits_while_a_cancel_goes_out` | 미실행 |  |
| `node-unit/venue::bybit::orders::tests::the_send_time_rule_is_exact_at_the_bound` | 미실행 |  |
| `node-unit/venue::bybit::orders::tests::a_slot_without_an_answer_counts_until_its_last_arrival_plus_a_second` | 미실행 |  |
| `node-unit/venue::bybit::checks::tests::a_local_clock_ahead_by_more_than_half_a_second_blocks` | 미실행 |  |
| `node-unit/venue::bybit::checks::tests::a_slow_clock_answer_proves_nothing` | 미실행 |  |
| `node-unit/venue::bybit::checks::tests::the_clock_slot_blocks_creates_only` | 미실행 |  |
| `node-unit/venue::bybit::gateway::tests::a_local_clock_ahead_of_the_venue_keeps_orders_home_but_not_cancels_or_the_link` | 미실행 |  |
| `node-unit/venue::bybit::gateway::tests::a_ban_met_by_the_clock_check_ends_the_round` | 미실행 |  |
| `node-unit/venue::bybit::gateway::tests::an_order_and_a_cancel_each_go_out_once_over_rest_and_get_one_result` | 미실행 |  |
| `node-unit/venue::bybit::gateway::tests::nothing_goes_out_while_blocked_or_for_an_untraded_symbol` | 미실행 |  |
| `node-unit/venue::bybit::checks::tests::steps_off_the_venue_steps_block_and_unreadable_steps_are_unchecked` | 미실행 |  |

## V-BYBIT-TRADE-1 — Bybit 게이트웨이는 주문과 취소를 WS Trade(/v5/trade)로 보내고, REST로는 WS Trade 연결 작업이 그 요청의 아무 바이트도 쓰지 않았을 때(인증된 연결 없음, 쓰기 전 막힘)만, 막힘을 다시 본 뒤 같은 한도 자리와 같은 요청 시각으로 보낸다. 요청은 두 경로를 합쳐 많아야 한 번 나가고 한도 자리는 경로와 상관없이 하나다(1초 안의 9번째 생성은 경로가 섞여도 기다린다). 요청은 reqId(생성 c + 고객 주문번호, 취소 x + 36진 시도 번호 + 고객 주문번호, 36자 이하), header의 X-BAPI-TIMESTAMP·X-BAPI-RECV-WINDOW, op, REST 본문 하나인 args로 되어 있다. 응답은 REST와 같은 표로 분류하고, 20006·10019·10429는 결과 불명, 10019는 연결을 닫고 다시 열며, 10403과 10018은 IP 차단 멈춤을, 키 거부 코드는 키 거부를 건다. 쓴 요청에 request_timeout_ms 안에 응답이 없거나 그 사이 연결이 내려가면(pong 없음, 끊김, 막힘) 결과 불명이고 어느 경로로도 다시 보내지 않으며, 한도는 요청 시각 + recv_window + 1초 + 1초까지 센다. 늦은 응답은 버린다. 같은 reqId가 기다리는 중이면 다시 쓰지 않는다. WS Trade 연결은 막힘이 없을 때만 IPv4로 접속하고, auth(비공개 스트림의 서명)의 retCode가 0일 때만 올라가며, 0이 아니면 재시작까지 키 거부, 업그레이드의 HTTP 403은 IP 차단 멈춤이다. ping 뒤 간격 안에 pong이 없으면 내려가고, 재연결 기다림은 첫 pong을 받은 연결 뒤에만 처음 값으로 돌아간다. WS Trade는 Link를 움직이지 않는다

- 상태: **미실행**
- 왜 중요한가: 주문 경로가 둘이 되어도 같은 주문이 두 번 놓이지 않고(G-3), 결과를 모르는 요청을 거부나 미전송으로 보지 않으며(G-2), 막힘·차단·거부된 키로 요청을 보내지 않고, 두 경로를 합쳐 거래소 요청 한도를 넘지 않는다는 전제
- 수준: 통합
- 전제와 범위: loopback 가짜 WS Trade 서버(auth·pong·주문 응답을 시험이 고른다: 응답 코드, 침묵, 요청 뒤 끊음, ping 무응답, auth 거부, 업그레이드 403, auth 뒤 읽을 수 없는 메시지)와 가짜 비공개 스트림·가짜 HTTP 서버, 가짜 연결 작업(orders 시험), 막힘을 시험이 정하는 가짜 Host(연결 작업 시험), 짧게 줄인 시간, 시험 23개
- 실제 구성요소: venue::bybit::{trade, orders, answer, gateway}(연결 작업, 경로 고르기, 한도, 분류), oms-core::rate, venue::start(명령 반복)
- 모의·시뮬레이션: Bybit WS Trade와 REST(문서 모양의 손으로 쓴 JSON), 시간(ping 150ms, 요청 제한 300ms 또는 5초, 멈춤 600ms)
- 주입한 장애: WS Trade 끊김과 복귀, 응답 없는 요청 셋, ping 무응답, 요청을 읽은 뒤 끊김, 20006, 10019, 10429, 10016, 110072, 110007, 10010, 10018, 10403, orderId 없는 접수, 늦은 응답, 기다리는 요청 없는 키 거부 응답, 줄 선 요청 앞의 10403, auth 거부, 업그레이드 403, pong 전에 거듭 끊김, 첫 확인과 REST 사이에 생긴 막힘, 쓰기 직전에 생긴 막힘, 기다리는 중인 reqId, 두 경로에 걸친 1초 9번째 생성, 응답 없는 WS 요청 8개
- 판정 기준과 근거: 코어에 간 결과(종류, 이유, venue_oid), 가짜 WS Trade가 받은 메시지(reqId, header, op, args, 주문마다 생성 수)와 가짜 HTTP가 받은 요청의 경로·개수, 막힘 이유, 연결 수와 재연결 간격, 9번째 생성이 나간 시각, Link 입력 수
- 재현: `cargo test -p oms-node --lib venue::bybit`
- 뒷받침하는 것: C-GW(Bybit 열: G-1, G-2, G-3, 주문 경로, 요청 한도), ADR-028 §7e
- 확인하지 않은 범위: 실제 WS Trade(testnet은 U-01 허가 뒤): 응답의 실제 모양(data, header, connId), 실패한 auth와 pong의 실제 모양, 무응답 때 거래소의 동작, reqId 중복의 범위와 기간, WS Trade의 요청 시각 검사가 REST와 같은지(G-4의 가정, ADR-028 §8), WS Trade와 REST의 한도 공유(T8). TLS 접속(시험은 ws://). 노드 끝에서 끝까지는 V-BYBIT-NODE-1. 상태 API의 WS Trade 상태(이 판에 없음)

| 시험 | 결과 | 비고 |
|---|---|---|
| `node-unit/venue::bybit::trade::tests::req_ids_are_unique_per_request_and_fit_the_venue_limit` | 미실행 |  |
| `node-unit/venue::bybit::trade::tests::a_request_carries_its_id_time_window_op_and_the_rest_body` | 미실행 |  |
| `node-unit/venue::bybit::trade::tests::the_auth_message_is_the_private_stream_signature` | 미실행 |  |
| `node-unit/venue::bybit::trade::tests::frames_are_read_by_their_documented_fields` | 미실행 |  |
| `node-unit/venue::bybit::orders::tests::a_request_goes_over_ws_trade_when_it_is_up_and_not_over_rest` | 미실행 |  |
| `node-unit/venue::bybit::orders::tests::a_request_not_written_goes_over_rest_once_unless_a_block_came` | 미실행 |  |
| `node-unit/venue::bybit::orders::tests::a_written_request_without_an_answer_is_unknown_and_not_sent_again` | 미실행 |  |
| `node-unit/venue::bybit::orders::tests::ws_answers_have_the_rest_classes_and_their_effects` | 미실행 |  |
| `node-unit/venue::bybit::orders::tests::the_ninth_create_in_a_second_waits_across_both_paths` | 미실행 |  |
| `node-unit/venue::bybit::orders::tests::a_ws_request_without_an_answer_counts_until_its_last_arrival_plus_a_second` | 미실행 |  |
| `node-unit/venue::bybit::gateway::tests::orders_go_over_ws_trade_when_it_is_up_over_rest_while_it_is_down_and_back` | 미실행 |  |
| `node-unit/venue::bybit::gateway::tests::a_silent_ws_trade_makes_every_unanswered_request_unknown_and_nothing_is_sent_again` | 미실행 |  |
| `node-unit/venue::bybit::gateway::tests::a_ws_trade_that_stops_answering_pings_makes_its_pending_request_unknown` | 미실행 |  |
| `node-unit/venue::bybit::gateway::tests::a_connection_that_drops_after_reading_the_request_makes_it_unknown` | 미실행 |  |
| `node-unit/venue::bybit::gateway::tests::answers_20006_and_10019_are_unknown_and_10019_reopens_the_connection` | 미실행 |  |
| `node-unit/venue::bybit::gateway::tests::a_late_answer_is_dropped` | 미실행 |  |
| `node-unit/venue::bybit::gateway::tests::a_refused_ws_trade_auth_rejects_the_key_until_the_restart` | 미실행 |  |
| `node-unit/venue::bybit::gateway::tests::a_ws_ip_limit_answer_starts_the_ban_pause` | 미실행 |  |
| `node-unit/venue::bybit::gateway::tests::a_403_on_the_ws_trade_upgrade_pauses_every_request` | 미실행 |  |
| `node-unit/venue::bybit::gateway::tests::ws_trade_reconnect_waits_grow_until_a_pong_then_start_over` | 미실행 |  |
| `node-unit/venue::bybit::gateway::tests::the_task_writes_nothing_under_a_block_and_no_reqid_twice` | 미실행 |  |
| `node-unit/venue::bybit::gateway::tests::the_task_applies_a_ban_or_key_answer_before_its_next_write` | 미실행 |  |
| `node-unit/venue::bybit::tests::endpoints_are_the_documented_hosts` | 미실행 |  |

## V-BYBIT-REFPX-1 — Bybit 게이트웨이는 공개 linear 스트림에 거래하는 종목마다 tickers.<symbol>을 구독하고(구독 요청 하나에 토픽 10개까지, 모든 응답이 success: true일 때만 구독됨, 거부되거나 handshake 시간 안에 답이 없는 요청이 하나라도 있으면 pong이 와도 끊고 다시 붙음), 종목마다 그 연결의 스냅숏의 markPrice를 mark로 삼아 RefPrice(px = mark, venue_ts = 메시지 ts의 µs)로 코어에 보낸다. 스냅숏 전의 델타는 버리고, markPrice가 있는 델타는 mark를 바꾸며, markPrice 없는 델타는 든 mark를 그 델타의 ts로 다시 보낸다. 내보냄은 종목마다 ref_price_interval_ms에 많아야 한 번이고(재연결을 넘어 센다) 메시지 없이 다시 보내지 않는다. mark는 연결마다 비운다. 읽을 수 없는 티커 메시지(JSON 아님, type·data·symbol·markPrice·ts 이상, markPrice 없는 스냅숏, 소수 9자리, 0 이하)는 연결을 끊고 다시 붙으며, 거래하지 않는 종목의 메시지는 읽지 않고 버린다. 연결은 막힘이 없을 때만 IPv4로 접속하고, 연결 중 막힘이 생기면 끊고 막힌 동안 다시 붙지 않으며, 업그레이드의 HTTP 403은 IP 차단 멈춤이다. ping 뒤 간격 안에 pong(op = pong, 또는 공개 linear의 op = ping + ret_msg = pong)이 없으면 끊는다. 첫 pong 전에 끊긴 연결은 재연결 대기를 늘리고, 첫 pong 뒤에 끊긴 연결만 대기를 처음으로 되돌린다. 공개 스트림은 Link를 움직이지 않는다. 설정의 ref_price_interval_ms는 1000 이상, ref_price_max_age_us의 절반 이하만 받는다. 시험용 연결 주소 OMS_TEST_BYBIT_WIRING은 다섯 주소의 호스트가 모두 127.0.0.1일 때만 받는다

- 상태: **미실행**
- 왜 중요한가: 가격 범위의 기준이 거래소가 정한 공정 가격이고, 게이트웨이가 앞 연결의 mark를 새 연결에서 다시 보내지 않으며(코어는 마지막으로 받은 가격의 나이를 ref_price_max_age_us로 제한한다. 재연결 직후 간격 안에서는 앞 연결이 보낸 마지막 가격으로 판정될 수 있다), 거래소가 멈춘 종목의 가격을 OMS가 살려 두지 않는다는 전제. 시험용 주소가 실제 거래소나 다른 호스트로 새지 않는다는 전제
- 수준: 통합
- 전제와 범위: loopback 가짜 공개 웹소켓 서버(구독 응답·pong을 시험이 고른다, 공개 linear의 pong 모양)와 가짜 비공개 스트림·가짜 HTTP 서버, 시각을 입력으로 받는 mark 상태, 짧게 줄인 시간(ping 150ms, 간격 300ms), 시험 18개
- 실제 구성요소: venue::bybit::ticker(메시지, mark 상태, 연결 작업), gateway(spawn_tasks, Host, Wiring::of_test), config::validate_bybit, InputQueues
- 모의·시뮬레이션: Bybit 공개 스트림(문서 모양의 손으로 쓴 JSON), 시간
- 주입한 장애: 스냅숏 전 델타, 거래하지 않는 종목, 간격 안의 델타, markPrice 없는 델타, 읽을 수 없는 메시지 16가지, 읽을 수 없는 델타 뒤 재연결, 구독 거부, 구독 무응답(요청 하나, 또는 둘 중 둘째), 첫 pong 전 실패의 반복, pong 없음, 업그레이드 403, 시작 확인 막힘, 연결 중 IP 변경, 종목 12개, 127.0.0.1이 아닌 주소 8가지와 빠진·모르는 키, 간격 0·999·7501ms와 낡음 한도가 작은 설정
- 판정 기준과 근거: 코어에 간 입력(RefPrice의 종목·가격·venue_ts, 개수, Link 수), 가짜 서버가 받은 구독 요청의 args와 개수, 접속 수, 막힘 이유, 손으로 적은 기대값
- 재현: `cargo test -p oms-node --lib -- venue::bybit config::tests`
- 뒷받침하는 것: C-GW(Bybit 열: 참조 가격), ADR-028 §7f
- 확인하지 않은 범위: 실제 공개 스트림(testnet은 U-01 허가 뒤): 구독 거부의 실제 모양, 실제 markPrice의 자릿수, 스냅숏이 구독마다 한 번이라는 것. TLS 접속(시험은 ws://). 델타의 순서 번호(cs)는 읽지 않는다(빠진 델타는 다음 markPrice가 고친다, ADR-028 §8). 연결 하나의 토픽 수 한도. 배포 바이너리에 시험용 주소가 없다는 것(cfg(feature = failpoints), 코드 검토만)

| 시험 | 결과 | 비고 |
|---|---|---|
| `node-unit/venue::bybit::ticker::tests::subscribe_requests_hold_at_most_ten_topics_each` | 미실행 |  |
| `node-unit/venue::bybit::ticker::tests::frames_are_read_by_their_documented_fields` | 미실행 |  |
| `node-unit/venue::bybit::ticker::tests::an_unreadable_ticker_frame_is_an_error` | 미실행 |  |
| `node-unit/venue::bybit::ticker::tests::a_snapshot_then_deltas_give_the_mark_at_most_once_per_interval` | 미실행 |  |
| `node-unit/venue::bybit::ticker::tests::a_new_connection_knows_no_mark_until_its_snapshot_and_keeps_the_interval` | 미실행 |  |
| `node-unit/venue::bybit::gateway::tests::the_ticker_mark_becomes_the_reference_price_at_most_once_per_interval` | 미실행 |  |
| `node-unit/venue::bybit::gateway::tests::an_unreadable_ticker_frame_ends_the_connection_and_the_next_starts_with_no_mark` | 미실행 |  |
| `node-unit/venue::bybit::gateway::tests::a_refused_ticker_subscription_is_retried` | 미실행 |  |
| `node-unit/venue::bybit::gateway::tests::an_unanswered_ticker_subscription_ends_the_connection_after_the_handshake_time` | 미실행 |  |
| `node-unit/venue::bybit::gateway::tests::one_unanswered_subscribe_request_of_two_ends_the_connection` | 미실행 |  |
| `node-unit/venue::bybit::gateway::tests::a_ticker_connection_that_fails_before_its_first_pong_keeps_the_waits_growing` | 미실행 |  |
| `node-unit/venue::bybit::gateway::tests::a_missing_ticker_pong_ends_the_connection_and_the_public_pong_keeps_it` | 미실행 |  |
| `node-unit/venue::bybit::gateway::tests::a_403_on_the_public_upgrade_pauses_every_request` | 미실행 |  |
| `node-unit/venue::bybit::gateway::tests::the_ticker_stream_connects_only_without_a_block` | 미실행 |  |
| `node-unit/venue::bybit::gateway::tests::the_ticker_stream_subscribes_ten_topics_per_request` | 미실행 |  |
| `node-unit/venue::bybit::gateway::tests::the_test_wiring_takes_only_hosts_on_127_0_0_1` | 미실행 |  |
| `node-unit/venue::bybit::tests::endpoints_are_the_documented_hosts` | 미실행 |  |
| `node-unit/config::tests::the_bybit_reference_price_interval_is_bounded_by_one_second_and_half_the_freshness_limit` | 미실행 |  |

## V-BYBIT-NODE-1 — kind = bybit인 oms-node 프로세스(failpoints 빌드, 시험용 연결 주소로 가짜 Bybit에 연결)는 참조 가격이 오기 전 가격 범위가 있는 주문을 RefPriceUnavailable로 거절하고 아무 요청도 보내지 않으며, 공개 스트림의 티커 스냅숏 뒤에는 같은 요청을 받아 WS Trade로 한 번 보내고, 가짜 WS Trade가 그 order.create를 읽을 때 디스크의 저널 파일에 그 주문의 수락 기록이 있다(이 확인은 순서의 증명이 아니다: 기록이 네트워크 왕복보다 먼저 파일에 닿으므로 쓰기 전에 보내도 통과한다. 기록이 내구화된 뒤에만 보낸다는 것은 다음 flush 실패 시험이 증명한다). 새 주문의 저널 flush가 실패하면(OMS_CRASH_AT=sync_error_after_write) 노드는 상태 2로 멈추고 REST와 WS Trade 어느 쪽에도 생성이 닿지 않으며, failpoint 없이 다시 시작하면 그 기록을 찾고 orderLinkId 조회(realtime과 history 두 번이 모두 비었음)로 주문을 '놓이지 않음'으로 닫고, 그래도 생성은 없다. WS Trade가 생성을 읽고 답하지 않으면 주문은 결과 불명(Transport)이 되고, orderLinkId로 묻는 realtime 조회가 열린 주문을 찾아 Working이 되며, 두 경로를 합친 생성은 하나다

- 상태: **미실행**
- 왜 중요한가: Bybit 게이트웨이를 실제 노드에 붙였을 때 커밋 순서(기록이 내구화된 뒤에만 전송, C-COMMIT, flush 실패 시험)와 결과 불명의 해소(G-2, G-3)가 프로세스 경계를 넘어 지켜진다는 전제. 단위 시험의 가짜 연결 작업이 아니라 실제 바이너리의 연결 전체
- 수준: 프로세스
- 전제와 범위: 시험 3개, 각 주문 1건. 노드는 실제 바이너리로 따로 돌고 가짜 REST(http-fake)·비공개 스트림·WS Trade·공개 스트림은 시험 프로세스 안의 loopback 서버. ping 1초, 요청 제한 500ms, 수신 창 1초, 조회 주기 50ms, 정기 열린 주문 조회 60초
- 실제 구성요소: oms-node 프로세스(커밋 단계, 대조기, Bybit 게이트웨이 전체: 시작 확인, 비공개 스트림, WS Trade, 공개 스트림, 조회), 저널 파일, 소켓, 재시작
- 모의·시뮬레이션: Bybit REST와 세 웹소켓(문서 모양의 손으로 쓴 JSON), flush 실패는 failpoints 빌드에만 있는 지점, 시험용 주소는 failpoints 빌드에만 있는 환경 변수
- 주입한 장애: 참조 가격 없음, 저널 flush 실패 뒤 프로세스 정지와 재시작, WS Trade의 무응답
- 판정 기준과 근거: 전략이 받은 응답(거절 이유, 수락, 중복), 가짜 WS Trade가 order.create를 읽은 순간 oms_journal::Tail로 읽은 디스크 저널에 OrderAccepted(그 주문 번호와 인스턴스)가 있는지, 가짜 서버가 받은 생성 수(WS Trade, REST POST), orderLinkId가 든 조회 수, 종료 코드, 주문의 phase·doubt·close
- 재현: `cargo test -p oms-node --features failpoints --test bybit_node`
- 뒷받침하는 것: C-GW(Bybit 열), C-COMMIT, ADR-028 §7f
- 확인하지 않은 범위: 실제 Bybit(testnet은 U-01 허가 뒤), TLS(시험은 ws://와 http://), 전원 장애(flush 실패는 프로세스가 살아 있는 경우), 프로세스 강제 종료 중의 WS Trade 전송, 체결과 취소의 끝에서 끝까지, 여러 주문과 요청 한도, 배포 바이너리(failpoints 없음)가 시험용 주소를 읽지 않는다는 것(코드 검토만)

| 시험 | 결과 | 비고 |
|---|---|---|
| `pipeline/an_order_waits_for_the_reference_price_and_goes_out_once` | 미실행 |  |
| `pipeline/a_failed_flush_sends_nothing_and_the_restart_resolves_the_order_as_not_placed` | 미실행 |  |
| `pipeline/an_unanswered_create_is_found_open_by_its_order_link_id_and_never_sent_again` | 미실행 |  |

## V-ALERT-DIRECT-1 — alert-direct의 송신 스레드는 텔레그램 전송이 처음 다시 보내기(Retry)로 끝난 시도의 시각을 '실패 시작'으로 두고, 텔레그램이 답하면(전달 또는 400·413 거절) 지운다. 보낼 것이 없으면 실패 시작은 생기지 않는다. 둘째 채널을 설정하면 once와 set이 만든 모든 메시지가 같은 분할·같은 한도(50개)의 둘째 보관함에도 통째로 들어가 자기 스레드(alert-second)로 한 번씩 나가고, 넘치면 오래된 것부터 버리고 세어 자기 버림 알림을 보내며, 오류는 그 조각을 두고 그 전송을 끝낸다. 실패 시작에서 second_after_ms(기본 60초)를 넘게 지나면 둘째 채널에만 'Telegram 전송이 N초째 실패함' 알림이 시작 1번, repeat_ms마다 1번, 텔레그램이 다시 답하면 해소 1번 나간다. flush_for는 두 보관함을 모두 기다린다. 둘째 채널이 없으면 스레드도 복사도 없다

- 상태: **미실행**
- 왜 중요한가: 텔레그램이 막혔을 때 그것을 알릴 다른 길(설계 §7.7, D3). U18a2의 OMS 직접 알림이 이 송신기를 쓴다. md-collector와 md-lake는 둘째 채널 없이 쓰므로 동작이 그대로여야 한다
- 수준: 통합
- 전제와 범위: 시험 6개(alert 5, second_off 1). 텔레그램은 loopback http-fake(500, 400, 200을 바꿔 가며), 둘째 채널은 시험 안의 가짜(성공, 실패, 300ms 지연). 시계는 시험이 움직인다(+59초, 정확히 +60초, +61초, +5분). flush 주기 20~50ms
- 실제 구성요소: alert-direct의 송신 스레드, 둘째 채널 스레드, 보관함, 반복 규칙, reqwest 클라이언트(loopback HTTP)
- 모의·시뮬레이션: 텔레그램 Bot API(http-fake), 둘째 채널(시험의 구현), 시계
- 주입한 장애: 텔레그램 500(계속), 400, 다시 200. 둘째 채널의 계속된 오류와 회복, 느린 둘째 채널
- 판정 기준과 근거: state()의 failing_since_ms와 second 계수(queued, delivered, dropped_total, failures), 가짜 텔레그램이 받은 요청 본문, 가짜 둘째 채널이 받은 문구와 순서, +61초 뒤 1초 안에 알림, 알림이 텔레그램 요청에 없음, /proc/self/task의 스레드 이름
- 재현: `cargo test -p alert-direct`
- 뒷받침하는 것: 설계 §7.7(둘째 채널, 60초), U18a1 결정
- 확인하지 않은 범위: 실제 텔레그램과 실제 둘째 채널 서비스(D3, 정하지 않음). 보낼 것이 없을 때의 텔레그램 장애(탐침 없음, 범위 밖). 둘째 채널이 멈춰 돌아오지 않는 경우(send가 끝나지 않으면 그 스레드가 멈추고 flush_for는 시간 한도까지 기다린다). 프로세스가 끝날 때 둘째 보관함에 남은 것(저장하지 않음). OMS에의 연결은 V-ALERT-1~5(U18a2)

| 시험 | 결과 | 비고 |
|---|---|---|
| `alert/failing_since_set_and_reset` | 미실행 |  |
| `alert/second_channel_gets_every_message` | 미실행 |  |
| `alert/second_outbox_bound` | 미실행 |  |
| `alert/telegram_down_notice_after_60s` | 미실행 |  |
| `alert/flush_for_waits_for_both` | 미실행 |  |
| `alert/second_off_by_default` | 미실행 |  |

## V-ALERT-1 — OMS의 알림 스레드(oms-alert)는 공개된 읽기 모델만 읽으므로, 알림은 그 원인이 된 기록이 내구화된 뒤에만 나간다. 코어가 결정했지만 커밋 단계가 아직 내구화하지 않은 계좌 중지는 보내지 않고, 내구화되면 보낸다. flush에 실패한 묶음의 계좌 중지는 어떤 요청에도 나오지 않고, 그 뒤의 새 요청은 치명 알림 하나뿐이다. 치명 알림은 치명 사유와 마지막 내구 seq만 담고(실패한 묶음의 내용 없음) 치명 래치를 처음 본 때 한 번 나가며, 그 뒤에는 조건을 평가하지 않는다: 유예가 지난 연결 끊김도 시작하지 않고, 읽기 모델에서 사라진 중지도 해소를 보내지 않는다. 평가가 읽기 모델을 읽은 뒤 래치가 걸려 있으면 그 평가는 시작 알림도 set도 하지 않는다. 종료는 래치가 걸려 있으면 받은 종료 코드와 상관없이 치명 알림을 넣고 전송을 기다린다. 프로세스에서: 감소 전용 상태로 들어가는 기록의 flush가 실패하면 노드는 종료 코드 2로 exit_flush_ms + 5초 안에 끝나고, 가짜 텔레그램은 시작 알림과 치명 알림만 받으며(첫 시도 500 뒤 종료 전 전송에서 다시) REDUCING에 관한 것은 없다. 시작 뒤 첫 flush(Boot 기록)가 실패하면 설정이 활성화되기 전이어도 노드는 종료 코드 2로 끝나고, 가짜 텔레그램은 치명 알림 하나만 받는다(종료 전 전송)

- 상태: **미실행**
- 왜 중요한가: ADR-004의 '내구화 전에는 아무것도 밖으로 나가지 않는다'를 알림도 지켜야 한다. 복구한 노드가 모르는 사건을 사람에게 알리면 운영자가 저널에 없는 상태를 찾게 된다(ADR-029)
- 수준: 통합
- 전제와 범위: 시험 8개: 프로세스 안(실제 코어 스레드·커밋 스레드·저널 파일·알림 스레드, loopback http-fake 텔레그램) 4개, 실제 oms-node·simvenue 프로세스 2개, 단위 2개. 커밋 단계는 가득 찬 게이트웨이 큐로 붙잡거나 failpoints의 flush 실패(fail_next_sync, OMS_CRASH_AT=sync_error_after_write와 OMS_CRASH_ON=reducing_entered 또는 boot)로 멈춘다
- 실제 구성요소: oms-node 파이프라인(코어 스레드, 커밋 스레드, 저널 쓰기와 flush), 공개된 읽기 모델, oms-alert 스레드, alert-direct 송신 스레드와 reqwest(loopback HTTP), node::run의 종료 경로
- 모의·시뮬레이션: 텔레그램 Bot API(http-fake), 저장 장치의 flush 실패(failpoints), 거래소(simvenue), 알림 시계(시험이 움직임)
- 주입한 장애: flush 실패(프로세스 안과 프로세스, 시작 중 Boot 기록 포함), 커밋 단계 정지(게이트웨이 큐 가득), 치명 래치 뒤의 시간 경과와 읽기 모델 변화, 평가 직전에 걸린 래치, 치명 알림에 대한 텔레그램 500
- 판정 기준과 근거: 가짜 텔레그램이 받은 요청 본문의 수와 내용(두 번째 중지 메모 없음, REDUCING·감소 전용·daily_loss 없음), 치명 문장 = fatal_text(사유, 실패 전 durable_seq), 종료 코드와 걸린 시간, 저널의 reducing_entered 기록 번호 > 알린 내구 seq. 결함(살아 있는 코어를 읽음, 치명 검사를 평가 뒤로, 치명 뒤 평가, 코드 2에서 종료 전송 없음, 시작 중 치명이 코드 3으로 남음, 코드 2가 아니면 치명 알림 없음, 잠금을 푼 뒤 치명 재확인 없음)을 넣어 각각 잡는 것을 확인했다(M124 포함)
- 재현: `cargo test -p oms-node --features failpoints --test pipeline --test crash_points; cargo test -p oms-node --test alerts; cargo test -p oms-node --lib alerts::`
- 뒷받침하는 것: C-ALERT의 '내보내는 시점', ADR-029 §7, ADR-004
- 확인하지 않은 범위: 실제 텔레그램. 게이트웨이 막힘 조건(저널에 없는 예외, 단위 시험만). 평가 두 번 사이에 생겼다 풀린 유예 0 조건(놓친다고 적었다). SIGKILL·abort·panic(아무것도 보내지 않는다). 실제 저장 장치의 flush 오류(주입한 실패만). 래치를 다시 본 직후와 set 사이의 창(명령 몇 개 길이, 시험하지 않음). 코어 결함 뒤 치명 문장의 내구 seq가 실제보다 작을 수 있는 것(C-ALERT 한계, 리뷰 N2)

| 시험 | 결과 | 비고 |
|---|---|---|
| `pipeline/no_alert_for_a_batch_that_failed_to_flush` | 미실행 |  |
| `pipeline/a_flush_failure_sends_one_fatal_alert_and_nothing_of_the_batch` | 미실행 |  |
| `pipeline/a_flush_failure_at_startup_exits_2_and_sends_the_fatal_message` | 미실행 |  |
| `node-unit/nothing_is_sent_before_its_record_is_durable` | 미실행 |  |
| `node-unit/after_a_fatal_stop_only_the_fatal_message_is_sent` | 미실행 |  |
| `node-unit/finish_sends_the_fatal_message_whatever_the_code` | 미실행 |  |
| `node-unit/alerts::tests::the_fatal_text_holds_the_reason_and_the_seq_only` | 미실행 |  |
| `node-unit/alerts::tests::a_fatal_trip_during_an_evaluation_sets_nothing` | 미실행 |  |

## V-ALERT-2 — 텔레그램이 계속 500으로 답하는 동안 시작 알림, 계좌 중지, 5분씩 시계를 58번 움직인 반복이 만든 60개 메시지 가운데 50개가 보관되고 10개가 버려져 status.alerts에 queued 50, dropped_total 10으로 보인다(failures와 failing_since_ms도 보이고, 토큰은 없다). 텔레그램이 다시 200으로 답하면 남은 50개가 나간 뒤 '[OMS P1] 전송 실패로 알림 10개를 버림' 하나가 나가고, 전달 51, 대기 0, failing_since_ms null이 된다

- 상태: **미실행**
- 왜 중요한가: 긴 텔레그램 장애 동안 메모리를 묶고, 잃은 알림이 있다는 사실은 알려야 한다(PR #3의 보관함 규칙, 설계 §7.7)
- 수준: 통합
- 전제와 범위: 프로세스 안 시험 1개(실제 파이프라인, 알림 스레드, alert-direct, loopback http-fake). 시계는 시험이 움직인다(5분 × 58), flush 주기 20 ms
- 실제 구성요소: oms-alert 스레드의 반복 호출, alert-direct 보관함과 버림 알림, alerts::status_json
- 모의·시뮬레이션: 텔레그램(http-fake 500 다음 200), 시계
- 주입한 장애: 텔레그램 500이 계속
- 판정 기준과 근거: status.alerts의 queued, dropped_total, active, failures, failing_since_ms, second, source. 회복 뒤 마지막 51개 요청의 본문(50개는 중지 문장, 마지막은 버림 알림)
- 재현: `cargo test -p oms-node --test alerts outbox_bound`
- 뒷받침하는 것: C-ALERT의 '중복·순서·시간 초과·중단', alert-direct 보관함 규칙
- 확인하지 않은 범위: 실제 텔레그램의 429·5xx 응답 모양. 프로세스를 넘는 보관(저장하지 않는다). 스펙 예시는 '중지 1 + 반복 59 = 60'이지만 시작 알림도 보관함에 들어가므로 시험은 '시작 1 + 중지 1 + 반복 58 = 60'으로 셌다

| 시험 | 결과 | 비고 |
|---|---|---|
| `node-unit/outbox_bound_holds_and_drops_are_counted` | 미실행 |  |

## V-ALERT-3 — 알림 조건은 C-ALERT 표의 키와 유예를 따른다: 차단 사유마다 비어 있지 않은 문장(Mismatch와 OperatorStrategy는 조건 없음), 래치와 운영자 행위는 유예 0, 지나가는 상태와 저장 공간 부족은 grace_ms. Core::handle로 만든 상태에서 hold:link_down, gateway_block, hold:resync, hold:operator_account, hold:storage_low, halt_strategy:<s>, mismatch:<kind>, hold:venue_setting:<i>, hold:account_mode, margin_stale, hold:reducing이 나오고 문장 끝은 '근거: 저널 seq n'이다. 불일치는 종류마다 키 하나에 개수와 최근 3개(subject: detail)를 담고 확인한 것은 빠진다. 계좌 중지는 시작 1번, +4분 0번, +5분에 그때의 메모로 1번, +10분 1번, 재개에 '해소: <시작 문장>' 1번, 그 뒤 +5분 0번이다. 연결 끊김은 20초 만에 다시 붙으면 아무것도 보내지 않고, 29.999초에는 아니고 정확히 30초에 시작을, 31초에는 더 보내지 않고, 다시 붙으면 해소를 보낸다. 저장 공간 부족이 20초 낮음·5초 정상을 열 번 되풀이하면 아무것도 보내지 않고, 30초를 넘게 계속되면 시작 1번(29.999초에는 아니고 30초에, 35초에 더 없음), 풀리면 해소 1번이다

- 상태: **미실행**
- 왜 중요한가: 알림이 소음이 되지 않고(지나가는 상태), 중요한 것은 바로 알리며(래치), 새 차단 사유가 알림 없이 들어오지 못해야 한다(와일드카드 없는 match, ADR-029)
- 수준: 통합
- 전제와 범위: 프로세스 안 시험 4개(실제 파이프라인, 알림 스레드, loopback http-fake, 시험이 움직이는 시계), 단위 시험 3개(Core::handle로 만든 코어와 직접 넣은 불일치)
- 실제 구성요소: alerts::conditions와 hold_condition, oms-alert 스레드의 유예, alert-direct 반복 규칙, oms-core의 holds
- 모의·시뮬레이션: 텔레그램(http-fake 200), 시계, 거래소 입력(시험이 넣는 Link·FillsSynced·OpenOrders·Positions·UnclassifiedExecution), 저장 공간 측정(시험이 넣는 Storage)
- 주입한 장애: 짧은 연결 끊김(20초), 유예 경계(29.999초, 30초, 31초), 반복 주기 안의 새 메모, 문턱 근처에서 들락거리는 저장 공간
- 판정 기준과 근거: C-ALERT 조건 표. 가짜 텔레그램이 받은 메시지의 수와 내용. 결함(유예 검사 없음, 유예 > , 불일치 키가 종류별이 아님, 감소 전용에 유예, 저장 공간 부족에 유예 0, 게이트웨이 막힘 조건 없음)을 넣어 각각 잡는 것을 확인했다(M123 포함)
- 재현: `cargo test -p oms-node --test alerts; cargo test -p oms-node --lib alerts::`
- 뒷받침하는 것: C-ALERT의 '조건 표', ADR-029 §7
- 확인하지 않은 범위: 결과 불명·거래소 설정·계좌 모드·증거금 신선도의 시작이 프로세스에서 나가는 것(단위 시험의 키와 유예만). 실제 거래소에서 생기는 불일치. 이미 알린 종류의 새 불일치가 다음 반복까지 기다리는 것(C-ALERT 한계, 리뷰 N3). 실제 디스크에서 측정한 저장 공간. 문장의 한국어가 운영자에게 충분한지(판단)

| 시험 | 결과 | 비고 |
|---|---|---|
| `node-unit/repeat_rule_with_an_injected_clock` | 미실행 |  |
| `node-unit/grace_hides_a_short_link_down_and_reports_a_long_one` | 미실행 |  |
| `node-unit/a_storage_low_that_flaps_inside_the_grace_sends_nothing` | 미실행 |  |
| `node-unit/a_storage_low_that_lasts_past_the_grace_sends_one_start` | 미실행 |  |
| `node-unit/alerts::tests::every_hold_reason_has_a_text_and_the_grace_of_the_table` | 미실행 |  |
| `node-unit/alerts::tests::conditions_give_the_keys_of_the_table` | 미실행 |  |
| `node-unit/alerts::tests::a_mismatch_condition_per_kind_lists_the_count_and_the_newest_three` | 미실행 |  |

## V-ALERT-4 — OMS 알림에 둘째 채널을 주면(alerts::start) 모든 메시지의 사본이 바로 그쪽에 가고, 텔레그램이 500으로 답한 첫 시도부터 59초에는 장애 알림이 없고 61초가 되면 1초 안에 'Telegram 전송이 61초째 실패함'이 둘째 채널에만 가며(텔레그램 요청에는 없음), 텔레그램이 다시 답하면 해소가 간다. 텔레그램이 답하지 않아도(연결은 받고 답 없음) 계좌 중지·재개 1,000건이 5초 안에 내구화되고 알림 스레드는 계속 평가한다

- 상태: **미실행**
- 왜 중요한가: 텔레그램이 막혔을 때 그것을 알릴 다른 길(설계 §7.7, D3), 그리고 알림이 주문 경로를 늦추지 않아야 한다(ADR-029 §2)
- 수준: 통합
- 전제와 범위: 프로세스 안 시험 2개(실제 파이프라인, 알림 스레드, alert-direct, loopback http-fake, 시험 안의 둘째 채널, 시험이 움직이는 시계). 1,000건의 5초는 판단(모델 아님)
- 실제 구성요소: oms-alert 스레드, alert-direct의 둘째 채널 스레드와 장애 알림, 커밋 단계와 저널 flush
- 모의·시뮬레이션: 텔레그램(http-fake 500, 200, 무응답), 둘째 채널(시험의 구현), 시계
- 주입한 장애: 텔레그램 500이 계속, 텔레그램 무응답(Hang, 요청 시간 10초)
- 판정 기준과 근거: 둘째 채널이 받은 문구와 순서, 61초 뒤 1초 안, 텔레그램 요청 본문에 장애 알림 없음, status.alerts.second, 1,000 기록의 내구화 시간과 평가 수
- 재현: `cargo test -p oms-node --test alerts -- telegram_ a_hung`
- 뒷받침하는 것: C-ALERT, ADR-029 §7, U18a1(V-ALERT-DIRECT-1)
- 확인하지 않은 범위: 실제 둘째 채널 서비스(D3, 정하지 않음; 설정은 second_channel = "none"만 받는다). 실제 텔레그램의 연결 3초·요청 10초에서의 탐지 시간. 측정한 기계의 디스크 밖에서의 1,000건 시간

| 시험 | 결과 | 비고 |
|---|---|---|
| `node-unit/telegram_failing_over_60s_notifies_the_second_channel` | 미실행 |  |
| `node-unit/a_hung_telegram_never_slows_the_commit_stage` | 미실행 |  |

## V-ALERT-5 — [alert] 설정은 절대 경로의 토큰 파일을 요구하고 그 파일을 설정을 읽을 때 alert-direct 규칙으로 읽는다: 0640, 심볼릭 링크, 저장소 안, 없는 파일은 '[alert] telegram: '로 시작하는 설정 오류이고 그 문장과 설정의 Debug에 토큰이 없다. 각 값(grace_ms, eval_ms, repeat_ms, exit_flush_ms, second_after_ms)은 양 끝에서 받고 바로 바깥에서 거부하며, second_channel은 "none"만 받는다. 표가 없으면 알림이 없고 status.alerts는 {"configured": false}다. mainnet(Hyperliquid, Bybit)은 표 없이 시작하지 않고, testnet과 가상 거래소에서는 선택이다. OMS_TEST_TELEGRAM_API는 loopback http만 받고, 배포하는 바이너리(failpoints 없음)는 그 변수를 무시해 저장소 안의 토큰 파일을 여전히 거부하고 종료 코드 2로 끝난다. 시작 알림은 그 평가에서 시작이 나간 조건과 유예 안의 조건을 따로 센다: 계좌 중지와 연결 끊김이면 '활성 조건 1개(유예 중 1개)'다. 프로세스에서: 시작 알림 'OMS 시작 (boot 1, config 1)'이 나가고, SIGTERM으로 정상 종료하면 'OMS 정상 종료. 걸린 주문 1개는 거래소에 남음'이 종료 전에 나간다. 설정이 활성화된 뒤 거래 소켓을 bind하지 못하면 종료 코드 3으로 끝나고 'OMS 시작 실패(exit 3): trading_socket' 하나가 종료 전에 나가며, 어떤 요청에도 소켓 경로·토큰·토큰 파일 이름이 없다

- 상태: **미실행**
- 왜 중요한가: 토큰이 저장소나 로그로 새지 않고, 실거래는 알림 없이 하지 않으며(소유자 질문 1의 기본값), 시험용 훅이 배포 바이너리에 없어야 한다(ADR-029)
- 수준: 통합
- 전제와 범위: 설정 단위 시험 2개, 설정 파일을 디스크에서 읽는 시험 1개, oms-node 실행 파일 시험 1개(failpoints 없는 빌드에서만), 프로세스 안 시험 1개(시작 알림의 수), 실제 oms-node·simvenue 프로세스 시험 2개(failpoints 빌드, loopback http-fake)
- 실제 구성요소: NodeConfig::load와 validate, alert_direct::TelegramConfig::load_with, oms-node 실행 파일, node::run의 시작과 종료 경로
- 모의·시뮬레이션: 텔레그램(http-fake), 거래소(simvenue), 저장소(시험이 만든 .git 디렉터리)
- 주입한 장애: 상대 경로, 0640, 심볼릭 링크, 저장소 안, 없는 파일, 범위 밖 값, 모르는 키, second_channel = pushover, mainnet에 표 없음, loopback이 아닌 시험 주소, 없는 디렉터리 안의 거래 소켓(bind 실패)
- 판정 기준과 근거: 오류 문장, Debug와 status에 토큰 없음, 종료 코드, 가짜 텔레그램이 받은 시작·정지 알림, 로그의 'alerts delivered before the exit'. 결함(mainnet 통과, 배포 바이너리가 시험 훅을 읽음, 상대 경로 통과, 정지 알림 없음, 활성 뒤 코드 3에 시작 실패 알림 없음)을 넣어 각각 잡는 것을 확인했다
- 재현: `cargo test -p oms-node --lib config::; cargo test -p oms-node --test alerts the_; cargo test -p oms-node --features failpoints --test crash_points -- a_clean_stop an_exit_3`
- 뒷받침하는 것: C-ALERT의 '설정'과 '종료', ADR-029 §7, U-11
- 확인하지 않은 범위: 실제 텔레그램으로의 시작·정지 알림. Hyperliquid·Bybit 노드 프로세스의 알림(가상 거래소 노드만). 종료 전 전송이 exit_flush_ms를 다 쓰는 경우(시험은 바로 또는 5초 재시도 한 번에 끝난다). 시험 디렉터리가 저장소 안이면 좋은 토큰 파일을 읽는 확인은 그 규칙의 거부를 확인하는 것으로 바뀐다. 설정이 활성화되기 전의 종료 코드 3이 아무것도 보내지 않는 것(코드만, 시험 없음). 게이트웨이 시작·관리 소켓·신호 처리기 실패의 시작 실패 알림(거래 소켓 하나로만 시험). 토큰 파일 경로가 설정 Debug와 오류 문장에 보이는 것(C-ALERT 한계, 리뷰 N6)

| 시험 | 결과 | 비고 |
|---|---|---|
| `node-unit/config::tests::the_alert_table_is_validated_and_required_on_mainnet` | 미실행 |  |
| `node-unit/config::tests::the_test_telegram_api_must_be_loopback_http` | 미실행 |  |
| `node-unit/the_token_file_is_loaded_with_its_rules` | 미실행 |  |
| `node-unit/the_shipped_binary_ignores_the_test_telegram_api` | 미실행 |  |
| `node-unit/the_start_notice_counts_sent_and_pending_conditions_apart` | 미실행 |  |
| `pipeline/a_clean_stop_sends_the_stop_notice_with_the_open_orders` | 미실행 |  |
| `pipeline/an_exit_3_after_activation_sends_one_start_failure_notice` | 미실행 |  |

## V-HOLD-1 — 증거금 정책이 있으면 워처 불일치와 거래소 변경 아래에서 감소 전용 주문은 통과하고 일반 주문은 거절되며, 킬 래치와 운영자 중지 아래에서는 둘 다 거절된다(TN 표 7-7). 정책이 없으면 모든 보류가 모든 주문을 거절한다. 새 사유 셋은 기존 사유 뒤에 붙어 기존 사유의 순서가 바뀌지 않는다. 프로세스에서: 운영 소켓의 watcher_mismatch 뒤 일반 주문은 Hold(WatcherMismatch)로 거절되고 감소 전용 주문은 받으며, watcher_kill 뒤에는 감소 전용도 Hold(KillLatched)로 거절된다. 워처 입력의 detail은 비지 않고 1 KiB(1024바이트) 이하여야 한다: 노드와 코어가 각각 거절하고 아무것도 기록하지 않는다

- 상태: **미실행**
- 왜 중요한가: 킬 루프는 미체결 0건을 확인해야 끝나므로 어떤 새 주문도 받으면 안 되고, 불일치나 거래소 변경에서는 포지션을 줄이는 길을 막으면 안 된다(설계 R5, Q22)
- 수준: 단위, 통합
- 전제와 범위: 보류 사유마다 한 줄(증거금 정책 있음, 신선한 보고, 롱 2), 정책 없음 한 경우, 사유의 순서. 실제 oms-node·simvenue 프로세스 하나(증거금 정책)
- 실제 구성요소: oms-core, oms-node 프로세스(운영 소켓, 코어 스레드, 커밋 단계, 저널), simvenue 프로세스
- 모의·시뮬레이션: 워처의 입력(시험이 운영 소켓으로 보냄), 거래소(simvenue)
- 주입한 장애: 같은 입력 두 번, 빈 detail, 1025바이트 detail, 거래 소켓에서의 워처 입력, 모르는 검사 이름
- 판정 기준과 근거: 거절 사유 목록에 그 Hold가 있는지, 감소 전용 주문의 접수 여부, holds의 정확한 목록, 저널의 hold_set 개수. 결함(KillLatched가 일반 주문만 막음)을 넣어 잡는 것을 확인했다(M125)
- 재현: `cargo test -p oms-core --test holds; cargo test -p oms-node --test holds_node watcher`
- 뒷받침하는 것: C-HOLD의 보류 표와 워처 입력, ADR-030 §7, ADR-008 §7
- 확인하지 않은 범위: 실제 거래소. 감시 없음(U18b4)과 전략 연결(U18b3)의 줄. 거래소가 감소 전용 주문의 포지션 확대를 실제로 막는지(T5)

| 시험 | 결과 | 비고 |
|---|---|---|
| `core/the_hold_table_of_table_7_7` | 미실행 |  |
| `core/the_new_holds_come_last_in_their_order` | 미실행 |  |
| `core/a_watcher_input_without_a_detail_is_refused` | 미실행 |  |
| `node-unit/watcher_mismatch_over_the_admin_socket_refuses_plain_orders_only` | 미실행 |  |

## V-HOLD-2 — clear_hold는 고정 코드로 거절한다: 걸려 있지 않음(not_held), 메모 없음(note_required), 이 프로세스의 킬 루프가 답하지 않음(kill_unconfirmed), 코어의 장부에 미체결이 있음(kill_orders_open, 킬 루프의 답이 confirmed든 unsupported든, kill_unconfirmed 다음에 본다), 거래소 변경 뒤 포지션을 아직 읽지 않음(venue_change_pending). 거절은 기록도 명령도 남기지 않는다. 래치 아래에서도 cancel-all은 받고, 거래소가 취소를 확인해 장부가 비면 킬의 해제를 받는다(코어와 가상 거래소 프로세스). 받으면 hold_cleared 하나와, 킬이면 KillStop 하나. 거래소 변경의 해제와 불일치 확인은 따로다. resume은 킬과 워처 불일치를 풀지 않는다. omsctl clear-hold는 세 사유와 메모를 읽는다

- 상태: **미실행**
- 왜 중요한가: 사람만 풀어야 하는 보류가 다른 명령이나 확인 없는 해제로 풀리면 킬 스위치가 뜻을 잃는다(ADR-030, 소유자 질문 2)
- 수준: 단위
- 전제와 범위: 코드마다 한 경우와 받는 경우, 거래소 변경의 두 단계, 운영자 중지와 함께 걸린 킬, 열린 주문 둘과 두 가지 답(confirmed, unsupported), 실제 oms-node·simvenue 프로세스에서 걸린 주문 둘과 cancel-all, omsctl 인자
- 실제 구성요소: oms-core, oms-api ctl, oms-node·simvenue 프로세스(가상 거래소의 unsupported 답과 취소)
- 모의·시뮬레이션: 코어 시험에서 킬 루프의 답(시험이 KillLoopEnded를 넣음), 거래소 포지션 응답, 취소 확인
- 주입한 장애: 공백뿐인 메모, 같은 해제 두 번, 운영자 중지가 없을 때의 resume
- 판정 기준과 근거: 응답 문장의 코드(kill_orders_open은 문장 전체와 개수), 거절 뒤 기록 수와 명령 수가 그대로인지, cancel-all의 응답과 취소 요청 수, 받은 뒤의 이벤트 이름과 명령, holds 목록, status의 open_orders. 넣은 결함과 잡은 시험: kill_unconfirmed 검사 없음(M126, clear_hold_refusals), kill_orders_open 검사 없음(M131, a_kill_is_not_cleared_while_the_book_has_open_orders), resume이 킬도 풂(M134, resume_leaves_kill_and_watcher_mismatch), 메모 검사 없음(M139, clear_hold_refusals)
- 재현: `cargo test -p oms-core --test holds clear_hold resume a_kill_is_not; cargo test -p oms-node --test holds_node watcher; cargo test -p oms-api`
- 뒷받침하는 것: C-HOLD의 clear_hold, ADR-030 §7
- 확인하지 않은 범위: omsctl 실행 파일 자체(본문 함수만, V-HOLD-4의 프로세스 시험에서도 함수로 부른다). 거래소에만 있고 장부에 없는 주문(kill_orders_open은 장부만 센다, C-HOLD 알려진 한계). Hyperliquid·Bybit 노드 프로세스

| 시험 | 결과 | 비고 |
|---|---|---|
| `core/clear_hold_refusals` | 미실행 |  |
| `core/a_kill_is_not_cleared_while_the_book_has_open_orders` | 미실행 |  |
| `node-unit/watcher_mismatch_over_the_admin_socket_refuses_plain_orders_only` | 미실행 |  |
| `core/resume_leaves_kill_and_watcher_mismatch` | 미실행 |  |
| `api/ctl::tests::clear_hold_parses_its_reason_and_note` | 미실행 |  |
| `api/tests::wire_examples_of_the_holds` | 미실행 |  |

## V-HOLD-3 — 킬 래치는 hold_set에서, 그 확인은 kill_loop_ended에서 재실행으로 같은 상태가 되고, 모든 boot가 확인을 지우고 래치의 필드 그대로 Kill 명령을 다시 요청한다. 래치가 없거나 이미 확인된 뒤의 KillLoopEnded는 기록되지 않는다. 모든 게이트웨이 명령은 기록된 이벤트에 command_for를 적용한 것과 같다. 커밋 단계는 Kill을 래치가 내구화된 뒤에만 넘기고, flush가 실패하면 Kill도 응답도 나가지 않으며 다시 시작한 노드에 래치가 없다

- 상태: **미실행**
- 왜 중요한가: 킬 루프의 일괄 취소는 래치가 디스크에 있은 뒤에만 나가야 하고(ADR-004), 앞 프로세스의 확인은 새 프로세스가 모르는 주문을 덮지 않는다(소유자 질문 5)
- 수준: 단위, 통합
- 전제와 범위: 코어: 래치, 확인, 재시작, unsupported 확인과 해제, 해제 뒤 재시작. 프로세스 안: 실제 코어 스레드·커밋 스레드·저널 파일로 정상과 flush 실패
- 실제 구성요소: oms-core, oms-node 파이프라인(코어 스레드, 커밋 스레드, 저널 쓰기와 flush), 복구
- 모의·시뮬레이션: 게이트웨이(명령 채널만 읽음), 저장 장치의 flush 실패(failpoints)
- 주입한 장애: 재시작, 두 번째 루프 답, flush 실패
- 판정 기준과 근거: 명령 목록의 정확한 값, confirmed_at·outcome, 재실행한 상태 = 라이브 상태, 명령을 받은 순간 읽기 모델과 durable_seq, 복구한 상태의 kill. 넣은 결함과 잡은 시험: boot가 확인을 남김(M127), 같은 프로세스의 두 번째 답도 기록(M138, 둘 다 the_kill_latch_and_its_confirmation_…), 커밋 단계가 Kill을 flush 전에 넘김(M130, a_kill_command_leaves_only_after_the_latch_is_durable)
- 재현: `cargo test -p oms-core --test holds; cargo test -p oms-node --features failpoints --test pipeline a_kill_command`
- 뒷받침하는 것: C-HOLD의 킬 명령, C-COMMIT, ADR-030 §7
- 확인하지 않은 범위: Bybit의 실제 킬 루프(U18b2, C-KILL). 게이트웨이 프로세스가 명령을 받은 뒤의 일(가상 거래소의 unsupported 답은 V-HOLD-4)

| 시험 | 결과 | 비고 |
|---|---|---|
| `core/the_kill_latch_and_its_confirmation_survive_replay_and_a_boot_asks_for_a_new_loop` | 미실행 |  |
| `core/every_command_comes_from_command_for_on_a_recorded_event` | 미실행 |  |
| `pipeline/a_kill_command_leaves_only_after_the_latch_is_durable` | 미실행 |  |

## V-HOLD-4 — 킬 마커가 있으면 OMS는 시작할 때 boot 기록 안에서 KillLatched를 걸고(booted, hold_set), 감소 전용을 포함한 어떤 주문도 받지 않으며, 가상 거래소가 unsupported로 답한 뒤 clear_hold kill이 마커를 지운다. 거절된 해제(빈 메모, 다른 사유, not_held, 답을 붙잡아 둔 동안 메모가 있는 kill_unconfirmed)는 마커를 건드리지 않고, 해제 기록의 flush가 실패하면 응답이 없고 노드가 멈추며 마커가 남는다. 그 둘 뒤의 다음 시작은 다시 걸린다. 다음 시작은 걸리지 않고 주문을 받는다. 지우지 못한 마커는 응답이 말하고 다음 시작이 다시 건다. 볼 수 없는 마커 경로(권한)도 건다. 마커 파일은 처음 512바이트를 제어 문자를 바꿔 읽고, 디렉터리·심볼릭 링크도 마커다

- 상태: **미실행**
- 왜 중요한가: 워처가 킬한 사실은 OMS 재시작을 넘어 남아야 하고(U-06), 확인할 수 없는 경로는 킬하지 않았다는 증거가 아니다. 마커를 해제 전에 지우면 래치를 잃을 수 있다
- 수준: 통합, 단위
- 전제와 범위: 실제 oms-node·simvenue 프로세스 다섯 개(증거금 정책 하나, failpoints 둘), 재시작 네 번, 권한을 바꾼 디렉터리. 단위: 파일 종류와 내용, 권한 거부, 디렉터리가 아닌 경로
- 실제 구성요소: oms-node 프로세스(시작 순서, 운영 소켓, 코어, 커밋 단계, 저널, 마커 읽기와 지우기), simvenue, omsctl 본문 함수
- 모의·시뮬레이션: 워처(시험이 마커 파일을 씀), 거래소(simvenue, 일괄 취소 없음), 게이트웨이 답의 지연(OMS_HOLD_KILL_ANSWER), 해제 기록의 flush 실패(OMS_CRASH_ON=clear_hold, sync_error_after_write)
- 주입한 장애: 읽을 수 없는 디렉터리(0000), 지울 수 없는 디렉터리(0500), 빈 메모의 해제, 두 번째 해제, 확인 전의 해제, 해제 기록의 flush 실패, 거래 소켓의 해제, 재시작
- 판정 기준과 근거: status의 core.kill(출처, 메모, confirmed_at, outcome)과 node.kill_marker, 저널의 boot 기록 이벤트와 kill_loop_ended의 순서, 거절 사유, 마커 파일의 존재, 응답 문장, 종료 코드 2, 저널의 hold_set·hold_cleared 개수. 넣은 결함과 잡은 시험: stat 오류를 마커 없음으로 읽음(M128, node::tests::a_marker_that_cannot_be_examined_latches), 해제를 코어에 넘기기 전에 늘 마커를 지움(M129, 빈 메모 해제 뒤의 마커 검사), 메모가 있을 때만 코어가 결정하기 전에 지움(M132, kill_unconfirmed 거절 시험. 리뷰가 찾은, 앞의 시험을 모두 통과하던 결함), M129와 같은 결함을 flush 실패 시험만으로(M133)
- 재현: `cargo test -p oms-node --test holds_node; cargo test -p oms-node --features failpoints --test crash_points a_clear_; cargo test -p oms-node --lib node::tests; cargo test -p oms-core --test holds marker`
- 뒷받침하는 것: C-HOLD의 킬 마커, ADR-030 §7, U-06
- 확인하지 않은 범위: root로 실행한 시험(권한 시험 셋은 건너뛴다). 해제가 내구화된 뒤 마커를 지우기 전의 중단(그 사이의 중단 시험은 없다. 내구화 전에 지우지 않는다는 것은 flush 실패 시험과 M133이 보인다). 실행 중 마커(범위 밖). Bybit·Hyperliquid 노드 프로세스

| 시험 | 결과 | 비고 |
|---|---|---|
| `core/the_kill_marker_latches_in_the_boot_record` | 미실행 |  |
| `core/a_second_trigger_keeps_the_first_note` | 미실행 |  |
| `node-unit/a_kill_marker_at_boot_refuses_every_order_until_clear_hold_kill_removes_it` | 미실행 |  |
| `pipeline/a_clear_refused_kill_unconfirmed_leaves_the_marker_and_the_next_start_latches` | 미실행 |  |
| `pipeline/a_clear_whose_record_fails_to_flush_leaves_the_marker_and_the_next_start_latches` | 미실행 |  |
| `node-unit/an_unremovable_marker_is_reported_and_latches_the_next_boot` | 미실행 |  |
| `node-unit/a_marker_path_that_cannot_be_examined_latches` | 미실행 |  |
| `node-unit/node::tests::the_kill_marker_is_whatever_is_at_its_path` | 미실행 |  |
| `node-unit/node::tests::a_marker_that_cannot_be_examined_latches` | 미실행 |  |

## V-HOLD-5 — 거래소 변경 보류는 새 이벤트 없이 venue_position_changed, unclassified_execution_noted, 종류가 venue_position_change인 mismatch_raised(넘침 경로)에서 apply가 건다. 그래서 U13b의 이벤트 목록은 그대로이고, U18b 전에 쓴 저널에 이 셋 중 하나가 있으면 재실행한 상태에 보류가 있다. 다른 종류의 불일치는 걸지 않는다. 불일치를 확인해도 보류는 남는다. 각 보류는 한 번만 걸리고 두 번째 원인은 첫 메모를 바꾸지 않는다

- 상태: **미실행**
- 왜 중요한가: 불일치 확인 한 번으로 바뀐 포지션을 보지 않고 일반 주문이 흐르면 안 된다. 보류가 결정에 있으면 옛 저널의 재실행이 그것을 잃는다
- 수준: 단위
- 전제와 범위: 세 경로, 손으로 만든 옛 기록 세 개와 다른 종류 하나, 각 보류의 두 번째 원인(워처 킬, 시작 마커, 워처 불일치, 거래소 변경)
- 실제 구성요소: oms-core(apply, replay)
- 모의·시뮬레이션: 거래소의 실행 통보(시험이 입력)
- 주입한 장애: 포지션 넘침, 옛 저널, 같은 원인 두 번
- 판정 기준과 근거: 이벤트 이름 목록, holds, venue_change의 since와 메모, 재실행 상태. 넣은 결함과 잡은 시험: 넘침 경로에서 안 걸림(M135, a_journal_with_a_venue_change_latches_on_replay), 두 번째 킬 원인이 덮어씀(M136), 두 번째 거래소 변경이 덮어씀(M137, 둘 다 a_second_trigger_keeps_the_first_note)
- 재현: `cargo test -p oms-core --test holds venue second`
- 뒷받침하는 것: C-HOLD의 보류 표(venue_change), ADR-030 §7, ADR-023
- 확인하지 않은 범위: 실제 거래소의 ADL·인수 통보. 거래소 변경을 apply 대신 결정에서 거는 결함(여러 파일이라 xtask mutants에 없다. 재실행 시험이 보는 성질이지만 넣어 본 결과는 근거로 들지 않는다). 넘침 경로의 두 번째 변경(C-HOLD 알려진 한계)

| 시험 | 결과 | 비고 |
|---|---|---|
| `core/a_venue_change_latches_without_a_new_event` | 미실행 |  |
| `core/a_journal_with_a_venue_change_latches_on_replay` | 미실행 |  |
| `core/a_second_trigger_keeps_the_first_note` | 미실행 |  |

## V-HOLD-6 — 킬 래치·워처 불일치·거래소 변경의 상태 필드, Boot의 kill_marker, 워처 입력과 새 이벤트 셋은 기록 스키마를 올리지 않는다: 비어 있으면 쓰지 않아 옛 상태의 바이트가 그대로이고, 이들이 든 저널은 라이브 상태로 복구되며 verify --re-decide를 통과한다. 상태 스키마는 2로 올랐다: U18b 전의 스냅샷(상태 스키마 1, venue_change 없음)은 사유와 함께 건너뛰고 저널을 처음부터 재생해, 거래소 변경 보류가 스냅샷의 유무에 따라 달라지지 않는다. 이벤트 필드 표는 새 이벤트의 모든 필드를 적는다. 알림 조건 hold:kill_latched·hold:watcher_mismatch·hold:venue_change는 유예 0이고, 킬 문장은 출처·메모·확인 상태를 담으며, 워처 불일치와 거래소 변경의 문장은 증거금 정책이 있을 때만 감소 전용을 받는다고 말하며, Alerts::notice는 접두어를 붙여 한 번 보낸다. [kill] marker는 절대 경로, 데이터 디렉터리 밖, 읽을 때 디렉터리가 있어야 하고, mainnet은 그것 없이 시작하지 않는다

- 상태: **미실행**
- 왜 중요한가: 기존 저널은 읽히고 기존 시험의 기대값이 바뀌지 않아야 하며(스펙 완료 기준), 복구한 보류가 스냅샷의 유무에 따라 달라지면 안 되고(리뷰 S3), 워처(U19a)는 필드 표만으로 만든다. 래치는 사람에게 곧바로 알려야 한다(C-ALERT)
- 수준: 단위, 통합
- 전제와 범위: 저널 하나를 쓰고 복구·재결정, 필드 표 대조, 알림 조건 표와 loopback 가짜 텔레그램, 설정 검증
- 실제 구성요소: oms-node 복구와 verify, 알림 스레드와 alert-direct(loopback HTTP), 설정 읽기
- 모의·시뮬레이션: 텔레그램(http-fake), 거래소 입력(시험이 넣음)
- 주입한 장애: 옛 상태, 상대 경로, 데이터 디렉터리 안의 마커, 없는 디렉터리, mainnet에 [kill] 없음
- 판정 기준과 근거: RECORD_SCHEMA = 2, STATE_SCHEMA = 2, 다이제스트 같음, 재결정 통과, 직렬화에 필드 없음, 건너뛴 스냅샷의 사유와 재생한 기록 수, 필드 표의 행, 알림 문장의 부분과 유예, 설정 오류 문장. 넣은 결함과 잡은 시험: 킬 알림이 지나가는 상태의 유예를 기다림(M140, alerts::tests::every_hold_reason_has_a_text_and_the_grace_of_the_table)
- 재현: `cargo test -p oms-node --test record_schema --test alerts; cargo test -p oms-node --lib alerts:: config::; cargo test -p oms-core --test event_fields`
- 뒷받침하는 것: C-HOLD의 상태와 알림, C-TAIL 필드 표, C-ALERT 조건 표, ADR-030 §7
- 확인하지 않은 범위: 실제 텔레그램. 실행 파일 단위의 mainnet 시작 거부(설정 검증 함수만)

| 시험 | 결과 | 비고 |
|---|---|---|
| `node-unit/kill_holds_liveness_and_heartbeats_recover_to_the_live_state` | 미실행 |  |
| `node-unit/a_snapshot_of_state_schema_1_is_skipped_and_the_journal_replays_the_venue_change` | 미실행 |  |
| `node-unit/alerts::tests::the_plain_only_hold_texts_follow_the_margin_policy` | 미실행 |  |
| `core/the_field_table_lists_every_field_the_encoder_writes` | 미실행 |  |
| `node-unit/alerts::tests::every_hold_reason_has_a_text_and_the_grace_of_the_table` | 미실행 |  |
| `node-unit/a_kill_latch_is_told_at_once_and_a_notice_goes_out_once` | 미실행 |  |
| `node-unit/config::tests::the_kill_marker_is_validated_and_required_on_mainnet` | 미실행 |  |

## V-KILL-1 — Bybit 게이트웨이는 개인 스트림의 마지막 생존 증거(구독 확인, pong, 시작 확인의 첫 통과) 뒤 kill_after_ms가 지나도록 연결이 내려가 있으면 끊김 하나에 한 번 StreamLost를 보내고, 코어는 래치가 없으면 hold_set{kill, source: stream}과 Kill 명령을 한 기록으로 낸다. 마지막 pong 전에는 걸리지 않고, ping에 답하는 조용한 스트림, 기한 전에 다시 구독된 연결, 시작 확인 전의 끊김, 코어 큐를 기다린 시간은 킬을 부르지 않는다. 프로세스에서: 반쯤 열린 개인 스트림(ping 1초, kill_after 10초)은 마지막 pong 9초 뒤에는 디스크에 hold_set이 없고 11초까지 하나 있으며, 가짜 거래소의 첫 일괄 취소는 그 기록이 이미 저널 파일에 있을 때 온다. 다시 시작해도 래치가 남고, 새 프로세스의 루프가 끝나기 전 clear-hold는 kill_unconfirmed로 거절되며 끝난 뒤 받아들여지고 일반 주문이 다시 받아진다

- 상태: **미실행**
- 왜 중요한가: Bybit에는 거래소 쪽 dead-man이 없어 OMS가 체결·주문 통보를 잃은 채 주문을 걸어 둘 수 있다. 확인하지 않은 계좌로 일괄 취소를 보내면 안 되고(시작 확인 전 무장 금지), 느린 코어를 끊김으로 읽으면 안 된다
- 수준: 단위, 통합
- 전제와 범위: 게이트웨이 시험 다섯(가짜 WS·http-fake, 실제 짧은 tokio 시간), 코어 시험 하나, 실제 oms-node 프로세스 하나(가짜 Bybit WS·REST, 재시작 한 번)
- 실제 구성요소: Bybit 게이트웨이(스트림 루프, health_poll 감시, 시작 확인), oms-core, oms-node 프로세스(커밋 단계, 저널, 운영 소켓)
- 모의·시뮬레이션: Bybit 개인·공개 스트림(가짜 WS 서버), REST(http-fake), 끊김의 모양(시험이 pong을 멈추거나 소켓을 닫음)
- 주입한 장애: 반쯤 열린 연결, 닫힌 소켓, 기한 전 재연결, 시작 확인 실패, 코어 큐가 가득 참, 루프 도중 프로세스 종료
- 판정 기준과 근거: StreamLost가 나온 시각(마지막 pong 기준 창), 나오지 않음, hold_set의 출처와 메모, 가짜 거래소가 첫 일괄 취소를 받는 순간 읽은 저널 파일의 킬 이벤트(http-fake on_request), 재시작 뒤 holds, clear-hold의 응답. 넣은 결함과 잡은 시험: 연결이 내려간 때부터 잼(M141), 큐 대기 보정 없음(M142), 시작 확인 전 무장(M143), 끊김을 무시하는 코어(저장소 밖 mut.py stream-lost-ignored)
- 재현: `cargo test -p oms-node --lib venue::bybit::gateway::tests::; cargo test -p oms-core --test holds stream; cargo test -p oms-node --features failpoints --test bybit_node a_ten_second`
- 뒷받침하는 것: C-KILL의 트리거(K1~K4), ADR-030 §7(U18b2), ADR-028
- 확인하지 않은 범위: 실제 Bybit 스트림. 첫 일괄 취소와 fsync의 순서: 이 시험은 파일에 쓰인 기록만 본다(쓰기와 fsync 사이에 Kill을 푸는 M130은 여기서 잡히지 않고, pipeline::a_kill_command_leaves_only_after_the_latch_is_durable가 잡는다). 구독은 되지만 pong에 한 번도 답하지 않는 연결의 되풀이(C-KILL 알려진 한계). 코어 입력 큐가 오래 막힌 동안의 실제 끊김(대기 시간만큼 킬이 늦어진다)

| 시험 | 결과 | 비고 |
|---|---|---|
| `node-unit/the_kill_fires_kill_after_the_last_pong_and_not_before` | 미실행 |  |
| `node-unit/a_quiet_stream_that_answers_pings_never_fires_the_kill` | 미실행 |  |
| `node-unit/a_reconnect_before_the_deadline_does_not_fire` | 미실행 |  |
| `node-unit/the_kill_is_not_armed_before_the_boot_checks_pass` | 미실행 |  |
| `node-unit/time_waiting_on_the_core_queue_does_not_fire_the_kill` | 미실행 |  |
| `core/a_stream_loss_latches_the_kill_and_asks_for_the_loop` | 미실행 |  |
| `pipeline/a_ten_second_stream_outage_latches_kill_through_a_restart_until_clear_hold_kill` | 미실행 |  |

## V-KILL-2 — 킬 루프는 일괄 취소 뒤 거래소의 미체결을 세어 0이 될 때까지 회차의 시작부터 kill_round_ms마다 일괄 취소를 되풀이하고, 첫 0 뒤 kill_watch_every_ms마다 kill_watch_ms 동안 다시 세어 모두 0일 때만 KillLoopEnded{confirmed}를 보낸다. 실패한 셈은 0이 아니고, 감시 중의 늦은 주문은 곧바로 회차로 돌아간다. 거래소가 첫 일괄 취소 뒤 주문 하나를 남기거나 5xx로 답해도 0을 확인할 때까지 되풀이하고, 어느 1초 창에도 일괄 취소는 둘을 넘지 않으며(비상 예산 2/1초, 회차가 100 ms여도), 생성이나 단건 취소는 하나도 나가지 않는다. 막힘(IP 차단 등)에는 요청을 보내지 않고 막힘 문장으로 회차가 실패하며, 예산을 기다리는 동안 생긴 막힘도 그 요청을 집에 둔다. 셈은 모든 쪽의 열린 행(New, PartiallyFilled, Untriggered)을 orderLinkId와 상관없이 세고, 상태를 읽을 수 없는 행, 실패한 쪽, 20쪽 넘음은 오류다. 정지(IP 차단 뒤, 키 거부) 중에는 셈을 보내지 않는다. 감시는 첫 0 + kill_watch_ms에 나간 셈으로만 끝난다(그 전에 나가 늦게 답한 셈으로는 끝나지 않는다)

- 상태: **미실행**
- 왜 중요한가: 일괄 취소는 비동기이고 한 번에 500개까지라 한 번으로는 0을 보장하지 않는다. 래치 전에 나간 생성은 6초 안에 처리되므로 30초 감시로 늦은 접수를 잡는다. 같은 UID의 워처 루프와 합쳐 거래소 한도 안에 있어야 한다
- 수준: 단위, 통합(드라이버)
- 전제와 범위: 순수 상태 기계(시간이 입력), 드라이버를 수동 시계와 http-fake(cancel-all, realtime, position/list) 위에서 100 ms 걸음으로
- 실제 구성요소: kill.rs의 KillLoop와 run 드라이버, Rest·Queries의 실제 요청 코드와 서명, RateBudget
- 모의·시뮬레이션: Bybit REST(http-fake), 시계(수동), 막힘(시험이 정함)
- 주입한 장애: 주문 하나 남음, cancel-all 5xx 두 번, 셈 실패, 감시 중 늦은 주문, 짧은 회차, 막힘, 예산 대기 중 막힘, 상태별 행과 남의 주문, 상태 없는 행, 끝없는 쪽, 정지, 늦게 답한 감시 셈
- 판정 기준과 근거: fake가 받은 POST의 루프 시각 목록, 1초 창마다의 개수, 셈 시각 목록, KillLoopEnded의 시각과 회차·호출 수. 넣은 결함과 잡은 시험: 실패한 셈을 0으로(M144), 감시 건너뜀(M145), 늦은 주문 무시(M146), 예산 안 씀(M147), 기다리지 않는 회차(M152), 막힘을 묻지 않음(M151), New만 셈(M155), 읽을 수 없는 행을 건너뜀(M156), 정지 중 셈(M157), 늦은 답으로 감시를 끝냄(M159)
- 재현: `cargo test -p oms-node --lib venue::bybit::kill::tests:: venue::bybit::gateway::tests::a_blocked venue::bybit::query::tests::the_kill_count`
- 뒷받침하는 것: C-KILL의 확인 루프(K6~K8), ADR-030 §7(U18b2)
- 확인하지 않은 범위: 실제 Bybit의 일괄 취소와 한도(문서의 한도만). 1,000건이 넘는 미체결(셈 오류 경로는 코드만). 워처 루프(U19b)와 합친 초당 2

| 시험 | 결과 | 비고 |
|---|---|---|
| `node-unit/one_order_left_after_the_first_cancel_all_repeats_until_zero_then_watches_30_s` | 미실행 |  |
| `node-unit/a_5xx_cancel_all_repeats_until_zero_and_never_exceeds_two_per_second` | 미실행 |  |
| `node-unit/the_emergency_budget_holds_two_per_second_with_a_short_round` | 미실행 |  |
| `node-unit/a_block_that_appears_during_the_budget_wait_keeps_the_cancel_all_home` | 미실행 |  |
| `node-unit/a_failed_count_is_never_zero` | 미실행 |  |
| `node-unit/the_machine_watches_30_s_after_the_first_zero` | 미실행 |  |
| `node-unit/a_late_order_in_the_watch_restarts_the_rounds` | 미실행 |  |
| `node-unit/a_blocked_gateway_sends_no_cancel_all_and_says_why` | 미실행 |  |
| `node-unit/the_kill_count_is_every_open_row_of_every_page_and_never_a_guess` | 미실행 |  |
| `node-unit/while_requests_are_paused_no_count_and_no_position_read_go_out` | 미실행 |  |
| `node-unit/a_slow_watch_check_answered_after_the_end_is_not_the_last_check` | 미실행 |  |

## V-KILL-3 — 킬 루프의 P1은 시작, 알려진 수의 바뀜, kill_notice_ms마다의 반복, 마지막(0건 확인 뒤 감시를 마침)뿐이고 모르는 수는 바뀜이 아니다. 10분 동안 셈이 실패하면 정확히 시작 1 + 반복 10 + 마지막 1 = 12개이고, 드라이버와 Alerts를 loopback 가짜 텔레그램에 붙여도 '킬 루프'가 든 요청은 12개 이하다. 모든 알림은 태그, 회차, 남은 수(모르면 이유), 일괄 취소 수와 바로 전에 읽은 포지션 묶음(심볼마다 수량, 미실현손익, 마크, 마크에서 강제청산가까지의 거리 %, 0.01 %에서 내림, 청산가가 없으면 모름, 크기 0은 뺌, 조회 실패는 실패라고)을 담는다. 알림은 루프 옆에서 순서대로 나가서, 포지션 조회가 답하지 않아도 첫 일괄 취소와 감시 중 늦은 주문의 일괄 취소는 기다리지 않는다. 느린 조회 뒤에서 기다리는 알림은 하나이고 가장 새 수의 것이 남으며, 마지막 알림은 KillLoopEnded보다 먼저 나간다. status의 notices는 보낸 알림 수다. 일괄 취소가 나갈 수 없는 동안(정지, 막힘)은 포지션을 읽지 않고 그렇다고 말한다

- 상태: **미실행**
- 왜 중요한가: 운영자는 끊긴 동안 포지션의 위험을 알아야 하지만 알림 폭주는 신호를 묻는다(TN 표 7-8 C4: 10분에 12개)
- 수준: 단위, 통합(드라이버)
- 전제와 범위: 순수 기계의 10분 실패, 바뀜·반복 규칙, 드라이버와 가짜 텔레그램, 포지션 두 심볼(청산가 빈 값 하나)과 크기 0 하나, 실패한 포지션 조회, 서식 함수
- 실제 구성요소: KillLoop, run 드라이버, Alerts와 alert-direct(loopback HTTP), position_block·liq_distance
- 모의·시뮬레이션: 텔레그램(http-fake), Bybit REST(http-fake), 시계(수동)
- 주입한 장애: 10분 동안 셈 500, 포지션 조회 실패, 답하지 않는 포지션 조회, 정지와 계정 막힘, 청산가 없음, 마크 0 이하
- 판정 기준과 근거: 알림의 시각과 종류 목록(정확히), 가짜 텔레그램이 받은 요청 수, 알림 문장의 부분 문자열, 거리 문자열. 넣은 결함과 잡은 시험: 반복이 notice_ms를 무시(M148), 모르는 수를 바뀜으로(M149), 알림이 루프를 붙잡음(M154), 막힘 중 포지션 조회(M158), 알림 큐에 상한 없음(M160), 상태가 만든 알림을 셈(M161)
- 재현: `cargo test -p oms-node --lib venue::bybit::kill::tests::`
- 뒷받침하는 것: C-KILL의 P1 알림, C-ALERT의 Alerts::notice, ADR-030 §7(U18b2)
- 확인하지 않은 범위: 실제 텔레그램. 교차 증거금에서 거래소 청산가 추정의 정확도. [alert]가 없을 때의 warn 줄(코드만)

| 시험 | 결과 | 비고 |
|---|---|---|
| `node-unit/ten_minutes_of_failed_queries_send_at_most_twelve_notices` | 미실행 |  |
| `node-unit/ten_minutes_of_failed_queries_reach_the_fake_telegram_at_most_twelve_times` | 미실행 |  |
| `node-unit/a_count_change_is_shown_and_an_unknown_count_is_not_a_change` | 미실행 |  |
| `node-unit/every_notice_lists_each_position_with_its_liquidation_distance_or_unknown` | 미실행 |  |
| `node-unit/the_liquidation_distance_is_rounded_down_and_unknown_without_a_price` | 미실행 |  |
| `node-unit/the_position_block_lists_each_position_and_leaves_out_size_zero` | 미실행 |  |
| `node-unit/the_notice_texts_hold_the_tag_the_round_the_count_and_the_calls` | 미실행 |  |
| `node-unit/a_hanging_position_read_holds_back_no_cancel_all` | 미실행 |  |
| `node-unit/while_requests_are_paused_no_count_and_no_position_read_go_out` | 미실행 |  |
| `node-unit/a_slow_position_read_lets_at_most_one_notice_wait_and_the_newest_wins` | 미실행 |  |

## V-KILL-4 — Bybit 게이트웨이는 Kill을 받으면 곧바로 킬 활성을 켜서 루프가 도는 동안 어떤 생성도 내보내지 않고(래치 전에 풀린 생성도 NotSent), 루프가 도는 동안의 두 번째 Kill은 무시하며, KillStop은 다음 대기에서 알림도 입력도 없이 루프를 끝내고 생성이 다시 나간다

- 상태: **미실행**
- 왜 중요한가: 새 주문은 0건 확인을 끝내지 못하게 하고(TN 표 7-7), 운영자가 푼 뒤에는 거래가 돌아와야 한다
- 수준: 단위, 통합(게이트웨이·드라이버)
- 전제와 범위: 게이트웨이 시험 둘(가짜 WS·http-fake), 드라이버 시험 하나(수동 시계)
- 실제 구성요소: Bybit 게이트웨이(create_allowed, 명령 처리, 킬 태스크), run 드라이버
- 모의·시뮬레이션: Bybit REST·WS(가짜), 시계(수동)
- 주입한 장애: 루프 도중의 생성, 두 번째 Kill, KillStop
- 판정 기준과 근거: fake가 받은 생성 POST 수, NotSent 문장, 루프의 끝(Stopped), 끝난 뒤 알림 수와 KillLoopEnded 없음, 생성이 다시 나감. 넣은 결함과 잡은 시험: 루프 중 생성(저장소 밖 mut.py create-during-kill), KillStop이 생성을 막아 둠(kill-stop-keeps-creates-home), KillStop 무시(stop-ignored)
- 재현: `cargo test -p oms-node --lib venue::bybit::gateway::tests::no_create venue::bybit::gateway::tests::kill_stop venue::bybit::kill::tests::a_stop`
- 뒷받침하는 것: C-KILL K5·K10, C-HOLD의 KillStop, G-14, ADR-030 §7(U18b2)
- 확인하지 않은 범위: 실제 거래소. 명령 채널에서 Kill보다 앞에 이미 전송 중이던 생성(거래소에 닿은 것은 감시가 잡는다)

| 시험 | 결과 | 비고 |
|---|---|---|
| `node-unit/no_create_leaves_while_the_kill_loop_runs` | 미실행 |  |
| `node-unit/kill_stop_ends_the_loop_and_creates_flow_again` | 미실행 |  |
| `node-unit/a_stop_ends_the_loop_without_a_notice_or_an_input` | 미실행 |  |

## V-KILL-5 — 이 프로세스의 루프가 거래소에서 0건을 확인한 뒤(confirmed) 새 킬 계기(StreamLost, watcher_kill)가 오면 코어는 kill_rearmed{source, note}로 확인(confirmed_at, outcome)을 지우고 새 Kill을 부르며, 래치의 첫 메모는 그대로다. 새 루프가 확인하기 전 clear-hold는 kill_unconfirmed로 거절되고, 재실행이 같은 상태를 만든다. 확인 전의 두 번째 계기와 unsupported 답 뒤의 계기는 아무것도 쓰지 않는다. [venue]의 킬 필드 다섯은 설계 기본값으로 읽히고 각 경계 밖 한 걸음은 거절된다(kill_after_ms는 2 × ping 이상)

- 상태: **미실행**
- 왜 중요한가: 확인 뒤의 새 끊김은 새 위험이다. 옛 확인으로 풀 수 있으면 그 뒤에 남은 주문을 아무도 확인하지 않는다(U18b1 리뷰 N3)
- 수준: 단위
- 전제와 범위: 코어 시험 하나(워처와 스트림 두 계기, 재실행), 설정 검증
- 실제 구성요소: oms-core(decide, apply, command_for, replay), 설정 읽기
- 모의·시뮬레이션: 계기 입력(시험이 넣음)
- 주입한 장애: 확인 뒤의 새 계기, 확인 전의 두 번째 계기, unsupported 답 뒤의 계기, 경계 밖 설정
- 판정 기준과 근거: 이벤트 이름 목록, core.kill의 confirmed_at·outcome·note, 명령 목록, 해제 거절 코드, 재실행 다이제스트, 설정 오류 문장. 넣은 결함과 잡은 시험: 다시 무장하지 않음(M150), unsupported 뒤에도 다시 무장(M153), 확인을 남김(저장소 밖 mut.py rearm-keeps-confirmation)
- 재현: `cargo test -p oms-core --test holds a_new_trigger second; cargo test -p oms-node --lib config::tests::the_kill_loop; cargo test -p oms-core --test event_fields`
- 뒷받침하는 것: C-KILL K9와 이벤트, C-TAIL 필드 표, ADR-030 §7(U18b2)
- 확인하지 않은 범위: 실행 파일 단위의 설정 거부(검증 함수만). status.gateway.kill_loop의 모든 필드(프로세스 시험이 phase와 kill_armed만 본다)

| 시험 | 결과 | 비고 |
|---|---|---|
| `core/a_new_trigger_after_the_confirmation_resets_it_and_asks_for_a_new_loop` | 미실행 |  |
| `core/a_new_trigger_after_an_unsupported_answer_records_nothing` | 미실행 |  |
| `core/a_second_trigger_keeps_the_first_note` | 미실행 |  |
| `node-unit/config::tests::the_kill_loop_fields_are_bounded` | 미실행 |  |
| `core/the_field_table_lists_every_field_the_encoder_writes` | 미실행 |  |

## V-LIVE-1 — [liveness]에 적힌 전략의 마지막 거래 연결이 닫히거나, 그 전략의 어느 연결에서도 liveness_ms 동안 요청이 없으면(ping도 요청이다), 노드는 다음 hello까지 한 번 Input::Session(Lost)를 넣고, 코어는 strategy_link_lost와 같은 기록에서 그 전략의 걸린 일반 주문 가운데 취소 요청이 없는 것마다 규칙 strategy_link의 취소를 요청한다. 그 전략의 감소 전용 주문과 다른 전략의 주문은 취소하지 않는다. 프로세스에서: a(5,000 ms)와 b가 적혔을 때 a의 연결을 닫으면 거래소에서 a의 일반 주문만 취소되고 a의 감소 전용과 b의 일반 주문은 남는다. ping만 8초 보내면 잃음이 없고, 멈추면 마지막 요청 5초 뒤에서 7초 사이에 잃음이 기록된다. 두 연결 가운데 하나가 닫혀도 잃음이 없고 마지막이 닫히면 있다. 운영 소켓의 hello는 세지 않는다. 한 연결이 다른 전략으로 다시 hello하면 앞 전략에서는 닫힌 연결이다: 그것이 a의 마지막 연결이면 곧바로 last trading connection closed로 잃음이 되고 a의 일반 주문이 취소된다. 노드의 표에서: 정확히 liveness_ms의 침묵은 잃음이 아니고 1 ms 넘으면 잃음이며, 침묵은 마지막 요청이나 마지막 hello부터 잰다

- 상태: **미실행**
- 왜 중요한가: 전략 프로세스가 죽으면 그 호가가 거래소에 남고, 전에는 OMS가 아무것도 취소하지 않았다(설계 R5 SHOULD 7). 한 전략의 죽음이 다른 전략의 주문이나 포지션을 줄이는 주문을 건드리면 안 된다
- 수준: 단위, 통합
- 전제와 범위: 코어 시험(H 하네스, 매 입력 재실행 대조), 노드의 표(시간이 입력, 큐는 주입한 함수), 실제 oms-node·simvenue 프로세스 셋(실제 시간)
- 실제 구성요소: oms-core의 결정과 apply, oms-node의 liveness 표, 서버의 연결 처리와 250 ms 타이머, 커밋 단계, 저널, 가상 거래소 프로세스
- 모의·시뮬레이션: 거래소(simvenue), 전략(시험의 클라이언트), 시간(표 시험에서만 인자)
- 주입한 장애: 연결 닫힘, 열린 채 침묵, ping만, 두 번째 연결, 운영 소켓의 hello, 다른 전략으로 다시 hello, 경계의 침묵
- 판정 기준과 근거: 거래소에 남은 열린 주문의 집합, 저널의 cancel_requested{by: rule strategy_link} 목록, strategy_link_lost 수와 기록된 때(마지막 요청 뒤 5~7초), status.strategies.<s>.link. 넣은 결함과 잡은 시험: 모든 전략의 일반 주문 취소(M162), 감소 전용 취소(M163), ping을 세지 않음(M164), 운영 연결을 셈(M168), 다른 전략으로 hello해도 앞 전략에서 셈(M172), 저장소 밖 mut.py의 경계·시계·닫힘 결함
- 재현: `cargo test -p oms-node --test liveness; cargo test -p oms-node --lib liveness::; cargo test -p oms-core --test strategy_link`
- 뒷받침하는 것: C-LIVE L1~L4, ADR-030 §7(U18b3)
- 확인하지 않은 범위: 실제 거래소. 취소가 거래소에서 거절되거나 시도가 다한 경우(C-CANCEL의 경로). 노드가 오래 멈췄다 깨어난 경우(단조 시계)

| 시험 | 결과 | 비고 |
|---|---|---|
| `node-unit/closing_the_last_connection_cancels_that_strategys_plain_orders_only` | 미실행 |  |
| `node-unit/five_seconds_without_a_request_cancel_and_a_ping_keeps_it_alive` | 미실행 |  |
| `node-unit/a_second_connection_keeps_the_strategy_alive` | 미실행 |  |
| `core/a_lost_strategy_gets_cancel_requests_for_its_plain_orders_only` | 미실행 |  |
| `node-unit/liveness::tests::no_request_for_liveness_ms_reports_a_loss_and_any_request_keeps_it_alive` | 미실행 |  |
| `node-unit/liveness::tests::the_last_close_reports_one_loss_until_the_next_hello` | 미실행 |  |
| `node-unit/liveness::tests::two_connections_count_and_only_the_last_close_reports` | 미실행 |  |
| `node-unit/a_hello_for_another_strategy_loses_the_first_on_that_connection` | 미실행 |  |
| `node-unit/liveness::tests::a_hello_for_another_strategy_closes_the_connection_for_the_first` | 미실행 |  |

## V-LIVE-2 — StrategyLink는 그 전략의 일반 주문만 거절한다: holds(Some(s))에만 있고 holds(None)과 다른 전략의 목록에는 없다. 증거금 정책이 있으면 그 전략의 감소 전용 주문은 받고, 없으면 모든 보류처럼 감소 전용도 거절한다. 걸린 동안의 잃음은 기록하지 않고 첫 이유와 시각이 남으며, 설정에 없는 전략의 잃음은 bad_request로 기록하지 않는다. 그 전략이 없는 설정이 활성화되면 보류가 사라진다. 표 7-7의 시험에서 StrategyLink 줄은 일반 주문 거절, 감소 전용 받음이다

- 상태: **미실행**
- 왜 중요한가: 죽은 전략의 새 노출을 막되 그 전략의 포지션을 줄이는 길은 열어 둔다(R5). 다른 전략은 거래를 계속한다
- 수준: 단위
- 전제와 범위: 코어 시험(H 하네스)
- 실제 구성요소: oms-core의 check_new_order, holds, 결정과 apply
- 모의·시뮬레이션: 노드의 입력(시험이 Input::Session을 넣음)
- 주입한 장애: 두 번째 잃음, 모르는 전략, 증거금 정책 없음, 전략을 뺀 설정
- 판정 기준과 근거: 거절 사유 목록, holds의 내용, 기록 수, core.strategy_link. 넣은 결함과 잡은 시험: 두 번째 잃음 기록, 모든 전략에 보류, 설정이 보류를 남김(저장소 밖 mut.py)
- 재현: `cargo test -p oms-core --test strategy_link --test holds`
- 뒷받침하는 것: C-LIVE의 코어, ADR-008 §7, TN 표 7-7
- 확인하지 않은 범위: 실제 거래소

| 시험 | 결과 | 비고 |
|---|---|---|
| `core/the_hold_refuses_only_that_strategys_plain_orders` | 미실행 |  |
| `core/the_hold_table_of_table_7_7` | 미실행 |  |
| `core/a_second_loss_records_nothing_and_keeps_the_first_why` | 미실행 |  |
| `core/a_loss_of_an_unknown_strategy_records_nothing` | 미실행 |  |
| `core/a_configuration_without_the_strategy_drops_its_hold` | 미실행 |  |

## V-LIVE-3 — 잃음 뒤(또는 내구화된 코어가 그 전략을 보류 중일 때) 거래 소켓의 hello는 Input::Session(Hello)를 control 레인에 넣고, strategy_link_restored가 내구화된 뒤에 답한다: 답이 온 순간 같은 연결의 status(내구화된 기록만)에 보류가 없고 저널 파일에 복구가 있다. 복구가 아직 답을 받지 않은 동안 같은 전략의 hello(다른 연결이든 같은 연결이든)도 복구를 넣으므로 그 답도 앞 복구가 내구화된 뒤에 온다(코어에서는 기록 없는 not held, 커밋 단계가 순서대로 답한다). 큐를 기다리던 잃음은 복구보다 먼저 넣는다(표의 hello가 넣는 순서: lost, hello). 큐가 가득이면 잃음은 버리지 않고 틱마다 다시 넣는다. 재시작 뒤 보류는 저널에서 복원되고 다음 hello가 푼다. 시작할 때는 걸린 일반 주문이 있는 적힌 전략만 무장한다: 프로세스에서 강제 종료(SIGKILL) 뒤 다시 붙지 않으면, 일반·감소 전용 주문이 걸린 a는 시작 5초 뒤 no request for 5000 ms로 잃음이 되고 일반 주문만 취소되며, 감소 전용 주문만 걸린 b는 무장하지 않는다. 걸리지 않은 전략의 hello는 기록하지 않는다

- 상태: **미실행**
- 왜 중요한가: 전략이 답을 받은 뒤의 첫 일반 주문이 보류로 거절되면 안 되고, 잃음과 복구가 겹쳐도 잃음의 취소가 사라지면 안 된다
- 수준: 단위, 통합
- 전제와 범위: 코어 시험, 노드의 표(큐가 가득인 경우 포함), 실제 oms-node·simvenue 프로세스(재시작 한 번)
- 실제 구성요소: 서버의 hello 처리, 커밋 단계와 읽기 모델(내구화된 기록만), 저널, 복구
- 모의·시뮬레이션: 거래소(simvenue), 큐가 가득(표 시험의 주입한 함수)
- 주입한 장애: 잃음 뒤 hello, 복구가 답을 받기 전의 두 번째 hello, 큐가 가득, 재시작, 강제 종료 뒤 다시 붙지 않음
- 판정 기준과 근거: hello 직후 status의 strategies.a.link.held와 holds, 저널의 strategy_link_restored 수, 표의 계획(lost_first, restore). 넣은 결함과 잡은 시험: 내구화 전 답(M165), 큐가 거절한 잃음을 보낸 것으로 읽음(M166), 기다리던 잃음을 건너뜀(M167), 복구가 답을 받기 전의 hello에 곧바로 답함(M169), 시작에서 무장하지 않음(M170), 감소 전용 주문으로 무장(M171), 노드의 잃음을 무시한 복구와 기다리던 잃음을 넣지 않는 hello(저장소 밖 mut.py)
- 재현: `cargo test -p oms-node --test liveness hello; cargo test -p oms-node --lib liveness::; cargo test -p oms-core --test strategy_link`
- 뒷받침하는 것: C-LIVE L5, C-COMMIT, ADR-030 §7(U18b3)
- 확인하지 않은 범위: 복구 기록의 flush 실패(C-COMMIT의 일반 경로로만, 이 경로의 시험은 없다). 이 시험이 보는 것은 읽기 모델과 파일의 기록이지 fsync 자체가 아니다(pipeline의 커밋 시험이 그 순서를 본다). 복구가 답을 받기 전의 두 번째 hello는 표 시험만 본다(프로세스에서 그 틈을 만들지 않았다). 서버가 복구의 답을 표에 알리지 않는 결함은 어떤 시험도 잡지 않는다(그 뒤의 hello가 모두 복구를 넣을 뿐, 안전한 쪽, mut.py r1S1-server-never-tells-the-table). 큐가 가득일 때의 순서는 표의 hello에서 보고 control 레인이 가득인 프로세스는 만들지 않았다. 코어가 보류 중인데 적히지 않은 전략의 hello는 표 시험만 본다

| 시험 | 결과 | 비고 |
|---|---|---|
| `node-unit/hello_after_a_loss_is_answered_after_the_restore_is_durable` | 미실행 |  |
| `core/hello_restores_only_a_held_strategy` | 미실행 |  |
| `core/the_hold_survives_replay_and_a_restart` | 미실행 |  |
| `node-unit/liveness::tests::hello_after_a_loss_asks_for_the_restore_and_a_waiting_loss_goes_first` | 미실행 |  |
| `node-unit/liveness::tests::a_full_queue_is_retried_on_the_next_tick` | 미실행 |  |
| `node-unit/liveness::tests::at_start_only_strategies_with_plain_orders_are_armed` | 미실행 |  |
| `node-unit/liveness::tests::a_hello_while_a_restore_is_in_flight_waits_for_it_too` | 미실행 |  |
| `node-unit/after_a_restart_a_strategy_with_resting_plain_orders_is_armed_without_a_hello` | 미실행 |  |

## V-LIVE-4 — Input::Session, strategy_link_lost, strategy_link_restored, core.strategy_link는 기록 스키마를 올리지 않는다(2 그대로): 비어 있으면 쓰지 않아 옛 상태의 바이트가 그대로이고, 잃음·복구·잃음이 든 저널은 라이브 상태로 복구되며 verify --re-decide를 통과한다. 이벤트 필드 표가 두 이벤트의 모든 필드를 적는다. 알림 strategy_link:<s>는 유예 0이고 이유·시각과 증거금 정책에 따른 감소 전용 문장을 담으며 hello로 해소된다. [liveness]의 값은 1,000..=60,000 ms, 이름은 [core]의 전략이어야 하고, mainnet은 사용 중인 전략이 모두 적혀야 시작한다. 적히지 않은 전략은 감시하지 않는다

- 상태: **미실행**
- 왜 중요한가: 기존 저널과 시험이 그대로여야 하고(스펙 완료 기준), 워처(U19a)는 필드 표만으로 만든다. 전략의 죽음은 사람에게 곧바로 알려야 한다(C-ALERT)
- 수준: 단위, 통합
- 전제와 범위: 저널 하나를 쓰고 복구·재결정, 필드 표 대조, 알림 조건, 설정 검증, 노드의 표
- 실제 구성요소: oms-node 복구와 verify, 알림 조건 함수, 설정 읽기
- 모의·시뮬레이션: 입력(시험이 넣음)
- 주입한 장애: 경계 밖 값, 모르는 전략, mainnet에 [liveness] 없음 또는 빠진 전략, 사용하지 않는 전략
- 판정 기준과 근거: RECORD_SCHEMA = 2, 다이제스트 같음, 재결정 통과, 직렬화에 필드 없음, 필드 표의 행, 알림의 키·문장·유예, 설정 오류 문장
- 재현: `cargo test -p oms-node --test record_schema; cargo test -p oms-node --lib alerts:: config:: liveness::; cargo test -p oms-core --test event_fields`
- 뒷받침하는 것: C-LIVE의 설정·상태·알림, C-TAIL 필드 표, C-ALERT 조건 표
- 확인하지 않은 범위: 실제 텔레그램. 실행 파일 단위의 mainnet 시작 거부(설정 검증 함수만). status.strategies.<s>.link의 모든 필드(프로세스 시험은 held, why, since, connections, liveness_ms를 본다)

| 시험 | 결과 | 비고 |
|---|---|---|
| `node-unit/kill_holds_liveness_and_heartbeats_recover_to_the_live_state` | 미실행 |  |
| `core/the_field_table_lists_every_field_the_encoder_writes` | 미실행 |  |
| `node-unit/alerts::tests::a_lost_strategy_link_is_its_own_condition_at_once` | 미실행 |  |
| `node-unit/config::tests::the_liveness_section_is_validated_and_required_on_mainnet` | 미실행 |  |
| `node-unit/liveness::tests::a_strategy_that_is_not_listed_has_no_liveness` | 미실행 |  |

## V-GATE-1 — 모니터링 게이트는 기록 시각으로 판단한다(250 ms 틱, 엄격한 >): 하트비트가 0초에 오고 끊기면 30.0초까지 감시됨, 30.25초에 monitor_changed{false}, 그보다 60초 넘은 첫 틱(90.5초)에 걸린 일반 주문의 취소를 한 번 요청하고(규칙 unmonitored) 그 뒤 틱은 더 묻지 않는다. 5초마다 오는 하트비트에서 W1이나 W2가 0초에 멈추면 10초의 하트비트(정확히 10초)는 감시됨, 15초의 하트비트가 닫고, 하트비트가 계속 와도 취소는 닫은 기록보다 60초 넘은 첫 틱에 온다. 검사의 나이는 그것을 알린 하트비트의 시각에서 잰다. W3·W4는 기록할 뿐 판정하지 않는다. 알려진 한계(리뷰 1 M1): 기록 시각이 멈춘 동안(호스트 시계가 뒤로 감) 틱 2,400개에도 monitor_changed와 취소가 없고, 옛 시계로 찍은 W1·W2의 하트비트는 받아들여져 게이트가 열린 채이며, 새 시계로 590초 뒤에 찍힌 하트비트는 게이트를 닫는다. 감소 전용 주문은 취소하지 않고, 연결이 내려가 있는 동안은 묻지 않으며, 다한 취소는 다시 묻지 않는다. 프로세스에서: W2가 11초 전인 하트비트가 곧바로 닫고, 거래소의 일반 주문은 10초(unmonitored_cancel_us) 전에는 남고 그 뒤 규칙으로 취소되며 감소 전용 주문은 남는다

- 상태: **미실행**
- 왜 중요한가: 워처가 없어지거나 검사가 실패하면 거래가 멈춰야 하고(설계 §7.5, R2), 걸린 호가도 60초 뒤에는 취소돼야 한다. 감소 전용 주문은 포지션을 줄이는 길이다
- 수준: 단위, 통합
- 전제와 범위: 코어 시험(H 하네스, 기록 시각을 정확히 넣음, 매 입력 재실행 대조), 실제 oms-node·simvenue 프로세스(실제 시간)
- 실제 구성요소: oms-core의 판정·보류·틱의 규칙 취소와 apply, 노드의 틱 작업, 커밋 단계, 저널, 가상 거래소 프로세스
- 모의·시뮬레이션: 워처(시험이 하트비트를 넣음), 거래소(simvenue), 시간(코어 시험의 기록 시각)
- 주입한 장애: 하트비트 끊김, W1·W2 멈춤, 계속 오는 오래된 하트비트, 정확히 경계의 나이, null 검사, 연결 내려감, 다한 취소, 멈춘 기록 시각
- 판정 기준과 근거: monitor_changed와 규칙 unmonitored의 cancel_requested가 기록된 시각의 목록, 주문마다의 취소 요청, 거래소의 열린 주문 집합. 넣은 결함과 잡은 시험: W2 대신 W3(M173), 2단계를 곧바로(M174), W1만 판정(M175), 저장소 밖 mut.py의 경계 >=, 틱 시각에서 잰 검사 나이, 마지막 하트비트에서 잰 2단계, 연결 내려감에도 취소, 다한 취소를 다시 물음
- 재현: `cargo test -p oms-core --test monitor; cargo test -p oms-node --test monitor_node a_stale_heartbeat`
- 뒷받침하는 것: C-LIVE 2부 G2~G4, ADR-030 §7 H
- 확인하지 않은 범위: 실제 워처(U19a)와 실제 거래소. 2단계 취소가 거래소에서 거절되거나 시도가 다한 경우(C-CANCEL의 경로). 프로세스 시험의 시각은 실제 시간이라 틱 단위의 정확한 시각은 코어 시험만 본다

| 시험 | 결과 | 비고 |
|---|---|---|
| `core/missing_heartbeats_unmonitor_at_30_s_and_cancel_plain_orders_at_90_s` | 미실행 |  |
| `core/a_heartbeat_with_w1_or_w2_older_than_10_s_unmonitors_and_cancels_60_s_later` | 미실행 |  |
| `core/stale_heartbeats_that_keep_coming_do_not_delay_the_cancels` | 미실행 |  |
| `core/the_check_age_edge_and_a_check_without_success` | 미실행 |  |
| `core/w3_and_w4_are_recorded_and_not_gated` | 미실행 |  |
| `core/stage_two_asks_only_while_the_link_is_up` | 미실행 |  |
| `core/stage_two_does_not_ask_again_for_an_exhausted_cancel` | 미실행 |  |
| `core/a_clock_that_steps_back_holds_the_gates_timers` | 미실행 |  |
| `node-unit/a_stale_heartbeat_unmonitors_and_the_node_cancels_resting_plain_orders_later` | 미실행 |  |

## V-GATE-2 — 게이트가 있는 설정의 booted는 감시 안 됨(no heartbeat since the start, 하트비트 없음)으로 시작한다: 재시작 전의 하트비트는 세지 않고, 첫 신선한 하트비트까지 일반 주문은 Hold(Unmonitored)로 거절되며, 하트비트가 없으면 booted의 기록 시각보다 60초 넘은 첫 틱에 걸린 일반 주문의 취소를 요청한다. 이미 감시 안 됨일 때의 부팅도 since를 부팅 시각으로 다시 정해, 그보다 60초 전에는 취소하지 않는다. 게이트를 켜는 config_activated도 활성화 시각부터 같다(no heartbeat since the gate came on), 끄는 설정은 상태와 보류를 지우고, 값만 바꾸는 설정은 상태를 그대로 둔다. W1·W2가 신선한 첫 하트비트가 monitor_changed{true}로 풀고, 그 뒤 2단계는 새 요청을 하지 않으며 이미 한 요청은 남는다. 게이트 상태는 저널에서 같은 상태로 재실행된다. 표 7-7의 시험에서 Unmonitored 줄은 일반 주문 거절, 감소 전용 받음이다(증거금 정책 있음). 프로세스에서: 재시작한 노드는 직전에 감시됨이었어도 no heartbeat since the start로 감시 안 됨이다

- 상태: **미실행**
- 왜 중요한가: 재시작 전의 하트비트는 지금 워처가 살아 있다는 증거가 아니다(설계 §7.5 의도된 동작). 풀린 뒤에도 2단계가 계속 취소하면 안 된다
- 수준: 단위, 통합
- 전제와 범위: 코어 시험(H 하네스와 저널 재실행), 실제 oms-node·simvenue 프로세스의 재시작 한 번
- 실제 구성요소: oms-core의 booted·config_activated의 apply, 판정, 보류 표, 재실행
- 모의·시뮬레이션: 워처(시험이 하트비트를 넣음), 거래소(simvenue)
- 주입한 장애: 재시작, 게이트를 켜고 끄는 설정, 값만 바꾸는 설정, 감시 안 됨 중의 재실행
- 판정 기준과 근거: core.monitor의 값(monitored, since, reason, last), holds의 내용, 거절 사유, 재실행한 상태와의 같음, 풀린 뒤의 취소 요청 없음. 넣은 결함과 잡은 시험: 부팅이 마지막 하트비트를 남김(M177), 감시 안 됨에서의 부팅이 옛 since를 남김(M179), 저장소 밖 mut.py의 보류 제거, 하트비트 때 판정 건너뜀, 켜는 설정이 상태를 만들지 않음, 끄는 설정이 상태를 남김, Unmonitored가 감소 전용도 거절
- 재현: `cargo test -p oms-core --test monitor; cargo test -p oms-core --test holds the_hold_table`
- 뒷받침하는 것: C-LIVE 2부 G3·G5와 시작, C-HOLD 표 7-7의 다섯째 줄, ADR-030 §7 H5
- 확인하지 않은 범위: 실제 워처. 증거금 정책 없이 감소 전용도 거절되는 경우는 다른 보류와 같은 check_new_order의 규칙으로만 본다(이 게이트만의 시험은 없음)

| 시험 | 결과 | 비고 |
|---|---|---|
| `core/the_gate_starts_unmonitored_at_boot_until_the_first_fresh_heartbeat` | 미실행 |  |
| `core/a_fresh_heartbeat_monitors_again_and_stops_the_cancels` | 미실행 |  |
| `core/a_configuration_that_turns_the_gate_on_or_off` | 미실행 |  |
| `core/the_gate_survives_replay_and_a_boot_starts_unmonitored` | 미실행 |  |
| `core/a_boot_while_unmonitored_times_stage_two_from_the_boot` | 미실행 |  |
| `core/the_hold_table_of_table_7_7` | 미실행 |  |
| `node-unit/a_stale_heartbeat_unmonitors_and_the_node_cancels_resting_plain_orders_later` | 미실행 |  |

## V-GATE-3 — monitor.sock은 게이트가 있을 때만 열리고(모드 0600) 자기 풀이 연결 둘이다: 셋째 연결은 거절된다. 그 소켓은 전략 없는 hello, ping, heartbeat만 받는다: status, list_open, new_order, halt, watcher_kill, snapshot은 forbidden, 전략을 단 hello는 unauthorized. heartbeat는 거래·운영 소켓에서 forbidden이고 기록되지 않는다. hello 전의 heartbeat는 unauthorized이고 기록되지 않는다. 계약의 줄(epoch ms, null 포함)은 해석되고, 기록이 내구화된 뒤 recorded로 답한다(답이 온 때 저널에 하트비트가 있다). 노드는 ms를 µs로 바꾸고(넘침은 거절), 연결마다 지난 1초에 둘을 받았으면 그 뒤를 overloaded로 거절한다(한꺼번에 보낸 넷 가운데 둘 기록). 코어는 검사 시각이 기록 시각보다 1초 넘게 뒤인 하트비트, W1·W2가 없거나 키가 W1..W9 밖이거나 이름이 1~64자가 아닌 하트비트, 게이트가 없을 때의 하트비트를 bad_request로 거절하고 아무것도 기록하지 않는다

- 상태: **미실행**
- 왜 중요한가: 거래 소켓의 클라이언트가 게이트를 열어 두거나 고장 난 워처가 저널을 채우면 안 되고, 노드보다 앞선 시각을 찍는 워처가 검사를 신선해 보이게 하면 안 된다. 워처(U19a)는 계약의 줄만으로 만든다
- 수준: 단위, 통합
- 전제와 범위: 코어 시험, 노드의 순수 함수(시간이 입력), oms-api의 줄 해석, 실제 oms-node·simvenue 프로세스(실제 소켓)
- 실제 구성요소: 서버의 소켓 종류·풀·op 거름·속도 한도, 줄 해석, 커밋 단계, 저널
- 모의·시뮬레이션: 워처(시험의 클라이언트와 날 줄)
- 주입한 장애: 다른 소켓의 heartbeat, hello 전의 heartbeat, 금지된 op, 전략을 단 hello, 셋째 연결, 초당 넷, 미래 시각, 모양이 틀린 줄, 게이트 없음, 넘치는 ms
- 판정 기준과 근거: 답의 오류 코드, 저널의 watcher_heartbeat 수, 소켓 파일의 유무와 모드. 넣은 결함과 잡은 시험: 미래 시각을 받음(M176), 다른 소켓의 heartbeat를 받음(M178), hello 전의 heartbeat를 받음(M180), 저장소 밖 mut.py의 속도 한도 제거, 모니터 소켓이 모든 op를 받음, 게이트 없이 소켓을 엶, ms를 µs로 바꾸지 않음, 1초 창이 100 ms, 게이트 없는 하트비트를 받음, 모양 검사 건너뜀
- 재현: `cargo test -p oms-node --test monitor_node; cargo test -p oms-node --lib monitor::; cargo test -p oms-core --test monitor; cargo test -p oms-api wire_example_of_the_heartbeat`
- 뒷받침하는 것: C-LIVE 2부 G1과 소켓, C-API의 heartbeat 줄과 모니터 소켓, ADR-030 §7 H1
- 확인하지 않은 범위: 다른 uid의 접속 거절(시험은 같은 uid로만 돈다, 코드 경로는 운영 소켓과 같다). 코어 큐가 가득일 때의 overloaded(다른 op와 같은 submit 경로)

| 시험 | 결과 | 비고 |
|---|---|---|
| `node-unit/heartbeats_over_monitor_sock_open_the_gate_and_other_ops_are_forbidden` | 미실행 |  |
| `node-unit/without_the_gate_there_is_no_monitor_socket` | 미실행 |  |
| `core/a_heartbeat_from_the_future_is_refused` | 미실행 |  |
| `core/a_heartbeat_of_the_wrong_shape_or_without_a_gate_records_nothing` | 미실행 |  |
| `node-unit/monitor::tests::two_heartbeats_per_second_per_connection` | 미실행 |  |
| `node-unit/monitor::tests::a_heartbeat_line_becomes_a_core_input_in_microseconds` | 미실행 |  |
| `api/tests::wire_example_of_the_heartbeat` | 미실행 |  |

## V-GATE-4 — Input::Watcher(Heartbeat), watcher_heartbeat, monitor_changed, policy.monitor, core.monitor는 기록 스키마를 올리지 않는다(2 그대로): 게이트가 없으면 쓰지 않아 옛 상태와 설정의 바이트가 그대로이고, 게이트를 켜는 설정·신선한 하트비트·오래된 하트비트·2단계의 틱이 든 저널은 라이브 상태로 복구되며 verify --re-decide를 통과한다. 이벤트 필드 표가 두 이벤트의 모든 필드를 적는다(W1..W9 포함). [core.policy.monitor]의 값은 heartbeat_max_age_us 10..=300초, check_max_age_us 2..=60초, unmonitored_cancel_us 10..=600초이고, 모르는 키는 거부되며, mainnet은 표 없이 시작하지 않고 testnet과 가상 거래소는 선택이다. 표가 있으면 runtime.tick_ms는 1,000 이하다(1,001은 거부, 표가 없으면 60,000도 받음). 세 소켓 경로는 서로 달라야 한다. 알림 hold:unmonitored는 지나가는 상태의 유예(30초)를 받고 이유·시각·2단계까지 남은 초(또는 취소 중)와 증거금 정책에 따른 감소 전용 문장을 담으며, 신선한 하트비트로 해소된다. status.core.monitor는 게이트가 없으면 gate: false다

- 상태: **미실행**
- 왜 중요한가: 기존 저널과 시험이 그대로여야 하고(스펙 완료 기준), 워처(U19a)는 필드 표만으로 만든다. 모든 시작이 이 상태를 지나므로 알림은 유예를 받는다(U18a의 지나가는 상태 규칙)
- 수준: 단위, 통합
- 전제와 범위: 저널 하나를 쓰고 복구·재결정, 필드 표 대조, 알림 조건, 설정 검증, 상태 문서
- 실제 구성요소: oms-node 복구와 verify, 알림 조건 함수, 설정 읽기와 검증, 상태 문서
- 모의·시뮬레이션: 입력(시험이 넣음)
- 주입한 장애: 경계 밖 값, 모르는 키, mainnet에 게이트 없음, 같은 소켓 경로, 게이트가 있는데 느린 틱
- 판정 기준과 근거: RECORD_SCHEMA = 2, 다이제스트 같음, 재결정 통과, 직렬화에 monitor 없음, 필드 표의 행, 알림의 키·문장·유예, 설정 오류 문장. 넣은 결함과 잡은 시험: 게이트가 있는데 아무 틱이나 받음(M181), 저장소 밖 mut.py의 mainnet 거부 제거
- 재현: `cargo test -p oms-node --test record_schema; cargo test -p oms-node --lib alerts:: config::; cargo test -p oms-core --test event_fields; cargo test -p oms-core --test monitor the_gate_values`
- 뒷받침하는 것: C-LIVE 2부의 설정·상태·알림·이벤트, C-TAIL 필드 표, C-ALERT 조건 표
- 확인하지 않은 범위: 실제 텔레그램. 실행 파일 단위의 mainnet 시작 거부(설정 검증 함수만). status.core.monitor의 모든 필드(프로세스 시험은 gate, monitored, reason, watcher, cancel_stage, checks의 W3·W4를 본다)

| 시험 | 결과 | 비고 |
|---|---|---|
| `node-unit/kill_holds_liveness_and_heartbeats_recover_to_the_live_state` | 미실행 |  |
| `core/the_field_table_lists_every_field_the_encoder_writes` | 미실행 |  |
| `core/the_gate_values_are_bounded` | 미실행 |  |
| `node-unit/config::tests::the_monitoring_gate_is_validated_and_required_on_mainnet` | 미실행 |  |
| `node-unit/alerts::tests::unmonitored_is_a_passing_condition_with_its_cancel_time` | 미실행 |  |
| `node-unit/without_the_gate_there_is_no_monitor_socket` | 미실행 |  |

## V-WATCH-1 — 워처 crate는 OMS와 코드를 나누지 않는다: oms-*·md-* crate를 어떤 종류(정상, 빌드, 개발)와 target으로도 선언하지 않고, 개발 의존성 http-fake 말고는 작업 공간 crate를 쓰지 않으며, 그 정상·빌드 그래프에서 OMS·시세·작업 공간 crate에 닿지 않고, watcher-conformance 말고는 아무 작업 공간 crate도 워처에 의존하지 않으며, 소스가 #[path]·include!·속성 안의 path =·심볼릭 링크로, 매니페스트의 target이 소스 경로로 패키지 밖의 파일을 가져오지 않는다. 검사 스크립트는 그 각각을 심으면 그 결과 줄과 종료 코드 1을 내고, 확인할 수 없을 때(맞지 않는 잠금, 없는 매니페스트)는 ok가 아니라 종료 코드 2를 낸다

- 상태: **미실행**
- 왜 중요한가: OMS의 저널 인코딩·대조·Bybit 해석의 결함을 같은 코드로 확인하면 확인이 아니다(ADR-031). 독립성은 사람의 약속이 아니라 매 변경의 CI가 지킨다
- 수준: 소스 검사
- 전제와 범위: 실제 트리 한 번(--locked, 호스트 플랫폼)과 합성 작업 공간의 22가지(실제 모양, 정상·빌드·개발·cfg(unix)·cfg(windows) 의존성, 거쳐서 닿는 oms-api, 다른 작업 공간 crate, 역방향, #[path], 시험의 include_str!, 패키지 안의 #[path], cfg_attr의 path =, 심볼릭 링크, 공백이 든 # [path]와 include_str !, 패키지 밖의 [lib] path·[[test]] path·build, 패키지 안의 [lib] path, 다시 만들지 않은 잠금, 없는 매니페스트)
- 실제 구성요소: cargo metadata(1.96.0), jq, 실제 매니페스트와 잠금 파일
- 모의·시뮬레이션: 빈 crate 아홉 개의 합성 작업 공간
- 주입한 장애: 심은 의존성과 소스 줄, 맞지 않는 잠금 파일, 없는 매니페스트. 스크립트 자신의 결함 여섯(개발·빌드 무시, target 무시, 도달 검사 생략, 역방향 생략, cargo 실패를 ok로, 규칙 6 생략)과 검토 1 뒤의 셋(규칙 7 생략, # [ 사이 공백 무시, include 뒤 공백 무시)을 심어 각각 해당 경우가 빨갛게 됨을 확인했다(U19a1 보고)
- 판정 기준과 근거: 실제 트리: 종료 코드 0과 'watcher_independent: ok'. 경우마다 정확한 종료 코드와 그 결과 줄(grep -F), 결과가 있는 경우에는 ok 줄이 없음
- 재현: `tools/ci/watcher_independent.sh && tools/ci/selftest.sh`
- 뒷받침하는 것: C-WATCH 1부, ADR-031
- 확인하지 않은 범위: 다른 플랫폼의 의존성을 거쳐서만 닿는 패키지(규칙 4는 호스트 그래프만 걷는다. 직접 선언은 규칙 2가 잡는다). 여러 줄에 걸친 속성 안의 path =(규칙 6은 평범한 텍스트 검색). 개발 의존성 http-fake가 뒷날 oms-*에 의존하는 경우(규칙 4는 개발 간선을 걷지 않는다). oms-*·md-*가 아닌 이름의 OMS 코드가 레지스트리에서 오는 경우. 저널 읽기와 집합이 코어와 같다는 것은 V-WATCH-2(U19a2)

| 시험 | 결과 | 비고 |
|---|---|---|
| `source/watcher_independent` | 미실행 |  |
| `source/selftest` | 미실행 |  |

## V-WATCH-2 — 워처의 독자 읽기와 저널 상태가 코어와 같은 집합을 세운다: 코어가 결정하고 oms-journal이 쓴 저널을 기록마다 읽어, 열린 주문 집합, 열린 주문마다 종목·방향·수량·전략·클라이언트 ID·거래소 주문 번호·누적 체결·결과 불명·거래소 종결, 종목마다 포지션, 체결·거래소 변경·펀딩 키와 그 거래소 시각, 체결 바닥, 킬 보류와 워처 불일치 보류, 일 기준값이 같은 기록을 재생한 코어와 같다. 시나리오들이 C-TAIL 표의 이벤트를 모두 쓴다. 닫힌 주문의 정보는 purged까지 남아 늦은 체결의 방향과 종목을 주고, 펀딩은 포지션을 바꾸지 않으며, 체결 바닥은 내려가지 않고 booted는 키를 잊지 않으며, 규칙에 맞지 않는 기록(모르는 주문의 체결, 열린 주문의 purged, 형식이 틀린 필드, 순서가 어긋난 기록)은 래치되는 오류다

- 상태: **미실행**
- 왜 중요한가: OMS의 저널 인코딩이나 집합 계산의 결함을 같은 코드가 확인하면 확인이 아니다(ADR-031). 워처의 W1·W2·W4는 이 집합과 거래소를 비교하므로, 집합이 코어와 다르면 거짓 불일치나 놓친 불일치가 된다
- 수준: 단위, 통합
- 전제와 범위: 메모리에서 만든 기록의 규칙 시험 10개. 핵심 시험 도구(H)로 코어를 움직이는 시나리오 열 개(이벤트 51개 모두, 4 KiB 세그먼트 회전 포함), 기록마다 비교
- 실제 구성요소: oms-core의 결정과 재생, oms-journal::Writer가 쓴 파일, 워처의 Tail과 JournalState
- 모의·시뮬레이션: 거래소 입력(시험 도구가 만든 Venue 입력)
- 주입한 장애: 닫힌 뒤의 체결, 파킹과 풀림, 결과 불명과 부재, 누락 체결, 거래소 변경 네 종류와 미분류 실행, 킬 재무장, 정리와 재시작으로 올라간 체결 바닥. 변이 M185~M187, M210, M231(모두 잡힘)
- 판정 기준과 근거: 모든 시나리오의 모든 기록 뒤에 위의 비교가 같고, 쓴 이벤트 이름의 합집합이 tail.md 표의 이름과 같다. 규칙 시험은 그 값과 오류를 정확히 단언한다
- 재현: `cargo test -p watcher --test state && cargo test -p watcher-conformance`
- 뒷받침하는 것: C-WATCH 2부, C-TAIL, ADR-031
- 확인하지 않은 범위: fact_at과 last_change_at은 코어에 같은 값이 없어 규칙 시험으로만 본다. 코어가 쓰지 않는 기록(규칙 위반)은 적합성 시나리오에 없다. 살아 있는 키보다 높은 바닥의 booted(코어의 boot는 첫 부팅에만 바닥을 새로 정하고, 그때는 키가 없다), 아는 거래소 주문 번호 뒤의 null 확인(코어는 한 번만 확인한다), 닫힌 주문의 거래소 종결 표시(열린 주문만 비교)도 규칙 시험으로만 본다. 재생 속도(100만 건당 약 1.3초)는 측정이고 기준이 아니며, 페이지 캐시에 있는 파일로만 쟀다

| 시험 | 결과 | 비고 |
|---|---|---|
| `watcher/the_watcher_sets_equal_the_core_on_journals_with_every_event_kind` | 미실행 |  |
| `watcher/the_sets_follow_the_field_table` | 미실행 |  |
| `watcher/a_fill_after_close_uses_the_kept_order_info` | 미실행 |  |
| `watcher/purged_forgets_closed_orders_and_old_keys` | 미실행 |  |
| `watcher/purging_an_unknown_order_stops_the_state` | 미실행 |  |
| `watcher/the_fill_floor_never_goes_down` | 미실행 |  |
| `watcher/the_fact_time_moves_only_on_the_compared_order_events` | 미실행 |  |
| `watcher/funding_never_changes_a_position` | 미실행 |  |
| `watcher/venue_terminal_marks_the_order_terminal_and_moves_its_fact_time` | 미실행 |  |
| `watcher/an_instrument_the_journal_never_changed_has_no_change_time` | 미실행 |  |
| `watcher/records_that_break_the_rules_stop_the_state` | 미실행 |  |

## V-WATCH-3 — 워처의 자기 Bybit 클라이언트는 GET을 거래소 문서대로 서명하고(정렬된 질의, 보낼 수 없는 값은 보내기 전에 거절, 머리글 다섯, 보내는 순간의 시계), 응답을 부류로 나누며(not_sent, no_answer, rate_limited, clock, refused, auth, ip_banned; unreadable은 검사가 만든다), 키 거절(HTTP 401, retCode 10003·10004·10005·10007·10010·33004) 뒤에는 프로세스가 다시 시작할 때까지 Bybit에 아무 요청도 보내지 않고, HTTP 403 뒤에는 응답 순간부터 600초 동안 모든 요청과 IP 반향을 보내지 않으며, 어느 10초 창에서도 60개를 넘겨 보내지 않고 넘는 요청은 버리지 않고 기다린다. 요청은 보내기 전에 세고, 지표는 VictoriaMetrics 가져오기 JSON 한 줄씩(watcher_up 포함) 큐 없이 밀어 넣는다

- 상태: **미실행**
- 왜 중요한가: OMS의 Bybit 해석 결함을 같은 코드로 확인하면 확인이 아니다(ADR-031). 거절된 키로 계속 보내거나 403 뒤에 일찍 보내면 OMS와 같이 쓰는 IP가 더 오래 막히고, 폭주 루프는 그 IP 한도를 먹는다
- 수준: 단위, 통합(loopback)
- 전제와 범위: 서명 함수와 질의 문자열, 수동 시계 위의 가드(1분 폭주 루프 포함), loopback 가짜 Bybit·반향·VM에 붙인 클라이언트와 내보내기(부류마다 한 번, 거절 코드 일곱 각각 10분, 403 전후, 가득 찬 가드의 대기)
- 실제 구성요소: reqwest(rustls), hmac·sha2, 클라이언트와 등록부 코드, loopback TCP
- 모의·시뮬레이션: Bybit, IP 반향, VictoriaMetrics(http-fake), 시계(수동)
- 주입한 장애: 연결 거절, 응답 없음, 연결 닫힘, 5xx, 읽을 수 없는 본문, retCode 없음·문자열, 10016, 429, 10006, 10002, 다른 retCode, 거절 코드 일곱, 403(응답이 500 ms 늦음), 가드에서 기다리는 동안 온 10010·403, 리다이렉트 301·302·307·308, 줄바꿈과 U+2028·U+2029가 든 긴 retMsg, 본문이 잘린 401·403·429·500·200, 동시에 나간 두 요청의 10010, 반향의 403, VM의 503·닫힘·500. 변이 M188, M191, M192, M232, M233, M234
- 판정 기준과 근거: 서명이 거래소 예시·RFC 4231·Python hmac 값과 같다. 부류와 카운터가 표대로다. 거절 뒤 10분 동안 가짜가 받는 요청이 하나뿐이고 오류 줄이 하나다. 403 뒤 응답 + 599,999 ms까지 어떤 요청도 가짜에 닿지 않고 응답 + 600,000 ms에 간다. 어느 10초 창도 60 이하이고 거절된 요청은 받은 순간에 간다. 가드에서 기다리던 요청은 그 사이 온 키 거절·403 뒤 보내지 않는다. 리다이렉트를 따르지 않는다. 본문이 잘린 401·403도 래치·멈춤을 걸고 다음 요청을 보내지 않는다. 동시에 거절된 두 요청의 오류 줄은 하나다. 거래소의 글은 오류와 로그에 200자 한 줄로 들어간다. 실패한 밀어 넣기는 다음 본문에 남지 않는다
- 재현: `cargo test -p watcher --test sign --test client --test metrics`
- 뒷받침하는 것: C-WATCH 3부, ADR-031
- 확인하지 않은 범위: 실제 Bybit와 실제 VictoriaMetrics(키가 없고 U-01 승인이 필요하다). 응답 형식은 문서와 OMS의 분류에서 왔다. 가드는 이 프로세스의 요청만 센다(OMS의 요청은 모른다). 이 클라이언트를 부르는 시작 확인(U19a4b), 검사(U19a5~), 밀어 넣기 타이머와 종료 때의 마지막 밀어 넣기(U19a7)는 아직 없다

| 시험 | 결과 | 비고 |
|---|---|---|
| `watcher/the_strings_to_sign_and_signatures_match_independent_values` | 미실행 |  |
| `watcher/query_strings_are_sorted_and_unsafe_values_refused` | 미실행 |  |
| `watcher/every_request_carries_the_five_headers_and_the_signed_query` | 미실행 |  |
| `watcher/answers_are_classified` | 미실행 |  |
| `watcher/auth_answers_are_never_retried` | 미실행 |  |
| `watcher/http_403_pauses_every_request_for_600_s` | 미실행 |  |
| `watcher/the_guard_holds_requests_beyond_60_in_10_s` | 미실행 |  |
| `watcher/a_request_beyond_the_guard_waits_for_the_window` | 미실행 |  |
| `watcher/a_request_waiting_at_the_guard_sees_a_later_rejection_or_pause` | 미실행 |  |
| `watcher/a_redirect_is_not_followed` | 미실행 |  |
| `watcher/a_status_counts_when_its_body_is_cut` | 미실행 |  |
| `watcher/two_rejections_in_flight_write_one_error_line` | 미실행 |  |
| `watcher/the_import_lines_are_victoria_metrics_json` | 미실행 |  |
| `watcher/a_failed_push_is_dropped` | 미실행 |  |

## V-WATCH-4 — 워처의 설정 파일은 스펙 표의 모든 필드를 그 기본값과 검사대로 읽는다: 모르는 키와 표는 거부하고, mainnet은 mainnet_confirmed = true일 때만 받으며, 범위는 양 끝을 포함하고, status_url과 vm_url은 loopback만, ip_echo_url은 https만, 자격 파일은 절대 경로이고 ..이 없으며 링크를 따라 봐도 데이터 디렉터리 밖이다. 자격 파일은 링크를 따르지 않고 FIFO를 기다리지 않는 한 핸들로, 이 프로세스의 uid가 소유하고 권한이 정확히 0600 또는 0400인 일반 파일일 때만, api_key와 api_secret 둘만 비어 있지 않게 읽는다. 모든 거부는 한 줄이고, 자격 파일의 오류는 내용을 인용하지 않으며 비밀은 어디에도 찍히지 않는다. 시작 확인(U19a4b)은 query-api의 마스터 키, 다른 userID, IP에 묶이지 않은 키([]·*)와 행이 없거나 USDT로 정산되지 않는 기호를 종료 2의 이유 한 줄로 거부하고, 키 거절(auth)이면 끝내지 않고 요청 없이 떠 있으며, 그 밖의 답은 답의 시각 + 5초(403이면 + 600초)에 다시 묻는다. watcher_api_key_days_left는 expiredAt까지의 날을 내림하고 해당 없음(없음, null, 1970-01-02 이하)이면 내보내지 않는다. watcher_public_ip_ok는 반향의 주소가 마지막으로 통과한 ips에 있을 때만 1이다. IP 재확인은 지난 재확인의 마지막 답 + ip_check_ms마다 돌고, 실패하면 0으로 두고 남은 날을 지키며, 시작 규칙에 더는 맞지 않는 키는 재시작까지 모든 Bybit 요청을 멈추고 0으로 둔다. 재확인이나 5단계에서 키가 거절되면 Bybit에는 묻지 않고 반향만 ip_check_ms마다 이어 간다. 요청이 답을 기다리지 않을 때 온 답은 아무것도 바꾸지 않는다

- 상태: **미실행**
- 왜 중요한가: 틀린 키 이름이 기본값으로 조용히 바뀌거나 단어 하나로 mainnet에 붙거나, 다른 사용자가 읽을 수 있는 키 파일이나 OMS 데이터 디렉터리 안의 키 파일로 시작하면 워처의 독립성과 키 분리(설계 §7.4)가 깨진다. 거부의 이유가 여러 줄이면 시작 거부 한 줄(스펙)을 지키지 못한다. 마스터 키나 다른 계정의 키, USDC 기호로 시작하면 워처가 OMS와 다른 계정이나 자기 조회(category=linear, settleCoin=USDT)가 보지 못하는 주문을 본다. 남은 날을 올리면 만료 경보가 하루 늦고, 403 뒤 일찍 다시 물으면 OMS와 함께 쓰는 IP의 차단을 늘린다
- 수준: 단위
- 전제와 범위: 필수 필드만의 설정과 기본값, 필드마다의 거부와 수용(범위 16개의 양 끝과 그 밖), 파일 읽기, 링크로 가린 디렉터리와 ..로 쓴 소켓. OMS_TEST_DIR 아래 실제 파일의 권한 열둘, 다른 소유자(메타데이터 규칙 함수, 그리고 root가 소유한 /etc/hostname을 creds::load로), 심볼릭 링크, 디렉터리, FIFO, 내용 여덟 가지, 데이터 디렉터리 안·링크로 가린 경우. 시작 확인과 IP 재확인의 sans-IO 단계를 수동 시계로, 부류마다의 답과 기다림, 거부 뒤와 키 거절 뒤의 요청 없음은 loopback 가짜 Bybit·반향과 실제 클라이언트로
- 실제 구성요소: toml·serde, 파일 시스템(open O_NOFOLLOW|O_NONBLOCK, fstat, canonicalize), rustix geteuid, 워처 자신의 Client(시작 확인 시험의 http-fake 경로)
- 모의·시뮬레이션: 시험이 만든 다른 소유자의 파일(root만 만들 수 있어 규칙 함수에 다른 uid를 주고, 실제 파일은 /etc/hostname으로 본다). 거래소 query-api·instruments-info 답(문서의 예로 만든 가짜)과 IP 반향
- 주입한 장애: 모르는 키·표, 빠진 필수 필드, 틀린 형식의 값, mainnet 미확인, 범위 밖, loopback 아닌 주소, 앞뒤 공백·탭·줄바꿈이 든 URL, 사용자가 든 URL, 상대 경로와 .., 데이터 디렉터리 안의 자격 파일(링크로 가린 경우 포함), 같은 곳을 가리키는 두 소켓, 권한 0644 등 열, 다른 소유자의 실제 파일, 링크, 디렉터리, FIFO, 빈 키, 다른 키, TOML 아님, UTF-8 아님. 변이 M193, M235~M237. 시작 확인: 마스터 키, 다른 uid, ips []·*·주소 없음, 읽을 수 없는 키 답 여섯, USDC·소문자·빈 정산 코인, 기호 없음, 부류마다의 실패(not_sent, no_answer, rate_limited, clock, refused, ip_banned, unreadable), 키 거절 다섯(10010, 10003, 33004, HTTP 401, 5단계 10004), 읽을 수 없는 expiredAt 열하나, 반향 실패·주소 아닌 본문, 재확인의 실패·규칙 위반·10010(그 뒤 반향만, 바뀐 IP), 기다리는 요청 없이 온 답. 변이 M189, M190, M225, M238
- 판정 기준과 근거: 받는 설정은 표의 값이고 거부는 그 이유를 담은 한 줄이다. 자격 파일은 0600·0400만 받고 나머지 경우마다 그 이유로 거부하며, 어떤 오류에도 비밀 문자열이 없다. FIFO는 5초 안에 거부된다. 시작: 거부는 정한 이유 한 줄이고 그 뒤 요청이 없다. 다시 묻기는 Wait {until_ms} = 답 + 5,000ms(403은 + 600,000ms)이고 그 전에는 poll이 요청을 내지 않는다. expiredAt = 지금 + 10일이면 10, + 10일 − 1시간이면 9. 재확인은 지난 마지막 답 + ip_check_ms에 query-api를 낸다. 규칙 위반 뒤 모든 경로가 보내지지 않고 auth로 끝나며 가짜에 요청이 더 오지 않고 watcher_public_ip_ok = 0이다. 키 거절 뒤에는 ip_check_ms마다 반향 하나만 가고, 같은 IP면 1, 다른 IP면 0이다. 기다리는 요청 없이 온 답 뒤에는 같은 Step, 같은 다음 요청과 시각이다
- 재현: `cargo test -p watcher --test config --test creds --test start`
- 뒷받침하는 것: C-WATCH 4부(설정, 자격 파일, 시작 확인, IP 재확인), ADR-031
- 확인하지 않은 범위: 시험이 만든 다른 소유자의 파일(root가 필요하다; 규칙 함수와 /etc/hostname으로 본다, 그 파일이 없거나 시험의 uid가 소유하면 건너뛴다), root로 돈 시험(바로 끝난다), 끊어진 심볼릭 링크로 같은 곳을 가리키는 두 소켓. 자격 파일이 있는 디렉터리의 권한, 상위 디렉터리의 링크, 읽은 뒤의 경로 변경, 키 값의 모양. 워처 키에 출금 권한이 없다는 것(운영 문서에만 적었다). 시작 확인과 재확인을 실제 Bybit와 실제 반향에 대해 돌린 적은 없다(키가 없다, U-01). 묶이지 않은 키의 ips 모양(Q4에 없음), readOnly와 permissions(보지 않음). 바이너리와 실행 코드가 시작 확인을 돌리는 것, OMS가 꺼진 동안의 재확인(U19a7)

| 시험 | 결과 | 비고 |
|---|---|---|
| `watcher/the_config_is_validated_and_mainnet_needs_confirmation` | 미실행 |  |
| `watcher/the_config_file_is_read_and_its_refusals_name_the_file` | 미실행 |  |
| `watcher/the_credentials_file_must_be_owner_only_and_outside_the_data_dir` | 미실행 |  |
| `watcher/a_traded_symbol_that_does_not_settle_in_usdt_refuses_the_start` | 미실행 |  |
| `watcher/a_key_that_expires_in_10_days_gives_days_left_10` | 미실행 |  |
| `watcher/a_symbol_missing_from_instruments_info_refuses_the_start` | 미실행 |  |
| `watcher/a_master_key_another_uid_or_a_key_bound_to_no_ip_refuses_the_start` | 미실행 |  |
| `watcher/the_documented_epoch_expiry_exports_no_days_left` | 미실행 |  |
| `watcher/the_public_ip_is_rechecked_every_60_s` | 미실행 |  |
| `watcher/query_api_answers_other_than_auth_or_a_refusal_retry_every_5_s` | 미실행 |  |
| `watcher/an_echo_failure_or_a_body_that_is_not_an_address_gives_public_ip_ok_0` | 미실행 |  |
| `watcher/settle_coin_answers_other_than_auth_or_a_refusal_retry_every_5_s` | 미실행 |  |
| `watcher/an_auth_answer_at_start_stays_up_without_requests` | 미실행 |  |
| `watcher/a_failed_recheck_sets_public_ip_ok_0_and_keeps_days_left` | 미실행 |  |
| `watcher/a_recheck_with_another_uid_a_master_key_or_no_bound_ip_stops_every_request` | 미실행 |  |
| `watcher/an_answer_without_a_request_in_flight_changes_nothing` | 미실행 |  |

## V-WATCH-5 — 워처의 검사 엔진은 첫 라운드와 첫 하트비트를 start_ms에 내고, 한 검사의 라운드를 겹치지 않으며 늦은 라운드는 한 번만 돌린다. 라운드는 거래소 쪽을 모두 읽은 뒤에만 그 라운드만의 상태 요청을 내고, 그 답은 물은 라운드만 쓰며, 그 상한까지 저널을 읽은 뒤 비교한다. 다른 인스턴스의 상태와 그 요청을 낼 때 읽은 기록보다 낮은 상한은 읽기를 재시작까지 래치하고 그 뒤 라운드는 아무것도 묻지 않는다. 다시 읽는 저널 오류는 다음 라운드에 낫고 watcher_journal_error를 0으로 돌린다. 기한(낸 시각 + request_timeout_ms + 11초)까지 답이 없는 요청은 라운드를 실패로 닫고 늦은 답은 버린다. 차이는 지난 완료 라운드와 이번 라운드에 같은 정체로 있고 이번 라운드에서 min_age_ms 이상 늙었을 때만 확정되며(실패한 라운드는 지난 완료 라운드의 sighting을 바꾸지 않는다), 두 번 보았지만 어린 차이는 라운드를 성공으로 치지 않는다. 확정은 정체마다 한 번 세고(한 라운드에 두 번 실린 정체도 하나) 1024바이트 이하의 detail을 실은 Mismatch 하나를 내며, watcher_mismatch_active는 라운드가 그것을 보지 않으면 내려간다. W1은 OMS의 쪽 규칙대로 열린 주문을 읽고(커서 없는 가득 찬 쪽, 같은 커서, 필드 없는 행은 unreadable, 20쪽 한도), 모르는 주문(foreign), 저널에서 닫힘, 거래소 종결 뒤에도 열림, 종목·방향 다름, 확인된 주문의 거래소 부재를 차이로 보고, 확인되지 않았거나 결과 불명이거나 거래소 종결된 주문의 부재는 비교하지 않는다. 나이는 주문의 fact_at(다른 이벤트로 움직이지 않음) 또는 첫 sighting이다

- 상태: **미실행**
- 왜 중요한가: 한 번 본 차이나 막 기록된 저널 항목으로 울리면 체결이 오가는 중에 거짓 보류가 걸리고, 두 번 본 어린 차이를 성공으로 치면 하트비트가 아무것도 확정하지 않는 동안 신선한 W1을 알린다. 저널을 거래소보다 먼저 읽거나 다른 라운드의 상한을 쓰면 OMS가 방금 보낸 주문을 모르는 주문으로 본다. 답 하나를 잃어 라운드가 영영 열리면 검사가 멈춘다
- 수준: 단위(sans-IO, 수동 시계)
- 전제와 범위: 엔진 시험 도구(프로세스 안의 모델 거래소, 시험 저널 쓰기, 지연 0의 답 또는 시험이 손으로 답함): W1 시험 13개, 엔진 시험 10개. 두 검사의 라운드가 함께 열린 경우는 W1과 W2(U19a5에서는 시험의 대역 검사, U19a6부터 실제 W2의 빈 쪽)
- 실제 구성요소: 워처의 Engine, W1, Tail, JournalState, 지표 등록부, ADR-011 형식으로 쓴 저널 파일
- 모의·시뮬레이션: Bybit의 order/realtime 쪽(모델 거래소와 스크립트한 쪽), OMS의 /api/status(시험 저널의 마지막 기록 또는 정한 상한), 시계(수동)
- 주입한 장애: 한 라운드만 보인 차이, 보였다 안 보였다 하는 차이, 1.5초·1µs 모자란·정확히 2초인 저널 항목, 확인되지 않음·결과 불명·거래소 종결 주문, 빈·남의·다른 인스턴스·모르는 orderLinkId, 종목·방향 다름, 두 쪽 사이에 저널에 들어온 51개 주문, 250ms마다 같은 주문의 체결, 커서 없는 가득 찬 쪽, 같은 커서, orderId 없는 행, 목록 아닌 list, 21쪽, 커서 있는 빈 쪽, 답 없는 거래소·상태 요청, 기한 뒤의 늦은 답, 상태 요청에 온 거래소 답, 거꾸로 온 상태 답, 답 없음·durable_seq 없음·16자리가 아닌 instance_id, 다른 인스턴스, 내려간 상한, 파일보다 높은 상한(빠진 세그먼트), 모르는 이벤트, 1200바이트의 orderLinkId, 30개 동시 확정, 실패한 라운드, 보았다·실패(거래소 무응답, OMS 무응답)·보았다, 두 쪽에 실린 같은 주문, 1µs 늦게 확인된 missing_at_venue 주문. 변이 M194, M195, M198, M211~M214, M219, M224, M229, M239~M242와 시험 밖 변이 28개(모두 잡힘, U19a5 보고와 검토 1 수정)
- 판정 기준과 근거: 확정은 정한 라운드(시각)에만 오고 Mismatch의 detail이 정확히 그 문자열이다. 그 전 라운드에는 Mismatch가 없다. 미룬 라운드 뒤 성공 시각과 하트비트의 W1이 움직이지 않는다. 상태 요청은 마지막 쪽의 답 뒤에만 나온다. 기한 1ms 전에는 라운드가 열려 있고 기한에 실패한다. 래치 뒤 다섯 라운드 동안 거래소 요청이 없고 래치 줄은 하나다. detail은 1024바이트 이하이고 글자 경계에서 잘린다
- 재현: `cargo test -p watcher --test w1 --test engine`
- 뒷받침하는 것: C-WATCH 5부, ADR-031
- 확인하지 않은 범위: 실행 코드가 엔진을 돌리는 것, Mismatch를 admin.sock에 보내는 것과 다시 보내기·빚, 하트비트를 monitor.sock에 보내는 것(U19a7). 실제 Bybit의 order/realtime 응답(키가 없다, U-01)과 열린 주문의 orderLinkId 유일성(문서로 확인하지 않음). 지워진(purged) 주문은 foreign으로 보인다(종류만 다르다). 시계가 뒤로 가는 경우는 saturating_sub으로 멈추는 것을 코드로만 본다

| 시험 | 결과 | 비고 |
|---|---|---|
| `watcher/a_one_round_difference_does_not_alarm` | 미실행 |  |
| `watcher/a_two_round_difference_older_than_2_s_alarms` | 미실행 |  |
| `watcher/a_journal_entry_younger_than_2_s_does_not_alarm` | 미실행 |  |
| `watcher/an_unacked_order_missing_at_the_venue_is_not_compared` | 미실행 |  |
| `watcher/a_foreign_order_at_the_venue_is_a_mismatch` | 미실행 |  |
| `watcher/an_order_closed_in_the_journal_and_open_at_the_venue_is_a_mismatch` | 미실행 |  |
| `watcher/the_venue_is_read_before_the_journal` | 미실행 |  |
| `watcher/a_difference_whose_order_keeps_receiving_events_is_confirmed_by_its_fact_age` | 미실행 |  |
| `watcher/a_young_difference_seen_twice_defers_the_round` | 미실행 |  |
| `watcher/a_full_page_without_a_cursor_or_an_unchanged_cursor_fails_the_round` | 미실행 |  |
| `watcher/an_order_ended_at_the_venue_in_the_journal_and_not_listed_agrees` | 미실행 |  |
| `watcher/an_order_ended_in_the_journal_and_still_open_at_the_venue_is_open_after_terminal` | 미실행 |  |
| `watcher/a_late_round_runs_once_and_rounds_do_not_overlap` | 미실행 |  |
| `watcher/the_first_rounds_and_heartbeat_are_due_at_start` | 미실행 |  |
| `watcher/a_retried_journal_error_heals_and_a_latched_one_fails_every_round` | 미실행 |  |
| `watcher/a_status_of_another_instance_or_a_cap_below_the_read_before_the_request_stops_the_reader` | 미실행 |  |
| `watcher/status_answers_in_reverse_order_are_each_used_by_their_own_round` | 미실행 |  |
| `watcher/the_detail_is_at_most_1024_bytes` | 미실행 |  |
| `watcher/a_confirmed_identity_is_counted_once` | 미실행 |  |
| `watcher/a_request_without_an_answer_fails_its_round_at_the_deadline_and_a_late_answer_is_ignored` | 미실행 |  |
| `watcher/mismatch_active_counts_the_confirmed_differences_a_round_still_sees` | 미실행 |  |
| `watcher/a_failed_round_keeps_the_sightings_of_the_last_completed_round` | 미실행 |  |
| `watcher/an_order_listed_on_two_pages_is_one_difference` | 미실행 |  |

## V-WATCH-6 — 워처의 W2는 position/list(category=linear, settleCoin=USDT, 200개씩 5쪽까지)를 W1의 쪽 규칙으로 읽고(커서 없는 가득 찬 쪽, 같은 커서, 한 기호의 두 행(헤지 모드), 모르는 방향, 소수가 아닌 크기, 크기 있는 빈·없는 방향, 음수 크기, 기호 없는 행은 unreadable, 6쪽째는 실패), 기호마다 d = 거래소 − 저널(체결 + 외부 체결 + 거래소 변경 + 조정)을 차이 (W2, position, 기호, d)로 본다(범위를 넘은 d는 값 overflow인 차이). 다른 d는 처음부터 센다. 저널의 마지막 변경 시각과 거래소의 최근 변경 시각(updatedTime, 없으면 그 크기를 처음 본 라운드 seen_at) 중 알려진 것이 min_age_ms보다 어리면 첫 sighting이어도 미루고 라운드는 성공이 아니며, 모르는 시각은 늙음이라 저널이 바꾼 적 없는 기호의 거래소 포지션은 두 번째 라운드에 확정된다. 행이 없는 기호는 크기 0으로 보아 막 닫힌 포지션도 처음 0으로 본 라운드부터 2초 미루고, 첫 완전한 읽기의 크기에는 seen_at이 없으며, 모든 쪽을 읽은 뒤 실패한 라운드의 크기는 마지막 거래소 답의 시각으로 남고, 쪽 사이에서 실패한 라운드는 아무것도 남기지 않는다. 초당 5번의 체결이 60초 이어지는 동안 Mismatch가 없고, 체결이 멈춘 뒤 심은 차이는 라운드의 어느 위상에서도 3초 안에 확정된다

- 상태: **미실행**
- 왜 중요한가: 체결이 오가는 동안 W2가 울리면 거짓 보류가 걸리고, 어린 차이를 성공으로 치면 아무것도 확정하지 않는 동안 하트비트가 신선한 W2를 알린다. 모르는 시각을 어림으로 보면 OMS가 모르는 포지션(수동 거래, 놓친 체결)을 영영 확정하지 못하고, 막 닫힌 포지션을 늙게 보면 OMS가 기록하기 전에 확정한다. 다른 두 값을 확정하면 움직이는 포지션에 보류를 건다
- 수준: 단위(sans-IO, 수동 시계)
- 전제와 범위: 엔진 시험 도구(프로세스 안의 모델 거래소: 열린 주문과 포지션 목록, 경로마다의 스크립트; 시험 저널 쓰기; 상태 무응답; 지연 0의 답): W2 시험 14개, W1과 함께 돈다
- 실제 구성요소: 워처의 Engine, W2, W1, Tail, JournalState, 지표 등록부, ADR-011 형식으로 쓴 저널 파일
- 모의·시뮬레이션: Bybit의 position/list 쪽(모델 거래소와 스크립트한 쪽), OMS의 /api/status(시험 저널의 마지막 기록 또는 무응답), 시계(수동), 체결 흐름(200ms마다 거래소, 300ms 뒤 저널; 판단(모델 아님))
- 주입한 장애: 저널이 모르는 기호의 거래소 포지션, 거래소가 모르는 저널 포지션, 라운드마다 바뀌는 d, 0.5초·정확히 2초·1µs 모자란 저널 변경, 100ms 전에 바뀐 updatedTime, 한 쪽과 두 쪽의 헤지 모드 행, 커서 없는 가득 찬 쪽, 같은 커서, Long 방향, 1e3 크기, 기호 없는 행, 6쪽, 거래소 무응답, 크기 있는 빈 방향과 없는 방향, 음수 크기, 크기 0인 빈 방향, 커서 있는 빈 쪽, 1쪽 뒤에 잃은 2쪽, 소수 범위를 넘는 d, 첫 라운드의 무응답과 읽을 수 없는 쪽, 뒤에 나타난 기호, 거래소에서 닫힌 포지션(저널이 1.5초에 따라감, 따라가지 않음), updatedTime 없이 라운드마다 바뀌는 크기, OMS 무응답 중 본 크기, 60초 동안 초당 5번의 체결, 위상 0~900ms와 읽기 1ms 뒤에 심은 차이. 변이 M196, M197, M218, M223, M243~M245와 시험 밖 변이 22개(모두 잡힘, U19a6 보고와 검토 1 수정)
- 판정 기준과 근거: 확정은 정한 라운드(시각)에만 오고 Mismatch의 detail이 정확히 그 문자열이다. 미룬 라운드 동안 W2 성공 시각과 하트비트의 W2가 움직이지 않고 watcher_check_age_s{W2}가 정확히 자란다. 체결 60초 동안 Mismatch 0개. 심은 차이의 확정은 라운드 시작의 심기에 +2,000ms, 위상 p에 +3,000 − p ms, 읽기 1ms 뒤의 심기에 +2,999ms
- 재현: `cargo test -p watcher --test w2`
- 뒷받침하는 것: C-WATCH 6부, ADR-031
- 확인하지 않은 범위: W4의 마지막 execTime으로의 대체(U19a9). 실제 Bybit의 position/list 응답과 updatedTime이 무엇에 움직이는지(키가 없다, U-01). 라운드 도중 쪽 사이를 옮긴 행은 헤지 모드와 구별하지 못한다(unreadable). 호스트 시계가 거래소보다 앞선 경우는 문서의 한계일 뿐 시험하지 않았다. 주차된 체결은 세지 않는다(소유자 질문 4). updatedTime이 크기 변경에 움직이지 않으면 막 바뀐 크기를 늙게 본다(검토 1 N4, testnet 항목). 시험 도구의 지연 0으로는 비교 시계와 마지막 거래소 답의 시계를 구별하지 못한다

| 시험 | 결과 | 비고 |
|---|---|---|
| `watcher/five_fills_per_second_for_60_s_give_no_mismatch` | 미실행 |  |
| `watcher/a_difference_planted_after_the_fills_stop_is_confirmed_within_3_s` | 미실행 |  |
| `watcher/a_changing_difference_restarts_the_count` | 미실행 |  |
| `watcher/a_deferred_round_is_not_a_success` | 미실행 |  |
| `watcher/a_venue_position_in_a_symbol_the_journal_never_changed_is_confirmed_on_the_second_round` | 미실행 |  |
| `watcher/a_young_journal_change_defers_even_at_the_first_sighting` | 미실행 |  |
| `watcher/hedge_mode_rows_are_unreadable` | 미실행 |  |
| `watcher/bad_pages_and_rows_fail_the_round` | 미실행 |  |
| `watcher/a_closed_venue_position_defers_until_2_s_after_it_was_first_seen_flat` | 미실행 |  |
| `watcher/a_difference_that_changes_every_round_without_a_venue_time_is_not_a_success` | 미실행 |  |
| `watcher/sizes_found_at_the_first_round_have_no_seen_time` | 미실행 |  |
| `watcher/a_size_seen_by_a_round_that_then_fails_keeps_its_seen_time` | 미실행 |  |
| `watcher/a_round_that_failed_between_pages_leaves_no_observation` | 미실행 |  |
| `watcher/a_difference_too_large_to_subtract_is_still_a_difference` | 미실행 |  |

## V-WATCH-7 — 워처의 엔진은 확정된 차이를 그것을 실은 보내기가 "held"로 답할 때까지 빚으로 두고("already held", 보류를 보이는 저널, 실패는 갚지 않고, 라운드가 더 보지 않아도 확정 때의 detail로 보낸다), 저널 상태가 그 검사의 최근 held/already held 답 뒤에 낸 상태 요청의 상한에서 보류 꺼짐을 보일 때, 지난 보내기 시작부터 mismatch_resend_ms가 지났을 때, 그 검사의 보내기가 진행 중이 아닐 때만 보낸다(보고가 없는 보내기는 낸 시각 + 2 × request_timeout_ms + 1초에 실패하고, 그 뒤에 온 늦은 보고는 번호가 달라 아무것도 갚지 않는다). W1·W2는 라운드가 아직 보는 확정된 차이를 해제 뒤 다시 보낸다. admin.sock 클라이언트는 hello 다음 watcher_mismatch를 보내고 두 답을 읽는다. monitor.sock 클라이언트는 연결 하나를 유지하고 hello를 먼저, 그다음 W1~W4의 성공 시각(없으면 null)을 보내며, 끊김·답 없음 뒤 다음 하트비트에 다시 연다. 실행은 설정을 읽은 때부터 지표를 밀어 넣고, 시작 확인, /api/status 기다림, 재생 동안에도 IP를 다시 확인하고, 재확인에서 시작 규칙에 맞지 않는 키는 모든 Bybit 요청을 멈추며, OMS가 답하지 않으면 아무것도 판단하지 않고, 시작 거부는 코드 2, shutdown 입력은 새 라운드 없이 마지막 밀어 넣기와 모니터 연결 닫음 뒤 0으로 끝난다

- 상태: **미실행**
- 왜 중요한가: 빚을 잃으면 OMS의 메모가 이름하지 않는 차이가 사라지고, 답 앞의 상한으로 보류 꺼짐을 믿으면 보류가 걸린 줄 모르고 보내거나 거둔다. 진행 중 보내기가 둘이면 held가 무엇을 갚는지 모른다. 하트비트가 다른 검사의 시각을 실으면 게이트가 틀리게 열리고, hello 없는 하트비트는 거절된다. OMS가 내려간 동안 판단하면 저널 없이 확정한다
- 수준: 단위(sans-IO, 수동 시계)와 통합(loopback 가짜, 실제 시간 하나)
- 전제와 범위: 엔진 시험 도구(모델 거래소, 시험 저널, 시험 도구의 OMS가 watcher_mismatch에 답하고 hold_set을 씀), 가짜 admin.sock·monitor.sock(UnixListener), runtime::run과 http-fake의 Bybit·반향·/api/status·VictoriaMetrics, 바이너리의 --version과 네트워크 전 거부: 시험 20개
- 실제 구성요소: 워처의 Engine, W1, W2, oms::admin_send, oms::Monitor, oms::Status, runtime::run, rest::Client, start::Start·IpCheck, metrics::Exporter, ADR-011 형식으로 쓴 저널 파일, Unix 소켓과 loopback HTTP
- 모의·시뮬레이션: OMS(소켓 답과 hold_set을 쓰는 시험 도구, /api/status), Bybit와 IP 반향과 VM(http-fake), 시계(수동, 종단 시험만 실제)
- 주입한 장애: 첫 보내기 실패(unavailable), 사라진 차이, 다른 보내기가 건 보류, 저널이 아직 못 본 보류와 already held, 늦은 첫 답, 보고 없는 보내기, 기한 뒤에 온 늦은 held, 상한 아래에서 끝난 재생, 재확인의 마스터 키, 답 앞에 낸 상태 요청의 상한, 없는 소켓, bad_request·overloaded 답, 닫힌 연결, 답 없는 하트비트, W2만 실패, 상태 500·무응답, 401 키, 마스터 키, 0644 자격 파일, Location 있는 302. 변이 M199, M200, M201, M220, M226, M228, M246과 시험 밖 변이 25개(모두 잡힘, U19a7 보고와 검토 1 수정)
- 판정 기준과 근거: 보내기는 정한 시각(라운드)에만, 정한 detail로, 정한 수만큼 온다. 하트비트 줄의 W1·W2는 그 순간 엔진의 성공 시각과 같고 첫 것은 모두 null. 소켓 줄은 hello가 먼저이고 정확히 그 문자열이다. 종단 시험에서 W1·W2가 있는 하트비트, W1 불일치, 서명된 네 경로, watcher_up이 든 밀어 넣기, 코드 0
- 재현: `cargo test -p watcher --test admin --test heartbeat --test runtime --test binary`
- 뒷받침하는 것: C-WATCH 7부, ADR-031, C-LIVE 2부, C-HOLD
- 확인하지 않은 범위: 실제 oms-node 프로세스와 실제 Bybit에 붙여 돈 적이 없다(키가 없다, U-01). SIGTERM·SIGINT에서 shutdown 입력으로의 연결(main.rs)은 시험하지 않았다. 빚은 메모리에만 있어 재시작하면 잃는다. W3·W4의 보내기와 하트비트 시각은 V-WATCH-8·V-WATCH-9가 본다. 재생 중에는 하트비트가 없다(소유자 질문 9). 종단 시험만 실제 시간이고 짧은 간격(500ms)으로 돌았다

| 시험 | 결과 | 비고 |
|---|---|---|
| `watcher/a_confirmed_mismatch_is_sent_once_and_again_after_a_clear` | 미실행 |  |
| `watcher/a_confirmed_mismatch_is_sent_until_answered_even_after_the_difference_is_gone` | 미실행 |  |
| `watcher/a_difference_confirmed_under_a_hold_is_sent_after_the_clear` | 미실행 |  |
| `watcher/a_second_send_waits_for_the_first_answer` | 미실행 |  |
| `watcher/a_hold_seen_off_at_a_cap_sampled_before_the_answer_does_not_send` | 미실행 |  |
| `watcher/a_late_report_of_one_send_pays_only_what_that_send_carried` | 미실행 |  |
| `watcher/the_admin_client_says_hello_and_reads_both_replies` | 미실행 |  |
| `watcher/heartbeats_carry_the_success_time_of_each_check` | 미실행 |  |
| `watcher/the_heartbeat_says_hello_first_and_reconnects` | 미실행 |  |
| `watcher/a_reply_that_does_not_come_in_time_reopens_the_connection` | 미실행 |  |
| `watcher/the_watcher_runs_against_loopback_fakes_end_to_end` | 미실행 |  |
| `watcher/without_an_answering_oms_nothing_is_judged` | 미실행 |  |
| `watcher/the_shutdown_input_ends_cleanly` | 미실행 |  |
| `watcher/the_ip_recheck_runs_while_the_oms_is_down` | 미실행 |  |
| `watcher/the_ip_recheck_runs_during_the_boot_replay` | 미실행 |  |
| `watcher/a_recheck_that_fails_a_start_rule_stops_every_bybit_request` | 미실행 |  |
| `watcher/a_key_rejected_at_start_is_exported` | 미실행 |  |
| `watcher/the_status_client_sends_the_loopback_host_and_follows_no_redirect` | 미실행 |  |
| `watcher/a_start_refusal_ends_the_run_with_exit_code_2` | 미실행 |  |
| `watcher/version_prints_and_anything_else_exits_2` | 미실행 |  |

## V-WATCH-8 — 워처의 W4는 execution/list를 고정된 끝(첫 쪽의 시계)과 시작 max(워터마크 − w4_overlap_ms, ⌈체결 바닥/1000⌉)(7일로 자름)의 창으로 훑고, 라운드마다 w4_max_pages쪽까지 읽어 더 긴 훑기는 다음 라운드가 같은 창과 커서로 이으며(성공 시각은 끝난 훑기에서만), 빈 쪽이나 빈 커서에서 끝내고, 바뀌지 않은 커서와 읽을 수 없는 쪽은 라운드를 실패시켜 같은 커서로 다시 읽되 연달아 3번이나 50라운드면 같은 시작과 새 끝으로 다시 시작한다. 행은 execType으로 먼저 나눠 Trade는 저널 체결과, AdlTrade·BustTrade·Delivery·Settle은 저널 거래소 변경과 키 <symbol>:<execId>로 비교하고, Funding은 비교하지 않으며, 모르는 형은 끝난 훑기에서 바로 확정하고, 형이 필요로 하는 필드가 없는 행과 execTime이 훑기의 끝보다 뒤인 행은 unreadable_row, 바닥 아래 행과 수량 0인 Trade·거래소 변경 행은 무시한다. 차이는 두 훑기와 execTime 나이로 확정하고 어린 차이는 훑기를 실패시키지 않는다. 워터마크는 확정되지 않은 가장 오래된 행(execTime 없는 행은 모든 행보다 오래됨) 아래의 가장 새 행으로만, 뒤로 가지 않게 움직인다. held 답(already held는 갚지 않는다)으로 갚고 그 답 뒤에 낸 상한에서 보류 꺼짐을 본 W4 정체는 거두어 다시 보내지도 세지도 않는다

- 상태: **미실행**
- 왜 중요한가: 쪽 상한이 없으면 큰 창이 예산을 넘기고, 이은 훑기가 1쪽부터면 끝나지 않는다. 겹침이 없으면 늦게 실린 행을 놓치고, 워터마크가 확정되지 않은 행을 지나면 그 행을 다시 읽지 않는다. Funding을 체결로 보거나 ADL을 체결과 맞추면 맞는 저널에서 보류가 걸린다. 모르는 형이 두 번째 훑기를 기다리면 OMS가 분류하지 못한 실행의 확정이 늦다. 해제 뒤 W4를 다시 보내면 조용한 계좌에서 운영자가 보류를 풀 수 없고, 답 앞의 상한이나 held 없이 거두면 어느 메모에도 없는 차이가 사라진다. 끝 뒤의 execTime을 워터마크로 믿으면 그 뒤 창이 [end, end]가 되어 W4가 조용히 멈추고, 수량 0 행을 비교하면 OMS가 기록하지 않는 행으로 거짓 보류가 걸린다
- 수준: 단위(sans-IO, 수동 시계)와 통합(loopback 가짜, 실제 시간 하나)
- 전제와 범위: 엔진 시험 도구(프로세스 안의 모델 거래소의 체결 목록: 창 안을 새것부터 100개씩, 커서 e<번호>; 시험 저널; 시험 도구의 OMS가 watcher_mismatch에 답하고 hold_set을 씀), 종단 시험의 http-fake execution/list: 시험 34개
- 실제 구성요소: 워처의 Engine, W4, W1, W2, ADR-011 형식으로 쓴 저널 파일, 종단 시험의 runtime::run과 rest::Client
- 모의·시뮬레이션: Bybit execution/list(모델 거래소와 대본 쪽), OMS(시험 도구), 시계(수동, 종단 시험만 실제)
- 주입한 장애: 250행 3쪽, 1,200행 12쪽(마지막 쪽 가득 참·빈 커서), 빈 쪽, 바뀌지 않은 커서, 연달아 실패한 쪽 3번, 멈춘 라운드 사이의 실패, 50라운드 안 끝난 훑기, 7일 넘는 빈 창, 늦게 실린 행, 사라진 새 행(워터마크 뒤로), 펀딩 행(수량·방향 있음과 없음), ADL·BustTrade(있음·없음·다른 종류), 다른 수량의 체결, 모르는 형(한 쪽과 여러 라운드), 수량·키·execTime 없는 행, 바닥 아래와 바닥 위 행, 어린 차이 두 번, 해제 뒤 조용한 계좌, 창을 떠났다 돌아온 행, 바뀐 값, 답 앞에 낸 상한, 다른 검사의 보류 아래 확정, 끝 뒤의 execTime, 수량 0인 Trade·Settle·AdlTrade, W1의 held 뒤 W4의 already held, 연달아 실패한 라운드 9번. 변이 M202~M208, M215~M217, M222, M227, M247~M250과 시험 밖 변이 27개(모두 잡힘, U19a8 보고)
- 판정 기준과 근거: W4의 Mismatch는 정한 훑기(라운드)에만, 정한 detail로, 정한 수만큼 온다. 요청의 startTime·endTime·cursor가 규칙의 값과 같고, 쪽 수가 정한 수이며, W4의 성공 시각과 watcher_check_age_s{W4}·watcher_differences{W4}·watcher_mismatch_active{W4}가 정한 값이다
- 재현: `cargo test -p watcher --test w4 --test runtime`
- 뒷받침하는 것: C-WATCH 8부, ADR-031
- 확인하지 않은 범위: W4의 가득 찬 마지막 쪽 한계: 거래소가 행이 더 있는데 가득 찬 쪽을 빈 커서로 끝내면 그 행은 비교하지 않는다(W2가 포지션으로 본다, 소유자 질문 11). 실제 Bybit execution/list의 응답, Funding·ADL 행의 모양, 빈 nextPageCursor의 뜻(facts Q9.9, 설계 R3 T15; 키가 없다, U-01). w4_initial_lookback_ms보다 오래 내려가 있던 뒤의 오래된 체결. 거둔 정체는 메모리에만 있어 재시작 뒤 다시 확정된다. 하트비트의 W4 시각은 V-WATCH-9, 예산은 V-WATCH-10이 본다

| 시험 | 결과 | 비고 |
|---|---|---|
| `watcher/a_missing_trade_needs_two_sweeps` | 미실행 |  |
| `watcher/the_sweep_starts_10_s_before_the_last_confirmed_exec_time` | 미실행 |  |
| `watcher/a_round_of_250_executions_is_compared_in_full_over_3_pages` | 미실행 |  |
| `watcher/a_row_loaded_late_inside_the_overlap_is_compared` | 미실행 |  |
| `watcher/the_watermark_never_moves_back` | 미실행 |  |
| `watcher/executions_1200_are_compared_over_two_rounds_and_the_w4_age_rises_between` | 미실행 |  |
| `watcher/an_empty_page_ends_the_sweep` | 미실행 |  |
| `watcher/an_unchanged_cursor_is_a_failed_page` | 미실행 |  |
| `watcher/a_failed_page_three_rounds_in_a_row_restarts_the_sweep_with_a_new_end` | 미실행 |  |
| `watcher/a_sweep_not_complete_after_50_rounds_restarts` | 미실행 |  |
| `watcher/an_adl_trade_the_journal_lacks_is_a_mismatch` | 미실행 |  |
| `watcher/a_trade_or_venue_change_with_other_details_is_a_difference` | 미실행 |  |
| `watcher/funding_rows_beside_known_fills_are_no_mismatch_while_a_position_is_held` | 미실행 |  |
| `watcher/an_unknown_exec_type_is_a_mismatch_at_once` | 미실행 |  |
| `watcher/an_unknown_exec_type_in_a_multi_round_sweep_is_confirmed_when_the_sweep_completes` | 미실행 |  |
| `watcher/a_funding_row_without_side_or_qty_is_ignored` | 미실행 |  |
| `watcher/a_trade_row_without_qty_is_an_unreadable_row_and_needs_two_sweeps` | 미실행 |  |
| `watcher/a_row_without_a_key_is_named_by_its_text` | 미실행 |  |
| `watcher/an_unconfirmed_row_without_a_readable_exec_time_holds_the_watermark` | 미실행 |  |
| `watcher/a_trade_at_the_fill_floor_millisecond_is_ignored` | 미실행 |  |
| `watcher/an_empty_window_logs_once_per_state_change` | 미실행 |  |
| `watcher/the_newest_agreeing_exec_time_is_kept_per_symbol` | 미실행 |  |
| `watcher/a_confirmed_row_is_not_sent_again_after_clear_hold_in_a_quiet_account` | 미실행 |  |
| `watcher/a_dismissed_row_that_leaves_the_window_and_a_changed_value_are_sent_anew` | 미실행 |  |
| `watcher/a_hold_seen_at_a_cap_sampled_before_the_answer_does_not_dismiss` | 미실행 |  |
| `watcher/a_row_confirmed_under_an_older_hold_is_sent_once_after_that_hold_is_cleared` | 미실행 |  |
| `watcher/a_young_difference_seen_twice_does_not_fail_the_sweep` | 미실행 |  |
| `watcher/a_trade_at_the_floor_itself_is_compared` | 미실행 |  |
| `watcher/a_round_that_reads_a_page_ends_the_run_of_failed_rounds` | 미실행 |  |
| `watcher/a_row_after_the_sweeps_end_is_an_unreadable_row_and_does_not_blind_w4` | 미실행 |  |
| `watcher/a_trade_or_venue_change_of_quantity_0_is_ignored_as_the_oms_ignores_it` | 미실행 |  |
| `watcher/an_already_held_answer_to_a_w4_send_does_not_pay_it` | 미실행 |  |
| `watcher/the_restart_warn_line_is_logged_once_per_run_of_failed_rounds` | 미실행 |  |
| `watcher/the_watcher_runs_against_loopback_fakes_end_to_end` | 미실행 |  |

## V-WATCH-9 — 워처의 W3은 w3_ms마다 wallet-balance?accountType=UNIFIED의 UNIFIED 계좌 하나(정확히 하나, 세 필드가 저널 소수, 유지증거금이 음수가 아님; 아니면 unreadable로 라운드 실패)를 OMS의 공식으로 판단한다: account_mm_rate는 maintenance > 0이고 maintenance × 10 000 ≥ account_mm_rate_max_bps × margin_balance(같으면 건다), daily_loss는 저널의 일 기준값이 오늘의 UTC 날이고 (base − totalEquity) × 10 000 > daily_loss_bps × base(오늘의 기준이 없으면 판단하지 않는다). 정체는 종류뿐이고 두 라운드 연속에서 나이 없이 확정하며, 위반이 남아 있으면 해제 뒤 다시 보낸다. W2는 W4가 마지막으로 맞춘 그 기호의 execTime이 updatedTime이나 seen_at보다 새면 그것을 거래소의 변경 시각으로 쓴다. 하트비트의 W3·W4는 그 검사의 마지막 성공 시각이고, 실행은 W2(W4의 시각 지도와 함께), W3, W4를 돌리며 엔진 전에는 네 검사의 나이를 boot_ms부터 내보낸다

- 상태: **미실행**
- 왜 중요한가: W3이 한 라운드로 확정하면 순간의 값 하나로 보류가 걸리고(설계 "한도 위반 2회 연속"), 한도와 같은 값을 통과시키면 OMS가 감소 전용으로 들어가는 계좌를 워처가 괜찮다고 본다. 어제의 기준으로 일 손실을 판단하면 새 날의 첫 시세 변동이 거짓 보류가 된다. 두 UNIFIED 계좌 중 하나를 고르거나 음수 유지증거금을 읽으면 OMS가 거부하는 보고로 판단한다. W2가 W4의 체결 시각을 쓰지 않으면 updatedTime이 없거나 움직이지 않는 거래소에서 체결이 막 맞은 기호의 차이를 어린 채로 확정한다. 하트비트가 W3·W4를 null로 보내면 OMS 기록에서 두 검사가 사라진다
- 수준: 단위(sans-IO, 수동 시계)와 통합(loopback 가짜, 실제 시간 하나)
- 전제와 범위: 엔진 시험 도구(프로세스 안의 모델 거래소에 지갑 계좌 목록과 대본, 포지션과 체결; 시험 저널의 day_base_set·hold_cleared; 시험 도구의 OMS), 가짜 monitor.sock과 실제 oms::Monitor, runtime::checks, runtime::run과 http-fake의 wallet-balance 경로: 시험 9개
- 실제 구성요소: 워처의 Engine, W3, W2, W4, runtime::checks, runtime::run, oms::Monitor, rest::Client(종단 시험), ADR-011 형식으로 쓴 저널 파일
- 모의·시뮬레이션: Bybit wallet-balance·position/list·execution/list(모델 거래소와 http-fake), OMS(시험 도구), 시계(수동, 종단 시험만 실제)
- 주입한 장애: 한도보다 10^-8 아래, 한도와 같음, 한 라운드만의 위반, 라운드마다 바뀌는 금액, 잔고 ≤ 0, 유지증거금 0, 기준 100에서 95.00과 94.99, 해제 뒤 이어지는 손실, 오늘의 새 기준, 기준 없음, 어제의 기준, 자정을 넘은 오늘의 기준, UNIFIED 없음·둘, 빈 소수, 음수 유지증거금, 지수, 목록이 아닌 list, 거래소 무응답, updatedTime 없는 기호와 늙은 updatedTime의 기호에 어린 W4 시각, 어느 시각도 없는 기호, 실패하는 W3 라운드, 엔진 전의 키 거절, 다른 검사의 간격으로 도는 W3·W4. 변이 M251~M254와 시험 밖 변이 17개(모두 잡힘, U19a9 보고)
- 판정 기준과 근거: W3의 Mismatch와 확정 줄은 정한 라운드에만 정한 detail로 오고, watcher_differences{W3}·watcher_mismatches_total{W3}·watcher_mismatch_active{W3}와 W3의 성공 시각이 정한 값이며, 읽을 수 없는 답마다 watcher_request_errors_total{wallet-balance, unreadable}이 하나씩 오른다. W2의 확정은 W4의 시각이 2초 늙은 라운드에 정한 detail로 온다. 하트비트 줄의 W3·W4는 그 순간 엔진의 성공 시각과 같다. 종단 시험의 하트비트에 W1~W4 시각이 모두 있다. runtime::checks로 돈 W3·W4의 성공 시각은 +1~+5초에 각자의 간격(W3 2초, W4 5초)으로만 움직인다(검토 1 S1)
- 재현: `cargo test -p watcher --test w3 --test w2 --test heartbeat --test runtime`
- 뒷받침하는 것: C-WATCH 9부, ADR-031, C-MARGIN
- 확인하지 않은 범위: 실제 Bybit wallet-balance의 응답(8자리를 넘는 소수를 보내는지; 키가 없다, U-01). 자정부터 OMS의 새 day_base_set까지는 일 손실을 판단하지 않는다(소유자 질문 6). OMS의 유지증거금 여유(MmBuffer) 규칙은 W3에 없다. 워처의 [w3] 한도가 OMS 설정과 같은지는 운영자의 몫이다. 범위를 넘는 곱을 위반으로 보는 규칙은 시험하지 않았다(실제 금액에서는 생기지 않는다; 검토 1 N1). 미래 날의 기준은 판단에서 빼는지 시험하지 않았다(OMS가 쓰지 않는다)

| 시험 | 결과 | 비고 |
|---|---|---|
| `watcher/a_margin_rate_at_the_limit_on_two_rounds_alarms` | 미실행 |  |
| `watcher/a_daily_loss_beyond_todays_base_on_two_rounds_alarms` | 미실행 |  |
| `watcher/without_todays_base_only_the_margin_rate_is_judged` | 미실행 |  |
| `watcher/an_unreadable_wallet_fails_the_round` | 미실행 |  |
| `watcher/a_missing_updated_time_falls_back_to_w4s_last_exec_time` | 미실행 |  |
| `watcher/w3_and_w4_success_times_reach_the_heartbeat` | 미실행 |  |
| `watcher/the_runtime_runs_w2_w3_and_w4_and_w2_reads_w4s_exec_times` | 미실행 |  |
| `watcher/a_key_rejected_at_start_is_exported` | 미실행 |  |
| `watcher/the_watcher_runs_against_loopback_fakes_end_to_end` | 미실행 |  |

## V-WATCH-10 — W1·W2·W3·W4를 설계 간격(1초, 1초, 2초, 5초)으로 돌린 모의 1분(수동 시계, 실제 rest::Guard를 거친 모델 거래소, 요청 지연 0)에서 5초에 체결 30개를 고르게 주면 대조 요청은 W1 60 + W2 60 + W3 30 + W4 12 = 162개, 초당 2.7 이하이고 차이도 실패한 라운드도 없다. 5초에 100개면 192개(초당 3.2; 정상 상태는 훑기당 4쪽, 초당 3.3)로 설계 예산을 넘지만 W4는 12 훑기를 모두 자기 라운드에서 끝내고 정상 상태의 창은 15초다. 열린 주문 500개(W1 라운드 11쪽)에서는 가드가 10초에 60을 지키며 W1 라운드를 늘리지만 하트비트마다 W1·W2의 나이는 10초 아래다

- 상태: **미실행**
- 왜 중요한가: 완료 기준이 요구하는 대조 예산(2.7 req/s, 설계 §7.5)을 잰다. W4의 겹침과 모든 커서 규칙이 큰 부하에서 그것을 넘는다는 충돌(소유자 질문 1)을 숫자로 보인다. 많은 열린 주문에서 가드가 W1·W2를 게이트의 10초 너머로 늘리면 OMS가 감시 없음으로 거래를 멈춘다
- 수준: 단위(sans-IO 엔진과 sans-IO 가드, 수동 시계)
- 전제와 범위: 시험 쪽 스케줄러(10ms 단계, 가드 앞에서 기다리는 요청은 먼저 온 것부터), 엔진 시험 도구의 모델 거래소(열린 주문, 포지션, 체결, 지갑)와 시험 저널: 시험 3개
- 실제 구성요소: 워처의 Engine, W1, W2, W3, W4, rest::Guard, ADR-011 형식으로 쓴 저널 파일
- 모의·시뮬레이션: Bybit(모델 거래소, 지연 0), OMS(시험 도구), 요청을 내보내는 클라이언트(시험 쪽 스케줄러), 시계(수동)
- 주입한 장애: 5초에 체결 30개와 100개(고르게, 저널이 같은 단계에 기록), 열린 주문 500개
- 판정 기준과 근거: 30/5초: 경로별 60·60·30·12와 합 ≤ 162, 비율 ≤ 2.7, Mismatch·확정·실패한 라운드 0. 100/5초: 2.7 < 비율 ≤ 3.4, W4 훑기 12개가 각자의 라운드 시각에 끝나고 첫 훑기 뒤의 창이 16초 이하, Mismatch 0. 500개: 어느 10초에도 요청 ≤ 60, 첫 하트비트 뒤 모든 하트비트에서 W1·W2의 나이 < 10초, Mismatch 0, W1·W2의 실패한 라운드 0
- 재현: `cargo test -p watcher --test budget -- --nocapture`
- 뒷받침하는 것: C-WATCH 9부, ADR-031
- 확인하지 않은 범위: 완료 기준 자신의 한도: 5초에 체결 100개에서 초당 2.7 이하는 그대로는 충족되지 않는다(범위의 W4 규칙을 따랐다, 소유자 질문 1). 그 부하의 비율(3.2, 정상 상태 3.3; 스펙의 어림 ≈ 3.1)은 budget::at_100_executions_per_5_s_the_rate_is_about_3_1_and_w4_keeps_up가 잰다. 실제 요청 지연과 실제 클라이언트의 가드 대기 순서(순서 없이 100ms마다 다시 묻는다), 고르지 않은 체결 몰림, IP 확인 요청은 모형에 없다. 실제 Bybit로 잰 적이 없다(키가 없다, U-01)

| 시험 | 결과 | 비고 |
|---|---|---|
| `watcher/at_30_executions_per_5_s_the_reconciliation_rate_in_a_simulated_minute_stays_at_or_below_2_7` | 미실행 |  |
| `watcher/at_100_executions_per_5_s_the_rate_is_about_3_1_and_w4_keeps_up` | 미실행 |  |
| `watcher/at_500_open_orders_the_guard_stretches_w1_rounds_below_10_s` | 미실행 |  |

## V-WATCH-11 — 워처의 [telegram]은 token_file 하나만 받고(모르는 키 거부) mainnet은 그것 없이 시작하지 않는다(testnet은 warn 한 줄). 토큰 파일은 설정의 경로 규칙과 자격 파일의 파일 규칙(링크 아님, 일반 파일, 실효 uid 소유, 정확히 0600·0400)을 여는 핸들에 적용하고, 적힌 경로나 가리키는 곳이 git 저장소 안이면 읽기 전에 거부하며, {"token": "<digits>:<[A-Za-z0-9_-]+>", "chat_id": 비지 않은 문자열 또는 정수} 말고는 거부한다. 거부는 내용과 토큰을 인용하지 않고, 실행은 그것을 시작 확인 전에 종료 코드 2로 끝내며 요청을 보내지 않는다. 읽은 값과 설정의 Debug는 토큰을 보이지 않는다. 메시지는 [WATCH P1] bybit {name} {MM-DD HH:MM:SS KST}로 시작하고, split은 각 부분을 3,500 UTF-16 단위 이하로 줄 끝에서(긴 줄은 문자 단위로, 이모지를 쪼개지 않고) 자르며 글자를 잃지 않는다(부분 경계와 맨 앞의 빈 줄은 사라질 수 있고, 빈 부분은 만들지 않는다). Outbox는 50개를 넘으면 가장 오래된 부분을 버리고 세고, 큐가 빈 뒤 알림 하나로 그 수를 알리며, 보내는 중 버려진 부분을 두 번 지우거나 세지 않는다. outcome은 2xx와 ok true만 Delivered, 400·413은 Refused(버리고 셈), 나머지는 Retry(두고 흘려보내기를 끝냄)다. Repeat는 시작 한 번, 300,000 ms마다 최신 문구 한 번, 해소: <시작 문구> 한 번을 낸다. 송신 태스크(alert::spawn, U19b1b)는 경로를 순서대로 써서 네트워크 오류면 다음 경로(DNS 다음 고정 주소)로 가고, HTTP 답이 결정하며, 답한 경로가 다음부터 맨 앞이다. 부분을 순서대로 하나씩 보내고 Retry에서 그 부분을 두고 흘려보내기를 끝낸다. 새 메시지가 Notify로 바로 깨우고, 그 밖에는 흘려보내기가 끝난 주입된 시계 + 5,000 ms에 실행의 틱(Handle::tick)만 깨운다(자기 타이머 없음). 답이 오지 않는 요청은 실제 시간 10,000 ms의 요청 한도에서 실패로 세고 그 부분을 둔다. 로그 줄은 경로와 원인을 적고 토큰과 URL을 적지 않는다. finish(5,000)은 마지막 흘려보내기를 실제 시간 5,000 ms 안에서 하고 남은 부분을 한 줄로 센다. 네 지표(queued, delivered, dropped, failures)가 이것을 따른다. P1 조건(U19b1c): 엔진은 라운드마다 새로 확정한 식별자의 내용을 Action::Confirmed 하나로 그 라운드의 Mismatch보다 앞에 내고 다시 보내기는 내지 않는다. 불일치 P1은 확정마다이고 검사마다 첫 것은 바로, 그 뒤 마지막 메시지에서 60,000 ms에 하나이며 그 사이 확정은 모아(내용 다섯, 각 600 UTF-16 단위, 나머지는 수) 다음 메시지로 간다. Client::auth_stop은 래치를 건 첫 답(retMsg, HTTP 401의 본문)이나 규칙 멈춤의 이유를 바꾸지 않고 들며, 실행은 틱마다 그것과 start::PublicIp(게이지가 0인 이유)를 반복 규칙에 줘서 다시 보내지 않는 답(10010·10003·33004·401 포함)마다 답 뒤의 첫 틱에 하나, 5분마다 하나, 공인 IP는 해소 하나를 보낸다. 송신기는 시작 확인 전에 뜨고(2단계의 키 거부도 알림), 시작 거부는 P1 하나를 보내고 5,000 ms 안에 흘려보낸 뒤 종료 코드 2, 종료는 60초 간격 안에 모인 불일치 메시지를 간격과 상관없이 넣고 U19a 단계 뒤 흘려보낸다. 시작 거부의 이유는 200자로 자른다. 실행의 틱이 송신기의 다시 보내기를 깨운다. [telegram]이 없으면 모든 P1(불일치, 키 거부, 시작 거부)은 warn 한 줄이다

- 상태: **미실행**
- 왜 중요한가: 토큰이 오류·로그·Debug·git 저장소에 나오면 누구나 그 봇으로 운영자 채팅에 쓸 수 있다. mainnet의 워처가 [telegram] 없이 돌면 OMS가 죽었을 때 아무도 모른다(설계 §7.7). 보관함이 새 부분을 버리면 가장 최근의 P1(대개 가장 중요한 것)을 잃고, 400을 다시 보내면 낫지 않는 메시지가 큐를 막으며, 500을 버리면 텔레그램이 잠깐 아플 때 P1을 잃는다. 반복 규칙이 매번 보내면 지속 조건 하나가 채팅을 덮는다. 4,096 UTF-16을 넘는 메시지는 텔레그램이 거부한다. DNS가 막힌 곳에서 고정 경로로 가지 않으면 OMS가 죽었다는 P1이 닿지 않는다. 송신이 자기 실제 시간 타이머로 돌면 수동 시계 시험이 간격을 증명하지 못하고, 종료가 한도 없이 기다리면 워처가 멈추지 않는다
- 수준: 단위(순수 함수, 파일 시스템)와 통합(runtime::run과 송신 태스크, loopback 가짜, 실제 시간)
- 전제와 범위: watcher::alert의 순수 부분, Config::from_toml, creds::load_token과 token_file_rules(시험 디렉터리 OMS_TEST_DIR의 파일, 링크, FIFO, .git 디렉터리와 worktree의 .git 파일), runtime::run의 시작(http-fake), alert::spawn의 송신 태스크(런타임 없이, http-fake와 dead_addr, telegram.invalid의 고정 경로), U19b1c의 P1 조건(엔진 rig와 수동 시계, Start·IpCheck를 손으로, 클라이언트는 http-fake)과 runtime::run의 연결(loopback 텔레그램 가짜, 수동 시계와 실제 시계): 시험 29개
- 실제 구성요소: 워처의 alert(Token, message, kst, split, Outbox, outcome, Repeat, Route, spawn, Handle, Task, MismatchNotices, Alerts, mismatch_text, auth_text, public_ip_text, refusal_text), engine::Action::Confirmed, rest::Client::auth_stop, start::PublicIp, config, creds, runtime::run, Endpoints::of
- 모의·시뮬레이션: 텔레그램(http-fake, 평문 http; 고정 경로는 telegram.invalid를 가짜 주소로; 실행 시험은 Route::direct 하나), Bybit(http-fake, 시작 거부 시험만), 시계(분할·보관함·반복은 시각 인자, 송신 태스크는 수동 시계와 시험이 부르는 틱, 실행 시험은 수동 시계와 실제 100ms 틱)
- 주입한 장애: 정확히 3,500 단위와 하나 넘음, 한글 3,500자, 이모지 1,751개, 한 단위 남고 이모지, 8,000자 한 줄, 정확히 두 부분인 줄, 섞인 긴 텍스트, 부분 경계와 맨 앞의 빈 줄, 경계의 줄바꿈; 51번째 부분, 보내는 중 넘쳐 버려진 부분의 전달과 거부, 알림을 보낸 뒤의 새 버림; 200·ok false·400·413·401·429·500·502·읽을 수 없는 본문; 5분 전·5분 정각·해소·다시 시작; 그룹·다른 사용자·실행·set-id 비트, 다른 소유자, 링크, 디렉터리, FIFO, 없는 파일, 데이터 디렉터리 안(링크로 가린 경우도), 상대 경로, .., 저장소 안(.git 디렉터리, worktree의 .git 파일, 저장소를 가리키는 링크), 객체 아닌 JSON, 형식이 틀린 토큰, 빈·소수 chat_id, mainnet에 [telegram] 없음, 모르는 키, 0640 토큰 파일로 시작; 연결 거부되는 첫 경로, 두 경로 모두 연결 거부, 500·502 뒤 200, 400과 버림 알림, 기한 1 ms 전의 틱과 틱 없는 시계 이동, 종료 때 남은 부분, 답하지 않는 텔레그램의 요청 한도와 그때의 종료. 한 라운드에 두 식별자 확정, 계속 보는 라운드, 해제 뒤 다시 보내기, 사라졌다 돌아온 식별자, 60초 1 ms 전과 정각, 두 검사, 시계가 뒤로 감, W4 확정 300개와 600 단위 넘는 내용, 이름 64자; 10010·10003·33004·10004(여러 줄 retMsg)·HTTP 401, 뒤의 규칙 멈춤, 규칙 멈춤 먼저; 목록 밖 주소, 재확인의 맞는 주소와 query-api 502; 시작 2단계의 10010과 텔레그램 500 뒤 다시 보내기, 마스터 키 시작 거부(답하는 텔레그램, 답하지 않는 텔레그램, [telegram] 없음), 실행 전체의 남의 주문, 60초 안의 두 번째 확정 뒤 종료([telegram] 있음과 없음), 300자 시작 거부 이유. 변이 M255~M264와 시험 밖 변이 17개(U19b1a), 19개(U19b1b), 16개(U19b1c; 첫 실행 15개, 놓친 하나는 시험을 더해 잡음)와 5개(U19b1c 리뷰 1)
- 판정 기준과 근거: 각 부분은 3,500 단위 이하이고 비지 않으며, 줄바꿈을 뺀 내용이 원문과 같고 정한 곳에서 잘린다(경계와 맨 앞의 빈 줄은 빈 부분이 되지 않는다). Outbox의 순서·len·dropped·dropped_total과 알림 문구가 정한 값이다. outcome이 정한 부류와 문구를 낸다. Repeat가 정한 시각에만 정한 문구를 낸다. 설정과 토큰 파일의 거부 줄은 정한 이유를 담고 토큰·내용을 담지 않으며 한 줄이다. 받는 파일은 정한 Token이 된다. 거부된 토큰 파일로 시작한 실행은 종료 코드 2, error 줄 하나, 요청 0개다. testnet에 [telegram]이 없으면 warn 줄이 정확히 하나다. 송신 태스크 시험: 가짜가 받은 요청의 수·순서·경로·본문, next_flush의 값, 네 지표의 값, 로그 줄에 토큰이 없음, finish의 반환값과 그 줄, 답하지 않는 가짜에서 finish(5,000)이 4.9~7초에 돌아옴, 답하지 않는 가짜에 보낸 요청이 9.5~13초에 실패 하나(원인 timed out)와 next_flush를 남김. U19b1c: Confirmed의 수·순서·내용, MismatchNotices가 정한 시각에만 정한 문구, auth_stop의 code·http·text, 가짜 텔레그램이 받은 sendMessage의 수와 문구(정확히 하나, 반복은 5분 뒤), 시작 거부의 종료 코드 2 전 메시지 하나와 답하지 않는 텔레그램에서 4.5~9초, [telegram] 없을 때의 warn 줄 수(불일치는 내용까지), 종료 뒤 가짜가 받은 불일치 메시지 둘, drain 뒤 빈 MismatchNotices, 시작 거부 문구의 200자
- 재현: `cargo test -p watcher --test alert --test config --test creds --test runtime --test engine --test client`
- 뒷받침하는 것: C-WATCH 10부, ADR-032
- 확인하지 않은 범위: 실제 텔레그램, 실제 DNS, 실제 TLS(고정 경로의 SNI와 인증서)에 보낸 적이 없다. 리디렉션을 따르지 않는 것은 시험하지 않았다. 송신 태스크 시험은 실제 시간을 기다린다(경계 있는 대기, 종료 시험 약 5초, 요청 한도 시험 약 10초). 연결 한도(3초)는 loopback에서 막힌 연결을 만들 수 없어 시험하지 않았다. 종료 한도에서 태스크를 abort하는 것은 시험하지 않았다(시험의 런타임이 바로 끝나 차이가 보이지 않는다). 토큰 파일의 저장소 검사는 조상 디렉터리의 .git만 본다(GIT_DIR·core.worktree로 둔 저장소, 읽을 수 없는 디렉터리는 보지 못함). root로 돌면 권한 시험은 일찍 끝난다. 시험 디렉터리(OMS_TEST_DIR 또는 임시 디렉터리)의 조상에 .git이 있으면 토큰 파일 시험은 저장소 규칙 하나만 보고 끝난다(M260은 거기서 살아남는다; CI의 /tmp와 실행기의 /mnt/nvme/gitproject/.cargo-target 아래는 해당하지 않음). chat_id가 틀리면(텔레그램 400 chat not found) 모든 부분과 버림 알림이 Refused로 세어질 뿐 사람에게 닿지 않는다(C-WATCH 10부 알려진 한계). U19b1c: 송신 태스크가 먼저 끝났을 때의 error 줄(그 길을 만들 수 없음), 흔들리는 반향의 public_ip 시작·해소 쌍의 수(반복 규칙이 묶지 않음, C-WATCH 10부), 실행 시험의 실제 시간 대기(약 3~6초)

| 시험 | 결과 | 비고 |
|---|---|---|
| `watcher/a_confirmation_emits_one_confirmed_action_per_identity` | 미실행 |  |
| `watcher/mismatch_notices_are_at_most_one_per_check_per_60_s_and_collect_the_rest` | 미실행 |  |
| `watcher/an_auth_answer_sends_one_watch_p1_at_once` | 미실행 |  |
| `watcher/public_ip_ok_0_sends_one_watch_p1_and_a_clear` | 미실행 |  |
| `watcher/a_confirmed_mismatch_sends_one_watch_p1` | 미실행 |  |
| `watcher/auth_stop_keeps_the_first_text` | 미실행 |  |
| `watcher/a_key_rejected_at_start_is_told_on_telegram` | 미실행 |  |
| `watcher/a_start_refusal_after_the_sender_is_up_sends_one_watch_p1` | 미실행 |  |
| `watcher/a_confirmed_mismatch_is_told_on_telegram_once` | 미실행 |  |
| `watcher/a_mismatch_collected_inside_the_pace_is_sent_at_shutdown` | 미실행 |  |
| `watcher/mismatch_notices_drain_tells_what_the_pace_holds` | 미실행 |  |
| `watcher/the_start_refusal_reason_is_cut_to_200_characters` | 미실행 |  |
| `watcher/a_recheck_that_fails_a_start_rule_stops_every_bybit_request` | 미실행 |  |
| `watcher/a_network_error_moves_to_the_pinned_route` | 미실행 |  |
| `watcher/the_token_is_never_in_a_log_line` | 미실행 |  |
| `watcher/the_sender_task_flushes_on_the_manual_clock` | 미실행 |  |
| `watcher/a_retry_keeps_the_part_for_the_next_flush` | 미실행 |  |
| `watcher/shutdown_flushes_within_5_s_and_counts_the_rest` | 미실행 |  |
| `watcher/set_queues_what_the_repeat_rule_gives` | 미실행 |  |
| `watcher/a_request_without_an_answer_fails_after_10_s` | 미실행 |  |
| `watcher/messages_split_at_3500_utf16_units_at_line_ends` | 미실행 |  |
| `watcher/the_outbox_keeps_50_parts_and_reports_the_drops_once` | 미실행 |  |
| `watcher/answers_400_and_413_drop_the_part_and_other_answers_keep_it` | 미실행 |  |
| `watcher/the_repeat_rule_sends_start_every_5_min_and_clear` | 미실행 |  |
| `watcher/the_token_is_never_in_debug` | 미실행 |  |
| `watcher/a_message_has_the_watch_p1_header_with_the_name_and_kst` | 미실행 |  |
| `watcher/telegram_is_validated_and_required_on_mainnet` | 미실행 |  |
| `watcher/the_token_file_must_be_owner_only_outside_any_repository` | 미실행 |  |
| `watcher/a_start_refusal_ends_the_run_with_exit_code_2` | 미실행 |  |

## V-WATCH-12 — 워처의 서명된 POST는 Client::cancel_all 하나뿐이다: 경로 /v5/order/cancel-all과 본문 {"category":"linear","settleCoin":"USDT"}는 상수이고, 서명은 timestamp + api_key + recv_window + 보내는 본문의 바이트 위에 하며(거래소의 POST 예와 독립 HMAC 값), 머리글 다섯과 Content-Type application/json을 달고, 요청 제한·403 멈춤·키 래치를 지나며 보내기 전에 watcher_requests_total에 센다. 답은 success가 없거나 "1"이면 result.list의 길이, "0"이면 refused, 목록이 아니거나 다른 값이면 unreadable이다. crates/watcher/src의 글자에는 .post(가 rest.rs·metrics.rs·alert.rs에 하나씩만 있고 Method::·.request(·.put(·.patch(·.delete(는 없다. [kill]은 round_ms·watch_ms·watch_every_ms·notice_ms를 OMS의 kill_* 기본값과 범위로 받고, 그것이 있으면 시작 2단계와 재확인은 readOnly 0(정수), ContractTrade의 Order, 어느 목록에도 Withdraw 없음을 요구한다(시작은 종료 2, 재확인은 재시작까지 모든 Bybit 요청과 일괄 취소를 멈춤). 읽히는 규칙 하나가 맞지 않으면 다른 필드나 목록을 읽을 수 없어도 거부하고, 읽히는 규칙이 모두 맞는데 읽을 수 없는 것이 있으면 시작은 5초 뒤, 재확인은 ip_check_ms 뒤 다시 묻는다(재확인 실패, 멈춤 아님). [kill]이 없으면 그것을 읽지 않는다. Dec::percent_floor는 0.01 %로 내리고 넘치면 None이다. 킬 알림의 포지션 묶음은 기호마다 방향·수량·미실현손익·마크·마크에서 강제청산가까지의 거리(내림)를 적고, 청산가가 비었거나 0이거나 마크가 0 이하면 모름, 크기 0인 행은 빼고 크기를 읽을 수 없는 행은 수량 모름으로 보이며(포지션 없음이라 하지 않음) 20행 뒤는 수로 센다. 포지션 읽기는 5쪽까지, 읽기 시작 + request_timeout_ms + 11,000 ms의 기한 때나 뒤에 답한 쪽에서 끝내고 다음 쪽을 보내지 않으며, 래치나 멈춤이면 묻지 않는다. 알림 문구는 워처 킬로 시작하고, 마지막 알림의 줄은 StopOutcome 다섯을 따르며 래치와 경로를 확인한 마커의 멈춤만 clear-hold를 말하고, 키 래치와 403 멈춤의 진행 문구는 정한 줄이며, 중단 알림은 포지션 조회 안 함(워처 종료 중)으로 끝난다. sans-IO KillLoop(U19b2b)는 보낸 모든 일괄 취소 뒤(실패해도) 세고, 보내지 않은 것 뒤에는 세지 않고 다음 라운드를 기다리며, 세기 오류는 0이 아니고, n > 0이면 t_r + round_ms까지 기다리며, 첫 0의 z부터 watch_every_ms마다 세어 z + watch_ms 때나 뒤에 낸 세기의 0에서만 끝난다(지켜봄의 늦은 주문은 곧바로 일괄 취소, C5에 들어올 때마다 새 z, watch_ms가 watch_every_ms의 배수가 아니어도 마지막 세기는 z + watch_ms, 지켜봄의 늦은 주문이 연 라운드는 그 세기의 시각에 시작). 두 번째 start는 아무것도 하지 않는다. 요청 둘이 함께 열리지 않고, 어느 [t, t + 1,000) ms에도 일괄 취소를 둘 넘게 내지 않으며(설정에 맞는 타이밍에서는 예산이 잡지 않고 두 일괄 취소가 1,000 ms보다 가깝지 않다), 요청의 기한은 낸 시각 + request_timeout_ms + 11,000 ms이고 세기는 기한 전에 답한 쪽마다 그것을 옮기며, 기한 때나 뒤의 답·쪽은 기한 뒤(세기 오류, 일괄 취소 Failed)이고 그 뒤의 같은 req_id 답과 쪽은 버리며(버린 세기의 쪽은 지금 열린 세기의 기한을 옮기지 않는다), next_wake는 기한·라운드·지켜봄·예산의 시각과 다음 진행 표시 중 가장 이른 것이다. 알림은 시작, 시작에 묶인 notice_ms 표시마다 진행 하나(건수가 같아도; 바뀐 건수의 알림은 없다), 끝에서 곧바로 마지막, 중단 하나와 끝이고, 10분 동안 조회가 실패해도 600초 창마다 12개 이하(시작 1 + 진행 10 + 마지막 1)다. U19b2c의 세기는 /v5/order/realtime linear USDT의 모든 쪽을 W1의 쪽 규칙으로 20쪽까지 읽고(20쪽째에도 커서가 있으면 세기 오류), orderStatus가 New·PartiallyFilled·Untriggered인 행을 orderLinkId와 상관없이 세며(남의 주문도), orderStatus가 없거나 읽을 수 없는 쪽은 세기 오류다. 세기 태스크는 루프가 그 세기를 낸 시각(첫 실행이 아님), 그 뒤 쪽마다 답한 시각 + request_timeout_ms + 11,000 ms의 기한을 스스로 지켜 기한 때나 뒤에 답한 쪽 뒤에는 묻지 않는다. 일괄 취소가 나갔는지는 보내기 전 상태로 가린다(래치 NotSent(Latched), 403 멈춤 동안 NotSent(Paused)이고 멈춤이 끝나면 다시 보냄, 연결 못 함 NotSent(Local), 그 밖은 Failed). kill::Inflight는 루프의 요청마다 태스크 하나를 돌리고 루프의 기한(open_request) 때나 뒤에, 또는 루프가 버린 요청이면 곧바로 끊고 그 태스크가 보낸 것을 루프에 주지 않으며, 새 요청 전에 산 태스크를 끊고, Inflight가 사라질 때 모두 끊는다. http-fake와 수동 시계의 시험 장치에서 일괄 취소 뒤 한 건이 남거나 5xx가 와도 루프는 0을 확인할 때까지 되풀이하고 어느 [t, t + 1,000) ms에도 일괄 취소가 거래소에 둘 넘게 닿지 않는다

- 상태: **미실행**
- 왜 중요한가: 일반 POST가 있으면 Order 권한을 가진 워처 키가 주문을 낼 길이 생긴다. 서명한 바이트와 보낸 본문이 다르면 거래소가 서명을 거부해 OMS가 사라진 때의 일괄 취소가 나가지 않는다. 읽기 전용이거나 Order가 없는 키는 킬 때에야 실패하고, 출금 권한이 있는 키는 워처가 털리면 돈이 나간다. 강제청산가까지의 거리를 올림하거나 빈 청산가를 0으로 읽으면 사람이 위험을 작게 본다. 끝나지 않는 포지션 읽기는 킬 알림을 막는다. 실패한 일괄 취소 뒤에 세지 않으면, 미체결이 없을 때 일괄 취소를 거부하는 거래소에서 루프가 끝나지 않는다. 실패한 세기를 0으로 읽거나 첫 0이나 늦은 답에서 끝내면 늦게 접수된 주문이 남는다. 예산이 없으면 바뀐 길이 초당 여러 번 일괄 취소를 보내고, 기한이 없거나 쪽이 옮기지 않으면 걸린 요청이 루프를 멈추거나 긴 세기가 실패한다. 바뀔 때마다 알리면 10분에 12개를 넘는다. 남의 주문을 세지 않으면 수동 주문을 남긴 채 끝나고, 늦은 쪽 뒤에 묻는 세기는 끊기 전에 요청을 하나 더 보내며, 기한에 끊지 않으면 걸린 요청 옆에서 다음 요청이 돌아 요청 제한을 쓰고, 보낸 뒤의 오류로 보냄을 가리면 보내지 않은 일괄 취소를 보낸 것으로 센다
- 수준: 단위(순수 함수)와 통합(실제 Client, loopback 가짜, 수동 시계); 루프는 sans-IO 상태 기계(손으로 준 시각, 시드 무작위 답 순서); 세기·Inflight·시험 장치는 통합(실제 Client와 tokio 태스크, loopback 가짜, 수동 시계, 실제 시간으로 기다린 뒤에만 시계를 옮김)
- 전제와 범위: watcher::sign의 POST 서명, rest::Client::cancel_all(http-fake), crates/watcher/src의 소스 글자, config::Kill, start::Start와 IpCheck의 [kill] 키 규칙(손으로와 http-fake), decimal::Dec::percent_floor, kill::notice_text·position_block(순수), kill::read_positions(http-fake, on_request로 수동 시계를 옮김); kill::KillLoop(sans-IO, 손으로 몬 순서와 시드 xorshift 답 순서 10,000개 × 두 타이밍): 시험 35개; kill::count_open, cancel_answer, Inflight, KillLoop::open_request·arrived(http-fake, tests/common/kill_rig.rs): 시험 10개 더
- 실제 구성요소: 워처의 sign(post_prehash, post_signature), rest(CANCEL_ALL, CANCEL_ALL_BODY, CancelAll, Client::cancel_all), config::Kill, start(read_key, read_cancel_rules), decimal::Dec::percent_floor, kill(NotSent, StopOutcome, AbortWhy, LastError, Notice, notice_text, Positions, position_block, read_positions, CancelAnswer, Phase, Report, Action, KillLoop, ORDER_BUDGET, ORDER_WINDOW_MS, PAST_DEADLINE, NOT_COUNTED), count_open, ORDERS, cancel_answer, Arrived, Inflight, KillLoop::open_request, KillLoop::arrived); tokio 태스크와 AbortHandle
- 모의·시뮬레이션: Bybit(http-fake, 평문 http), 시계(수동 시계; 느린 쪽은 http-fake의 on_request가 시계를 옮김); 루프의 거래소 답과 시계(시험이 정한 답·늦은 답·걸린 요청과 그 시각, 100 ms·7,000 ms·1 ms·1,000 ms의 poll); 시험 장치(U19b2c): 경로마다 정한 http-fake 답과 요청 때 옮기는 수동 시계(느린 거래소), Answer::Hang, 닫힌 loopback 포트
- 주입한 장애: 거래소의 POST 예(공백 있는 본문), 공백이 다른 본문, 끝에 / 있는 기준 주소, 시계가 옮겨진 두 번째 일괄 취소; success "1"·없음·빈 목록·"0"·숫자 1·"2", list가 객체, result 없음·null, 503, 10010 래치 뒤 일괄 취소와 GET, 403과 599,999 ms·600,000 ms; 읽기 전용, Order 없음, ContractTrade 없음, Spot에만 있는 Order, Wallet과 ContractTrade의 Withdraw, null 목록이나 읽을 수 없는 readOnly·permissions와 함께 온 읽기 전용·Order 없음·Withdraw, null 목록, ContractTrade와 다른 목록 둘 다 읽을 수 없음, readOnly 없음·false·"0", permissions가 목록, ContractTrade가 문자열, 숫자가 든 목록, [kill] 없이 같은 키들, 재확인의 같은 키들, 재확인의 null 목록; 범위의 양끝과 한 걸음 밖, watch_every_ms > watch_ms, 모르는 키, 문자열 값; 1/3, 2/3, 0.99999999/100, 아주 작은 값, 음수, 0과 음수 분모, 두 곱셈의 넘침; 롱·숏·빈 청산가·청산가 0·마크 0·크기 0·빈 크기·JSON 수 크기·소수 9자리 크기·크기 없음·읽을 수 없는 손익·소문자 기호·빈 방향·넘치는 거리, 행 없음, 50행; 기한 뒤·정각·1 ms 전의 쪽, 6쪽, 목록 아닌 list, 커서 없는 가득 찬 쪽, 같은 커서(요청 둘에서 끝남), 503, 래치, 멈춤과 그 끝; 400자 이유와 오류, 64자 이름, 300자 시작 거부 이유. 변이 M265~M268과 시험 밖 변이 31개(U19b2a; 검토 1 수정의 7개 포함; 첫 실행의 놓친 하나는 같은 상한이 두 곳에 있어 생긴 동등 변이였고 코드를 한 곳으로 줄인 뒤 잡힘); 루프(U19b2b): 503과 거부된 일괄 취소, 모든 일괄 취소가 거부되고 세기는 0, Latched·Paused·Local로 보내지 않음, 세기 오류(C3와 C5), 줄어드는 건수, 그대로인 건수, 지켜봄의 늦은 주문, 지켜봄 끝 직전에 낸 세기의 늦은 0, 기한까지 걸린 일괄 취소와 세기, 기한 정각과 1 ms 뒤의 답·쪽·세기 결과, 기한 뒤의 늦은 답, 다른 req_id와 다른 종류의 답, 1,300 ms짜리 11쪽, 늦은 poll(+65초, 7,000 ms)과 표시 둘을 한 번에 지나는 poll, 600초의 실패, 시작 전·라운드·지켜봄의 중단, 루프 중 바뀐 StopOutcome, 범위 밖 100 ms 타이밍의 시드 답 순서; 검토 1: 지켜봄의 늦은 주문 뒤 n > 0, 버린 세기의 쪽, 요청이 열린 때·지켜봄·끝·중단 뒤의 두 번째 start, 진행 표시를 넘는 기한, 예산이 잡은 일괄 취소, 30,000/7,000과 범위 밖 10,000/15,000의 지켜봄, 알던 건수 뒤 보내지 않은 일괄 취소; 시험 밖 변이 41개(mut.py, 검토 1의 9개 포함; 적용된 40개 모두 잡힘); 세기·Inflight(U19b2c): 남의 링크·빈 링크·링크 없는 행, Filled·Cancelled, orderStatus 없는 행, 목록 아닌 list, 커서 없는 가득 찬 쪽, 같은 커서, 503, 50행 20쪽의 커서(라운드 둘), 일괄 취소 뒤 남은 한 건, 503 일괄 취소와 503 세기, 900 ms씩 늦은 거래소, 14,000 ms·13,000 ms·12,999 ms 늦은 둘째 쪽, 1,300 ms 간격 11쪽, 걸린 둘째 쪽과 걸린 일괄 취소, 기한 1 ms 전과 기한의 끊기, 큐에 든 끊긴 태스크의 답과 쪽, 중단한 루프, 산 태스크 옆의 새 요청, 래치된 키, 받은 403과 그 멈춤과 멈춤 끝 1 ms 전·정각, 닫힌 포트, 루프가 낸 뒤 5 ms 늦게 처음 도는 세기 태스크와 루프의 기한 정각·1 ms 전에 답한 첫 쪽, 요청 제한에서 기다리다 버려진 Inflight; 변이 M279~M281과 시험 밖 변이 23개(U19b2c mut.py; 첫 실행 16개, 같은 검사인 20쪽 break는 코드에서 빼고 범위를 변이, 시험을 더한 뒤 19개, 검토 1의 4개를 더해 23개 모두 잡힘)
- 판정 기준과 근거: 가짜가 받은 요청의 방법·경로·본문 바이트·머리글과 서명 값, 센 요청 수, cancel_all의 반환값과 부류, 오류 지표, 래치와 멈춤 뒤 가짜가 받은 요청 수; 소스의 .post( 수와 금지 문자열 0; 설정의 기본값과 거부 줄; 시작 Step(거부 줄, Wait, Next)과 재확인의 StopRequests 뒤 요청 수, 재확인 Wait와 watcher_public_ip_ok; percent_floor의 값과 None; 포지션 묶음과 알림 문구의 정확한 글자, clear-hold의 유무, 3,500 UTF-16 이하와 한 부분; read_positions의 반환값, 가짜가 받은 대상 목록, 끝난 시계; 루프가 낸 Action과 그 순서와 시각, phase·round·remaining·cancel_all_calls·next_wake·ignored·budget_holds·outcome, Report, 일괄 취소 셋이 든 1,000 ms 창의 없음, 600초 창의 알림 수와 종류별 수; 가짜가 받은 요청의 경로·순서와 그때의 수동 시계, 1,000 ms 창마다의 일괄 취소 수, 세기 결과와 쪽 수, Arrived의 순서와 종류, Inflight의 open·aborted·stray·queued, 루프의 remaining·cancel_all_calls·ignored·Report·알림 종류와 시각
- 재현: `cargo test -p watcher --lib --test sign --test client --test config --test start --test kill`
- 뒷받침하는 것: C-WATCH 11부, ADR-032
- 확인하지 않은 범위: 실제 Bybit에 POST를 보낸 적이 없다(키가 없다): Order 권한이 일괄 취소를 허락하는지, 미체결이 없을 때의 답(Q11.7), Content-Type과 바이트가 같은 본문을 거래소가 요구하는지는 확인하지 않았다. 실행은 아직 cancel_all, read_positions, notice_text, position_block을 부르지 않고 KillLoop를 돌리지 않는다(U19b3c): 루프는 시험 장치에서만 실제 Client, 세기 태스크, Inflight와 함께 http-fake에 대해 돌았다(U19b2c). 걸린 연결이 거래소 쪽에서 닫혔는지는 보지 않았다(http-fake가 적지 않음; 끊긴 태스크가 더 보내지 않는 것만 봄). 걸린 포지션 요청의 abort는 실행(U19b3c)의 몫이라 여기서 시험하지 않았다. 터무니없이 긴 값의 20행은 3,500 단위를 넘을 수 있다(송신기가 나눔). 일괄 취소가 요청 제한에서 기다리는 동안 다른 태스크가 래치·403 멈춤을 걸면 보내지 않은 그 요청을 Failed로 센다(C-WATCH 11부 알려진 한계, 검토 1 N1). 모르는 orderStatus는 열리지 않은 주문으로 읽힌다(스펙의 규칙; 소유자 결정 몫, 검토 1 N2)

| 시험 | 결과 | 비고 |
|---|---|---|
| `watcher/the_post_string_to_sign_matches_the_published_example_and_independent_values` | 미실행 |  |
| `watcher/the_only_post_is_cancel_all_with_the_fixed_body` | 미실행 |  |
| `watcher/cancel_all_answers_are_classified` | 미실행 |  |
| `watcher/no_other_post_exists` | 미실행 |  |
| `watcher/kill_loop_fields_are_validated` | 미실행 |  |
| `watcher/a_key_that_must_cancel_needs_read_write_order_and_no_withdraw` | 미실행 |  |
| `watcher/percent_floor_rounds_toward_zero_and_refuses_overflow` | 미실행 |  |
| `watcher/the_position_block_lists_size_upnl_mark_and_liquidation_distance` | 미실행 |  |
| `watcher/a_failed_position_read_says_unknown` | 미실행 |  |
| `watcher/a_notice_fits_one_part_with_50_positions` | 미실행 |  |
| `watcher/the_final_notice_names_the_stop_outcome` | 미실행 |  |
| `watcher/a_latched_key_says_the_watcher_must_restart` | 미실행 |  |
| `watcher/the_abort_notice_says_positions_were_not_read` | 미실행 |  |
| `watcher/the_position_reader_pages_and_keeps_its_deadline` | 미실행 |  |
| `watcher/the_loop_cancels_counts_and_ends_after_the_watch` | 미실행 |  |
| `watcher/a_count_above_zero_waits_for_the_round` | 미실행 |  |
| `watcher/a_failed_cancel_all_is_still_followed_by_a_count` | 미실행 |  |
| `watcher/a_cancel_all_that_succeeds_clears_the_last_error` | 미실행 |  |
| `watcher/a_cancel_all_not_sent_waits_for_the_next_round` | 미실행 |  |
| `watcher/a_failed_count_is_never_zero` | 미실행 |  |
| `watcher/a_late_order_in_the_watch_goes_back_to_cancel_at_once` | 미실행 |  |
| `watcher/the_watch_ends_only_on_the_count_due_at_its_end` | 미실행 |  |
| `watcher/two_requests_of_the_loop_are_never_in_flight` | 미실행 |  |
| `watcher/a_request_past_its_deadline_fails_and_a_late_answer_is_ignored` | 미실행 |  |
| `watcher/an_answer_handled_at_its_deadline_is_past_it` | 미실행 |  |
| `watcher/a_slow_paged_count_is_not_failed_while_pages_answer` | 미실행 |  |
| `watcher/a_progress_notice_comes_every_60_s_also_with_an_unchanged_count` | 미실행 |  |
| `watcher/the_notice_rule_sends_no_change_notice` | 미실행 |  |
| `watcher/ten_minutes_of_failing_queries_make_at_most_12_notices` | 미실행 |  |
| `watcher/abort_sends_one_notice_and_ends` | 미실행 |  |
| `watcher/the_outcome_set_during_the_loop_is_in_the_final_notice` | 미실행 |  |
| `watcher/the_order_budget_holds_under_any_answer_sequence` | 미실행 |  |
| `watcher/a_watch_that_is_no_multiple_of_its_step_ends_at_watch_ms` | 미실행 |  |
| `watcher/a_second_start_does_nothing` | 미실행 |  |
| `watcher/the_next_wake_is_the_earliest_due_instant_or_progress_mark` | 미실행 |  |
| `watcher/foreign_orders_count` | 미실행 |  |
| `watcher/an_aborted_task_sends_no_answer_to_the_loop` | 미실행 |  |
| `watcher/one_order_left_after_the_first_cancel_all_repeats_until_zero_is_confirmed` | 미실행 |  |
| `watcher/a_5xx_cancel_all_or_count_repeats_until_zero_is_confirmed` | 미실행 |  |
| `watcher/a_late_count_page_ends_the_count_task` | 미실행 |  |
| `watcher/a_count_past_its_deadline_sends_no_further_page` | 미실행 |  |
| `watcher/more_than_1000_open_orders_is_a_count_error` | 미실행 |  |
| `watcher/a_cancel_all_not_sent_is_told_from_the_state_before_the_call` | 미실행 |  |
| `watcher/a_count_task_keeps_the_loops_deadline_from_its_emission` | 미실행 |  |
| `watcher/a_dropped_inflight_aborts_its_tasks` | 미실행 |  |

## V-WATCH-13 — [w5]은 probe_ms 1,000(500~5,000)과 silence_ms 10,000(3,000~60,000)을 받고 [kill]이 있으면 silence_ms ≥ 3 × probe_ms + 3 × request_timeout_ms + 1,000이어야 한다(잃은 탐침 하나도, 틈 한계보다 짧은 워처의 멈춤 하나도 건강한 OMS를 킬하지 않게). [kill]은 marker(OMS의 규칙: 절대 경로, .. 없음, 가리키는 곳이 data_dir 밖, 디렉터리가 있음)와 oms_unit([A-Za-z0-9:_.@-] 1~255자, .service로 끝남, -로 시작하지 않음)이 필수이고 systemd system|user와 stop_wait_ms 10,000(1,000~120,000)을 받으며, mainnet은 [kill] 없이 시작하지 않고 testnet은 warn 한 줄을 남긴다. sans-IO W5는 [kill]이 없으면 Off, arm 전에는 탐침도 쏨도 없고, Armed와 Rearming에서 앞 탐침을 낸 때부터 probe_ms마다 탐침 하나를 내되 둘을 함께 열지 않으며, 낸 시각 + request_timeout_ms + 1,000 ms 때나 뒤에 답 없는 탐침은 실패이고, 탐침의 답은 온 때 받으며(기한 정각이라도 poll이 적용하기 전이면), 열리지 않은 탐침의 답은 버리고, 2xx JSON에 16자리 16진수 core.instance_id인 답만 정상이다. Armed에서 now − last_good_ms ≥ silence_ms이고 last_good_ms 때나 뒤에 낸 탐침이 실패했을 때만 Fire를 한 번 내고 Killing에서는 탐침이 없다. 두 시각이 2 × probe_ms + request_timeout_ms보다 멀면 W5가 가진 시각을 모두 그 틈만큼 옮기고 warn 한 줄을 남긴다(보지 못한 시간은 침묵이 아니다). kill_ended 뒤에는 정상 응답 하나가 Armed로 돌리고, 정지를 빚졌으면 그 응답이 OwedHalt 하나를 내며(진행 중에는 둘째가 없음) latched나 already latched로 시작하는 답이면 늦은 정지 P1과 Armed, 그 밖에는 빚이 남고 첫 실패에만 늦은 정지 실패 P1, 실패 뒤의 래치는 해소 줄을 단다. clock::Steady는 안쪽 시계가 뒤로 가도 앞서 준 값보다 작은 값을 주지 않고 그 뒤의 진행은 그대로 더하며 앞으로 뛴 걸음은 지나가게 하고 모든 읽는 쪽에 한 시간선을 준다. oms::admin_kill은 연결 하나에 hello 다음 watcher_kill {detail}을 보내 답마다 timeout_ms, 전체 2 × timeout_ms 안에 result나 이유를 준다. stop::Stopper는 정확히 <systemctl> [--user] --no-ask-password stop -- <oms_unit>을 stdin·stdout null, stderr 비차단 파이프로 띄우고, 틱마다 try_wait만 하며 끝난 뒤에만 stderr를 4 KiB까지 기다리지 않고 읽어, 0이면 Stopped, 다른 끝은 exit <code>: <첫 줄>이나 signal <n>, 주입된 시계의 기한 때나 뒤에 아직 돌면 죽이고 Unconfirmed, 그 뒤의 틱이 거둔다. crates/watcher/src의 글자에 Command::new는 stop.rs에 하나뿐이다. 킬 순서 kill::Kill(U19b3b, sans-IO)은 K1 WriteMarker(기한 2,000 ms, 실패나 기한에도 계속, 늦은 결과는 버림) → K2 Halt(기한 2 × request_timeout_ms + 1,000 ms; latched·already latched·재무장 답만 정지; 열린 시도의 답은 기한 정각이라도 poll 전이면 받고 닫힌 시도의 답은 버림) → K2가 정지하지 못했을 때만 K3 StopUnit(now + stop_wait_ms; 지나면 Unconfirmed, 늦은 결과는 버림) → KillLoop::start의 순서로 내고, 첫 CancelAll과 시작 알림은 그 뒤에만 나온다. 머리는 W5의 침묵, 원래 시계의 마지막 정상 응답 KST(없으면 이 프로세스에서 없음), 킬 마커(씀·이미 있음, 경로를 확인하지 않았으면 그 말, 쓰지 못함(<kind>), 시간 초과), OMS 정지(watcher_kill → <답>, 또는 watcher_kill 실패(<이유>) → systemctl stop <unit>: 정지됨·실패·정지 확인 못 함)를 말하고 K2가 실패하고 멈춤이 확인되지 않으면 X8 줄을 달며, 자유 문구는 글자 경계에서 200 UTF-16 단위로 잘리고(BMP 밖 글자는 2단위) 제어 문자가 없어 머리는 침묵 10초에서 가장 길 때 1,029 단위이고 침묵의 초 문구 글자 하나마다 1단위 늘어 가장 길 때 1,048 단위다(래치한 답도 같이 자름). 루프를 시작할 때 halt_owed = !halted && !(stopped && marker_in_place)이고 marker_in_place는 Written·AlreadyPresent와 marker_checked다; StopOutcome은 Latched·StoppedMarked·StoppedUncheckedMarker·StoppedUnmarked·Unconfirmed이고 마지막 알림의 줄과 Ended의 halt_owed가 같다. 빚이 있는 동안 루프에서 앞 시도가 끝난 때(답이 온 때나 기한, 답 없는 K2의 기한 포함)부터 5,000 ms마다 Halt를 다시 내되 둘을 함께 열지 않고(워처 키가 래치되어도), 루프가 끝날 때 열린 시도는 닫으며(halt_open 없음, 늦은 답은 버림, Ended의 halt_owed는 참), 래치한 답은 LateHalt 알림 하나, 빚 없음, 마지막 줄 Latched와 재시도 끝을 준다. Kill::abort는 루프 전이면 그때까지의 머리(끝나지 않은 단계는 진행 중)와 일괄 취소 전 줄, 포지션 조회 안 함 줄의 알림 하나와 Ended(aborted, halt_owed)를, 루프 안이면 루프의 중단을 준다. clock::Steady 위의 Kill은 원래 시계가 킬 전과 루프 중에 60초 뒤로 가도 일괄 취소·재시도·진행 알림을 진행 기준으로 제때 내고, 마커 문구와 머리의 시각은 원래 시계다. kill::write_marker는 무엇이든 있으면 AlreadyPresent로 건드리지 않고, 없으면 자기 임시 이름(옛 것은 먼저 지움)에 0600으로 써 sync_all, hard_link, 임시 이름 지움, 디렉터리 sync_all 뒤 Written, 오류는 Failed(<io 종류>); 문구는 512바이트, detail은 1,024바이트 이하로 글자 경계에서 자르고 OMS의 read_kill_marker는 그 파일에서 marker: <문구>를 읽는다. 실행(U19b3c)은 시작 확인이 끝난 때 W5를 무장하고 그 뒤 모든 단계에서 틱마다 돌린다. [kill]이 있으면 node.kill_marker가 워처의 마커와 같은 파일(가리키는 곳으로 비교)을 가리키지 않는 상태 답(configured false 포함)에 시작을 거부한다(종료 코드 2, 시작 거부 P1, 같은 파일을 가리키는 링크는 됨). W5가 쏘면 마커를 쓰고, watcher_kill을 보내고(답 없는 시도는 K2 기한에 태스크를 끊어 연결을 닫음), 그것이 래치하지 못했을 때만 systemctl stop을 돌린 뒤에야 첫 일괄 취소를 보내며, 루프를 Inflight로 돌려 남은 주문·5xx 일괄 취소·5xx 세기에도 0건 확인까지 round_ms마다(어느 1초에도 둘 이하, 보낸 일괄 취소마다 세기 하나) 되풀이한다. 빚진 정지는 루프 중 5초마다 다시 보내 돌아온 OMS를 킬 동안 래치하고(watcher_kill 하나, 늦은 정지 알림 하나, 마지막 알림 OMS는 래치됨, 워처 키가 10010으로 래치되어 일괄 취소가 없어도), 킬 동안 엔진은 라운드도 하트비트도 내지 않고 검사 나이는 계속 오르며, 끝나면 엔진이 다시 돌고 정상 응답 하나 뒤 W5가 다시 무장한다. 시계가 루프 중 60초 뒤로 가도 다음 일괄 취소는 round_ms + 한 걸음 안에 나간다. OMS가 내려간 채 시작한 워처도 침묵 뒤 킬하고 시작 알림은 이 프로세스에서 없음과 경로 확인 못 함을 말하며, 실행이 남긴 마커는 0600 일반 파일이고 임시 파일이 남지 않는다. 킬 알림마다 포지션 묶음이 붙는다. 루프 뒤 정지를 빚진 킬 다음 OMS가 다시 답하면 watcher_kill 하나를 보내고(K2의 기한, 답이 없으면 늦은 정지 실패 P1 하나, 래치하면 늦은 정지 P1과 해소, W5가 다시 지킴), 시작 뒤 탐침 답의 마커 경로가 다르면 marker_path P1과 함께 마커를 제자리로 세지 않아 그때의 킬이 정지를 빚으며, 킬 중의 종료는 중단 알림 하나로 끝나고 그 뒤 일괄 취소가 없고 systemctl 클라이언트를 죽이지 않으며, 킬이 끝난 바로 뒤의 종료도 포지션 읽기를 기다리던 마지막 알림을 조회 안 함 줄과 함께 보내고, 킬 중의 시작 거부는 중단 알림 다음 거부 P1과 종료 코드 2이고 정지를 빚졌으면(루프 전 K1~K3 중이면 빚으로 봄) 그 전에 watcher_kill 하나(나가 있는 것이 있으면 그것)를 보내 결과를 거부 P1에 적는다. 600초 동안 조회가 실패해도 [s, s + 600 s]의 메시지는 12개 이하이고, 킬의 모든 메시지가 포지션 묶음이나 조회 실패나 조회 안 함 줄을 가지며, 기한 뒤에 답한 포지션 읽기는 시간 초과이고 다음 쪽을 묻지 않는다

- 상태: **미실행**
- 왜 중요한가: W5가 너무 일찍 쏘거나 정상이 아닌 답을 정상으로 읽으면 건강한 OMS를 킬하거나 죽은 OMS를 놓친다. 시작 확인 전에 무장하면 확인되지 않은 계좌로 일괄 취소가 나간다. 워처 자신이 멈춘 시간을 침묵으로 세면 워처가 굶을 때마다 건강한 OMS를 킬하고, 시계가 뒤로 간 만큼 W5와 킬을 잡으면 사라진 OMS의 주문을 아무도 취소하지 않는다. 킬 뒤 곧바로 다시 무장하면 워처가 멈춘 OMS를 10초마다 다시 킬하고, 빚진 정지를 잊으면 systemd가 다시 띄웠거나 멈추기만 했던 OMS가 래치 없이 거래한다. 멈춤 프로그램의 인자가 다른 단위를 담거나 --를 빼면 워처의 결함이 호스트의 결함이 되고, stderr를 막으며 읽으면 단일 스레드 실행의 틱이 멈춘다. mainnet의 워처가 [kill] 없이 돌면 OMS가 죽었을 때 아무도 취소하지 않는다. 첫 일괄 취소가 마커나 정지보다 먼저 나가면 다시 시작한 OMS가 래치 없이 거래하고, 정지된 OMS를 멈추면 쓸데없이 단위를 멈추며, 정지 실패 뒤 멈추지 않으면 살아 있는 OMS가 래치 없이 주문한다. 마커 쓰기나 정지 응답을 기다리면 걸린 디스크나 소켓이 취소를 막고, 있는 마커를 바꾸면 운영자나 앞 킬의 사유를 잃는다. 경로를 확인하지 않은 마커를 제자리로 치거나 빚진 정지를 루프 뒤에만 보내면 돌아온 OMS가 래치 없이 거래한다. 머리의 문구를 자르지 않으면 64 KiB 답 하나가 알림을 여러 메시지로 쪼갠다
- 수준: 단위(sans-IO W5, 손으로 준 시각과 Steady 위의 수동 시계; 설정의 순수 검증; Steady의 두 스레드)와 통합(oms::admin_kill과 시험의 UnixListener, w5::good_answer와 http-fake의 /api/status, stop::Stopper와 OMS_TEST_DIR 아래의 셸 스크립트, 실제 프로세스와 실제 시간); 단위(sans-IO Kill, 손으로 준 시각과 Steady 위의 수동 시계)와 통합(kill::write_marker와 OMS_TEST_DIR 아래의 실제 파일, spawn_blocking; watcher-conformance에서 OMS 자신의 read_kill_marker); 통합(runtime::run과 loopback http-fake의 Bybit·/api/status·텔레그램·VM, 시험의 admin.sock·monitor.sock, OMS_TEST_DIR 아래의 가짜 systemctl, 시험 디렉터리의 마커, 수동 시계와 실제 시간의 walk_kill)
- 전제와 범위: config::Config::from_toml의 [w5]·[kill] 필드, runtime::run의 [kill] 없음 warn, clock::Steady, w5::W5(sans-IO, 시험 장치 Sim)와 w5::good_answer, oms::admin_kill과 oms::latched, stop::Stopper와 stop::args, crates/watcher/src의 소스 글자, kill::Kill(Setup, Step, Stage, Steps, halt_detail, late_halt_detail, marker_text, utc_ms), kill::write_marker와 spawn_marker, oms_node::node::read_kill_marker(적합성), runtime::run의 W5·킬 연결(마커 경로 규칙, 기다리던 메시지, K1~K3 실행, 루프의 Inflight, 루프 중의 정지 재시도, 킬 알림과 포지션 읽기, 킬 중 엔진 멈춤과 Engine::export_ages, W5·킬 지표), 그리고 U19b3d의 빚진 정지(Owed, owed_halt_done의 P1), 탐침 답의 마커 경로(marker_path), 킬 중의 종료(shutdown_kill, flush_notices)와 시작 거부(refuse, finish_refusal, halt_outcome)
- 실제 구성요소: 워처의 config(W5, Kill의 marker·oms_unit·systemd·stop_wait_ms, Systemd, check_marker_path), clock::Steady, w5(Timing, State, Probe, Fire, OwedHalt, PROBE_EXTRA_MS, good_answer, W5), oms(admin_kill, latched), stop(SYSTEMCTL, STDERR_MAX, args, Outcome, Stopper), oms::Status::get; 실제 자식 프로세스(/bin/sh 스크립트), 파이프와 rustix fcntl; kill(Kill, Setup, Step, Stage, MarkerOutcome, write_marker, spawn_marker, marker_text, halt_detail, late_halt_detail, utc_ms, MARKER_WAIT_MS, HALT_EXTRA_MS, HALT_RETRY_MS, notice_text의 LateHalt·AbortBefore, KillLoop), 실제 파일 시스템(OMS_TEST_DIR), oms_node::node::read_kill_marker; runtime(Runtime, KillRun, Notices, marker_rule, QUEUED_MAX, NOTICES_MAX), engine::Engine::export_ages, alert 송신 태스크, tokio 태스크와 AbortHandle, 실제 자식 프로세스(가짜 systemctl 스크립트), 시험 디렉터리의 실제 마커 파일
- 모의·시뮬레이션: OMS(W5 시험은 탐침마다 정한 답·늦은 답·답 없음, /api/status는 http-fake의 답, admin.sock은 시험의 UnixListener), systemctl(OMS_TEST_DIR 아래의 셸 스크립트: argv를 적음, 0·1·신호 9로 끝남, sleep 30이 stderr를 문 채 끝남, 2 MiB를 써서 끝나지 않음), 시계(손으로 준 시각, 1 ms·100 ms 걸음과 15초 틈, 60초 뒤로 간 ManualClock 위의 Steady, 두 스레드가 읽는 동안 xorshift로 앞뒤로 뛰는 ManualClock); 킬 순서의 바깥(마커 쓰기·watcher_kill·systemctl의 결과를 손으로 줌: Written·AlreadyPresent·Failed, latched·already latched·재무장 답·held·not latched·bad_request·unauthorized·소켓 없음·답 없음, Stopped·Failed·Unconfirmed; 루프의 요청은 답을 곧바로 줌: Done·NotSent(Latched), 열린 주문 0·1); 실행 시험의 바깥(Bybit·/api/status·텔레그램·VM은 http-fake, admin.sock은 latched·답 없음·없음·루프 5초 뒤 나타남, systemctl은 0·1로 끝나는 스크립트, 시계는 수동 시계와 60초 뒤로 간 걸음)
- 주입한 장애: 설정: marker·oms_unit 없음, 상대·..·데이터 디렉터리 안(링크 포함)·없는 디렉터리의 마커, 나쁜 단위 이름 열다섯(빈 것, .service만, 확장자 없음, .target, 공백, *, ?, [ ], 앞의 -, --user.service, 끝 공백, /, 비ASCII, 줄바꿈, 256자)과 ;reboot, 255자 단위, systemd session, stop_wait_ms 999·120,001, 모르는 키, probe_ms 499·5,001, silence_ms 60,001·2,999, 한계 6,999·앞의 한계 4,500·9,999·probe_ms 1,001·probe_ms 5,000과 기본 침묵·21,999·34,000, [kill] 없이 10,000 ms 요청, mainnet [kill] 없음. W5: [kill] 없음, arm 전 60초 침묵, 두 번째 arm, 50 ms·2,500 ms 답, 모르는 id와 두 번째 답, 3,000 ms 기한 1 ms 전과 정각, 실패한 탐침의 늦은 답, poll 전 기한 정각의 답, +9,999·+10,000 ms, +9,000 ms의 정상 응답, 열린 탐침의 기한보다 짧은 침묵, 정상이 아닌 답, 같은 틱에 낸 탐침, 마지막 정상 응답 앞의 실패, 잃은 탐침 하나와 1,499 ms 답(잃은 탐침 몫 4,500), 틈 한계만큼 멈춘 워처와 request_timeout_ms − 1에 답하는 OMS(기본값, probe_ms 5,000의 침묵 22,000·10,000·12,001), 15초 틈과 앞으로 뛴 시계, 틈 뒤 처음 온 답, 틈 앞의 실패, 한계 정각의 간격, 60초 뒤로 간 시계, 킬 동안 60초, 멈춘 OMS 2분, 빚진 정지의 latched·실패·held·재무장 답·400자 답; /api/status 503·404·JSON 아님·15·17자리·16진수 아님·수·없음·연결 실패. admin.sock: 없는 소켓, latched·already latched·재무장 답·bad_request·unauthorized, 따옴표와 한글이 든 detail, 답 없음. 멈춤: system·--user, 0으로 끝남, 없는 프로그램, stderr를 문 sleep 30과 exit 1, 2 MiB의 stderr, 신호 9. 변이 M282~M289와 M305, 시험 밖 변이 36개(U19b3a mut.py, 검토 1 고침 뒤 다시, 모두 잡힘). 킬 순서: 마커 실패·시간 초과(1,999·2,000 ms)·늦은 결과, K2 기한 1 ms 전과 정각·poll 전 정각의 답·늦은 답·닫힌 시도의 답, K3 기한 9,999·10,000 ms와 늦은 결과, 64 KiB 답·4 KiB stderr·1,000자 io 종류(제어 문자), 64자 4바이트 이름·100자·1,000자 이름, 255자 단위, 경로를 확인하지 않은 마커, 정상 응답 없는 워처, 여덟 가지 멈춤 결과, 래치된 워처 키의 루프와 답 없는 재시도, K1·K2·K3·루프의 종료·시작 거부, 킬 전과 루프 중에 60초 뒤로 간 시계, BMP 밖 글자의 자유 문구, 루프 끝에 열린 재시도와 그 늦은 답, 답 없는 K2 뒤 1,000 ms에 실패한 K3, 침묵 59,999·999,999 ms와 u64::MAX의 머리, 제어 문자와 BMP 밖 글자 300개가 붙은 already latched 답; 마커: 옛 임시 파일, 파일·끊긴 링크·디렉터리가 이미 있음(0644 그대로), 없는 디렉터리. 변이 M290~M299, M306~M310과 U19b3b mut.py 27개(25개 잡힘, 시험 하나를 더한 뒤 26개; 살아남은 하나는 sync_all), 검토 1 고침 뒤 31개(30개 잡힘, 살아남은 하나는 sync_all), 검토 2 고침 뒤 34개(33개 잡힘, 살아남은 하나는 sync_all). 실행: 다른 마커 경로·configured false·링크를 거친 같은 파일의 상태 답, 침묵한 OMS, admin.sock 없음, 답하지 않는 watcher_kill, 0·1로 끝나는 systemctl, 일괄 취소 뒤 남은 주문 하나, 503 일괄 취소와 503 세기, 루프 5초 뒤 돌아온 OMS, 10010으로 래치된 워처 키, OMS가 내려간 채 시작한 워처, 루프 중 60초 뒤로 간 시계, 가득 찬 요청 가드 뒤 60초 뒤로 간 시계. 변이 M300·M301과 U19b3c mut.py 21개(첫 실행 16개 잡힘; 시험을 고친 뒤 정지 태스크 끊기도 잡힘, 남은 넷은 보고서). U19b3d: K2·K3가 실패하고 0건으로 끝난 킬 뒤 답 없는 admin.sock과 그다음 래치하는 admin.sock, 시작 뒤 다른 마커 경로와 configured 없음, 루프 중·K3 중의 SIGTERM, 포지션 읽기가 걸린 채 SIGTERM, 끝난 킬의 마지막 알림 읽기가 걸린 채 SIGTERM, K3 중의 다른 마커 경로 답, 킬 중의 다른 마커 경로 답(래치된 킬, 빚진 킬), 끝난 킬 뒤 답을 잡아 둔 admin.sock, 600초 동안 모든 킬 경로 503, 답할 때 시계를 14,000 ms 옮기는 포지션 경로. 변이 M302·M303·M304와 U19b3d mut.py(보고서)
- 판정 기준과 근거: 설정의 기본값과 거부 줄(한 줄, 정한 문구); W5가 낸 탐침의 시각, Fire의 시각과 값, State와 armed·silent_ms·halt_owed·probe_in_flight, OwedHalt의 시각과 수, owed_halt_done의 P1 문구, warn 줄; Steady가 준 값과 스레드마다의 순서; good_answer의 참거짓; admin_kill의 반환값과 가짜 소켓이 받은 두 줄과 걸린 시간; Stopper의 Outcome, 그것이 나온 틱, 스크립트가 적은 argv, poll 한 번의 시간(50 ms 아래), reaped; 소스의 Command::new 위치와 수; 실행의 warn 줄 수; Kill이 낸 Step의 순서와 시각, header()와 KillLoop의 머리 문구, outcome·halt_owed·halt_open·stage·next_wake, Ended의 report와 halt_owed, 알림의 종류와 문구, 메시지의 UTF-16 단위와 부분 수; 마커 파일의 종류·모드·내용, 디렉터리의 이름들, 문구의 바이트 수와 글자 경계, read_kill_marker의 결과; 실행: 종료 코드와 거부 줄, 가짜가 받은 요청의 순서(마커 파일 → watcher_kill 답 → systemctl argv → 첫 일괄 취소), 1,000 ms 창마다의 일괄 취소 수, 텔레그램 메시지의 문구, VM에 보낸 W5·킬·검사 나이 지표, 하트비트 수, 마커 파일의 모드·내용, 연결이 닫힌 실제 시간
- 재현: `cargo test -p watcher --test config --test clock --test w5 --test admin --test stop --test runtime --test kill_sequence && cargo test -p watcher-conformance --test marker`
- 뒷받침하는 것: C-WATCH 12부, ADR-032
- 확인하지 않은 범위: 멈춤은 셸 스크립트로만, 마커 쓰기는 시험 디렉터리에서만 돌았다. 16개를 넘게 기다리는 알림의 버리기는 단위 시험(runtime::tests)만 보고 실행 시험은 보지 못한다(설정 범위에서 큐가 16에 닿지 않음). 기다리던 메시지를 틱 전에 다루기는 실행 시험이 보지 못한다(경쟁의 창을 좁힐 뿐). 킬 중 엔진 멈춤 때문에 래치된 OMS가 30초 뒤 hold:unmonitored를 내는 것은 진짜 OMS로 보지 않았다. 마커 쓰기를 거는 진짜 디스크와 정전 뒤 마커가 남는지(sync_all)는 시험하지 않았다(정전을 흉내 내지 않음). 코어가 걸리고 HTTP 태스크가 답하는 OMS는 W5가 보지 못한다(소유자 질문 11). 진짜 systemctl과 그 권한(U20b가 주는 그 단위 하나의 stop), systemd가 클라이언트를 죽인 뒤에도 멈춤 작업을 끝내는지, 워처가 진짜 OMS 프로세스를 멈춘 적은 없다(U20b와 testnet). 실제 OMS의 admin.sock에 watcher_kill을 보낸 적이 없다(시험의 UnixListener; OMS 쪽 규칙은 U18b1의 시험). 시계가 뒤로 간 걸음과 다음 읽기 사이의 진행은 Steady가 걸음에 묻는다(최대 한 틱). 틈 규칙의 한계 4,000 ms는 판단이고 실제 부하에서 재지 않았다. 종료 때 K1의 결과를 기다리는 것과 main.rs의 shutdown_timeout(1,000 ms)은 시험하지 않았다(K1은 시험 디렉터리에서 곧 끝나고, 걸린 디스크를 흉내 내지 않음). 거부 전의 watcher_kill을 기다리는 동안 킬을 멈추는 것(그동안 킬의 재시도나 K2가 둘째 연결을 열지 않음)과 그 기다림의 기한은 실행 시험이 보지 못한다(그 몇 초 안에 킬이 무언가를 내는 경우를 만들지 않음). OMS를 기다리는 동안의 탐침 답을 마커 경로로 보지 않는 것은 실행 시험이 가리지 못한다(그 시험에서 킬 중 W5는 탐침을 보내지 않음). 600초 시험은 메시지 수만 보고 포지션 묶음의 문구는 보지 않는다(빠른 시계에서 읽기가 시간 초과일 수 있음)

| 시험 | 결과 | 비고 |
|---|---|---|
| `watcher/kill_and_w5_are_validated_and_kill_is_required_on_mainnet` | 미실행 |  |
| `watcher/kill_loop_fields_are_validated` | 미실행 |  |
| `watcher/a_steady_clock_does_not_step_back` | 미실행 |  |
| `watcher/w5_is_not_armed_before_arm_and_off_without_kill` | 미실행 |  |
| `watcher/probes_are_one_per_probe_ms_and_never_two_in_flight` | 미실행 |  |
| `watcher/a_lost_probe_is_failed_at_its_deadline` | 미실행 |  |
| `watcher/ten_seconds_without_a_good_status_answer_fire_once` | 미실행 |  |
| `watcher/w5_fires_only_after_a_failed_probe` | 미실행 |  |
| `watcher/one_lost_probe_and_a_slow_answer_at_the_silence_bound_do_not_fire` | 미실행 |  |
| `watcher/a_stall_of_the_watcher_shorter_than_a_gap_does_not_fire_at_the_silence_bound` | 미실행 |  |
| `watcher/a_gap_in_the_watchers_own_ticks_is_not_oms_silence` | 미실행 |  |
| `watcher/a_clock_step_back_does_not_blind_w5` | 미실행 |  |
| `watcher/after_a_kill_w5_waits_for_a_good_answer_before_it_watches_again` | 미실행 |  |
| `watcher/an_owed_halt_is_sent_at_the_first_good_answer` | 미실행 |  |
| `watcher/only_a_2xx_status_with_a_16_hex_instance_id_is_good` | 미실행 |  |
| `watcher/admin_kill_sends_hello_then_watcher_kill_and_reads_the_reply` | 미실행 |  |
| `watcher/the_stop_command_names_only_the_configured_unit` | 미실행 |  |
| `watcher/the_watcher_starts_one_program_only` | 미실행 |  |
| `watcher/a_child_that_keeps_stderr_open_does_not_block` | 미실행 |  |
| `watcher/a_start_refusal_ends_the_run_with_exit_code_2` | 미실행 |  |
| `watcher/the_marker_and_the_halt_come_before_the_first_cancel_all` | 미실행 |  |
| `watcher/a_marker_write_that_does_not_return_does_not_hold_the_halt` | 미실행 |  |
| `watcher/a_halt_without_an_outcome_goes_to_the_stop_at_its_deadline` | 미실행 |  |
| `watcher/a_marker_written_before_the_path_check_says_so` | 미실행 |  |
| `watcher/already_latched_and_the_rearm_reply_count_as_halted` | 미실행 |  |
| `watcher/a_halt_answered_with_an_error_goes_to_the_stop` | 미실행 |  |
| `watcher/a_stop_past_stop_wait_ms_is_unconfirmed_and_the_loop_starts` | 미실행 |  |
| `watcher/both_failed_says_oms_stop_failed_in_the_start_notice` | 미실행 |  |
| `watcher/a_kill_without_a_confirmed_stop_owes_the_halt` | 미실행 |  |
| `watcher/an_oms_that_answers_during_the_loop_is_latched_at_once` | 미실행 |  |
| `watcher/no_halt_is_retried_when_the_halt_is_not_owed` | 미실행 |  |
| `watcher/the_first_retry_after_a_halt_timeout_counts_from_its_deadline` | 미실행 |  |
| `watcher/a_retry_in_flight_at_the_loops_end_is_given_up` | 미실행 |  |
| `watcher/abort_before_the_loop_tells_the_steps_done` | 미실행 |  |
| `watcher/a_start_notice_with_the_longest_header_fits_one_part` | 미실행 |  |
| `watcher/the_marker_text_and_detail_are_cut_at_a_character_boundary` | 미실행 |  |
| `watcher/a_marker_file_is_what_a_marker_rule_reads` | 미실행 |  |
| `watcher/an_existing_marker_is_left_as_it_is` | 미실행 |  |
| `watcher/a_marker_that_cannot_be_written_does_not_stop_the_sequence` | 미실행 |  |
| `watcher/a_clock_step_back_does_not_hold_the_loop` | 미실행 |  |
| `watcher/the_oms_reads_the_watchers_marker_file` | 미실행 |  |
| `watcher/a_status_with_another_marker_path_refuses_the_start` | 미실행 |  |
| `watcher/oms_silence_writes_the_marker_and_halts_the_oms_before_the_first_cancel_all` | 미실행 |  |
| `watcher/when_watcher_kill_fails_the_oms_unit_is_stopped_before_the_first_cancel_all` | 미실행 |  |
| `watcher/a_watcher_started_with_the_oms_down_kills_after_the_silence` | 미실행 |  |
| `watcher/a_marker_written_by_the_watcher_is_read_by_a_marker_rule` | 미실행 |  |
| `watcher/the_engine_is_paused_during_a_kill_and_resumes_after` | 미실행 |  |
| `watcher/the_watchers_loop_repeats_until_zero_at_no_more_than_2_cancel_alls_per_second` | 미실행 |  |
| `watcher/an_oms_that_comes_back_during_the_loop_is_latched_before_the_loop_ends` | 미실행 |  |
| `watcher/a_clock_step_back_during_a_kill_does_not_hold_the_loop` | 미실행 |  |
| `watcher/a_halt_without_a_reply_is_aborted_at_its_deadline` | 미실행 |  |
| `watcher/a_round_in_flight_at_the_fire_sends_nothing_more_during_the_kill` | 미실행 |  |
| `watcher/a_stop_that_ended_before_the_k3_deadline_tick_is_reported_as_stopped` | 미실행 |  |
| `watcher/a_clock_step_back_does_not_hold_the_guard` | 미실행 |  |
| `watcher/a_full_notice_queue_drops_the_oldest_progress_notice_never_a_start_or_a_final` | 미실행 |  |
| `watcher/during_a_kill_an_answer_keeps_the_mismatch_p1_and_drops_every_request` | 미실행 |  |
| `watcher/an_oms_that_comes_back_after_an_unconfirmed_stop_is_latched` | 미실행 |  |
| `watcher/a_marker_path_change_after_the_boot_is_told` | 미실행 |  |
| `watcher/shutdown_during_a_kill_sends_one_abort_notice` | 미실행 |  |
| `watcher/a_start_refusal_during_a_kill_sends_the_abort_notice_first` | 미실행 |  |
| `watcher/a_marker_path_refusal_sends_the_owed_halt_first` | 미실행 |  |
| `watcher/a_marker_path_refusal_after_the_kill_waits_for_the_owed_halt_in_flight` | 미실행 |  |
| `watcher/a_marker_path_refusal_before_the_kill_loop_sends_the_halt_first` | 미실행 |  |
| `watcher/shutdown_right_after_the_kill_ended_sends_the_final_notice` | 미실행 |  |
| `watcher/ten_minutes_of_failing_queries_send_at_most_12_p1_messages` | 미실행 |  |
| `watcher/every_kill_notice_carries_the_position_block` | 미실행 |  |
| `watcher/a_slow_position_read_sends_the_notice_with_unknown_positions` | 미실행 |  |
| `watcher/the_refusal_names_a_latch_only_for_a_reply_that_latched` | 미실행 |  |

