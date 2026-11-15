# Hyperliquid 프로토콜 계약 조사 (OMS 베뉴 어댑터용 1차 자료 조사)

- 조사일(모든 URL의 접근일): **2026-10-02**
- 대상: Hyperliquid perpetuals (testnet 우선), Rust OMS의 멱등성·복구·대사(reconciliation) 설계 근거
- 원칙: 이 문서의 모든 주장은 이번 세션에서 실제로 내려받아 읽은 페이지/소스에만 근거한다. 공식 자료에 없으면 **"문서에 명시 없음"** 으로 적는다. 인용은 원문(영어) 그대로이며 30단어 이하로 잘랐다.
- 인용 표기 규칙: 산문 인용은 글자 그대로다 (마크다운 링크 문법과 GitBook 의 `&#x20;` 만 제거). 코드·JSON·표 인용은 토큰은 원문 그대로 두되, 여러 줄을 한 줄로 잇거나 중간 주석·후행 쉼표·줄바꿈 태그를 생략한 경우가 있다. A–L 절의 산문 인용 225건과 코드 인용 103건을 내려받은 원문과 스크립트로 대조했다. 불일치로 표시된 21건은 모두 수작업으로 확인했고, 위 축약 규칙에 해당하거나(줄 이음, 주석·링크 문법 생략) GitHub 이슈·PR 본문과 실측 응답처럼 대조 말뭉치 밖의 인용이었다.
- 확신도 표기
  - **문서 명시**: 공식 GitBook 문서에 문장/표/예시로 적혀 있음
  - **SDK 구현에서 확인**: 공식 SDK 소스 코드에서만 확인됨 (문서에는 없음)
  - **문서에 명시 없음**: 내려받은 공식 문서 54쪽과 SDK 어디에도 없음
  - **2차 자료**: 공식 문서/공식 SDK 소스가 아닌 자료 (이슈 트래커 글, 커뮤니티 SDK 등)
  - **실측(비인증)**: 공개 testnet `info` 엔드포인트에 서명 없이 보낸 읽기 전용 요청의 실제 응답. 문서 근거가 아니므로 별도 표기
  - **추론**: 문서에 적힌 규칙들을 결합한 논리적 귀결. 문서가 직접 말한 것이 아니므로 실험 검증 대상

---

## 1. 조사 범위와 한계

### 1.1 수집 방법

- GitBook은 페이지 URL 뒤에 `.md` 를 붙이면 원문 마크다운을 내려준다 (각 페이지 첫 줄 안내문: "Markdown versions of documentation pages are available by appending `.md` to page URLs"). 요약 모델을 거치지 않고 `curl` 로 원문을 받아 직접 읽었다. 색인은 `llms.txt` 를 사용했다.
- 54개 페이지 모두 HTTP 200. **수집 실패 없음.**
- **버전/갱신 표식**: GitBook에는 버전 번호가 없다. `.md` 응답에는 "last updated" 표식이 없고 HTTP `Last-Modified` 헤더도 없었다. 따라서 "2026-10-02 시점의 스냅샷" 이라는 것 외에 문서 판(edition)을 특정할 수 없다. 문서 스스로도 현재 API를 v0 이라고 부른다: "The current v0 API currently uses some nonstandard notation. Relevant standardization will be batched into a breaking v1 API change." ([D-NOT])
- 재현용 지문(내려받은 `.md` 의 sha256 앞 16자리): exchange-endpoint `6df658a502e587db`, info-endpoint `f4228fc7734af5db`, rate-limits `0e94f9c44edfae68`, nonces `e63b1672b6d05ddd`.

### 1.2 공식 문서 (기준 URL: `https://hyperliquid.gitbook.io/hyperliquid-docs`)

| ID | 제목 | URL | 열람 범위 |
|---|---|---|---|
| [D-IDX] | llms.txt (문서 색인) | https://hyperliquid.gitbook.io/hyperliquid-docs/llms.txt | 전체 |
| [D-API] | API | https://hyperliquid.gitbook.io/hyperliquid-docs/for-developers/api | 전체 |
| [D-NOT] | Notation | https://hyperliquid.gitbook.io/hyperliquid-docs/for-developers/api/notation | 전체 |
| [D-AID] | Asset IDs | https://hyperliquid.gitbook.io/hyperliquid-docs/for-developers/api/asset-ids | 전체 |
| [D-TICK] | Tick and lot size | https://hyperliquid.gitbook.io/hyperliquid-docs/for-developers/api/tick-and-lot-size | 전체 |
| [D-NONCE] | Nonces and API wallets | https://hyperliquid.gitbook.io/hyperliquid-docs/for-developers/api/nonces-and-api-wallets | 전체 |
| [D-INFO] | Info endpoint | https://hyperliquid.gitbook.io/hyperliquid-docs/for-developers/api/info-endpoint | **부분**: 서두, openOrders, frontendOpenOrders, userFills, userFillsByTime, userRateLimit, orderStatus, historicalOrders 만 정독. vault/staking/borrow-lend/referral/portfolio 절은 읽지 않음 |
| [D-PERP] | Info endpoint → Perpetuals | https://hyperliquid.gitbook.io/hyperliquid-docs/for-developers/api/info-endpoint/perpetuals | **부분**: perpDexs, meta, metaAndAssetCtxs, clearinghouseState, userFunding/userNonFundingLedgerUpdates, perpsAtOpenInterestCap, activeAssetData, perpDexLimits, perpDexStatus. 나머지(funding history, annotation 등)는 읽지 않음 |
| — | Info endpoint → Spot | https://hyperliquid.gitbook.io/hyperliquid-docs/for-developers/api/info-endpoint/spot | 내려받기만 함 (perp 대상이라 읽지 않음) |
| [D-EX] | Exchange endpoint | https://hyperliquid.gitbook.io/hyperliquid-docs/for-developers/api/exchange-endpoint | **부분**: 서두(Asset/Subaccounts/Expires After), order, cancel, cancelByCloid, scheduleCancel, modify, batchModify, updateLeverage, updateIsolatedMargin, approveAgent, approveBuilderFee, twapOrder/twapCancel, trailingStop, reserveRequestWeight, noop. 이체/스테이킹/HIP-3/outcome 절은 읽지 않음 |
| [D-WS] | Websocket | https://hyperliquid.gitbook.io/hyperliquid-docs/for-developers/api/websocket | 전체 |
| [D-SUB] | Websocket → Subscriptions | https://hyperliquid.gitbook.io/hyperliquid-docs/for-developers/api/websocket/subscriptions | 전체 |
| [D-POST] | Websocket → Post requests | https://hyperliquid.gitbook.io/hyperliquid-docs/for-developers/api/websocket/post-requests | 전체 |
| [D-HB] | Websocket → Timeouts and heartbeats | https://hyperliquid.gitbook.io/hyperliquid-docs/for-developers/api/websocket/timeouts-and-heartbeats | 전체 |
| [D-ERR] | Error responses | https://hyperliquid.gitbook.io/hyperliquid-docs/for-developers/api/error-responses | 전체 |
| [D-SIGN] | Signing | https://hyperliquid.gitbook.io/hyperliquid-docs/for-developers/api/signing | 전체 |
| [D-RATE] | Rate limits and user limits | https://hyperliquid.gitbook.io/hyperliquid-docs/for-developers/api/rate-limits-and-user-limits | 전체 |
| [D-ACT] | Activation gas fee | https://hyperliquid.gitbook.io/hyperliquid-docs/for-developers/api/activation-gas-fee | 전체 |
| [D-LAT] | Optimizing latency | https://hyperliquid.gitbook.io/hyperliquid-docs/for-developers/api/optimizing-latency | 전체 |
| [D-PRIO] | Priority fees | https://hyperliquid.gitbook.io/hyperliquid-docs/for-developers/api/priority-fees | 전체 |
| [D-L1] | Nodes → L1 data schemas | https://hyperliquid.gitbook.io/hyperliquid-docs/for-developers/nodes/l1-data-schemas | 전체 |
| [D-FEE] | Trading → Fees | https://hyperliquid.gitbook.io/hyperliquid-docs/trading/fees | 전체 |
| [D-SUBACC] | Trading → Sub-accounts | https://hyperliquid.gitbook.io/hyperliquid-docs/trading/sub-accounts | 전체 |
| [D-BLD] | Trading → Builder codes | https://hyperliquid.gitbook.io/hyperliquid-docs/trading/builder-codes | 전체 |
| [D-SPEC] | Trading → Contract specifications | https://hyperliquid.gitbook.io/hyperliquid-docs/trading/contract-specifications | 전체 |
| [D-MARGIN] | Trading → Margining | https://hyperliquid.gitbook.io/hyperliquid-docs/trading/margining | 전체 |
| [D-AAM] | Trading → Account abstraction modes | https://hyperliquid.gitbook.io/hyperliquid-docs/trading/account-abstraction-modes | 전체 |
| — | Trading → Portfolio margin | https://hyperliquid.gitbook.io/hyperliquid-docs/trading/portfolio-margin | 키워드 검색만 |
| [D-TIER] | Trading → Margin tiers | https://hyperliquid.gitbook.io/hyperliquid-docs/trading/margin-tiers | 전체 |
| [D-PX] | Trading → Robust price indices | https://hyperliquid.gitbook.io/hyperliquid-docs/trading/robust-price-indices | 전체 |
| [D-LIQ] | Trading → Liquidations | https://hyperliquid.gitbook.io/hyperliquid-docs/trading/liquidations | 전체 |
| [D-ADL] | Trading → Auto-deleveraging | https://hyperliquid.gitbook.io/hyperliquid-docs/trading/auto-deleveraging | 전체 |
| [D-FUND] | Trading → Funding | https://hyperliquid.gitbook.io/hyperliquid-docs/trading/funding | 전체 |
| [D-TOB] | Trading → Order book | https://hyperliquid.gitbook.io/hyperliquid-docs/trading/order-book | 전체 |
| [D-OT] | Trading → Order types | https://hyperliquid.gitbook.io/hyperliquid-docs/trading/order-types | 전체 |
| [D-TPSL] | Trading → Take profit and stop loss orders | https://hyperliquid.gitbook.io/hyperliquid-docs/trading/take-profit-and-stop-loss-orders-tp-sl | 전체 |
| [D-PNL] | Trading → Entry price and pnl | https://hyperliquid.gitbook.io/hyperliquid-docs/trading/entry-price-and-pnl | 전체 |
| [D-STP] | Trading → Self-trade prevention | https://hyperliquid.gitbook.io/hyperliquid-docs/trading/self-trade-prevention | 전체 |
| [D-DELIST] | Trading → Delisting | https://hyperliquid.gitbook.io/hyperliquid-docs/trading/delisting | 전체 |
| [D-MM] | Trading → Market making | https://hyperliquid.gitbook.io/hyperliquid-docs/trading/market-making | 전체 |
| — | Trading → Perpetual assets / Hyperps / Miscellaneous UI | `.../trading/perpetual-assets`, `.../trading/hyperps`, `.../trading/miscellaneous-ui` | 앞 둘은 내려받기만, Misc UI 는 전체 (관련 내용 없음) |
| [D-FAUCET] | Onboarding → Testnet faucet | https://hyperliquid.gitbook.io/hyperliquid-docs/onboarding/testnet-faucet | 전체 |
| [D-APISRV] | HyperCore → API servers | https://hyperliquid.gitbook.io/hyperliquid-docs/hypercore/api-servers | 전체 |
| [D-HOB] | HyperCore → Order book | https://hyperliquid.gitbook.io/hyperliquid-docs/hypercore/order-book | 전체 |
| [D-CH] | HyperCore → Clearinghouse | https://hyperliquid.gitbook.io/hyperliquid-docs/hypercore/clearinghouse | 전체 |
| [D-OV] | HyperCore → Overview | https://hyperliquid.gitbook.io/hyperliquid-docs/hypercore/overview | 전체 |
| — | HyperCore → Oracle / Multi-sig | `.../hypercore/oracle`, `.../hypercore/multi-sig` | 내려받기 + 키워드 검색만 |
| [D-HIST] | Historical data | https://hyperliquid.gitbook.io/hyperliquid-docs/historical-data | 전체 |
| [D-FAQ-EXP] | Support → What does "Action already expired" mean? | https://hyperliquid.gitbook.io/hyperliquid-docs/support/faq/trade-outcome-looks-incorrect/what-does-action-already-expired-mean | 전체 |
| [D-FAQ-API] | Support → I have API related questions | https://hyperliquid.gitbook.io/hyperliquid-docs/support/faq/building-on-hyperliquid/i-have-api-related-questions | 전체 |
| — | Support → My TP/SL did not execute correctly / Why was I liquidated? | `.../support/faq/trade-outcome-looks-incorrect/...` | 내려받기만 |

### 1.3 공식 SDK (구현 증거)

| ID | 저장소 / 경로 | 본 커밋 | 비고 |
|---|---|---|---|
| [S-PY] | https://github.com/hyperliquid-dex/hyperliquid-python-sdk | `2fdb18f9517675ea03695a0962bd19eece9c83f0` (2026-06-04, "Support multisig userSetAbstraction (#299)") | MIT. `pyproject.toml` version 0.24.0. GitHub API: archived=false, pushed_at 2026-06-04 |
| [S-PY-SIGN] | [S-PY] `hyperliquid/utils/signing.py` | 동일 | action_hash, phantom agent, EIP-712, float_to_wire, order wire |
| [S-PY-EX] | [S-PY] `hyperliquid/exchange.py` | 동일 | order/bulk_orders/market_open/cancel/modify/schedule_cancel/noop |
| [S-PY-INFO] | [S-PY] `hyperliquid/info.py` | 동일 | orderStatus, userFills 등 |
| [S-PY-TYPES] | [S-PY] `hyperliquid/utils/types.py` | 동일 | Cloid 검증, WS 메시지 타입 |
| [S-PY-WS] | [S-PY] `hyperliquid/websocket_manager.py` | 동일 | ping 주기, 구독 식별자 |
| [S-PY-API] | [S-PY] `hyperliquid/api.py`, `utils/error.py`, `utils/constants.py` | 동일 | HTTP 오류 처리, URL 상수 |
| [S-PY-EXM] | [S-PY] `examples/basic_adding.py`, `basic_order_with_cloid.py`, `rounding.py`, `basic_order_modify.py`, `basic_schedule_cancel.py`, `priority_fees.py`; `tests/signing_test.py` | 동일 | 사용 예와 서명 테스트 벡터 |
| [S-RS] | https://github.com/hyperliquid-dex/hyperliquid-rust-sdk | `aac75585daf12d0a3761126cc7da7a5e035b5853` (2025-10-20, "Fix: accept null premium/impact_pxs in AssetContext (#133)") | **MIT** (`Cargo.toml` license = "MIT", `LICENSE.md` Copyright (c) 2024 Hyperliquid Labs Pte. Ltd.). crate version 0.6.0. 커밋 116개 |
| [S-RS-*] | [S-RS] `src/signature/agent.rs`, `src/signature/create_signature.rs`, `src/exchange/exchange_client.rs`, `src/exchange/order.rs`, `cancel.rs`, `modify.rs`, `exchange_responses.rs`, `src/helpers.rs`, `src/req.rs`, `src/errors.rs`, `src/ws/sub_structs.rs`, `src/ws/ws_manager.rs`, `src/info/response_structs.rs`, `sub_structs.rs` | 동일 | |

**Rust SDK 유지보수 상태 (Rust OMS에 직접 영향)**

- GitHub API 조회 결과: `archived=false`, 마지막 push **2025-10-21**, open issue/PR 59건. 즉 조사일 기준 약 11개월간 `master` 에 커밋이 없다.
- 공식 문서의 API 페이지는 더 이상 이 저장소를 Rust SDK로 안내하지 않는다. 대신: "Rust SDK written by community member, Infinite Field: <https://github.com/infinitefield/hypersdk>" ([D-API]). 공식 조직의 `hyperliquid-rust-sdk` 는 그 페이지에 언급되지 않는다.
- [S-RS] 에는 문서에 있는 기능이 빠져 있다 (grep 결과 0건): `expiresAfter` 해싱, `noop` 액션, cancel 의 `f`(fast), modify 의 `a`(always_place). `Actions::hash` 는 `rmp_serde::to_vec_named(self)` + nonce + vault 까지만 넣는다.
- 미병합 PR [S-RS-PR198](2차 자료): `next_nonce()` 가 동시 호출자에게 같은 nonce 를 줄 수 있다는 보고. C절 참고.

### 1.4 2차 자료 / 실측

| ID | 내용 | URL | 성격 |
|---|---|---|---|
| [X-HSDK] | infinitefield/hypersdk (공식 문서가 링크한 커뮤니티 Rust SDK). 커밋 `940e909e2b5b95071b9cdbb1763813a2cb268ea6` (2026-09-30), v0.2.18, **MPL-2.0** | https://github.com/infinitefield/hypersdk | 2차 자료. README 와 일부 소스(`src/hypercore/http.rs`, `signing.rs`, `types/mod.rs`)를 grep 수준으로만 확인 |
| [X-ISS253] | python-sdk issue #253 "Info.query_order_by_oid returns unknownOid sometimes" (open, 2025-12-12) 와 PR #268 (open, 미병합) | https://github.com/hyperliquid-dex/hyperliquid-python-sdk/issues/253 | 2차 자료 (사용자 보고, 공식 답변 없음) |
| [S-RS-PR198] | rust-sdk PR #198 "fix: hand out unique nonces when the counter catches up" (open, 2026-08-28) | https://github.com/hyperliquid-dex/hyperliquid-rust-sdk/pull/198 | 2차 자료 |
| [M-TESTNET] | `https://api.hyperliquid-testnet.xyz/info`, `/exchange` 에 서명 없는 요청 8건 (`orderStatus` 3, 잘못된 본문 3, `userRateLimit` 1, `meta` 1; 그중 1건은 연결 리셋 후 재시도) | — | 실측(비인증). 계정·키 미사용 |

### 1.5 접근하지 못했거나 하지 않은 것

- **Discord `#api-traders`, Telegram `https://t.me/hyperliquid_api`**: 문서가 API 질문과 공지 채널로 지정한 곳이다 ("API announcements: <https://t.me/hyperliquid_api>" [D-FAQ-API]). 열람하지 않았다. 문서에 없는 동작(예: cloid 중복 처리)이 여기에만 공지되었을 가능성이 있다.
- `hyperliquid-dex/node`, `hyperliquid-dex/order_book_server` 저장소: 읽지 않음.
- 웹 검색 1회 시도: 결과 요약이 다른 프로토콜(Tempo TIP-1003)의 clientOrderId 규칙을 Hyperliquid 것처럼 섞어 반환했다. **근거로 쓰지 않았다.**
- 실측 요청 중 첫 1건이 `Connection reset by peer` 로 실패, 재시도에서 성공.
- 로컬 Go 어댑터(`hyperliquid-trading/internal/ems/hyperliquid/*.go`)와 `docs/research/ack-diagnosis.md` 는 교차 확인용으로만 읽었다. 1차 자료가 아니다. `.env` 는 읽지 않았다.

---

## 2. 사실 조사 (A–L)

### A. 주문 제출 (`order` 액션)

| 주장 | 출처 | 인용 | 확신도 |
|---|---|---|---|
| 엔드포인트는 `POST /exchange`, 본문은 `action`, `nonce`, `signature` 필수 + `vaultAddress`, `expiresAfter` 선택 | [D-EX] | nonce: "Recommended to use the current timestamp in milliseconds" | 문서 명시 |
| 주문 와이어 필드는 `a,b,p,s,r,t,c` | [D-EX] | "a is asset / b is isBuy / p is price / s is size / r is reduceOnly / t is type / c is cloid (client order id)" (원문은 줄바꿈 구분) | 문서 명시 |
| `p`, `s` 는 문자열, `a` 는 숫자, `b`,`r` 은 불리언 | [D-EX] | `"a": Number, "b": Boolean, "p": String, "s": String, "r": Boolean` | 문서 명시 |
| perp 의 asset 은 `meta.universe` 내 인덱스 | [D-EX] | "For perpetuals this is the index in the `universe` field returned by the`meta` response." | 문서 명시 |
| TIF 값은 `Alo`, `Ioc`, `Gtc` 세 가지 (와이어 표기) | [D-EX] | `"tif": "Alo" \| "Ioc" \| "Gtc"` | 문서 명시 |
| TIF 는 주문이 처음 호가창에 닿을 때의 동작만 정한다 | [D-EX] | "For limit orders, TIF (time-in-force) sets the behavior of the order upon first hitting the book." | 문서 명시 |
| ALO: 즉시 체결될 주문이면 체결 대신 취소 | [D-EX] | "ALO (add liquidity only, i.e. \"post only\") will be canceled instead of immediately matching." | 문서 명시 |
| IOC: 미체결 잔량은 resting 하지 않고 취소 | [D-EX] | "IOC (immediate or cancel) will have the unfilled part canceled instead of resting." | 문서 명시 |
| GTC: 특별 동작 없음 | [D-EX] | "GTC (good til canceled) orders have no special behavior." | 문서 명시 |
| FOK 등 다른 TIF | — | — | 문서에 명시 없음 (와이어 스펙에 3종만 있음) |
| **"시장가" 는 와이어에 별도 타입이 없다.** 일반 주문에서는 공격적 가격의 IOC limit 으로 표현한다 | [S-PY-EX] | `# Market Order is an aggressive Limit Order IoC` (기본 슬리피지 `DEFAULT_SLIPPAGE = 0.05`) | SDK 구현에서 확인 |
| Rust SDK 도 동일 (`market_open` 이 `tif: "Ioc"` limit 생성, 기본 5%) | [S-RS] `exchange_client.rs` | `let slippage = params.slippage.unwrap_or(0.05); // Default 5% slippage` | SDK 구현에서 확인 |
| 트리거 주문에서는 `isMarket` 으로 시장가 여부를 표현 | [D-EX] | `"trigger": { "isMarket": Boolean, "triggerPx": String, "tpsl": "tp" \| "sl" }` | 문서 명시 |
| 조회 응답에는 `"orderType": "Market"`, `"tif": "FrontendMarket"` 가 나올 수 있다 (와이어 스펙에 없는 TIF 값이 응답 예시에 존재) | [D-INFO] | `"orderType": "Market", "origSz": "0.0076", "tif": "FrontendMarket"` | 문서 명시 (예시에만 등장, 의미 설명 없음) |
| TP/SL 시장가는 슬리피지 10% | [D-TPSL] | "TP/SL market orders have a slippage tolerance of 10%." | 문서 명시 |
| 주문 금액 상한 | [D-SPEC] | "Maximum market order value \| $30,000,000 for max leverage >= 25, $5,000,000 for max leverage in [20, 25), $2,000,000 for max leverage in [10, 20), otherwise $500,000" | 문서 명시 |
| limit 주문 금액 상한은 그 10배 | [D-SPEC] | "Maximum limit order value \| 10 * maximum market order value" | 문서 명시 |
| reduceOnly 의미 | [D-OT] | "Reduce Only: An order that reduces a current position as opposed to opening a new position in the opposite direction" | 문서 명시 |
| reduceOnly 위반 시 거부 문자열 | [D-ERR] | "Reduce only order would increase position." | 문서 명시 |
| resting 중인 reduce-only 주문은 나중에 `reduceOnlyCanceled` 로 취소될 수 있다 | [D-INFO] | "Canceled reduced-only order that does not reduce position" | 문서 명시 |
| 트리거(TP/SL)는 mark price 로 발동 | [D-TPSL] | "The mark price is used to trigger TP/SL orders." | 문서 명시 |
| reduce-only 가 아닌 TP/SL 은 pre-validation 에서 페이로드 전체가 거부되는 예로 언급됨 | [D-ERR] | "Examples include: empty batch of orders, non-reduce-only TP/SL orders, order too far from reference price, and some forms of tick size validation." | 문서 명시 |
| **cloid 형식**: 128비트 hex 문자열, 선택 필드 | [D-EX] | "Client Order ID (cloid) is an optional 128 bit hex string, e.g. `0x1234567890abcdef1234567890abcdef`" | 문서 명시 |
| SDK 검증: `0x` + hex 32자 (16바이트) | [S-PY-TYPES] | `if not len(self._raw_cloid[2:]) == 32: raise TypeError("cloid is not 16 bytes")` | SDK 구현에서 확인 |
| Rust SDK 는 `Uuid` 를 hex 로 변환해 cloid 로 사용, `None` 이면 필드 자체를 직렬화하지 않음 | [S-RS] `order.rs`, `helpers.rs` | `#[serde(rename = "c", alias = "cloid", skip_serializing_if = "Option::is_none")]` | SDK 구현에서 확인 |
| **cloid 유일성 범위** (계정별? 서명자별? 자산별? 열린 주문 한정? 영구?) | — | — | **문서에 명시 없음** |
| **같은 cloid 를 두 번 제출하면 어떻게 되는가** (거부 여부, 조건, 수명) | — | — | **문서에 명시 없음.** 54개 페이지 전체에서 `duplicat` 검색 결과는 [D-ERR] 의 "the response could be duplicated `n`times" 한 건뿐이고 cloid 와 무관하다. [D-ERR] 의 오류 표에도 cloid 중복 오류가 없다. SDK 에도 중복 처리 코드가 없다 |
| `cancelByCloid` 의 요청이 `asset` + `cloid` 쌍이라는 점 (cloid 단독으로 주문을 지정하지 않음) | [D-EX] | `"cancels": [ { "asset": Number, "cloid": String } ]` | 문서 명시 (유일성 범위에 대한 간접 단서일 뿐, 범위를 말해주지는 않음) |
| 응답 형태: HTTP 200 + `status:"ok"` + `response.type:"order"` + `data.statuses[]` | [D-EX] | `{"status":"ok","response":{"type":"order","data":{"statuses":[{"resting":{"oid":77738308}}]}}}` | 문서 명시 |
| 주문별 상태 3종: `resting{oid}`, `filled{totalSz,avgPx,oid}`, `error` 문자열 | [D-EX] | `"filled":{"totalSz":"0.02","avgPx":"1891.4","oid":77747314}` / `"error":"Order must have minimum value of $10."` | 문서 명시 |
| **주문별 거부도 HTTP 200 / `status:"ok"` 로 온다** (탭 제목이 "200: OK Error Response") | [D-EX] | `{"status":"ok", ... "statuses":[{"error":"Order must have minimum value of $10."}]}` | 문서 명시 |
| 문서 예시에 없는 주문 상태 `waitingForFill`, `waitingForTrigger`, `success` 가 SDK 타입에 있다 | [S-RS] `exchange_responses.rs` | `pub enum ExchangeDataStatus { Success, WaitingForFill, WaitingForTrigger, Error(String), Resting(RestingOrder), Filled(FilledOrder), }` | SDK 구현에서 확인 (의미는 문서에 명시 없음) |
| 배치: 오류는 "보통" 요청과 같은 길이의 벡터로 반환 | [D-ERR] | "Order and cancel errors are usually returned as a vector with same length as the batched request." | 문서 명시 |
| 배치: pre-validation 오류는 페이로드 전체에 대해 **하나만** 반환 | [D-ERR] | "In this case only one error is returned for the entire payload, as some of these errors do not apply to a specific order or cancel." | 문서 명시 |
| 배치 사용자는 단일 오류가 오는 경우를 처리해야 한다 | [D-ERR] | "it's recommended to handle the case where a single error is returned for a batch of multiple orders." | 문서 명시 |
| 배치 내 부분 성공 (일부는 resting, 일부는 error) | [D-ERR] | (위 "vector with same length" 문장) | 문서 명시 (간접). "partial success" 라는 표현이나 예시는 없음 |
| 배치의 원자성, statuses 순서가 요청 순서와 같은지 | — | — | 문서에 명시 없음 ("same length" 만 명시) |
| IOC 부분 체결 시 응답 형태 (`filled.totalSz` < 요청 수량인지, 잔량 취소가 어떻게 표현되는지) | — | — | 문서에 명시 없음 |
| `expiresAfter`: 그 시각 이후 액션 거부 | [D-EX] | "Some actions support an optional field `expiresAfter` which is a timestamp in milliseconds after which the action will be rejected." | 문서 명시 |
| `expiresAfter` 로 만료된 액션은 주소 기반 rate limit 을 5배 소모 | [D-EX] | "Note that actions consume 5x the usual address-based rate limit when canceled due to a stale `expiresAfter` field." | 문서 명시 |
| user-signed 액션은 `expiresAfter` 미지원 | [D-EX] | "User-signed actions such as Core USDC transfer do not support the `expiresAfter` field." | 문서 명시 |
| `expiresAfter` 비교 기준 시각(블록 시각인지 API 서버 시각인지), 만료 시 오류 문자열 | — | — | 문서에 명시 없음 |
| `expiresAfter` 는 서명 해시에 포함된다 | [S-PY-SIGN] | `if expires_after is not None: data += b"\x00"; data += expires_after.to_bytes(8, "big")` (원문은 여러 줄) | SDK 구현에서 확인 |
| `grouping` 값 | [D-EX] | `"grouping": "na" \| "normalTpsl" \| "positionTpsl"` | 문서 명시 |
| `grouping` 각 값의 의미 설명 | — | — | 문서에 명시 없음 (TP/SL 동작은 [D-TPSL] 에 UI 관점으로만 설명) |
| `grouping` 에 우선순위 수수료 `{"p": N}` 형태도 가능 | [D-PRIO] | "Orders can be sent with grouping of the form `{\"p\": 12345}` where the rate is interpreted as a fraction `p / 100000000.0`." | 문서 명시 |
| 우선순위 그룹 조건 | [D-PRIO] | "every order is IOC, or every order is a non-reduce-only ALO" | 문서 명시 |
| `builder` 필드 | [D-EX] | "f is the size of the fee in tenths of a basis point e.g. if f is 10, 1bp of the order notional will be charged to the user" | 문서 명시 |
| builder 수수료는 사용자의 메인 지갑 승인 필요, 상한 | [D-BLD] | "Builder fees charged can be at most 0.1% on perps and 1% on spot." | 문서 명시 |
| 서브계정/볼트 주문은 마스터가 서명하고 `vaultAddress` 지정 | [D-EX] | "signing should be done by the master account and the vaultAddress field should be set to the address of the subaccount or vault." | 문서 명시 |

### B. 취소 / 정정 / 데드맨 스위치

| 주장 | 출처 | 인용 | 확신도 |
|---|---|---|---|
| `cancel`: `{a: asset, o: oid}` 배열 | [D-EX] | "a is asset / o is oid (order id)" | 문서 명시 |
| 취소 성공 응답 | [D-EX] | `"data":{"statuses":["success"]}` | 문서 명시 |
| 취소 실패 문자열 (없는 주문/이미 취소/이미 체결을 **구분하지 않는다**) | [D-EX], [D-ERR] | "Order was never placed, already canceled, or filled." | 문서 명시 |
| `fast` 플래그 `f` | [D-EX] | "Orders with `f: true` are rejected if they refer to trigger orders. Currently `fast` has no other effect." | 문서 명시 |
| `f` 가 false 면 필드를 넣지 말아야 한다 | [D-EX] | "Note that `f` must be skipped if false, i.e. actions hashed with `f: false` will be rejected." | 문서 명시 |
| `cancelByCloid`: `{asset, cloid}` 배열 | [D-EX] | `"type": "cancelByCloid", "cancels": [ { "asset": Number, "cloid": String } ]` | 문서 명시 |
| `cancelByCloid` 응답 예시 | [D-EX] | (성공/오류 탭이 비어 있음) | 문서에 명시 없음 |
| `modify`: 대상은 oid 또는 cloid | [D-EX] | `"oid": Number \| Cloid` | 문서 명시 |
| `modify` 는 취소 + 신규 배치로 서술된다 | [D-EX] | "When `always_place = true` will place the new order regardless of whether the cancel succeeded." | 문서 명시 |
| `always_place = false` 일 때 제약 | [D-EX] | "the new order must be a non-trigger order, and must have TIF = ALO or a non-executable order with TIF = GTC." | 문서 명시 |
| 그 경우 TIF 가 ALO 로 덮어써진다 | [D-EX] | "In the latter case, TIF of the new order is overridden to ALO." | 문서 명시 |
| `a` 가 false 면 필드를 넣지 말아야 한다 | [D-EX] | "Note that `a` must be skipped if false, i.e. actions hashed with `a: false` will be rejected." | 문서 명시 |
| **modify 후 oid 가 바뀌는가** | — | — | **문서에 명시 없음.** "regardless of whether the cancel succeeded" 표현은 새 주문(=새 oid)을 시사하지만 추론일 뿐이다. 응답 예시 탭도 비어 있다 |
| modify 시 cloid 승계, 큐 우선순위, 부분 체결분 처리 | — | — | 문서에 명시 없음 |
| modify 는 블록 내 정렬에서 "새로 놓는 주문" 기준으로 분류 | [D-HOB] | "Modifies are categorized according to the new order they place." | 문서 명시 |
| `batchModify`: `modifies[]` + 선택 `a` | [D-EX] | `"type": "batchModify", "modifies": [{ "oid": Number \| Cloid, "order": {...} }]` | 문서 명시 |
| `batchModify` 원자성 (일부만 성공 가능한지) | — | — | 문서에 명시 없음 |
| Python SDK 의 단건 modify 는 실제로 `batchModify` 를 보낸다. `f`/`a` 플래그는 구현되어 있지 않다 | [S-PY-EX] | `return self.bulk_modify_orders_new([modify])` (`modify_order` 본문), 그 안에서 `"type": "batchModify",` | SDK 구현에서 확인 |
| `scheduleCancel`: 미래 시각에 전체 취소 예약, `time` 생략 시 해제, 최소 5초 뒤 | [D-EX] | "Schedule a cancel-all operation at a future time. Not including time will remove the scheduled cancel operation. The time must be at least 5 seconds after the current time." | 문서 명시 |
| 발동 시 모든 열린 주문 취소 + 트리거 카운트 증가, 하루 최대 10회 | [D-EX] | "Once the time comes, all open orders will be canceled and a trigger count will be incremented. The max number of triggers per day is 10." | 문서 명시 |
| 카운트 리셋 시각 | [D-EX] | "This trigger count is reset at 00:00 UTC." | 문서 명시 |
| 발동으로 취소된 주문의 상태값 | [D-INFO] | "scheduledCancel \| API only. Canceled due to exceeding scheduled cancel deadline (dead man's switch)" | 문서 명시 |
| 10회 초과 후 동작 (예약 거부? 무시?), 트리거/TWAP 주문 포함 여부, 범위(계정 단위인지 `vaultAddress` 단위인지), 예약 조회 방법 | — | — | 문서에 명시 없음 |
| `noop`: 아무것도 하지 않지만 nonce 를 사용 처리 | [D-EX] | "This action does not do anything (no operation), but causes the nonce to be marked as used." | 문서 명시 |
| `noop` 은 비행 중(in-flight) 주문을 무효화하는 수단으로 **문서가 직접 권한다** | [D-EX] | "This can be a more effective way to cancel in-flight orders than the cancel action." | 문서 명시 |
| 같은 nonce 가 이미 쓰였을 때 `noop` (또는 어떤 액션이든) 의 응답 형태/오류 문자열 | — | — | 문서에 명시 없음 |
| 혼잡 시 이미 결과가 반환된 cancel 을 재전송하지 말라는 권고 | [D-RATE] | "it can therefore be helpful to not resend cancels whose results have already been returned via the API." | 문서 명시 |

### C. Nonce 와 API 지갑

| 주장 | 출처 | 인용 | 확신도 |
|---|---|---|---|
| 주소별로 가장 큰 nonce 100개를 저장 | [D-NONCE] | "On Hyperliquid, the 100 highest nonces are stored per address." | 문서 명시 |
| 유효 조건 1·2: 집합의 최솟값보다 커야 하고, 한 번도 쓰인 적 없어야 함 | [D-NONCE] | "Every new transaction must have nonce larger than the smallest nonce in this set and also never have been used before." | 문서 명시 |
| 유효 조건 3: 시간 창 | [D-NONCE] | "Nonces must be within `(T - 2 days, T + 1 day)`, where `T` is the unix millisecond timestamp on the block of the transaction." | 문서 명시 |
| nonce 는 **서명자** 단위로 추적 (agent 로 서명하면 agent 주소 기준) | [D-NONCE] | "Nonces are tracked per signer, which is the user address if signed with private key of the address, or the agent address if signed with an API wallet." | 문서 명시 |
| 엄격한 순서 불필요 (이더리움식 nonce+1 이 아님) | [D-NONCE] | "Requiring a precise ordering of inclusion on the blockchain will break any strategy." | 문서 명시 |
| **동일 서명 페이로드의 재전송/중복 배달**: 같은 nonce 는 "never have been used before" 조건 때문에 두 번 적용될 수 없다 | [D-NONCE] | (위 인용) | **추론** (규칙의 귀결. 문서가 "중복 배달 시 한 번만 적용된다" 고 직접 쓰지는 않았고, 두 번째 제출이 받는 응답/오류 문자열도 명시 없음) |
| 예외: agent 가 해제(deregister)되어 nonce 상태가 prune 되면 과거 서명 액션이 **재생될 수 있다** | [D-NONCE] | "For example, previously signed actions can be replayed once the nonce set is pruned." | 문서 명시 |
| 그래서 agent 주소 재사용 금지를 강하게 권고 | [D-NONCE] | "it is **strongly** suggested to not reuse their addresses. Once an agent is deregistered, its used nonce state may be pruned." | 문서 명시 |
| prune 조건 3가지 | [D-NONCE] | "1. The wallet is deregistered. ... 2. The wallet expires. 3. The account that registered the agent no longer has funds." (중략 포함) | 문서 명시 |
| deregister 가 일어나는 경우 | [D-NONCE] | "This happens to an existing unnamed API Wallet when an ApproveAgent action is sent to register a new unnamed API Wallet." | 문서 명시 |
| named agent 는 같은 이름으로 다시 승인하면 기존 것이 해제된다 | [D-NONCE] | "This also happens to an existing named API Wallet when an ApproveAgent action is sent with a matching name." | 문서 명시 |
| agent 수 제한 | [D-EX] | "An account can have 1 unnamed approved wallet and up to 3 named ones. And additional 2 named agents are allowed per subaccount." | 문서 명시 |
| agent 만료: 이름 뒤에 `valid_until {timestamp}`, 최대 180일 | [D-EX] | "The expiration can be at most 180 days in the future" | 문서 명시 |
| 만료를 지정하지 않은 agent 의 기본 수명 | — | — | 문서에 명시 없음 |
| 조회는 agent 주소가 아니라 실제 계정 주소로 해야 한다 | [D-NONCE], [D-INFO] | "A common pitfall is to use the agent wallet which leads to an empty result." | 문서 명시 |
| 다중 프로세스 권고 1: 프로세스당 API 지갑 하나 | [D-NONCE] | "Use a API wallet per trading process." | 문서 명시 |
| 같은 agent 로 여러 서브계정을 서명하면 nonce 추적을 공유 | [D-NONCE] | "separate subaccounts signed by the same API wallet will share the nonce tracker of the API wallet." | 문서 명시 |
| 권고 2: 0.1초마다 주문/취소를 배치 | [D-NONCE] | "have a task that periodically batches order and cancel requests every 0.1 seconds." | 문서 명시 |
| 권고: IOC/GTC 와 ALO 를 다른 배치로 | [D-NONCE] | "It is recommended to batch IOC and GTC orders separately from ALO orders because ALO order-only batches are prioritized by the validators." | 문서 명시 |
| 권고 4: 원자적 카운터로 nonce 발급, 필요 시 현재 ms 로 앞당김 | [D-NONCE] | "The atomic counter can be fast-forwarded to current unix milliseconds if needed." | 문서 명시 |
| 이 구조가 견디는 순서 뒤바뀜 한도 | [D-NONCE] | "This structure is robust to out-of-order transactions within 2 seconds" | 문서 명시 |
| Python SDK 는 매 액션마다 현재 시각(ms)을 nonce 로 쓴다 (카운터 없음) | [S-PY-EX] | `timestamp = get_timestamp_ms()` | SDK 구현에서 확인 |
| Rust SDK 는 프로세스 전역 원자 카운터를 쓰고, 300초 이상 뒤처지면 현재 시각으로 앞당긴다 | [S-RS] `helpers.rs` | `if nonce + 300000 < now_ms { CUR_NONCE.fetch_max(now_ms + 1, Ordering::Relaxed); return now_ms; }` | SDK 구현에서 확인 |
| 그 앞당김 분기에서 동시 호출자가 같은 nonce 를 받을 수 있다는 보고 | [S-RS-PR198] | "`next_nonce()` can hand the same nonce to two concurrent callers, and the exchange rejects the duplicate." | 2차 자료 (미병합 PR. "거래소가 중복을 거부한다" 는 서술은 PR 작성자의 주장) |

### D. Info 엔드포인트

| 주장 | 출처 | 인용 | 확신도 |
|---|---|---|---|
| `orderStatus` 는 oid(u64) 또는 cloid(16바이트 hex)로 조회 | [D-INFO] | "Either u64 representing the order id or 16-byte hex string representing the client order id" | 문서 명시 |
| 찾은 경우 응답: `{"status":"order","order":{"order":{...,"cloid":...},"status":<status>,"statusTimestamp":...}}` | [D-INFO] | `"status": <status>, "statusTimestamp": 1724361546645` | 문서 명시 |
| **모르는 주문**: `{"status":"unknownOid"}` (탭 제목 "200: OK Missing Order") | [D-INFO] | `{ "status": "unknownOid" }` | 문서 명시 |
| 실측: 임의의 cloid, oid=1 모두 HTTP 200 + `{"status":"unknownOid"}` | [M-TESTNET] | `{"status":"unknownOid"}` | 실측(비인증) |
| `unknownOid` 가 "한 번도 존재한 적 없음" 을 뜻하는지, 보존 기간이 지나 잊힌 주문도 포함하는지 | — | — | 문서에 명시 없음 |
| HIP-3 dex 주문은 `orderStatus` 에 `dex` 인자가 없어 `unknownOid` 가 나온다는 사용자 보고 | [X-ISS253] | "I receive the following response ... `\"status\": \"unknownOid\"`" | 2차 자료 (공식 확인 없음. 문서의 `orderStatus` 요청 본문에는 `dex` 필드가 없다) |

**`orderStatus` / `historicalOrders` / WS `orderUpdates` 의 `status` 값 전체 목록** ([D-INFO] 표 그대로. 확신도: 문서 명시)

| status | 문서 설명 (원문) |
|---|---|
| `open` | Placed successfully |
| `filled` | Filled |
| `canceled` | Canceled by user |
| `triggered` | Trigger order triggered |
| `rejected` | Rejected at time of placement |
| `marginCanceled` | Canceled because insufficient margin to fill |
| `vaultWithdrawalCanceled` | Vaults only. Canceled due to a user's withdrawal from vault |
| `openInterestCapCanceled` | Canceled due to order being too aggressive when open interest was at cap |
| `selfTradeCanceled` | Canceled due to self-trade prevention |
| `reduceOnlyCanceled` | Canceled reduced-only order that does not reduce position |
| `siblingFilledCanceled` | TP/SL only. Canceled due to sibling ordering being filled |
| `delistedCanceled` | Canceled due to asset delisting |
| `liquidatedCanceled` | Canceled due to liquidation |
| `scheduledCancel` | API only. Canceled due to exceeding scheduled cancel deadline (dead man's switch) |
| `tickRejected` | Rejected due to invalid tick price |
| `minTradeNtlRejected` | Rejected due to order notional below minimum |
| `perpMarginRejected` | Rejected due to insufficient margin |
| `reduceOnlyRejected` | Rejected due to reduce only |
| `badAloPxRejected` | Rejected due to post-only immediate match |
| `iocCancelRejected` | Rejected due to IOC not able to match |
| `badTriggerPxRejected` | Rejected due to invalid TP/SL price |
| `marketOrderNoLiquidityRejected` | Rejected due to lack of liquidity for market order |
| `positionIncreaseAtOpenInterestCapRejected` | Rejected due to open interest cap |
| `positionFlipAtOpenInterestCapRejected` | Rejected due to open interest cap |
| `tooAggressiveAtOpenInterestCapRejected` | Rejected due to price too aggressive at open interest cap |
| `openInterestIncreaseRejected` | Rejected due to open interest cap |
| `insufficientSpotBalanceRejected` | Rejected due to insufficient spot balance |
| `oracleRejected` | Rejected due to price too far from oracle |
| `perpMaxPositionRejected` | Rejected due to exceeding margin tier limit at current leverage |

- 주의 1: `...Rejected` 상태가 목록에 있다는 것은 **주문별로 거부된 주문도 주문 기록으로 남아 조회될 수 있음을 시사**한다. 그러나 (a) pre-validation 으로 페이로드 전체가 거부된 주문, (b) 서명 오류·nonce 오류로 거부된 액션이 기록을 남기는지는 문서에 명시 없음.
- 주의 2: 부분 체결을 뜻하는 별도 status 는 없다. `open` 과 `sz`/`origSz` 조합으로 판단해야 한다.
- 주의 3: **`sz` 의 의미가 문서 안에서 엇갈린다.** [D-INFO] 예시는 체결 완료 주문에 `"sz": "0.0"`, `"origSz": "0.0076"` 을 보여 주어 `sz` = 잔량처럼 보인다. 반면 [D-L1] 의 order status 예시는 `"status": "canceled"` 주문의 `"sz": "257.0"` 에 `// filled size` 주석, `"origSz": "257.0"` 에 `// original order size` 주석을 달았다. 로컬 Go 어댑터(`lookup.go`)는 `filled = origSz - sz` (sz = 잔량) 로 가정한다. → 4절 실험 항목.

| 주장 | 출처 | 인용 | 확신도 |
|---|---|---|---|
| `openOrders`: 필드 예시 `coin, limitPx, oid, side, sz, timestamp` | [D-INFO] | `"coin": "BTC", "limitPx": "29792.0", "oid": 91490942, "side": "A", "sz": "0.0", "timestamp": 1681247412573` | 문서 명시 |
| `openOrders` 응답에 cloid 가 포함되는가 | [S-RS] `info/response_structs.rs` | `pub struct OpenOrdersResponse { ... pub cloid: Option<String>, }` | SDK 구현에서 확인 (문서 예시에는 cloid 없음) |
| `frontendOpenOrders`: 추가 필드 `isPositionTpsl, isTrigger, orderType, origSz, reduceOnly, triggerCondition, triggerPx` | [D-INFO] | `"orderType": "Limit", "origSz": "5.0", "reduceOnly": false` | 문서 명시 |
| `openOrders` / `frontendOpenOrders` 는 `dex` 인자를 받는다 (기본은 첫 perp dex) | [D-INFO] | "Perp dex name. Defaults to the empty string which represents the first perp dex." | 문서 명시 |
| open orders 응답의 개수 상한/페이지네이션 | — | — | 문서에 명시 없음 (계정당 열린 주문 한도는 F절) |
| `historicalOrders`: 최근 2000건까지 | [D-INFO] | "Returns at most 2000 most recent historical orders" | 문서 명시 |
| `historicalOrders` 예시의 status 는 6종만 적혀 있다 | [D-INFO] | `"status": "filled" \| "open" \| "canceled" \| "triggered" \| "rejected" \| "marginCanceled"` | 문서 명시 (전체 목록과 불일치. 전체 목록은 [D-ERR] 가 orderStatus 표를 가리킴) |
| `userFills`: 최근 2000건까지 | [D-INFO] | "Returns at most 2000 most recent fills" | 문서 명시 |
| `userFillsByTime`: 응답당 최대 2000건, **최근 10000건만 조회 가능** | [D-INFO] | "Returns at most 2000 fills per response and only the 10000 most recent fills are available" | 문서 명시 |
| `startTime`/`endTime` 은 모두 inclusive (ms) | [D-INFO] | "Start time in milliseconds, inclusive" / "End time in milliseconds, inclusive. Defaults to current time." | 문서 명시 |
| 일반 페이지네이션 규칙 (500건) | [D-INFO] | "Responses that take a time range will only return 500 elements or distinct blocks of data." | 문서 명시 (userFillsByTime 의 "2000" 과 수치가 다르다. 어느 쪽이 적용되는지 문서로는 불명확) |
| 페이지네이션 방법 | [D-INFO] | "To query larger ranges, use the last returned timestamp as the next `startTime` for pagination." | 문서 명시 (inclusive 이므로 경계 중복이 생긴다는 점은 추론) |
| 시간 범위 자체의 최대 폭 | — | — | 문서에 명시 없음 (건수 상한만 있음) |
| `aggregateByTime` | [D-INFO] | "When true, partial fills are combined when a crossing order gets filled by multiple different resting orders." | 문서 명시 |
| resting 쪽 집계 조건 | [D-INFO] | "Resting orders filled by multiple crossing orders are only aggregated if in the same block." | 문서 명시 |
| fill 필드 (REST 예시): `closedPnl, coin, crossed, dir, hash, oid, px, side, startPosition, sz, time, fee, feeToken, builderFee(선택), tid` | [D-INFO] | `"fee": "0.01", // the total fee, inclusive of builderFee below` | 문서 명시 |
| fill 필드 의미 (WS 타입 주석) | [D-SUB] | `hash: string; // L1 transaction hash` / `crossed: boolean; // whether order crossed the spread (was taker)` / `fee: string; // negative means rebate` / `tid: number; // unique trade id` | 문서 명시 |
| `dir` 은 프런트엔드 표시용 | [D-SUB] | `dir: string; // used for frontend display` | 문서 명시 (값의 전체 목록은 명시 없음. 예시에 "Open Long", "Sell") |
| `liquidation` 필드 (선택) | [D-SUB] | `interface FillLiquidation { liquidatedUser?: string; markPx: number; method: "market" \| "backstop"; }` | 문서 명시 |
| **fill 에 `cloid` 가 들어 있는가** | [S-RS] `ws/sub_structs.rs` | `pub struct TradeInfo { ... pub oid: u64, pub cloid: Option<String>, ... }` | SDK 구현에서 확인 (WS 쪽). 문서의 `WsFill` 타입과 REST 예시에는 cloid 가 **없다**. Rust SDK 의 REST 용 `UserFillsResponse`, Python SDK `Fill` 타입에도 cloid 가 없다 |
| `tid` 의 전역 유일성 한계 (공개 trades 피드 타입 `WsTrade` 의 주석 두 줄) | [D-SUB] | "tid is 50-bit hash of (buyer_oid, seller_oid). For a globally unique trade id, use (block_time, coin, tid)" | 문서 명시 |
| `clearinghouseState`: `assetPositions[].position{coin, szi, entryPx, leverage, liquidationPx, marginUsed, positionValue, unrealizedPnl, cumFunding...}`, `marginSummary`, `crossMarginSummary`, `withdrawable`, `time` | [D-PERP] | "See a user's open positions and margin summary for perpetuals trading." | 문서 명시 |
| unified/portfolio margin 계정에서는 이 응답이 잔고 기준이 아니다 | [D-PERP] | "Under unified account or portfolio margin, use spot balances endpoint instead for trading account balance across spot and perps." | 문서 명시 |
| `userRateLimit` 응답 | [D-INFO] | `"nRequestsUsed": 2890, // max(0, cumulative_used minus reserved)` / `"nRequestsSurplus": 0, // max(0, reserved minus cumulative_used)` | 문서 명시 |
| 실측: 거래 이력 없는 주소의 `nRequestsCap` 은 10000 | [M-TESTNET] | `{"cumVlm":"0.0","nRequestsUsed":0,"nRequestsCap":10000,"nRequestsSurplus":0}` | 실측(비인증). F절의 "initial buffer of 10000 requests" 와 일치 |
| `meta`: `universe[].{name, szDecimals, maxLeverage, onlyIsolated, isDelisted, marginMode}` + `marginTables` | [D-PERP] | `"marginMode": "strictIsolated", // "strictIsolated" means margin cannot be removed, "noCross" means only isolated margin allowed` | 문서 명시 |
| `metaAndAssetCtxs`: `[meta, ctxs[]]`, ctx 에 `funding, markPx, midPx, oraclePx, openInterest, premium, impactPxs, dayNtlVlm, prevDayPx` | [D-PERP] | `"funding":"0.0000125", ... "markPx":"14.3161", "midPx":"14.314", "openInterest":"688.11", "oraclePx":"14.32"` | 문서 명시 |
| **보존 한도**: fill 은 API 로 최근 10000건, 주문 이력은 최근 2000건 | [D-INFO] | (위 두 인용) | 문서 명시 |
| `orderStatus` 로 조회 가능한 주문의 보존 기간 | — | — | 문서에 명시 없음 |
| 그 이상의 과거 체결은 S3/제3자 도구 | [D-HIST] | "`s3://hl-mainnet-node-data/node_fills_by_block` has fills which are streamed via `--write-fills --batch-by-block` from a non-validating node." | 문서 명시 |
| S3 아카이브는 완전성 보장이 없다 (자산 데이터 버킷에 대한 문장) | [D-HIST] | "There is no guarantee of timely updates and data may be missing." | 문서 명시 |

### E. WebSocket

| 주장 | 출처 | 인용 | 확신도 |
|---|---|---|---|
| 구독 메시지 형식 | [D-SUB] | `{ "method": "subscribe", "subscription": { ... } }` | 문서 명시 |
| 구독 ack | [D-SUB] | "The server will respond to successful subscriptions with a message containing the `channel` property set to `\"subscriptionResponse\"`" | 문서 명시 |
| `orderUpdates`: `{ "type": "orderUpdates", "user": "<address>" }`, 데이터 `WsOrder[]` | [D-SUB] | `interface WsOrder { order: WsBasicOrder; status: string; statusTimestamp: number; }` | 문서 명시 |
| `WsBasicOrder` 에 cloid 포함 | [D-SUB] | `oid: number; timestamp: number; origSz: string; cloid: string \| undefined;` | 문서 명시 |
| `orderUpdates` 의 status 는 orderStatus 표와 같은 값 집합 | [D-SUB] | "See https://hyperliquid.gitbook.io/hyperliquid-docs/for-developers/api/info-endpoint#query-order-status-by-oid-or-cloid for a list of possible values" | 문서 명시 |
| `orderUpdates` 구독 시 스냅샷 제공 여부 | — | — | 문서에 명시 없음 (`WsOrder` 에 `isSnapshot` 없음. 스냅샷은 "time series data (e.g. user fills)" 에 대해서만 서술) |
| `userFills`: `{ "type": "userFills", "user": "<address>" }`, 선택 `aggregateByTime` | [D-SUB] | `interface WsUserFills { isSnapshot?: boolean; user: string; fills: Array<WsFill>; }` | 문서 명시 |
| 구독 시 스냅샷 → 이후 스트림 | [D-SUB] | "the first message has `isSnapshot: true` and the following streaming updates have `isSnapshot: false`." | 문서 명시 |
| 스냅샷은 이미 처리했다면 무시 가능 (= **수신자가 중복을 걸러야 한다**) | [D-SUB] | "These snapshot messages are tagged with `isSnapshot: true` and can be ignored if the previous messages were already processed." | 문서 명시 |
| 스냅샷이 담는 범위(건수/기간) | — | — | 문서에 명시 없음 |
| `userEvents`: 채널명은 `user`, 주문 업데이트가 아닌 이벤트 | [D-SUB] | "Note: The channel name for `userEvents` messages is `\"channel\": \"user\"`" | 문서 명시 |
| `WsUserEvent` 는 4종 합집합 | [D-SUB] | `type WsUserEvent = {"fills": WsFill[]} \| {"funding": WsUserFunding} \| {"liquidation": WsLiquidation} \| {"nonUserCancel" :WsNonUserCancel[]};` | 문서 명시 |
| `nonUserCancel`: 사용자가 요청하지 않은 취소 통지 `{coin, oid}` | [D-SUB] | `interface WsNonUserCancel { coin: String; oid: number; }` | 문서 명시 (어떤 사유에서 발생하는지는 명시 없음) |
| `userFundings`: 스냅샷 후 매시 정각 funding | [D-SUB] | "Funding payments snapshot followed by funding payments on the hour" | 문서 명시 |
| `userNonFundingLedgerUpdates` 범위 | [D-SUB] | "Ledger updates not including funding payments: withdrawals, deposits, transfers, and liquidations" | 문서 명시 |
| ledger liquidation 타입 | [D-SUB] | `type: "liquidation"; accountValue: number; leverageType: "Cross" \| "Isolated"; liquidatedPositions: Array<LiquidatedPosition>;` | 문서 명시 |
| `webData3`: 구독 목록에 있음 | [D-SUB] | "Additional undocumented fields in WebData3 will be removed on a future update" | 문서 명시 |
| `webData2`: 구독 목록에는 **없고** Data formats 에만 남아 있다. 예시는 `"type": "webData"` 를 쓴다 | [D-SUB] | "`WebData2`: Aggregate information about a user, used primarily for the frontend." | 문서 명시 (문서 내부 불일치. Python SDK 에는 `webData2` 구독 타입이 있음 [S-PY-TYPES]) |
| `notification`: `{ "type": "notification", "user": "<address>" }` | [D-SUB] | `Notification { notification: string }` | 문서 명시 (내용 종류는 명시 없음) |
| 사용자별 스트림 대안: `openOrders`, `clearinghouseState` 구독도 존재 | [D-SUB] | `{ "type": "openOrders", "user": "<address>", "dex": "<dex>" }` | 문서 명시 |
| 서버는 60초 동안 보낸 메시지가 없으면 연결을 끊는다 | [D-HB] | "The server will close any connection if it hasn't sent a message to it in the last 60 seconds." | 문서 명시 |
| heartbeat | [D-HB] | `{ "method": "ping" }` → `{ "channel": "pong" }` | 문서 명시 |
| 공식 SDK 는 50초마다 ping | [S-PY-WS], [S-RS] `ws_manager.rs` | `while not self.stop_event.wait(50):` / `const SEND_PING_INTERVAL: u64 = 50;` | SDK 구현에서 확인 |
| **서버 측 단절은 예고 없이 주기적으로 일어날 수 있다** | [D-WS] | "Disconnection from API servers may happen periodically and without announcement." | 문서 명시 |
| 자동화 사용자는 재접속을 처리해야 한다 | [D-WS] | "all automated users should handle disconnects from the server side and gracefully reconnect." | 문서 명시 |
| 단절 중 놓친 데이터는 재접속 시 스냅샷 ack 에 들어 있다 | [D-WS] | "Missed data during the reconnect will be present in the snapshot ack on reconnect." | 문서 명시 (스냅샷이 있는 스트림에 한함. 범위·한도는 명시 없음) |
| 또는 info 요청으로 직접 조회 | [D-WS] | "Users can also manually query any missed data using the corresponding info request." | 문서 명시 |
| 메시지 순서 보장, 시퀀스 번호, 채널 간 순서(orderUpdates vs userFills) | — | — | 문서에 명시 없음 (메시지에 시퀀스 필드가 없다) |
| 재접속 시 중복 | [D-SUB] | (위 "can be ignored if the previous messages were already processed") | 문서 명시 (스냅샷 중복에 한함. 스트림 메시지 자체의 중복 가능성은 명시 없음) |
| WS 로 액션 전송 (`post`) | [D-POST] | `{ "method": "post", "id": <number>, "request": { "type": "info" \| "action", "payload": { ... } } }` | 문서 명시 |
| 응답 상관관계는 `id` 로 | [D-POST] | "It is recommended that you use a unique `id` for every post request you send in order to track outstanding requests through the channel." | 문서 명시 |
| 응답 형식 | [D-POST] | `{ "channel": "post", "data": { "id": <number>, "response": { "type": "info" \| "action" \| "error", "payload": { ... } } } }` | 문서 명시 |
| 오류는 HTTP 상태 코드/설명을 흉내 낸 문자열 | [D-POST] | "For errors, a `String` is returned mirroring the HTTP status code and description that would have been returned if the request were sent through HTTP." | 문서 명시 |
| 동시 inflight post 한도 100 | [D-RATE] | "Maximum of 100 simultaneous inflight post messages across all websocket connections" | 문서 명시 |
| post 응답을 받기 전에 연결이 끊기면 그 액션이 어떻게 되는지 | — | — | 문서에 명시 없음 |
| Python SDK 는 `userEvents`/`orderUpdates` 를 연결당 1개만 구독 허용 (메시지에 user 가 없어 다중화 불가) | [S-PY-WS] | `# TODO: ideally the userEvent and orderUpdates messages would include the user so that we can multiplex` | SDK 구현에서 확인 |
| 시세 스냅샷 주기 | [D-SUB] | "Snapshot feed, pushed on each block that is at least 0.5 since last push" | 문서 명시 |

### F. Rate limit 과 한도

**IP 단위** ([D-RATE], 확신도: 문서 명시)

| 항목 | 수치 | 인용 |
|---|---|---|
| REST 합산 weight | 1200 / 분 | "REST requests share an aggregated weight limit of 1200 per minute." |
| `exchange` 액션 weight | `1 + floor(batch_length / 40)` | "All documented `exchange` API requests have a weight of `1 + floor(batch_length / 40)`." |
| 예시 | 단건 1, 79건 배치 2 | "unbatched actions have weight `1` and a batched order request of length 79 has weight `2`." |
| info weight 2 | `l2Book, allMids, clearinghouseState, orderStatus, spotClearinghouseState, exchangeStatus` | "The following `info` requests have weight 2: `l2Book, allMids, clearinghouseState, orderStatus, spotClearinghouseState, exchangeStatus.`" |
| info weight 60 | `userRole` | "The following `info` requests have weight 60: `userRole` ." |
| 그 외 info | 20 | "All other documented `info` requests have weight 20." |
| 반환 건수 가중 | 20건당 추가 weight (크기는 미기재) | "have an additional rate limit weight per 20 items returned in the response: `recentTrades`, `historicalOrders`, `userFills`, `userFillsByTime`, ..." |
| WS 연결 수 | 10 | "Maximum of 10 websocket connections" |
| WS 신규 연결 | 30 / 분 | "Maximum of 30 new websocket connections per minute" |
| WS 구독 수 | 1000 | "Maximum of 1000 websocket subscriptions" |
| 사용자별 구독의 고유 사용자 수 | 10 | "Maximum of 10 unique users across user-specific websocket subscriptions" |
| WS 송신 메시지 | 2000 / 분 (모든 연결 합산) | "Maximum of 2000 messages sent to Hyperliquid per minute across all websocket connections" |
| WS inflight post | 100 (모든 연결 합산) | "Maximum of 100 simultaneous inflight post messages across all websocket connections" |

- 문서에 명시 없음: IP 한도 초과 시의 HTTP 상태 코드(429 여부)·응답 본문·차단 시간, WS `post` 액션이 REST 1200 weight 에 합산되는지, "20건당 추가 weight" 의 크기.

**주소(계정) 단위** ([D-RATE], 확신도: 문서 명시)

| 항목 | 수치 | 인용 |
|---|---|---|
| 적용 단위 | 사용자별, 서브계정은 별도 | "Address-based limits apply per user, with sub-accounts treated as separate users." |
| 누적 한도 | 누적 거래 1 USDC 당 요청 1건 | "The rate limiting logic allows 1 request per 1 USDC traded cumulatively since address inception." |
| 초기 버퍼 | 10000 요청 | "Each address starts with an initial buffer of 10000 requests." |
| 소진 시 | 10초에 1건 | "When rate limited, an address is allowed one request every 10 seconds." |
| 취소 여유분 | `min(limit + 100000, limit * 2)` | "Cancels have cumulative limit `min(limit + 100000, limit * 2)` where `limit` is the default limit for other actions." |
| 취지 | 한도에 걸려도 취소는 가능 | "This way, hitting the address-based rate limit still allows open orders to be canceled." |
| 적용 범위 | 액션만 (info 제외) | "Note that this rate limit only applies to actions, not info requests." |
| 배치 계산 | IP 로는 1건, 주소로는 n건 | "A batched request with `n` orders (or cancels) is treated as one request for IP based rate limiting, but as `n` requests for address-based rate limiting." |
| 열린 주문 한도 | 1000 + 거래량 5M USDC 당 1, 최대 5000 | "Each user has a default open order limit of 1000 plus one additional order for every 5M USDC of volume, capped at a total of 5000 open orders." |
| 1000개 이상일 때 | reduce-only / trigger 주문은 거부 | "it will be rejected if it is reduce-only or a trigger order." |
| 혼잡 시 블록 공간 | 전일 maker 점유율의 2배 | "During high congestion, addresses are limited to use 2x their previous day maker share percentage of the block space." |
| 요청 예약 구매 | 요청당 0.0005 USDC ([D-EX]) | "this action allows reserving additional actions for 0.0005 USDC per request. The cost is paid from the Perps balance." |
| 만료 액션 페널티 | 5배 ([D-EX]) | "actions consume 5x the usual address-based rate limit when canceled due to a stale `expiresAfter` field." |
| unified/portfolio margin 계정 | 일 50k 액션 ([D-AAM]) | "Portfolio margin and unified account are limited to 50k user actions per day. Standard mode has no such restrictions." |

- 문서에 명시 없음: 주소 한도 초과 시의 오류 문자열/응답 형태, 열린 주문 한도 초과 시의 오류 문자열, 거부된 주문이 주소 한도를 소모하는지.

**지연·처리량 참고** (확신도: 문서 명시)

| 항목 | 인용 | 출처 |
|---|---|---|
| 종단 지연 | "end-to-end latency has a median 0.2 seconds and 99th percentile 0.9 seconds." | [D-OV] |
| 체인 처리량 | "Mainnet currently supports approximately 200k orders/sec." | [D-OV] |
| 커밋까지 | "time to commit (usually 2 blocks in pipelined HyperBFT consensus)" | [D-LAT] |

### G. Tick / Lot size

| 주장 | 출처 | 인용 | 확신도 |
|---|---|---|---|
| 가격: 유효숫자 5자리 이하 **그리고** 소수 자릿수 ≤ `MAX_DECIMALS - szDecimals` (perp 는 6, spot 은 8) | [D-TICK] | "Prices can have up to 5 significant figures, but no more than `MAX_DECIMALS - szDecimals` decimal places where `MAX_DECIMALS` is 6 for perps and 8 for spot." | 문서 명시 |
| 정수 가격은 유효숫자 제한과 무관하게 허용 | [D-TICK] | "Integer prices are always allowed, regardless of the number of significant figures. E.g. `123456` is a valid price even though `12345.6` is not." | 문서 명시 |
| 수량: `szDecimals` 자릿수까지 | [D-TICK] | "Sizes are rounded to the `szDecimals` of that asset. For example, if `szDecimals = 3` then `1.001` is a valid size but `1.0001` is not." | 문서 명시 |
| perp 가격 예 | [D-TICK] | "`1234.5` is valid but `1234.56` is not (too many significant figures)" / "`0.001234` is valid, but `0.0012345` is not (more than 6 decimal places)" | 문서 명시 |
| `szDecimals` 출처 | [D-TICK] | "`szDecimals` for an asset is found in the meta response to the info endpoint" | 문서 명시 |
| 최소 주문 금액 $10 | [D-ERR] | "Order must have minimum value of $10." | 문서 명시 |
| 잘못된 tick 의 거부 문자열 | [D-ERR] | "Price must be divisible by tick size." | 문서 명시 |
| tick 검증 일부는 pre-validation (배치 전체 단일 오류) | [D-ERR] | "... and some forms of tick size validation." | 문서 명시 (어떤 형태가 pre-validation 인지는 명시 없음) |
| 해당 주문 상태값 | [D-INFO] | "tickRejected \| Rejected due to invalid tick price" / "minTradeNtlRejected \| Rejected due to order notional below minimum" | 문서 명시 |
| 수량 자릿수 초과 시 거부 문자열, 서버가 반올림하는지 거부하는지 | — | — | 문서에 명시 없음 ("Sizes are rounded" 와 "is not [valid]" 가 한 문단에 같이 있어 모호) |
| 최소 금액이 limit 가격 기준인지 mark 가격 기준인지, reduce-only 청산 주문의 예외 여부 | — | — | 문서에 명시 없음 |
| 반올림은 클라이언트 책임: SDK 예제는 5 유효숫자 → `6 - szDecimals` 소수로 반올림 | [S-PY-EXM] `rounding.py` | `px = round(float(f"{px:.5g}"), max_decimals - sz_decimals[coin])` | SDK 구현에서 확인 |
| SDK 는 와이어 변환 시 반올림이 발생하면 예외 (조용히 자르지 않음) | [S-PY-SIGN] | `raise ValueError("float_to_wire causes rounding", x)` | SDK 구현에서 확인 |
| 서명 시 뒤따르는 0 제거 | [D-TICK] | "Note that if implementing signing, trailing zeroes should be removed." | 문서 명시 |
| 가격·수량은 tick/lot 의 정수배 | [D-TOB] | "Orders are added where price is an integer multiple of the tick size, and size is an integer multiple of lot size." | 문서 명시 |

### H. 오류 응답

**문서의 배치 오류 표 전체** ([D-ERR], 확신도: 문서 명시)

| Source | Type | Error string |
|---|---|---|
| Order | Tick | Price must be divisible by tick size. |
| Order | MinTradeNtl | Order must have minimum value of $10. |
| Order | MinTradeSpotNtl | Order must have minimum value of 10 {quote_token}. |
| Order | PerpMargin | Insufficient margin to place order. |
| Order | ReduceOnly | Reduce only order would increase position. |
| Order | BadAloPx | Post only order would have immediately matched, bbo was {bbo}. |
| Order | IocCancel | Order could not immediately match against any resting orders. |
| Order | BadTriggerPx | Invalid TP/SL price. |
| Order | MarketOrderNoLiquidity | No liquidity available for market order. |
| Order | PositionIncreaseAtOpenInterestCap | Order would increase open interest while open interest is capped |
| Order | PositionFlipAtOpenInterestCap | Order would increase open interest while open interest is capped |
| Order | TooAggressiveAtOpenInterestCap | Order rejected due to price more aggressive than oracle while at open interest cap |
| Order | OpenInterestIncrease | Order would increase open interest too quickly |
| Order | InsufficientSpotBalance | (Spot-only) Order has insufficient spot balance to trade |
| Order | Oracle | Order price too far from oracle |
| Order | PerpMaxPosition | Order would cause position to exceed margin tier limit at current leverage |
| Cancel | MissingOrder | Order was never placed, already canceled, or filled. |

| 주장 | 출처 | 인용 | 확신도 |
|---|---|---|---|
| 이 표가 전체 목록이라는 보장은 없다 | [D-ERR] | "Below is a list of possible batched error responses:" | 문서 명시 ("possible" 이라고만 함. cloid 중복, nonce 오류, rate limit, 열린 주문 한도 초과 등의 문자열은 표에 없음) |
| pre-validation: 페이로드만으로 결정되는 오류는 더 일찍, 하나만 반환 | [D-ERR] | "Some errors are a deterministic function of the payload itself, and these are instead returned earlier as part of pre-validation." | 문서 명시 |
| 그 예시 | [D-ERR] | "empty batch of orders, non-reduce-only TP/SL orders, order too far from reference price, and some forms of tick size validation." | 문서 명시 |
| pre-validation 오류의 JSON 형태 (`statuses` 길이 1인지, 최상위 `status:"err"` 인지) | — | — | 문서에 명시 없음 |
| 최상위 오류 형태 `{"status":"err","response":"<문자열>"}` | [S-RS] `exchange_responses.rs` | `#[serde(tag = "status", content = "response")] pub enum ExchangeResponseStatus { Ok(ExchangeResponse), Err(String), }` | SDK 구현에서 확인 (문서에는 `status:"err"` 예시가 없음) |
| 서명이 틀리면 다른 주소가 복구되어 이런 오류가 난다 | [D-SIGN] | `"L1 error: User or API Wallet 0x0123... does not exist."` / `Must deposit before performing actions. User: 0x123...` | 문서 명시 |
| 서명 오류는 원인을 알려주지 않는다 | [D-SIGN] | "An incorrect signature does not indicate why it is incorrect which makes debugging more challenging." | 문서 명시 |
| **HTTP 상태 코드 체계** | — | — | 문서에 명시 없음 (주문별 오류가 "200: OK" 로 온다는 것만 예시로 확인됨) |
| SDK 의 HTTP 처리: 4xx 는 JSON `{code,msg,data}` 를 기대하는 ClientError, 그 외 ≥400 은 ServerError | [S-PY-API] | `raise ClientError(status_code, err["code"], err["msg"], response.headers, error_data)` / `raise ServerError(status_code, response.text)` | SDK 구현에서 확인 |
| 실측: 역직렬화 불가 본문은 `/info`, `/exchange` 모두 HTTP 422, `text/plain` | [M-TESTNET] | "Failed to deserialize the JSON body into the target type" | 실측(비인증). 본문이 JSON 이 아니므로 SDK 의 `{code,msg}` 가정과 다르다 |
| **API 서버는 블록 커밋 후에 응답한다** (응답을 받았다면 결과는 확정) | [D-APISRV] | "Once the transaction has been included in a committed block on the L1, the API server responds to the original request with the execution response from the L1." | 문서 명시 |
| API 서버는 노드를 따라가는 로컬 상태를 제공한다 | [D-APISRV] | "API servers listen to updates from a node and maintain the blockchain state locally." | 문서 명시 (즉 info 조회 결과가 최신 블록보다 뒤처질 수 있음은 추론) |
| **타임아웃/알 수 없는 결과**: 오류가 난 요청이 그래도 적용되었을 수 있는지 | — | — | **문서에 명시 없음** (API 문서에는 없음) |
| 간접 근거 1 (프런트엔드 FAQ): 지연된 액션이 나중에 적용되어 주문이 여러 번 나갈 수 있다 | [D-FAQ-EXP] | "Be aware that disabling this protection may cause delayed orders to be placed after reconnection or when congestion eases." | 문서 명시 (프런트엔드 설정 설명. API 계약 서술은 아님) |
| 그 결과 | [D-FAQ-EXP] | "This may result in multiple order placements (if you made multiple attempts to execute an order during the delay) when you only intended for a single action to take place." | 문서 명시 (동일) |
| 프런트엔드의 보호 장치는 15초 | [D-FAQ-EXP] | "it's not accepted by the L1 within 15 seconds." | 문서 명시 (프런트엔드가 `expiresAfter` 를 쓰는지는 명시 없음) |
| 간접 근거 2 (공식 예제): in-flight 주문을 10초 후 "취소된 것으로 간주" 하고, 주기적으로 openOrders 를 폴링해 모르는 oid 를 취소한다 | [S-PY-EXM] `basic_adding.py` | `# The maximum time (in milliseconds) to wait for an in-flight order before treating it as cancelled.` | SDK 구현에서 확인 (공식 예제조차 결정적 해소 수단을 쓰지 않음) |

### I. OMS 관련 거래 의미론

| 주장 | 출처 | 인용 | 확신도 |
|---|---|---|---|
| 매칭: 가격-시간 우선 | [D-HOB] | "Orders are matched in price-time priority." | 문서 명시 |
| 증거금 검사 시점: 주문 접수 시 + 체결 시 resting 쪽 재검사 | [D-HOB] | "Margin checks happen on the opening of a new order, and again for the resting side at the matching of each order." | 문서 명시 (→ resting 주문이 나중에 `marginCanceled` 될 수 있음) |
| 블록 내 정렬 | [D-HOB] | "1. Actions that do not send GTC or IOC orders to any book 2. Cancels 3. Actions that send at least one GTC or IOC" | 문서 명시 |
| 같은 범주 안에서는 제안 순서 | [D-HOB] | "Within each category, actions are sorted in the order they were proposed by the block proposer." | 문서 명시 |
| 취소/ALO 는 같은 시각의 IOC/GTC 보다 거의 항상 먼저 실행 | [D-LAT] | "cancels and ALO orders sent at time `t` will almost always execute before IOC and GTC orders sent at time `t`." | 문서 명시 |
| **자전거래 방지**: resting 주문을 취소 ("expire maker"), 체결 없음 | [D-STP] | "Trades between the same address cancel the resting order instead of causing a fill. No fees are deducted, nor does the the cancel show up in the trade feed." | 문서 명시 |
| 공격 주문은 계속 진행 | [D-STP] | "the aggressing order would like to continue getting fills against liquidity behind the maker order up until the limit price." | 문서 명시 |
| STP 로 취소된 주문의 상태값 | [D-INFO] | "selfTradeCanceled \| Canceled due to self-trade prevention" | 문서 명시 |
| STP 범위는 "same address" (서브계정 간에도 적용되는지) | — | — | 문서에 명시 없음 (주소 기반 한도는 서브계정을 별도 사용자로 취급 [D-RATE]) |
| **trade bust / 체결 정정** | — | — | **문서에 명시 없음** (`bust`, `revert`, `rollback`, `reorg` 검색 0건). 관련 서술은 합의로 단일 순서를 얻는다는 것뿐 |
| 단일 거래 순서 | [D-OV] | "A core design principle is full decentralization with one consistent order of transactions achieved through HyperBFT consensus." | 문서 명시 |
| **청산** (외부 원인 포지션 변화 1): 유지 증거금 미달 시 호가창에 시장가 청산 | [D-LIQ] | "the positions are first attempted to be entirely closed by sending market orders to the book." | 문서 명시 |
| 부분 청산 | [D-LIQ] | "For liquidatable positions larger than 100k USDC (10k USDC on testnet for easier testing), only 20% of the position will be sent as a market liquidation order to the book." | 문서 명시 |
| 백스톱 청산: 포지션과 증거금이 liquidator 로 이전 | [D-LIQ] | "When a cross position is backstop liquidated, the trader's cross positions and cross margin are all transferred to the liquidator." | 문서 명시 |
| 청산 시 열린 주문 취소 상태 | [D-INFO] | "liquidatedCanceled \| Canceled due to liquidation" | 문서 명시 |
| 청산이 보이는 곳: fill 의 `liquidation` 필드, `userEvents` 의 `liquidation`, ledger 의 `liquidation` | [D-SUB] | `interface WsLiquidation { lid: number; liquidator: string; liquidated_user: string; liquidated_ntl_pos: string; liquidated_account_value: string; }` | 문서 명시 |
| **ADL** (외부 원인 포지션 변화 2): 반대편 이익 포지션을 이전 mark price 로 강제 종료 | [D-ADL] | "Those traders' positions are closed at the previous mark price against the now underwater user, ensuring that the platform has no bad debt." | 문서 명시 |
| ADL 대상 순위 | [D-ADL] | "`(mark_price / entry_price) * (notional_position / account_value)`" | 문서 명시 |
| ADL 이 API 에 어떻게 나타나는지 (fill 의 `dir`/`oid` 값, 전용 이벤트 유무) | — | — | 문서에 명시 없음 |
| **delisting** (외부 원인 3): 포지션 정산 + 열린 주문 취소 | [D-DELIST] | "When an asset is delisted, all positions are settled and open orders are cancelled." | 문서 명시 |
| **네트워크 업그레이드**의 post-only 기간에는 시장가가 체결되지 않는다 | [D-OT] | "Like normal market orders, TWAP suborders do not fill during the post-only period of a network upgrade." | 문서 명시 (업그레이드 일정·통지 방법·그 기간의 API 동작은 명시 없음) |
| **한 주문의 체결 식별**: fill 마다 `tid`("unique trade id"), `oid`, `hash`("L1 transaction hash"), `time` | [D-SUB] | `tid: number; // unique trade id` | 문서 명시 |
| `tid` 는 (buyer_oid, seller_oid) 해시이므로 전역 유일 키는 `(block_time, coin, tid)` | [D-SUB] | "For a globally unique trade id, use (block_time, coin, tid)" | 문서 명시 (WsTrade 주석. 같은 두 주문이 여러 번 체결될 때 tid 가 달라지는지는 명시 없음) |
| 한 트랜잭션이 여러 fill 을 만들 때 `hash` 가 같은지 | — | — | 문서에 명시 없음 |
| **funding**: 매시 지급 | [D-FUND] | "The funding rate on Hyperliquid is paid every hour." | 문서 명시 |
| funding 금액 | [D-FUND] | "the funding payment at the end of the interval is `position_size * oracle_price * funding_rate`." | 문서 명시 |
| funding 상한 | [D-FUND] | "Funding on Hyperliquid is capped at 4%/hour." | 문서 명시 |
| 프리미엄 표본 | [D-FUND] | "The premium is sampled every 5 seconds and averaged over the hour." | 문서 명시 |
| funding 은 fill 이 아니라 별도 스트림/ledger (`userFundings`, `userFunding`) | [D-SUB], [D-PERP] | `"type": "funding", "usdc":" -3.625312"` | 문서 명시 |
| **증거금 모드**: cross 가 기본, isolated 지원 | [D-MARGIN] | "*Cross margin* is the default, which allows for maximal capital efficiency by sharing collateral between all other cross margin positions." | 문서 명시 |
| strict isolated | [D-MARGIN] | "Some assets are *strict isolated*, which functions the same as isolated margin with the additional constraint that margin cannot be removed." | 문서 명시 |
| 레버리지 | [D-MARGIN] | "Leverage can be set by a user to any integer between 1 and the max leverage." | 문서 명시 |
| 필요 증거금 | [D-MARGIN] | "The margin required to open a position is `position_size * mark_price / leverage`." | 문서 명시 |
| 유지 증거금 | [D-MARGIN] | "The maintenance margin is currently set to half of the initial margin at max leverage." | 문서 명시 |
| mark/oracle 갱신 주기 | [D-PX] | "mark and oracle price are updated approximately once every 3 seconds." | 문서 명시 |
| 포지션 모드는 one-way (`"type": "oneWay"`), hedge 모드는 미도입 | [D-EX] | "this parameter won't have any effect until hedge mode is introduced" | 문서 명시 |
| **수수료 개요**: 14일 거래량 기반 등급, UTC 일 마감에 산정 | [D-FEE] | "Fees are based on your rolling 14 day volume and are assessed at the end of each day in UTC." | 문서 명시 |
| perp 기본 요율 (tier 0) | [D-FEE] | 표: Taker "0.045%", Maker "0.015%" | 문서 명시 |
| 서브계정 거래량은 마스터에 합산 | [D-FEE] | "Sub-account volume counts toward the master account and all sub-accounts share the same fee tier." | 문서 명시 |
| maker 리베이트는 체결마다 지급, fill 의 `fee` 가 음수 | [D-FEE], [D-SUB] | "Maker rebates are paid out continuously on each trade directly to the trading wallet." | 문서 명시 |
| `closedPnl`, entry price 는 프런트엔드 편의값 | [D-PNL] | "entry price, unrealized pnl, and closed pnl are purely frontend components provided for user convenience. The fundamental accounting is based on margin (balance for spot) and trades." | 문서 명시 |
| TP/SL 자식 주문은 부모가 완전 체결되거나 부분 체결 후 증거금 부족으로 취소될 때만 놓인다 | [D-TPSL] | "children TP/SL orders associated with a parent order will be placed if and only if the parent order fully fills or is partially filled followed by a cancelation" | 문서 명시 |
| 상태 사유 값들(`marginCanceled`, `openInterestCapCanceled`, `selfTradeCanceled`, `reduceOnlyCanceled`, `siblingFilledCanceled`, `scheduledCancel` 등) | [D-INFO] | D절 표 참고 | 문서 명시 |

### J. 서명

| 주장 | 출처 | 인용 | 확신도 |
|---|---|---|---|
| **문서는 서명 절차를 규정하지 않고 SDK 를 보라고 한다** | [D-SIGN] | "It is recommended to use an existing SDK instead of manually generating signatures." | 문서 명시 |
| 디버깅 방법도 SDK 대조 | [D-SIGN] | "it is recommended to read through the Python SDK carefully and make sure the implementation matches exactly." | 문서 명시 |
| 서명 방식이 두 가지 | [D-SIGN] | "Not realizing that there are two signing schemes (the Python SDK methods are `sign_l1_action` vs `sign_user_signed_action`)." | 문서 명시 |
| 함정: msgpack 필드 순서 | [D-SIGN] | "Not realizing that the order of fields matter for msgpack." | 문서 명시 |
| 함정: 뒤따르는 0 | [D-SIGN] | "Issues with trailing zeroes on numbers." | 문서 명시 |
| 함정: 주소 대문자 | [D-SIGN] | "It is recommended to lowercase any address before signing and sending." | 문서 명시 |
| 함정: 로컬 recover 가 맞아도 서버가 구성하는 페이로드와 다를 수 있다 | [D-SIGN] | "The payload for recover signer is constructed based on the action and does not necessarily match." | 문서 명시 |
| 함정: false 인 선택 플래그(`f`, `a`)는 생략해야 해시가 맞는다 | [D-EX] | "actions hashed with `f: false` will be rejected." | 문서 명시 |
| L1 액션 해시 = keccak( msgpack(action) ‖ nonce(8바이트 BE) ‖ vault 표식 ‖ [expiresAfter] ) | [S-PY-SIGN] | `data = msgpack.packb(action)` / `data += nonce.to_bytes(8, "big")` | SDK 구현에서 확인 |
| vault 표식: 없으면 `0x00`, 있으면 `0x01` + 20바이트 주소 | [S-PY-SIGN] | `if vault_address is None: data += b"\x00" else: data += b"\x01"; data += address_to_bytes(vault_address)` (원문은 여러 줄) | SDK 구현에서 확인 |
| expiresAfter 가 있으면 `0x00` + 8바이트 BE 추가 | [S-PY-SIGN] | `data += b"\x00"` / `data += expires_after.to_bytes(8, "big")` | SDK 구현에서 확인 |
| "phantom agent": `source` 는 mainnet `"a"`, testnet `"b"`; `connectionId` 는 위 해시 | [S-PY-SIGN] | `return {"source": "a" if is_mainnet else "b", "connectionId": hash}` | SDK 구현에서 확인 |
| EIP-712 도메인 (L1 액션) | [S-PY-SIGN] | `"chainId": 1337, "name": "Exchange", "verifyingContract": "0x0000000000000000000000000000000000000000", "version": "1"` | SDK 구현에서 확인 |
| primaryType `Agent(string source, bytes32 connectionId)` | [S-PY-SIGN] | `"Agent": [ {"name": "source", "type": "string"}, {"name": "connectionId", "type": "bytes32"} ]` | SDK 구현에서 확인 |
| Rust SDK 도 동일한 도메인/소스 | [S-RS] `signature/agent.rs`, `create_signature.rs` | `name: "Exchange", version: "1", chain_id: 1337, verifying_contract: Address::ZERO` / `let source = if is_mainnet { "a" } else { "b" }.to_string();` | SDK 구현에서 확인 |
| Rust SDK 의 msgpack 은 named(맵) 직렬화, 서명 `v` 는 27 + parity | [S-RS] `exchange_client.rs` | `rmp_serde::to_vec_named(self)` / `state.serialize_field("v", &(27 + sig.v() as u64))?;` | SDK 구현에서 확인 |
| 서명 JSON 형태 | [S-PY-SIGN] | `return {"r": to_hex(signed["r"]), "s": to_hex(signed["s"]), "v": signed["v"]}` | SDK 구현에서 확인 |
| user-signed 액션(approveAgent 등)은 다른 도메인 `HyperliquidSignTransaction`, `hyperliquidChain` 은 `"Mainnet"`/`"Testnet"` | [S-PY-SIGN], [D-EX] | `"hyperliquidChain": "Mainnet" (on testnet use "Testnet" instead)` | 문서 명시 (필드) + SDK 구현에서 확인 (도메인) |
| `signatureChainId` 는 서명에 쓴 체인 ID (문서 예 `0xa4b1`, SDK 는 `0x66eee` 고정) | [D-EX], [S-PY-SIGN] | `# signatureChainId is the chain used by the wallet to sign and can be any chain.` | 문서 명시 + SDK 구현에서 확인 |
| testnet/mainnet 간 재생 방지 | [S-PY-SIGN] | `# hyperliquidChain determines the environment and prevents replaying an action on a different chain.` | SDK 구현에서 확인 (L1 액션은 source `a`/`b` 가 같은 역할) |
| 숫자 와이어 변환: 소수 8자리 고정 후 정규화(뒤 0 제거) | [S-PY-SIGN], [S-RS] `helpers.rs` | `rounded = f"{x:.8f}"` ... `normalized = Decimal(rounded).normalize()` | SDK 구현에서 확인 |
| 필드 순서 (주문): `a, b, p, s, r, t, [c]`; 액션: `type, orders, grouping, [builder]` | [S-PY-SIGN] | `order_wire: OrderWire = {"a": asset, "b": ..., "p": ..., "s": ..., "r": ..., "t": ...}` | SDK 구현에서 확인 |
| 서명 테스트 벡터 존재 (cloid 포함 주문, vault, tpsl, mainnet/testnet) | [S-PY-EXM] `tests/signing_test.py` | `def test_l1_action_signing_order_with_cloid_matches():` | SDK 구현에서 확인 (Rust 구현 검증용으로 그대로 사용 가능) |
| chain ID 1337, source "a"/"b" 가 공식 문서에 적혀 있는가 | — | — | 문서에 명시 없음 (SDK 에만 있음) |

### K. 엔드포인트 / testnet

| 주장 | 출처 | 인용 | 확신도 |
|---|---|---|---|
| REST mainnet / testnet | [D-API] | "All example API calls use the mainnet url (<https://api.hyperliquid.xyz>), but you can make the same requests against testnet using the corresponding url (<https://api.hyperliquid-testnet.xyz>)" | 문서 명시 |
| 경로: `/info`, `/exchange` | [D-EX], [D-INFO] | "`POST` `https://api.hyperliquid.xyz/exchange`" | 문서 명시 |
| WS | [D-WS] | "Mainnet: `wss://api.hyperliquid.xyz/ws`" / "Testnet: `wss://api.hyperliquid-testnet.xyz/ws`." | 문서 명시 |
| testnet faucet 조건: **같은 주소로 mainnet 입금 이력 필요** | [D-FAUCET] | "To use the testnet faucet, you need to have deposited on mainnet with the same address." | 문서 명시 |
| 지급량 | [D-FAUCET] | "Users who have deposited on mainnet may then receive 1,000 mock USDC for testnet use: <https://app.hyperliquid-testnet.xyz/drip>" | 문서 명시 |
| 이메일 로그인은 mainnet/testnet 주소가 다르다 | [D-FAUCET] | "Privy generates a different wallet address for mainnet and testnet." | 문서 명시 |
| 차이 1: asset ID 가 다르다 | [D-AID] | "mainnet and testnet have different asset IDs." | 문서 명시 |
| 실측: testnet `meta.universe[0]` 은 SOL, BTC 는 인덱스 3 (`szDecimals` 5). 문서는 mainnet 에서 `BTC = 0` 이라고 함 | [M-TESTNET], [D-AID] | "E.g. `BTC = 0` on mainnet." | 실측(비인증) + 문서 명시 → **asset 인덱스를 하드코딩하면 안 된다** |
| 차이 2: 부분 청산 기준 | [D-LIQ] | "100k USDC (10k USDC on testnet for easier testing)" | 문서 명시 |
| 차이 3: margin tier | [D-TIER] | "The tiers on testnet are lower than mainnet, for ease of testing." | 문서 명시 |
| 차이 4: 서명 source `"b"`, `hyperliquidChain: "Testnet"` | [S-PY-SIGN], [D-EX] | (J절) | SDK 구현에서 확인 / 문서 명시 |
| 차이 5: 일부 기능은 testnet 전용 | [D-IDX] | "HIP-4 deployer actions ... Testnet-only" | 문서 명시 |
| rate limit·보존 한도·지연이 testnet 에서 다른지, testnet 초기화(reset) 정책, testnet 유동성 | — | — | 문서에 명시 없음 |
| 신규 계정 활성화 수수료 | [D-ACT] | "New HyperCore accounts require 1 quote token (e.g., 1 USDC, 1 USDT) of fees for the first transaction which has the new account as destination address." | 문서 명시 |

### L. 전달 보장 / 순서 / exactly-once

| 주장 | 출처 | 인용 | 확신도 |
|---|---|---|---|
| "exactly-once", "at-least-once", "idempotent", 시퀀스 번호, 메시지 순서 보장에 대한 서술 | — | — | **문서에 명시 없음** (54쪽 전체에서 `idempot`, `exactly.once`, `at.least.once`, `sequenc`(메시지 의미) 검색 0건) |
| 문서가 말하는 "guarantee" 는 두 곳뿐이며 둘 다 **보장하지 않는다**는 문장 | [D-LAT], [D-HIST] | "They are not trading advice and do not guarantee any latency or trading outcome." / "There is no guarantee of timely updates and data may be missing." | 문서 명시 |
| 실행 측 유일한 중복 방지 장치로 문서화된 것은 nonce | [D-NONCE] | "also never have been used before." | 문서 명시 |
| 전달 측: 스냅샷으로 누락을 메우고, 중복 제거는 수신자 몫 | [D-WS], [D-SUB] | "can be ignored if the previous messages were already processed." | 문서 명시 |
| 전달 측: 단절은 예고 없이 발생 | [D-WS] | "Disconnection from API servers may happen periodically and without announcement." | 문서 명시 |
| 체인 수준 순서는 합의로 단일화 | [D-OV] | "one consistent order of transactions achieved through HyperBFT consensus." | 문서 명시 |
| 전송 순서와 체인 순서의 관계는 통계적 서술뿐 | [D-LAT] | "users should be able to run experiments with <10ms variation between send times and see predictable ordering on the L1." | 문서 명시 (보장이 아니라 관찰 권유) |
| 응답은 커밋 후에 온다 (응답을 받은 액션의 결과는 확정) | [D-APISRV] | "Once the transaction has been included in a committed block on the L1, the API server responds ..." | 문서 명시 |
| 응답을 못 받은 액션에 대한 계약 | — | — | 문서에 명시 없음 |

---

## 3. OMS 설계에 대한 함의

표의 "지원하는 것" 은 위 근거에서 직접 나온 것이고, "지원하지 않는 것" 은 문서가 침묵하거나 반대로 말하는 것이라 **OMS 가 가정해서는 안 되는 것**이다.

| 설계 질문 | 베뉴 계약이 지원하는 것 | 지원하지 않는 것 (가정 금지) |
|---|---|---|
| **(i) 결과를 모르는 주문을 cloid 조회로 결정적으로 해소할 수 있는가** | `orderStatus` 가 cloid 로 조회되고, 있으면 전체 상태(거부 사유 포함)를, 없으면 `unknownOid` 를 준다 (D). 응답을 받은 액션은 커밋 후 결과다 (H). 미결 상태를 **닫는 도구가 두 가지 문서화**되어 있다: ① `expiresAfter` — 그 시각 뒤에는 액션이 거부된다 (A). ② `noop` — 같은 서명자·같은 nonce 로 보내 nonce 를 소진시켜 in-flight 주문을 무효화한다. 문서가 직접 권하는 용도다 (B). 따라서 "원 주문에 `expiresAfter` 를 달고, 타임아웃 시 같은 nonce 로 `noop` 을 보내 그 응답을 확인한 뒤 cloid 를 조회" 하면 그 nonce 로는 더 이상 아무것도 적용될 수 없다 (추론) | **`unknownOid` 는 부재의 증명이 아니다.** 잔여 공백: (a) 요청이 아직 mempool/전송 중일 수 있다 — FAQ 는 지연된 주문이 나중에 놓일 수 있다고 쓴다 (H). nonce 창이 `(T-2일, T+1일)` 이라 `expiresAfter` 가 없으면 이론상 이틀까지 유효하다. (b) info 는 API 서버의 로컬 상태라 방금 커밋된 블록보다 뒤처질 수 있다 (추론, 지연 한도 미기재). (c) cloid 유일성 범위가 문서에 없어, cloid 가 재사용되었다면 조회 결과가 어느 주문인지 모호하다. (d) pre-validation·서명·nonce 오류로 거부된 액션이 주문 기록을 남기는지 미기재. (e) `orderStatus` 보존 기간 미기재. (f) `expiresAfter` 의 기준 시계 미기재. (g) 이미 쓰인 nonce 로 보낸 `noop` 의 응답 형태 미기재 — "noop 실패 = 원 주문 적용됨" 으로 단정할 수 없다. (h) HIP-3 dex 주문은 `unknownOid` 가 나온다는 사용자 보고 (2차 자료) |
| **(ii) 동일한 서명 페이로드를 다시 보내도 안전한가** | 이중 적용에 대해서는 안전하다고 **추론**된다: 같은 서명자·같은 nonce 는 "never have been used before" 조건으로 한 번만 유효하다 (C). 100개 집합에서 밀려난 오래된 nonce 도 "집합의 최솟값보다 커야 한다" 조건으로 거부된다 | 문서가 "재전송은 안전하다" 고 쓴 적은 없다. 두 번째 제출이 받는 응답(오류 문자열, 원래 결과를 돌려주는지)이 미기재라, **재전송으로 원 요청의 결과를 알아낼 수는 없다** — 결국 조회가 필요하다. agent 가 prune 되면 과거 서명 액션이 재생될 수 있다고 문서가 명시한다 → agent 키를 교체/만료시킨 뒤에는 과거 페이로드를 절대 재전송하지 말 것, agent 주소 재사용 금지. 재전송은 주소 기반 rate limit 을 소모할 수 있다 (소모 여부 미기재) |
| **(iii) 같은 cloid 에 새 nonce 로 다시 보내도 안전한가** | 지원 근거 없음 | **안전하다고 가정하면 안 된다.** 새 nonce = 새 트랜잭션이고, cloid 중복 거부는 공식 문서·오류 표·공식 SDK 어디에도 없다. 중복 주문이 두 건 놓일 가능성을 배제할 수 없다. cloid 는 "조회·취소 키" 로만 문서화되어 있고 "멱등 키" 로는 문서화되어 있지 않다. 재시도가 필요하면 (i) 의 절차로 원 nonce 를 먼저 닫은 뒤 **새 cloid** 로 보내는 설계가 문서 근거와 합치한다 |
| **(iv) 단절 후 완전하고 중복 없는 체결 이력을 얻는 방법** | ① WS `userFills` 재구독 → 첫 메시지 `isSnapshot:true` 가 놓친 구간을 포함한다고 문서가 말한다 (E). ② REST `userFillsByTime(startTime=마지막 처리 시각)` 로 보충: inclusive 범위, 응답당 최대 2000건, 마지막 timestamp 를 다음 `startTime` 으로 (D). ③ 중복 제거 키: fill 의 `tid`. 문서는 전역 유일 키로 `(block_time, coin, tid)` 를 제시한다. ④ `aggregateByTime` 을 끄면 부분 체결이 개별로 온다. ⑤ 외부 원인 변화는 fill 의 `liquidation`, `userEvents`(liquidation, nonUserCancel), `userNonFundingLedgerUpdates`, `userFundings` 로 따로 수집. ⑥ 최종 대사는 `clearinghouseState` 포지션과 비교 | 시퀀스 번호가 없어 **누락을 스트림만으로 탐지할 수 없다.** 스냅샷 범위 미기재 → 긴 단절에서는 스냅샷만 믿으면 안 된다. REST 로도 **최근 10000건까지만** 조회된다 — 그보다 오래 끊기면 API 로는 복원 불가 (S3/제3자뿐, 완전성 보장 없음). 페이지네이션 한도가 500 과 2000 으로 엇갈린다. inclusive 경계라 페이지 경계에서 중복이 생기고, 같은 ms 에 한도보다 많은 fill 이 있으면 진행이 막힐 수 있다 (추론). `orderUpdates` 는 스냅샷이 문서화되지 않았다 → 재접속 후 주문 상태는 `openOrders`/`orderStatus`/`historicalOrders`(최근 2000건)로 재구성해야 한다. fill 의 cloid 는 문서 타입에 없다 (Rust SDK 타입에만) → fill 귀속은 `oid` 로 하고 oid↔cloid 매핑을 OMS 가 보관. ADL 의 표현 방식 미기재. `closedPnl`/`dir` 은 프런트엔드 편의값 |
| **(v) 처리량의 상한은 무엇인가** | IP: REST weight 1200/분, 액션 weight `1 + floor(n/40)` → 단건 요청이면 분당 최대 1200건(초당 20건), 39건 배치면 weight 1 로 분당 최대 46,800 주문 (계산). WS: 송신 2000 메시지/분, inflight post 100. 주소: 누적 거래 1 USDC 당 1 요청 + 초기 10000, 배치는 n건으로 계산, 취소는 `min(limit+100000, limit*2)`. 열린 주문 1000(최대 5000). 필요 시 요청당 0.0005 USDC 로 예약 구매. 지연 중앙값 0.2초, p99 0.9초 | **신규/저거래 계정의 실질 상한은 IP 가 아니라 주소 한도다**: 10000건을 다 쓰면 10초에 1건. 체결 없이 주문·취소만 반복하는 흐름은 한도를 회복하지 못한다. 조회도 같은 IP weight 를 쓴다(`orderStatus` 2, `openOrders`/`userFills` 20 + 건수 가중) → 대사 폴링이 주문 예산을 깎는다. unified/portfolio margin 계정은 일 50k 액션. 혼잡 시 블록 공간이 전일 maker 점유율의 2배로 제한된다. 만료(`expiresAfter`)된 액션은 5배 소모. 한도 초과 시 응답 형태가 미기재라 **오류 문자열 매칭으로 rate limit 을 판별하는 설계는 문서 근거가 없다.** WS post 가 REST weight 와 합산되는지 미기재. 서명자당 nonce 는 프로세스 간 공유하면 충돌한다 → 프로세스당 agent 1개 |
| 취소의 멱등성 | 취소는 oid 또는 `asset+cloid` 로 지정. 이미 없는 주문 취소는 주문별 오류로 온다 | 오류 문자열 "Order was never placed, already canceled, or filled." 는 세 경우를 **구분하지 않는다** → 취소 실패 후 최종 상태는 `orderStatus` 로 확인해야 한다. `cancelByCloid` 응답 예시 미기재 |
| 정정(modify) 추적 | oid 또는 cloid 로 대상 지정, 새 주문에 cloid 지정 가능 | oid 유지 여부, 부분 체결분 승계, 실패 시 원 주문 생존 여부(`always_place` 미지정 시) 미기재 → modify 를 "같은 주문의 변경" 으로 모델링하지 말고 **취소 + 신규**로 모델링하는 것이 문서 문구("regardless of whether the cancel succeeded")와 합치한다 |
| 데드맨 스위치 | `scheduleCancel(time)` 을 주기적으로 갱신, 최소 5초 뒤, 하루 10회 발동 | 10회 소진 후 동작 미기재 → 발동을 일상적 수단으로 쓰면 안 된다. 포지션은 청산하지 않는다(주문만 취소). 범위·트리거 주문 포함 여부 미기재 |
| 주문 상태 기계 | 종결 상태가 사유별로 세분화되어 있다 (D 표). 베뉴가 사용자 요청 없이 취소하는 경로가 다수 문서화: margin, OI cap, STP, reduce-only, sibling, delist, liquidation, scheduled | OMS 는 "내가 취소하지 않았으면 살아 있다" 고 가정하면 안 된다. 부분 체결 전용 상태값이 없다. `sz` 의미가 문서 내에서 엇갈린다 |
| 수량·가격 검증 | 5 유효숫자 / `6 - szDecimals` 소수 / `szDecimals` / 최소 $10 규칙이 문서화 → 전송 전 로컬 검증 가능 | tick 위반이 주문별 오류인지 배치 전체 오류인지 경우에 따라 다르다. 서버가 대신 반올림해 준다고 가정 금지 |
| Rust 구현 선택 | 서명 절차는 Python SDK 가 사실상의 명세이며 테스트 벡터가 있다 | 공식 `hyperliquid-rust-sdk` 는 11개월간 무커밋이고 `expiresAfter`·`noop`·`f`·`a` 가 없다 — (i) 의 해소 절차에 필요한 두 기능이 모두 빠져 있다. 공식 문서가 링크하는 Rust SDK 는 커뮤니티의 `hypersdk`(MPL-2.0) 다. 어느 쪽이든 서명 해시는 Python 테스트 벡터로 검증해야 한다 |

**로컬 Go 어댑터와의 대조 (1차 자료 아님, 참고)**

- `lookup.go` 는 `unknownOid` 를 "Unknown" 으로 돌려준다 — 부재로 단정하지 않는 점은 위 (i) 와 합치.
- `lookup.go` 는 체결 수량을 `origSz - sz` 로 계산한다 — `sz` 의미가 문서에서 엇갈리므로 실험 확인 필요.
- `lookup.go` 는 `userFills`(최근 2000건)에서 해당 oid 의 fill 을 모은다 — 체결이 2000건 넘게 쌓인 뒤의 조회는 불완전할 수 있다.
- `order.go` 는 `expiresAfter` 를 쓰지 않는다 — 타임아웃 후 주문이 늦게 놓일 수 있는 창이 닫히지 않는다.
- `lookup.go` 의 상태 문자열 목록은 [D-INFO] 표 29종과 일치한다.

---

## 4. 실제 거래소로만 확인 가능한 항목 (testnet 실험 목록)

문서가 열어 둔 동작이다. 각 항목은 OMS 설계가 의존하기 전에 testnet 에서 확인해야 한다.

**멱등성·복구**

1. 같은 cloid 재제출: (a) 원 주문이 open 일 때, (b) filled/canceled 뒤, (c) 같은 배치 안에서, (d) 다른 asset 에서, (e) 다른 agent 로 서명해서. 거부되는가, 거부 문자열은 무엇인가, 두 주문이 다 놓이는가.
2. cloid 가 중복된 상태에서 `orderStatus(cloid)` 와 `cancelByCloid` 가 어느 주문을 가리키는가.
3. 동일 서명 페이로드(같은 nonce) 재전송 시 응답: HTTP 상태, 본문, 원 결과 반환 여부, 주소 rate limit 소모 여부.
4. 같은 nonce 로 `noop` 을 보냈을 때: 원 주문이 이미 적용된 경우와 아닌 경우의 응답 차이. `noop` 성공 뒤 원 주문이 결코 나타나지 않는지.
5. `expiresAfter`: 기준 시계(블록 시각?), 만료 시 응답/오류 문자열, 만료된 주문이 `orderStatus` 에 남는지, 5배 소모가 `userRateLimit` 에 어떻게 보이는지.
6. 타임아웃 재현: 응답 전에 연결을 끊은 요청이 얼마 뒤까지 적용될 수 있는가.
7. 응답 직후 `orderStatus`/`openOrders` 조회의 read-your-writes 지연 (API 서버 상태 지연의 실제 크기).
8. agent 를 재승인(같은 이름)하거나 만료시킨 뒤 과거 서명 페이로드가 재생되는지.

**주문 조회·상태**

9. 주문별 거부(`...Rejected`)된 주문이 cloid 로 조회되는가. pre-validation 거부, 서명 오류, nonce 오류는 기록을 남기는가.
10. `orderStatus` 보존 기간: 오래된 종결 주문이 언제 `unknownOid` 가 되는가.
11. `order.sz` 의 의미: 부분 체결 후 취소된 주문에서 `sz` 가 잔량인가 체결량인가.
12. IOC 부분 체결의 제출 응답 형태와 최종 status (`filled`? `canceled`? `iocCancelRejected`?).
13. `waitingForFill` / `waitingForTrigger` 응답이 언제 나오는가.
14. 배치: `statuses` 순서가 요청 순서와 같은가, 일부 실패 시 나머지는 놓이는가, pre-validation 단일 오류의 JSON 형태, 최상위 `status:"err"` 가 나오는 조건.
15. modify: oid 가 바뀌는가, cloid 가 승계되는가, 부분 체결분이 어떻게 되는가, `always_place` 유무에 따른 실패 시 원 주문 상태, `batchModify` 원자성.
16. `cancelByCloid` 의 성공/실패 응답 형태.
17. HIP-3 dex 주문의 `orderStatus` 가 `unknownOid` 인지 (2차 자료 보고 확인).

**체결·스트림**

18. fill 에 `cloid` 가 실제로 오는가 (WS `userFills`, `userEvents`, REST `userFills`).
19. `tid` 유일성: 한 주문의 여러 부분 체결, 같은 두 주문 간 반복 체결에서 `tid` 가 달라지는가. 한 트랜잭션의 여러 fill 이 같은 `hash` 를 갖는가.
20. `userFills` 스냅샷의 크기와 재접속 시 중복 범위. `orderUpdates` 구독 시 스냅샷이 오는가.
21. `orderUpdates` 와 `userFills` 의 도착 순서, 한 채널 안의 순서, 스트림 메시지 중복 여부.
22. `userEvents.fills` 와 `userFills` 가 같은 fill 을 이중 전달하는가.
23. `userFillsByTime` 페이지네이션: 실제 한도(500 vs 2000), inclusive 경계 중복, 같은 ms 다건 처리.
24. STP 발생 시 resting 쪽이 받는 이벤트 (`selfTradeCanceled` orderUpdate, `nonUserCancel` 여부). 서브계정 간 STP 적용 여부.
25. 청산·ADL·delisting 이 fill/이벤트에 어떻게 표현되는가 (`dir`, `oid`, `liquidation` 필드). testnet 은 부분 청산 기준이 10k USDC 라 재현이 쉽다.
26. funding 지급의 실제 timestamp 와 `userFundings` 도착 시점.

**한도·오류**

27. IP weight 초과 시 HTTP 상태(429?)·본문·회복 시간. WS post 가 REST weight 에 합산되는가.
28. 주소 한도 소진 시 응답/오류 문자열, 거부된 주문도 한도를 소모하는가, 취소 여유분의 실제 동작.
29. 열린 주문 한도 초과 시 오류 문자열.
30. 수량 자릿수 초과 시 거부 문자열. tick 위반이 주문별 오류로 오는 경우와 배치 전체 오류로 오는 경우.
31. 최소 $10 의 기준 가격, reduce-only 청산 주문 예외 여부.
32. WS: post 응답 전 연결이 끊긴 액션의 운명, 60초 무메시지 단절의 실제 동작, 서버 주도 단절 빈도.
33. `scheduleCancel`: 트리거 주문 포함 여부, 10회 소진 후 동작, `vaultAddress` 별 범위.

**환경**

34. testnet 의 rate limit·보존 한도·지연이 mainnet 과 같은가. testnet 초기화 주기.
35. 네트워크 업그레이드(post-only 기간) 중 API 응답과 통지 경로.

---

[D-IDX]: https://hyperliquid.gitbook.io/hyperliquid-docs/llms.txt
[D-API]: https://hyperliquid.gitbook.io/hyperliquid-docs/for-developers/api
[D-NOT]: https://hyperliquid.gitbook.io/hyperliquid-docs/for-developers/api/notation
[D-AID]: https://hyperliquid.gitbook.io/hyperliquid-docs/for-developers/api/asset-ids
[D-TICK]: https://hyperliquid.gitbook.io/hyperliquid-docs/for-developers/api/tick-and-lot-size
[D-NONCE]: https://hyperliquid.gitbook.io/hyperliquid-docs/for-developers/api/nonces-and-api-wallets
[D-INFO]: https://hyperliquid.gitbook.io/hyperliquid-docs/for-developers/api/info-endpoint
[D-PERP]: https://hyperliquid.gitbook.io/hyperliquid-docs/for-developers/api/info-endpoint/perpetuals
[D-EX]: https://hyperliquid.gitbook.io/hyperliquid-docs/for-developers/api/exchange-endpoint
[D-WS]: https://hyperliquid.gitbook.io/hyperliquid-docs/for-developers/api/websocket
[D-SUB]: https://hyperliquid.gitbook.io/hyperliquid-docs/for-developers/api/websocket/subscriptions
[D-POST]: https://hyperliquid.gitbook.io/hyperliquid-docs/for-developers/api/websocket/post-requests
[D-HB]: https://hyperliquid.gitbook.io/hyperliquid-docs/for-developers/api/websocket/timeouts-and-heartbeats
[D-ERR]: https://hyperliquid.gitbook.io/hyperliquid-docs/for-developers/api/error-responses
[D-SIGN]: https://hyperliquid.gitbook.io/hyperliquid-docs/for-developers/api/signing
[D-RATE]: https://hyperliquid.gitbook.io/hyperliquid-docs/for-developers/api/rate-limits-and-user-limits
[D-ACT]: https://hyperliquid.gitbook.io/hyperliquid-docs/for-developers/api/activation-gas-fee
[D-LAT]: https://hyperliquid.gitbook.io/hyperliquid-docs/for-developers/api/optimizing-latency
[D-PRIO]: https://hyperliquid.gitbook.io/hyperliquid-docs/for-developers/api/priority-fees
[D-L1]: https://hyperliquid.gitbook.io/hyperliquid-docs/for-developers/nodes/l1-data-schemas
[D-FEE]: https://hyperliquid.gitbook.io/hyperliquid-docs/trading/fees
[D-SUBACC]: https://hyperliquid.gitbook.io/hyperliquid-docs/trading/sub-accounts
[D-BLD]: https://hyperliquid.gitbook.io/hyperliquid-docs/trading/builder-codes
[D-SPEC]: https://hyperliquid.gitbook.io/hyperliquid-docs/trading/contract-specifications
[D-MARGIN]: https://hyperliquid.gitbook.io/hyperliquid-docs/trading/margining
[D-AAM]: https://hyperliquid.gitbook.io/hyperliquid-docs/trading/account-abstraction-modes
[D-TIER]: https://hyperliquid.gitbook.io/hyperliquid-docs/trading/margin-tiers
[D-PX]: https://hyperliquid.gitbook.io/hyperliquid-docs/trading/robust-price-indices
[D-LIQ]: https://hyperliquid.gitbook.io/hyperliquid-docs/trading/liquidations
[D-ADL]: https://hyperliquid.gitbook.io/hyperliquid-docs/trading/auto-deleveraging
[D-FUND]: https://hyperliquid.gitbook.io/hyperliquid-docs/trading/funding
[D-TOB]: https://hyperliquid.gitbook.io/hyperliquid-docs/trading/order-book
[D-OT]: https://hyperliquid.gitbook.io/hyperliquid-docs/trading/order-types
[D-TPSL]: https://hyperliquid.gitbook.io/hyperliquid-docs/trading/take-profit-and-stop-loss-orders-tp-sl
[D-PNL]: https://hyperliquid.gitbook.io/hyperliquid-docs/trading/entry-price-and-pnl
[D-STP]: https://hyperliquid.gitbook.io/hyperliquid-docs/trading/self-trade-prevention
[D-DELIST]: https://hyperliquid.gitbook.io/hyperliquid-docs/trading/delisting
[D-MM]: https://hyperliquid.gitbook.io/hyperliquid-docs/trading/market-making
[D-FAUCET]: https://hyperliquid.gitbook.io/hyperliquid-docs/onboarding/testnet-faucet
[D-APISRV]: https://hyperliquid.gitbook.io/hyperliquid-docs/hypercore/api-servers
[D-HOB]: https://hyperliquid.gitbook.io/hyperliquid-docs/hypercore/order-book
[D-CH]: https://hyperliquid.gitbook.io/hyperliquid-docs/hypercore/clearinghouse
[D-OV]: https://hyperliquid.gitbook.io/hyperliquid-docs/hypercore/overview
[D-HIST]: https://hyperliquid.gitbook.io/hyperliquid-docs/historical-data
[D-FAQ-EXP]: https://hyperliquid.gitbook.io/hyperliquid-docs/support/faq/trade-outcome-looks-incorrect/what-does-action-already-expired-mean
[D-FAQ-API]: https://hyperliquid.gitbook.io/hyperliquid-docs/support/faq/building-on-hyperliquid/i-have-api-related-questions
[S-PY]: https://github.com/hyperliquid-dex/hyperliquid-python-sdk/tree/2fdb18f9517675ea03695a0962bd19eece9c83f0
[S-PY-SIGN]: https://github.com/hyperliquid-dex/hyperliquid-python-sdk/blob/2fdb18f9517675ea03695a0962bd19eece9c83f0/hyperliquid/utils/signing.py
[S-PY-EX]: https://github.com/hyperliquid-dex/hyperliquid-python-sdk/blob/2fdb18f9517675ea03695a0962bd19eece9c83f0/hyperliquid/exchange.py
[S-PY-INFO]: https://github.com/hyperliquid-dex/hyperliquid-python-sdk/blob/2fdb18f9517675ea03695a0962bd19eece9c83f0/hyperliquid/info.py
[S-PY-TYPES]: https://github.com/hyperliquid-dex/hyperliquid-python-sdk/blob/2fdb18f9517675ea03695a0962bd19eece9c83f0/hyperliquid/utils/types.py
[S-PY-WS]: https://github.com/hyperliquid-dex/hyperliquid-python-sdk/blob/2fdb18f9517675ea03695a0962bd19eece9c83f0/hyperliquid/websocket_manager.py
[S-PY-API]: https://github.com/hyperliquid-dex/hyperliquid-python-sdk/blob/2fdb18f9517675ea03695a0962bd19eece9c83f0/hyperliquid/api.py
[S-PY-EXM]: https://github.com/hyperliquid-dex/hyperliquid-python-sdk/tree/2fdb18f9517675ea03695a0962bd19eece9c83f0/examples
[S-RS]: https://github.com/hyperliquid-dex/hyperliquid-rust-sdk/tree/aac75585daf12d0a3761126cc7da7a5e035b5853
[S-RS-PR198]: https://github.com/hyperliquid-dex/hyperliquid-rust-sdk/pull/198
[X-HSDK]: https://github.com/infinitefield/hypersdk
[X-ISS253]: https://github.com/hyperliquid-dex/hyperliquid-python-sdk/issues/253
