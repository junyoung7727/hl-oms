# ADR-036 OMS·워처·프로젝터·내보내기의 배치 파일과 워처의 정지 권한

상태: 채택 (2026-11-25).

운영 절차: [docs/operations/observability.md](../operations/observability.md) §13. 설계 출처: 통합 설계 원장(`RESEARCH/CRYPTO_TRADING_INTEGRATED_TN_20261118/10_DESIGN_DRAFT.md`, 저장소 밖) §7.5~§7.7, §8.2, §8.5와 개정 R2(S7, SHOULD 5), R4(SHOULD 2), R5(SHOULD 1·3), TN 표 7-7·8-6·8-6a·9-4. 앞선 결정: [ADR-035](ADR-035-observability-deployment-files.md)(관측 스택의 배치와 그 검사), [ADR-032](ADR-032-watcher-kill-and-alerts.md)(워처 킬).

## 1. 문제와 발생 가능한 실패

- `oms-node`, 워처, 프로젝터, `oms-export`는 실행 파일과 설정만 있었고 이 PC에서 어떻게 도는지 적힌 곳이 없었다. 런북의 OMS 단위는 시험하지 않은 예시였고 `Restart=on-failure`였다: 워처가 멈춘 OMS가 사람 없이 돌아온다.
- 워처의 킬 단계 K3(`systemctl stop <oms_unit>`)는 그것을 허락하는 규칙이 없어 늘 실패했다.
- 저널 루프 마운트가 빠지면 그 경로는 레이크 파일 시스템의 평범한 디렉터리다. 거기서 뜬 OMS는 진짜 저널 옆에 빈 새 저널을 시작한다(킬 래치도 없다).
- 킬 마커의 자리가 정해지지 않았다. tmpfs에 두면 재부팅에 사라진다.
- 검사가 없으면 편집 한 번에 OMS 단위가 자동 재시작을 얻거나, OMS·워처가 OOM 보호를 잃거나, 워처가 아무 단위나 멈출 수 있게 된다.

## 2. 일반적으로 만족해야 하는 성질

- 이 저장소는 아무것도 설치·활성화·시작하지 않는다. 단위 파일, polkit 규칙, 설치 지도와 그 검사만 있다.
- OMS의 모든 시작은 사람이 한다(충돌, 치명 종료, 워처 킬, 재부팅 뒤 모두).
- 워처는 `oms-node.service`의 `stop` 하나만 할 수 있다.
- 저널 파일 시스템이 없으면 OMS도 그것을 읽는 단위도 시작하지 않는다(조용히 건너뛰지 않고 실패한다).
- 킬 마커는 재부팅 뒤에도 남는다.
- OMS와 워처는 메모리 압박에서 마지막에 죽고 스왑되지 않는다.

## 3. 제약

| 사실 | 출처 |
|---|---|
| `admin.sock`·`monitor.sock`은 0600이고 OMS 자신의 uid만 받는다 | `crates/oms-node/src/node.rs:365`, `:374`, `crates/oms-node/src/server.rs:188–192` |
| 워처의 정지는 정확히 `/usr/bin/systemctl [--user] --no-ask-password stop -- <oms_unit>`이다 | `crates/watcher/src/stop.rs:27–44` |
| OMS는 `sd_notify`를 보내지 않는다. 종료 코드 0(SIGTERM), 2(설정·치명), 3(시작 실패), panic은 SIGABRT. 종료 전 알림 전송은 최대 30초 | `crates/oms-node/src/node.rs:15–23`, `crates/oms-node/src/config.rs:210–212` |
| OMS의 직접 알림은 상태 파일이 없다: 모든 시작은 사람이 한다는 전제 | `crates/oms-node/src/alerts.rs:605–608` |
| 킬 마커는 절대 경로, 데이터 디렉터리 밖, 디렉터리가 있어야 한다. OMS는 시작에 한 번 읽고 `clear_hold kill` 뒤 지운다 | `crates/oms-node/src/config.rs:80–113`, `crates/oms-node/src/server.rs:762–766` |
| 프로젝터는 `/proc/locks`에서 저널 `LOCK`을 찾는다 | `crates/oms-projector/src/probe.rs:114–130` |
| 워처는 OMS가 `/api/status`에 답한 뒤에만 저널을 읽는다. W5는 시작 확인이 끝나면 OMS가 답한 적이 없어도 무장한다 | `docs/operations/watcher.md`, `crates/watcher/src/w5.rs:165–172` |
| 일일 작업 잠금의 사용자는 모두 root다 | ADR-035 D4 |
| `ProtectSystem=strict`에서 `/run`은 읽기 전용이고, 잠금 파일이 없으면 flock이 종료 코드 73으로 실패한다. `/run/lock`은 부팅마다 비는 tmpfs다 | 이 PC에서 bwrap으로 재현(2026-11-25) |
| systemd는 틀린 `Restart=` 값을 거부하지 않고 경고만 한다 | `systemd-analyze verify`, systemd 259, 이 PC |

## 4. 필수 조건과 선택 조건

필수: 파일만, OMS는 사람만 시작, 워처의 정지 권한은 그 단위의 `stop`만, 저널 마운트 확인, 영속하는 킬 마커, TN 표 8-6a의 보호와 설계 §8.5의 상한, 소유자 확인 경로를 건드리지 않음. 선택: OMS와 워처의 사용자 분리(코드 변경이 필요해 범위 밖).

## 5. 대안

- A. 워처를 root로 돌림(polkit 불필요). B. 사용자 단위(`systemctl --user`, lingering). C. sudoers 한 줄. D. polkit 규칙 하나.
- E. OMS `Restart=on-failure`와 시작 횟수 제한(런북의 옛 예시). F. OMS `Restart=no`, `[Install]` 없음.
- G. OMS를 부팅 때 활성화. H. 워처를 부팅 때 활성화하지 않음. I. 워처를 부팅 때 활성화.
- J. `RequiresMountsFor=`만. K. `Condition…MountPoint`. L. `AssertPathIsMountPoint=`.

## 6. 비교

| | 장점 | 실패 모드 | 비용 |
|---|---|---|---|
| A | 권한 설정이 없음 | 워처가 모든 단위를 멈출 수 있고, `admin.sock`의 uid 검사에 막힘(OMS의 uid가 아님) | 없음 |
| B | root 불필요 | lingering이 필요하고 OMS가 시스템 슬라이스 밖으로 나감 | 사용자 세션 관리 |
| C | 익숙함 | 워처는 `sudo`가 아니라 `systemctl`을 부른다 | 코드 변경 |
| D | `StopUnit` 하나만 허락 | systemd가 polkit에 `unit`·`verb`를 넘기지 않으면 규칙이 맞지 않아 K3가 실패(A-22) | 설치 때 확인 한 번 |
| E | 충돌 뒤 사람 없이 돌아옴 | 워처가 멈춘 OMS도 돌아옴. 직접 알림의 반복 상태가 없어 재시작마다 같은 P1 | 없음 |
| F | 모든 시작이 사람 | 밤의 충돌 뒤 사람이 올 때까지 거래가 멈춤 | 운영자의 시작 |
| G | 재부팅 뒤 바로 거래 | 사람 없는 시작 | 없음 |
| H | 재부팅 뒤 알림이 적음 | 죽은 OMS가 거래소에 남긴 주문을 아무도 취소하지 않음 | 없음 |
| I | 재부팅 뒤 남은 주문을 일괄 취소하고 다음 OMS 시작을 래치 | 재부팅마다 킬 알림과 해제 한 번, 사람이 OMS를 띄울 때까지 `OmsDown`·`WatcherKillHaltOwed` P1 반복 | 해제 명령 한 번 |
| J | 순서만 맞춤 | 마운트 단위가 없으면 아무것도 하지 않음 | 없음 |
| K | 조용함 | 단위를 건너뛰어 아무도 모름 | 없음 |
| L | 시작이 크게 실패 | 마운트가 없으면 워처도 뜨지 않아 재부팅 킬이 없음 | 없음 |

## 7. 선택과 이유

D, F, I, L(과 J). 돈과 데이터에 안전한 쪽이다.

- 신원: OMS와 워처는 같은 사용자 `hloms`(`admin.sock`의 uid 검사가 강제한다). OMS는 `UMask=0027`로 새 저널 파일을 그룹이 읽게 한다. 프로젝터는 관측 사용자 `hlobs`에 보조 그룹 `hloms`: 저널은 읽고 `admin.sock`(0600, uid 검사)과 0600 자격 파일은 못 연다. 내보내기는 root(일일 작업 잠금의 사용자, ADR-035).
- OMS(`oms-node.service`): `Restart=no`, 재시작·감시 키 없음, `[Install]` 없음. 다른 단위는 `After=`/`Before=`로만 이름을 쓸 수 있고 같은 이름의 타이머·소켓·경로 단위는 없다. `Type=simple`(sd_notify 없음). `TimeoutStopSec=60`: 정리, 마지막 스냅샷, 알림 전송 최대 30초. 워처의 `stop_wait_ms` 10초와 일부러 다르다: 워처는 10초 뒤 `OMS 정지 미확인`을 알리고 일괄 취소로 넘어가며, systemd의 정지 작업은 늦어도 60초에 SIGKILL로 끝낸다. 더 긴 `stop_wait_ms`는 걸린 OMS의 주문 취소를 늦추고, 더 짧은 `TimeoutStopSec`는 OMS의 정리를 자른다.
- 워처(`watcher.service`): `Restart=on-failure`, `RestartSec=5`, `RestartPreventExitStatus=2`(거부된 시작을 되풀이하지 않음), `After=oms-node.service`만(멈추는 의존 없음: OMS를 멈춰도 워처의 킬 루프는 이어지고, 종료 때 워처가 먼저 멈춘다). 부팅 때 활성화한다: 저널이 마운트돼 있으면 재부팅 뒤 워처가 꺼진 OMS를 킬한다(마커, 꺼진 단위의 `stop`, 자기 키로 일괄 취소). 그 킬은 OMS가 이 프로세스에서 답한 적이 없어 마커 경로를 확인하지 못하므로 정지를 빚는다. 사람이 OMS를 띄울 때까지 `OmsDown`과 `WatcherKillHaltOwed`가 P1로 30분마다 오는 것이 정상이고, OMS가 처음 답하면 워처가 `watcher_kill`을 보내 `늦은 정지: OMS가 다시 답해 watcher_kill로 래치함`을 알린다. 운영자는 그 줄을 본 뒤에만 `clear-hold --reason kill`을 한다.
- 정지 권한: `deploy/observability/polkit/50-hl-oms-watcher-stop.rules` 하나. `org.freedesktop.systemd1.manage-units`에서 사용자 `hloms`, `unit` = `oms-node.service`, `verb` = `stop`일 때만 `YES`. 시작, 재시작, kill과 다른 단위는 polkit의 기본(비관리자 거절)으로 남는다. 워처 설정은 `oms_unit = "oms-node.service"`, `systemd = "system"`.
- 저널: 네 단위 모두 `RequiresMountsFor=/mnt/nvme/hl-oms-journal`과 `AssertPathIsMountPoint=/mnt/nvme/hl-oms-journal`. 데이터 디렉터리 `/mnt/nvme/hl-oms-journal/data`는 OMS만 쓰고(`ReadWritePaths=`) 나머지 셋은 읽기만 한다(`ReadOnlyPaths=`; 읽기 전용 바인드 위의 소켓도 연결된다, A-23).
- 킬 마커: `StateDirectory=hl-oms`(0700)를 OMS와 워처가 함께 쓴다 → `/var/lib/hl-oms/kill.marker`. 영속하고, `ProtectSystem=strict`에서 쓸 수 있고, 저널 파일 시스템이 차도 쓸 수 있다. `RuntimeDirectory`(tmpfs)는 쓰지 않는다. 프로젝터의 커서는 `StateDirectory=oms-projector`.
- 격리: 네 단위 모두 `NoNewPrivileges`, `ProtectSystem=strict`, `ProtectHome`, `PrivateTmp`. OMS와 워처는 Bybit 비밀을 쥐므로 `LimitCORE=0`. 프로젝터는 `/proc/locks`를 읽으므로 `ProcSubset=` 없음.
- 메모리(설계 §8.5, TN 표 8-6·8-6a): OMS 512M, 워처 100M, 프로젝터 100M, 내보내기 4G. OMS와 워처는 `trading.slice`, `OOMScoreAdjust=-900`, `MemorySwapMax=0`, `MemoryMin`(상한의 절반), `IOWeight=1000`. 프로젝터는 슬라이스만(CPU 가중치). 슬라이스 `MemoryMin=818M` = 512M + 256M + 50M.
- 내보내기: `oms-export.service`(oneshot, 일일 작업 잠금 아래, `TimeoutStartSec=6h`, 슬라이스 밖)와 `oms-export.timer`(매일 03:30 UTC, `Persistent=true`). `ReadWritePaths`는 정확히 `/mnt/nvme/lake/crypto/trading`과 `/run/lock`: `ProtectSystem=strict`에서 flock이 잠금 파일을 만들 수 있게. 종료 코드 3(OMS가 답하지 않음)은 단위를 실패로 남긴다(`SuccessExitStatus=3` 아님): 하루가 빠진 것이 `systemctl --failed`에 보여야 한다.
- 검사: ADR-035의 xtask 통합 시험에 규칙 U20(OMS는 사람만 시작), U21(워처의 재시작과 의존), U22(신원과 polkit 규칙의 글자), U23(저널 마운트, 경로, 상태 디렉터리, `ProtectSystem=strict` 아래의 `/run/lock`), U24(격리, 코어 덤프, 정지 시간, 슬라이스)를 더하고, U3(보호 묶음에 OMS와 워처), U4(슬라이스 합), U5·U6·U7·U14·U19(타이머, 상한, 잠금, polkit 설치 경로, 실행 제한), P1(`.rules`는 `polkit/` 아래만)을 넓혔다.

## 8. 포기한 장점, 남은 위험, 소유자가 정할 것

- 포기: 충돌한 OMS가 사람 없이 돌아오는 것. 밤의 충돌 뒤 사람이 올 때까지 거래가 멈춘다.
- 위험: A-22가 틀리면 polkit 규칙이 맞지 않아 K3가 실패한다. 그때 워처의 알림은 `OMS 정지 실패`이고 `watcher_kill_step{step="stop"}` = 0이다. 살아 있는 OMS는 K2(`watcher_kill`)가 여전히 래치한다. 설치 때 확인한다(§13.6).
- 위험: 재부팅 뒤 저널이 마운트되지 않으면 워처가 뜨지 않아(마운트 확인) 재부팅 킬도 일괄 취소도 없다. 죽은 OMS가 남긴 주문은 거래소에 남고, `WatcherDown`과 `systemctl --failed`만 보인다. 재부팅 뒤의 첫 단계를 마운트와 워처 확인으로 둔다(운영 절차).
- 위험: OMS와 워처가 같은 사용자라 서로의 자격 파일을 읽을 수 있다. 나누려면 `admin.sock`·`monitor.sock`의 uid 규칙을 바꾸는 코드 변경이 필요하다.
- 위험: 내보내기의 종료 코드 1(레이크 쓰기·스키마 오류)은 지표가 없어 실패한 단위로만 보인다. 저널은 남으므로 다음 성공한 실행이 밀린 날짜를 쓴다.
- 위험: 운영자의 `/etc/hl-oms/*.toml`이 단위와 맞는지(예: 워처의 `oms_unit`, 같은 킬 마커 경로)는 아무것도 검사하지 않는다. 처음 킬할 때 `OMS 정지 실패`로만 드러난다.
- 소유자가 정할 것(기본값으로 진행): (1) OMS 부팅 시작(기본: 없음, 사람이 띄움), (2) 워처 부팅 시작(기본: 활성화, 재부팅마다 킬 한 번과 해제 한 번; 저널 없는 재부팅 킬은 다른 단위 설계), (3) OMS와 워처의 한 사용자(기본: 함께 `hloms`), (4) 워처·프로젝터 100M 상한(기본: 설계 값, 시범 운용에서 잰다), (5) OMS가 꺼진 날의 내보내기(기본: 종료 코드 3으로 실패로 남김; `Persistent=true`라 03:30을 놓친 재부팅마다 실패가 하나 남는다).

## 9. 근거

- 시험: `crates/xtask/tests/deploy/`의 `the_deploy_tree_passes`, `tree::every_file_parses_by_its_kind`, `units::an_oms_node_unit_that_restarts_fails`, `units::no_unit_can_start_the_oms_node`, `units::an_oms_or_watcher_unit_without_oom_score_adjust_fails`, `units::an_oms_or_watcher_unit_without_memory_swap_max_zero_fails`, `units::the_watcher_restarts_but_never_follows_the_oms`, `units::the_oms_and_watcher_share_one_user`, `units::the_stop_permission_names_only_the_oms_unit_and_stop`, `units::the_oms_units_refuse_an_unmounted_journal`, `units::the_kill_marker_directory_is_persistent`, `units::the_projector_can_read_proc_locks`, `units::the_export_can_take_the_daily_lock`, `units::the_oms_units_are_sandboxed`와 상한·타이머·잠금·슬라이스·설치 지도 시험의 새 행.
- 변이: M362~M371(`cargo run -p xtask -- mutants --only M36`, `--only M37`).
- 도구: `systemd-analyze verify`(systemd 259)를 단위·타이머·슬라이스 사본에 돌렸다(바이너리 경로만 바꿈, 종료 코드 0). polkit 규칙은 `node --check`(문법만)를 통과했다. polkit 규칙은 root 없이 불러올 수 없어 동작은 확인하지 않았다(A-22).

## 10. 재검토 조건

- 설치 확인(§13.6)에서 polkit 규칙이 `stop`을 허락하지 않거나 다른 것을 허락할 때(A-22).
- OMS와 워처의 소켓 uid 규칙이 바뀔 때(사용자 분리).
- OMS가 `sd_notify`나 감시를 보내게 될 때.
- 워처나 프로젝터가 100M에 닿을 때.
- 둘째 OMS나 둘째 거래소를 이 PC에 둘 때(평범한 단위 이름, 슬라이스 예약).
