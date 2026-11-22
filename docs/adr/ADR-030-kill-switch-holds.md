# ADR-030 킬 스위치, 운영자가 푸는 보류, 전략 연결 감시, 모니터링 게이트

상태: 채택. 구현은 네 PR로 나눈다: U18b1(보류 셋, 킬 마커, `clear_hold`, 워처 입력, 킬 명령 — 구현됨, 2026-11-21), U18b2(Bybit 킬 루프, C-KILL — 구현됨, 2026-11-22), U18b3(전략 연결 감시, C-LIVE 1부), U18b4(모니터링 게이트, C-LIVE 2부). 계약: [C-HOLD](../contracts/holds.md)(U18b1), [C-KILL](../contracts/kill.md)(U18b2). [ADR-008](ADR-008-risk-reservation.md) §7의 차단 표와 [ADR-028](ADR-028-bybit-gateway.md)(킬 트리거, `kill_scope`의 사용, U18b2)을 고친다.

## 1. 문제와 발생 가능한 실패

- Bybit는 이 계좌에 거래소 쪽 dead-man(DCP)을 주지 않는다(기관 고객만, 저장소 밖 `BYBIT_V5_PRIVATE_API_FACTS_20261118.md` §4). 개인 스트림을 잃으면 OMS는 체결도 취소도 보지 못하고, 걸린 주문은 거래소에 남는다. 지금은 연결이 내려가고 신규 주문이 거절될 뿐, 아무것도 취소하지 않는다.
- 워처(U19)가 불일치를 찾거나 OMS가 조용하다고 판단해도, 줄이는 주문(감소 전용)까지 막지 않고 주문을 막을 입력이 없다. 있는 것은 `halt`뿐이고, 그것은 포지션을 닫는 감소 전용 주문도 막는다(설계 R5).
- 거래소가 스스로 포지션을 바꾸거나(ADL, 인수, 인도, 정산) 분류 못 한 실행이 오면 불일치가 오르고 `ack_mismatch`로 풀린다. 확인 한 번으로 바뀐 포지션을 보지 않고 일반 주문이 다시 흐를 수 있다.
- 워처가 킬한 사실이 OMS 재시작을 넘어 남지 않으면, 재시작한 OMS가 거래를 다시 시작한다(U-06).
- 전략 프로세스가 죽으면 그 호가가 남는다. OMS는 답을 버릴 뿐 아무것도 취소하지 않는다(`docs/contracts/api.md`). 워처 자체가 없어져도 아무것도 거래를 멈추지 않는다(U18b3, U18b4).

## 2. 일반적으로 만족해야 하는 성질

- 보류는 저널에 남는 래치이고, 재실행(복구)이 이벤트만으로 같은 상태를 만든다.
- 사람이 풀어야 하는 보류는 사람만 푼다. 시간이나 다음 입력이 저절로 풀지 않는다.
- 거래소에 나가는 효과(킬의 일괄 취소)는 그 원인이 된 기록이 내구화된 뒤에만 나간다(ADR-004).
- 포지션을 줄이는 주문은, 그것조차 위험한 경우(킬)를 빼고 막지 않는다.
- 불확실하면 막는 쪽: 확인하지 못한 것은 확인되지 않은 것으로 다룬다.
- 기존 시험의 기대값은 바뀌지 않는다(기록 스키마 2 그대로, 기존 이벤트 목록 그대로). 기존 저널은 그대로 읽힌다. 예외는 거래소 변경이 있는 U18b 전의 저널이다(리뷰 S3): 그 저널로 시작하면 `venue_change`가 걸려 있고, `verify --re-decide`는 그 뒤 옛 빌드가 받은 첫 주문(증거금 정책이 있으면 첫 일반 주문)에서 차이를 보고한다. 옛 스냅샷은 상태 스키마를 2로 올려 건너뛴다(§8).

## 3. 제약

| 사실 | 출처 |
|---|---|
| 차단은 상태에서 매번 계산한다. 저장된 차단 목록은 없다 | `crates/oms-core/src/core.rs` `Core::holds` |
| 일반 주문만 막는 차단의 통과는 증거금 정책이 있을 때만 적용한다 | `decide.rs` `check_new_order`, ADR-018 |
| 명령은 라이브 처리에서 이벤트마다 `command_for`로 만들고, 커밋 단계가 flush 뒤에 풀어 준다 | `core.rs` `command_for`, `pipeline.rs` `Committer::commit`, C-COMMIT |
| 첫 입력은 언제나 `Boot`이고 게이트웨이보다 먼저 내구화된다 | `node.rs` 3단계 |
| U13b의 시험이 거래소 변경 입력의 이벤트 목록을 그대로 비교한다 | `crates/oms-core/tests/venue_changes.rs` |
| 비어 있는 상태 필드는 직렬화하지 않아 옛 상태의 바이트가 그대로다 | `core.rs`의 `margin`, `venue_change_wait` |
| Bybit 일괄 취소: `orderFilter` 없이 모든 종류, 한 번에 많아야 500, 비동기, 자기 한도 행(linear 10/s) | 저장소 밖 `BYBIT_V5_GATEWAY_FACTS_20261120.md` Q11 |
| 소유자 결정: 불일치 때 새 주문을 막는다(Q22), 킬 스위치는 OMS와 워처가 각각 가진다, 10초 트리거, 8 + 2 요청 나눔(Q23) | 저장소 밖 `01_GRILL_DECISIONS.md` Q22–Q23 |
| 가상 거래소와 Hyperliquid 게이트웨이에는 일괄 취소가 없다 | `node.rs`, `venue/simnet.rs`, `venue/hyperliquid` |

## 4. 필수 조건과 선택 조건

필수: TN 표 7-7의 보류(킬 래치, 워처 불일치, 거래소 변경, 전략 연결, 감시 없음)와 각각의 감소 전용 열·해제 명령. 킬 래치는 재시작을 넘어 남고, 킬 마커가 있으면 시작이 래치를 건다. 킬 해제는 이 프로세스의 킬 루프가 답한 뒤에만, 그리고 코어의 장부에 미체결이 없을 때만(리뷰 S1). 킬 루프의 일괄 취소는 래치가 내구화된 뒤에만 나간다.
선택: 가상 거래소·Hyperliquid의 킬 루프(하지 않는다: 운영자의 `cancel_all`), 실행 중 마커 감시(하지 않는다: 지우기와 경쟁), 킬 뒤의 자동 청산(하지 않는다: TN §7.6).

## 5. 대안

| 대안 | 설명 |
|---|---|
| A. 워처가 `halt` | 지금 있는 운영자 중지를 워처가 건다 |
| B. 일괄 취소 한 번, 확인 없음 | 스트림을 잃으면 일괄 취소를 한 번 보내고 끝낸다 |
| C. 노드 쪽 게이트(전이만 기록) | 노드가 워처 하트비트의 신선도를 판단하고 상태 전이만 저널에 쓴다 |
| D. 전략을 잃으면 모든 전략의 주문 취소 | 연결 하나를 잃으면 계좌 전체를 일괄 취소 |
| E. 저널에 남는 래치와 `clear_hold`, 내구화된 명령이 시작하는 확인 루프, 노드가 감시하고 코어가 거는 전략 보류, 저널에 남는 하트비트를 코어가 판단하는 게이트 | 이 결정 |

## 6. 비교

| 기준 | A | B | C | D | E |
|---|---|---|---|---|---|
| 감소 전용 주문 | 막는다(R5 위반) | — | — | — | 킬만 막고 나머지는 통과 |
| 취소의 완결 | — | 비동기·500 상한이라 남을 수 있다 | — | 확인 없음 | 0건을 확인하고 30초 지켜본다(U18b2) |
| 재시작 | 운영자 중지는 남는다 | 남는 것 없음 | — | — | 래치와 마커가 남고, 프로세스마다 다시 확인 |
| 재실행으로 같은 판단 | 예 | — | 아니다(30초/10초/60초 규칙이 두 층에 산다) | — | 예(하트비트도 기록) |
| 다른 전략에 미치는 영향 | 계좌 전체 | 계좌 전체 | — | 계좌 전체 | 그 전략의 일반 주문만(U18b3) |
| 저널 크기 | 작다 | 작다 | 작다 | 작다 | 하트비트 5초마다 한 기록(하루 약 17,280, 수 MB, 판단(모델 아님)) |

## 7. 선택과 이유

E. U18b1이 만든 부분은 다음과 같다(C-HOLD).

| 단계 | 입력 | 처리 | 출력 | 실패하면 |
|---|---|---|---|---|
| H1 마커 읽기 | `[kill] marker` 경로 | `Boot` 전에 `symlink_metadata`. 없음 → 없음, 있음(무슨 파일이든) → 내용 512바이트, 그 밖의 오류 → `marker unreadable: <kind>` | `Boot { kill_marker }` | 볼 수 없는 경로도 래치(fail-safe) |
| H2 래치 | `Boot.kill_marker`, 워처의 `watcher_kill`(U18b2: `StreamLost`) | 걸려 있지 않으면 `hold_set{kill}`. 마커는 `booted`와 같은 기록에 | `core.kill`, `Kill` 명령(내구화 뒤) | flush 실패면 응답도 명령도 없이 멈춘다 |
| H3 루프의 답 | `KillLoopEnded{confirmed \| unsupported}` | 래치가 있고 아직 확인되지 않았으면 `kill_loop_ended` | `kill.confirmed_at`, `outcome` | 래치 없음·이미 확인 → 기록 없음 |
| H4 해제 | `clear_hold{reason, note}` | 거절 함수(`not_held`, `note_required`, `kill_unconfirmed`, `kill_orders_open`, `venue_change_pending`), 아니면 `hold_cleared` | 보류 해제, 킬이면 `KillStop`, 그다음 마커 지우기 | 거절은 아무것도 쓰지 않고 마커를 건드리지 않는다. 기록의 flush가 실패해도 마커는 그대로다. 마커를 못 지우면 해제는 그대로, 응답이 그렇게 말하고 다음 시작이 다시 건다 |
| H5 재시작 | 저널 | `booted`가 확인을 지우고, 래치가 있으면 `Kill`을 다시 요청 | 새 루프 | 확인 전에는 `kill_unconfirmed` |
| H6 거래소 변경 | `venue_position_changed`, `unclassified_execution_noted`, `mismatch_raised(venue_position_change)` | `apply`가 `venue_change`를 한 번 건다 | 일반 주문 거절 | — |

결정과 이유:

- **킬은 감소 전용도 막는다:** 킬 루프는 미체결 0건을 확인해야 끝난다. 감소 전용 주문도 걸린 주문이므로 확인을 끝내지 못하게 한다(TN 표 7-7). 포지션은 사람이 닫는다.
- **워처 불일치와 거래소 변경은 일반 주문만:** 포지션을 줄이는 길은 열어 둔다(R5, Q22).
- **거래소 변경은 새 이벤트 없이:** `apply`가 이미 있는 세 이벤트에서 건다. 같은 입력에 `hold_set`을 더하면 U13b의 이벤트 목록이 바뀐다. 결과로 U18b 전의 저널은 이 셋 중 하나가 있으면 래치가 걸린 채 시작한다(안전한 쪽, 실제 거래소 저널은 없다).
- **킬 해제는 장부가 빈 뒤에만(`kill_orders_open`, 스펙을 고침):** U18b1 스펙은 `unsupported` 답만으로 해제를 받았다. 그러면 이 빌드에서는(Bybit도 U18b2 전에는 `unsupported`) 주문이 걸린 채 곧바로 풀 수 있고, 래치의 보장(0건 확인 뒤에만 풀림)이 어느 게이트웨이에서도 성립하지 않는다(리뷰 S1). 그래서 답의 종류와 상관없이 코어 자신의 장부에 미체결이 있으면 거절한다. 나아가는 길은 `cancel-all`이다. 취소는 새 주문이 아니므로 래치 아래에서도 받는다. 거래소에만 있는 주문은 이 검사가 보지 못한다. 그것은 Bybit의 0건 확인(U18b2)과 대조의 `OrphanOrder`가 맡는다. 판단(소유자 쪽, 스펙보다 한 단계 안전).
- **해제는 `clear_hold` 하나, 강제 없음:** `clear_reduce`와 같은 모양(거절 코드, 이벤트 하나). 거래소에 닿지 못하면 닿을 때까지 남는다(소유자 질문 2의 기본값).
- **확인은 프로세스마다:** `booted`가 `confirmed_at`을 지우고, 래치 아래의 시작마다 `Kill`을 다시 요청한다. 앞 프로세스의 확인 뒤에 들어온 주문을 새 프로세스가 모르기 때문이다(소유자 질문 5의 기본값).
- **명령은 `command_for`에서만:** `hold_set{kill}`과(래치가 있을 때의) `booted`가 `Kill`, `hold_cleared{kill}`이 `KillStop`을 부른다. 그래서 다른 명령과 같이 커밋 단계가 flush 뒤에 풀고, 래치가 디스크에 있은 뒤에 첫 일괄 취소가 나간다.
- **게이트웨이의 `kill`은 곧바로 돌아온다:** 루프는 자기 태스크에서, 명령 허가 없이 돈다. 전송이 몇 개 걸려 있어도 킬이 기다리지 않는다. 일괄 취소가 없는 게이트웨이는 `unsupported`로 답한다(그 거래소의 주문은 운영자가 `cancel_all`로).
- **마커는 시작할 때만 읽고, 내구화된 해제 뒤에만 지운다:** 지우기 전에 멈추면 다음 시작이 다시 건다. 반대 순서면 래치를 잃을 수 있다.
- **mainnet은 `[kill]` 없이 시작하지 않는다:** 증거금 정책, `[alert]`와 같은 규칙(소유자 질문 1의 기본값).
- **불일치 항목은 그대로:** 거래소 변경을 풀려면 `ack_mismatch`와 `clear_hold venue_change` 둘(소유자 질문 3의 기본값). 기존 시험과 불일치 계약이 바뀌지 않는다.

F. U18b2가 만든 부분은 다음과 같다(C-KILL, 단계 K1~K10은 계약에).

| 단계 | 입력 | 처리 | 출력 | 실패하면 |
|---|---|---|---|---|
| B1 트리거 | 마지막 생존 증거(구독 확인, pong, 시작 확인의 첫 통과), 250 ms 감시 | 연결이 내려가 있고 `지금 − 증거 > kill_after_ms`면 끊김 하나에 한 번. 코어 큐를 기다린 시간은 증거를 민다 | `VenueInput::StreamLost { since, detail }` | 시작 확인 전에는 무장하지 않는다 |
| B2 래치 | `StreamLost` | 없으면 `hold_set{kill, source: stream}`, 확인 전이면 기록 없음, 확인 뒤면 `kill_rearmed` | `Kill`(내구화 뒤) | flush 실패면 명령 없이 멈춘다 |
| B3 루프 | `Kill` | 생성 거절을 켜고 `KillLoop`(순수 기계)를 드라이버가 주입한 시계 위에서 돈다: 일괄 취소 → 셈 → 되풀이 또는 30초 감시 | P1들, `KillLoopEnded{confirmed}` | 실패한 셈은 0이 아니다. 막힘이면 보내지 않는다 |
| B4 멈춤 | `KillStop` | 다음 대기에서 끝, 알림·입력 없음, 생성 다시 허용 | — | — |

결정과 이유(U18b2):

- **트리거는 마지막 증거에서 잰다:** 연결이 내려간 때에서 재면 반쯤 열린 연결(pong 없음)이 `2 × ping`만큼 늦게 걸린다. 시작 확인의 첫 통과도 증거로 쳐서 첫 연결 전에도 무장한다. 확인 전에는 계좌가 설정한 하위 계좌인지 모르므로 일괄 취소를 보내지 않는다.
- **루프는 순수 기계와 드라이버로:** 시간은 입력이고(`RateBudget`처럼) 시험은 수동 시계로 10분을 몇 초에 돈다. 그래서 "10분에 12개", "1초 창마다 둘 이하"를 정확한 목록으로 본다.
- **비상 예산은 주문 예산과 따로:** 일괄 취소는 거래소 한도 표의 자기 행이다. 회차마다 하나, `kill_round_ms ≥ 1,000`이 초당 하나를 지키고, 예산(2/1초)은 바깥 가드다.
- **확인 뒤의 새 계기는 다시 무장한다(`kill_rearmed`, U18b1 리뷰 N3):** 옛 확인으로 풀 수 있으면 그 뒤의 끊김에서 남은 주문을 아무도 확인하지 않는다. 새 이벤트는 확인을 지우고 `command_for`가 새 `Kill`을 부른다. 래치의 첫 메모·출처·시각은 그대로다. 판단(소유자 기본값 "예", 스펙 밖). `unsupported` 답 뒤에는 다시 무장하지 않는다: 거래소에서 확인한 것이 없고 게이트웨이가 같은 답을 하며, 해제는 `kill_orders_open`이 막는다. 그래서 U18b1의 응답(`already latched`)이 그대로이고, 답이 두 계기 사이에 오느냐에 따라 응답이 달라지지 않는다(판단).
- **`kill_loop_ended`는 C-KILL로 옮겼다:** 루프의 답이므로. 필드와 뜻은 그대로다.
- **`[alert]`가 없으면 알림마다 `warn`:** 알림을 잃지 않고 로그에 남긴다. mainnet은 `[alert]` 없이 시작하지 않는다(ADR-029).

U18b3~U18b4가 정할 것(이 ADR의 결정, 계약은 그 PR에서): 노드가 감시하고 코어가 거는 전략 연결 보류(그 전략의 일반 주문만 취소), 저널에 남는 하트비트를 코어가 판단하는 게이트(30초에 일반 주문 거절, 90초에 걸린 일반 주문 취소).

## 8. 포기한 장점, 남은 위험, 가정

- 포기: 거래소에 닿지 못할 때의 킬 해제. 키를 폐기(X8)하고 새 키로 다시 시작하면 새 루프가 돈다.
- 포기: 가상 거래소·Hyperliquid(와 U18b2 전의 Bybit)의 자동 취소. `unsupported` 뒤 운영자가 `cancel-all`로 취소하고, 장부가 빌 때까지 해제는 `kill_orders_open`으로 거절된다. 확인 문장이 그것을 말한다.
- 위험: 워처가 자기 입력을 보내지 못하면(운영 소켓에 닿지 못함) OMS는 모른다. 워처 자신의 루프(U19b)가 맡는다.
- 위험: 실행 중에 쓴 마커는 다음 시작까지 효과가 없다. 워처는 `watcher_kill`을 함께 보낸다(U19b).
- 위험: U18b 전의 저널에 거래소 변경 이벤트가 있으면 시작 뒤 `clear_hold venue_change`가 필요하다(실제 저널 없음). 그 저널의 `verify --re-decide`는 변경 뒤, `ack` 다음에 옛 빌드가 받은 첫 주문(증거금 정책이 있으면 첫 일반 주문)에서 "this build decides the input differently"를 보고한다. 판단이 바뀐 것이지 저널의 손상이 아니다.
- 결정(리뷰 S3): 상태 스키마를 1에서 2로 올렸다(`pipeline.rs` `STATE_SCHEMA`). 옛 빌드의 스냅샷에는 `venue_change`가 없어서, 스냅샷에서 복구한 노드와 저널 전체를 재생한 노드가 다른 보류로 시작할 수 있었다. 이제 상태 스키마 1의 스냅샷은 사유와 함께 건너뛰고 저널을 처음부터 재생한다. 세그먼트는 지우지 않으므로 저널은 처음부터 있다(ADR-011 §7 버전 표). 대가: 첫 시작이 저널 전체를 재생하는 만큼 느리다.
- 위험(알려진 한계, 리뷰 N2): 넘침 경로의 두 번째 거래소 변경이, 첫 변경의 불일치가 확인되기 전에 `venue_change`가 풀렸다면 다시 걸리지 않는다. 일반 주문은 `ack`까지 `Mismatch`가 막는다. i64 넘침이 필요하다. C-HOLD 알려진 한계.
- 위험(알려진 한계, U18b2): 구독은 되지만 pong에 한 번도 답하지 않는 연결이 되풀이되면 올라갈 때마다 증거가 새로 정해져 킬이 늦거나 걸리지 않을 수 있다(다시 연결하는 대기가 10초까지 길어지면 걸린다). 스펙이 "연결 올림"을 증거로 정했다. C-KILL 알려진 한계.
- 위험(U18b2): 루프의 셈은 거래소의 realtime 목록을 믿는다. 목록이 늦게 실리면 감시(30초)가 늦은 접수를 잡는다는 가정이 틀릴 수 있다. 실제 거래소에서 잰 적이 없다.
- 가정: 실제 거래소에서 이 경로를 실행한 적이 없다(가상 거래소와 코어 시험, U18b2는 loopback 가짜 Bybit와 수동 시계만).

## 9. 근거

- 시험(U18b1): `crates/oms-core/tests/holds.rs` 12개, `crates/oms-node/tests/holds_node.rs` 4개(실제 oms-node·simvenue 프로세스), `tests/crash_points.rs`의 마커 시험 2개(`kill_unconfirmed`로 거절된 해제, flush가 실패한 해제), `tests/pipeline.rs::a_kill_command_leaves_only_after_the_latch_is_durable`, `tests/record_schema.rs::kill_holds_liveness_and_heartbeats_recover_to_the_live_state`와 `a_snapshot_of_state_schema_1_is_skipped_and_the_journal_replays_the_venue_change`, `tests/alerts.rs::a_kill_latch_is_told_at_once_and_a_notice_goes_out_once`, `node::tests` 2개, `config::tests::the_kill_marker_is_validated_and_required_on_mainnet`, `alerts::tests`(증거금 정책에 따른 문장 포함), `oms-api`의 `wire_examples_of_the_holds`와 `ctl::tests::clear_hold_parses_its_reason_and_note`. 주장 V-HOLD-1~6.
- 시험(U18b2): `venue::bybit::kill::tests` 16개(순수 기계 9, 수동 시계와 http-fake의 드라이버 7), `venue::bybit::gateway::tests`의 킬 시험 8개, `config::tests::the_kill_loop_fields_are_bounded`, `holds.rs`의 `a_stream_loss_latches_the_kill_and_asks_for_the_loop`, `a_new_trigger_after_the_confirmation_resets_it_and_asks_for_a_new_loop`, `a_new_trigger_after_an_unsupported_answer_records_nothing`, 프로세스 시험 `bybit_node::a_ten_second_stream_outage_latches_kill_through_a_restart_until_clear_hold_kill`. 주장 V-KILL-1~5. 넣은 결함 M141~M153(`cargo run -p xtask -- mutants --only M14`, `--only M15`)을 하나씩 돌려 모두 잡혔다(2026-11-22).
- 넣은 결함 중 근거로 드는 것은 저장소에서 다시 돌릴 수 있는 `xtask mutants`의 M125~M140뿐이다(하나씩: `cargo run -p xtask -- mutants --only M131-`). M125~M130은 구현 때, M131~M140은 리뷰 수정 때(2026-11-21) 하나씩 돌려 모두 잡혔다. M134~M140은 구현 때 저장소 밖 스크립트로만 돌린 결함을 옮긴 것이다(리뷰 N7). 여러 파일을 바꾸는 둘(`Kill`을 `command_for` 밖에서 만듦, `venue_change`를 결정이 걸음)은 `xtask mutants`가 한 파일만 바꾸므로 옮기지 않았고, 근거로 들지 않는다.
- 설계: 저장소 밖 `10_DESIGN_DRAFT.md` §7.5, §7.6, R2–R5, TN §7.5–§7.6 표 7-7·7-8, `pr_plan.json` U18b. 스펙: `.scratch/kill-switch/spec.md`.

## 10. 재검토 조건

- 소유자가 질문 1~5에 기본값과 다르게 답할 때(mainnet 거부, 강제 해제, 거래소 변경 해제의 두 단계, 전략 연결 감시의 기본, 재시작마다의 루프).
- Bybit가 이 계좌에 DCP를 줄 때: 거래소 쪽 dead-man이 트리거를 대신할 수 있다.
- 워처(U19)의 입력 형식이 바뀔 때.
- 노드를 자동으로 다시 띄우게 될 때(U-06): 래치 아래의 재시작마다 루프와 P1.
