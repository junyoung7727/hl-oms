# 증거금 규칙 계약

상태: 채택, 구현됨(2026-11-16, 티켓 01~12. "계좌 유지증거금률"은 2026-11-21, U17d). 가상 거래소와 시뮬레이션으로 시험했다. **실제 거래소에서는 아무것도 확인하지 않았다**: 이 계약의 조회는 testnet에도 보낸 적이 없고, 규칙이 읽는 숫자의 뜻은 가정 T1~T11이다("검증"의 끝).
모든 절이 코드에 있고, 이벤트와 주장(V-MARGIN-1~12)이 `registry/`에 있다. 티켓별로 들어온 것:

- 선택 구역 `policy.margin`과 그 기본값, "차단 표"의 `Resync`, `InDoubt`, `Mismatch`(티켓 01, V-MARGIN-1).
- 입력 `MarginReport`, `MarginReportInvalid`와 이벤트 `MarginReported`, `MarginReportRejected`, "신선도"의 보고 조건(일반 주문의 `MarginUnavailable`, 감소 전용 주문의 면제), 노드의 설정 검사 가운데 mainnet 조건과 값의 범위, `runtime.margin_poll_ms` 간격의 조회, `status`의 계좌 가치와 나이(티켓 02, V-MARGIN-2, V-MARGIN-11).
- "계좌 레버리지 검사" 전체와 거절 사유 `AccountLeverage`, "신선도"의 참조 가격 조건, `status`의 계좌 노출(티켓 03, V-MARGIN-3, V-MARGIN-11).
- "일 손실과 기준 금액" 전부(운영자가 기준을 정하는 `set_by: Operator`는 빼고), "감소 전용 상태"의 진입과 그 안의 규칙 취소(`Rule(DailyLoss)`), 차단 사유 `Reducing`과 "차단 표"의 그 행, 이벤트 `DayBaseSet`, `ReducingEntered`, `status`의 감소 전용 상태(`core.margin.reducing`: 규칙, 내용, 시각. 정상이면 null)와 기준 금액(`core.margin.day_base`)(티켓 04, V-MARGIN-4, V-MARGIN-5, V-MARGIN-11).
- "해제와 기준 재설정" 전부(유지증거금 여유 조건과 거절 사유 `MmBufferActive`는 티켓 07이 더했다), 운영자 입력 `ClearReduce`, `RebaseDay`, 이벤트 `ReducingCleared`, `DayBaseSet`의 `set_by: Operator`, 운영 소켓의 `clear_reduce`, `rebase_day`와 `omsctl clear-reduce`, `omsctl rebase-day`, "중단과 재실행"의 프로세스 시험(티켓 05, V-MARGIN-6, V-MARGIN-11).
- "보고 끊김 취소" 전부와 규칙 이름 `Rule(MarginStale)`(티켓 06, V-MARGIN-7).
- "거래소 설정과 계좌 모드" 전체: 입력 `VenueSetting`, `AccountMode`와 이벤트 `VenueSettingNoted`, `AccountModeNoted`, 차단 `VenueSetting(종목)`, `AccountMode`, 노드의 두 조회와 `runtime.venue_setting_poll_ms`, 가상 거래소의 응답과 제어 명령(`venue_setting`, `account_mode`), `status`의 종목별 거래소 설정과 계좌 모드(티켓 08, V-MARGIN-9, V-MARGIN-11).
- "유지증거금 여유" 전체: 받은 보고마다의 판정, `Rule(MmBuffer)`로 들어가는 감소 전용 상태와 그 이름의 규칙 취소, 조건이 참일 때의 해제 거절 `MmBufferActive`. `status`의 `core.margin.reducing.rule`은 `mm_buffer`가 된다(티켓 07, V-MARGIN-8, V-MARGIN-4, V-MARGIN-6).
- "Hyperliquid 조회" 전체: Hyperliquid 게이트웨이의 증거금 조회, 거래소 설정 조회, 계좌 모드 조회와 그 응답의 번역, 조회 weight 예산 검사, 증거금 정책을 담은 Hyperliquid 예제 설정(티켓 09, V-MARGIN-10, V-CFG-1). 실제 거래소에는 한 번도 묻지 않았다.
- 코드 검토(2026-11-16, 헤드 228373e. 티켓 12의 Comments)에서 고친 것: 정책이 없으면 감소 전용 상태가 주문을 막지 않는다("설정"). 받은 보고가 마지막 거부 사유를 지운다. 설정에 없는 종목의 보고 포지션은 거래소 설정으로 남기지 않는다. 대조기는 답이 없는 거래소 설정·계좌 모드 조회를 `venue_setting_poll_ms`에 많아야 한 번 되묻는다. 게이트웨이의 큐에 자리가 없어 나가지 못한 증거금 조회는 다음 바퀴에 나간다. simvenue의 증거금 응답에 포지션 목록이 없으면 보고를 거부한다. simvenue의 제어 명령 `margin_used`. 변이 M116~M122.
- "계좌 유지증거금률" 전체(U17d, [ADR-028](../adr/ADR-028-bybit-gateway.md) §7g): 정책 값 `account_mm_rate_max_bps`, 보고의 `account_mm`, `Rule(AccountMm)`으로 들어가는 감소 전용 상태와 그 이름의 규칙 취소, 한도가 있을 때 `account_mm` 없는 보고의 거부, 조건이 참일 때의 해제 거절 `AccountMmActive`, Bybit 게이트웨이의 두 필드 번역, 노드의 설정 검사(Bybit는 반드시, 다른 거래소는 거부). `status`의 `core.margin.account_mm`과 `core.margin.reducing.rule = account_mm`(V-MARGIN-13).
- 시뮬레이션의 증거금 모형과 검사: 가상 거래소(`oms-sim`의 `SimVenue`. simvenue 프로세스도 같은 것을 쓴다)가 계좌 가치를 자기 모형으로 계산하고(현금 + 포지션 × 기준 가격. 체결 대금과 수수료, 펀딩이 현금에서 나간다), 보고에 포지션(가치, 증거금, 증거금 방식, 레버리지, 최대 레버리지)을 담고, 감소 전용 주문이 포지션을 키우지 못하게 한다. 시뮬레이션 세계는 증거금 프로파일 `margin_calm`, `margin_lossy`에서 증거금 정책을 켜고 매 입력 뒤에 증거금 불변식을 검사한다(티켓 10, V-MARGIN-12, V-SIM-1, V-MARGIN-11). 프로파일 `margin_days`는 같은 검사를 계약 기본값의 증거금 시계로 가상 시간 50시간(UTC 자정 세 번) 동안 돌린다(티켓 11, V-MARGIN-12). 프로파일과 검사의 목록은 [simulation.md](../verification/simulation.md)에 있다.

가상 거래소에 대해 알아 둘 것: 계좌 가치와 포지션의 증거금은 우리가 만든 모형이고 거래소의 산술이 아니다. 포지션의 증거금은 가치 ÷ 레버리지이고 손실이 나도 줄지 않는다(모형은 진입 가격을 기억하지 않는다). 그래서 유지증거금 여유 규칙은 증거금을 일부러 다르게 말하게 할 때만 걸린다(시뮬레이션은 0으로, simvenue 프로세스는 제어 명령 `margin_used`가 정한 값으로). simvenue 프로세스는 참조 가격을 스스로 내지 않고 제어 명령(`ref_price`)을 받았을 때만 낸다(그 가격이 모형의 기준 가격도 된다). 그래서 simvenue에서 `policy.margin`을 켜면 그 명령으로 가격을 준 뒤 15초 동안만 일반 주문을 받는다. simvenue 프로세스의 접수 요청에는 감소 전용 표시가 없어서 프로세스는 모든 주문을 일반 주문으로 다룬다(감소 전용 주문의 거절은 시뮬레이션 세계에서만 일어난다). 가상 거래소는 모든 종목을 격리·레버리지 2로(제어 명령 `venue_setting`으로 바꿀 수 있다), 계좌 모드를 `standard`(가상 거래소가 붙인 이름이고 Hyperliquid의 값이 아니다)로 답한다. 기본 허용 목록은 비어 있으므로 simvenue에서 일반 주문을 내려면 `account_modes_allowed = ["standard"]`를 적어야 한다.

설계 근거: [ADR-018](../adr/ADR-018-margin-rules.md). 용어: [GLOSSARY.md](../../GLOSSARY.md). 스펙과 티켓: `.scratch/margin-rules/`.
실제 거래소에서는 아무것도 확인하지 않는다(U-01). 가정 T1~T11은 ADR-018 §8에 있다.

## C-MARGIN 증거금 규칙

### 설정

규칙은 `policy.margin`이 있을 때만 동작한다. 없으면 이 계약의 입력은 상태를 바꾸지 않고, 어떤 주문도 이 계약의 사유로 거절되지 않는다. 아래 "차단 표"도 같다: 정책이 없으면 모든 차단이 지금처럼 모든 신규 주문을 막는다.
감소 전용 상태인 채로 `policy.margin`이 없는 설정을 켜도 같다. 상태(`reducing`)는 저널에서 온 상태로 남지만 차단 `Reducing`은 계산되지 않고, 일반 주문을 전처럼 받는다. 정책이 있는 설정을 다시 켜면 같은 상태(같은 규칙, 같은 시각)가 다시 막고, 푸는 것은 운영자의 해제뿐이다. 정책이 없던 동안 받은 일반 주문은 정책이 돌아와도 규칙 취소를 받지 않는다(규칙 취소는 상태에 들어갈 때 한 번이다). 스펙은 이 경우를 말하지 않는다. 코드 검토(2026-11-16)에서 이렇게 정했다.
노드 설정 검사는 `mainnet_confirmed = true`인데 `policy.margin`이 없으면 시작을 거부한다.

| 값 | 기본 | 뜻 | 설정 검사 |
|---|---|---|---|
| `max_account_leverage` | 2.0 | 계좌 레버리지 한도. 고정소수점 정수로 저장 | 0보다 크다 |
| `daily_loss_bps` | 500 | 일 손실 한도. 기준 금액의 5 % | 0보다 크다 |
| `mm_buffer_bps` | 25 | 유지증거금 여유. 0.25 %p | 0 이상 |
| `margin_max_age_us` | 30 000 000 | 보고의 신선도 한도. 30초 | 0보다 크다 |
| `stale_cancel_us` | 60 000 000 | 보고 끊김 취소 한도. 60초 | `margin_max_age_us`보다 크다 |
| `venue_leverage` | 2 | 종목마다 거래소에 있어야 하는 레버리지 | 1 이상 |
| `isolated` | true | 종목마다 거래소에 있어야 하는 증거금 방식 | — |
| `account_modes_allowed` | 비어 있음 | 허용하는 계좌 모드 문자열 목록 | `unifiedAccount`, `portfolioMargin`은 적어도 허용되지 않는다 |
| `account_mm_rate_max_bps` | 없음(규칙 꺼짐, 쓰지 않음) | 교차 증거금 계좌의 유지증거금이 증거금 잔고의 이만큼(bps)에 닿으면 감소 전용 상태. Bybit의 값은 5000(50 %, 판단) | 있으면 1..=9999. 노드: Bybit에서 `policy.margin`이 있으면 반드시 있어야 하고, Hyperliquid와 가상 거래소는 거부한다(그 보고에는 계좌 숫자가 없다) |
| `runtime.margin_poll_ms` | 5 000 | 증거금 보고 조회 간격 | 0보다 크다 |
| `runtime.venue_setting_poll_ms` | 60 000 | 포지션이 없는 종목의 거래소 설정 조회 간격 | 0보다 크다 |

조회 weight 예산: 설정에서 계산한 분당 조회 weight(증거금 보고, 참조 가격, 15초 훑기, 종목별 거래소 설정)가 거래소 한도 1200의 절반인 600을 넘으면 설정 검사가 시작을 거부한다. 계산과 범위는 아래 "Hyperliquid 조회"에 있다.

### 입력, 상태, 이벤트

| 입력 | 조건 | 이벤트 | 바뀌는 상태 |
|---|---|---|---|
| `MarginReport { session, seq, queried_at, account_value, positions[], account_mm? }` | 링크가 올라와 있고, `session`이 현재 세션이고, `seq`가 이 세션에서 받은 마지막 보고의 `seq`보다 크다 | `MarginReported` | 마지막 보고(값, 조회 시각, 순번). 마지막 거부 사유를 지운다 |
| 같은 입력 | 위 조건이 아니다 | 없음(입력을 버린다) | 없음 |
| `MarginReportInvalid { session, seq, queried_at, reason }` | 링크가 올라와 있고 현재 세션이다 | `MarginReportRejected { reason }` | 마지막 거부 사유(다음에 받은 보고가 지운다). 마지막 보고는 그대로다 |
| `MarginReport`인데 어느 포지션의 `max_leverage`가 0 | 첫 행의 조건(링크, 세션, `seq`)이 맞다 | `MarginReportRejected` | 위와 같다. 코어가 직접 거부한다 |
| `MarginReport`인데 `account_mm_rate_max_bps`가 있고 `account_mm`이 없거나 그 유지증거금이 음수 | 첫 행의 조건이 맞다 | `MarginReportRejected` | 위와 같다. 코어가 직접 거부한다(사유 `account_mm is missing: …`, `account_mm: maintenance margin … is negative`). 한도가 없으면 `account_mm`은 보지 않는다 |
| `VenueSetting { session, instrument, isolated, leverage }` | 현재 세션 | `VenueSettingNoted` | 그 종목의 거래소 설정 |
| `AccountMode { session, mode }` | 현재 세션 | `AccountModeNoted` | 계좌 모드 |
| 운영자 `ClearReduce { note }` | 아래 "해제" | `DayBaseSet { set_by: Operator }`, `ReducingCleared { note, old_base, new_base }` | 기준 금액, 감소 전용 상태 해제 |
| 운영자 `RebaseDay { note }` | 아래 "기준 재설정" | `DayBaseSet { set_by: Operator }` | 기준 금액 |
| `Tick` | 아래 "보고 끊김 취소" | `CancelRequested { by: Rule(MarginStale) }` | 주문의 취소 요청 |

`positions[]`의 한 항목: `instrument, szi, position_value, margin_used, leverage_type, leverage_value, max_leverage`. `account_mm = { maintenance, margin_balance }`은 교차 증거금 계좌의 유지증거금과 증거금 잔고(USD)이고 Bybit 게이트웨이만 채운다. 없으면 저널에 쓰지 않는다(Hyperliquid 보고와 이 필드 전의 기록은 바이트가 같다. 기록 스키마는 2 그대로다). `szi`는 부호 있는 수량이다(코드의 이름은 `qty`. 저널과 `status`의 키는 `szi` 그대로다). 설정에 없는 종목의 포지션도 버리지 않는다.
받은 보고 하나는 한 입력 안에서 아래 순서로 처리된다: 기록 → 거래소 설정 갱신 → 유지증거금 여유 판정 → 계좌 유지증거금률 판정 → 일 손실 판정 → 날짜 넘김.
규칙 판정은 받은 보고마다 한다. 도착했을 때 이미 30초보다 오래된 보고도 판정한다(값은 거래소가 계산한 것이고 가진 것 중 가장 새것이다).

상태 소유자는 코어다. 모든 변경은 위 이벤트로만 일어나고 저널에서 복원된다. 링크가 바뀌면 거래소 설정과 계좌 모드는 "모름"이 된다. 마지막 보고, 기준 금액, 감소 전용 상태는 링크가 바뀌어도 남는다.

### 신선도

- 보고의 나이 = `at − queried_at`. 둘 다 노드의 시각이다.
- 신선하다: `0 ≤ 나이 ≤ margin_max_age_us`. 정확히 30초는 신선하다. `queried_at`이 `at`보다 미래면 신선하지 않다.
- 신선한 보고가 없으면 일반 주문을 `MarginUnavailable`로 거절한다. 감소 전용 주문은 신선한 보고가 필요 없다.
- 노출이 0이 아닌 종목 가운데 신선한 참조 가격이 없는 것이 하나라도 있으면 일반 주문을 `MarginUnavailable`로 거절한다.
  - 참조 가격이 신선하다: `at − 가격을 기록한 시각 ≤ policy.ref_price_max_age_us`(기본 15초). 가격대 검사가 쓰는 한도와 같다.
  - "노출이 0이 아닌 종목": 계좌 장부의 포지션이 0이 아니거나, 남은 예약 수량이 있는 주문이 있는 종목. 새 주문의 종목도 여기에 든다(그 주문이 노출이 된다). 포지션도 주문도 없는 종목은 가격이 없어도 된다.
  - 이것도 차단이 아니다. 새 가격이 오면 끝난다. 감소 전용 주문은 이 조건도 받지 않는다.

### 계좌 레버리지 검사

종목마다, `pos`는 계좌 장부의 포지션, `ref`는 참조 가격, 합은 그 종목의 모든 미체결 주문(감소 전용 포함)의 남은 예약 수량에 대한 것이다.

```
N_up   = max(0,  pos × ref + Σ_매수 예약 × max(지정가, ref))
N_down = max(0, −pos × ref + Σ_매도 예약 × max(지정가, ref))
종목 노출 = max(N_up, N_down)
계좌 노출 = 종목 노출의 합
```

- 일반 주문을 `AccountLeverage`로 거절하는 조건: `받은 뒤 노출 > max_account_leverage × 계좌 가치` 이고 `받은 뒤 노출 > 받기 전 노출`.
- 정확히 한도와 같으면 받는다. 계좌 가치가 0 이하면 일반 주문을 모두 `AccountLeverage`로 거절한다(노출을 올리지 않는 주문도).
- 계좌 가치는 신선한 보고의 값이다. 신선한 보고가 없으면 이 검사는 하지 않는다(그 주문은 이미 `MarginUnavailable`로 거절된다).
- 감소 전용 주문은 이 검사를 건너뛴다. 그래도 뒤에 오는 주문의 합에는 들어간다.
- 주문을 받기 전에만 검사한다. 가격이 움직여 한도를 넘어도 래치는 없고 취소도 없다.
- 취소를 요청한 것만으로는 노출이 줄지 않는다. 종결 증거가 와서 예약이 풀릴 때 줄어든다([C-CANCEL](order-lifecycle.md)).
- 성질: 주문을 더하면 노출은 줄지 않는다. 롱과 숏을 뒤집으면 노출이 같다.
- 계산: 가격(1e-8 단위 정수) × 수량(1e-8 단위 정수)의 `i128` 곱을 그대로 더하고 비교한다(단위 1e-16). 나눗셈과 반올림이 없다. 곱 하나는 항상 `i128`에 들어간다. 합이 `i128`에 들어가지 않으면 일반 주문을 기존 사유 `Overflow`로 거절한다. 값이 감겨서 주문이 통과하는 일은 없다.
- `status`의 계좌 노출(`core.margin.account_exposure`)은 같은 식을 종목마다 마지막 참조 가격으로(나이와 상관없이) 계산한 값이다. 노출이 있는 종목에 참조 가격이 한 번도 없었거나 합이 들어가지 않으면 null이다.

### 일 손실과 기준 금액

- 상태: `day_base = { day, value, set_by }` 또는 없음. `day = queried_at`의 UTC 날짜 번호.
- 받은 보고마다, 이 순서로:
  1. 기준이 있고 `(value − account_value) × 10 000 > daily_loss_bps × value`이면 감소 전용 상태로 들어간다(`Rule(DailyLoss)`).
  2. 기준이 없거나 `day(queried_at) > day_base.day`이면 `DayBaseSet { day, value: account_value, set_by: Report }`.
- 기준 100에서 95.00은 걸리지 않고 94.99는 걸린다.
- 자정을 넘는 공백(하루든 여러 날이든) 뒤의 첫 보고는 옛 기준으로 먼저 판정된다.
- 보고의 날짜가 기준의 날짜보다 앞서면 그 보고는 기준을 바꾸지 않는다(보고에 의한 날짜 넘김은 날짜를 뒤로 돌리지 않는다). 날짜를 앞선 날로 둘 수 있는 것은 운영자의 기준 재설정과 해제뿐이다(아래 "해제와 기준 재설정").
- 새 날은 감소 전용 상태를 풀지 않는다. 부팅은 기준을 바꾸지 않는다.
- 수수료와 펀딩은 계좌 가치에 들어 있으므로 손실로 센다. 출금도 손실로 읽힌다. 입금은 그만큼의 손실을 가린다.

### 유지증거금 여유

- 받은 보고의 격리 포지션 가운데 `position_value > 0`인 것마다 판정한다. 교차 증거금 포지션은 판정하지 않는다(그 종목은 `VenueSetting` 차단에 걸린다). 설정에 없는 종목의 포지션도 판정한다.
- 조건: `margin_used × 2 × max_leverage × 10 000 < position_value × (10 000 + 2 × max_leverage × mm_buffer_bps)`. 참이면 감소 전용 상태로 들어간다(`Rule(MmBuffer)`).
- `max_leverage` 40, 여유 25 bp, 포지션 가치 200: 격리 증거금 2.99는 걸리고 3.00은 걸리지 않는다. `max_leverage` 25면 선은 4.50, 50이면 2.50이다.
- 비교는 `i128` 곱이고 나눗셈이 없다. 왼쪽은 언제나 들어간다. 오른쪽 곱이 `i128`에 들어가지 않으면(여유가 수십억 bp인 설정에서만 생긴다) 선 아래로 본다. 음수 증거금은 언제나 선 아래다.
- 한 종목에서 걸려도 계좌 전체가 감소 전용 상태가 된다. 걸리는 포지션이 여럿이면 보고에 먼저 적힌 것 하나가 `detail`에 남는다: `position <종목>: isolated margin <증거금> is below the maintenance margin at maximum leverage <최대 레버리지> plus <여유> bps of the position value <가치>`.
- 도착했을 때 이미 30초보다 오래된 보고도 판정한다. 한 보고가 일 손실과 함께 걸면 이 규칙을 먼저 판정하므로 `MmBuffer`가 남는다.

### 계좌 유지증거금률

교차 증거금 계좌(Bybit)를 위한 규칙이다. 유지증거금 여유 규칙은 격리 포지션만 판정하므로 교차 계좌에서는 걸리지 않는다. 소유자 질문(11-20)에 11-21까지 답이 없어 질문에 적은 기본값(규칙을 더한다, 50 %)을 따랐다. 소유자가 규칙을 뺄 수 있다.

- 조건: `account_mm_rate_max_bps`가 있고, 받은 보고의 `account_mm`이 `maintenance > 0`이고 `maintenance × 10 000 ≥ account_mm_rate_max_bps × margin_balance`. 참이면 감소 전용 상태로 들어간다(`Rule(AccountMm)`).
- 비율 = 유지증거금 ÷ 증거금 잔고(Bybit `totalMaintenanceMargin / totalMarginBalance`). 나누지 않고 `i128` 곱으로 비교한다(곱 하나는 2^95보다 작아 언제나 들어간다). 정확히 한도와 같으면 건다.
- 한도 5000에서 잔고 1000: 유지증거금 499.99는 걸리지 않고 500은 건다. 한도 1에서 잔고 100: 0.00999999는 걸리지 않고 0.01은 건다.
- 잔고가 0 이하이고 유지증거금이 0보다 크면 어느 한도에서도 건다(거래소는 비율 100 %에서 청산한다). 유지증거금이 0이면 잔고가 얼마든 걸지 않는다(청산될 것이 없다. 잔고 0, 유지증거금 0도 걸지 않는다). 음수 유지증거금은 보고 전체의 거부다(위 "입력, 상태, 이벤트").
- `detail`: `account maintenance margin <유지증거금> is at or above <한도> bps of the margin balance <잔고>`.
- 한 보고가 여러 규칙을 걸면 위 순서대로 판정하므로 유지증거금 여유, 계좌 유지증거금률, 일 손실 순으로 먼저 건 것이 남는다. 이미 감소 전용 상태이면 아무것도 내지 않는다.
- 거래소의 `accountMMRate`는 읽지 않는다: 그 값이 비율인지 백분율인지 문서에 없다(F20 13.4). 게이트웨이는 보고마다의 로그 한 줄에만 남긴다(testnet에서 비교).
- 도착했을 때 이미 30초보다 오래된 보고도 판정한다(다른 규칙과 같다).

### 감소 전용 상태

- 상태: `reducing = { rule, detail, since }` 또는 없음. `ReducingEntered`로 켜고 `ReducingCleared`로 끈다. 차단 사유 `Reducing`은 이 상태에서 계산한다.
- 들어갈 때, 같은 입력 안에서: `ReducingEntered`, 그리고 `reduce_only`가 아니고 취소 요청이 없거나 취소 시도가 다 된 모든 열린 주문에 `CancelRequested { by: Rule(rule) }`.
- 그 뒤로 일반 주문은 `Hold(Reducing)`으로 거절한다. 감소 전용 주문은 받는다. 걸려 있는 감소 전용 주문은 그대로 둔다. OMS는 포지션을 닫지 않는다.
- 이미 감소 전용 상태이면 다시 들어가지 않는다. 이벤트도 취소도 새로 내지 않고, 처음 건 규칙이 남는다.
- 계좌 전체에 하나다. 풀 수 있는 것은 운영자의 해제뿐이다. 재시작, 새 날, 회복은 풀지 않는다.

### 해제와 기준 재설정

| 명령 | 받는 조건 | 거절 사유 |
|---|---|---|
| `clear-reduce --note` | 감소 전용 상태다. 보고가 신선하다. 그 보고에서 유지증거금 여유 조건이 거짓이다. 그 보고에서 계좌 유지증거금률 조건이 거짓이다. 사유가 비어 있지 않다 | `NotReducing`, `MarginUnavailable`, `MmBufferActive`, `AccountMmActive`, `NoteRequired` |
| `rebase-day --note` | 보고가 신선하다. 사유가 비어 있지 않다 | `MarginUnavailable`, `NoteRequired` |

- 둘 다 운영자 소켓에서만 받는다. 전략 소켓에서는 거절한다(`forbidden`. 코어에 닿지 않는다).
- 거절은 아무것도 기록하지 않는다. 응답 문자열은 사유 코드로 시작한다: `not_reducing`, `margin_unavailable`, `mm_buffer_active`, `account_mm_active`, `note_required`. 조건이 여럿 어긋나면 이 순서로 처음 것 하나를 답한다(기준 재설정은 `margin_unavailable`, `note_required` 순). 공백뿐인 사유는 빈 것으로 본다.
- 받아들인 해제는 한 입력 안에서 `DayBaseSet { set_by: Operator }`, `ReducingCleared { note, old_base, new_base }` 순서로 낸다. 기준 재설정의 사유는 이벤트가 아니라 같은 저널 기록의 입력에 남는다.
- `policy.margin`이 없으면 보고가 신선할 수 없으므로 둘 다 `margin_unavailable`이다(감소 전용 상태가 아니면 해제는 `not_reducing`).
- 해제는 현재 계좌 가치를 새 기준 금액으로 삼는다. 날짜는 그 보고의 날짜로 둔다(앞선 날짜라도 그대로 쓴다). 그 뒤 새 기준에서 5 %를 넘게 잃으면 다시 걸린다(기준 94에서 89.30은 걸리지 않고 89.29는 걸린다).
- 기준 재설정은 감소 전용 상태를 건드리지 않는다. 날짜는 그 보고의 날짜로 둔다(앞선 날짜라도 그대로 쓴다).
- 계좌 유지증거금률 조건이 참으로 읽히는 동안에도 해제할 수 없다(`account_mm_active`, U17d의 판단: 스펙은 말하지 않는다. 풀어도 다음 보고가 다시 건다). 한도가 없거나 마지막 보고에 `account_mm`이 없으면 이 조건은 거짓이다.
- `--force`는 없다. 유지증거금 여유 조건이 참으로 읽히는 동안에는 해제할 수 없다(`mm_buffer_active`). 조건은 마지막으로 받은 보고에서, 규칙과 같은 계산으로 읽는다. 어느 규칙으로 감소 전용 상태에 들어왔는지는 보지 않는다. 기준 재설정은 이 조건을 보지 않는다.

### 보고 끊김 취소

- 조건: `policy.margin`이 있고, 링크가 올라와 있고, `at − max(마지막으로 받은 보고의 queried_at, 현재 링크 세션이 올라온 시각) > stale_cancel_us`.
- 동작: `Tick`이 `reduce_only`가 아니고 취소 요청이 없는 모든 열린 주문에 `CancelRequested { by: Rule(MarginStale) }`를 낸다. 주문마다 한 번이다. 취소 시도가 다 된 주문에 다시 내지 않는다.
- 래치도 차단도 없다. 새 보고를 받으면 끝나고, 일반 주문을 다시 받는다.
- 링크가 내려가 있으면 아무것도 취소하지 않는다.
- "현재 링크 세션이 올라온 시각"은 상태 `margin.link_up_at`이고, `policy.margin`이 있을 때 링크가 올라오는 이벤트(`LinkChanged { up: true }`)가 그 기록 시각으로 정한다. 링크가 이미 올라와 있는 동안 `policy.margin`이 없던 설정에서 있는 설정으로 바뀌면 그 설정 이벤트의 기록 시각으로 정한다(정책이 켜지기 전부터 걸려 있던 주문이 곧바로 취소되지도, 영영 남지도 않게).

### 차단 표

| 모든 신규 주문을 막는다 | 일반 주문만 막는다 |
|---|---|
| `NoConfig`, `LinkDown`, `OperatorAccount`, `OperatorStrategy`, `StorageLow` | `Resync`, `InDoubt`, `Mismatch`, `Reducing`, `VenueSetting(종목)`, `AccountMode` |

이 표는 `policy.margin`이 있을 때만 적용한다. 정책이 없으면 오른쪽 열의 차단도 감소 전용 주문을 포함한 모든 신규 주문을 막는다(이 계약 이전의 동작). 그래서 정책 없이 기록된 저널은 `verify --re-decide`에서 기록된 결과와 같다.

감소 전용 주문은 오른쪽 열의 차단, 증거금 보고 신선도, 레버리지 검사만 통과한다. 다른 검사([ADR-008](../adr/ADR-008-risk-reservation.md) §7: 전략·종목 허용, 수량, 가격, 주문 금액, 가격대, 계좌와 전략의 포지션 구간, 주문 수, 진행 중 전송 수, 만료)는 모두 받는다.

### 거래소 설정과 계좌 모드

- 출처: 증거금 보고의 포지션(`leverage_type`, `leverage_value`), 그리고 `VenueSetting` 입력(링크가 올라올 때와 포지션이 없는 종목에 60초마다).
- `VenueSetting(종목)` 차단: 그 종목의 설정을 이번 링크 세션에서 받지 못했거나, `isolated`나 `venue_leverage`가 설정과 다르다. 그 종목의 일반 주문만 막는다.
- `AccountMode` 차단: 이번 링크 세션에서 계좌 모드를 받지 못했거나, `unifiedAccount`나 `portfolioMargin`이거나, 허용 목록에 없다. 계좌의 일반 주문을 막는다.
- OMS는 거래소 설정을 바꾸지 않는다.
- 받은 보고(`MarginReported`)의 포지션은 따로 이벤트를 내지 않고 그 종목의 설정이 된다. 보고가 기록이다. 설정에 있는 종목의 포지션만 그렇다: 설정에 없는 종목의 포지션은 보고에는 남고(유지증거금 여유도 판정한다) 거래소 설정으로는 남기지 않는다. `VenueSetting` 입력도 설정에 있는 종목의 것만 받는다.
- 차단 조회(`holds`)는 걸린 종목마다 `VenueSetting(종목)`을 하나씩 보여 준다. 신규 주문 검사는 그 주문의 종목 것만 센다.
- 기본 허용 목록은 비어 있다. 그러면 어떤 모드도 통과하지 못한다. 거래소가 표준 모드를 어떤 문자열로 부르는지는 확인하지 않았으므로(T10) 운영자가 그 문자열을 적어야 일반 주문이 나간다.
- 노드는 링크 세션이 시작하면 코어가 모르는 설정과 계좌 모드를 바로 묻는다. 그 뒤로 종목 하나의 설정은 `runtime.venue_setting_poll_ms`(기본 60초)에 많아야 한 번 묻는다: 코어가 아직 모르는 동안, 그리고 그 종목에 포지션이 없는 동안. 답이 오지 않아도 더 자주 묻지 않는다(조회 weight 예산이 세는 그대로다). 그래서 응답 하나를 잃으면 그 종목은 최대 한 간격 동안 막혀 있다. 계좌 모드는 링크 세션마다 한 번 읽고, 답을 받지 못한 동안에만 같은 간격으로 다시 묻는다. 거래소에서 모드를 바꾸면 다음 링크 세션에서 보인다.
- 게이트웨이의 조회 큐에 자리가 없어 나가지 못한 증거금 조회, 거래소 설정 조회, 계좌 모드 조회는 간격을 쓰지 않는다. 다음 바퀴(`runtime.reconcile_ms`)에 다시 나가고, 나가지 못한 증거금 조회는 순번도 쓰지 않는다.
- `status`의 `core.margin.venue_settings`는 설정에 있는 종목마다 `{isolated, leverage, as_configured}`(모르면 null), `core.margin.account_mode`는 `{mode, allowed}`(모르면 null)이다.

### Hyperliquid 조회

Hyperliquid 게이트웨이는 세 조회를 `/info`로 보내고 응답을 코어 입력으로 옮긴다. 읽기만 한다. 응답이 없으면(시간 초과, HTTP 오류, 요청 예산 부족) 입력이 없다.

| 조회 | 요청 본문 | weight | 응답 → 입력 |
|---|---|---|---|
| 증거금 보고 | `{"type":"clearinghouseState","user":계좌}` | 2 | `MarginReport` 또는 `MarginReportInvalid { reason }` |
| 거래소 설정 | `{"type":"activeAssetData","user":계좌,"coin":종목}` | 20 | `VenueSetting` 또는 없음 |
| 계좌 모드 | `{"type":"userAbstraction","user":계좌}` | 20 | `AccountMode` 또는 없음 |

증거금 보고의 번역:

| 입력의 값 | 응답의 필드 | 형식 |
|---|---|---|
| `account_value` | `marginSummary.accountValue` (교차와 격리를 합친 값. `crossMarginSummary`가 아니다) | 소수 문자열 |
| `instrument` | `assetPositions[].position.coin` | 문자열. 식별자 규칙에 맞지 않는 글자는 `_`로 바꾼다 |
| `szi` | `position.szi` | 부호 있는 소수 문자열 |
| `position_value` | `position.positionValue` | 소수 문자열 |
| `margin_used` | `position.marginUsed` | 소수 문자열 |
| `leverage_type` | `position.leverage.type` | `"isolated"` 또는 `"cross"` |
| `leverage_value` | `position.leverage.value` | JSON 정수 |
| `max_leverage` | `position.maxLeverage` | JSON 정수, 0이 아니다 |

- 위 값 가운데 하나라도 없거나, 형식이 다르거나(문자열이어야 할 소수가 JSON 숫자인 것, 지수 표기, 소수 9자리 이상을 포함한다), `maxLeverage`가 0이거나, `assetPositions`가 목록이 아니거나, 포지션에 종목 이름이 없으면 응답 전체가 `MarginReportInvalid`가 되고 사유에 그 필드 이름이 들어간다. 어떤 값도 0이나 기본값으로 채우지 않고, 어떤 포지션도 건너뛰지 않는다.
- 번역은 설정된 종목 목록을 받지 않는다. 설정에 없는 종목의 포지션도 보고에 들어간다.
- 거래소 설정: 응답의 `coin`이 물은 종목과 같고 `leverage.type`이 `"isolated"`나 `"cross"`이고 `leverage.value`가 JSON 정수일 때만 `VenueSetting`이 된다. 아니면 입력이 없고 설정은 "모름"으로 남는다.
- 계좌 모드: 응답은 JSON 문자열 하나다. 글자 그대로 `AccountMode`가 된다. 게이트웨이는 문서에 없는 문자열을 문서에 있는 값(`unifiedAccount`, `portfolioMargin`, `disabled`, `default`, `dexAbstraction`)으로 바꾸지 않는다(공백을 떼지 않고 대소문자를 고치지 않는다). 그래서 모르는 값은 허용 목록에 같은 글자로 적혀 있지 않은 한 계좌를 막는다. 문자열이 아니거나 64바이트를 넘으면 입력이 없고 모드는 "모름"으로 남는다.

조회 weight 예산(분당, 설정에서 계산):

| 항 | 계산 | 기본 설정의 값 |
|---|---|---|
| 증거금 보고 | 2 × (60 000 / `runtime.margin_poll_ms`) | 24 |
| 참조 가격 | 2 × (60 000 / `venue.ref_price_interval_ms`) | 24 |
| 훑기(체결 20 + 열린 주문 20 + 포지션 2) | 42 × (60 000 / `runtime.sweep_ms`) | 168 |
| 거래소 설정 | 20 × 종목 수 × (60 000 / `runtime.venue_setting_poll_ms`) | 종목마다 20 |

- 합은 216 + 종목 수 × 20이다. 예제 설정(종목 1개)은 236. 나누어떨어지지 않으면 항마다 올림한다.
- Hyperliquid 설정에 `policy.margin`이 있고 합이 600을 넘으면 시작을 거부한다. 600은 받는다. 기본 간격에서 종목 19개(596)까지 받고 20개(616)는 거부한다.
- `policy.margin`이 없으면 증거금 보고와 거래소 설정 항이 없고(192) 검사를 하지 않는다. 이 계약 이전에 읽히던 설정은 그대로 읽힌다. 가상 거래소 설정에는 예산이 없다.
- 거래소 설정 항은 모든 종목에 포지션이 없는 최악의 경우다. 참조 가격은 이 게이트웨이가 스트림으로 받으므로 실제로는 쓰지 않는 여유다.
- 계산에 없는 것: 계좌 모드 조회(링크 세션마다 20 한 번. 답을 받지 못한 동안에는 `venue_setting_poll_ms`마다 20), 링크 세션이 시작할 때 포지션이 있는 종목의 설정을 한 번 묻는 것(종목마다 20), 주문 상태 조회, 주문과 취소. 답이 없는 거래소 설정 조회는 계산 안에 있다: 종목마다 간격에 한 번이다(처음 구현은 1초마다 되물었고, 코드 검토 뒤에 고쳤다).

게이트웨이의 요청 예산과 취소 폭주: 게이트웨이는 자기 쪽에서 요청을 센다. 예산은 최대 1200, 시작할 때 600, 초당 20씩 찬다. 조회는 300(주문 몫 200 + 취소 몫 100)을 남길 수 있을 때만 나간다. 취소는 예산이 얼마든 나간다(위험을 줄이는 요청이다). 감소 전용 상태 진입이 열린 주문 상한 500건의 취소를 한꺼번에 내면:

| 폭주 직전의 예산 | 500건 뒤 | 다음 증거금 조회(weight 2, 302가 필요)가 나갈 때까지 |
|---|---|---|
| 1200(가득) | 700 | 0초 |
| 600(시작 직후) | 100 | 10.1초 |
| 0 | 0 | 15.1초(가장 긴 경우) |

- 가장 긴 15.1초도 신선도 한도 30초보다 짧다. 다만 증거금 조회는 `runtime.margin_poll_ms`마다만 시도되므로 실제 대기는 그 간격만큼 더 길 수 있고(기본 5초), 같은 예산을 훑기와 거래소 설정 조회가 함께 쓴다.
- 이 숫자는 게이트웨이가 스스로 세는 예산이다. 거래소가 500건의 취소를 실제로 어떻게 세는지, 한도를 넘었을 때(HTTP 429) 무엇을 하는지는 확인하지 않았다.

확인하지 않은 것: 실제 응답이 문서의 예와 같은지, `marginUsed`가 격리 자산인지(T1), 계좌 가치가 격리 자산을 포함하는지(T2), 어느 `userAbstraction` 값이 표준 모드인지(T10), 포지션이 없을 때 `activeAssetData`가 레버리지를 주는지(T11).

### Bybit 조회

Bybit 게이트웨이([ADR-028](../adr/ADR-028-bybit-gateway.md) §7b)는 세 조회를 서명한 REST GET으로 보낸다. 읽기만 한다. 응답이 없으면(시간 초과, HTTP 오류, 한도) 입력이 없다. 조회는 U16d부터 노드의 조정기가 보낸다. 요청 한도 예산은 아직 없다(U17).

| 조회 | 요청 | 응답 → 입력 |
|---|---|---|
| 증거금 보고 | `wallet-balance?accountType=UNIFIED`, `account/info`, `position/list?category=linear&settleCoin=USDT`(모든 쪽), 처음 보는 종목은 `instruments-info` | `MarginReport` 또는 `MarginReportInvalid { reason }` |
| 거래소 설정 | `account/info`, `position/list?category=linear&symbol=종목` | `VenueSetting` 또는 없음 |
| 계좌 모드 | `account/info` | `AccountMode { mode: marginMode }` 또는 없음 |

| 입력의 값 | 응답의 필드 |
|---|---|
| `account_value` | `accountType = UNIFIED`인 계좌 하나의 `totalEquity`(USD, 모든 자산) |
| `account_mm.maintenance` | 같은 계좌의 `totalMaintenanceMargin`(USD, "∑ Asset Total Maintenance Margin", F18 §6) |
| `account_mm.margin_balance` | 같은 계좌의 `totalMarginBalance`(USD, "totalWalletBalance + totalPerpUPL", F20 13.2) |
| `instrument` | `symbol` |
| `szi` | `side`(`Buy` +, `Sell` −) × `size`. `positionIdx`가 0이 아니면 못 읽음 |
| `position_value` | `positionValue` |
| `margin_used` | `positionIM` |
| `leverage_type` | 계좌의 `marginMode`: `REGULAR_MARGIN` = 교차, `ISOLATED_MARGIN` = 격리, 그 밖은 보고 전체가 무효. 격리로 읽혀도 `margin_used`가 개시증거금(`positionIM`)이라 유지증거금 여유 규칙에 맞는 값이 아니다. Bybit 설정은 격리를 받지 않는다(설정 검증) |
| `leverage_value` | `leverage`(문자열). 0보다 큰 정수여야 한다. `2.5`는 무효 |
| `max_leverage` | `instruments-info`의 `leverageFilter.maxLeverage`(`"100.00"` = 100). 종목마다 프로세스에서 한 번 읽는다 |

- Hyperliquid와 같이 하나라도 못 읽으면 응답 전체가 `MarginReportInvalid`이고 아무것도 기본값으로 채우지 않는다. `settleCoin=USDT` 밖의 포지션(USDC 등)은 보고에 없다. 계좌 가치에는 들어 있다.
- 교차 계좌라 유지증거금 여유 규칙은 어떤 포지션에도 걸리지 않는다(격리 포지션에만 있다). 교차 계좌는 "계좌 유지증거금률"이 판정한다(U17d). 게이트웨이는 두 필드를 언제나 채우고, 없거나 문자열 소수가 아니면 응답 전체가 `MarginReportInvalid`다. `accountMMRate`는 판정에 쓰지 않는다.
- 거래소 설정의 격리 여부는 계좌 모드에서 온다(통합 계좌는 종목별 모드가 없다). 설정 검증은 Bybit에서 `isolated = false`와 `account_modes_allowed`에 `REGULAR_MARGIN`만 받는다.

### 변형 입력

| 상황 | 동작 |
|---|---|
| 같은 보고가 두 번 온다 | 두 번째는 `seq`가 크지 않으므로 버린다 |
| 늦게 온 옛 보고(작은 `seq`) | 버린다. 저장하지 않고 판정하지 않는다 |
| 옛 세션의 보고, 링크가 내려간 동안의 보고 | 버린다 |
| 읽지 못한 보고가 계속 온다 | 매번 `MarginReportRejected`. 마지막 정상 보고가 나이를 먹어 30초 뒤 거절, 60초 뒤 취소로 간다 |
| 조회가 응답 없이 끝난다 | 입력이 없다. 위와 같이 나이를 먹는다 |
| 감소 전용 상태에서 조건이 다시 참 | 아무 일도 없다 |
| 규칙이 낸 취소가 막힌다(`CancelStuck`) | `Mismatch`가 생긴다. 감소 전용 주문은 계속 받는다 |
| 전송 대기 중인 주문이 있을 때 감소 전용 상태로 들어간다 | 취소 요청을 기록하고, 거래소 접수 뒤에 취소를 보낸다. 그 사이에 체결될 수 있다 |
| 해제 명령이 두 번 온다 | 두 번째는 `NotReducing` |

### 중단과 재실행

| 중단 지점 | 재시작 뒤 |
|---|---|
| 감소 전용 상태를 결정했지만 기록이 내구화되기 전 | 상태도 취소 요청도 없다. 기준 금액은 그대로다. 다음 보고가 같은 판정을 다시 한다 |
| 기록이 내구화된 뒤, 취소가 거래소로 나가기 전 | 감소 전용 상태와 취소 요청이 복원된다. 재시작이 보내지 못한 취소를 다시 보낸다([C-BOOT](recovery.md)) |
| `DayBaseSet`이 내구화되기 전 | 기준은 이전 값이다. 다음 보고가 다시 정한다 |
| 재생 | 이벤트만 적용한다. 판정을 다시 하지 않고 외부 효과가 없다. 상태 다이제스트가 같다 |

누가 언제 보는가: 이벤트가 내구화된 뒤 `status`에 나타난다. `status`는 감소 전용 상태(규칙, 시각), 기준 금액, 마지막 보고의 계좌 가치와 나이, 마지막 보고의 계좌 유지증거금과 증거금 잔고(`core.margin.account_mm`, 보고에 없으면 키가 없다), 계좌 노출, 종목별 거래소 설정, 계좌 모드, 마지막 보고 거부 사유(그 뒤에 받은 보고가 없는 동안)를 보여 준다. `policy.margin`이 없으면 `core.margin`은 null이다(남아 있는 감소 전용 상태도 보이지 않는다).

자원 상한: 보고는 5초에 하나이므로 저널에 하루 최대 17 280건을 더한다(링크 세션이 새로 시작하면 그 세션의 첫 조회가 바로 나가므로 세션마다 한 건이 더 있을 수 있다). 시뮬레이션이 건수로 확인한다(`sim::margin_journal_records_stay_within_one_per_poll`). 디스크 바이트는 재지 않았고 저널은 지워지지 않는다(U-04). 노출 계산은 열린 주문 수(상한 500)에 비례한다. 감소 전용 상태 진입이 내는 취소는 열린 주문 상한 이하이고 명령 채널 용량 안에 있다.

### 전략이 지켜야 하는 것

- 포지션을 줄이는 주문에는 반드시 reduce-only 표시를 한다. 표시가 없으면 감소 전용 상태와 보고 끊김 취소에서 취소되고, 새로 내면 거절된다.
- 표시한 익절 주문은 진입이 체결된 뒤에 낸다. 포지션이 없으면 거래소가 그 주문을 취소한다(`reduceOnlyCanceled`).
- OMS에는 조건부 주문이 없다. 손절은 조건이 됐을 때 전략이 IOC로 낸다. 감소 전용 상태에서는 그 IOC에도 표시가 있어야 한다.
- 언제 취소하고 다시 낼지는 전략이 정한다. OMS는 주문을 고치지 않고 수명 제한을 두지 않는다.

## 검증

아래 표의 시험은 모두 그 이름으로 코드에 있다(2026-11-16에 이름마다 확인했다). 통과 여부는 [generated/claims.md](../generated/claims.md)가 결과 파일에서 계산한다. 변이(M..)는 `xtask mutants`의 목록이고 [검증 문서 §4](../verification/README.md)에 내용이 있다. 시나리오 S1~S25와의 대응은 `.scratch/margin-rules/spec-ko.md` §9에 있다.

| 절 | 주장 | 시험 |
|---|---|---|
| 차단 표 | V-MARGIN-1 | `scenarios::reduce_only_order_is_accepted_during_resync`, `reduce_only_order_is_accepted_while_another_order_is_in_doubt`, `reduce_only_order_is_accepted_under_an_unacknowledged_mismatch`, `reduce_only_order_is_rejected_by_every_block_all_hold`, `reduce_only_order_is_still_bound_by_the_strategy_position_limit`, `without_a_margin_policy_every_hold_still_refuses_a_reduce_only_order`. 변이 M21~M23 |
| 설정(구역의 유무와 기본값) | V-MARGIN-1 | `margin::absent_margin_policy_adds_no_bytes_to_a_journal_record`, `margin::empty_margin_section_takes_the_contract_defaults` |
| 입력, 상태, 이벤트(보고 수신) | V-MARGIN-2 | `margin::margin_report_of_the_current_session_is_recorded`, `margin_report_of_an_old_session_or_with_the_link_down_is_ignored`, `late_margin_report_with_a_lower_sequence_is_ignored`, `invalid_margin_report_is_journaled_and_never_becomes_fresh`. 변이 M24~M27, M110 |
| 신선도(보고 조건) | V-MARGIN-2 | `margin::plain_order_is_rejected_margin_unavailable_without_a_fresh_report`, `margin_report_dated_after_the_record_time_is_not_fresh`, `reduce_only_order_needs_no_fresh_report`. 변이 M28~M31, M105 |
| 신선도(참조 가격 조건) | V-MARGIN-3 | `margin::plain_order_is_rejected_when_an_exposed_instrument_has_no_fresh_reference_price`. 변이 M102 |
| 계좌 레버리지 검사(신선한 보고가 없으면 판정하지 않는다) | V-MARGIN-3 | `margin::without_a_fresh_report_an_order_past_the_limit_is_rejected_margin_unavailable_only`. 변이 M115 |
| 계좌 레버리지 검사(한도) | V-MARGIN-3 | `margin::order_past_twice_the_account_value_is_rejected_account_leverage`, `order_exactly_at_the_limit_is_accepted`, `account_value_zero_or_negative_rejects`. 변이 M32, M101 |
| 계좌 레버리지 검사(노출 계산) | V-MARGIN-3 | `margin::resting_order_counts_at_the_higher_of_limit_and_reference`, `reduce_only_and_plain_buy_on_a_short_count_at_full_reach`, `exposure_is_the_sum_over_instruments`. 변이 M34~M36, M103 |
| 계좌 레버리지 검사(미체결 주문) | V-MARGIN-3 | `margin::an_unfilled_resting_order_keeps_its_exposure_and_is_never_repriced_or_expired_by_the_oms`, `partial_fill_then_cancel_frees_exposure_only_on_terminal_evidence`, `a_venue_rejection_of_an_accepted_order_frees_its_exposure`. 변이 M104 |
| 계좌 레버리지 검사(래치 없음, 감소 전용) | V-MARGIN-3 | `margin::after_a_passive_breach_a_plain_reducing_order_passes_and_an_increasing_one_does_not`, `reduce_only_order_skips_the_leverage_gate`. 변이 M33, M106 |
| 계좌 레버리지 검사(성질) | V-MARGIN-3 | `margin::adding_an_order_never_lowers_exposure`, `mirrored_books_have_equal_exposure_and_equal_decisions`, `exposure_is_exact_at_extreme_price_and_quantity` (proptest). 변이 M114 |
| 설정(정책이 없을 때) | V-MARGIN-2 | `margin::without_a_margin_policy_no_margin_check_runs`. 변이 M111 |
| 중단과 재실행(재생) | V-MARGIN-2, V-REPLAY-1 | `margin::margin_events_replay_to_the_same_state` |
| 설정(노드의 설정 검사) | V-CFG-1 | `config::tests::mainnet_is_refused_without_a_margin_policy_and_bad_margin_values_are_refused` (oms-node 단위) |
| 조회와 `status` | V-MARGIN-11 | `e2e::node_polls_margin_and_status_shows_value_and_age`, `e2e::status_shows_account_exposure`, `e2e::status_shows_reducing_its_rule_and_the_day_base`, `e2e::node_reads_venue_setting_and_account_mode_and_status_shows_them` |
| 일 손실과 기준 금액 | V-MARGIN-5 | `margin::first_report_sets_the_day_base`, `loss_greater_than_five_percent_of_the_base_enters_reducing`, `first_report_after_midnight_is_judged_against_the_old_base_then_rolls_the_day`, `a_report_of_an_earlier_day_does_not_change_the_day_base`, `a_loss_between_two_reports_enters_reducing_on_the_next_report`, `a_report_older_than_the_freshness_limit_on_arrival_is_still_judged`. 변이 M37~M39, M108 |
| 감소 전용 상태(진입, 규칙 취소, 주문 받기) | V-MARGIN-4 | `margin::entering_reducing_cancels_plain_orders_and_keeps_reduce_only_ones`, `under_reducing_a_plain_order_is_rejected_and_a_reduce_only_one_is_accepted`, `an_order_pending_send_at_entry_is_cancelled_once_acknowledged`, `a_rule_cancel_that_races_a_fill_books_the_fill`, `a_stuck_rule_cancel_does_not_block_reduce_only_orders`, `reducing_is_entered_once`, `a_new_day_does_not_clear_reducing`, `an_external_fill_raises_a_mismatch_and_a_loss_report_enters_reducing`. 변이 M40~M42, M107. 기존 시험 `scenarios::cancel_race_final_status_decides`는 그대로 통과한다 |
| 보고 끊김 취소 | V-MARGIN-7 | `margin::report_stale_past_the_cancel_limit_cancels_each_plain_order_once`, `stale_age_starts_at_link_up_when_no_report_has_arrived`, `stale_cancel_is_silent_while_the_link_is_down_or_the_policy_is_off`, `stale_cancel_does_not_reissue_an_exhausted_cancel`, `a_fresh_report_after_a_stale_episode_restores_acceptance_without_a_latch`, `a_forward_clock_jump_reads_the_report_as_stale_and_cancels`, `stale_age_starts_at_policy_activation_when_the_link_is_already_up`. 변이 M60~M66 |
| 중단과 재실행(재생: 감소 전용 상태와 기준 금액) | V-MARGIN-4, V-MARGIN-5, V-REPLAY-1 | `margin::latch_and_day_base_replay_to_the_same_state` |
| 거래소 설정과 계좌 모드(종목 설정) | V-MARGIN-9 | `margin::instrument_with_unknown_venue_setting_holds_plain_orders`, `a_setting_that_differs_from_config_holds_only_that_instrument`, `a_matching_setting_releases_the_hold_and_link_loss_makes_it_unknown_again`, `a_cross_position_in_a_margin_report_raises_the_hold`, `venue_setting_hold_names_its_instrument_in_holds`. 변이 M43~M45, M112 |
| 거래소 설정과 계좌 모드(계좌 모드) | V-MARGIN-9 | `margin::unified_or_portfolio_account_mode_holds_the_account`, `an_account_mode_off_the_allow_list_or_unknown_holds_the_account`. 변이 M46~M49, M113 |
| Hyperliquid 조회(증거금 보고의 번역) | V-MARGIN-10 | `wire::tests::documented_clearinghouse_state_parses_to_a_margin_report`, `missing_zero_or_unparseable_max_leverage_rejects_the_whole_report`, `any_unparseable_number_rejects_the_whole_report`, `a_position_on_an_unconfigured_coin_is_kept` (oms-node 단위). 변이 M67~M73 |
| Hyperliquid 조회(거래소 설정, 계좌 모드, 요청 본문) | V-MARGIN-10 | `wire::tests::documented_active_asset_data_parses_leverage_and_mode`, `user_abstraction_values_parse_and_an_unknown_value_stays_unknown`, `margin_and_setting_request_json` (oms-node 단위). 변이 M74~M76 |
| Hyperliquid 조회(조회 weight 예산, 예제 설정) | V-CFG-1 | `config::tests::steady_state_query_weight_is_computed_from_config_and_a_budget_above_600_is_refused`, `example_config_with_a_margin_policy_loads` (oms-node 단위). 변이 M77, M78 |
| Hyperliquid 조회(취소 폭주 뒤의 요청 예산) | V-MARGIN-10 | `venue::hyperliquid::tests::a_full_cancel_burst_leaves_the_margin_query_waiting_for_a_known_time` (oms-node 단위). 변이 M79 |
| Bybit 조회(증거금 보고, 거래소 설정, 계좌 모드의 번역) | V-BYBIT-READ-1 | `venue::bybit::query::tests::the_margin_report_maps_the_unified_account`, `a_margin_answer_that_cannot_be_read_as_a_whole_is_refused_as_a_whole`, `the_venue_setting_and_the_account_mode_are_read_as_they_are`, `config::tests::a_bybit_configuration_loads_and_each_unsafe_value_is_refused` (oms-node 단위) |
| 차단 표(거래소 설정, 계좌 모드) | V-MARGIN-9 | `margin::reduce_only_order_passes_the_venue_setting_and_account_mode_holds`. 변이 M50 |
| 설정(`runtime.venue_setting_poll_ms`) | V-CFG-1 | `config::tests::mainnet_is_refused_without_a_margin_policy_and_bad_margin_values_are_refused` (oms-node 단위) |
| 해제와 기준 재설정 | V-MARGIN-6 | `margin::clear_reduce_is_refused_when_stale_or_without_a_note_or_when_not_reducing`, `clear_reduce_rebases_the_day_and_a_further_loss_latches_again`, `rebase_day_sets_the_base_and_leaves_the_latch_alone`, `a_withdrawal_without_rebase_reads_as_a_loss`. 변이 M51~M57 |
| 유지증거금 여유 | V-MARGIN-8 | `margin::isolated_margin_below_mmr_plus_buffer_enters_reducing`, `mmr_follows_the_max_leverage_of_each_position`, `a_cross_position_is_not_judged_by_the_mm_rule`, `an_old_report_on_arrival_is_still_judged_by_the_mm_rule`, `mm_comparison_is_exact_at_extreme_values` (proptest). 변이 M80~M86, M109 |
| 유지증거금 여유(계좌 전체, 먼저 건 규칙) | V-MARGIN-8, V-MARGIN-4 | `margin::a_trigger_on_one_instrument_blocks_plain_orders_on_all`, `reducing_records_the_rule_that_tripped_first`. 변이 M88 |
| 해제와 기준 재설정(유지증거금 여유 조건) | V-MARGIN-6 | `margin::clear_reduce_is_refused_while_the_mm_condition_holds`. 변이 M87 |
| 해제와 기준 재설정(운영 소켓, 운영자 도구) | V-MARGIN-11 | `e2e::strategy_socket_cannot_clear_reducing_or_rebase`, `e2e::omsctl_clear_reduce_and_rebase_day_work_end_to_end`. 변이 M58, M59 |
| 감소 전용 상태(프로세스: 재시작, 많은 주문의 취소) | V-MARGIN-11 | `e2e::reducing_and_day_base_survive_a_node_kill`, `e2e::reducing_cancels_every_resting_plain_order_through_the_node` |
| 중단과 재실행(중단 지점) | V-MARGIN-11 | `crash_points::stopped_before_the_write_of_a_reducing_decision_the_next_report_decides_again`, `stopped_after_the_reducing_decision_is_durable_the_cancels_are_sent_after_restart`, `stopped_around_a_day_base_set_the_base_is_set_exactly_once` (`--features failpoints`) |
| 시뮬레이션(가상 거래소의 증거금 모형) | V-MARGIN-12 | `sim::world_account_value_follows_fills_price_fees_and_funding`, `a_loss_made_only_of_fees_and_funding_enters_reducing`, `world_refuses_a_reduce_only_order_that_would_grow_the_position`. 변이 M89~M92 |
| 시뮬레이션(임의 순서의 실행에서 증거금 불변식) | V-MARGIN-12, V-SIM-1 | `sim::profile_margin_calm`, `profile_margin_calm_reducing_invariant`, `profile_margin_calm_release_invariant`, `profile_margin_lossy`. 변이 M94~M96 |
| 시뮬레이션(여러 UTC 날: 자정 세 번, 재시작, 손실, 해제, 기준 재설정. 증거금 기록 수) | V-MARGIN-12, V-SIM-1 | `sim::profile_margin_days_crosses_three_utc_days_with_restarts`, `margin_journal_records_stay_within_one_per_poll`. 변이 M97~M100 |
| 설정(정책이 없을 때: 기존 프로파일의 저널이 main의 것과 같다) | V-MARGIN-12 | `sim::existing_profiles_give_the_same_run_with_margin_off`. 변이 M93 |
| 조회와 `status`(가상 거래소 프로세스의 포지션) | V-MARGIN-11 | `e2e::margin_report_of_the_venue_process_carries_its_positions` |
| 설정(정책이 없을 때의 감소 전용 상태) | V-MARGIN-4 | `margin::without_a_margin_policy_the_reducing_latch_refuses_no_order_and_returns_with_the_policy`. 변이 M116 |
| 입력, 상태, 이벤트(받은 보고가 거부 사유를 지운다. simvenue의 응답에 포지션 목록이 없으면 거부) | V-MARGIN-2 | `margin::a_recorded_report_ends_the_last_rejection`, `venue::simproto::tests::a_margin_answer_without_its_position_list_is_not_read` (oms-node 단위). 변이 M117, M119 |
| 거래소 설정과 계좌 모드(보고의 포지션 가운데 설정에 있는 종목만) | V-MARGIN-9 | `margin::a_report_position_on_an_unconfigured_instrument_leaves_no_venue_setting`. 변이 M118 |
| 거래소 설정과 계좌 모드, 조회(대조기의 조회 간격) | V-MARGIN-11 | `reconciler::a_margin_query_the_gateway_had_no_room_for_goes_out_on_the_next_round`, `reconciler::an_unanswered_setting_or_mode_query_is_repeated_at_the_setting_period_only` (대조기 태스크의 두 포트, 곧 공개된 상태와 조회 채널에서 실제 시계로. 묶음 `node-unit`). 변이 M120~M122 |
| 유지증거금 여유(프로세스: 진입) | V-MARGIN-11 | `e2e::venue_process_states_the_margin_a_control_sets_and_the_mm_rule_latches_on_it` |
| 계좌 유지증거금률(한도, 잔고 0 이하, 유지증거금 0, 정확한 곱) | V-MARGIN-13 | `account_mm::rate_just_below_the_limit_does_not_latch_and_at_the_limit_latches_with_the_cancels`, `the_limit_is_judged_exactly_at_one_basis_point`, `a_margin_balance_of_zero_or_below_with_maintenance_latches`, `no_maintenance_margin_latches_nothing_whatever_the_balance`, `the_comparison_is_exact_at_the_largest_amounts`. U17d 변이 |
| 계좌 유지증거금률(보고의 거부, 먼저 건 규칙, 해제, `status`) | V-MARGIN-13 | `account_mm::a_report_without_account_mm_is_rejected_under_the_limit_and_recorded_without_it`, `the_latch_is_not_replaced_by_a_later_rule`, `clear_reduce_is_refused_while_the_account_rate_is_at_or_above_the_limit`, `status_shows_the_account_numbers_and_the_rule` |
| 계좌 유지증거금률(저널: 옛 바이트, 재생, 복구) | V-MARGIN-13 | `account_mm::journals_with_and_without_account_mm_replay_to_the_live_state`, `record_schema::margin_reports_with_and_without_account_mm_recover_to_the_live_state` |
| 계좌 유지증거금률(설정, Bybit 번역) | V-MARGIN-13, V-BYBIT-READ-1 | `account_mm::the_limit_is_one_to_9999_bps`, `config::tests::the_account_mm_limit_is_required_on_bybit_and_refused_elsewhere`, `venue::bybit::query::tests::the_margin_report_maps_the_unified_account`, `a_margin_answer_that_cannot_be_read_as_a_whole_is_refused_as_a_whole`, `the_account_mm_rate_field_is_not_used_for_the_judgment` (oms-node 단위) |

보고 수신과 신선도의 시험이 보이지 않는 것: 새 보고를 받지 못하는 동안에는 어떤 규칙도 판정하지 않는다(판정은 받은 보고마다 한 번이다). 거래소의 계좌 가치가 격리 포지션의 자산을 포함하는지(T2). 기본 간격 5초 그대로의 실행(프로세스 시험은 100 ms로 줄였다).

레버리지 검사의 시험이 보이지 않는 것: 주문 사이에 가격이 움직인 뒤의 노출은 지켜보지 않는다(검사는 새 주문이 올 때만 한다. 가격이 움직여 한도를 넘어도 아무 일도 없다). 참조 가격은 mid이고 거래소의 mark가 아니다. 검사가 받은 주문을 거래소가 거절할 수 있다(경계에서의 수수료, 거래소 쪽 증거금 계산). 계좌 가치가 격리 포지션의 자산을 포함하는지(T2). 주문 500건이 걸려 있을 때의 검사 비용은 쟀다: 열린 주문 약 500건에서 신규 주문 한 건의 판단이 정책 없이 중앙값 27.4 µs, p99 33.4 µs이고 정책이 있으면 중앙값 65.9 µs, p99 89.8 µs다(V-PERF-5, [성능 문서](../performance/README.md). 이 작업 전의 측정은 22.6 µs, 30.6 µs였다). 검사가 걸려 있는 주문 전체를 두 번 더하므로 열린 주문 수에 비례한다. 정책을 켠 노드의 응답 지연과 거절 경로의 시간은 재지 않았다.

일 손실과 감소 전용 상태의 시험이 보이지 않는 것: 규칙 취소가 체결보다 먼저 닿는다는 것(취소는 체결에 질 수 있고, 그때는 체결을 기장한다). 두 보고 사이에 난 손실과 새 보고를 받지 못하는 동안의 손실은 막지 못한다(100 다음 보고가 88이면 12 %를 잃은 뒤에 걸린다). 실제 청산 체결의 모양(T6. 외부 체결과 손실 보고의 조합으로만 본다). 입금이 가리는 손실. 감소 전용 상태인 채로 `policy.margin`을 없앤 설정이 켜진 동안에는 그 상태가 아무 주문도 막지 않는다(위 "설정". 정책을 빼는 것은 운영자의 결정이다). 그 동안 받은 일반 주문은 정책이 돌아와도 취소되지 않는다.

해제, 기준 재설정, 중단 지점의 시험이 보이지 않는 것: 운영자의 판단이 옳은 것. 입금이 가린 손실(100, 입금 10, 손실 14는 걸리지 않는다. `a_withdrawal_without_rebase_reads_as_a_loss`가 그대로 보여 준다). 디스크 유실과 전원 장애(중단은 프로세스 `abort()`와 SIGKILL이다. 운영체제는 살아 있다). `omsctl` 실행 파일의 `main`(인자 수집, 출력, 종료 코드. 시험은 그 실행 파일이 부르는 `oms_api::ctl::run`을 부른다). 가상 거래소에는 요청 예산이 없으므로 취소 300건 뒤에 Hyperliquid 게이트웨이의 조회가 얼마나 밀리는지(예산의 산술만 "Hyperliquid 조회"의 표에 고정했다). `policy.margin`을 없앤 설정이 켜진 동안에는 해제할 수 없다(`margin_unavailable`). 정책을 다시 켜야 한다.

유지증거금 여유의 시험이 보이지 않는 것: 거래소의 격리 포지션 `marginUsed`가 그 포지션의 자산(미실현 손익 포함)이라는 것(T1). 틀리면 규칙은 양쪽으로 틀릴 수 있고, 잘못 걸린 래치에서 나가는 길은 없다(조건이 참으로 읽히는 동안 `clear-reduce`는 `mm_buffer_active`다). 증거금 구간(T9. 유지증거금률은 보고의 `max_leverage` 하나로만 계산한다). 규칙은 청산 가격의 0.13~0.37 % 앞에서 걸리고(BTC, 거래소 2배) 5초 간격의 조회가 그 사이를 건너뛸 수 있다. 새 보고를 받지 못하는 동안에는 판정하지 않는다. 프로세스 수준의 시험은 진입 하나다(`e2e::venue_process_states_the_margin_a_control_sets_and_the_mm_rule_latches_on_it`: simvenue의 제어 명령 `margin_used`로 증거금을 선 위의 1.5, 선 아래의 1.49로 말하게 하고 `status`의 `mm_buffer`를 본다). `omsctl`의 `mm_buffer_active` 응답은 프로세스에서 시험하지 않았다. 임의 순서의 실행에서는 `sim::profile_margin_lossy`가 본다(가상 거래소가 증거금을 0으로 말하는 동안).

계좌 유지증거금률의 시험이 보이지 않는 것: 실제 Bybit 응답의 두 필드(가짜 HTTP 서버에 문서 모양의 응답만 준다. testnet은 U-01 허가 뒤). `totalMaintenanceMargin`이 거래소의 청산 판정과 같은 숫자라는 것(문서의 정의만 읽었다). 5000 bps가 적당하다는 것(판단이고 모형이 아니다. testnet에서 다시 본다). 5초 간격의 조회가 급한 가격 변화를 건너뛸 수 있다. 새 보고를 받지 못하는 동안에는 판정하지 않는다. 노드 프로세스를 거친 진입(Bybit 노드 시험은 증거금 정책 없이 돈다). `omsctl`의 `account_mm_active` 응답은 프로세스에서 시험하지 않았다. 잔고가 음수이고 유지증거금이 0인 계좌는 걸지 않는다(판단. 일 손실 규칙이 계좌 가치를 본다).

보고 끊김 취소의 시험이 보이지 않는 것: 포지션은 그대로 남는다(OMS는 닫지 않는다). 감소 전용 주문은 남는다. 링크가 내려가 있는 동안에는 아무것도 취소하지 않는다(취소를 보낼 수 없다. 링크가 돌아오면 그 세션이 올라온 시각부터 다시 60초를 잰다). 취소가 체결보다 먼저 닿는다는 것(취소는 체결에 질 수 있고, 그때는 체결을 기장한다). 취소 시도가 다 된 주문은 다시 요청하지 않는다(운영자가 `CancelStuck` 불일치로 본다). 틱이 오는 간격만큼 늦을 수 있다. 노드 프로세스를 거친 동작(이 규칙의 프로세스 시험은 없다. 무작위로 섞인 실행은 `sim::profile_margin_lossy`가 본다).

시뮬레이션의 시험이 보이지 않는 것: 실제 거래소에 관한 것은 아무것도 없다. 계좌 가치와 증거금은 우리 모형이다(T1, T2, T3). 감소 전용 주문의 거절과 잘림은 문서를 읽고 가정으로 적은 것이다(T5). 취소가 체결보다 먼저 닿는다는 것(검사는 취소 요청이 있는지만 본다. 체결이 이기면 체결을 기장한다). 주문을 받은 뒤 가격이 움직여 노출이 한도를 넘는 것(검사는 받는 순간만 본다). 청산은 모형에 없다(계좌 가치가 0 아래로 가도 포지션은 남는다). 종목은 하나다. `margin_calm`과 `margin_lossy`의 증거금 시계는 계약 기본값의 50분의 1이고(조회 100 ms, 신선도 600 ms, 끊김 취소 1.2초) 자정을 한 번 넘는다. `margin_days`는 계약 기본값의 시계로 자정을 세 번 넘지만 가상 시간이다: 노드 프로세스를 실제로 며칠 돌리지 않았다. 저널이 느는 것은 증거금 기록의 건수로만 확인했다(디스크 바이트는 재지 않았고, 저널은 지워지지 않는다, U-04). 증거금 기록이 아닌 기록(참조 가격, 틱의 정리 기록 등)의 수는 묶지 않았다. `margin_days`는 seed를 표준 실행의 10분의 1만 돌린다.

차단 표의 시험이 보이지 않는 것: 귀속되지 않은 포지션을 OMS로 줄이는 길(없다), 거래소가 감소 전용 주문의 포지션 확대를 막는지(T5).

거래소 설정과 계좌 모드의 시험이 보이지 않는 것: 거래소가 표준 모드를 어떤 문자열로 부르는지(T10. 시험의 `standard`는 임의의 이름이다). 거래소가 걸려 있는 주문에 그 레버리지를 실제로 적용하는지(T8). 포지션이 없는 종목의 설정을 거래소가 알려 주는지(T11). 포지션이 있는 종목의 설정은 보고의 포지션으로 갱신된다(`e2e::margin_report_of_the_venue_process_carries_its_positions`). 그 사이, 곧 다음 보고까지의 시간에 바뀐 것은 모른다. 계좌 모드가 링크 세션 도중에 바뀐 것(다음 링크 세션에서 읽는다). 기본 간격 60초 그대로의 실행(프로세스 시험은 200 ms로 줄였다).

Hyperliquid 조회의 시험이 보이지 않는 것: 시험은 문서의 응답 예를 번역 함수에 넣는다. 거래소에 묻지 않으므로 실제 응답이 그 예와 같은지는 모른다. `activeAssetData`의 예는 저장소의 조사 문서에 없어 구현자가 아는 문서의 예를 적었고(계좌는 자리 표시로 바꿨다) 문서와 다시 대조하지 않았다. 게이트웨이가 조회를 보내고 응답을 번역 함수에 넘기는 부분(세 `match` 갈래)과 HTTP 오류·429의 처리는 시험이 없다. 요청 예산 시험은 예산의 산술만 본다: 거래소가 실제로 세는 weight, 여러 조회가 같은 예산을 다투는 순서, 조회 간격 때문에 더 길어지는 대기는 보지 않는다.

해결한 사항(티켓 09가 찾았고 코드 검토 뒤 2026-11-16에 고쳤다): 대조기는 이번 링크 세션에서 모르는 거래소 설정과 계좌 모드를 1초마다 되물었다(종목 하나에 분당 최대 1200의 weight, 설정 검사가 세는 600의 밖). 지금은 종목마다, 그리고 계좌 모드에 대해 `runtime.venue_setting_poll_ms`에 많아야 한 번 묻는다("거래소 설정과 계좌 모드"). 시험은 `reconciler::an_unanswered_setting_or_mode_query_is_repeated_at_the_setting_period_only`다. 남는 것: 응답 하나를 잃으면 그 종목(또는 계좌)의 일반 주문이 최대 한 간격(기본 60초) 동안 막힌다. T11이 틀리면(포지션 없는 종목의 설정을 거래소가 말해 주지 않으면) 그 종목은 계속 막혀 있고, 조회는 예산 안에 머문다.

### 실제 거래소에서 확인하지 않은 것 (T1~T11)

이 계약의 어떤 시험도 실제 거래소에 닿지 않는다. 증거금 조회, 거래소 설정 조회, 계좌 모드 조회는 testnet에도 보낸 적이 없다. 읽기 전용 조회 묶음(`hl-live`)은 이 작업에서 실행하지 않았고 주장 V-HL-3은 "미실행"이다. 아래는 문서에서 읽은 가정이고 하나도 확인하지 않았다.

| # | 가정 | 여기에 기대는 것 |
|---|---|---|
| T1 | 격리 포지션의 `marginUsed`는 미실현 손익을 포함한 그 포지션의 자산이다 | 유지증거금 여유. 틀리면 규칙이 양쪽으로 틀릴 수 있다 |
| T2 | 계좌 가치(`accountValue`)는 격리 포지션의 자산을 포함한다 | 계좌 레버리지 검사, 일 손실 |
| T3 | 격리 포지션의 수수료와 펀딩이 격리 증거금에서 빠지는지 교차 잔고에서 빠지는지 | 거래소 레버리지 2배(O1) |
| T4 | 포지션이 있을 때 레버리지를 낮추거나 증거금 방식을 바꿀 수 있는지 | 운영 절차 |
| T5 | 포지션보다 큰 감소 전용 주문을 줄여 받는지 거절하는지, 여러 건일 때는 어떤지 | 어느 주문이 남는지 |
| T6 | 청산 체결이 체결 목록에 어떤 모양으로 오는지 | 외부 체결과 일 손실의 조합(S19) |
| T7 | 감소 전용 청산 주문이 최소 주문 금액의 예외인지 | 아주 작은 포지션 닫기 |
| T8 | 걸려 있는 격리 주문이 증거금을 잡아 두는지 | 한도 근처에서의 거래소 거부 |
| T9 | 포지션의 최대 레버리지가 증거금 구간을 따라 바뀌는지 | 1구간 밖의 유지증거금률 |
| T10 | 계좌 모드 값 중 어느 것이 표준 모드인지 | 허용 목록에 적을 문자열 |
| T11 | 포지션이 없을 때 거래소 설정 조회가 레버리지를 알려 주는지 | 포지션이 없는 종목의 설정 확인. 틀리면 그 종목의 일반 주문은 포지션이 생길 때까지 막혀 있다 |

testnet 실주문 예행(감소 전용 상태 훈련, 보고 끊김 취소 훈련, 열린 주문이 있는 채로 강제 종료와 재시작)과 T1~T11의 확인은 이 작업 밖이다. mainnet 전에 필요하고 사용자의 승인이 필요하다(U-01, U-09).
스펙과 다르게 했거나 스펙이 비워 둔 곳에서 정한 것, 보장하지 않는 것의 전체 목록은 [검증 문서 §11](../verification/README.md)에 있다.
