# 보류 계약: 킬 래치, 워처 불일치, 거래소 변경

상태: 채택, 구현됨(U18b1, 2026-11-21). 설계 근거: [ADR-030](../adr/ADR-030-kill-switch-holds.md). 주장: V-HOLD-1~6. 실제 거래소에서는 한 번도 실행하지 않았다(가상 거래소와 코어 시험만). Bybit의 킬 루프(스트림 끊김 감지, 일괄 취소 반복, 비상 예산, P1, 확인 뒤의 다시 무장)는 [C-KILL](kill.md)(U18b2)이 정한다. Bybit는 거래소에서 미체결 0건을 확인하고 `confirmed`로 답한다. 가상 거래소와 Hyperliquid는 `unsupported`로 답한다. 킬의 해제는 코어 자신의 장부에 미체결이 없을 때만 받는다(`kill_orders_open`, 리뷰 S1).

## C-HOLD 운영자가 푸는 보류

### 보류 표(TN 표 7-7 중 U18b1의 셋)

| 사유 | 거는 것(이벤트) | 거절하는 것 | 푸는 것 | 재실행 |
|---|---|---|---|---|
| `kill_latched` | `hold_set{hold: kill}`: 워처의 `watcher_kill`, 시작할 때의 킬 마커, Bybit 개인 스트림 끊김(`StreamLost`, C-KILL) | **모든 주문, 감소 전용 포함**(`refuses_only_plain_orders = false`). 킬 루프가 미체결 0건을 확인해야 하고, 새 주문은 어떤 것이든 그 확인을 끝내지 못하게 한다 | `clear_hold{reason: kill}`만. 이 프로세스의 킬 루프가 답한 뒤, 그리고 코어의 장부에 미체결이 없을 때만 | `core.kill`은 `hold_set`·`hold_cleared`에서, 확인(`confirmed_at`, `outcome`)은 `kill_loop_ended`에서. `booted`가 확인을 지운다 |
| `watcher_mismatch` | `hold_set{hold: watcher_mismatch}`: 워처의 `watcher_mismatch` | 일반 주문만(`true`) | `clear_hold{reason: watcher_mismatch}` | `core.watcher_mismatch` |
| `venue_change` | 새 이벤트 없음. `apply`가 이미 있는 이벤트 `venue_position_changed`, `unclassified_execution_noted`, 종류가 `venue_position_change`인 `mismatch_raised`(넘침 경로)에서 건다 | 일반 주문만(`true`) | `clear_hold{reason: venue_change}` | `core.venue_change` |

표 7-7의 넷째 줄 `strategy_link`(그 전략의 일반 주문만, 그 전략의 `hello`가 푼다, `clear_hold`로는 풀지 않는다)는 [C-LIVE](liveness.md)에 있다(U18b3). 다섯째 줄 `unmonitored`(일반 주문만, `monitor_changed{false}`와 게이트가 있는 `booted`·`config_activated`가 걸고, 신선한 하트비트의 `monitor_changed{true}`가 푼다, `clear_hold`로는 풀지 않는다)는 [C-LIVE](liveness.md) 2부에 있다(U18b4).

- 보류는 다른 차단처럼 상태에서 매번 계산한다(`Core::holds`). 새 셋은 기존 사유 뒤에 이 순서로 붙는다. 그래서 기존 사유의 순서를 그대로 비교하는 시험이 바뀌지 않는다.
- 일반 주문만 거절하는 보류도 증거금 정책이 없으면 모든 주문을 거절한다(ADR-008 §7, ADR-018과 같다. `check_new_order`).
- 각 보류는 한 번만 걸린다: 걸려 있는 동안의 두 번째 원인은 아무 이벤트도 만들지 않고 처음의 메모가 남는다(`enter_reducing`과 같다).
- 상태 필드 셋(`kill`, `watcher_mismatch`, `venue_change`)은 비어 있으면 직렬화하지 않는다. 이 보류가 없는 실행의 상태는 바이트와 다이제스트가 그대로다. 기록 스키마는 2 그대로다.
- `venue_change`에 새 이벤트를 쓰지 않는 이유: 같은 입력에 `hold_set`을 더하면 U13b의 이벤트 목록(`["venue_position_changed", "mismatch_raised"]`, `["unclassified_execution_noted", "mismatch_raised"]`)이 바뀐다. 그 결과, U18b 전에 쓴 저널에 이 세 이벤트 중 하나가 있으면 그 저널로 시작한 노드는 `venue_change`가 걸려 있다. 안전한 쪽이고, 실제 거래소의 저널은 없다. 같은 이유로 두 가지가 따른다(리뷰 S3).
  - 옛 빌드가 쓴 스냅샷에는 `venue_change`가 없다. 그래서 이 빌드는 상태 스키마를 2로 올렸다(`STATE_SCHEMA`). 상태 스키마 1의 스냅샷은 사유와 함께 건너뛰고 저널을 처음부터 재생한다. 세그먼트는 지우지 않으므로 저널은 처음부터 있다. 보류가 스냅샷이 있었는지에 따라 달라지지 않는다.
  - 그런 저널의 `verify --re-decide`는 차이를 보고한다. 거래소 변경 뒤, 불일치를 확인(`ack`)한 다음 옛 빌드가 받은 첫 주문에서다. 증거금 정책이 있으면 첫 일반 주문, 없으면 첫 주문이다. 이 빌드는 그 주문을 `Hold(VenueChange)`로 거절한다. 판단 로직이 바뀐 것이지 저널이 상한 것이 아니다(운영 지침 7.14, 8.x의 버전 표).
- 불일치 항목은 그대로다(`ack_mismatch`를 항목마다). 거래소 변경을 풀려면 `ack_mismatch`와 `clear_hold venue_change` 둘 다 필요하다(소유자 질문 3의 기본값).
- 운영자 `halt`/`resume`은 바뀌지 않는다. `resume`은 운영자 중지만 푼다. 킬과 운영자 중지가 함께 걸려 있으면 `resume` 뒤에도 `kill_latched`가 남는다.

### 이벤트

| 이벤트 | 필드 | 뜻 |
|---|---|---|
| `hold_set` | `hold`(`kill` \| `watcher_mismatch`), `source`(`stream` \| `watcher` \| `marker`, 워처 불일치는 검사 이름 `W1`..`W4`), `note` | 보류가 걸렸다. 킬이면 라이브 처리에서 `Kill` 명령 하나가 뒤따른다 |
| `hold_cleared` | `hold`(`kill` \| `watcher_mismatch` \| `venue_change`), `note` | 운영자가 풀었다. 킬이면 `KillStop` 명령 하나가 뒤따른다 |
| `kill_loop_ended` | `outcome`(`confirmed` \| `unsupported`), `rounds`, `cancel_all_calls`, `detail` | 게이트웨이의 킬 루프가 끝났다. 래치는 남고, 이 프로세스에서 확인된 것으로 표시된다. 이벤트 종류는 C-KILL의 것이지만 U18b1이 `unsupported` 답을 위해 먼저 더한다 |

세 이벤트 모두 열린 주문·체결·포지션 집합을 바꾸지 않는다([C-TAIL](tail.md) 필드 표).

### 워처의 입력(운영 소켓, U19a/U19b가 보낸다)

새 입력 종류 `Input::Watcher`. 다른 입력처럼 기록된다.

| op | 필드 | 효과 | 응답 |
|---|---|---|---|
| `watcher_mismatch` | `check`(`W1`..`W4`), `detail` | 걸려 있지 않으면 `hold_set{watcher_mismatch, source: check, note: detail}` | `"held"` 또는 `"already held"`(기록 없음) |
| `watcher_kill` | `detail` | 걸려 있지 않으면 `hold_set{kill, source: watcher, note: detail}` | `"latched"` 또는 `"already latched"`(기록 없음). 내구화된 뒤에 답한다: 워처(U19b)는 OMS가 더 보내지 않는다는 것을 알고 자기 루프를 시작한다 |

- `W1`..`W4` 밖의 `check`, 빈(공백뿐인) `detail`, 1 KiB(`WATCHER_DETAIL_MAX` = 1024바이트)보다 긴 `detail` → `bad_request`, 기록 없음. 노드가 코어에 넘기기 전에 거절하고, 코어도 다시 거절한다. 그래서 워처 입력 하나가 저널에 쓰는 메모는 1 KiB를 넘지 않는다(리뷰 N8, 1 KiB는 판단(모델 아님)). 거래 소켓 → `forbidden`.

### `clear_hold`(운영), omsctl `clear-hold --reason kill|watcher_mismatch|venue_change --note TEXT`

`OperatorCmd::ClearHold { reason, note }`. `clear_reduce`와 같은 모양이다: 거절 함수가 고정 코드를 돌려주거나, 아니면 `hold_cleared` 하나. 거절은 `bad_request`이고 문장이 코드로 시작하며, 아무것도 쓰지 않는다. 아래 순서로 본다.

| 코드 | 언제 |
|---|---|
| `not_held` | 그 보류가 걸려 있지 않다 |
| `note_required` | 메모가 비었거나 공백뿐이다 |
| `kill_unconfirmed` | 사유가 `kill`이고 `core.kill.confirmed_at`이 없다: 래치 뒤 또는 이 프로세스가 시작한 뒤 `kill_loop_ended`가 없었다. 문장: "the kill loop has not confirmed zero open orders in this process" |
| `kill_orders_open` | 사유가 `kill`이고 코어의 장부에 미체결 주문이 있다(`core.open`, 결과가 `confirmed`이든 `unsupported`이든). 문장: "kill_orders_open: N open orders in the book; run omsctl cancel-all first". U18b1 스펙을 고친 것이다(리뷰 S1, 소유자 쪽 판단: 스펙보다 한 단계 안전) |
| `venue_change_pending` | 사유가 `venue_change`이고 `core.venue_change_wait`가 비어 있지 않다: 변경 뒤의 거래소 포지션을 아직 읽지 않았다(U13b) |

- 강제 해제는 없다(`clear_reduce`와 같다, 소유자 질문 2의 기본값). 거래소에 닿을 수 없으면 닿을 때까지 보류가 남는다.
- `kill_orders_open`에서 나아가는 길: `omsctl cancel-all`. 래치 아래에서도 받는다(취소는 새 주문이 아니다). 거래소가 취소를 확인해 장부의 주문이 닫히면 해제를 받는다. 장부는 코어가 아는 주문뿐이다. 거래소에만 있는 주문(외부 주문, 고아 주문)은 이 검사가 보지 못한다(알려진 한계).
- `booted`는 `kill.confirmed_at`(과 `outcome`)을 지운다: 프로세스마다 킬을 풀기 전에 0건을 다시 확인한다(소유자 질문 5의 기본값: 래치 아래의 재시작마다 루프가 다시 돈다).

### 킬 명령(C-GW)

- `VenueCommand::Kill { since, source, note }`: `command_for(hold_set{kill})`, 그리고 시작 전부터 래치가 있으면(저널에서 재실행된 래치) `command_for(booted)`. 다른 명령처럼 커밋 단계 2단계에서 내구화된 뒤에 풀린다: 래치가 디스크에 있은 뒤에 첫 일괄 취소가 나간다.
- `VenueCommand::KillStop`: `command_for(hold_cleared{kill})`. 게이트웨이가 루프를 끝낸다.
- `VenueGateway::kill`은 곧바로 돌아온다(루프는 자기 태스크에서 돌고, 명령 허가를 쓰지 않는다). 구동기가 두 명령을 허가 없이 넘긴다.
- 가상 거래소와 Hyperliquid: `Kill`에 `VenueInput::KillLoopEnded { outcome: Unsupported, detail: "this gateway has no cancel-all; cancel with omsctl cancel-all" }`로 답하고 `warn`을 남긴다. 코어는 `kill_loop_ended{unsupported}`로 확인을 표시한다. 그 거래소에 걸린 주문은 운영자가 `omsctl cancel-all`로 취소한다. 장부에 미체결이 남아 있는 동안 `clear_hold kill`은 `kill_orders_open`으로 거절된다. 그래서 래치는 어느 게이트웨이에서도 장부가 빈 뒤에만 풀린다. Bybit는 그 위에 거래소의 0건 확인(C-KILL)이 있어야 한다.
- 래치가 없거나 이미 확인된 뒤의 `KillLoopEnded`는 이벤트를 만들지 않는다.

### 킬 마커

- 설정: 노드 구역 `[kill] marker = "/abs/path/kill.marker"`. 절대 경로, `..` 없음, 해석한 경로가 데이터 디렉터리 밖, 읽을 때 부모 디렉터리가 있어야 한다. 없으면 마커를 보지 않는다(시작할 때 `warn` 한 줄). mainnet(`mainnet_confirmed = true`)은 `[kill]` 없이 시작하지 않는다(소유자 질문 1의 기본값). testnet과 가상 거래소에서는 선택이다.
- 시작: `Input::Boot` 전에 노드가 `symlink_metadata(marker)`를 한다.
  - `NotFound` → 마커 없음.
  - 있음(파일 종류 무관, 심볼릭 링크 포함) → `Boot { binary, kill_marker: Some(text) }`. 일반 파일이면 `text` = `marker: ` + 처음 512바이트(lossy UTF-8, 제어 문자는 공백), 아니면 `marker: (a directory at the marker path)` 같은 문장. 512는 판단(모델 아님).
  - 그 밖의 오류(권한, 입출력) → 래치. `text = "marker unreadable: <io kind>"`(fail-safe: OMS가 볼 수 없는 경로는 워처가 킬하지 않았다는 것을 증명하지 못한다).
- `Boot.kill_marker`는 없으면 쓰지 않는다(기록 스키마 2 그대로). `boot()`는 `booted` 바로 다음에, 마커가 있고 `core.kill`이 없으면 같은 기록에 `hold_set{kill, source: marker, note: text}`를 쓴다. 이미 걸려 있으면 더 쓰지 않는다.
- 지우는 것은 `clear_hold kill`뿐이다: 운영 처리기가 내구화된 `Ok`를 기다린 뒤 `unlink(marker)`를 한다(`NotFound`는 지운 것으로 본다). 응답은 `"cleared; kill marker removed"` 또는 `"cleared; kill marker NOT removed (<io kind>): the next start latches again"`. 내구화가 먼저, 지우기가 다음이다: 그 사이에 멈추면 다음 시작이 다시 래치를 건다(안전한 쪽). 반대 순서면 래치를 잃을 수 있다. 거절된 해제(`not_held`, `note_required`, `kill_unconfirmed`, `kill_orders_open`)는 파일을 건드리지 않는다. 기록의 flush가 실패한 해제도 그렇다(응답이 없고 노드가 멈춘다). `[kill]`이 없으면 응답은 `"cleared"`다.
- 마커는 시작할 때만 읽는다(실행 중 감시는 범위 밖: 지우기와 경쟁한다).

### 상태와 알림

- `status.core.kill`(`since`, `source`, `note`, `confirmed_at`, `outcome`), `status.core.watcher_mismatch`, `status.core.venue_change`(`since`, `source`, `note`). 걸려 있지 않으면 `null`. `status.node.kill_marker`: `{configured, path, present_at_boot}`(프로세스 상태). 차단 목록에 새 사유가 나온다.
- [C-ALERT](alerts.md)의 조건 `hold:kill_latched`, `hold:watcher_mismatch`, `hold:venue_change`, 유예 0. 킬 문장은 출처·시각·메모와 확인 상태(`확인: 킬 루프가 0건을 확인함 …`, `확인: 킬 루프가 아직 0건을 확인하지 않음`, `unsupported`이면 `확인: 이 게이트웨이에는 일괄 취소가 없음(…). 걸린 주문은 운영자가 취소해야 함(omsctl cancel-all). 장부에 미체결이 남아 있으면 해제는 거절됨(kill_orders_open)`)를 담는다. 워처 불일치와 거래소 변경의 문장은 증거금 정책이 있으면 `일반 주문 거절, 감소 전용은 받음`, 없으면 `일반 주문 거절, 증거금 정책이 없어 감소 전용도 거절`이라고 쓴다(리뷰 S4).
- 분류표에 없는 체결 하나는 P1 두 개를 낸다: `hold:venue_change`와 `mismatch:unclassified_execution`. 둘은 따로 풀린다(`clear-hold`와 `ack`, 리뷰 N6).
- `Alerts::notice(text)`: 접두어를 붙인 한 번의 메시지. 킬 루프 알림이 쓴다(C-KILL). `[alert]`가 없으면 게이트웨이가 알림마다 `warn` 한 줄을 남긴다.

### 중복·순서·중단·재실행

| 경우 | 결과 |
|---|---|
| 같은 `watcher_kill`이나 `watcher_mismatch`가 두 번 | 두 번째는 기록 없음, 첫 메모가 남는다 |
| 래치 아래에서 마커와 함께 다시 시작 | `booted`만(새 `hold_set` 없음). `Kill` 명령은 재실행된 래치에서 나온다 |
| `hold_set{kill}`의 flush 실패 | 응답도 `Kill` 명령도 나가지 않고 노드가 멈춘다(C-COMMIT). 다시 시작하면 래치가 없다. 워처는 답을 받지 못했으므로 다시 보낸다 |
| `clear_hold kill`이 내구화된 뒤, 마커를 지우기 전에 멈춤 | 다음 시작에서 마커가 다시 래치를 건다 |
| `clear_hold kill` 기록의 flush 실패 | 응답이 없고("outcome not confirmed") 노드가 멈춘다(exit 2). 마커는 그대로다. 기록이 파일에 닿았으면 다음 시작이 해제를 재실행하고, 마커가 그 시작의 boot 기록에서 다시 건다 |
| `kill_unconfirmed`로 거절된 해제(메모 있음) | 기록 없음, 마커 그대로. 다음 시작도 걸린다 |
| 마커를 지우지 못함(권한) | 해제는 그대로, 응답이 그렇게 말한다. 다음 시작이 다시 건다 |
| 확인 뒤 재시작 | `booted`가 확인을 지운다. 새 루프의 `kill_loop_ended`까지 `kill_unconfirmed` |
| 재실행(복구) | 세 보류와 확인은 이벤트만으로 다시 만들어진다(`apply`). `venue_change`도 결정이 아니라 `apply`에 있다. U18b 전의 스냅샷(상태 스키마 1)은 건너뛰고 저널을 처음부터 재생한다 |

### 알려진 한계

- 넘침 경로의 두 번째 거래소 변경(리뷰 N2). 넘침 경로가 `mismatch_raised{venue_position_change, instrument: X}`를 내어 `venue_change`가 걸린다. 운영자가 불일치를 확인(`ack`)하기 전에 `clear_hold venue_change`를 한다. 그 사이 X에 두 번째 넘침 변경이 오면 `raise`가 확인되지 않은 같은 항목과 겹친다고 보고 아무것도 내지 않는다. 그래서 `venue_change`가 다시 걸리지 않는다. 일반 주문은 `ack`까지 `Mismatch`가 거절한다. 그러나 `ack` 뒤에는 두 번째 변경에 보류가 없다. i64 넘침이 필요하므로 거의 일어나지 않는다. 고치려면 이벤트가 하나 더 필요하다. 그래서 고치지 않고 적어 둔다.
- 장부 밖의 주문. `kill_orders_open`은 코어의 장부만 센다. 거래소에만 있는 주문(외부 주문, 고아 주문)은 세지 않는다. 그런 주문은 대조가 `OrphanOrder` 불일치로 드러낸다. 0건의 확인은 Bybit의 킬 루프(C-KILL)가 한다.

### 범위 밖

워처 자체(W1~W5, 마커 쓰기, 자기 루프), Bybit 킬 루프([C-KILL](kill.md)), 전략 연결 감시([C-LIVE](liveness.md) 1부, U18b3), 모니터링 게이트([C-LIVE](liveness.md) 2부, U18b4), 가상 거래소와 Hyperliquid의 킬 루프, 실행 중 마커 감시, 킬 뒤의 포지션 청산(TN §7.6: 자동 감소 전용 청산 없음).

래치 전에 풀린 주문(리뷰 N5). 명령 채널에서 `Kill`보다 앞에 있던 `SendOrder`는 래치 뒤에도 거래소로 나간다(가상 거래소, Hyperliquid). Bybit는 `Kill`을 받은 때부터 루프가 끝날 때까지 생성을 `NotSent("kill switch active")`로 집에 둔다(C-KILL). 그 전에 이미 나간 생성은 루프의 30초 감시가 잡는다. 이 빌드에서는 그 주문이 코어의 장부에 이미 있다(받은 때부터 `core.open`). 그래서 `kill_orders_open`이 그 주문이 닫힐 때까지 해제를 막는다. 운영자는 `cancel-all`로 그 주문도 취소한다.
