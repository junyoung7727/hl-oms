# ADR-034 Bybit RSA 서명 키

상태: 채택 (2026-11-25).

## 1. 문제와 발생 가능한 실패

- 워처와 OMS의 Bybit 게이트웨이는 모든 비공개 요청을 HMAC-SHA256으로만 서명했다. 소유자의 시험 운영(dogfood)용 testnet 키 두 개(소유자의 testnet 하위 계정, IP에 묶임)는 RSA 키다. Bybit testnet의 AI 하위 계정은 직접 만든(RSA) 키만 내 주고 시스템이 만든(HMAC) 키는 고를 수 없다. 그래서 두 바이너리 모두 이 키를 쓸 수 없었다.
- RSA 서명을 잘못된 모양(hex)으로 보내면 거래소는 "Invalid sign"으로 거절한다(F2). 키 거절은 재시작까지 래치되므로(ADR-028, C-WATCH 3부) 잘못된 모양은 운영을 멈춘다.
- 개인 키 파일이 두 개(비밀 하나, PEM 경로 하나)면 두 파일의 권한 규칙이 따로 놀 수 있다. 오류 줄이나 `Debug`에 키가 나오면 로그와 메시지로 샌다.

## 2. 일반적으로 만족해야 하는 성질

- HMAC 키의 동작은 바이트까지 그대로다(기존 시험 벡터가 바뀌지 않는다).
- 모든 서명이 한 곳을 지난다: HMAC이면 소문자 hex, RSA면 표준 base64. 서명하는 문자열은 둘이 같다.
- 키는 시작할 때 한 번 읽고 한 번 파싱한다. 읽을 수 없는 키는 시작 거부다. 오류 줄과 `Debug`는 키의 어느 바이트도 담지 않는다.
- 서명을 만들지 못하면 그 요청은 보내지 않는다. 서명 없이 보내거나 다른 방식으로 서명해 보내지 않는다.
- 워처는 여전히 OMS와 코드를 나누지 않는다(ADR-031).

## 3. 제약

| 사실 | 출처 |
|---|---|
| F1 REST: `X-BAPI-SIGN = base64(RSA-SHA256 PKCS#1 v1.5 서명(timestamp + api_key + recv_window + payload))`, 패딩이 있는 표준 base64. HMAC과 같은 문자열, 변환만 다르다. 이렇게 서명한 `GET /v5/user/query-api`가 retCode 0 | 2026-11-25 01:0x KST testnet 측정(openssl + python, 소유자의 testnet 하위 계정 키), Bybit v5 "Create A Request"("lowercase HEX string for HMAC_SHA256, or base64 for RSA_SHA256") |
| F2 비공개 WebSocket `auth`: `"GET/realtime" + expires`의 RSA-SHA256 base64 서명은 `{"success":true}`, 같은 서명의 소문자 hex는 `{"success":false,"ret_msg":"Invalid sign"}` | 같은 측정 |
| F3 머리글을 더할 필요가 없다(F1에서 `X-BAPI-SIGN-TYPE`을 보내지 않았다) | 같은 측정 |
| F4 `openssl genpkey -algorithm RSA -pkeyopt rsa_keygen_bits:2048`의 파일은 PKCS#8 PEM(`BEGIN PRIVATE KEY` 줄) | 같은 측정 |
| F5 `ring 0.17.14`와 `base64 0.23.1`이 이미 `Cargo.lock`에 있다(rustls·reqwest를 거쳐). ring은 PKCS#8 DER에서 RSA PKCS#1 v1.5 SHA-256으로 서명하고(`RsaKeyPair::from_pkcs8`, `RSA_PKCS1_SHA256`) 2048비트 미만의 모듈러스를 거부한다 | `Cargo.lock`, ring 0.17.14 소스 |
| 워처는 작업 공간 crate를 쓰지 않는다 | `tools/ci/watcher_independent.sh`, ADR-031 |
| 저장소는 공개다. 추적되는 파일에 PEM 머리 줄이 있으면 `tools/ci/secret_scan.sh`가 실패한다 | `AGENTS.md`, `tools/ci/secret_scan.sh` |

## 4. 필수 조건과 선택 조건

필수: 위 F1~F3대로 서명, HMAC 동작 불변, 키 한 번 파싱, 시작 거부, 키가 새지 않음, 서명 실패 시 보내지 않음, 새 패키지 없음(`cargo build --locked`), 워처 독립성. 선택: 키 회전, Ed25519, 암호화된 PEM의 복호화(범위 밖).

## 5. 대안

- A. 자격 파일에 PEM 파일의 경로(`rsa_private_key_file`)를 둔다.
- B. 자격 파일 안에 PEM 글을 그대로 둔다(TOML 여러 줄 문자열).
- C. PKCS#1(`BEGIN RSA PRIVATE KEY`)과 암호화된 PKCS#8도 받는다.
- D. PKCS#8만 받고, 다른 형은 변환 명령을 적은 한 줄로 거부한다.
- E. 서명 함수를 crate마다 호출 지점에 둔다(HMAC 옆에 RSA 분기).
- F. crate마다 서명기 하나(`Signer`)를 두고 모든 호출 지점이 그것을 지난다.

## 6. 비교

| | 파일 규칙 | 실패 모드 | 코드 |
|---|---|---|---|
| A | 파일 둘, 규칙 둘(두 번째 파일도 같은 검사가 필요) | 한쪽만 느슨할 수 있다 | 경로 검사 하나 더 |
| B | 파일 하나, 규칙 하나 | 없음 | TOML 여러 줄 문자열 |
| C | — | 암호를 물을 곳이 없다(무인 실행). PKCS#1은 ring의 `from_der`로 되지만 받는 형이 둘 | 늘어난다 |
| D | — | 운영자가 변환 한 번 | 작다 |
| E | — | 한 호출 지점이 HMAC으로 남을 수 있다 | 분기가 흩어진다 |
| F | — | 없음 | 한 곳 |

## 7. 선택과 이유

B, D, F. 결정 D1~D7(스펙):

- D1 자격 파일(두 바이너리, 같은 모양): TOML `api_key`와 `api_secret`(HMAC, 그대로) 또는 `rsa_private_key`(PKCS#8 PEM 글, TOML 여러 줄 문자열) 중 **정확히 하나**. 둘 다 또는 둘 다 없으면 파일을 인용하지 않는 한 줄로 시작 거부. `api_secret` 값이 PEM 개인 키(`PRIVATE KEY-----`를 담음)면 HMAC 비밀로 쓰지 않고 "RSA 키는 `rsa_private_key`에" 한 줄로 시작 거부(검토 S2: 그대로 쓰면 모든 요청이 서명 거절 10004로 막힌다). 모르는 키는 여전히 거부. 파일 규칙(일반 파일, 소유자, 0600/0400(OMS는 그룹·기타 비트 없음), 데이터 디렉터리 밖, 링크 아님)은 그대로이고 이제 RSA 키도 지킨다. 비밀 파일이 하나라서 검사도 한 벌이다.
- D2 PEM: `BEGIN PRIVATE KEY` 줄의 PKCS#8(암호화 안 됨)만. PKCS#1(`BEGIN RSA PRIVATE KEY`)과 `BEGIN ENCRYPTED PRIVATE KEY`는 고치는 명령(`openssl pkcs8 -topk8 -nocrypt -in old.pem -out new.pem`)을 적은 줄로 거부한다(두 형 모두 이 명령으로 고쳐짐을 openssl 3.5.5로 확인). 2048비트 미만은 거부(ring). 시작할 때 한 번 파싱하고, 파싱되지 않는 키는 시작 거부다: 워처는 다른 자격 오류처럼 종료 코드 2, OMS는 기존 자격 오류 길(게이트웨이를 띄우지 못함).
- D3 crate마다 서명기 하나: 자격 정보가 `Signer`(HMAC 비밀 | 파싱된 RSA 키 쌍)를 들고, `sign(prehash) -> Result<String, String>`이 HMAC은 소문자 hex, RSA는 표준 base64를 돌려준다. 모든 호출 지점(워처의 GET과 일괄 취소 POST, OMS의 REST GET·POST와 비공개 스트림·WS Trade의 `auth`)이 그것을 지난다. 따로 HMAC을 부르는 호출 지점은 없다(HMAC 자유 함수는 기존 시험 벡터용으로만 남는다).
- D4 워처는 F1~F3과 Bybit 문서로 자기 구현을 쓴다. OMS 코드를 쓰지 않는다(`watcher_independent.sh` 통과).
- D5 의존성: `ring = "0.17"`, `base64 = "0.23"`을 `crates/watcher`와 `crates/oms-node`에 더한다. `Cargo.lock`에는 두 crate의 의존성 줄 네 개만 늘고 새 패키지는 없다(`cargo build --locked` 통과).
- D6 `Debug`는 비밀도 키도 찍지 않는다: `Credentials { api_key: "…", api_secret: <hidden> }` 또는 `… rsa_private_key: <hidden> }`. `Signer`에는 `Debug`가 없다.
- D7 RSA 서명 실패(ring의 `Err`)는 파싱된 키에서 기대하지 않는다. 생기면 그 요청은 보내지 않은 것이다: 워처는 `not_sent`(그 경로의 오류로 셈), OMS REST는 `RestError::NotSent`(주문 경로는 보내지 않음으로 확정), 스트림은 "auth not sent"로 다시 연결. 서명 없이나 HMAC으로 보내지 않는다.

판단(스펙 밖, 이 구현의 결정):
- RSA 키의 요청에는 `X-BAPI-SIGN-TYPE` 머리글을 보내지 않는다. F1에서 측정한 RSA 요청이 이 머리글 없이 retCode 0이었다. HMAC 요청은 전처럼 `2`를 보낸다. 측정한 모양을 따르는 쪽을 골랐다.
- 워처는 서명을 요청 제한(가드)을 통과한 뒤, 보내는 순간의 시계로 만든다(전과 같다). 서명 실패는 `watcher_requests_total`을 올리지 않는다(나가지 않은 요청).

## 8. 포기한 장점, 남은 위험, 가정

- 포기: 암호화된 키 파일. 무인 실행에서 암호를 물을 곳이 없다. 키는 0600 파일의 보호만 받는다(HMAC 비밀과 같다).
- 위험: 서명 모양은 2026-11-25의 testnet 측정 한 번(F1, F2)과 문서에서 왔다. mainnet과 WS Trade(`/v5/trade`)의 `auth`는 RSA로 보내 본 적이 없다(WS Trade는 비공개 스트림과 같은 서명을 쓴다는 문서에 기댄다).
- 위험: 서명 실패 경로(D7)는 시험으로 일으키지 못했다(ring이 파싱된 키로 실패하게 만들 방법이 시험에 없다). 코드와 검토로만 본다.
- 가정: Bybit가 RSA 요청의 `X-BAPI-SIGN-TYPE` 부재를 계속 받는다(F3). 받지 않으면 키 거절(10004)로 래치되고, 그때 머리글을 되살린다.

## 9. 근거

- 시험(워처): `tests/sign.rs` `an_rsa_signature_is_openssls_base64_and_verifies_with_the_public_key`, `tests/creds.rs` `an_rsa_credentials_file_is_read_and_its_refusals_quote_no_key`, `tests/client.rs` `an_rsa_key_signs_the_get_and_the_cancel_all_in_base64`(주장 V-WATCH-14).
- 시험(OMS): `venue::bybit::sign::tests::rsa_signatures_are_openssls_base64_and_verify_with_the_public_key`, `venue::bybit::rest::tests::an_rsa_credentials_file_is_read_and_its_refusals_quote_no_key`, `venue::bybit::rest::tests::an_rsa_key_signs_a_get_in_base64_over_the_query_sent`, `venue::bybit::orders::tests::a_create_with_an_rsa_key_is_signed_in_base64_over_the_body_sent`, `venue::bybit::stream::tests::the_auth_message_with_an_rsa_key_carries_the_base64_signature`, `venue::bybit::trade::tests::the_auth_message_with_an_rsa_key_carries_the_base64_signature`(주장 V-BYBIT-RSA-1).
- 기대값은 시험 코드가 아니라 openssl에서 왔다: `printf '%s' "$prehash" | openssl dgst -sha256 -sign key.pem | base64 -w0`. PKCS#1 v1.5는 결정적이라 정확히 같아야 한다. 시험 키는 시험 전용으로 openssl로 한 번 만들었고 소스에는 base64 본문만 있다(머리 줄은 실행 때 만든다).
- 변이: M311~M318(xtask), 스펙의 고의 결함 점검(`mut.py`, 저장소 밖).

## 10. 재검토 조건

- Bybit가 HMAC 키를 다시 주거나 RSA 서명 규칙·머리글 규칙을 바꿀 때.
- 키 회전이나 Ed25519 키가 필요할 때(범위 밖).
- 암호화된 키 파일이 필요해질 때(비밀 관리자, 하드웨어 키).
