# ADR-032 워처의 직접 알림과 자기 킬: OMS가 조용하면 마커를 쓰고 OMS를 멈춘 뒤 자기 키로 일괄 취소

상태: 채택. U19b1a 구현(2026-11-24): `[telegram]` 설정과 mainnet 거부, 토큰 파일, `watcher::alert`의 순수 부분(메시지와 분할, 보관함, 응답의 결과, 반복 규칙). U19b1b 구현(2026-11-24): 송신 태스크(`alert::spawn`: 두 경로, `Notify`로 깨우기, 주입된 시계의 흘려보내기 간격, 지표, 종료 때 흘려보내기), loopback 가짜로만 시험. 실행이 아직 띄우지 않아 아무것도 보내지 않는다(조건과 실행 연결은 U19b1c). 이 ADR의 나머지 결정(§7의 루프, W5, 킬 순서)은 여기서 정하고, 구현은 U19b2a~U19b3d가 맡아 각 §에 자기 결과를 더한다. 계약: [C-WATCH](../contracts/watcher.md) 10부(U19b1), 11부(U19b2), 12부(U19b3). [ADR-031](ADR-031-watcher.md)을 고친다(아래 "ADR-031과의 관계").

## 1. 문제와 발생 가능한 실패

- U19a의 워처는 OMS가 살아 있을 때만 쓸모가 있다: 확정된 차이를 `admin.sock`에 보내고 하트비트를 보낸다. OMS 프로세스가 죽거나 HTTP에 답하지 않으면 그 미체결 주문은 Bybit에 남고 아무도 취소하지 않는다. OMS의 킬 루프(C-KILL)는 OMS 안에서 돌고, 이 계좌에는 거래소 쪽 데드맨이 없다(DCP는 기관용, `docs/contracts/kill.md`).
- 아무도 모른다: 워처는 메시지를 보내지 않고, 관측 스택은 따로 있는 길이라 같이 내려가 있을 수 있다. OMS의 직접 알림(ADR-029)은 OMS가 죽으면 같이 죽는다.
- 워처가 OMS를 멈추지 않고 일괄 취소만 하면, 멈춘 듯했던 OMS가 다시 살아나 새 주문을 내고, 두 프로세스가 같은 계좌에서 서로 반대로 움직인다.
- 멈춘 OMS가 다시 시작할 때(사람이든 systemd든) 래치 없이 거래를 시작하면 워처의 킬은 한 번뿐이고 다음 시작이 그것을 지운다.
- 알림 규칙이 없으면 10분의 조회 실패가 600개의 메시지가 되어 정작 중요한 것을 덮는다(설계 R2 SHOULD 4: 10분에 12개 이하).

## 2. 일반적으로 만족해야 하는 성질

- 워처는 여전히 OMS와 코드를 나누지 않는다(ADR-031 규칙 1~7, `alert-direct`도 포함). 송신기와 킬 루프는 문서로 다시 쓴다.
- 워처의 P1은 OMS가 죽어도 사람에게 간다(같은 봇, 접두어 `[WATCH P1]`, 소유자 결정 Q7·Q18).
- OMS가 10초 동안 정상 응답을 주지 않으면(설계 §7.5 W5, grill Q23) 워처는 첫 일괄 취소 **전에** 킬 마커를 쓰고 OMS를 멈춘다(`watcher_kill`, 안 되면 `systemctl stop`).
- 워처의 루프는 미체결 0건을 확인할 때까지 돈다. 일괄 취소는 어느 1초에도 둘 이하(설계 표 7-8 C4), P1은 10분에 12개 이하이고 킬 P1마다 포지션 묶음이 붙는다(R3 SHOULD 6).
- 토큰과 키는 어느 오류·로그·`Debug`에도 나오지 않는다.

## 3. 제약

| 사실 | 출처 |
|---|---|
| 독립성 규칙: 개발 의존성 `http-fake` 말고 작업 공간 crate 없음. `alert-direct`도 쓸 수 없다 | `tools/ci/watcher_independent.sh`, ADR-031 |
| PR #3의 전달 규칙: 분할 3,500 UTF-16, 보관 50개(넘치면 오래된 것부터 버리고 셈, 큐가 빈 뒤 알림 하나), 400·413 버림, 나머지 다시 보냄, DNS 다음 149.154.167.220, 요청 10초·연결 3초, 5분 반복 | `crates/alert-direct/src/lib.rs`(모형만, 링크하지 않음), ADR-029 §3 |
| `watcher_kill {detail}` → `hold_set{kill, source: watcher}`, 내구화 뒤 `"latched"` 등으로 답한다. `admin.sock`은 노드와 같은 uid만 받는다 | C-HOLD "워처의 입력", U18b1 |
| 킬 마커: OMS는 시작 때 한 번 읽고, 무엇이든 있으면 래치한다. 내구화된 `clear_hold kill` 뒤에만 지운다. `/api/status`의 `node.kill_marker`가 경로를 보인다 | C-HOLD, `crates/oms-node/src/node.rs`(읽기만) |
| `/api/status`는 코어 루프가 아니라 공개된 상태에서 HTTP 태스크가 만든다 | `crates/oms-node/src/status.rs:43-46` |
| 일괄 취소의 답은 미체결이 없을 때의 모양이 문서에 없다. 비동기이고 500개까지 | Bybit facts Q11.3, Q11.5, Q11.7 |
| 워처의 실행은 단일 스레드 tokio 루프와 100ms 틱이고, 기다림은 모두 sans-IO 단계가 돌려주는 시각이다 | ADR-031 §7, `runtime.rs` |
| OMS 단위는 지금 `Restart=on-failure`(런북), 설계는 `Restart=no`(U20b) | `docs/operations/runbook.md`, 설계 R2 SHOULD 5 |

## 4. 필수 조건과 선택 조건

필수: 2의 다섯 성질. mainnet은 `[telegram]`(U19b1a)과 `[kill]`(U19b3a) 없이 시작하지 않는다. 시작 확인이 끝나기 전에는 W5가 무장하지 않는다(확인되지 않은 계좌에 일괄 취소를 보내지 않는다).
선택: 둘째 채널, 텔레그램 상태 탐침 — 하지 않음(ADR-029와 같다). 포지션 청산 — 하지 않음(킬은 주문만 지운다, 설계 §7.6).

## 5. 대안

| 대안 | 설명 |
|---|---|
| A. `alert-direct`를 링크한다 | OMS의 송신기를 그대로 쓴다 |
| B. 워처 자신의 송신기(PR #3 규칙을 다시 씀) | 이 결정의 알림 부분 |
| C. OMS를 `halt`로 멈춘다 | 운영자 명령 `halt`를 `admin.sock`에 보낸다 |
| D. `watcher_kill`, 안 되면 `systemctl stop` 한 단위 | 이 결정의 멈춤 부분 |
| E. OMS를 신호로 멈춘다(`kill -TERM <pid>`) | pid를 찾아 신호를 보낸다 |
| F. 마커 없이 멈추고 일괄 취소 | 다음 시작은 래치 없이 |
| G. 마커를 쓰고 멈춘 뒤 일괄 취소 | 이 결정의 순서 |

## 6. 비교

| 기준 | A | B | C | D | E | F | G |
|---|---|---|---|---|---|---|---|
| 독립성 | 규칙 3이 막는다 | 지킨다 | — | — | — | — | — |
| OMS 기록에 남는가 | — | — | 운영자 `halt`로 남아 워처의 일과 섞인다 | `source: watcher`로 남는다(R5 SHOULD 1) | 남지 않는다 | — | — |
| OMS가 답하지 않을 때 | — | — | 실패 | `systemctl`로 멈춘다 | 같은 uid면 멈춘다 | — | — |
| 권한 | — | — | 같은 uid | 같은 uid 또는 systemd 권한 하나 | 같은 uid(워처가 OMS의 모든 프로세스를 죽일 수 있다) | — | — |
| 다음 시작 | — | — | — | — | — | 래치 없이 거래 | 마커로 래치 |

## 7. 선택과 이유

- **알림(B, U19b1)**: 워처는 PR #3의 전달 규칙을 `watcher::alert`에 다시 쓴다. A는 독립성 규칙 3이 막는다. 순수 부분(U19b1a)과 송신 태스크(U19b1b)를 나눈다: 분할, 보관함, 응답의 결과, 반복 규칙은 시계도 네트워크도 없는 함수라 단위 시험으로 끝까지 보이고, 송신 태스크는 그 규칙을 그대로 쓴다.
  - 접두어는 상수 `[WATCH P1]`, 머리는 `"[WATCH P1] bybit {name} {MM-DD HH:MM:SS KST}\n{body}"`(KST는 주입된 시계). 같은 봇을 써도 OMS의 `[OMS P1]`과 구별된다.
  - 응답의 결과: 2xx이고 본문 JSON의 `"ok": true`만 전달됨. 400·413은 이 메시지의 문제라 버리고 센다. 나머지(5xx, 429, 2xx인데 `ok`가 아님, 네트워크 오류)는 다시 보낸다. 대안 "5xx도 버린다"는 텔레그램이 잠깐 아플 때 P1을 잃는다.
  - 보관함 50개: 넘치면 가장 오래된 것을 버리고 센다(새 소식이 옛 소식보다 중요하다). 큐가 빈 뒤 알림 하나 `[WATCH P1] 전송 실패로 알림 N개를 버림`. 보내는 중에 넘쳐서 이미 버려진 부분은 두 번 지우거나 두 번 세지 않는다.
  - 반복 규칙: 시작 한 번, 지속되는 동안 300,000 ms마다 최신 문구로 한 번, 해소 한 번(`해소: <시작 문구>`). 메모리에만 있다(ADR-031의 "저장하지 않음"): 재시작하면 지속 중인 조건의 시작 문구가 한 번 더 간다. 시계가 뒤로 가면 반복이 그만큼 늦다(`saturating_sub`).
  - 토큰 파일: `alert-direct`와 같은 형식(`{"token": "<digits>:<[A-Za-z0-9_-]+>", "chat_id": …}`)이라 운영자는 한 파일을 둘에 줄 수 있다. 자격 파일과 같은 파일 규칙(일반 파일, 링크 아님, 소유자, `0600`·`0400`, 데이터 디렉터리 밖)에 더해 git 저장소 안이면 거부한다(토큰이 커밋되는 것을 막는다; `alert-direct`의 규칙). 시작 때 설정 다음, 시작 확인 전에 한 번 읽고, 거부는 종료 코드 2다. 그래서 시작 확인이 키를 거부할 때 송신기가 이미 있다(U19b1c).
  - mainnet은 `[telegram]` 없이 시작하지 않는다: OMS가 `[alert]` 없이 mainnet을 시작하지 않는 것과 같은 이유(워처의 P1이 사람에게 닿아야 한다). testnet은 `warn` 한 줄을 남기고, 모든 P1을 로그에만 쓴다(U19b1c).
- **멈춤(D)**: `admin.sock`의 `watcher_kill`이 먼저다(R5 SHOULD 1). C는 운영자 명령을 워처가 흉내 내는 것이라 기록에서 둘을 구별할 수 없다. `watcher_kill`이 실패하면(OMS가 답하지 않으면) `systemctl --no-ask-password stop -- <oms_unit>` 하나만 실행한다. 워처가 멈출 수 있는 것은 설정에 적힌 그 단위 하나뿐이다: 워처가 다른 것을 멈출 수 있으면 워처의 결함이 호스트의 결함이 된다. E는 pid를 찾는 규칙(같은 이름의 다른 프로세스, pid 재사용)과 systemd가 다시 띄우는 문제를 남긴다.
- **순서(G)**: K1 마커 → K2 `watcher_kill` → (K2가 래치하지 못했으면) K3 멈춤 → 루프. 첫 일괄 취소는 K1·K2(·K3)의 결과나 기한 뒤에만 나간다. F는 OMS가 다시 시작하면 래치 없이 거래하게 둔다. 마커는 OMS가 읽는 경로여야 한다(시작 때 `/api/status`의 `node.kill_marker`와 비교, U19b3c). 각 단계는 주입된 시계의 기한을 가져 디스크나 소켓이 걸려도 루프를 막지 않는다(U19b3b).
- **루프(U19b2)**: OMS의 `KillLoop`을 모형으로 다시 쓴 sans-IO `watcher::kill::KillLoop`(C2 일괄 취소, C3 세기, C4 반복, C5 30초 지켜봄). 일부러 OMS와 다른 것이 둘이다: (1) 실패한 일괄 취소 뒤에도 센다. 미체결이 없을 때의 일괄 취소 답이 문서에 없어(Q11.7), 그때 거래소가 거부하면 세지 않는 루프는 끝나지 않는다. 세기가 진실이다. (2) 바뀐 건수를 바로 알리지 않는다(60초 진행 알림만). 10분에 12개 이하를 지키기 위해서다. 그래서 두 루프가 함께 돌 때 OMS의 `킬 루프`가 건수 변화를 최대 60초 먼저 보일 수 있다. 일괄 취소 예산은 1,000 ms 반열린 창에 둘 이하이고 맞는 설정에서는 묶이지 않는다(타이밍이 바뀔 때의 방어). 결과는 U19b2a~c가 이 §에 더한다.
- **W5(U19b3a)**: `/api/status`에 1초마다 묻고, 10초 동안 정상 응답이 없고 그 사이 실패한 탐침이 하나라도 있으면 한 번 쏜다. 시작 확인이 끝난 뒤에만 무장한다(OMS가 이 프로세스에서 한 번도 답하지 않았어도: 워처가 OMS가 내려간 동안 시작해도 킬한다, 소유자 질문 1의 기본값). 워처가 보지 못한 시간(자기 프로세스가 멈췄거나 시계가 앞으로 뛴 시간)은 OMS의 침묵이 아니다(틈 규칙). 킬 뒤에는 정상 응답 하나를 볼 때까지 다시 무장하지 않는다(멈춘 OMS를 10초마다 다시 킬하지 않는다). 래치도 확인된 멈춤도 없이 끝난 킬은 정지를 빚지고, OMS가 다시 답하면 `watcher_kill`을 보낸다. 결과는 U19b3a가 이 §에 더한다.
- **W5가 보지 못하는 것**: `/api/status`는 HTTP 태스크가 공개된 상태로 만든다. 코어가 걸리고 HTTP 태스크가 답하는 OMS는 W5가 보지 못한다(하트비트의 `recorded`가 빠질 뿐이다). 설계가 `/api/status`를 신호로 정했으므로 규칙은 그대로 두고 한계로 적는다(C-WATCH 12부, 소유자 질문 11).
- **킬 중 엔진은 멈춘다(U19b3c)**: 킬 동안 새 라운드도 `watcher_mismatch`도 하트비트도 없다. 래치된 OMS는 30초 뒤 `hold:unmonitored`를 기록하지만, 그 보류는 킬 래치가 이미 막는 일반 주문만 막는다(소유자 질문 10의 기본값).
- **저장하지 않음**: 반복 상태, 킬의 진행 모두 메모리에만 있다. 킬 중에 다시 시작한 워처는 이어 가지 않고, 시작이 끝나면 W5를 다시 무장한다.

### ADR-031과의 관계

ADR-031 §2의 "잠금을 잡지 않고, 파일을 쓰지 않는다(C-TAIL)"와 §7의 "워처는 아무 파일도 쓰지 않는다"는 저널 디렉터리와 커서에 대해서는 그대로 참이다. U19b3부터 워처는 `data_dir` 밖에 파일 하나를 쓴다: 킬 마커(와 그 옆의 임시 파일). 이 ADR이 그 두 문장을 고친다. U19b1a는 아무 파일도 쓰지 않는다(토큰 파일은 읽기만 한다).

## 8. 포기한 장점, 남은 위험, 가정

- 포기: 이미 시험된 `alert-direct`. 같은 전달 규칙을 두 번 구현하고 두 번 시험한다(U19b1a의 단위 시험, U19b1b의 송신 태스크 시험).
- 포기: 지속 조건의 반복 상태를 재시작 너머로 들고 가는 것(재시작마다 시작 문구가 한 번 더 간다).
- 위험(U19b1a): 토큰 파일의 git 저장소 규칙은 조상 디렉터리의 `.git`(디렉터리 또는 worktree의 파일)만 본다. `GIT_DIR`로 다른 곳을 가리키는 저장소, 또는 `.git`을 읽을 수 없는 디렉터리는 보지 못한다(`alert-direct`와 같다).
- 위험: 실제 텔레그램에 보낸 적이 없다. 모든 시험은 loopback 가짜다(U19b1b부터: 평문 `http`, 고정 경로는 `telegram.invalid`를 가짜 주소로). 고정 경로가 TLS에서 호스트 이름으로 SNI와 인증서를 확인하는 것은 `reqwest`의 `resolve`에 기대고 확인하지 않았다(U19b1b).
- 가정(U19b1b): 송신 태스크는 자기 타이머 없이 `Notify`로만 깨고, 남은 부분의 다시 보내기는 실행의 틱이 주입된 시계로 깨운다(U19b1c가 실행에 연결한다). 틱이 멈추면 다시 보내기도 멈춘다(새 메시지는 바로 간다).
- 위험(U19b1a): `chat_id`가 틀리면 텔레그램의 400(`chat not found`로 본다, 확인하지 않음)을 "이 메시지의 문제"로 읽어 모든 P1과 버림 알림이 버려지고 세어질 뿐 사람에게 닿지 않는다(PR #3과 같다). 드러나는 곳은 로그 줄과 `watcher_alerts_dropped_total`(U19b1b)뿐이다. 그 지표의 경보 규칙이나 시작 메시지의 거부를 `error`로 남기는 것은 U19b1c·U20b가 정한다(C-WATCH 10부 알려진 한계).
- 가정: 400·413은 이 메시지의 문제이고 다시 보내도 낫지 않는다(PR #3). 429(`retry_after`)는 따로 읽지 않고 다시 보냄으로 다룬다(다음 흘려보내기, U19b1b).
- 루프, W5, 킬 순서의 남은 위험과 가정은 U19b2a~U19b3d가 이 §에 더한다(설계 값: 1초 탐침, 10초 침묵, 1초 라운드, 30초 지켜봄, 60초 알림).

## 9. 근거

- 시험(U19b1a): `crates/watcher/tests/alert.rs` 6개(`alert::messages_split_at_3500_utf16_units_at_line_ends`, `alert::the_outbox_keeps_50_parts_and_reports_the_drops_once`, `alert::answers_400_and_413_drop_the_part_and_other_answers_keep_it`, `alert::the_repeat_rule_sends_start_every_5_min_and_clear`, `alert::the_token_is_never_in_debug`, `alert::a_message_has_the_watch_p1_header_with_the_name_and_kst`), `config::telegram_is_validated_and_required_on_mainnet`, `creds::the_token_file_must_be_owner_only_outside_any_repository`, 확장한 `runtime::a_start_refusal_ends_the_run_with_exit_code_2`(주장 V-WATCH-11). 변이 M255~M260.
- 시험(U19b1b): 같은 파일에 6개(`alert::a_network_error_moves_to_the_pinned_route`, `alert::the_token_is_never_in_a_log_line`, `alert::the_sender_task_flushes_on_the_manual_clock`, `alert::a_retry_keeps_the_part_for_the_next_flush`, `alert::shutdown_flushes_within_5_s_and_counts_the_rest`, `alert::set_queues_what_the_repeat_rule_gives`), 리뷰 1 뒤 하나 더(`alert::a_request_without_an_answer_fails_after_10_s`, 요청 한도 10초), 변이 M261. 연결 한도 3초와 종료 한도의 `abort`는 시험하지 않았다(C-WATCH 10부 알려진 한계). 주장 V-WATCH-11.
- 설계: `10_DESIGN_DRAFT.md` §7.5(W5), §7.6(킬 스위치 표), §7.7(직접 알림), R1(C1~C5), R2 SHOULD 4~6, R3 SHOULD 6, R5 SHOULD 1·4(저장소 밖, 스펙 `.scratch/watcher-kill/spec.md`가 줄을 인용한다).

## 10. 재검토 조건

- 텔레그램이 응답 규칙(400·413의 뜻, 4,096 한도)을 바꿀 때, 또는 둘째 채널을 정할 때(ADR-029 D3).
- OMS의 `/api/status`가 코어 루프를 거치게 될 때(W5의 한계가 사라진다), 또는 OMS 단위가 `Restart=no`가 될 때(U20b: 빚진 정지의 이유가 줄어든다).
- Bybit가 거래소 쪽 데드맨(DCP)을 이 계좌에 줄 때.
- 워처가 실제 거래소와 실제 텔레그램에서 처음 돌 때(U20b, testnet).
