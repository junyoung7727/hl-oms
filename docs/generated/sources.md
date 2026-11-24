# 검토 기준의 출처

> 자동 생성 문서다. 고치려면 `registry/*.toml`을 고치고 `cargo run -p xtask -- docs`를 실행한다.
> 소스 다이제스트 `29ace372034370f6`, 검증 결과의 다이제스트 `762a4acf55af90fb`.

| ID | 자료 | 판 | 역할 | 실제 접근한 범위 | 주의 |
|---|---|---|---|---|---|
| ISO25010 | ISO/IEC 25010:2023 Product quality model | 2판, 2023-11 (2011판은 폐지) | 품질 분류 모델 | 초록, 수명주기 정보, 미리보기(목차·서문·도입·적용 범위). 3~4절 본문은 읽지 못했다(유료). 하위 특성 40개의 목록과 정의는 iso25000.com에서 가져왔다(2차 자료). | 누락 점검용 분류 축으로만 쓴다. 준수나 인증을 주장하지 않는다. Safety가 금전 손실을 포함하는지는 본문 미확인. |
| ISO42010 | ISO/IEC/IEEE 42010:2022 Architecture description | 2판, 2022-11 | 아키텍처 기술 기준 | 초록, IEEE SA 페이지, 미리보기(전체 목차·서문·도입·적용 범위). 절 본문은 읽지 못했다(유료). 동반 사이트는 2011판 기준이다.  | 이해관계자·관심사·뷰·결정 근거를 구분해 문서를 구성하는 데만 쓴다. 적합성을 주장하지 않는다. |
| SSDF | NIST SP 800-218 Secure Software Development Framework | v1.1, 2022-02. Rev.1(v1.2)은 2025-12 초안이며 확정 여부 미확인 | 보안 개발 관행 | v1.1 전문(36쪽). 관행 PO.1–PO.5, PS.1–PS.3, PW.1–PW.9, RV.1–RV.3.  | 관행 목록이지 구현 의무가 아니다. 어떤 관행을 수행했는지만 적는다. |
| ASVS | OWASP Application Security Verification Standard | 5.0.0, 2025-05 | 보안 검증 요구 | 요구사항 CSV 전체(345건)와 서문·레벨·인증 장. V1, V5, V10, V11, V17의 개별 요구는 자세히 읽지 않았다.  | 웹/API 표면(대시보드, 로컬 API)에 해당하는 요구만 골라 점검 축으로 쓴다. 레벨 충족을 주장하지 않는다. |
| FIX-OSC | FIX Order State Changes | FIX Latest EP284, 2023-11 (57쪽) | 참조 의미 | PDF 전체를 추출했으나 정독한 것은 1~2장, 3.1 개요, 시나리오 B.1.b–f, C.3.c, D.2.d, E.1.b, F.1.a–b, G.1.a, G.1.c, J.1.a–b, J.1.d다. 나머지 시나리오 묶음(A, C.1–2, D.1, H, I, K, L)은 제목만 봤다. 세션 계층 표준은 3.1.13, 4.8.4, 4.8.7, 4.9만.  | 대상 거래소는 FIX가 아니다. 취소 중 체결, 종결 뒤 정정 같은 사례의 존재 근거로만 쓴다. |
| FIA24 | FIA Best Practices for Automated Trading Risk Controls and System Safeguards | 2024-07 (22쪽) | 산업 권고 | 기존 프로젝트 문서 작성 때 전문 열람. 이번에는 URL 도달 가능 여부와 쪽수만 확인.  | 권고이며 이 시스템에 대한 의무가 아니다. 요구로 옮긴 결과는 docs/architecture/FIA-OMS-model.md의 R01–R15다. |
| FIA15 | FIA Guide to the Development and Operation of Automated Trading Systems | 2015-03 (48쪽) | 산업 권고 | FIA24와 같다.  | FIA24와 같다. |
| LMAX | Martin Fowler, The LMAX Architecture / LMAX Disruptor technical paper | 2011 | 원저작 | Fowler 글 전문, Disruptor 논문과 사용자 안내서. 글의 그림은 읽지 못했다.  | 성능 수치는 2011년 Java 환경의 것이다. 이 프로젝트의 성능 근거로 쓰지 않는다. 재생 시 출력 억제와 시계 처리는 원문에 없다. |
| HL-DOCS | Hyperliquid 공식 문서 (GitBook) | 버전 표기 없음. 2026-10-02에 54쪽을 내려받음 | 프로토콜 계약 | websocket, nonce, rate limit, tick/lot, 오류, 서명 쪽은 전부. exchange 쪽은 부분(서두, order, cancel, cancelByCloid, scheduleCancel, modify 등 주문 관련 액션만). info 쪽은 부분(openOrders, userFills, userFillsByTime, userRateLimit, orderStatus, historicalOrders, meta, clearinghouseState 등 perp 관련만. spot·vault·staking은 읽지 않음). Discord와 Telegram 공지는 읽지 않았다.  | 문서에 없는 동작은 가정하지 않는다. 문서 자체에 모순이 있다(주문의 sz 의미, 페이지 크기). |
| HL-SDK | hyperliquid-python-sdk (공식) | commit 2fdb18f, 2026-06-04 | 프로토콜 계약 | signing.py, exchange.py, 예제, tests/signing_test.py.  | 서명 절차는 문서가 아니라 이 SDK가 사실상의 명세다. 공식 Rust SDK는 2025-10 이후 커밋이 없고 expiresAfter가 없어 쓰지 않았다. |
| LINUX | Linux man-pages: fsync(2), fdatasync(2), open(2), write(2), rename(2); ext4 커널 문서 | man7.org 현재판; 커널 문서는 7.3-rc5 표기(호스트는 7.0) | 시스템 계약 | 해당 쪽 전문.  | 장치가 flush를 지키는지는 소프트웨어로 확인할 수 없다. |
| FSYNC-RESEARCH | PostgreSQL wiki Fsync Errors; Rebello 외 (USENIX ATC'20); Pillai 외 (OSDI'14); SQLite Atomic Commit | 각 문서의 현재판 | 연구 | 위키 전문, 논문 초록과 핵심 결과, SQLite 문서 해당 절.  | Pillai의 ext4 결과는 2014년 커널 기준이며 7.0에서 다시 확인하지 않았다. |
| RUST | Rust 표준 라이브러리와 tokio 문서 | Rust 1.96.0, tokio 1.53.1 | 시스템 계약 | File::sync_data, try_lock, tokio mpsc와 select!의 취소 안전성 절.  | std가 EINTR에서 재시도한다는 점은 문서가 아니라 소스에서 확인했다. |
| REQ | 기존 프로젝트 요구 문서 | docs/architecture/OMS.md (2026-09-28), FIA-OMS-model.md, docs/research/oms-three-guarantees.md | 프로젝트 요구 | 전문.  | Go 구현을 전제로 쓴 문서다. 요구 R01–R15와 정책 결정은 계승하고 구현 서술은 계승하지 않는다. |
| BRIEF | 프로젝트 지시문 | 2026-10-02 | 프로젝트 요구 | 전문.  |  |
| PRACTICE | 일반 엔지니어링 관행 | — | 일반 관행 | —  | 출처가 문서가 아니라 관행인 항목임을 표시한다. |
