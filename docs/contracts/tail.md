# C-TAIL 저널 꼬리 읽기와 이벤트 필드 표

설계 근거: [ADR-025](../adr/ADR-025-journal-tail.md). 코드: `crates/oms-journal/src/tail.rs`(읽기), `crates/oms-core/src/event.rs`(이벤트). 이 문서는 OMS 밖의 프로그램(프로젝터 U14b, 감시자 U19a)이 저널을 읽는 규칙과, 읽은 기록을 해석하는 데 필요한 것을 모두 적는다. 감시자는 이 문서만 보고 구현한다(OMS 코드에 의존하지 않는다). 감시자의 구현: `crates/watcher/src/journal.rs`(읽기), `crates/watcher/src/events.rs`(아는 이벤트), C-WATCH 1부(U19a1). 아래 "이벤트가 바꾸는 상태"의 감시자 구현: `crates/watcher/src/state.rs`, C-WATCH 2부(U19a2).

## C-TAIL 꼬리 읽기

| 항목 | 내용 |
|---|---|
| 입력 | 데이터 디렉터리, 이미 처리한 마지막 번호(커서), 내구화 번호 상한(OMS `/api/status`의 `journal.durable_seq`) |
| 출력 | 번호 순서대로 `(seq, record_schema, payload)`. payload는 아래 기록 형식의 JSON |
| 잠금 | `LOCK`을 잡지 않는다. 파일을 쓰지 않는다. OMS가 실행 중이어도, 꺼져 있어도 읽는다 |
| 상한 | 상한보다 큰 번호는 절대 돌려주지 않는다. 디스크에 더 있어도 그렇다. 상한을 넘는 프레임은 아직 쓰는 중이거나, 장애 뒤 복구가 잘라 내고 같은 번호에 다른 기록을 쓸 수 있다 |
| 세그먼트 | 다음 번호로 시작하는 세그먼트가 생기면 넘어간다. 기록 스키마가 바뀌면 새 세그먼트가 열린다(C-JOURNAL). 헤더만 있는 세그먼트가 새 스키마 헤더로 다시 쓰이는 경우를 위해, 아직 프레임을 읽지 않은 세그먼트는 헤더를 다시 읽는다 |
| 손상 | 상한 이하의 프레임이 완전하지 않거나, 체크섬이 틀리거나, 번호가 이어지지 않으면 오류다. 커서 앞의 기록이 손상돼도(뒤에 유효한 프레임이 있으면) 오류다. 그 앞의 기록은 먼저 돌려주고, 다음 호출이 오류를 낸다. 위치는 움직이지 않는다(같은 호출이 고칠 때까지 같은 오류) |
| 상한 너머 엿보기 | `peek_beyond`: 현재 위치 뒤의 유효한 프레임을 위치를 움직이지 않고 돌려준다. 완전하지 않거나 틀린 첫 프레임에서 멈춘다(오류가 아니다). OMS가 죽었다고 판단한 읽는 쪽만 쓰고, 보여 준 것을 미확정으로 표시한다 |
| 내구화 번호의 근거 | OMS는 시작할 때 복구가 읽은 마지막 세그먼트를 `fsync`한 뒤에 그 번호를 내구화 번호로 낸다(검토 2026-11-20 전에는 하지 않아, 강제 종료 뒤 페이지 캐시에만 있던 기록이 내구화로 보일 수 있었다) |
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

감시자가 만드는 것: **주문 정보**(주문 번호 → 전략, 종목, 방향, 수량, 클라이언트 주문 ID, 거래소 주문 번호, 누적 체결, 확인·결과 불명·거래소 종결 표시; `order_accepted`부터 `purged`까지 유지), 그중 **열린 주문**(아직 `order_closed`가 없는 것), **키**(체결 키 → 수량, 방향, 종목, 거래소 시각; 거래소 변경 키; 펀딩 키. OMS는 셋을 한 집합으로 두고 같은 기록에서 잊는다), **포지션**(종목 → 부호 있는 수량), **그 밖의 상태**(체결 바닥, 킬 보류와 워처 불일치 보류, 일 기준값). 아래 표의 모든 행이 감시자의 규칙이다(C-WATCH 2부, U19a2 검토 1 M1에서 확인·결과 불명·종결 표시, 거래소 변경·펀딩 키, 체결 바닥, 보류, 일 기준값의 행을 더했다).

닫힌 주문의 정보를 `order_closed`에서 지우면 안 된다. 닫힌 뒤에 그 주문의 `fill_booked`가 올 수 있고(늦은 체결, 불일치 `FillAfterClose`), 그 체결의 방향과 종목은 주문 정보에서만 알 수 있다(검토 2026-11-20).

| 이벤트 | 주문 정보·열린 주문 | 체결·거래소 변경·펀딩 키 | 포지션 | 그 밖의 상태 |
|---|---|---|---|---|
| `order_accepted` | 주문 정보와 열린 주문에 추가: `order_id`, `request.strategy`, `request.instrument`, `request.side`, `request.qty`, `request.client_order_id`. 거래소 주문 번호 없음, 누적 체결 0, 확인·결과 불명·거래소 종결 표시는 아님 | — | — | — |
| `order_acked` | 확인됨. 결과 불명이 풀린다. `venue_oid`가 null이 아니면 그 주문의 거래소 주문 번호(null이면 아는 번호를 남긴다) | — | — | — |
| `venue_oid_learned` | 그 주문의 거래소 주문 번호 | — | — | — |
| `order_in_doubt` | 결과 불명 | — | — | — |
| `venue_terminal` | 거래소 종결 표시. 결과 불명이 풀린다. 열린 주문에 남는다(`order_closed`까지, C-TERM) | — | — | — |
| `fill_booked` | 그 주문의 누적 체결 += `qty`(닫힌 주문일 수도 있다) | 추가: `key` → `qty`, `venue_ts`, 방향과 종목은 주문 정보의 `request.side`, `request.instrument` | 그 종목 += 부호 있는 `qty`(매수 +, 매도 −) | — |
| `external_fill_booked` | — | 추가: `fill.key` → `fill.qty`, `fill.side`, `fill.instrument`, `fill.venue_ts` | `fill.instrument` += 부호 있는 `fill.qty` | — |
| `order_closed` | 열린 주문에서 제거. 주문 정보는 남긴다. 거래소 종결 표시는 바꾸지 않는다 | — | — | — |
| `venue_position_changed` | — | 거래소 변경 키 추가: `change.key` → `change.instrument`, `change.qty`, `change.kind`, `change.venue_ts`(체결이 아니다. 감시자 W4는 거래소의 위치 변경 행과 비교한다) | `change.instrument` += `change.qty`(이미 부호 있음) | — |
| `position_adjusted` | — | — | `instrument` += `delta`(부호 있음) | — |
| `funding_booked` | — | 펀딩 키 추가: `funding.key` → `funding.instrument`, `funding.venue_ts`(현금만. 수량을 바꾸지 않는다) | — | — |
| `purged` | `order_ids`의 주문 정보를 지운다(모두 닫힌 주문이다. 열린 주문이나 모르는 주문이면 저널이 이 표와 다르다). 열린 주문은 바뀌지 않는다 | 바닥을 올린 뒤, 세 가지 키 중 거래소 시각 < 체결 바닥인 것을 잊는다(바닥과 같은 시각은 남긴다) | — | 체결 바닥 = max(옛 값, `fill_floor`) |
| `booted` | — | 잊지 않는다(키는 `purged`에서만 잊는다) | — | 체결 바닥 = max(옛 값, `fill_floor`) |
| `hold_set` / `hold_cleared` | — | — | — | `hold`가 `kill`이면 킬 보류를, `watcher_mismatch`면 워처 불일치 보류를 건다(`hold_set`) 또는 푼다(`hold_cleared`). `hold_cleared`의 `venue_change`는 이 둘을 바꾸지 않는다(그 보류는 거래소 변경 이벤트에서 생긴다) |
| `day_base_set` | — | — | — | 일 기준값 = `{day, value}` |
| 그 밖의 이벤트 | — | — | — | — |

그 밖의 이벤트(링크, 상태 기록, 취소, 대조, 불일치, 운영자, 저장 공간, 참조 가격, 증거금, 펀딩·변경 대기, 부재 기록, 킬 루프 `kill_loop_ended`·`kill_rearmed`, 전략 연결 `strategy_link_lost`·`strategy_link_restored`, 모니터링 게이트 `watcher_heartbeat`·`monitor_changed`)는 위의 상태를 바꾸지 않는다. 상태 화면과 알림은 이들을 쓴다. 뜻은 아래 필드 표와 각 계약 문서(C-NEW 등, C-MISMATCH, C-MARGIN, C-HOLD, C-LIVE)에 있다. 감시자가 이 표에서 더 구하는 시각(주문의 `fact_at`, 종목의 `last_change_at`)은 C-WATCH 2부에 있다.

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
| `fill_booked` | `is_maker` | bool | 거래소의 maker 표시(Bybit `isMaker`). 게이트웨이가 모르면 쓰지 않음 |
| `fill_booked` | `key` | string | 체결 키(게이트웨이가 정함, C-GW G-8) |
| `fill_booked` | `order_id` | u64 | 주문 번호(코어가 1부터 매김) |
| `fill_booked` | `px` | decimal string | 가격 |
| `fill_booked` | `qty` | decimal string | 수량(0 이상) |
| `fill_booked` | `recv_ns` | i64 ns | 게이트웨이가 체결 통보를 받은 시각. 모르면 쓰지 않음 |
| `fill_booked` | `venue_ts` | u64 µs | 거래소 시각 |
| `fill_parked` | `fill.fee` | decimal string | 체결 통보의 수수료, 양수는 비용 |
| `fill_parked` | `fill.instrument` | string | 체결 통보의 종목 |
| `fill_parked` | `fill.is_maker` | bool | 체결 통보의 maker 표시. 모르면 쓰지 않음 |
| `fill_parked` | `fill.key` | string | 체결 통보의 체결 키(게이트웨이가 정함, C-GW G-8) |
| `fill_parked` | `fill.order_id` | u64 또는 null | 게이트웨이가 클라이언트 ID에서 복원한 주문 번호(없으면 null) |
| `fill_parked` | `fill.px` | decimal string | 체결 통보의 가격 |
| `fill_parked` | `fill.qty` | decimal string | 체결 통보의 수량(0 이상) |
| `fill_parked` | `fill.recv_ns` | i64 ns | 체결 통보를 받은 시각. 모르면 쓰지 않음 |
| `fill_parked` | `fill.side` | enum buy/sell | 체결 통보의 방향 |
| `fill_parked` | `fill.venue_oid` | string 또는 null | 체결 통보의 거래소 주문 번호 |
| `fill_parked` | `fill.venue_ts` | u64 µs | 체결 통보의 거래소 시각 |
| `fill_unparked` | `key` | string | 체결 키(게이트웨이가 정함, C-GW G-8) |
| `external_fill_booked` | `fill.fee` | decimal string | 체결 통보의 수수료, 양수는 비용 |
| `external_fill_booked` | `fill.instrument` | string | 체결 통보의 종목 |
| `external_fill_booked` | `fill.is_maker` | bool | 체결 통보의 maker 표시. 모르면 쓰지 않음 |
| `external_fill_booked` | `fill.key` | string | 체결 통보의 체결 키(게이트웨이가 정함, C-GW G-8) |
| `external_fill_booked` | `fill.order_id` | u64 또는 null | 게이트웨이가 클라이언트 ID에서 복원한 주문 번호(없으면 null) |
| `external_fill_booked` | `fill.px` | decimal string | 체결 통보의 가격 |
| `external_fill_booked` | `fill.qty` | decimal string | 체결 통보의 수량(0 이상) |
| `external_fill_booked` | `fill.recv_ns` | i64 ns | 체결 통보를 받은 시각. 모르면 쓰지 않음 |
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
| `margin_reported` | `report.account_mm.maintenance` | decimal string | 교차 증거금 계좌의 유지증거금(USD, Bybit `totalMaintenanceMargin`). 게이트웨이가 주지 않으면(Hyperliquid) `account_mm` 전체를 쓰지 않음 |
| `margin_reported` | `report.account_mm.margin_balance` | decimal string | 교차 증거금 계좌의 증거금 잔고(USD, Bybit `totalMarginBalance`). 위와 같이 쓰지 않을 수 있음 |
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
| `reducing_entered` | `rule` | enum RuleId | daily_loss/mm_buffer/account_mm/margin_stale |
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
| `hold_set` | `hold` | enum kill/watcher_mismatch | 걸린 보류(C-HOLD). `venue_change`는 이 이벤트로 걸지 않는다 |
| `hold_set` | `source` | enum stream/watcher/marker/W1..W4 | 건 쪽: 킬은 개인 스트림·워처·킬 마커, 워처 불일치는 워처의 검사 이름 |
| `hold_set` | `note` | string | 이유 문장(워처의 detail, 마커 내용 등) |
| `hold_cleared` | `hold` | enum kill/watcher_mismatch/venue_change | 운영자가 푼 보류(C-HOLD, `clear_hold`) |
| `hold_cleared` | `note` | string | 운영자의 메모 |
| `kill_loop_ended` | `outcome` | enum confirmed/unsupported | 킬 루프의 끝: 미체결 0건 확인, 또는 일괄 취소가 없는 게이트웨이(C-HOLD, C-KILL) |
| `kill_loop_ended` | `rounds` | u32 | 일괄 취소 회차 수 |
| `kill_loop_ended` | `cancel_all_calls` | u32 | 보낸 일괄 취소 요청 수 |
| `kill_loop_ended` | `detail` | string | 설명 문장 |
| `kill_rearmed` | `source` | enum stream/watcher | 킬 루프의 0건 확인(`confirmed`) 뒤의 새 킬 계기(C-KILL, `unsupported` 뒤는 아님). 킬 래치의 확인을 지우고 새 킬 루프를 부른다. 래치의 첫 메모는 그대로다 |
| `kill_rearmed` | `note` | string | 새 계기의 메모 |
| `strategy_link_lost` | `strategy` | string | 노드가 연결을 잃은 전략(C-LIVE). 그 전략의 일반 주문을 보류하고, 같은 기록의 `cancel_requested`(`by` = `{"rule": "strategy_link"}`)가 그 전략의 걸린 일반 주문 취소를 요청한다 |
| `strategy_link_lost` | `why` | string | 노드가 본 이유: `last trading connection closed` 또는 `no request for {ms} ms` |
| `strategy_link_restored` | `strategy` | string | 다시 `hello`한 전략(C-LIVE). 그 전략의 보류가 풀린다 |
| `watcher_heartbeat` | `watcher` | string | 하트비트를 보낸 워처의 이름(C-LIVE 2부, 1~64자) |
| `watcher_heartbeat` | `checks.W1` | u64 또는 null | 워처 검사 W1(미체결 주문 대조)의 마지막 성공 시각(µs, 호스트 시계), null = 아직 성공 없음. 늘 있다. 게이트는 이 하트비트 기록의 시각에서 이 값까지가 `check_max_age_us`보다 길면 `unmonitored`로 본다 |
| `watcher_heartbeat` | `checks.W2` | u64 또는 null | W2(포지션 대조)의 마지막 성공 시각, W1과 같은 규칙. 늘 있다 |
| `watcher_heartbeat` | `checks.W3` | u64 또는 null | W3의 마지막 성공 시각(있을 때만). 기록하고 보여 줄 뿐 게이트는 보지 않는다 |
| `watcher_heartbeat` | `checks.W4` | u64 또는 null | W4의 마지막 성공 시각(있을 때만). 기록하고 보여 줄 뿐 게이트는 보지 않는다 |
| `watcher_heartbeat` | `checks.W5` | u64 또는 null | W5의 마지막 성공 시각(있을 때만). 기록하고 보여 줄 뿐 게이트는 보지 않는다 |
| `watcher_heartbeat` | `checks.W6` | u64 또는 null | W6의 마지막 성공 시각(있을 때만). 기록하고 보여 줄 뿐 게이트는 보지 않는다 |
| `watcher_heartbeat` | `checks.W7` | u64 또는 null | W7의 마지막 성공 시각(있을 때만). 기록하고 보여 줄 뿐 게이트는 보지 않는다 |
| `watcher_heartbeat` | `checks.W8` | u64 또는 null | W8의 마지막 성공 시각(있을 때만). 기록하고 보여 줄 뿐 게이트는 보지 않는다 |
| `watcher_heartbeat` | `checks.W9` | u64 또는 null | W9의 마지막 성공 시각(있을 때만). 기록하고 보여 줄 뿐 게이트는 보지 않는다 |
| `monitor_changed` | `monitored` | bool | 모니터링 게이트의 판정이 바뀌었다(C-LIVE 2부). false: 일반 주문 보류(`unmonitored`), `unmonitored_cancel_us` 뒤 같은 규칙의 `cancel_requested`(`by` = `{"rule": "unmonitored"}`)가 걸린 일반 주문 취소를 요청한다. true: 보류가 풀린다 |
| `monitor_changed` | `reason` | string | 판정의 이유(`no heartbeat for more than {ms} ms`, `W2 last succeeded more than {ms} ms before the heartbeat`, `W1 and W2 fresh (watcher)` 등) |
<!-- event-fields:end -->

쓰지 않는 기본값: `position_adjusted.reason`은 `operator`일 때 쓰지 않는다. `order_accepted.request.market_ref`, `request_rejected.request.market_ref`는 없을 때 쓰지 않는다. `order_acked.session`은 옛 기록에 없을 수 있다(0).

## 검증

`tail::the_tail_never_returns_a_record_above_the_durable_cap`, `tail::the_tail_follows_segment_rotation_and_starts_after_a_cursor`, `tail::the_tail_never_takes_the_lock`, `tail::a_damaged_durable_frame_stops_the_tail_without_moving_it`, `tail::peek_beyond_shows_valid_frames_past_the_cap_without_moving`, `tail::a_frame_with_the_wrong_sequence_comes_after_the_good_records`, `tail::damage_before_the_cursor_is_an_error`, `tail::the_tail_follows_a_schema_change_to_a_new_segment`, `tail::the_tail_reads_a_header_only_segment_rewritten_with_a_new_schema`(주장 V-TAIL-1). 필드 표: `event_fields`의 세 시험(주장 V-TAIL-2). 복구의 `fsync`는 시험하지 않았다(페이지 캐시 유실을 흉내 낼 수 없다).

감시자의 독자 구현(주장 V-WATCH-2. 워처가 OMS 코드에 의존하지 않는 것은 V-WATCH-1): `crates/watcher/tests/journal.rs`의 시험 9개(상한, 회전과 스키마 변경, 헤더만 있는 세그먼트, 상한 이하 손상, 모르는 이벤트, 다시 읽는 오류와 래치하는 오류, 프레임 하나씩 읽는 큰 세그먼트, ADR-011의 바이트 배치), `events::the_known_events_are_the_rows_of_the_tail_table`(이 표의 이벤트 이름과 감시자가 아는 이름이 양방향으로 같다. 이 문서를 시험 시각에 파일로 읽는다). 적합성(`crates/watcher-conformance`): `conformance::the_watcher_reads_while_the_oms_writer_holds_the_lock`(코어가 결정한 기록을 `oms-journal`이 쓰고 잠금을 쥔 동안 감시자가 같은 이벤트로 읽고, 디렉터리를 바꾸지 않는다), `conformance::the_watcher_base32_pair_equals_the_core_form`. 상태(V-WATCH-2): `crates/watcher/tests/state.rs`의 시험 10개와 `conformance::the_watcher_sets_equal_the_core_on_journals_with_every_event_kind`(이 표의 이벤트 51개를 모두 쓰는 코어 시나리오 열 개에서, 기록마다 감시자의 열린 주문·주문 필드·포지션·체결 키·체결 바닥이 코어와 같다).
