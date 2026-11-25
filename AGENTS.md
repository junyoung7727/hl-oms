# hl-oms: 어디에 무엇이 있나

| 경로 | 내용 |
|---|---|
| `CONTRIBUTING.md` | 작업 규칙(저장소, registry, 문서) |
| `CODING_STANDARDS.md` | 코드 검토 기준 |
| `README.md` | 무엇인가, 빌드·실행, 수용 시험 |
| `docs/explanation.md` | 상황별 동작 설명(응답이 안 오는 주문, 프로세스가 죽음, …)과 시험 ID |
| `docs/generated/requirements.md` | 요구 R01~R15별 확인 현황(로직 / 실제 Bybit), 생성됨 |
| `docs/generated/` | registry에서 만든 표(주장과 결과, 가정, 사용자 결정, 추적표), 생성됨 |
| `registry/requirements.toml` | 요구 R01~R15와 연결된 주장, 시험 없는 규칙. 정본 |
| `registry/claims.toml` | 검증 주장: 무엇을, 어떤 조건에서, 어떤 시험으로 |
| `registry/assumptions.toml` | 외부(거래소, OS)에 기대는 가정과 확인 여부 |
| `registry/user-decisions.toml` | 소유자가 정한 것과 정해야 하는 것 |
| `registry/contracts.toml` | 저널 이벤트 → 계약 → ADR → 코드 → 주장 색인 |
| `registry/considerations-*.toml`, `coverage.toml`, `sources.toml` | 검토 항목과 출처 |
| `docs/adr/` | 설계 결정(고치지 않음) |
| `docs/operations/` | 운영 절차서(주인, 검토일) |
| `docs/research/`, `docs/verification/`, `docs/performance/` | 조사 기록, 검증 방법, 성능 측정 |
| `GLOSSARY.md` | 용어 |
| `.scratch/<기능>/` | 디자인 문서(`spec.md`)와 작업 티켓(`issues/`). 규칙: `docs/agents/issue-tracker.md`, `docs/agents/triage-labels.md`, `docs/agents/domain.md`. 회고: `docs/agents/retro.md` |
| `crates/oms-core` | 결정적 코어(I/O·시계·스레드 없음) |
| `crates/oms-node` | 실행 계층과 게이트웨이(가상, Hyperliquid, Bybit), `omsctl`, `simvenue` |
| `crates/oms-sim` | 결정적 시뮬레이션(장애 주입) |
| `crates/http-fake` | loopback 가짜 HTTP/WS 서버(시험용) |
| `crates/watcher` | 독립 워처(OMS 코드에 의존하지 않음) |
| `crates/xtask` | 수용 시험, 변이 점검, 성능, 문서 생성기 |
| `tools/ci/` | CI 검사 스크립트와 그 자체 시험(`selftest.sh`). 소유자 확인이 필요한 경로: `owner_review_paths.txt`, 층 규칙: `layers.toml` |
