# 계약 추적표

> 자동 생성 문서다. 고치려면 `registry/*.toml`을 고치고 `cargo run -p xtask -- docs`를 실행한다.
> 소스 다이제스트 `4ba2cb2744d3dae4`, 검증 결과의 다이제스트 `4f6788394bb348d6`.

| 계약 | 제목 | 상태 전이(이벤트) | 설계 결정 | 코드 | 주장 |
|---|---|---|---|---|---|
| [C-COMMIT](../../docs/contracts/commit.md) | 기록·내구화·적용·응답·전송의 순서 |  | ADR-003, ADR-004, ADR-012 | `crates/oms-node/src/pipeline.rs` | V-COMMIT-1(이전 버전 결과), V-COMMIT-2(이전 버전 결과), V-COMMIT-3(이전 버전 결과), V-COMMIT-4(이전 버전 결과), V-COMMIT-5(이전 버전 결과) |
| [C-NEW](../../docs/contracts/order-lifecycle.md) | 신규 주문 수용과 예약 | `order_accepted` `request_rejected` | ADR-008, ADR-009 | `crates/oms-core/src/decide.rs` | V-NEW-1(이전 버전 결과), V-NEW-2(이전 버전 결과) |
| [C-IDEM](../../docs/contracts/idempotency.md) | 멱등성 | `purged` | ADR-006 | `crates/oms-core/src/decide.rs` | V-IDEM-1(이전 버전 결과), V-IDEM-2(이전 버전 결과), V-IDEM-3(이전 버전 결과) |
| [C-SEND](../../docs/contracts/order-lifecycle.md) | 전송 결과 | `order_acked` `venue_oid_learned` `order_in_doubt` | ADR-005, ADR-007 | `crates/oms-core/src/decide.rs` | V-DOUBT-1(이전 버전 결과), V-BOOT-1(이전 버전 결과) |
| [C-DOUBT](../../docs/contracts/order-lifecycle.md) | 결과 불명의 해소 | `order_in_doubt` | ADR-005 | `crates/oms-core/src/decide.rs` | V-DOUBT-1(이전 버전 결과), V-DOUBT-2(이전 버전 결과), V-DOUBT-3(이전 버전 결과) |
| [C-FILL](../../docs/contracts/order-lifecycle.md) | 체결 기장 | `fill_booked` `fill_parked` `fill_unparked` `external_fill_booked` | ADR-007 | `crates/oms-core/src/decide.rs`<br>`crates/oms-core/src/core.rs` | V-FILL-1(이전 버전 결과), V-EXT-1(이전 버전 결과), V-MIS-1(이전 버전 결과), V-FUZZ-1(이전 버전 결과) |
| [C-CANCEL](../../docs/contracts/order-lifecycle.md) | 취소 | `cancel_requested` `cancel_sent` `cancel_outcome_noted` | ADR-005, ADR-007 | `crates/oms-core/src/decide.rs` | V-CANCEL-1(이전 버전 결과) |
| [C-TERM](../../docs/contracts/order-lifecycle.md) | 종결 | `venue_terminal` `order_closed` `missing_fills_flagged` `status_noted` `order_recheck` | ADR-007 | `crates/oms-core/src/decide.rs` | V-TERM-1(이전 버전 결과) |
| [C-JOURNAL](../../docs/contracts/recovery.md) | 저널에서 상태 복원 |  | ADR-003, ADR-011, ADR-013 | `crates/oms-node/src/recovery.rs`<br>`crates/oms-journal/src/segment.rs` | V-JRN-1(이전 버전 결과), V-JRN-2(이전 버전 결과), V-JRN-3(이전 버전 결과), V-JRN-5(이전 버전 결과), V-REPLAY-1(이전 버전 결과), V-BOOT-2(이전 버전 결과) |
| [C-SNAP](../../docs/contracts/recovery.md) | 스냅샷 |  | ADR-011 | `crates/oms-journal/src/snapshot.rs`<br>`crates/oms-node/src/pipeline.rs` | V-JRN-4(이전 버전 결과), V-PERF-3(이전 버전 결과) |
| [C-BOOT](../../docs/contracts/recovery.md) | 재시작 | `booted` `config_activated` | ADR-005, ADR-002 | `crates/oms-core/src/decide.rs`<br>`crates/oms-node/src/node.rs` | V-BOOT-1(이전 버전 결과), V-CFG-1(이전 버전 결과), V-BOOT-2(이전 버전 결과), V-COMMIT-4(이전 버전 결과) |
| [C-LINK](../../docs/contracts/reconciliation.md) | 링크 상태 | `link_changed` | ADR-014 | `crates/oms-core/src/decide.rs` | V-RECON-1(이전 버전 결과), V-LINK-1(이전 버전 결과) |
| [C-RECON](../../docs/contracts/reconciliation.md) | 재확인과 대조 | `fills_synced` `open_orders_swept` `positions_verified` `status_noted` `order_recheck` | ADR-014 | `crates/oms-core/src/decide.rs`<br>`crates/oms-core/src/needs.rs`<br>`crates/oms-node/src/venue/reconciler.rs` | V-RECON-1(이전 버전 결과), V-RECON-2(이전 버전 결과) |
| [C-MISMATCH](../../docs/contracts/reconciliation.md) | 불일치와 조정 | `mismatch_raised` `mismatch_acked` `position_adjusted` | ADR-014 | `crates/oms-core/src/decide.rs` | V-RECON-1(이전 버전 결과), V-EXT-1(이전 버전 결과), V-MIS-1(이전 버전 결과) |
| [C-OVERLOAD](../../docs/contracts/overload.md) | 과부하와 저장 공간 | `storage_low_changed` | ADR-010 | `crates/oms-node/src/queues.rs`<br>`crates/oms-node/src/server.rs` | V-OVL-1(이전 버전 결과), V-OVL-2(이전 버전 결과), V-OVL-3(이전 버전 결과), V-API-3(이전 버전 결과), V-PERF-2(이전 버전 결과) |
| [C-OPS](../../docs/contracts/api.md) | 운영 명령 | `operator_halted` `operator_resumed` `ref_price_updated` | ADR-008, ADR-015 | `crates/oms-core/src/decide.rs` | V-API-1(이전 버전 결과), V-CANCEL-1(이전 버전 결과) |
| [C-API](../../docs/contracts/api.md) | 로컬 API |  | ADR-015 | `crates/oms-api/src/lib.rs`<br>`crates/oms-node/src/server.rs` | V-API-1(이전 버전 결과), V-API-2(이전 버전 결과), V-API-3(이전 버전 결과), V-DASH-1(이전 버전 결과) |
| [C-GW](../../docs/contracts/gateway.md) | 게이트웨이 계약 |  | ADR-005, ADR-014, ADR-017 | `crates/oms-node/src/venue/mod.rs`<br>`crates/oms-node/src/venue/simnet.rs`<br>`crates/oms-node/src/venue/hyperliquid/mod.rs` | V-HL-1(이전 버전 결과), V-HL-2(이전 버전 결과), V-HL-3(미실행), V-SLICE-1(이전 버전 결과) |
| [C-MARGIN](../../docs/contracts/margin.md) | 증거금 규칙 |  | ADR-008, ADR-018 | `crates/oms-core/src/config.rs`<br>`crates/oms-core/src/decide.rs` | V-MARGIN-1(미실행) |

모든 저널 이벤트는 위 표의 어느 계약에 속한다(생성기가 확인한다).
