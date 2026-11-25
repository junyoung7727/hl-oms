# L1 아카이버 운영 (md-archiver)

주인: 준영
검토일: 2026-11-19

`md-archiver`는 일별 봉인이 끝난 L0 날짜에서 L1 Parquet 표(`book_msg`, `trade`, `ticker`, `gap`, `instrument`)를 만든다. 계약은 quant-data-platform `docs/crypto/lake-contract.md` §3, 설계는 `.scratch/md-archiver/spec.md`(U7, 검토 R1–R11), 계약에 없는 곳의 결정은 [ADR-020](../adr/ADR-020-md-archiver-input-rules.md)이다.

이 저장소는 서비스를 설치하거나 켜지 않는다. `deploy/systemd/md-archiver@.service`와 `.timer`는 틀이다.

## 1. 명령

```text
md-archiver run --lake DIR --venue V [--small-channels publicTrade,tickers]
    [--rebuild YYYY-MM-DD..YYYY-MM-DD] [--retain-l1-days N]
    [--disk-floor-bytes B] [--threads N] [--now-ns N]
md-archiver version     # crate 버전과 parser_ver
```

| 인자 | 기본값 | 뜻 |
|---|---|---|
| `--small-channels` | `publicTrade,tickers` | 날짜마다 `day.parquet` 하나로 쓰는 채널. 나머지(`orderbook.*`)는 `hour=HH.parquet` |
| `--rebuild FROM..TO` | 없음 | 그 범위의 L0가 있는 날짜를, L1이 있어도 다시 만든다(`parser_ver`를 올린 뒤). 범위에 L0가 하나도 없으면 거절 |
| `--retain-l1-days N` | 없음(영구 보관) | 오늘(UTC) − N일보다 오래된 날짜의 `book_msg`·`trade`·`ticker`를 지운다(5절) |
| `--disk-floor-bytes` | 20,000,000,000 | 디스크 하한(설계 9.4). 날짜를 시작하기 전 여유 ≥ 하한 + 그 날짜의 L0 바이트, 새 파일마다 여유 ≥ 하한 |
| `--threads` | 4 | 동시에 재생하는 토픽 수. 결과 바이트는 같다(시험 `threads_do_not_change_the_bytes`). 메모리는 토픽 하루치 × 이 수 |
| `--now-ns` | 벽시계 | 보존 정리의 "오늘". 미래 값은 거절(`MD_ARCHIVER_TEST_ALLOW_FUTURE_NOW=1`은 시험 전용) |

| 종료 코드 | 뜻 | systemd |
|---|---|---|
| 0 | 끝남(만들 날짜가 없어도). 봉인 전이라 기다리는 날짜는 로그에만 남긴다 | — |
| 1 | 나중 실행이 고칠 수 있는 실패: 입출력, 읽기 무결성(프레이밍 오류, 레코드 수 불일치), 디스크 하한, 다른 실행이 잠금을 쥠, `--rebuild` 안의 날짜가 봉인 전 | 다음 타이머가 다시 한다 |
| 2 | 거절(아무것도 쓰지 않음): 모르는 거래소, 잘못된 인자, 더 새 계약(`contract_ver` > 1), `by_connection` 배치, 미래 `--now-ns` | 사람이 고친다 |

## 2. 무엇을 만드나

- **어느 날짜.** 일별 봉인이 끝났고(매니페스트, `.partial`·열린 파일 없음, 봉인 파일마다 매니페스트 줄·사이드카·크기 일치) gap 파일이 없는 날짜를 오래된 순으로. 전날이 L0는 있는데 봉인 전이면 그 날짜는 기다린다. 이틀을 놓쳐도 한 번에 둘 다 만든다.
- **어떻게.** 전날을 다시 재생해 00:00 호가 상태를 얻고(행은 안 씀) 그날을 재생한다. 파서는 수집기와 같은 md-bybit다.
- **어디에.** `crypto/l1/<table>/venue=V/market=M/channel=C/date=D/hour=HH.parquet`(작은 채널은 `day.parquet`), `crypto/l1/gap/venue=V/date=D/day.parquet`, `crypto/l1/instrument/venue=V/market=M/date=D/day.parquet`. zstd 9, 메타데이터 `contract_ver`, `parser_ver`, `table`(`book_msg`는 `first_update_id`·`last_update_id`도).
- **원자적 교체.** 모두 `.tmp`로 쓰고 fsync → 그 날짜의 옛 파일 중 새로 안 만든 것 삭제, 나머지 이름 바꾸기, 폴더 fsync → gap 파일 이름 바꾸기, fsync. `--rebuild`는 먼저 gap 파일을 지운다. 어디서 멈추든 gap 파일이 없으니 다음 실행이 그 날짜를 처음부터 다시 만든다.
- **결정성.** 같은 L0와 같은 `parser_ver`면 같은 바이트다(시험 `second_run_and_rebuild_are_identical`). L0는 읽기만 한다.

로그는 표준 오류에 날짜마다 JSON 한 줄이다: 파일 수, 표별 행 수, 종류별 gap 행 수, 걸린 초, 짝 핸드오버가 없는데 버린 체결 행 수.

## 3. 대처

| 보이는 것 | 뜻 | 할 일 |
|---|---|---|
| 로그 `"waiting": "... not day-sealed"` | 봉인이 그 날짜(또는 전날)를 아직 끝내지 않음 | 봉인 로그를 본다([md-collector.md](md-collector.md) 4절). 봉인되면 다음 실행이 만든다 |
| 종료 1, `below the floor` | 디스크 여유 부족. 새 날짜를 시작하지 않았거나, 쓰던 `.tmp`를 지웠다 | 공간을 늘리거나 `--retain-l1-days`. 수집기가 같은 디스크를 쓰므로 미루지 않는다 |
| 종료 1, `holds the lock` | 같은 거래소의 다른 실행 중 | 기다린다 |
| 종료 1, `ends inside a record` / `records, the manifest says` | 봉인 파일 읽기 무결성 실패 | `md-lake verify --json`과 감사 결과(qdp)를 본다. L0를 고치지 않는다 |
| 종료 2, `by_connection` | 그 날짜가 연결별 배치로 봉인됨 | U7은 읽지 않는다. 그 날짜는 L1 없이 둔다 |
| 종료 2, `contract_ver` | 더 새 계약의 파일 | 계약을 다시 반입하고 아카이버를 올린다 |
| 파서를 고쳤다 | `parser_ver`를 올렸다 | `--rebuild FROM..TO` |
| `unattributed_trade_drops` > 0 | 같은 체결 ID가 핸드오버가 아닌 곳(같은 연결, 재연결)에서 또 왔다 | 행은 한 번만 남았다. 잦으면 수집기 쪽을 본다 |

## 4. 보존 정리 (`--retain-l1-days N`)

날짜 D < (UTC 오늘) − N이고 L1이 있으면, 아래가 모두 맞을 때만 그 날짜의 `book_msg`·`trade`·`ticker` 폴더를 지운다.

- D의 L0가 일별 봉인 상태다.
- `crypto/audit/venue=V/date=D.json`이 온전한 JSON이고, `contract_ver` ≤ 1, `venue`·`date`가 맞고, `status = "clean"`이다(`not_audited`, `fail`, 없음은 남김).

`gap`과 `instrument` 일 파일은 남는다. gap 파일이 남아 있어야 다음 실행이 그 날짜를 다시 만들지 않는다. 되살리려면 `--rebuild`. `crypto/l0/`과 `crypto/audit/`은 건드리지 않는다.

## 5. 한계 (U7)

- 메모리는 토픽 하루치 행 × `--threads`다. 실측(Linux PC, 2026-11-19, 릴리스, 합성 호가 한 종목 320만 행, 변경분마다 4단계, `--threads 1`): 최대 RSS 2.57 GB, 그 날짜 만들기 18.8초(`tests/memory.rs`, 수동 실행). 행 하나에 약 800 B다. 처음 적은 "1 GB 안팎"(판단)은 틀렸다. Bybit 전 종목 하루의 시간은 아직 재지 않았다.
- D−1에 스냅샷 없이 D−2부터 살아 있던 호가 상태는 이어받지 못한다(ADR-020 2번).
- `by_connection` 배치, Binance·OKX·Hyperliquid는 읽지 않는다(U10, U11).
- `--rebuild` 도중 멈춘 경우(gap 파일을 먼저 지우는 순서)는 코드만 있고 시험이 없다. 보통 실행의 커밋 도중 실패는 시험한다(`crash_mid_write_rebuilds`).
