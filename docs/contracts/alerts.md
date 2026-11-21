# 직접 알림 계약

상태: 채택, 구현됨(U18a2, 2026-11-21). 설계 근거: [ADR-029](../adr/ADR-029-oms-direct-alerts.md). 송신 규칙은 `crates/alert-direct`(U6, U18a1). 주장: V-ALERT-1~5. 실제 텔레그램에는 보낸 적이 없다(시험은 loopback 가짜만).

## C-ALERT 직접 알림

### 입력과 전제

- `[alert]` 설정이 있을 때만 동작한다. 없으면 알림이 없고 시작할 때 `warn` 한 줄, `status.alerts = {"configured": false}`. mainnet(`mainnet_confirmed = true`, Hyperliquid·Bybit)은 `[alert]` 없이 시작하지 않는다.
- 읽는 상태: 공개된 읽기 모델(`Published`, 내구화된 기록만)과 게이트웨이 막힘(Bybit만). 상태 소유자는 커밋 단계다. 알림 스레드는 저널도 코어도 쓰지 않는다.
- 이 계약은 저널 이벤트를 정의하지 않는다(`events = []`).

### 조건 표

메시지는 `[OMS P1] <거래소> <KST 시각>` 다음 줄부터 본문이고, 본문의 마지막 줄은 `근거: 저널 seq <공개된 durable_seq>`다. 문장은 PR #3의 `HOLDS` 표에 숫자를 더했다.

| 키 | 시작(내구화된 상태) | 해소 | 유예 |
|---|---|---|---|
| `hold:reducing` | `HoldReason::Reducing`(규칙, 내용, 시각) | `ReducingCleared`(운영자 `clear-reduce`) | 0 |
| `mismatch:<kind>` | 그 `MismatchKind`의 확인하지 않은 불일치 1개 이상(개수, 최근 3개의 subject: detail). 대조기의 불일치와 `venue_position_change`·`unclassified_execution` 포함 | 그 종류의 확인하지 않은 불일치가 없음 | 0 |
| `hold:operator_account` | `halt_account`(메모) | resume | 0 |
| `halt_strategy:<s>` | `halt_strategy`에 `s`(메모) | `s`의 resume | 0 |
| `hold:storage_low` | `storage_low`(5초마다 측정, 문턱 근처에서 들락거릴 수 있다) | `StorageLowChanged{low:false}` | 30초 |
| `margin_stale` | 증거금 정책 있음, 연결 올라옴, `!margin_report_is_fresh(now)`(나이 또는 "아직 없음", 신선도 한도, 취소 한도) | 신선한 보고 | 30초 |
| `hold:link_down` | `LinkDown`(게이트웨이 막힘이 있으면 문장에 더함) | 연결 올라옴 | 30초 |
| `hold:resync` | `Resync` | 확인 끝남 | 30초 |
| `hold:in_doubt` | `InDoubt`(개수, 가장 오래된 `doubt.since`) | 결과 불명 없음 | 30초 |
| `hold:venue_setting:<i>` | `VenueSetting(i)`(거래소가 말한 값과 정책) | 설정대로 | 30초 |
| `hold:account_mode` | `AccountMode`(거래소가 말한 모드) | 허용된 모드 | 30초 |
| `hold:no_config` | `NoConfig` | 설정 활성 | 30초 |
| `gateway_block` | 게이트웨이 막힘 `Some(이유)`(키 거부, IP 차단, 계좌, 확인 못 함, 시계) | `None` | 30초 |

- 유예 30초는 `alert.grace_ms`다. 조건은 평가마다 계속 있어야 하고, 유예가 지난 평가(`now − 처음 본 시각 ≥ grace_ms`)에서 시작이 나간다. 한 번이라도 없으면 처음부터 다시 센다. 유예 안에 끝난 조건은 시작도 해소도 보내지 않는다.
- 반복 규칙(alert-direct): 시작 1번, `repeat_ms`(5분)마다 그때의 문장으로 1번, 해소 때 `해소: <시작 문장>` 1번.
- 차단 문장은 `HoldReason`에 대한 와일드카드 없는 `match` 하나에서 나온다. `Mismatch`는 조건이 없고(종류별로 맡는다), `OperatorStrategy`는 `holds(None)`에 나오지 않는다(`halt_strategy:<s>`가 맡는다).
- 그 밖의 알림: 설정이 활성화된 뒤 첫 평가에서 한 번 `OMS 시작 (boot n, config v). 활성 조건 k개(유예 중 m개)`(k는 그 평가에서 시작이 나간 조건, m은 아직 유예 안의 조건), 정상 종료(코드 0)에 한 번 `OMS 정상 종료. 걸린 주문 n개는 거래소에 남음`.

### 내보내는 시점(확정 시점과 외부 효과)

1. 커밋 단계가 기록을 쓰고 flush가 성공한 뒤 1단계에서 읽기 모델을 고친다([C-COMMIT](commit.md)). 실패한 묶음은 읽기 모델에 닿지 않는다.
2. 스레드 `oms-alert`가 100 ms마다 먼저 치명 래치를 보고, 아니면 공개된 `durable_seq`가 바뀌었거나 `eval_ms`(1초)가 지났을 때 읽기 잠금 안에서 조건을 계산한다. 잠금을 푼 뒤 조건마다 `AlertHandle::set`을 부른다.
3. alert-direct의 송신 스레드가 텔레그램으로(둘째 채널이 있으면 그쪽에도) 보낸다. `set`과 `once`는 뮤텍스만 잡는다. 상태 파일이 없으므로 평가 스레드에서 파일을 쓰지 않는다.

그러므로 알림은 그 원인이 된 기록이 내구화된 뒤에만 나간다. 예외는 둘이다:

- **게이트웨이 막힘:** 게이트웨이 프로세스의 상태이고 저널 이벤트가 없다. 문장의 근거 줄이 그렇게 말한다(`근거: 게이트웨이 프로세스 상태(저널에 없음)`). 저널의 값을 담지 않는다.
- **치명 알림:** 내구성이 실패했다는 알림이다. 문장은 `치명 종료: <사유>. 노드가 멈춤(exit 2). 마지막 내구 seq <n>. 걸린 주문은 거래소에 남아 있음, 워처와 운영자가 확인해야 함`이고, 사유(`commit failed: <io error>`, 코어 결함은 입력 이름만)와 마지막 내구 seq 말고는 실패한 묶음의 어떤 것도 담지 않는다. 치명 래치를 처음 본 때 한 번 나가고, 그 뒤에는 조건을 평가하지 않는다(시작도 해소도 없다).

### 놓칠 수 있는 것

| 경우 | 결과 |
|---|---|
| 유예 0 조건이 평가 두 번 사이(보통 100 ms 미만)에 생겼다 풀림 | 알림 없음. 운영자가 푸는 래치라 드물다 |
| 유예 안에 끝난 지나가는 상태 | 알림 없음(의도) |
| 종료 전 전송(`exit_flush_ms`, 기본 15초) 안에 텔레그램이 받지 못함 | 그 알림을 잃는다. 둘째 채널이 있으면 그쪽이 사본을 받았다 |
| SIGKILL, abort, panic | 아무것도 보내지 못한다. 관측 경로(`oms_up` = 0)와 감시자 W5가 맡는다 |
| 보낼 것이 없을 때의 텔레그램 장애 | 모른다(탐침 없음) |
| 다시 시작 | 걸려 있는 조건마다 시작을 다시 보낸다(반복 상태를 저장하지 않음) |

### 종료

`node::run`은 drain 뒤, 런타임을 내리기 전에 평가 스레드를 멈추고 정지 알림(코드 0) 또는 치명 알림(코드 2, 이미 나갔으면 다시 넣지 않음)을 넣은 뒤 `exit_flush_ms`까지 전송을 기다린다. 그동안 `/api/status`는 `fatal`이 채워진 채 답한다. 텔레그램이 실패하면 alert-direct가 5초마다 다시 보내므로 15초 안에 세 번까지 시도한다.

### 설정(`[alert]`)

| 값 | 기본 | 검사 |
|---|---|---|
| `telegram` | 필수 | 절대 경로. 설정을 읽을 때 alert-direct 규칙으로 읽는다(0600, 일반 파일, 심볼릭 링크 아님, 저장소 밖, 토큰 형식). 못 읽으면 설정 오류(종료 코드 2). 오류 문장에 토큰이 없다 |
| `grace_ms` | 30 000 | 0..=600 000 |
| `eval_ms` | 1 000 | 100..=10 000 |
| `repeat_ms` | 300 000 | 60 000..=3 600 000 |
| `exit_flush_ms` | 15 000 | 1 000..=30 000 |
| `second_channel` | `"none"` | `"none"`만(D3) |
| `second_after_ms` | 60 000 | 10 000..=600 000 |

시험용: `failpoints` 빌드에서만 `OMS_TEST_TELEGRAM_API`(`http://<loopback 주소>:<port>`, 다른 호스트는 설정 오류)를 읽어 경로를 그 하나로 바꾸고 토큰 파일의 저장소 규칙을 건너뛴다. 배포하는 바이너리는 무시한다.

### 상태(`status.alerts`)

`configured`, `queued`, `delivered`, `dropped_total`, `failures`, `failing_since_ms`, `last_errors`(토큰 없음), `active`(시작이 나간 키), `second`(이름과 계수, 없으면 null), `source: "process counters"`. 토큰도 파일 경로도 보이지 않는다.

### 중복·순서·시간 초과·중단

| 상황 | 동작 |
|---|---|
| 같은 조건이 계속 | 반복 규칙대로. 문장(seq, 메모)이 바뀌어도 반복 시각에만 나간다 |
| 텔레그램 무응답 | 송신 스레드만 기다린다. 커밋 단계와 평가는 계속된다(1,000 기록 5초 안, 판단(모델 아님)) |
| 텔레그램 실패가 계속 | 보관함 50개, 넘치면 오래된 것부터 버리고 세며, 다 보낸 뒤 버린 수를 한 번 알린다. 둘째 채널이 있으면 60초를 넘은 실패를 그쪽에 알린다 |
| 전송 뒤 시간 초과 | 두 번 갈 수 있다(잃는 것보다 낫다, PR #3) |
| 노드 중단 뒤 다시 시작 | 내구화된 상태에서 다시 계산한다. 걸린 조건은 시작을 다시 보낸다 |

## 검증

| 성질 | 주장 | 시험 |
|---|---|---|
| 내구화된 것만, 치명 예외 | V-ALERT-1 | `pipeline::no_alert_for_a_batch_that_failed_to_flush`, `crash_points::a_flush_failure_sends_one_fatal_alert_and_nothing_of_the_batch`, `alerts::nothing_is_sent_before_its_record_is_durable`, `alerts::after_a_fatal_stop_only_the_fatal_message_is_sent` |
| 보관함 상한과 버린 수 | V-ALERT-2 | `alerts::outbox_bound_holds_and_drops_are_counted` |
| 조건 표, 유예, 반복 | V-ALERT-3 | `alerts::repeat_rule_with_an_injected_clock`, `alerts::grace_hides_a_short_link_down_and_reports_a_long_one`, 단위 시험 `alerts::tests` |
| 텔레그램 장애와 둘째 채널, 커밋 단계 독립 | V-ALERT-4 | `alerts::telegram_failing_over_60s_notifies_the_second_channel`, `alerts::a_hung_telegram_never_slows_the_commit_stage` |
| 설정, 시작·정지, 종료 전송 | V-ALERT-5 | `config::tests` 두 개, `alerts::the_token_file_is_loaded_with_its_rules`, `alerts::the_shipped_binary_ignores_the_test_telegram_api`, `crash_points::a_clean_stop_sends_the_stop_notice_with_the_open_orders` |
