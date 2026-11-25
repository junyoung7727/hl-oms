# oms-projector 운영

주인: 준영
검토일: 2026-11-20

설계: [ADR-026](../adr/ADR-026-observability-projector.md). 읽는 규칙: [C-TAIL](../contracts/tail.md). 이 문서는 실행 방법, 내보내는 것, 상황별 의미를 적는다. 서비스 설치와 알림 규칙은 U20에서 정한다(아직 없음).

## 실행

```sh
oms-projector --config /etc/hl-oms/projector.toml
```

```toml
data_dir = "/var/lib/hl-oms/data"              # OMS 데이터 디렉터리(읽기만 함)
status_url = "http://127.0.0.1:8080/api/status" # loopback만 받음
venue = "bybit"                                # 모든 계열과 줄의 venue 라벨
cursor_file = "/var/lib/hl-oms/projector.cursor"
# host = "tokyo-1"                             # 기본: /etc/hostname
# poll_ms = 1000          dead_after_ms = 10000   disk_every_ms = 10000
# max_records = 1000      max_pending_lines = 20000

[telemetry]                                    # 없으면 아무것도 보내지 않음
vm_url = "http://127.0.0.1:8428"
vl_url = "http://127.0.0.1:9428"
# queue_lines = 100000                         # max_pending_lines보다 커야 함
```

- 데이터 디렉터리에 쓰지 않고 `LOCK`을 잡지 않는다. OMS가 실행 중이어도, 꺼져 있어도 된다.
- 커서 파일만 쓴다. 지우면 다음 시작 때 1번 기록부터 모든 줄을 다시 보낸다(중복).
- 다른 데이터 디렉터리의 커서 파일이면 시작하지 않는다(인스턴스 번호가 다름).
- `status_url`의 답이 다른 OMS 인스턴스면 쓰지 않는다(오류 줄 "status answer ignored", `oms_up=0`). 한 PC에 OMS가 여럿이면 포트를 확인한다.
- OMS와 같은 PID 네임스페이스(같은 호스트, 같은 컨테이너)에서 돌린다. 다르면 `/proc/locks`에서 OMS의 잠금이 보이지 않아, 살아 있는 OMS가 10초 답하지 않을 때도 죽은 것으로 보고 미확정 줄을 낸다.

## 지표(VictoriaMetrics)

모든 계열에 `venue`. 값은 내구화된 기록까지의 상태다. 시작한 뒤 재생이 처음 상한에 닿기 전(`projector_caught_up=0`)에는 상태 계열을 내지 않는다. 표와 계산은 `crates/oms-projector/src/metrics.rs` 머리말과 `limits`에 있다.

| 계열 | 볼 때 |
|---|---|
| `oms_up`, `oms_heartbeat_ts` | OMS가 상태 조회에 답하는가 |
| `oms_journal_durable_seq`, `projector_lag_records` | 프로젝터가 얼마나 뒤처졌나(줄 저장소가 죽으면 는다) |
| `oms_halt`, `oms_holds{reason}` | 새 노출을 막는 이유 |
| `oms_open_orders`, `oms_in_doubt_orders`, `oms_parked_fills` | 주문 상태 수 |
| `oms_position_qty`, `oms_realized_pnl`, `oms_upnl` `{symbol}` | 포지션과 손익(평균 단가법) |
| `oms_account_value`, `oms_margin_ratio{symbol}` | 마지막 증거금 보고 |
| `oms_limit{kind}`, `oms_limit_distance{kind}` | 한도와 남은 거리(음수 = 이미 넘음) |
| `projector_tail_error` | 1이면 저널 손상 또는 재생 불가로 멈춤 |
| `projector_cursor_seq`, `projector_pending_lines`, `projector_unconfirmed_records`, `projector_caught_up` | 프로젝터 자신 |
| `projector_rejected_lines` | 저장소가 영구히 거절한 줄(다시 보내지 않음, 커서는 지나감) |
| `journal_disk_free_bytes` | 저널 파일 시스템 여유(남은 일수는 vmalert가 계산, U20b) |

## 이벤트 줄(VictoriaLogs)

`service=oms`, `_time` = 기록 시각, 이벤트마다 한 줄. 필드: `seq`, `event_index`, `kind`, `order_id`, `wire_id`, `symbol`, `side`, `qty`, `px`, `phase`, `reason`, `fill_key`, `recv_ns`, `venue_ts_ns`, `unconfirmed`. 어디서 읽는지는 `crates/oms-projector/src/lines.rs` 머리말의 표.

- 같은 `(seq, event_index)`가 두 번 올 수 있다(재시작, 줄 버림 뒤 재전송). 조회할 때 하나만 쓴다.
- `unconfirmed=true` 줄은 OMS가 죽은 동안 디스크에서 읽은 내구화 너머의 기록이다. OMS가 다시 살아나면 같은 `seq`가 표시 없이 다시 오고, 그 내용이 다를 수 있다(복구가 잘린 꼬리를 버림). 표시 없는 줄을 믿는다.
- 프로젝터 자신의 줄은 `service=oms-projector`, `level` 필드(error, warn, info).

## 상황별

| 보이는 것 | 뜻 | 할 일 |
|---|---|---|
| `projector_tail_error=1`, error 줄 "journal tail stopped on damage" | 상한 이하 프레임 손상. 커서는 그대로 | 원장 판정은 OMS(C-JOURNAL). OMS 복구 보고와 `torn/`을 본다 |
| error 줄 "projector stopped: a durable record cannot be replayed" | 이 빌드가 기록을 읽지 못함(스키마) | 프로젝터를 OMS와 같은 버전으로 올리고 다시 시작 |
| `projector_lag_records`가 계속 는다, `projector_pending_lines` 큼 | VL이 받지 않음. 역압으로 읽기를 멈춤 | VL을 살린다. 줄은 버려지지 않았다 |
| warn 줄 "the push queue dropped log lines" | 큐가 넘쳐 줄이 버려짐. 커서 다음 기록을 모두 다시 보냄 | 한 번이면 정상 복구. 반복되면 `queue_lines`를 늘린다 |
| error 줄 "the log store refused lines for good", `projector_rejected_lines` 증가 | VL이 400·413·422로 거절. 그 줄은 없다 | VL 로그로 원인을 본다. 줄 모양이나 크기 제한 |
| error 줄 "status answer ignored" | `status_url`이 다른 OMS | 설정의 포트를 고친다 |
| `oms_up=0`인데 `unconfirmed` 줄이 없다 | `LOCK`이 잡혀 있음(멈춘 OMS), 아직 `dead_after_ms`가 안 지남, 잠금 확인 실패(stderr 경고), 꼬리 오류, 또는 상한 너머에 기록이 없음 | OMS 상태를 확인 |
