# C-TAIL 저널 꼬리 읽기와 이벤트 필드 표

설계 근거: [ADR-025](../adr/ADR-025-journal-tail.md). 코드: `crates/oms-journal/src/tail.rs`(읽기), `crates/oms-core/src/event.rs`(이벤트). 이 문서는 OMS 밖의 프로그램(프로젝터 U14b, 감시자 U19a)이 저널을 읽는 규칙과, 읽은 기록을 해석하는 데 필요한 것을 모두 적는다. 감시자는 이 문서만 보고 구현한다(OMS 코드에 의존하지 않는다).

## C-TAIL 꼬리 읽기

| 항목 | 내용 |
|---|---|
| 입력 | 데이터 디렉터리, 이미 처리한 마지막 번호(커서), 내구화 번호 상한(OMS `/api/status`의 `journal.durable_seq`) |
| 출력 | 번호 순서대로 `(seq, record_schema, payload)`. payload는 아래 기록 형식의 JSON |
| 잠금 | `LOCK`을 잡지 않는다. 파일을 쓰지 않는다. OMS가 실행 중이어도, 꺼져 있어도 읽는다 |
| 상한 | 상한보다 큰 번호는 절대 돌려주지 않는다. 디스크에 더 있어도 그렇다. 상한을 넘는 프레임은 아직 쓰는 중이거나, 장애 뒤 복구가 잘라 내고 같은 번호에 다른 기록을 쓸 수 있다 |
| 세그먼트 | 다음 번호로 시작하는 세그먼트가 생기면 넘어간다. 기록 스키마가 바뀌면 새 세그먼트가 열린다([C-JOURNAL](recovery.md)). 헤더만 있는 세그먼트가 새 스키마 헤더로 다시 쓰이는 경우를 위해, 아직 프레임을 읽지 않은 세그먼트는 헤더를 다시 읽는다 |
| 손상 | 상한 이하의 프레임이 완전하지 않거나 체크섬이 틀리면 오류다. 그 앞의 기록은 먼저 돌려주고, 다음 호출이 오류를 낸다. 위치는 움직이지 않는다(같은 호출이 고칠 때까지 같은 오류) |
| 상한 너머 엿보기 | `peek_beyond`: 현재 위치 뒤의 유효한 프레임을 위치를 움직이지 않고 돌려준다. 완전하지 않거나 틀린 첫 프레임에서 멈춘다(오류가 아니다). OMS가 죽었다고 판단한 읽는 쪽만 쓰고, 보여 준 것을 미확정으로 표시한다 |
| 커서 | 읽는 쪽이 자기 커서를 따로 저장한다. 다시 시작하면 커서 다음 번호부터 읽는다. 커서는 처리를 마친 기록까지만 올린다(최소 한 번 전달, 중복은 `seq`로 거른다) |

## 기록 형식

payload 하나는 JSON 객체 하나다: `{"seq": u64, "at": u64 µs(기록 시각), "input": {…}, "events": [이벤트, …]}`.

- 복구는 `events`만 적용한다. `input`은 감사용이다. 읽는 쪽도 `events`만 해석하면 된다.
- 이벤트 하나는 `{"<이벤트 이름>": {필드…}}` 모양이다(필드 없는 이벤트는 없다).
- 수량·가격·금액은 소수 문자열이다(최대 소수점 아래 8자리, 지수 표기 없음). 숫자로 읽지 않는다.
- 열거형은 문자열(`"buy"`)이거나 한 키짜리 객체(`{"canceled": {"venue_reason": "…"}}`)다.
- 값이 기본값인 일부 필드는 쓰지 않는다(아래 표에 "쓰지 않음"으로 표시). 없으면 기본값으로 읽는다.
- 기록 스키마 1과 2를 읽는다. 2는 1에 이벤트 종류를 더한 것이다([ADR-023](../adr/ADR-023-venue-made-executions.md)). 모르는 이벤트 이름이 나오면 읽기를 멈춘다(건너뛰지 않는다).

## 이벤트가 바꾸는 상태

감시자가 만드는 세 집합: **열린 주문**(주문 번호 → 종목, 방향, 수량, 거래소 주문 번호), **체결**(체결 키 → 수량, 방향, 종목), **포지션**(종목 → 부호 있는 수량).

| 이벤트 | 열린 주문 | 체결 | 포지션 |
|---|---|---|---|
| `order_accepted` | 추가: `order_id`, `request.strategy`, `request.instrument`, `request.side`, `request.qty`, `request.client_order_id` | — | — |
| `order_acked` | `venue_oid`가 있으면 그 주문의 거래소 주문 번호 | — | — |
| `venue_oid_learned` | 그 주문의 거래소 주문 번호 | — | — |
| `fill_booked` | 그 주문의 누적 체결 += `qty` | 추가: `key` → `qty`, 방향은 그 주문의 `request.side`, 종목은 그 주문의 `request.instrument` | 그 종목 += 부호 있는 `qty`(매수 +, 매도 −) |
| `external_fill_booked` | — | 추가: `fill.key` → `fill.qty`, `fill.side`, `fill.instrument` | `fill.instrument` += 부호 있는 `fill.qty` |
| `order_closed` | 제거 | — | — |
| `venue_position_changed` | — | — (체결이 아니다. 감시자 W4는 거래소의 위치 변경 행과 비교한다) | `change.instrument` += `change.qty`(이미 부호 있음) |
| `position_adjusted` | — | — | `instrument` += `delta`(부호 있음) |
| `funding_booked` | — | — (현금만. 수량을 바꾸지 않는다) | — |
| `purged` | 닫힌 주문만 지운다. 열린 주문은 바뀌지 않는다 | `fill_floor`보다 이른 체결 키는 잊어도 된다 | — |
| 그 밖의 이벤트 | — | — | — |

그 밖의 이벤트(링크, 결과 불명, 상태 기록, 취소, 대조, 불일치, 운영자, 저장 공간, 참조 가격, 증거금, 펀딩·변경 대기, 부재 기록)는 세 집합을 바꾸지 않는다. 상태 화면과 알림은 이들을 쓴다. 뜻은 아래 필드 표와 각 계약 문서([C-NEW 등](order-lifecycle.md), [C-MISMATCH](reconciliation.md), [C-MARGIN](margin.md))에 있다.

포지션 = `fill_booked` + `external_fill_booked` + `venue_position_changed` + `position_adjusted` 수량의 합이다(감시자 W2, ADR-023).

## 이벤트 필드 표

시험 `event_fields::the_field_table_lists_every_field_the_encoder_writes`가 인코더가 쓰는 모든 필드가 이 표에 있는지, `every_row_of_the_table_is_a_field_the_encoder_writes`가 표의 모든 행이 실제로 쓰이는지 본다. 타입이 `enum`이나 `object`로 시작하는 행은 그 값을 통째로 읽는다(안을 표에 따로 적지 않는다). `list[]`는 목록의 각 항목이다.

<!-- event-fields:begin -->
| 이벤트 | 필드 | 타입 | 뜻 |
|---|---|---|---|
| `config_activated` | `config` | object CoreConfig | 활성화된 설정 전체(종목, 전략, 정책). C-OPS의 설정 표 |
| `booted` | `binary` | string | 바이너리 식별 |
| `booted` | `boot` | u32 | 부팅 번호(1부터) |
| `booted` | `fill_floor` | u64 µs | 이 거래소 시각 이전 체결은 이 인스턴스와 무관 |
| `link_changed` | `reason` | string | 사유 문장 |
| `link_changed` | `session` | u64 | 링크 세션 번호 |
| `link_changed` | `up` | bool | 링크가 올라왔는가 |
| `request_rejected` | `reasons` | enum list RejectReason | 거절 사유 코드들(ADR-008 §7) |
| `request_rejected` | `request.client_order_id` | string | 요청의 전략이 정한 멱등 키 |
| `request_rejected` | `request.expires_at` | u64 µs | 요청의 이 시각 뒤에는 보내지 않음 |
| `request_rejected` | `request.instrument` | string | 요청의 종목 |
| `request_rejected` | `request.market_ref.channel` | string | 요청의 시세 참조: 채널(orderbook.50 등) |
| `request_rejected` | `request.market_ref.market` | string | 요청의 시세 참조: 시장(linear 등) |
| `request_rejected` | `request.market_ref.recv_ns` | i64 ns | 요청의 시세 참조: 수신 시각 |
| `request_rejected` | `request.market_ref.symbol` | string | 요청의 시세 참조: 종목 |
| `request_rejected` | `request.market_ref.update_id` | i64 또는 null | 요청의 시세 참조: 마지막 적용 메시지의 update_id |
| `request_rejected` | `request.market_ref.venue` | string | 요청의 시세 참조: 거래소 |
| `request_rejected` | `request.market_ref.venue_ts_ns` | i64 ns | 요청의 시세 참조: 거래소 시각 |
| `request_rejected` | `request.px` | decimal string | 요청의 가격 |
| `request_rejected` | `request.qty` | decimal string | 요청의 수량(0 이상) |
| `request_rejected` | `request.reduce_only` | bool | 요청의 거래소 감소 전용 표시 |
| `request_rejected` | `request.side` | enum buy/sell | 요청의 방향 |
| `request_rejected` | `request.strategy` | string | 요청의 전략 |
| `request_rejected` | `request.tif` | enum gtc/ioc/alo | 요청의 유효 조건 |
| `order_accepted` | `config_version` | u32 | 판단에 쓴 설정 버전 |
| `order_accepted` | `order_id` | u64 | 주문 번호(코어가 1부터 매김) |
| `order_accepted` | `ref_px` | decimal string 또는 null | 수용 때의 참조 가격 |
| `order_accepted` | `request.client_order_id` | string | 요청의 전략이 정한 멱등 키 |
| `order_accepted` | `request.expires_at` | u64 µs | 요청의 이 시각 뒤에는 보내지 않음 |
| `order_accepted` | `request.instrument` | string | 요청의 종목 |
| `order_accepted` | `request.market_ref.channel` | string | 요청의 시세 참조: 채널(orderbook.50 등) |
| `order_accepted` | `request.market_ref.market` | string | 요청의 시세 참조: 시장(linear 등) |
| `order_accepted` | `request.market_ref.recv_ns` | i64 ns | 요청의 시세 참조: 수신 시각 |
| `order_accepted` | `request.market_ref.symbol` | string | 요청의 시세 참조: 종목 |
| `order_accepted` | `request.market_ref.update_id` | i64 또는 null | 요청의 시세 참조: 마지막 적용 메시지의 update_id |
| `order_accepted` | `request.market_ref.venue` | string | 요청의 시세 참조: 거래소 |
| `order_accepted` | `request.market_ref.venue_ts_ns` | i64 ns | 요청의 시세 참조: 거래소 시각 |
| `order_accepted` | `request.px` | decimal string | 요청의 가격 |
| `order_accepted` | `request.qty` | decimal string | 요청의 수량(0 이상) |
| `order_accepted` | `request.reduce_only` | bool | 요청의 거래소 감소 전용 표시 |
| `order_accepted` | `request.side` | enum buy/sell | 요청의 방향 |
| `order_accepted` | `request.strategy` | string | 요청의 전략 |
| `order_accepted` | `request.tif` | enum gtc/ioc/alo | 요청의 유효 조건 |
| `order_acked` | `order_id` | u64 | 주문 번호(코어가 1부터 매김) |
| `order_acked` | `session` | u64 | 증거를 모은 링크 세션, 옛 증거면 0 |
| `order_acked` | `venue_oid` | string 또는 null | 거래소 주문 번호 |
| `order_acked` | `via` | enum EvidenceSource | 증거의 출처: send_result/stream/query/fill |
| `venue_oid_learned` | `order_id` | u64 | 주문 번호(코어가 1부터 매김) |
| `venue_oid_learned` | `venue_oid` | string | 새로 알게 된 거래소 주문 번호 |
| `order_in_doubt` | `detail` | string | 설명 문장 |
| `order_in_doubt` | `order_id` | u64 | 주문 번호(코어가 1부터 매김) |
| `order_in_doubt` | `reason` | enum DoubtReason | transport/restart/no_result |
| `status_noted` | `order_id` | u64 | 주문 번호(코어가 1부터 매김) |
| `status_noted` | `reported_cum` | decimal string | 거래소가 말한 누적 체결 수량의 최댓값 |
| `status_noted` | `session` | u64 | 링크 세션 번호 |
| `status_noted` | `venue_ts` | u64 µs | 거래소 시각 |
| `order_recheck` | `order_id` | u64 | 주문 번호(코어가 1부터 매김) |
| `fill_booked` | `fee` | decimal string | 수수료, 양수는 비용 |
| `fill_booked` | `key` | string | 체결 키(게이트웨이가 정함, C-GW G-8) |
| `fill_booked` | `order_id` | u64 | 주문 번호(코어가 1부터 매김) |
| `fill_booked` | `px` | decimal string | 가격 |
| `fill_booked` | `qty` | decimal string | 수량(0 이상) |
| `fill_booked` | `venue_ts` | u64 µs | 거래소 시각 |
| `fill_parked` | `fill.fee` | decimal string | 체결 통보의 수수료, 양수는 비용 |
| `fill_parked` | `fill.instrument` | string | 체결 통보의 종목 |
| `fill_parked` | `fill.key` | string | 체결 통보의 체결 키(게이트웨이가 정함, C-GW G-8) |
| `fill_parked` | `fill.order_id` | u64 또는 null | 게이트웨이가 클라이언트 ID에서 복원한 주문 번호(없으면 null) |
| `fill_parked` | `fill.px` | decimal string | 체결 통보의 가격 |
| `fill_parked` | `fill.qty` | decimal string | 체결 통보의 수량(0 이상) |
| `fill_parked` | `fill.side` | enum buy/sell | 체결 통보의 방향 |
| `fill_parked` | `fill.venue_oid` | string 또는 null | 체결 통보의 거래소 주문 번호 |
| `fill_parked` | `fill.venue_ts` | u64 µs | 체결 통보의 거래소 시각 |
| `fill_unparked` | `key` | string | 체결 키(게이트웨이가 정함, C-GW G-8) |
| `external_fill_booked` | `fill.fee` | decimal string | 체결 통보의 수수료, 양수는 비용 |
| `external_fill_booked` | `fill.instrument` | string | 체결 통보의 종목 |
| `external_fill_booked` | `fill.key` | string | 체결 통보의 체결 키(게이트웨이가 정함, C-GW G-8) |
| `external_fill_booked` | `fill.order_id` | u64 또는 null | 게이트웨이가 클라이언트 ID에서 복원한 주문 번호(없으면 null) |
| `external_fill_booked` | `fill.px` | decimal string | 체결 통보의 가격 |
| `external_fill_booked` | `fill.qty` | decimal string | 체결 통보의 수량(0 이상) |
| `external_fill_booked` | `fill.side` | enum buy/sell | 체결 통보의 방향 |
| `external_fill_booked` | `fill.venue_oid` | string 또는 null | 체결 통보의 거래소 주문 번호 |
| `external_fill_booked` | `fill.venue_ts` | u64 µs | 체결 통보의 거래소 시각 |
| `venue_terminal` | `final_cum` | decimal string 또는 null | 최종 누적 체결 수량 |
| `venue_terminal` | `kind` | enum VenueFinal | filled/canceled/rejected |
| `venue_terminal` | `order_id` | u64 | 주문 번호(코어가 1부터 매김) |
| `venue_terminal` | `reason` | string | 사유 문장 |
| `venue_terminal` | `venue_ts` | u64 µs | 거래소 시각 |
| `missing_fills_flagged` | `order_id` | u64 | 주문 번호(코어가 1부터 매김) |
| `order_closed` | `order_id` | u64 | 주문 번호(코어가 1부터 매김) |
| `order_closed` | `reason` | enum CloseReason | 종결 사유: filled, canceled{venue_reason}, rejected_by_venue{reason}, not_sent{reason}, not_placed{evidence}, manual{note} |
| `cancel_requested` | `by` | enum Actor | 요청자: strategy(이름)/operator/rule(규칙) |
| `cancel_requested` | `order_id` | u64 | 주문 번호(코어가 1부터 매김) |
| `cancel_sent` | `attempt` | u32 | 취소 시도 번호(1부터) |
| `cancel_sent` | `order_id` | u64 | 주문 번호(코어가 1부터 매김) |
| `cancel_outcome_noted` | `attempt` | u32 | 취소 시도 번호(1부터) |
| `cancel_outcome_noted` | `order_id` | u64 | 주문 번호(코어가 1부터 매김) |
| `cancel_outcome_noted` | `outcome` | enum CancelOutcome | confirmed, rejected{reason}, unknown{reason}, not_sent{reason}, lost_on_restart |
| `fills_synced` | `session` | u64 | 링크 세션 번호 |
| `fills_synced` | `up_to` | u64 µs | 이 거래소 시각까지의 체결은 모두 전달됨 |
| `open_orders_swept` | `session` | u64 | 링크 세션 번호 |
| `positions_verified` | `session` | u64 | 링크 세션 번호 |
| `mismatch_raised` | `detail` | string | 설명 문장 |
| `mismatch_raised` | `id` | u32 | 불일치 번호 |
| `mismatch_raised` | `kind` | enum MismatchKind | 불일치 종류(C-MISMATCH 표) |
| `mismatch_raised` | `subject` | string | 대상: order:N, instrument:X 등. 같은 종류·대상은 확인 전 한 건 |
| `mismatch_acked` | `id` | u32 | 불일치 번호 |
| `mismatch_acked` | `note` | string | 운영자가 적은 사유 |
| `operator_halted` | `note` | string | 운영자가 적은 사유 |
| `operator_halted` | `scope` | enum Scope | account 또는 strategy(이름) |
| `operator_resumed` | `note` | string | 운영자가 적은 사유 |
| `operator_resumed` | `scope` | enum Scope | account 또는 strategy(이름) |
| `storage_low_changed` | `free_bytes` | u64 | 측정한 남은 바이트 |
| `storage_low_changed` | `low` | bool | 저장 공간 부족 상태 |
| `ref_price_updated` | `instrument` | string | 종목 |
| `ref_price_updated` | `px` | decimal string | 가격 |
| `ref_price_updated` | `venue_ts` | u64 µs | 거래소 시각 |
| `position_adjusted` | `delta` | decimal string, 부호 있음 | 포지션 증감 |
| `position_adjusted` | `instrument` | string | 종목 |
| `position_adjusted` | `ledger` | enum Ledger | unattributed 또는 strategy(이름) |
| `position_adjusted` | `note` | string | 사유: 운영자가 적은 것, 또는 코어가 거래소 포지션으로 맞춘 근거 |
| `position_adjusted` | `reason` | enum AdjustReason | operator(기본, 쓰지 않음) 또는 snapshot_after_venue_change |
| `purged` | `cutoff` | u64 µs | 이보다 오래된 확인된 불일치도 지움 |
| `purged` | `fill_floor` | u64 µs | 새 체결 바닥 시각. 이보다 이른 체결 키는 잊음 |
| `purged` | `order_ids` | u64 list | 메모리에서 지운 닫힌 주문 |
| `margin_reported` | `report.account_value` | decimal string | 증거금 보고의 계좌 가치 |
| `margin_reported` | `report.positions[].instrument` | string | 보고된 포지션의 종목 |
| `margin_reported` | `report.positions[].leverage_type` | enum isolated/cross | 보고된 포지션의 증거금 방식 |
| `margin_reported` | `report.positions[].leverage_value` | u32 | 보고된 포지션의 레버리지 |
| `margin_reported` | `report.positions[].margin_used` | decimal string | 보고된 포지션의 사용 증거금 |
| `margin_reported` | `report.positions[].max_leverage` | u32 | 보고된 포지션의 거래소 최대 레버리지 |
| `margin_reported` | `report.positions[].position_value` | decimal string | 보고된 포지션의 포지션 가치 |
| `margin_reported` | `report.positions[].szi` | decimal string, 부호 있음 | 보고된 포지션의 포지션 수량(키 이름은 처음 쓰인 그대로) |
| `margin_reported` | `report.queried_at` | u64 µs | 증거금 조회를 보내기 직전의 노드 시각 |
| `margin_reported` | `report.seq` | u64 | 증거금 보고의 증거금 조회 번호(세션 안에서 증가) |
| `margin_reported` | `report.session` | u64 | 증거금 보고의 링크 세션 번호 |
| `margin_report_rejected` | `queried_at` | u64 µs | 조회를 보내기 직전의 노드 시각 |
| `margin_report_rejected` | `reason` | string | 사유 문장 |
| `margin_report_rejected` | `seq` | u64 | 증거금 조회 번호(세션 안에서 증가) |
| `margin_report_rejected` | `session` | u64 | 링크 세션 번호 |
| `day_base_set` | `day` | u64 | UTC 일 번호 |
| `day_base_set` | `set_by` | enum BaseSetBy | report/operator |
| `day_base_set` | `value` | decimal string | 일 기준 계좌 가치 |
| `reducing_entered` | `detail` | string | 설명 문장 |
| `reducing_entered` | `rule` | enum RuleId | daily_loss/mm_buffer/margin_stale |
| `venue_setting_noted` | `instrument` | string | 종목 |
| `venue_setting_noted` | `isolated` | bool | 격리 마진인가 |
| `venue_setting_noted` | `leverage` | u32 | 레버리지 |
| `venue_setting_noted` | `session` | u64 | 링크 세션 번호 |
| `account_mode_noted` | `mode` | string | 거래소가 부르는 계좌 모드 이름 |
| `account_mode_noted` | `session` | u64 | 링크 세션 번호 |
| `reducing_cleared` | `new_base` | decimal string | 새 일 기준 |
| `reducing_cleared` | `note` | string | 운영자가 적은 사유 |
| `reducing_cleared` | `old_base` | decimal string 또는 null | 판정에 쓴 옛 일 기준 |
| `funding_booked` | `funding.amount` | decimal string, 부호 있음 | 펀딩의 금액, 양수는 계좌가 냄 |
| `funding_booked` | `funding.instrument` | string | 펀딩의 종목 |
| `funding_booked` | `funding.key` | string | 펀딩의 체결 키(게이트웨이가 정함, C-GW G-8) |
| `funding_booked` | `funding.rate` | string | 거래소가 쓴 펀딩 비율(감사용) |
| `funding_booked` | `funding.venue_ts` | u64 µs | 펀딩의 거래소 시각 |
| `venue_position_changed` | `change.instrument` | string | 거래소 변경의 종목 |
| `venue_position_changed` | `change.key` | string | 거래소 변경의 체결 키(게이트웨이가 정함, C-GW G-8) |
| `venue_position_changed` | `change.kind` | enum VenueChangeKind | 변경 종류: adl/takeover/delivery/settle |
| `venue_position_changed` | `change.px` | decimal string | 거래소 변경의 가격 |
| `venue_position_changed` | `change.qty` | decimal string, 부호 있음 | 거래소가 옮긴 수량, 양수는 롱에 더함 |
| `venue_position_changed` | `change.venue_ts` | u64 µs | 거래소 변경의 거래소 시각 |
| `unclassified_execution_noted` | `detail` | string | 설명 문장 |
| `unclassified_execution_noted` | `exec_type` | string | 거래소의 실행 종류 이름 |
| `unclassified_execution_noted` | `instrument` | string | 종목 |
| `unclassified_execution_noted` | `key` | string | 체결 키(게이트웨이가 정함, C-GW G-8) |
| `unclassified_execution_noted` | `venue_ts` | u64 µs | 거래소 시각 |
| `venue_change_settled` | `instrument` | string | 종목 |
| `absence_noted` | `order_id` | u64 | 주문 번호(코어가 1부터 매김) |
| `absence_noted` | `queried_at` | u64 µs | 조회를 보내기 직전의 노드 시각 |
<!-- event-fields:end -->

쓰지 않는 기본값: `position_adjusted.reason`은 `operator`일 때 쓰지 않는다. `order_accepted.request.market_ref`, `request_rejected.request.market_ref`는 없을 때 쓰지 않는다. `order_acked.session`은 옛 기록에 없을 수 있다(0).

## 검증

`tail::the_tail_never_returns_a_record_above_the_durable_cap`, `tail::the_tail_follows_segment_rotation_and_starts_after_a_cursor`, `tail::the_tail_never_takes_the_lock`, `tail::a_damaged_durable_frame_stops_the_tail_without_moving_it`, `tail::peek_beyond_shows_valid_frames_past_the_cap_without_moving`(주장 V-TAIL-1). 필드 표: 위 두 시험(주장 V-TAIL-2).
