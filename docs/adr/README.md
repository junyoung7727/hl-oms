# 설계 결정 기록

정합성, 데이터 유실, 외부 효과, 동시성, 보안, 가용성, 성능, 큰 변경 비용에 영향을 주는 결정이다.
각 기록은 같은 열 개 절로 구성된다: 문제와 실패, 일반적으로 만족해야 하는 성질, 이번 프로젝트의 제약, 필수·선택 조건,
대안(가장 단순한 것 포함), 비교, 선택과 이유, 포기한 것과 남은 위험, 근거, 재검토 조건.

비교표의 값은 측정값, 예상값, 정성 판단을 구분해 적었다. 점수는 쓰지 않는다. 필수 조건을 만족하지 않는 대안은 비교에서 제외하고 그 이유를 적었다.

| 기록 | 결정 | 영향 범위 |
|---|---|---|
| [ADR-001](ADR-001-lmax-scope.md) | LMAX에서 따르는 것과 단순화한 것 | 전체 구조 |
| [ADR-002](ADR-002-deterministic-core.md) | 코어는 I/O·시계 없이, 시간과 설정은 입력으로 | 코어, 시험 가능성 |
| [ADR-003](ADR-003-journal-record.md) | 저널 기록 = 입력 + 결정된 이벤트 | 복구, 버전 호환 |
| [ADR-004](ADR-004-commit-gate.md) | 내구화 뒤에만 응답·전송·공개 | 정합성, 지연 |
| [ADR-005](ADR-005-unknown-outcome.md) | 결과 불명 전송은 재전송하지 않고 증거로 해소 | 외부 효과 |
| [ADR-006](ADR-006-idempotency.md) | 멱등성 키 = (전략, 키) + 절대 만료 시각 | 중복 방지 |
| [ADR-007](ADR-007-order-state.md) | 주문 단계는 증거 수준, 포지션은 체결 키로만 | 주문 상태 |
| [ADR-008](ADR-008-risk-reservation.md) | 잠재 체결을 포함한 수량 구간 예약 | 리스크 |
| [ADR-009](ADR-009-numeric.md) | 1e-8 고정소수점, 십진 문자열 | 수치 |
| [ADR-010](ADR-010-overload-storage.md) | 큐 셋과 종류별 과부하 동작, 저장 실패는 정지 | 과부하 |
| [ADR-011](ADR-011-file-format.md) | 파일 형식, 꼬리와 중간 손상의 구분, 버전 | 영속화 |
| [ADR-012](ADR-012-threads-crates.md) | 커밋 스레드 분리(측정), 채택하지 않은 최적화 | 성능 |
| [ADR-013](ADR-013-availability.md) | 단일 인스턴스, 수동 복구 | 가용성 |
| [ADR-014](ADR-014-reconciliation.md) | 무상태 대조기, 구간 포지션 검사 | 복구 |
| [ADR-015](ADR-015-api-trust.md) | Unix 소켓 둘, 읽기 전용 대시보드 | 보안 |
| [ADR-016](ADR-016-observability.md) | 읽기 모델 기반 관측, 판정 종류 표시 | 운영 |
| [ADR-017](ADR-017-hyperliquid-gateway.md) | Hyperliquid 게이트웨이를 직접 구현, 공백은 보수적으로 | 거래소 |
| [ADR-018](ADR-018-margin-rules.md) | 증거금 규칙: 계좌 레버리지 검사, 감소 전용 상태, 거래소 설정 확인(구현됨. 실제 거래소 미확인) | 리스크 |
| [ADR-019](ADR-019-market-conn-slots.md) | 시세 레코드의 시장은 conn_id 구간으로 정한다(Bybit 프레임에 시장이 없음) | 시세 |
| [ADR-020](ADR-020-md-archiver-input-rules.md) | 아카이버가 계약에 없는 다섯 곳(종목정보 응답, 전날 상태, 완료 표시, 보존, update_id 메타데이터)을 정하는 방법 | 시세 |
| [ADR-021](ADR-021-venue-gateway-trait.md) | 게이트웨이 trait 하나와 구동기 하나, 지금 쓰는 네 가지만(동작 변화 없음) | 거래소 |
| [ADR-022](ADR-022-wire-id-form.md) | 거래소마다 고객 주문번호 형식(16진수 32자리, base32 27자)을 따로 두고 상태 API도 그 형식으로 | 거래소 |
| [ADR-023](ADR-023-venue-made-executions.md) | 펀딩·거래소가 만든 포지션 변경·분류 못 한 실행을 저널에 따로 기록하고 불일치로 막음, 기록 스키마 2 | 거래소 |
| [ADR-024](ADR-024-absence-count-and-market-ref.md) | 부재 판정의 "없음" 횟수(Bybit 2), Bybit 만료는 게이트웨이가 recv window로 강제, 새 주문의 market_ref | 거래소 |
| [ADR-025](ADR-025-journal-tail.md) | OMS 밖에서 저널을 읽는 법: 잠금 없이 내구화 번호까지, 이벤트 필드 표를 시험으로 인코더와 맞춤 | 운영 |
| [ADR-026](ADR-026-observability-projector.md) | 저널을 따라 읽어 VictoriaMetrics·VictoriaLogs로 내보내는 별도 프로젝터, 저장소가 받은 줄까지만 커서, 죽은 OMS의 미확정 줄(ADR-016 §8을 대신함) | 운영 |
| [ADR-027](ADR-027-trading-export.md) | 저널에서 레이크 거래 테이블(orders·fills·events)을 내구화된 기록, 완결된 날짜만, 바뀐 파일만 쓰기 | 운영 |
| [ADR-028](ADR-028-bybit-gateway.md) | Bybit 게이트웨이: 0600 자격 파일, 서명, 키 거부 비재시도, 읽기만 하는 시작 확인과 IP 확인(만드는 중) | 거래소 |
| [ADR-029](ADR-029-oms-direct-alerts.md) | OMS 직접 알림: 공개된(내구화된) 상태만 읽는 노드 안의 평가 스레드, 지나가는 상태의 유예 30초, 치명 알림과 종료 전 전송(ADR-016 §8의 "알림이 없다"를 대신함) | 운영 |
| [ADR-030](ADR-030-kill-switch-holds.md) | 킬 스위치와 운영자가 푸는 보류: 저널에 남는 래치와 `clear_hold`, 내구화된 명령이 시작하는 확인 루프, 킬 마커, 전략 연결 감시, 모니터링 게이트(U18b1·U18b2 구현, U18b3~4 남음) | 리스크 |
