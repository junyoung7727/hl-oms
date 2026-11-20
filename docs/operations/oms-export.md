# oms-export 운영

설계: [ADR-027](../adr/ADR-027-trading-export.md). 행의 모양: 레이크 계약 4절(`contracts/lake/trading/*.table.json`). 읽는 규칙: [C-TAIL](../contracts/tail.md). 주기 실행(systemd 타이머)은 U20에서 정한다(아직 없음).

## 실행

```sh
oms-export --config /etc/hl-oms/export.toml
```

```toml
data_dir = "/var/lib/hl-oms/data"              # OMS 데이터 디렉터리(읽기만 함)
lake_root = "/mnt/nvme/lake"                   # crypto/trading/ 아래에 씀
status_url = "http://127.0.0.1:8080/api/status" # loopback만 받음
venue = "bybit"                                # 모든 행의 venue
market = "linear"                              # 모든 주문 행의 market
# status_timeout_ms = 2000   batch = 1000000
```

- 데이터 디렉터리에 쓰지 않고 `LOCK`을 잡지 않는다. OMS가 실행 중이어야 한다: 같은 인스턴스의 상태 답이 있어야 무엇이 내구화됐는지 안다.
- 끝난 UTC 날짜만 쓴다. 마지막으로 읽은 기록의 날짜(`open_date`)와 그 뒤는 쓰지 않는다. 어제 파일은 오늘 첫 기록이 내구화된 뒤 실행하면 나온다.
- 내용이 바뀐 파일만 바꾼다. 같으면 파일 시각도 그대로다. 여러 번 실행해도 된다. 한 번에 하나만 돈다(`crypto/trading/.oms-export.lock`).
- OMS의 `node.venue`가 `venue` 또는 `<venue>:mainnet`이어야 한다. testnet·sim 저널은 레이크에 쓰지 않는다.
- 레이크의 거래 파일을 지워도 된다(저널에서 다시 만든다). 저널은 지우면 안 된다.

## 출력과 종료 코드

표준 출력 한 줄(JSON):

```json
{"durable_seq":15,"read_to_seq":15,"open_date":"2026-01-03","written":["crypto/trading/orders/date=2026-01-02/day.parquet"],"unchanged":2,"rows":[3,2,20]}
```

`rows`는 완결된 날짜의 orders, fills, events 행 수다.

| 종료 코드 | 뜻 | 할 일 |
|---|---|---|
| 0 | 끝 | — |
| 3 | 상태 답이 없음, 다른 인스턴스의 답, 또는 `instance_id`나 `node.venue` 없는 답. 아무것도 쓰지 않음 | OMS가 떠 있는지, `status_url`의 포트가 이 데이터 디렉터리의 OMS인지 본다 |
| 1 | 그 밖(다른 거래소·testnet·sim의 OMS, 다른 실행이 잠금을 잡음, 내구화 번호보다 짧은 저널, Bybit 체결 키의 모양이 다름, 손상된 프레임, 모르는 기록 스키마, 계약에 안 맞는 행, 쓰기 실패). 오류는 표준 오류에 JSON 한 줄 | 손상이면 `oms-node verify --data-dir <dir>`로 저널을 검사한다([runbook](runbook.md) 7.3). 그 밖은 오류 문장을 본다. 이미 바꾼 파일은 완전한 파일이다 |

## 열의 출처

| 열 | 출처 |
|---|---|
| `venue`, `market` | 설정 |
| `symbol` | 저널의 `instrument` |
| `fills.exec_id` | 체결 키. Bybit 키 `<symbol>:<execId>`에서는 `execId` |
| `wire_id` | 주문 번호와 인스턴스 번호를 상태 답의 `node.wire_id_form`으로 쓴 것(Bybit `orderLinkId`). 답에 형식이 없으면 null |
| `fills.is_maker`, `fills.recv_ns` | 아직 null(저널에 없음, U16) |
| `fills.venue_ts_ns` | 저널의 `venue_ts`(µs) × 1000 |
| `events.payload` | 이벤트의 필드 객체, 키 정렬 |

우리 주문이 아닌 체결(`external_fill_booked`)과 거래소가 만든 위치 변경은 `fills`가 아니라 `events`에만 있다.
