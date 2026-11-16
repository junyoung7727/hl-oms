# 출처 요소별 대응표

> 자동 생성 문서다. 고치려면 `registry/*.toml`을 고치고 `cargo run -p xtask -- docs`를 실행한다.
> 소스 다이제스트 `624fe48b9f4ed0a6`, 검증 결과의 다이제스트 `0a86a80a92f2313c`.

검토 기준의 모수다. 출처가 나누어 둔 요소마다 이 프로젝트가 어디에서 다뤘는지, 다루지 않았다면 왜인지를 적는다.
요소 목록은 [조사 기록](../research/standards-sources.md) §3과 [Hyperliquid 조사](../research/hyperliquid-protocol.md) §2에서 왔다. 표준의 본문을 읽지 못한 요소는 제목만으로 판단했고 그렇게 적었다.

요소 102건: 다룸 81 · 비적용 13 · 보류 4 · 미검토 4.

| 뜻 | |
|---|---|
| 다룸 | 검토 항목이 있다. 그 항목의 설계·구현·검증 상태는 [검토 항목 대응표](considerations.md)에 있다. 다뤘다는 것이 구현했다는 뜻은 아니다 |
| 비적용 | 이 프로젝트에 해당하지 않는다. 이유를 적는다 |
| 보류 | 해당하지만 지금 다루지 않는다. 영향과 재개 조건을 적는다 |
| 미검토 | 판단하지 못했다. 이유를 적는다 |

| 출처 | 요소 | 상태 | 검토 항목 | 비고 |
|---|---|---|---|---|
| ISO25010 | Functional suitability: correctness, completeness | 다룸 | G-QUAL-01, D-RISK-01, D-ORD-01, D-TEST-01 |  |
| ISO25010 | Functional suitability: appropriateness (운영 명령이 비상 절차를 덮는가) | 다룸 | G-QUAL-04, D-RISK-07, G-OPS-06 | 비상 절차의 훈련은 하지 않았다 |
| ISO25010 | Performance efficiency: time behaviour, capacity | 다룸 | G-QUAL-02, P-PERF-01, D-OVL-01 |  |
| ISO25010 | Performance efficiency: resource utilization | 다룸 | G-RES-01, G-RES-02 |  |
| ISO25010 | Compatibility: interoperability | 다룸 | P-VEN-01, G-QUAL-03 |  |
| ISO25010 | Compatibility: co-existence (같은 호스트의 전략·대시보드와 자원 공유) | 보류 |  | 영향: 전략 프로세스가 CPU나 디스크를 다 쓰면 OMS의 응답 지연이 늘고 디스크가 차면 신규 주문이 막힌다. 측정하지 않았다. 재개 조건: 배포 구성(같은 호스트에 무엇을 함께 돌릴지)이 정해질 때 |
| ISO25010 | Interaction capability: operability, user error protection, self-descriptiveness | 다룸 | G-QUAL-04, G-OPS-08 |  |
| ISO25010 | Interaction capability: learnability, user engagement, inclusivity, user assistance | 비적용 |  | 운영자 1인의 내부 도구다 |
| ISO25010 | Reliability: recoverability | 다룸 | G-REL-01, G-REL-02, D-PER-01, D-REC-01 |  |
| ISO25010 | Reliability: fault tolerance | 다룸 | D-UNK-01, D-DUP-02, D-DUP-03, G-REL-05 |  |
| ISO25010 | Reliability: faultlessness | 다룸 | D-TEST-01, G-QUAL-08 |  |
| ISO25010 | Reliability: availability | 다룸 | G-REL-03, D-AVL-01 | 단일 노드와 수동 복구로 범위를 정했다 |
| ISO25010 | Security: integrity | 다룸 | D-PER-03, G-OPS-03, G-SEC-05 |  |
| ISO25010 | Security: confidentiality | 다룸 | G-SEC-03 |  |
| ISO25010 | Security: authenticity | 다룸 | G-SEC-01 |  |
| ISO25010 | Security: accountability | 다룸 | G-SEC-05 |  |
| ISO25010 | Security: non-repudiation | 비적용 |  | 운영자와 자금주가 같은 사람이다 |
| ISO25010 | Security: resistance | 다룸 | G-RES-03, D-OVL-01, G-SEC-13 | 노출면은 로컬 소켓과 loopback이다 |
| ISO25010 | Maintainability: modularity, analysability, modifiability, testability | 다룸 | G-QUAL-05, D-DET-01 |  |
| ISO25010 | Maintainability: reusability | 보류 |  | 영향: 없음. 재개 조건: 두 번째 거래소를 붙일 때 |
| ISO25010 | Flexibility: adaptability, replaceability | 다룸 | G-QUAL-06 |  |
| ISO25010 | Flexibility: installability | 다룸 | G-OPS-02, P-OPS-03 |  |
| ISO25010 | Flexibility: scalability | 비적용 |  | 단일 계좌·단일 노드로 범위를 정했다(ADR-001) |
| ISO25010 | Safety: fail safe, operational constraint | 다룸 | G-QUAL-07, D-RISK-07, G-REL-05 | 금전 손실이 25010의 Safety 정의에 드는지는 본문을 읽지 못했다 |
| ISO25010 | Safety: hazard warning, risk identification | 다룸 | G-OPS-05 | 알림은 구현하지 않았다 |
| ISO25010 | Safety: safe integration | 다룸 | D-OWN-01, G-SEC-01, G-QUAL-06 |  |
| ISO42010 | Stakeholders, Concerns (6.2, 6.4 제목) | 다룸 | G-ARCH-01 |  |
| ISO42010 | Stakeholder perspectives, Aspects (6.3, 6.5 제목) | 미검토 |  | 2022판의 새 개념이고 본문을 읽지 못해 무엇을 요구하는지 모른다 |
| ISO42010 | Viewpoints, Views (6.6, 6.7 제목) | 다룸 | G-ARCH-01 |  |
| ISO42010 | View components, Model kinds (6.8, 8.2 제목) | 미검토 |  | 본문을 읽지 못했다. 표기 규약을 따로 정하지 않았다 |
| ISO42010 | Correspondences (6.9 제목) | 다룸 | G-QUAL-01 |  |
| ISO42010 | Decisions and rationale (6.10 제목) | 다룸 | G-ARCH-01, D-LMAX-01 |  |
| ISO42010 | Conformance (4), ADF, ADL (7) | 비적용 |  | 적합성을 주장하지 않는다. 자체 프레임워크나 언어를 정의하지 않는다 |
| SSDF | PO.1.1, PO.1.2 보안 요구의 문서화 | 다룸 | G-SEC-01, G-SEC-11 |  |
| SSDF | PO.1.3 상용 공급자에게 요구 전달 | 비적용 |  | 상용 공급 계약이 없다 |
| SSDF | PO.2 역할, 교육, 경영진 의지 | 비적용 |  | 1인 조직이다. 운영자·개발자·승인자가 같은 사람이라는 점은 P-OPS-01에 위험으로 적었다 |
| SSDF | PO.3 도구 체인과 그 산출물 | 다룸 | G-SEC-04, G-QUAL-05, G-CHG-02 | 의존성 점검 도구는 설치돼 있지 않다 |
| SSDF | PO.4 릴리스 전 보안 점검 기준 | 다룸 | G-SEC-11 |  |
| SSDF | PO.5 개발·시험·운영 환경 분리 | 다룸 | P-VEN-09, G-SEC-12 |  |
| SSDF | PS.1 소스와 설정의 접근 통제 | 다룸 | G-SEC-08 | 저장소가 아직 없다(U-10) |
| SSDF | PS.2 릴리스 무결성 검증 정보 | 다룸 | G-SEC-08 |  |
| SSDF | PS.3 릴리스 보관, provenance, SBOM | 다룸 | G-SEC-08, G-OPS-02 | SBOM은 만들지 않았다 |
| SSDF | PW.1.1, PW.1.2 위협 모델, 보안 요구·위험·결정의 추적 | 다룸 | G-SEC-11 |  |
| SSDF | PW.1.3 표준 보안 기능의 사용 | 다룸 | G-SEC-06 | TLS는 라이브러리, 서명은 SDK 벡터로 대조한 자체 구현 |
| SSDF | PW.2 설계 검토 (설계자가 아닌 사람 또는 자동화) | 다룸 | G-QUAL-08 |  |
| SSDF | PW.4.1, PW.4.4 의존성의 선택과 유지 | 다룸 | G-SEC-04 |  |
| SSDF | PW.4.2 자체 공통 컴포넌트 | 비적용 |  | 다른 프로젝트와 공유하는 컴포넌트가 없다 |
| SSDF | PW.5 secure coding practice | 다룸 | G-SEC-10, G-QUAL-05, G-SEC-02 |  |
| SSDF | PW.6 컴파일러와 빌드 설정 | 다룸 | G-QUAL-05, G-REL-05 |  |
| SSDF | PW.7 코드 리뷰와 분석 | 다룸 | G-QUAL-08, G-QUAL-05 |  |
| SSDF | PW.8 실행 코드 시험 | 다룸 | D-TEST-01, G-SEC-02 | fuzzing 도구는 쓰지 않았다 |
| SSDF | PW.9 안전한 기본 설정 | 다룸 | G-SEC-07 |  |
| SSDF | RV.1.1, RV.1.2 취약점 정보 수집과 확인 | 다룸 | G-SEC-04 |  |
| SSDF | RV.1.3 취약점 공개 정책 | 비적용 |  | 비공개 1인 소프트웨어다. 공개 저장소가 되면 다시 본다 |
| SSDF | RV.2 취약점 평가와 조치 | 다룸 | G-SEC-09 | 절차를 정하지 않았다 |
| SSDF | RV.3 근본 원인 분석과 유사 결함 제거 | 다룸 | G-SEC-09, G-QUAL-08 | 간격 검토에서 찾은 결함마다 같은 종류의 결함을 다시 찾지는 않았다 |
| SSDF | v1.2 초안의 PO.6, PS.4 | 비적용 |  | 초안이다 |
| ASVS | 목표 레벨의 선택 | 보류 |  | 영향: 어느 요구까지 만족해야 하는지의 기준이 없다. 재개 조건: 원격 접근이나 다중 사용자가 생길 때. 지금은 로컬 소켓과 loopback뿐이다 |
| ASVS | V3, V4 브라우저 경유 접근(교차 출처, CSP, 허용 메서드) | 다룸 | G-SEC-13 |  |
| ASVS | V6~V9 인증, 세션, 인가 (제어 API) | 다룸 | G-SEC-01 | 네트워크가 아니라 소켓 파일 권한과 peer uid로 판별한다. 토큰은 없다 |
| ASVS | V12 전송 구간 보호 | 다룸 | G-SEC-06 | 로컬 소켓과 loopback 구간은 암호화하지 않는다 |
| ASVS | V13 설정과 비밀 정보 | 다룸 | G-SEC-03, G-SEC-07 |  |
| ASVS | V15.2.3 시험·개발용 코드가 운영 빌드에 없는가 | 다룸 | G-SEC-12 |  |
| ASVS | V15.4 동시성 (L3) | 다룸 | D-CONC-01, D-OWN-01 |  |
| ASVS | V16 로그와 오류 처리 (오류 중 거래 처리 금지) | 다룸 | G-SEC-05, D-PER-02, G-REL-05 |  |
| ASVS | V5, V10, V17 파일 처리, OAuth, WebRTC | 비적용 |  | 그 기능을 쓰지 않는다 |
| ASVS | V2.3.5 고가치 흐름의 다자 승인 | 비적용 |  | 운영자가 한 명이다. 위험한 명령의 확인 절차는 G-OPS-08에 미해결로 적었다 |
| FIX-OSC | 2장 OrdStatus 우선순위 | 다룸 | D-ORD-01 | 순서 관계만 참고했다. 거래소의 상태 집합은 다르다 |
| FIX-OSC | Pending Cancel ≠ Canceled | 다룸 | D-RISK-02, D-ORD-02 |  |
| FIX-OSC | B.1.b, B.1.c 취소 대기 중 체결, 취소가 늦은 경우 | 다룸 | D-ORD-02 |  |
| FIX-OSC | B.1.d, B.1.e 접수 확인 전 취소 | 다룸 | D-ORD-02 |  |
| FIX-OSC | B.1.f, G.1.a 모르는 주문에 대한 취소·조회 응답 | 다룸 | D-UNK-01 |  |
| FIX-OSC | C, D 정정(cancel/replace) | 다룸 | D-ORD-03 | 정정을 지원하지 않기로 했다. C.1–2, D.1은 제목만 봤다 |
| FIX-OSC | E 요청 없는 취소·축소 | 다룸 | P-VEN-07 |  |
| FIX-OSC | F.1.a, F.1.b, Session 4.9 클라이언트 ID 중복과 재송신 | 다룸 | D-DUP-01, D-ORD-04 |  |
| FIX-OSC | PossDupFlag와 PossResend | 다룸 | D-DUP-01 | 필드는 없다. 중복 판정은 응용 계층의 책임이라는 원칙만 가져왔다 |
| FIX-OSC | G.1.c 조회 응답과 사건 스트림의 관계 | 다룸 | D-REC-01 |  |
| FIX-OSC | H 거래일 경계의 GT 주문 | 비적용 | D-ORD-05 | 24시간 시장이다. 본문은 제목만 봤다 |
| FIX-OSC | I TimeInForce별 종료 상태 | 미검토 |  | 제목만 봤다. IOC의 종료 상태는 거래소 문서와 실주문으로 확인해야 한다(P-VEN-01) |
| FIX-OSC | J 체결 취소·정정 | 다룸 | D-ORD-03 | 지원하지 않고 포지션 대조로 탐지한다 |
| FIX-OSC | K 거래 정지 | 다룸 | P-VEN-13 | 제목만 봤다. 거래소의 실제 동작은 확인하지 못했다 |
| FIX-OSC | A, L, 4장 | 미검토 |  | 제목만 봤다. 주제가 메시지 축약 방식 등이라 관련이 적다고 판단했다 |
| FIA24 | Best Practices for Automated Trading Risk Controls (기존 분석의 요구 R01~R15로 재사용) | 다룸 | D-RISK-01, D-RISK-02, D-RISK-03, D-RISK-04, D-RISK-05, D-RISK-06, D-RISK-07, D-OVL-03, D-REC-01 | 이번에는 표지와 목차만 다시 확인했다. 내용은 기존 저장소의 분석 문서에 기댄다 |
| FIA15 | Guide to the Development and Operation of Automated Trading Systems | 다룸 | G-OPS-01, G-OPS-02, G-OPS-03, G-OPS-04, G-OPS-05, G-OPS-06, G-SEC-01, G-SEC-05 | 이번에는 표지와 서문만 다시 확인했다 |
| BRIEF | 규제 문서(RTS 6, SEC 15c3-5, 가상자산이용자보호법)의 적용 여부 | 보류 | P-OPS-04 | 영향: 의무를 놓칠 수 있다. 조문 일부만 읽었고 적용 여부는 법률 검토가 필요하다. 재개 조건: U-12 |
| HL-DOCS | A. 주문 제출 | 다룸 | P-VEN-01 |  |
| HL-DOCS | B. 취소, 정정, 예약 취소 | 다룸 | P-VEN-11, D-ORD-03 |  |
| HL-DOCS | C. nonce와 API 지갑 | 다룸 | P-VEN-03, P-VEN-12 |  |
| HL-DOCS | D. Info 조회 | 다룸 | P-VEN-06, D-REC-01 |  |
| HL-DOCS | E. WebSocket | 다룸 | D-DUP-03, D-DUP-02 |  |
| HL-DOCS | F. 요청 한도 | 다룸 | P-VEN-04, P-OPS-02 |  |
| HL-DOCS | G. 가격·수량 단위 | 다룸 | P-VEN-05 |  |
| HL-DOCS | H. 오류 응답 | 다룸 | D-UNK-01, P-VEN-01 |  |
| HL-DOCS | I. 거래 의미론(청산, 자기 체결, 거래소 주도 취소, 상장 폐지) | 다룸 | P-VEN-07, P-VEN-08, D-RISK-06, P-VEN-13 |  |
| HL-DOCS | J. 서명 | 다룸 | P-VEN-02 |  |
| HL-DOCS | K. 엔드포인트와 testnet | 다룸 | P-VEN-09 |  |
| HL-DOCS | L. 전달 보장과 순서 | 다룸 | D-DUP-02, D-DUP-03 |  |
| HL-DOCS | spot, vault, staking, HIP-3 | 비적용 |  | perp 단일 계좌만 다룬다. 이 절들은 읽지 않았다. 같은 계좌의 spot 체결은 기장하지 않고 경고만 남긴다(P-VEN-13) |
| LMAX | 단일 writer, 입력 저널, 스냅샷, 복제, Disruptor | 다룸 | D-LMAX-01, D-OWN-01, D-PER-01, D-PER-04, D-OPT-01 |  |
| LINUX | fsync, fdatasync, rename, 디렉터리 fsync의 계약 | 다룸 | D-PER-02, D-PER-03, D-PER-04, G-REL-04 |  |
| FSYNC-RESEARCH | fsync 실패 뒤의 동작, 파일 시스템별 충돌 일관성 | 다룸 | D-PER-02, D-PER-03, G-REL-04 |  |
| RUST | 표준 라이브러리와 tokio의 동시성·취소 계약 | 다룸 | D-CONC-01, D-CONC-02, D-CONC-03 |  |
