# OMS·워처 합동 시험

실제 `oms-node`(failpoints 빌드, Bybit 게이트웨이)와 실제 워처(기능 `test-endpoints`의 시험 빌드)를 함께 띄우고, 상태가 있는 가짜 Bybit 하나(`crates/oms-node/tests/bybit_fake/`: 두 키, 장부 하나), 가짜 Telegram, 가짜 VictoriaMetrics, 가짜 `systemctl`에 loopback으로만 붙인다. 시험은 두 프로그램을 실제 인터페이스로만 본다: 저널 파일, `/api/status`(워처가 묻는 그대로 HTTP)와 admin `Status`, 거래 소켓과 `omsctl`, 워처의 stderr와 지표, 킬 마커, 가짜들의 기록. 결정: [ADR-039](../adr/ADR-039-oms-watcher-joint-tests.md), 이 묶음이 찾은 어긋남의 수정은 [ADR-040](../adr/ADR-040-w1-during-a-kill.md). 주장: V-JOINT-1, V-JOINT-2, V-JOINT-3, V-JOINT-4, V-JOINT-5, V-JOINT-6.

## 1. 돌리는 법

```sh
export OMS_TEST_DIR=/mnt/nvme/gitproject/.cargo-target/joint-test   # git 작업 트리 밖, 58바이트 이하, 절대 경로
cargo run --locked -q -p xtask -- joint --runs 5      # 세 빌드, 그다음 묶음 5번
cargo run --locked -q -p xtask -- joint --only j1     # 이름에 j1이 든 시험만(cargo test의 거르개)
```

- `OMS_TEST_DIR`에는 기본값이 없다. 없거나, 상대 경로이거나, git 작업 트리 안이거나(그 경로와 모든 조상에서 `.git`을 찾는다, 경로가 없어도), 58바이트를 넘거나, 디렉터리가 아닌 것이 있으면 `xtask joint`는 그 규칙을 적은 한 줄과 함께 종료 코드 2로 끝난다. 없는 디렉터리는 만들고(마지막 부분 0700) `joint: created <경로>`를 찍는다. 58바이트: 가장 긴 소켓 경로가 기준 + 실행 디렉터리 12바이트 + `/<태그>-<pid>/o/d/monitor.sock` 29바이트이고, 리그는 100바이트 이상의 소켓 경로를 거부한다(SUN_LEN 108, 여유 8은 판단(모델 아님)). 워처는 git 작업 트리 안의 토큰 파일을 거부한다.
- 단계: (1) 위 규칙, `--runs`는 1~99. (2) `cargo build -p watcher --features test-endpoints --bin watcher`, `-p oms-api --bin omsctl`, `-p oms-node --features joint --bin oms-node`의 `compiler-artifact` 실행 파일; 워처와 `omsctl`은 `$OMS_TEST_DIR/r<pid>-bin/`에 복사하고 워처의 `--version`이 ` test-endpoints`로 끝나는지 본다. (3) `cargo test --locked -p oms-node --features joint --test joint -- --test-threads=1`을 N번, 실행마다 `OMS_TEST_DIR=$OMS_TEST_DIR/r<pid>-<k>`와 `JOINT_WATCHER_BIN`, `JOINT_OMSCTL_BIN`으로. 실행은 종료 0, `test result: ok`, 한 건 이상, 무시 0, 고아 프로세스 없음일 때만 통과하고 `joint run k/N: ok in <초> s`를 찍는다. 첫 실패에서 멈추고 종료 코드 1.
- 고아 프로세스 검사: 실행이 끝날 때마다 `/proc/*/cmdline`이 그 실행 디렉터리를 이름하는 프로세스를 찾는다. 실행 파일이 세 시험 바이너리(합동 빌드의 `oms-node`, 복사한 워처와 `omsctl`) 가운데 하나면 SIGCONT 다음 SIGKILL을 보내고 실패(`orphan process <pid> <exe>`), 다른 프로그램(셸, `tail -f`)이면 신호 없이 실패한다. 디렉터리 이름에 `xtask`의 pid가 들어 있어 다른 호출의 실행은 맞지 않는다.
- `xtask verify`의 기본 묶음 `joint`는 `xtask joint --runs 1`이다. `OMS_TEST_DIR` 없이는 실패로 기록된다(건너뜀도 통과도 아님).
- 통과한 시험의 디렉터리는 지운다(`OMS_KEEP`이 있으면 남김). 실패한 시험의 디렉터리와 실패한 호출의 바이너리 복사본은 남는다.
- 시험은 한 번에 하나씩 돈다: 실시간이고, 세계 하나가 프로세스 여럿과 소켓 열 개쯤(판단(모델 아님))을 쥔다.

## 2. 리그

- 시험마다 `$OMS_TEST_DIR/<태그>-<pid>/`: `o/`(OMS 설정 `oms.toml`, 자격 파일 0600, `node.log`, `oms.pid`, 데이터 디렉터리 `o/d/`), `w/`(워처 설정, 자격 파일 0600, `watcher.log`), `telegram.json`(0600, 두 프로그램이 같은 봇), 두 데이터 디렉터리 밖의 마커 디렉터리 `s/`, 가짜 `bin/systemctl`(0700)과 `systemctl.log`.
- 키 둘: OMS의 `OKEY`(`ContractTrade: [Order, Position]`), 워처의 `WKEY`(`[Order]`, `readOnly` 0, `Withdraw` 없음). 둘 다 하위 계정 4242, `ips: ["127.0.0.1"]`. 지어낸 값이다.
- `Oms::start`만 `oms-node` 명령을 만든다: `OMS_TEST_BYBIT_WIRING`과 `OMS_TEST_TELEGRAM_API`, 그다음 시험의 추가 변수(`OMS_TEST_`로 시작하면 시험 오류). 운영 소켓의 `Status`와 `/api/status`가 둘 다 이 프로세스의 pid(`node.pid`)로 답하거나(규칙 `up`: 포트를 쥔 다른 서버, 예를 들어 다른 실행의 OMS가 `/api/status`에 답해도 통과하지 않는다. 기다리는 동안 `/api/status`의 읽기 시간 상한은 1초, 판단(모델 아님)), 프로세스가 끝나거나, 로그에 `[http]` 포트의 bind 오류(`Address already in use`)가 나올 때까지 기다린다. 포트가 잡혀 있어도 OMS는 대시보드 없이 계속 돌기 때문에(`dashboard stopped`, `crates/oms-node/src/node.rs:390-393`) 리그가 그 프로세스를 SIGKILL로 끝낸다. OMS의 첫 시작(데이터 디렉터리도 워처 설정도 아직 없음)이면 데이터 디렉터리를 지우고 포트를 바꿔 한 번 더 띄운다. 다시 잡혀 있거나, 저널이나 워처 설정(`status_url`)이 이미 그 포트를 쥔 나중의 시작이면 전제 조건 실패다.
- 워처: `JOINT_WATCHER_BIN --version`이 ` test-endpoints`로 끝날 때만 설정을 쓰고 `WATCHER_TEST_ENDPOINTS`(REST·IP 반향은 가짜 Bybit, Telegram은 가짜, `systemctl`은 리그의 스크립트)로 띄운다.
- 가짜 `systemctl`: 부를 때마다 `systemctl.log`에 인자, 그때 마커가 있었는지(`marker=present|absent`), 밀리초 시각을 한 줄(`date +%s%N`에서: 이 PC의 `date`(uutils coreutils 0.10.0)는 `%3N`의 앞자리 0을 빼고 찍어, 한 초의 첫 100 ms 안의 호출을 늦게 적었다; 리그 시험 `the_fake_systemctl_logs_the_wall_clock_in_milliseconds`). `--no-ask-password stop -- oms-node-joint.service`이면 `o/oms.pid`의 프로세스가 리그의 `o/oms.toml`을 명령 줄에 가질 때만 SIGTERM 다음 SIGCONT를 보내고(systemd.kill(5)) 5,000 ms(판단(모델 아님))까지 그것이 끝나기를 기다린다. 그 밖의 인자는 종료 1.
- 기다림은 25 ms 간격의 폴링이고 자기 상한과 시험의 예산 가운데 먼저 오는 것에서 멈춘다. 실패하면 두 로그와 가짜의 요청 기록의 꼬리를 담아 멈춘다. 리그 자신의 전제 조건(기준 디렉터리, 소켓 경로 길이, 바이너리 변수와 가드, 다시 시도한 뒤에도 쓰이는 포트, 나중의 시작에서 쓰이는 포트)은 `joint rig precondition:`으로 시작한다.
- 자식마다 거두는 스레드가 20 ms마다 `try_wait`하고 종료 상태와 시각을 지킨다. OMS의 것은 종료를 기록하기 전에 `o/oms.pid`를 지운다. 리그를 버리면 살아 있는 자식에게 SIGCONT 다음 SIGKILL을 보내고 기다린다.
- 시간값: `Timers::Deployed`(두 설정이 시간값을 적지 않음: 코드의 기본값)와 `Timers::Short`(스펙의 표). 시험은 워처 설정이 쓰이기 전에 그 시간값 줄 하나를 바꿀 수 있다(`World::set_watcher`, J2b의 `w4_ms = 5000`; 시간값이 쓰지 않는 줄이면 시험 오류이고, 리그 자기 시험 `the_rig_refuses_a_base_in_a_git_work_tree_a_long_socket_path_and_an_oms_test_variable`이 그것을 보인다). `Timers::Short`는 OMS의 `[venue] fills_lag_ms = 2000`도 적는다: J2b의 단언 `min_age_ms + 500 > fills_lag_ms`는 리그가 적은 그 줄을 읽고, 줄이 없으면 시험 오류다(코드 기본값의 사본과 비교하지 않는다). OMS 설정의 시간값 줄 하나는 첫 시작 전에 바꿀 수 있다(`World::set_oms`, J3의 `kill_after_ms = 30000`; 시간값이 쓰지 않는 줄이면 시험 오류). 두 설정 모두 testnet이고 `[core.policy.margin]`, `[liveness]`가 없다.
- 창의 시각: 가짜 Bybit의 요청 기록(키, 경로, 쿼리, 답의 종류, 벽시계 ms)과 저널 기록의 시각(OMS의 벽시계 µs)은 같은 호스트의 시계다. 심기는 장부의 잠금 안에서 기록의 길이를 함께 재므로, 그 뒤의 워처 키 답은 모두 심은 것을 담는다(`World::answers_after`, 고장 응답은 뺀다).

## 3. 무엇을 증명하나

| 시험 | 보이는 것 |
|---|---|
| `j0_the_rig_refuses_a_watcher_built_without_test_endpoints_and_a_test_build_without_wiring_exits_2` | 리그의 가드가 `watcher 0.1.0`을 찍는 스크립트를 설정을 쓰기 전에 거부한다. 실제 시험 빌드를 `WATCHER_TEST_ENDPOINTS` 없이 띄우면 종료 코드 2와 한 줄 `this build is for tests: WATCHER_TEST_ENDPOINTS is required`로 끝나고 가짜 Bybit·Telegram·VictoriaMetrics는 요청을 받지 않는다(실제 호스트로의 요청은 이 시험이 볼 수 없다: 시험 빌드에 그 길이 없다는 것은 V-TESTHOOK-1) |
| `j1_a_whole_life_cycle_at_the_deployed_timers_has_no_mismatch_and_opens_the_gate` | 배포 기본 시간값에서: OMS는 감시 없음으로 시작해 하트비트 전 일반 주문을 거부하고, W1·W2가 신선한 하트비트의 기록에서 관문이 열린다. 주문 A의 부분·전체 체결, 주문 B의 취소. 그 뒤 W1~W4 요청 각 3번 이상과 17초 동안 불일치·워처 보류·`[WATCH P1]`·`confirmed difference`·킬 마커가 없다. 하트비트 기록의 검사 시각은 기록 시각 이전 15초 안. 끝에서 가짜와 OMS의 포지션(+0.002)과 열린 주문(0)이 같고, 거래소 불변식(워처 키의 주문 생성·수정·취소 없음, `orderLinkId`마다 생성 하나 이하, 키마다 초당 일괄 취소 둘 이하)이 성립한다 |
| `j2a_a_venue_only_order_holds_plain_orders_after_two_rounds_and_two_seconds_until_clear_hold` | 거래소에만 있는 주문 `manual-1`: `hold_set{watcher_mismatch}`(출처 W1, 메모 `W1 foreign …`)은 그 주문을 담은 첫 W1 답 + `min_age_ms`(2초)와 둘째 W1 답 가운데 늦은 것보다 앞서지 않고 그 뒤 5초 안에 하나 기록된다. `/api/status`의 보류, 워처의 `confirmed difference` 한 줄, 워처의 `[WATCH P1]` 하나, OMS의 `[OMS P1]`. 새 일반 주문은 `Hold(WatcherMismatch)`로 거절되고 걸린 주문의 취소는 가짜와 저널에서 닫힌다. 거래소에서 그 주문을 취소하고 W1 두 라운드 뒤 `omsctl clear-hold --reason watcher_mismatch` → `cleared`, `hold_cleared`, 새 일반 주문이 가짜에 닿고 `mismatch_resend_ms`와 W1 두 라운드 동안 보류가 돌아오지 않는다 |
| `j2b_a_venue_only_fill_holds_after_two_w2_rounds_and_is_released_after_the_oms_books_it` | 거래소에만 있는 체결(외부 `orderLinkId`의 `Trade` 0.001, `min_age_ms + 500` ms 전, OMS의 `fill_floor` + 1초 이상; 리그는 먼저 `min_age_ms + 500 > fills_lag_ms`를 단언한다), `w4_ms` 5초: 보류는 그 포지션을 보인 둘째 W2 답과 그 행을 돌려준 둘째 W4 답 가운데 이른 것보다 앞서지 않는다(출처 W2 또는 W4). 확정 줄과 `[WATCH P1]`(하나나 둘)은 W2·W4의 것뿐. 거절과 취소는 J2a와 같다. 비공개 스트림을 끊으면 OMS가 다시 붙어 체결을 올리고(`external_fill_booked`, `ExternalFill` 하나와 많아야 `PositionOutOfRange` 하나, 둘 다 `instrument:BTCUSDT`), 그 뒤 W2 두 라운드와 W4 두 답 동안 새 확정이 없다. 불일치마다 `omsctl ack`, `clear-hold` 뒤 보류가 돌아오지 않고 새 일반 주문이 받아들여지며, 가짜와 OMS의 포지션이 +0.001로 같다 |
| `j3_a_hung_oms_is_killed_by_the_watcher_and_boots_latched_until_the_operator_clears_it` | 걸린 OMS(SIGSTOP, 걸린 일반 주문 둘, 워처 `silence_ms` 6초, OMS `kill_after_ms` 30초): SIGSTOP + `silence_ms` − `probe_ms` − `request_timeout_ms`(4.5초) 전에는 마커도 워처 키의 일괄 취소도 없다. 가짜 `systemctl`의 첫 호출은 `--no-ask-password stop -- oms-node-joint.service marker=present`이고 마커(`watcher joint W5 …`)는 그보다 먼저, 워처 키의 첫 일괄 취소는 그 뒤다. OMS는 그 호출 뒤 `stop_wait_ms`(5초) 안에 SIGTERM 경로(`stopped cleanly`, 종료 0)로 끝난다. 가짜가 첫 일괄 취소 뒤 주문 하나를 남기면 워처는 세고 다시 일괄 취소해 0건으로 만들고 둘째 일괄 취소 + `watch_ms`(10초) 뒤 `the kill ended: zero open orders confirmed … the halt is not owed`로 끝낸다. `[WATCH P1]` 시작 알림(`킬 마커: 씀.`, `systemctl stop … 정지됨`, 포지션)과 정리 완료 알림(`다음 시작은 킬 마커로 래치됨`, 포지션) 하나씩. 다시 띄운 OMS: `present_at_boot` 참, `kill_latched`, 부트 기록의 `hold_set{kill, source: marker}`(메모 `marker: watcher joint W5 …`) 하나, 일반·감소 전용 주문은 `Hold(KillLatched)`로 거절, 루프가 확인하기 전의 `clear-hold --reason kill`은 `kill_unconfirmed`로 거절되고 마커는 남음, 확인과 대조의 닫힘 뒤 `cleared`, 마커 지워짐, 기준가와 관문 뒤 일반 주문이 가짜에 닿음. 워처는 둘째 킬이 없고(systemctl 호출 하나, 시작 알림 하나), W5가 다시 무장되고(`watcher_w5_armed` 1), 불일치가 없다 |
| `j4_a_ten_second_stream_outage_kills_in_the_oms_and_the_watcher_raises_nothing` | 비공개 스트림이 반쯤 열림(pong도 다시 붙기의 auth도 없음), 걸린 일반 주문 둘: 마지막 pong 뒤 9초 전에는 래치도 일괄 취소도 없고 11초 안에 `hold_set{kill, source: stream}` 하나, `OKEY`의 일괄 취소로 장부가 비고 래치 + `kill_watch_ms`(10초) 뒤 `kill_loop_ended{Confirmed}` 하나. 링크가 내려간 동안 두 주문은 저널에서 열려 있고 `clear-hold --reason kill`은 `kill_orders_open: 2 open orders`로 거절된다. 워처는 래치 뒤에도 W1 요청 10개 이상과 하트비트 5개 이상을 이어 가고, W5 줄·마커·systemctl 호출이 없고, 확인 뒤 W1 세 라운드까지 불일치 보류·`confirmed difference`·불일치 `[WATCH P1]`이 없다. 스트림이 돌아오면 대조가 두 주문을 취소로 닫고 `clear-hold`가 `cleared`, 일반 주문이 가짜에 닿고, W1 두 라운드 동안 워처는 아무것도 내지 않는다. ADR-040 전의 워처로는 래치 약 1초 뒤 두 주문의 `W1 missing_at_venue`가 확정되어 `hold_set{watcher_mismatch}`가 기록됐다(발견 F1) |
| `j5a_a_node_killed_while_its_create_is_at_the_venue_restarts_into_one_order_and_no_mismatch` | 전송 중의 죽음(워처 `silence_ms` 20초): 가짜가 OMS의 WS Trade `order.create`를 읽는 순간 주문을 장부에 올리고 자기 스레드에서 OMS에 SIGKILL을 보내며, 그 생성의 답도 `order` 프레임도 보내지 않는다(거래소가 요청을 읽었다는 것이 정해진다; 시험의 신호가 거래소보다 먼저 닿는 일이 없다). 죽은 OMS(종료 신호 9)의 저널에는 그 주문의 `order_accepted`만 있고 가짜에는 열린 주문 하나와 그 `orderLinkId`의 생성 하나가 있다. 다시 띄운 OMS는 둘째 `booted`와 같은 기록에 `order_in_doubt{Restart}` 하나를 쓰고, `OKEY`의 realtime 조회(`orderLinkId`)로 그것을 열린 주문으로 찾아 결과 불명 없는 `Working`으로 두며, 그 링크의 생성은 WS Trade와 REST, 두 프로세스를 합쳐 하나다. 관문이 다시 열린 뒤의 취소는 저널(`Canceled`)과 가짜(`Cancelled`)에서 닫는다. 워처는 내내 불일치·워처 보류·`[WATCH P1]`·`confirmed difference`·W5의 킬 줄·마커·`systemctl` 호출을 내지 않고 계속 돈다 |
| `j5b_a_node_stopped_after_the_flush_before_the_send_closes_the_order_as_not_placed` | 내구화 뒤 전송 전의 멈춤(failpoint `OMS_CRASH_AT=after_sync`: 새 주문의 기록을 fdatasync한 뒤 응답과 전송 전에 `abort`, `crash_points.rs`의 같은 이름 시험과 같은 경계; 주문 5초 유효): 전략은 응답을 받지 못하고 가짜에는 생성이 없다. 다시 띄운 OMS는 `order_in_doubt{Restart}`를 쓰고 생성 없이 `orderLinkId` 조회(재시작 뒤 realtime 둘 이상, history 넷 이상)를 이어 가며, `expires_at` + `absence_margin_us`(0.3초) 뒤에 묻고 서로 여유 이상 떨어진 찾지 못함 둘(`absence_noted` 둘)을 기록한 기록 안에서만 `order_closed{NotPlaced}`를 쓴다. 그 뒤에도 생성은 0이고, 관문이 다시 열리면 일반 주문이 받아들여진다. 워처는 J5a와 같이 아무것도 내지 않는다 |
| `j6a_a_stopped_watcher_closes_the_gate_after_the_heartbeat_limit_and_its_return_opens_it` | 워처를 SIGTERM으로 멈추면(5초 안에 종료 0) OMS는 마지막 하트비트 기록 뒤 `heartbeat_max_age`(10초)보다 늦고 그 + 틱 + 1초 이내에 `monitor_changed{monitored: false}`를 기록하고 일반 주문을 `Hold(Unmonitored)`로 거절하며, 그 기록 + `unmonitored_cancel`(10초) 뒤 + 틱 + 1초 이내에 걸린 일반 주문을 `OKEY`로 한 번 취소한다. 워처를 다시 띄우면 `boot replay done` 줄 뒤 10초 안에 관문이 열리고 주문이 받아들여진다. `--nocapture`에서 `gate closed after <ms> ms`를 찍는다 |
| `j6b_failing_watcher_checks_close_the_gate_after_the_check_age_limit` | 워처 키의 W1·W2 경로에 503: 하트비트는 계속 기록되고 W1·W2 시각은 그대로이며, 관문은 하트비트가 기록된 그 기록에서 오래된 성공 시각 + `check_max_age`(5초)보다 늦고 그 + `heartbeat_ms` + 1초 이내에 닫힌다. 닫힌 동안 일반 주문은 거절되고 OMS의 취소는 거래소에 닿는다(OMS 요청은 모두 `ok`). 고장을 걷으면 두 하트비트 + 1초 안에 열리고 주문이 받아들여진다. 내내 불일치가 없다 |
| `the_fake_systemctl_logs_the_wall_clock_in_milliseconds` | 가짜 `systemctl`은 한 초의 첫 50 ms 안에 부른 호출도 부르기 전과 뒤 사이의 벽시계 ms로 적고, 리그의 정지가 아닌 호출은 종료 1이다 |
| `the_rig_refuses_a_base_in_a_git_work_tree_a_long_socket_path_and_an_oms_test_variable` | 리그 자신의 규칙: git 작업 트리 안(없는 하위 경로도)과 상대 경로의 기준 디렉터리, 100바이트 소켓 경로를 거부하고 99바이트는 받는다. `OMS_TEST_`로 시작하는 추가 변수를 준 `Oms::start`는 프로세스를 띄우기 전에 멈춘다. 소켓 길이는 지어낸 절대 경로로 재므로 `OMS_TEST_DIR`의 길이(58바이트까지)와 상관없다 |
| `the_rig_moves_a_taken_http_port_once_and_a_second_taken_port_is_a_precondition` | 첫 시작에서 `[http]` 포트가 잡혀 있으면 리그가 대시보드 없이 돈 OMS를 끝내고 포트를 옮겨 다시 띄운다: 새 포트에서 `/api/status`가 답하고 저널에는 부팅이 하나다. 포트를 쥔 쪽이 `/api/status`에 `200`과 JSON으로 답해도 포트를 옮기고, 새 포트의 `/api/status`는 OMS의 pid를 말한다. 옮긴 포트도 잡혀 있거나, 저널이나 워처 설정이 이미 있는 시작에서 포트가 잡혀 있으면 `joint rig precondition:`으로 멈추고 OMS가 남지 않는다 |
| `the_rig_takes_an_oms_as_up_only_when_the_admin_socket_and_api_status_name_its_pid` | 규칙 `up`: 두 상태가 모두 시작한 프로세스의 pid를 말할 때만 참이다. `/api/status`가 없거나, `{}`이거나, 다른 pid이거나, admin이 다른 pid이면 거짓이고, admin이 답하지 않으면 `/api/status`를 묻지 않는다 |
| `the_rig_waits_for_the_bind_error_when_a_holder_answers_api_status` | `Oms::start`의 기다림(`port_taken`): admin이 이미 답하는 동안 포트를 쥔 쪽이 `/api/status`에 `{}`로 답해도 끝나지 않고 다음 차례에 로그의 bind 오류로 끝난다(포트 잡힘). 두 상태가 이 pid로 답하거나 프로세스가 끝나면 첫 차례에 끝난다 |

J1은 배포 기본 시간값에서 돌았다는 것도 본다: OMS 관문 정책이 코드 기본값이고, 하트비트 기록 사이가 평균 4초 이상이고 어느 둘도 2.5초보다 가깝지 않다(워처 기본 `heartbeat_ms` 5,000; 기록 하나가 2.5초까지 늦어도 통과하게 한 여유, 둘 다 판단(모델 아님)).

## 4. 증명하지 않는 것

| 대상 | 이유 | 어디로 |
|---|---|---|
| 실제 Bybit testnet에서의 합동 실행 | 범위가 거래소 호출을 금한다. testnet 주문은 `bybit-testnet` 작업만 허가됨(U-01) | 소유자 허가 뒤의 testnet 단위 |
| 실제 systemd와 polkit의 K3(A-22) | 가짜 `systemctl`이 정지를 흉내 낸다 | 설치 확인(`docs/operations/observability.md` §13.6) |
| 전원 손실, 디스크 가득, `/api/status`가 답하는 동안 걸린 OMS 코어 | SIGSTOP은 프로세스 전체를 멈춘다. 나머지는 다른 장치가 필요하다 | C-WATCH 12부의 알려진 한계 |
| 킬 마커 unlink 경쟁 | OMS 쪽 수정이고 합동 시험의 질문이 아니다 | 알려진 한계(C-HOLD) |
| 합동 설정의 `[core.policy.margin]` | 증거금 규칙은 OMS 내부이고 V-MARGIN-*가 시험한다 | U-18 |
| `watcher_mismatch` 보류 아래의 감소 전용 주문 | 증거금 정책이 있을 때만 통과하고(배포 Bybit 설정에는 있음) 합동 설정에는 없다 | U-18 |
| "킬은 감소 전용 주문도 거부한다"(표 7-7) | 증거금 정책이 없으면 모든 보류가 감소 전용 주문을 거부하므로(`oms-core/src/decide.rs:251`) 그 규칙이 있든 없든 통과한다 | U-18 |
| 합동 시험의 RSA 키 | 서명은 V-WATCH-14, V-BYBIT-RSA-1 | — |
| Hyperliquid, Binance, OKX | 소유자가 미룸; 그것들에는 워처가 없다 | — |
| 실제 `admin.sock`에 쓰인 `watcher_kill` 요청: OMS의 `"latched"` 답, `hold_set{kill, source: watcher}`, 그것으로 시작한 OMS의 킬 루프, 정지 뒤 K3를 건너뜀 | W5가 쏘는 시나리오에서 멈춘 OMS는 `hello`에 답하지 않아 K2가 요청을 쓰지 못한다 | U-21 |
| 재시작을 넘는 워처 상태 | 설계상 기록 1부터 재생한다 | — |
| REST 생성 중의 죽음 | 가짜의 훅은 WS Trade 생성에만 걸린다. REST 생성은 WS Trade가 한 바이트도 쓰지 않았을 때만 쓰인다 | — |
| 쓴 뒤 fdatasync 전, 응답 뒤 전송 전에 멈춘 노드 | J5b는 fdatasync 뒤의 경계 하나다 | V-COMMIT-2, V-COMMIT-4(가상 거래소의 실제 프로세스) |
| 결과 불명인 채 체결된 주문, 재시작이 잃은 취소(`LostOnRestart`) | J5의 주문에는 체결도 진행 중이던 취소도 없다 | V-DOUBT-*, V-BOOT-1 |
| 재시작이 W5의 `silence_ms`보다 오래 걸리는 것 | J5는 `silence_ms`를 20초로 두어 재시작보다 길게 잡았다(판단(모델 아님)). 더 길면 W5가 킬한다(J3) | — |
| W1의 두 조건(접수되지 않음, 결과 불명) 하나씩 | 다시 띄운 OMS에서 J5의 주문은 둘 다이므로 하나만 빠진 비교는 이 시험이 가려내지 못한다(mut.py V7) | 워처의 단위 시험 |
| 여유보다 가까운 찾지 못함 둘을 하나로 세는 규칙 | 게이트웨이의 조회 간격이 여유(0.3초)보다 넓어 J5b에서는 그런 둘이 생기지 않는다(mut.py V3) | V-ABSENCE-N-1 |
| 워처의 킬 루프가 도는 동안 OMS를 다시 띄우는 것 | J3은 워처의 정리 완료 알림 뒤에 다시 띄운다 | — |
| 킬 동안 거래소에서 사라진 주문 뒤의 체결, 킬 동안 나타난 거래소만의 주문 | J4에는 체결도 새 주문도 없다 | V-WATCH-6·8(W2·W4), V-WATCH-15(단위 시험) |
| 다시 시작한 OMS가 부트 기록(마커의 래치)을 내구화하기 전에 `/api/status`가 답하는가 | 답하지 않는다: 부트·설정 기록이 내구화된 뒤에야 HTTP 서버가 뜬다(`oms-node/src/node.rs`, 코드를 읽은 것; 그 순서만을 겨냥한 시험은 없다). J3은 래치가 `booted`와 같은 기록에 있음을 확인한다 | ADR-040 §8 |
| 배포 기본 시간값의 J3·J4(`silence_ms` 10초, `watch_ms`·`kill_watch_ms` 30초) | J1만 기본값으로 돈다 | U-17 |
| 배포 기본 시간값의 J2와 J6 | J1만 기본값으로 돈다(U-17). 관문의 닫힘 시각은 같은 식이지만 30초·10초·60초로 재지 않았다 | U-17 |
| 보류를 푼 뒤 차이가 남아 워처가 다시 보내는 것, W3의 불일치, W4만의 확정 | J2의 두 시험은 풀기 전에 차이가 사라지고, J2b에서는 보통 W2가 먼저 확정한다 | V-WATCH-* |
| SIGKILL·SIGSTOP으로 멈춘 워처, 링크가 내려간 동안의 2단계 취소 | J6a는 SIGTERM만, 링크는 올라 있다 | — |
| 가짜 Bybit의 동작이 실제 Bybit와 같다는 것 | 일괄 취소를 곧바로 적용하고, 타임스탬프와 WS `auth` 서명을 확인하지 않는다 | `docs/verification/README.md` §1의 가짜 설명 |

## 5. 실행 기록

| 언제 | 소스 | 명령 | 결과 |
|---|---|---|---|
| 2026-11-26 16:05~16:08 KST, 이 PC(Linux, `OMS_TEST_DIR`은 970 EVO Plus의 `/mnt/nvme/…`) | 커밋 `552e899`(`crates/`는 `e17c2fd`와 같음) | `xtask joint --runs 5` | 5번 모두 통과, 실행마다 시험 3건: 28.5 s(첫 실행, 빌드 확인 포함), 23.4 s, 23.4 s, 23.4 s, 23.3 s. 시험 자체의 시간은 23.2~23.3 s(J1이 거의 전부: 조용한 시간 17 s 포함) |
| 2026-11-26 22:13~22:20 KST, 이 PC(같은 디스크) | 커밋 `0ed69a9`(J2a, J2b, J6a, J6b가 더해짐) | `xtask joint --runs 5` | 5번 모두 통과, 실행마다 시험 10건: 88.4 s(첫 실행, 빌드 확인 포함), 82.6 s, 83.8 s, 82.7 s, 82.7 s |

| 2026-11-27 00:12~00:25 KST, 이 PC(같은 디스크) | 커밋 `3a3e943`(J3, J4와 리그 시험 `the_fake_systemctl_logs_the_wall_clock_in_milliseconds`가 더해짐; 워처의 W1은 ADR-040) | `xtask joint --runs 5` | 5번 모두 통과, 실행마다 시험 13건: 151.0 s(첫 실행, 빌드 확인 포함), 145.1 s, 146.0 s, 146.9 s, 146.4 s |

| 2026-11-27 01:59~02:13 KST, 이 PC(같은 디스크) | 커밋 `c7f6858`(J5a, J5b가 더해짐: 이 묶음의 시나리오 J0~J6이 모두 있다) | `xtask joint --runs 5` | 5번 모두 통과, 실행마다 시험 15건: 175.6 s(첫 실행, 빌드 확인 포함), 169.9 s, 170.0 s, 170.0 s, 170.1 s |

잰 값(커밋 `0ed69a9`, `--nocapture`로 J2·J6을 손으로 3번, 짧은 시간값, 이 PC; 측정값이고 다른 호스트나 부하에서의 범위가 아니다):

| 무엇 | 1 | 2 | 3 | 시험의 창 |
|---|---|---|---|---|
| J2a: F를 담은 첫 W1 답에서 보류까지 | 2,043 ms | 2,044 ms | 2,038 ms | ≥ 2,000 ms이고 둘째 W1 답 뒤, 그 + 5,000 ms 이내 |
| J2b: 비공개 스트림을 끊기 전에 OMS가 체결을 올렸나 | 아니오 | 아니오 | 아니오 | 단언하지 않음 |
| J6a: 마지막 하트비트 기록에서 관문 닫힘까지 | 10,028 ms | 10,010 ms | 10,027 ms | (10,000, 11,050] ms |
| J6a: 관문 닫힘에서 2단계 취소까지 | 10,056 ms | 10,011 ms | 10,010 ms | [10,000, 11,050] ms |
| J6a: `boot replay done`을 본 뒤 관문이 열리기까지 | 1,136 ms | 1,134 ms | 1,136 ms | ≤ 10,000 ms |
| J6b: 오래된 W1·W2 성공 시각에서 관문 닫힘까지 | 5,089 ms | 5,088 ms | 5,090 ms | (5,000, 7,000] ms |
| J6b: 고장을 걷은 뒤 관문이 열리기까지 | 984 ms | 988 ms | 980 ms | ≤ 3,000 ms |

J6a의 닫힘은 판정 틱(50 ms)이 `heartbeat_max_age`를 넘긴 첫 틱이고, J6b의 닫힘은 오래된 성공 시각 + 5초 뒤에 기록된 첫 하트비트(1초마다)다. 배포 기본값(30초, 10초, 60초, 틱 250 ms, 워처 하트비트 5초)에서는 같은 식으로 30초 + 틱까지와 10초 + 5초까지가 되지만 그 값으로는 재지 않았다(판단(모델 아님), 식에서 나온 값).

잰 값(커밋 `3a3e943`, `--nocapture`로 J3·J4를 손으로 3번, 짧은 시간값, 이 PC; 측정값이고 다른 호스트나 부하에서의 범위가 아니다):

| 무엇 | 1 | 2 | 3 | 시험의 창 |
|---|---|---|---|---|
| J3: SIGSTOP에서 마커까지 | 5,897 ms | 5,833 ms | 5,879 ms | ≥ 4,500 ms(`silence_ms` − `probe_ms` − `request_timeout_ms`) |
| J3: 마커에서 `systemctl` 호출까지(K2의 `hello` 무응답) | 1,012 ms | 1,017 ms | 1,017 ms | 마커 ≤ 호출 |
| J3: `systemctl` 호출에서 OMS 종료를 본 때까지 | 32 ms | 50 ms | 49 ms | ≤ 5,000 ms(`stop_wait_ms`) |
| J3: 워처 키의 둘째 일괄 취소에서 킬 끝 줄을 본 때까지 | 10,327 ms | 10,338 ms | 10,333 ms | ≥ 10,000 ms(`watch_ms`) |
| J4: 마지막 pong에서 래치까지 | 10,247 ms | 10,248 ms | 10,247 ms | (9,000, 11,000] ms |
| J4: 래치에서 `kill_loop_ended{Confirmed}`까지 | 10,053 ms | 10,052 ms | 10,093 ms | ≥ 10,000 ms(`kill_watch_ms`) |

J3의 마커는 SIGSTOP 뒤 약 5.8~5.9초다: 마지막 정상 탐침이 SIGSTOP보다 앞서고, W5는 `silence_ms`(6초)와 그 뒤 실패한 탐침 하나를 기다린다. 이 측정 전의 한 실행(커밋 `ff2aefd`)에서 J3은 첫 일괄 취소가 `systemctl` 호출보다 336 ms 앞선다고 실패했다: 호출은 실제로 …284.048초였고(OMS가 …284.054초에 멈추기 시작함) `date +%3N`이 `048`을 `483`으로 찍었다. 프로그램의 어긋남이 아니라 리그의 시각이었고, 리그를 고쳤다(§2).

잰 값(커밋 `b929dd6`, `--nocapture`로 J5a·J5b를 손으로 3번, 짧은 시간값, 이 PC; 측정값이고 다른 호스트나 부하에서의 범위가 아니다):

| 무엇 | 1 | 2 | 3 | 시험의 창 |
|---|---|---|---|---|
| J5a: OMS가 꺼져 있던 시간(거두는 스레드가 본 종료에서 다시 띄운 OMS가 `up`까지) | 87 ms | 93 ms | 85 ms | 단언하지 않음(`silence_ms` 20초보다 짧아야 W5가 쏘지 않는다) |
| J5a: 다시 띄운 OMS가 `up`인 때부터 그 주문이 `Working`·결과 불명 없음까지 | 305 ms | 308 ms | 307 ms | ≤ 10,000 ms |
| J5b: OMS가 꺼져 있던 시간 | 416 ms | 415 ms | 401 ms | 단언하지 않음 |
| J5b: 센 찾지 못함 둘의 물은 시각(`expires_at` 뒤) | +4,611, +9,710 ms | +4,412, +9,511 ms | +4,506, +9,506 ms | 둘 다 ≥ +300 ms(`absence_margin_us`), 서로 ≥ 300 ms |
| J5b: `expires_at`에서 `order_closed{NotPlaced}` 기록까지 | 9,793 ms | 9,594 ms | 9,589 ms | 시험 예산 안(재시작 뒤 20초) |

J5b의 찾지 못함은 약 5초 간격이다: 결과 불명 주문의 상태 조회 사이가 여유(0.3초)보다 훨씬 넓어서, 여유보다 가까운 둘을 하나로 세는 규칙은 이 시험에서 쓰이지 않는다(§4, mut.py V3). J5a의 OMS는 SIGKILL 뒤 0.1초 안에 다시 떠, 워처의 W1은 그 사이 판단하지 않고 W5는 쏘지 않는다.

## 6. CI가 돌리는 법

- `.github/workflows/ci.yml`의 둘째 작업 `joint`: `check`와 같은 계기(pull request, main으로의 push), `ubuntu-24.04`, 시간 상한 60분(판단(모델 아님): 빌드 약 8분 + 실행 5번에 각 약 5분으로 잡은 값), `check`와 나란히 돌아 `check`의 30분은 그대로다.
- 단계: `check`와 같은 고정 커밋의 checkout과 도구 사슬, 자기 키의 캐시 `joint-<OS>-<rust-toolchain.toml과 Cargo.lock의 해시>`(`check`의 캐시에는 `joint`·`test-endpoints` 기능의 빌드가 없고, 같은 키를 쓰면 이 작업이 자기 캐시를 저장하지 않는다), clippy 둘(`-p oms-node --features joint --all-targets`, `-p watcher --features test-endpoints --all-targets`; 작업 공간 clippy는 `required-features` 때문에 합동 대상을 건너뛰고 워처의 시험 기능을 빌드하지 않는다), 그다음 `cargo run --locked -q -p xtask -- joint --runs 5`.
- 작업의 `env`는 `CARGO_TARGET_DIR=${{ github.workspace }}/target`(없으면 `.cargo/config.toml`이 이 PC의 `/mnt/nvme/…`를 가리킨다), `CARGO_INCREMENTAL=0`, `CARGO_TERM_COLOR=always`. `OMS_TEST_DIR`은 작업의 `env`가 아니라 `xtask joint` 단계의 `env`에만 `${{ runner.temp }}/joint`로 둔다: 작업의 `env`에서는 `runner` 문맥을 쓸 수 없고, 쓰면 워크플로 파일이 무효가 되어 `check`까지 아무 작업도 시작하지 않는다(GitHub의 문맥 표, 여기서 돌려 보지 않음). 그 경로는 checkout 밖(git 작업 트리 밖)이고 58바이트보다 짧으며(GitHub 실행기 배치에서 `/home/runner/work/_temp/joint`, 29바이트, 여기서 재지 않음) `xtask joint`가 만든다.
- `joint`는 main 잠금 규칙이 필수 검사로 이름할 때만 머지를 막는다(U-20). 그때까지는 참고 검사다.

## 7. 변이 점검

- `xtask mutants`는 시험 인자에 `--features joint`가 있는 변이(합동 변이)마다 `cargo test` 전에 변이를 낸 사본에서 `xtask joint`와 같은 세 빌드를 하고(`joint::build`: 시험 빌드 워처, `omsctl`, 합동 빌드 `oms-node`, 사본의 대상 디렉터리에), 워처와 `omsctl`을 `$OMS_TEST_DIR/r<pid>-mbin/`에 복사하고 `--version`을 본 뒤 `JOINT_WATCHER_BIN`, `JOINT_OMSCTL_BIN`, `OMS_TEST_DIR=$OMS_TEST_DIR/r<pid>-m`으로 시험을 돌린다. 그래서 워처 소스의 변이가 시험이 띄우는 워처 바이너리에 들어간다. `OMS_TEST_DIR`의 규칙은 §1과 같고, 합동 변이는 한 번에 하나라 `r<pid>-m`은 변이마다 먼저 지운다. 점검이 끝나면 두 디렉터리를 지운다.
- 합동 변이가 죽은 것으로 세지 않는 경우: 빌드 실패나 `OMS_TEST_DIR`의 규칙 위반(`the mutant did not compile (joint prebuild)`, 규칙의 줄), 실패한 출력에 `joint rig precondition:`이 있음(`rig precondition failed`: 기준 디렉터리, 소켓 경로 길이, 다시 시도한 뒤에도 쓰이는 포트, 가드에 걸린 바이너리, 빠진 변수), 시험 뒤 `xtask joint`와 같은 고아 프로세스 검사에서 실행 디렉터리를 이름하는 프로세스가 남음(`orphan process after the run`, 시험 결과와 상관없이), 그리고 실패한 실행의 첫 panic이 시나리오의 실패가 아님(`not a scenario failure: <그 메시지>`). 시나리오의 실패는 둘뿐이다: `tests/joint.rs` 안의 단언(`unwrap` 실패는 빼고), 그리고 리그의 기다림·점검 실패(`World::fail`, 메시지가 `joint scenario:`로 시작한다; 예산을 넘긴 기다림과 끝의 거래소 불변식이 여기다). 리그 파일 쓰기의 실패(예: 디스크가 가득 참), 리그나 가짜 스레드의 다른 panic은 변이에 대해 아무것도 말하지 않으므로 죽은 것으로 세지 않는다. 그래서 합동 변이는 시나리오의 단언이나 예산으로만 죽는다. 메모는 어느 쪽이든 첫 panic의 메시지 첫 줄이다(죽은 변이에서는 실패한 단언, 앞의 `joint scenario:`는 뺀다). 한계: `tests/joint.rs` 안의 `expect(…)` 실패는 메시지로 가려낼 수 없어 단언으로 센다. 리그에 새 시나리오 점검을 넣는 부분은 그것을 `World::fail`로 실패시켜야 그 실패가 변이를 죽인다.
- 이 순서(시험 디렉터리, `r<pid>-m` 비우기, 변이가 든 사본에서 빌드, 시험, 그 디렉터리의 고아 검사, 그 결과로 판정, 소스 복원)는 `mutants.rs`의 `run_mutant`/`joint_mutant`가 하고, 시험 `mutants::tests::a_joint_mutant_is_built_from_the_mutated_copy_tested_in_an_emptied_run_directory_and_scanned_after`가 가짜 단계로 확인한다. 빌드와 변이는 `run_mutant`의 같은 인자 `work`를 쓰므로 둘이 다른 트리일 수 없다. `run`이 `run_mutant`에 사본 대신 원래 트리를 넘기는 고장은 이 시험이 보지 못한다(그때 판정은 맞지만 원래 소스가 실행 동안 바뀐다).

| ID | 고장 낸 내용 | 잡는 시험 |
|---|---|---|
| M407 | 워처가 저널을 `data_dir/journal`에서 읽는다(OMS는 거기에 쓰지 않는다): 재생이 부팅 상태에 닿지 않아 하트비트가 없고 관문이 열리지 않는다 | J1 |
| M408 | 워처의 하트비트가 검사 시각을 밀리초가 아닌 초로 보낸다: OMS는 모든 검사를 아주 오래된 것으로 읽고 관문을 열지 않는다 | J1 |
| M409 | 리그가 버전 가드 없이 워처 설정을 쓴다 | J0 |
| M410 | 워처 시험 빌드가 `WATCHER_TEST_ENDPOINTS` 없는 시작을 거부하면서 종료 코드 0으로 끝난다(여전히 `run` 전에 돌아가 아무것도 보내지 않는다) | J0 |
| M411 | OMS가 워처의 `watcher_mismatch`에 아무것도 기록하지 않고 `"held"`로 답한다: 워처는 갚은 것으로 보고, 일반 주문을 거절하는 보류가 없다 | J2a |
| M412 | OMS의 관문이 하트비트 안의 W1·W2 나이를 무시한다: 검사가 실패해도 하트비트가 오면 열려 있다 | J6b |
| M413 | 워처가 W2 차이를 둘째 라운드가 아닌 첫 목격에서 확정한다 | J2b |
| M414 | OMS가 부트 기록 전에 킬 마커를 읽지 않는다: 워처가 멈춘 OMS가 래치 없이 시작한다 | J3 |
| M415 | 워처의 킬 루프가 첫 일괄 취소 뒤 세지 않고 0건 확인으로 끝난다: 늦게 취소되는 주문이 남는다 | J3 |
| M416 | 워처의 킬이 K1(마커) 전에 K3(`systemctl stop`)를 돌린다(K1·K2 없음): 마커 없이 멈춘 OMS | J3(`marker=absent`) |
| M417 | OMS가 내구화된 `clear_hold kill` 뒤에도 마커를 남긴다: 다음 시작마다 다시 래치 | J3 |
| M418 | 워처의 W1이 저널에 킬 래치가 있는 동안에도 거래소에 없는 주문을 확정한다(ADR-040의 예외 없음) | J4 |
| M419 | 워처의 W1이 킬 래치 동안 아무 종류도 비교하지 않는다(예외가 `foreign`까지 덮음) | `watcher/a_foreign_order_during_a_kill_is_still_reported` |
| M420 | 워처의 W1이 킬 래치 동안 우리 주문의 거래소 쪽 종류(`closed_in_journal`, `open_after_terminal`, `details`)를 건너뛴다 | `watcher/the_venue_side_kinds_during_a_kill_are_still_reported` |
| M421 | OMS가 재시작이 남긴 결과 불명 주문(`order_in_doubt{Restart}`)에 `SendOrder`를 낸다. 이 묶음에서는 그 전송이 게이트웨이의 부팅 검사 전에 나가 `NotSent`(`unchecked: the boot checks have not run yet`)로 닫히므로, 거래소에 열린 주문이 저널에서 닫힌다(검사 뒤에 나갔다면 같은 `orderLinkId`의 둘째 생성; J5a는 둘 다 곧바로 실패로 본다) | J5a |
| M422 | OMS가 만료 + 여유 뒤의 첫 찾지 못함에 `NotPlaced`로 닫는다(Bybit는 둘이 필요하다, `absence_not_found_needed` 2) | J5b |
