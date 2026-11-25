# ADR-038 워처 알림 규칙, OmsDown 억제 규칙, 거래·사건 대시보드

상태: 채택 (2026-11-26).

운영 절차: [docs/operations/observability.md](../operations/observability.md) §9, §10, §12.8, §12.9, §13.7. 설계 출처: 통합 설계 원장(`RESEARCH/CRYPTO_TRADING_INTEGRATED_TN_20261118/10_DESIGN_DRAFT.md`, 저장소 밖) §5.4.5, §7.5~§7.7, §8.3, §8.4, 개정 R5(SHOULD 4). 앞선 결정: [ADR-035](ADR-035-observability-deployment-files.md)(관측 스택과 그 검사), [ADR-036](ADR-036-oms-deployment-files.md)(OMS 쪽 단위), [ADR-037](ADR-037-oms-alert-rules.md)(OMS·프로젝터·저널 알림).

## 1. 문제와 발생 가능한 실패

- 워처는 `watcher_*` 26개 계열을 VictoriaMetrics에 넣지만 그것을 읽는 규칙이 없었다. 워처가 죽으면 W5 킬도 대사도 없는데 스택은 알리지 않는다. 워처의 Bybit 키는 만료일이 있고(`watcher_api_key_days_left`), 만료되면 대사와 킬의 일괄 취소가 멈춘다.
- OMS가 꺼지면 워처의 W1 라운드(상태 API가 필요), 하트비트(보낼 OMS가 없음), W5(답이 와야 다시 무장)가 함께 나빠진다. 원인 하나에 P2 셋이 따라온다.
- 설계 §8.4의 화면 D1(거래)과 D4(사건 타임라인)가 없었다. "지금 한도까지 얼마나 남았나"와 "무슨 일이 어떤 순서로 있었나"에 답하는 곳이 없다.
- 저널 내보내기(`oms-export`)의 종료 코드 1(레이크 쓰기·스키마 오류)은 OMS가 살아 있을 때 나고 지표를 넣지 않는다. D3의 매일 확인 명령은 `oms-export*`를 이름하지 않았다.
- 검사가 없으면 키 문턱이 바뀌거나, 억제 규칙이 넓어지거나, D1에서 한도와 거리 표가 빠지거나, 새 저널 이벤트가 D4의 어느 목록에도 들지 않아도 모른다.

## 2. 일반적으로 만족해야 하는 성질

- 규칙과 화면은 코드가 실제로 넣는 이름과 라벨만 쓴다.
- 돈에 닿는 워처 상태(킬, 빚진 정지, 확정된 불일치, 거절된 키)는 곧바로 P1이다.
- 하나의 원인이 여러 알림을 내면 원인만 보낸다(설계 §8.3 B3). 원인과 무관한 알림(키 만료, 알림 경로 실패)은 막지 않는다.
- 사건 타임라인은 저널 이벤트 종류마다 보이는지 빼는지가 정해져 있다.

## 3. 제약

| 사실 | 출처 |
|---|---|
| 워처의 계열은 모두 `venue="bybit"`, `host`, `watcher`를 붙이고 `obs-push`를 쓰지 않는다 | `crates/watcher/src/metrics.rs:5`, `:38–41` |
| 네 킬 계열은 `[kill]`이 있을 때만 있다 | `crates/watcher/src/runtime.rs:455–462` |
| 재부팅 뒤의 킬은 OMS가 답한 적이 없어 마커 경로를 확인하지 못하고 정지를 빚는다(`watcher_kill_halt_owed` = 1), 첫 답에 `watcher_kill`로 래치한다 | `crates/watcher/src/kill.rs:1668–1673`, `crates/watcher/src/w5.rs:316` |
| 워처의 `export_ms`는 1 000~60 000 | `crates/watcher/src/config.rs:437` |
| 프로젝터는 저널 이벤트마다 VictoriaLogs에 한 줄(`service="oms"`, 필드 `seq`, `event_index`, `kind`, …)을 넣고, 다시 시작하면 같은 줄이 다시 올 수 있다 | `crates/oms-projector/src/lines.rs:8–22`, `:234`, 설계 §5.4.5 |
| 저널 이벤트 이름은 `Event::name`의 51개 | `crates/oms-core/src/event.rs:462–514` |
| vmalert는 `ALERTS`를 VictoriaMetrics에 쓰고, Grafana의 데이터 소스는 VictoriaMetrics와 VictoriaLogs 둘뿐이다(G4) | ADR-035 |
| `oms-export`는 지표를 넣지 않는다 | `crates/oms-export/src/main.rs:47–51` |

## 4. 필수 조건과 선택 조건

필수: 키 남은 날 < 14 = P2(R5 SHOULD 4), 워처 생존 P1(설계 §8.3 "모니터링 상실"), D1·D4(설계 §8.4), 검사가 규칙과 화면의 핵심을 고정. 선택: 직접 알림(`[WATCH P1]`)과 겹치는 `[MON P1]`을 줄이는 것(소유자 질문 Q7).

## 5. 대안

- A. OMS가 꺼진 동안 워처 알림을 모두 막음. B. OMS가 꺼지면 따라 나빠지는 셋(검사 경과, 하트비트, W5 무장 안 됨)의 P2·P3만 막음. C. 막지 않음.
- D. 열린 알림을 Alertmanager에서 읽음(데이터 소스 하나 더). E. vmalert의 `ALERTS`를 VictoriaMetrics에서 읽음.
- F. D4가 모든 이벤트를 보여 줌. G. 종류를 두 패널(주문·체결, 정지·킬·불일치)과 뺀 목록으로 나누고 검사가 그 분할을 지킴.
- H. 워처의 P1 넷을 규칙 파일에만 둠. I. 검사가 그 식·등급을 고정.

## 6. 비교

| | 장점 | 실패 모드 | 비용 |
|---|---|---|---|
| A | 문자가 가장 적음 | 꺼진 OMS 동안의 키 만료, 알림 경로 실패, 확정된 불일치가 숨음 | 패턴 하나 |
| B | 원인만 보냄 | 걸린 OMS(`OmsDown`)일 때 실제로 고장 난 W1도 숨음: `OmsDown` P1이 이미 옴 | 규칙 하나 |
| C | 단순 | OMS 정지마다 P2 셋 | 없음 |
| D | 보낸 알림만 보임 | 데이터 소스와 G4 규칙이 늘고, Alertmanager가 죽으면 화면도 빔 | 데이터 소스 하나 |
| E | 저장소 하나 | 침묵·억제된 알림도 보임(설명에 적음) | 없음 |
| F | 빠짐이 없음 | 5초마다의 하트비트와 기준가 줄이 사건을 덮음 | 없음 |
| G | 사람이 볼 사건만 | 새 이벤트 종류가 어디에도 없으면 숨음(G10이 막음) | 목록 셋 |
| H | 규칙 파일만 고침 | 편집 한 번에 킬 알림이 P2가 되거나 사라져도 CI가 모름 | 없음 |
| I | 돈에 닿는 페이지가 고정됨 | 문턱을 바꾸려면 검사도 고침 | 표 넷 |

## 7. 선택과 이유

B, E, G, I. 돈과 데이터에 안전한 쪽이다.

- 규칙 파일 `deploy/observability/vmalert/metrics/watcher.rules.json`, 그룹 `watcher`(10초). 거래 거래소마다 `WatcherDown`(`watcher_up` 1분 부재, P1). 곧바로 P1: `WatcherMismatchActive`, `WatcherKillActive`, `WatcherKillHaltOwed`(재부팅 뒤 꺼진 OMS에서도 울리는 것이 정상이라고 요약에 적음, ADR-036), `WatcherAuthRejected`. P2: `WatcherApiKeyExpiring`(< 14일), `WatcherPublicIpNotOk`(2분), `WatcherCheckStale`(30초 넘게 1분), `WatcherHeartbeatStale`(30초 넘게 1분), `WatcherW5Unarmed`(5분, 킬 중이 아니고 `[kill]`이 있을 때), `WatcherJournalError`, `WatcherAlertsFailing`, `WatcherAlertsDropped`. 설계 값이 아닌 시간과 문턱은 판단(모델 아님)이다.
- 셋째 억제 규칙(B): `OmsDown`이 같은 거래소의 P2·P3 `WatcherCheckStale|WatcherHeartbeatStale|WatcherW5Unarmed`를 막는다(설계 §8.3 B3 "oms_down이면 파생 알림 억제"). 워처의 P1과 키·알림 경로 알림은 막지 않는다.
- D1 `d1-trading`(거래): OMS 생존, 정지 사유, 포지션, 한도와 거리 표(`oms_limit`과 `oms_limit_distance`를 종류·종목마다 나란히), 손익, 증거금, 미체결과 결과 불명, 워처, 프로젝터, 열린 알림. 열린 알림은 `ALERTS{alertstate="firing"}`(E): 침묵되거나 억제된 알림도 나온다고 설명에 적는다.
- D4 `d4-events`(사건 타임라인): 거래소 하나를 고르는 변수, 주문·체결 패널(19종)과 정지·킬·불일치 패널(23종)은 VictoriaLogs의 `service="oms"` 줄을 `seq`, `event_index`와 함께 보여 주고, 프로젝터 자신의 warn·error 줄, 알림의 시간 순서(`ALERTS`). 하트비트, 기준가, 증거금 보고 같은 9종은 뺀다(판단(모델 아님)). 같은 `seq`·`event_index`의 줄은 한 사건으로 읽는다고 설명에 적는다.
- D3의 매일 확인 명령은 `systemctl --failed 'md-*' 'crypto-lake-audit*' 'oms-export*'`. 내보내기 종료 코드 1의 유일한 신호다.
- 검사(ADR-035의 xtask 통합 시험): 지표 목록에 워처의 26개 계열(R1·R2, 이름을 가진 워처 파일 여섯을 읽음). R6에 `WatcherDown`, R7에 키 규칙, 워처 P1 넷과 P2 일곱(`WatcherDown` 말고 모두)의 식·`for`·등급과 그룹 간격, R10에 셋째 억제 규칙. G6에 매일 확인 명령, G8에 D1·D4와 D1의 한도 표(표 패널, 두 식과 변환 그대로: `__name__`·`Time`을 빼고 `merge`)와 D4 두 사건 패널의 질의 틀(VictoriaLogs, 같은 필드·정렬·한도), 새 규칙 G10(`Event::name`의 종류마다 D4의 두 목록과 뺀 목록 중 정확히 하나, 목록의 이름은 모두 이벤트 종류).

## 8. 포기한 장점, 남은 위험, 소유자가 정할 것

- 위험: 걸린 OMS(`OmsDown`)일 때 워처의 W1·하트비트·W5가 다른 이유로도 고장 나 있으면 그 P2는 보이지 않는다. `OmsDown` P1이 이미 와 있다.
- 위험: 재부팅 뒤 사람이 OMS를 띄울 때까지 `OmsDown`과 `WatcherKillHaltOwed`(둘 다 P1)가 30분마다 되풀이된다(ADR-036, Q2). 같은 계열이 도는 OMS에서는 진짜 위험이라 규칙을 두었다.
- 위험: `WatcherDown`은 워처 `[intervals] export_ms` ≤ 20 000에 기댄다(1분 창에 세 번). 코드는 60 000까지 받고, 아무것도 그 값을 검사하지 않는다(운영 절차 §13.2).
- 위험: D4의 사건 줄은 프로젝터가 넣는다. 프로젝터가 죽거나 VictoriaLogs가 거절하면 타임라인에 구멍이 난다(`ProjectorDown`, `ProjectorLogsRejected`, ADR-037). 워처의 로그는 VictoriaLogs에 없다(`journalctl -u watcher`).
- 소유자가 정할 것(기본값으로 진행): (Q5) 문턱값(기본: 위의 값, 주간 검토), (Q7) 직접 알림과 겹치는 `[MON P1]`(기본: 두 길 모두), D4에서 뺀 9종(기본: 위의 목록).

## 9. 근거

- 시험: `crates/xtask/tests/deploy/`의 `the_deploy_tree_passes`, `catalogue::every_projector_and_watcher_series_is_catalogued`, `rules::a_rule_with_an_unknown_oms_metric_fails`, `rules::every_trading_venue_has_its_liveness_rules`, `rules::the_design_journal_and_key_rules_are_present`, `alertmanager::the_oms_inhibit_rules_hold_back_only_derived_alerts`, `grafana::a_dashboard_with_an_unknown_metric_fails`, `grafana::the_daily_check_names_the_export`, `grafana::the_trading_and_event_dashboards_exist`, `grafana::the_trading_dashboard_shows_limits_and_distances`, `grafana::the_event_dashboard_names_only_journal_event_kinds`.
- 변이: M383~M386(`cargo run -p xtask -- mutants --only M38`).
- 도구(CI 밖, 이 PC, 2026-11-26): `vmalert -dryRun`(v1.157.0)이 `watcher.rules.json`을 받아들였다. `vmalert-tool unittest`가 `watcher.test.json`(30개 경우: 알림마다 울리는 경우와 울리지 않는 경우, 다른 거래소의 워처, 재부팅 킬의 빚진 정지, `[kill]` 없는 워처의 W5)과 기존 넷을 통과했다. `amtool check-config`(0.35.1)가 억제 규칙 셋을 받아들였다. loopback의 임시 Alertmanager에 `OmsDown`(bybit)과 `WatcherCheckStale`(bybit, okx)과 `WatcherApiKeyExpiring`(bybit)을 넣자 bybit의 `WatcherCheckStale`만 억제됐다. loopback의 임시 VictoriaLogs v1.54.0에 프로젝터 모양의 줄(같은 `seq` 둘, 다른 거래소 하나, 뺀 종류 하나, 프로젝터 info·error 줄)을 넣고 D4의 질의를 돌리자 bybit의 사건만, 같은 `seq` 둘 다, 프로젝터 error 줄만 나왔다. 임시 Grafana 13.3.2가 D1·D4를 프로비저닝으로 읽었다.

## 10. 재검토 조건

- 저널 이벤트 종류를 더하거나 뜻을 바꿀 때(G10이 실패한다).
- 워처가 VictoriaLogs에 줄을 넣게 될 때(D4에 워처 패널).
- 주간 검토에서 워처 P2의 시간이나 억제 대상이 맞지 않을 때.
- 둘째 거래 거래소를 더할 때(워처의 거래소 라벨, R13).
