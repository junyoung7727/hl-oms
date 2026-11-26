# ADR-039 OMS·워처 합동 프로세스 시험

상태: 채택 (2026-11-26).

검증 문서: [docs/verification/joint.md](../verification/joint.md). 설계 출처: 통합 설계 원장(`RESEARCH/CRYPTO_TRADING_INTEGRATED_TN_20261118/10_DESIGN_DRAFT.md`, 저장소 밖) §7.5~§7.7과 `pr_plan.json`의 단위 U19c(저장소 밖). 앞선 결정: [ADR-031](ADR-031-watcher.md)(독립 워처, OMS와 코드를 나누지 않음), [ADR-030](ADR-030-kill-switch-holds.md)(보류와 킬 마커), [ADR-032](ADR-032-watcher-kill-and-alerts.md)(워처의 킬과 직접 알림).

## 1. 문제와 발생 가능한 실패

- OMS(`oms-node`)와 독립 워처는 각자 상대의 가짜에만 대고 시험했다. 워처 쪽은 리그의 OMS 모형, 가짜 소켓, `http-fake`; OMS 쪽은 시험이 쓴 하트비트와 `watcher_mismatch` 줄이다. 두 실제 프로그램을 함께 띄운 시험이 없었다.
- 그래서 두 프로그램이 쓰인 인터페이스(저널 파일과 C-TAIL의 필드 표, `/api/status`, `admin.sock`의 `watcher_mismatch`·`watcher_kill`, `monitor.sock`의 하트비트, 킬 마커 경로, 같은 거래소 계좌 하나)에서 실제로 합의하는지 아무것도 보이지 않았다. 한쪽의 가짜가 상대의 실제 동작과 다르면 두 단위 시험이 모두 통과하고 배포에서 처음 어긋난다.
- 두 바이너리는 각자의 시스템 시계로 돈다. 한 프로세스 안의 주입된 시계로 둘을 함께 움직일 수 없다.
- 합동 시험이 띄우는 바이너리가 실제 Bybit, 실제 Telegram, 실제 `systemctl`에 닿으면 시험이 돈과 운영을 건드린다.

## 2. 일반적으로 만족해야 하는 성질

- 시험은 두 프로그램을 배포된 모습 그대로, 실제 인터페이스로만 본다. 한쪽의 내부를 링크해 들여다보지 않는다.
- 워처는 OMS 코드에 의존하지 않는다(ADR-031). 합동 시험이 그 경계를 흐리지 않는다.
- 시험이 띄운 바이너리는 loopback 가짜에만 닿는다.
- 실시간 시험은 기다림마다 상한이 있고, 실패하면 두 로그와 가짜의 요청 기록을 남긴다. 리그 자신의 전제 조건 실패는 시나리오의 실패와 구별된다.
- 한 번 통과는 증거가 아니다: 같은 묶음을 여러 번 연속으로 통과해야 한다.

## 3. 제약

| 사실 | 출처 |
|---|---|
| cargo는 같은 패키지의 통합 시험에만 그 패키지 바이너리의 경로(`CARGO_BIN_EXE_<이름>`)를 준다. 다른 패키지의 바이너리는 경로를 주지 않는다 | cargo 문서(`CARGO_BIN_EXE_<name>`), 이 저장소의 `crates/oms-node/tests/bybit_node.rs` |
| `failpoints` 빌드의 OMS만 `OMS_TEST_BYBIT_WIRING`과 `OMS_TEST_TELEGRAM_API`를 읽고, 그것 없이는 Bybit 게이트웨이와 직접 알림을 시작하지 않는다 | `crates/oms-node/src/venue/bybit/gateway.rs`, `crates/oms-node/src/node.rs`(V-TESTHOOK-1) |
| 워처의 `test-endpoints` 빌드만 `WATCHER_TEST_ENDPOINTS`를 읽고, 그 빌드에는 `Endpoints::of`의 호출이 없다; `--version`이 ` test-endpoints`로 끝난다 | `crates/watcher/src/main.rs`(V-TESTHOOK-1) |
| `/v5/order/` 문자열은 `crates/oms-node/tests/`, `crates/watcher/`, `crates/xtask/src/mutants.rs`, Bybit 게이트웨이에만 둘 수 있다; `watcher-conformance` 말고는 `oms-node`를 링크하지 못한다 | `tools/ci/layers.toml` |
| 워처의 토큰 파일은 git 작업 트리 안에 있으면 거부된다 | `crates/watcher/src/creds.rs` |
| Unix 소켓 경로는 108바이트(SUN_LEN)를 넘지 못한다 | `unix(7)` |
| 상태가 있는 가짜 Bybit(`Book`, `FakeBybit`)가 두 키의 요청에 같은 장부로 답한다 | `crates/oms-node/tests/bybit_fake/` |

## 4. 필수 조건과 선택 조건

필수: 두 실제 바이너리, 가짜 하나의 장부, 실제 인터페이스만, 배포 기본 시간값의 시험 하나 이상, 시험마다 시간 예산, 5회 연속 통과, 워처의 독립성 검사 통과, 실제 호스트에 요청 없음. 선택: 시험 하나의 실행 시간을 줄이는 것.

## 5. 대안

- A. 새 crate(예: `joint-tests`)에 시험을 둔다.
- B. `watcher-conformance`에 둔다.
- C. 두 라이브러리를 한 시험 프로세스에 링크하고 각자의 실행 함수를 부른다.
- D. 두 바이너리를 띄우는 감싸개 바이너리를 하나 만든다.
- E. 시험 안에서 cargo를 다시 불러 워처를 빌드한다.
- F. `oms-node` 패키지의 통합 시험(`tests/joint.rs`)으로 두고 기능 `joint`(= `failpoints`)와 `required-features`로 막는다. 워처와 `omsctl`은 `xtask joint`가 먼저 빌드해 복사하고 경로를 변수로 준다.

## 6. 비교

| | 장점 | 실패 모드 | 비용 |
|---|---|---|---|
| A | 경계가 이름으로 보임 | 두 바이너리 모두 cargo에서 경로를 얻지 못함; `layers.toml`의 `[[deny]]`와 `/v5/order/` 규칙을 고쳐야 함 | 새 crate, 층 규칙 변경 |
| B | 이미 OMS와 워처를 함께 링크함 | 같은 경로 문제; 그 매니페스트에 바이너리 기능의 관문이 없음; 적합성 시험(라이브러리 대조)과 프로세스 시험이 섞임 | 매니페스트 변경 |
| C | 한 프로세스, 시계 주입 가능 | 실제 바이너리(`main.rs`, 시그널, 시작 순서, 환경 변수)를 보지 않음; 같은 프로세스의 전역 상태가 섞임; 워처 쪽 링크가 독립성 검사와 충돌 | 큼 |
| D | 시험이 단순 | 감싸개가 또 하나의 시험되지 않은 프로그램; 배포에 없는 모양 | 바이너리 하나 |
| E | 변수 없이 돔 | 시험 안의 빌드가 시험 시간과 잠금을 먹고, 같은 대상 디렉터리의 빌드가 서로 기다림 | 시험마다 빌드 |
| F | OMS 바이너리 경로를 cargo가 보장; `/v5/order/` 규칙과 링크 규칙을 그대로 둠; 기본 `cargo test`와 `--features failpoints`는 건너뜀 | 워처와 `omsctl` 경로는 변수에 기댐(없으면 리그가 전제 조건 실패로 멈춤); 워처의 시험 빌드는 `xtask joint`만 만듦 | `xtask joint` 하나 |

## 7. 선택과 이유

F. 다른 어떤 패키지도 `oms-node`나 `watcher`를 새로 링크하지 않고, 두 바이너리를 배포된 모습 그대로 띄운다.

- `crates/oms-node/Cargo.toml`: `joint = ["failpoints"]`, `[[test]] joint`에 `required-features = ["joint"]`. 합동 시험은 `crates/oms-node/tests/joint.rs`, 리그는 `tests/joint_rig/`, 가짜는 `tests/bybit_fake/`(`main.rs`가 없는 디렉터리라 시험 대상이 아님).
- 리그는 시험마다 `$OMS_TEST_DIR/<태그>-<pid>/`에 OMS(`o/`), 워처(`w/`), 두 프로그램이 함께 쓰는 토큰 파일, 두 데이터 디렉터리 밖의 마커 디렉터리 `s/`, 가짜 `systemctl`(`bin/`)을 둔다. `Oms::start`만 `oms-node` 명령을 만들고 두 배선 변수를 넣는다(`OMS_TEST_`로 시작하는 추가 변수는 시험 오류). 워처에는 `--version`이 ` test-endpoints`로 끝날 때만 설정을 쓴다. 기다림은 15~50 ms(판단(모델 아님)) 간격의 폴링이고 시험의 예산 안에서만 기다린다. 자식마다 거두는 스레드가 종료 상태와 시각을 지키고, 리그를 버리면 살아 있는 자식에게 SIGCONT 다음 SIGKILL을 보낸다. 리그의 전제 조건 실패는 `joint rig precondition:`으로 시작한다.
- `cargo run -p xtask -- joint [--runs N] [--only <이름>]`: `OMS_TEST_DIR`(설정됨, 절대 경로, git 작업 트리 밖, 58바이트 이하; 없으면 0700으로 만듦), 세 빌드(워처 `--features test-endpoints`, `omsctl`, `oms-node --features joint`)와 복사, 시험을 한 번에 하나씩 N번, 실행마다 판정(종료 0, `test result: ok`, 한 건 이상, 무시 0)과 고아 프로세스 검사(실행 디렉터리를 이름하는 프로세스: 시험 바이너리면 SIGCONT·SIGKILL과 실패, 다른 것이면 신호 없이 실패).
- `xtask verify`의 기본 묶음에 `joint`(플래그 뒤에 두지 않음): `OMS_TEST_DIR` 없이는 실패로 남고 건너뜀이나 통과로 남지 않는다.
- 시나리오 J1은 배포 기본 시간값(코드의 기본값)으로 돈다. 짧은 시간값은 다른 시나리오의 것이다.

## 8. 포기한 장점, 남은 위험, 소유자가 정할 것

- 포기: 주입된 시계 하나(두 프로세스는 실제 시간으로 돌고, 시험은 시간 예산과 폴링으로 본다; 결정적 재현은 없다), 실제 systemd와 polkit(가짜 `systemctl`이 정지를 흉내 냄, A-22), 실제 Bybit의 시각과 비동기 일괄 취소(가짜는 곧바로 적용).
- 위험: 실시간 시험은 부하가 큰 호스트에서 늦어질 수 있다. 예산과 시간값은 판단(모델 아님)이고, 5회 연속 통과가 그 판단의 근거다.
- 위험: 가짜 Bybit가 실제 Bybit와 다르게 답하면 두 프로그램이 가짜에서 합의하고 실제에서 어긋날 수 있다. 가짜의 키 집합은 기록된 testnet 답과 대조했지만(`bybit_fake_book.rs`) 동작은 판단이다.
- 위험: 일반 `xtask verify`가 이제 `OMS_TEST_DIR`를 요구한다(없으면 그 묶음이 실패).
- 소유자가 정할 것: U-15~U-21(기본값으로 진행).

## 9. 근거

- 시험: `joint/j0_the_rig_refuses_a_watcher_built_without_test_endpoints_and_a_test_build_without_wiring_exits_2`, `joint/j1_a_whole_life_cycle_at_the_deployed_timers_has_no_mismatch_and_opens_the_gate`(V-JOINT-1), `xtask`의 `joint::tests`(규칙, 판정, 고아 검사).
- 실행: `xtask joint --runs 5`의 결과는 [docs/verification/joint.md](../verification/joint.md).

## 10. 재검토 조건

- 실제 Bybit testnet에서 합동 실행을 할 때(소유자 허가 뒤).
- 둘째 거래소에 워처를 둘 때.
- 두 프로그램이 같은 주입 시계를 받을 수 있게 될 때.
