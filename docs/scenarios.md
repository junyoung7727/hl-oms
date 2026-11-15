# 정상·장애 시나리오

시나리오마다 다음을 적는다: 무엇이 일어나는가, 저널에 무엇이 남는가, 누가 무엇을 보는가, OMS가 무엇을 알고 무엇을 모르는가, 어떤 시험이 확인했는가.
이벤트 순서는 대시보드의 "동작 탐색"에서 눌러 볼 수 있다(원본: `registry/paths.toml`). 상세 계약은 [contracts/](contracts/README.md)에 있다.

시험 이름의 앞부분: `scenarios::`는 `crates/oms-core/tests/scenarios.rs`, `e2e::`는 `crates/oms-node/tests/e2e.rs`(실제 프로세스), `pipeline::`는 `crates/oms-node/tests/pipeline.rs`, `sim::`는 `crates/oms-sim/tests/sim.rs`.

## 1. 정상 경로

### 1.1 접수 → 거래소 접수 → 체결 → 종결

| 단계 | 일어나는 일 | 저널 | 밖에서 보이는 것 |
|---|---|---|---|
| 1 | 전략이 `new_order`를 보낸다 | — | — |
| 2 | 코어가 검사하고 한도 여유를 예약한다 | `order_accepted` | 아직 없음 |
| 3 | 커밋 단계가 기록을 쓰고 `fdatasync`한다 | 내구화 | 이제 응답 `accepted`, 게이트웨이로 전송 명령, 화면 갱신 |
| 4 | 게이트웨이가 거래소에 보내고 "놓임" 응답을 받는다 | `order_acked` | 구독자에게 이벤트 |
| 5 | 체결 통보 | `fill_booked`(체결 키로 한 번만) | 포지션과 예약 변경 |
| 6 | 체결량이 주문 수량에 도달하거나 종결 통보 | `venue_terminal`, `order_closed` | 예약 해제 |

`accepted`는 "저널에 내구화됐다"는 뜻이다. "거래소가 받았다"는 뜻이 아니다.

확인: `e2e::vertical_slice_new_partial_fill_cancel_restart`, `scenarios::full_fill_by_fills_alone_closes_the_order`, `scenarios::fia_7_1_reservation_follows_potential_fills`.

### 1.2 부분 체결 후 취소

취소 요청은 예약을 풀지 않는다. 취소가 거래소에 닿기 전에 체결될 수 있기 때문이다. 예약은 거래소의 종결 통보와 체결 동기화가 풀어 준다.

취소가 잘 안 될 때:

| 상황 | 동작 |
|---|---|
| 취소 결과를 모른다, 보내지 못했다, 거부됐다 | 1초 뒤 다시 시도. 한 번의 요청에 5회까지 |
| 결과가 30초 동안 오지 않는다 | 불명으로 기록하고 다시 시도 |
| 5회를 다 썼다 | `CancelStuck` 불일치가 한 번 뜬다. 신규 주문이 막힌다 |
| 그 뒤 전략이나 운영자가 다시 취소를 요청 | 새로 5회 |
| 주문이 결과 불명인 동안 취소를 시도했다가, 주문이 발견됨 | 그동안 쓴 시도는 세지 않는다. 발견된 주문에 취소를 보낸다 |

확인: `scenarios::cancel_race_final_status_decides`, `scenarios::cancel_is_authorised_per_owner_and_idempotent`, `review_core::f1_…` 5건, `sim` 전체(변이 M13이 이 성질을 깨면 시뮬레이션이 잡는다).

### 1.3 코어의 거절, 거래소의 거부

| 경우 | 저널 | 키 | 예약 |
|---|---|---|---|
| 코어가 거절(형식, 한도, 차단 사유) | `request_rejected` | 소모되지 않는다. 같은 키로 다시 낼 수 있다 | 없음 |
| 거래소가 명시적으로 거부 | `order_accepted`, `order_closed(rejected_by_venue)` | 소모된다 | 전부 해제 |
| 게이트웨이가 보내지 못했음이 확실 | `order_accepted`, `order_closed(not_sent)` | 소모된다 | 전부 해제 |

확인: `scenarios::field_validation_reasons`, `a_rejection_does_not_consume_the_key`, `definite_gateway_failures_close_and_release`.

## 2. 같은 입력이 다시 온다

| 상황 | 결과 | 확인 |
|---|---|---|
| 전략이 같은 키·같은 내용으로 다시 보낸다 | `duplicate`. 기록 없음, 전송 없음 | `scenarios::same_key_same_request_creates_one_order_and_one_send`, `e2e::a_repeated_request_never_creates_a_second_order` |
| 같은 키·다른 내용 | `conflict`. 기록 없음 | `scenarios::same_key_different_request_is_a_conflict_without_effect` |
| 재시작 뒤 같은 키 | `duplicate`. 키는 저널에서 복원된다 | `scenarios::the_duplicate_answer_survives_restart` |
| 요청 만료 + 유예 뒤 같은 키 | 주문이 메모리에서 지워졌다. 만료 시각이 지났으므로 `expired`로 거절 | `scenarios::a_key_is_forgotten_only_after_its_request_expired`, `sim::profile_purge` |
| 같은 체결이 스트림과 조회로 두 번 온다 | 체결 키가 같으므로 한 번만 기장 | `scenarios::duplicate_fill_is_booked_once` |
| 같은 취소 요청 | `already_requested` | `scenarios::cancel_is_authorised_per_owner_and_idempotent` |

멱등성 키의 범위는 전략별이고, 요청 내용에는 **절대 만료 시각**이 포함된다. 그래서 전략은 재시도할 때 같은 만료 시각을 보내야 한다.

## 3. 입력 순서가 바뀐다

| 상황 | 결과 | 확인 |
|---|---|---|
| 체결이 접수 응답보다 먼저 온다(주문을 식별할 수 있다) | 체결이 곧 존재의 증거다. `order_acked` 뒤 `fill_booked` | `scenarios::fill_before_send_result_acknowledges_the_order` |
| 체결에 거래소 주문 ID만 있고 그 ID를 아직 모른다 | `fill_parked`. ID를 알게 되면 `fill_unparked`, `fill_booked` | `scenarios::fill_by_venue_id_before_the_id_is_known_is_parked_then_booked` |
| 주인을 끝내 찾지 못한 체결 | 주인일 수 있는 주문이 없어지거나 120초가 지나면 외부 체결로 기장하고 불일치 | `scenarios::unidentified_fill_is_booked_as_external_after_the_wait_and_holds_trading`, `a_parked_fill_is_external_once_no_order_can_own_it` |
| 종결 통보가 체결보다 먼저 온다 | `Closing` 단계에서 기다린다. 아직 기장되지 않은 수량은 예약에 남는다 | `scenarios::terminal_before_fills_keeps_the_unbooked_quantity_reserved` |
| 종결 통보에 최종 수량이 없다 | 체결 동기화가 종결 시각을 지난 뒤에만 닫는다 | `scenarios::terminal_without_final_quantity_waits_for_fill_sync` |
| 취소 접수 뒤 체결 통보 | 기장한다. 최종 상태는 거래소의 종결 통보가 정한다 | `scenarios::cancel_race_final_status_decides` |
| 닫은 주문에 체결이 온다 | **버리지 않는다.** 닫은 사유가 무엇이든 그 주문에 기장하고 `FillAfterClose` 불일치. 주문이 메모리에서 지워진 뒤라면 외부 체결로 기장 | `gaps::a_fill_after_any_kind_of_close_is_booked_and_flagged`, `gaps::a_fill_for_a_purged_order_is_booked_as_external` |
| 거래소 주문 ID만 가진 체결이 대기하는 중에, 같은 주문의 다른 체결이 그 ID를 알려 준다 | 두 체결 모두 그 주문에 기장 | `review_core::f2_…` |
| "놓이지 않음"으로 닫은 주문이 놓였다는 응답이 온다 | `OpenAfterClose` 불일치. 신규 주문 차단 | `review_core::f3_…` |
| 이전 연결에 대한 단절 통보가 늦게 온다 | 무시. 지금 연결은 그대로다 | `review_core::f5_…` |
| 이전 연결에서 온 늦은 통보 | 적용은 하지만 "현재 세션의 증거"로 세지 않는다 | `scenarios::stale_session_evidence_does_not_satisfy_resync` |

## 4. 외부 결과를 모른다

### 4.1 응답이 유실됐다

게이트웨이가 시간 초과나 연결 끊김을 만나면 결과를 `Unknown`으로 보고한다. 코어는 주문을 결과 불명(`order_in_doubt`)으로 표시한다.

- 예약은 유지된다. 주문이 놓였을 수 있기 때문이다.
- 계좌 전체의 신규 주문이 막힌다(`in_doubt` 차단).
- **다시 보내지 않는다.** 거래소가 같은 식별자의 재전송을 거부한다는 보장이 없다.
- 스트림 통보나 조회에서 주문이 보이면 정상 주문이 된다.

확인: `scenarios::unknown_send_outcome_keeps_the_reservation_and_blocks_new_exposure`, `in_doubt_order_found_by_query_becomes_working`, `e2e::a_lost_response_is_covered_by_stream_evidence`.

### 4.2 요청이 유실됐다

조회하면 "없음"이 나온다. 그런데 "없음"은 "아직 도착하지 않음"일 수도 있다. 그래서:

- 요청마다 거래소에 만료 시각(`expiresAfter`)을 함께 보낸다. 거래소는 그 시각이 지난 요청을 놓지 않는다(가정 A-01).
- 만료 시각 + 여유(기본 10초)가 **지난 뒤**의 조회에서 "없음"일 때만 `order_closed(not_placed)`로 닫는다.
- 거래소가 만료를 강제하지 않는 설정이면 절대 자동으로 닫지 않는다. 운영자가 닫는다.

확인: `scenarios::not_found_closes_an_in_doubt_order_only_after_expiry_plus_margin`, `not_found_never_closes_when_the_venue_does_not_enforce_expiry`, `e2e::a_lost_request_is_closed_only_after_its_send_window`, `sim::absence_rule_*`(가정을 깨면 위반이 난다).

### 4.3 결과가 아예 오지 않는다

게이트웨이가 결과를 보고하지 못한 채 30초(설정 `send_result_timeout_us`)가 지나면 코어가 스스로 결과 불명으로 바꾼다.

확인: `scenarios::a_send_without_any_result_becomes_in_doubt_after_the_timeout`.

## 5. 프로세스가 죽는다

"기록 → 내구화 → 응답·전송"의 각 경계에서 죽었을 때 남는 것은 [C-COMMIT](contracts/commit.md)의 표에 있다. 요약:

| 죽은 시점 | 저널 | 전략이 본 것 | 거래소 | 재시작 뒤 |
|---|---|---|---|---|
| 판단 뒤, 기록 전 | 없음 | 응답 없음 | 전송 없음 | 그 요청은 없었던 것이다. 전략이 같은 키로 다시 보낸다 |
| 기록 중(쓰다 만 프레임) | 불완전한 꼬리 | 응답 없음 | 전송 없음 | 꼬리를 보존하고 자른다. 위와 같다 |
| 내구화 뒤, 응답·전송 전 | `order_accepted` | 응답 없음 | 전송 없음 | 결과 불명으로 표시. 조회에서 없으므로 만료 뒤 닫힌다. 전략이 다시 보내면 `duplicate` |
| 전송 뒤, 결과 기록 전 | `order_accepted` | `accepted` | 주문이 있을 수 있다 | 결과 불명으로 표시. 조회로 찾는다. **다시 보내지 않는다** |
| 결과 기록 뒤 | `order_acked` | `accepted` | 주문 있음 | 재확인 뒤 계속 |

재시작한 프로세스는 링크가 붙고 체결·열린 주문·포지션을 현재 세션에서 다시 확인할 때까지 신규 주문을 받지 않는다.
죽어 있던 동안의 체결과 거래소 주도 취소는 조회로 가져온다.

확인: 표의 행마다 그 경계에서 프로세스를 멈추는 시험이 있다(`crash_points.rs` 5건, 전송 중은 `e2e::a_kill_during_send_does_not_duplicate_the_order`).
그 밖에 `scenarios::restart_puts_outstanding_sends_in_doubt_and_never_resends`, `e2e::events_missed_while_down_are_recovered_from_the_venue`, `e2e::random_kills_under_load_keep_books_consistent`, `sim::profile_crashy`.

panic이 나면 프로세스 전체가 끝난다. 코어 스레드만 죽고 소켓이 살아 있는 상태는 만들지 않는다.

확인하지 않은 것: 전원 장애. SIGKILL과 abort는 OS가 이미 받은 쓰기를 잃지 않는다. panic으로 인한 종료.

## 6. 저장이 실패한다

| 상황 | 동작 |
|---|---|
| `write`나 `fdatasync`가 오류를 돌려준다 | 그 배치의 응답·전송·화면 갱신을 하나도 내보내지 않는다. 다시 시도하지 않는다. 프로세스가 종료 코드 2로 멈춘다 |
| 여유 공간이 기준 아래 | 신규 주문만 막는다. 체결 기장·취소·대조는 계속한다 |
| 저널 끝이 불완전하다 | 원본을 보존하고 자른 뒤 시작 |
| 저널 중간이 손상됐다 | 시작을 거부한다. 파일을 건드리지 않는다 |

flush를 다시 시도하지 않는 이유: Linux에서 `fsync` 실패 뒤의 재시도는 성공을 돌려주면서 데이터가 사라져 있을 수 있다([ADR-004](adr/ADR-004-commit-gate.md)).

확인: `pipeline::outputs_follow_durability_with_a_commit_thread`, `…_with_inline_commit`, `scenarios::storage_low_blocks_new_orders_but_not_fills_or_cancels`, `e2e::journal_damage_is_handled_by_position`, `oms-journal`의 `journal.rs`.

## 7. 거래소 연결이 끊긴다

링크가 내려가면 신규 주문이 막힌다. 다시 붙으면 세션 번호가 바뀌고, 다음 넷을 **새 세션에서** 확인할 때까지 계속 막힌다:
미종결 주문마다의 상태, 체결 동기화, 열린 주문 목록, 포지션.

포지션은 정확히 같은지를 보지 않는다. 열린 주문이 체결될 수 있으므로 `[기장 − 매도 예약, 기장 + 매수 예약]` 구간 안에 있는지를 본다.
마지막 체결보다 오래된 포지션 스냅샷으로는 판정하지 않는다.

확인: `scenarios::link_loss_blocks_new_orders_and_resync_is_required_after_it_returns`, `position_check_uses_the_reachable_range`, `a_position_snapshot_older_than_the_last_booked_fill_is_inconclusive`, `e2e::link_loss_holds_new_orders_until_resynchronised`.

## 8. OMS 밖에서 계좌가 바뀐다

| 사건 | 탐지 | 결과 |
|---|---|---|
| 수동 거래, 다른 프로그램의 체결 | 주인 없는 체결 | 계좌 장부에 기장, `ExternalFill` 불일치, 신규 주문 차단 |
| 남의 열린 주문 | 열린 주문 조회 | `ForeignOrder` 불일치 |
| 청산, 자동 디레버리징 | 체결 통보 또는 포지션 대조 | 외부 체결 또는 `PositionOutOfRange` |
| funding | 포지션을 바꾸지 않는다 | 탐지 대상이 아니다. 비용은 장부에 없다(U-07) |

운영자가 원인을 확인하고 `ack`해야 풀린다. 수동 거래를 병행할지는 U-08이다.

확인: `scenarios::foreign_and_orphan_open_orders_are_flagged`, `e2e::external_activity_is_detected_and_needs_an_operator`, `sim::profile_external`.

## 9. 과부하

| 상황 | 동작 |
|---|---|
| 한 전략이 빈도 한도를 넘는다 | 그 전략의 요청만 `overloaded`. 기록 없음 |
| 주문 큐가 가득 찼다 | `overloaded`. 기록 없음. 키도 쓰이지 않으므로 같은 키로 다시 낼 수 있다 |
| 신규 주문이 폭주하는 중의 취소 | 취소는 다른 큐와 다른 빈도 한도를 쓴다 |
| 거래소 통보가 밀린다 | 버리지 않는다. 게이트웨이가 기다린다 |
| 구독자가 이벤트를 읽지 않는다 | 그 구독을 끊는다. 전략은 재접속해서 조회로 맞춘다 |
| 전략 연결이 자리를 다 차지한다 | 운영 소켓은 자리를 따로 가진다. `hello` 없는 연결은 10초 뒤 닫힌다 |
| 한 연결이 구독을 여러 번 요청한다 | 구독은 연결당 하나다 |

확인: `e2e::overload_is_refused_at_the_door_and_cancels_still_work`, `scenarios::capacity_limits_bound_open_orders_and_outstanding_sends`, `queues::tests`, `review_node`의 연결·구독 시험 3건.
성능 측정: 기본 한도에서의 초당 2000건(초과분이 거절되는 것을 본다)과, 수용 한도를 푼 설정에서의 초당 2000건·8000건(파이프라인이 감당하는 양을 본다). 둘은 다른 설정이다([performance](performance/README.md)).

## 10. 운영자의 개입

| 명령 | 기록 | 확인 |
|---|---|---|
| 중지·재개 | `operator_halted`, `operator_resumed` | `scenarios::holds_are_independent` |
| 전체 취소 | 주문마다 `cancel_requested` | `scenarios::cancel_all_requests_every_open_order_in_scope` |
| 불일치 확인 | `mismatch_acked` | `scenarios::acknowledging_one_mismatch_does_not_clear_another` |
| 포지션 조정 | `position_adjusted` | `scenarios::position_adjustment_is_explicit_and_keeps_the_ledgers_consistent` |
| 설정 변경(재시작) | `config_activated` 또는 거부 | `scenarios::configuration_changes_are_validated_against_existing_exposure`, `e2e::configuration_changes_need_a_new_version` |

절차는 [operations/runbook.md](operations/runbook.md)에 있다.
