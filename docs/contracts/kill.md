# C-KILL: Bybit 킬 스위치(스트림 끊김 트리거와 확인 루프)

상태: 채택, 구현됨(U18b2, 2026-11-22). 설계 근거: [ADR-030](../adr/ADR-030-kill-switch-holds.md) §7(U18b2). 주장: V-KILL-1~5. **실제 거래소에서는 한 번도 실행하지 않았다**(loopback 가짜 Bybit, 가짜 텔레그램, 수동 시계만). 래치·해제·마커·`unsupported` 답은 [C-HOLD](holds.md)가 정한다. 이 계약은 Bybit 게이트웨이가 래치를 거는 트리거와, 래치가 내구화된 뒤 도는 일괄 취소 확인 루프를 정한다.

## 한눈에

| 단계 | 입력 | 처리 | 출력 | 실패하면 |
|---|---|---|---|---|
| K1 무장 | 시작 확인 첫 통과(`checks.rs` boot) | 마지막 증거 = 지금, 트리거 무장 | `kill_armed = true` | 확인 전에는 무장하지 않는다: 확인하지 않은 계좌로 일괄 취소를 보내지 않는다 |
| K2 증거 | 구독 확인(연결 올림), pong | 마지막 증거 = 지금. 코어 큐를 기다린 시간만큼 뒤로 민다(pong 기한과 같다) | `last_proof_age_ms` | — |
| K3 트리거 | 250 ms마다(`health_poll`) | 연결이 내려가 있고 `지금 − 마지막 증거 > kill_after_ms`이면, 끊김 하나에 한 번 | `VenueInput::StreamLost { since, detail }` | 연결이 올라가 있는 동안은 판단하지 않는다(스트림 자신이 pong 없음으로 내린다) |
| K4 래치 | `StreamLost` | 걸려 있지 않으면 `hold_set{kill, source: stream, note: "private stream down for N ms (no proof of life since …): …"}`. 걸려 있고 확인 전이면 아무것도 없음. 걸려 있고 루프가 0건을 확인한 뒤(`confirmed`)면 `kill_rearmed`(K9). `unsupported` 답 뒤면 아무것도 없음 | `Kill` 명령(내구화 뒤) | flush 실패면 명령 없이 멈춘다(C-COMMIT) |
| K5 C1 | `Kill` | 게이트웨이가 곧바로 "킬 활성"을 켠다: 그 뒤 생성은 `NotSent("kill switch active")`(래치 전에 풀린 생성도) | 루프 시작, 시작 P1 | — |
| K6 C2 | 회차 시작 `t_r` | 막힘(`allowed()`)이면 요청 없이 회차 실패(막힘 문장). 아니면 비상 예산의 칸을 기다려 받고 막힘을 다시 묻고 `POST /v5/order/cancel-all`(linear, USDT) | 성공 → C3, 실패 → C4 | 5xx·거부·응답 없음은 실패한 요청(호출 수에 든다). 연결도 못 한 요청은 보낸 것이 아니다 |
| K7 C3 | C2 성공 뒤 곧바로 | `/v5/order/realtime` 킬 범위 전 페이지, 열린 행(`New`, `PartiallyFilled`, `Untriggered`)을 센다. `orderLinkId`와 상관없이(OMS가 보내지 않은 주문도) | n = 0 → C5, n > 0 → C4 | 실패한 셈은 0이 아니다(C4). 1,000건(20쪽 × 50) 넘음도 실패 |
| K8 C4·C5 | n > 0 또는 실패 / 첫 0 | C4: `t_r + kill_round_ms`까지 기다려 다음 회차. C5: `kill_watch_every_ms`마다 C3을 다시, 늦은 주문(n > 0)이면 곧바로 C2, 셈 실패면 C4, `kill_watch_ms` 동안 모두 0이면 끝 | 끝: 마지막 P1, `KillLoopEnded { confirmed, rounds, cancel_all_calls, detail }` | 프로세스가 멈추면 루프도 끝난다. 다음 시작의 `Kill`이 새 루프를 돈다 |
| K9 다시 무장(N3) | 0건 확인(`outcome: confirmed`) 뒤의 새 계기(`StreamLost`, `watcher_kill`). `unsupported` 뒤는 아님: 거래소에서 확인한 것이 없고 게이트웨이가 같은 답을 한다 | `kill_rearmed{source, note}`: 확인(`confirmed_at`, `outcome`)을 지운다. 래치의 첫 메모는 그대로 | 새 `Kill`(내구화 뒤), 새 루프 | 새 루프가 확인하기 전 해제는 `kill_unconfirmed` |
| K10 멈춤 | `KillStop`(`clear_hold kill`이 내구화된 뒤) | 다음 대기에서 루프를 끝낸다. 알림도 입력도 없다. "킬 활성"을 끈다 | 생성이 다시 나간다 | — |

## 트리거

- 마지막 증거(`last_proof`)는 연결이 올라갈 때(인증과 구독 확인), pong마다, 시작 확인이 처음 통과할 때 정해진다. 게이트웨이가 코어 입력 큐를 기다린 시간은 pong 기한과 똑같이 증거를 뒤로 민다: 느린 코어는 끊김이 아니다.
- 결과: 마지막 pong `t_p` 뒤 소켓이 닫히면 `t_p + kill_after_ms`에 킬(그 전에 다시 구독되면 없음). 반쯤 열린 연결이면 스트림이 `t_p + 2 × ping`쯤 내려가고 킬은 `t_p + kill_after_ms`. ping에 답하는 조용한 스트림은 킬하지 않는다. 기본값(ping 5초, `kill_after_ms` 10초)으로 계획의 "소켓 닫힘 또는 10초 pong 없음"이다.
- 키 거부, IP 차단, IP 불일치로 연결이 내려가도 10초 뒤 킬이 걸리고, 루프는 보낼 수 없다고 알린다(사람이 알아야 한다).
- `detail`은 스트림이 마지막으로 끝난 이유(`no pong within … ms of a ping`, `closed by the venue`, `not connected: <막힘>` 등), 512바이트까지(판단(모델 아님)).
- 알려진 한계: 구독은 되지만 pong에 한 번도 답하지 않는 연결이 되풀이되면(올라감 → 2 × ping 뒤 내려감 → 다시 올라감), 올라갈 때마다 증거가 새로 정해져 킬이 늦거나 걸리지 않을 수 있다. 다시 연결하는 대기가 길어지면(0.5초부터 두 배씩 10초까지) 내려간 시간이 `kill_after_ms`를 넘을 때 걸린다. 스펙이 "연결 올림"을 증거로 정했다. pong만 증거로 할지는 소유자 결정 사항이다.

## 확인 루프

- `venue/bybit/kill.rs`의 `KillLoop`는 입출력 없는 규칙이고(시간이 입력, `RateBudget`처럼), 드라이버 `run`이 주입된 시계(`KillClock`: 운영은 tokio 시간, 시험은 수동 시계) 위에서 그 동작을 한다. 루프는 자기 태스크에서 돌고 명령 허가를 쓰지 않는다.
- 회차마다 일괄 취소 하나, `kill_round_ms ≥ 1,000`: OMS는 초당 많아야 하나를 보낸다. 비상 예산 `RateBudget::new(2, 2, 1 s)`(취소 등급, 루프의 시계)는 바깥 가드다: 어느 1초에도 일괄 취소가 둘을 넘지 않는다(칸은 답을 받은 뒤 1초까지 센다, `rate.rs`). 주문 예산(`orders.rs`)과 따로다: 일괄 취소는 거래소 한도 표의 자기 행이다(저장소 밖 `BYBIT_V5_GATEWAY_FACTS_20261120.md` Q11.6). 워처의 루프(U19b)가 같은 UID에 초당 하나를 더해 둘이 합쳐 초당 2 이하다(TN 표 7-8 C4).
- 셈은 일괄 취소의 답 바로 뒤에 읽는다(비동기 취소는 1초 뒤 다음 회차에 드러난다). 더 기다리지 않는 것은 판단(모델 아님)이다.
- 30초 감시로 충분한 이유: 래치 전에 나간 생성은 `ts + recv_window + 1 s`까지 처리되거나 버려진다(`orders.rs` `fits_expiry`, 기본 6초). 래치 뒤의 생성은 나가지 않는다.
- 루프가 도는 동안 두 번째 `Kill`은 기록하고 무시한다. 루프가 없을 때의 `KillStop`도 무시한다. 끝난(`done`) 뒤의 `Kill`(K9)은 새 루프를 돈다.
- 실행 중 프로세스가 멈추면 루프도 끝난다. 래치는 저널에 있고, 다음 시작의 `booted`가 `Kill`을 다시 요청한다(시작 확인이 통과하기 전에는 회차마다 `unchecked`로 실패한다).

## P1 알림(C4 규칙)

| 종류 | 언제 |
|---|---|
| 시작 | 루프가 시작할 때 |
| 수 바뀜 | 알려진 수 n > 0이 마지막으로 보인 수와 다를 때 |
| 반복 | 어떤 종류든 마지막 알림 뒤 `kill_notice_ms`가 지났을 때 |
| 마지막 | 0건을 확인하고 `kill_watch_ms` 동안 지켰을 때 |

- 모르는 수(C2나 C3의 실패)는 바뀜이 아니다. 0으로 가는 전이는 따로 알리지 않는다: 마지막 알림이 "0건 확인"이다(10분에 1 + 10 + 1 = 12, TN 표 7-8 C4).
- 한계: 길이 T의 어느 창에서도 루프는 2 + 수 바뀜 + ⌊T / notice_ms⌋ 이하를 보낸다. 10분 동안 셈이 실패하면(알려진 수 없음) 12 이하다.
- 모든 알림에 태그 `킬 루프`, 회차, 남은 수(모르면 `모름: <이유>`), 지금까지의 일괄 취소 수, 그리고 바로 전에 `/v5/position/list`에서 읽은 포지션 묶음이 든다(조회 경로, 주문 예산 아님, 10분에 12번 이하):

```
포지션(거래소 /v5/position/list, 11-22 12:00:00 KST, 강제청산가는 거래소 추정):
- BTCUSDT 매수 0.010, 미실현손익 -1.23, 마크 70000.0, 마크에서 강제청산가까지 12.34 %
- ETHUSDT 매도 0.5, 미실현손익 2.00, 마크 3000.0, 마크에서 강제청산가까지 모름(거래소가 청산가를 주지 않음)
```

- 크기 0인 행은 뺀다. 없으면 `포지션 없음`. 조회가 실패하면 `포지션 조회 실패(<종류>). 수량·손익 모름`. 거리 = |마크 − 청산가| / 마크 × 100, 0.01 %에서 내림. `liqPrice`가 비었거나 0이거나 읽을 수 없으면 `모름(거래소가 청산가를 주지 않음)`, 마크가 0 이하면 `모름(마크 가격 없음)`. 교차 증거금의 청산가는 거래소의 추정이다(저장소 밖 `BYBIT_V5_PRIVATE_API_FACTS_20261118.md`): 머리줄이 그렇게 말한다. 킬은 주문만 취소하고 포지션은 사람이 닫을 때까지 둔다(TN §7.6).
- 문장: 시작 `킬 루프 시작({출처}: {메모}). 모든 주문 거절, 미체결 0건 확인까지 1초마다 일괄 취소. 포지션은 닫지 않음`, 진행 `킬 루프 {r}회차: 남은 미체결 {n}건, 일괄 취소 {k}회. 마지막 오류: …`, 마지막 `킬 루프 정리 완료: 미체결 0건 확인 뒤 30초 동안 늦은 접수 없음({r}회차, 일괄 취소 {k}회). 해제는 운영자만: omsctl clear-hold --reason kill --note …`.
- 보내는 길: `[alert]`가 있으면 `Alerts::notice`(접두어 `[OMS P1]`, [C-ALERT](alerts.md)). 없으면 알림마다 `warn` 한 줄(`kill loop notice (no [alert]: not sent)`). 루프는 내구화 뒤에 풀린 명령에서 시작하므로 알림은 내구화된 래치에 대한 것이다.

## 설정(`[venue]`, Bybit)

| 필드 | 기본값 | 검증 |
|---|---|---|
| `kill_after_ms` | 10,000(설계 §7.6) | `2 × ping_interval_ms` ..= 60,000(생존 규칙이 끊김을 말하기 전의 킬은 거절) |
| `kill_round_ms` | 1,000(표 7-8 C4) | 1,000..=10,000(1초 이상이어야 OMS가 초당 일괄 취소 하나 이하) |
| `kill_watch_ms` | 30,000(C5) | 10,000..=300,000 |
| `kill_watch_every_ms` | 5,000(C5) | 1,000..=`kill_watch_ms` |
| `kill_notice_ms` | 60,000(C4) | 60,000..=600,000(60초 이상이어야 10분에 12개 한계) |

검증의 범위(기본값이 아니라 경계)는 모두 판단(모델 아님)이다. 결과: `ping_interval_ms`가 5,000을 넘는 설정은 `kill_after_ms`도 올려야 시작한다(`ping_interval_ms = 20000`이면 40,000 이상).

## 상태

`status.gateway`(프로세스 상태, 막힘 보기와 같다): `kill_loop`(`phase`: `idle` \| `rounds` \| `watch` \| `done` \| `stopped`, `round`, `remaining`(수 또는 null), `cancel_all_calls`, `last_error`, `started_at`, `zero_since`, `notices`), `kill_armed`, `last_proof_age_ms`, `source: "gateway process state"`. 가상 거래소와 Hyperliquid에서는 null이다.

## 이벤트

| 이벤트 | 필드 | 뜻 |
|---|---|---|
| `kill_loop_ended` | `outcome`(`confirmed` \| `unsupported`), `rounds`, `cancel_all_calls`, `detail` | 게이트웨이의 킬 루프가 끝났다. 래치는 남고 이 프로세스에서 확인된 것으로 표시된다(`confirmed_at`, `outcome`). 래치가 없거나 이미 확인됐으면 기록하지 않는다. `unsupported`는 일괄 취소가 없는 게이트웨이의 답이다(C-HOLD) |
| `kill_rearmed` | `source`(`stream` \| `watcher`), `note` | 0건 확인(`confirmed`) 뒤의 새 계기(K9). 확인을 지우고 새 `Kill`을 부른다. 래치의 `since`, `source`, `note`는 첫 것 그대로다 |

`StreamLost`는 새 입력(`venue.stream_lost`)이고 이벤트가 아니다. 래치는 C-HOLD의 `hold_set{source: stream}`이다. 두 이벤트 모두 열린 주문·체결·포지션 집합을 바꾸지 않는다([C-TAIL](tail.md) 필드 표).

## 장애와 결과

| 경우 | 결과 |
|---|---|
| 래치 기록의 flush 실패 | `Kill`이 나가지 않는다. 노드가 멈춘다. 다음 시작이 저널에서 래치를 찾으면 루프를 돈다 |
| 시작 확인 전의 끊김 | 무장하지 않아 킬 없음. 확인이 처음 통과한 때부터 `kill_after_ms` 안에 연결이 오르지 않으면 킬 |
| 일괄 취소 5xx | 실패한 회차, 호출 수에 든다, 1초 뒤 다시 |
| 셈 실패(조회 5xx, 읽을 수 없는 행, 1,000건 넘음) | 0이 아니다. 회차를 되풀이한다 |
| IP 차단 정지(`quiet`) | 일괄 취소·셈·포지션 조회 모두 나가지 않는다. 회차는 막힘 문장으로 실패한다 |
| 키 거부 | 회차마다 `auth_rejected`로 실패, 확인하지 못한다. 해제할 수 없다(강제 없음): 키를 폐기(X8)하고 새 키로 다시 시작 |
| 감시 중 늦은 주문 | 곧바로 C2, 감시는 다음 0부터 다시 |
| 0건 확인 뒤의 새 계기 | `kill_rearmed`, 새 루프, 그 확인 전에는 해제 거절 |
| `unsupported` 답 뒤의 새 계기 | 기록 없음, 응답 `already latched`. 해제는 여전히 장부가 빈 뒤에만(`kill_orders_open`) |
| 루프 도중 재시작 | 확인 없음. 새 프로세스의 루프가 다시 확인한다 |
| `clear_hold kill` 뒤 | `KillStop`이 루프를 끝내고 생성이 다시 나간다 |

## 범위 밖

워처의 루프(U19b), 가상 거래소와 Hyperliquid의 킬 루프(그 둘은 `unsupported`), 킬 뒤의 자동 청산(TN §7.6), 실행 중 마커 감시, 실제 거래소에서의 시험(testnet 예행은 소유자 승인 뒤, U-01·U-09).
