# ADR-028 Bybit 게이트웨이

상태: 만드는 중. U16b: 설정, 서명, REST 클라이언트, 시작 확인, IP 확인, cancel-all. U16c(이 판): 조회 일곱 가지와 실행 분류표, 증거금 보고. U16d: 비공개 스트림과 노드 연결. 주문 전송은 U17. 노드는 U16d 전까지 `kind = "bybit"`로 시작하지 않는다.

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
| 막힘 상태 | 칸 하나(마지막 결과가 덮어씀), 출처마다 칸 | 출처마다 칸: 시작 확인, IP 확인, 키 거부. 연결은 세 칸이 모두 빌 때만 올라온다. 각 출처는 제 칸만 비운다. 칸이 하나면 IP 재확인 통과가 시작 확인의 막힘(헤지 모드, 격리, 확인 못 함)을 지운다(독립 검토가 찾음). 키 거부는 어느 출처에서 오든 키 거부 칸으로 가고 재시작까지 남는다 |
| HTTP 403 | 요청 한도(10006·429와 같음), 따로 | 따로(`IpBanned`). 거래소는 403 뒤 모든 HTTP를 멈추고 10분 이상 기다리라고 한다(F18 L186). 짧은 한도처럼 60초마다 다시 보내면 차단이 끝나지 않는다. 멈춤 시간 상수 `IP_BAN_PAUSE` = 600초. 멈춤을 지키는 반복은 U16d |
| cancel-all 응답 | retCode만 봄, `result.success`도 봄 | `success = "1"`일 때만 성공. 아니면 거부(F20 Q11.4: `"0"` = 실패). 취소 수가 500(한 번의 최대, 넘으면 거래소가 무작위로 500개만 취소, F20 Q11.3)이면 `maybe_more`로 알려 호출자가 다시 보낸다 |
| 나가는 주소 | 시스템 기본, IPv4만 | IPv4만(`LOCAL_ADDRESS`). 확인 서비스와 Bybit가 다른 주소 계열로 닿으면 IP 확인이 거짓으로 막는다. IPv4에 대응된 IPv6 주소(`::ffff:a.b.c.d`)는 IPv4로 비교한다 |
| 확인 실패 시 | 노드를 멈춤, 게이트웨이만 막음 | 게이트웨이만 막음(`Block`): 연결이 올라오지 않아 코어의 LinkDown 보류가 새 주문을 막는다. 저널·상태 API·대시보드는 계속 돈다. 막힌 이유를 상태 API에 싣는 것은 U16d |

## 7. 선택과 이유 (U16b)

`crates/oms-node/src/venue/bybit/`:

| 모듈 | 하는 일 |
|---|---|
| `sign.rs` | REST 서명, 비공개 스트림 인증 서명(`GET/realtime` + expires), 질의 문자열 |
| `rest.rs` | 자격 파일(`Credentials::load`), 서명한 GET·POST, 응답 분류(`classify`, `RestError`) |
| `checks.rs` | 시작 확인(`boot`), IP 재확인(`recheck_ip`), 막힘 상태(`Health`, `Block`) |
| `mod.rs` | 엔드포인트, `cancel_all`(`CancelAll`) |

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
- `credentials_file`은 `..`를 담지 않고, 심볼릭 링크와 상대 데이터 디렉터리를 풀어 비교해도 데이터 디렉터리 밖이어야 한다.
- 자격 파일은 한 번 열어 그 핸들로 검사하고 읽는다: 일반 파일, 심볼릭 링크 아님(`O_NOFOLLOW`), OMS를 돌리는 사용자 소유, 그룹·기타 권한 없음.

## 7b. 조회와 실행 분류표 (U16c)

`venue/bybit/query.rs`(`Queries::answer`가 `QueryReq` 하나에 입력 목록 `Answer`를 돌려준다. 노드 연결은 U16d), `venue/bybit/exec.rs`(실행 행 하나 → 입력 하나, 스트림도 같은 함수를 쓴다).

| 조회 | 요청 | 처리 | 출력 | 못 읽으면 |
|---|---|---|---|---|
| Status | `GET /v5/order/realtime`, 없으면 `GET /v5/order/history` 두 번: 기간 없이(최근 7일)와 코어가 주문을 받은 시각 − 1분부터 7일(category linear, settleCoin USDT, 우리 `orderLinkId`) | 우리 `orderLinkId`의 행만 본다. 상태표: `New`·`PartiallyFilled` = 열림, `Filled`, `Cancelled`(사유 `cancelType`), `Rejected`(사유 `rejectReason`). `cum` = `cumExecQty`, 시각 = `updatedTime` | `OrderStatus` 또는 셋 다 없을 때 `StatusNotFound`(첫 요청 전 시각) | 입력 없음 |
| Fills | `GET /v5/execution/list`(linear, USDT, limit 100, cursor) | `since` − 2분부터 지금 − `fills_lag_ms`까지 7일 이하 창(2분은 늦게 실리는 행을 다음 조회가 다시 읽게 한다. 같은 키는 코어가 거른다). 창 시작은 1ms 앞에서(경계 포함 여부가 문서에 없다). 행마다 분류표, 같은 키는 한 번 | 창마다 실행 입력들 + `FillsSynced { up_to: 창 끝 − 1µs }` | 그 창부터 멈춤(앞 창의 선언은 남는다) |
| OpenOrders | `GET /v5/order/realtime`(linear, USDT, limit 50, 모든 쪽) | 열린 상태의 행만. `remaining` = `leavesQty` | `OpenOrders` | 입력 없음 |
| Positions | `GET /v5/position/list`(linear, USDT, limit 200, 모든 쪽) | 단방향 행만, 수량 = `side` 부호 × `size` | `Positions` | 입력 없음 |
| Margin | `wallet-balance`(UNIFIED), `account/info`, `position/list`(모든 쪽), 처음 보는 종목은 `instruments-info` | 계좌 가치 `totalEquity`, 포지션마다 `positionValue`, 증거금 `positionIM`, 레버리지 정수, 증거금 모드 = 계좌의 `marginMode`, `maxLeverage` | `MarginReport` | `MarginReportInvalid`(응답 없음이면 입력 없음) |
| VenueSetting | `account/info`, `position/list`(symbol) | 행 하나, 단방향, 그 종목. 격리 = `ISOLATED_MARGIN` | `VenueSetting` | 입력 없음 |
| AccountMode | `account/info` | `marginMode` 그대로(64자 이하) | `AccountMode` | 입력 없음 |

실행 분류표(`exec::classify`):

| `execType` | 입력 |
|---|---|
| `Trade` | `Fill`. `orderLinkId`가 이 인스턴스의 `Base32Pair`이면 그 주문, 아니면 `order_id` 없음(외부 체결, 코어는 `orderId`로도 찾는다). `fee` = `execFee`, `is_maker` = `isMaker`, 조회 행의 `recv_ns`는 비움 |
| `Funding` | `Funding`. 금액 = `execFee`(양수 = 계좌가 냄, F20 Q9.5) |
| `AdlTrade`, `BustTrade`, `Delivery`, `Settle` | `PositionChange`(adl, takeover, delivery, settle). 수량 = `side` 부호 × `execQty`, 가격 = `execPrice` |
| 그 밖, 없음 | `UnclassifiedExecution`(종류 32자, 행의 주요 값) |

체결 키는 `<symbol>:<execId>`. 행을 읽을 수 없으면(키·종목·시각·수량·가격·수수료) 입력이 아니라 오류다: 조회는 그 창을 완료로 선언하지 않고, 스트림(U16d)은 끊고 다시 붙는다(G-9).

| 결정 | 대안 | 고른 것과 이유 |
|---|---|---|
| 마지막 쪽 | 빈 커서 = 끝, 빈 커서 + 덜 찬 쪽 = 끝 | 둘 다일 때만 끝. 빈 커서의 뜻은 이 엔드포인트들의 문서에 없다(F20 Q9.9). 다 찬 쪽에 커서가 없으면 스냅숏은 넘기지 않고, 체결 창은 반으로 나눠 다시 읽는다 |
| 상태 조회의 정산 범위 | 주문의 종목, `settleCoin` | `settleCoin=USDT`. linear는 셋 중 하나가 필요하고(F20 Q10.4), `orderLinkId`가 우선한다. `QueryReq::Status`에 종목이 없다 |
| `cum` | 비움(Hyperliquid), 채움 | 채움. `cumExecQty`의 뜻이 문서에 하나다(Hyperliquid의 `sz`는 두 뜻으로 적혀 있어 비웠다). 코어는 체결 합이 그 값과 같으면 닫고, 모자라면 체결 동기화 뒤 `MissingFills` 불일치를 낸다. 거래소가 종결 상태를 최종 수량보다 먼저 낮은 값으로 말하면 일찍 닫힌다(가정: 종결 상태의 `cumExecQty`는 최종이다) |
| 조건부·스팟 상태 | 열림·닫힘으로 번역, 결론 없음 | 결론 없음(`Untriggered`, `Triggered`, `Deactivated`, `PartiallyFilledCanceled`). OMS는 그런 주문을 내지 않는다(G-13). 열린 주문 목록에는 `Untriggered`도 넣는다(남의 주문이라도 걸려 있다) |
| 계좌 가치 | `totalEquity`, `totalMarginBalance` | `totalEquity`(모든 자산의 USD 합). 하루 손실 규칙이 보는 계좌 전체 값이다(판단) |
| 포지션의 증거금 모드 | 포지션 필드, 계좌 모드 | 계좌 모드(`account/info`). 통합 계좌는 모드가 계좌 하나에 하나이고 포지션의 `tradeMode`는 폐기되었다(F18 §6). 그래서 증거금 보고마다 요청이 셋이다 |
| 소수 레버리지 | 내림, 거절 | 거절. 보고 전체가 `MarginReportInvalid`, 설정은 모름으로 남는다(코어가 일반 주문을 막는 쪽). 문서는 0.01 단위를 허용하는 것으로 보인다(F20 Q8.5) |
| `maxLeverage` | 보고마다 읽음, 프로세스당 한 번 | 한 번(종목마다 캐시). 유지증거금 규칙은 격리 포지션에만 쓰고 이 계좌는 교차다. 보고에는 0이 아닌 값이 있어야 한다 |
| 계좌 모드 허용 목록 | 아무 값, `REGULAR_MARGIN`만 | 설정 검증이 `REGULAR_MARGIN` 밖의 값을 거절한다 |
| 주문 내역의 기간 | 기간 없이, 접수 시각부터 | 둘 다. 기간이 없으면 최근 7일만 답한다(F20 Q10.7). 오래 꺼져 있다 켜면 체결된 옛 주문이 realtime(닫힌 주문 500건, 거래소 재시작 때 비움)과 최근 7일 내역 모두에 없어 `StatusNotFound`가 되고, 코어가 `NotPlaced`로 닫아 예약을 일찍 푼다(독립 검토가 찾음). 그래서 코어가 `Need::OrderStatus`와 `QueryReq::Status`에 주문을 받은 시각(`accepted_at`)을 싣고, 게이트웨이는 그 시각 − 1분부터 7일을 따로 묻는다. 기간이 생성 시각을 거르는지 갱신 시각을 거르는지는 문서에 없어 두 요청을 다 한다 |
| 체결 다시 읽기 | `since`부터, 앞으로 당겨서 | `since` − 2분부터(판단). 끝을 지금 − 2초로만 잡으면 거래소가 늦게 싣는 행(펀딩·ADL·외부 체결)은 `FillsSynced` 뒤에 나타나 다시 읽히지 않는다. 우리 주문은 `cum` 검사가 `MissingFills`로 드러내지만 나머지는 아무것도 잡지 않는다(독립 검토가 찾음). 2분보다 늦은 행은 여전히 놓친다 |
| 체결 한 번의 일 | 끝까지, 쪽 수 상한 | 30쪽. 다 읽은 창은 `FillsSynced`로 선언했으므로 다음 조회가 거기서 잇는다(400일 공백은 여러 번에 나눠 읽는다) |

## 8. 포기한 장점, 남은 위험, 가정

- 위험: 교차 계좌에는 유지증거금 여유 규칙(C-MARGIN)이 걸리지 않는다(격리 포지션에만 있다). 주문을 보내는 U17 전에 정할 일이다(소유자 질문).
- 위험: query-api에서 묶이지 않은 키의 `ips` 모양은 문서에 없다. 빈 목록과 `*`를 둘 다 묶이지 않음으로 본다. 다른 모양(예: `""` 한 칸)은 주소로 읽히지 않아 IP 불일치로 막힌다(안전한 쪽).
- 위험: 헤지 모드 종목이 행을 둘 주는지는 문서에 없다. 어느 행이든 0이 아니면 막는다.
- 위험: 서버가 질의 순서를 요구하는지 모른다(정렬해서 보낸다). 실제 응답은 testnet에서 확인한다.
- 위험: IP 확인은 확인용 서비스를 믿는다. https만 받는다.
- 가정: `REGULAR_MARGIN`이 교차 증거금이다(F18 §6).
- 확인하지 않음: 실제 Bybit 응답(이 단위의 시험은 loopback 가짜 서버만 쓴다).
- 확인하지 않음: 다른 사용자 소유 파일의 거절(시험에 root가 필요하다).
- 위험: `api.bybit.com`이 IPv6로만 닿는 경우는 생각하지 않았다. IPv4만 쓰므로 그때는 연결이 안 되어 막힌다(안전한 쪽).
- 위험(U16c): 거래소 변경(ADL 등) 직후의 포지션 응답이 거래소 쪽에서 늦은 값이면(문서가 변동이 클 때 `position/list`의 지연을 경고한다, F20 Q12.9) 코어가 변경을 되돌리는 `PositionAdjusted`를 쓴다. ADR-023 §8의 가정(응답 시각 기준 일관성)과 같은 위험이고 감시자 W2가 다시 본다. 포지션 행의 `seq`·`updatedTime`으로 변경보다 오래된 응답을 거르는 것은 코어 입력을 바꾸는 일이라 하지 않았다.
- 위험(U16c): 취소·거부된 주문은 주문 내역에 24시간만 남는다(F18 §1.7). 체결 없이 닫힌 주문을 하루 뒤에 물으면 "없음"이 된다. 체결이 없으므로 예약을 푸는 결과는 같지만 열려 있던 주문이면 상태 불일치가 날 수 있다.
- 위험(U16c): `totalEquity`는 담보로 쓰지 않는 자산까지 더한다. 그런 자산이 있으면 계좌 레버리지 상한과 하루 손실이 실제 담보보다 넉넉하게 계산된다. 서브계정에 USDT만 둔다는 운영 가정이다.
- 위험(U16c): 같은 `execTime`의 행은 정렬이 흔들릴 수 있다고 문서가 쓴다. 커서가 그런 행을 건너뛰지 않는다는 보장은 없다(중복은 걸러도 빠진 행은 못 찾는다).
- 위험(U16c): 숫자가 소수 9자리 이상이면(코어는 1e-8 단위) 그 응답은 못 읽은 것이 된다. 증거금 보고면 보고 전체가 무효라 일반 주문이 막힌다(안전한 쪽). 실제 자릿수는 testnet에서 본다.
- 위험(U16c): 같은 밀리초에 쪽 한도(100)의 배수만큼 실행이 있고 거래소가 마지막 다 찬 쪽에 빈 커서를 주면, 1ms 창까지 나눠도 끝을 알 수 없어 동기화가 멈춘다(오류 기록, 주문이 닫히지 않는 쪽).
- 가정(U16c): `execFee`는 Trade에서 양수 = 비용이다. maker 리베이트의 부호는 문서에 없다(F20 Q9.5).
- 가정(U16c): ADL·청산 인수 행의 `side`는 우리 포지션이 움직인 쪽이다(T15 전까지).
- 확인하지 않음(U16c): 펀딩·ADL 행이 실행 목록에 실제로 실리는지, maker 체결에 `orderLinkId`가 있는지(F20 Q9.6, Q14).

## 9. 근거

- 시험(U16b, Linux PC): `venue::bybit::{sign,rest,checks}`, `venue::bybit::tests`와 `config::tests`의 새 시험 19개. 문서의 서명할 문자열, RFC 4231, Python 값, 보낸 바이트에 대한 서명(GET 질의, POST 본문), 거부 코드마다 요청 한 번, 응답 종류별 분류, 자격 파일 권한과 비밀 비출력, 시작 확인이 모두 맞을 때 읽기 요청 넷, 어긋나는 사실 12가지 각각, 묶이지 않은 IP, IP 변경과 풀리지 않는 막힘, 설정 거절 12가지.
- 결함 10가지를 일부러 넣어 시험이 모두 잡았다: 거부 코드 분류, 서명 대상, 마스터 키, IP 미바인딩, 헤지 모드, 정산 코인, 막힘 해제, 자격 파일 권한, ADR-024 값, https.
- 독립 검토(꼭 고칠 것 1, 고칠 것 5) 반영 뒤 결함 8가지를 더 넣어 모두 잡았다: IP 칸이 시작 칸을 덮음, cancel-all `success` 무시, 403을 한도로 봄, 데이터 디렉터리 글자 비교, 심볼릭 링크 따라감, 일반 파일 아님, IPv4 대응 주소, 500건 표시.
- 시험(U16c, Linux PC): `venue::bybit::exec::tests` 6개, `venue::bybit::query::tests` 16개, `config::tests`의 Bybit 시험에 계좌 모드 거절 추가(주장 V-BYBIT-READ-1). 코어 `Need::OrderStatus`에 `accepted_at`(시나리오 시험 두 곳이 값을 본다).
- 독립 검토(꼭 고칠 것 0, 고칠 것 3, 참고 5): 주문 내역 기간과 체결 다시 읽기는 고쳤고, 늦은 포지션 응답·`totalEquity`·정렬은 위험으로 적었다. 가짜 서버(경로별 응답)로 보낸 요청의 경로·질의·순서와 만든 입력 전체를 본다.

## 10. 재검토 조건

- testnet(U-01 허가 뒤)에서 응답 모양이 다를 때.
- 거래소가 포지션 모드를 읽는 엔드포인트를 낼 때.
- 교차 계좌용 증거금 규칙이 정해질 때(U17 전).
