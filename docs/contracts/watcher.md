# C-WATCH 독립 워처

상태: 1~2부 구현됨(U19a1, U19a2, 2026-11-23). 설계 근거: [ADR-031](../adr/ADR-031-watcher.md). 주장: V-WATCH-1, V-WATCH-2. 코드: `crates/watcher/src`, 시험 `crates/watcher/tests`, 적합성 시험 `crates/watcher-conformance/tests`, 검사 스크립트 `tools/ci/watcher_independent.sh`.

워처는 OMS와 코드를 나누지 않는 별도 프로그램이다. 저널을 자기 읽기로 읽고(이 문서와 [C-TAIL](tail.md), [ADR-011](../adr/ADR-011-file-format.md)만으로 썼다), 자기 키와 클라이언트로 Bybit를 조회해 OMS와 비교하고, 확정된 차이를 `admin.sock`에 `watcher_mismatch`로 보내고, `monitor.sock`에 하트비트를 보낸다. **U19a2까지는 저널 읽기, 저널 상태, 독립성 검사만 있다. 검사(W1~W4), Bybit 클라이언트, 하트비트는 아직 없어 워처는 아직 아무것도 감시하지 않는다.** 바이너리는 `watcher --version`에만 답하고, 그 밖의 호출은 "not built yet: the checks arrive in U19a5–U19a7"를 찍고 코드 2로 끝난다(네트워크도 파일도 건드리지 않는다).

| 부 | 내용 | 단위 |
|---|---|---|
| 1 | 독립성과 그 검사, 저널 읽기, 아는 이벤트, 소수, `Base32Pair` | U19a1(이 문서) |
| 2 | 저널 상태(주문, 열린 주문, 체결, 포지션, 보류, 기준일), 재생 속도 | U19a2(이 문서) |
| 3 | Bybit 서명과 클라이언트, 응답 분류, 요청 제한, 지표 | U19a3(아직 없음) |
| 4 | 설정, 자격 파일, 시작 확인, IP 재확인 | U19a4, U19a4b(아직 없음) |
| 5~9 | 검사 엔진과 W1, W2, OMS 연결과 실행, W4, W3과 예산 | U19a5~U19a9(아직 없음) |

## 1부. 독립성

### 규칙

| 항목 | 내용 |
|---|---|
| `watcher`의 정상 의존성 | 레지스트리 crate만(U19a1: `serde_json`, `crc32fast`). 작업 공간 crate는 없다 |
| `watcher`의 개발 의존성 | 작업 공간에서는 `http-fake`만(U19a3부터 쓴다) |
| `watcher`에 의존하는 패키지 | `watcher-conformance`만 |
| `watcher-conformance` | `publish = false`, 빈 `lib.rs`, 시험만. 개발 의존성 `watcher`, `oms-core`, `oms-journal`, `serde_json` |
| 소스 | `watcher` 디렉터리의 어떤 `.rs`도 `#[path]`, `include!` 계열, 속성 안의 `path =`로 패키지 밖의 파일을 가져오지 않는다. 심볼릭 링크가 없다. 매니페스트의 target(`[lib]`, `[[bin]]`, `[[test]]`, `[[bench]]`, `[[example]]`의 `path`, `build`)의 소스 파일도 패키지 안에 있다. `tail.md`는 시험이 파일로 읽는다 |

### 검사 `tools/ci/watcher_independent.sh [manifest]`

`cargo metadata --format-version 1 --locked --offline --filter-platform <host>`(호스트는 `rustc -vV`의 `host:` 줄)를 매니페스트의 디렉터리에서 실행하고 `jq`로 읽는다. 규칙 2·3·5는 선언된 의존성(`packages[].dependencies`, 모든 target)을, 규칙 4는 해석된 그래프(`resolve.nodes`, 호스트 플랫폼)를, 규칙 7은 `watcher` 매니페스트의 target(`packages[].targets[].src_path`)을 읽는다.

| 규칙 | 결과 줄(`watcher_independent: ` 뒤) |
|---|---|
| 1. 패키지 `watcher`가 없다 | `missing: package watcher` |
| 2. `watcher`가 이름이 `oms-*`·`md-*`인 crate를 선언한다(정상, `build`, `dev`; 어느 target이든) | `declared: watcher -> <dep> (<normal\|build\|dev>[, target <cfg>])` |
| 3. `watcher`가 개발 의존성 `http-fake`가 아닌 로컬 경로 crate를 선언한다 | `path: watcher -> <dep> (<kind>[, target <cfg>])` |
| 4. `watcher`에서 정상·빌드 간선으로 닿는 `oms-*`·`md-*` 또는 작업 공간 crate | `reachable: watcher -> <a> -> … -> <pkg>`(패키지마다 가장 짧은 경로 하나) |
| 5. `watcher-conformance`가 아닌 작업 공간 crate가 `watcher`에 의존한다 | `reverse: <pkg> -> watcher (<kind>)` |
| 6a. `#[path`, `include!`, `include_str!`, `include_bytes!`, 속성 안의 `path =`의 인자가 하나의 평범한 문자열이 아니거나, 절대 경로이거나, 그 파일의 디렉터리에서 풀어 패키지 밖이다(주석 안도 결과). `#`·`!`·`[` 사이와 매크로 이름과 `!` 사이의 공백도 찾는다(`# [path = …]`, `include_str !(…)`) | `source: <작업 공간 기준 파일>:<줄>` |
| 6b. 패키지 디렉터리 안의 심볼릭 링크(가리키는 곳과 상관없이) | `symlink: <작업 공간 기준 경로>` |
| 7. `watcher` 매니페스트의 target(라이브러리, 바이너리, 시험, 벤치, 예제, 빌드 스크립트) 중 소스 파일이 패키지 디렉터리 밖인 것(검토 1 S1) | `target: watcher <kind> <name> -> <작업 공간 기준 경로>` |

| 종료 코드 | 뜻 | 출력 |
|---|---|---|
| 0 | 결과 없음 | `watcher_independent: ok` |
| 1 | 결과 하나 이상 | 결과마다 한 줄. `ok`는 찍지 않는다 |
| 2 | 확인할 수 없다(매니페스트 없음, `rustc -vV`에 `host:` 없음, `cargo metadata` 실패(예: `--locked`에 맞지 않는 잠금), `jq` 실패, 예상과 다른 JSON) | `watcher_independent: cannot check: <이유>`와 그 도구의 오류 |

- CI: "Build" 바로 뒤의 단계 "Watcher depends on no OMS or market-data crate"가 실제 트리에서 `--locked`로 돈다. 1과 2 모두 실패다. 빌드 뒤에는 호스트 플랫폼의 패키지가 받아져 있어 `--offline`이 해석된다.
- 자체 시험: `tools/ci/selftest.sh`가 버려지는 저장소의 하위 디렉터리 `ws/`에 레지스트리 의존성이 없는 빈 crate 아홉 개(`watcher`, `watcher-conformance`, `oms-core`, `md-core`, `oms-journal`, `oms-api`, `obs-push`, `http-fake`, `helper`)와 저장소의 `rust-toolchain.toml` 사본으로 작업 공간을 만든다. 경우마다 기준 매니페스트로 되돌리고, 하나를 심고, 잠금 파일을 다시 만든 뒤(`cargo generate-lockfile --offline`), 종료 코드와 결과 줄을 함께 확인한다(`expect_finding`: 코드가 정확히 1이고 그 줄이 있고 `ok`가 없을 때만 통과). 22가지: 실제 모양(0, `ok`), 정상 `oms-core`, 빌드 `md-core`, 개발 `oms-journal`, `cfg(unix)` `oms-core`, `cfg(windows)` `oms-core`(호스트가 아니어도 보인다), `helper`를 거친 `oms-api`(`reachable:` 줄), `obs-push`(`path:`), `oms-api`가 `watcher`에 의존(`reverse:`), `#[path]`로 OMS 소스, 시험 파일의 `include_str!`, 패키지 안의 `#[path]`(0, `ok`), `cfg_attr` 안의 `path =`, OMS 소스로의 심볼릭 링크, 공백이 든 `# [path]`, 공백이 든 `include_str !`, OMS 소스를 가리키는 `[lib] path`(`target: watcher lib watcher -> oms-core/src/lib.rs`), `[[test]] path`(`target: watcher test t -> …`), `build`(`target: watcher custom-build build-script-lib -> …`), 패키지 안의 `[lib] path`(0, `ok`), 다시 만들지 않은 잠금(2, `cannot check:`), 없는 매니페스트(2). `cargo`가 없는 곳에서는 이 경우들을 건너뛴다고 찍는다.

### 알려진 한계

- 규칙 4는 호스트 플랫폼의 해석된 그래프만 걷는다. 다른 플랫폼의 의존성을 거쳐서만 닿는 패키지는 보지 못한다. `watcher`가 그것을 직접 선언하면 규칙 2가 잡는다.
- 규칙 6은 평범한 텍스트 검색이다. 여러 줄에 걸친 속성 안의 `path =`(`#`와 `[`가 다른 줄에 있는 경우도)은 보지 못한다. 주석 안의 일치도 결과다(주석을 다르게 쓴다). 한 줄에 둘 이상의 일치가 있으면 판단하지 않고 결과로 낸다.
- 이름 규칙(`oms-*`, `md-*`)에 기댄다. 다른 이름의 작업 공간 crate는 규칙 3과 4가 막는다.
- 규칙 4는 개발 간선을 걷지 않는다(스펙대로 정상·빌드만). 허용된 개발 의존성 `http-fake`가 뒷날 `oms-*`에 의존하게 되면 워처의 시험이 OMS 코드를 링크해도 스크립트는 초록이다. 규칙 3은 직접 선언만 본다(검토 1 N4).

## 1부. 저널 읽기

`watcher::journal::Tail::open(data_dir)`, `Tail::read(cap, max_records) -> Result<Vec<Record>, JournalError>`. `Record = {seq, at(µs), record_schema, events: [(이름, 필드)]}`. `input`은 해석하지 않는다.

| 항목 | 내용 |
|---|---|
| 파일 | `journal-<첫 번호 20자리>.oj`만. 읽기 전용으로만 연다. `LOCK`은 열지 않는다. 아무것도 만들거나 이름을 바꾸거나 쓰지 않는다 |
| 위치 | (세그먼트 첫 번호, 바이트 오프셋, 다음 번호). 호출마다 그 오프셋부터 16바이트 프레임 머리, 그다음 정확히 그 길이의 payload를 읽는다. `max_records`, 상한, 파일 끝에서 멈춘다. 세그먼트를 통째로 읽지 않고, 나머지를 매번 다시 읽지 않는다 |
| 상한 | 다음 번호가 `cap`보다 크면 읽지 않는다. `cap` 위의 프레임은 열어 보지도 않는다. 상한 너머의 쓰는 중인 프레임이나 쓰레기는 오류가 아니다 |
| 헤더 | 매직 `OMSJRNL\x01`, 길이 64 KiB 이하, CRC, `format == 1`, `record_schema ∈ {1, 2}`, `first_seq` = 기다리는 다음 번호, 모든 세그먼트의 `instance_id`가 같음. 프레임을 아직 읽지 않은 세그먼트는 호출마다 헤더를 다시 읽는다(헤더만 있는 세그먼트가 새 스키마로 다시 쓰이는 경우, C-TAIL) |
| 프레임 | 길이 16 MiB 이하, 번호 + payload의 CRC-32, 번호 = 기다리는 번호. payload는 JSON 객체이고 `seq`가 프레임의 번호와 같고 `at`이 앞 기록보다 작지 않다(C-JOURNAL 3단계), `events`는 키가 하나인 객체의 목록 |
| 회전 | 세그먼트 끝에서 다음 번호로 시작하는 세그먼트 `journal-<다음 번호>.oj`를 연다 |
| 첫 세그먼트 | 워처는 상태를 저장하지 않고 시작할 때마다 기록 1부터 읽는다. 가장 작은 세그먼트가 1에서 시작하지 않으면 `NoStart` |
| 호출당 기록 수 | `MAX_RECORDS` = 10,000(판단(모델 아님)). 긴 저널의 시작 재생은 여러 호출이고, U19a5부터의 검사 회차는 그 뒤에서 기다리지 않는다 |
| 오류 뒤 | 좋은 기록 뒤에 오류가 오면 이번 호출은 좋은 기록을 돌려주고 다음 호출이 오류를 낸다. 위치는 오류 앞에 머물고, 파일이 바뀔 때까지 같은 호출은 같은 오류를 낸다 |

### 오류와 그 부류

| 오류 | 언제 | 부류 |
|---|---|---|
| `Damaged {seq, why}` | 상한 이하에서: 짧은 헤더나 프레임, 매직·CRC·길이 상한 위반, 번호가 틀림, 세그먼트 없음, 헤더만 있는 세그먼트, 읽은 위치보다 짧아진 세그먼트, JSON이 아니거나 `seq`·`at`·`events`가 규칙에 맞지 않는 payload | 다시 읽음 |
| `NoStart` | 첫 세그먼트가 1에서 시작하지 않는다 | 다시 읽음 |
| `Io(kind)` | 디렉터리나 파일을 읽지 못했다(없는 데이터 디렉터리, 파일 기술자 한도 등) | 다시 읽음 |
| `UnknownEvent {seq, name}` | 필드 표에 없는 이벤트 이름 | 래치 |
| `Schema {segment, schema}` | 기록 스키마가 1, 2가 아니다 | 래치 |
| `Format {segment, format}` | 형식이 1이 아니다(스펙에 없는 판단: 낫지 않으므로 래치) | 래치 |
| `Instance {expected, found}` | 다른 인스턴스의 세그먼트 | 래치 |

- 다시 읽음: 위치를 움직이지 않고 다음 호출(다음 회차, 다음 시작 단계)이 같은 위치에서 다시 읽는다. 지나가는 조건은 재시작 없이 낫는다. 오래 가면 회차마다 같은 오류이고, 검사의 나이가 자라 OMS 게이트가 닫힌다(U19a5부터).
- 래치: 그 `Tail`은 파일을 다시 읽지 않고 같은 오류만 낸다. 파일을 고쳐도 마찬가지다. 프로세스가 다시 시작해야 한다(워처가 그 이벤트나 스키마를 배우거나, 다른 저널이다). U19a5의 "내구화 번호가 뒤로 감"도 이 부류에 든다.
- 지표 `watcher_journal_error`와 상태가 바뀔 때마다 한 줄의 기록은 U19a5가 만든다.

### 아는 이벤트, 소수, 식별자

- `watcher::events::KNOWN`: C-TAIL 필드 표(`tail.md`의 `event-fields` 사이)의 이벤트 51개. 시험 `events::the_known_events_are_the_rows_of_the_tail_table`이 시험 시각에 `tail.md`를 파일로 읽어 양쪽 방향으로 비교한다. OMS가 이벤트를 더하면 표가 바뀌고(필드 표 시험, V-TAIL-2), 이 시험이 워처가 배울 때까지 실패한다.
- `watcher::decimal::Dec(i128)`, 10^8 배: 선택적 `-`, 숫자, 선택적 `.`과 1~8자리. 지수, `+`, 9자리 이상, 빈 문자열, 앞뒤 공백은 거절한다. 덧셈·뺄셈·부호 바꿈은 정확하고 넘치면 오류다.
- `watcher::wire_id::{encode, decode}`: `Base32Pair`(`identifiers.md`, ADR-022 §7). 13자리 `-` 13자리, 소문자 `0-9a-v`만, 첫 자리 `f` 이하(64비트를 넘는 값 거절). 시험 벡터 인스턴스 `0x0123456789abcdef`, 주문 42 → `028q5cu4qnjff-000000000001a`.

### 알려진 한계(읽기)

- 이미 읽은 프레임이 그 뒤에 손상되면 그 프로세스는 보지 못한다(다시 읽지 않는다). 다음 시작의 재생이 본다.
- 재생 시간은 저널 길이에 비례한다. 측정은 아래 2부 "재생 시간"에 있다. 재생 중에는 하트비트가 없다(소유자 질문 9).
- 세그먼트 목록을 읽는 순간과 파일을 여는 순간 사이의 경합은 시험하지 않았다. 저널이 세그먼트를 지우는 것은 복구 수리가 내구 번호 위의 꼬리 세그먼트를 지울 때뿐이다(`crates/oms-journal/src/segment.rs`의 `r.remove`; 그 밖에는 U-04). 그 세그먼트의 프레임은 상한 위라 읽지 않았으므로, 그 사이의 변화는 새 세그먼트나 사라진 꼬리 세그먼트이고, 둘 다 없는 세그먼트(다시 읽음)로 나타난다(검토 1 N3).
- 한 번의 `read`는 기록 수(`max_records`, 최대 10,000)로만 묶이고 바이트로는 묶이지 않는다. 기록 하나가 16 MiB까지이므로 한 호출이 붙잡는 메모리의 이론상 상한은 매우 크다. 실제 OMS 기록은 작다. 워처의 메모리 한도를 정하는 뒷 부분이 호출당 바이트 예산을 더할 수 있다(검토 1 N2).

### 검증

`journal::frames_and_headers_follow_adr_011`, `journal::schema_1_and_2_are_read`, `journal::the_reader_never_returns_a_record_above_the_durable_cap`, `journal::the_reader_follows_segment_rotation_and_a_schema_change`, `journal::a_header_only_segment_rewritten_with_a_new_schema_is_read_again`, `journal::a_damaged_frame_at_or_below_the_cap_stops_without_moving`(짧은 프레임, 짧은 프레임 머리, CRC 필드·payload·번호 필드의 비트 뒤집기, 맞는 payload 아래의 틀린 프레임 번호, CRC가 맞는 틀린 번호, CRC가 맞는 프레임 안의 JSON이 아닌 기록·시각이 뒤로 가는 기록·`events`가 없는 기록(검토 1 S3), 없는 세그먼트, 줄어든 세그먼트), `journal::an_unknown_event_stops_the_reader`, `journal::io_damage_and_no_start_are_retried_and_the_other_errors_latch`, `journal::a_large_segment_is_read_frame_by_frame_in_bounded_calls`(기록 25,000개 세그먼트의 마지막 기록 손상을 셋째 호출 뒤에야 본다), `events::the_known_events_are_the_rows_of_the_tail_table`, `decimal::decimals_parse_exactly_and_refuse_other_forms`, `wire_id::base32_pair_matches_the_documented_vector`, `binary::version_prints_and_anything_else_exits_2`. 적합성: `conformance::the_watcher_base32_pair_equals_the_core_form`(가장자리 값과 고정 시드 10,000쌍, 잘못된 문자열 23개를 두 디코더가 똑같이 거절), `conformance::the_watcher_reads_while_the_oms_writer_holds_the_lock`(코어가 결정한 기록을 `oms-journal::Writer`가 512바이트 세그먼트로 쓰고 잠금을 쥔 동안 워처가 모두 같은 이벤트로 읽고, 디렉터리의 목록·길이·수정 시각이 그대로이며, 잠금이 여전히 쓰는 쪽에 있다). 독립성: `tools/ci/selftest.sh`의 워처 22건과 실제 트리의 `tools/ci/watcher_independent.sh`(주장 V-WATCH-1). 변이: M182(상한 + 1 기록), M183(프레임 CRC 생략), M184(모르는 이벤트 건너뜀), M221(`Io` 래치), M230(기록 검사 전에 위치를 옮김, 검토 1 S3).

## 2부. 저널 상태

`watcher::state::JournalState`. `apply(&Record)`가 기록 하나를 적용하고, `read_from(&mut Tail, cap, max_records)`가 읽기와 적용을 함께 하며 세그먼트 헤더의 `instance_id`를 옮겨 적는다. 모든 규칙은 [C-TAIL](tail.md) "이벤트가 바꾸는 상태" 표의 한 행이고, 필드는 그 필드 표의 것이다. 워처는 OMS 코드에 의존하지 않고 이 두 표만 따른다. 표의 확인·결과 불명·종결 표시, 거래소 변경·펀딩 키, 체결 바닥, 보류, 일 기준값의 행은 U19a2 검토 1(M1)에서 C-TAIL에 더했다. 스펙이 이 규칙들을 코어의 동작(`core.rs`)에서 가져왔으므로, 그 행들이 코어와 같다는 것은 아래 적합성 시험이 확인한다(보류와 일 기준값 포함, 검토 1 S1).

| 이벤트 | 바뀌는 것 |
|---|---|
| `order_accepted` | 주문 정보 `{strategy, instrument, side, qty, client_order_id, venue_oid: 없음, filled: 0, open, acked: 아님, in_doubt: 아님, terminal: 아님, fact_at}`, 열린 주문에 추가 |
| `order_acked` | `acked`, `in_doubt` 풀림, `venue_oid`가 null이 아니면 그 값 |
| `venue_oid_learned` | `venue_oid` |
| `order_in_doubt` | `in_doubt` |
| `venue_terminal` | `terminal`, `in_doubt` 풀림. 열린 주문에는 남는다(코어는 `order_closed`까지 `Closing`으로 둔다, C-TERM) |
| `fill_booked` | 그 주문의 `filled += qty`(닫힌 주문일 수 있다). 체결 키 → `{qty, side, instrument, venue_ts}`, 방향과 종목은 주문 정보에서. 포지션 += 부호 있는 수량(매수 +, 매도 −) |
| `external_fill_booked` | 체결 키 `fill.key` → `{fill.qty, fill.side, fill.instrument, fill.venue_ts}`. 포지션 += 부호 있는 `fill.qty` |
| `order_closed` | 열린 주문에서 뺀다. 주문 정보는 남긴다(늦은 체결의 방향과 종목) |
| `venue_position_changed` | 거래소 변경 키 `change.key` → `{instrument, qty, kind, venue_ts}`. 포지션 += `change.qty`(부호 있음) |
| `position_adjusted` | 포지션 += `delta` |
| `funding_booked` | 펀딩 키 `funding.key` → `{instrument, venue_ts}`. 수량은 바뀌지 않는다 |
| `purged` | `order_ids`의 주문 정보를 지운다(닫힌 주문만). `fill_floor = max(옛 값, 기록의 값)`. 그다음 체결·거래소 변경·펀딩 키 중 `venue_ts < fill_floor`인 것을 잊는다 |
| `booted` | `fill_floor = max(옛 값, 기록의 값)`. 키는 잊지 않는다(코어와 같다) |
| `hold_set` / `hold_cleared` | `holds.watcher_mismatch`, `holds.kill`. `hold_cleared`의 `venue_change`는 바꾸는 것이 없다 |
| `day_base_set` | `day_base = {day, value}` |
| 그 밖의 아는 이벤트 | 없음 |

- 시각: 주문마다 `fact_at` = 그 주문의 마지막 `order_accepted`, `order_acked`, `order_in_doubt`, `venue_terminal`, `order_closed`의 기록 시각. W1이 비교하는 것(열림, 확인, 결과 불명, 거래소의 종결)을 바꾸는 이벤트만 움직인다. 취소, 상태 기록, 재확인, 체결은 움직이지 않으므로, 이벤트를 계속 받는 주문이 W1의 차이를 젊게 유지하지 못한다. 종목마다 `last_change_at` = 그 종목의 마지막 `fill_booked`, `external_fill_booked`, `venue_position_changed`, `position_adjusted`의 기록 시각이다. 저널이 한 번도 바꾸지 않은 종목은 값이 없다(W2는 오래된 것으로 본다, U19a6). 펀딩은 이 시각을 움직이지 않는다.
- 그 밖에 `instance_id`(헤더), `last_seq`, `last_at`을 둔다. 포지션이 0이 된 종목은 0으로 남는다(`position(i)`는 없는 종목에 0).
- 메모리: 주문 정보와 키는 OMS 자신의 `purged`가 묶는다. 워처는 OMS가 잊는 것을 잊는다. 다른 한도는 없다(판단(모델 아님): OMS의 메모리 상태도 같은 크기다).

### 상태 오류

규칙에 맞지 않는 기록은 `JournalError::State {seq, why}`다: 저널이 받은 적 없는 주문의 체결(방향을 알 수 없다), 받은 적 없는 주문의 확인·종결·결과 불명, 같은 주문 번호의 두 번째 `order_accepted`, 열린 주문이나 모르는 주문의 `purged`, 없거나 문서의 형식이 아닌 필드(소수의 지수 표기, `buy`/`sell`이 아닌 방향), 모르는 거래소 변경 종류나 보류, 포지션의 넘침, 마지막 기록 + 1이 아닌 번호. 이 오류는 **래치**다. 상태가 그 기록의 일부를 이미 담고 있을 수 있으므로, 그 뒤의 모든 `apply`와 `read_from`이 같은 오류를 내고 프로세스가 다시 시작해야 한다. 코어는 이런 기록을 쓰지 않는다(같은 경우에 코어의 `apply`도 오류다). 그러니 이 오류는 저널이 C-TAIL과 다르다는 뜻이다(1부의 래치 부류).

### 재생 시간

워처는 상태를 저장하지 않고 시작마다 기록 1부터 다시 읽는다(소유자 질문 9). 측정: `cargo test --release -p watcher --test replay -- --ignored --nocapture`(시험 `replay::replay_speed_of_100000_records`, 단언하지 않고 보고만 한다). 워처의 시험 쓰기로 만든 기록 10만 건(21.4 MB, 하트비트 5, 주문 하나의 수명 3, 참조 가격 1, 증거금 보고 1의 비율, 판단(모델 아님))을 `Tail`과 `JournalState`로 다 읽는 데 3회 133~134 ms였다. **초당 약 74만 건, 기록 100만 건당 약 1.3초**다(i9-14900K, release, 2026-11-23, 방금 쓴 파일이라 페이지 캐시에 있다. 디스크에서 처음 읽는 경우는 재지 않았다). 이 속도라면 게이트가 일반 주문을 거절하기 시작하는 30초는 약 2,200만 건이다(판단(모델 아님): 같은 기록 구성과 캐시를 가정한 단순 비례). OMS는 하트비트만으로 하루 약 17,000건을 쓴다.

### 알려진 한계(상태)

- 파킹된 체결(`fill_parked`)은 포지션에도 체결에도 넣지 않는다(소유자 질문 4). 코어도 풀릴 때까지 넣지 않는다.
- 상태 오류 뒤에는 그 프로세스가 저널을 더 적용하지 않는다(래치). 저널이 고쳐져도 다시 시작해야 한다.
- 재생 시간은 캐시에 있는 파일로만 쟀다.
- 상태 오류가 난 뒤에도 필드는 공개된 채 그 기록의 일부를 담고 있을 수 있다(예: 포지션이 넘치기 전에 올라간 `filled`, `purged`가 이미 지운 주문 몇 개). 집합을 쓰는 쪽(엔진, U19a5)은 쓸 때마다 먼저 `latched()`를 봐야 한다. 이 부분에는 아직 쓰는 쪽이 없다(검토 1 N2).
- 닫힌 주문의 `terminal`은 "거래소가 끝냈다"는 뜻으로만 쓸 수 있다. `order_closed`는 그것을 바꾸지 않고, `venue_terminal` 없이 닫힌 주문은 `terminal`이 아니다(`state::the_sets_follow_the_field_table`). 코어와는 열린 주문의 것만 비교한다(검토 1 N3).
- `booted`가 살아 있는 키보다 높은 바닥을 쓰는 저널은 코어가 쓰지 않는다. 첫 부팅의 바닥은 키가 생기기 전에 쓰이고, 그 뒤의 부팅은 지금의 바닥을 그대로 쓴다(`crates/oms-core/src/decide.rs`의 `boot`). 그래서 "`booted`는 키를 잊지 않는다"는 규칙 시험(`state::the_fill_floor_never_goes_down`)과 변이 M231로만 본다(검토 1 S2). 같은 이유로 null 확인이 아는 거래소 주문 번호를 남기는 규칙(코어는 주문을 한 번만 확인한다)도 규칙 시험으로만 본다.

### 검증(2부)

`state::the_sets_follow_the_field_table`, `state::a_fill_after_close_uses_the_kept_order_info`, `state::purged_forgets_closed_orders_and_old_keys`, `state::purging_an_unknown_order_stops_the_state`, `state::the_fill_floor_never_goes_down`, `state::the_fact_time_moves_only_on_the_compared_order_events`, `state::funding_never_changes_a_position`, `state::venue_terminal_marks_the_order_terminal_and_moves_its_fact_time`, `state::an_instrument_the_journal_never_changed_has_no_change_time`, `state::records_that_break_the_rules_stop_the_state`(`crates/watcher/tests/state.rs`, 기록을 메모리에서 만든다). 적합성: `conformance::the_watcher_sets_equal_the_core_on_journals_with_every_event_kind`. 핵심 시험 도구(`H`)로 코어를 움직이는 시나리오 열 개(일반 체결과 종결, 부분 체결과 취소, 닫힌 뒤의 체결, 외부·파킹·풀린 체결, 결과 불명·재확인·부재·누락 체결, 거래소 변경 네 종류·펀딩·미분류·스냅숏 조정, 운영자·저장 공간·참조 가격·불일치·전략 연결·두 번째 설정, 킬과 재무장·워처 불일치·킬 마커, 증거금·기준일·감소 전용, 모니터링 게이트, 정리와 재시작)가 C-TAIL 표의 이벤트 51개를 모두 쓴다(시험이 `tail.md`를 읽어 확인하고 빠진 이름을 적는다). 기록을 `oms-journal::Writer`가 4 KiB 세그먼트로 쓰고, 워처가 기록마다 상한을 그 기록으로 두고 읽어, 같은 기록을 재생한 코어와 비교한다: 열린 주문 집합, 열린 주문마다 종목·방향·수량·전략·클라이언트 ID·거래소 주문 번호·누적 체결·결과 불명·거래소 종결, 종목마다 포지션(0은 없는 것으로), 체결·거래소 변경·펀딩 키와 그 거래소 시각(스펙은 바닥 이상만 요구하지만, 양쪽이 같은 기록에서 잊으므로 집합 전체를 비교한다), 체결 바닥, 킬 보류(`core.kill`), 워처 불일치 보류(`core.watcher_mismatch`), 일 기준값(`core.margin.day_base`의 `day`, `value`; 검토 1 S1). 변이: M185(종결이 주문 정보를 지움), M186(거래소 변경을 포지션에서 뺌), M187(바닥과 같은 시각의 키를 잊음), M210(`purged`·`booted`가 바닥을 낮춤), M231(`booted`가 바닥 아래의 키를 잊음, 검토 1 S2).
