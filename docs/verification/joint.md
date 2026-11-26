# OMS·워처 합동 시험

실제 `oms-node`(failpoints 빌드, Bybit 게이트웨이)와 실제 워처(기능 `test-endpoints`의 시험 빌드)를 함께 띄우고, 상태가 있는 가짜 Bybit 하나(`crates/oms-node/tests/bybit_fake/`: 두 키, 장부 하나), 가짜 Telegram, 가짜 VictoriaMetrics, 가짜 `systemctl`에 loopback으로만 붙인다. 시험은 두 프로그램을 실제 인터페이스로만 본다: 저널 파일, `/api/status`(워처가 묻는 그대로 HTTP)와 admin `Status`, 거래 소켓과 `omsctl`, 워처의 stderr와 지표, 킬 마커, 가짜들의 기록. 결정: [ADR-039](../adr/ADR-039-oms-watcher-joint-tests.md). 주장: V-JOINT-1.

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
- `Oms::start`만 `oms-node` 명령을 만든다: `OMS_TEST_BYBIT_WIRING`과 `OMS_TEST_TELEGRAM_API`, 그다음 시험의 추가 변수(`OMS_TEST_`로 시작하면 시험 오류). 운영 소켓과 `/api/status`가 답하거나, 프로세스가 끝나거나, 로그에 `[http]` 포트의 bind 오류(`Address already in use`)가 나올 때까지 기다린다. 포트가 잡혀 있어도 OMS는 대시보드 없이 계속 돌기 때문에(`dashboard stopped`, `crates/oms-node/src/node.rs:390-393`) 리그가 그 프로세스를 SIGKILL로 끝낸다. OMS의 첫 시작(데이터 디렉터리도 워처 설정도 아직 없음)이면 데이터 디렉터리를 지우고 포트를 바꿔 한 번 더 띄운다. 다시 잡혀 있거나, 저널이나 워처 설정(`status_url`)이 이미 그 포트를 쥔 나중의 시작이면 전제 조건 실패다.
- 워처: `JOINT_WATCHER_BIN --version`이 ` test-endpoints`로 끝날 때만 설정을 쓰고 `WATCHER_TEST_ENDPOINTS`(REST·IP 반향은 가짜 Bybit, Telegram은 가짜, `systemctl`은 리그의 스크립트)로 띄운다.
- 가짜 `systemctl`: 부를 때마다 `systemctl.log`에 인자, 그때 마커가 있었는지(`marker=present|absent`), 밀리초 시각을 한 줄. `--no-ask-password stop -- oms-node-joint.service`이면 `o/oms.pid`의 프로세스가 리그의 `o/oms.toml`을 명령 줄에 가질 때만 SIGTERM 다음 SIGCONT를 보내고(systemd.kill(5)) 5,000 ms(판단(모델 아님))까지 그것이 끝나기를 기다린다. 그 밖의 인자는 종료 1.
- 기다림은 25 ms 간격의 폴링이고 자기 상한과 시험의 예산 가운데 먼저 오는 것에서 멈춘다. 실패하면 두 로그와 가짜의 요청 기록의 꼬리를 담아 멈춘다. 리그 자신의 전제 조건(기준 디렉터리, 소켓 경로 길이, 바이너리 변수와 가드, 다시 시도한 뒤에도 쓰이는 포트, 나중의 시작에서 쓰이는 포트)은 `joint rig precondition:`으로 시작한다.
- 자식마다 거두는 스레드가 20 ms마다 `try_wait`하고 종료 상태와 시각을 지킨다. OMS의 것은 종료를 기록하기 전에 `o/oms.pid`를 지운다. 리그를 버리면 살아 있는 자식에게 SIGCONT 다음 SIGKILL을 보내고 기다린다.
- 시간값: `Timers::Deployed`(두 설정이 시간값을 적지 않음: 코드의 기본값)와 `Timers::Short`(스펙의 표). 두 설정 모두 testnet이고 `[core.policy.margin]`, `[liveness]`가 없다.

## 3. 무엇을 증명하나

| 시험 | 보이는 것 |
|---|---|
| `j0_the_rig_refuses_a_watcher_built_without_test_endpoints_and_a_test_build_without_wiring_exits_2` | 리그의 가드가 `watcher 0.1.0`을 찍는 스크립트를 설정을 쓰기 전에 거부한다. 실제 시험 빌드를 `WATCHER_TEST_ENDPOINTS` 없이 띄우면 종료 코드 2와 한 줄 `this build is for tests: WATCHER_TEST_ENDPOINTS is required`로 끝나고 가짜 Bybit·Telegram·VictoriaMetrics는 요청을 받지 않는다(실제 호스트로의 요청은 이 시험이 볼 수 없다: 시험 빌드에 그 길이 없다는 것은 V-TESTHOOK-1) |
| `j1_a_whole_life_cycle_at_the_deployed_timers_has_no_mismatch_and_opens_the_gate` | 배포 기본 시간값에서: OMS는 감시 없음으로 시작해 하트비트 전 일반 주문을 거부하고, W1·W2가 신선한 하트비트의 기록에서 관문이 열린다. 주문 A의 부분·전체 체결, 주문 B의 취소. 그 뒤 W1~W4 요청 각 3번 이상과 17초 동안 불일치·워처 보류·`[WATCH P1]`·`confirmed difference`·킬 마커가 없다. 하트비트 기록의 검사 시각은 기록 시각 이전 15초 안. 끝에서 가짜와 OMS의 포지션(+0.002)과 열린 주문(0)이 같고, 거래소 불변식(워처 키의 주문 생성·수정·취소 없음, `orderLinkId`마다 생성 하나 이하, 키마다 초당 일괄 취소 둘 이하)이 성립한다 |
| `the_rig_refuses_a_base_in_a_git_work_tree_a_long_socket_path_and_an_oms_test_variable` | 리그 자신의 규칙: git 작업 트리 안(없는 하위 경로도)과 상대 경로의 기준 디렉터리, 100바이트 소켓 경로를 거부하고 99바이트는 받는다. `OMS_TEST_`로 시작하는 추가 변수를 준 `Oms::start`는 프로세스를 띄우기 전에 멈춘다. 소켓 길이는 지어낸 절대 경로로 재므로 `OMS_TEST_DIR`의 길이(58바이트까지)와 상관없다 |
| `the_rig_moves_a_taken_http_port_once_and_a_second_taken_port_is_a_precondition` | 첫 시작에서 `[http]` 포트가 잡혀 있으면 리그가 대시보드 없이 돈 OMS를 끝내고 포트를 옮겨 다시 띄운다: 새 포트에서 `/api/status`가 답하고 저널에는 부팅이 하나다. 옮긴 포트도 잡혀 있거나, 저널이나 워처 설정이 이미 있는 시작에서 포트가 잡혀 있으면 `joint rig precondition:`으로 멈추고 OMS가 남지 않는다 |

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
| 불일치, 관문 닫힘, 킬, 충돌의 합동 시나리오 | 이 묶음의 지금 시험은 J0과 J1뿐이다 | 시나리오 J2~J6(V-JOINT-2~6) |
| 가짜 Bybit의 동작이 실제 Bybit와 같다는 것 | 일괄 취소를 곧바로 적용하고, 타임스탬프와 WS `auth` 서명을 확인하지 않는다 | `docs/verification/README.md` §1의 가짜 설명 |

## 5. 실행 기록

| 언제 | 소스 | 명령 | 결과 |
|---|---|---|---|
| 2026-11-26 16:05~16:08 KST, 이 PC(Linux, `OMS_TEST_DIR`은 970 EVO Plus의 `/mnt/nvme/…`) | 커밋 `552e899`(`crates/`는 `e17c2fd`와 같음) | `xtask joint --runs 5` | 5번 모두 통과, 실행마다 시험 3건: 28.5 s(첫 실행, 빌드 확인 포함), 23.4 s, 23.4 s, 23.4 s, 23.3 s. 시험 자체의 시간은 23.2~23.3 s(J1이 거의 전부: 조용한 시간 17 s 포함) |
