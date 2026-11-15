# C-IDEM 멱등성

설계 근거: [ADR-006](../adr/ADR-006-idempotency.md). 구현: `Tx::new_order`(`crates/oms-core/src/decide.rs`), `Event::Purged`의 적용(`core.rs`).

## 계약

| 항목 | 내용 |
|---|---|
| 키 | `(strategy, client_order_id)`. 범위는 한 데이터 디렉터리 |
| 보장 | 한 키로 만들어지는 주문은 최대 하나. 그 주문의 거래소 전송 명령은 최대 한 번 |
| 읽는 상태 | 색인 `by_client`와 그 주문 |
| 기억하는 기간 | 주문이 메모리에 있는 동안. 제거 조건: 종결 + 보존 시간(1시간) 경과 **그리고** `expires_at + dedup_grace`(60초) 경과 |
| 자원 상한 | 메모리의 주문 수 상한(100,000). 넘으면 신규 주문을 `capacity_exhausted`로 거절한다. 종결 주문은 보존 시간(1시간) 동안 남으므로, 기본값에서 초당 약 27.8건을 넘는 수용이 한 시간 이어지면 상한에 닿는다. 거래소의 IP 한도(초당 20건)에서는 시간당 72,000건이라 닿지 않는다 |

## 요청별 답

| 요청 | 조건 | 답 | 상태 변화 | 기록 |
|---|---|---|---|---|
| 처음 보는 키 | 검사 통과 | `accepted { order_id }` | 주문 생성 | `OrderAccepted` |
| 처음 보는 키 | 검사 실패 | `rejected { reasons }` | 없음(통계만). **키는 소모되지 않는다** | `RequestRejected` |
| 있는 키, 같은 요청 | — | `duplicate { order_id, phase, cum_qty }` | 없음 | 없음 |
| 있는 키, 다른 요청 | — | `conflict { order_id }` | 없음 | 없음 |
| 제거된 키 | 만료 시각이 지났다 | `rejected { [expired] }` | 없음 | `RequestRejected` |

"같은 요청"은 종목·방향·수량·가격·TIF·reduce_only·`expires_at`이 모두 같은 것이다. 주문의 필드와 직접 비교한다.

## 왜 삭제 뒤에도 안전한가

키가 제거되는 시점은 `expires_at + dedup_grace` 이후다. 그 뒤에 도착한 같은 요청은 `expires_at`이 과거이므로 `expired`로 거절된다.
다른 `expires_at`을 가진 요청은 "다른 요청"이지만 키가 이미 없으므로 새 요청으로 평가된다.
이것은 전략이 같은 키를 새 의도에 다시 쓴 경우이며 전략의 책임이다.

## 변형 입력

| 상황 | 동작 |
|---|---|
| 응답을 못 받고 다시 보냄(같은 키, 같은 내용) | 주문이 내구화됐으면 `duplicate`. 내구화 전에 OMS가 죽었으면 새로 평가(주문은 여전히 하나) |
| 거절 응답을 못 받고 다시 보냄 | 새로 평가. 그 사이 상황이 바뀌었으면 수용될 수 있다. 만료 시각이 창을 묶는다 |
| 여러 연결이 같은 요청을 동시에 | 코어가 한 줄로 처리한다. 첫 번째만 `accepted`, 나머지는 `duplicate` |
| `overloaded`로 거절된 뒤 다시 보냄 | 코어에 닿지 않았으므로 새 요청과 같다 |
| OMS 재시작 뒤 다시 보냄 | 색인이 저널에서 복원된다. 같은 답 |
| 데이터 디렉터리를 새로 만든 뒤 | 기억이 없다. 새 주문이 된다. `instance_id`가 달라 거래소에서는 구별된다 |

취소는 대상 지정 방식이라 본래 멱등이다. 두 번째 취소는 `already_requested`다.
운영 명령(중지, 재개, 확인)은 상태에 따라 "이미 그 상태"로 답하거나 다시 기록한다. 포지션 조정(`adjust`)은 **멱등이 아니다.** 두 번 보내면 두 번 적용된다.

## 검증

`scenarios::same_key_same_request_creates_one_order_and_one_send`, `same_key_different_request_is_a_conflict_without_effect`,
`the_key_scope_is_per_strategy`, `a_rejection_does_not_consume_the_key`, `the_duplicate_answer_survives_restart`,
`a_key_is_forgotten_only_after_its_request_expired`, `expiry_rules`.
시뮬레이션의 `one-order-per-key`, `one-send-per-key` 검사(전 프로파일). 프로세스 `e2e::a_repeated_request_never_creates_a_second_order`.

확인하지 않은 것: 전략의 시계가 OMS와 다른 경우(같은 호스트라는 전제 밖).
