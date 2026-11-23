# C-WATCH 독립 워처

상태: 1~3부 구현됨(U19a1, U19a2, U19a3, 2026-11-23), 4부 구현됨(설정과 자격 파일 U19a4, 시작 확인과 IP 재확인 U19a4b, 2026-11-23), 5부 구현됨(검사 엔진과 W1, U19a5, 2026-11-23), 6부 구현됨(W2 포지션, U19a6, 2026-11-23). 설계 근거: [ADR-031](../adr/ADR-031-watcher.md). 주장: V-WATCH-1, V-WATCH-2, V-WATCH-3, V-WATCH-4, V-WATCH-5, V-WATCH-6. 코드: `crates/watcher/src`, 시험 `crates/watcher/tests`, 적합성 시험 `crates/watcher-conformance/tests`, 검사 스크립트 `tools/ci/watcher_independent.sh`.

워처는 OMS와 코드를 나누지 않는 별도 프로그램이다. 저널을 자기 읽기로 읽고(이 문서와 [C-TAIL](tail.md), [ADR-011](../adr/ADR-011-file-format.md)만으로 썼다), 자기 키와 클라이언트로 Bybit를 조회해 OMS와 비교하고, 확정된 차이를 `admin.sock`에 `watcher_mismatch`로 보내고, `monitor.sock`에 하트비트를 보낸다. **U19a6까지는 저널 읽기, 저널 상태, 독립성 검사, Bybit 서명·클라이언트와 지표 내보내기, 설정 파일과 자격 파일 읽기, 시작 확인과 IP 재확인의 sans-IO 단계, sans-IO 검사 엔진과 W1(열린 주문), 그리고 W2(포지션)만 있다. W3·W4와 이 부분들을 부르는 실행 코드(U19a7)는 아직 없어 워처는 아직 아무것도 감시하지 않는다(엔진의 하트비트와 불일치 행동을 보낼 곳이 없다).** 바이너리는 `watcher --version`에만 답하고, 그 밖의 호출은 "not built yet: the checks arrive in U19a5–U19a7"를 찍고 코드 2로 끝난다(네트워크도 파일도 건드리지 않는다).

| 부 | 내용 | 단위 |
|---|---|---|
| 1 | 독립성과 그 검사, 저널 읽기, 아는 이벤트, 소수, `Base32Pair` | U19a1(이 문서) |
| 2 | 저널 상태(주문, 열린 주문, 체결, 포지션, 보류, 기준일), 재생 속도 | U19a2(이 문서) |
| 3 | Bybit 서명과 클라이언트, 응답 분류, 요청 제한, 지표 | U19a3(이 문서) |
| 4 | 설정과 자격 파일, 시작 확인과 IP 재확인(이 문서) | U19a4, U19a4b |
| 5 | 검사 엔진(일정, 상태 맞추기, 라운드의 저널 읽기, 기한, 두 라운드 규칙)과 W1 열린 주문(이 문서) | U19a5 |
| 6 | W2 포지션(거래소 쪽, 기호별 차이, 두 변경 시각과 미룸, 거래소 관찰 시각 `seen_at`)(이 문서) | U19a6 |
| 7~9 | OMS 연결과 실행, W4, W3과 예산 | U19a7~U19a9(아직 없음) |

## 1부. 독립성

### 규칙

| 항목 | 내용 |
|---|---|
| `watcher`의 정상 의존성 | 레지스트리 crate만(U19a1: `serde_json`, `crc32fast`; U19a3: `hmac`, `sha2`, `hex`, `reqwest`(rustls, 기본 기능 없음), `tokio`). 작업 공간 crate는 없다 |
| `watcher`의 개발 의존성 | 작업 공간에서는 `http-fake`만(U19a3부터 쓴다). 레지스트리에서는 `tokio`(시험 매크로) |
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

## 3부. Bybit 서명과 클라이언트, 지표

`watcher::sign`, `watcher::rest`, `watcher::metrics`, `watcher::clock`. OMS의 Bybit 코드(`crates/oms-node/src/venue/bybit`)는 API를 알려고 읽기만 했고 쓰지 않는다. 시험은 loopback 가짜(`http-fake`)와 수동 시계로만 한다. **실제 Bybit에는 한 번도 보내지 않았다**(키가 아직 없다). 이 부분은 클라이언트만 만든다. 그것을 부르는 시작 확인(U19a4b)과 검사(U19a5~)는 아직 없다.

### 서명

- GET만 서명한다(POST는 U19b의 일괄 취소가 쓸 때 더한다). `X-BAPI-SIGN = hex(HMAC_SHA256(secret, timestamp + api_key + recv_window + query))`. `query`는 URL에 보내는 문자열과 바이트까지 같다.
- 질의 문자열: `k=v`를 `&`로 잇고 키로 정렬한다. 값은 그대로 보낸다(거래소의 커서는 이미 퍼센트 인코딩되어 온다). 키나 값에 `[A-Za-z0-9-_.~%:,]` 밖의 글자가 있으면 **보내기 전에 거절**한다(`not_sent`). 인코딩해서 보내지 않는다. 이 거절은 요청이 아니므로 `watcher_requests_total`에 세지 않고 `watcher_request_errors_total{class="not_sent"}`에만 센다.
- 머리글 다섯: `X-BAPI-API-KEY`, `X-BAPI-TIMESTAMP`(보내는 순간의 주입된 시계, 요청 제한에서 기다린 뒤에 읽는다), `X-BAPI-RECV-WINDOW`(`recv_window_ms`), `X-BAPI-SIGN`, `X-BAPI-SIGN-TYPE: 2`.
- 연결: 지역 주소는 IPv4만(IP 반향이 보는 주소가 Bybit가 보는 주소가 되도록), 연결 제한 3초, 요청 제한 `request_timeout_ms`, 리다이렉트를 따르지 않고(서명된 요청을 다시 보내게 된다), 프록시를 쓰지 않는다.
- 자격(`rest::Credentials`)의 `Debug`는 비밀을 찍지 않는다. 파일에서 읽는 것은 4부(`watcher::creds`)다.

### 응답 분류

`rest::classify(status, body)`와 `Client::get(path, params) -> Result<Value, RestError>`. retCode 0이면 `result`(없으면 `null`)를 돌려준다.

| 응답 | 부류(`class`) | 효과 |
|---|---|---|
| 연결 실패, 아무것도 보내지 않음. 보낼 수 없는 질의 값 | `not_sent` | 그 라운드 실패 |
| 시간 초과, HTTP 5xx와 그 밖의 2xx 아닌 상태, 읽을 수 없는 본문, retCode 없음(정수 아님 포함), retCode 10016 | `no_answer` | 그 라운드 실패 |
| HTTP 401. retCode 10003, 10004, 10005, 10007, 10010, 33004 | `auth` | **다시 보내지 않는다**: 그 뒤 모든 Bybit 요청이 프로세스가 다시 시작할 때까지 멈춘다. `watcher_auth_rejected = 1`, 코드를 적은 오류 줄 하나 |
| HTTP 403 | `ip_banned` | 모든 Bybit 요청과 IP 반향이 600초 멈춘다 |
| HTTP 429, retCode 10006 | `rate_limited` | 그 라운드 실패 |
| retCode 10002 | `clock` | 그 라운드 실패 |
| 그 밖의 0 아닌 retCode | `refused` | 그 라운드 실패 |
| retCode 0인데 행을 읽을 수 없다 | `unreadable` | 그 라운드 실패. 클라이언트가 아니라 검사가 만든다(`Client::unreadable(endpoint, why)`가 세고 돌려준다) |

- 스코프가 이름 붙인 코드는 10010, 10003, 33004다. 10004(서명), 10005(권한), 10007(인증 실패)와 HTTP 401은 OMS가 낫지 않는 것으로 보는 같은 묶음이라 더했다(소유자 질문 3, 멈추는 쪽이 안전하다). HTTP 401의 코드는 `401`로 적는다.
- 클라이언트 안에서 다시 시도하지 않는다. 다음 라운드가 다시 시도다(스펙 "Errors at start and in a round").
- 거래소의 글(`retMsg`, 2xx가 아닌 상태의 본문)은 오류와 로그 줄에 200자(바이트가 아니라 문자, 많아야 800바이트)까지만, 제어 문자(줄바꿈 등)와 유니코드 줄·문단 구분자(U+2028, U+2029)는 빈칸으로 바꿔 넣는다(U19a3 검토 1 S3, 검토 2 N2). 그래서 키 거절의 오류 줄은 언제나 한 줄이다.
- 2xx가 아닌 상태는 본문을 끝까지 읽지 못해도(본문 중에 연결이 닫힘, 본문 중에 시간 초과) 그 상태로 나눈다: 잘린 401은 `auth`(래치), 잘린 403은 `ip_banned`(멈춤), 잘린 429는 `rate_limited`. 메시지는 `(body cut: …)`다. 잘린 2xx 본문은 `no_answer`다(U19a3 검토 2 S1).

### 다시 보내지 않는 코드

- 첫 `auth` 응답이 클라이언트에 그 코드를 래치한다(`Client::auth_rejected()`). 그 뒤의 모든 `get`은 보내지 않고 세지 않고 `auth`(`"not sent: the key was rejected earlier…"`)로 끝난다. 오류 줄은 첫 응답에 한 번만 쓴다(동시에 나간 두 요청이 모두 거절돼도 하나다, 검토 2 N1). 프로세스를 다시 시작해야 풀린다.
- `watcher_auth_rejected`는 클라이언트를 만들 때 0으로 두고, 래치할 때 1로 둔다.
- IP 반향(`Client::echo(url)`)은 이 래치로 멈추지 않는다(판단(모델 아님)): 반향은 Bybit 요청이 아니고, 10010의 흔한 원인이 바뀐 공인 IP라 `watcher_public_ip_ok`가 계속 맞는 값을 보여 주는 편이 낫다. 403 멈춤과 요청 제한은 반향에도 걸린다.

### 403 멈춤

- HTTP 403을 받으면 **그 응답을 받은 순간의 시계 + 600,000 ms**까지 모든 Bybit 요청과 IP 반향이 멈춘다(F18 L186 "최소 10분", OMS와 같은 값). 그 사이의 요청은 보내지 않고 세지 않고 `ip_banned`로 끝난다. 끝나는 순간(`Client::paused_until()`)부터 다시 보낸다. 시작 확인의 재시도(U19a4b)는 이 값을 읽지 않고 자기가 받은 403 답의 시각 + 600초를 기다린다. 자기가 보낸 403이면 같은 순간이다(검토 1 N1).
- 반향 서비스 자신의 403은 Bybit의 차단이 아니다: 반향의 2xx가 아닌 상태는 모두 `no_answer`다.

### 요청 제한(가드)

- 어느 10초 창에서든 요청은 60개까지다. 모든 Bybit 경로와 IP 반향을 합친다(판단(모델 아님); 최악의 경우는 "Budget", U19a9). OMS와 같이 쓰는 IP 한도(5초에 600, 설계 §7.3)를 워처의 폭주 루프에서 지킨다.
- `rest::Guard::admit(now) -> Result<(), u64>`는 주입된 시계 위의 sans-IO 부분이다. 요청을 기록하고 보내게 하거나, 보낼 수 있는 가장 이른 순간(창의 첫 요청 + 10,000 ms)을 돌려준다. 거절된 요청은 기록하지 않는다. 시계가 뒤로 가면 옛 기록이 창에 남아 시계가 그 값을 다시 지날 때까지 막는다.
- 넘는 요청은 버리지 않고 기다린다. 클라이언트는 그 순간까지 자고 다시 묻는다. 한 번에 최대 100 ms만 자고 시계를 다시 읽는다(판단(모델 아님): 수동 시계나 한 걸음 움직인 시계를 놓치지 않도록. 실제 시계에서 10초를 기다리면 깨는 횟수가 100번이다). 깰 때마다 래치와 403 멈춤을 다시 본다.

### 요청 집계

- `watcher_requests_total{endpoint}`: 보내기 직전에 센다(요청 제한을 지난 뒤, 보내기 전). `endpoint`는 경로(`/v5/order/realtime` 등). 반향은 `ip_echo`다(URL은 설정이라 이름표에 넣지 않는다). 페이지마다 하나다.
- `watcher_request_errors_total{endpoint,class}`: 위 표의 부류. 래치와 403 멈춤 때문에 보내지 않은 요청은 세지 않는다(요청이 아니다).

### 지표 등록부와 VM 가져오기 클라이언트

- `metrics::Metrics`: 게이지(`set`, `remove`)와 카운터(`add`)를 이름과 이름표로 둔다. 모든 계열에 `venue="bybit"`, `host`, `watcher`가 붙는다(같은 이름의 계열 이름표보다 이 셋이 이긴다). 이름표는 정렬한다. 값이 유한하지 않은 계열과 지운 게이지는 보내지 않는다.
- 가져오기 본문(`Metrics::import_lines(ts_ms)`): 표본마다 JSON 한 줄 `{"metric":{"__name__":…,이름표},"values":[v],"timestamps":[ms]}`, 모두 내보내는 순간의 시각. `watcher_up 1`이 언제나 첫 줄이다(없으면 워처나 그 밀어 넣기가 사라진 것). 문자열은 JSON으로 따옴표를 친다.
- `metrics::Exporter::push(&Metrics, now_ms)`: `<vm_url>/api/v1/import`로 POST 하나(제한 시간 `request_timeout_ms`, 리다이렉트·프록시 없음). **큐가 없다**: 실패한 밀어 넣기는 버린다(게이지는 다음에 새 값으로, 카운터는 누적이라 다음 밀어 넣기가 따라잡는다). 첫 실패와 그 뒤 1분에 한 번까지만 로그를 쓴다. 워처는 작업 공간의 `obs-push`를 쓰지 않는다(독립성 규칙 3).
- 밀어 넣기 타이머(`export_ms`)는 엔진이 아니라 런타임(U19a7)의 것이다. 설정을 읽은 직후부터 돌고, 종료 때 한 번 더 민다.

### 알려진 한계(3부)

- 실제 Bybit, 실제 VictoriaMetrics에 보낸 적이 없다. 응답 형식과 코드는 문서(gateway facts, F18, F20)와 OMS의 분류에서 왔다.
- 요청 제한은 이 프로세스의 요청만 센다. OMS의 요청은 모른다. 60/10초는 IP 한도(10초에 1,200)의 5%다. 60이라는 값은 판단(모델 아님)이다.
- 요청 제한에서 기다리는 동안 실제 시간으로 최대 100 ms씩 잔다. 시계가 뒤로 가면 그만큼 더 기다린다(공통 결정 "Clock"의 한계와 같다).
- 카운터는 프로세스마다 0에서 시작한다(재시작 뒤 VM의 증가율 함수가 처리한다).
- 10004의 `retMsg`는 거래소가 서명한 문자열(`origin_string[<timestamp><api_key>…]`)을 담는다. 그래서 키 거절의 오류 줄에 API 키(비밀이 아니다. `Credentials`의 `Debug`도 찍는다)가 200자 안에서 나올 수 있다. 비밀은 어떤 오류나 로그에도 나오지 않는다.
- 본문을 읽는 데 크기 상한이 없다. 상한은 `request_timeout_ms` × 대역폭뿐이다(OMS의 클라이언트와 같다. 반향 서비스는 소유자가 고른 https 주소다).
- 연결 제한 3초는 `request_timeout_ms`가 3,000 이하이면(기본 2,000) 먼저 끝나는 전체 제한에 가려진다: 매달린 연결은 `not_sent`가 아니라 `no_answer`가 된다. 둘 다 그 라운드 실패라 효과는 같고 이름표만 다르다. 연결 제한의 부류는 시험하지 않았다.
- 요청 제한에서 기다리는 요청들은 순서 없이 100 ms마다 다시 묻는다. W1의 페이지가 계속 가득 채우면 한 IP 확인이나 W3 요청이 여러 번 질 수 있다(U19a9의 예산 시험이 판단한다). 이미 보낸(가드를 지난) 요청은 그 사이 다른 응답이 키 거절이나 403 멈춤을 걸어도 나간다. 막을 수 없고, 시험은 그 응답 뒤에 묻는 요청과 그때 가드에서 기다리던 요청을 본다.
- 재확인의 규칙(다른 `userID`, `isMaster`, 묶인 IP 없음 → `auth`처럼 재시작까지 멈춤, U19a4b)에는 클라이언트를 밖에서 래치하는 길이 필요하다. 이 부분에는 읽기(`auth_rejected()`)만 있고, 쓰기는 U19a4b가 더한다.
- 값에 `%`를 허용하므로(거래소 커서), 깨진 퍼센트 열은 그대로 간다. 거래소가 10004로 답하면 클라이언트는 재시작까지 래치한다(멈추는 쪽이라 안전하다). 값은 코드의 상수, 검사된 설정, 거래소 커서에서만 온다.
- IPv4 지역 주소와 프록시 없음은 loopback 가짜로 보일 수 없어 시험하지 않았다.
- `retCode`는 2xx 본문에서만 읽는다. 401·403·429가 아닌 2xx 아닌 상태의 본문 안 retCode는 보지 않는다(`no_answer`).

### 검증(3부)

`sign::the_strings_to_sign_and_signatures_match_independent_values`(거래소의 예시 문자열, RFC 4231 경우 2, 지어낸 비밀 `watcher-made-up-secret-42`로 Python `hmac`이 계산한 값; `tools/ci/secret_scan_allow.txt`에 이유와 함께), `sign::query_strings_are_sorted_and_unsafe_values_refused`, `client::every_request_carries_the_five_headers_and_the_signed_query`(Python 값과 같은 서명, 가짜가 요청을 받는 순간 카운터가 이미 올라가 있음, 보낼 수 없는 값은 아무것도 보내지 않음), `client::answers_are_classified`(부류마다 한 번, 다음 요청은 그대로 감; 줄바꿈이 든 긴 `retMsg`와 본문은 200자 한 줄), `client::auth_answers_are_never_retried`(10010, 10003, 33004, 10004, 10005, 10007, HTTP 401 각각: 응답 뒤 수동 시계를 10분 움직이며 매초 여섯 경로에 요청해도 가짜에 닿는 요청이 더 없고, 오류 줄은 하나), `client::http_403_pauses_every_request_for_600_s`(응답이 보낸 뒤 500 ms에 와도 멈춤은 응답부터 600초, 끝나기 1 ms 전까지 모든 경로와 반향이 보내지지 않음), `client::the_guard_holds_requests_beyond_60_in_10_s`(가드만, 수동 시계, 1분의 폭주 루프에서 어느 10초 창도 60 이하), `client::a_request_beyond_the_guard_waits_for_the_window`(클라이언트가 가득 찬 가드에서 기다리다 시계가 그 순간에 이르면 보내고, 그 요청은 기다린 뒤의 시계로 서명됨; 실제 시간), `client::a_request_waiting_at_the_guard_sees_a_later_rejection_or_pause`(가드의 마지막 자리를 받은 요청의 10010 또는 403 응답을 가짜가 다른 요청이 가드에서 기다릴 때까지 잡아 두고, 창이 끝나 깬 그 요청이 `auth`·`ip_banned`로 끝나며 가짜에 닿지 않음; 실제 시간; 검토 1 S2), `client::a_redirect_is_not_followed`(301, 302, 307, 308에 `get`과 반향 모두 `no_answer`, 리다이렉트 대상에 요청이 가지 않음; 검토 1 S1), `client::a_status_counts_when_its_body_is_cut`(`content-length: 100`에 5바이트를 보내고 닫는 loopback 서버: 401은 `auth`와 래치, 403은 `ip_banned`와 응답부터 600초 멈춤, 둘 다 다음 요청을 보내지 않음; 429는 `rate_limited`, 500과 200은 `no_answer`이고 다음 요청이 감; 검토 2 S1), `client::two_rejections_in_flight_write_one_error_line`(두 연결을 함께 잡았다가 둘 다 10010으로 답하는 loopback 서버: 둘 다 `auth`, 오류 줄 하나; 검토 2 N1), `metrics::the_import_lines_are_victoria_metrics_json`, `metrics::a_failed_push_is_dropped`. 변이: M188(10010을 다음 라운드에 다시 보냄), M191(질의 문자열을 정렬하지 않음), M192(403 멈춤 60초), M232(리다이렉트를 따름), M233(가드에서 기다린 요청이 깬 뒤 래치와 403 멈춤을 보지 않음; 둘 다 검토 1), M234(본문이 잘린 2xx 아닌 상태를 `no_answer`로 읽음; 검토 2 S1).

## 4부. 설정과 자격 파일

`watcher::config`, `watcher::creds`, `watcher::start`. `watcher --config <path>`가 읽을 TOML 파일 하나와 그 파일이 가리키는 자격 파일 하나, 그리고 그 뒤의 시작 확인(query-api, 공인 IP, 정산 코인)과 IP 재확인(아래 "시작 확인", "IP 재확인", U19a4b)이다. 바이너리는 아직 이것을 부르지 않는다(실행은 U19a7). 거부는 시작 거부가 된다: 종료 코드 2와 이유 한 줄이다(스펙 "Errors at start and in a round"). 읽기는 파일을 쓰지 않고 네트워크를 쓰지 않는다. 운영자가 보는 같은 표와 예는 [운영 문서](../operations/watcher.md)에 있다.

### 설정 파일(`config::Config::load(path)`, `Config::from_toml(text)`)

- TOML이다. **모르는 키는 거부한다**: 최상위, 모든 표(`[intervals]`, `[rules]`, `[w3]`, `[http]`, `[telemetry]`), 모르는 표(`[venue]` 등) 모두다. 틀린 키 이름이 기본값으로 조용히 바뀌지 않게 한다.
- 거부는 모두 이유 한 줄이다. TOML 자신의 오류는 파일을 인용하며 여러 줄에 걸치므로, 메시지와 시작 줄 번호와 그 줄의 키만 남긴다(`invalid type: string "fast", expected u64 at line 11 (key w1_ms)`). 빠진 필수 필드는 메시지만 남긴다(``missing field `status_url` ``): 그 오류의 범위는 파일 전체여서 줄 번호와 키가 다른, 맞게 쓴 줄을 가리키게 된다(검토 1 S1). `Config::load`는 앞에 `config <path>:`를 붙인다.
- 경로는 상대 경로를 설정 파일의 위치로 풀지 않는다. 경로마다 절대 경로여야 한다.
- 두 경로를 비교할 때(자격 파일이 데이터 디렉터리 밖인지, 두 소켓이 다른지)는 "가리키는 곳"으로 비교한다. 존재하는 가장 긴 앞부분은 파일 시스템으로 풀고(심볼릭 링크를 따른다), 나머지는 글자로 `.`과 `..`을 접는다(`config::resolved`, OMS의 같은 이름 함수와 별도로 썼다).

| 필드 | 기본 | 검사(범위는 판단(모델 아님)) |
|---|---|---|
| `name` | 필수 | 1~64자(바이트가 아니라 문자), 제어 문자 없음. 하트비트의 `watcher`(C-API `heartbeat`)와 지표의 `watcher` 이름표 |
| `network` | 필수 | `testnet` 또는 `mainnet`. REST 주소 `https://api-testnet.bybit.com` / `https://api.bybit.com`(OMS와 같은 호스트) |
| `mainnet_confirmed` | `false` | `mainnet`은 이것이 `true`일 때만 받는다(OMS와 같다). `testnet`에서는 아무것도 바꾸지 않는다 |
| `credentials_file` | 필수 | 절대 경로, `..` 없음, 데이터 디렉터리 밖(디렉터리 자신도 안 됨, 심볼릭 링크로 가린 경우도). 파일의 규칙은 아래 "자격 파일" |
| `sub_account_uid` | 필수 | 0보다 큰 정수. 키의 `userID`가 이것과 같아야 한다(시작 확인, U19a4b) |
| `symbols` | 필수 | 이름 1~50개, 각각 `[A-Z0-9]` 1~32자, 같은 이름 두 번 없음 |
| `ip_echo_url` | 필수 | `https`(위조된 답이 IP 확인을 통과할 수 있다, OMS와 같다; `https` URL은 호스트 없이 읽히지 않는다). 사용자·비밀번호 없음, 공백·제어 문자 없음(URL 읽기는 양 끝의 공백을 지우므로 적힌 글자와 쓰이는 URL이 달라진다; 검토 1 N3) |
| `data_dir` | 필수 | 절대 경로. OMS의 데이터 디렉터리이고 워처는 읽기만 한다. 있는지는 보지 않는다 |
| `status_url` | 필수 | 정확히 `http://127.0.0.1:<port>/api/status` 또는 `http://[::1]:<port>/api/status`. `<port>`는 0으로 시작하지 않는 십진수 1~65535, 80은 안 됨. `localhost`, 다른 주소, `https`, 사용자, 질의, 조각, 다른 경로는 거부(대시보드는 loopback만 듣는다) |
| `admin_socket`, `monitor_socket` | `<data_dir>/admin.sock`, `<data_dir>/monitor.sock` | 절대 경로, 서로 다름(가리키는 곳으로) |
| `host` | `/etc/hostname`(없거나 비면 `unknown`) | 지표의 `host` 이름표. 1~255자, 제어 문자 없음(판단(모델 아님)) |
| `[intervals]` `w1_ms`, `w2_ms` | 1000(설계 §7.5) | 500~10,000 |
| `w3_ms` | 2000(설계 §7.5) | 1000~30,000 |
| `w4_ms` | 5000(설계 §7.5) | 1000~60,000 |
| `heartbeat_ms` | 5000(설계 §7.5) | 1000~10,000(OMS의 30초 안) |
| `ip_check_ms` | 60,000(스코프) | 10,000~3,600,000 |
| `export_ms` | 1000(설계 §5.4.5의 1초 해상도) | 1000~60,000 |
| `[rules]` `min_age_ms` | 2000(설계 §7.5, 스코프) | 500~10,000 |
| `w4_overlap_ms` | 10,000(스코프) | 0~60,000 |
| `w4_max_pages` | 10(스코프) | 1~50 |
| `w4_initial_lookback_ms` | 600,000(판단(모델 아님)) | 0~86,400,000 |
| `mismatch_resend_ms` | 10,000(판단(모델 아님)) | 1000~600,000 |
| `[w3]` `account_mm_rate_max_bps` | 5000(OMS의 Bybit 값, C-MARGIN, 거기서도 판단) | 1~9999 |
| `daily_loss_bps` | 500(OMS의 기본, C-MARGIN) | 1~10,000 |
| `[http]` `recv_window_ms` | 5000(OMS의 기본) | 1000~20,000(OMS와 같은 범위) |
| `request_timeout_ms` | 2000(판단(모델 아님)) | 200~10,000 |
| `[telemetry]` `vm_url` | 없음(아무것도 내보내지 않음) | `http://`와 loopback 주소 숫자(`127.0.0.0/8`, `[::1]`)만. 사용자, 질의, 조각 없음. 공백·제어 문자 없음: 내보내기는 적힌 글자 뒤에 `/api/v1/import`를 붙이므로, 끝에 공백이 있으면 검사는 통과해도 모든 전송이 실패한다(검토 1 S3). 거부 줄은 URL을 인용하되 `@`가 있으면(비밀번호가 있을 수 있다) 인용하지 않는다(검토 1 N2) |

- 범위는 양 끝을 포함한다. 음수는 TOML을 읽는 단계에서 거부된다.
- 확인 횟수(2라운드)는 설정이 아니라 상수다(스코프가 정한다). "모든 주기는 설정에"(스코프)는 `[intervals]`와 `[rules]`가 맞춘다.
- `Config::client_config()`가 3부의 클라이언트에 REST 주소, `recv_window_ms`, `request_timeout_ms`를 준다.

### 자격 파일(`creds::load(path, data_dir)`)

- TOML `api_key`, `api_secret` 두 개만. 둘 다 비어 있지 않아야 하고, 다른 키가 있으면 거부한다.
- 읽는 순간에 설정의 경로 규칙(절대, `..` 없음, 데이터 디렉터리 밖)을 한 번 더 적용한다. 설정을 읽은 뒤 생긴 심볼릭 링크도 여기서 본다.
- 여는 핸들 하나로 검사하고 읽는다(검사와 읽기 사이에 파일을 바꿔치기할 수 없다). 열 때 마지막 구성 요소의 심볼릭 링크를 따르지 않고(`O_NOFOLLOW`; 링크면 "is a symbolic link"), FIFO의 쓰는 쪽을 기다리지 않는다(`O_NONBLOCK`; 바로 "is not a regular file").
- 그 핸들의 메타데이터: 일반 파일, 소유자 = 이 프로세스의 실효 uid(`geteuid`), 권한 정확히 `0600` 또는 `0400`(그룹·다른 사용자 비트, 소유자 실행 비트, set-id·sticky 비트가 있으면 거부; OMS의 `& 0o077` 규칙보다 좁다. 스펙의 표대로다).
- **오류는 파일 내용을 인용하지 않는다**: TOML 오류(줄을 인용한다)를 버리고 "is not TOML with api_key and api_secret and nothing else"만 쓴다. 경로는 적는다. 비밀은 어디에도 찍히지 않는다(`Credentials`의 `Debug`는 `<hidden>`).

### 시작 확인(`start::Start`, U19a4b)

설정과 자격 파일(1단계) 다음의 2~5단계다. 하나의 sans-IO 단계 기계이고 시계를 읽지도 자지도 않는다(스펙 "Clock"): `Start::new(cfg, metrics, log, now)`, `poll(now) -> Option<Request>`(다음 요청, 기다리는 중이거나 요청이 답을 기다리는 중이거나 끝났으면 `None`), `on_answer(now, 답) -> Step`. `Step`은 `Wait {until_ms}`, `Next`, `Refuse {reason}`(종료 코드 2와 이 한 줄), `StayUpNoRequests`(`auth`), `Done`이다. 요청은 `Request::Venue {path, params}`(서명된 Bybit GET) 또는 `Request::Echo`(`ip_echo_url`의 서명 없는 GET, 답은 다듬은 본문의 JSON 문자열)이고 `start::send(client, echo_url, req)`가 3부의 클라이언트로 보낸다. U19a7의 실행 코드가 이 기계를 돌린다. 시험은 수동 시계로 돌린다.

| 단계 | 요청 | 통과 | 거부(종료 2) |
|---|---|---|---|
| 2 | `GET /v5/user/query-api` | `isMaster == false`, `userID == sub_account_uid`, `ips`가 문자열 목록이고 IP 주소가 하나 이상, `*` 없음 | 마스터 키, 다른 `userID`, IP에 묶이지 않은 키(`[]` 또는 `*`가 든 목록), 주소가 하나도 없는 `ips` |
| 3 | (2단계의 답) | `watcher_api_key_days_left` = `floor((expiredAt − now) / 86,400초)`, 만료 뒤에는 음수 | 없음(아래) |
| 4 | `GET ip_echo_url` | `watcher_public_ip_ok` = 본문 주소가 `ips`에 있으면 1 | 없음(아래) |
| 5 | 기호마다 `GET /v5/market/instruments-info?category=linear&symbol=<s>` | 그 기호의 행이 있고 `settleCoin == "USDT"` | 행 없음(`<s> is not in instruments-info for category linear; the watcher's scope is linear/USDT`), 다른 코인(`<s> settles in <coin>; the watcher's scope is linear/USDT`) |

- 거부 이유는 한 줄이고 2단계의 것은 `query-api: `로 시작한다(예: `query-api: the API key belongs to user 4243; the config says sub_account_uid 4242`). 거부의 줄은 실행 코드가 찍는다. 기계는 거부 때 아무 줄도 쓰지 않고, 거부 뒤에는 어떤 요청도 내지 않는다.
- **`auth`**(키 거절, 3부): 끝내지 않는다. 클라이언트가 이미 재시작까지 모든 Bybit 요청을 멈췄고(`watcher_auth_rejected = 1`, 오류 한 줄), 기계는 `StayUpNoRequests`를 내고 그 뒤 아무것도 묻지 않는다. 검사가 성공하지 않으니 OMS의 게이트가 닫힌다. 내보내기(설정을 읽은 뒤부터)와 로그로 보인다. 재시작 루프로 다시 보내지 않는다.
- **그 밖의 답**(`not_sent`, `no_answer`, `rate_limited`, `clock`, `refused`, 그리고 retCode 0이지만 규칙으로 읽을 수 없는 답 `unreadable`: 필드가 없거나 형이 다름): 한 줄(`start: <단계> failed (<class>): …; asked again at <ms> ms`)을 쓰고 **답의 시각 + 5,000ms**(판단(모델 아님))에 같은 요청을 다시 낸다. **`ip_banned`**(HTTP 403)이면 **답의 시각 + 600,000ms**, 곧 클라이언트의 멈춤이 끝날 때다(검토 3 S2). 각 단계는 읽을 수 있는 답을 얻기 전에는 다음으로 가지 않는다. 5단계에서는 실패한 그 기호를 다시 묻는다(앞의 기호는 다시 묻지 않는다). `unreadable`은 기계가 `watcher_request_errors_total{endpoint,class="unreadable"}`로 센다(나머지는 클라이언트가 센다).
- 3단계 `expiredAt`: RFC 3339 UTC `YYYY-MM-DDTHH:MM:SSZ`만 읽는다(자기 해석기 `start::rfc3339_utc_ms`; 소수 초는 받고 버린다. 시차 표기, 소문자, 공백, 없는 날짜는 읽지 않는다). 필드가 없거나 `null`이거나 `1970-01-02T00:00:00Z` 이하(문서의 "해당 없음" 예가 epoch)이면 내보내지 않는다(게이지를 지운다). 읽을 수 없으면 내보내지 않고 경고 한 줄. 어느 경우도 거부가 아니다.
- 4단계: 반향 본문(다듬음)이 IPv4나 IPv6 주소이고 `ips`의 한 주소와 같으면 1(IPv4에 대응된 IPv6는 IPv4로 비교). 다르면 0과 경고 한 줄. 반향이 실패하거나(클라이언트의 어느 부류든, 403 멈춤 중 포함) 본문이 주소가 아니어도 0과 경고 한 줄이다: 모르는 IP는 맞는 IP가 아니다(검토 2 S3). 거부가 아니고 시작은 이어진다. 묶인 IP가 다르면 Bybit가 10010으로 답하고 워처는 스스로 멈춘다(3부).
- 5단계의 요청에는 `settleCoin`이 없다(instruments-info에는 그 거르개가 없다, gateway facts Q8.2). 행은 `symbol`이 같은 것을 고른다.
- `Done` 뒤 `Start::ip_check()`가 재확인을 준다. 첫 재확인은 4단계의 반향이 답한 시각 + `ip_check_ms`다(5단계가 그보다 오래 걸리면 바로). 5단계에서 키가 거절된 뒤(`StayUpNoRequests`)에도 `ip_check()`는 반향만 묻는 재확인을 준다(2단계의 `ips`를 알고 있으므로; 검토 1 M1). 2단계에서 거절되면 `ips`를 모르므로 재확인이 없고 `watcher_public_ip_ok`는 내보내지 않는다. U19a7의 실행 코드는 `Done`과 `StayUpNoRequests` 뒤 둘 다 `ip_check()`를 돌린다.

### IP 재확인(`start::IpCheck`, U19a4b)

- `ip_check_ms`(기본 60,000)마다 query-api를 다시 묻고(IP 목록이 바뀔 수 있다), 통과하면 바로 반향을 묻는다. `watcher_public_ip_ok`와 `watcher_api_key_days_left`를 새로 쓴다. 다음 재확인은 **이번 재확인의 마지막 답 + `ip_check_ms`**다. 같은 sans-IO 모양이다(`poll(now)`, `on_answer(now, 답) -> Step`, 다음 시각은 `Wait {until_ms}`).
- query-api가 실패하면(`auth` 말고 어느 부류든, `unreadable` 포함): `watcher_public_ip_ok = 0`과 경고 한 줄, `watcher_api_key_days_left`는 지난 값을 그대로 둔다, 반향은 묻지 않는다, 다음 재확인은 보통 간격으로. 반향이 실패하거나 본문이 주소가 아니면 0과 경고 한 줄(그 앞의 query-api가 준 남은 날은 새로 쓴다).
- `auth`: 클라이언트가 멈췄다(3부). 재확인은 `Next`를 내고 **반향만** 이어 간다: 바로 한 번, 그 뒤 마지막 답 + `ip_check_ms`마다. 비교하는 목록은 마지막으로 통과한 query-api의 `ips`다(첫 재확인이면 2단계의 것). 반향은 Bybit 요청이 아니고(3부), 10010의 흔한 원인이 바뀐 공인 IP라 `watcher_public_ip_ok`가 그것을 보여 줘야 한다(검토 1 M1). query-api는 다시 묻지 않으니 `watcher_api_key_days_left`는 마지막 값으로 남는다.
- 읽을 수 있는 query-api 답이 시작 규칙에 맞지 않으면(다른 `userID`, `isMaster = true`, 묶인 IP 없음 `[]`·`*`): `Step::StopRequests {reason}`. `start::apply(client, step)`이 `Client::stop_key(reason)`을 불러 **재시작까지 모든 Bybit 요청을 멈춘다**. 키 거절과 같은 래치이고 코드는 `rest::OWN_RULE_CODE`(-1, 거래소의 코드가 아님)다. `watcher_auth_rejected = 1`, 규칙을 적은 오류 한 줄(`error: the key no longer passes the start check: query-api: …; every Bybit request stops until the watcher restarts`)은 처음 한 번만이다(소유자 질문 13, 안전한 쪽). `watcher_public_ip_ok = 0`이 된다(통과하는 목록이 없으니 맞는 IP가 아니다, 검토 1 M1; 따로 경고 줄은 없다, 오류 줄이 이유다). 재확인은 그 뒤 반향도 묻지 않는다.
- 요청이 답을 기다리지 않는 때 들어온 답(앞선 `poll` 없는 답)은 아무것도 바꾸지 않는다: `Start`와 `IpCheck` 둘 다 지금의 `Step`을 돌려주고 다음 요청과 그 시각, 게이지를 그대로 둔다(검토 1 S1, 시험 `start::an_answer_without_a_request_in_flight_changes_nothing`).
- 이 요청들은 대조 예산(2.7 req/s)의 밖(분당 query-api 1, 반향 1)이고 요청 제한(3부) 안이다. U19a7의 실행 코드는 5단계가 끝난 뒤부터, `/api/status`를 기다리는 동안과 처음 재생 중에도 이것을 돌린다(검토 3 N2; 그 시험은 U19a7).

### 지표(4부)

| 이름 | 종류 | 뜻 |
|---|---|---|
| `watcher_api_key_days_left` | 게이지 | `expiredAt`까지 남은 날, 내림. 해당 없음(없음, `null`, epoch)이나 읽을 수 없으면 없음 |
| `watcher_public_ip_ok` | 게이지 0/1 | 반향이 본 공인 IP가 키의 `ips`(마지막으로 통과한 query-api의 것)에 있음. 반향 실패, 주소 아닌 본문, 실패한 재확인, 시작 규칙에 더는 맞지 않는 재확인은 0. 키 거절 뒤에도 반향만으로 계속 쓴다. 2단계에서 키가 거절되면 없음 |
| `watcher_auth_rejected` | 게이지 0/1 | 3부의 키 거절 또는 시작 규칙에 더는 맞지 않는 재확인이 모든 Bybit 요청을 멈췄음 |

### 알려진 한계(4부)

- 다른 소유자의 실제 파일은 시험이 만들지 못한다(root만 만든다). 그래서 소유자 규칙은 규칙 함수(`creds::file_rules`)에 다른 uid를 주어 보고, `creds::load`로는 root가 소유한 `/etc/hostname`을 읽어 본다(검토 1 S4; 그 파일이 없거나 시험의 uid가 소유하면 이 경우만 건너뛴다). root로 돌리면 자격 시험은 바로 끝난다(OMS의 권한 시험과 같다).
- 상위 디렉터리의 심볼릭 링크는 따른다(마지막 구성 요소만 거부한다). 데이터 디렉터리 밖인지는 링크를 따른 뒤의 위치로 본다. 자격 파일이 있는 디렉터리의 권한은 보지 않는다.
- 경로 규칙은 읽는 순간까지 본다. 읽은 뒤에 경로가 바뀌는 것은 보지 않는다(워처는 자격 파일을 시작 때 한 번 읽는다).
- 키와 비밀의 모양(HTTP 머리글에 넣을 수 있는 글자인지)은 보지 않는다. 그런 키는 첫 요청이 `not_sent`나 거래소의 키 거절로 끝난다.
- `status_url`의 포트 80을 거부하는 것은 판단(모델 아님)이다: URL 형식이 `http`의 기본 포트 `:80`을 지우므로 클라이언트가 보내는 `Host`에 포트가 없고, 대시보드는 포트 없는 `Host`를 거부한다(`127.0.0.1:<port>`만 받는다). `vm_url`의 `localhost`를 거부하는 것도 판단(모델 아님)이다(이름은 다른 주소로 풀릴 수 있다).
- 경로 비교는 존재하는 가장 긴 앞부분만 파일 시스템으로 푼다. 끊어진 심볼릭 링크(가리키는 곳이 아직 없는 링크)는 따르지 못하므로, `admin_socket`이 아직 없는 `monitor.sock`을 가리키는 링크면 두 소켓은 다르게 보인다(검토 1 N4). OMS가 엉뚱한 소켓으로 온 요청을 거부한다.
- 데이터 디렉터리와 두 소켓이 있는지는 보지 않는다. OMS가 꺼져 있어도 워처는 시작해서 기다린다(U19a7).
- 워처 키에 출금 권한이 없어야 한다(설계 §7.4). 시작 확인은 이것을 보지 않는다(OMS도 자기 키에 대해 보지 않는다). [운영 문서](../operations/watcher.md)에 적었다. `readOnly`와 `permissions`도 보지 않는다(워처는 U19a에서 조회만 한다).
- 시작 확인과 재확인은 loopback 가짜와 문서의 예(gateway facts Q4, Q8)로만 시험했다. 묶이지 않은 키의 query-api `ips`가 `["*"]`인지 `[]`인지는 문서로 정해지지 않았다(Q4): 둘 다 거부한다. CIDR 같은 주소가 아닌 항목은 주소와 맞지 않는 것으로 보고, 주소가 하나도 없는 목록은 거부한다(판단(모델 아님)).
- 403 뒤의 다시 묻기는 "그 답의 시각 + 600초"다. 다른 요청이 먼저 403을 받아 이미 멈춘 동안 보내지 않고 돌아온 `ip_banned`에도 같은 600초를 기다린다. 멈춤보다 늦게 다시 물을 수는 있어도 일찍 묻지는 않는다(안전한 쪽, 판단(모델 아님)).
- `Step::StopRequests`는 스스로 아무것도 멈추지 않는다. 호출자가 `start::apply(client, step)`을 불러야 클라이언트가 멈춘다(검토 1 N2). U19a7의 실행 코드가 부르고, 그 시험(실행 코드를 거친 규칙 위반 재확인 뒤 어느 경로로도 요청 없음)은 U19a7의 몫이다.
- `Start`와 `IpCheck`는 답을 기다리는 요청에 기한을 두지 않는다(검토 1 N3). 3부의 클라이언트는 언제나 답한다(시간 초과, 요청 제한). 그래서 실행 코드(U19a7)가 보낸 요청의 답을 버리지 않는 한 멈추지 않는다. 답을 잃으면(작업이 취소되면) 게이지가 조용히 그대로 남는다.
- 거래 중이 아닌 기호를 instruments-info가 기본 목록에서 빼는지는 확인하지 않았다(실제 호스트를 부르지 않았다, gateway facts가 정하지 않는다; 검토 1 N4). 빼면 그 기호의 행이 없어 시작이 종료 2로 거부된다(스펙의 "행이 있어야 한다", 거부하는 쪽). 계정에 그 기호의 포지션이 남아 있어도 워처는 재시작을 되풀이한다. 운영자는 `symbols`에서 그 기호를 빼거나 OMS로 정리한다.
- 2단계의 거부는 시작 때의 규칙이다. 재확인의 같은 규칙 위반은 끝내지 않고 요청을 멈춘다(소유자 질문 13). 재확인의 query-api가 계속 실패하면 공인 IP는 0으로 남고 남은 날은 마지막 값으로 남는다. 키가 그 사이 만료돼도 남은 날은 마지막 값에서 줄지 않는다(내보내는 값이 답을 받은 시각의 계산이다).
- 키가 거절된 뒤(재확인이나 5단계)에는 query-api를 묻지 않으므로 `watcher_api_key_days_left`는 마지막으로 통과한 답의 값에서 다시 줄지 않는다. `days_left`의 문턱에 거는 경보(U20b)는 이 상태에서 울리지 않는다. 이때의 신호는 `watcher_auth_rejected = 1`이다(검토 2 N2).
- `watcher_auth_rejected = 1`과 `watcher_public_ip_ok = 0`이 함께 보이는 원인은 셋이다: 바뀐 공인 IP, 실패한 반향(반향 서비스 고장이나 403 멈춤), 시작 규칙 위반. 두 게이지만으로 "IP가 바뀌었다"고 말할 수 없다. 경보 문구(U20b)는 원인을 단정하지 말고 경고·오류 줄을 보라고 적는다(검토 2 N3).

### 검증(4부)

`config::the_config_is_validated_and_mainnet_needs_confirmation`(필수 필드만의 기본값 전부, `mainnet`은 `mainnet_confirmed = true`일 때만, 최상위·표마다 모르는 키와 모르는 표, 필수 필드 여덟 각각 없음, 한 줄 오류와 줄 번호, 이름의 문자 수(`é` 64개는 받음), 기호의 모양·개수·중복, `https` 반향, 경로 규칙(상대, `..`, 데이터 디렉터리 안·자신·`.`·`..`로 쓴 데이터 디렉터리), `status_url` 15가지 거부와 세 가지 수용, `ip_echo_url`의 사용자·앞뒤 공백·줄바꿈·탭, 소켓의 기본·절대·같음(`.`, 링크로 가린 디렉터리, `..`; 검토 1 S2), 빠진 필드의 거부 줄에 줄 번호와 키가 없음(검토 1 S1), `host` 1~255자, 범위 16개의 양 끝과 그 밖 하나씩, 한 표의 두 필드, `vm_url` 세 가지 수용과 열두 가지 거부(조각, 앞뒤 공백, 탭, 줄바꿈 포함; 검토 1 S3), 비밀번호가 든 `vm_url`을 인용하지 않음), `config::the_config_file_is_read_and_its_refusals_name_the_file`, `creds::the_credentials_file_must_be_owner_only_and_outside_the_data_dir`(`0600`·`0400`은 받고 `0644`·`0640`·`0604`·`0660`·`0610`·`0700`·`0500`·`4600`·`2600`·`1600`은 거부, 다른 소유자(규칙 함수, 그리고 `creds::load`로 root의 `/etc/hostname`), 좋은 파일로 가는 심볼릭 링크, 디렉터리, FIFO(5초 안에 거부), 없는 파일, 내용 일곱 가지와 UTF-8 아닌 파일(어떤 오류에도 비밀이 없음), 데이터 디렉터리 안, `..`, 상대 경로, 데이터 디렉터리를 가리키는 링크(자격 쪽, 데이터 쪽), 다른 디렉터리를 가리키는 링크는 받음). 변이: M193(`mainnet_confirmed` 없이 `mainnet`을 받음; 스펙의 고의 결함 12), M235(두 소켓을 적힌 대로 비교), M236(자격 파일의 소유자를 자기 자신과 비교), M237(끝에 공백이 있는 `vm_url`을 받음; 검토 1).

시작 확인과 IP 재확인(U19a4b, `crates/watcher/tests/start.rs` 13개, 수동 시계, 클라이언트가 필요한 곳은 loopback 가짜 `http-fake`): `start::a_traded_symbol_that_does_not_settle_in_usdt_refuses_the_start`(USDC, 소문자 `usdt`, 빈 코인은 거부, 그 뒤 세 번째 기호도 아무 요청도 없음; 모두 USDT면 `Done`), `start::a_symbol_missing_from_instruments_info_refuses_the_start`, `start::a_master_key_another_uid_or_a_key_bound_to_no_ip_refuses_the_start`(마스터, 다른 uid, `[]`, `["*"]`, 주소와 `*`, 주소 없음; 대응된 IPv6는 받음), `start::a_key_that_expires_in_10_days_gives_days_left_10`(+10일은 10, +10일 − 1시간은 9, 소수 초, 만료 1시간 뒤 −1, 윤일), `start::the_documented_epoch_expiry_exports_no_days_left`(없음, `null`, epoch, 1970-01-02 이하; 읽을 수 없는 형식 열하나는 경고 한 줄), `start::an_echo_failure_or_a_body_that_is_not_an_address_gives_public_ip_ok_0`, `start::query_api_answers_other_than_auth_or_a_refusal_retry_every_5_s`(부류마다 `Wait {until_ms}` = 답 + 5,000ms, 403은 + 600,000ms, 그 전에는 요청 없음, 읽을 수 없는 답 여섯 가지를 셈), `start::settle_coin_answers_other_than_auth_or_a_refusal_retry_every_5_s`, `start::an_auth_answer_at_start_stays_up_without_requests`(10010, 10003, 33004, HTTP 401, 5단계의 10004; 시계 10분 동안 가짜에 요청 없음), `start::the_public_ip_is_rechecked_every_60_s`, `start::a_failed_recheck_sets_public_ip_ok_0_and_keeps_days_left`, `start::a_recheck_with_another_uid_a_master_key_or_no_bound_ip_stops_every_request`(멈춘 뒤 모든 경로가 보내지 않고 `auth`, 오류 한 줄, `watcher_public_ip_ok = 0`; 재확인의 10010 뒤에는 반향만 60초마다, 같은 IP는 1·다른 IP는 0, Bybit에는 요청 없음), `start::an_answer_without_a_request_in_flight_changes_nothing`(첫 요청 전, 기다림 중, 거부·`Done`·키 거절 뒤, 쉬는 재확인, 반향 차례, 키 거절 뒤 반향만, 규칙 위반 뒤; 검토 1 S1). 5단계의 키 거절 뒤 반향만의 재확인은 `start::an_auth_answer_at_start_stays_up_without_requests`가 본다(첫 반향은 4단계 반향의 답 + 60초에 처음 묻고, 그 1ms 전에는 묻지 않는다; 검토 2 S1). 변이: M189(정산 코인을 보지 않음; 고의 결함 8), M190(남은 날을 올림; 고의 결함 9), M225(403 뒤 5초에 다시 물음; 고의 결함 44), M238(재확인의 키 거절 뒤 반향도 묻지 않음; 검토 1 M1).

## 5부. 검사 엔진과 W1(U19a5)

### 엔진(`engine::Engine`, sans-IO)

- `Engine::new(cfg, tail, state, metrics, log, boot_ms, start_ms)`는 처음 재생을 마친 `Tail`과 `JournalState`를 받고 W1을 돌린다. 다른 검사(W2~W4, U19a6~U19a9)는 `Engine::add_check(logic, interval_ms)`로 더한다. 검사가 엔진에 더하는 것은 `engine::Logic` 하나다: 라운드의 첫 요청(`first`), 거래소 쪽 하나의 해석(`page` → `Next`, `Done`, `Unreadable`, `Fail`), 비교(`compare` → 차이 목록). 일정, 상태 맞추기, 저널 읽기, 두 라운드 규칙, 지표는 엔진의 것이다.
- `poll(now) -> Vec<Action>`, `on_venue(now, req_id, Result<Value, RestError>)`, `on_status(now, req_id, Result<Value, String>)`. `Action`은 `Venue {req_id, path, params}`, `Status {req_id}`, `Mismatch {check, detail}`, `Heartbeat {checks}`다. 엔진은 시계를 읽지도 자지도 않고 아무것도 보내지 않는다. U19a7의 실행 코드가 행동을 실행하고 답을 돌려준다. 시험은 모델 거래소와 수동 시계로 돌린다. 저널은 엔진이 `Tail`로 직접 읽는다(라운드마다 `read` 한 번, `MAX_RECORDS` 10,000건까지).
- **일정**: 검사마다와 하트비트에 `next_due`가 있고 처음 값은 `start_ms`다. 그래서 첫 라운드와 첫 하트비트(모두 `null`)는 첫 `poll`에 나온다. `poll(now)`에서 기한이 된 검사는 그 검사의 지난 라운드가 닫혔을 때만 라운드를 연다(겹치지 않고 쌓이지 않는다). `next_due` = 라운드 시작 + 간격이므로 늦은 라운드는 한 번만 돈다. 하트비트는 `heartbeat_ms`마다 W1~W4 각각의 마지막 성공 시각(ms, 없거나 돌지 않는 검사는 `null`)을 싣는다.
- **라운드**: (1) 거래소 쪽을 하나씩, (2) 마지막 거래소 답 뒤에 그 라운드만의 `Status {req_id}`, (3) 그 답의 `journal.durable_seq`까지 저널 읽기, (4) 비교. 거래소를 저널보다 먼저 읽는다: OMS는 기록이 내구화되기 전에 아무것도 보내지 않으므로(C-COMMIT) 거래소가 보여 주는 OMS의 주문은 그 뒤에 읽는 저널에 있다.
- **상태 답**: 답은 `req_id`로 그것을 물은 라운드 하나에만 쓴다. 두 라운드가 한 답을 나누지 않고 답은 어떤 순서로도 올 수 있다. 상태 요청은 낼 때 상태의 `last_seq`(`seq_at_send`)를 적어 둔다.
  - 라운드 실패(래치 없음): 답이 없음(`Err`: 닿지 않음, JSON 아님), `journal.durable_seq`가 u64가 아님, `core.instance_id`가 16자리 16진수가 아님. OMS가 답하지 않으면 아무것도 판단하지 않는다(소유자 질문 8).
  - 래치(재시작까지): `instance_id`가 저널 세그먼트의 것과 다름(`Instance`), 상한이 `seq_at_send`보다 낮음(`WentBack`: OMS가 워처가 묻기 전에 읽은 기록보다 낮은 상한을 냈다, 다른 저널이다). 저널이 아직 기록을 하나도 읽지 않았으면(세그먼트를 모름) 읽은 뒤 비교한다.
  - 상한이 지금의 `last_seq` 이하이지만 `seq_at_send` 이상이면 오류가 아니다. 다른 라운드가 그 사이 더 읽었으니 읽을 것이 없고, 이 라운드는 자기 거래소 읽기보다 새 상태와 비교한다(안전한 순서).
- **저널 오류**(1부의 두 부류): 다시 읽는 부류(`Io`, `Damaged`, `NoStart`)는 그 라운드를 실패시키고 다음 라운드가 같은 자리부터 다시 읽는다. 래치 부류(`UnknownEvent`, `Schema`, `Format`, `Instance`, `State`, `WentBack`)는 재시작까지 읽기를 멈추고, 그 뒤의 라운드는 **아무 요청도 내지 않고** 실패한다(판단할 수 없는 라운드에 요청 예산을 쓰지 않는다, 판단(모델 아님)). `watcher_journal_error`는 두 부류 모두 1이고, 다시 읽는 부류에서 상한까지 읽은 라운드가 0으로 돌린다. 래치 부류에서는 0으로 돌아가지 않는다. 줄은 상태가 바뀔 때 하나씩이다(`error: …; the journal is read again at the next round`, `the journal is read again without error`, `error: …; the journal reader stops until the watcher restarts`). 한 번의 `read`로 상한에 닿지 못한 라운드는 실패하고 다음 라운드가 이어 읽는다. 원인은 둘이다: 10,000건 넘게 밀림, 또는 몇 건을 읽은 뒤 다시 읽는 부류의 오류(상한 아래 기록의 손상, 빠진 세그먼트, 모르는 이벤트 등; `read`는 오류 앞까지 읽은 것을 돌려준다). 뒤의 경우 오류와 `watcher_journal_error = 1`은 다음 라운드(그 자리에서 다시 읽을 때)에 나오고, 래치 부류면 래치도 한 라운드 늦어 그 라운드는 거래소에 한 번 더 묻는다. 어느 쪽이든 그 라운드는 판단하지 않는다(검토 1 N1).
- **기한과 길 잃은 답**(검토 3 N3): 낸 `Venue`·`Status` 요청마다 기한 = 낸 시각 + `request_timeout_ms` + 11,000ms(요청 제한의 10초 창 + 1초, 판단(모델 아님)). `poll`에서 기한에 닿은 요청(`now ≥ 기한`)은 `no_answer`로 보고 그 라운드를 실패로 닫는다. 그래서 답 하나를 잃어도 검사가 영영 열려 있지 않다. 어느 열린 요청도 갖지 않은 `req_id`의 답(기한 뒤의 늦은 답, 중복, 상태 요청에 온 거래소 답)은 `debug:` 한 줄과 함께 버린다.
- **실패**: 거래소 답의 오류(3부의 어느 부류든), 읽을 수 없는 쪽(retCode 0; 엔진이 `watcher_request_errors_total{endpoint,class="unreadable"}`로 센다), 쪽 수 초과, 상태 답, 저널. 라운드 안에서 다시 묻지 않는다(간격이 곧 물러섬이고, 다음 라운드가 다시 묻는다). 줄은 실패 이유가 바뀔 때만 `warn: <check> round failed: …` 하나, 다시 성공하면 `<check> rounds succeed again` 하나다.
- **성공**: 모든 요청이 답했고, 저널이 상한에 닿았고, 모든 차이가 판단됐다(일치, 또는 셀 수 있는 차이). 성공 시각은 비교를 마친 시계(`on_status`의 `now`)다. 실패하거나 미룬 라운드는 지난 성공 시각을 그대로 두고 `watcher_check_age_s{check}`가 자란다. "라운드가 무엇을 본 시각"(모르는 주문의 첫 sighting)도 이 같은 시계다.

### 두 라운드 규칙

- 차이의 정체(identity)는 `(check, kind, subject, value)`다. **보류 중**(pending): 지난 완료 라운드에 없던 첫 sighting, 라운드는 성공할 수 있다. **미룸**(deferred): 지난 완료 라운드에도 이번에도 있지만 나이 조건이 맞지 않음, 라운드는 성공이 아니다(OMS의 게이트가 W1 나이 10초로 미룸을 묶는다, 설계 R5). **확정**(confirmed): 둘 다 있고 이번 라운드에서 나이 조건이 맞음. 완료 라운드는 비교까지 간 라운드다. 실패한 라운드는 지난 차이 목록을 바꾸지 않는다: 보았다, 실패, 보았다면 세 번째 라운드가 확정한다(검토 1 S1).
- 나이 조건은 어디서나 `now − t ≥ min_age_ms`(기본 2,000ms)이고 **확정하는 라운드에서** 본다. 첫 라운드는 어려도 된다. W1의 `t`: 저널이 아는 주문은 그 주문의 `fact_at`(µs를 ms로 올림: 실제보다 늙게 보지 않는다), 저널이 모르는 주문은 그 정체를 처음 본 라운드의 비교 시각.
- 한 라운드 안에서 같은 정체가 두 번 나오면(주문이 쪽 사이를 옮겨 두 쪽에 실림) 하나로 센다(검토 1 S2).
- 한 라운드에서만 본 차이는 울리지 않는다. 한 완료 라운드에서 보이지 않거나 정체가 바뀌면(다른 값, 다른 종류) 처음부터 센다.
- 확정되면: `watcher_mismatches_total{check}` += 1(정체마다 한 번: 그 뒤 라운드가 같은 정체를 계속 보아도 다시 세지 않는다, 검토 2 N2), `error: confirmed difference: <detail>` 한 줄, 그리고 그 라운드가 새로 확정한 것을 모두 싣는 `Mismatch {check, detail}` 하나. 확정된 차이는 그것을 보지 않는 라운드가 올 때까지 목록에 남는다(로그와 `watcher_differences`에서는 빠진다). 다시 보내기, 빚, 보류 상태 따르기는 U19a7이다. U19a5의 엔진은 확정하는 라운드에서 한 번 낸다.
- `detail`: `"<check> <kind> <subject>: venue <…>, journal <…>; seen 2 rounds, journal entry <a> s old"`(모르는 주문은 `first seen <a> s ago`; `<a>`는 소수 한 자리, 내림). 여럿이면 첫 것 뒤에 `"; and N more"`. 1024바이트를 넘으면 글자 경계에서 자른다(`api.md`).

### W1 열린 주문(`w1::W1`, `w1_ms`마다, 기본 1초)

- 거래소: `GET /v5/order/realtime?category=linear&settleCoin=USDT&limit=50`과 `cursor` 쪽들, 라운드에 20쪽까지(판단(모델 아님), OMS와 같다). 20쪽째에도 커서가 있으면 라운드 실패(`more than 1000 open orders`). 쪽 규칙(OMS 자신의 규칙): 커서가 비었고 가득 차지 않은 쪽이 마지막이다. 커서가 빈 가득 찬 쪽(50행)은 `unreadable`(마지막이 아닐 수 있다), 방금 보낸 커서와 같은 커서도 `unreadable`, 그 밖에는 커서를 따른다(커서가 있는 빈 쪽도 쪽 한도 안에서 따른다). `result.list`가 목록이 아니거나 `nextPageCursor`가 문자열이 아니면 `unreadable`. 행마다 `orderStatus`, `orderLinkId`(문자열, 비어도 됨), `orderId`, `symbol`, `side`, `qty`가 문자열이어야 하고 아니면 `unreadable`. 열린 주문은 `orderStatus ∈ {New, PartiallyFilled, Untriggered}`이고 다른 행은 버린다.
- 저널: 열린 주문 집합과 주문 정보(2부).

| 종류 | 조건 | 대상 |
|---|---|---|
| `foreign` | 거래소에 열림. `orderLinkId`가 저널의 인스턴스로 `Base32Pair` 해독되지 않거나 저널이 갖지 않은 주문 번호 | 거래소 `orderId` |
| `closed_in_journal` | 거래소에 열림. 저널에서 닫힘 | `orderLinkId` |
| `open_after_terminal` | 거래소에 열림. 저널에 `venue_terminal`이 있고 `order_closed`가 없음. 나이는 `venue_terminal`이 옮긴 `fact_at`이라, 저널이 끝을 적고 `min_age_ms` 뒤에도 거래소가 열어 둔 주문이다 | `orderLinkId` |
| `details` | 둘 다 열림. 종목이나 방향이 다름 | `orderLinkId` |
| `missing_at_venue` | 저널에서 열림, 확인됨(`acked`), 결과 불명이 아님, 거래소 종결이 아님. 거래소에 열려 있지 않음 | `orderLinkId`(부호화) |

- 비교하지 않는 것: 확인되지 않았거나 결과 불명인 저널의 열린 주문이 거래소에 없는 것(보내지 않았거나 결과를 모르는 주문이고 OMS가 스스로 푼다, C-GW G-1/G-5, 소유자 질문 5). 거래소 종결(`venue_terminal`)이 있고 거래소에 없는 주문은 일치다(저널이 거래소가 끝냈음을 적었다. 빠진 체결은 W2와 W4가 본다, 검토 2 S1). 수량(정정과 부분 체결이 바꾼다. 체결은 W2와 W4).
- `fact_at`은 주문의 `order_accepted`, `order_acked`, `order_in_doubt`, `venue_terminal`, `order_closed`만 옮긴다(2부). 그래서 다른 이벤트를 계속 받는 주문(닫힌 뒤의 체결 등)이 W1 차이를 어리게 붙잡지 못한다(검토 1 S1).

### 지표(5부)

| 이름 | 종류 | 뜻 |
|---|---|---|
| `watcher_journal_seq` | 게이지 | 마지막으로 적용한 기록 |
| `watcher_journal_error` | 게이지 0/1 | 마지막 저널 읽기가 실패했거나(다시 읽는 부류) 읽기가 래치됨 |
| `watcher_check_age_s{check}` | 게이지 | 검사의 마지막 성공부터의 초. 첫 성공 전에는 `boot_ms`(설정을 읽은 시각)부터. `poll`마다 새로 쓴다 |
| `watcher_differences{check}` | 게이지 | 확정되지 않은 차이(보류 중과 미룸), 마지막 완료 라운드의 것 |
| `watcher_mismatches_total{check}` | 카운터 | 확정된 차이, 정체마다 한 번 |
| `watcher_mismatch_active{check}` | 게이지 | 확정된 차이 중 라운드가 아직 보는 것. 그것을 보지 않는 완료 라운드가 0으로 돌린다(U19a7에서 빚진 차이도 센다; 검토 4 S1) |

### 알려진 한계(5부)

- 엔진을 돌리는 실행 코드(U19a7)가 아직 없다. U19a5 뒤에도 바이너리는 아무것도 감시하지 않는다. `Mismatch`는 아직 아무 데도 보내지지 않고 하트비트도 그렇다.
- 시계가 뒤로 가면 경과 시간이 `saturating_sub`이라 타이머는 시계가 옛 값을 다시 지날 때까지 멈춘다(OMS 게이트와 같다, U18b4 검토 M1). 기한, 일정, 나이 모두 그렇다. 그동안 라운드도 하트비트도 없고 `watcher_check_age_s`는 0으로 읽힌다. 그래서 U20b의 경보는 이 게이지 하나에만 기대지 않는다(검토 1 N4).
- 짧은 동안(두 라운드 안에) 새 `orderId`로 바뀌는 남의 주문처럼 라운드마다 정체가 바뀌는 차이는 확정되지 않고 라운드는 성공한다(늘 첫 sighting). 그 체결은 W2와 W4가 본다. `watcher_differences`가 오래 0이 아니면 이것이다(U20b의 경보 후보; 검토 1 N8).
- 실패 이유에 숫자가 든 줄(`the journal read stopped at record N, below the cap M`)은 이유가 라운드마다 바뀌므로 밀린 동안 라운드마다(1초에 한 줄) 쓴다. 빠르기는 묶여 있고 메모리는 자라지 않는다(검토 1 N7).
- 저널에서 지워진(`purged`) 주문이 거래소에 열려 있으면 그 정보가 없으므로 `closed_in_journal`이 아니라 `foreign`으로 보인다. 둘 다 차이이고 종류만 다르다(판단(모델 아님): 지운 번호를 따로 기억하면 메모리가 묶이지 않는다).
- 래치된 읽기 뒤의 라운드는 거래소에도 묻지 않는다. 래치의 신호는 `watcher_journal_error = 1`, 오류 한 줄, 자라는 `watcher_check_age_s`이고, OMS의 게이트는 W1·W2가 10초 넘게 성공하지 않으면 닫힌다.
- 두 검사의 라운드가 함께 열린 경우(상태 답의 순서)는 U19a5에서 W1과 시험의 대역 검사로 시험했고, U19a6부터 W1과 실제 W2(빈 쪽, 일치)로 시험한다.
- W1은 같은 `orderLinkId`를 가진 거래소 행 둘을 따로 판단한다(Bybit에서 열린 주문의 `orderLinkId`는 유일하다고 보지만 문서로 확인하지 않았다, 판단(모델 아님)).
- 거래소 응답의 모양은 문서(gateway facts Q10)와 OMS의 쪽 규칙에서 왔다. 실제 Bybit에서 돌려 본 적이 없다(키가 없다, U-01).

### 검증(5부)

`crates/watcher/tests/w1.rs` 13개와 `crates/watcher/tests/engine.rs` 10개(엔진 시험 도구 `tests/common/rig.rs`: 프로세스 안의 모델 거래소, 시험 저널 쓰기, 수동 시계, 지연 0의 답). 완료 기준: `w1::a_one_round_difference_does_not_alarm`(보았다, 안 보았다, 보았다를 되풀이해도 울리지 않음), `w1::a_two_round_difference_older_than_2_s_alarms`(첫 라운드 1.5초로 어리고 확정 라운드 2.5초), `w1::a_journal_entry_younger_than_2_s_does_not_alarm`(정확히 2초는 확정, 1µs 모자라면 미룸; `closed_in_journal`과 `missing_at_venue` 모두). 그 밖에 `w1::an_unacked_order_missing_at_the_venue_is_not_compared`, `w1::a_foreign_order_at_the_venue_is_a_mismatch`(빈 `orderLinkId`, 남의 형식, 다른 인스턴스, 모르는 주문 번호; 첫 sighting부터 2초), `w1::an_order_closed_in_the_journal_and_open_at_the_venue_is_a_mismatch`(`details` 둘 포함), `w1::the_venue_is_read_before_the_journal`(51개 주문, 두 쪽; 상태 요청은 마지막 쪽 뒤), `w1::a_difference_whose_order_keeps_receiving_events_is_confirmed_by_its_fact_age`, `w1::a_young_difference_seen_twice_defers_the_round`, `w1::a_full_page_without_a_cursor_or_an_unchanged_cursor_fails_the_round`(빈 커서의 가득 찬 쪽, 같은 커서, 필드 없는 행, 목록 아닌 `list`, 21쪽, 커서 있는 빈 쪽), `w1::an_order_ended_at_the_venue_in_the_journal_and_not_listed_agrees`, `w1::an_order_ended_in_the_journal_and_still_open_at_the_venue_is_open_after_terminal`, `w1::an_order_listed_on_two_pages_is_one_difference`(검토 1 S2), `engine::a_late_round_runs_once_and_rounds_do_not_overlap`, `engine::the_first_rounds_and_heartbeat_are_due_at_start`, `engine::a_retried_journal_error_heals_and_a_latched_one_fails_every_round`, `engine::a_status_of_another_instance_or_a_cap_below_the_read_before_the_request_stops_the_reader`, `engine::status_answers_in_reverse_order_are_each_used_by_their_own_round`, `engine::the_detail_is_at_most_1024_bytes`, `engine::a_confirmed_identity_is_counted_once`, `engine::a_request_without_an_answer_fails_its_round_at_the_deadline_and_a_late_answer_is_ignored`, `engine::mismatch_active_counts_the_confirmed_differences_a_round_still_sees`, `engine::a_failed_round_keeps_the_sightings_of_the_last_completed_round`(거래소 실패와 OMS 무응답, 검토 1 S1). 변이: M194(한 라운드가 확정; 고의 결함 13), M195(나이를 첫 라운드에서 봄; 14), M198(확인되지 않은 주문을 비교; 17), M211(상태 답을 다른 라운드가 씀; 30), M212(W1 나이를 저널의 마지막 기록에서; 31), M213(두 번 본 어린 차이를 성공으로; 32), M214(커서 없는 가득 찬 쪽을 받음; 33), M219(거래소 종결 주문을 `missing_at_venue`로; 38), M224(기한 지난 요청이 라운드를 열어 둠; 43), M229(`watcher_mismatch_active`가 내려가지 않음; 48), M239(나이를 보지 않음; 14의 둘째 꼴), M240(내려감을 지금의 `last_seq`와 비교; 30의 둘째 꼴), M241(실패한 라운드가 지난 sighting을 지움; 검토 1 S1), M242(한 라운드의 같은 정체를 두 번 셈; 검토 1 S2).

## 6부. W2 포지션(U19a6)

### W2(`w2::W2`, `w2_ms`마다, 기본 1초)

- 거래소: `GET /v5/position/list?category=linear&settleCoin=USDT&limit=200`과 `cursor` 쪽들, 라운드에 5쪽까지(판단(모델 아님)). 5쪽째에도 커서가 있으면 라운드 실패(`more than 1000 positions`). 쪽 규칙은 W1과 같다(5부: 커서가 빈 가득 찬 쪽과 방금 보낸 커서는 `unreadable`, 커서가 있는 빈 쪽은 따른다). 행마다 `symbol`(문자열), `size`(소수: 지수, `+`, 9자리 이상의 소수는 거절), `side`(`Buy` +, `Sell` −, `""`이나 없음은 0, 그 밖의 값은 `unreadable`), `updatedTime`(ms 문자열, 선택. 숫자로 읽히지 않으면 없는 것으로 본다). 한 기호의 행이 둘 이상이면(헤지 모드, 쪽을 넘어서도) `unreadable`이다(OMS도 헤지 모드를 거부한다, `oms-node/src/venue/bybit/checks.rs:313-326`). `result.list`가 목록이 아니거나 `nextPageCursor`가 문자열이 아니어도 `unreadable`이다.
- 저널: 종목별 포지션 = 체결 + 외부 체결 + 거래소 변경 + 조정(2부, C-TAIL). 주차된 체결은 세지 않는다(소유자 질문 4). 저널의 종목 이름은 거래소의 `symbol`과 같다고 본다(W1과 같다).
- 차이: 어느 한쪽에 있는 기호마다 `d = 거래소 − 저널`. `d = 0`이면 일치, 아니면 차이 `(W2, position, <기호>, d)`. `d`가 정체의 값이라 다른 `d`는 다른 차이이고 처음부터 센다. `detail`: `W2 position <기호>: venue <거래소 크기>, journal <저널 포지션>; seen 2 rounds, journal entry <a> s old`(`<a>`는 아래 두 시각 중 새 것의 나이; 둘 다 모르면 `change time unknown`).

### 두 변경 시각과 미룸

- 두 시각을 본다. 저널의 `last_change_at`: 그 종목의 마지막 `fill_booked`·`external_fill_booked`·`venue_position_changed`·`position_adjusted` 기록 시각(2부; µs를 ms로 올림, 실제보다 늙게 보지 않는다). 거래소의 최근 변경 시각(아래). 판단에는 둘 중 알려진 것의 새 것을 쓴다.
- 시각이 **어리다**: 알려져 있고 `now − t < min_age_ms`(기본 2,000ms). **모르는 시각은 늙음**이다: 저널이 그 종목을 바꾼 적이 없음(수동 거래, 옮겨진 포지션, OMS가 보지 못한 체결)과 거래소 시각이 없음 모두(검토 2 M2, 소유자 질문 10). 그래서 저널이 한 번도 바꾸지 않은 종목의 거래소 포지션은 두 번째 라운드에 확정된다. `Option`으로 시각을 가진 구현은 "알려져 있고 어림"을 시험해야 하고 "알려져 있고 늙음"을 시험하면 안 된다.
- 규칙(W1보다 엄격, 설계 R5 SHOULD 2): 시각이 어리면 **첫 sighting이어도 미룸**이다. 라운드는 성공이 아니고 그 차이는 `watcher_differences`에 남는다(다음 라운드가 같은 `d`를 보면 두 번째 sighting이다). 어리지 않고 지난 완료 라운드에 같은 `d`가 있었으면 확정, 아니면 보류 중이고 라운드는 성공한다. 엔진에는 `engine::Logic::young_defers_at_first_sighting()`(기본 거짓, W2만 참) 하나로 들어간다. 일정, 두 라운드 규칙, 확정과 지표는 5부의 엔진 그대로다.
- 체결이 이어지는 동안 W2는 성공하지 않아 `watcher_check_age_s{check="W2"}`만 자라고 하트비트의 W2 시각은 마지막 성공에 머문다. 10초를 넘는 미룸은 OMS의 모니터링 게이트가 다룬다(설계 R5, C-LIVE 2부). W2 미룸만의 지표는 없다(범위).

### 거래소 관찰 시각 `seen_at`(검토 3 S1)

- `settleCoin=USDT`이고 `symbol`이 없으면 `position/list`는 크기가 0보다 큰 행만 준다(gateway facts Q12.1). 그래서 막 0이 된 포지션에는 행도 `updatedTime`도 없다. 바로 닫힌 순간에 그렇다. W2는 기호마다 `{크기, seen_at}`를 둔다: 모든 쪽을 읽은 마지막 라운드의 부호 있는 크기(행이 없으면 0)와, 그 크기를 처음 본 라운드의 시각(그 라운드의 비교 시계, 5부 "성공").
- 모든 쪽을 읽은 첫 라운드의 크기는 `seen_at`이 없다. 시작 때 찾은 상태이지 본 변경이 아니다. 그 뒤 다른 크기를 읽은 라운드가 `seen_at`을 자기 시각으로 한다.
- 모든 쪽을 읽은 뒤 비교 전에 실패한 라운드(`/api/status` 무응답, 저널 오류, 상태 요청의 기한)도 크기를 남기고, 시각은 마지막 거래소 답의 시계다(검토 4 N2). OMS가 내려가 있는 동안 본 변경은 OMS가 다시 답할 때 이미 늙었고 다시 어려지지 않는다. 남기는 일은 다음 라운드의 첫 요청에서 한다(그 사이 아무도 이 목록을 읽지 않는다). 쪽 중간에 실패했거나 읽을 수 없는 쪽이 있던 라운드는 아무것도 남기지 않는다.
- 거래소의 최근 변경 시각 = 행의 `updatedTime`(있고 읽히면), 아니면 `seen_at`. U19a9에서 W4가 마지막으로 맞춘 그 기호의 `execTime`이 더 새면 그것이 더해진다. 셋 다 없을 때만(첫 라운드부터 크기가 그대로이고 `updatedTime`이 없음) 모름, 곧 늙음이다.
- 그래서 거래소에서 닫힌 포지션(t에 닫힘, 그것을 처음 0으로 본 라운드 r ≤ t + 1초)은 `now − r < 2초`인 동안 미룬다. `updatedTime` 있는 0 아닌 행과 같다. `updatedTime` 없이 라운드마다 크기가 바뀌면 라운드마다 어려 미루므로, 아무것도 확정하지 않는 동안 W2가 신선한 성공을 보고하지 않고 게이트가 10초에 닫힌다.
- 목록에는 거래소나 저널이 보인 모든 기호가 하나씩 있고 거래소의 linear 기호 수로 묶인다(판단(모델 아님): 수백 개).

### 3초 완료 기준

- 수동 시계와 지연 0의 답에서, 체결이 멈춘 뒤 심은 차이(`updatedTime` = 심은 순간, 저널은 따라가지 않음)는 라운드 시작(거래소 읽기 전)에 심으면 +2,000ms, 라운드의 거래소 읽기 1ms 뒤에 심으면 +2,999ms에 확정된다. 첫 라운드는 어려서 미루고, 나이가 2초 이상인 첫 라운드가 지난 라운드와 같은 `d`로 확정한다. 요청 지연은 수동 시계에서 0이고 기준에 들지 않는다. `updatedTime`이 없으면 `seen_at`(변경 뒤 많아야 한 간격)이 쓰여 확정은 변경 뒤 많아야 1,000 + 2,000ms다.

### 지표(6부)

새 지표는 없다. W2는 5부의 `watcher_check_age_s{check="W2"}`, `watcher_differences{check="W2"}`, `watcher_mismatches_total{check="W2"}`, `watcher_mismatch_active{check="W2"}`와 3부의 `watcher_request_errors_total{endpoint="/v5/position/list",class="unreadable"}`를 쓴다.

### 알려진 한계(6부)

- 실행 코드(U19a7)가 아직 없다. `Engine::new`는 지금도 W1만 더하고(5부), U19a7의 실행 코드가 `Engine::add_check(Box::new(W2::default()), w2_ms)`로 W2를 더한다. U19a6 뒤에도 워처는 아무것도 감시하지 않는다.
- 호스트 시계가 거래소보다 `min_age_ms`(2초) 이상 앞서면 모든 거래소 시각(`updatedTime`)이 늙어 보인다(`recv_window` 5초 안이면 그런 시계도 서명을 통과한다). 효과는 더 이른 확정이지 놓친 확정이 아니다(검토 3 N7). 시계가 뒤로 가는 한계(5부)와 함께 본다.
- `updatedTime`의 뜻("Position updated timestamp (ms)", gateway facts Q12.7)은 문서에서만 왔다. 크기가 아닌 것(가격, 증거금, 펀딩)이 그것을 옮기는지는 모른다. 옮긴다면 미룸이 늘 뿐 확정을 놓치지는 않는다.
- 라운드 도중 포지션이 바뀌어 한 기호가 두 쪽에 실리면 헤지 모드와 구별하지 못해 그 라운드는 `unreadable`이다(다음 라운드가 다시 읽는다; 판단(모델 아님): 쪽이 둘 이상인 계좌, 곧 200개 넘는 포지션에서만 생긴다).
- `side`가 `""`인 행은 크기와 관계없이 0으로 읽는다(스펙의 규칙).
- W2는 크기만 비교한다(진입가, 레버리지, 증거금은 아니다).
- 주차된 체결은 세지 않으므로 약 3초 넘게 주차된 체결은 W2 확정을 부를 수 있다(소유자 질문 4; 보류는 안전한 쪽).
- 실제 Bybit의 `position/list` 응답으로 돌려 본 적이 없다(키가 없다, U-01). 행의 모양은 gateway facts Q12와 스펙에서 왔다.

### 검증(6부)

`crates/watcher/tests/w2.rs` 12개(5부의 엔진 시험 도구; 모델 거래소에 포지션 목록과 쪽마다의 스크립트, 상태 무응답을 더했다). 완료 기준: `w2::five_fills_per_second_for_60_s_give_no_mismatch`(모델 거래소가 200ms마다 체결하고 시험 저널이 300ms 뒤에 기록, 판단(모델 아님); 60초 동안 `Mismatch` 0개, W2 성공은 T0에 머물고 `watcher_check_age_s{W2}` = 60, 체결이 멈추면 다시 성공), `w2::a_difference_planted_after_the_fills_stop_is_confirmed_within_3_s`(라운드의 위상 0~900ms를 100ms 간격으로, 그리고 읽기 1ms 뒤; 확정은 +2,000~+3,000ms이고 위상마다 정확한 값). 그 밖에 `w2::a_changing_difference_restarts_the_count`(거래소가 모르는 저널 포지션 포함), `w2::a_deferred_round_is_not_a_success`(하트비트의 W2 시각), `w2::a_venue_position_in_a_symbol_the_journal_never_changed_is_confirmed_on_the_second_round`(검토 2 M2; 요청 매개변수), `w2::a_young_journal_change_defers_even_at_the_first_sighting`(정확히 2초는 늙음, 1µs 모자라면 어림), `w2::hedge_mode_rows_are_unreadable`(한 쪽, 두 쪽), `w2::bad_pages_and_rows_fail_the_round`(커서 없는 가득 찬 쪽, 같은 커서, 모르는 방향, 지수 크기, 기호 없음, 6쪽, 거래소 오류, 빈 방향, 커서 있는 빈 쪽), `w2::a_closed_venue_position_defers_until_2_s_after_it_was_first_seen_flat`, `w2::a_difference_that_changes_every_round_without_a_venue_time_is_not_a_success`(검토 3 S1), `w2::sizes_found_at_the_first_round_have_no_seen_time`(무응답과 읽을 수 없는 쪽 뒤의 첫 읽기), `w2::a_size_seen_by_a_round_that_then_fails_keeps_its_seen_time`(검토 4 N2). `engine::status_answers_in_reverse_order_are_each_used_by_their_own_round`는 U19a6부터 W1과 실제 W2로 돈다. 변이: M196(W2가 다른 두 값을 확정; 고의 결함 15), M197(미룬 W2 라운드를 성공으로; 16), M218(저널 시각이 없으면 어림으로; 37), M223(행이 없는 기호에 `seen_at`이 없음; 42).
