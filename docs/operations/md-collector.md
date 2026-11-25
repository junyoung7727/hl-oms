# 시세 수집기 운영 (md-collector, md-lake)

주인: 준영
검토일: 2026-11-21

수집기는 거래소마다 한 프로세스다. 받은 프레임을 모두 레이크의 1단계 파일(열린 파일·세션 파일)에 쓰고, `md-lake seal`이 매시 닫힌 시간을 봉인한다. 계약은 quant-data-platform `docs/crypto/lake-contract.md` §2, 설계는 `.scratch/md-collector/spec.md`(U5b)와 `.scratch/obs-push/spec.md`(U6: 관측·직접 알림, §5)다. 시장을 `conn_id`로 정하는 규칙은 [ADR-019](../adr/ADR-019-market-conn-slots.md)에 있다.

이 저장소는 서비스를 설치하거나 켜지 않는다. `deploy/observability/systemd/`의 파일은 틀이다(설치와 단위 설정: [observability.md](observability.md)). `md-collector@.service`는 죽으면 약 2초 뒤 다시 뜨고(`RestartSec=2`, 시작 횟수 제한 없음, 재시작 지연 늘림 없음), `trading.slice`에서 `OOMScoreAdjust=-900`, `MemorySwapMax=0`, `MemoryMin=512M`, `IOWeight=1000`으로 메모리를 지킨다. `md-lake-seal@.timer`는 매시 5분(UTC)에 돌고, 봉인 한 번은 `MemoryMax=1G`, 시작 제한 6시간(걸린 봉인만 끝냄)이다.

## 1. 명령

```text
md-collector run   --config <file>   # 수집. SIGTERM이면 파일을 닫고 0으로 끝난다
md-collector check --config <file>   # 설정만 검사(네트워크·파일 없음). 0 = 통과, 2 = 오류
md-lake seal      --venue V --lake DIR --first-hour YYYY-MM-DDTHH [--layout by_symbol|by_connection] [--telegram FILE]
md-lake seal-days --venue V --lake DIR --first-hour YYYY-MM-DDTHH --from YYYY-MM-DD
md-lake verify    --venue V --lake DIR --date YYYY-MM-DD --json
```

| 종료 코드 | md-collector | md-lake |
|---|---|---|
| 0 | SIGTERM·Ctrl-C로 정상 종료 | 끝남(잠긴 파일 때문에 기다리는 시간은 보고만 함) |
| 1 | 다시 시작하면 나을 수 있는 실패(종목정보 받기 실패, 쓰기 오류) | 봉인 오류(`Unroutable`, `UnknownSource`, `Check`, 입출력) |
| 2 | 설정 오류, 설정으로 계획을 만들 수 없음(전략용 토픽이 거래 중이 아님 등), 읽을 수 없는 토큰 파일. systemd는 다시 시작하지 않는다 | 사용법 오류, 등록되지 않은 거래소, 읽을 수 없는 `--telegram` 파일(모두 파일을 건드리지 않음), 미래의 `--now-ns` |
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
| `write.queue_bytes` / `write.total_queue_bytes` | 16 MiB / 512 MiB | 연결 자리 하나와 전체의 받기 큐 상한(바이트만 셈. 큐 항목·zstd 문맥·호가창 메모리는 따로) |
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
| `[telemetry] vm_url` / `vl_url` | 없음 | VictoriaMetrics·VictoriaLogs 주소. 표가 없거나 주소가 없으면 그 전송을 하지 않는다 |
| `telemetry.fast_ms` / `slow_ms` | 1000 / 10000 | 거래 종목(`traded_channels`)과 나머지의 요약 주기. `fast_ms ≥ 100`, `slow_ms ≥ fast_ms` |
| `telemetry.queue_lines` | 100000 | 전송 큐 줄 수(넘치면 오래된 것부터 버리고 `obs_push_dropped_total`로 셈) |
| `[alert] telegram` | 없음 | 토큰 파일 `{"token", "chat_id"}`. 일반 파일, 권한 0600, 저장소 밖. 읽을 수 없으면 시작 거절(2). 표가 없으면 직접 알림을 끈다(경고 로그) |
| `alert.state_file` | `$RUNTIME_DIRECTORY/alert-state.json` | 반복 규칙 상태(재시작해도 유지). 레이크 파일 시스템이 아닌 곳 |
| `alert.api_base` | 없음 | 시험 전용(루프백, `loopback_only`일 때만) |

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

## 5. 관측과 직접 알림 (U6)

수집기는 관측 스택(VictoriaMetrics·VictoriaLogs·vmalert)이 없어도 돈다. 전송은 따로 도는 스레드가 맡고, 큐가 차면 오래된 줄부터 버린다. 프레임 받기는 전송을 기다리지 않는다.

### 5.1 지표 (VictoriaMetrics `/api/v1/import`)

이름과 라벨은 설계 §5.4.5. `conn` 라벨은 연결 자리(`conn_index`)다. 다시 연결해도 바뀌지 않는다.

| 지표 | 라벨 | 주기 |
|---|---|---|
| `md_spread_bp`, `md_mid_px` | venue, market, symbol, channel | 거래 종목 `fast_ms`, 나머지 `slow_ms`. 호가창이 낡았거나 교차하면 보내지 않는다 |
| `md_depth_qty` | + side(bid/ask), levels(10/50) | 같음 |
| `md_recv_lag_ms` | + quantile(0.5/0.99) | 같음. 구간에 표본이 없으면 없음. 로그 버킷(×1.25), 오차 약 ±12% |
| `md_recv_lag_negative_total` | venue | `slow_ms`. 거래소 시각이 받은 시각보다 늦은 메시지 수(시계 차이) |
| `md_gaps_total` | venue, market, symbol, channel | 같음. 호가창 토픽만 |
| `md_reconnects_total` | venue, market, symbol, channel | 같음. 쓰던 연결이 끊긴 것만 센다(자정 전환·실패한 후보·재시도 제외) |
| `md_last_msg_age_s` | venue, market, symbol, channel | 같음. 첫 프레임 전에는 그 연결 자리가 시작한 뒤로 지난 시간 |
| `md_msgs`, `md_bytes` | venue, market, channel, conn | 같음(그 자리의 주기). 주기 안의 합 |
| `lake_l0_open_files`, `lake_l0_bytes_total`, `lake_l0_write_errors_total`, `lake_disk_free_bytes` | venue | `slow_ms`. `lake_l0_bytes_total`은 압축 전 페이로드 바이트다(디스크에 쓴 바이트가 아님) |
| `md_queue_bytes` | venue, conn | `slow_ms` |
| `md_collector_heartbeat_ts` | venue | `slow_ms`(우선 큐). 값은 쓰기 루프가 1초마다 정한다(아래) |
| `node_clock_offset_ms` | host | `slow_ms`. `chronyc -c tracking` 5번째 칸 × 1000. 양수 = 시스템 시계가 NTP보다 늦음 |
| `lake_seal_lag_hours` | venue | 60 s. 열린 파일이 남은 가장 오래된 닫힌 시간(7일 안)부터 지금 시간까지 |
| `obs_push_dropped_total`, `obs_push_queue_lines` | service, venue, endpoint | 10 s. 전송 큐가 버린 줄과 남은 줄 |

디스크 하한으로 멈춘 토픽과 연결은 다시 구독할 때까지 요약에서 빠진다. 수집기는 디스크가 며칠 남았는지 계산하지 않는다(vmalert, U20a).

### 5.2 심장박동과 systemd 감시

쓰기 루프는 파일 틱이 모두 성공한 뒤 1초에 한 번 박동한다. 프레임이 없어도 박동한다. 박동마다 `md_collector_heartbeat_ts`(유닉스 초)를 정하고 `$NOTIFY_SOCKET`에 `WATCHDOG=1`을 보낸다. 쓰기가 막히거나 쓰기 오류가 나면 박동이 멈추고, systemd(`WatchdogSec=30s`)가 수집기를 죽이고 다시 시작한다. 거래소가 조용하거나 네트워크가 끊긴 것은 박동을 멈추지 않는다. 그것은 `md_last_msg_age_s`와 공백 노트로 본다.

### 5.3 로그 (VictoriaLogs `/insert/jsonline`)

stderr의 JSON 줄과 같은 항목을 보낸다. `_stream_fields=service,host,venue`, `service=md-collector`, `level`과 필드. 필드 이름이 `_msg`·`venue` 같은 예약 이름이면 `fields.<이름>`으로 적는다.

### 5.4 직접 알림 `[MD P1]` (Telegram)

관측 스택을 거치지 않는다. PR #3의 전송 규칙을 따른다: 3,500 UTF-16 단위로 나눔, 50개 보관(넘치면 오래된 것 버리고 다음 전송 때 "알림 N개를 버림"), 400·413은 그 메시지를 버림, 그 밖의 응답·네트워크 오류는 보관했다 다시 보냄, 첫 경로가 막히면 `149.154.167.220`(인증서는 `api.telegram.org`로 확인), 경로마다 연결 3초. 토큰은 오류·로그 어디에도 쓰지 않는다.

송신기는 텔레그램 전송이 실패하기 시작한 시각을 상태에 둔다(`failing_since_ms`: 텔레그램이 마지막으로 답한 뒤 처음 실패한 시도의 시각. 전달이나 400·413 거절로 답하면 지운다. 보낼 것이 없으면 실패를 알 수 없다). alert-direct는 둘째 채널(`SecondChannel`, U18a1)을 받을 수 있다: 주면 모든 알림의 복사본이 자기 보관함(같은 50개 한도, 넘치면 자기 "알림 N개를 버림")과 스레드 `alert-second`로 나가고, 텔레그램 실패가 60초(`second_after_ms`)를 넘으면 둘째 채널에만 `Telegram 전송이 N초째 실패함. 대기 M개. 마지막 오류: ...`가 시작 1번, 5분마다, 해소 1번 나간다. 수집기와 `md-lake`는 둘째 채널을 주지 않는다(어느 서비스를 쓸지 정하지 않음, D3). 그래서 이들의 동작은 그대로다.

| 조건 | 보내는 때 | 문구 |
|---|---|---|
| L0 쓰기 실패(`l0_write`) | 시작 1번, 계속되면 5분마다, 해소 1번. 해소는 다음 세션이 실패 없이 `fsync_ms` + 5초를 지난 뒤(첫 fsync에서 실패하는 디스크가 재시작마다 해소·시작을 번갈아 보내지 않게). 상태 파일 덕분에 systemd가 다시 시작해도 재시작마다 보내지 않는다. 종료 전에 보내지 못하면 상태에서 지워 다음 프로세스가 다시 보낸다 | `L0 쓰기 실패: ENOSPC (...) — 수집기가 종료되고 systemd가 다시 시작함` |
| 쓰기 멈춤(`writer_stall`) | 박동이 15초 넘게 없을 때(오류 없이 쓰기가 막힘). 30초에 systemd가 죽이기 전에 나간다 | `L0 쓰기 멈춤: 쓰기 루프가 N초 동안 응답 없음 — ...` |
| 디스크 하한(`disk_floor`) | 시작 1번(피드를 멈추기 전), 5분마다, 해소 1번. 하한 위로 올랐는데 피드 재시작이 실패하면 문구가 바뀌어 계속 알린다 | `디스크 하한: 여유 N 바이트 < 하한 M 바이트 — 거래 종목만 수집 중` |
| 수집기 정지(`md-lake seal`) | 봉인 실행마다 1통(시간이 여러 개여도) | `수집기 정지: 수집기 파일이 하나도 없는 시간 N개 (UTC)` 와 시간 목록(24개까지) |

쓰기 실패로 끝날 때 수집기는 알림을 최대 20초 기다렸다 보내고 종료 코드 1로 끝난다. 보내지 못하면 stderr에 적는다. SIGTERM으로 끝날 때는 남은 알림을 최대 5초 보낸다.

알려진 한계: `md-lake seal`의 수집기 정지 알림은 그 실행에서 텔레그램에 닿지 않으면 다시 보내지 않는다(그 시간은 이미 봉인돼 다음 실행이 다시 찾지 않는다). stderr(journald)에만 남는다.

| 보이는 것 | 뜻 | 할 일 |
|---|---|---|
| `[MD P1] L0 쓰기 실패: ENOSPC` | 레이크 디스크가 꽉 찼다 | 공간을 늘린다. 수집기는 다시 시작하며 실패하면 5분마다 다시 알린다 |
| `[MD P1] 수집기 정지` | 그 시간에 수집기가 파일을 하나도 쓰지 않았다(죽었거나 시작하지 못함) | `systemctl status md-collector@<venue>`, 종료 코드를 본다 |
| `obs_push_dropped_total`이 오른다 | VictoriaMetrics/Logs가 느리거나 죽어서 지표·로그를 버렸다 | 관측 스택을 본다. 레이크와 직접 알림에는 영향이 없다 |
