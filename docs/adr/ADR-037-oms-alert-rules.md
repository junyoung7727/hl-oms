# ADR-037 OMS·프로젝터·저널 알림 규칙과 보류 알림 등급

상태: 채택 (2026-11-26).

운영 절차: [docs/operations/observability.md](../operations/observability.md) §9, §10, §13.3~§13.5. 설계 출처: 통합 설계 원장(`RESEARCH/CRYPTO_TRADING_INTEGRATED_TN_20261118/10_DESIGN_DRAFT.md`, 저장소 밖) §5.4.5, §8.1~§8.3, 개정 R4(SHOULD 2), TN 표 7-7·9-4. 앞선 결정: [ADR-035](ADR-035-observability-deployment-files.md)(관측 스택과 그 검사), [ADR-036](ADR-036-oms-deployment-files.md)(OMS 쪽 단위).

## 1. 문제와 발생 가능한 실패

- 프로젝터는 `oms_*`, `projector_*`, `journal_disk_free_bytes`를 VictoriaMetrics에 넣지만 그것을 읽는 규칙이 없었다. OMS가 죽어도(`Restart=no`라 사람이 올 때까지) 스택은 알리지 않고, 저널 파일 시스템이 차도 남은 일수가 없다.
- 보류(`oms_holds{reason}`)의 사유는 열여섯 가지이고 성격이 다르다. 사람만 푸는 래치, 시작마다 잠깐 켜지는 `unmonitored`, 스스로 풀리는 것, 운영자가 건 것. 모두 같은 P1이면 정상 시작마다 페이지가 오고, 모두 같은 지연이면 킬 래치가 늦게 온다.
- `oms_holds`는 저널의 마지막 상태라 OMS가 꺼진 동안에도 남는다. 계획된 정지가 남긴 `unmonitored`가 30분마다 P1을 되풀이할 수 있다.
- 시세 쪽의 `MdObsPushDropping`은 모든 서비스의 `obs_push_dropped_total`을 더해, 프로젝터의 손실을 시세 알림으로 보내고 `MdCollectorDown`이 그것을 막았다.
- 검사가 없으면 새 보류 사유가 어느 알림에도 들지 않거나, 규칙이 프로젝터가 넣지 않는 이름을 쓰거나, 억제 규칙이 넓어져도 모른다.

## 2. 일반적으로 만족해야 하는 성질

- 규칙은 코드가 실제로 넣는 이름과 라벨만 쓴다.
- 사람이 풀어야 하는 보류는 곧바로 P1이고, OMS가 꺼져 있어도 보인다.
- 정상 시작과 계획된 정지는 P1을 내지 않는다(꺼진 OMS 자체의 `OmsDown`만).
- 하나의 원인이 여러 알림을 내면 원인만 보낸다(설계 §8.3 B3). 원본 데이터를 잃는 알림은 막지 않는다.

## 3. 제약

| 사실 | 출처 |
|---|---|
| `oms-node`는 지표를 넣지 않는다. OMS 생존은 프로젝터의 `oms_up`(상태 API가 답하면 1)뿐이다 | `crates/oms-projector/src/metrics.rs:118`, `crates/oms-node/src/`에 `obs_push` 없음 |
| `oms_holds{reason}`은 `Core::holds(None)`의 사유이고, 이번 실행에서 본 사유는 0으로 남는다. 첫 따라잡기 전에는 상태 계열이 없다 | `crates/oms-projector/src/metrics.rs:134–145` |
| `operator_strategy`와 `strategy_link`는 전략을 지정할 때만 더해져 프로젝터가 넣지 않는다 | `crates/oms-core/src/core.rs:220–224`, `:257–261` |
| 모니터링 관문이 있으면 모든 시작이 워처의 첫 하트비트까지 `unmonitored`이고, 60초 뒤 OMS가 걸린 일반 주문을 취소한다 | `crates/oms-core/src/core.rs:262–266`, `crates/oms-core/src/config.rs:132` |
| `journal_disk_free_bytes`는 `disk_every_ms`(기본 10초)마다 `statvfs`가 성공할 때만 온다 | `crates/oms-projector/src/projector.rs:640–656` |
| 워처의 W5는 OMS가 10초 조용하면 킬한다 | `docs/operations/runbook.md` 7.18 |
| 워처의 계열은 `venue="bybit"`만 붙인다 | `crates/watcher/src/metrics.rs:40` |
| 수집기는 OMS 없이 레이크에 쓴다 | ADR-035 |

## 4. 필수 조건과 선택 조건

필수: 설계 §9.4의 저널 남은 일수(7일 P2, 2일 P1, 5분 부재 P2), OMS 생존 P1, 보류 사유마다 한 등급, 검사가 규칙의 이름과 등급을 고정. 선택: 직접 알림과 겹치는 `[MON P1]`을 줄이는 것(소유자 질문 Q7).

## 5. 대안

- A. 보류마다 같은 P1. B. 래치만 P1, 나머지는 대시보드. C. 등급 넷: 래치(곧바로 P1), 관문 `unmonitored`(2분 P1), 지나감(5분 P1), 운영자(P3).
- D. 관문·지나감을 OMS 생존과 무관하게. E. 관문·지나감만 `oms_up` = 1일 때. F. 모든 등급을 `oms_up` = 1일 때.
- G. OMS가 꺼진 동안 시세 알림을 막음(범위의 문장). H. 막지 않음.
- I. 프로젝터가 죽으면 OMS 알림을 모두 막음. J. 프로젝터가 유일한 작성자인 저널 계열 알림만 막음.

## 6. 비교

| | 장점 | 실패 모드 | 비용 |
|---|---|---|---|
| A | 단순 | 정상 시작마다 `unmonitored` P1, 링크가 잠깐 끊겨도 P1 | 없음 |
| B | 페이지가 적음 | 5분 넘게 끊긴 연결로 거래가 멈춰도 아무도 모름 | 없음 |
| C | 사유의 뜻에 맞춤 | 등급 없는 새 사유는 알림이 없음(검사 R12가 막음) | 목록 넷 |
| D | 꺼진 OMS의 상태도 보임 | 계획된 정지·충돌 뒤 `unmonitored`·`link_down`이 30분마다 P1 | 없음 |
| E | 꺼진 OMS는 `OmsDown` 하나 | 걸린 OMS(`oms_up` = 0)의 지나가는 보류는 `OmsDown`으로만 보임 | 식 하나 |
| F | 가장 조용함 | 꺼진 OMS의 킬 래치가 다음 시작까지 안 보임 | 식 하나 |
| G | OMS 장애 때 문자가 적음 | `Restart=no`라 몇 시간일 수 있는 장애 동안 원본 시세 손실이 숨음 | 규칙 하나 |
| H | 시세 손실이 늘 보임 | OMS 장애 때 문자가 더 옴 | 없음 |
| I | 문자가 적음 | 프로젝터가 죽으면 OMS 알림은 어차피 값이 없음: 얻는 것이 없음 | 패턴 하나 |
| J | 원인만 보냄 | 없음 | 규칙 하나 |

## 7. 선택과 이유

C, E, H, J. 돈과 데이터에 안전한 쪽이다.

- 규칙 파일 `deploy/observability/vmalert/metrics/oms.rules.json`. 기록 `journal_disk_days_left`(레이크 규칙과 같은 모양, 그룹 `journal-recording` 1분), `JournalDiskDaysLeftLow`(< 7, P2)와 `…Critical`(< 2, P1), 둘 다 10분 지속과 12시간 준비 시간. 거래 거래소마다 `JournalDiskGaugeMissing`(5분 부재, P2), `OmsDown`(30초 동안 `oms_up`이 모두 0, P1; 30초는 W5의 10초보다 길어 워처가 먼저 움직인다), `ProjectorDown`(`projector_caught_up` 2분 부재, P2). 한도(`OmsLimitPassed` P1, `OmsLimitNear` 10% P2), `OmsParkedFills`, 프로젝터 자신의 다섯(꼬리 오류, 지연, 따라잡지 못함, 거절된 줄, 버린 줄).
- 보류 알림 등급(C). 래치 `kill_latched|watcher_mismatch|venue_change|reducing|mismatch|storage_low` → `OmsHoldLatched` 곧바로 P1. 관문 `unmonitored` → `OmsUnmonitored` 2분 P1(정상 시작의 몇 초를 넘기고, OMS의 60초 취소 뒤 약 1분 안에 페이지). 지나감 `link_down|resync|in_doubt|no_config|venue_setting|account_mode` → `OmsHoldLasting` 5분 P1. 운영자 `operator_account` → `OmsOperatorHalt` P3. 시간은 판단(모델 아님).
- OMS 생존 관문(E). 관문과 지나감은 `and on(venue) max by (venue) (oms_up) == 1`. 래치와 운영자는 관문 없음: 꺼진 OMS의 래치도 다음 시작에 남는다.
- 거래 거래소 목록. `venues.toml`의 `trading = ["bybit"]`: 이 PC의 OMS가 거래하는 거래소. 그 거래소는 워처가 라벨로 붙이는 거래소여야 한다.
- 억제(J, H). 둘째 억제 규칙: `ProjectorDown`이 같은 거래소의 P2·P3 `JournalDiskGaugeMissing`을 막는다. OMS가 꺼진 동안 시세 알림은 막지 않는다: 범위가 요구한 것과 다르다(소유자 질문 Q4). `MdObsPushDropping`은 `service="md-collector"`만 읽고, 프로젝터의 손실은 `ProjectorObsPushDropping`이다.
- 검사(ADR-035의 xtask 통합 시험): 지표 목록에 프로젝터의 23개 계열(R1·R2). R6에 거래 거래소마다의 세 규칙, R7에 저널 규칙과 그룹 간격, R10에 억제 규칙 둘의 순서와 값, 낡은 대상 이름 금지, 수집기 패턴에 걸리는 알림이 수집기가 넣지 않는 지표를 읽는 것 금지. 새 규칙 R12(`HoldReason`의 사유마다 한 등급, 관문은 `unmonitored` 하나, 네 보류 알림의 식 고정)와 R13(거래 거래소마다 워처의 `("venue", "<V>")`).

## 8. 포기한 장점, 남은 위험, 소유자가 정할 것

- 위험: 프로젝터가 죽으면 스택에는 OMS 생존 정보가 없다. `OmsDown`도 울리지 않고 P2 `ProjectorDown` 하나만 온다. OMS와 워처의 직접 알림이 그 경로다(설계 §8.1).
- 위험: 걸린 OMS(프로세스는 있지만 상태 API가 답하지 않음)의 지나가는 보류는 `OmsDown`으로만 보인다.
- 위험: 규칙은 운영자 설정의 세 값(프로젝터 `poll_ms` ≤ 10 000, `disk_every_ms` ≤ 60 000, 워처 `export_ms` ≤ 20 000)에 기대고, 그 값은 아무것도 검사하지 않는다(운영 절차 §13.2).
- 소음: 전략이 한도에 붙어 있는 동안 `OmsLimitNear`(P2)가 계속 울린다. 결함이 아니다.
- 소유자가 정할 것(기본값으로 진행): (Q4) OMS 장애 동안 시세 알림 억제(기본: 하지 않음), (Q5) 문턱값과 그것이 기대는 설정 상한(기본: 위의 값, 주간 검토), (Q7) 직접 알림과 겹치는 `[MON P1]`(기본: 두 길 모두).

## 9. 근거

- 시험: `crates/xtask/tests/deploy/`의 `the_deploy_tree_passes`, `catalogue::every_projector_and_watcher_series_is_catalogued`, `rules::a_rule_with_an_unknown_oms_metric_fails`, `rules::the_design_journal_and_key_rules_are_present`, `rules::every_trading_venue_has_its_liveness_rules`, `rules::every_hold_reason_has_an_alert_class`, `alertmanager::the_oms_inhibit_rules_hold_back_only_derived_alerts`.
- 변이: M376~M380(`cargo run -p xtask -- mutants --only M37`, `--only M38`).
- 도구(CI 밖, 이 PC): `vmalert -dryRun`(v1.157.0)이 규칙 파일 넷과 로그 규칙을 받아들였다. `vmalert-tool unittest`가 `oms.test.json`(알림마다 울리는 경우와 울리지 않는 경우, 등급과 `oms_up` 관문)과 기존 셋을 통과했다. `amtool check-config`(0.35.1)가 억제 규칙 둘을 받아들였다. loopback의 임시 Alertmanager에 `ProjectorDown`과 `JournalDiskGaugeMissing`(같은 거래소)을 넣자 둘째가 억제됐고, 다른 거래소의 것은 억제되지 않았다.

## 10. 재검토 조건

- 코어에 보류 사유를 더하거나 사유의 뜻(래치인지)을 바꿀 때(R12가 실패한다).
- OMS가 스스로 지표를 넣게 될 때(생존을 프로젝터에 기대지 않게 됨).
- 주간 검토에서 `OmsUnmonitored`의 2분이나 `OmsHoldLasting`의 5분이 맞지 않을 때.
- 둘째 거래 거래소를 더할 때(워처의 거래소 라벨, R13).
