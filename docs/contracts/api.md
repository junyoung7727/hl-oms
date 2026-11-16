# C-API 로컬 API

설계 근거: [ADR-015](../adr/ADR-015-api-trust.md). 타입: `crates/oms-api/src/lib.rs`. 서버: `crates/oms-node/src/server.rs`. 명령줄 도구: `omsctl`.

## 전송과 틀

- Unix 도메인 소켓. 거래용 `<data_dir>/oms.sock`, 운영용 `<data_dir>/admin.sock`.
- 한 줄에 JSON 객체 하나, `\n`으로 끝난다. 한 줄 최대 64 KiB.
- 요청에는 `id`가 있고 응답이 같은 `id`를 돌려준다. 한 연결에서 여러 요청을 연달아 보낼 수 있고 응답 순서는 요청 순서와 다를 수 있다.
- 수량·가격·금액은 십진 문자열(`"0.001"`). JSON 숫자는 거부한다. 시각은 Unix epoch 기준 마이크로초 정수.
- 첫 요청은 `hello`여야 한다. 접속 후 10초(설정 `api.hello_timeout_ms`) 안에 `hello`가 없으면 서버가 연결을 닫는다.

## 요청

| op | 소켓 | 필드 | 응답의 `result` |
|---|---|---|---|
| `hello` | 둘 다 | `strategy`(거래 소켓에서는 필수), `client` | 인스턴스 ID, 부팅 번호, 내구 순번, 서버 시각, 버전 |
| `ping` | 둘 다 | — | `"pong"` |
| `new_order` | 거래 | `client_order_id`, `instrument`, `side`, `qty`, `px`, `tif`(`gtc`/`ioc`/`alo`), `reduce_only`, `expires_at` | `accepted` / `rejected` / `duplicate` / `conflict` |
| `cancel` | 둘 다 | `order_id` 또는 `client_order_id` | `requested` / `already_requested` / `already_closed` / `not_found` / `not_owner` |
| `get_order` | 둘 다 | `order_id` 또는 `client_order_id` | 주문 전체. 거래 소켓에서는 자기 전략의 것만 |
| `list_open` | 둘 다 | — | 미종결 주문 목록. 거래 소켓에서는 자기 전략의 것만 |
| `status` | 둘 다 | — | 운영 상태 문서. 거래 소켓에서는 계좌 전체의 차단 사유·결과 불명·불일치 목록과 자기 전략의 장부만(계좌 장부와 다른 전략의 장부는 빠진다) |
| `subscribe` | 둘 다 | — | 이후 주문 이벤트가 온다. 한 연결에 구독은 하나다. 다시 보내면 `already subscribed` |
| `halt`, `resume` | 운영 | `strategy`(없으면 계좌 전체), `note` | 문자열 |
| `cancel_all` | 운영 | `strategy`(없으면 전체), `note` | 요청한 주문 수 |
| `ack_mismatch` | 운영 | `mismatch_id`, `note` | 문자열 |
| `resolve_order` | 운영 | `order_id`, `note` | 문자열 |
| `adjust_position` | 운영 | `instrument`, `strategy`(없으면 미귀속), `delta`, `note` | 문자열 |
| `clear_reduce`, `rebase_day` | 운영 | `note` | 문자열. 거절이면 `bad_request`와 사유 코드로 시작하는 문장([C-MARGIN](margin.md)) |
| `snapshot` | 운영 | — | 문자열 |

예:

```json
{"id":7,"op":"new_order","client_order_id":"k1","instrument":"BTC","side":"buy","qty":"0.001","px":"70000","tif":"gtc","expires_at":1700000005000000}
{"id":7,"ok":true,"result":{"accepted":{"order_id":3}}}
```

## 응답의 의미

| 응답 | 뜻 | 기록 |
|---|---|---|
| `ok: true`, `accepted` | 주문이 저널에 내구화됐다. 거래소 전송은 그 뒤에 시작된다. 거래소가 받았다는 뜻이 아니다 | 있음 |
| `ok: true`, `rejected` | 코어가 검사해 거절했다. 사유 코드 목록이 온다 | 있음 |
| `ok: true`, `duplicate` | 같은 키의 같은 요청이 이미 있다. 현재 단계와 누적 체결량이 온다 | 없음 |
| `ok: true`, `conflict` | 같은 키의 다른 요청이 이미 있다 | 없음 |
| `ok: false`, `overloaded` | 큐가 찼거나 빈도 한도. 코어에 닿지 않았다 | 없음 |
| `ok: false`, `unavailable` | OMS가 멈추는 중이다. 요청이 기록됐는지 **알 수 없다**. 같은 키로 다시 보내면 된다 | 모름 |
| `ok: false`, `bad_request` | 형식 오류 | 없음 |
| `ok: false`, `unauthorized` | `hello` 전이거나 모르는 전략, 허용되지 않은 사용자 | 없음 |
| `ok: false`, `forbidden` | 이 소켓에서 허용되지 않는 동작 | 없음 |
| 응답이 오지 않음(연결 끊김) | 알 수 없다. 같은 키로 다시 보내면 된다 | 모름 |

거절 사유 코드: `unknown_strategy`, `strategy_disabled`, `unknown_instrument`, `instrument_not_allowed`, `qty_not_positive`, `qty_step`,
`px_not_positive`, `px_tick`, `px_sig_figs`, `below_min_notional`, `max_order_qty`, `max_order_notional`, `ref_price_unavailable`, `price_band`,
`account_position_limit`, `strategy_position_limit`, `max_open_orders_account`, `max_open_orders_strategy`, `gateway_busy`, `capacity_exhausted`,
`expired`, `expiry_too_far`, `overflow`, `{"hold": <사유>}`.

## 이벤트

`subscribe` 뒤에는 응답 사이에 다음 줄이 섞여 온다.

```json
{"seq":968,"at":1700000013371826,"order_id":195,"events":[{"venue_terminal":{...}},{"order_closed":{...}}]}
```

- `seq`는 저널 순번이다. 증가한다. 빈 번호는 다른 전략이나 다른 종류의 기록이다.
- 이벤트는 그 기록이 내구화된 뒤에 나간다.
- 전달은 최선 노력이다. 구독자의 버퍼(4096)가 차면 서버가 `overloaded` 오류를 한 줄 보내고 연결을 끊는다.
  끊기면 재접속해서 `list_open`과 `get_order`로 맞춘다. 과거 이벤트를 다시 받는 기능은 없다.

## 권한

| 주체 | 판별 | 할 수 있는 것 |
|---|---|---|
| 전략 | 거래 소켓에 접속한 프로세스(소유자 uid 또는 `allowed_uids`) + `hello`의 전략 이름 | 그 전략의 주문·취소·조회·구독 |
| 운영자 | 운영 소켓에 접속한 프로세스(소유자 uid) | 전부 |
| 열람자 | loopback HTTP | 읽기 |

## 운영 명령

모두 `Operator` 입력으로 코어에 들어가고 사유와 함께 저널에 남는다.

| 명령 | 이벤트 | 효과 | 주의 |
|---|---|---|---|
| `halt` | `operator_halted` | 그 범위의 신규 주문을 거절 | 이미 풀린 전송은 막지 못한다. 열린 주문은 그대로다 |
| `resume` | `operator_resumed` | 그 범위의 운영자 중지만 해제 | 다른 차단 사유는 남는다 |
| `cancel_all` | 주문마다 `cancel_requested` | 범위 안의 미종결 주문에 취소 요청 | 취소 완료가 아니다 |
| `ack_mismatch` | `mismatch_acked` | 그 불일치 하나를 확인 처리 | 원인이 남아 있으면 다시 제기된다 |
| `resolve_order` | `order_closed(manual)` | 주문을 종결하고 예약을 푼다 | 그 주문에 더 체결이 없다고 운영자가 단언하는 것이다. 틀리면 `FillAfterClose` |
| `adjust_position` | `position_adjusted` | 계좌와 지정 장부의 포지션을 증감 | 멱등이 아니다. 두 번 보내면 두 번 적용된다 |
| `clear_reduce` | `day_base_set(operator)`, `reducing_cleared` | 감소 전용 상태를 끝내고 현재 계좌 가치를 기준 금액으로 삼는다 | 신선한 증거금 보고와 사유가 필요하다. 그 뒤 5 %를 더 잃으면 다시 걸린다([C-MARGIN](margin.md)) |
| `rebase_day` | `day_base_set(operator)` | 현재 계좌 가치를 기준 금액으로 삼는다 | 감소 전용 상태는 그대로다. 입금 뒤에 하지 않으면 그만큼의 손실이 가려진다 |
| `snapshot` | 없음 | 다음 커밋에서 스냅샷 요청 | 기록되지 않는다 |

참조 가격(`ref_price_updated`)은 운영 명령이 아니라 게이트웨이의 입력이다.

## 연결별 동작

| 상황 | 동작 |
|---|---|
| 접속 수가 상한에 도달 | `unauthorized`(too many connections) 한 줄을 받고 닫힌다. 거래 소켓(64)과 운영 소켓(8)은 따로 센다 |
| `hello` 없이 10초 | `unauthorized` 한 줄을 받고 닫힌다 |
| 줄이 64 KiB를 넘음 | `bad_request` 후 연결 종료 |
| JSON이 아니거나 필드가 틀림 | `bad_request`. 연결은 유지 |
| 진행 중 요청이 창(32)에 도달 | 다음 줄을 읽지 않는다. 응답이 나가면 다시 읽는다 |
| 클라이언트가 응답을 읽지 않음 | 응답 버퍼가 차면 그 연결의 응답 전달이 멈춘다. 다른 연결과 코어는 영향 없다 |
| 연결이 끊김 | 큐에 들어간 요청은 처리된다. 응답은 버려진다. 구독 전달도 그때 끝나고 연결 자리가 반환된다 |
| OMS가 치명 상태 | 연결을 닫는다 |

## 검증

`api::tests::wire_examples`, `e2e::a_strategy_sees_and_cancels_only_its_own_orders`, `e2e::external_activity_is_detected_and_needs_an_operator`,
`e2e::vertical_slice_new_partial_fill_cancel_restart`(구독 순서), `e2e::overload_is_refused_at_the_door_and_cancels_still_work`.
연결과 구독의 상한: `review_node::idle_trading_connections_do_not_lock_the_operator_out`, `a_departed_subscriber_gives_its_connection_slot_back`, `one_connection_cannot_register_thousands_of_subscriptions`.
프로세스 수준에서 실행되는 운영 명령은 `ack_mismatch`, `cancel_all`, `halt`, `status`, `clear_reduce`, `rebase_day`다. 나머지(`resume`, `resolve_order`, `adjust_position`, `snapshot`)는 코어 시험으로만 확인했다. `omsctl`은 `status`, `clear-reduce`, `rebase-day`만 시험한다(`e2e::omsctl_clear_reduce_and_rebase_day_work_end_to_end`. 실행 파일이 부르는 함수 `oms_api::ctl::run`을 부른다).

## C-OPS 운영 명령의 질문들

| 질문 | 답 |
|---|---|
| 중간에 멈추면 무엇이 남는가 | 명령의 기록이 내구화됐으면 효과가 남고, 아니면 없다. 응답을 못 받았으면 `status`로 확인한다 |
| 같은 명령이 다시 오면 | `halt`: 다시 기록된다(사유가 갱신된다). `resume`: 중지 상태가 아니면 오류. `ack_mismatch`: 이미 확인됐으면 오류. `resolve_order`: 이미 닫혔으면 오류. `cancel_all`: 아직 요청되지 않은 주문과 시도가 다한 주문에만 요청. **`adjust_position`: 다시 적용된다. 멱등이 아니다** |
| 순서가 바뀌면 | 운영 명령은 control 큐에서 선착순이다. 신규 주문과는 큐가 달라 `halt`가 먼저 들어온 신규 주문보다 먼저 처리될 수 있다(그 주문은 거절된다) |
| 누가 언제 보는가 | 내구화 뒤 응답이 가고, 그 뒤의 입력부터 효과가 적용된다. `halt` 전에 이미 내보낸 전송은 막지 못한다 |
| 외부 결과를 아는가 | `cancel_all`의 응답은 요청한 주문 수다. 취소됐다는 뜻이 아니다. `resolve_order`는 거래소를 확인하지 않는다. 운영자의 말을 기록할 뿐이다 |
| 복구 뒤 무엇을 다시 하는가 | 운영 명령은 다시 실행되지 않는다. 중지와 확인 상태는 저널에서 복원된다 |

확인하지 않은 것: 다른 uid의 접속 거부, 줄 길이 상한, hello 시간 제한, 느린 구독자의 끊김, `unavailable` 응답 경로.
