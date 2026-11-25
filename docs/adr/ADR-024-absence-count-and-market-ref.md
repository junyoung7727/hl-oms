# ADR-024 부재 판정에 필요한 "없음" 횟수, Bybit의 만료, 주문의 market_ref

상태: 채택 (2026-11-20).

## 1. 문제와 발생 가능한 실패

- ADR-005 규칙 5: 결과 불명 주문은 `expires_at + absence_margin` 뒤에 보낸 상태 조회가 "없음"이면 `NotPlaced`로 닫고 예약을 푼다. 거래소가 만료 뒤의 요청을 받지 않아야 성립한다. Hyperliquid에는 `expiresAfter`가 있다. Bybit에는 만료 필드가 없다.
- `venue_enforces_expiry = false`로 두면 Bybit의 결과 불명 주문은 매번 운영자가 닫아야 한다. 그동안 계좌 전체가 막힌다(`InDoubt`).
- Bybit `/v5/order/realtime`은 닫힌 주문을 "최근 500건"까지만 들고 있고 서비스 재시작 때 비운다. 놓였다가 체결·취소되고 목록에서 빠진 주문이 "없음"으로 보일 수 있다. 게이트웨이가 주문 내역도 보지만, 예약을 푸는 근거로 한 번의 답은 얇다. 잘못 닫으면 실제로 있는 포지션의 예약이 풀려 한도를 넘는 주문이 들어갈 수 있다.
- 저널에 전략이 어떤 호가창 상태를 보고 주문했는지가 없어 주문과 시세 레이크를 이을 수 없다(설계 TN §5.4.4).

## 2. 일반적으로 만족해야 하는 성질

- 결과 불명 주문은 거래소가 더는 그 요청을 놓을 수 없다는 근거와 "없음" 증거가 함께 있을 때만 닫힌다.
- Hyperliquid의 동작과 기록은 바뀌지 않는다.
- `market_ref`는 주문과 함께 저널에 남고, 멱등성(같은 요청은 같은 주문)을 바꾸지 않는다.

## 3. 제약

| 사실 | 출처 |
|---|---|
| "Bybit_server_time - X-BAPI-RECV-WINDOW <= X-BAPI-TIMESTAMP < Bybit_server_time + 1000". 기본 recv window 5000 ms | Bybit v5 문서, 설계 TN 근거 파일 05 §1 |
| realtime 조회는 닫힌 주문 최근 500건, 서비스 재시작 때 비움 | 같은 파일 §2 |
| 기존 규칙은 "만료 + 여유 뒤 없음 한 번" | `decide.rs` `on_not_found`, ADR-005 §7 |
| 멱등성 비교는 필드별(종목, 방향, 수량, 가격, tif, 감소 전용, 만료) | `decide.rs` `new_order` |
| `md_feed::MarketRef`의 필드 | `crates/md-feed/src/state.rs` |

## 4. 필수 조건과 선택 조건

필수: 2의 세 성질. 기본 설정에서 기존 시험과 기록이 그대로.
선택: Bybit에서 운영자 없이 결과 불명이 풀림.

## 5. 대안

| 대안 | 설명 |
|---|---|
| A. Bybit는 `venue_enforces_expiry = false` | 규칙 5를 쓰지 않는다. 운영자가 닫는다 |
| B. 게이트웨이가 recv window로 만료를 강제 + "없음" 횟수 설정 | 이 결정 |
| C. 코어가 recv window 값을 알고 전송 시각 + recv window로 판정 | 전송 시각을 코어에 넘기는 입력이 새로 필요하다 |

## 6. 비교

| 기준 | A | B | C |
|---|---|---|---|
| 결과 불명이 저절로 풀림 | 아니다 | 그렇다 | 그렇다 |
| 코어 변경 | 없음 | 정책 필드 하나, 이벤트 하나 | 입력 필드, 판정 변경 |
| 판정 근거가 기존 규칙과 같음 | — | 같다(`expires_at` 기준) | 다르다 |
| 짧은 불일치(조회 사본의 지연)에 대한 방어 | — | 여유 시간 이상 떨어진 "없음" 2회 | 별도로 필요 |
| 500건 한도·서비스 재시작에 대한 방어 | — | 횟수로는 못 막는다(빠진 주문은 계속 없다). 게이트웨이의 주문 내역 확인이 막는다 | 같음 |

## 7. 선택과 이유

B.

- 게이트웨이 계약(C-GW G-4 Bybit): 요청 시각 = 보내는 순간의 로컬 시각, `요청 시각 + recv_window + 1초 ≤ expires_at`일 때만 보낸다. 아니면 `NotSent`. 거래소는 서버 시각이 `요청 시각 + recv_window`를 넘으면 받지 않는다. 로컬 시계가 1초 넘게 앞서면 Bybit가 거절하므로 로컬 기준 마감은 늦어도 `요청 시각 + recv_window + 1초`다. 로컬 시계가 뒤처지면 마감이 앞당겨질 뿐이다. 그래서 `expires_at` 뒤에는 요청이 놓일 수 없고 Bybit 설정은 `venue_enforces_expiry = true`다. 처리 지연과 남은 시계 차이는 기존 `absence_margin_us`(기본 10초)가 덮는다(ADR-005와 같은 가정).
- 정책 `absence_not_found_needed`(기본 1, 0은 설정 오류): 규칙 5의 "없음"을 그 수만큼 모아야 닫는다. 2 이상일 때만 세는 답마다 `AbsenceNoted { order_id, queried_at }`를 기록하고, 앞에 센 답보다 `absence_margin_us` 이상 뒤에 보낸 조회만 센다. 같은 조회의 두 답이나 잠깐 사이의 두 조회(예: 재연결 직후 다시 묻기)는 거래소의 같은 순간 상태를 볼 수 있어 하나로 친다(검토 2026-11-20).
- 이 횟수가 막는 것은 짧은 불일치뿐이다. 500건 한도에서 빠지거나 서비스 재시작으로 지워진 주문은 몇 번 물어도 없다. 그래서 Bybit 게이트웨이는 realtime 조회와 주문 내역(`/v5/order/history`) 조회에서 모두 없을 때만 `StatusNotFound`를 보내야 한다. 이것은 `absence_not_found_needed = 2`를 쓰는 전제 조건이다(U16의 요구 사항). 세는 동안 예약은 그대로다. 주문의 존재 증거가 오면 주문이 `PendingSend`를 벗어나므로 이 셈은 더 쓰이지 않는다.
- 기본값 1일 때는 `AbsenceNoted`를 쓰지 않고, 정책의 새 필드도 직렬화하지 않는다. Hyperliquid 설정과 기존 기록은 같은 바이트다.
- `NewOrderReq.market_ref: Option<MarketRef>`(`venue`, `market`, `symbol`, `channel`은 64자 이하의 `[A-Za-z0-9_.:-]`, `update_id`, `venue_ts_ns`, `recv_ns`). 없으면 쓰지 않는다. `OrderAccepted.request`로 저널에 남는다. 주문 상태(메모리)에는 두지 않는다(내보내기 U15는 저널을 읽는다). 멱등성 비교에서 뺀다: 재시도는 보통 새 호가로 오므로, 넣으면 같은 주문의 재시도가 `conflict`가 된다. 처음 요청의 값이 남는다.
- 거래 API(`oms-api` `Op::NewOrder`)는 `market_ref`를 선택 필드로 받는다. 이름이 형식에 맞지 않으면 요청 해석 단계에서 거절된다(코어에 닿지 않는다).

## 8. 포기한 장점, 남은 위험, 가정

- 포기: C의 더 이른 해소(전송 시각 + recv window는 `expires_at`보다 이를 수 있다).
- 위험: 두 번의 "없음"이 둘 다 500건 한도나 서비스 재시작 때문이면 여전히 잘못 닫는다. 게이트웨이가 주문 내역(`/v5/order/history`)도 보고 둘 다 없을 때만 "없음"을 보내는 것이 전제다(U16). 닫은 뒤 주문이 나타나면 기존 규칙대로 `OpenAfterClose`·`FillAfterClose`로 드러난다.
- 위험: Bybit가 요청을 받은 뒤 매칭 엔진이 늦게 처리하는 시간은 문서에 없다. `absence_margin_us`가 그보다 길다는 것은 가정이다(ADR-005 가정 A-01과 같은 종류).
- 가정: 대조기는 결과 불명 주문을 계속 다시 묻는다. 두 번째로 세는 답은 첫 답보다 `absence_margin_us`(기본 10초) 이상 뒤의 조회에서 온다. 그만큼 해소가 늦어진다.

## 9. 근거

- 시험: `crates/oms-core/tests/absence_and_market_ref.rs` 8개, `oms-api` `tests::a_new_order_line_may_carry_a_market_ref`(주장 V-ABSENCE-N-1, V-MARKET-REF-1).
- 설계 TN(암호화폐 거래 통합 설계 v1.0) §7.3 O5, §5.4.4. 저장소 밖 문서다.

## 10. 재검토 조건

- testnet에서 recv window 밖의 요청이 거절되지 않거나 거절 코드가 결과 불명으로만 보일 때.
- 주문 내역 조회도 닫힌 주문을 잃는 경우가 확인될 때(횟수를 늘리거나 운영자 해소로 돌아간다).
