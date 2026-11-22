# ADR-031 독립 워처: OMS 코드를 쓰지 않는 저널 읽기와 그 증명

상태: 채택. U19a1 구현(2026-11-23): crate `watcher`(라이브러리와 `--version`만 답하는 바이너리), `watcher-conformance`(적합성 시험), 독자 저널 읽기, 아는 이벤트, 소수, `Base32Pair`, 독립성 검사 `tools/ci/watcher_independent.sh`. U19a2 구현(2026-11-23): 저널 상태 `watcher::state::JournalState`(C-TAIL 표의 규칙만으로)와 코어와의 적합성 시나리오, 재생 속도 측정. 검사 W1~W4, Bybit 클라이언트, 하트비트는 U19a3~U19a9가 이 ADR에 절을 더한다. 계약: [C-WATCH](../contracts/watcher.md).

## 1. 문제와 발생 가능한 실패

- OMS는 스스로를 확인한다: 게이트웨이가 미체결·체결·포지션을 Bybit와 대조한다. U18b부터는 워처의 말(`watcher_mismatch`)로 일반 주문을 막고, 워처가 보고하지 않으면 거래를 멈춘다(모니터링 게이트, C-LIVE 2부). 그러나 그 워처가 아직 없다.
- OMS의 저널 인코딩, 대조, Bybit 응답 해석에 결함이 있으면, 같은 코드로 만든 워처는 같은 결함으로 같은 답을 낸다. 확인이 확인이 아니게 된다.
- mainnet은 모니터링 게이트 없이 시작하지 않는다(U18b4). 워처가 없으면 OMS는 거래하지 못한다.
- 독립성이 사람의 약속이면, 언젠가 편의로 `oms-journal`이나 `oms-core`를 가져다 쓰는 변경이 들어와도 아무도 모른다.

## 2. 일반적으로 만족해야 하는 성질

- 워처는 OMS와 코드를 나누지 않는다. 그 사실을 사람이 아니라 기계가 매 변경마다 확인한다.
- 워처는 OMS를 방해하지 않는다: 잠금을 잡지 않고, 파일을 쓰지 않는다(C-TAIL).
- 모르는 것은 추측하지 않는다: 모르는 이벤트, 모르는 스키마, 다른 인스턴스의 세그먼트에서 멈춘다.
- 같은 저널에서 워처와 OMS의 답이 같다는 것을 시험이 보인다(그 시험만 두 코드를 함께 링크한다).

## 3. 제약

| 사실 | 출처 |
|---|---|
| 파일 형식: 세그먼트 헤더(매직, 길이, JSON, CRC)와 프레임(길이, CRC, 번호, payload) | ADR-011 §7 |
| 읽는 규칙: 잠금 없음, 쓰기 없음, 상한 너머 금지, 회전, 헤더만 있는 세그먼트 다시 읽기, 상한 이하 손상은 오류이고 위치 고정 | C-TAIL(`docs/contracts/tail.md:7-17`) |
| 기록 형식과 이벤트 필드 표, 모르는 이벤트에서 멈춤 | C-TAIL(`tail.md:21-28`, 필드 표) |
| 워처가 OMS와 이야기하는 길: 저널 파일, `/api/status`, `admin.sock`의 `watcher_mismatch`, `monitor.sock`의 `heartbeat` | C-API, C-HOLD, C-LIVE 2부 |
| 저널은 세그먼트를 지우지 않는다 | U-04, `oms-journal` `recover` |
| 작업 공간의 crate 이름: OMS는 `oms-*`, 시세는 `md-*` | `Cargo.toml` |

## 4. 필수 조건과 선택 조건

필수: 2의 네 성질. 선택: 워처의 커서를 저장해 재시작 때 처음부터 읽지 않기(이번에 하지 않음, 8).

## 5. 대안

| 대안 | 설명 |
|---|---|
| A. `oms-journal::Tail`을 그대로 쓴다 | 이미 시험된 읽기. 워처가 OMS crate에 의존한다 |
| B. 문서만으로 쓴 독자 읽기(이 결정) | ADR-011과 C-TAIL만 보고 `crates/watcher`에 따로 쓴다 |
| C. B에 커서 저장을 더한다 | 재시작 때 처음부터 읽지 않는다. 워처가 상태 파일을 쓴다 |

독립성의 증명:

| 대안 | 설명 |
|---|---|
| a. 검토로만 지킨다 | 사람이 diff를 본다 |
| b. `cargo metadata`를 읽는 스크립트를 CI에서 돌린다(이 결정) | 선언된 의존성, 해석된 그래프, 역방향, 소스의 경로 끌어오기 |
| c. 워처를 다른 저장소로 옮긴다 | 저장소 경계가 독립성을 보장한다 |

## 6. 비교

| 기준 | A | B | C |
|---|---|---|---|
| OMS 저널 코드의 결함을 같이 갖는가 | 그렇다 | 아니다 | 아니다 |
| OMS 변경에 따라오는가 | 컴파일러가 알린다 | 필드 표 시험과 적합성 시험이 알린다 | B와 같다 |
| 재시작 비용 | 커서에 따라 | 저널 전체 재생 | 커서 다음부터 |
| 워처가 쓰는 파일 | 없음 | 없음 | 커서 파일(손상, 다른 저널의 커서) |

| 기준 | a | b | c |
|---|---|---|---|
| 몰래 들어온 의존성 | 놓칠 수 있다 | 매 변경 CI가 막는다 | 저장소가 막는다 |
| 적합성 시험(두 코드를 함께 링크) | 가능 | 가능(전용 crate 하나만 허용) | 두 저장소를 묶는 별도 장치가 필요 |
| 비용 | 없음 | 스크립트 하나와 자체 시험 22건 | 저장소, CI, 릴리스가 둘 |

## 7. 선택과 이유

B와 b. 커서 저장(C)은 하지 않는다.

- crate `watcher`: 라이브러리와 바이너리 `watcher`(`src/main.rs`는 플래그만 읽는다). 정상 의존성은 레지스트리 crate만(U19a1: `serde_json`, `crc32fast`). 작업 공간 crate는 개발 의존성 `http-fake`만 허용한다.
- crate `watcher-conformance`(`publish = false`, 빈 `lib.rs`): 워처와 `oms-core`, `oms-journal`을 함께 링크하는 유일한 패키지. 핵심 시험 도구를 `#[path]`로 가져와 코어가 결정한 기록으로 `oms-journal::Writer`가 쓴 저널을, 쓰는 쪽이 잠금을 쥔 동안 워처가 읽는다. 워처의 `Base32Pair`는 코어의 것과 같은 문자열을 만들고 같은 문자열을 받고 거절한다.
- `tools/ci/watcher_independent.sh [manifest]`: `cargo metadata --format-version 1 --locked --offline --filter-platform <host>`와 `jq`. 규칙 일곱:
  1. 패키지 `watcher`가 없으면 결과(`missing:`).
  2. `watcher`의 선언된 의존성 중 이름이 `oms-*`·`md-*`인 것(정상, 빌드, 개발, 어느 target이든; `declared:`).
  3. `watcher`의 로컬 경로 의존성 중 개발 의존성 `http-fake`가 아닌 것(`path:`).
  4. `watcher`에서 정상·빌드 간선으로 닿는 해석된 그래프 안의 `oms-*`·`md-*` 또는 작업 공간 crate(가장 짧은 경로 하나씩; `reachable:`).
  5. `watcher-conformance`가 아닌 작업 공간 crate가 `watcher`에 의존하면 결과(`reverse:`).
  6. `watcher` 디렉터리의 모든 `*.rs`에서 `#[path`, `include!`, `include_str!`, `include_bytes!`, 속성 안의 `path =`을 찾는다. 인자가 하나의 평범한 문자열이 아니거나, 절대 경로이거나, 그 파일의 디렉터리에서 풀어 패키지 밖으로 나가면 결과(`source:`). `#`·`!`·`[` 사이와 매크로 이름과 `!` 사이의 공백도 찾는다. 패키지 안의 심볼릭 링크는 가리키는 곳과 상관없이 결과(`symlink:`).
  7. `watcher` 매니페스트의 target(`[lib]`, `[[bin]]`, `[[test]]` 등의 `path`, `build`) 중 소스 파일이 패키지 디렉터리 밖인 것(`target:`). 의존성 간선도 `#[path]`도 없이 OMS 파일을 워처로 컴파일하는 길이다(검토 1 S1).
  종료 코드: 0 결과 없음(`watcher_independent: ok`), 1 결과 있음(한 줄씩, `ok`는 찍지 않음), 2 확인할 수 없음(`cannot check:`, 예: `--locked`에 맞지 않는 잠금 파일, 없는 매니페스트, `jq` 실패). CI는 1과 2 모두 실패로 본다. CI 단계 "Watcher depends on no OMS or market-data crate"는 "Build" 바로 뒤에 실제 트리에서 돈다. `tools/ci/selftest.sh`가 합성 작업 공간 `ws/`에서 22가지를 심어 스크립트가 각 경우에 맞는 줄과 종료 코드를 내는지 본다.
- `--filter-platform`인 이유: 그것 없이는 `cargo metadata --offline`이 다른 플랫폼의 패키지까지 요구해 실패한다(스펙 작성 때 101로 확인). 호스트 빌드가 받은 패키지만으로 해석된다.
- 독자 읽기 `watcher::journal::Tail`: `open(data_dir)`, `read(cap, max_records)`. 세그먼트 첫 번호, 바이트 오프셋, 다음 번호를 기억하고 프레임 하나씩 읽는다(세그먼트를 통째로 읽지 않는다). 파일은 읽기 전용으로만 열고 `LOCK`은 열지 않는다. 오류는 두 부류다: 다시 읽으면 나을 수 있는 `Io`, `Damaged`, `NoStart`는 위치를 그대로 두고 다음 호출이 다시 읽는다. 다시 읽어서는 낫지 않는 `UnknownEvent`, `Schema`, `Format`, `Instance`는 그 읽기를 프로세스가 다시 시작할 때까지 멈춘다(래치).
- 커서를 저장하지 않는다: 워처는 시작할 때마다 기록 1부터 읽는다. 첫 세그먼트가 1에서 시작하지 않는 저널은 읽지 않는다(`NoStart`). 워처는 아무 파일도 쓰지 않는다.
- 아는 이벤트 `watcher::events::KNOWN`은 C-TAIL 필드 표의 이벤트 51개다. 시험이 시험 시각에 `tail.md`를 파일로 읽어 양쪽 방향으로 비교한다(문서 사본을 소스에 넣지 않는다). OMS가 새 이벤트를 만들면 이 시험이 워처가 배울 때까지 실패한다.

## 8. 포기한 장점, 남은 위험, 가정

- 포기: 이미 시험된 `oms-journal::Tail`. 같은 규칙을 두 번 구현하고 두 번 시험한다. 적합성 시험이 두 구현이 같은 답을 내는지 본다(U19a1은 잠금 중 읽기와 식별자, U19a2는 집합).
- 포기: 재시작 비용. 저널 전체를 다시 읽는다. 저널은 하트비트(5초마다, 하루 약 17,000건)로도 자란다. U19a2의 측정(기록 10만 건, release, 페이지 캐시에 있는 파일): 초당 약 74만 건, 기록 100만 건당 약 1.3초(C-WATCH 2부 "재생 시간"). 게이트의 30초는 약 2,200만 건이다(판단(모델 아님): 단순 비례). 재생 중에는 하트비트가 없다(소유자 질문 9).
- 위험: 워처가 이미 읽은 프레임이 그 뒤에 손상되면 그 프로세스는 보지 못한다(다시 읽지 않는다). 다음 시작의 재생이 본다. C-TAIL의 "커서 앞의 손상"은 다시 읽는 쪽의 규칙이다.
- 한계(규칙 4): 해석된 그래프는 호스트 플랫폼의 것뿐이다. 다른 플랫폼의 의존성을 거쳐서만 닿는 패키지는 걷지 않는다. 워처가 그것을 직접 선언하면 규칙 2가 잡는다.
- 한계(규칙 4): 개발 간선은 걷지 않는다. 허용된 개발 의존성 `http-fake`가 뒷날 `oms-*`에 의존하면 보지 못한다(검토 1 N4).
- 한계(규칙 6): 평범한 텍스트 검색이다. 여러 줄에 걸친 속성의 `path =`(`#`와 `[`가 다른 줄인 경우도)은 보지 못하고, 주석 안의 일치도 결과다(주석을 다르게 쓴다). 매크로가 만든 경로나 다른 crate를 통한 끌어오기는 규칙 2~4의 몫이다.
- 가정: OMS가 `oms-*`, 시세가 `md-*` 이름을 지킨다. 다른 이름의 작업 공간 crate는 규칙 3·4가 막는다.

## 9. 근거

- 시험(U19a2): `crates/watcher/tests/state.rs` 10개, 적합성 `conformance::the_watcher_sets_equal_the_core_on_journals_with_every_event_kind`(이벤트 51개를 모두 쓰는 시나리오 열 개, 기록마다 코어와 비교; 주장 V-WATCH-2), 재생 측정 `replay::replay_speed_of_100000_records`. 변이 M185~M187, M210.
- 시험: `crates/watcher/tests/journal.rs` 9개, `crates/watcher/src` 단위 시험 3개(`events`, `decimal`, `wire_id`), `crates/watcher/tests/binary.rs` 1개, `crates/watcher-conformance/tests/conformance.rs` 2개, `tools/ci/selftest.sh`의 워처 22건(주장 V-WATCH-1). 변이 M182~M184, M221, M230(검토 1 S3).
- 설계 TN(암호화폐 거래 통합 설계) §7.5 독립성, `pr_plan.json` U19a, 스펙 `.scratch/watcher-core/spec.md`. 설계 문서는 저장소 밖에 있다.

## 10. 재검토 조건

- 재생 시간이 게이트의 30초에 가까워질 때(U19a2의 측정으로 약 2,200만 건): 커서 저장(C)을 다시 본다.
- 워처가 다른 플랫폼에서 돌거나, 작업 공간에 `oms-*`·`md-*`가 아닌 OMS crate가 생길 때.
- C-TAIL이 바뀔 때(새 이벤트, 새 스키마, 새 프레임 형식).
