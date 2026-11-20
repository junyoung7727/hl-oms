# ADR-028 Bybit 게이트웨이

상태: 만드는 중. U16b(이 판): 설정, 서명, REST 클라이언트, 시작 확인, IP 확인, cancel-all. U16c: 조회와 체결 분류표, 증거금 보고. U16d: 비공개 스트림과 노드 연결. 주문 전송은 U17. 노드는 U16d 전까지 `kind = "bybit"`로 시작하지 않는다.

[ADR-017](ADR-017-hyperliquid-gateway.md)의 틀을 따른다.

## 1. 문제와 발생 가능한 실패

- 키가 다른 계좌(마스터 계좌, 다른 서브계정)의 것이면 엉뚱한 계좌에서 거래한다.
- 계좌가 교차 증거금이 아니거나(격리, 포트폴리오), 헤지 모드이거나, 종목이 USDT 정산이 아니면, 코어의 장부·한도·정리 범위가 거래소와 다른 것을 가리킨다.
- 서명 문자열과 보내는 바이트가 다르면 모든 요청이 거부된다. 거부를 시간 문제로 오인하면 같은 거부를 계속 보낸다.
- 거부된 키(만료, IP 불일치, 잘못된 키)로 계속 요청하면 거래소가 IP를 막을 수 있고, 운영자는 원인을 모른다.
- 요청이 키에 묶이지 않은 IP에서 나가면(호스트 이동, NAT 변경) 다음 주문부터 실패한다. 미리 알아야 한다.
- 비밀이 로그·저널·오류 문장에 남는다.

## 2. 일반적으로 만족해야 하는 성질

- 시작 확인은 읽기만 한다. 거래소 설정을 바꾸지 않는다(ADR-018과 같다).
- 확인 하나라도 어긋나면 거래하지 않는다. 확인할 수 없으면(응답 없음) 통과로 보지 않는다.
- 다시 시도해도 바뀌지 않는 거부는 다시 시도하지 않는다.
- 서명한 바이트 = 보낸 바이트.
- 비밀은 파일 하나에만 있다.

## 3. 제약

| 사실 | 출처(저장소 밖 조사 노트: F18 = `BYBIT_V5_PRIVATE_API_FACTS_20261118.md`, F20 = `BYBIT_V5_GATEWAY_FACTS_20261120.md`) |
|---|---|
| 서명: `timestamp + api_key + recv_window + (질의 문자열 또는 본문)`의 HMAC-SHA256, 소문자 hex. 헤더 `X-BAPI-API-KEY`, `X-BAPI-TIMESTAMP`, `X-BAPI-SIGN`, `X-BAPI-RECV-WINDOW`(기본 5000) | F20 Q1.1–1.3 (문서) |
| 문서의 서명 예시는 비밀을 공개하지 않아 HMAC 결과를 맞춰 볼 수 없다 | F20 Q1.11 |
| `X-BAPI-SIGN-TYPE: 2`는 예시에만 있고 뜻은 문서에 없다. SDK는 늘 보낸다 | F20 Q1.5–1.6 |
| SDK는 질의를 키 순서로 정렬하고 값을 인코딩하지 않는다. 서버가 순서를 요구하는지는 문서에 없다 | F20 Q1.13–1.14 |
| 오류 코드 10003(키 무효), 10004(서명), 10005(권한), 10007(인증 실패), 10010(IP 불일치), 33004(키 만료), 10006(한도), 10002(시각 창), 10016(서버 오류) | F20 Q3 (문서) |
| `GET /v5/user/query-api`: `userID`, `isMaster`, `readOnly`(정수), `ips`, `permissions.ContractTrade` | F20 Q4–5 (문서) |
| IP에 묶이지 않은 키는 90일 뒤 무효. 묶이지 않은 키의 `ips` 값은 query-api에서는 문서에 없다(이웃 예시는 `["*"]`) | F20 Q4 |
| 포지션 모드를 읽는 엔드포인트는 없다. `position/list`의 `positionIdx`(0 = 단방향)로만 안다. 종목을 주면 포지션이 없어도 행을 준다 | F20 Q7.1–7.3 |
| `instruments-info`의 `settleCoin`, 요청에 정산 코인 필터는 없다 | F20 Q8.1–8.2 |
| `account/info`의 `marginMode`(`REGULAR_MARGIN` = 교차), `unifiedMarginStatus` 3–6 = 통합 계좌 | F18 §6, F20 Q6 |
| 엔드포인트: REST `https://api.bybit.com`, testnet `https://api-testnet.bybit.com`; 비공개 스트림 `wss://stream(-testnet).bybit.com/v5/private` | F18 L63, L311 |

## 4. 필수 조건과 선택 조건

필수: 2의 다섯 성질. 서브계정, 교차 증거금, 통합 계좌, 단방향, USDT 정산의 확인. 키 거부의 비재시도. 공인 IP 확인.
선택: 묶이지 않은 키 허용 — 하지 않음(아래).

## 5–6. 대안과 비교

| 결정 | 대안 | 고른 것과 이유 |
|---|---|---|
| 비밀의 위치 | 환경 변수(Hyperliquid 방식), 설정 파일 안, 별도 파일 | 별도 파일(0600, 데이터 디렉터리 밖, 절대 경로). 설정 파일은 운영 문서와 함께 복사·공유되고, 환경 변수는 `/proc/<pid>/environ`과 자식 프로세스로 퍼진다. 파일은 권한을 검사할 수 있다 |
| 질의 문자열 | 인코딩해서 보냄, 그대로 보냄 | 그대로 보내고 뜻을 바꾸는 문자는 거절. 거래소가 주는 커서는 이미 인코딩되어 있어 다시 인코딩하면 커서가 깨진다(SDK도 그대로 보낸다) |
| 서명 시험값 | 문서 예시, 직접 계산 | 문서의 서명할 문자열(조립 확인) + RFC 4231 사례 2(HMAC 자체) + Python `hmac`으로 따로 계산한 값(같은 문자열, 정한 비밀). 문서 예시의 서명은 비밀이 없어 쓸 수 없다 |
| 키 거부 코드 | 범위에 적힌 셋(10003, 10010, 33004), 인증 계열 전부 | 여섯(+10004, 10005, 10007). 서명 오류·권한 없음·인증 실패도 다시 보내서 낫지 않는다(판단) |
| 묶이지 않은 키 | 허용, 거절 | 거절. 90일 뒤 무효가 되고, IP 확인이 의미가 없다 |
| IP 불일치 뒤 | 다음 확인이 맞으면 풂, 재시작까지 막음 | 재시작까지 막음. 범위가 "키 거부처럼"이라고 했고, 키 거부는 다시 시도하지 않는다 |
| 확인 실패 시 | 노드를 멈춤, 게이트웨이만 막음 | 게이트웨이만 막음(`Block`): 연결이 올라오지 않아 코어의 LinkDown 보류가 새 주문을 막는다. 저널·상태 API·대시보드는 계속 돈다. 막힌 이유를 상태 API에 싣는 것은 U16d |

## 7. 선택과 이유 (U16b)

`crates/oms-node/src/venue/bybit/`:

| 모듈 | 하는 일 |
|---|---|
| `sign.rs` | REST 서명, 비공개 스트림 인증 서명(`GET/realtime` + expires), 질의 문자열 |
| `rest.rs` | 자격 파일(`Credentials::load`), 서명한 GET·POST, 응답 분류(`classify`, `RestError`) |
| `checks.rs` | 시작 확인(`boot`), IP 재확인(`recheck_ip`), 막힘 상태(`Health`, `Block`) |
| `mod.rs` | 엔드포인트, `cancel_all` |

| 단계 | 입력 | 처리 | 출력 | 실패하면 |
|---|---|---|---|---|
| B1 키 | `GET /v5/user/query-api` | `isMaster = false`, `userID = sub_account_uid`, `readOnly = 0`, `ContractTrade`에 `Order`·`Position` | 키의 `ips` | `Block::Account`, 거부 코드면 `Block::AuthRejected` |
| B2 IP | `GET <ip_echo_url>` | 본문 = 주소. 키의 `ips`에 있어야 한다. `ips`가 비었거나 `*`면 거절 | — | 다름 → `AuthRejected`(재시작까지), 묶이지 않음 → `Account`, 응답 없음 → `Unchecked` |
| B3 계좌 | `GET /v5/account/info` | `marginMode = REGULAR_MARGIN`, `unifiedMarginStatus` 3–6 | — | `Account` |
| B4 종목 | 종목마다 `instruments-info`, `position/list`(category linear, symbol) | `settleCoin = USDT`, 모든 행 `positionIdx = 0`, 행이 없으면 확인 못 함 | — | `Account` 또는 `Unchecked` |

설정(`[venue] kind = "bybit"`): `network`, `credentials_file`, `sub_account_uid`, `ip_echo_url`(https), `ip_check_secs`(60, 10 이상), `recv_window_ms`(5000, 1000–20000), `request_timeout_ms`, `mainnet_confirmed`, `fills_lag_ms`. 검증:
- mainnet은 `mainnet_confirmed`와 `[core.policy.margin]`이 있어야 한다(Hyperliquid와 같다).
- `[core.policy]`는 `venue_enforces_expiry = true`, `absence_not_found_needed = 2`(ADR-024).
- 증거금 정책이 있으면 `isolated = false`(교차 계좌).

## 8. 포기한 장점, 남은 위험, 가정

- 위험: 교차 계좌에는 유지증거금 여유 규칙(C-MARGIN)이 걸리지 않는다(격리 포지션에만 있다). 주문을 보내는 U17 전에 정할 일이다(소유자 질문).
- 위험: query-api에서 묶이지 않은 키의 `ips` 모양은 문서에 없다. 빈 목록과 `*`를 둘 다 묶이지 않음으로 본다. 다른 모양(예: `""` 한 칸)은 주소로 읽히지 않아 IP 불일치로 막힌다(안전한 쪽).
- 위험: 헤지 모드 종목이 행을 둘 주는지는 문서에 없다. 어느 행이든 0이 아니면 막는다.
- 위험: 서버가 질의 순서를 요구하는지 모른다(정렬해서 보낸다). 실제 응답은 testnet에서 확인한다.
- 위험: IP 확인은 확인용 서비스를 믿는다. https만 받는다.
- 가정: `REGULAR_MARGIN`이 교차 증거금이다(F18 §6).
- 확인하지 않음: 실제 Bybit 응답(이 단위의 시험은 loopback 가짜 서버만 쓴다).

## 9. 근거

- 시험(U16b, Linux PC): `venue::bybit::{sign,rest,checks}`와 `config::tests`, 18개. 문서의 서명할 문자열, RFC 4231, Python 값, 보낸 바이트에 대한 서명(GET 질의, POST 본문), 거부 코드마다 요청 한 번, 응답 종류별 분류, 자격 파일 권한과 비밀 비출력, 시작 확인이 모두 맞을 때 읽기 요청 넷, 어긋나는 사실 12가지 각각, 묶이지 않은 IP, IP 변경과 풀리지 않는 막힘, 설정 거절 12가지.
- 결함 10가지를 일부러 넣어 시험이 모두 잡았다: 거부 코드 분류, 서명 대상, 마스터 키, IP 미바인딩, 헤지 모드, 정산 코인, 막힘 해제, 자격 파일 권한, ADR-024 값, https.

## 10. 재검토 조건

- testnet(U-01 허가 뒤)에서 응답 모양이 다를 때.
- 거래소가 포지션 모드를 읽는 엔드포인트를 낼 때.
- 교차 계좌용 증거금 규칙이 정해질 때(U17 전).
