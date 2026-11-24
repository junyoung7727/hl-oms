# C-WATCH 독립 워처

상태: 1~3부 구현됨(U19a1, U19a2, U19a3, 2026-11-23), 4부 구현됨(설정과 자격 파일 U19a4, 시작 확인과 IP 재확인 U19a4b, 2026-11-23), 5부 구현됨(검사 엔진과 W1, U19a5, 2026-11-23), 6부 구현됨(W2 포지션, U19a6, 2026-11-23), 7부 구현됨(OMS 연결과 실행, U19a7, 2026-11-23), 8부 구현됨(W4 체결, U19a8, 2026-11-23), 9부 구현됨(W3 지갑과 증거금, W2의 대신 시각, 예산, U19a9, 2026-11-23), 10부 구현됨(직접 알림의 설정, 토큰 파일과 전달 규칙 U19b1a, 2026-11-24; 송신 태스크 U19b1b, 2026-11-24; P1 조건과 실행 연결 U19b1c, 2026-11-24), 11부 앞부분 구현됨(서명된 일괄 취소 POST, `[kill]`의 키 규칙과 루프 필드, 킬 알림의 문구와 포지션 묶음 U19b2a, 2026-11-24; sans-IO 루프 `KillLoop` U19b2b, 2026-11-24; 아직 아무것도 일괄 취소를 보내거나 루프를 돌리지 않음). 설계 근거: [ADR-031](../adr/ADR-031-watcher.md), 10~12부는 [ADR-032](../adr/ADR-032-watcher-kill-and-alerts.md). 주장: V-WATCH-1, V-WATCH-2, V-WATCH-3, V-WATCH-4, V-WATCH-5, V-WATCH-6, V-WATCH-7, V-WATCH-8, V-WATCH-9, V-WATCH-10, V-WATCH-11, V-WATCH-12. 코드: `crates/watcher/src`, 시험 `crates/watcher/tests`, 적합성 시험 `crates/watcher-conformance/tests`, 검사 스크립트 `tools/ci/watcher_independent.sh`.

워처는 OMS와 코드를 나누지 않는 별도 프로그램이다. 저널을 자기 읽기로 읽고(이 문서와 [C-TAIL](tail.md), [ADR-011](../adr/ADR-011-file-format.md)만으로 썼다), 자기 키와 클라이언트로 Bybit를 조회해 OMS와 비교하고, 확정된 차이를 `admin.sock`에 `watcher_mismatch`로 보내고, `monitor.sock`에 하트비트를 보낸다. **U19a7부터 워처는 돈다**: `watcher --config <path>`가 설정과 자격 파일을 읽고, 시작 확인을 거쳐 OMS의 `/api/status`를 기다린 뒤 저널을 처음부터 재생하고, W1(열린 주문), W2(포지션), U19a8부터 W4(체결), U19a9부터 W3(지갑과 증거금)을 돌려 확정된 차이를 `admin.sock`에 `watcher_mismatch`로 보내고(다시 보내기 규칙, 7부), `heartbeat_ms`마다 `monitor.sock`에 하트비트를 보내고, 지표를 VictoriaMetrics로 밀어 넣는다. 네 검사가 모두 돈다(U19a9). loopback 가짜와 시험 저널로만 시험했고 실제 OMS 프로세스나 실제 Bybit에 붙여 돌려 본 적이 없다(키가 없다, U-01). 서비스 설치는 U20b다.

| 부 | 내용 | 단위 |
|---|---|---|
| 1 | 독립성과 그 검사, 저널 읽기, 아는 이벤트, 소수, `Base32Pair` | U19a1(이 문서) |
| 2 | 저널 상태(주문, 열린 주문, 체결, 포지션, 보류, 기준일), 재생 속도 | U19a2(이 문서) |
| 3 | Bybit 서명과 클라이언트, 응답 분류, 요청 제한, 지표 | U19a3(이 문서) |
| 4 | 설정과 자격 파일, 시작 확인과 IP 재확인(이 문서) | U19a4, U19a4b |
| 5 | 검사 엔진(일정, 상태 맞추기, 라운드의 저널 읽기, 기한, 두 라운드 규칙)과 W1 열린 주문(이 문서) | U19a5 |
| 6 | W2 포지션(거래소 쪽, 기호별 차이, 두 변경 시각과 미룸, 거래소 관찰 시각 `seen_at`)(이 문서) | U19a6 |
| 7 | OMS 연결(`/api/status`, `admin.sock`의 다시 보내기 규칙, `monitor.sock` 하트비트)과 실행, 바이너리(이 문서) | U19a7 |
| 8 | W4 체결(훑기, 쪽, 다시 시작, 워터마크, 형별 행 읽기, `execType` 표, 거둠)(이 문서) | U19a8 |
| 9 | W3 지갑과 증거금, W2의 대신 시각(W4의 `execTime`), 하트비트의 W3·W4, 예산 측정(이 문서) | U19a9 |
| 10 | 직접 알림 `[WATCH P1]`: `[telegram]`, 토큰 파일, 전달 규칙(분할, 보관함, 응답의 결과, 반복 규칙), 송신 태스크(경로, 깨우기와 흘려보내기 간격, 지표, 종료 때 흘려보내기), P1 조건(불일치, 키 거부, 공인 IP, 시작 거부)과 실행 연결(이 문서) | U19b1a~c |
| 11 | 워처의 일괄 취소 확인 루프: 서명된 POST 하나(일괄 취소), `[kill]`이 있을 때의 키 규칙, `[kill]`의 루프 필드, 킬 알림의 문구와 포지션 묶음, 포지션 읽기, 범위 linear/USDT, sans-IO 루프 `KillLoop`(C2~C5, 일괄 취소 예산, 기한, 알림 규칙, 중단)(이 문서). 세기와 `Inflight`는 U19b2c | U19b2a~c |

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

- GET과 POST 하나를 서명한다. GET: `X-BAPI-SIGN = hex(HMAC_SHA256(secret, timestamp + api_key + recv_window + query))`. `query`는 URL에 보내는 문자열과 바이트까지 같다. POST(U19b2a): 질의 자리에 보내는 본문의 바이트가 온다(`sign::post_prehash`). 서명된 POST는 킬 루프의 일괄 취소 하나뿐이고 경로와 본문이 상수다(`Client::cancel_all`, 11부). 일반 POST는 없다.
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

- 어느 10초 창에서든 요청은 60개까지다. 모든 Bybit 경로와 IP 반향을 합친다(판단(모델 아님); 최악의 경우와 측정은 9부 "예산"). OMS와 같이 쓰는 IP 한도(5초에 600, 설계 §7.3)를 워처의 폭주 루프에서 지킨다.
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
- 요청 제한에서 기다리는 요청들은 순서 없이 100 ms마다 다시 묻는다. W1의 페이지가 계속 가득 채우면 한 IP 확인이나 W3 요청이 여러 번 질 수 있다. U19a9의 예산 시험(9부)은 기다리는 요청을 먼저 온 것부터 내보내는 모형으로 쟀다. 이미 보낸(가드를 지난) 요청은 그 사이 다른 응답이 키 거절이나 403 멈춤을 걸어도 나간다. 막을 수 없고, 시험은 그 응답 뒤에 묻는 요청과 그때 가드에서 기다리던 요청을 본다.
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
| `[telegram]` `token_file` | 표 없음(testnet만; mainnet은 거부) | 표가 있으면 필수. 절대 경로, `..` 없음, 데이터 디렉터리 밖. 파일의 규칙은 10부 "토큰 파일"(U19b1a) |
| `[kill]`, `[w5]` | 표 없음(`[kill]`은 testnet만; mainnet은 거부), `[w5]`는 기본값 | 11부(루프 필드, U19b2a)와 12부(`marker`, `oms_unit`, `systemd`, `stop_wait_ms`, `[w5]`, U19b3a) |

- 범위는 양 끝을 포함한다. 음수는 TOML을 읽는 단계에서 거부된다.
- mainnet은 `mainnet_confirmed = true`에 더해 `[telegram]`(U19b1a, 10부)과 `[kill]`(U19b3a, 12부)이 있어야 한다.
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
| 2 | `GET /v5/user/query-api` | `isMaster == false`, `userID == sub_account_uid`, `ips`가 문자열 목록이고 IP 주소가 하나 이상, `*` 없음. `[kill]`이 있으면 더해 `readOnly == 0`, `ContractTrade`에 `Order`, `Withdraw` 없음(U19b2a, 11부) | 마스터 키, 다른 `userID`, IP에 묶이지 않은 키(`[]` 또는 `*`가 든 목록), 주소가 하나도 없는 `ips` |
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
- 워처 키에 출금 권한이 없어야 한다(설계 §7.4). U19b2a부터 `[kill]`이 있으면 시작 2단계와 재확인이 `readOnly == 0`, `permissions.ContractTrade`의 `Order`, 어느 목록에도 `Withdraw` 없음을 본다(11부). `[kill]`이 없으면(testnet만, U19b3a부터) `readOnly`와 `permissions`를 보지 않는다: 그 워처는 조회만 하므로 읽기 전용 키도 되고 출금 권한은 사람이 키를 만들 때 확인한다([운영 문서](../operations/watcher.md)). OMS는 자기 키에 대해 보지 않는다.
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
- `poll(now) -> Vec<Action>`, `on_venue(now, req_id, Result<Value, RestError>)`, `on_status(now, req_id, Result<Value, String>)`. `Action`은 `Venue {req_id, path, params}`, `Status {req_id}`, `Mismatch {check, detail}`(U19a7부터 `send_id`도, 7부), `Heartbeat {checks}`다. 엔진은 시계를 읽지도 자지도 않고 아무것도 보내지 않는다. U19a7의 실행 코드가 행동을 실행하고 답을 돌려준다. 시험은 모델 거래소와 수동 시계로 돌린다. 저널은 엔진이 `Tail`로 직접 읽는다(라운드마다 `read` 한 번, `MAX_RECORDS` 10,000건까지).
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
- 확정되면: `watcher_mismatches_total{check}` += 1(정체마다 한 번: 그 뒤 라운드가 같은 정체를 계속 보아도 다시 세지 않는다, 검토 2 N2), `error: confirmed difference: <detail>` 한 줄, 그리고 그 라운드가 새로 확정한 것을 모두 싣는 `Mismatch {check, detail}` 하나. 확정된 차이는 그것을 보지 않는 라운드가 올 때까지 목록에 남는다(로그와 `watcher_differences`에서는 빠진다). U19a5의 엔진은 확정하는 라운드에서 한 번 냈다. U19a7부터는 보내기를 7부의 다시 보내기 규칙이 정한다(확정은 빚이 되고, 보류가 꺼져 있으면 같은 라운드에서 나간다).
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

- (U19a5 당시) 엔진을 돌리는 실행 코드가 없었다. U19a7이 그것을 더했다(7부).
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

- 거래소: `GET /v5/position/list?category=linear&settleCoin=USDT&limit=200`과 `cursor` 쪽들, 라운드에 5쪽까지(판단(모델 아님)). 5쪽째에도 커서가 있으면 라운드 실패(`more than 1000 positions`). 쪽 규칙은 W1과 같다(5부: 커서가 빈 가득 찬 쪽과 방금 보낸 커서는 `unreadable`, 커서가 있는 빈 쪽은 따른다). 행마다 `symbol`(문자열), `size`(소수: 지수, `+`, 9자리 이상의 소수는 거절. 크기는 음수가 아니고 부호는 `side`가 준다: `-`로 시작하는 크기는 `unreadable`), `side`(`Buy` +, `Sell` −, `""`이나 없음은 크기가 0일 때만 0이고 크기가 있으면 `unreadable`, 그 밖의 값은 `unreadable`; 검토 1 S1, OMS의 규칙 `oms-node/src/venue/bybit/query.rs` `signed_size`와 같다), `updatedTime`(ms 문자열, 선택. 숫자로 읽히지 않으면 없는 것으로 본다). 한 기호의 행이 둘 이상이면(헤지 모드, 쪽을 넘어서도) `unreadable`이다(OMS도 헤지 모드를 거부한다, `oms-node/src/venue/bybit/checks.rs:313-326`). `result.list`가 목록이 아니거나 `nextPageCursor`가 문자열이 아니어도 `unreadable`이다.
- 저널: 종목별 포지션 = 체결 + 외부 체결 + 거래소 변경 + 조정(2부, C-TAIL). 주차된 체결은 세지 않는다(소유자 질문 4). 저널의 종목 이름은 거래소의 `symbol`과 같다고 본다(W1과 같다).
- 차이: 어느 한쪽에 있는 기호마다 `d = 거래소 − 저널`. `d = 0`이면 일치, 아니면 차이 `(W2, position, <기호>, d)`. `d`가 소수의 범위(i128, 10^8 배)를 넘으면 값이 `overflow`인 차이다(일치로 읽지 않는다, 검토 1 N1). `d`가 정체의 값이라 다른 `d`는 다른 차이이고 처음부터 센다. `detail`: `W2 position <기호>: venue <거래소 크기>, journal <저널 포지션>; seen 2 rounds, journal entry <a> s old`(`<a>`는 아래 두 시각 중 새 것의 나이; 둘 다 모르면 `change time unknown`).

### 두 변경 시각과 미룸

- 두 시각을 본다. 저널의 `last_change_at`: 그 종목의 마지막 `fill_booked`·`external_fill_booked`·`venue_position_changed`·`position_adjusted` 기록 시각(2부; µs를 ms로 올림, 실제보다 늙게 보지 않는다). 거래소의 최근 변경 시각(아래). 판단에는 둘 중 알려진 것의 새 것을 쓴다.
- 시각이 **어리다**: 알려져 있고 `now − t < min_age_ms`(기본 2,000ms). **모르는 시각은 늙음**이다: 저널이 그 종목을 바꾼 적이 없음(수동 거래, 옮겨진 포지션, OMS가 보지 못한 체결)과 거래소 시각이 없음 모두(검토 2 M2, 소유자 질문 10). 그래서 저널이 한 번도 바꾸지 않은 종목의 거래소 포지션은 두 번째 라운드에 확정된다. `Option`으로 시각을 가진 구현은 "알려져 있고 어림"을 시험해야 하고 "알려져 있고 늙음"을 시험하면 안 된다.
- 규칙(W1보다 엄격, 설계 R5 SHOULD 2): 시각이 어리면 **첫 sighting이어도 미룸**이다. 라운드는 성공이 아니고 그 차이는 `watcher_differences`에 남는다(다음 라운드가 같은 `d`를 보면 두 번째 sighting이다). 어리지 않고 지난 완료 라운드에 같은 `d`가 있었으면 확정, 아니면 보류 중이고 라운드는 성공한다. 엔진에는 `engine::Logic::young_defers_at_first_sighting()`(기본 거짓, W2만 참) 하나로 들어간다. 일정, 두 라운드 규칙, 확정과 지표는 5부의 엔진 그대로다.
- 체결이 이어지는 동안 W2는 성공하지 않아 `watcher_check_age_s{check="W2"}`만 자라고 하트비트의 W2 시각은 마지막 성공에 머문다. 10초를 넘는 미룸은 OMS의 모니터링 게이트가 다룬다(설계 R5, C-LIVE 2부). W2 미룸만의 지표는 없다(범위).

### 거래소 관찰 시각 `seen_at`(검토 3 S1)

- `settleCoin=USDT`이고 `symbol`이 없으면 `position/list`는 크기가 0보다 큰 행만 준다(gateway facts Q12.1). 그래서 막 0이 된 포지션에는 행도 `updatedTime`도 없다. 바로 닫힌 순간에 그렇다. W2는 기호마다 `{크기, seen_at}`를 둔다: 모든 쪽을 읽은 마지막 라운드의 부호 있는 크기(행이 없으면 0)와, 그 크기를 처음 본 라운드의 시각(그 라운드의 비교 시계, 5부 "성공").
- 모든 쪽을 읽은 첫 라운드의 크기는 `seen_at`이 없다. 시작 때 찾은 상태이지 본 변경이 아니다. 그 뒤 다른 크기를 읽은 라운드가 `seen_at`을 자기 시각으로 한다.
- 모든 쪽을 읽은 뒤 비교 전에 실패한 라운드(`/api/status` 무응답, 저널 오류, 상태 요청의 기한)도 크기를 남기고, 시각은 마지막 거래소 답의 시계다(검토 4 N2). OMS가 내려가 있는 동안 본 변경은 OMS가 다시 답할 때 이미 늙었고 다시 어려지지 않는다. 남기는 일은 다음 라운드의 첫 요청에서 한다(그 사이 아무도 이 목록을 읽지 않는다). 쪽 중간에 실패했거나 읽을 수 없는 쪽이 있던 라운드는 아무것도 남기지 않는다(시험 `w2::a_round_that_failed_between_pages_leaves_no_observation`, 검토 1 S2).
- 거래소의 최근 변경 시각 = 행의 `updatedTime`(있고 읽히면), 아니면 `seen_at`. U19a9부터 W4가 마지막으로 맞춘 그 기호의 `execTime`이 그보다 새면 그것을 쓴다(9부). 셋 다 없을 때만(첫 라운드부터 크기가 그대로이고 `updatedTime`도 W4의 시각도 없음) 모름, 곧 늙음이다.
- 그래서 거래소에서 닫힌 포지션(t에 닫힘, 그것을 처음 0으로 본 라운드 r ≤ t + 1초)은 `now − r < 2초`인 동안 미룬다. `updatedTime` 있는 0 아닌 행과 같다. `updatedTime` 없이 라운드마다 크기가 바뀌면 라운드마다 어려 미루므로, 아무것도 확정하지 않는 동안 W2가 신선한 성공을 보고하지 않고 게이트가 10초에 닫힌다.
- 목록에는 거래소가 완전한 읽기에서 보인 모든 기호가 하나씩 있고 거래소의 linear 기호 수로 묶인다(판단(모델 아님): 수백 개). 저널에만 있는 기호는 항목이 없고, 그것은 `{0, 없음}` 항목과 같다(검토 1 N2).

### 3초 완료 기준

- 수동 시계와 지연 0의 답에서, 체결이 멈춘 뒤 심은 차이(`updatedTime` = 심은 순간, 저널은 따라가지 않음)는 라운드 시작(거래소 읽기 전)에 심으면 +2,000ms, 라운드의 거래소 읽기 1ms 뒤에 심으면 +2,999ms에 확정된다. 첫 라운드는 어려서 미루고, 나이가 2초 이상인 첫 라운드가 지난 라운드와 같은 `d`로 확정한다. 요청 지연은 수동 시계에서 0이고 기준에 들지 않는다. `updatedTime`이 없으면 `seen_at`(변경 뒤 많아야 한 간격)이 쓰여 확정은 변경 뒤 많아야 1,000 + 2,000ms다.

### 지표(6부)

새 지표는 없다. W2는 5부의 `watcher_check_age_s{check="W2"}`, `watcher_differences{check="W2"}`, `watcher_mismatches_total{check="W2"}`, `watcher_mismatch_active{check="W2"}`와 3부의 `watcher_request_errors_total{endpoint="/v5/position/list",class="unreadable"}`를 쓴다.

### 알려진 한계(6부)

- `Engine::new`는 지금도 W1만 더하고(5부), 실행 코드가 `runtime::checks`로 W2(W4의 `execTime` 지도와 함께, U19a9), W3, W4를 더한다(7부, 9부; 종단 시험 `runtime::the_watcher_runs_against_loopback_fakes_end_to_end`가 W2 요청과 하트비트의 W2 시각을 본다).
- 호스트 시계가 거래소보다 `min_age_ms`(2초) 이상 앞서면 모든 거래소 시각(`updatedTime`)이 늙어 보인다(`recv_window` 5초 안이면 그런 시계도 서명을 통과한다). 효과는 더 이른 확정이지 놓친 확정이 아니다(검토 3 N7). 시계가 뒤로 가는 한계(5부)와 함께 본다.
- `updatedTime`의 뜻("Position updated timestamp (ms)", gateway facts Q12.7)은 문서에서만 왔다. 크기가 아닌 것(가격, 증거금, 펀딩)이 그것을 옮기는지는 모른다. 옮긴다면 미룸이 늘 뿐 확정을 놓치지는 않는다.
- 라운드 도중 포지션이 바뀌어 한 기호가 두 쪽에 실리면 헤지 모드와 구별하지 못해 그 라운드는 `unreadable`이다(다음 라운드가 다시 읽는다; 판단(모델 아님): 쪽이 둘 이상인 계좌, 곧 200개 넘는 포지션에서만 생긴다).
- `updatedTime`이 있으면 `seen_at`보다 오래되어도 그것을 쓴다(스펙의 규칙). 실제 `updatedTime`이 크기 변경에 움직이지 않거나 `"0"`이면 막 바뀐 크기를 늙게 보아 첫 sighting의 미룸을 건너뛰고, 차이는 한 라운드 뒤 확정한다(OMS가 1초 넘게 늦게 기록하면 거짓 보류; 검토 1 N4). testnet에서 확인할 항목이다.
- `positionIdx`는 읽지 않는다. 헤지 모드에서 한쪽만 열린 계좌는 행 하나라 통과하지만 부호 있는 크기는 맞고, OMS는 시작 때 헤지 모드를 거부한다(`checks.rs:319-326`). `side`가 문자열 `"None"`이면 `unreadable`이다(안전한 쪽). Bybit가 이 계좌 종류에 그 문자열을 보내는지는 확인하지 않았다(검토 1 N6).
- `detail`의 `journal entry <a> s old`는 새 시각이 거래소의 것일 때도 그 말을 쓴다(5부의 형식; 운영자에게 보이는 문구는 U19a7에서 정한다, 검토 1 N3).
- 시험 도구의 답은 지연 0이라 시험은 비교 시계와 마지막 거래소 답의 시계를 구별하지 못한다(검토 1 N5).
- W2는 크기만 비교한다(진입가, 레버리지, 증거금은 아니다).
- 주차된 체결은 세지 않으므로 약 3초 넘게 주차된 체결은 W2 확정을 부를 수 있다(소유자 질문 4; 보류는 안전한 쪽).
- 실제 Bybit의 `position/list` 응답으로 돌려 본 적이 없다(키가 없다, U-01). 행의 모양은 gateway facts Q12와 스펙에서 왔다.

### 검증(6부)

`crates/watcher/tests/w2.rs` 14개(5부의 엔진 시험 도구; 모델 거래소에 포지션 목록과 쪽마다의 스크립트, 상태 무응답을 더했다). 완료 기준: `w2::five_fills_per_second_for_60_s_give_no_mismatch`(모델 거래소가 200ms마다 체결하고 시험 저널이 300ms 뒤에 기록, 판단(모델 아님); 60초 동안 `Mismatch` 0개, W2 성공은 T0에 머물고 `watcher_check_age_s{W2}` = 60, 체결이 멈추면 다시 성공), `w2::a_difference_planted_after_the_fills_stop_is_confirmed_within_3_s`(라운드의 위상 0~900ms를 100ms 간격으로, 그리고 읽기 1ms 뒤; 확정은 +2,000~+3,000ms이고 위상마다 정확한 값). 그 밖에 `w2::a_changing_difference_restarts_the_count`(거래소가 모르는 저널 포지션 포함), `w2::a_deferred_round_is_not_a_success`(하트비트의 W2 시각), `w2::a_venue_position_in_a_symbol_the_journal_never_changed_is_confirmed_on_the_second_round`(검토 2 M2; 요청 매개변수), `w2::a_young_journal_change_defers_even_at_the_first_sighting`(정확히 2초는 늙음, 1µs 모자라면 어림), `w2::hedge_mode_rows_are_unreadable`(한 쪽, 두 쪽), `w2::bad_pages_and_rows_fail_the_round`(커서 없는 가득 찬 쪽, 같은 커서, 모르는 방향, 지수 크기, 기호 없음, 크기 있는 빈 방향과 없는 방향, 음수 크기(검토 1 S1), 6쪽, 거래소 오류, 크기 0인 빈 방향, 커서 있는 빈 쪽), `w2::a_closed_venue_position_defers_until_2_s_after_it_was_first_seen_flat`, `w2::a_difference_that_changes_every_round_without_a_venue_time_is_not_a_success`(검토 3 S1), `w2::sizes_found_at_the_first_round_have_no_seen_time`(무응답과 읽을 수 없는 쪽 뒤의 첫 읽기), `w2::a_size_seen_by_a_round_that_then_fails_keeps_its_seen_time`(검토 4 N2), `w2::a_round_that_failed_between_pages_leaves_no_observation`(검토 1 S2), `w2::a_difference_too_large_to_subtract_is_still_a_difference`(검토 1 N1). `engine::status_answers_in_reverse_order_are_each_used_by_their_own_round`는 U19a6부터 W1과 실제 W2로 돈다. 변이: M196(W2가 다른 두 값을 확정; 고의 결함 15), M197(미룬 W2 라운드를 성공으로; 16), M218(저널 시각이 없으면 어림으로; 37), M223(행이 없는 기호에 `seen_at`이 없음; 42), M243(크기 있는 빈 방향을 포지션 없음으로; 검토 1 S1), M244(커서 있는 쪽까지의 읽기를 완전한 읽기로; 검토 1 S2), M245(범위를 넘은 차이를 일치로; 검토 1 N1).

## 7부. OMS 연결과 실행(U19a7)

### `/api/status`(`oms::Status`)

- `GET status_url`. 주소는 loopback만이라(4부) `Host` 머리글은 `127.0.0.1:<port>` 또는 `[::1]:<port>`이고 대시보드의 규칙에 맞는다. 시간 한도 `request_timeout_ms`, 리다이렉트와 프록시 없음. 2xx가 아니거나 본문이 JSON이 아니면 실패다. 읽는 것은 `journal.durable_seq`와 `core.instance_id`뿐이다(5부의 엔진, 시작 때의 기다림).

### `admin.sock`의 `watcher_mismatch`(`oms::admin_send`)

- 보낼 때마다 연결 하나: 연결, `{"id":1,"op":"hello","client":"watcher/<name>"}`와 그 답, `{"id":2,"op":"watcher_mismatch","check":"W<n>","detail":"…"}`와 그 답, 닫음. 답마다 `request_timeout_ms`, 보내기 전체는 2 × `request_timeout_ms`(가장 큰 설정에서 20초). 결과는 `result`(`"held"`, `"already held"`) 또는 `<code>: <message>`(오류 답), 시간 초과, `not sent: …`(소켓이 없거나 거절).

### 다시 보내기 규칙(엔진, sans-IO)

- **빚**(검토 2 S2, 검토 4 S2): 확정된 차이는 그것을 실은 보내기가 `"held"`로 답할 때까지 빚이다. `"already held"`(다른 보내기가 건 보류이고 그 메모는 이 차이를 이름하지 않는다, `holds.md`), 보류를 보이는 저널 상태, 실패는 갚지 않는다. 빚은 라운드가 더는 보지 않아도(주문이 닫힘, 포지션이 맞춰짐) 확정 때의 `detail`로 보낸다.
- **보류 꺼짐**(검사 `W<n>`마다): 엔진의 저널 상태가 `holds.watcher_mismatch = false`이고, `W<n>`의 보내기가 `"held"`나 `"already held"`로 답한 적이 있으면 그 상태가 가장 최근의 그런 답 **뒤에 낸** 상태 요청의 상한까지 읽은 것일 때. 답은 `hold_set`이 내구화된 뒤에 오므로 그런 상한은 그것을 포함하고, 그 상한에서 꺼진 보류는 그 뒤 운영자가 푼 것이다. 답 앞에 낸 상태 요청의 상한은 아직 `hold_set`이 없어 꺼져 보일 수 있으므로 증거가 되지 않는다(시험 `admin::a_hold_seen_off_at_a_cap_sampled_before_the_answer_does_not_send`).
- **때가 됨**: 보류 꺼짐, 빚이 있거나 (W1·W2·W3만) 라운드가 아직 보는 확정된 차이가 있음, 지난 보내기의 시작(엔진이 그것을 낸 시각)부터 `mismatch_resend_ms`(기본 10초, 판단(모델 아님))가 지남 또는 지난 보내기가 없음, 그리고 그 검사의 보내기가 진행 중이 아님. 보내기 하나가 그것을 모두 싣는다: 빚을 확정한 순서로 먼저, 그다음 아직 보는 확정된 차이. `detail`은 첫 것 뒤에 `"; and N more"`, 1024바이트에서 자른다. 엔진은 `poll`과 상한까지 읽은 상태 답 뒤에 이것을 본다.
- **진행 중 하나**(검토 4 S6): 답도 실패도 없는 보내기가 있으면 그 검사의 다음 보내기는 시작하지 않는다. 엔진은 낸 시각 + 2 × `request_timeout_ms` + 1초(판단(모델 아님)) 안에 보고가 없으면 실패로 본다(`warn: W<n> mismatch send has no report before its deadline (taken as failed)`): 잃은 보고 하나가 그 검사의 보내기를 막지 않는다. 보내기마다 번호가 있다(`Mismatch {check, send_id, detail}`, `on_admin(now, check, send_id, 답)`; 검토 1 S1). 엔진은 진행 중인 보내기의 번호를 단 답만 받으므로 `"held"`는 그 보내기가 실은 것만 갚는다. 기한이 지난 보내기의 늦은 보고(벽시계가 앞으로 뛰거나 루프가 1초 넘게 멈추면 클라이언트의 단조 시계 한도보다 먼저 엔진의 기한이 지난다)는 `debug: an answer to W<n> send <id>, which is not the send in flight, is ignored` 한 줄과 함께 버리고 아무것도 갚지 않는다. 그 뒤의 보내기가 더 많이 실었어도 그렇다(시험 `admin::a_late_report_of_one_send_pays_only_what_that_send_carried`).
- **답**: `"held"` → 실은 것을 모두 갚는다. `"already held"` → 아무것도 갚지 않고, 보류가 꺼질 때까지 기다린다. `bad_request` → `error: W<n> mismatch send refused (bad_request: …): a watcher bug` 한 줄, 같은 속도로 다시. `overloaded`, `unavailable`, `unauthorized`, `internal`, 그 밖의 코드, 시간 초과, 없는 소켓 → `warn: W<n> mismatch send failed: …` 한 줄, 같은 속도로 다시(검토 4 N4). 진행 중 보내기가 없거나 번호가 다른 답은 `debug:` 한 줄과 함께 버린다(검토 1 S1).
- 그래서 W1~W3은 차이가 남아 있는 동안 운영자가 `clear-hold`로 풀면 보류를 다시 건다(소유자 질문 7, 많아야 `mismatch_resend_ms`마다). 이미 걸린 보류 아래에서 확정된 차이는 그 보류가 풀린 뒤 한 번 보내지고, 새 보류의 메모가 그것을 이름한다(OMS는 첫 메모만 남긴다). W4는 갚은 정체를 해제 때 거둔다(8부).
- `watcher_mismatch_active{check}` = 라운드가 아직 보는 확정된 차이와 빚의 합집합의 크기(검토 4 S1). 차이가 끝나고 갚아지면 0으로 돌아온다.

### `monitor.sock` 하트비트(`oms::Monitor`)

- 연결 하나를 유지한다. 없으면 하트비트 때 연결하고 `hello`(전략 없음, `client` `watcher/<name>`)의 답을 받은 뒤 `{"id":n,"op":"heartbeat","watcher":"<name>","checks":{"W1":<ms|null>,"W2":…,"W3":…,"W4":…}}`를 보낸다. 값은 엔진의 마지막 성공 시각(5부 "성공", ms)이고 성공이 없으면 `null`이다. W4는 U19a8부터, W3은 U19a9부터 그 검사의 성공 시각이다(9부).
- `"recorded"` → `watcher_heartbeat_ok_ts` = 그 하트비트의 시계(epoch 초).
- 연결 실패(소켓 없음: `[core.policy.monitor]`가 없는 OMS, 거절), 끊김, `request_timeout_ms` 안에 답 없음 → 연결을 버리고 다음 하트비트에 다시 연다(검토 4 N4). 그래서 연결 시도는 하트비트마다 많아야 한 번이고, 속도는 `heartbeat_ms`마다 하나로 연결마다 초당 둘(`api.md`)보다 한참 낮다. 줄은 상태가 바뀔 때만 하나다(`monitor.sock <path> connected`, `warn: monitor.sock <path> not connected: <이유>; tried again at the next heartbeat`; 검토 3 N6).
- 오류 답(`bad_request`: 예를 들어 OMS의 시계보다 앞선 검사 시각, `overloaded`, `unavailable`, `unauthorized`, `internal`) → 연결은 그대로, `error: monitor.sock heartbeat refused (…)` 줄은 1분에 하나까지.
- 앞의 하트비트를 아직 보내는 중이면 새 것은 버린다(쌓지 않는다, `debug:` 한 줄).
- 첫 하트비트(`start_ms`)는 아직 끝난 라운드가 없어 모두 `null`이다. 게이트를 열 수 있는 첫 하트비트는 그다음, `heartbeat_ms` 뒤다(검토 3 N6).
- 검사가 모두 실패해도 하트비트는 간다. 그러면 게이트는 오래된 W1·W2를 보고 닫힌다(의도). OMS는 하트비트의 기록 시각에서 W1·W2를 판단하므로(C-LIVE 2부), 게이트는 W1이나 W2의 시각이 10초보다 오래된 첫 하트비트에서 닫힌다: 마지막 성공 뒤 10초에서 10 + `heartbeat_ms`초, 기본값에서 **10~15초**(검토 4 N1).

### 실행(`runtime::run`)과 바이너리

- `run(cfg, Endpoints {rest_base, ip_echo_url}, clock, log, shutdown) -> 종료 코드`. 바이너리는 두 끝점을 설정에서만 얻는다(REST 기반 주소는 `network`에서, 덮어쓰기 없음). 시험은 loopback 가짜 끝점으로 부른다.
- **시작 순서**: 설정 시각 `boot_ms` → 자격 파일(4부) → 지표 밀어 넣기 타이머(`[telemetry] vm_url`이 있으면 `export_ms`마다, 여기서부터; 검토 2 S4) → 시작 확인(4부의 `Start`) → `/api/status` 기다림(읽을 수 있는 답까지, 실패하면 1초 뒤 다시, 줄은 10초에 하나까지, 판단(모델 아님)) → 기록 1부터 그 답의 상한까지 재생(`Tail::read` 한 번마다 루프에 양보) → 엔진(`Engine::new`가 W1을, 실행이 `add_check`로 W2와 W4(U19a8)를 더한다, 시작 = 재생을 마친 시각). 하트비트는 엔진과 함께 시작한다.
- IP 재확인(4부의 `IpCheck`)은 시작 확인이 끝난 뒤(`Done`, 또는 5단계 키 거절 뒤의 반향만) 계속 돈다. `/api/status`를 기다리는 동안과 재생 중에도 돌아 OMS가 내려가 있어도 `watcher_public_ip_ok`와 `watcher_api_key_days_left`가 새것이다(검토 3 N2).
- 엔진이 돌기 전에는 실행이 `watcher_check_age_s{W1}`, `{W2}`, `{W4}`(U19a8)를 `boot_ms`부터 센다(검토 3 N2). 재생 중 `watcher_journal_seq`를 쓴다.
- 시작 2단계나 5단계에서 키가 거절되면(`StayUpNoRequests`) 시작 확인은 거기 머물고, 실행은 상태 기다림, 재생, 엔진으로 간다. 클라이언트가 모든 Bybit 요청을 보내지 않고 거절하므로 라운드는 실패하고, 하트비트는 `null`이나 오래된 시각을 실어 게이트가 닫힌다(판단: 저널 지표와 하트비트가 워처가 살아 있음을 보인다).
- 재생 중 다시 읽는 오류(`Io`, `Damaged`, `NoStart`)나 상한 아래에서 끝난 파일 → 1초 뒤 같은 자리에서 다시(줄은 상태가 바뀔 때 하나, `watcher_journal_error = 1`). 래치 오류 → 엔진을 시작하고, 엔진의 첫 라운드가 래치한다(5부).
- **루프**: 단일 스레드 tokio. 답이 올 때와 100ms(판단(모델 아님))마다 깨어나 각 부분에 무엇이 때가 됐는지 묻는다(재생 중에는 기다리지 않는다). 요청마다 자기 태스크이고 답은 루프로 돌아온다. 진행 중 요청은 검사마다 하나(엔진), 상태, 관리 보내기(검사마다 하나), 하트비트 하나, 시작이나 재확인 하나, 밀어 넣기 하나다.
- **종료**(shutdown 입력; 바이너리에서는 SIGTERM, SIGINT): 새 라운드 없음(엔진을 더 `poll`하지 않는다), 진행 중 요청은 `request_timeout_ms`까지 기다린 뒤 버린다, 마지막 지표 밀어 넣기 하나(`request_timeout_ms`), 모니터 연결을 닫음, 코드 0. 아무것도 저장하지 않는다.
- **거부**: 자격 파일, 시작 규칙(4부) → `error: …` 한 줄, 코드 2. 바이너리: `watcher --config <path>`, `watcher --version`. 그 밖의 인자는 사용법 한 줄과 코드 2, 읽을 수 없거나 틀린 설정 파일은 이유 한 줄과 코드 2(둘 다 네트워크 전).

### 지표(7부)

| 이름 | 종류 | 뜻 |
|---|---|---|
| `watcher_heartbeat_ok_ts` | 게이지 | 마지막 `"recorded"` 답의 하트비트 시각(epoch 초) |
| `watcher_mismatch_active{check}` | 게이지 | 5부의 뜻에 빚을 더한다 |

### 알려진 한계(7부)

- 빚은 메모리에만 있다. 재시작하면 잃고, 새 프로세스는 자기가 다시 확정한 것만 보낸다(`watcher_mismatch_active`도 그렇다).
- `"held"` 답을 잃었는데(시간 초과) OMS가 보류를 기록했으면 그 차이는 다음 해제 뒤에 한 번 더 간다(안전한 쪽). 기한 뒤에 온 늦은 `"held"`도 같다: 버리므로 그 보내기가 실은 것은 갚아지지 않고 다음 해제 뒤에 한 번 더 간다(검토 1 S1).
- 보류가 오래 걸려 있는 동안 빚 목록에는 크기 한도가 없다(검토 1 N4). 새로 확정된 정체마다 하나(정체와 1KiB까지의 `detail`)가 늘고 `"held"` 답에서만 준다. 값이 계속 바뀌는 W2 차이(안정된 값마다 새 정체)는 많아야 두 라운드마다 하나를 더한다: `w2_ms` = 1000에서 하루 많아야 약 43,000개, 43MB(판단(모델 아님)). 한도(앞의 N개만 두고 나머지는 세기)는 두지 않았다: 버린 빚은 어느 보류 메모에도 남지 않는다.
- 보류 꺼짐은 검사마다 판단한다(스펙). 다른 검사의 보내기가 건 보류를 저널 상태가 아직 보이지 않으면 이 검사가 보내 `"already held"`를 받는다. 아무것도 갚지 않고 보류가 꺼지기를 기다리므로 남는 것은 보내기 하나다.
- `admin.sock`과 `monitor.sock`은 노드 자신의 uid만 받는다(`oms-node/src/server.rs:188-191`). 그래서 워처는 OMS의 uid로 돌고, 두 프로세스는 서로의 자격 파일을 읽을 수 있다. U20b에 넘기는 한계다(검토 1 N2).
- 재생 중에는 하트비트가 없다(소유자 질문 9). 재생이 30초를 넘으면 게이트가 닫힌다(2부의 측정: 100만 건에 약 1.3초).
- 시계가 뒤로 가면 다시 보내기 속도, 보내기 기한, 지표 밀어 넣기 타이머도 5부의 타이머처럼 시계가 옛 값을 지날 때까지 멈춘다.
- 시그널에서 shutdown 입력으로의 연결(`main.rs`, 몇 줄)은 시험하지 않았다. 시험 프로세스에 시그널을 보내면 그 시험 바이너리의 모든 시험이 맞기 때문이다.
- `runtime::the_shutdown_input_ends_cleanly`의 두 단언 "모니터 연결이 닫힘"과 "종료 입력 뒤 라운드 없음"은 `run`이 돌아오고 시험 스레드가 tokio 실행기를 버린 뒤에 본다. 그래서 `Runtime::stop`이 그 일을 하지 않아도 성립한다. 진행 중 요청을 `request_timeout_ms`까지 기다리는 것도 단언하지 않는다. 단언하는 것은 마지막 밀어 넣기 하나와 코드 0이다(검토 1 N3).
- `detail`의 형식(`journal entry <a> s old`, 6부 검토 1 N3)은 스펙의 형식 그대로다. 운영자가 읽는 뜻은 런북 7.13에 있다.
- 실행은 loopback 가짜(Bybit, 반향, `/api/status`, VictoriaMetrics, 두 소켓)와 시험 저널로만 시험했다. 실제 oms-node 프로세스나 실제 Bybit에 붙여 돌려 본 적이 없다(키가 없다, U-01). 서비스 설치는 U20b다.

### 검증(7부)

`crates/watcher/tests/admin.rs` 7개(엔진 시험 도구의 OMS가 `watcher_mismatch`에 답하고 `hold_set`을 쓴다; 가짜 `admin.sock`), `crates/watcher/tests/heartbeat.rs` 3개(가짜 `monitor.sock`, 엔진의 하트비트를 실제 클라이언트로), `crates/watcher/tests/runtime.rs` 9개(`run`을 loopback 가짜에; 종단 시험은 실제 시간, 나머지는 수동 시계), `crates/watcher/tests/binary.rs` 1개. 완료 기준: `heartbeat::heartbeats_carry_the_success_time_of_each_check`(첫 하트비트는 모두 `null`, 그 뒤 W1·W2 = 엔진의 성공 시각, W2가 실패한 뒤 W2 시각은 그대로). 그 밖에 `admin::a_confirmed_mismatch_is_sent_once_and_again_after_a_clear`, `admin::a_confirmed_mismatch_is_sent_until_answered_even_after_the_difference_is_gone`, `admin::a_difference_confirmed_under_a_hold_is_sent_after_the_clear`, `admin::a_second_send_waits_for_the_first_answer`, `admin::a_hold_seen_off_at_a_cap_sampled_before_the_answer_does_not_send`, `admin::a_late_report_of_one_send_pays_only_what_that_send_carried`(검토 1 S1: 기한 뒤의 늦은 `"held"`는 다음 보내기가 실은 것을 갚지 않는다), `admin::the_admin_client_says_hello_and_reads_both_replies`, `heartbeat::the_heartbeat_says_hello_first_and_reconnects`, `heartbeat::a_reply_that_does_not_come_in_time_reopens_the_connection`, `runtime::the_watcher_runs_against_loopback_fakes_end_to_end`, `runtime::without_an_answering_oms_nothing_is_judged`, `runtime::the_shutdown_input_ends_cleanly`, `runtime::the_ip_recheck_runs_while_the_oms_is_down`, `runtime::the_ip_recheck_runs_during_the_boot_replay`(검토 1 S3: 상한 아래에서 끝난 저널로 재생에 머문 채 세 번의 재확인), `runtime::a_recheck_that_fails_a_start_rule_stops_every_bybit_request`(검토 1 S2: 재확인의 마스터 키 → `watcher_auth_rejected 1` 밀어 넣기, 그 뒤 Bybit 요청 없음), `runtime::a_key_rejected_at_start_is_exported`, `runtime::the_status_client_sends_the_loopback_host_and_follows_no_redirect`(`Location`이 있는 302), `runtime::a_start_refusal_ends_the_run_with_exit_code_2`(마스터 키, 0644 자격 파일), `binary::version_prints_and_anything_else_exits_2`. 변이: M199(하트비트가 W2 시각을 W1로; 고의 결함 18), M200(해제 뒤 다시 보내지 않음; 19), M201(모니터에 `hello` 없이; 20), M220(보지 않게 된 빚을 버림; 39), M226(`"already held"`가 갚음; 45), M228(첫 답 전에 둘째 보내기; 47), M246(늦은 보고가 다음 보내기가 실은 것을 갚음; 검토 1 S1).

U19a7은 U19a5·U19a6 시험 몇 개의 단언을 바꿨다(시험 도구의 OMS가 이제 `watcher_mismatch`에 답하고 `hold_set`을 쓰므로, 첫 보류 아래에서 확정된 둘째 차이는 바로 보내지지 않는다): `w1::a_journal_entry_younger_than_2_s_does_not_alarm`, `engine::a_confirmed_identity_is_counted_once`, `w2::a_changing_difference_restarts_the_count`, `w2::sizes_found_at_the_first_round_have_no_seen_time`, `w2::a_difference_planted_after_the_fills_stop_is_confirmed_within_3_s`는 둘째 확정의 시각을 `Mismatch`가 아니라 확정 줄(`error: confirmed difference`, 시험 도구의 `confirmed`)에서 읽는다. 확정의 시각과 내용은 그대로다.

## 8부. W4 체결(U19a8)

### W4(`w4::W4`, `w4_ms`마다, 기본 5초)

- **훑기**(sweep)는 창 `[start, end]`를 `GET /v5/execution/list?category=linear&settleCoin=USDT&limit=100&startTime=<start>&endTime=<end>`와 그 커서 쪽으로 읽는다. `end` = 훑기의 첫 쪽 때 시계이고 훑기 동안 바뀌지 않는다(새 체결이 커서의 행 집합을 움직이지 않게). `start = max(워터마크 − w4_overlap_ms, ⌈fill_floor / 1000⌉)`(바닥은 µs이고 ms로 **올림**, 검토 1 S7). `end − start`가 7일 이상이면 창을 `end − 7일 + 1ms`부터로 자르고(거래소의 한도, facts Q9.7), `warn: W4 window … is longer than 7 days; cut to the last 7 days` 줄은 상태(잘림/안 잘림)가 바뀔 때만 하나다(돌아오면 `W4 window is 7 days or shorter again`; 검토 1 N6).
- **쪽**: 라운드마다 많아야 `w4_max_pages`(기본 10, 판단(모델 아님))쪽. 커서가 아직 비지 않았으면 라운드는 비교도 성공도 실패도 없이 끝나고(`Page::Pause`), 다음 라운드가 같은 `start`, `end`, 커서로 같은 훑기를 잇는다. 성공 시각은 끝난 훑기에서만 움직이므로 여러 라운드에 걸친 훑기 동안 `watcher_check_age_s{check="W4"}`가 오른다. 쪽마다 `watcher_requests_total` 하나다.
- **훑기의 끝**(검토 1 S3): 비어 있는 첫 쪽, 또는 `nextPageCursor`가 빈 첫 쪽에서 훑기가 끝나고 그 행을 비교한다. 가득 찬 쪽(100행)에 빈 커서도 **W4만** 마지막 쪽으로 받는다(완료 기준 "1,200건 … 10쪽, 그다음 2쪽"은 마지막 커서가 빈 가득 찬 쪽 12개를 필요로 한다; 알려진 한계). 방금 보낸 커서와 같은 `nextPageCursor`는 실패한 쪽이다.
- **실패한 쪽**(클라이언트 오류, `result.list`가 목록이 아님, `nextPageCursor`가 문자열이 아님, 바뀌지 않은 커서)은 라운드를 실패시키고, 다음 라운드가 같은 커서로 같은 쪽을 다시 읽는다. 그 쪽까지 읽은 행은 남는다. 한 훑기에서 실패한 라운드가 연달아 3번(판단(모델 아님))이면 훑기는 같은 `start`와 새 `end`로 1쪽부터 다시 시작한다(거래소가 더는 받지 않는 커서가 W4를 영영 멈추지 않게; 검토 1 S2). `warn:` 줄은 실패가 이어지는 동안 한 번만 쓰고(3라운드마다 다시 시작해도), 그 뒤 쪽을 다시 읽으면 `W4 pages are read again after a restart` 한 줄을 쓴다(U19a8 검토 1 N1, 시험 `w4::the_restart_warn_line_is_logged_once_per_run_of_failed_rounds`). 쪽을 읽은 라운드(멈춤)는 연속을 끊는다. 50라운드(판단(모델 아님); 기본 상한에서 500쪽, 50,000행) 안에 끝나지 않은 훑기도 같은 방식으로 다시 시작하고 `error:` 줄 하나를 쓴다. 끝까지 읽은 훑기가 비교 전에 실패하면(상태, 저널, 기한) 다음 라운드는 새 훑기다(같은 워터마크에서).
- **처음 워터마크**: 첫 훑기 때 `now − w4_initial_lookback_ms`(기본 600초), `⌈fill_floor / 1000⌉`보다 낮지 않게. 시험 도구처럼 엔진이 저널을 읽기 전의 첫 훑기는 바닥 0을 본다(실행에서는 재생이 먼저라 바닥을 안다).
- `settleCoin=USDT`는 OMS가 보내는 대로 보내지만(`query.rs:681`) `execution/list`의 문서화된 매개변수가 아니다(facts Q9.3). W4는 그 거름에 기대지 않는다: 답에 든 USDC 계약의 체결도 다른 행과 같이 분류하고 비교한다(검토 4 N6).

### 행 읽기와 `execType` 표

- 먼저 분류하고, 그 형이 비교하는 필드만 읽는다(검토 1 S2; Funding과 ADL 행의 모양은 시험되지 않았다, 설계 R3 T15).
  - 모든 행: `execId`, `symbol`, `execType`(빈 문자열이 아닌 문자열), `execTime`(ms, 10진 문자열). 키 `<symbol>:<execId>`(`gateway.md:97`).
  - `Trade`, `AdlTrade`, `BustTrade`, `Delivery`, `Settle`: 더해서 `execQty`(소수, `-`로 시작하지 않음)와 `side`(`Buy`/`Sell`).
  - `Funding`과 모르는 형: 더 읽지 않는다.
- 형이 필요로 하는 필드가 없는(또는 읽을 수 없는) 행은 그 자체로 차이 `unreadable_row`다. 주제 = `execId`와 `symbol`을 읽을 수 있으면 키, 아니면 행의 JSON 텍스트를 200바이트(판단(모델 아님))에서 자른 것. 값 = 없는 필드의 이름(쉼표로). `missing_*`처럼 두 훑기 규칙을 따르고, 나이는 `execTime`을 읽을 수 있으면 그것, 아니면 늙음(소유자 질문 10과 같다).
- 바닥(검토 1 S7): 비교 때 `execTime × 1000 < fill_floor`(그 라운드가 비교하는 저널 상태의 바닥)인 행은 무시한다. OMS도 그런 체결을 무시하고(`decide.rs:1596`) 저널은 그것을 갖지 않는다. 바닥과 같은 시각은 비교한다.
- 훑기의 끝 뒤(U19a8 검토 1 S1): `execTime`이 그 훑기의 `end`보다 뒤인 행은 `execTime`을 읽을 수 없는 행으로 본다. 거래소는 `endTime`으로 거르므로 그런 행은 오지 않아야 하지만 W4는 그 거름에 기대지 않는다. 그 행은 `unreadable_row`(값 `execTime`, 나이 늙음, `what`에 그 시각과 `end`)이고 확정될 때까지 워터마크를 붙잡는다. 그래서 워터마크는 그것을 읽은 훑기의 `end`를 넘지 않고, 앞선 시각 하나가 뒤의 모든 창을 `[end, end]`로 만들어 W4를 조용히 멈추게 하지 못한다(시험 `w4::a_row_after_the_sweeps_end_is_an_unreadable_row_and_does_not_blind_w4`).
- 수량 0(U19a8 검토 1 S2): 수량이 0인 `Trade`와 거래소 변경 행은 무시한다. OMS는 그런 체결도 거래소 변경도 기록하지 않는다(`decide.rs` `on_fill`의 `qty <= 0`, `on_venue_change`의 `qty == 0`). 그 행은 포지션을 바꾸지 않고, 비교하면 맞는 저널에서 거짓 보류가 걸린다. 일치한 행이 아니므로 `exec_times()`에도 적지 않는다(시험 `w4::a_trade_or_venue_change_of_quantity_0_is_ignored_as_the_oms_ignores_it`).

| `execType` | 비교 대상 | 차이 | 값 |
|---|---|---|---|
| `Trade` | 저널 체결(`fill_booked`, `external_fill_booked`)을 키로 | 키 없음 `missing_fill`; 수량이나 방향이 다름 `fill_details` | 거래소의 `<side> <execQty>` |
| `AdlTrade`, `BustTrade`, `Delivery`, `Settle` | 저널 거래소 변경을 키로, 종류 `adl`/`takeover`/`delivery`/`settle` | 키 없음 `missing_venue_change`; 종류나 부호 있는 수량이 다름 `venue_change_details` | `<kind> <부호 있는 수량>`(`Sell`은 −) |
| `Funding` | 없음 | 무시(펀딩은 수량을 바꾸지 않는다) | — |
| 그 밖 | — | `unknown_exec_type`: **바로 확정**, 둘째 훑기와 나이 규칙 없음 | `execType` |

- "바로"는 끝난 훑기에서다: 행은 훑기가 끝날 때만 비교하므로 여러 라운드 훑기에서는 확정이 훑기 길이만큼 늦다(검토 2 N2). `detail`은 `seen 1 sweep`이다.
- `missing_*`, `*_details`, `unreadable_row`는 훑기를 건너 두 라운드 규칙을 따르고 나이 조건은 "행의 `execTime`이 적어도 `min_age_ms` 지남"(`now − execTime ≥ min_age_ms`)이다. 한 훑기에서만 본 차이, 두 번 봤지만 어린 차이는 **훑기를 실패시키지 않는다**(W4는 OMS 게이트가 판단하지 않고, 어린 행은 워터마크가 그것을 지나지 않으므로 다음 훑기가 다시 읽는다). `detail`: `"W4 <kind> <key>: venue …, journal …; seen 2 sweeps, execTime <a> s old"`(`execTime`이 없으면 `execTime unknown`).
- W4는 종목마다 일치한 `Trade`나 거래소 변경 행의 가장 새 `execTime`을 적는다(`W4::exec_times()`, 공유 지도). U19a9부터 W2가 그것이 `updatedTime`이나 `seen_at`보다 새면 거래소의 변경 시각으로 쓴다(9부).

### 워터마크

- 끝난 훑기 뒤 워터마크 = `max(옛 워터마크, w)`. `w` = 그 시각 이하의 모든 비교한 행이 일치하거나, 무시되거나, 이미 확정된(또는 거둔) 가장 새 `execTime`. 확정되지 않은 차이 행(보류 중이거나 어림)이 그보다 새 행을 막는다. `execTime`을 읽을 수 없는 확정되지 않은 행은 모든 행보다 오래된 것으로 센다(검토 4 S3): 워터마크는 그대로이고 다음 훑기는 같은 `start`로 그것을 다시 읽는다. 그래서 읽을 수 없는 행은 워터마크의 조용한 멈춤이 아니라 보류로 끝난다.
- 맞는 행이 없으면(가장 오래된 비교 행이 확정되지 않은 차이이거나 행이 없음) 워터마크는 그대로다. 뒤로 가지 않는다(검토 3 N1). 확정되지 않은 행은 그 훑기의 `start` 이후에 있고 워터마크가 그대로면 `start`도 그대로라, 다음 훑기가 다시 읽는다. 확정된 행은 더는 창을 붙잡지 않는다.
- 행이 하나도 없는 훑기는 워터마크를 두고, 창은 7일 자름까지 자란다.

### W4의 거둠(검토 3 M1, 검토 4 S2)

- W4 차이는 지난 체결 하나에 대한 사실이고 바뀌지 않는다. 그래서 W4는 `clear-hold` 뒤에 그것을 다시 보내지 않는다(W1~W3은 다시 보낸다, 7부). 이 문단이 예전 문장 "W4의 해제 뒤 다시 보내기는 그만큼만 간다"를 대신한다(조용한 계좌에서는 틀린 문장이었다).
- **거둠**: 확정된 W4 정체 `(W4, 주제, 종류, 값)`는 (가) 갚아졌고(그것을 실은 보내기가 `"held"`로 답해 그 글이 그 보류의 메모에 있다; `"already held"`나 보류를 보이는 저널은 갚지 않는다), (나) 갚은 뒤 그 답 **뒤에 낸** 상태 요청의 상한까지 읽은 저널 상태가 `holds.watcher_mismatch == false`를 보일 때 거둔다. 답 앞에 낸 상태 요청의 상한은 `hold_set` 전일 수 있어 너무 일찍 거두므로 증거가 아니다(시험 `w4::a_hold_seen_at_a_cap_sampled_before_the_answer_does_not_dismiss`).
- 이미 걸린 보류 아래에서 확정된 정체는 빚이지 갚아진 것이 아니다: 그 보류가 풀린 뒤 한 번 보내지고, 그 보내기가 `"held"`로 답하고 새 보류가 풀린 뒤에야 거둔다(시험 `w4::a_row_confirmed_under_an_older_hold_is_sent_once_after_that_hold_is_cleared`).
- 거둔 정체는 훑기가 그것을 보는 동안 다시 보내지도 세지도 않는다(`watcher_mismatch_active`, `watcher_differences`, `watcher_mismatches_total`에 없다). 그것을 더는 보지 않는 첫 끝난 훑기에서 잊는다. 같은 행을 그 뒤 다시 읽으면 새 차이이고 다시 두 훑기가 필요하다(안전한 쪽).
- 같은 행의 다른 값(거래소가 행을 바꿈)은 다른 정체다: 확정되고 새로 보낸다(시험 `w4::a_dismissed_row_that_leaves_the_window_and_a_changed_value_are_sent_anew`).
- 오래가는 포지션 차이는 여전히 W2가 다시 보낸다(검토 1 N8).

### 지표(8부)

- W4는 5부의 `watcher_check_age_s{W4}`, `watcher_differences{W4}`, `watcher_mismatches_total{W4}`, `watcher_mismatch_active{W4}`(거둔 정체는 세지 않는다)와 3부의 `watcher_requests_total{endpoint="/v5/execution/list"}`(쪽마다), `watcher_request_errors_total`을 쓴다. 엔진이 돌기 전에는 실행이 `watcher_check_age_s{W4}`도 `boot_ms`부터 센다. 하트비트의 `W4`는 W4의 마지막 성공 시각이다(U19a8부터; 시험 `heartbeat::w3_and_w4_success_times_reach_the_heartbeat`, U19a9).

### 알려진 한계(8부)

- **가득 찬 마지막 쪽**: 거래소가 행이 더 있는데 가득 찬 쪽을 빈 커서로 끝내면 그 행은 W4가 비교하지 않는다. W2가 그 행이 만드는 포지션을 여전히 비교한다. OMS의 체결 동기화는 더 엄격하게 창을 나누지만(`gateway.md:96`) W4는 따라 하지 않는다: 나누기는 예산에 없는 쪽을 쓴다(소유자 질문 11).
- **처음 되돌아보기**: 워처가 `w4_initial_lookback_ms`(600초)보다 오래 내려가 있었으면 시작 때 그보다 오래된 체결은 W4가 비교하지 않는다(검토 2 N3). W2가 그것이 만드는 포지션을 비교한다.
- **거둠은 메모리에만**: 재시작하면 거둔 정체를 잊는다. 재시작의 `w4_initial_lookback_ms` 안에 있는 거둔 행은 두 훑기 뒤 다시 확정되어 보류를 한 번 더 걸고, 운영자가 한 번 더 푼다(안전한 쪽).
- 바뀐 행의 정체는 거래소의 `<side> <execQty>`(또는 종류와 부호 있는 수량, 형, 없는 필드 이름)로 정한다. 가격이나 수수료만 바뀐 행은 같은 정체다(W4는 그것을 비교하지 않는다).
- 끝난 훑기의 행은 비교 때까지 메모리에 있다(행마다 JSON 값 전체). 50라운드 × `w4_max_pages` × 100행이 위 한도다(기본에서 50,000행, 설정의 최대 `w4_max_pages` 50에서 250,000행, 판단(모델 아님)). 행 하나의 바이트는 재지 않았다. U20b는 단위의 `MemoryMax`를 이 최대로 정해야 한다(U19a8 검토 1 N3).
- 쪽을 읽은 뒤 다음 쪽에서 실패한 라운드도 연달아 실패한 라운드로 센다(멈춘 라운드만 줄기를 끊는다, 스펙의 "한 훑기에서 실패한 라운드가 연달아 3번"). 자주 실패하는 거래소에서는 긴 훑기가 끝나지 못하고 다시 시작을 되풀이할 수 있다. 그동안 `watcher_check_age_s{W4}`가 오르고, 다른 것은 그것을 알리지 않는다(U19a8 검토 1 N2).
- 저널 읽기가 래치되면 엔진이 라운드마다 W4를 실패시키므로(5부), 쪽이 실패하지 않았어도 W4는 3라운드마다 훑기를 다시 시작하고 같은 `warn:` 줄을 (한 번) 쓴다(U19a8 검토 1 N1).
- `execTime`(거래소 시계)과 이 호스트의 시계를 비교한다. 거래소보다 `min_age_ms` 이상 앞선 호스트 시계는 확정을 앞당긴다(5부의 한계와 같다). 창의 시작은 `end`를 넘지 않게 자르고, 워터마크는 그것을 읽은 훑기의 `end`를 넘지 않는다(`end` 뒤의 `execTime`은 `unreadable_row`, U19a8 검토 1 S1).
- W4는 loopback 모델 거래소와 시험 저널로만 시험했다. 실제 `execution/list`의 Funding·ADL 행 모양은 시험되지 않았다(설계 R3 T15; 키가 없다, U-01).

### 검증(8부)

`crates/watcher/tests/w4.rs` 33개(프로세스 안의 모델 거래소: 창 안의 체결을 새것부터 100개씩, 커서 `e<다음 번호>`; 시험 저널, 수동 시계). 완료 기준: `w4::funding_rows_beside_known_fills_are_no_mismatch_while_a_position_is_held`(W2와 W4가 함께 돌고, 포지션 ≠ 0, `execQty`와 `side`가 있는 펀딩 행), `w4::an_adl_trade_the_journal_lacks_is_a_mismatch`, `w4::a_round_of_250_executions_is_compared_in_full_over_3_pages`, `w4::executions_1200_are_compared_over_two_rounds_and_the_w4_age_rises_between`. 그 밖에 스펙의 U19a8 시험 18개(`w4::an_unknown_exec_type_is_a_mismatch_at_once`, `w4::the_sweep_starts_10_s_before_the_last_confirmed_exec_time`, `w4::a_row_loaded_late_inside_the_overlap_is_compared`, `w4::a_missing_trade_needs_two_sweeps`, `w4::a_failed_page_three_rounds_in_a_row_restarts_the_sweep_with_a_new_end`, `w4::an_unchanged_cursor_is_a_failed_page`, `w4::an_empty_page_ends_the_sweep`, `w4::a_funding_row_without_side_or_qty_is_ignored`, `w4::a_trade_row_without_qty_is_an_unreadable_row_and_needs_two_sweeps`, `w4::a_trade_at_the_fill_floor_millisecond_is_ignored`, `w4::an_empty_window_logs_once_per_state_change`, `w4::an_unknown_exec_type_in_a_multi_round_sweep_is_confirmed_when_the_sweep_completes`, `w4::a_confirmed_row_is_not_sent_again_after_clear_hold_in_a_quiet_account`, `w4::a_dismissed_row_that_leaves_the_window_and_a_changed_value_are_sent_anew`, `w4::a_hold_seen_at_a_cap_sampled_before_the_answer_does_not_dismiss`, `w4::the_watermark_never_moves_back`, `w4::a_row_confirmed_under_an_older_hold_is_sent_once_after_that_hold_is_cleared`, `w4::an_unconfirmed_row_without_a_readable_exec_time_holds_the_watermark`)와 더한 11개(`w4::a_sweep_not_complete_after_50_rounds_restarts`, `w4::a_trade_or_venue_change_with_other_details_is_a_difference`, `w4::a_row_without_a_key_is_named_by_its_text`, `w4::the_newest_agreeing_exec_time_is_kept_per_symbol`, `w4::a_young_difference_seen_twice_does_not_fail_the_sweep`, `w4::a_trade_at_the_floor_itself_is_compared`, `w4::a_round_that_reads_a_page_ends_the_run_of_failed_rounds`; U19a8 검토 1의 `w4::a_row_after_the_sweeps_end_is_an_unreadable_row_and_does_not_blind_w4`, `w4::a_trade_or_venue_change_of_quantity_0_is_ignored_as_the_oms_ignores_it`, `w4::an_already_held_answer_to_a_w4_send_does_not_pay_it`(W4 보내기가 진행 중일 때 W1 보내기가 `"held"`로 이기고 W4는 `"already held"`: W4 정체는 빚으로 남아 해제 뒤 한 번 보내진다), `w4::the_restart_warn_line_is_logged_once_per_run_of_failed_rounds`). `runtime::the_watcher_runs_against_loopback_fakes_end_to_end`는 서명된 `execution/list` 요청과 W4 성공 시각이 든 하트비트를 본다. 변이: M202(Funding을 Trade로; 고의 결함 21), M203(ADL을 체결과; 22), M204(모르는 형이 둘째 훑기를 기다림; 23), M205(쪽 상한 무시; 24), M206(겹침을 빼지 않음; 25), M207(이은 훑기가 1쪽부터; 26), M208(워터마크가 확정되지 않은 행을 지남; 27), M247(`execTime` 없는 행이 워터마크를 붙잡지 않음; 27의 둘째 형, 검토 4 S3), M215(실패한 쪽을 한없이 다시; 34), M216(`side` 없는 Funding이 읽을 수 없는 행; 35), M217(바닥을 내림; 36), M222(거둔 정체를 다시 확정하고 보냄; 41), M227(`"held"`로 답한 적 없는 정체를 해제 때 거둠; 46), M248(`end` 뒤의 `execTime`을 믿음; 검토 1 S1), M249(수량 0 행을 비교; 검토 1 S2), M250(`"already held"`가 W4 정체를 갚음; 검토 1 S3).

## 9부. W3 지갑과 증거금, W2의 대신 시각, 예산(U19a9)

### W3(`w3::W3`, `w3_ms`마다, 기본 2초)

- 거래소: `GET /v5/account/wallet-balance?accountType=UNIFIED` 한 쪽. `result.list`에서 `accountType = "UNIFIED"`인 계좌가 정확히 하나여야 하고, 그 계좌의 `totalEquity`, `totalMaintenanceMargin`, `totalMarginBalance`가 저널 소수(1부: 지수, `+`, 9자리 이상의 소수는 거절)여야 한다. 아니면, 또는 유지증거금이 음수이면 `unreadable`이고 라운드는 실패한다. OMS의 증거금 보고도 같은 필드를 같은 규칙으로 읽고(`oms-node/src/venue/bybit/query.rs` `margin`: UNIFIED 계좌 하나, `Amount::parse`) 음수 유지증거금이면 보고 전체를 거부한다(C-MARGIN "입력, 상태, 이벤트").
- 위반 둘. 공식은 OMS의 것(C-MARGIN), 한도는 워처 설정 `[w3]`(기본은 OMS의 Bybit 값: `account_mm_rate_max_bps` 5000, `daily_loss_bps` 500; 4부):
  - `account_mm_rate`: `maintenance > 0`이고 `maintenance × 10 000 ≥ account_mm_rate_max_bps × margin_balance`(C-MARGIN "계좌 유지증거금률"). 정확히 한도와 같으면 건다. 한도 5000에서 잔고 1000: 유지증거금 499.99999999는 걸리지 않고 500은 건다. 잔고가 0 이하이고 유지증거금이 0보다 크면 언제나 걸고, 유지증거금이 0이면 잔고가 얼마든 걸지 않는다.
  - `daily_loss`: 저널의 일 기준값(2부, `day_base_set`)의 `day`가 오늘의 UTC 날짜 번호(`now_ms / 86 400 000`, OMS의 `queried_at / DAY_US`와 같은 날)이고 `(base − totalEquity) × 10 000 > daily_loss_bps × base`(C-MARGIN "일 손실과 기준 금액"). 기준 100에서 95.00은 걸리지 않고 94.99는 건다. 오늘의 기준이 없으면 이 부분은 판단하지 않는다(소유자 질문 6). 계좌 유지증거금률은 그래도 판단한다.
- 곱은 나누지 않는 정확한 정수(i128, 10^8 배)다. 곱이 범위를 넘으면 위반으로 본다(안전한 쪽, 판단(모델 아님); 실제 금액에서는 생기지 않는다).
- 정체는 종류뿐이다: `(W3, <kind>, account, "")`(값은 정체에 들지 않는다, 스펙). 금액이 라운드마다 바뀌어도 같은 위반이다.
- 확정: 같은 종류를 두 라운드 연속으로 보면(설계 §7.5 "한도 위반 2회 연속") 엔진의 두 라운드 규칙(5부)으로 확정한다. 나이 규칙은 없다(시각을 `Since::Unknown`으로 주어 늘 늙음). 첫 sighting은 보류 중이고 라운드는 성공한다. 실패한 라운드는 5부처럼 sighting을 지우지 않는다(본다, 실패, 본다 → 세 번째 라운드에서 확정).
- `detail`(나이 없음): `W3 account_mm_rate account: venue totalMaintenanceMargin <m>, totalMarginBalance <b>, at or above <한도> bps; seen 2 rounds`, `W3 daily_loss account: venue totalEquity <e>, journal day base <v> of day <d>, more than <한도> bps below; seen 2 rounds`.
- 다시 보내기(7부): W1·W2처럼 위반이 남아 있는 동안 운영자가 `clear-hold`로 풀면 `mismatch_resend_ms`(기본 10초)마다 다시 보내 보류가 돌아온다(소유자 질문 7). W3은 OMS의 한도와 같은 값을 쓰므로, OMS가 일 손실이나 계좌 유지증거금률 규칙으로 감소 전용 상태에 들어가면 W3도 보통 `watcher_mismatch` 보류를 건다. 일 손실의 위반은 저널에 오늘의 새 `day_base_set`(운영자의 `clear_reduce`, `rebase_day`)이 오거나 UTC 날짜가 바뀌면 끝난다. 운영자의 순서는 런북 7.13(먼저 OMS의 감소 상태, 그다음 워처 보류).

### W2의 대신 시각(U19a9)

- W2(6부)의 거래소 최근 변경 시각은 행의 `updatedTime`, 없으면 `seen_at`이다. U19a9부터 W4가 마지막으로 맞춘(일치한 `Trade`나 거래소 변경 행의) 그 기호의 가장 새 `execTime`이 그보다 새면 그것을 쓴다: `updatedTime.or(seen_at).max(W4 시각)`. 그래서 `updatedTime`이 있어도 W4의 시각이 더 새면 W4의 시각이다(스펙 "the time chosen so far"보다 새면). 셋 다 없을 때만 모름(늙음)이다.
- 지도는 `W4::exec_times()`가 W4와 W2에 나눠 주는 공유 지도다(`W2::with_exec_times`). 실행 코드는 `runtime::checks`에서 둘을 잇고 W2·W3·W4를 각자의 간격(`w2_ms`, `w3_ms`, `w4_ms`)으로 넣는다(시험 `runtime::the_runtime_runs_w2_w3_and_w4_and_w2_reads_w4s_exec_times`: +1~+5초에 W3의 성공은 2초마다, W4는 5초에만 움직인다, 검토 1 S1). W4가 훑기를 비교할 때만 바뀌므로 W4 훑기 사이(기본 5초)에는 지난 훑기의 값이다.
- 효과: 거래소가 `updatedTime`을 주지 않거나 늙은 `updatedTime`을 줄 때도, 체결이 막 맞은 기호의 차이는 `min_age_ms` 동안 미룬다. 미룸은 W2를 실패로 보이게 할 뿐 확정을 놓치지 않는다: W4의 시각은 그 체결 뒤 많아야 `min_age_ms`만 차이를 미룬다.
- 저널의 기록 시각(`last_change_at`, OMS 시계)이 같은 체결의 `execTime`(거래소 시계)보다 늦는 보통의 경우 이 시각은 저널의 시각보다 새지 않아 아무것도 바꾸지 않는다. 바꾸는 것은 두 시계가 다를 때와 `updatedTime`이 크기 변경에 움직이지 않을 때다(6부 한계).

### 하트비트의 W3·W4

- 하트비트(7부)의 `W3`·`W4`는 그 검사의 마지막 성공 시각이다(성공 전에는 `null`, 실패한 라운드는 시각을 바꾸지 않는다). OMS는 W3..W9를 기록하지만 판단하지 않는다(U18b4; C-LIVE 2부). 시험 `heartbeat::w3_and_w4_success_times_reach_the_heartbeat`(실제 `oms::Monitor`가 가짜 `monitor.sock`에 보낸 줄).
- 엔진이 돌기 전에는 실행 코드가 네 검사 모두의 `watcher_check_age_s`를 `boot_ms`부터 센다(시험 `runtime::a_key_rejected_at_start_is_exported`).

### 예산(측정, 강제하지 않음)

- 설계 예산: W1 1 + W2 1 + W3 0.5 + W4 0.2 = 2.7 req/s(설계 §7.5), W1·W2가 한 쪽, W4가 훑기당 한 쪽일 때. 강한 한도는 요청 제한(10초에 60, 3부)이다. 예산은 잴 뿐 막지 않는다.
- 측정(`crates/watcher/tests/budget.rs`): 엔진에 W1~W4를 설계 간격(1초, 1초, 2초, 5초)으로, 수동 시계로 모의 1분([T0, T0 + 60초)), 10ms 단계(판단(모델 아님)). 거래소 요청은 모두 실제 `rest::Guard`를 거친 뒤 모델 거래소가 답한다. 체결은 5초에 n개를 고르게(체결 i는 `T0 + i × 5000 / n` ms), 저널은 그 단계에서 기록하고 거래소 포지션이 따라간다(차이 없음). 요청 지연은 0이다.

| 부하 | W1 | W2 | W3 | W4 쪽 | 합 | req/s |
|---|---|---|---|---|---|---|
| 5초에 체결 30 | 60 | 60 | 30 | 12(훑기 12, 각 1쪽) | 162 | 2.700 |
| 5초에 체결 100 | 60 | 60 | 30 | 42(훑기 12: 1, 2, 3, 그 뒤 4쪽씩) | 192 | 3.200 |

- **범위와 완료 기준의 충돌**(소유자 질문 1, 기본값: 범위를 따름). 완료 기준 "5초에 체결 100 이하에서 2.7 req/s 이하"는 **그대로는 충족되지 않는다**. 범위의 W4 규칙(마지막으로 확정한 `execTime`의 10초 전부터, 모든 커서)이 매 훑기 15초 창을 다시 읽기 때문이다. 5초에 30(창에 약 90행, 1쪽)에서는 정확히 2.7이다(162 ≤ 162). 고른 100/5초에서는 창 `[w − 10초, end]`가 양 끝을 포함해 301행이라 정상 상태의 훑기는 4쪽이고, 정상 상태의 비율은 (60 + 60 + 30 + 48) / 60 = 3.3 req/s다. 첫 1분은 처음 훑기들이 작아(1, 2, 3쪽) 3.2 req/s로 쟀다. 스펙의 어림 ≈ 3.1(300행 = 3쪽)은 창 끝의 한 행을 세지 않았다. 3.3은 IP당 거래소 한도(5초에 600, 곧 초당 120, 설계 §7.3)의 약 2.75 %다. W4는 따라잡는다: 12 훑기가 모두 자기 라운드에서 끝나고 정상 상태의 창은 15초다. 대안(소유자가 고를 수 있다): 2.7에 묶고 W4가 뒤처지게 하거나, 겹침을 줄인다.
- **많은 열린 주문**(검토 3 N4): 코어의 기본 `max_open_orders` 500이면 W1 라운드가 11쪽(가득 찬 10쪽과 빈 마지막 쪽)이라 1초 라운드에서 10초에 110요청이다. 시험 `budget::at_500_open_orders_the_guard_stretches_w1_rounds_below_10_s`(열린 주문 500개, 같은 1분)의 측정: 요청 W1 292, W2 35, W3 22, W4 11(가드가 10초에 60을 지켰다); W1 라운드 26개, 라운드 시간 평균 1,915 ms, 최대 6,990 ms; 하트비트마다 W1·W2의 나이는 최대 7,000 ms로 OMS 게이트의 10초 아래다. 가드는 미끄러지는 창이라 몇 초 만에 60을 다 쓰면 그 요청들이 창을 떠날 때(10초 뒤)까지 아무것도 나가지 않는다. 그래서 평균(약 2초, 스펙의 어림 2.5초)보다 최대가 훨씬 크고, W2·W3도 늘어난다(W2는 60 중 35 라운드). 시험에서 가드 앞에서 기다리는 요청은 먼저 온 것부터 나간다. 실제 클라이언트는 순서 없이 100ms마다 다시 묻는다(3부): 실제의 최대 나이는 다를 수 있다(판단(모델 아님)).
- 가드의 최악 경우(판단(모델 아님)): 10초에 W1 10 + W2 10 + W3 5 + W4 2훑기 × 10쪽 + IP 확인 2 = 47 ≤ 60(W1·W2가 한 쪽, 곧 열린 주문 49개·포지션 199개 이하일 때).

### 지표(9부)

새 지표는 없다. W3은 5부의 `watcher_check_age_s{check="W3"}`, `watcher_differences{check="W3"}`, `watcher_mismatches_total{check="W3"}`, `watcher_mismatch_active{check="W3"}`와 3부의 `watcher_requests_total{endpoint="/v5/account/wallet-balance"}`, `watcher_request_errors_total{endpoint="/v5/account/wallet-balance",class}`를 쓴다. 예산은 `watcher_requests_total{endpoint}`의 증가율로 본다.

### 알려진 한계(9부)

- **자정 뒤**: OMS는 자정을 넘은 첫 보고를 옛 기준으로 먼저 판정하고 그 뒤 새 기준을 기록한다(C-MARGIN). 워처는 오늘의 기준만 쓰므로 자정부터 OMS의 새 `day_base_set`(다음 증거금 보고)까지 일 손실을 판단하지 않는다. OMS의 증거금 보고가 멈추면 그 날 내내 판단하지 않는다(소유자 질문 6의 기본값; OMS 자신의 `margin_stale` 규칙이 그 경우를 다룬다).
- W3은 OMS의 유지증거금 여유 규칙(격리 포지션별 `MmBuffer`)을 보지 않는다(스펙의 W3은 두 규칙뿐; Bybit 설정은 격리를 받지 않는다, C-MARGIN).
- 워처의 `[w3]` 한도는 OMS 설정과 따로 있다. 다르게 두면 두 판단이 다르다(운영 문서: 같은 값으로 둔다).
- 8자리를 넘는 소수는 읽지 못한다(OMS와 같다). 실제 Bybit가 이 계좌에 더 긴 자리를 보내는지는 확인하지 않았다(키가 없다, U-01).
- W2의 대신 시각은 W4가 맞춘 행에서만 온다. 저널에 없는 체결(바로 W2가 보는 차이)의 `execTime`은 쓰지 않는다(그 행은 W4의 차이다).
- 예산은 측정일 뿐 강제하지 않는다. 시험의 요청 지연은 0이다. 실제 지연(판단(모델 아님): 요청당 수십~수백 ms)은 라운드를 늘려 비율을 낮춘다. 체결이 몰리면(고르지 않으면) 한 훑기의 쪽이 늘 수 있다(쪽 상한 `w4_max_pages`와 가드가 위 한도).
- W2의 대신 시각이 닿는 범위(검토 1 N3): W4의 시각은 훑기가 끝날 때(`w4_ms`마다)만, 많아야 그 훑기의 `end`로 움직인다. 그래서 기본 간격(5초, `min_age_ms` 2초)에서는 훑기 뒤 약 2초 동안만 W2의 차이를 미루고, 나머지 약 3초에는 W4의 시각이 이미 늙었다. "거래소에서 체결이 이어지는 동안 W2가 미룬다"는 뜻이 아니다. 나머지는 저널의 `last_change_at`과 `updatedTime`이 맡는다.
- W2의 `detail`은 거래소 쪽 시각(`updatedTime`, `seen_at`, W4의 `execTime`)을 써도 `journal entry <a> s old`라고 쓴다(검토 1 N2; 그 말은 U19a9 전에도 `updatedTime`에 쓰였다). 나이는 맞고 이름만 느슨하다: 런북 7.13은 W2의 이 나이를 "저널과 거래소의 변경 시각 중 새 것"으로 풀고, 거래소 쪽에 W4의 `execTime`이 든다고 쓴다. 결정에는 영향이 없다. 문구를 바꾸면 W1·W2의 모든 `detail` 형식과 그 시험이 바뀌므로 이 부에서는 바꾸지 않았다.
- "두 라운드 연속"은 끝난 라운드로 센다(검토 1 N4; 5부의 엔진 규칙, W1·W2와 같다). 실패한 W3 라운드(읽을 수 없는 지갑, 시간 초과)는 sighting을 지우지 않으므로, 본다·실패 n번·본다도 확정한다. 두 sighting이 시간으로 붙어 있지 않아도 확정하는 안전한 쪽(보류)이다.
- 열린 주문 500개의 여유(검토 1 N5): 하트비트 때의 W1·W2 나이 최대 7,000 ms는 요청 지연 0, 먼저 온 것부터 나가는 대기의 모형에서 잰 값이다. OMS 게이트(10초)까지 여유는 3초다. 실제 지연에서는 게이트가 닫힐 수 있다(거래가 멈춘다: 안전한 쪽이지만 가용성 문제). U20b·testnet에서 잴 것이다(판단(모델 아님)).
- W3, W2의 대신 시각, 예산 모두 loopback 모델 거래소와 시험 저널로만 시험했다. 실제 `wallet-balance` 응답으로 돌려 본 적이 없다.

### 검증(9부)

`crates/watcher/tests/w3.rs` 4개: `w3::a_margin_rate_at_the_limit_on_two_rounds_alarms`(한도 바로 아래, 한 라운드만, 두 라운드 연속, 금액이 바뀌어도 같은 정체, 잔고 ≤ 0, 유지증거금 0, 요청 매개변수), `w3::a_daily_loss_beyond_todays_base_on_two_rounds_alarms`(95.00과 94.99, 해제 뒤 10초에 다시 보냄, 오늘의 새 기준 뒤 끝남), `w3::without_todays_base_only_the_margin_rate_is_judged`(기준 없음, 어제의 기준, 자정을 넘은 오늘의 기준), `w3::an_unreadable_wallet_fails_the_round`(UNIFIED 없음, 둘, 빈 소수, 음수 유지증거금, 지수, 목록 아님, 거래소 오류). `w2::a_missing_updated_time_falls_back_to_w4s_last_exec_time`(`updatedTime` 없는 기호, 늙은 `updatedTime`의 기호, 어느 시각도 없는 기호). `heartbeat::w3_and_w4_success_times_reach_the_heartbeat`. `runtime::the_runtime_runs_w2_w3_and_w4_and_w2_reads_w4s_exec_times`, 확장한 `runtime::the_watcher_runs_against_loopback_fakes_end_to_end`(지갑 경로, 서명된 요청, W3 시각이 든 하트비트)와 `runtime::a_key_rejected_at_start_is_exported`(엔진 전 네 검사의 나이). `crates/watcher/tests/budget.rs` 3개: `budget::at_30_executions_per_5_s_the_reconciliation_rate_in_a_simulated_minute_stays_at_or_below_2_7`, `budget::at_100_executions_per_5_s_the_rate_is_about_3_1_and_w4_keeps_up`, `budget::at_500_open_orders_the_guard_stretches_w1_rounds_below_10_s`. 변이: M251(W3이 한 라운드로 확정; 고의 결함 28), M252(한도와 같은 값이 통과), M253(다른 날의 기준으로 일 손실), M254(W2가 W4의 `execTime`을 쓰지 않음).

## 10부. 직접 알림 [WATCH P1](U19b1)

`watcher::alert`, `watcher::creds::load_token`, `[telegram]`. 설계: [ADR-032](../adr/ADR-032-watcher-kill-and-alerts.md). 워처는 OMS가 죽어도 사람에게 닿는 자기 텔레그램 송신기를 갖는다(설계 §7.7, R2 SHOULD 6, 소유자 결정 Q7·Q18: OMS와 같은 봇, 접두어로 구별). 전달 규칙은 초안 PR #3의 것이고, `alert-direct`가 OMS를 위해 구현한 것을 **링크하지 않고** 다시 썼다(독립성 규칙 3, 1부). U19b1a가 규칙과 토큰 파일을, U19b1b가 송신 태스크(경로, 깨우기와 흘려보내기 간격, 지표, 종료 때 흘려보내기)를 만들었다. U19b1c가 P1 조건(불일치, 키 거부, 공인 IP, 시작 거부)과 실행 연결을 더했다. **U19b1c부터 워처는 `[WATCH P1]`을 보낸다**(`[telegram]`이 있을 때; 없으면 로그에만). loopback 가짜 텔레그램에 대해서만 시험했다.

### `[telegram]`(U19b1a)

- 선택 표다. 모르는 키는 거부한다(`chat_id`나 `token`을 설정에 적으면 거부: 비밀은 토큰 파일에만 둔다).
- `token_file`: 표가 있으면 필수. 절대 경로, `..` 없음, 데이터 디렉터리 밖(가리키는 곳으로 비교, 4부의 `credentials_file`과 같은 규칙). 거부 줄은 `[telegram] token_file must …`로 시작한다.
- **mainnet은 `[telegram]` 없이 시작하지 않는다**: `mainnet requires [telegram]: the watcher's P1 must reach a person`(OMS가 `[alert]` 없이 mainnet을 시작하지 않는 것과 같다). `mainnet_confirmed` 검사가 먼저다.
- testnet은 없어도 된다. 그때 실행은 시작에서 `warn: no [telegram]: every [WATCH P1] is written to this log only, not sent` 한 줄을 남긴다(U19b1a). U19b1c부터 모든 P1이 `warn: [WATCH P1] (no [telegram]: not sent) <본문 첫 줄>` 한 줄이다(지속 조건은 같은 반복 규칙으로).

### 토큰 파일(`creds::load_token(path, data_dir)`, U19b1a)

- 형식은 `alert-direct`의 파일과 같은 JSON이다(운영자는 한 파일을 둘에 줄 수 있다): `{"token": "<digits>:<[A-Za-z0-9_-]+>", "chat_id": <비지 않은 문자열 또는 정수>}`. 다른 키는 읽지 않고 그대로 둔다. 소수인 `chat_id`, 빈 문자열, 객체가 아닌 JSON은 거부한다.
- 읽는 순간 설정의 경로 규칙을 다시 적용하고, 자격 파일과 같은 파일 규칙을 여는 핸들 하나에 적용한다(4부 "자격 파일": `O_NOFOLLOW`·`O_NONBLOCK`, 일반 파일, 소유자 = 실효 uid, 권한 정확히 `0600` 또는 `0400`). 거부 줄은 `token file <path> …`로 시작한다.
- **git 저장소 안이면 거부한다**: 적힌 경로나 가리키는 곳(심볼릭 링크를 따른)의 조상 디렉터리 하나가 `.git`(디렉터리, 또는 worktree의 파일)을 가지면 `token file <path> is inside the git repository <dir>; keep the token outside any repository`. 이 검사는 내용을 읽기 전이다. 토큰이 커밋되는 것을 막는다(`alert-direct`의 규칙).
- **오류는 파일 내용을 인용하지 않는다**: JSON 오류(내용을 인용한다)를 버리고 `is not JSON with token and chat_id`만 쓴다. 토큰 형식이 틀리면 `the token is not of the form <digits>:<letters, digits, _ or ->`이고 토큰은 적지 않는다.
- 읽은 값 `alert::Token`의 `Debug`는 `Token { chat_id: "…", token: <hidden> }`이다. 설정의 `Debug`는 경로만 보인다.
- 실행은 자격 파일 다음, 시작 확인 전에 한 번 읽는다(U19b1a). 거부는 종료 코드 2와 `error:` 한 줄이고 그 전에 아무 요청도 나가지 않는다. 실행은 HTTP 클라이언트를 만든 뒤, 시작 확인 전에 그 토큰으로 송신 태스크를 띄운다(U19b1c). 그래서 시작 확인의 2단계가 키를 거부할 때 송신기가 이미 있다.

### 전달 규칙(`watcher::alert`의 순수 부분, U19b1a)

시계도 네트워크도 없는 함수와 구조다. 송신 태스크(U19b1b)가 이것을 그대로 쓴다.

| 부분 | 규칙 | 근거 |
|---|---|---|
| `message(name, now_ms, body)` | `[WATCH P1] bybit {name} {MM-DD HH:MM:SS KST}\n{body}`. `name`은 설정의 `name`, 시각은 주입된 시계(초 아래는 버림, KST = UTC+9). 접두어 `PREFIX`, 거래소 이름 `VENUE` = `bybit`는 상수 | 설계 §7.7, PR #3 |
| `split(text, 3500)` | UTF-16 단위로 센다(한글 한 글자 1, 이모지 2). 줄 끝에서 자르고, 줄을 잇는 줄바꿈도 센다. 한 줄이 한도보다 길면 문자 단위로 자르고(이모지를 쪼개지 않는다), 그 나머지가 다음 부분을 시작한다. 빈 부분은 만들지 않는다(텍스트가 비지 않는 한): 부분 경계와 맨 앞의 빈 줄은 사라진다(글자는 잃지 않는다, 리뷰 1 S1) | PR #3 `split_message`; 텔레그램 한도 4,096의 여유 |
| `Outbox`(50) | 메시지를 분할해 넣는다. 50개를 넘으면 가장 오래된 부분을 버리고 센다. 큐가 빈 뒤에만 알림 하나 `[WATCH P1] 전송 실패로 알림 N개를 버림`. 알림이 전달되거나 거부되면 그 알림이 센 수만 지우고, 그 사이에 버려진 것은 다음 알림이 센다. 보내는 중에 넘쳐서 이미 버려진 부분은 결과가 와도 두 번 지우거나 두 번 세지 않는다. `dropped_total()`은 지금까지 버린 수(U19b1b의 지표) | PR #3 `Outbox` |
| `outcome(status, body)` | `Delivered`: 2xx이고 본문 JSON의 `"ok": true`. `Refused`: 400, 413(이 메시지의 문제: 부분을 버리고 센다). `Retry`: 그 밖의 모두(5xx, 429, `ok`가 참이 아닌 2xx, 읽을 수 없는 본문)와 네트워크 오류(부분을 두고 이번 흘려보내기를 끝낸다). 문구는 `telegram answered {status}: {description}`, 설명은 200자까지, 제어 문자는 공백. 요청을 담지 않으므로 토큰이 없다 | PR #3 |
| `Repeat`(300,000 ms) | 키별 조건: 처음 `Some(text)`이면 바로 보냄, 지속되는 동안 마지막으로 보낸 때부터 300,000 ms마다 최신 문구로 한 번, `None`이 되면 `해소: <시작 문구>` 한 번. 다시 `Some`이면 새 시작 | 설계 §7.7, PR #3 |

- **저장하지 않는다**(ADR-031의 원칙): 보관함과 반복 상태는 메모리에만 있다. 재시작하면 지속 중인 조건의 시작 문구가 한 번 더 가고, 보내지 못한 부분은 사라진다.
- 반복 규칙의 경과 시간은 `saturating_sub`다: 시계가 뒤로 가면 다음 반복이 그만큼 늦다(새 조건과 해소는 바로 간다).
- 위의 상수(3,500, 50, 300,000 ms)는 설계 값이다(PR #3, 설계 §7.7; 설계에서도 판단).

### 송신 태스크(`alert::spawn`, U19b1b)

`alert::spawn(name, routes, token, clock, metrics, log) -> Result<(Handle, Task), String>`. 경로마다 클라이언트 하나를 만들고(요청 10초, 연결 3초, 리디렉션을 따르지 않음, 프록시 없음) 지금의 tokio 런타임에 태스크 하나를 띄운다. 스펙의 서명에 `name`(머리말의 워처 이름)을 더했고, 경로가 없거나 클라이언트를 만들 수 없으면 `Err`다(실행은 `error: the alert sender: …` 한 줄과 종료 코드 2로 끝난다, U19b1c). 실행은 단일 스레드라 태스크는 실행의 틱과 같은 스레드에서 돈다.

| 부분 | 규칙 | 근거 |
|---|---|---|
| 경로 `Route` | `Route::telegram()` = DNS로 푸는 `https://api.telegram.org/`, 그다음 `api.telegram.org`를 `149.154.167.220:443`으로 고정한 것(`reqwest`의 `resolve`; SNI와 인증서는 호스트 이름). `Endpoints::of(cfg).telegram`이 이 둘이고 설정으로 바꿀 수 없다(REST 주소와 같다). `Route`는 토큰을 담지 않는다 | PR #3 `lib.rs:33-36`, `:493-546` |
| 보내기 | 부분마다 `POST {base}bot{token}/sendMessage`, 본문 `{"chat_id", "text", "disable_web_page_preview": true}`. 경로를 순서대로 쓴다: 네트워크 오류(연결, 시간 초과, 본문 읽기 실패)는 다음 경로로, HTTP 답은 `outcome`(U19b1a)으로 결정하고 답한 경로가 다음부터 맨 앞이다. 모든 경로가 네트워크 오류면 `Retry`. 배달 뒤의 시간 초과는 같은 부분을 두 번 보낼 수 있다(잃는 것보다 낫다) | PR #3 `lib.rs:1179-1233` |
| 넣기 | `Handle::once(body)`는 `message(name, now, body)`를 보관함에 넣고 태스크를 깨운다. `Handle::set(key, Option<text>)`는 `Repeat`(주입된 시계)이 내는 문구만 넣는다. 둘 다 잠금만 잡고 네트워크를 기다리지 않는다 | 스펙 "The sender" |
| 깨우기 | `tokio::sync::Notify` 하나. 태스크는 어떤 타이머에도 자지 않는다. 새 메시지가 바로 깨운다(남은 부분부터 순서대로 간다). 흘려보내기가 부분을 남기고 끝나면 `next_flush` = 그 끝의 주입된 시계 + 5,000 ms이고, 실행의 틱이 `Handle::tick(now)`로 그 시각이 되면 깨운다(`now ≥ next_flush`; 실행의 틱마다 `Alerts::poll`이 부른다, U19b1c). 흘려보내기가 시작할 때 `next_flush`를 지운다. 그래서 수동 시계 시험에서 태스크는 실제 시간이 아니라 시계와 틱으로 움직인다 | 스펙 "Clock and waits", 리뷰 1 N6; 5,000 ms는 PR #3의 설계 값 |
| 흘려보내기 | 보관함의 순서대로 한 번에 하나. `Delivered`와 `Refused`는 다음 부분으로 가고, `Retry`는 그 부분을 두고 이번 흘려보내기를 끝낸다. 큐가 비면 버림 알림(U19b1a)이 같은 흘려보내기에서 간다 | PR #3 |
| 종료 | `Task::finish(5_000)`: 멈춤을 표시하고 깨운 뒤, 마지막 흘려보내기 한 번(진행 중인 흘려보내기가 있으면 그 뒤)을 실제 시간 5,000 ms(판단(모델 아님)) 안에서 기다리고, 넘으면 태스크를 끊는다. 남은 부분은 버리고 한 줄로 센다: `alert sender stopped: nothing left to send`, 또는 `warn: alert sender stopped[ at its bound]: N parts not delivered are dropped[ (and the notice of M earlier drops)]`. 돌려주는 값은 남은 부분의 수. 실행은 U19a의 종료 단계 뒤와 시작 거부의 종료 코드 2 앞에 부른다(U19b1c) | 스펙 "Shutdown (U19b1b)" |

지표(라벨은 3부의 기본 `venue`, `host`, `watcher`뿐, `spawn`부터 있다):

| 계열 | 형 | 뜻 |
|---|---|---|
| `watcher_alerts_queued` | 게이지 | 보관함의 부분 수 |
| `watcher_alerts_delivered_total` | 카운터 | 텔레그램이 받은 부분(버림 알림 포함) |
| `watcher_alerts_dropped_total` | 카운터 | 넘침이나 거부(400·413)로 버린 부분(`Outbox::dropped_total`) |
| `watcher_alert_failures_total` | 카운터 | 실패한 시도: 네트워크 오류가 난 경로마다 하나, `Delivered`가 아닌 HTTP 답마다 하나(거부 포함; `alert-direct`와 같다) |

- 로그: 실패한 시도마다 `warn: telegram send failed: route {i}: {원인}`(거부면 끝에 ` (part dropped)`). 원인은 `reqwest` 오류에서 URL을 뺀 것(`without_url`)과 그 원인 사슬, 또는 `outcome`의 문구라 토큰이 없다.
- 시간: 흘려보내기 간격과 반복 규칙은 주입된 시계, 요청·연결 시간 초과와 종료 한도는 tokio의 실제 시간이다.

### P1 조건(`alert::Alerts`, U19b1c)

실행이 조건을 읽고 보낸다. 본문은 머리말(`[WATCH P1] bybit {name} {KST}`) 다음 줄부터다. OMS의 문구는 모형일 뿐이고 여기서 새로 썼다.

| 키·종류 | 시작 | 해소 | 규칙 | 본문 |
|---|---|---|---|---|
| `auth` | `Client::auth_stop()`이 `Some`이 됨: 다시 보내지 않는 답(HTTP 401, retCode 10003, 10004, 10005, 10007, 10010, 33004; 범위가 이름한 것은 10010·10003·33004이고 나머지는 U19a부터 같은 부류) 또는 재확인의 규칙 멈춤 `stop_key`(코드 −1). 래치를 건 첫 답의 `retMsg`(401은 그 본문)나 규칙의 이유를 한 줄 200자로 들고 그 뒤 바뀌지 않는다(`auth_rejected()`는 그대로) | 프로세스 안에서는 없음(재시작까지) | 반복 규칙(시작, 5분마다) | `워처 키가 거부됨(retCode {code}: {text})`, `워처 키가 거부됨(HTTP 401: {text})`, 규칙 멈춤은 `워처 키가 시작 규칙을 더는 지키지 않음({reason})`. 셋 다 꼬리 `. 워처의 Bybit 요청이 모두 멈춤(재시작 전까지): W1~W4는 성공하지 않아 OMS가 일반 주문을 막고, 워처의 일괄 취소(W5)도 나갈 수 없음. 키·IP 목록을 확인하고 워처를 다시 시작` |
| `public_ip` | `start::PublicIp::get()`이 `Some(why)`: `watcher_public_ip_ok`가 0이 됨(시작 4단계나 재확인의 반향: 목록 밖 주소, 반향 실패, 주소가 아닌 본문; 재확인의 query-api 실패는 `unknown: the IP recheck's query-api failed (<부류>): <오류>`) | 다시 1(`해소: <시작 문구>`) | 반복 규칙 | `워처의 공인 IP가 키의 IP 목록에 없음({why}). Bybit가 10010으로 거부할 수 있음(그러면 워처 키 거부 P1이 따름). 이 호스트의 공인 IP와 키 설정을 확인` |
| 불일치(한 번) | 엔진이 한 라운드에서 `W<n>`의 식별자 하나 이상을 확정(`Action::Confirmed`) | — | 검사마다 첫 것은 바로, 그 뒤 마지막 메시지에서 60,000 ms(판단(모델 아님))에 하나. 그 사이 확정은 모아 다음 메시지로. 종료 때 모여 있는 것은 간격과 상관없이 마지막 흘려보내기 전에 넣는다(`Alerts::drain`, 리뷰 1 S2). `MismatchNotices`는 검사마다 처음 다섯 내용과 나머지 수만 든다(W4는 한 라운드에 수백 개를 확정할 수 있다) | `워처 {check} 불일치 확정 {k}건. OMS에 일반 주문 차단(watcher_mismatch)을 보내기 시작함(실패하면 다시 보냄; 결과는 OMS의 hold 알림). {첫 내용}`, `k > 1`이면 다음 줄에 `그 밖에: {내용 4개까지, "; "로}`와 `m = k − 5 > 0`일 때 `; 그리고 {m}건 더`. 내용은 엔진의 확정 내용(1024바이트로 자름)을 600 UTF-16 단위(판단(모델 아님))로 자른 것이라 메시지 하나가 한 부분이다(이름 64자에서도) |
| 시작 거부(한 번) | 송신기가 뜬 뒤 시작 확인이 시작을 거부(종료 코드 2) | — | 한 번, 종료 전에 흘려보냄(실제 시간 5,000 ms 이하) | `워처 시작 거부: {이유 200자}. 워처가 꺼짐: W1~W4와 W5가 돌지 않음` |

- **불일치 P1 하나는 식별자의 확정 하나다**, `watcher_mismatch` 보내기 하나가 아니다. 운영자의 `clear-hold` 뒤의 다시 보내기(7부)는 새 P1을 보내지 않는다: 돌아온 보류는 OMS 자신의 `hold:watcher_mismatch` 알림이 알린다. 라운드에서 사라졌다가 다시 확정된 식별자는 새 확정이다(`watcher_mismatches_total`의 규칙과 같다). 같은 차이를 다시 확정하는 재시작도 하나를 더 보낸다(메모리만).
- `Action::Confirmed {check, details}`: `judge`가 `watcher_mismatches_total`을 세는 곳에서 라운드마다 많아야 하나를 낸다. 새로 확정한 식별자마다 내용 하나이고, 그 라운드의 `Mismatch` 보내기보다 앞이다. 다시 보내기는 그것을 내지 않는다. `details`의 합은 `watcher_mismatches_total`과 같다.
- `PublicIp`는 `Start`가 만들고 `IpCheck`와 실행이 나눠 쓰는 칸(`Arc<Mutex<Option<String>>>`)이다. 게이지와 함께 바뀌되 하나가 다르다: 재확인의 규칙 멈춤은 게이지를 0으로 두지만 칸은 바꾸지 않는다. 같은 사건을 `auth` P1이 그 이유로 알리기 때문이다(한 사건에 P1 하나; 스펙에 없는 판단).
- 설정, 자격 파일, 토큰 파일의 거부와 HTTP 클라이언트나 송신기를 만들지 못한 것은 송신기 전이라 로그에만 남는다.
- 상한(판단(모델 아님)): 불일치 P1은 어느 창 T에서도 4검사 × (1 + ⌊T / 60초⌋) 이하이고, 종료 때 검사마다 하나가 더해질 수 있다(리뷰 1 S2). 흔들리지 않는 지속 조건은 1 + ⌊T / 5분⌋ + 1(해소) 이하. 흔들리는 조건은 다르다: 해소 뒤의 다음 시작은 새 조건이라 반복 규칙이 묶지 않으므로, 반향이 흔들리면 `public_ip`는 재확인마다(`ip_check_ms`, 기본 60초) 시작·해소 한 쌍, 창 T에 약 2 × ⌈T / ip_check_ms⌉개까지 낼 수 있다(스펙의 상한은 흔들리지 않는 조건에만 맞다; 런북 7.17).

### 실행 연결(U19b1c)

- 시작 순서: 설정 → 자격 파일과 토큰 파일 → 지표, HTTP 클라이언트 → **송신기**(`[telegram]`이 있으면 `alert::spawn(name, Endpoints.telegram, …)`, 없으면 로그 출력 `Alerts::log_only`) → 시작 확인 → `/api/status` 기다림 → 재생 → 엔진. 송신기를 만들지 못하면(경로 없음 등) `error: the alert sender: …`와 종료 코드 2.
- 틱(실제 100 ms, 그리고 답마다)마다 맨 먼저 `Alerts::poll(now, client, public_ip)`: `auth`와 `public_ip`를 `set`에 주고(바뀔 때와 반복 때만 보냄), 기한이 된 불일치 메시지를 `once`로 넣고, 송신기의 `Handle::tick(now)`로 다시 보내기를 깨운다. 엔진의 모든 행동은 실행되기 전에 `Alerts::on_action`을 지난다(확정만 모은다). 그래서 확정, 키 거부, 공인 IP는 그 답을 받은 뒤의 첫 틱에 큐에 들어간다.
- `[telegram]`이 없으면(testnet만) 모든 P1이 `warn: [WATCH P1] (no [telegram]: not sent) <본문 첫 줄>` 한 줄이다. 지속 조건은 같은 반복 규칙(자기 `Repeat`)을 따른다.
- 시작 거부: `error: start refused: …` 줄, P1 하나, 요청 태스크를 끊고, `Task::finish(5_000)`, 종료 코드 2.
- 종료(SIGTERM/SIGINT): 먼저 `Alerts::drain()`이 60초 간격 안에 모여 있는 불일치 메시지를 간격과 상관없이 넣는다(확정마다 P1 하나를 지키려고; 리뷰 1 S2, 스펙의 Amendment). 그 뒤 U19a의 단계(7부), `Task::finish(5_000)`, 종료 코드 0. 종료 단계는 끝나는 요청의 답을 엔진에 주지 않으므로 그 뒤에 새 확정은 없다.
- 송신 태스크가 종료 전에 끝나면(결함) 틱이 처음 한 번 `error: the alert sender task ended: no [WATCH P1] is sent until the watcher restarts`를 남긴다(U19b1b 리뷰 1 N1).

### 알려진 한계(10부, U19b1c)

- 실제 텔레그램에 보낸 적이 없다. 실행 시험은 loopback 텔레그램 가짜 하나(`Route::direct`)를 경로로 주고, 고정 경로는 U19b1b의 시험만 쓴다.
- 시계가 뒤로 Δ 가면 불일치 간격과 반복 규칙이 Δ까지 늦다(`saturating_sub`, 날 시계). 새 확정의 메시지도 그 검사의 마지막 메시지에서 60초를 기다린다. 키 거부와 공인 IP의 시작, 시작 거부는 바로 간다.
- 불일치 메시지는 확정 때의 일이다. `watcher_mismatch` 보내기가 끝내 실패해도 워처는 P1을 더 보내지 않는다(7부의 다시 보내기와 `warn: … mismatch send failed` 줄은 이어진다). 보류가 걸렸는지는 OMS의 `hold:watcher_mismatch` 알림이 알린다.
- 검사의 60초 안에 모인 확정은 다음 메시지가 다섯까지만 보이고 나머지는 수로만 센다. 모든 내용은 확정마다 `error: confirmed difference: …` 로그 줄에 있다.
- `auth`의 HTTP 401 문구는 응답 본문을 담는다(스펙은 상태 줄이라 했다. 코드와 상태는 `HTTP 401`이 이미 말하고, 본문이 이유를 말한다; 스펙과 다른 판단). 본문이 비면 `HTTP 401: )`처럼 빈 채로 간다.
- 시작 거부의 P1은 시작마다 하나다. 종료 코드 2에 다시 띄우는 단위는 재시작마다 하나를 보낸다(U20b의 단위는 2에서 다시 띄우지 않아야 한다, U19b3d가 운영 문서에 적는다).
- 송신 태스크가 끝난 것을 알리는 줄에는 지표가 없다(스펙의 지표 표에 없다). 그 길(태스크 안의 패닉, 독이 든 잠금)을 만드는 시험 수단이 없어 그 줄은 시험하지 않았다.
- 실행 시험은 실제 시간을 기다린다(약 3~6초, 텔레그램이 답하지 않는 시작 거부는 5초 한도).
- retCode 10004의 `retMsg`는 서명한 문자열(`origin_string[<timestamp><api_key>…]`)을 담으므로(3부), U19b1c부터 `auth` P1이 그 200자 안의 API 키 아이디를 텔레그램에도 보낼 수 있다. 비밀은 어디에도 없다(리뷰 1 N1).
- 불일치 문구의 `OMS에 일반 주문 차단(watcher_mismatch)을 보내기 시작함`은 OMS가 이미 `watcher_mismatch`를 쥐고 있으면 맞지 않는다: 엔진은 보류가 켜진 동안 보내지 않고 그 식별자를 해제까지 빚진다(7부). 일반 주문은 이미 막혀 있어 돈의 위험은 없고 문구만 부정확하다(리뷰 1 N2).
- 잠금이 독이 들면(`Client::auth_stop`, `PublicIp::get`) `None`으로 읽혀 `public_ip`에 거짓 `해소:`가 가고 `auth`의 반복이 멈춘다. 잠금을 쥔 채 패닉하는 길은 찾지 못했다(리뷰 1 N3).
- 재확인이 반향 실패(`public_ip` 시작) 뒤 규칙 멈춤으로 끝나면 칸은 더 바뀌지 않아, 재시작까지 `auth`와 옛 이유의 `public_ip`가 각각 5분마다 간다(게이지 0과 맞지만 시끄럽다; 리뷰 1 N5).
- 불일치 메시지는 내용을 `; `로 잇는데 내용 자체도 `; `를 담아(`…; seen 2 rounds, first seen 2.0 s ago`) 채팅에서 경계가 잘 보이지 않는다(읽기만의 문제; 리뷰 1 N7).

### 알려진 한계(10부, U19b1b)

- 실제 텔레그램, 실제 DNS, 실제 TLS는 시험하지 않았다. 시험은 평문 `http`의 loopback 가짜이고, 고정 경로 시험은 예약된 이름 `telegram.invalid`를 가짜의 주소로 고정한다(`resolve`가 빠지면 이름을 풀지 못할 뿐 `api.telegram.org`에 닿지 않는다). 고정 경로가 TLS에서 SNI와 인증서에 호스트 이름을 쓰는 것은 `reqwest`의 `resolve`의 동작이고 여기서 확인하지 않았다.
- 텔레그램이 안 되는 동안 로그 줄이 흘려보내기마다(5초마다) 경로 수만큼 나온다(경로 둘이면 시간당 약 1,440줄; `alert-direct`와 같다).
- 시계가 뒤로 가면 다음 흘려보내기가 그만큼 늦다(`next_flush`는 주입된 시계의 값이다). 새 메시지는 바로 간다.
- 종료 한도에 끊긴 요청은 텔레그램이 이미 받았을 수 있다(로그는 그 부분을 버렸다고 센다). 첫 경로의 연결이 3초 막히면 두 번째 경로에 남는 종료 한도는 약 2초다.
- 3xx는 따르지 않고 `Retry`다(리디렉션이 토큰을 다른 곳으로 가져가지 않게). 시험하지 않았다(`http-fake`는 `Location`을 보내지 못한다).
- 연결 한도(3초)는 시험하지 않았다: loopback에서는 연결이 바로 되거나 바로 거부되어 막힌 연결을 만들 수 없다. 요청 한도(10초)는 답하지 않는 가짜로 시험한다(리뷰 1 S1).
- 종료 한도에서 태스크를 `abort`하는 것은 시험하지 않았다. 바이너리는 `finish` 바로 뒤에 런타임을 끝내므로, 그것이 빠져도 끊긴 요청이 이어지는 시간은 짧다(리뷰 1 N2).
- 송신 태스크가 `finish` 전에 끝나면(`send` 안의 패닉, 독이 든 잠금) 부르는 쪽은 모른다: `Handle::once`는 계속 받거나(잠금이 독이면 말없이 버리고) 종료 때까지 아무 줄도 없다. 지금 코드에서 그런 길은 찾지 못했다(잠금을 await 너머로 쥐지 않음). U19b1c: 실행이 틱마다 태스크가 끝났는지 보고 처음 한 번 `error` 줄을 남긴다(지표는 더하지 않았다, 아래 U19b1c의 한계; 리뷰 1 N1).
- 낫지 않는 답(토큰 폐기 401·404, 봇이 채팅에서 빠짐 403)은 `Retry`라 맨 앞의 부분이 5초마다 다시 가고 뒤의 부분은 넘쳐 버려질 때까지 기다린다(PR #3의 규칙). `watcher_alert_failures_total`이 오르는데 `watcher_alerts_delivered_total`이 그대로인 것으로 보인다; 그 경보 규칙은 U20b의 몫이다(리뷰 1 N3).
- 새 메시지는 언제나 태스크를 깨우므로, 남은 부분은 5,000 ms 간격이나 429의 `retry_after`보다 일찍 다시 간다. 그 빈도는 부르는 쪽의 빈도로 묶인다(반복 규칙은 조건마다 5분에 하나). U19b2b의 킬 루프 알림이 그 빈도를 다시 본다(리뷰 1 N4).
- `spawn`은 경로가 없으면 `Err`다. U19b1c부터 실행 시험의 `Endpoints.telegram`은 loopback 텔레그램 가짜 하나다(`crates/watcher/tests/runtime.rs`의 `World`; 리뷰 1 N5). 실행은 `[telegram]`이 있는데 경로가 없으면 종료 코드 2다.

### 알려진 한계(10부, U19b1a)

- 토큰 파일의 git 저장소 검사는 조상 디렉터리의 `.git`만 본다. `GIT_DIR`이나 `core.worktree`로 다른 곳에 둔 저장소, 읽을 수 없는 디렉터리 안의 `.git`은 보지 못한다(`alert-direct`도 같다).
- 실제 텔레그램에 보낸 적이 없고, 이후 시험도 loopback 가짜만 쓴다(U19b1c부터 실행이 송신 태스크를 띄운다).
- 429의 `retry_after`는 읽지 않는다: 다른 실패처럼 다음 흘려보내기에 다시 보낸다(PR #3과 같다).
- `chat_id`가 틀리면(텔레그램이 `400 Bad Request: chat not found`로 답한다고 본다; 실제 봇으로 확인하지 않음) 400은 "이 메시지의 문제"로 읽히므로 모든 부분이 `Refused`로 버려지고 세어지며, 버림 알림도 거부되어 알린 것으로 친다. 사람에게는 아무것도 닿지 않고 로그 줄과 U19b1b의 `watcher_alerts_dropped_total`만 남는다. 공백뿐인 문자열 `chat_id`(`" "`)도 받아들이고 같은 결과다. 스펙과 PR #3(`alert-direct`)의 규칙을 따랐다(리뷰 1 N1). U20b의 몫(U19b1c는 더하지 않았다): `watcher_alerts_dropped_total`에 대한 경보 규칙, 또는 거부되면 `error`로 남는 시작 메시지 하나.
- `Outbox::add("")`는 빈 텍스트의 부분 하나를 넣는다(텔레그램이 400으로 거부하고 버림 알림이 따른다). 모든 실제 메시지는 `message`의 머리말로 시작하므로 송신 태스크(U19b1b)는 빈 텍스트로 `add`를 부르지 않는다(리뷰 1 N3).

### 검증(10부, U19b1a)

주장 V-WATCH-11. `crates/watcher/tests/alert.rs` 6개: `alert::messages_split_at_3500_utf16_units_at_line_ends`(정확히 3,500, 하나 넘음, 한글, 이모지, 한 단위 남고 이모지, 긴 줄과 그 나머지, 정확히 두 부분, 섞인 긴 텍스트의 보존), `alert::the_outbox_keeps_50_parts_and_reports_the_drops_once`(넘침, 큐가 빈 뒤의 알림 하나, 알림 뒤에 버려진 수, 보내는 중 버려진 부분), `alert::answers_400_and_413_drop_the_part_and_other_answers_keep_it`(순수 `outcome`과 보관함; 스펙의 이름 `400_and_413_…`은 Rust 이름이 될 수 없어 앞에 `answers_`를 붙였다), `alert::the_repeat_rule_sends_start_every_5_min_and_clear`, `alert::the_token_is_never_in_debug`(`Token`과 설정의 `Debug`, 거부 문구), `alert::a_message_has_the_watch_p1_header_with_the_name_and_kst`. `config::telegram_is_validated_and_required_on_mainnet`. `creds::the_token_file_must_be_owner_only_outside_any_repository`(모드, 소유자, 링크, FIFO, 경로, 저장소 안(적힌 경로와 링크가 가리키는 곳, worktree의 `.git` 파일), 내용 규칙, 읽은 값의 `Debug`; root로 돌면 일찍 끝난다, U19a의 권한 시험처럼). 확장한 `runtime::a_start_refusal_ends_the_run_with_exit_code_2`(좋은 토큰 파일은 2단계로 이어짐, `0640` 토큰 파일은 요청 없이 종료 코드 2, testnet에 `[telegram]` 없으면 `warn` 한 줄). 변이: M255(보관함이 새 부분을 버림; 고의 결함 1), M256(400을 다시 보냄), M257(500이 부분을 버림), M258(반복 규칙이 매번 보냄), M259(mainnet이 `[telegram]` 없이 시작), M260(토큰 파일이 그룹 읽기를 받음).

### 검증(10부, U19b1b)

같은 주장 V-WATCH-11. `crates/watcher/tests/alert.rs`에 7개를 더했다(모두 런타임 없이 `alert::spawn`을 자기 스레드의 단일 스레드 tokio에 띄운다): `alert::a_network_error_moves_to_the_pinned_route`(첫 경로는 `dead_addr`, 둘째는 `telegram.invalid`를 가짜에 고정; 경로, 본문, `Host`, 머리말; 답한 경로가 맨 앞이 되어 죽은 경로를 다시 쓰지 않음; `Route::telegram()`과 `Endpoints::of`의 두 경로), `alert::the_token_is_never_in_a_log_line`(두 경로 모두 연결 거부: 경로마다 원인이 있는 줄, 어느 줄에도 토큰·`bot<id>`·`sendMessage` 없음, 종료 줄도), `alert::the_sender_task_flushes_on_the_manual_clock`(500 뒤 `next_flush` = 끝 + 5,000 ms, 그 1 ms 전의 틱과 틱 없는 시계 이동은 아무것도 보내지 않고 그때의 틱이 한 번 보냄, 그 뒤의 틱은 아무것도), `alert::a_retry_keeps_the_part_for_the_next_flush`(502는 부분을 두고 흘려보내기를 끝냄, 새 메시지는 바로 가되 남은 부분 뒤, 400은 버리고 세고 같은 흘려보내기에서 버림 알림, 네 지표와 로그 줄), `alert::shutdown_flushes_within_5_s_and_counts_the_rest`(남은 부분은 틱 없이 마지막 흘려보내기로 간다; 답하지 않는 가짜에서 `finish(5_000)`은 4.9~7초에 돌아오고 남은 3개를 한 줄로 센다), `alert::set_queues_what_the_repeat_rule_gives`(스펙에 없는 시험: `Handle::set`이 시작, 5분 뒤 최신 문구, `해소:`만 넣음), `alert::a_request_without_an_answer_fails_after_10_s`(리뷰 1 S1: 답하지 않는 가짜에 보낸 요청이 9.5~13초에 실패 하나와 `timed out` 줄을 남기고 부분은 남으며 `next_flush` = 시작 + 5,000 ms). 변이: M261(네트워크 오류 뒤 고정 경로를 쓰지 않음; 고의 결함 7). 시험 밖 변이 19개(U19b1b 보고; 요청 한도 셋은 리뷰 1)를 모두 잡았다.

### 검증(10부, U19b1c)

같은 주장 V-WATCH-11. 9개를 더했다: `engine::a_confirmation_emits_one_confirmed_action_per_identity`(두 식별자를 한 라운드에서 확정하면 `Confirmed` 하나에 내용 둘, 그 라운드의 `Mismatch`보다 앞; 계속 보는 라운드와 해제 뒤의 다시 보내기는 내지 않음; 사라졌다가 돌아와 다시 확정된 하나는 새 `Confirmed`; 내용의 합 = `watcher_mismatches_total`), `alert::mismatch_notices_are_at_most_one_per_check_per_60_s_and_collect_the_rest`(첫 것은 바로, 59,999 ms에는 없고 60,000 ms에 모인 셋, 다른 검사는 자기 간격, 간격은 마지막 메시지부터, 시계가 뒤로 가면 기다림; W4의 확정 300개는 내용 다섯(각 600 단위)과 `그리고 295건 더`, 이름 64자에서도 한 부분), `alert::an_auth_answer_sends_one_watch_p1_at_once`(10010, 10003, 33004와 HTTP 401 각각: 답 뒤의 첫 `poll`에 정확한 문구 하나, 5분 1 ms 전까지 없음, 5분에 반복; 규칙 멈춤의 문구), `alert::public_ip_ok_0_sends_one_watch_p1_and_a_clear`(시작 4단계의 목록 밖 주소: `why`를 담은 하나, 재확인이 맞으면 `해소:` 하나, 재확인의 query-api 실패는 새 시작), `alert::a_confirmed_mismatch_sends_one_watch_p1`(완료 기준: 엔진 rig가 W1 차이를 확정하고 `Alerts`가 송신기에 주어 loopback 가짜에 `[WATCH P1] bybit`로 시작하고 내용을 담은 `sendMessage` 정확히 하나; 그 뒤 계속 보는 라운드와 해제 뒤의 다시 보내기는 60초를 한참 넘겨도 없음), `client::auth_stop_keeps_the_first_text`(`retMsg` 한 줄 200자, HTTP 401의 본문, 규칙 멈춤의 이유; 뒤의 멈춤과 거절된 요청은 바꾸지 않음), `runtime::a_key_rejected_at_start_is_told_on_telegram`(2단계가 10010: 첫 시도가 500이면 주입된 시계 5,000 ms 뒤 실행의 틱이 다시 보냄, 시작은 2단계에 머묾, 5분 뒤 반복, 종료 때 흘려보냄), `runtime::a_start_refusal_after_the_sender_is_up_sends_one_watch_p1`(마스터 키: 종료 코드 2 전에 `워처 시작 거부` 하나가 가짜에 닿음; 답하지 않는 텔레그램이면 4.5~9초에 종료 코드 2와 `at its bound` 줄; `[telegram]` 없으면 `warn` 한 줄), `runtime::a_confirmed_mismatch_is_told_on_telegram_once`(실행 전체: 가짜 거래소의 남의 주문이 확정되어 `admin.sock`에 가고 텔레그램에 정확히 하나, 3초 동안의 라운드는 더 보내지 않음). 고친 시험: `runtime::a_recheck_that_fails_a_start_rule_stops_every_bybit_request`(규칙 멈춤의 P1 로그 줄 하나), `runtime::a_start_refusal_ends_the_run_with_exit_code_2`(`[telegram]` 없는 거부의 P1 로그 줄). 변이: M262(불일치 P1을 보내기마다; 고의 결함 8), M263(60초 간격 무시; 9), M264(10010만 알림; 10). 시험 밖 변이 16개(U19b1c 보고): 첫 실행에서 15개를 잡았고 놓친 하나(실행이 송신기의 다시 보내기를 깨우지 않음)는 실행 시험에 500 뒤의 다시 보내기를 더해 잡았다. 리뷰 1 뒤 3개를 더했다: `runtime::a_mismatch_collected_inside_the_pace_is_sent_at_shutdown`(S2·S1: W1이 v900을 확정해 바로 하나, 60초 안에 v901을 확정해 모임, 종료가 그것을 보냄; `[telegram]`이 있을 때 가짜에 둘, 없을 때 내용을 담은 `warn: [WATCH P1] (no [telegram]: not sent) 워처 W1 불일치 확정 1건…` 줄 둘), `alert::mismatch_notices_drain_tells_what_the_pace_holds`(간격 안의 W1 둘과 W2 하나가 `drain`에서 나오고 그 뒤 비어 있음), `alert::the_start_refusal_reason_is_cut_to_200_characters`(S3: 300자 이유는 200자로). 시험 밖 변이 5개를 더해 모두 잡았다(U19b1c 리뷰 1 보고).

## 11부. 워처의 일괄 취소 확인 루프(U19b2)

U19b2a가 이 부의 앞부분을 쓴다: 서명된 POST 하나(일괄 취소), `[kill]`이 있을 때의 키 규칙, `[kill]`의 루프 필드, 킬 알림의 문구와 포지션 묶음, 그 묶음의 포지션 읽기. 루프 자체(`KillLoop`, C2~C5, 일괄 취소 예산, 기한, 알림 규칙)는 U19b2b, 세기와 `Inflight`는 U19b2c다. **U19b2a 뒤 워처는 일괄 취소를 서명해 보낼 수 있고 `[kill]`이 있으면 취소할 수 없는 키를 거부하지만, 아직 아무것도 일괄 취소를 보내지 않는다**(부르는 코드가 없다). **U19b2b 뒤 sans-IO 루프가 있고 손으로 몬 시험을 지나지만, 아직 아무것도 그것을 돌리지 않는다**(세기와 `Inflight`는 U19b2c, 실행은 U19b3c). 시험은 loopback 가짜(`http-fake`)와 수동 시계로만 한다. Bybit 키가 아직 없어 실제 거래소에는 보낸 적이 없다.

### 서명된 POST 하나(`rest::Client::cancel_all`)

- `Client::cancel_all() -> Result<CancelAll, RestError>`가 GET이 아닌 유일한 서명 요청이고 REST 기준 주소로 가는 유일한 POST다. 경로 `/v5/order/cancel-all`(`rest::CANCEL_ALL`)과 본문 `{"category":"linear","settleCoin":"USDT"}`(`rest::CANCEL_ALL_BODY`)는 `rest.rs`의 상수다. 일반 `post(path, body)`는 없다. 그래서 워처의 어느 코드 길도 주문을 만들거나 고칠 수 없다(워처 키는 취소하려고 `Order` 권한이 필요하고, 그 권한은 주문 생성도 허락한다).
- 서명은 3부의 GET과 같은 HMAC이고 질의 자리에 **보내는 본문의 바이트**가 온다: `X-BAPI-SIGN = hex(HMAC_SHA256(secret, timestamp + api_key + recv_window + 본문))`(`sign::post_prehash`, `sign::post_signature`; gateway facts Q1.1). 서명한 문자열과 보낸 본문은 같은 상수다(Q1.15는 바이트가 같아야 한다는 추론). `Content-Type: application/json`과 3부의 머리글 다섯을 단다.
- 3부의 요청처럼 요청 제한(가드), 403 멈춤, 키 래치를 지난다: 키가 거절된 뒤나 403 멈춤 중에는 보내지 않는다. `watcher_requests_total{endpoint="/v5/order/cancel-all"}`에 보내기 전에 센다.
- 답: 3부의 부류 그대로. retCode 0이고 `result.success`가 없거나 `"1"`이면 `CancelAll {cancelled: result.list의 길이}`. `"0"`이면 `refused`(`cancel-all answered success 0`, 코드 0). `result.list`가 목록이 아니거나 `result`가 없거나 `success`가 다른 값(숫자 1, `"2"` 등)이면 `unreadable`(마지막은 스펙에 없는 판단: 모르는 값은 성공이 아니다). 모두 `watcher_request_errors_total{endpoint="/v5/order/cancel-all", class}`로 센다. 일괄 취소는 비동기라(Q11.5) `cancelled`는 취소가 끝났다는 뜻이 아니다: 남은 것은 세기(U19b2b C3)만 말한다.
- 워처의 다른 POST 둘은 서명하지 않고 다른 호스트로 간다: VM 가져오기(`metrics.rs`, U19a3)와 텔레그램 `sendMessage`(`alert.rs`, U19b1b). 소스 시험 `client::no_other_post_exists`가 시험 때 `crates/watcher/src/*.rs`를 글자로 읽어(주석도 센다) `.post(`가 `rest.rs`(`cancel_all` 안), `metrics.rs`, `alert.rs`에 하나씩만 있고 다른 파일에는 없으며, `Method::`, `.request(`, `.put(`, `.patch(`, `.delete(`가 어느 파일에도 없음을 본다(다른 메서드 뒤에 요청을 숨길 수 없게). `.get(`은 보지 않는다(맵 조회가 쓴다). 서명된 GET은 모두 `Client::get`을 지난다.

### `[kill]`이 있을 때의 키 규칙(`start::Start` 2단계와 `IpCheck`)

| 규칙 | 통과 | 거부(종료 2, `query-api: ` 다음 이유) | 읽을 수 없음(시작은 5초 뒤, 재확인은 `ip_check_ms` 뒤 다시) |
|---|---|---|---|
| 읽기·쓰기 | `readOnly == 0`(정수) | `the API key is read-only (readOnly 1); with [kill] the watcher cancels orders and needs a read-write key` | `readOnly`가 없거나 정수가 아님(불리언 `false`, 문자열 `"0"` 포함) |
| 주문 권한 | `permissions.ContractTrade`가 `"Order"`를 담음 | `the API key has no ContractTrade Order permission; with [kill] the watcher needs it to cancel orders`(`ContractTrade`가 없어도) | `permissions`가 객체가 아님, `ContractTrade`가 있지만 문자열 목록이 아님 |
| 출금 권한 없음 | `permissions`의 어느 목록에도 `"Withdraw"` 없음 | `the API key has the Withdraw permission (<목록 이름>); the watcher's key must not be able to withdraw` | `permissions`가 객체가 아님, 그 값 하나가 문자열 목록이 아님 |

- 이 규칙은 2단계의 기존 규칙(하위 계정, `sub_account_uid`, 묶인 IP) 뒤에 본다. `[kill]`이 없으면 읽지 않는다: 읽기 전용 키도 된다(U19a의 워처는 조회만 한다).
- 재확인(`IpCheck`)도 같은 규칙을 본다. 거기서 맞지 않으면 4부의 규칙 멈춤이다(`Step::StopRequests`, `Client::stop_key`, 재시작까지 모든 Bybit 요청과 일괄 취소가 멈춤; 소유자 질문 5).
- `Order`가 일괄 취소를 허락한다는 것은 권한 이름에서 한 추론이고 문서에 없다(Q4). `Withdraw`는 Q4에 마스터 전용이라 하위 계정 키에는 없어야 하고, 거부해도 잃는 것이 없다(설계 §7.4 "출금 권한 없음").
- 읽을 수 없는 답을 "읽을 수 없음"(다시 묻기)으로 둔 것은 기존 2단계와 같은 판단이다. 거부가 아니라서 시작이 끝나지 않을 뿐 아무것도 허락하지 않는다.
- 읽히는 규칙이 맞지 않으면 같은 답의 다른 필드나 목록을 읽을 수 없어도 거부다(검토 1 S4; 스펙 Amendment). 순서는 표의 위에서 아래(읽기·쓰기, 주문, 출금)이고, `Withdraw`는 읽히는 모든 목록에서 찾는다. "읽을 수 없음"은 읽히는 규칙이 모두 맞을 때만이다. 그래서 재확인에서 키가 읽기 전용이 되거나 `Withdraw`를 얻으면, 답의 다른 목록 하나가 `null`이어도 규칙 멈춤이다(IP 재확인 실패로 끝나지 않는다).
- 재확인의 "읽을 수 없음"은 4부의 재확인 실패다: 경고 한 줄, `watcher_public_ip_ok` 0, `ip_check_ms` 뒤 다시(규칙 멈춤이 아니다; 검토 1 N4).

### `[kill]`의 루프 필드(`config::Kill`, U19b2a)

| 필드 | 기본값 | 범위(판단(모델 아님)) |
|---|---|---|
| `round_ms` | 1,000(표 7-8 C4, 설계 값) | 1,000..=10,000(1초 이상이면 워처는 초당 일괄 취소 하나 이하) |
| `watch_ms` | 30,000(C5) | 10,000..=300,000 |
| `watch_every_ms` | 5,000(C5) | 1,000..=`watch_ms` |
| `notice_ms` | 60,000(C4) | 60,000..=600,000(60초 이상이면 10분에 12개 상한이 지켜진다) |

OMS의 `kill_*` 필드와 같은 기본값과 범위를 여기 다시 썼다. 적지 않은 루프 필드는 기본값이다. 모르는 키는 거부한다. U19b3a가 `marker`(필수), `oms_unit`(필수), `systemd`, `stop_wait_ms`를 더했다(12부): U19b2a에서 맞던 빈 `[kill]`은 이제 `missing field`로 거부된다(그 사이에 배포하지 않았다; 검토 3 N8).

### 킬 알림의 문구(`kill::notice_text(&Notice, header)`, 순수)

본문이다. 실행(U19b3c)이 `[WATCH P1] bybit {name} {KST}` 머리를 붙이고, 중단 알림 말고는 그 앞에 읽은 포지션 묶음을 붙인다. `header`는 킬 순서(U19b3b)가 주고 시작 알림에만 들어간다. 모든 문구가 `워처 킬`이라고 해서 OMS의 `킬 루프`와 한 채팅에서 구별된다. 외부에서 온 글(이유, 오류)은 200자로 자르고 제어 문자를 공백으로 바꾼다. 초는 ms를 정확히 쓴다(1,500 → `1.5`).

| 종류(`Notice`) | 문구 |
|---|---|
| `Start {round_ms}` | `워처 킬 시작({header}). 워처 키로 미체결 0건 확인까지 {round_ms/1000}초마다 일괄 취소(linear, USDT 정산). 포지션은 닫지 않음.` |
| `Progress {round, remaining, cancel_all_calls, last_error}` | `워처 킬 {r}회차: 남은 미체결 {n}건, 일괄 취소 {k}회.`(세지 못했으면 `남은 미체결 모름: {why}`), 마지막 오류가 있으면 한 칸 띄고 그 문구 |
| `Final {watch_ms, round, cancel_all_calls, outcome}` | `워처 킬 정리 완료: 미체결 0건 확인 뒤 {watch_ms/1000}초 동안 늦은 접수 없음({r}회차, 일괄 취소 {k}회). ` 다음 `StopOutcome`의 문구 |
| `Abort {why, round, remaining, cancel_all_calls}` | `{워처 종료 \| 워처 시작 거부: <이유 200자>}로 워처 킬 중단({r}회차, 남은 미체결 {n건 \| 모름}, 일괄 취소 {k}회). OMS의 래치·마커는 그대로.` 다음 줄에 고정 문구 `포지션 조회 안 함(워처 종료 중). 수량·손익·마크·강제청산가 모름`(워처가 끝나는 중이라 읽지 않지만 "모름"으로 보인다; 실행은 묶음을 붙이지 않는다) |

진행 알림의 마지막 오류(`LastError`):

| 마지막 일괄 취소 | 문구 |
|---|---|
| `NotSent(Latched)`(키 래치, `auth_stop()`이 `Some`) | `워처 키가 멈춰 일괄 취소를 보낼 수 없음: 워처를 다시 시작해야 루프가 끝남` |
| `NotSent(Paused)`(403 멈춤) | `워처 요청 일시 정지(HTTP 403): 정지가 풀리면 다시 보냄` |
| `NotSent(Local(why))` | `일괄 취소를 보내지 못함(로컬 오류: {why})`(스펙에 없는 문구, 판단) |
| `Failed(why)`(보냈고 쓸 답이 없거나 거부) | `마지막 일괄 취소 실패: {why}`(스펙에 없는 문구, 판단) |

마지막 알림의 마지막 문구(`StopOutcome`, 킬 순서 U19b3b가 정한다). `clear-hold`를 말하는 것은 래치와, 이 프로세스에서 경로를 확인한 마커가 있는 멈춤 둘뿐이다: 아무것도 래치하지 않았는데 래치됐다고 말하지 않는다.

| `StopOutcome` | 문구 |
|---|---|
| `Latched`(K2가 래치) | `OMS는 래치됨(watcher_kill). 해제는 운영자만: omsctl clear-hold --reason kill --note …` |
| `StoppedMarked`(K3 멈춤, 마커가 있고 경로 확인됨) | `OMS는 멈춤(systemctl stop). 다음 시작은 킬 마커로 래치됨. 해제는 운영자만: omsctl clear-hold --reason kill --note …` |
| `StoppedUncheckedMarker`(K3 멈춤, 마커는 있으나 경로 확인 못 함) | `OMS는 멈춤(systemctl stop). 킬 마커는 OMS가 읽는 경로인지 확인 못 함: 워처는 OMS가 다시 답하면 watcher_kill을 보냄` |
| `StoppedUnmarked`(K3 멈춤, 마커 못 씀) | `OMS는 멈춤(systemctl stop). 킬 마커가 없어 다음 시작은 래치 없이 시작함: 워처는 OMS가 다시 답하면 watcher_kill을 보냄` |
| `Unconfirmed`(K2 래치 못 함, K3 실패나 미확인) | `OMS 정지 미확인: OMS가 살아 있으면 래치 없이 주문을 낼 수 있음. 마커는 다음 시작에만 걸림. 워처는 OMS가 다시 답하면 watcher_kill을 보냄` |

### 포지션 묶음(`kill::position_block(&Positions, kst)`, 순수)

- `Positions::Rows(행)`: 머리 줄 `포지션(거래소 /v5/position/list, {KST}, 강제청산가는 교차 증거금이면 거래소 추정):` 다음 행마다 `- {symbol} {매수|매도} {size}, 미실현손익 {unrealisedPnl}, 마크 {markPrice}, 마크에서 강제청산가까지 {d} %`.
  - `d = |mark − liq| / mark × 100`, 0.01 %로 **내림**: `Dec::percent_floor(num, den)`(`num ≥ 0`, `den > 0`; `num × 10^4 / den`을 검사된 `i128`로 0 쪽으로 나눈 몫이 백분의 일 퍼센트; 넘치면 `None`). 값은 워처의 `Dec`로 읽고 정규형으로 쓴다(`10`, `9.99`).
  - `liqPrice`가 비었거나 0이거나 읽을 수 없으면 `모름(거래소가 청산가를 주지 않음)`(교차 증거금이면 거래소가 주지 않을 수 있다), `markPrice`가 0 이하이거나 읽을 수 없으면 `모름(마크 가격 없음)`, 계산이 넘치면 `모름(계산 넘침)`. 손익이나 마크가 읽히지 않으면 그 자리가 `모름`.
  - `size`가 0인 행은 뺀다(`settleCoin`만 주면 크기 > 0인 행만 온다, Q12.1). `size`가 읽히지 않는 행(필드 없음, JSON 수, 소수 9자리 이상, 빈 문자열)은 크기를 모르는 포지션이라 빼지 않고 `{size}` 자리에 `수량 모름`을 쓴다(검토 1 S3; 스펙 Amendment): 그런 행만 있어도 `포지션 없음`이라 하지 않는다. 방향이 `Buy`·`Sell`이 아니면 `방향 모름`, 기호는 `[A-Z0-9]` 32자까지(다른 글자는 `?`)로 쓴다: 행을 숨기지 않는다(스펙에 없는 판단). 음수 크기는 문서상 없지만(Q12.3) 읽히면 그대로 보인다.
  - 20행까지 쓰고 나머지는 `그 밖에 {m}종목`(판단(모델 아님): 알림 하나가 3,500 단위 한 부분에 들어가 "12개 P1"이 텔레그램 메시지 12개로 남게). 남는 행이 없으면 머리 줄 다음 `포지션 없음`.
- `Positions::Failed(class)`: `포지션 조회 실패({class}). 수량·손익·마크·강제청산가 모름`(`class`는 3부의 부류, 기한을 넘기면 `시간 초과`).
- `Positions::NotRead(why)`: `포지션 조회 안 함({why}). 수량·손익 모름`(래치나 멈춤이라 묻지 않았다).
- 스펙은 `position_block(rows: Result<&[Value], &str>, kst)`라 했다. "실패"와 "묻지 않음"의 문구가 다르므로 셋을 가진 `Positions`로 썼다(스펙과 다른 판단, 뜻은 같다).

### 포지션 읽기(`kill::read_positions(client, clock, request_timeout_ms)`)

- `GET /v5/position/list?category=linear&settleCoin=USDT&limit=200`과 커서의 쪽을 W1의 쪽 규칙(빈 커서면 끝, 가득 찬 쪽에 커서가 없거나 보낸 커서가 그대로 오면 `unreadable`)으로 5쪽까지(판단(모델 아님)) 읽는다. 더 있으면 `unreadable`(1,000 포지션 넘음).
- 기한: 읽기 시작의 주입된 시계 + `request_timeout_ms` + 11,000 ms(5부 엔진의 요청 기한과 같은 덧붙임: 가드의 10초 창 + 1초; 검토 4 N2). 쪽마다 답 뒤에 시계를 읽고, 기한 **때나 뒤에** 답한 쪽이면 `시간 초과`로 끝내고 다음 쪽을 보내지 않는다(검토 3 S2). 걸린 요청을 끊는 것(`AbortHandle`)은 실행(U19b3c)이 한다.
- 키가 래치됐거나(`워처 키가 멈춤`) 403 멈춤 중이면(`HTTP 403 뒤 요청 일시 정지`) 묻지 않고 `NotRead`다.

### 루프(`kill::KillLoop`, sans-IO, U19b2b)

`KillLoop::new(timing: config::Kill, request_timeout_ms)`. 입력: `start(now, header, outcome)`, `cancel_done(now, req_id, CancelAnswer)`, `count_page(now, req_id)`(세기의 한 쪽이 답함; 검토 2 S2), `count_done(now, req_id, Result<usize, String>)`, `set_outcome(StopOutcome)`(루프 중 다시 보낸 정지가 래치함, U19b3b; 검토 3 M1), `poll(now)`, `abort(now, why)`. 출력 `Action`: `CancelAll {req_id}`, `Count {req_id}`, `Notice(Notice)`, `Ended(Report {rounds, cancel_all_calls, zero_since, aborted})`. 접근자: `phase()`, `round()`, `remaining()`, `cancel_all_calls()`, `next_wake()`, `header()`, `outcome()`, `ignored()`, `budget_holds()`. 모든 입력은 루프 시계의 `now`를 받아 그 자리에서 할 일을 돌려주고 아무것도 기다리지 않는다. `next_wake()`는 다음 `poll`을 원하는 시각이다: 열린 요청의 기한, 다음 라운드, 다음 지켜봄 세기, 예산의 시각, 다음 진행 알림 표시 중 가장 이른 것. `CancelAnswer` = `Done(CancelAll)` | `Failed(why)`(보냈고 쓸 답이 없거나 거부) | `NotSent(NotSent)`(아무것도 나가지 않음: `Latched`, `Paused`, `Local(why)`).

| 상태(`Phase`) | 들어옴 | 함 | 다음 |
|---|---|---|---|
| `Cancel`(C2) | 라운드 r이 `t_r`에 시작 | 예산이 받으면 `CancelAll`, 아니면 예산의 시각까지 기다림 | `Done`·`Failed` → 곧바로 C3, `NotSent` → C4 |
| `Count`(C3) | 보낸 일괄 취소 뒤(실패했어도) | `Count`(읽기는 U19b2c: `/v5/order/realtime` linear USDT의 모든 쪽, 열린 상태 셋, 남의 주문도 셈) | 0 → C5, n > 0 → C4, 오류 → C4(실패한 세기는 0이 아니다) |
| `Repeat`(C4) | n > 0 또는 실패 | `t_r + round_ms`까지 기다림(이미 지났으면 곧바로) | C2, 라운드 r + 1(`t_{r+1}`은 그 시각) |
| `Watch`(C5) | 이번 통과의 첫 0의 시각 `z`(C5에 들어올 때마다 새 `z`; 검토 4 N6) | `z + k × watch_every_ms`에 `Count`, 마지막은 `z + watch_ms` | n > 0 → 곧바로 C2(라운드 r + 1), 오류 → C4, `z + watch_ms` 때나 뒤에 낸 세기의 0 → 끝(얼마나 늦게 답했든) |
| `Ended` | 위의 끝 | 마지막 알림(곧바로), `Ended(Report)` | 아무것도 안 함 |
| `Aborted` | `abort`(U19b3d: 종료나 루프 중 시작 거부) | 중단 알림 하나, `Ended(aborted)` | 아무것도 안 함 |

- **요청 둘이 함께 열리지 않는다**(검토 1 N7): 하나가 열려 있는 동안 기한이 된 세기나 일괄 취소는 그 답이나 기한을 기다렸다가 곧바로 간다. 그래서 지켜봄의 세기는 늦을 수 있고, 지켜봄을 끝내는 세기는 `z + watch_ms` 때나 뒤에 처음 낸 세기다. 그 전에 낸 세기가 늦게 답한 0은 끝이 아니다(OMS의 N4와 같다). 시작 전의 `abort`는 아무것도 하지 않는다(시작 전의 중단은 킬 순서가 알린다, U19b3b). 두 번째 `start`(요청이 열린 때, 지켜봄, 끝이나 중단 뒤)도 아무것도 돌려주지 않고 아무것도 바꾸지 않는다: 킬 하나에 루프 하나다(스펙 고침, 검토 1 S3).
- **일부러 OMS와 다른 점 1(C2 실패 뒤의 세기)**: OMS는 실패한 일괄 취소에서 세지 않고 C4로 간다(`crates/oms-node/src/venue/bybit/kill.rs:238-259`). 워처는 보낸 모든 일괄 취소 뒤에 센다. 세기가 진실이고(누가 취소했든 미체결 0이 목표), 미체결이 없을 때의 일괄 취소 답이 문서에 없다(Q11.7): Bybit가 취소할 것이 없는 일괄 취소를 거부하면, 거부 뒤에 세지 않는 루프는 끝나지 않는다(소유자 질문 3의 기본값). 보내지 않은 일괄 취소(래치, 멈춤, 로컬 오류) 뒤에는 세지 않는다(세기도 나가지 않는다).
- **일괄 취소 예산**: 반열린 창 `[t, t + 1,000)` ms에 `CancelAll` 둘 이하, 낸 시각으로 센다(`ORDER_BUDGET` 2, `ORDER_WINDOW_MS` 1,000; 설계 표 7-8 C4의 비상 경로 초당 2). 예산이 잡은 일괄 취소는 예산의 시각(마지막 둘 중 이른 것 + 1,000 ms)에 `poll`에서 나간다. 설정에 맞는 타이밍(`round_ms ≥ 1,000`, `watch_every_ms ≥ 1,000`)에서는 두 일괄 취소가 1,000 ms보다 가깝지 않아(다음은 `t_r + round_ms`, 지켜봄 길에서는 `z ≥ t_r`에서 `watch_every_ms` 뒤) 예산이 묶이지 않는다: 나중에 타이밍이나 길이 바뀔 때의 방어다(검토 1 S2). `KillLoop::new`는 타이밍을 그대로 받으므로(범위는 설정을 읽을 때만 본다) 시험은 범위 밖의 100 ms로 예산이 묶이는 것을 본다.
- **기한**(검토 2 S2, 검토 3 S2): `CancelAll`의 기한은 낸 시각 + `request_timeout_ms` + 11,000 ms(5부 엔진의 한 요청 덧붙임: 가드의 10초 창 + 1초). `Count`는 20 요청까지라 기한이 쪽마다다: 낸 시각 + 같은 덧붙임이고, 기한 전의 `count_page(now)`가 그것을 `now` + 덧붙임으로 옮긴다(1.3초짜리 11쪽 세기도 쪽이 답하는 동안은 실패하지 않는다). 쪽, `count_done`, `cancel_done`을 기한 **때나 뒤에** 받으면 기한 뒤다: 세기는 오류, 일괄 취소는 `Failed`(보낸 것으로 센다)이고 기한은 옮기지 않는다. `poll`이 기한을 먼저 보아도 같다(`no answer before the deadline`). 그 뒤에 온 같은 `req_id`의 답이나 쪽, 열린 요청이 아닌 `req_id`나 다른 종류의 답은 버리고 `ignored()`에 센다(디버그 줄은 실행 U19b3c가 쓴다). 기한이 지난 요청의 태스크를 끊는 것(`AbortHandle`)은 `kill::Inflight`(U19b2c, 아래 "요청 태스크")다.
- **남은 건수와 마지막 오류**: `remaining()`은 마지막 세기의 결과다. 첫 세기 전에는 `모름: 아직 세지 않음`, 세기 오류는 그 이유의 모름이고 0이 되지 않는다. 보내지 않은 일괄 취소는 건수를 바꾸지 않는다(스펙에 없는 판단). 마지막 오류(`LastError`)는 마지막 일괄 취소의 것이고 `Done`이 지운다(스펙에 없는 판단: 진행 알림이 지난 오류를 지금 것처럼 보이지 않게). `NotSent`의 종류는 다음 진행 알림까지 남는다(검토 2 S5).
- **라운드 수의 상한은 없다**: 0을 확인하거나 프로세스가 멈출 때까지 돈다(OMS의 루프와 표 7-8). 라운드마다 요청은 일괄 취소 하나와 세기의 쪽 20까지이고, 요청 제한(10초에 60)이 단단한 상한이다.

### 알림 규칙(C4, U19b2b)

- 종류: **시작**(`start` 때), **진행**(`poll`에서: 진행 알림 k는 `s + k × notice_ms`, `s`는 시작 알림의 시각; 루프가 끝나지 않은 동안 그 표시 때나 뒤의 첫 `poll`에서 나간다), **마지막**(끝나는 입력에서 곧바로, `notice_ms`로 잡지 않는다), **중단**(`abort` 때). 늦은 진행 알림은 다음 표시를 옮기지 않고, 한 `poll`에서 표시 둘 이상이 지났으면 알림 하나에 다음 표시는 `now` 뒤의 첫 표시다(검토 3 S3). 그래서 실행이 얼마나 늦게 `poll`해도(틱 100 ms; 빠른 수동 시계에서는 수 초) 아래 상한이 정확하다.
- 진행 알림은 최신 라운드, 건수(알거나 `모름: <이유>`), 마지막 오류를 싣고, 건수가 바뀌지 않아도 `notice_ms`마다 나간다(표 7-8 "바뀌지 않아도 60초마다"; 소유자 질문 2의 기본값).
- **일부러 OMS와 다른 점 2(바뀐 건수의 알림 없음; 검토 1 S4)**: OMS는 0보다 큰 알려진 건수가 마지막에 보인 것과 다르면 곧바로 바뀜 알림을 보내고(`kill.rs:192-235`, `Change`) 60초마다 반복한다. TN `05_trading.html:213`은 "60초마다, 그리고 남은 건수가 바뀔 때 보냄"이라 한다. 워처는 바뀜 알림을 보내지 않는다: 바뀐 건수는 다음 진행 알림을 기다린다. 이유는 범위의 "60초에 한 번까지"와 10분에 12개 상한(설계 R2 SHOULD 4)이고, OMS의 바뀜 알림은 이 상한을 지키지 않는다(바뀔 때마다 하나 더). 그래서 `watcher_kill` 뒤 두 루프가 함께 돌면 `[OMS P1] 킬 루프`가 건수 변화를 `[WATCH P1] 워처 킬`보다 최대 60초 먼저 보일 수 있다.
- 상한: 길이 T의 어느 창에서도 시작·진행 알림은 1 + ⌈T / notice_ms⌉ 이하다(창 안의 첫 알림 뒤의 알림은 저마다 창 안에 자기 표시가 있다. 알림 하나가 그 전에 지난 모든 표시를 덮기 때문이다). 여기에 마지막 하나나 중단 하나가 더해진다. 10분 동안 조회가 실패해도 1 + 10 + 1 = 12다(`notice_ms` ≥ 60,000, 위 `[kill]`의 범위). 킬 순서는 킬마다 `LateHalt` 알림을 하나까지 더한다(U19b3b, 루프의 12 밖).

### 세기(`kill::count_open(client, clock, start_ms, request_timeout_ms, on_page)`, U19b2c)

- `GET /v5/order/realtime?category=linear&settleCoin=USDT&limit=50`과 커서의 쪽을 W1의 쪽 규칙(빈 커서면 끝, 가득 찬 쪽에 커서가 없거나 보낸 커서가 그대로 오면 `unreadable`)으로 20쪽(`w1::MAX_PAGES`, 판단(모델 아님), OMS와 같음)까지 읽는다. 20쪽째에도 다음 커서가 있으면 세기 오류 `more than 1000 open orders`이고 21쪽째는 보내지 않는다(검토 3 N8).
- 열린 주문은 `orderStatus`가 `New`, `PartiallyFilled`, `Untriggered`인 행이다(Q10.1, `w1::OPEN`). `orderLinkId`는 보지 않는다: 남의 주문(다른 링크, 빈 링크, 링크 없는 행)도 센다. 누가 냈든 미체결 0이 목표다. `orderStatus`가 없는 행과 읽을 수 없는 쪽은 세기 오류이고 수가 되지 않는다.
- 세기 태스크의 자기 기한(검토 3 S2): 세기는 루프가 그 `Count`를 낸 시각(`start_ms`: `Inflight::start`의 `emitted_at`) + `request_timeout_ms` + 11,000 ms, 그 뒤에는 쪽마다 답한 시각 + 같은 덧붙임을 스스로 지킨다(루프의 쪽마다 기한과 같은 규칙). 첫 기한도 태스크가 처음 도는 때가 아니라 루프가 낸 때에서 잰다: 그렇지 않으면 첫 쪽의 기한이 낸 때부터 첫 실행까지만큼 루프의 것보다 늦어, 루프가 이미 버린 세기가 한 쪽을 더 물을 수 있다(검토 1 M1). 기한 **때나 뒤에** 답한 쪽 뒤에는 다음 쪽을 보내지 않고 `no answer before the deadline`으로 끝난다. 태스크는 쪽 k의 답 뒤 곧바로 쪽 k+1을 묻고, 그 사이에 실행이 끊을 수 있는 await가 없다(`rest.rs`의 `turn`은 가드가 받으면 곧바로 돌아온다). 그래서 끊기만으로는 늦은 쪽 뒤의 요청을 막지 못하고, 태스크 스스로 멈춘다. 기한 전에 돌아온 쪽 요청마다 `Arrived::Page`, 끝에 `Arrived::Counted`를 보낸다.

### 일괄 취소의 답(`kill::cancel_answer`, U19b2c)

- 보냈는지는 보내기 전의 클라이언트 상태로 가린다(U19b2a의 한계, 검토 1 N2): 키가 래치됐으면 `NotSent(Latched)`, 403 멈춤 중이면 `NotSent(Paused)`이고 아무것도 보내지 않는다. 연결하지 못한 요청(`RestError::NotSent`)은 `NotSent(Local)`이다. 그 밖의 오류는 보낸 것이라 `Failed`이고 루프는 그 뒤에 센다. 래치나 멈춤을 낳은 10010·403 답 자체도 `Failed`다. 멈춤은 `paused_until` 때 끝나고 그때부터 일괄 취소를 다시 보낸다(검토 1 S1).

### 요청 태스크(`kill::Inflight`, U19b2c)

- 루프가 내는 `CancelAll`과 `Count`마다 실제 `Client`로 tokio 태스크 하나를 띄우고(`start(action, emitted_at)`: `emitted_at`은 그 동작을 낸 `poll`·`arrived`의 `now`이고 세기 태스크의 첫 기한이 거기서 시작한다), 태스크가 보낸 것은 `take`로 받아 `KillLoop::arrived(now, …)`에 준다. 기한은 루프의 것이다(`KillLoop::open_request()`: 열린 요청의 `req_id`와 지금의 기한). 기한 규칙은 루프 한 곳에만 있다.
- 끊기(검토 2 S2): 걸음마다 루프의 다음 입력 전에 `abort_due(now, open_request())`를 부른다. 열린 요청의 태스크는 기한 때나 뒤에, 루프가 더는 열어 두지 않은 태스크(기한 뒤의 답이나 쪽으로 실패시켰거나 루프를 중단한 것)는 곧바로 끊는다(`AbortHandle`). 끊은 태스크가 이미 보낸 것도 버리고 루프에 주지 않는다(`stray()`에 센다). 루프가 새 요청을 내면(지난 것을 닫았을 때만 낸다) 아직 살아 있는 태스크를 먼저 끊는다. `Inflight`가 사라질 때도 모두 끊는다(요청 제한에서 기다리던 세기도 나가지 않는다; 검토 1 S2). 그래서 버린 요청은 더 보내지 않고, 루프의 요청 둘이 함께 돌지 않으며, 요청 제한의 10초에 60은 산 요청만 쓴다.
- 시험 장치(`crates/watcher/tests/common/kill_rig.rs`, 스펙 "Test rig"): 수동 시계, 실제 `Client`, 경로마다 정한 `http-fake`의 답(요청이 올 때 수동 시계를 옮길 수 있다: 느린 거래소; `on_request`가 요청마다 수동 시계를 적는다)과 걸음: `abort_due` → `poll` → 낸 요청 시작(태스크가 열려 있으면 시험 실패: 요청 둘이 함께) → 열린 요청이 낼 것을 다 낼 때까지 실제 시간으로 기다림(쪽과 답은 도착한 때의 시계로 루프에 준다; 5초를 넘으면 `the guard held a request: the rig's clock did not move`로 실패; `Answer::Hang`으로 정한 요청은 기다리지 않는다) → 시계를 `min(next_wake, now + 100)`로. 기다리는 동안 루프가 낸 요청은 앞 태스크가 끝난 뒤 다음 걸음에 시작한다. 걸음 상한 20,000(판단(모델 아님)). 그래서 어느 시험의 결과도 가짜가 얼마나 빨리 답하는지에 기대지 않고, 가짜가 적은 일괄 취소의 시각은 루프의 시계다(검토 4 S1).

### 범위

킬은 linear/USDT만 본다: 설정의 모든 기호가 USDT로 정산된다(4부 시작 5단계). 다른 정산 코인의 주문은 이 루프도 W1도 보지 않는다(표 7-8과 같다).

### 확인 안 됨(11부, U19b2a)

- `Order` 권한이 일괄 취소를 허락하는지(Q4, 권한 이름에서 한 추론): testnet 실험 몫.
- 미체결이 없을 때 일괄 취소의 답(Q11.7, 문서에 없음): 거부로 오면 `refused`이고 U19b2b의 루프는 그 뒤에도 센다.
- 500개가 넘는 미체결에서 한 번에 무엇이 취소되는지(Q11.3, 무작위): 루프가 반복한다(U19b2b).
- 실제 Bybit가 서명한 바이트와 보낸 본문이 같아야 하는지(Q1.15는 추론), 그리고 `Content-Type`을 요구하는지: 실제 호스트에 보낸 적이 없다.

### 알려진 한계(11부, U19b2a)

- 실행은 아직 `cancel_all`, `read_positions`, `notice_text`, `position_block`을 부르지 않는다(U19b3c). 키 규칙만 시작에서 돈다. U19b2c부터 시험 장치의 `Inflight`가 `http-fake`에 대해 `cancel_all`을 부른다.
- 값이 터무니없이 긴 행(수십 자리 수)이 20개면 알림이 3,500 단위를 넘을 수 있다. 그때는 송신기가 메시지를 여러 부분으로 나눌 뿐 잃지 않는다(10부). 시험은 현실적인 긴 값(8자리 소수)으로 50행을 본다.
- `cancelled`는 거래소가 받은 취소 목록의 길이다. 취소가 끝났다는 뜻이 아니다(Q11.5).
- `cancel_all`은 보내지 않은 요청(키 래치, 403 멈춤)과 보내서 10010이나 403을 받은 요청에 같은 오류(`RestError::Auth`, `RestError::IpBanned`)를 돌려준다(검토 1 N2). 그래서 U19b2c의 `kill::cancel_answer`는 `CancelAnswer::NotSent(Latched|Paused)`와 `Failed`를 보낸 뒤의 `auth_stop()`·`paused_until()`이 아니라 보내기 전의 상태로 가린다(위 "일괄 취소의 답").
- `Dec`는 소수 8자리까지만 읽는다(검토 1 N3). 9자리 이상의 `unrealisedPnl`·`markPrice`는 `모름`, `liqPrice`는 `모름(거래소가 청산가를 주지 않음)`(원인을 틀리게 말함, 수는 만들지 않음), `size`는 `수량 모름`이다. 사실 파일에는 그런 값이 없다: testnet 확인 몫.

### 검증(11부, U19b2a)

주장 V-WATCH-12. `sign::the_post_string_to_sign_matches_the_published_example_and_independent_values`(Q1.10의 서명할 문자열 그대로, 만든 비밀의 Python `hmac` 값 둘, 공백이 다른 본문은 다른 값), `client::the_only_post_is_cancel_all_with_the_fixed_body`(방법, 경로, 바이트까지 같은 본문, `Content-Type`, 머리글 다섯, 독립 값의 서명, 도착한 바이트로 다시 계산한 서명, 보내기 전에 센 요청), `client::cancel_all_answers_are_classified`(`"1"`, 없음, 빈 목록, `"0"`, 목록 아님, `result` 없음, 숫자 1, `"2"`, 503, 10010 래치 뒤 아무 길로도 보내지 않음, 403 멈춤 600초), `client::no_other_post_exists`, `config::kill_loop_fields_are_validated`, `start::a_key_that_must_cancel_needs_read_write_order_and_no_withdraw`(시작의 거부 열하나: 다섯, `ContractTrade` 밖의 `Order`(검토 1 S2), 다른 필드·목록을 읽을 수 없어도 읽히는 규칙의 거부 다섯(검토 1 S4); 읽을 수 없는 여덟, `[kill]` 없이 읽지 않음, 재확인의 규칙 멈춤 뒤 일괄 취소도 보내지 않음, 재확인의 읽을 수 없는 답은 `ip_check_ms` 뒤 다시이고 멈춤이 아님(검토 1 N4)), `decimal::percent_floor_rounds_toward_zero_and_refuses_overflow`, `kill::the_position_block_lists_size_upnl_mark_and_liquidation_distance`(읽을 수 없는 크기는 `수량 모름`이고 `포지션 없음`이 아님, 검토 1 S3), `kill::a_failed_position_read_says_unknown`, `kill::a_notice_fits_one_part_with_50_positions`, `kill::the_final_notice_names_the_stop_outcome`, `kill::a_latched_key_says_the_watcher_must_restart`, `kill::the_abort_notice_says_positions_were_not_read`, `kill::the_position_reader_pages_and_keeps_its_deadline`(두 쪽, 기한 뒤와 기한 정각의 쪽, 기한 1 ms 전, 5쪽 상한, 읽을 수 없는 쪽 셋과 503과 그 요청 수(검토 1 S1), 래치와 멈춤에서 요청 없음, 멈춤이 끝나면 다시 읽음). 변이 M265~M268과 시험 밖 변이 31개(U19b2a; 검토 1 수정의 7개 포함).

### 알려진 한계(11부, U19b2b)

- 실행은 아직 `KillLoop`를 돌리지 않는다(U19b3c). 이 부의 루프 시험은 루프를 손으로 몬다(클라이언트도 태스크도 없다). U19b2c의 시험 장치가 세기와 `Inflight`로 `http-fake`에 대해 돌린다(아래).
- 앞의 세기가 열려 있으면 지켜봄의 세기가 늦어 `watch_every_ms`보다 드물 수 있다. 끝은 여전히 `z + watch_ms` 때나 뒤에 낸 세기의 0이다.
- 라운드 수의 상한이 없다: 키가 래치되거나 403 멈춤이 길면 `NotSent` 라운드가 끝없이 돈다(요청은 나가지 않는다). 그런 루프는 워처를 다시 시작해야 끝난다(진행 알림의 고정 문구가 말한다; 런북 7.18은 U19b3d).
- 바뀐 건수는 최대 `notice_ms`(기본 60초) 늦게 알려진다(일부러 OMS와 다른 점 2).
- `poll` 한 번은 열린 요청의 기한 하나만 처리한다: 오래 `poll`하지 않은 실행은 지난 기한 하나에 한 `poll`씩 따라잡는다(그 사이 요청은 하나뿐이라 늦어질 뿐 둘이 열리지 않는다).
- `cancel_all_calls`(알림의 `일괄 취소 k회`)는 답(`Done`·`Failed`, 기한 뒤 포함)에서 센다: 일괄 취소가 열려 있는 동안의 진행·중단 알림은 이미 호스트를 떠났을 수 있는 그 하나를 빼고 센다(검토 1 N3). 낸 때 세면 `NotSent`를 다시 빼야 하므로 답에서 센다.

### 검증(11부, U19b2b)

주장 V-WATCH-12(늘림). `kill::the_loop_cancels_counts_and_ends_after_the_watch`(C2→C3→C5, 5초마다 세기 여섯, `z + 30초`의 세기에서 끝, 마지막 알림과 `Report`, 끝 뒤에는 아무것도), `kill::a_count_above_zero_waits_for_the_round`(n > 0이면 `t_r + round_ms`, 늦은 `poll`은 그 라운드의 시작을 늦춤), `kill::a_failed_cancel_all_is_still_followed_by_a_count`(503 뒤 세기; 모든 일괄 취소가 거부되고 세기는 0인 거래소에서도 루프가 `T0 + 30초`에 끝남), `kill::a_cancel_all_that_succeeds_clears_the_last_error`, `kill::a_cancel_all_not_sent_waits_for_the_next_round`(`Latched`, `Paused`, `Local` 각각: 세기 없음, 보낸 수 0, 60초의 진행 알림이 그 종류를 실음), `kill::a_failed_count_is_never_zero`(C3와 C5의 세기 오류, 다음 0의 새 `z`), `kill::a_late_order_in_the_watch_goes_back_to_cancel_at_once`, `kill::the_watch_ends_only_on_the_count_due_at_its_end`(`z + 25초`에 낸 세기가 `z + 31초`에 0으로 답해도 끝이 아님, 그 뒤 곧바로 낸 세기가 10초 늦게 답해도 끝), `kill::two_requests_of_the_loop_are_never_in_flight`, `kill::a_request_past_its_deadline_fails_and_a_late_answer_is_ignored`, `kill::an_answer_handled_at_its_deadline_is_past_it`(쪽, `count_done`, `cancel_done`을 기한 정각과 1 ms 뒤, `poll` 전에), `kill::a_slow_paged_count_is_not_failed_while_pages_answer`(11쪽 × 1,300 ms, `request_timeout_ms` 2,000: `+14,300 ms`에 결과), `kill::a_progress_notice_comes_every_60_s_also_with_an_unchanged_count`(60·120·180초; `+65초`부터의 `poll`은 `+65초`와 `+120초`; 표시 둘을 한 번에 지나면 알림 하나), `kill::the_notice_rule_sends_no_change_notice`(5·4·3·2·1로 줄어도 시작·진행 하나·마지막뿐), `kill::ten_minutes_of_failing_queries_make_at_most_12_notices`(100 ms와 7,000 ms의 `poll`: 시작 1 + 진행 10 + 마지막 1, 600초 창마다 12 이하, 마지막은 끝나는 세기의 시각), `kill::abort_sends_one_notice_and_ends`(시작 전 아무것도; 라운드, 지켜봄, 모르는 건수; 그 뒤 아무것도), `kill::the_outcome_set_during_the_loop_is_in_the_final_notice`, `kill::the_order_budget_holds_under_any_answer_sequence`(시드 xorshift의 답 순서 10,000개 × 두 타이밍, 순서마다 300걸음까지; 기본 타이밍에서 일괄 취소 약 21만 개에 예산이 잡은 것 0이고 이웃한 둘이 1,000 ms보다 가깝지 않음(검토 1 S1), 100 ms 타이밍에서 약 24만 개에 잡은 것 2,162; 어느 셋도 1,000 ms 안에 없고 요청 둘이 함께 열리지 않음). 검토 1에서 더한 것: `kill::a_late_order_in_the_watch_goes_back_to_cancel_at_once`에 늦은 주문의 라운드 뒤 n > 0이면 그 세기의 시각 + `round_ms`(S1), `kill::a_request_past_its_deadline_fails_and_a_late_answer_is_ignored`에 버린 세기의 쪽은 열린 세기의 기한을 옮기지 않음(S2), `kill::a_second_start_does_nothing`(S3), `kill::the_next_wake_is_the_earliest_due_instant_or_progress_mark`(표시가 기한보다 이르면 표시, 예산이 잡으면 예산의 시각; S4), `kill::a_watch_that_is_no_multiple_of_its_step_ends_at_watch_ms`(30,000/7,000은 `z + 7·14·21·28·30초`, 범위 밖 10,000/15,000은 `z + 10초` 하나; N1), `kill::a_cancel_all_not_sent_waits_for_the_next_round`에 보내지 않은 일괄 취소는 알던 건수를 바꾸지 않음(N2). 변이 M269~M278(스펙의 고의 결함 15~24)과 시험 밖 변이 32개(U19b2b의 `mut.py`; 31개 잡힘, 놓친 하나는 `poll`이 이미 막는 같은 검사라 코드에서 뺐다), 검토 1 뒤 41개: 검토 1의 9개를 더해 40개 모두 잡힘, `progress-after-end`는 그 검사를 뺐으므로 적용되지 않음(`3b835ab`에서).

### 알려진 한계(11부, U19b2c)

- 아직 실행은 `Inflight`를 쓰지 않는다(U19b3c): 시험 장치만 루프를 `http-fake`에 대해 돌린다. 걸린 포지션 읽기를 끊는 것도 U19b3c다.
- 세기 태스크는 쪽이 돌아온 때의 시계로 자기 기한을 옮기고, 루프는 실행이 그 쪽을 받은 때의 시계로 옮긴다. 둘의 첫 기한은 같은 시각(루프가 낸 때)에서 시작한다(검토 1 M1). 그래서 태스크의 기한은 첫 쪽부터 루프의 것보다 늦지 않다: 태스크가 먼저 멈출 수는 있어도(그 결과는 세기 오류) 루프보다 오래 묻지는 않는다. 다만 실행이 쪽을 늦게 받으면(틱 100 ms) 태스크가 제때라고 본 쪽 뒤의 한 쪽을 루프는 이미 버렸을 수 있다. 그 요청은 루프의 다음 요청이 시작될 때 끊긴다(`Inflight::start`).
- `on_page`는 기한 전에 돌아온 쪽 요청마다 불린다. 클라이언트가 보내지 않은 요청(래치, 멈춤)도 한 쪽으로 알린 뒤 그 오류가 결과다: 루프의 기한을 한 번 옮길 뿐 결과는 세기 오류다.
- 끊긴 태스크의 열린 연결은 reqwest가 닫는다. 시험은 끊긴 태스크가 클라이언트의 자기 시간 초과(실제 시간 2초) 뒤에도 아무것도 보내지 않는 것으로 본다(`stray() == 0`); 거래소 쪽에서 연결이 닫혔는지는 보지 않는다(`http-fake`는 걸어 둔 연결의 닫힘을 적지 않는다).
- 일괄 취소가 요청 제한에서 기다리는 동안 다른 태스크가 키를 래치하거나 403 멈춤을 걸면, `turn`이 보내지 않은 그 요청(`not sent: …`)을 `cancel_answer`는 `Failed`로 읽는다(보내기 전의 상태만 보므로; 검토 1 N1). 그 라운드의 `cancel_all_calls`가 하나 많고, 뒤의 세기도 보내지 않아 세기 오류이며, 그 라운드의 진행 알림은 고정 문구 대신 오류 글자를 싣는다. 다음 라운드는 바르게 가린다. 킬 동안 엔진은 멈추므로(U19b3c) 드물다. 바로잡으려면 클라이언트가 보냈는지를 돌려줘야 한다(10010·403 답과 보내지 않은 요청이 같은 오류다).
- 모르는 `orderStatus` 글자는 열리지 않은 주문으로 읽힌다(열린 것은 `w1::OPEN`의 셋뿐, 스펙의 C3과 W1의 규칙; 검토 1 N2). 끝 조건이 미체결 0인 루프에는 세기 오류로 읽는 쪽이 안전하다: 소유자 결정 몫이다. Bybit의 지금 상태 목록은 확인하지 않았다(네트워크를 쓰지 않음).
- 시험 장치에는 포지션 읽기가 없다(스펙의 시험 장치는 `/v5/position/list`와 그 결과를 기다림을 든다; 알림의 포지션 읽기는 실행(U19b3c)의 몫이고 어느 경로든 `routes`로 정할 수 있다). 5초 그물은 클라이언트 호출마다의 `tokio::time::timeout`이 아니라 열린 요청을 기다리는 전체에 하나이고 스펙의 문구로 실패한다(검토 1 N3; 요청 제한에 걸린 요청에는 같은 효과).

### 검증(11부, U19b2c)

주장 V-WATCH-12(늘림). `kill::foreign_orders_count`(두 쪽, 열린 상태 셋, 우리 링크·남의 링크·빈 링크·링크 없는 행을 모두 셈, `Filled`·`Cancelled`는 세지 않음, 0; 목록 아닌 `list`, `orderStatus` 없는 행, 커서 없는 가득 찬 쪽, 같은 커서, 503은 세기 오류와 그 요청 수), `kill::an_aborted_task_sends_no_answer_to_the_loop`(기한 1 ms 전에는 끊지 않음, 기한에 끊고 이미 온 답을 버림(루프의 `ignored()` 0, `poll`이 실패시킴), 첫 쪽이 옮긴 기한에 끊긴 세기의 그 쪽도 버림, 중단한 루프의 태스크는 기한 전에 끊음, 새 요청이 산 태스크를 끊음, 요청 아닌 동작은 태스크를 띄우지 않음), `kill::one_order_left_after_the_first_cancel_all_repeats_until_zero_is_confirmed`(완료 기준: 일괄 취소 `T0`·`T0 + 1초`, 세기 `0·1·6·11·16·21·26·31초`, 시작과 `T0 + 31초`의 마지막 알림, `Report`, 어느 `[t, t + 1,000)`에도 일괄 취소 둘 이하), `kill::a_5xx_cancel_all_or_count_repeats_until_zero_is_confirmed`(완료 기준: 503 일괄 취소 뒤에도 셈, 503 세기는 오류, 2건, 0: 라운드 셋과 `T0 + 32초`의 마지막 알림; 요청마다 900 ms 늦게 답하는 거래소에서도 초당 둘 이하), `kill::a_late_count_page_ends_the_count_task`(둘째 쪽이 14,000 ms 늦음: 끊기 없이 셋째 쪽이 없고 세기 오류, 다음 요청은 일괄 취소; 정각 13,000 ms도 기한 뒤, 12,999 ms는 제때; 1,300 ms 간격 11쪽은 실패가 아님), `kill::a_count_past_its_deadline_sends_no_further_page`(둘째 쪽 `Hang`: 첫 쪽이 옮긴 기한 `T0 + 13초`에 끊고 다음 요청은 다음 라운드의 일괄 취소, 클라이언트의 시간 초과(실제 2초) 뒤에도 끊긴 태스크가 보낸 것 없음), `kill::more_than_1000_open_orders_is_a_count_error`(50행 20쪽에 커서: 라운드 둘 × 요청 21 = 42, 10초에 60 아래), `kill::a_cancel_all_not_sent_is_told_from_the_state_before_the_call`(래치: 가짜에 요청 없음, `NotSent(Latched)`, 보낸 수 0; 연결하지 못함: `NotSent(Local)`; 받은 403은 `Failed`이고 그 멈춤 동안 세기와 다음 일괄 취소는 보내지 않음, `NotSent(Paused)`; 멈춤 끝 1 ms 전에는 보내지 않고 `paused_until` 정각에는 보냄(검토 1 S1)), `kill::a_count_task_keeps_the_loops_deadline_from_its_emission`(루프가 `T0`에 세기를 내고 태스크는 5 ms 뒤에 처음 돎: 루프의 기한 정각 `T0 + 13초`에 답한 첫 쪽 뒤 둘째 쪽 없음, `Counted`가 `no answer before the deadline`; 1 ms 전이면 두 쪽과 `Ok(1)`; 검토 1 M1), `kill::a_dropped_inflight_aborts_its_tasks`(요청 제한(수동 시계 10초에 60)에서 기다리는 세기: `Inflight`가 살아 있으면 창이 지난 뒤 나가고, 버려진 `Inflight`의 세기는 창이 지난 뒤에도 나가지 않음; 검토 1 S2). 시험 장치를 쓰는 시험은 실제 시간을 기다리므로 세 번씩 돌렸다(U19b2c 보고서). 변이 M279~M281(스펙의 고의 결함 25~27)과 시험 밖 변이 19개(U19b2c의 `mut.py`): 첫 실행에서 16개 잡힘. 놓친 셋 중 20쪽 상한의 `break`는 반복의 범위가 이미 하는 같은 검사라 코드에서 빼고 범위를 변이했고, 세기 태스크 기한의 정각과 연결하지 못한 일괄 취소의 `Local`은 시험을 더해 잡혔다(19개 모두). 검토 1 뒤 23개: 검토 1의 4개(세기 태스크의 첫 기한을 첫 실행에서 잼, `Inflight`가 낸 시각을 넘기지 않음, 403 멈춤이 끝나지 않음, `Drop`이 끊지 않음)를 더해 23개 모두 잡힘. 검토 1의 변이는 xtask 변이로 넣지 않았다: M255~M304는 스펙이 부마다 고의 결함에 매긴 번호다(M282는 U19b3a의 몫).

## 12부. W5와 OMS 멈추기(U19b3)

U19b3a가 이 부의 앞부분을 쓴다: `[w5]`와 `[kill]`의 멈춤 필드, sans-IO W5(`watcher::w5::W5`), 킬 경로의 시계(`clock::Steady`), `watcher_kill` 클라이언트(`oms::admin_kill`), OMS 단위를 멈추는 프로그램(`stop::Stopper`). 킬 순서(K1 마커 → K2 `watcher_kill` → K3 멈춤 → 루프)는 U19b3b, 실행 연결은 U19b3c·d다. **U19b3a 뒤 W5와 멈춤은 있고 시험을 지나지만, 아직 아무것도 그것을 돌리지 않는다**(실행이 W5를 무장하지 않고, `admin_kill`과 `Stopper`를 부르는 코드가 없다). 시험은 손으로 준 시각, 시험이 가진 `UnixListener`, `OMS_TEST_DIR` 아래의 셸 스크립트로만 한다. 진짜 `systemctl`은 어느 시험도 돌리지 않는다.

### `[w5]`(`config::W5`, U19b3a)

| 필드 | 기본값 | 범위(판단(모델 아님)) |
|---|---|---|
| `probe_ms` | 1,000(설계 §7.5 W5 "1초", 설계 값) | 500..=5,000 |
| `silence_ms` | 10,000(설계 §7.5 "10초 무응답", §7.6, 설계 값) | 3,000..=60,000, 그리고 `[kill]`이 있으면 `probe_ms + 2 × request_timeout_ms + 1,000` 이상 |

- 표가 없으면 기본값이다. 모르는 키는 거부한다. W5는 `[kill]`이 있을 때만 돈다.
- 아래 한계의 이유(검토 1 S6, 판단(모델 아님)): 마지막 정상 응답 `g` 뒤 잃은 탐침은 약 `g + probe_ms + request_timeout_ms`에 실패하고, 그다음 탐침이 답하는 데 `request_timeout_ms`가 걸릴 수 있다. 그래서 탐침 하나를 잃어도 침묵은 `probe_ms + 2 × request_timeout_ms`를 넘지 않는다(기본값 1,000 + 2 × 2,000 + 1,000 = 6,000 ≤ 10,000). 거부 줄은 `[w5] silence_ms must be at least probe_ms + 2 × request_timeout_ms + 1000 = <값>: one lost probe must not fire W5`다. `[kill]`이 없으면 이 한계는 읽지 않는다: W5가 꺼져 있으므로 U19a의 설정(긴 `request_timeout_ms`)을 거부하지 않는다(스펙에 없는 판단).

### `[kill]`의 멈춤 필드(`config::Kill`, U19b3a)

| 필드 | 기본값 | 받는 값 |
|---|---|---|
| `marker` | 표 안에서 필수 | OMS의 `[kill] marker` 규칙 그대로: 절대 경로, `..` 없음, 가리키는 곳이 `data_dir` 밖, 그 디렉터리가 읽을 때 있음(`oms-node/src/config.rs:91-108`) |
| `oms_unit` | 표 안에서 필수 | `[A-Za-z0-9:_.@-]` 1~255자, `.service`로 끝나고 그 앞에 이름이 있음, `-`로 시작하지 않음(systemd 단위 이름; 글롭·공백 없음) |
| `systemd` | `"system"` | `"system"` 또는 `"user"`(`systemctl --user`) |
| `stop_wait_ms` | 10,000(판단(모델 아님)) | 1,000..=120,000 |

- **mainnet은 `[kill]` 없이 시작하지 않는다**: `mainnet requires [kill]: without it the watcher cannot stop the OMS or cancel its orders`(종료 2, 소유자 질문 9). U19b2a에서 맞던 `marker`·`oms_unit` 없는 `[kill]`은 이제 `missing field`로 거부된다(그 사이에 배포하지 않았다).
- testnet은 `[kill]` 없이 시작하고 `warn` 한 줄을 남긴다: `warn: no [kill]: W5 is off; when the OMS is silent the watcher neither stops it nor cancels its orders, and no [WATCH P1] tells it`. 그러면 W5가 꺼져 있어 워처는 OMS를 멈추지도, OMS가 조용하다고 알리지도 않는다: OMS 자신의 모니터링 게이트와 관측 스택만이 신호다(검토 3 N7).
- 마커 경로는 OMS의 설정을 읽어 비교하지 않는다(독립). OMS가 읽는 경로와 같은지는 U19b3c가 `/api/status`의 `node.kill_marker`로 본다.

### W5(`watcher::w5::W5`, sans-IO)

W5는 시각, 탐침의 답, 킬의 끝을 받아 보낼 탐침, 킬을 시작하는 `Fire` 하나, 빚진 정지와 그 결과의 P1 문구를 돌려준다. 자지도, 보내지도, 시계를 읽지도 않는다. 실행(U19b3c)은 틱마다, 모든 상태에서, `clock::Steady`의 시각으로 W5를 부른다(답 → `poll` → `fire`의 순서).

| 상태 | 들어감 | 탐침 | 침묵 판단 | 나감 |
|---|---|---|---|---|
| `Off` | `[kill]` 없음 | 없음 | 없음 | 없음 |
| `Unarmed` | `[kill]`이 있고 시작 확인이 끝나기 전 | 없음 | 없음 | `arm(now)` → `Armed {now}` |
| `Armed {last_good_ms}` | `arm`, 정상 응답 | `probe_ms`마다 | 있음 | `fire` → `Killing` |
| `Killing` | `fire` | 없음 | 없음 | `kill_ended(halt_owed)` → `Rearming {halt_owed}` |
| `Rearming {halt_owed}` | 킬의 끝(중단 포함) | `probe_ms`마다 | 없음 | 정상 응답: `false`면 `Armed {now}`, `true`면 아래의 빚진 정지 |

- 무장: 실행은 시작 기계가 `Done`이나 `StayUpNoRequests`를 낼 때 `arm`한다. 그 전에는 계좌가 확인되지 않았고, 확인되지 않은 계좌로 일괄 취소가 나가면 안 된다(C-KILL의 "부트 확인 전에는 무장하지 않음"과 같다). OMS가 이 프로세스에서 한 번도 답하지 않았어도 무장한다(소유자 질문 1의 기본값): OMS가 내려간 동안 시작한 워처는 시작 확인 10초 뒤 킬한다.
- 탐침: 앞 탐침을 낸 때부터 `probe_ms`마다 하나, 둘이 함께 열리지 않는다(답이 `probe_ms`보다 늦으면 다음 탐침은 그 답이나 기한 때 간다). 탐침은 W5 자신의 `/api/status` 요청이다(엔진의 상태 요청은 세지 않는다). 낸 시각 + `request_timeout_ms` + 1,000 ms(`w5::PROBE_EXTRA_MS`, 판단(모델 아님))까지 답이 없으면 실패한 탐침이다(`poll`과 `fire`가 그 기한을 그때나 뒤에 적용한다).
- 답: `on_answer(now, req_id, good)`. `good`은 `w5::good_answer`: `oms::Status::get`이 2xx JSON을 주고 `core.instance_id`가 16자리 16진수(시작의 규칙에서 상한을 뺀 것)다. 정상 응답은 `last_good_ms = now`, 정상이 아닌 답은 실패한 탐침이고 그 밖에는 아무것도 바꾸지 않는다. 열려 있지 않은 탐침의 답은 버린다. 탐침의 답은 **온 때 받는다**: `poll`이 아직 기한을 적용하지 않았으면 기한 때나 뒤에 온 답도 받는다(OMS에 대한 사실이 먼저; 스펙 "Clock and waits"의 예외 둘 중 하나).
- 쏨: `fire(now)`는 `Armed`에서 `now − last_good_ms ≥ silence_ms`이고 **`last_good_ms` 때나 뒤에 낸 탐침 하나가 실패했을 때**(정상이 아닌 답이나 그 기한) 한 번 `Fire {last_good_ms, silent_ms}`를 내고 `Killing`으로 간다(검토 2 S3). 정상 응답과 같은 틱에 낸 탐침(낸 시각 `== last_good_ms`)이 실패해도 센다(검토 3 N1). 둘째 조건은 진짜 킬을 늦추지 않는다: `last_good_ms + probe_ms`에 낸 탐침은 `last_good_ms + probe_ms + request_timeout_ms + 1,000 ≤ last_good_ms + silence_ms`까지 실패한다(위 한계).
- 틈 규칙(검토 2 S3): W5는 앞의 `poll`·`fire`·답의 시각을 기억한다. 두 시각이 `2 × probe_ms + request_timeout_ms`(판단(모델 아님); 기본값으로 4,000 ms, 100 ms 틱에 대해)보다 멀면 그 사이는 워처가 보지 못한 시간이다(자기 프로세스가 멈췄거나 굶었음, VM 멈춤, 시계가 앞으로 뜀). W5가 가진 시각(`last_good_ms`, 열린 탐침의 낸 시각과 기한, 앞 탐침과 실패한 탐침의 낸 시각)을 모두 그 틈만큼 앞으로 옮기고 `warn` 한 줄을 남긴다: `warn: W5: <틈> ms between two polls (more than <한계> ms): the watcher did not observe that time, so it is not OMS silence`. 보지 못한 시간은 OMS의 침묵이 아니다. 틈 뒤 처음 온 것이 답이면 틈을 먼저 적용하므로 그 답의 시각이 마지막 정상 응답이다.
- 킬 뒤: `kill_ended(halt_owed)`는 `Killing`을 `Rearming {halt_owed}`로 바꾼다. 다시 지켜보기 전에 정상 응답 하나를 기다린다: 워처가 멈춘 OMS가 내려가 있는 동안 10초마다 새 킬을 시작하지 않는다.
- 지표의 값(U19b3c가 내보낸다): `watcher_w5_armed` = `armed()`(`Armed`에서만 1), `watcher_oms_silence_s` = `silent_ms(now) / 1000`(`Armed`에서만), `watcher_kill_halt_owed`의 W5 몫 = `halt_owed()`(`Rearming {halt_owed: true}`).

### 빚진 정지(W5의 몫, 검토 2 M1)

- U20b가 권한을 주기 전까지 K3은 늘 실패하고, 런북의 OMS 단위는 `Restart=on-failure`·`RestartSec=5`다(`runbook.md:102-103`; 설계는 `Restart=no`, U20b). 죽었다가 systemd가 다시 띄운 OMS는 부트 기록 전에 마커를 한 번 읽고(아마 K1이 쓰기 전에), 멈추기만 했던 OMS는 루프 뒤에 다시 돈다. 어느 쪽이든 킬 래치 없이 `/api/status`에 답한다.
- 그래서 `Rearming {halt_owed: true}`에서 첫 정상 응답은 무장하지 않는다: `on_answer`가 `OwedHalt`를 돌려주고(한 번에 하나만 진행, 진행 중에 온 정상 응답은 아무것도 내지 않는다) W5는 실행이 `owed_halt_done(now, Result<String, String>)`로 결과를 알릴 때까지 `Rearming`에 남는다.
  - 답이 `"latched"`이거나 `"already latched"`로 시작하면(`oms::latched`; OMS의 재무장 답도 여기 든다): `[WATCH P1]` 하나 `늦은 정지: OMS가 다시 답해 watcher_kill로 래치함(<답>)`과 `Armed {now}`. 앞에 실패가 있었으면 둘째 줄 `해소: <실패 문구>`가 붙는다(반복 규칙의 해소와 같은 모양).
  - 그 밖의 답(`"held"` 같은 래치가 아닌 결과, 오류 답, 소켓 없음, 시간 초과): 여전히 `Rearming {halt_owed: true}`이고 다음 정상 응답에서 다시 보낸다. 첫 실패에만 `[WATCH P1]` 하나 `늦은 정지 실패: OMS가 답하지만 watcher_kill이 안 됨(<이유>). OMS가 래치 없이 주문을 낼 수 있음`. 답과 이유는 200자로 자르고 제어 문자를 공백으로 바꾼다.
- 새 일괄 취소 루프는 시작하지 않는다: 래치된 OMS가 자기 루프를 돌린다(C-KILL). 정지를 빚진 동안 W5는 침묵을 보지 않는다(`Rearming`). 그 사이 다시 죽은 OMS는 런북의 몫이다(알려진 한계). 킬 동안의 정지 재시도는 W5가 아니라 `Kill`의 몫이다(U19b3b): W5의 빚진 정지는 정지를 빚진 채 끝났거나 중단된 킬의 것뿐이다.

### W5가 보지 못하는 것

`/api/status`는 코어 루프가 아니라 읽기 잠금 아래의 공개된 상태로 만든다(`oms-node/src/status.rs:43-46`). 그래서 W5는 OMS 프로세스가 없거나 HTTP에 더는 답하지 않을 때 쏜다. HTTP 태스크는 답하는데 코어나 파이프라인이 걸린 OMS는 W5가 보지 못한다(하트비트의 `recorded`가 빠져 `watcher_heartbeat_ok_ts`만 바뀐다). 설계가 `/api/status`를 신호로 정했으므로(§7.5 W5) 규칙은 그대로 두고 한계로 적는다(검토 1 S1, 소유자 질문 11; V-WATCH-13 `not_covered`).

### 킬 경로의 시계(`clock::Steady`, 검토 4 S3)

- `SystemClock`은 벽시계다(`clock.rs`). 시계가 Δ만큼 뒤로 가면 `saturating_sub`가 타이머를 Δ 동안 잡는다: W5는 탐침도 침묵 판단도 없고, 돌던 킬은 다음 일괄 취소·K 단계의 기한·진행 표시를 Δ 늦춘다. 사라진 OMS를 취소할 다른 것은 없다.
- `Steady`는 `Clock`을 감싼다: 안쪽 시계 + 보정값을 돌려주고, 안쪽 시계가 앞서 돌려준 값보다 낮게 읽히면 보정값을 올려 돌려주는 값이 그 값에 머물게 한다(뒤로 가는 걸음은 길이 0인 멈춤, 앞으로 가는 걸음은 그대로 지나가 W5의 틈 규칙이 다룬다). 보정값과 마지막 값은 `Mutex` 하나에 있어 그것을 읽는 모든 태스크(세기, 포지션 읽기)가 한 시간선을 본다.
- 실행(U19b3c)은 `Steady`를 W5, `Kill`(과 그 `KillLoop`), `Inflight`, 세기와 포지션 읽기, K 단계의 기한에 준다. 나머지(요청 서명: Bybit의 `recv_window`는 진짜 시각이 필요하다, KST·UTC 문구, 엔진, 송신기의 흘려보내기 간격, `Repeat`, `MismatchNotices`)는 원래 시계를 읽는다. 킬 경로의 시각을 말하는 문구는 원래 시계로 쓴다(머리의 `마지막 정상 응답`은 원래 `now − silent_ms`, 마커 문구의 UTC도 같다; U19b3b).

### `watcher_kill` 클라이언트(`oms::admin_kill(path, name, detail, timeout_ms)`)

`admin_send`(7부)와 같은 연결과 시간 규칙: 새 연결 하나, `{"id":1,"op":"hello","client":"watcher/<name>"}`, 그다음 `{"id":2,"op":"watcher_kill","detail":<detail>}`, 답마다 `timeout_ms` 안, 전체 2 × `timeout_ms` 안, 끝나면 닫는다. `result`(`"latched"`, `"already latched"`, 재무장 답) 또는 없는 이유(오류 답은 `<code>: <message>`, 소켓이 없으면 `not sent: …`, 시간 초과는 `no reply …`)를 돌려준다. OMS는 `hold_set{kill, source: watcher, note}`를 기록하고 영속한 뒤 답한다(C-HOLD "워처의 입력"). 정지됨의 규칙은 `oms::latched(reply)`: `"latched"`이거나 `"already latched"`로 시작하는 답(K2와 빚진 정지가 같이 쓴다). `detail`의 1,024바이트 상한과 문구는 U19b3b다.

### OMS 단위 멈추기(`stop::Stopper`, K3의 프로그램)

- 워처가 돌리는 프로그램은 이것 하나다: 정확히 `<systemctl> [--user] --no-ask-password stop -- <oms_unit>`(`stop::args`). 단위 이름은 설정의 규칙을 지났고 `--`가 그 앞의 옵션을 끝낸다. `<systemctl>`은 바이너리에서 `/usr/bin/systemctl`(`stop::SYSTEMCTL`)이고 시험은 `OMS_TEST_DIR` 아래의 스크립트를 준다. 소스 시험 `stop::the_watcher_starts_one_program_only`가 `crates/watcher/src/*.rs`를 글자로 읽어 `Command::new`가 `stop.rs`에 한 번만 있음을 본다.
- `stdin`·`stdout`은 null, `stderr`는 파이프이고 그 읽는 끝을 띄운 바로 뒤 비차단으로 둔다(`rustix::fs::fcntl_setfl`, `OFlags::NONBLOCK`; 바꾸지 못하면 stderr를 읽지 않는다). 실행은 틱마다 `Stopper::poll(now)`를 부르고 그것은 `try_wait`만 한다. stderr는 `try_wait`가 끝을 준 뒤에만 4 KiB까지 읽고, `WouldBlock`이나 끝에서 멈춘다: 파이프를 물려받아 열어 둔 도우미가 틱을 잡을 수 없다(검토 3 S6). 실행은 단일 스레드라 이것이 틱 안에서 하는 유일한 프로세스 호출이다(띄우기, `try_wait`, `kill`, 비차단 읽기; 어느 것도 기다리지 않는다).
- 결과는 한 번 나온다: 0으로 끝나면 `Stopped`, 다른 끝은 `Failed("exit <code>: <읽은 것의 첫 줄>")`(신호로 끝나면 `signal <n>`), 띄우지 못하면 `Stopper::start`의 오류(실패한 멈춤). 주입된 시계의 기한(K3의 `now + stop_wait_ms`) 때나 뒤에 아직 돌면 `systemctl` 클라이언트를 죽이고(`Child::kill`, SIGKILL) `Unconfirmed`다. 죽인 클라이언트는 뒤의 틱의 `try_wait`가 거둔다(`reaped()`). 막는 `wait`는 쓰지 않는다. 파이프를 끝나기 전에 채우는 클라이언트는 끝나지 않으므로 기한에서 `Unconfirmed`다. 클라이언트를 죽여도 systemd의 멈춤 작업은 취소되지 않아 OMS가 나중에 멈출 수 있다(검토 1 N5): 그래서 머리는 `실패`가 아니라 `정지 확인 못 함`이라 하고, 빚진 정지와 X8 줄에서는 실패로 센다(안전한 쪽; U19b3b).
- 워처가 멈출 수 있는 것은 설정의 그 단위 하나다. `stop` 권한(그 단위 하나의 `stop`만)은 U20b가 준다: 그때까지 K3은 실패한다(알려진 한계).

### 알려진 한계(12부, U19b3a)

- 실행은 아직 W5를 무장하지 않고 `admin_kill`과 `Stopper`를 부르지 않는다(U19b3b·c). 이 부의 W5 시험은 W5를 손으로 몬다(시계 객체 없이 시각을 준다; 하나는 `Steady`로).
- W5는 죽은 프로세스나 조용한 HTTP 태스크만 본다(위 "W5가 보지 못하는 것").
- 시작 확인이 끝나기 전에 OMS를 기다리지 않는다: 워처가 OMS보다 먼저 시작하거나 `status_url`이 틀리면 `silence_ms` 뒤 OMS를 킬한다(소유자 질문 1; 런북의 시작 순서는 U19b3d).
- 틈 규칙: 워처가 보지 못한 시간은 침묵으로 세지 않는다. 그 틈과 겹친 진짜 침묵은 틈의 끝부터 세므로 W5는 최대 틈 하나만큼 늦게 쏜다. 틈(> `2 × probe_ms + request_timeout_ms`)이 거듭되는 워처는 W5를 쏘지 않는다: 그때 OMS 자신의 모니터링 게이트(30초 하트비트 없음 → `hold:unmonitored`)만이 지킨다.
- 정지를 빚진 동안 W5는 침묵을 보지 않는다. 빚은 메모리에만 있다: 워처를 다시 시작하면 잊는다.
- 시계가 Δ만큼 뒤로 가면 W5와 킬 경로는 `Steady`로 그 걸음에서 시간이 흐르지 않은 것처럼 간다. `Steady`가 뒤로 간 걸음을 보는 것은 읽을 때뿐이다: 걸음과 다음 읽기 사이의 진행은 걸음에 묻힌다(틱마다 읽으므로 최대 한 틱). 송신기의 다시 보내기 간격, 반복 규칙, 불일치 알림 간격은 원래 시계를 읽어 Δ까지 기다린다(새 메시지는 바로 간다).
- `stop` 권한이 없으면(U20b 전) K3은 실패한다. `stop_wait_ms` 안에 끝나지 않은 `systemctl` 클라이언트는 죽이지만 systemd의 멈춤 작업은 나중에 OMS를 멈출 수 있다.
- 시스템의 `head`(uutils coreutils 0.10)는 아무도 읽지 않는 파이프에 1 MiB를 써도 막히지 않고 끝났다(이 PC에서 확인). 그래서 파이프를 채우는 시험은 셸의 내장 `printf`로 2 MiB를 쓴다.

### 검증(12부, U19b3a)

주장 V-WATCH-13. `config::kill_and_w5_are_validated_and_kill_is_required_on_mainnet`(`marker`·`oms_unit` 필수, 마커의 상대 경로·`..`·데이터 디렉터리 안(링크 포함)·없는 디렉터리, 단위 이름의 좋은 넷과 나쁜 열다섯(빈 것, `.service`만, 확장자 없음, `.target`, 공백, `*`, `?`, `[ ]`, 앞의 `-`, `--user.service`, 끝 공백, `/`, 비ASCII, 줄바꿈, 256자), `systemd`, `stop_wait_ms`의 양끝과 한 걸음 밖, 모르는 키; `[w5]` 기본값과 범위, `silence_ms`의 한계(`probe_ms` 500·`request_timeout_ms` 1,500에서 4,500은 되고 4,499는 거부, 기본값에서 6,000과 5,999, 10,000 ms 요청에서 22,000), `[kill]` 없이는 한계를 읽지 않음; mainnet은 `[kill]` 없이 거부), `config::kill_loop_fields_are_validated`(필수 둘과 기본값), `clock::a_steady_clock_does_not_step_back`(10,000 → 4,000에도 10,000, +500 → 10,500, 앞으로 뛴 걸음은 지나감, 두 번째 뒤로 감, 두 스레드가 시드 xorshift로 앞뒤로 뛰는 시계를 2만 번씩 읽어 각자 줄지 않고 뒤의 읽기 이하), `w5::w5_is_not_armed_before_arm_and_off_without_kill`, `w5::probes_are_one_per_probe_ms_and_never_two_in_flight`(50 ms 답이면 1초마다, 2,500 ms 답이면 2,500 ms마다, 모르는 id와 두 번째 답은 버림), `w5::a_lost_probe_is_failed_at_its_deadline`(2,999 ms 열림, 3,000 ms 실패, 실패한 탐침의 늦은 답은 버림, `poll` 전 기한 정각의 답은 받음, `request_timeout_ms`를 따름), `w5::ten_seconds_without_a_good_status_answer_fire_once`(완료 기준의 W5 몫: +9,999 ms 없음, +10,000 ms 한 번, 킬 동안 탐침 없음; +9,000 ms의 정상 응답이면 +19,000 ms), `w5::w5_fires_only_after_a_failed_probe`(범위 밖 타이밍으로 침묵이 지나도 열린 탐침의 기한까지 없음; 정상이 아닌 답; 같은 틱에 낸 탐침(검토 3 N1); 마지막 정상 응답보다 앞선 실패는 세지 않음), `w5::one_lost_probe_and_a_slow_answer_at_the_silence_bound_do_not_fire`(검토 1 S6, 1 ms 걸음), `w5::a_gap_in_the_watchers_own_ticks_is_not_oms_silence`(15초 틈과 앞으로 뛴 시계, `warn` 줄, 틈 끝부터 `silence_ms`, 틈 뒤 처음 온 답, 틈 앞에 실패한 탐침, 한계 정각은 틈이 아님), `w5::a_clock_step_back_does_not_blind_w5`(`Steady` 위의 W5: 60초 뒤로 간 시계에서 안쪽 진행 10초에 쏨), `w5::after_a_kill_w5_waits_for_a_good_answer_before_it_watches_again`, `w5::an_owed_halt_is_sent_at_the_first_good_answer`(`latched`, 실패 한 번의 P1, 래치 아닌 답, 재무장 답과 `해소` 줄, 200자 자름), `w5::only_a_2xx_status_with_a_16_hex_instance_id_is_good`(`http-fake`의 `/api/status`: 503, 404, JSON 아님, 15·17자리, 16진수 아님, 수, 없음), `oms::admin_kill_sends_hello_then_watcher_kill_and_reads_the_reply`(시험의 `UnixListener`: 없는 소켓, 답 다섯, 보낸 두 줄, `latched` 규칙, 시간 초과), `stop::the_stop_command_names_only_the_configured_unit`(system과 `--user`의 argv, 0으로 끝나면 `Stopped`, 없는 프로그램, 설정이 거부하는 단위 이름), `stop::the_watcher_starts_one_program_only`, `stop::a_child_that_keeps_stderr_open_does_not_block`(`sleep 30`이 stderr를 물고 1로 끝나는 스크립트: 끝난 다음 틱에 `exit 1: <첫 줄>`, 어느 `poll`도 50 ms를 넘지 않음; 2 MiB를 쓰는 스크립트: 기한 정각에 `Unconfirmed`, 뒤의 틱이 거둠; 신호 9), `runtime::a_start_refusal_ends_the_run_with_exit_code_2`(testnet의 `[kill]` 없음 `warn` 한 줄). 실제 시간을 기다리는 `stop` 시험은 세 번씩 돌렸다(U19b3a 보고서). 변이 M282~M289(스펙의 고의 결함 28~35)와 시험 밖 변이(U19b3a의 `mut.py`).
