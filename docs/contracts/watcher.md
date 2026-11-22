# C-WATCH 독립 워처

상태: 1부 구현됨(U19a1, 2026-11-23). 설계 근거: [ADR-031](../adr/ADR-031-watcher.md). 주장: V-WATCH-1. 코드: `crates/watcher/src`, 시험 `crates/watcher/tests`, 적합성 시험 `crates/watcher-conformance/tests`, 검사 스크립트 `tools/ci/watcher_independent.sh`.

워처는 OMS와 코드를 나누지 않는 별도 프로그램이다. 저널을 자기 읽기로 읽고(이 문서와 [C-TAIL](tail.md), [ADR-011](../adr/ADR-011-file-format.md)만으로 썼다), 자기 키와 클라이언트로 Bybit를 조회해 OMS와 비교하고, 확정된 차이를 `admin.sock`에 `watcher_mismatch`로 보내고, `monitor.sock`에 하트비트를 보낸다. **U19a1까지는 저널 읽기와 독립성 검사만 있다. 검사(W1~W4), Bybit 클라이언트, 하트비트는 아직 없어 워처는 아직 아무것도 감시하지 않는다.** 바이너리는 `watcher --version`에만 답하고, 그 밖의 호출은 "not built yet: the checks arrive in U19a5–U19a7"를 찍고 코드 2로 끝난다(네트워크도 파일도 건드리지 않는다).

| 부 | 내용 | 단위 |
|---|---|---|
| 1 | 독립성과 그 검사, 저널 읽기, 아는 이벤트, 소수, `Base32Pair` | U19a1(이 문서) |
| 2 | 저널 상태(주문, 열린 주문, 체결, 포지션, 보류, 기준일), 재생 속도 | U19a2(아직 없음) |
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
- 재생 시간은 저널 길이에 비례한다. 측정은 U19a2(C-WATCH 2부)에 있다. 재생 중에는 하트비트가 없다(소유자 질문 9).
- 세그먼트 목록을 읽는 순간과 파일을 여는 순간 사이의 경합은 시험하지 않았다. 저널이 세그먼트를 지우는 것은 복구 수리가 내구 번호 위의 꼬리 세그먼트를 지울 때뿐이다(`crates/oms-journal/src/segment.rs`의 `r.remove`; 그 밖에는 U-04). 그 세그먼트의 프레임은 상한 위라 읽지 않았으므로, 그 사이의 변화는 새 세그먼트나 사라진 꼬리 세그먼트이고, 둘 다 없는 세그먼트(다시 읽음)로 나타난다(검토 1 N3).
- 한 번의 `read`는 기록 수(`max_records`, 최대 10,000)로만 묶이고 바이트로는 묶이지 않는다. 기록 하나가 16 MiB까지이므로 한 호출이 붙잡는 메모리의 이론상 상한은 매우 크다. 실제 OMS 기록은 작다. 워처의 메모리 한도를 정하는 뒷 부분이 호출당 바이트 예산을 더할 수 있다(검토 1 N2).

### 검증

`journal::frames_and_headers_follow_adr_011`, `journal::schema_1_and_2_are_read`, `journal::the_reader_never_returns_a_record_above_the_durable_cap`, `journal::the_reader_follows_segment_rotation_and_a_schema_change`, `journal::a_header_only_segment_rewritten_with_a_new_schema_is_read_again`, `journal::a_damaged_frame_at_or_below_the_cap_stops_without_moving`(짧은 프레임, 짧은 프레임 머리, CRC 필드·payload·번호 필드의 비트 뒤집기, 맞는 payload 아래의 틀린 프레임 번호, CRC가 맞는 틀린 번호, CRC가 맞는 프레임 안의 JSON이 아닌 기록·시각이 뒤로 가는 기록·`events`가 없는 기록(검토 1 S3), 없는 세그먼트, 줄어든 세그먼트), `journal::an_unknown_event_stops_the_reader`, `journal::io_damage_and_no_start_are_retried_and_the_other_errors_latch`, `journal::a_large_segment_is_read_frame_by_frame_in_bounded_calls`(기록 25,000개 세그먼트의 마지막 기록 손상을 셋째 호출 뒤에야 본다), `events::the_known_events_are_the_rows_of_the_tail_table`, `decimal::decimals_parse_exactly_and_refuse_other_forms`, `wire_id::base32_pair_matches_the_documented_vector`, `binary::version_prints_and_anything_else_exits_2`. 적합성: `conformance::the_watcher_base32_pair_equals_the_core_form`(가장자리 값과 고정 시드 10,000쌍, 잘못된 문자열 23개를 두 디코더가 똑같이 거절), `conformance::the_watcher_reads_while_the_oms_writer_holds_the_lock`(코어가 결정한 기록을 `oms-journal::Writer`가 512바이트 세그먼트로 쓰고 잠금을 쥔 동안 워처가 모두 같은 이벤트로 읽고, 디렉터리의 목록·길이·수정 시각이 그대로이며, 잠금이 여전히 쓰는 쪽에 있다). 독립성: `tools/ci/selftest.sh`의 워처 22건과 실제 트리의 `tools/ci/watcher_independent.sh`(주장 V-WATCH-1). 변이: M182(상한 + 1 기록), M183(프레임 CRC 생략), M184(모르는 이벤트 건너뜀), M221(`Io` 래치), M230(기록 검사 전에 위치를 옮김, 검토 1 S3).
