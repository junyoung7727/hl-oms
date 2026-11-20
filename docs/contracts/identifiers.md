# 식별자의 발급·범위·수명

설계 근거: [ADR-006](../adr/ADR-006-idempotency.md). 타입: `crates/oms-core/src/ids.rs`.

| 식별자 | 형식 | 발급 | 유일성 범위 | 수명 | 재사용 |
|---|---|---|---|---|---|
| `strategy` | 1~32자, `[A-Za-z0-9_.:-]` | 설정 | 인스턴스 | 설정에 있는 동안 | 설정이 정한다 |
| `client_order_id` | 1~64자, 같은 문자 집합 | 전략 | (전략, 키) | 요청 만료 + 60초, 그리고 주문이 메모리에 있는 동안 | 잊힌 뒤에는 가능. 권하지 않는다 |
| `instrument` | 1~32자 | 설정. Hyperliquid에서는 코인 이름 | 인스턴스 | 설정에 있는 동안 | — |
| `order_id` | u64 | 코어. 1부터 증가 | 인스턴스(데이터 디렉터리) | 저널에 영구 | 저널이 온전하면 없음. **과거 시점의 저널 사본으로 거래를 재개하면 사본 이후의 번호를 다시 쓴다**(가정 A-12) |
| `instance_id` | u64, 0이 아님 | 데이터 디렉터리를 처음 쓸 때 `/dev/urandom` | 전역(확률적) | 디렉터리의 수명 | 없음 |
| 거래소 클라이언트 ID(cloid) | `0x` + 32 hex = `instance_id`(상위) ‖ `order_id`(하위) | 파생 | 계좌(확률적) | 주문과 같다 | `order_id`와 같다. 다시 쓰이면 조회가 옛 주문의 상태를 새 주문의 것으로 돌려줄 수 있다 |
| 거래소 클라이언트 ID, `Base32Pair` 형식 | `instance_id` 13자리 ‖ `-` ‖ `order_id` 13자리, 소문자 base32hex(`0-9a-v`), 27자. Bybit `orderLinkId`용([ADR-022](../adr/ADR-022-wire-id-form.md)) | 파생 | 계좌(확률적) | 주문과 같다 | 위와 같다 |
| 거래소 주문 ID(oid) | 거래소가 정한다. 문자열로 보관 | 거래소 | 거래소 | 거래소 | 거래소 |
| 체결 키 | 게이트웨이가 구성. Hyperliquid는 `시각:코인:tid:oid` | 게이트웨이 | 계좌 | 체결 동기화 시점 − 24시간까지 기억 | — |
| 저널 순번 `seq` | u64, 1부터 빈틈 없이 | 코어 스레드 | 인스턴스 | 영구 | 없음 |
| 부팅 번호 `boot` | u32, 1부터 | 코어(`Booted`) | 인스턴스 | 영구 | 없음 |
| 링크 세션 | u64 = `boot`(상위 32비트) ‖ 연결 횟수 | 게이트웨이 | 인스턴스 | 다음 연결까지 | 없음(증가만) |
| 불일치 ID | u32, 1부터 | 코어 | 인스턴스 | 확인 후 보존 시간까지 메모리, 저널에는 영구 | 없음 |
| nonce | ms 시각 기반, 증가만 | 게이트웨이 | 서명자(agent 지갑) | 거래소가 상위 100개를 기억 | 없음 |
| 설정 버전 | u32 | 운영자 | 인스턴스 | 영구 | 없음(증가만) |

## 경계에서의 규칙

- 문자열 식별자는 생성과 역직렬화에서 검증된다. 검증을 통과하지 못한 값은 코어 안에 존재할 수 없다.
- `order_id`는 `OrderAccepted` 이벤트가 "다음 ID와 같은가"를 확인하며 적용된다. 건너뛰거나 되돌아가면 `CoreFault`다.
- 내구화되지 않은 채 사라진 수용의 `order_id`는 재시작 뒤 다른 주문에 다시 배정된다. 그 ID로는 전송이 나간 적이 없으므로([C-COMMIT](commit.md)) 거래소에서 충돌하지 않는다.
- 다른 데이터 디렉터리(다른 `instance_id`)가 낸 주문은 거래소 조회에서 "남의 주문"으로 분류된다.
- 같은 저널의 사본으로 띄운 두 프로세스는 `instance_id`가 같다. 서로를 구별하지 못한다(가정 A-06).
- 거래일 개념이 없으므로 일 단위로 초기화되는 식별자는 없다.

## 검증

`ids::tests::rejects_invalid_identifiers`, `ids::tests::venue_client_id_roundtrip`, `ids::tests::base32_pair_roundtrips_and_fits_an_order_link_id`, `ids::tests::base32_pair_decoding_is_strict`, `scenarios::the_key_scope_is_per_strategy`,
`journal::a_journal_of_another_instance_is_refused`, `e2e::only_one_node_can_own_a_data_directory`.
