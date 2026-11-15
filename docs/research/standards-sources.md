# 검토 기준 출처 조사 (standards-sources)

기준일: 2026-10-02 (KST) 열람. 대상: Rust OMS (단일 운영자, 암호화폐 무기한 선물 venue 우선, LMAX식 단일 writer 코어, 로컬 저널, 로컬 HTTP 대시보드/운영자 제어 API).

상태: **출처 조사 및 매핑 제안**. 이 문서는 어떤 표준·규정에 대한 준수, 적합성, 인증도 주장하지 않는다. 아래에 적힌 조항·ID·인용은 모두 이번 세션에서 실제로 내려받아 읽은 페이지/PDF에서 온 것이다. 읽지 못한 부분은 "원문 미확인"으로 적었다.

표기:

- **1차**: 발행 기관 자체 문서(또는 공식 배포처의 공식 미리보기)를 직접 읽음.
- **2차 자료**: 발행 기관이 아닌 곳의 요약·해설. 조항 근거로 쓰지 않는다.
- **추론**: 원문에 없는, 이 조사자의 해석. 결정은 리드 엔지니어 몫이다.
- 인용은 원문 언어 그대로, 각 30단어 이하.

---

## 1. 접근 기록 표

| # | 출처 | 판/버전/일자 | 가져온 URL | 접근일 | 접근 범위 | 역할 | 주의 (주장하면 안 되는 것) |
|---|---|---|---|---|---|---|---|
| 1a | ISO/IEC 25010 | Edition 2, 2023-11 (IEC 웹스토어 표기 2023-11-15), 22쪽, stage 60.60. 2011판은 Withdrawn | `https://committee.iso.org/standard/78176.html` (200) | 2026-10-02 | **abstract only** + 상태/생애주기 메타데이터 | 품질 분류 모델 | 본문 조항 번호 인용 금지. "25010 적합/준수" 금지 |
| 1b | 〃 공식 미리보기 PDF | ISO/IEC 25010:2023(E) 앞 9쪽 | `https://www.vde-verlag.de/iec-normen/preview-pdf/info_isoiec25010{ed2.0}en.pdf` (200, 공식 배포처 VDE) | 2026-10-02 | **partial**: 표지, 목차, Foreword, Introduction, 1 Scope, 2, 3의 첫 문단. 정의·4절·부속서 본문은 없음 | 〃 | 특성·부특성의 **정의**와 4절 구조는 1차 원문 미확인 |
| 1c | 〃 IEC 웹스토어 | Ed. 2.0, 2023-11-15, 22쪽, ICS 35.080 | `https://webstore.iec.ch/en/publication/90024` (200) | 2026-10-02 | abstract + 서지 | 〃 | — |
| 1d | iso25000.com "ISO/IEC 25010" | 페이지 하단 © 2024, 판 표기 없음 (9개 특성 기술) | `https://iso25000.com/index.php/en/iso-25000-standards/iso-25010` 및 `?start=5` (200) | 2026-10-02 | 2쪽 전부 | **2차 자료** (특성·부특성 목록과 정의 문구) | ISO 공식 사이트가 아니다. 문구를 "ISO 원문"이라 부르지 않는다 |
| 1e | iso.org 표준 페이지 / OBP | — | `https://www.iso.org/standard/78176.html`, `https://www.iso.org/standard/74393.html`, `https://www.iso.org/obp/ui/en/#iso:std:iso-iec:25010:ed-2:v1:en` | 2026-10-02 | **not accessible** (HTTP 403, Cloudflare "Just a moment…" 챌린지; curl·WebFetch 모두) | — | OBP 미리보기를 읽었다고 쓰지 않는다 |
| 2a | ISO/IEC/IEEE 42010 | Edition 2, 2022-11, 62쪽, stage 60.60. 2011판 Withdrawn | `https://committee.iso.org/standard/74393.html` (200) | 2026-10-02 | abstract only + 메타데이터 | 아키텍처 기술(記述) 표준 | 본문 "shall" 요구를 읽지 않았다. 조항 **제목**만 안다 |
| 2b | 〃 공식 미리보기 PDF | ISO/IEC/IEEE 42010:2022(E) 앞 9쪽 | `https://www.vde-verlag.de/iec-normen/preview-pdf/info_isoiecieee42010{ed2.0}en.pdf` (200) | 2026-10-02 | **partial**: 표지, 전체 목차(조항 제목), Foreword(2011 대비 변경), Introduction, 1 Scope, 2, 3.1 | 〃 | 5~8절 본문, 4 Conformance 본문, 부속서 원문 미확인 |
| 2c | IEEE SA 페이지 | IEEE/ISO/IEC 42010-2022, Active, Board Approval 2022-09-21, Published 2022-11-07, Superseding 42010-2011 | `https://standards.ieee.org/ieee/42010/6846/` (200) | 2026-10-02 | abstract + 서지 | 〃 | — |
| 2d | 동반 사이트 iso-architecture.org | 홈은 "ISO/IEC/IEEE 42010:2011" 사이트로 자기소개. `cm/`에 2022판 변경 요약 1개 절 + 2011판 개념 모델 설명 | `http://www.iso-architecture.org/42010/`, `…/42010/cm/`, `…/42010/faq.html` (200) | 2026-10-02 | 홈·개념모델 페이지 전부, FAQ 머리말(Version 5.7, 2013-11-07) | 표준 작업반 측 해설 (준-1차) | 개념 설명 대부분이 **2011판 기준**이다. 2022판 조항으로 인용 금지 |
| 3a | NIST SP 800-218 (SSDF v1.1) | Final, February 2022, 36쪽 (CSRC 이력: 02/03/22 Final) | `https://nvlpubs.nist.gov/nistpubs/SpecialPublications/NIST.SP.800-218.pdf` (200), `https://csrc.nist.gov/pubs/sp/800/218/final` (200) | 2026-10-02 | **full text** (PDF 전체 추출, Table 1 전 행 확인) | 안전한 개발 실무(practices) | 인증 체계가 아니다. "SSDF 준수" 주장 금지. 미 연방 조달용 자가증명과 무관 |
| 3b | NIST SP 800-218 Rev. 1 (SSDF v1.2) | **Initial Public Draft**, 2025-12-17, 57쪽, 의견수렴 2026-01-30 마감 | `https://csrc.nist.gov/pubs/sp/800/218/r1/ipd` (200), `https://nvlpubs.nist.gov/nistpubs/SpecialPublications/NIST.SP.800-218r1.ipd.pdf` (200) | 2026-10-02 | partial: CSRC 페이지 전부, PDF에서 Note to Reviewers와 practice/task ID 목록만 | 〃 (초안) | 초안이다. 기준으로 채택하지 않는다. `…/218/r1/final`은 오늘 404였다 |
| 3c | NIST SP 800-218A | Final, July 2024 (07/26/24) | `https://csrc.nist.gov/pubs/sp/800/218/a/final` (200) | 2026-10-02 | abstract only | 생성형 AI 모델 개발용 SSDF Community Profile | 본 OMS와 주제가 다르다 (비적용 후보) |
| 4 | OWASP ASVS | 5.0.0 (README: "dated May 2025"; GitHub 릴리스 `v5.0.0_release` 2025-05-30). 다음 목표는 5.0.1 패치(미발행). `latest` 태그는 master 자동 빌드(2026-09-03)로 안정판이 아님 | `https://raw.githubusercontent.com/OWASP/ASVS/master/README.md`, `https://api.github.com/repos/OWASP/ASVS/releases`, `https://github.com/OWASP/ASVS/raw/v5.0.0/5.0/docs_en/OWASP_Application_Security_Verification_Standard_5.0.0_en.csv`, `https://raw.githubusercontent.com/OWASP/ASVS/v5.0.0/5.0/en/0x02-Preface.md`, `…/0x03-What-is-the-ASVS.md`, `…/0x04-Assessment_and_Certification.md` (모두 200) | 2026-10-02 | **full text** (요구사항 CSV 345행 전부 + 서문 3개 장). 장별 도입부 산문과 부록은 내려받았으나 정독하지 않음 | 보안 검증 요구사항 | "ASVS 인증/Level N 달성" 금지. 요구 ID는 반드시 `v5.0.0-` 접두 |
| 5a | FIX Latest — Order State Changes | "FIX Latest as of EP284", November 2023, 57쪽 (사이트 표기 Last Updated 2023-11-19) | `https://fixtrading.org/packages/fix-latest-specification-order-state-changes/` → 다운로드 링크 (200, PDF) | 2026-10-02 | **full text** 추출. 정독은 1~2장, 3.1 개요, 시나리오 B.1.b–f, C.3.c, D.2.d, E.1.b, F.1.a–b, G.1.a, G.1.c, J.1.a–b, J.1.d | 참조 의미론 (대상 venue는 FIX가 아님) | 프로토콜 적합성 주장 금지. venue의 실제 동작 근거로 쓰지 않는다 |
| 5b | FIX Session Layer Technical Standard | "Errata November 2020", 82쪽 | `https://fixtrading.org/packages/fix-session-layer-technical-standard/` → 다운로드 (200, PDF) | 2026-10-02 | partial: 3.1.13, 4.8.4, 4.8.7, 4.9, 4.9.1 첫 문단 | 〃 | 나머지 절 미독 |
| 5c | FIX Latest 필드 정의 (Orchimate) | FIX.Latest | `https://orchimate.org/fixtrading/fix-latest/fields/ClOrdID` 외 `PossDupFlag`, `PossResend`, `OrigClOrdID` (200; `fiximate.fixtrading.org`가 여기로 리다이렉트) | 2026-10-02 | 해당 필드 설명문 | 〃 | — |
| 5d | 과제 지시의 원 URL | — | `https://www.fixtrading.org/online-specification/order-state-changes/` | 2026-10-02 | **not accessible** (404, 사이트 개편) | — | 이 URL을 근거로 적지 않는다 |
| 6a | FIA "Best Practices For Automated Trading Risk Controls And System Safeguards" | July 2024, 22쪽 (PDF 생성 2024-07-17) | `https://www.fia.org/sites/default/files/2024-07/FIA_WP_AUTOMATED%20TRADING%20RISK%20CONTROLS_FINAL_0.pdf` (200) | 2026-10-02 | 도달 확인 + 표지/목차만. 내용 분석은 기존 문서 참조 | 업계 권고 | FIA 인증·준수 주장 금지 |
| 6b | FIA "Guide to the Development and Operation of Automated Trading Systems" | March 2015, 48쪽 (PDF 생성 2015-03-24; URL의 2019는 게시 경로) | `https://www.fia.org/sites/default/files/2019-09/FIA-Guide-to-the-Development-and-Operation-of-Automated-Trading-Systems_1.pdf` (200) | 2026-10-02 | 도달 확인 + 표지/서문만 | 업계 권고 | 〃 |
| 7a | Commission Delegated Regulation (EU) 2017/589 (MiFID II RTS 6) | 2016-07-19 채택, OJ L 87/417 (2017-03-31), 2018-01-03 적용 | `https://publications.europa.eu/resource/celex/32017R0589` (200, EU 간행물청). `eur-lex.europa.eu`는 202/빈 응답 | 2026-10-02 | partial: 제목, 전 조문 **표제**, Art. 1 도입부, Art. 12, Art. 29 | 규제(EU 투자회사 대상) | 법적 적용 여부를 단정하지 않는다. 통합본(개정 반영) 여부 미확인 |
| 7b | SEC Rule 15c3-5 (17 CFR 240.15c3-5) | eCFR "current" (2026-10-02 조회분) | `https://www.ecfr.gov/api/renderer/v1/content/enhanced/current/title-17?part=240&section=240.15c3-5` (200) | 2026-10-02 | (a)~(d)(2) 정독, 이후 미독 | 규제(미 broker-dealer 대상) | 〃 |
| 7c | 가상자산 이용자 보호 등에 관한 법률 (약칭 가상자산이용자보호법) | [시행 2026. 10. 2.] [법률 제21857호, 2026. 8. 4., 타법개정], 소관 금융위원회 | `https://www.law.go.kr/법령/가상자산이용자보호등에관한법률` → `https://www.law.go.kr/LSW/lsInfoR.do` (lsiSeq=290735) (200) | 2026-10-02 | partial: 조문 목차(제1~22조), 제1·2·3·4조, 제10조 ①~⑤ 일부 | 법률(국내) | 법률 결론 금지. 시행령·감독규정 미확인 |

내려받은 사본은 세션 임시 디렉터리에만 있고 저장소에 넣지 않았다. 나중에 같은 파일인지 대조할 수 있도록 SHA-256 앞 16자리를 남긴다: SSDF v1.1 `617746e553a9e2da`, SSDF v1.2 초안 `0f40af24b5d0175c`, FIX OSC EP284 `cf6e439697bfb91c`, FIX Session `526e60993fce12bd`, 25010 미리보기 `6f113c2b610f5701`, 42010 미리보기 `4da25d521c575ea4`, FIA 2024 `acf403a653f958a8`, FIA 2015 `ef8576aadac2b688`, ASVS 5.0.0 CSV `98c8fe911b9edb40`.

---

## 2. 출처별 정리

### 2.1 ISO/IEC 25010:2023 — 품질 분류 모델

**판 확인 (1차).** 현행은 2판(2023-11)이고 2011판은 폐지 상태다. ISO 위원회 페이지의 Life cycle에 "Previously Withdrawn ISO/IEC 25010:2011 / Now Published ISO/IEC 25010:2023, Stage 60.60"으로 나온다. (`https://committee.iso.org/standard/78176.html`)

**2011 대비 변경 (1차, Foreword — 공식 미리보기 PDF).**

- > "This second edition of ISO/IEC 25010, together with the first edition of ISO/IEC 25002 and the first edition of ISO/IEC 25019, cancels and replaces ISO/IEC 25010:2011"
- 2011판 중 제품 품질 모델만 이 문서에 남았다. 나머지는 ISO/IEC 25002(품질 모델 개요·사용)와 ISO/IEC 25019(quality-in-use 모델)로 옮겨졌다.
- > "Safety has been added as a quality characteristic with subcharacteristics, i.e. operational constraint, risk identification, fail safe, hazard warning and safe integration."
- > "Usability and portability have been replaced with interaction capability and flexibility respectively."
- > "Inclusivity and self-descriptiveness, resistance, and scalability have been added as subcharacteristics of interaction capability, security, and flexibility respectively."
- > "User interface aesthetics and maturity have been replaced with user engagement and faultlessness respectively."
- > "Accessibility has been split into inclusivity and user assistance."
- 대상이 ICT 제품과 정보시스템 전반으로 넓어졌고, 여러 특성·부특성의 이름과 정의가 다듬어졌다.

출처: `https://www.vde-verlag.de/iec-normen/preview-pdf/info_isoiec25010{ed2.0}en.pdf` (Foreword, p. iv–v)

**범위 (1차, 1 Scope).**

> "The product quality model is composed of nine characteristics (which are further subdivided into subcharacteristics) that relate to quality properties of the products."

**목차 (1차).** 1 Scope / 2 Normative references / 3 Terms and definitions / 4 Product quality model (4.1 structure, 4.2 targets) / 5 Relationship to the quality-in-use model / Annex A (2011판 비교) / Annex B (dependability 매핑 예) / Annex C (측정에 쓰는 법) / Annex D (이해관계자 관점). 부속서는 모두 informative. Normative reference는 없다고 적혀 있다.

**특성·부특성 목록 (2차 자료: iso25000.com).** 9개 특성, 40개 부특성. Foreword(1차)가 직접 이름을 댄 항목에는 †를 붙였다. 나머지 이름과 모든 정의 문구는 1차 원문으로 확인하지 못했다.

| 특성 | 부특성 |
|---|---|
| Functional Suitability | Functional completeness, Functional correctness, Functional appropriateness |
| Performance Efficiency | Time behaviour, Resource utilization, Capacity |
| Compatibility | Co-existence, Interoperability |
| Interaction Capability† | Appropriateness recognizability, Learnability, Operability, User error protection, User engagement†, Inclusivity†, User assistance†, Self-descriptiveness† |
| Reliability | Faultlessness†, Availability, Fault tolerance, Recoverability |
| Security | Confidentiality, Integrity, Non-repudiation, Accountability, Authenticity, Resistance† |
| Maintainability | Modularity, Reusability, Analysability, Modifiability, Testability |
| Flexibility† | Adaptability, Scalability†, Installability, Replaceability |
| Safety† | Operational constraint†, Risk identification†, Fail safe†, Hazard warning†, Safe integration† |

출처: `https://iso25000.com/index.php/en/iso-25000-standards/iso-25010`, `…/iso-25010?start=5`

참고 인용 (2차 자료, iso25000.com의 Recoverability 설명):

> "Degree to which, in the event of an interruption or a failure, a product or system can recover the data directly affected and re-establish the desired state of the system."

주의할 점 두 가지.

- iso25000.com은 Safety를 "human life, health, property, or the environment"가 위험해지는 상태를 피하는 정도로 설명한다. 금전 손실이 여기의 property에 드는지는 원문 정의를 읽어야 판단할 수 있다. 원문 미확인.
- 같은 페이지의 "User assistance" 설명문은 접근성 정의처럼 읽힌다. 2차 자료의 문구 오류일 수 있으므로 정의 근거로 쓰지 않는다.

### 2.2 ISO/IEC/IEEE 42010:2022 — 아키텍처 기술 표준

**판 확인 (1차).** 2판, 2022-11, 62쪽. 2011판을 대체한다. IEEE 쪽 명칭은 IEEE/ISO/IEC 42010-2022 (Published 2022-11-07). 제목이 "Systems and software engineering — Architecture description"에서 "Software, systems and enterprise — Architecture description"으로 바뀌었다. (`https://committee.iso.org/standard/74393.html`, `https://standards.ieee.org/ieee/42010/6846/`)

**이 표준이 다루는 것과 다루지 않는 것 (1차, Scope).**

> "This document distinguishes the architecture of an entity of interest from an AD expressing that architecture. Architectures are not the subject of this document."

> "This document does not specify the processes, architecting methods, models, notations, techniques or tools by which an AD is created, utilized or managed."

> "This document does not specify any format or media for recording an AD."

즉 "좋은 아키텍처"의 기준이 아니라, 아키텍처를 **적은 문서(AD)** 가 갖춰야 할 구조에 대한 요구다. 적합성 대상은 AD, ADF, ADL, architecture viewpoint, model kind다 (Scope).

**조항 제목 (1차, 목차만 확인. 본문 요구는 원문 미확인).**

- 4 Conformance
- 5 Conceptual foundations: 5.2.1 Context of architecture description / 5.2.2 Architectures and architecture descriptions / 5.2.3 Stakeholders and concerns / 5.2.4 Stakeholder perspectives / 5.2.5 Aspects / 5.2.6 Architecture considerations / 5.2.7 Architecture views and architecture viewpoints / 5.2.8 Model kinds, legends and architecture view components / 5.2.9 Architecture description (AD) elements / 5.2.10 View methods / 5.2.11 AD element correspondence / 5.2.12 Architecture decisions and rationale / 5.3 AD in the life cycle / 5.4 ADF와 ADL
- 6 Specification of an architecture description: 6.1 AD identification and overview / 6.2 Identification of stakeholders / 6.3 Identification of stakeholder perspectives / 6.4 Identification of concerns / 6.5 Identification of aspects / 6.6 Inclusion of architecture viewpoints / 6.7 Inclusion of architecture views / 6.8 Inclusion of view components / 6.9 Recording of architecture correspondences (6.9.1 Consistency within an AD, 6.9.2 Correspondences, 6.9.3 Correspondence methods) / 6.10 Recording of architecture decisions and rationale (6.10.1 Decision recording, 6.10.2 Rationale recording)
- 7 ADF와 ADL의 명세 / 8 Architecture viewpoints and model kinds (8.1, 8.2, 8.3 View methods)
- Annex A–F (모두 informative)

**2011 대비 변경 (1차, Foreword).**

- "system of interest" → "entity of interest" (EoI).
- "architecture framework" → "architecture description framework" (ADF).
- > "The term “architecture view component” is introduced as a separable portion of one or more architecture views, replacing “architecture model” in the 2011 edition."
- Aspect와 stakeholder perspective가 정식 개념으로 정의되었다.
- Model kind가 새 적합성 대상으로 추가되었다.
- 그림 표기가 UML 클래스 다이어그램에서 비형식 ER 표기로 바뀌었다.

**동반 사이트가 설명하는 판.** 홈은 여전히 2011판 사이트라고 소개한다.

> "This is the website for ISO/IEC/IEEE 42010:2011"

2022판 발행 소식(2022-11-30)과 "FAQ, 템플릿, 서지를 갱신할 예정"(2023-04-18)이라는 공지가 있고, FAQ는 Version 5.7 (2013-11-07)이다. `cm/` 페이지에는 2022판 변경 요약 절이 하나 있고 나머지는 2011판 개념 모델 설명이다. (`http://www.iso-architecture.org/42010/`, `…/cm/`)

**핵심 개념 (동반 사이트, 2011판 설명 기준).** 2022판 용어와의 대응을 괄호에 적었다.

| 개념 | 동반 사이트의 설명 요지 |
|---|---|
| Stakeholder | 대상에 concern을 가진 개인·집단·조직 |
| Concern | 대상에 대한 모든 이해관심. 예: purpose, functionality, structure, behavior, cost, supportability, safety, interoperability |
| Architecture Viewpoint | 한 종류의 view를 만들고 해석하고 분석하는 규약의 집합. 특정 concern 집합을 frame한다 |
| Architecture View | 하나 이상의 stakeholder 관점에서 특정 concern을 다루도록 아키텍처를 표현한 것. viewpoint의 규약을 따른다 |
| Architecture Model (2022: View Component) | view를 구성하는 단위 |
| Model Kind | 한 종류의 모델에 대한 규약 |
| AD Element / Correspondence / Correspondence Rule | AD 안의 모든 항목 / AD element 사이의 관계 / 그 관계를 강제하는 규칙 |
| Architecture Decision / Rationale | 결정은 AD element에 영향을 주고 concern에 속한다. 결정이 새 concern을 낳을 수 있다 |

> "Correspondences and Correspondence Rules are used to express and enforce architecture relations such as composition, refinement, consistency, traceability, dependency, constraint and obligation within or between ADs."

> "Architecture Rationale records the explanation, justification or reasoning about Architecture Decisions that have been made and architectural alternatives not chosen."

2022판에 대해 같은 페이지가 말하는 것:

> "This 2nd edition introduces Stakeholder Perspectives as a means to group Concerns and therefore to organize Viewpoints framing those Concerns."

출처: `http://www.iso-architecture.org/42010/cm/`

### 2.3 NIST SP 800-218 (SSDF v1.1) — 안전한 개발 실무

**판 확인 (1차).** 현행 최종본은 v1.1 (February 2022, CSWP 13을 대체). 후속으로 SP 800-218 Rev. 1 = SSDF **v1.2 Initial Public Draft**가 2025-12-17에 나왔고 의견수렴은 2026-01-30에 끝났다. 오늘 `…/218/r1/final`은 404였다. 최종본이 다른 경로로 나왔는지는 확인하지 못했다. SP 800-218A (July 2024)는 생성형 AI 모델 개발용 Community Profile이다.

**성격 (1차).**

> "Not all practices are applicable to all use cases; organizations should adopt a risk-based approach to determine what practices are relevant, appropriate, and effective"

> "The intention of the SSDF is not to create a checklist to follow, but to provide a basis for planning and implementing a risk-based approach"

출처: `https://nvlpubs.nist.gov/nistpubs/SpecialPublications/NIST.SP.800-218.pdf` (Section 1–2). 각 practice는 Practice / Task / Notional Implementation Example / Reference로 정의된다. 예시는 필수가 아니라고 프로젝트 페이지가 밝힌다 (`https://csrc.nist.gov/projects/ssdf`).

**Table 1 전 항목 (1차).** practice 제목은 원문 그대로, task는 한 줄 요약(조사자 번역).

Prepare the Organization (PO)

| ID | Practice 제목 / Task 요약 |
|---|---|
| **PO.1** | Define Security Requirements for Software Development |
| PO.1.1 | 개발 인프라와 프로세스에 대한 보안 요구를 식별·문서화하고 유지 |
| PO.1.2 | 조직이 개발하는 소프트웨어가 충족할 보안 요구를 식별·문서화하고 유지 |
| PO.1.3 | 재사용할 상용 컴포넌트를 공급하는 제3자에게 요구를 전달 (구 PW.3.1) |
| **PO.2** | Implement Roles and Responsibilities |
| PO.2.1 | SDLC 전 부분을 포괄하도록 역할·책임을 만들고 주기적으로 검토 |
| PO.2.2 | 안전한 개발에 관여하는 인원에게 역할 기반 교육 제공, 주기적 검토 |
| PO.2.3 | 상위 관리자의 안전한 개발 의지를 확보하고 전달 |
| **PO.3** | Implement Supporting Toolchains |
| PO.3.1 | 위험 완화를 위해 각 toolchain에 넣을 도구(유형)와 통합 방식을 명시 |
| PO.3.2 | 도구와 toolchain을 권장 보안 실무에 따라 배포·운영·유지 |
| PO.3.3 | 도구가 실무 수행의 증거(artifact)를 생성하도록 설정 |
| **PO.4** | Define and Use Criteria for Software Security Checks |
| PO.4.1 | 소프트웨어 보안 점검 기준을 정의하고 SDLC 전반에서 추적 |
| PO.4.2 | 기준을 뒷받침할 정보를 수집·보호하는 절차와 수단 구현 |
| **PO.5** | Implement and Maintain Secure Environments for Software Development |
| PO.5.1 | 개발에 관여하는 각 환경을 분리하고 보호 |
| PO.5.2 | 개발 endpoint를 위험 기반으로 보호·강화 |

Protect Software (PS)

| ID | Practice 제목 / Task 요약 |
|---|---|
| **PS.1** | Protect All Forms of Code from Unauthorized Access and Tampering |
| PS.1.1 | 모든 형태의 코드(소스, 실행 코드, configuration-as-code)를 최소 권한 원칙으로 보관 |
| **PS.2** | Provide a Mechanism for Verifying Software Release Integrity |
| PS.2.1 | 릴리스 무결성 검증 정보를 소프트웨어 취득자에게 제공 |
| **PS.3** | Archive and Protect Each Software Release |
| PS.3.1 | 릴리스마다 필요한 파일과 지원 데이터(무결성 정보, provenance)를 안전하게 보관 |
| PS.3.2 | 릴리스의 모든 컴포넌트에 대한 provenance 데이터(예: SBOM)를 수집·보호·유지·공유 |

Produce Well-Secured Software (PW)

| ID | Practice 제목 / Task 요약 |
|---|---|
| **PW.1** | Design Software to Meet Security Requirements and Mitigate Security Risks |
| PW.1.1 | threat modeling, attack modeling, attack surface mapping 등으로 보안 위험 평가 |
| PW.1.2 | 보안 요구·위험·설계 결정을 추적하고 유지 |
| PW.1.3 | 적절한 경우 표준화된 보안 기능·서비스를 쓰도록 지원 (구 PW.4.3) |
| **PW.2** | Review the Software Design to Verify Compliance with Security Requirements and Risk Information |
| PW.2.1 | 설계에 관여하지 않은 자격 있는 사람 및/또는 toolchain의 자동화 절차가 설계를 검토 |
| **PW.3** | Verify Third-Party Software Complies with Security Requirements — *Moved to PW.4* |
| PW.3.1 / PW.3.2 | Moved to PO.1.3 / Moved to PW.4.4 |
| **PW.4** | Reuse Existing, Well-Secured Software When Feasible Instead of Duplicating Functionality |
| PW.4.1 | 제3자(상용·오픈소스)의 잘 보호된 컴포넌트를 취득하고 유지 |
| PW.4.2 | 제3자로 충족되지 않는 공통 수요는 SDLC 절차에 따라 자체 컴포넌트로 만들고 유지 |
| PW.4.3 | Moved to PW.1.3 |
| PW.4.4 | 취득한 모든 제3자 컴포넌트가 수명주기 내내 조직이 정한 요구를 따르는지 검증 |
| PW.4.5 | Moved to PW.4.1 and PW.4.4 |
| **PW.5** | Create Source Code by Adhering to Secure Coding Practices |
| PW.5.1 | 개발 언어·환경에 맞는 secure coding practice를 따름 |
| PW.5.2 | Moved to PW.5.1 as example |
| **PW.6** | Configure the Compilation, Interpreter, and Build Processes to Improve Executable Security |
| PW.6.1 | 실행 파일 보안을 높이는 기능을 제공하는 컴파일러·빌드 도구 사용 |
| PW.6.2 | 어떤 기능을 어떻게 설정할지 정하고 승인된 설정을 구현·사용 |
| **PW.7** | Review and/or Analyze Human-Readable Code to Identify Vulnerabilities and Verify Compliance with Security Requirements |
| PW.7.1 | 코드 리뷰(사람)와 코드 분석(도구) 중 무엇을 쓸지 결정 |
| PW.7.2 | 리뷰·분석을 수행하고 발견 사항을 이슈 추적 체계에 기록·분류 |
| **PW.8** | Test Executable Code to Identify Vulnerabilities and Verify Compliance with Security Requirements |
| PW.8.1 | 실행 코드 테스트를 할지, 어떤 유형으로 할지 결정 |
| PW.8.2 | 범위를 정하고 테스트를 설계·수행하고 결과와 발견 사항을 기록·분류 |
| **PW.9** | Configure Software to Have Secure Settings by Default |
| PW.9.1 | 보안에 영향을 주는 각 설정의 안전한 기준선(기본값)을 정의 |
| PW.9.2 | 기본 설정을 구현하고 각 설정을 관리자용으로 문서화 |

Respond to Vulnerabilities (RV)

| ID | Practice 제목 / Task 요약 |
|---|---|
| **RV.1** | Identify and Confirm Vulnerabilities on an Ongoing Basis |
| RV.1.1 | 취득자·사용자·공개 출처에서 소프트웨어와 제3자 컴포넌트의 잠재 취약점 정보를 수집하고 조사 |
| RV.1.2 | 코드를 검토·분석·테스트해 미발견 취약점을 식별·확인 |
| RV.1.3 | 취약점 공개·조치 정책을 두고 필요한 역할·절차 구현 |
| **RV.2** | Assess, Prioritize, and Remediate Vulnerabilities |
| RV.2.1 | 각 취약점을 분석해 조치 계획에 필요한 위험 정보 확보 |
| RV.2.2 | 취약점에 대한 위험 대응을 계획하고 실행 |
| **RV.3** | Analyze Vulnerabilities to Identify Their Root Causes |
| RV.3.1 | 식별된 취약점의 근본 원인 분석 |
| RV.3.2 | 근본 원인을 시간에 걸쳐 분석해 패턴 식별 |
| RV.3.3 | 유사 취약점을 찾아 같은 부류를 선제적으로 제거 |
| RV.3.4 | SDLC 절차를 검토하고 재발 방지를 위해 갱신 |

집계: practice ID 20개(그중 PW.3은 이동되어 실질 19개), task ID 47개(그중 5개는 "Moved", 실질 42개).

**v1.2 초안에서 달라지는 점 (1차, 초안).**

> "Two new practices have been added in this update: PO.6 to address the need for improvement over time and PS.4 to address the need for robust and reliable updates."

초안 PDF에서 확인한 추가 ID: PO.6.1–PO.6.3, PS.4.1–PS.4.4. PO.6의 제목은 "Define and Implement a Continuous Process Improvement Plan"으로 읽혔다. PS.4의 전체 제목과 task 본문은 정독하지 않았다. 초안이므로 기준에 넣지 않는다. (`https://nvlpubs.nist.gov/nistpubs/SpecialPublications/NIST.SP.800-218r1.ipd.pdf`, Note to Reviewers)

### 2.4 OWASP ASVS 5.0.0 — 보안 검증 요구사항

**판 확인 (1차).** README: "The latest stable version is version 5.0.0 (dated May 2025)". GitHub 릴리스 `v5.0.0_release`는 2025-05-30. 5.0.1은 "next release target"으로만 언급되고 릴리스 목록에 없다.

**인증에 대한 OWASP의 입장 (1차).**

> "OWASP, as a vendor-neutral nonprofit, does not certify any vendors, verifiers, or software."

출처: `https://raw.githubusercontent.com/OWASP/ASVS/v5.0.0/5.0/en/0x04-Assessment_and_Certification.md`

**레벨 정의 (1차, 0x03-What-is-the-ASVS.md).** 요구사항 수는 CSV를 직접 센 값이다: 전체 345개, L1 70개, L2 183개, L3 92개.

| 레벨 | 원문 설명 |
|---|---|
| L1 | "This level contains the minimum requirements to consider when securing an application and represents a critical starting point." 전체의 약 20% |
| L2 | "Most applications should be striving to achieve this level of security." 약 50%가 L2. L2를 따르려면 L1+L2 (약 70%) |
| L3 | "Requirements in this section are generally either defense-in-depth mechanisms or other useful but hard-to-implement controls." 나머지 약 30% |

레벨 선택에 대해: 표준이 정해주지 않고 조직이 위험을 분석해 정하라고 한다. 범위 축소도 명시적으로 허용한다.

> "for a machine-to-machine API, the requirements in chapter V3 related to web frontends will not be relevant."

> "If there is no use of OAuth or WebRTC, then those chapters can be ignored as well."

검증 보고에서 비적용 항목은 적어 두라고 한다: "Some requirements may be non-applicable (e.g., session management in stateless APIs), and this must be noted in the report." (0x04)

**요구 ID 표기 (1차).** `v<version>-<chapter>.<section>.<requirement>`, 예 `v5.0.0-1.2.5`. 이 문서도 이 형식을 쓴다.

**장 구조 (1차, CSV 집계).**

| 장 | 제목 | L1 | L2 | L3 | 계 |
|---|---|---:|---:|---:|---:|
| V1 | Encoding and Sanitization | 8 | 19 | 3 | 30 |
| V2 | Validation and Business Logic | 4 | 7 | 2 | 13 |
| V3 | Web Frontend Security | 8 | 11 | 12 | 31 |
| V4 | API and Web Service | 2 | 8 | 6 | 16 |
| V5 | File Handling | 4 | 5 | 4 | 13 |
| V6 | Authentication | 13 | 22 | 12 | 47 |
| V7 | Session Management | 6 | 12 | 1 | 19 |
| V8 | Authorization | 4 | 3 | 6 | 13 |
| V9 | Self-contained Tokens | 4 | 3 | 0 | 7 |
| V10 | OAuth and OIDC | 5 | 24 | 7 | 36 |
| V11 | Cryptography | 3 | 11 | 10 | 24 |
| V12 | Secure Communication | 3 | 6 | 3 | 12 |
| V13 | Configuration | 1 | 12 | 8 | 21 |
| V14 | Data Protection | 2 | 7 | 4 | 13 |
| V15 | Secure Coding and Architecture | 3 | 10 | 8 | 21 |
| V16 | Security Logging and Error Handling | 0 | 16 | 1 | 17 |
| V17 | WebRTC | 0 | 7 | 5 | 12 |

**이 OMS의 HTTP 표면에 대한 장별 적용 가능성 (제안).** 전제: (a) loopback/tailnet에 묶인 읽기 전용 관측 대시보드, (b) 별도의 인증된 운영자 제어 API (halt/resume/cancel-all, 비운영 환경의 fault injection). 아래 "읽은 요구"는 CSV에서 실제로 읽은 것만 적었다. 판정은 조사자의 추론이다.

| 장 | (a) 대시보드 | (b) 제어 API | 이유와 읽은 요구 |
|---|---|---|---|
| V1 | 조건부 | 조건부 | 대시보드가 venue·전략 유래 문자열을 HTML로 그리면 출력 인코딩이 해당. SQL·OS 명령·역직렬화를 쓰지 않는 구간은 해당 절이 빠진다. 개별 요구는 정독하지 않음 |
| V2 | 낮음 | 적용 | 제어 명령 입력 검증: `v5.0.0-2.2.1`, `v5.0.0-2.2.2`. 순서·한도: `v5.0.0-2.3.1`, `v5.0.0-2.3.2`. 과도 호출 방어: `v5.0.0-2.4.1`. `v5.0.0-2.3.5`(L3, multi-user approval)는 단일 운영자라 비적용 후보 |
| V3 | 적용 (브라우저로 연다면) | 브라우저 클라이언트가 없으면 비적용 후보 | ASVS 스스로 M2M API에는 V3가 무관하다고 한다. 대시보드는 CORS `v5.0.0-3.4.2`, CSP `v5.0.0-3.4.3`, 교차 출처 요청 검증 `v5.0.0-3.5.1`–`3.5.3`이 후보. 추론: loopback 바인딩이어도 운영자 브라우저가 다른 사이트의 요청을 중계할 수 있으므로, 제어 API를 브라우저에서 부를 수 있게 만들면 3.5절이 핵심이 된다 |
| V4 | 적용 | 적용 | `v5.0.0-4.1.1`(Content-Type), `v5.0.0-4.1.4`(L3, 허용 메서드만), 4.2절(메시지 경계). 4.3 GraphQL은 미사용이면 비적용. 4.4 WebSocket은 대시보드가 WS를 쓰면 조건부 (`v5.0.0-4.4.2` Origin 검사) |
| V5 | 비적용 후보 | 비적용 후보 | 파일 업로드·다운로드가 없다는 전제. 저널 export를 HTTP로 내주면 5.4가 조건부 |
| V6 | 조건부 | 적용 | 제어 API 인증. `v5.0.0-6.3.1`, `v5.0.0-6.3.2`. 비밀번호를 쓰지 않으면 6.2는 빠진다. `v5.0.0-6.3.3`(L2, MFA 또는 단일 요소 조합)은 단일 운영자·로컬 환경에서 어떻게 할지 리드 결정. 6.8 IdP는 비적용 후보. 대시보드를 무인증으로 두고 네트워크 바인딩에만 의존할지는 `v5.0.0-8.4.2`와 함께 결정 |
| V7 | 조건부 | 조건부 | 세션이 없는 API라면 ASVS가 비적용 예로 든다. 다만 정적 토큰을 쓰면 `v5.0.0-7.2.2` 문언과 어긋난다 (아래 인용). 편차로 기록할지 동적 토큰으로 갈지 결정 필요 |
| V8 | 조건부 | 적용 | `v5.0.0-8.1.1`(권한 규칙 문서화), `v5.0.0-8.2.1`(기능 단위 접근 제한), `v5.0.0-8.3.1`(신뢰 계층에서 집행). `v5.0.0-8.4.1` multi-tenant는 비적용. `v5.0.0-8.4.2`(L3)는 관리 인터페이스에 직접 해당 |
| V9 | 비적용 후보 | 조건부 | JWT 등 self-contained token을 쓸 때만. `v5.0.0-9.1.1`–`9.1.3`, `v5.0.0-9.2.1` |
| V10 | 비적용 후보 | 비적용 후보 | OAuth/OIDC 미사용. ASVS가 미사용 시 무시 가능하다고 명시 |
| V11 | 조건부 | 조건부 | 자체 암호 구현은 없다는 전제. 토큰 난수는 `v5.0.0-11.5.1`. venue 서명 키 취급은 V13.3과 함께 |
| V12 | 조건부 | 조건부 | 아웃바운드(venue)는 `v5.0.0-12.3.1`, `v5.0.0-12.3.2` 적용. 인바운드 loopback 평문은 `12.3.1` 문언과의 편차로 기록할 사항. tailnet 구간이 `v5.0.0-12.3.3`의 "another appropriate transport encryption mechanism"에 해당하는지는 리드 판단. `v5.0.0-12.2.2`(공인 인증서)는 외부 공개 서비스가 아니라 비적용 후보 |
| V13 | 적용 | 적용 | 비밀 관리 `v5.0.0-13.3.1`, `13.3.2`. 디버그 `v5.0.0-13.4.2`. 모니터링 endpoint 노출 `v5.0.0-13.4.5`. 아웃바운드 allowlist `v5.0.0-13.2.4`. 통신 요구 문서화 `v5.0.0-13.1.1` |
| V14 | 조건부 | 적용 | `v5.0.0-14.2.1`(URL·쿼리에 토큰 금지). 개인정보는 없지만 계좌 잔고·포지션·API 키를 "sensitive"로 분류할지 결정 필요. 14.3은 대시보드 한정 |
| V15 | 적용 | 적용 | 의존성 `v5.0.0-15.1.1`, `15.1.2`, `15.2.1`. 운영 빌드에서 개발 기능 제외 `v5.0.0-15.2.3` (fault injection과 직접 관련). 동시성 `v5.0.0-15.4.1`–`15.4.4`(L3). `v5.0.0-15.3.6`은 대시보드 JS에만 |
| V16 | 조건부 | 적용 | 인증 로그 `v5.0.0-16.3.1`, 인가 실패 로그 `v5.0.0-16.3.2`, 로그 변조 방지 `v5.0.0-16.4.2`, fail-open 금지 `v5.0.0-16.5.3`. 별도 시스템 전송 `v5.0.0-16.4.3`은 단일 호스트라 조건부 |
| V17 | 비적용 | 비적용 | WebRTC 미사용 |

설계 결정에 직접 닿는 요구 원문 (1차, CSV):

- `v5.0.0-13.4.5` (L2): > "Verify that documentation (such as for internal APIs) and monitoring endpoints are not exposed unless explicitly intended."
- `v5.0.0-15.2.3` (L2): > "Verify that the production environment only includes functionality that is required for the application to function, and does not expose extraneous functionality such as test code…"
- `v5.0.0-7.2.2` (L1): > "Verify that the application uses either self-contained or reference tokens that are dynamically generated for session management, i.e. not using static API secrets and keys."
- `v5.0.0-8.4.2` (L3): > "…ensuring that network location or trusted endpoints are not the sole factors for authorization even though they may reduce the likelihood of unauthorized access."
- `v5.0.0-3.5.3` (L1): > "Verify that HTTP requests to sensitive functionality use appropriate HTTP methods such as POST, PUT, PATCH, or DELETE…"
- `v5.0.0-14.2.1` (L1): > "…the URL and query string do not contain sensitive information, such as an API key or session token."
- `v5.0.0-16.5.3` (L2): > "Verify that the application fails gracefully and securely, including when an exception occurs, preventing fail-open conditions such as processing a transaction despite errors resulting from validation logic."
- `v5.0.0-16.4.2` (L2): > "Verify that logs are protected from unauthorized access and cannot be modified."
- `v5.0.0-2.3.5` (L3): > "Verify that high-value business logic flows require multi-user approval to prevent unauthorized or accidental actions."

출처: `https://github.com/OWASP/ASVS/raw/v5.0.0/5.0/docs_en/OWASP_Application_Security_Verification_Standard_5.0.0_en.csv`

ASVS가 스스로 범위 밖이라고 하는 것도 기록한다: CI/CD 수명주기 활동, 백업 같은 외부 프로세스는 ASVS 범위가 아니다 (0x03 "Scope of the ASVS"). 그 부분은 SSDF 쪽에서 다룬다.

### 2.5 FIX — 참조 의미론 (대상 venue는 FIX가 아님)

#### (a) Order State Changes (FIX Latest, EP284, November 2023)

**성격.** ExecutionReport(35=8)의 ExecType(150)과 OrdStatus(39) 의미론을 시나리오 표로 정의한다. 3장은 일반 시나리오, 4장은 거래소용 축약 워크플로다. 사이트는 이 패키지를 "Normative specification of the order state changes of FIX Latest"라고 소개한다. (`https://fixtrading.org/packages/fix-latest-specification-order-state-changes/`)

**시나리오 그룹 (1차, 1장과 3.1).**

| 그룹 | 1장 표현 | 3.1 표제 | 시나리오 Ref |
|---|---|---|---|
| A | Order executions (aka Vanilla) | Vanilla | A.1.a–b |
| B | Order cancellations | Cancel | B.1.a–f |
| C | Order modifications | Cancel/Replace quantity changes | C.1.a–c, C.2.a, C.3.a–c |
| D | Order sequencing and chaining | Cancel/Replace sequencing and chaining | D.1.a–c, D.2.a–d |
| E | Order restatements | Unsolicited/Reinstatement | E.1.a–f |
| F | Order rejections | Order Reject | F.1.a–c |
| G | Order status information | Status | G.1.a–c |
| H | Multi-day orders (GTC) | GT | H.1.a–d |
| I | Non-resting orders (IOC, FOK) | TimeInForce | I.1.a–b |
| J | Execution corrections and cancellations | Execution Cancels/Corrects | J.1.a–d |
| K | Trading halts | Trading Halt | K.1.a–b |
| L | Miscellaneous | Miscellaneous | L.1.a–b |

4장(거래소용)은 A.1.a–d와 I.1.a–b만 둔다.

**OrdStatus 우선순위 (2장).**

> "If an order simultaneously exists in more than one order state, the value with highest precedence is the value that is reported in OrdStatus(39)."

2장 표의 순서(높은 것부터): Pending Cancel 11 > Pending Replace 10 > Done for Day 9 > Calculated 8 > Filled 7 > Stopped 6 > Suspended 5 > Canceled 4 = Expired 4 > Partially Filled 3 > New 2 = Rejected 2 = Pending New 2 > Accepted for bidding 1.

Pending Cancel 설명에는 대문자로 "DOES NOT INDICATE THAT THE ORDER HAS BEEN CANCELED."가 붙어 있다. Rejected 설명에는 접수 확인 뒤에도 거절될 수 있다는 NOTE가 있다 ("an order can pass from New to Rejected status").

문서 내부 불일치 한 가지: 3.2의 전이 표 머리글은 "Partially Filled (4)", "Canceled (5)"로 적혀 2장 표의 숫자(3, 4)와 다르다(추출 텍스트 기준). 순서 관계는 같다. 숫자 자체를 코드 상수로 가져오지 말고 순서 관계만 참고한다.

3.2는 표에 없는 전이를 금지로 읽지 말라고 한다:

> "Note that absence of a scenario should not necessarily be interpreted as meaning that the state transition is not allowed"

**pending cancel 중 체결 (B.1.b, B.1.c).** 취소 요청과 체결 보고가 연결 위에서 엇갈린다. Pending Cancel 상태에서도 Trade 보고가 계속 온다.

> "Execution for 1000 whilst order is pending cancel – ‘pending cancel’ order status takes precedence over ‘partially filled’ order status."

B.1.c에서는 Pending Cancel 중 전량 체결되고, 취소 요청은 Cancel Reject로 끝난다: "Cancel request rejected – CxlRejectReason = 0 (too late to cancel)". 그 Cancel Reject의 OrdStatus는 Filled다.

**접수 확인 전 취소 (B.1.d, B.1.e).** 미확인 주문에 대한 취소 요청도 OrigClOrdID로 그 주문을 가리킨다 ("OrigClOrd set to X even though X has not yet been ‘accepted’"). 접수 확인(New)과 취소 요청이 엇갈리는 경우가 B.1.e다.

**모르는 주문에 대한 취소 (B.1.f).**

> "Cancel request rejected with reject reason of “Unknown Order”, OrdStatus is “Rejected” and OrderID is “NONE”"

**정정(cancel/replace) 경합 (C.3, D.2).** C.3.c: 요청 수량(7000)이 이미 체결된 누적(8000)보다 작으면 OrderQty가 CumQty로 맞춰지고 상태는 Filled가 된다.

> "The replace request is interpreted as requiring the balance of the order to be canceled – the ‘filled’ order status takes precedence over ‘canceled’."

D.2는 연속 정정 요청의 처리 방식이 상대방에 따라 다르다는 것을 보여 준다(순차 처리, pending 먼저 처리, 둘 다 거절, 둘째 거절). D.2.d:

> "This matrix illustrates the case where the broker/order receiver does not support multiple outstanding order cancel or order cancel/replace requests"

**요청하지 않은 취소 (E.1.b, E.1.d, E.1.e).** E.1.b는 부분 체결 주문이 요청 메시지 없이 Canceled로 보고되는 경우다. ExecType=Canceled, OrdStatus=Canceled, CumQty 유지, LeavesQty 0. 사유는 ExecRestatementReason으로 전달된다. E.1.d는 sell-side의 수량 축소, E.1.e는 조건부 주문의 자동 취소다.

**체결 취소·정정 (J).** J.1.a: Filled 주문에 Trade Cancel이 오면 CumQty가 줄고 OrdStatus가 Partially Filled로 **되돌아간다**. 이후 Trade Correct로 가격·수량이 바뀐다. 각 보고는 ExecRefID로 원 체결을 가리킨다. J.1.b: 이미 Canceled인 주문에 체결 취소와 새 체결이 온다.

> "Cancel of the execution. ‘Canceled’ order status takes precedence over ‘New’"

J.1.b의 다음 행은 Canceled 상태에서 "Fill for 4000"이 보고된다. 즉 종료 상태 뒤에도 누적 체결이 변한다.

**주문 상태 조회 (G).** G.1.a: 모르는 주문에 대한 조회는 ExecType=Order Status, OrdStatus=Rejected, "OrdRejReason = unknown order"로 응답한다. G.1.c: 접수 직후 조회하면 Pending New가 온다. 2장도 Pending New는 OrderStatusRequest(35=H)에 대한 응답으로만 보낸다고 적는다. 조회 응답은 "LastQty not required when ExecType=Order Status"다.

**ClOrdID 중복과 PossResend (F.1.a, F.1.b).**

- F.1.a: 같은 ClOrdID로 다른 주문을 보내면 ExecType=Rejected로 거절하되 OrdStatus는 **첫 주문의 현재 상태**를 싣는다. "OrdRejReason = duplicate order."
- F.1.b: PossResend=Y이고 이미 받은 ClOrdID면 거절하지 않고 현재 상태를 돌려준다.
  > "Because order X has already been received, confirm back the current state of the order."
- 같은 ClOrdID가 PossResend=N(또는 미설정)으로 오면 duplicate order로 거절한다.
- PossResend=Y인데 처음 보는 ClOrdID면 새 주문으로 받는다.
  > "Because order Y has not been received before, confirm back as a new order."

출처: FIX Latest as of EP284 – Order State Changes PDF (위 패키지 페이지의 다운로드), 해당 시나리오 표.

#### (b) ClOrdID 유일성 범위, PossDupFlag / PossResend

**ClOrdID(11) (FIX Latest 필드 정의).**

> "Uniqueness must be guaranteed within a single trading day."

같은 정의는 ClOrdID가 buy-side가 부여하는 식별자이고 SenderCompID(49) 또는 OnBehalfOfCompID(115)로 식별되는 주체 범위라고 적는다. 다일 주문·글로벌 거래·장 마감을 넘는 경우에는 날짜를 넣는 식으로 날을 넘는 유일성을 확보하라고 권한다. (`https://orchimate.org/fixtrading/fix-latest/fields/ClOrdID`)

OrigClOrdID(41): "ClOrdID (11) of the previous order (NOT the initial order of the day)…". 취소·정정 요청에서 직전 주문을 가리킨다.

**PossDupFlag(43) — 세션 계층 재전송.** 필드 정의: "Indicates possible retransmission of message with this sequence number".

> "A retransmitted message uses the original MsgSeqNum(34) value with PossDupFlag(43)=Y." (Session Layer 3.1.13)

> "It is the responsibility of the receiving firm to properly respond to the message with PossDupFlag(43) set to “Y” by determining if the message was previously processed." (4.8.4)

> "A message received with the PossDupFlag(43) set to “Y” should be ignored when a message with the same MsgSeqNum(34) was previously processed." (4.8.7)

**PossResend(97) — 응용 계층 재송신.** 필드 정의: "Indicates that message may contain information that has been sent under another sequence number."

> "The application layer is responsible for the detection and proper handling of duplicate application messages." (4.9)

4.9는 재송신 메시지가 새 MsgSeqNum을 받고 세션 계층은 이를 새 메시지로 취급한다고 적는다. 그리고 대안을 허용한다:

> "The use of a globally unique persistent identifier that enforces idempotent behavior at the application layer may be used as an alternative to sending a PossResend(97) field by counterparty agreement."

출처: FIX Session Layer Technical Standard, Errata November 2020 (`https://fixtrading.org/packages/fix-session-layer-technical-standard/`), `https://orchimate.org/fixtrading/fix-latest/fields/PossDupFlag`, `…/PossResend`

요지: 전송 계층의 "같은 번호 재전송"과 응용 계층의 "새 번호로 같은 내용 재송신"은 다른 문제이고, 중복 판정 책임은 응용 계층에 있다.

### 2.6 FIA — 업계 권고

분석은 `/home/junyoung/github/TradingSystem/docs/architecture/FIA-OMS-model.md`에 있다. 다시 하지 않았다. 그 문서의 두 URL은 오늘도 HTTP 200으로 PDF를 돌려준다.

| 문서 | 표지 제목 | 일자 | 쪽수 | 확인 |
|---|---|---|---:|---|
| F24 | Best Practices For Automated Trading Risk Controls And System Safeguards | July 2024 (PDF 생성 2024-07-17) | 22 | 기존 문서의 "22쪽"과 일치. 목차 §1.1–§5.1도 기존 문서의 절 번호와 일치 |
| F15 | Guide to the Development and Operation of Automated Trading Systems | March 2015 (PDF 생성 2015-03-24) | 48 | 기존 문서의 "48쪽", "URL의 2019는 출판연도가 아니다"와 일치 |

기존 문서는 Go OMS를 대상으로 적혀 있다 ("단일 거래소·단일 계좌·다중 전략 Go OMS"). 이번 Rust OMS에 R01–R15를 그대로 가져올지는 리드 결정이다.

### 2.7 규제 문서 — 역할과 관련성 메모

법률 결론이 아니다. 문언상 누구를 대상으로 쓰였는지만 적는다.

**ESMA MiFID II RTS 6 — Commission Delegated Regulation (EU) 2017/589.** 역할: EU 규제(투자회사의 조직 요건). 제목이 수범자를 밝힌다: "…regulatory technical standards specifying the organisational requirements of investment firms engaged in algorithmic trading". 2018-01-03부터 적용. 조문 표제 중 OMS 통제와 주제가 겹치는 것은 Art. 5–8(시험·배포), Art. 12 Kill functionality, Art. 14 Business continuity arrangements, Art. 15 Pre-trade controls on order entry, Art. 16 Real-time monitoring, Art. 17 Post-trade controls, Art. 18 Security and limits to access다. Art. 12(1): "An investment firm shall be able to cancel immediately, as an emergency measure, any or all of its unexecuted orders submitted to any or all trading venues…". 한국에서 자기 자금으로 암호화폐 venue에서 거래하는 개인에게 이 규정이 미치는지는 **판단 불가, 법률 검토 필요**. 조사자 의견: 문언의 수범자는 investment firm이므로 설계 점검표의 참고 항목(비구속)으로만 쓰는 편이 안전하다. Art. 15–18 본문은 읽지 않았다. 출처: `https://publications.europa.eu/resource/celex/32017R0589`

**SEC Rule 15c3-5 (17 CFR 240.15c3-5).** 역할: 미국 규제(시장 접근을 가진 broker-dealer의 위험 통제). (b): "A broker or dealer with market access… shall establish, document, and maintain a system of risk management controls and supervisory procedures". (a)(1)은 market access를 "Access to trading in securities on an exchange or alternative trading system"으로 정의한다. (c)(1)(ii)는 가격·수량 한도 초과 주문과 중복 주문("or that indicate duplicative orders")의 진입 방지를 요구한다. 개인 운영자에 대한 적용 여부는 **판단 불가, 법률 검토 필요**. 조사자 의견: 문언의 수범자는 broker or dealer이고 대상은 securities다. 사전 통제 항목(신용·자본 한도, 오주문, 중복 주문)의 분류만 참고한다. 출처: `https://www.ecfr.gov/current/title-17/chapter-II/part-240/subject-group-ECFRc8401dcba174f73/section-240.15c3-5` (본문은 접근 기록 표 7b의 eCFR 렌더러 API로 조회)

**가상자산 이용자 보호 등에 관한 법률.** 역할: 국내 법률. 열람본은 [시행 2026. 10. 2.] [법률 제21857호, 2026. 8. 4., 타법개정]. 제1조는 목적을 "가상자산 이용자 자산의 보호와 불공정거래행위 규제"로 든다. 제2조는 "가상자산사업자"를 매매·교환·이전·보관·중개 등을 "영업으로 하는 자"로, "이용자"를 가상자산사업자를 통해 매매 등을 하는 자로 정의한다. 제6~9조, 제11~13조는 표제상 사업자 의무다(예치금 보호, 보관, 보험, 거래기록, 이상거래 감시). 반면 제10조 ②~④는 주어가 "누구든지"다. 통정매매·가장매매, 시세 변동·고정 매매, 부정한 수단 사용 등을 금지한다. 제3조는 "국외에서 이루어진 행위로서 그 효과가 국내에 미치는 경우에도 적용"한다고 한다. 해외 venue의 무기한 선물이 이 법의 "가상자산의 매매"에 드는지, 같은 계좌의 전략 간 자기 체결이 제10조와 어떤 관계인지는 **판단 불가, 법률 검토 필요**. 조사자 의견: 사업자 의무 조항은 OMS 검토 기준에서 뺄 후보이고, 제10조는 self-match 방지(FIA 문서의 R10)와 주제가 겹치므로 법률 검토 대상으로 표시해 둔다. 시행령, 감독규정, 다른 법률(특정금융정보법, 자본시장법, 외국환거래법 등)은 조사하지 않았다. 출처: `https://www.law.go.kr/법령/가상자산이용자보호등에관한법률`

---

## 3. OMS 검토 항목으로의 매핑 제안

리드 엔지니어가 결정할 **제안**이다. 준수 선언이 아니다. 판정: 적용 / 조건부 / 비적용 후보. R번호는 `docs/architecture/FIA-OMS-model.md`의 요구 ID다.

### 3.1 ISO/IEC 25010:2023 (특성 이름: 2차 자료 기준, †는 1차 확인)

| 요소 | OMS 검토 관심사 후보 | 판정 | 이유 |
|---|---|---|---|
| Functional suitability → correctness, completeness | 주문 상태 기계, 수용 검사, 예약·해제 산술의 정확성 (R01–R05) | 적용 | 핵심 기능의 정답 여부 |
| Functional suitability → appropriateness | 운영자 명령 집합(halt/resume/cancel-all)이 실제 비상 절차를 덮는가 | 조건부 | 비상 절차(D7)가 정해져야 평가 가능 |
| Performance efficiency → time behaviour, capacity | 결정 지연, 체결 기장 지연, 큐 깊이 상한 (R14) | 적용 | 단일 writer의 처리 한계가 곧 안전 한계 |
| Performance efficiency → resource utilization | 메모리·디스크(저널 증가) 상한 | 적용 | 저널이 로컬 디스크를 채우면 정지 |
| Compatibility → interoperability | venue 어댑터 계약 (주문·취소·조회·오류 의미) | 적용 | venue 의미 불일치가 대사 오류의 원천 (D6) |
| Compatibility → co-existence | 같은 호스트의 전략 프로세스·대시보드와 자원 공유 | 조건부 | 배치 구성이 정해져야 함 |
| Interaction capability† → operability, user error protection, self-descriptiveness† | 제어 API의 오조작 방지, 대시보드가 hold 사유를 스스로 설명하는가 | 조건부 | 사용자는 운영자 1명이지만 비상 시 오조작 비용이 크다 |
| Interaction capability† → learnability, user engagement†, inclusivity†, user assistance† | — | 비적용 후보 | 단일 운영자 내부 도구 |
| Reliability → recoverability | 저널 replay, snapshot, 재시작 후 대사 (R06, D3) | 적용 | 로컬 저널이 유일한 영속 근거 |
| Reliability → fault tolerance | venue 단절, 부분 장애, 결과 불명(unknown) 처리 | 적용 | R06, R07 |
| Reliability → faultlessness† | 정상 운영 중 결함 없는 동작. 모델 검사·property test의 대상 | 적용 | — |
| Reliability → availability | 가동 목표 | 조건부 | 단일 프로세스·단일 호스트. 고가용 구성은 범위 밖일 수 있다. 대신 "죽었을 때 안전한가"가 Safety 쪽 질문 |
| Security → integrity | 저널 변조·손상 탐지, 설정 버전 고정 (R13) | 적용 | — |
| Security → confidentiality | venue API 키, 제어 API 토큰 | 적용 | — |
| Security → authenticity | 제어 API 호출자 인증 | 적용 | ASVS V6과 연결 |
| Security → accountability | 운영자 명령과 설정 변경의 감사 이력 | 적용 | 1인이어도 사후 분석에 필요 |
| Security → non-repudiation | — | 조건부 | 운영자와 자금주가 같은 사람이면 실익이 작다 |
| Security → resistance† | 제어 API에 대한 과도 호출·공격 중 동작 유지 | 조건부 | 노출면이 loopback/tailnet으로 좁다 |
| Maintainability → modularity, analysability, modifiability, testability | 코어·어댑터·저널 경계, 결정적 재현, 테스트 가능성 | 적용 | LMAX식 코어의 결정성이 검토 대상 |
| Maintainability → reusability | — | 조건부 | 둘째 venue를 붙일 때만 의미 |
| Flexibility† → adaptability, replaceability | venue 어댑터 교체 | 조건부 | "venue first"이므로 지금은 과설계 위험 |
| Flexibility† → installability | 배포·롤백 절차 | 적용 | SSDF PS.3과 연결 |
| Flexibility† → scalability† | — | 비적용 후보 | 단일 계좌·단일 운영자 |
| Safety† → fail safe†, operational constraint† | halt/hold, 사전 한도, 불확실 시 신규 노출 차단 (R05, R07) | 조건부 | 금전 손실이 25010의 safety 정의에 드는지 원문 미확인. 분류 이름으로 빌려 쓸지 리드 결정 |
| Safety† → hazard warning†, risk identification† | 경보, 한도 근접 경고 | 조건부 | 위와 같음 |
| Safety† → safe integration† | 전략·어댑터를 붙일 때 통제가 유지되는가 | 조건부 | 위와 같음 |

### 3.2 ISO/IEC/IEEE 42010:2022 (조항 제목만 1차 확인)

| 요소 | OMS 검토 관심사 후보 | 판정 | 이유 |
|---|---|---|---|
| Stakeholders (6.2 제목) | 운영자 겸 개발자 겸 자금주, 미래의 자기 자신(사고 조사자), venue | 적용 | 누가 문서를 읽는지 정해야 검토 기준이 선다 |
| Concerns (6.4 제목) | 자금 안전, 복구 가능성, 귀속 정확성, 운영 가능성, 변경 안전 | 적용 | 검토 기준의 최상위 목차가 된다 |
| Stakeholder perspectives, Aspects (6.3, 6.5 제목) | — | 조건부 | 2022판 신규 개념. 본문 미확인이라 도입 이득 불명 |
| Viewpoints / Views (6.6, 6.7 제목) | 상태 기계 뷰, 저널·복구 뷰, 배포·네트워크 뷰, 운영(runbook) 뷰 | 적용 | 뷰마다 어떤 concern을 다루는지 명시 |
| View components, Model kinds (6.8, 8.2 제목) | TLA+ 모델, 전이 표, 시퀀스 다이어그램의 표기 규약 | 조건부 | 규약을 문서화하면 좋지만 형식 요구는 원문 미확인 |
| Correspondences (6.9 제목) | 요구(R) ↔ 모델 불변식 ↔ 테스트 ↔ 코드 모듈의 추적 표 | 적용 | 검토에서 "어느 요구가 어디서 검증되는가"를 물을 근거 |
| Decisions and rationale (6.10 제목) | ADR: 단일 writer, 로컬 저널, 대시보드 바인딩, 토큰 방식. 기각한 대안 포함 | 적용 | D1–D7과 자연스럽게 연결 |
| Conformance (4), ADF, ADL (7) | — | 비적용 후보 | 적합성 주장을 하지 않는다. 자체 프레임워크·언어를 정의하지 않는다 |

### 3.3 NIST SP 800-218 SSDF v1.1

| 요소 | OMS 검토 관심사 후보 | 판정 | 이유 |
|---|---|---|---|
| PO.1 (PO.1.1, PO.1.2) | OMS와 개발 환경의 보안 요구 문서화 | 적용 | 1인이어도 요구가 적혀 있어야 검토 가능 |
| PO.1.3 | 상용 컴포넌트 공급자에게 요구 전달 | 비적용 후보 | 상용 공급 계약이 없다 |
| PO.2 (PO.2.1–PO.2.3) | 역할, 교육, 경영진 의지 | 비적용 후보 | 1인 조직. 다만 "운영자=개발자=승인자"라는 사실은 위험으로 기록 |
| PO.3 | 빌드·정적 분석·의존성 점검 toolchain과 그 산출물 | 적용 | PO.3.3의 증거 생성이 릴리스 게이트 근거 |
| PO.4 | 릴리스 전 보안 점검 기준 | 적용 | "무엇을 통과해야 실거래 배포인가" |
| PO.5 | 개발·시험·운영 환경 분리, 운영 키 격리 | 적용 | testnet 키와 mainnet 키의 분리, fault injection 환경 구분 |
| PS.1 | 소스·설정의 접근 통제 | 적용 | 저장소 권한, 서명된 커밋 여부는 리드 결정 |
| PS.2 | 릴리스 무결성 검증 정보 | 조건부 | 외부 취득자는 없다. "운영에 올라간 바이너리 = 검토한 커밋"을 확인하는 내부 용도로 축소 |
| PS.3 (PS.3.1, PS.3.2) | 릴리스 보관, provenance, SBOM | 적용 / 조건부 | 롤백과 사고 재현에 PS.3.1 필요. PS.3.2의 공유 부분은 대상이 없다 |
| PW.1 (PW.1.1, PW.1.2) | threat model, 보안 요구·위험·설계 결정 추적 | 적용 | 제어 API와 venue 키가 공격면 |
| PW.1.3 | 표준 보안 기능 사용 | 조건부 | 로그·인증을 자체 구현할지 여부 |
| PW.2 | 설계 검토 (설계자가 아닌 사람 또는 자동화) | 조건부 | 1인이라 독립 검토자가 없다. 자동화나 외부 리뷰로 대체할지 결정 |
| PW.4 (PW.4.1, PW.4.4) | 의존성 선택·유지·수명주기 검증 | 적용 | crate 재사용이 많다 |
| PW.4.2 | 자체 공통 컴포넌트 | 조건부 | 모노레포 내 공유 crate가 생길 때 |
| PW.5 | secure coding practice | 적용 | 언어 특화 규칙(예: unsafe 정책)은 리드가 정의 |
| PW.6 | 컴파일러·빌드 설정 | 적용 | 릴리스 프로파일, 경고 정책 |
| PW.7 | 코드 리뷰·분석 | 적용 | 도구 분석은 가능, 사람 리뷰는 PW.2와 같은 제약 |
| PW.8 | 실행 코드 테스트 | 적용 | property test, fuzz, 장애 주입 시험 |
| PW.9 | 안전한 기본 설정 | 적용 | 기본 바인딩 주소, fault injection 기본 비활성, 한도 미설정 시 거절 (R01) |
| RV.1.1, RV.1.2 | 의존성·자체 코드의 취약점 정보 수집과 확인 | 적용 | — |
| RV.1.3 | 취약점 공개 정책 | 비적용 후보 | 비공개 1인 소프트웨어. 공개 저장소가 되면 재검토 |
| RV.2 | 취약점 평가·우선순위·조치 | 적용 | 조치 시한은 ASVS `v5.0.0-15.1.1`과 함께 정의 |
| RV.3 | 근본 원인 분석, 유사 결함 제거, 절차 갱신 | 적용 | 거래 사고의 사후 분석에도 같은 틀을 쓸 수 있다 (추론) |
| v1.2 초안의 PO.6, PS.4 | 지속 개선 계획, 견고한 업데이트 | 비적용 후보 | 초안. 최종본 확인 후 재검토 |

### 3.4 OWASP ASVS 5.0.0

장별 판정과 요구 ID는 2.4의 표를 따른다. 여기에는 결정이 필요한 묶음만 적는다.

| 요소 | OMS 검토 관심사 후보 | 판정 | 이유 |
|---|---|---|---|
| 목표 레벨 | L1을 기준선으로 하고 제어 API에 한해 L2 항목을 고를지 | 조건부 | ASVS는 레벨을 조직이 정하라고 한다. 실거래 자금이 걸린 관리 인터페이스라는 점이 변수 |
| V3 + V4 (대시보드) | 브라우저 경유 교차 출처 요청, CSP, 허용 메서드 | 적용 | 읽기 전용이어도 잔고·포지션이 노출된다 |
| V6 + V7 + V8 + V9 (제어 API) | 인증 방식, 토큰 수명, 기능 단위 인가 | 적용 | `v5.0.0-7.2.2`(정적 키 금지)와 `v5.0.0-8.4.2`(네트워크 위치만으로 인가 금지)가 현재 설계 전제와 부딪칠 수 있다 |
| V12 (전송) | venue 방향 TLS 검증, loopback/tailnet 구간의 암호화 편차 기록 | 조건부 | `v5.0.0-12.3.1` 문언은 모든 인·아웃바운드 연결을 대상으로 한다 |
| V13 (설정·비밀) | venue API 키 보관, 디버그·모니터링 endpoint 노출 | 적용 | `v5.0.0-13.3.1`, `13.4.2`, `13.4.5` |
| V15.2.3 | fault injection이 운영 빌드에 들어가지 않거나 접근 불가인가 | 적용 | 요구 문언이 "test code… development functionality"를 직접 언급 |
| V15.4 (L3) | 단일 writer 밖의 공유 상태(대시보드 읽기 경로, 저널 writer) | 조건부 | L3이지만 설계 주제와 정확히 겹친다 |
| V16 | 제어 명령·인증·인가 실패의 로그, 로그 변조 방지, fail-open 금지 | 적용 | `v5.0.0-16.5.3`은 "오류가 났는데 거래를 처리"하는 것을 직접 금지 |
| V5, V10, V17 | — | 비적용 후보 | 기능 미사용 (ASVS가 생략을 허용) |
| V2.3.5 | 고가치 흐름의 다자 승인 | 비적용 후보 | 단일 운영자. 대신 확인 절차(2단계 명령 등)를 둘지는 별도 결정 |

### 3.5 FIX (의미론 참조)

| 요소 | OMS 검토 관심사 후보 | 판정 | 이유 |
|---|---|---|---|
| 2장 OrdStatus 우선순위 | 내부 주문 투영에서 "취소 요청 중"과 "부분 체결"이 겹칠 때의 표시·판정 규칙 | 조건부 | 순서 관계만 참고. venue의 상태 집합이 다르다 |
| Pending Cancel ≠ Canceled | 취소 요청만으로 예약을 풀지 않는다 (R07) | 적용 | 원문이 대문자로 경고하는 지점 |
| B.1.b, B.1.c | 취소 대기 중 체결, "too late to cancel" 뒤의 최종 상태 | 적용 | 취소 거절의 결과가 Filled일 수 있음을 테스트 케이스로 |
| B.1.d, B.1.e | 접수 확인 전 취소, 확인과 취소의 엇갈림 | 적용 | 클라이언트 ID로 미확인 주문을 가리킬 수 있어야 한다 |
| B.1.f, G.1.a | 모르는 주문에 대한 취소·조회 응답 | 적용 | "없음" 응답을 "체결 안 됨"으로 읽지 않기 (R06) |
| C, D (cancel/replace) | 정정 경합, 연속 정정 | 조건부 | venue가 정정(amend)을 지원하고 OMS가 쓸 때만. C.3.c의 "요청 수량 < 누적 체결" 처리는 venue 계약 확인 필요 |
| E (unsolicited) | venue가 먼저 취소·축소하는 경우 | 적용 | 요청 없이 오는 종료 사건을 받아들이는 전이가 있어야 한다 (R11) |
| F.1.a, F.1.b, Session 4.9 | 클라이언트 주문 ID 중복 거절, 재송신 시 멱등 응답 | 적용 | venue의 클라이언트 ID 의미(유일성 범위, 재사용 가능 시점)는 venue 문서로 확인. FIX의 "single trading day" 범위는 24시간 시장에 그대로 맞지 않는다 (추론) |
| PossDupFlag vs PossResend | 전송 재시도와 응용 재송신의 구분 | 조건부 | 필드 자체는 없다. "중복 판정은 응용 계층 책임"이라는 원칙만 가져온다 |
| G.1.c | 조회 응답과 사건 스트림의 관계 | 적용 | 조회 결과를 대사 기준점으로 쓰는 규칙 (D5) |
| H (GT), I (TimeInForce) | GTC 일자 경계, IOC/FOK 종료 상태 | 조건부 / 비적용 후보 | I는 해당 주문 유형을 쓸 때. H의 일일 restatement는 거래일 개념이 있는 시장 전제 |
| J (trade cancel/correct) | 체결 취소·정정 뒤 누적 체결과 상태가 되돌아감 | 적용 | 종료 상태 뒤에도 누적이 변할 수 있다 (R11). venue가 실제로 bust를 내는지는 별도 확인 |
| K (trading halt) | venue 거래 정지 중 주문 처리 | 조건부 | venue 동작 확인 필요 |
| L, 4장 | — | 비적용 후보 | 주제가 다르거나 메시지 축약 방식에 관한 것 |

### 3.6 FIA, 규제 문서

| 요소 | OMS 검토 관심사 후보 | 판정 | 이유 |
|---|---|---|---|
| FIA F24, F15 | R01–R15 | 적용 (기존 분석 재사용) | `FIA-OMS-model.md` 참조. Rust OMS로의 이식 여부만 결정 |
| RTS 6 Art. 12, 15–18 (표제) | kill 기능, 사전 통제, 실시간 감시, 사후 통제, 접근 통제의 **목차** | 조건부 (참고용) | 법적 적용은 판단 불가. Art. 12 외 본문 미독 |
| SEC 15c3-5 (c)(1) | 신용·자본 한도, 오주문, 중복 주문 방지의 **분류** | 조건부 (참고용) | 법적 적용은 판단 불가 |
| 가상자산이용자보호법 제10조 | 전략 간 자기 체결 방지 정책 (R10) | 법률 검토 필요 | 조사자가 판정하지 않는다 |
| 같은 법 제6~9, 11~13조 | — | 비적용 후보 | 표제상 가상자산사업자 의무. 최종 판단은 법률 검토 |
| SP 800-218A | — | 비적용 후보 | AI 모델 개발 프로파일 |

---

## 4. 한계

**접근하지 못한 것.**

- `www.iso.org`의 표준 페이지와 OBP는 Cloudflare 챌린지(HTTP 403)로 막혔다. ISO 측 정보는 `committee.iso.org` 미러와 공식 배포처(VDE, IEC 웹스토어)의 미리보기로 대신했다. ANSI 웹스토어 미리보기도 403이었다.
- ISO/IEC 25010:2023 본문(3절 정의, 4절 모델, 부속서 A–D)은 유료다. 읽지 못했다.
- ISO/IEC/IEEE 42010:2022 본문(4절 적합성, 5~8절, 부속서)은 유료다. 조항 제목만 안다. "shall" 요구가 정확히 무엇인지 모른다.
- `eur-lex.europa.eu`는 빈 응답(HTTP 202)과 연결 재설정이었다. RTS 6은 EU 간행물청(`publications.europa.eu`)의 OJ 원문으로 읽었다. 이후 개정이 반영된 통합본인지 확인하지 못했다.
- 과제에 적힌 FIX URL(`/online-specification/order-state-changes/`)은 404였다. 사이트 개편 뒤의 패키지 페이지에서 같은 이름의 문서(EP284)를 받았다. EP284 이후의 Extension Pack이 이 문서를 바꿨는지는 확인하지 못했다.
- NIST SP 800-218 Rev. 1의 최종본 여부. `…/r1/final`이 404라는 것만 확인했다.

**2차 자료에 기대는 것.**

- 25010의 9개 특성·40개 부특성 **전체 목록과 정의 문구**는 iso25000.com에서 왔다 (2차 자료). 1차로 확인한 것은 "nine characteristics"라는 수와 Foreword가 직접 이름을 댄 항목(†)뿐이다. iso25000.com의 문구 일부(User assistance)는 오류로 의심된다.
- 42010의 stakeholder, concern, viewpoint, view, model kind, correspondence, decision, rationale 설명은 동반 사이트의 **2011판** 개념 모델 페이지에서 왔다. 2022판에서 이 정의가 어떻게 바뀌었는지는 Foreword가 말한 범위까지만 안다.

**읽었지만 정독하지 않은 것.**

- ASVS 5.0.0: 요구사항 345행은 CSV로 모두 가졌고 2.4에 인용한 것은 실제로 읽었다. V1, V5, V10, V11, V17의 개별 요구와 각 장 도입부 산문, 부록 A–E는 정독하지 않았다. 해당 장의 판정은 절 제목과 ASVS의 일반 지침에 기댄 것이다.
- FIX Order State Changes: 2.5에 적은 시나리오만 정독했다. A, C.1, C.2, D.1, H, I, K, L과 4장의 표는 제목만 보았다.
- FIX Session Layer: 82쪽 중 4개 절만 읽었다.
- SSDF v1.1: Table 1의 practice·task는 전부 확인했다. Notional Implementation Example과 Reference 열, Appendix A(EO 14028 매핑)는 읽지 않았다.
- SEC 15c3-5: (d)(2) 이후의 항은 읽지 않았다.
- 가상자산이용자보호법: 제10조 ⑤ 이후, 제14~22조, 부칙, 시행령은 읽지 않았다. 열람본이 조사일 당일 시행(2026-10-02)인 개정본이라 직전 판과의 차이도 확인하지 못했다.

**추론으로 표시한 것.** 2.4와 3절의 적용/조건부/비적용 판정 전부, FIX 의미를 비-FIX venue에 옮기는 해석, 25010 Safety를 금전 손실에 대응시킬지 여부. 모두 리드 엔지니어의 결정 사항이다.

**하지 않은 것.** 대상 venue의 API 문서 대조, 구현 코드 검토, 법률 자문. 이 문서 어디에도 "준수", "적합", "인증"의 주장은 없다.
