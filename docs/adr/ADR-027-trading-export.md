# ADR-027 레이크 거래 테이블 내보내기: 내구화된 저널에서 완결된 날짜만

상태: 채택(구현, U15). 레이크 계약(qdp `quant_data/crypto/contract`, 이 저장소의 `contracts/lake/`) 4절의 거래 테이블을 쓰는 쪽이다.

## 1. 문제와 발생 가능한 실패

- 연구자는 우리 주문·체결을 그때의 시장 데이터(L1 `book_msg`)와 이어 보려 한다(레이크 계약 3.7 주문-시세 결합). 주문과 체결은 저널에만 있고, 저널은 OMS 크레이트가 있어야 읽히는 JSON 프레임이다.
- 내구화되지 않은 기록을 내보내면, 장애 뒤 복구가 그 꼬리를 잘라 같은 번호에 다른 기록을 쓸 때 레이크와 저널이 어긋난다.
- 하루의 일부만 최종 경로에 쓰면 읽는 쪽은 그날이 완결됐는지 모른다.
- 다시 돌릴 때마다 같은 파일을 새로 쓰면 바뀐 것이 없는데도 파일 시각이 바뀌고, 무엇이 바뀌었는지 알 수 없다.
- 계약의 열을 저널이 채우지 못하면(필수 열인데 저널에 없는 값) 내보낼 수 없다.

## 2. 일반적으로 만족해야 하는 성질

- 저널을 잠그지도 쓰지도 않는다.
- 내구화된 기록만 내보낸다.
- 최종 경로의 날짜 파일은 그날의 모든 기록을 담는다.
- 같은 저널은 같은 바이트를 만든다. 바뀐 파일만 다시 쓴다.
- 행의 모양은 계약의 테이블 파일 하나가 정한다(열 이름, 순서, 형식, null 허용을 여기서 다시 적지 않는다).

## 3. 제약

| 사실 | 출처 |
|---|---|
| 잠금 없는 꼬리 읽기와 내구화 상한 | C-TAIL, ADR-025 |
| OMS는 `/api/status`에 `journal.durable_seq`, `node.wire_id_form`, `core.instance_id`를 낸다 | ADR-026 §3 |
| 다른 OMS의 답을 쓰면 내구화되지 않은 기록을 확정으로 낼 수 있다 | ADR-026 §7(검토 2026-11-20) |
| 저널 세그먼트는 기본 64 MiB | `crates/oms-node/src/config.rs` `segment_max_bytes` |
| 거래 테이블의 열, 경로 `crypto/trading/<table>/date=<date>/day.parquet`, zstd 9, 메타데이터 `contract_ver`·`export_ver`·`table`, 날짜 = `at_us`의 UTC 날짜 | 레이크 계약 4 |
| 저널의 체결(`fill_booked`)에는 거래소의 maker 표시와 우리 수신 시각이 없다. 거래소 시각은 µs | C-TAIL 필드 표 |
| 저널 기록에는 거래소·시장 이름이 없다(종목 `instrument`만) | C-TAIL 필드 표 |
| 기록 시각은 뒤로 가지 않는다(`wall_us().max(last_at)`) | `crates/oms-node/src/pipeline.rs` `core_loop` |
| L1 Parquet 쓰기 코드(시간·호스트 없음, 셀 검사) | `crates/md-archiver/src/parquet_out.rs`, ADR-020 |

## 4. 필수 조건과 선택 조건

필수: 2의 다섯 성질, 계약 4.1–4.3의 세 테이블, 계약 golden 행과 같은 결과.
선택: 열린 날짜(오늘)의 중간 내보내기 — 하지 않음(5의 D).

## 5. 대안

| 대안 | 설명 |
|---|---|
| A. 닫힌 세그먼트만 읽음 | 상태 조회 없음 |
| B. 같은 인스턴스의 상태 답의 `durable_seq`까지 읽음 | 이 결정 |
| C. 프로젝터의 커서처럼 이어 읽기 | 상태 파일 |
| D. 열린 날짜도 쓰고 매번 바꿈 | 오늘 데이터가 바로 보임 |

## 6. 비교

| 기준 | A | B | C | D |
|---|---|---|---|---|
| 지연 | 세그먼트 64 MiB가 찰 때까지(며칠, 판단) | 실행 시점까지 | 실행 시점까지 | 실행 시점까지 |
| OMS가 꺼져 있으면 | 읽힘 | 쓰지 않음(종료 코드 3) | 쓰지 않음 | — |
| 상태 | 없음 | 없음(매번 1번부터) | 커서 파일 | — |
| 읽는 쪽이 보는 것 | 완결된 날 | 완결된 날 | 완결된 날 | 미완결 날이 최종 경로에 |

## 7. 선택과 이유

B. 크레이트 `crates/oms-export`(라이브러리 + `oms-export --config <file>`). 한 번 실행하고 끝난다(설치와 주기는 U20).

| 단계 | 입력 | 처리 | 출력 | 실패하면 |
|---|---|---|---|---|
| E1 상한 | `/api/status` | `core.instance_id`가 저널의 `INSTANCE`와 같은 답만 쓴다(`oms_projector::probe`). `node.venue`가 설정의 `venue`이거나 `<venue>:mainnet`이어야 한다 | `durable_seq`, `wire_id_form` | 답 없음·다른 인스턴스·`instance_id`나 `node.venue` 없음 → 종료 코드 3. 다른 거래소·testnet·sim → 종료 코드 1. 둘 다 아무것도 쓰지 않음 |
| E1b 잠금 | `<lake>/crypto/trading/.oms-export.lock` | `flock`(기다리지 않음), 실행 끝까지 | — | 다른 실행이 잡고 있음 → 종료 코드 1 |
| E2 읽기 | 저널(잠금 없음) | 1번 기록부터 `Tail::read(durable_seq)`, 기록 스키마 1·2. 한 번에 100만 기록(세그먼트 하나를 한 번에 읽음) | 기록 | 손상·모르는 스키마 → 종료 코드 1. 끝까지 읽었는데 `durable_seq`보다 짧음 → 종료 코드 1. 그 전에 완결된 날짜는 이미 썼을 수 있다(완전한 파일) |
| E3 행 | 기록 | `rows::Mapper`(아래). 열린 날짜 하나의 행만 메모리에 둔다 | 날짜별 세 테이블의 행 | 계약에 안 맞는 행 → 종료 코드 1 |
| E4 쓰기 | 완결된 날짜의 행(뒤 날짜의 기록이 나오는 순간) | 그 날짜의 세 테이블 임시 파일을 모두 만들고(fsync) 나서, 기존 파일과 바이트가 다른 것만 rename → 디렉터리 fsync, 같으면 임시 파일 삭제 | `day.parquet` | 행 거절·쓰기 실패 → 종료 코드 1. 임시 파일을 다 만들기 전이면 그 날짜는 그대로 |
| E5 요약 | — | JSON 한 줄: `durable_seq`, `read_to_seq`, `open_date`, `written`, `unchanged`, `rows` | 표준 출력 | — |

결정과 이유:

- **완결된 날짜:** 마지막으로 읽은 기록의 UTC 날짜(`open_date`)보다 앞선 날짜만 쓴다. 기록 시각은 벽시계가 뒤로 가도 뒤로 가지 않으므로(`crates/oms-node/src/pipeline.rs` `core_loop`) 그 앞 날짜에는 기록이 더 오지 않는다.
- **바뀐 파일만:** 쓰기 코드에 시간·호스트가 없어 같은 행은 같은 바이트다. `export_ver`가 오르면 모든 파일의 메타데이터가 바뀌므로 저절로 다시 쓴다.
- **매번 1번 기록부터:** 뒤 날짜의 체결은 앞 날짜의 `order_accepted`에 있는 종목·방향이 필요하다. 저널은 작다(판단). 재검토 조건 참고.
- **거래소·시장 이름은 설정에서:** OMS 하나는 거래소 하나에서 거래한다. `symbol`은 저널의 `instrument`다.
- **행 규칙**(`crates/oms-export/src/rows.rs`):
  - `orders`: `order_accepted`마다 한 행. `order_id`는 u64의 10진 문자열. `tif` gtc → `GTC`, ioc → `IOC`, alo → `PostOnly`. `px`는 늘 값이 있다(OMS에 시장가 주문이 없음). `wire_id` = `WireIdForm::encode(instance_id, order_id)`, 형식은 상태 답에서(없으면 null).
  - `fills`: `fill_booked`마다 한 행. 종목과 방향은 그 주문의 `order_accepted`에서. `exec_id`: Bybit 게이트웨이의 체결 키는 `<symbol>:<execId>`(C-GW G-8)이므로 bybit면 그 접두어를 뗀 `execId`, 다른 모양이면 종료 코드 1. 다른 거래소는 키 그대로(검토 2026-11-20). `venue_ts_ns` = `venue_ts` × 1000. `is_maker`, `recv_ns`는 저널 체결의 같은 이름 필드(U16a부터, 게이트웨이가 아는 경우), 없으면 null. 우리 주문이 아닌 체결(`external_fill_booked`), 대기 체결, 거래소 위치 변경은 `events`에만 있다.
  - `events`: 이벤트마다 한 행. `kind` = `Event::name()`, `order_id` = `Event::order_id()`. `symbol`은 이벤트의 종목 필드(`rows::INSTRUMENT_PATHS`, 시험이 C-TAIL 표와 맞춤), 없으면 그 주문의 종목. 목록 필드(`report.positions[].instrument`)는 종목 하나가 아니므로 쓰지 않는다. `venue`는 `symbol`이 있을 때만. `payload`는 이벤트의 필드 객체를 키 정렬, 공백 없이.
  - 소수 문자열과 `payload`는 저널에 있는 JSON 그대로 쓴다(형식 있는 값으로 다시 쓰지 않음).
- **계약 변경(qdp U15a, PR #32):** `fills.is_maker`, `fills.recv_ns`를 null 허용으로 바꿨다. 계약 golden 거래 행은 이 단위의 시험 저널을 내보낸 것으로 바꿨다. 실제 L0 파일이 아직 없어 초안 1판 규칙(계약 6)이므로 `contract_ver`는 1 그대로다.
- **쓰기 코드 하나:** `md-archiver`의 쓰기 코드를 계약 테이블 아무것이나 받는 `SchemaWriter`로 떼었다. L1의 `FileWriter`는 그것을 감싸고 동작은 같다.

## 8. 포기한 장점, 남은 위험, 가정

- 포기: 오늘 데이터. 하루가 끝나고 다음 날의 기록이 내구화된 뒤 실행해야 그날이 나온다.
- 포기: OMS가 꺼져 있을 때의 내보내기. 마지막 날은 OMS를 한 번 켜야(복구가 내구화를 확정) 나온다.
- 위험: 실행 시간이 저널 길이에 비례해 는다.
- 위험: 한 날짜의 세 파일 교체는 rename 세 번이다. 그 사이에 멈추면 그 날짜의 테이블 일부만 새 판이다(모두 완전한 파일, 다음 실행이 나머지를 바꾼다).
- 위험: 행 거절 뒤에도 앞 테이블의 파일을 남기지 않는 동작(E4)은 시험이 없다. 유효한 저널로는 거절되는 행을 만들 수 없다.
- 가정: OMS의 `node.venue`는 `<venue>` 또는 `<venue>:<network>`다(지금 `sim`, `hyperliquid:testnet|mainnet`; Bybit 게이트웨이는 U16에서 같은 모양으로 정한다).
- 위험: `payload`의 키 정렬은 `serde_json`의 기본 맵이 이미 정렬하므로 `rows::sorted`를 빼도 시험이 실패하지 않는다. 의존성이 `preserve_order`를 켜는 경우를 위한 보호다.
- 위험: golden 행은 이 코드가 만든 것을 사람이 계약 문장과 대조해 확정했다. 계약과 이 코드가 같은 실수를 하면 시험이 잡지 못한다.
- 가정: 같은 기록의 행은 같은 `at_us`이고 날짜도 같다.

## 9. 근거

- 시험: `crates/oms-export/tests/export.rs` 17개. 시험 저널 → 내보내기 → Parquet을 다시 읽은 행 = 계약 golden 행(세 테이블). 메타데이터와 열 형식, 두 번째 실행은 파일을 바꾸지 않음, 바뀐 파일은 교체, 열린 날짜를 쓰지 않음, 상한 너머를 읽지 않음, 다른 인스턴스·`instance_id` 없음·무응답이면 아무것도 쓰지 않음, 앞 날짜 주문의 체결, 스키마 1 저널, 잠금을 잡은 저널을 바꾸지 않음, C-TAIL의 종목 필드 목록, 설정 거절, 바이너리 종료 코드 3, Bybit 체결 키의 접두어와 다른 모양, 다른 거래소·testnet·sim·`node.venue` 없음 거절, 레이크 잠금, 내구화 번호보다 짧은 저널.
- 이 단위는 코드를 시험보다 먼저 썼다. 대신 결함 8가지를 일부러 넣어 시험이 모두 잡는 것을 확인했다(2026-11-20, Linux PC): `PostOnly` 이름, 완결 날짜 경계, 인스턴스 확인, 같은 바이트도 다시 씀, 체결 방향, 내구화 상한, 종목 없는 이벤트의 `venue`, ns 변환. 검토 반영분의 결함 5가지도 각각 그 시험이 잡았다: 잠금 없음, 거래소 확인 없음, testnet 통과, 짧은 저널 통과, 체결 키 그대로.
- 독립 검토(2026-11-20): 꼭 고칠 것 2개(Bybit 체결 키, 겹친 실행의 임시 파일)와 고칠 것 4개(거래소 확인, 세그먼트 반복 읽기와 메모리, 짧은 저널, 날짜 단위 교체)를 반영했다. 체결 키 때문에 계약 golden의 events payload가 바뀌었다(qdp PR #33).
- 설계 TN(암호화폐 거래 통합 설계 v1.0), `pr_plan.json` U15. 저장소 밖 문서다.
- 운영 문서: [docs/operations/oms-export.md](../operations/oms-export.md).

## 10. 재검토 조건

- 한 번 실행이 60초를 넘을 때(판단): 날짜별 주문 정보를 저장해 이어 읽기.
- ~~U16이 `FillIn`·`FillBooked`에 maker 표시와 수신 시각을 넣을 때~~: U16a에서 넣었고 `fills`의 두 열을 채운다. `export_ver`는 `oms-export-2`.
- 오늘 데이터가 필요해질 때: 최종 경로와 다른 곳에 열린 날짜를 쓰기.
