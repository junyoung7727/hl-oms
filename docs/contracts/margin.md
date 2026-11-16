# 증거금 규칙 계약

상태: 채택(미구현). 이 계약은 만들 동작을 적은 것이다. 지금 코드에는 이 동작이 없다. 각 절은 그 절을 맡은 티켓이 머지될 때 구현되고, 그때 이벤트와 주장(V-MARGIN-n)이 `registry/`에 들어온다.

설계 근거: [ADR-018](../adr/ADR-018-margin-rules.md). 용어: [GLOSSARY.md](../../GLOSSARY.md). 스펙과 티켓: `.scratch/margin-rules/`.
실제 거래소에서는 아무것도 확인하지 않는다(U-01). 가정 T1~T11은 ADR-018 §8에 있다.

## C-MARGIN 증거금 규칙

### 설정

규칙은 `policy.margin`이 있을 때만 동작한다. 없으면 이 계약의 입력은 상태를 바꾸지 않고, 어떤 주문도 이 계약의 사유로 거절되지 않는다. 아래 "차단 표"도 같다: 정책이 없으면 모든 차단이 지금처럼 모든 신규 주문을 막는다.
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
| `runtime.margin_poll_ms` | 5 000 | 증거금 보고 조회 간격 | 0보다 크다 |
| `runtime.venue_setting_poll_ms` | 60 000 | 포지션이 없는 종목의 거래소 설정 조회 간격 | 0보다 크다 |

조회 weight 예산: 설정에서 계산한 분당 조회 weight(증거금 보고, 참조 가격, 15초 훑기, 종목별 거래소 설정)가 거래소 한도 1200의 절반인 600을 넘으면 설정 검사가 시작을 거부한다.

### 입력, 상태, 이벤트

| 입력 | 조건 | 이벤트 | 바뀌는 상태 |
|---|---|---|---|
| `MarginReport { session, seq, queried_at, account_value, positions[] }` | 링크가 올라와 있고, `session`이 현재 세션이고, `seq`가 이 세션에서 받은 마지막 보고의 `seq`보다 크다 | `MarginReported` | 마지막 보고(값, 조회 시각, 순번) |
| 같은 입력 | 위 조건이 아니다 | 없음(입력을 버린다) | 없음 |
| `MarginReportInvalid { session, seq, queried_at, reason }` | 현재 세션 | `MarginReportRejected { reason }` | 마지막 거부 사유. 마지막 보고는 그대로다 |
| `MarginReport`인데 어느 포지션의 `max_leverage`가 0 | 현재 세션 | `MarginReportRejected` | 위와 같다. 코어가 직접 거부한다 |
| `VenueSetting { session, instrument, isolated, leverage }` | 현재 세션 | `VenueSettingNoted` | 그 종목의 거래소 설정 |
| `AccountMode { session, mode }` | 현재 세션 | `AccountModeNoted` | 계좌 모드 |
| 운영자 `ClearReduce { note }` | 아래 "해제" | `DayBaseSet { set_by: Operator }`, `ReducingCleared { note, old_base, new_base }` | 기준 금액, 감소 전용 상태 해제 |
| 운영자 `RebaseDay { note }` | 아래 "기준 재설정" | `DayBaseSet { set_by: Operator }` | 기준 금액 |
| `Tick` | 아래 "보고 끊김 취소" | `CancelRequested { by: Rule(MarginStale) }` | 주문의 취소 요청 |

`positions[]`의 한 항목: `instrument, szi, position_value, margin_used, leverage_type, leverage_value, max_leverage`. 설정에 없는 종목의 포지션도 버리지 않는다.
받은 보고 하나는 한 입력 안에서 아래 순서로 처리된다: 기록 → 거래소 설정 갱신 → 유지증거금 여유 판정 → 일 손실 판정 → 날짜 넘김.
규칙 판정은 받은 보고마다 한다. 도착했을 때 이미 30초보다 오래된 보고도 판정한다(값은 거래소가 계산한 것이고 가진 것 중 가장 새것이다).

상태 소유자는 코어다. 모든 변경은 위 이벤트로만 일어나고 저널에서 복원된다. 링크가 바뀌면 거래소 설정과 계좌 모드는 "모름"이 된다. 마지막 보고, 기준 금액, 감소 전용 상태는 링크가 바뀌어도 남는다.

### 신선도

- 보고의 나이 = `at − queried_at`. 둘 다 노드의 시각이다.
- 신선하다: `0 ≤ 나이 ≤ margin_max_age_us`. 정확히 30초는 신선하다. `queried_at`이 `at`보다 미래면 신선하지 않다.
- 신선한 보고가 없으면 일반 주문을 `MarginUnavailable`로 거절한다. 감소 전용 주문은 신선한 보고가 필요 없다.
- 노출이 0이 아닌 종목 가운데 신선한 참조 가격이 없는 것이 하나라도 있으면 일반 주문을 `MarginUnavailable`로 거절한다.

### 계좌 레버리지 검사

종목마다, `pos`는 계좌 장부의 포지션, `ref`는 참조 가격, 합은 그 종목의 모든 미체결 주문(감소 전용 포함)의 남은 예약 수량에 대한 것이다.

```
N_up   = max(0,  pos × ref + Σ_매수 예약 × max(지정가, ref))
N_down = max(0, −pos × ref + Σ_매도 예약 × max(지정가, ref))
종목 노출 = max(N_up, N_down)
계좌 노출 = 종목 노출의 합
```

- 일반 주문을 `AccountLeverage`로 거절하는 조건: `받은 뒤 노출 > max_account_leverage × 계좌 가치` 이고 `받은 뒤 노출 > 받기 전 노출`.
- 정확히 한도와 같으면 받는다. 계좌 가치가 0 이하면 거절한다.
- 감소 전용 주문은 이 검사를 건너뛴다. 그래도 뒤에 오는 주문의 합에는 들어간다.
- 주문을 받기 전에만 검사한다. 가격이 움직여 한도를 넘어도 래치는 없고 취소도 없다.
- 취소를 요청한 것만으로는 노출이 줄지 않는다. 종결 증거가 와서 예약이 풀릴 때 줄어든다([C-CANCEL](order-lifecycle.md)).
- 성질: 주문을 더하면 노출은 줄지 않는다. 롱과 숏을 뒤집으면 노출이 같다. 계산은 `i128`이고 넘치지 않는다.

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

- 받은 보고의 격리 포지션 가운데 `position_value > 0`인 것마다 판정한다.
- 조건: `margin_used × 2 × max_leverage × 10 000 < position_value × (10 000 + 2 × max_leverage × mm_buffer_bps)`. 참이면 감소 전용 상태로 들어간다(`Rule(MmBuffer)`).
- `max_leverage` 40, 여유 25 bp, 포지션 가치 200: 격리 증거금 2.99는 걸리고 3.00은 걸리지 않는다.
- 한 종목에서 걸려도 계좌 전체가 감소 전용 상태가 된다.

### 감소 전용 상태

- 상태: `reducing = { rule, detail, since }` 또는 없음. `ReducingEntered`로 켜고 `ReducingCleared`로 끈다. 차단 사유 `Reducing`은 이 상태에서 계산한다.
- 들어갈 때, 같은 입력 안에서: `ReducingEntered`, 그리고 `reduce_only`가 아니고 취소 요청이 없거나 취소 시도가 다 된 모든 열린 주문에 `CancelRequested { by: Rule(rule) }`.
- 그 뒤로 일반 주문은 `Hold(Reducing)`으로 거절한다. 감소 전용 주문은 받는다. 걸려 있는 감소 전용 주문은 그대로 둔다. OMS는 포지션을 닫지 않는다.
- 이미 감소 전용 상태이면 다시 들어가지 않는다. 이벤트도 취소도 새로 내지 않고, 처음 건 규칙이 남는다.
- 계좌 전체에 하나다. 풀 수 있는 것은 운영자의 해제뿐이다. 재시작, 새 날, 회복은 풀지 않는다.

### 해제와 기준 재설정

| 명령 | 받는 조건 | 거절 사유 |
|---|---|---|
| `clear-reduce --note` | 감소 전용 상태다. 보고가 신선하다. 그 보고에서 유지증거금 여유 조건이 거짓이다. 사유가 비어 있지 않다 | `NotReducing`, `MarginUnavailable`, `MmBufferActive`, `NoteRequired` |
| `rebase-day --note` | 보고가 신선하다. 사유가 비어 있지 않다 | `MarginUnavailable`, `NoteRequired` |

- 둘 다 운영자 소켓에서만 받는다. 전략 소켓에서는 거절한다.
- 해제는 현재 계좌 가치를 새 기준 금액으로 삼는다. 날짜는 그 보고의 날짜로 둔다(앞선 날짜라도 그대로 쓴다). 그 뒤 새 기준에서 5 %를 넘게 잃으면 다시 걸린다(기준 94에서 89.30은 걸리지 않고 89.29는 걸린다).
- 기준 재설정은 감소 전용 상태를 건드리지 않는다. 날짜는 그 보고의 날짜로 둔다(앞선 날짜라도 그대로 쓴다).
- `--force`는 없다. 유지증거금 여유 조건이 참으로 읽히는 동안에는 해제할 수 없다.

### 보고 끊김 취소

- 조건: `policy.margin`이 있고, 링크가 올라와 있고, `at − max(마지막으로 받은 보고의 queried_at, 현재 링크 세션이 올라온 시각) > stale_cancel_us`.
- 동작: `Tick`이 `reduce_only`가 아니고 취소 요청이 없는 모든 열린 주문에 `CancelRequested { by: Rule(MarginStale) }`를 낸다. 주문마다 한 번이다. 취소 시도가 다 된 주문에 다시 내지 않는다.
- 래치도 차단도 없다. 새 보고를 받으면 끝나고, 일반 주문을 다시 받는다.
- 링크가 내려가 있으면 아무것도 취소하지 않는다.

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

누가 언제 보는가: 이벤트가 내구화된 뒤 `status`에 나타난다. `status`는 감소 전용 상태(규칙, 시각), 기준 금액, 마지막 보고의 계좌 가치와 나이, 계좌 노출, 종목별 거래소 설정, 계좌 모드, 마지막 보고 거부 사유를 보여 준다.

자원 상한: 보고는 5초에 하나이므로 저널에 하루 최대 17 280건을 더한다. 노출 계산은 열린 주문 수(상한 500)에 비례한다. 감소 전용 상태 진입이 내는 취소는 열린 주문 상한 이하이고 명령 채널 용량 안에 있다.

### 전략이 지켜야 하는 것

- 포지션을 줄이는 주문에는 반드시 reduce-only 표시를 한다. 표시가 없으면 감소 전용 상태와 보고 끊김 취소에서 취소되고, 새로 내면 거절된다.
- 표시한 익절 주문은 진입이 체결된 뒤에 낸다. 포지션이 없으면 거래소가 그 주문을 취소한다(`reduceOnlyCanceled`).
- OMS에는 조건부 주문이 없다. 손절은 조건이 됐을 때 전략이 IOC로 낸다. 감소 전용 상태에서는 그 IOC에도 표시가 있어야 한다.
- 언제 취소하고 다시 낼지는 전략이 정한다. OMS는 주문을 고치지 않고 수명 제한을 두지 않는다.

## 검증

시험은 아직 없다. 계획한 시험 이름은 티켓 파일(`.scratch/margin-rules/issues/`)의 수용 기준에 있다. 시험이 들어오는 티켓이 이 절과 `registry/claims.toml`을 채운다.

확인하지 않는 것: 실제 거래소의 응답 모양과 값의 뜻(T1, T2), 수수료와 펀딩이 빠지는 잔고(T3), 감소 전용 주문의 세부 처리(T5), 청산 체결의 모양(T6), 그 밖의 T4, T7~T11. testnet 실주문 예행은 이 작업 밖이고 mainnet 전에 필요하다(U-01, U-09).
