# ADR-022 거래소마다 고객 주문번호 형식을 따로 둔다

상태: 채택 (2026-11-20).

## 1. 문제와 발생 가능한 실패

- OMS가 거래소에 내는 고객 주문번호는 한 형식뿐이었다: `0x` + 16진수 32자리(`VenueClientId::to_hex`, Hyperliquid의 cloid).
- Bybit의 고객 주문번호(`orderLinkId`)는 36자 이하, 숫자·영문자·`-`·`_`다. 게이트웨이가 형식을 제멋대로 고르면, 상태 API(`/api/orders/{id}`의 `venue_client_id`)는 여전히 16진수를 보여 주어 운영자가 거래소에서 찾을 수 없는 번호를 보게 된다.
- 해석이 느슨하면(대소문자를 무시하는 등) 다른 데이터 디렉터리나 사람이 낸 주문이 우리 주문으로 읽힐 수 있다.

## 2. 일반적으로 만족해야 하는 성질

- 한 형식 안에서 `VenueClientId`와 문자열이 일대일이다. 주문번호가 다시 쓰이지 않으면(C-ID) 거래소에 보내는 번호도 다시 쓰이지 않는다.
- `WireIdForm::decode`는 그 형식이 쓴 문자열만 받는다. 나머지는 모두 "남의 주문"이다.
- 상태 API는 게이트웨이가 실제로 쓰는 형식으로 보여 준다.

## 3. 제약

| 사실 | 출처 |
|---|---|
| Bybit `orderLinkId`: 36자 이하, 숫자·영문 대소문자·`-`·`_` | Bybit v5 문서, 설계 TN 근거 파일 05 §3 |
| Bybit `orderLinkId`의 유일성 기간·범위는 문서에 없다 | 같은 곳(미확인. 설계 TN의 testnet 항목 T1이며, 이 저장소 ADR-018의 T1과는 다른 항목이다) |
| `instance_id`, `order_id`는 각각 u64 | `crates/oms-core/src/ids.rs` |
| base32 13자리는 65비트, 12자리는 60비트 | 계산 |
| Hyperliquid cloid와 가상 거래소는 `0x` + 32 hex | `hyperliquid/mod.rs`, `simproto.rs` |

## 4. 필수 조건과 선택 조건

필수: Hyperliquid·가상 거래소 형식 불변. Bybit 형식이 36자 이하이고 일대일. 엄격한 해석.
선택: 사람이 읽기 쉬운 형식, 거래소 문자 집합을 최대한 쓰는 짧은 형식.

## 5. 대안

| 대안 | 설명 |
|---|---|
| A. Bybit도 `0x` + 32 hex | 34자라 길이는 맞는다 |
| B. 형식 목록 `WireIdForm` + 게이트웨이 상수 `WIRE_ID` | `Hex128`(지금 형식), `Base32Pair`(base32hex 13자리 두 개를 `-`로, 27자) |
| C. 게이트웨이마다 자유 함수 | trait에 `encode`/`decode` 메서드를 두고 구현이 알아서 |

## 6. 비교

| 기준 | A | B | C |
|---|---|---|---|
| 길이 | 34자 | 27자 | 구현마다 |
| 상태 API가 실제 형식을 앎 | 형식이 하나라 자명 | `NodeInfo.wire_id_form`으로 | 게이트웨이 객체가 필요 |
| 형식 시험 | 기존 시험 | `oms-core`에서 한 번 | 구현마다 |
| 앞자리 `0x`가 Bybit에서 뜻을 가질 위험 | 없다고 보지만 확인 안 함 | 없음 | 구현마다 |

A도 동작은 한다. B를 고른 이유는 형식을 코드에 이름으로 남기고, Bybit 쪽 길이 여유(9자)를 두며, 형식 시험을 `oms-core` 한 곳에 모으기 위해서다(판단).

## 7. 선택과 이유

B. `crates/oms-core/src/ids.rs`의 `WireIdForm { Hex128, Base32Pair }`, `crates/oms-node/src/venue/mod.rs`의 `const WIRE_ID`.

- `Base32Pair`는 두 반쪽을 항상 13자리로 쓴다. 고정 폭이라 일대일이고 해석이 엄격하다. 설계 TN §7.3의 "instance_id 12자"는 60비트밖에 못 담아 13자로 고쳤다.
- 소문자 base32hex(`0-9a-v`, RFC 4648 §7)만 쓴다. 대문자, 다른 길이, 구분자 없음, 64비트를 넘는 13자리(맨 앞이 `g` 이상)는 해석하지 않는다.
- 가상 거래소와 Hyperliquid는 `Hex128`을 선언하고, 주문·취소·상태 조회에 넣는 번호를 `<Self as VenueGateway>::WIRE_ID.encode`로 만든다. 선언한 형식과 실제로 보내는 형식이 한 곳에서 나온다. `Hex128.encode`는 `to_hex`와 같으므로 보내는 바이트는 그대로다.
- `Hex128.decode`는 소문자 16진수만 받는다. 기존 `parse_hex`는 대문자와 부호(`+`)도 받는다(`u128::from_str_radix`). 게이트웨이가 거래소 응답을 읽는 곳(`hyperliquid/wire.rs`, `simnet.rs`)은 `parse_hex`를 그대로 쓴다. 바꾸면 Hyperliquid 응답 해석이 바뀌므로 이번 단위에서 하지 않았다.
- `node.rs`가 띄운 게이트웨이의 `WIRE_ID`를 `NodeInfo.wire_id_form`에 넣고 상태 API가 그것으로 출력한다.
- 체결 키는 이미 게이트웨이가 정한다(C-GW G-8). Bybit 행 `<symbol>:<execId>`(69자 이하)를 C-GW에 미리 적고 구현은 Bybit 게이트웨이 단위가 한다.

## 8. 포기한 장점, 남은 위험, 가정

- 포기: 사람이 읽기 쉬운 형식(예: 10진수). 27자 base32는 눈으로 `order_id`를 읽기 어렵다. 상태 API가 둘을 나란히 보여 준다.
- 위험: Bybit가 같은 `orderLinkId`를 일정 기간 거절하거나 다른 하위 계정과 범위를 공유하는지 모른다(설계 TN T1).
- 위험: Hyperliquid 응답 해석은 여전히 `parse_hex`라 대문자로 돌아온 cloid도 우리 것으로 읽는다. Hyperliquid가 cloid를 우리가 보낸 대로(소문자) 돌려준다는 것은 기존 가정이다. 형식은 재사용을 하지 않으므로 영향은 "과거 저널 사본으로 재개" 경우(가정 A-12)에 한정된다.
- 가정: 하나의 노드는 게이트웨이 하나만 띄운다. 형식은 노드 수명 동안 바뀌지 않는다.

## 9. 근거

- 시험: `ids::tests::hex128_is_the_hex_form_of_today`, `ids::tests::base32_pair_roundtrips_and_fits_an_order_link_id`, `ids::tests::base32_pair_gives_a_million_distinct_ids`, `ids::tests::base32_pair_decoding_is_strict`, `venue::tests::the_gateways_of_today_write_the_hex_form`(주장 V-WIRE-ID-1).
- 설계 TN(암호화폐 거래 통합 설계 v1.0) §7.2, §7.3. 저장소 밖 문서다.

## 10. 재검토 조건

- testnet에서 `orderLinkId`의 문자 집합·길이·재사용 규칙이 문서와 다르게 나올 때(설계 TN T1).
- 한 노드가 거래소 둘 이상에 주문을 내게 될 때.
