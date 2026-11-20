# 연결과 대조 계약

설계 근거: [ADR-014](../adr/ADR-014-reconciliation.md). 구현: `Tx::on_link`, `on_status`, `on_fills_synced`, `on_open_orders`, `on_positions`(`crates/oms-core/src/decide.rs`),
`Core::needs`(`needs.rs`), 대조기(`crates/oms-node/src/venue/reconciler.rs`).

## C-LINK 링크 상태

| 항목 | 내용 |
|---|---|
| 입력 | `Link { up, session, reason }`. 게이트웨이가 스트림 연결의 성립·상실을 알린다 |
| 변경 | `LinkChanged`. 올라올 때 세션 번호가 바뀐다 |
| 차단 | 내려가 있으면 `LinkDown`. 올라온 뒤 재확인이 끝날 때까지 `Resync` |
| 외부 효과 | 올라올 때: 요청만 돼 있고 보내지 못한 취소를 보낸다 |

| 상황 | 동작 |
|---|---|
| 같거나 작은 세션 번호의 `up` | 무시(늦게 온 통지) |
| 현재보다 작은 세션 번호의 `down` | 무시. 이전 연결에 대한 늦은 통지가 지금 연결을 내리면 안 된다 |
| 이미 내려가 있는데 `down` | 무시 |
| 내려간 동안의 신규 주문 | `Hold(LinkDown)`으로 거절 |
| 내려간 동안의 취소 | 요청을 기록. 전송은 링크가 돌아온 뒤 |
| 내려갈 때 진행 중이던 전송 | 게이트웨이가 각각 결과(`Unknown`)를 돌려준다. 주문은 결과 불명이 된다 |

## C-RECON 재확인과 대조

`Resync`가 풀리는 조건(모두 현재 세션에서):

| 조건 | 충족시키는 입력 | 이벤트 |
|---|---|---|
| 모든 `Working` 주문에 신선한 증거 | 주문별 `OrderStatus`(조회 또는 스트림), 또는 열린 주문 목록에 포함 | `StatusNoted`, 또는 접수 증거일 때 `OrderAcked { session }` |
| 체결 동기화 | `FillsSynced { up_to }` | `FillsSynced` |
| 열린 주문 훑기 | `OpenOrders { queried_at, orders }` | `OpenOrdersSwept` |
| 포지션 검사 통과 | `Positions { queried_at, positions }` | `PositionsVerified` |

"신선한 증거"는 현재 세션에서 얻은 것만이다. 세션이 붙어 오는 증거(스트림 통보, 조회 응답)는 그 세션 번호로 판단한다.
세션이 붙지 않는 증거(전송 응답, 체결)는 주문을 내보낸 뒤 링크가 바뀌지 않았을 때만 현재 세션의 것으로 친다.
결과 불명이던 주문을 접수 처리하는 증거에도 같은 규칙이 적용된다: 옛 세션의 통보로 접수는 되지만 재확인 조건은 채우지 못한다.

읽는 상태는 주문, 계좌 장부, 동기화 진행 상태다. 대조 입력은 포지션을 직접 바꾸지 않는다. 바뀌는 것은 증거 표시와 불일치 목록이다.

### 대조별 규칙

**체결 동기화.** 게이트웨이는 `since` 이후의 체결을 모두 `Fill`로 전달한 **뒤에** `FillsSynced { up_to }`를 보낸다.
`up_to`는 "이 거래소 시각까지의 체결은 모두 전달했다"는 뜻이며, 조회가 평가된 시각보다 엄격히 앞이어야 한다(같은 시각의 체결이 뒤에 생길 수 있다).
코어는 `fills_up_to`를 올리고 `Closing` 주문의 종결을 다시 판정한다.

**열린 주문 훑기.**

| 거래소 목록의 주문 | 로컬 | 동작 |
|---|---|---|
| 우리 ID, 로컬에 미종결 | — | 상태 증거로 처리(결과 불명이면 접수) |
| 우리 ID, 로컬에 종결, 종결 시각 < 조회 발신 시각 | 모순 | `OpenAfterClose` |
| 우리 ID, 로컬에 종결, 종결 시각 ≥ 조회 발신 시각 | 스냅샷이 오래됨 | 무시 |
| 우리 ID, 로컬에 없음 | 저널 유실 또는 부재 판정 오류 | `OrphanOrder` |
| 남의 ID 또는 ID 없음 | 외부 주문 | `ForeignOrder`. 단, ID가 없고 거래소 주문 ID를 아직 모르는 같은 종목·방향의 우리 주문이 있으면 이번에는 넘어간다(전송 응답이 오는 중일 수 있다). 다음 훑기에서 식별되거나 그때 제기된다 |
| (목록에 없음) 로컬 `Working`, 접수 시각 < 조회 발신 시각 | 닫혔거나 통보를 놓침 | `OrderRecheck` → 대조기가 그 주문의 상태를 조회 |

**포지션 검사.** 조회 발신 뒤에 체결을 기장했으면 판정하지 않는다(다음 회차에 다시). 그렇지 않으면 종목마다
`거래소 포지션 ∈ [q − S, q + B]`를 확인한다. 벗어나면 `PositionOutOfRange`. 설정에 없는 종목에 포지션이 있으면 `ForeignPosition`.

### 변형 입력

| 상황 | 동작 |
|---|---|
| 같은 조회 결과가 두 번 | 두 번째는 변화 없음(누적량 단조, 체결 키 중복 제거, 같은 불일치 재제기 안 함) |
| 이전 세션의 결과가 늦게 옴 | 주문 상태의 내용은 적용, 신선한 증거로는 세지 않음(접수 증거여도 같다). 훑기와 포지션은 무시 |
| 접수된 주문에 대한 "없음" 응답 | 닫지 않는다. 접수 시각 + 여유 뒤의 조회면 `StatusConflict`를 제기한다. 그 주문의 재확인은 끝나지 않으므로 `Resync`도 남는다. 운영자가 거래소에서 확인하고 `resolve`한다 |
| 조회가 유실됨 | 필요가 남아 있으므로 대조기가 다시 보낸다 |
| 조회 시간 초과 | 게이트웨이는 아무것도 전달하지 않는다. 위와 같다 |
| 대조 도중 프로세스 중단 | 대조기에는 복구할 상태가 없다. 재시작 뒤 새 세션에서 처음부터 다시 |
| 대조 도중 체결 도착 | 정상 기장. 포지션 검사는 그 회차를 건너뛴다 |

### 시스템은 외부 상태를 실제로 아는가

대조가 끝난 시점에 아는 것: 미종결 주문 각각의 거래소 상태(현재 세션에서 확인), 동기화 시각까지의 체결 전부, 열린 주문 목록에 외부 주문이 없었음, 포지션이 설명 범위 안이었음.
모르는 것: 그 뒤의 변화(스트림이 알려 준다), 조회 응답과 실제 사이의 지연, 15초 훑기 간격 사이에 생겼다 사라진 외부 주문.

열린 주문과 포지션의 판정은 "응답이 조회를 보낸 시각보다 오래된 상태가 아니다"라는 가정(A-13)에 기댄다. 뒤처진 응답은 거짓 불일치를 만든다.
거짓 불일치는 거래를 막을 뿐 장부를 바꾸지 않는다.

## C-MISMATCH 불일치

| 항목 | 내용 |
|---|---|
| 변경 | `MismatchRaised { id, kind, subject, detail }`. 같은 `(kind, subject)`의 미확인 항목이 있으면 다시 제기하지 않는다. 대상은 주문, 종목, 거래소 주문 중 하나다. 외부 체결은 종목마다 한 건으로 묶인다(체결마다 따로 기장되고 저널에 남는다) |
| 중단되면 | `MismatchRaised`가 내구화됐으면 재시작 뒤에도 남고 차단도 남는다. 안 됐으면 같은 입력이 다시 올 때 다시 제기된다 |
| 누가 언제 보는가 | 내구화 뒤 `status`와 화면에 나타난다. 구독 이벤트로는 가지 않는다(주문 이벤트가 아니다) |
| 복구 뒤 | 다시 실행하는 것은 없다. 확인하지 않은 항목은 그대로 차단한다 |
| 차단 | 미확인 항목이 하나라도 있으면 계좌 전체 `Mismatch` |
| 해제 | 운영자의 `ack`(건별, 사유 기록). 다른 항목은 남는다 |
| 조정 | 포지션을 고치려면 `adjust`(종목, 장부, 증감, 사유). 계좌와 지정 장부에 함께 적용돼 I-ATTR이 유지된다. 거래소가 만든 포지션 변경 뒤에는 코어가 다음 포지션 응답으로 직접 맞춘다(`reason: snapshot_after_venue_change`, 미귀속 장부). 운영자 조정은 `reason`을 쓰지 않는다(기록 스키마 1과 같은 바이트) |
| 보존 | 확인된 항목은 보존 시간 뒤 메모리에서 제거. 저널에는 남는다 |

원인이 그대로면 확인 뒤 다음 훑기에서 다시 제기된다. 예외: `CancelStuck`은 시도가 상한에 이른 순간에 한 번만 제기한다(확인한 뒤 다시 뜨지 않는다. 취소를 다시 요청하면 새로 센다).

| 종류 | 대상 | 뜻 |
|---|---|---|
| `Overfill`, `CumExceedsFinal`, `MissingFills`, `FillAfterClose`, `FillFieldConflict`, `StatusConflict`, `CancelStuck` | 주문 | 그 주문에 대한 거래소의 통보가 장부나 서로와 맞지 않는다 |
| `OpenAfterClose` | 주문 또는 거래소 주문 | 닫은 주문이 거래소에 열려 있다 |
| `OrphanOrder`, `ForeignOrder` | 거래소 주문 | 거래소에 장부에 없는 주문이 있다(우리 ID / 남의 것) |
| `ExternalFill`, `PositionOutOfRange`, `ForeignPosition` | 종목 | 주문 없이 체결이나 포지션이 생겼다 |
| `ParkedFillOverflow` | 전체 | 주인을 기다리는 체결이 상한을 넘었다 |
| `VenuePositionChange` | 종목 | 거래소가 스스로 포지션을 옮겼다(ADL, 청산 포지션 인수, 인도, 정산). [ADR-023](../adr/ADR-023-venue-made-executions.md) |
| `UnclassifiedExecution` | 종목 | 게이트웨이가 분류하지 못한 실행이 왔다. 기장하지 않았다 |

## 검증

`scenarios::link_loss_blocks_new_orders_and_resync_is_required_after_it_returns`, `stale_session_evidence_does_not_satisfy_resync`,
`a_pending_cancel_is_sent_when_the_link_returns`, `position_check_uses_the_reachable_range`,
`a_position_snapshot_older_than_the_last_booked_fill_is_inconclusive`, `foreign_and_orphan_open_orders_are_flagged`,
`an_in_doubt_order_seen_in_the_open_order_sweep_is_acknowledged`, `acknowledging_one_mismatch_does_not_clear_another`,
`position_adjustment_is_explicit_and_keeps_the_ledgers_consistent`.
`gaps::each_part_of_the_re_confirmation_is_required`, `review_core::f4_…`, `f5_…`, `f7_…`, `f8_…`. 불일치 종류마다의 시험은 주장 V-MIS-1에 있다.
시뮬레이션의 최종 검사(`final-position`, `final-orders`, `final-holds`, `no-false-mismatch`, `external-detection`).
프로세스 `e2e::events_missed_while_down_are_recovered_from_the_venue`, `link_loss_holds_new_orders_until_resynchronised`, `external_activity_is_detected_and_needs_an_operator`.

확인하지 않은 것: 실제 거래소의 조회 지연과 스냅샷 범위(A-13). 체결이 끊임없이 들어오는 상황에서 포지션 검사가 얼마나 미뤄지는지.
