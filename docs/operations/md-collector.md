# 시세 수집기 운영 (md-collector, md-lake)

수집기는 거래소마다 한 프로세스다. 받은 프레임을 모두 레이크의 1단계 파일(열린 파일·세션 파일)에 쓰고, `md-lake seal`이 매시 닫힌 시간을 봉인한다. 계약은 quant-data-platform `docs/crypto/lake-contract.md` §2, 설계는 `.scratch/md-collector/spec.md`(U5b)다. 시장을 `conn_id`로 정하는 규칙은 [ADR-019](../adr/ADR-019-market-conn-slots.md)에 있다.

이 저장소는 서비스를 설치하거나 켜지 않는다. `deploy/systemd/`의 파일은 틀이다.

## 1. 명령

```text
md-collector run   --config <file>   # 수집. SIGTERM이면 파일을 닫고 0으로 끝난다
md-collector check --config <file>   # 설정만 검사(네트워크·파일 없음). 0 = 통과, 2 = 오류
md-lake seal      --venue V --lake DIR --first-hour YYYY-MM-DDTHH [--layout by_symbol|by_connection]
md-lake seal-days --venue V --lake DIR --first-hour YYYY-MM-DDTHH --from YYYY-MM-DD
md-lake verify    --venue V --lake DIR --date YYYY-MM-DD --json
```

| 종료 코드 | md-collector | md-lake |
|---|---|---|
| 0 | SIGTERM·Ctrl-C로 정상 종료 | 끝남(잠긴 파일 때문에 기다리는 시간은 보고만 함) |
| 1 | 시작 실패(종목정보 실패 포함), 쓰기 오류 | 봉인 오류(`Unroutable`, `UnknownSource`, `Check`, 입출력) |
| 2 | 설정 오류 | 사용법 오류, 등록되지 않은 거래소(파일을 건드리지 않음), 미래의 `--now-ns` |
| 3 | — | 같은 거래소의 다른 봉인이 실행 중 |

`--now-ns`는 과거 상태를 다시 돌릴 때만 쓴다. 미래 값은 거절한다(`MD_LAKE_TEST_ALLOW_FUTURE_NOW=1`은 시험 전용).

## 2. 설정 (TOML)

모든 표에 모르는 키가 있으면 시작을 거절한다.

| 키 | 기본값 | 뜻 |
|---|---|---|
| `venue` | (필수) | 등록된 거래소. 지금은 `bybit`뿐 |
| `collector` | `md_collector` | 파일 이름에 들어가는 작성자 이름 `^[a-z0-9][a-z0-9_]*$` |
| `lake_root` | (필수) | 레이크 루트 |
| `host` | `/etc/hostname`, 없으면 `unknown` | 파일 머리의 호스트 |
| `write.flush_ms` / `write.fsync_ms` | 1000 / 5000 | zstd 블록 flush, fsync 간격 |
| `write.queue_bytes` | 16 MiB | 연결 자리 하나의 받기 큐 상한. 전체 상한은 512 MiB(코드 상수) |
| `disk.floor_bytes` / `disk.margin_bytes` / `disk.check_ms` | 20 GB / 20 GB / 10 s | 디스크 하한(판단: 전체 계획 약 하루치) |
| `day_change.start_offset_ms` / `stagger_ms` | 5000 / 2000 | 자정 전환 시작(00:00:05 UTC)과 연결 간격 |
| `instruments.refresh_ms` | 3600000 | 종목정보 다시 받기 간격 |
| `[[markets]] name` | (필수) | 거래소의 시장(`linear`, `inverse`, `spot`). 한 번씩만 |
| `symbols` | (필수) | 목록, 또는 `"all"`(시작 때 거래 중인 종목 전부) |
| `channels` | (필수) | 모든 종목의 채널(`orderbook.50`, `publicTrade`, `tickers`) |
| `conn_budget` | (필수) | 이 시장의 연결 수 상한. 시장 합계 ≤ 250(Bybit: 5분 500개 신규 연결의 절반) |
| `max_topics_per_conn` | 200 | 연결 하나의 토픽 수 |
| `ws_url` / `rest_base` | 거래소 기본값 | 시험에서만 바꾼다 |
| `[[markets.extra]] channel, symbols` | 없음 | 일부 종목의 추가 채널(예: `orderbook.1000`) |
| `[[traded_channels]] market, symbol, channel` | 없음 | 전략이 읽는 토픽. 모두 수집 대상이어야 한다(아니면 시작 거절). 별도 연결로 계획하고 디스크 하한에서도 남긴다 |
| `loopback_only` | false | 시험 전용: 루프백이 아닌 주소를 거절한다 |

예:

```toml
venue = "bybit"
lake_root = "/mnt/nvme/lake"

[[markets]]
name = "linear"
symbols = "all"
channels = ["orderbook.50", "publicTrade", "tickers"]
conn_budget = 100

[[markets.extra]]
channel = "orderbook.1000"
symbols = ["BTCUSDT"]

[[traded_channels]]
market = "linear"
symbol = "BTCUSDT"
channel = "orderbook.1000"
```

## 3. 무엇이 어디에 쓰이나

| 받은 것 | 파일 |
|---|---|
| 웹소켓 시세 프레임(토픽이 있는 것) | 그 연결의 열린 파일 `_open/.../<collector>-<session_ns>-c<conn_id>.qraw.zst.partial` |
| 구독 응답, pong, 오류 등 제어 프레임 | 수집기 세션 파일 `venue=<v>/_session/date=<d>/<collector>-<session_ns>.qraw.zst.partial` |
| 종목정보 응답(시장을 밝힌 것) | REST 열린 파일 `...-c0...` → 봉인 후 `channel=instruments/.../_all.qraw.zst` |
| 실패한 REST 응답(시장이 없는 것) | 세션 파일 |
| 노트 `clock_step`, `overflow`, `disk_floor` | 세션 파일 |

## 4. 대처

| 보이는 것 | 뜻 | 할 일 |
|---|---|---|
| 세션 파일의 `overflow` 노트 | 쓰기가 늦어 받기 큐가 찼다. 그 연결을 닫고 다시 연결했다. 넘치기 전 프레임은 모두 썼다 | 디스크 지연을 본다. 반복되면 `queue_bytes`보다 디스크를 먼저 의심한다 |
| `disk_floor` 노트, 상태 `disk_floor = true` | 여유 공간이 하한 아래. 전략용 토픽만 받는다 | 공간을 늘린다. 하한 + 여유를 넘으면 스스로 다시 구독한다 |
| `clock_step` 노트 | 한 연결 안에서 `recv_ns`가 뒤로 갔다(시계 조정) | 레코드는 받은 순서대로 남는다. NTP를 본다 |
| `md-lake seal` 종료 1, `UnknownSource` | 봉인된 시간에 모르는 열린 파일이 생겼다 | 사람이 판단한다(계약 2.5). 수집기는 지난 시간에 새 파일을 만들지 않는다(R2) |
| `md-lake seal`이 매번 같은 시간에서 1(`Check`) | U4에서 고치지 않은 경우: 전원 손실로 길이가 온전한 블록 안에 쓰레기가 남았을 수 있다 | 그 거래소 봉인이 멈춘다. 사람이 그 파일을 보고 결정한다 |
| 새 종목 상장 로그 | `symbols = "all"`이어도 세션 중에는 계획을 바꾸지 않는다 | 다음 시작부터 수집한다(재시작) |
| 시작 거절 "traded channel ... is not collected" | 전략용 토픽이 수집 계획에 없다(설정, 또는 거래 중이 아닌 종목) | 설정을 고친다 |
