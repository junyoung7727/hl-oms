# ADR-019 시세 레코드의 시장은 conn_id 구간으로 정한다

상태: 채택, 구현됨(2026-11-19, U5b `.scratch/md-collector/`). 계약(quant-data-platform `docs/crypto/lake-contract.md`)은 바꾸지 않았다. 나중에 계약이 이 규칙을 받아들일 수 있다(U8).

## 1. 문제

봉인기는 L0 레코드(`recv_ns, src, conn_id, frame_no, payload`)만 보고 그 레코드의 토픽(시장, 채널, 종목)을 정해야 한다(계약 2.5). Bybit 프레임에는 시장이 없다. 선형 `orderbook.50.BTCUSDT`와 현물 `orderbook.50.BTCUSDT`는 같은 글자다. U4 시험의 분류기는 언제나 `linear`라고 답해서 이 문제가 드러나지 않았다.

## 2. 결정

- 거래소마다 시장의 순서(슬롯)를 고정한다. Bybit: `linear` 0, `inverse` 1, `spot` 2.
- 슬롯 `k`의 시장은 `conn_id` `[max(k << 28, 1), (k + 1) << 28)`를 쓴다. 수집기는 시장마다 md-feed `ConnIds::range`로 이 구간에서만 번호를 받는다. 모든 구간이 REST `frame_no` 카운터 하나를 같이 쓴다(`conn_id 0`은 세션에 하나뿐인 REST 의사 연결).
- 분류기(`md-collector` registry): 웹소켓 레코드는 `conn_id >> 28`로 시장, 어댑터의 `route`로 채널·종목. REST 본문은 본문이 밝히는 시장(Bybit 종목정보 응답의 `result.category`), 토픽 `{market, instruments, _all}`. 노트는 분류하지 않는다.
- 슬롯 번호는 바꾸지 않는다(시험 `market_slots_pinned`). 바꾸면 옛 파일이 다른 시장으로 분류된다. 새 시장은 뒤에 붙인다.

## 3. 대안

| 대안 | 버린 이유 |
|---|---|
| 열린 파일 머리에 `market`을 넣는다 | 계약의 머리 스키마가 닫혀 있다(`additionalProperties: false`). 계약 변경과 qdp 재반입이 필요하다 |
| 시장마다 수집기 프로세스를 따로 둔다 | 계약은 거래소마다 수집기 하나를 전제한다(2.5 "One collector runs per venue"). 동시 세션은 고장으로 본다 |
| 연결과 시장의 대응을 세션 파일에 노트로 남긴다 | 노트 종류가 닫혀 있고, 봉인기가 잠긴 세션 파일을 읽어야 한다 |

## 4. 결과

- `conn_id`는 연속이 아니다(선형 1, 2, …; 현물 536870912, …). 계약은 연속을 요구하지 않는다(`conn_id`는 "세션 안의 연결 번호", 최대 2^32−1).
- `by_connection` 일 파일을 읽는 쪽도 이 규칙으로 시장을 안다. qdp 쪽 독자(U8)는 이 ADR을 따라야 한다.
- 시장 하나는 2^28 − 1개 연결 번호를 쓸 수 있다(재연결마다 하나). 하루 수천 번 재연결해도 남는다.
