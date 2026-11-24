# ADR-033 저장소 표준: 검사를 바꾸는 변경은 소유자가 보고, 구조와 돈 코드의 문법은 기계가 막는다

상태: 채택. 1부(소유자 확인 `tools/ci/owner_review.sh`) 구현(2026-11-24). 2부(구조 규칙 `xtask layers`), 3부(순수 crate의 lint), 4부(리뷰 규칙 `CODING_STANDARDS.md`와 주 1회 회고 `docs/agents/retro.md`) 구현(2026-11-24). main 잠금은 GitHub 설정이라 이 저장소의 파일이 아니다(7절).

## 1. 문제와 발생 가능한 실패

- 이 저장소의 변경은 거의 모두 외부 실행기(에이전트)가 쓰고, 검사(`check`)가 통과하면 실행기가 머지한다. 2026-11-16~24에 머지한 PR 48건이 모두 그렇다.
- 검사를 약하게 만드는 변경도 그 검사를 통과한다. 비밀값 허용목록에 한 줄을 더하거나, 테스트를 지우거나 `#[ignore]`하거나, crate가 작업 공간 lint에서 빠지거나, release 프로필에서 `overflow-checks`를 지우면 CI는 초록불이다. 이 PR은 사람만 잡을 수 있다.
- main에 보호가 없었다(2026-11-24 확인: rulesets 0개, `protected=false`). 직접 push, 강제 push, 삭제, 검사가 끝나기 전의 머지가 모두 가능했다.
- 에이전트는 이미 있는 코드의 모양을 베낀다. 지금 지켜지는 구조(워처는 OMS를 모른다, 순수 코어에는 시계·실수·네트워크가 없다)가 규칙으로 적혀 있지 않으면, 한 번 깨진 모양이 다음 변경에서 퍼진다.

## 2. 일반적으로 만족해야 하는 성질

- 검사 자체를 바꾸는 변경은 머지 전에 사람이 본다. 그 판단은 결정적인 스크립트가 하고, 스크립트도 심어 둔 경우로 시험한다(`tools/ci/selftest.sh`).
- 사람을 부르는 횟수는 적어야 한다. 모든 돈 코드 변경을 부르면 확인이 형식이 된다.
- 기계로 잡을 수 있는 실수는 문장 규칙이 아니라 검사로 막는다. 판단이 필요한 것만 리뷰 에이전트가 읽는 문서에 적는다.

## 3. 제약

| 사실 | 출처 |
|---|---|
| PR 48건(#1~#49, #3 제외)을 `owner_review.sh`에 대면 10건(21%)이 결과를 낸다. 7건이 `secret_scan_allow.txt` | 2026-11-24 Linux PC, 각 머지 커밋의 두 부모로 실행 |
| 경로를 주문·감시기·키·한도 파일 전부로 넓히면 40건(83%) | 같은 날 `gh pr diff` 집계 |
| PR을 여는 계정과 머지하는 계정이 같다(`junyoung7727`). GitHub은 자기 PR의 승인을 받지 않는다 | GitHub 문서, 실행기 `orch.sh` |
| 공개 저장소라 rulesets를 무료로 쓸 수 있다. `bypass_actors`가 비면 관리자도 `--admin` 머지가 거부된다 | 2026-11-24 `ruleset-drill` 연습(PR #50, #51) |
| CI 단계 `check`(GitHub Actions, `integration_id` 15368)가 유일한 필수 검사다 | `.github/workflows/ci.yml`, check-runs API |

## 4. 필수 조건과 선택 조건

필수: 검사를 바꾸는 변경이 소유자 확인 없이 머지되지 않는다(실행기가 지키는 수준이라도). 판단이 결정적이고 시험된다. 실행기의 자동 머지는 그대로 돈다.
선택: GitHub이 강제한다(계정 분리가 필요). 돈 코드의 다른 변경도 가끔 사람이 본다.

## 5. 대안

1. 그대로 둔다(문장 규칙만).
2. 돈 코드 경로 전부를 소유자 확인으로 한다.
3. 검사를 바꾸는 변경만 소유자 확인으로 하고, 다른 돈 코드는 하루 한 건을 골라 카드만 보낸다.
4. 3에 더해 실행기에 별도 봇 계정을 주고 `CODEOWNERS`와 ruleset의 코드 소유자 리뷰로 GitHub이 강제한다.

## 6. 비교

| | 사람 호출 비율 | 강제 주체 | 비용 |
|---|---|---|---|
| 1 | 0% | 없음 | 없음. 1절의 실패가 그대로 |
| 2 | 83%(측정) | 실행기 | 거의 모든 PR이 사람을 기다린다 |
| 3 | 21%(측정) | 실행기 | 스크립트 하나와 시험 |
| 4 | 21% | GitHub | 봇 계정과 토큰 관리(소유자 결정 필요) |

## 7. 선택과 이유

3을 택한다. 4는 소유자가 봇 계정을 정하면 옮긴다(재검토 조건).

- main 잠금(GitHub ruleset `main-lock`, 대상 기본 브랜치): `deletion`, `non_fast_forward`, `pull_request`(승인 0명), `required_status_checks`(`check`, `strict` 꺼짐), `bypass_actors` 없음. 승인 0명이라 실행기의 자동 머지는 그대로다. `strict`를 끄면 따로 통과한 두 PR이 합쳐서 깨질 수 있지만, 실행기는 PR을 하나씩 머지하고 main push에서도 CI가 돈다. 연습 브랜치에서 직접 push, 강제 push, 삭제, 검사 전 머지, 빨간 PR 머지(`--admin` 포함)가 모두 거부됐다.
- 1부 `tools/ci/owner_review.sh <base>`: 규칙 넷(`path`, `manifest`, `lint`, `tests`)을 `<base>...HEAD`에 적용한다. 종료 코드 0 결과 없음(`owner_review: none`), 1 결과 있음(한 줄씩), 2 확인할 수 없음. 경로 목록은 `tools/ci/owner_review_paths.txt`이고, 목록과 스크립트가 `tools/ci/` 아래에 있어 그것을 바꾸는 변경도 결과가 된다. 새 crate가 `[lints] workspace = true`만 갖는 것은 결과가 아니다. 시험 파일(`tests/` 등)의 crate 수준 `allow`는 읽지 않는다(관례로 `#![allow(clippy::unwrap_used)]`로 시작한다). 새 시험을 처음부터 `#[ignore]`하는 것은 결과가 아니고, 같은 파일이 더하는 시험보다 많은 `#[ignore]`는 결과다. 둘째 인자로 경로 목록 파일을 받는다: 머지하는 실행기는 기준 브랜치(main)의 목록을 넘겨, 검토 대상 변경이 자기 목록을 줄여 빠져나가지 못하게 한다. CI는 이 스크립트를 실패로 쓰지 않는다. 실행기가 머지 전에 돌리고, 결과가 있으면 머지를 멈추고 소유자를 부른다.
- 검사 자체의 시험: `tools/ci/selftest.sh`의 "owner review" 절이 버려지는 저장소 `or/`에서 31가지를 심는다(결과 없음 7, 결과 22, 확인 불가 2).
- 2부 구조 규칙: `tools/ci/layers.toml`의 규칙을 `cargo run -p xtask -- layers`가 CI 단계 "Crate layering table"(워처 독립 검사 바로 뒤)에서 확인한다. 규칙 넷: `only`(이 crate는 목록 밖의 작업 공간 crate에 의존하지 않는다), `deny`(이 crate들은 저 crate들에 의존하지 않는다, `except` 가능), `external`(작업 공간 밖의 normal·build 의존성은 목록만), `text`(이 문자열은 이 경로 밖의 `.rs`에 없다, 주석 포함). 의존성은 `cargo metadata --no-deps`의 선언된 것이고 종류(normal, dev, build)와 플랫폼을 가리지 않는다. 표의 이름이 어느 crate와도 맞지 않으면 결과다. 2026-11-24의 모양을 잠근다: 순수 crate(`oms-core`, `md-core`)는 작업 공간 crate에 의존하지 않고 `oms-core`의 외부 의존성은 `serde`, `serde_json`뿐; `oms-journal`·`oms-sim`·`oms-api`는 코어만; `oms-node`는 코어, 저널, API, 가상 거래소, 직접 알림만(읽기 쪽 `oms-projector`의 f64 손익이 판단에 닿지 않는다); 시세 crate는 `oms-*`에 의존하지 않고, `oms-node`에는 워처의 적합성 시험만 의존한다; `/v5/order/create`·`/v5/order/amend`는 Bybit 게이트웨이와 그 시험에만, `/v5/order/`는 거기와 워처와 `xtask/src/mutants.rs`에만, `"/exchange"`는 Hyperliquid 게이트웨이에만 있다. 워처 자신의 규칙은 `watcher_independent.sh`에 그대로 둔다. 시험은 `crates/xtask/src/layers.rs`의 단위 시험(심은 경우: 시세 crate가 코어에 의존(세 종류), 셸이 읽기 쪽에 의존, 코어가 `tokio`에 의존(normal·build는 결과, dev는 아님), 예외는 그 crate에만, 없는 이름, 모르는 필드, 게이트웨이 밖의 주문 경로)과 실제 트리를 실제 표로 돌리는 시험이다. 이전부터 있던 `xtask verify`의 `core_purity`(코어 소스의 `std::time`·`f64` 등 문자열과 의존성)는 CI에서 돌지 않았다. 2부와 3부가 그 내용을 CI에 옮긴다.
- 3부 lint: `oms-core`의 `lib.rs`가 `#![deny(..)]`로 `float_arithmetic`, `expect_used`, `cast_possible_truncation`, `cast_sign_loss`, `cast_possible_wrap`, `disallowed_methods`, `allow_attributes`, `allow_attributes_without_reason`을 건다. 예외는 `#[expect(lint, reason = "..")]`로만 두고, 위반이 사라지면 `unfulfilled_lint_expectations`가 `-D warnings`로 실패한다. 처음 걸었을 때 실제 코드의 위반은 하나(`ids.rs`의 `v as u64`, cloid의 아래 64비트, 이유를 단 `expect`)였다. 시계 호출 목록(`SystemTime::now`, `Instant::now`, `tokio::time::Instant::now`)은 루트 `clippy.toml`에 있고, `clippy.toml`은 작업 공간 전체에 적용되므로 `[workspace.lints.clippy]`에서 `disallowed_methods = "allow"`로 두고 순수 crate(`oms-core`, `md-core`)의 `lib.rs`에서만 `deny`로 켠다(셸 crate의 시계 호출은 2026-11-24에 121곳). `md-core`는 시계 규칙만 건다: 감시용 `mid`·`spread_bp`의 실수 2곳과 `decimal.rs`의 변환 4곳은 따로 정한다. 넘침은 release의 `overflow-checks = true`와 `panic = "abort"`가 이미 멈춤으로 바꾸므로 `arithmetic_side_effects`(코어 67곳)는 걸지 않는다. 이 설정을 바꾸는 변경은 1부의 `path`·`manifest`·`lint` 결과다.

- 4부 리뷰 규칙과 회고: 판단이 필요한 규칙만 루트의 `CODING_STANDARDS.md`에 근거(건수와 기간)와 함께 두고, 리뷰 에이전트가 읽는다(구현 에이전트는 읽지 않아도 된다). 첫 세 규칙은 실행기의 리뷰 29건(U18b~U19b, 2026-11-24까지)의 MUST·SHOULD 70건에서 왔다: 규칙에 시험이 없음 34건, 문서의 수와 주장이 코드와 다름 13건, 시계 7건. 기계적인 지적은 문장이 아니라 검사와 심은 경우로 바꾼다(`docs/agents/retro.md`, 주 1회).

## 8. 포기한 장점, 남은 위험, 가정

- 강제는 실행기가 한다. 같은 계정을 쓰는 사람이나 다른 프로그램은 이 확인을 거치지 않고 머지할 수 있다(ruleset은 검사 통과만 강제한다).
- 텍스트 검사다. 이유 문자열에 `]`가 든 lint 속성은 그 `]`까지 읽고, 두 줄에 걸친 시험 속성은 세지 않는다. 한 파일에서 기존 시험 하나를 `#[ignore]`하면서 새 시험 하나를 더하면 결과가 없다.
- 돈 코드의 검사를 약하게 하지 않는 잘못(틀린 로직)은 이 확인의 대상이 아니다. 그것은 시험, 변이 기록, 독립 리뷰가 맡는다.

## 9. 근거

- 2026-11-24 측정: `RESEARCH/AUTO_PR_HUMAN_GATE_20261122/31_REPO_STANDARDS_DETAIL_20261124.md`(소유자 작업 공간, 저장소 밖).
- Lauren Tan(Grok 엔지니어)의 발표: 기계적인 지적은 검사로 바꾸고, 구조 규칙이 가장 강한 강제라는 경험담. Matt Pocock `retro` 스킬: 기계적 위반은 결정적 검사, 판단은 리뷰 에이전트가 읽는 `CODING_STANDARDS.md`.

## 10. 재검토 조건

소유자가 실행기용 봇 계정이나 GitHub App을 정할 때(대안 4로), 결과 비율이 한 주 동안 40%를 넘을 때, 사람이 아닌 다른 머지 경로가 생길 때.
