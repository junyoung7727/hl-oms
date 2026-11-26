# 운영 절차

주인: 준영
검토일: 2026-11-25

대상: 단일 호스트에서 `oms-node` 하나를 운용하는 운영자. 기준: 2026-10-02.

이 문서의 절차 가운데 **실제로 실행해 확인한 것**과 **적어만 둔 것**을 구분한다.

| 절차 | 상태 |
|---|---|
| 빌드, 가상 거래소 실행, 대시보드 | 실행해 확인 |
| 정상 종료와 재시작, 강제 종료 후 재시작 | 자동 시험으로 확인(`e2e`) |
| 저널 손상 시의 시작 거부와 꼬리 보존 | 자동 시험으로 확인 |
| 설정 버전 변경 | 자동 시험으로 확인 |
| 운영 명령(중지, 재개, 전체 취소, 불일치 확인, 수동 종결, 포지션 조정) | 코어 시험으로 전부 확인. 실제 프로세스에서 실행한 것은 중지, 전체 취소, 불일치 확인, 상태 조회다. `omsctl` 자체의 자동 시험은 없다(§3의 명령을 손으로 실행했다) |
| 커밋 경계마다의 프로세스 중단과 재시작 | 자동 시험으로 확인(`crash_points`) |
| systemd 단위 | 적어만 둠. 이 호스트에서 설치해 보지 않았다. 관측 스택, 시세 단위(수집기·봉인·아카이버·레이크 감사), OMS·워처·프로젝터·내보내기의 단위 파일과 워처의 정지 권한(polkit 규칙)은 `deploy/observability/`, 절차는 [observability.md](observability.md)(배치 검사 `cargo test -p xtask --test deploy`와 `systemd-analyze verify`로만 확인) |
| 서버 유실 뒤 복구 | 적어만 둠. 연습하지 않았다 |
| 증거금 규칙의 절차(7.8 감소 전용 상태의 해제, 7.9 입출금 뒤의 기준 재설정) | 명령은 자동 시험으로 확인(코어 시험, 가상 거래소에 붙인 노드 프로세스에서 `clear-reduce`, `rebase-day`, 강제 종료 뒤의 상태). `omsctl` 실행 파일 자체는 시험하지 않았다(그 본문 함수를 부른다). **실제 거래소에서는 한 번도 해 보지 않았다** |
| testnet 실주문 | 실행하지 않았다(U-01). 증거금 규칙의 testnet 예행과 가정 T1~T11의 확인도 하지 않았다. mainnet 전에 필요하고 사용자의 승인이 필요하다(§10) |
| 버전 올리기와 되돌리기 | 스키마 검사는 시험으로 확인. 절차 전체는 연습하지 않았다 |
| 직접 알림(7.11: 토큰 파일, 시작·정지·치명 알림, 조건별 뜻) | 자동 시험으로 확인(loopback 가짜 텔레그램, 가상 거래소에 붙인 노드 프로세스). **실제 텔레그램에는 보낸 적이 없다** |
| 모니터링 게이트(7.16) | 기록 시각을 넣은 코어 시험과 가상 거래소에 붙인 노드 프로세스(실제 시간)로 확인. 하트비트는 시험의 클라이언트가 보냈다. 워처(U19a7부터 하트비트와 불일치를 보낸다)는 loopback 가짜 소켓으로만 시험했다. **실제 거래소에서는 한 번도 해 보지 않았다** |
| 전략 연결 감시(7.15) | 코어 시험, 시간을 입력으로 받는 노드 표 시험, 가상 거래소에 붙인 노드 프로세스(실제 시간)로 확인. **실제 거래소에서는 한 번도 해 보지 않았다** |
| 보류(7.12 킬 래치와 킬 마커, 7.13 워처 불일치, 7.14 거래소 변경) | 코어 시험과 가상 거래소에 붙인 노드 프로세스로 확인(`clear-hold`는 `omsctl`의 본문 함수로). 가상 거래소와 Hyperliquid는 일괄 취소가 없어 킬 루프 대신 `unsupported`로 답한다. Bybit 킬 루프(U18b2, C-KILL)는 loopback 가짜 Bybit와 수동 시계, 가짜 Bybit에 붙인 노드 프로세스로 확인. **실제 거래소에서는 한 번도 해 보지 않았다** |

## 1. 구성 요소

| 실행 파일 | 역할 |
|---|---|
| `oms-node run --config <file>` | OMS 본체. 한 데이터 디렉터리에 하나만 뜬다 |
| `oms-node verify --data-dir <dir> [--re-decide]` | 저널을 읽기만 하며 검사한다. 실행 중인 노드 옆에서 돌려도 된다 |
| `oms-node trace --data-dir <dir> --order <N>` | 저널에서 한 주문의 기록을 순서대로 출력한다 |
| `oms-node dashboard` | 노드 없이 문서와 검증 결과만 보는 화면 |
| `omsctl --socket <path> …` | 로컬 소켓 클라이언트. 전략용과 운영용 |
| `simvenue <socket>` | 가상 거래소. 시험과 연습용 |

데이터 디렉터리의 내용:

| 파일 | 내용 | 지워도 되는가 |
|---|---|---|
| `INSTANCE` | 인스턴스 ID. 거래소 주문 식별자의 앞 절반 | **안 된다** |
| `LOCK` | 중복 실행 방지 잠금 | 프로세스가 없을 때는 무관 |
| `journal-<첫 순번>.oj` | 저널 세그먼트. 64 MiB마다 새 파일 | 안 된다(§8 보관 참고) |
| `snapshot-<순번>.snap` | 스냅샷. 최신 것 하나만 쓰인다 | 오래된 것은 된다 |
| `torn/` | 복구 때 잘라 낸 불완전한 꼬리의 원본 | 확인 뒤에 |
| `oms.sock`, `admin.sock` | 로컬 소켓 | 프로세스가 다시 만든다 |

종료 코드: 0 정상 종료, 2 치명 상태(저장 실패나 코어 결함. 종료 중에 일어나도 2), 3 시작 거부(잠금, 손상, 설정).
코드 없이 SIGABRT로 끝났으면 panic이다(프로그램 결함). 로그 마지막 줄의 `"msg":"fatal"`에 사유가 있다. 그대로 다시 띄워도 되지만 같은 입력에서 다시 죽을 수 있으므로 로그를 보관한다.

## 2. 빌드와 설치

```sh
cargo build --release
T=$(cargo metadata --format-version 1 --no-deps | python3 -c 'import json,sys; print(json.load(sys.stdin)["target_directory"])')
ls $T/release/{oms-node,omsctl,simvenue}
$T/release/oms-node version        # 버전과 소스 다이제스트
cargo run --release -p xtask -- digest   # 같은 다이제스트가 나와야 한다
$T/release/watcher --version       # 정확히 watcher <버전>
```

설치하는 바이너리는 위처럼 기능 없이 만든다(작업 공간의 어떤 crate도 시험 기능을 켜지 않는다). `/usr/local/bin/watcher --version`은 정확히 `watcher <버전>`을 찍어야 한다. `--features test-endpoints`(워처)나 `failpoints`(OMS)로 만든 바이너리는 시험용이고 설치하지 않는다: 끝점을 시험 변수(`WATCHER_TEST_ENDPOINTS`, `OMS_TEST_BYBIT_WIRING`, `OMS_TEST_TELEGRAM_API`)에서만 받고 그것 없이는, 또 mainnet에서는 시작하지 않는다(V-TESTHOOK-1). OMS의 `failpoints` 빌드는 버전 줄로 구별되지 않으므로 빌드 명령으로만 막는다.

배포할 때는 `oms-node`, `omsctl`, 설정 파일, `dashboard/` 디렉터리를 옮긴다. `dashboard/` 옆에 `docs/`와 `crates/`가 없으면 화면의 문서·코드 열람만 빠진다.

저널을 두는 파일 시스템: 로컬 ext4. tmpfs나 네트워크 파일 시스템에 두지 않는다(내구성과 잠금의 전제가 깨진다, [ADR-013](../adr/ADR-013-availability.md)).
새 호스트에서는 먼저 저장 장치의 동기 쓰기 시간을 잰다. 이 값이 주문 응답 지연의 하한이다.

```sh
cargo run --release -p xtask -- fsync-probe --dir <data_dir가 놓일 디렉터리>
```

### Bybit 자격 파일 (`[venue] kind = "bybit"`의 `credentials_file`)

Bybit 게이트웨이가 시작할 때 한 번 여는 파일이다([ADR-028](../adr/ADR-028-bybit-gateway.md), RSA 키는 [ADR-034](../adr/ADR-034-bybit-rsa-keys.md)). 워처의 자격 파일과 모양이 같고, 키 만들기와 Bybit 등록도 같다([워처 운영 문서](watcher.md) "자격 파일"). 두 형 가운데 정확히 하나:

```toml
# HMAC 키(System-generated)
api_key = "..."
api_secret = "..."
```

```toml
# RSA 키(Self-generated): openssl genpkey가 쓴 PKCS#8 PEM 글을 머리 줄과 꼬리 줄까지 그대로
api_key = "..."
rsa_private_key = """
(BEGIN PRIVATE KEY 줄부터 END PRIVATE KEY 줄까지)
"""
```

- 데이터 디렉터리 밖, OMS를 돌리는 사용자 소유, 그룹·기타 권한 없음(`install -m 600 /dev/null <경로>`), 심볼릭 링크 아님.
- 거부되면 노드는 `cannot start the Bybit gateway` 줄(오류 문장은 `error`)을 남기고 종료 코드 3(`bybit_gateway`, 7.4)으로 끝난다. 오류 문장은 파일 내용과 키의 바이트를 담지 않는다. 문장은 워처의 표와 같고(PKCS#1과 암호화된 PEM은 `openssl pkcs8 -topk8 -nocrypt -in old.pem -out new.pem`을 적는다) 비밀이 둘 다 없을 때만 `has neither api_secret nor rsa_private_key`로 다르다.
- RSA 키의 요청은 base64 서명을 싣고 `X-BAPI-SIGN-TYPE`을 보내지 않는다(측정한 RSA 요청의 모양). 키가 거절되면(10004 등) 7.5의 `auth_rejected`다.

## 3. 가상 거래소로 연습

```sh
mkdir -p data/sim
$T/release/simvenue data/sim/venue.sock &
$T/release/oms-node run --config config/sim.toml &
# 화면: http://127.0.0.1:8188/

C="$T/release/omsctl --socket data/sim/oms.sock --strategy demo"
$C new --key k1 --instrument BTC --side buy --qty 0.001 --px 70000 --ttl-ms 20000
$C open
$C cancel --key k1
A="$T/release/omsctl --socket data/sim/admin.sock"
$A status
$A halt --note "연습"
$A resume --note "연습 끝"
```

같은 키로 `new`를 다시 실행하면 `conflict`가 나온다. `omsctl`이 실행할 때마다 만료 시각을 새로 계산하기 때문이다.
전략 프로그램은 재시도할 때 **같은 만료 시각**을 보내야 `duplicate`를 받는다(C-IDEM).

## 4. systemd 단위

OMS 단위는 [`deploy/observability/systemd/oms-node.service`](../../deploy/observability/systemd/oms-node.service)이고, 워처·프로젝터·내보내기 단위와 설치 절차는 [observability.md](observability.md) §13이다(설치해 보지 않았다, ADR-036).

- `Restart=no`이고 `[Install]`이 없다: 충돌, 치명 종료(종료 코드 2·3, panic), 워처 킬 뒤에도, 재부팅 뒤에도 OMS는 사람이 띄운다. 다른 단위는 OMS를 끌어들이지 못한다(배치 검사 U20).
- 사용자 `hloms`, 저널 파일 시스템이 마운트돼 있지 않으면 시작이 실패한다. 자격 파일은 `EnvironmentFile=`이 아니라 OMS 설정이 가리키는 0600 파일이다.
- 재시작 뒤 거래를 자동으로 재개할지는 사용자 결정 U-06이다(단위가 다시 띄우지는 않는다).
- 로그는 표준 오류로 한 줄에 JSON 하나씩 나온다. journald가 보관한다. 로그에는 키가 나오지 않는다. 주문 내용은 나온다.

## 5. 시작과 종료

### 시작할 때 일어나는 일

1. 디렉터리 잠금 → 스냅샷과 저널로 상태 복원 → `Boot` 입력(전송 결과가 기록되지 않은 주문을 결과 불명으로 표시).
2. 설정 파일의 `[core]`를 입력으로 제출. 같은 버전에 다른 내용이면 종료 코드 3.
3. 소켓을 연다. 이때부터 요청을 받지만 신규 주문은 `link_down`으로 거절된다.
4. 거래소 스트림 접속 → 체결 동기화, 열린 주문, 포지션을 확인 → 차단 해제.

화면의 "운영 상태"에서 볼 것: 신규 주문 차단 사유가 비었는가, 결과 불명 주문과 불일치가 0인가.

### 정상 종료

`SIGTERM`. 큐에 있던 입력까지 기록하고 스냅샷을 쓴 뒤 끝난다. **거래소의 주문은 그대로 남는다.**
주문을 남기지 않으려면 먼저 중지하고 전체 취소한 뒤 열린 주문이 0이 된 것을 보고 종료한다.

```sh
$A halt --note "종료 준비"
$A cancel-all --note "종료 준비"
$A status | grep open_orders     # 0이 될 때까지
kill -TERM <pid>
```

## 6. 운영 명령

모두 운영 소켓(`admin.sock`)으로 하고 사유(`--note`)와 함께 저널에 남는다. 대시보드에서는 할 수 없다.
운영 소켓은 연결 자리(8)를 거래 소켓과 따로 가진다. 전략 연결이 가득 차도 운영 명령은 들어간다.

| 명령 | 언제 | 효과 | 주의 |
|---|---|---|---|
| `halt [--target-strategy S]` | 신규 주문을 멈추고 싶을 때 | 그 범위의 신규 주문 거절 | 열린 주문은 그대로다. 이미 내보낸 전송은 막지 못한다 |
| `resume [--target-strategy S]` | 중지를 풀 때 | 운영자 중지만 해제 | 다른 차단 사유는 남는다 |
| `cancel-all [--target-strategy S]` | 열린 주문을 없앨 때 | 미종결 주문마다 취소 요청. 이미 요청했지만 시도가 다한 주문에는 새로 시도한다 | 요청일 뿐이다. 결과는 `status`로 본다 |
| `ack --id N` | 불일치의 원인을 확인했을 때 | 그 불일치 하나를 닫는다 | 원인이 남아 있으면 다시 뜬다 |
| `resolve --order N` | 주문이 거래소에 없음을 직접 확인했을 때 | 주문을 종결하고 예약을 푼다 | "이 주문에 더 체결이 없다"는 운영자의 단언이다. 틀리면 `FillAfterClose` 불일치가 뜬다 |
| `adjust --instrument I --delta D [--target-strategy S]` | 장부의 포지션을 거래소에 맞출 때 | 포지션 증감 | **멱등이 아니다.** 두 번 실행하면 두 번 적용된다. 실행 전후에 `status`로 확인한다 |
| `clear-reduce` | 감소 전용 상태를 풀 때(7.8) | 현재 계좌 가치를 기준 금액으로 삼고 감소 전용 상태를 끝낸다 | 30초 안의 증거금 보고가 있어야 한다. 그 뒤 새 기준에서 5 %를 더 잃으면 다시 걸린다 |
| `rebase-day` | 입금이나 출금을 한 뒤(7.9) | 현재 계좌 가치를 기준 금액으로 삼는다 | 감소 전용 상태는 건드리지 않는다 |
| `clear-hold --reason kill\|watcher_mismatch\|venue_change` | 킬 래치(7.12), 워처 불일치(7.13), 거래소 변경(7.14)을 풀 때 | 그 보류 하나를 푼다. 킬이면 킬 마커도 지운다 | 강제 옵션은 없다. 킬은 이 프로세스의 게이트웨이가 킬에 답한 뒤에만, 그리고 OMS 장부에 미체결이 없을 때만(`kill_orders_open`이면 먼저 `cancel-all`), 거래소 변경은 변경 뒤의 포지션을 읽은 뒤에만 풀린다 |
| `snapshot` | 재시작 전에 복구 시간을 줄이고 싶을 때 | 다음 커밋에서 스냅샷 | |

## 7. 상황별 절차

### 7.1 결과 불명 주문이 남아 있다 (`in_doubt`)

뜻: 주문을 보냈는데 거래소가 받았는지 모른다. 그동안 계좌 전체의 신규 주문이 막힌다. OMS는 다시 보내지 않는다.

자동 해소: 조회에서 주문이 발견되면 정상 주문이 된다. 만료 시각 + 여유(기본 10초)가 지난 뒤의 조회에서 없으면 "놓이지 않음"으로 닫힌다.
만료 시각은 요청마다 최대 60초(기본) 앞이므로 보통 70초 안에 풀린다.

풀리지 않을 때: 링크가 내려가 있거나 조회가 계속 실패하는 것이다. 화면의 "지금 필요한 조회"와 로그를 본다.
거래소 화면에서 그 식별자(화면의 "거래소로 보낸 식별자")의 주문이 없음을 직접 확인했고, 만료 시각이 한참 지났다면 `resolve`로 닫을 수 있다.

### 7.2 불일치가 떴다 (`mismatches_unacked`)

신규 주문이 막힌다. 체결 기장과 취소는 계속된다. 종류별로 볼 것:

| 종류 | 뜻 | 조치 |
|---|---|---|
| `external_fill`, `foreign_order`, `foreign_position` | 이 OMS가 내지 않은 주문·체결·포지션이 계좌에 있다 | 누가 냈는지 확인(수동 거래, 다른 프로그램, 청산). 외부 체결은 이미 계좌 장부에 기장됐다. 외부 체결은 종목마다 한 건으로만 뜨므로 몇 건인지는 계좌 장부나 저널(`external_fill_booked`)에서 본다. 확인 뒤 `ack` |
| `orphan_order` | 이 인스턴스의 식별자를 가진 주문이 거래소에 있는데 장부에 없다 | **심각.** 저널이 과거로 돌아갔다는 뜻일 수 있다(§7.6). 거래를 재개하지 않는다 |
| `fill_after_close` | 닫았던 주문에 체결이 왔다 | 체결은 그 주문에 기장됐다. 포지션을 거래소와 대조하고 원인을 기록한 뒤 `ack` |
| `overfill`, `cum_exceeds_final`, `fill_field_conflict`, `status_conflict` | 거래소의 통보가 서로 또는 장부와 맞지 않는다. `status_conflict`에는 "접수됐던 주문을 거래소가 모른다고 한다"도 있다 | 거래소 화면과 대조. 주문이 실제로 없으면 `resolve`로 닫는다. 게이트웨이의 해석 오류일 수 있으므로 기록을 남긴다 |
| `missing_fills` | 거래소가 말한 체결 수량만큼 체결이 오지 않았다 | 체결 조회가 따라올 때까지 기다린다. 계속되면 거래소 화면과 대조 |
| `position_out_of_range` | 거래소의 포지션이 장부로 설명되지 않는다 | 차이의 원인을 찾는다(자금 조달은 포지션을 바꾸지 않는다. 청산·ADL·수동 거래는 바꾼다). 맞춰야 하면 `adjust` 후 `ack` |
| `cancel_stuck` | 취소를 5번 시도했는데 확인되지 않았다 | `cancel`이나 `cancel-all`을 다시 요청하면 새로 5번 시도한다. 그래도 안 되면 거래소 화면에서 직접 취소한다. 확인 처리(`ack`)하면 다시 뜨지 않는다 |
| `open_after_close` | 닫았던 주문이 거래소에 열려 있다 | 부재 판정이 틀렸거나(A-01, A-02) `resolve`가 틀렸다. 그 주문은 예약 없이 체결될 수 있다. 거래소 화면에서 취소하고 원인을 기록한 뒤 `ack` |
| `parked_fill_overflow` | 주인을 모르는 체결이 1024건을 넘었다 | 외부 거래가 대량으로 일어나고 있다. 계좌를 점검한다 |

### 7.3 치명 상태로 멈췄다 (종료 코드 2)

원인은 로그의 마지막 `fatal` 줄에 있다. 대부분 저널 쓰기 실패다(디스크 가득, I/O 오류).
`[alert]`가 있으면 텔레그램에 `치명 종료: <사유>. … 마지막 내구 seq <n>`이 먼저 온다(7.11). 노드는 그것을 보내려고 끝나기 전에 15초까지 기다린다.

1. **거래소의 주문은 살아 있다.** 급하면 거래소 화면에서 직접 취소한다. `schedule_cancel_secs`를 켜 두었다면 그 시간 뒤 거래소가 전부 취소한다(U-05).
2. `df`로 여유 공간을, `dmesg`로 I/O 오류를 본다.
3. 원인을 없앤 뒤 `oms-node verify --data-dir <dir>`로 저널을 검사한다.
4. 다시 띄운다. 마지막 내구 기록부터 복구하고, 멈춰 있던 동안의 체결은 대조가 거래소에서 가져온다.

저장 공간은 여유가 1 GiB(설정) 아래로 내려가면 신규 주문부터 막는다. 그 전에 조치하는 것이 낫다.

### 7.4 시작을 거부한다 (종료 코드 3)

| 로그의 사유 | 뜻 | 조치 |
|---|---|---|
| 잠금 | 같은 디렉터리에 다른 `oms-node`가 떠 있다 | 그 프로세스를 확인한다. 강제로 잠금을 지우지 않는다 |
| 저널 중간 손상, 순번 빈틈 | 파일이 손상됐다 | 파일을 건드리지 않았다. `verify`로 위치를 확인한다. §7.6 |
| 다른 인스턴스 | `INSTANCE`와 세그먼트의 ID가 다르다 | 디렉터리가 섞였다. 원래 파일을 찾는다 |
| 스키마 | 이 바이너리가 읽을 수 없는 버전의 기록 | 그 기록을 쓴 버전의 바이너리로 띄운다(§9) |
| 설정 | 같은 버전에 다른 내용, 또는 열린 주문이 있는 전략·종목을 뺐다 | `[core].version`을 올리거나 설정을 고친다 |

불완전한 꼬리(쓰다 만 마지막 기록)는 거부 사유가 아니다. `torn/`에 원본을 보존하고 자른 뒤 시작한다. 잘린 기록은 응답도 전송도 나간 적이 없는 것이다.

### 7.5 거래소 연결이 끊겼다

신규 주문이 `link_down`으로 거절된다. 게이트웨이가 다시 붙고 재확인이 끝나면 자동으로 풀린다.
재확인이 끝나지 않고 `resync`가 계속되면 화면의 "지금 필요한 조회"를 본다. 체결 동기화가 끝나지 않는 경우(체결이 1만 건 넘게 밀렸거나 해석할 수 없는 체결이 있다)는 로그에 "fills are not declared complete"가 남는다.
끊긴 동안 거래소의 주문은 살아 있고 체결될 수 있다. 다시 붙으면 체결 조회가 빠진 것을 가져온다.

Bybit 게이트웨이의 주문과 취소는 WS Trade 연결로 나간다. 그 연결이 내려가 있으면 REST로 나가므로 주문이 멈추지는 않는다. 로그의 "WS Trade is down; orders and cancels go over REST until it is back"(이유는 `error`)는 그 뜻이고, 다시 붙으면 "WS Trade is up"이 남는다. 내려갈 때 응답을 기다리던 주문은 결과 불명이 되고 대조가 조회로 푼다(다시 보내지 않는다). 계속 되풀이되면 네트워크와 거래소 공지를 본다. 응답 없는 늦은 답은 "a WS Trade answer for no pending request"로 남고 버려진다.

Bybit 게이트웨이의 참조 가격은 공개 스트림의 티커(mark price)에서 온다. 로그의 "the public ticker stream is down; no reference price reaches the core until it is back"(이유는 `error`)가 남으면 그 연결이 내려가 있다는 뜻이고, 다시 붙으면 "the public ticker stream is subscribed"가 남는다. 내려간 동안 가격은 새로 오지 않고 `ref_price_max_age_us`(기본 15초)가 지나면 가격 범위(`price_band_bps`)가 있는 종목의 새 주문이 `ref_price_unavailable`로 거절된다. 열린 주문과 취소는 그대로다. 다시 붙어 스냅숏이 오면 저절로 풀린다. 연결이 붙어 있는데도 거절되면 그 종목의 티커가 오지 않는 것이다(거래 중지 등): 거래소 공지를 본다.

Bybit 게이트웨이는 막힘이 있으면 다시 붙지 않는다. `status`의 `gateway.block`이 이유를 보여 준다([ADR-028 §7c](../adr/ADR-028-bybit-gateway.md)):

| `gateway.block` | 뜻 | 할 일 |
|---|---|---|
| `auth_rejected: …` | 키가 거부됐거나(조회, 주문 응답, 비공개 스트림 또는 WS Trade 인증. WS Trade면 `auth_rejected: WS Trade auth: …`), 요청이 키에 묶이지 않은 IP에서 나갔다. 프로세스는 더 요청하지 않는다 | 거래소에서 키와 묶인 IP를 확인하고 고친 뒤 다시 시작한다 |
| `ip_banned: …` | HTTP 403(REST, 스트림·WS Trade 업그레이드), REST 10018 또는 WS Trade 10403(IP 요청 한도). 10분 동안 요청과 접속을 멈춘다 | 기다린다. 끝나면 저절로 다시 붙는다. 무엇이 요청을 많이 보냈는지 로그를 본다 |
| `account: …` | 계좌·키·종목이 설정과 다르다(격리, 헤지 모드, 정산 코인 등) | 거래소에서 고친다. 시작 확인이 10초마다 다시 돌아 맞으면 붙는다 |
| `unchecked: …` | 시작 확인을 하지 못했다(응답 없음) | 네트워크와 거래소 상태를 본다. 저절로 다시 확인한다 |
| `clock: …` | 이 PC의 시계가 거래소보다 0.5초 넘게 앞설 수 있다(왕복 시간 포함). 새 주문을 보내지 않는다. 취소와 스트림은 그대로 | `timedatectl`·NTP를 고친다(계단 보정 대신 slew 권장). 막힌 동안 10초마다 다시 재서 맞으면 풀린다 |

### 7.6 저널을 잃었거나 과거 사본밖에 없다 (서버 유실)

먼저 **옛 호스트가 다시 살아나 주문을 낼 수 없음**을 사람이 확인한다. 소프트웨어는 이것을 막지 못한다(A-06).

그다음은 사본이 완전한지에 따라 다르다.

| 사본의 상태 | 절차 |
|---|---|
| 디스크를 그대로 옮겼다(마지막 내구 기록까지 있다) | 그 디렉터리로 띄운다. 평소의 재시작과 같다 |
| 과거 시점의 사본이다, 또는 완전한지 모른다 | **그 사본으로 거래를 재개하지 않는다.** 아래 "새 인스턴스로 시작" |

과거 사본으로 재개하면 안 되는 이유: 사본 이후에 낸 주문의 번호를 새 주문이 다시 쓰게 된다. 거래소로 보내는 식별자가 `인스턴스 ID + 주문 번호`이므로
옛 주문과 새 주문의 식별자가 같아지고, 조회 결과를 새 주문의 것으로 잘못 읽을 수 있다. 사본 이후의 주문이 `orphan_order`로 보이는 것이 그 신호다.

새 인스턴스로 시작:

1. 거래소 화면에서 열린 주문을 모두 취소하고 포지션을 적어 둔다.
2. **빈** 데이터 디렉터리로 `oms-node`를 띄운다. 새 인스턴스 ID가 만들어진다. 옛 사본은 기록용으로 따로 보관한다.
3. 포지션 대조가 `position_out_of_range` 또는 `foreign_position`을 띄운다. 거래소 포지션만큼 `adjust`(전략을 지정하지 않으면 미귀속분)하고 `ack`한다.
4. 옛 인스턴스의 주문이 남아 있으면 `foreign_order`로 보인다. 취소를 확인하고 `ack`한다.

이 절차로 잃는 것: 전략별 포지션 귀속(미귀속분으로 합쳐진다), 사본 이후의 주문·체결 기록(거래소의 체결 내역에는 남아 있다).

### 7.7 화면이 갱신되지 않는다

화면 상단의 "브라우저와의 차이"가 커지거나 "노드 응답 없음"이 뜬다. 프로세스가 죽었거나 멈춘 것이다. 알림 기능은 없다(U-11).
`systemctl status`, 로그의 마지막 줄, 종료 코드를 본다.

### 7.8 감소 전용 상태가 걸렸다 (`holds`에 `reducing`)

증거금 정책(`policy.margin`)이 있을 때만 생긴다. `status`의 `core.margin.reducing`이 규칙(`daily_loss`, `mm_buffer`, 또는 Bybit의 `account_mm`), 본 숫자(`detail`), 걸린 시각을 보여 준다. `account_mm`은 교차 계좌의 유지증거금이 증거금 잔고의 `account_mm_rate_max_bps`(Bybit 5000 = 50 %)에 닿았다는 뜻이고, 마지막 보고의 두 숫자는 `core.margin.account_mm`(`maintenance`, `margin_balance`)에 있다. `core.margin.day_base`가 기준 금액이다.
이 상태에서 OMS는 일반 주문을 거절하고, 걸려 있던 일반 주문을 취소했고, 감소 전용(reduce-only) 주문은 받는다. 포지션은 닫지 않는다. 재시작과 새 날은 이 상태를 풀지 않는다.

1. `status`로 규칙과 숫자를 본다. 거래소 화면에서 계좌 가치와 포지션을 직접 확인한다.
2. `core.open_orders`와 거래소의 열린 주문을 본다. 감소 전용 주문만 남아 있어야 한다. 취소가 막혔으면 불일치(`CancelStuck`)가 떠 있다(7.2).
3. 손실의 원인을 정한다(전략 오류, 시장, 출금). 포지션을 줄일지는 사람이 정한다. 줄이려면 전략이 reduce-only 주문을 내거나 거래소 화면에서 닫는다.
4. 다시 거래해도 된다고 판단했으면 푼다. 사유는 저널에 남는다.
   ```
   omsctl --socket <data_dir>/admin.sock clear-reduce --note "원인과 조치"
   ```
5. `status`에서 `reducing`이 null이고 `day_base`가 현재 계좌 가치(`set_by: operator`)인지 확인한다.

거절되면 응답의 첫 단어가 이유다.

| 응답 | 뜻 | 할 일 |
|---|---|---|
| `not_reducing` | 걸려 있지 않다 | 없음. 같은 명령을 두 번 보낸 경우도 이것이다 |
| `margin_unavailable` | 30초 안의 증거금 보고가 없다 | `core.margin.age_us`와 `last_rejection`, 링크 상태를 본다. 보고가 다시 오면 다시 실행한다 |
| `mm_buffer_active` | 마지막 증거금 보고에서 어느 격리 포지션의 증거금이 (유지증거금률 + 여유) × 포지션 가치보다 작다. 청산이 가깝다 | 거래소 화면에서 그 포지션을 본다. 포지션을 줄이거나 격리 증거금을 더한 뒤, 다음 보고(5초)가 온 다음에 다시 실행한다. 강제로 푸는 길은 없다. 숫자가 틀려 보이면(거래소 화면의 증거금은 충분한데 계속 걸린다) 풀 수 없으니 개발자에게 알린다 |
| `account_mm_active` | 마지막 증거금 보고에서 교차 계좌의 유지증거금이 증거금 잔고의 `account_mm_rate_max_bps` 이상이다(Bybit). 청산이 가깝다 | 거래소 화면에서 계좌의 유지증거금률(Account MM rate)을 본다. 포지션을 줄이거나 담보를 더한 뒤, 다음 보고(5초)가 온 다음에 다시 실행한다. 강제로 푸는 길은 없다. 숫자가 틀려 보이면 한도를 바꾼 설정(`version`을 올림)으로만 풀린다. 개발자에게 알린다 |
| `note_required` | 사유가 비었다 | `--note`에 사유를 쓴다 |

주의: 해제는 "오늘의 손실 한도를 다시 준다". 해제한 시점의 계좌 가치에서 5 %를 더 잃어야 다시 걸린다. 강제로 푸는 옵션은 없다. 전략 소켓에서는 이 명령을 쓸 수 없다.

### 7.9 입금이나 출금을 했다

OMS는 입출금을 모른다. 계좌 가치만 본다. 노드가 도는 동안에는 입출금을 하지 않는 것이 원칙이다(ADR-018 O5).

- 출금하면 그만큼이 손실로 읽힌다. 기준 금액의 5 %를 넘으면 5초 안에 감소 전용 상태가 걸린다(안전한 쪽). 걸렸으면 7.8의 절차로 푼다. 해제가 기준도 다시 잡는다.
- 입금하면 그만큼의 손실이 가려진다. 예: 기준 100, 입금 10, 그 뒤 손실 14면 계좌 가치가 96이라 걸리지 않는다. 그래서 입금 직후에 기준을 다시 잡는다.
  ```
  omsctl --socket <data_dir>/admin.sock rebase-day --note "입금 10"
  ```
- 5 %보다 작은 출금 뒤에도 같은 명령으로 기준을 다시 잡는다. 하지 않으면 그날은 손실 한도가 그만큼 좁아진다.
- 명령 뒤에 `status`의 `core.margin.day_base`가 현재 계좌 가치인지 확인한다. 기준 재설정은 감소 전용 상태를 풀지 않는다.

### 7.10 증거금 정책이 있는데 일반 주문이 거절된다

증거금 정책이 있으면 일반 주문(reduce-only 표시가 없는 주문)은 아래 사유로도 거절된다. 감소 전용 주문은 이 사유들에 걸리지 않는다. `status`의 `core.margin`과 `core.holds`를 본다.

| 거절 사유 | 뜻 | 볼 것과 할 일 |
|---|---|---|
| `AccountLeverage` | 이 주문을 받으면 계좌 노출이 계좌 가치의 2배(정책의 `max_account_leverage`)를 넘고 지금보다 커진다. 계좌 가치가 0 이하일 때도 이 사유다 | `core.margin.account_exposure`와 `account_value`. 걸려 있는 주문도 노출에 들어간다. 주문을 줄이거나 걸린 주문을 취소한다. 상태는 걸리지 않는다 |
| `MarginUnavailable` | 30초 안의 증거금 보고가 없다. 또는 포지션이나 주문이 있는 종목에 15초 안의 참조 가격이 없다 | `core.margin.age_us`, `last_rejection`(보고를 읽지 못한 이유), 링크 상태. 60초 넘게 보고가 없으면 걸려 있는 일반 주문이 취소된다(보고 끊김 취소). 새 보고가 오면 저절로 풀린다. 설정에 없는 종목에 포지션이 있으면 그 포지션이 0이 될 때까지 계속 이 사유다(거래소에서 닫는다) |
| `Hold(VenueSetting(종목))` | 그 종목의 거래소 레버리지나 증거금 방식을 이번 연결에서 받지 못했거나 정책과 다르다 | `core.margin.venue_settings`. 거래소 화면에서 격리·정책의 레버리지로 맞춘다. OMS는 바꾸지 않는다. 바꾼 것은 다음 조회(최대 60초)에 보인다 |
| `Hold(AccountMode)` | 계좌 모드를 이번 연결에서 받지 못했거나, 통합 계좌·포트폴리오 증거금이거나, 정책의 `account_modes_allowed`에 없다 | `core.margin.account_mode`. 허용 목록의 기본값은 비어 있다. 거래소의 표준 모드가 어떤 문자열인지는 확인하지 않았다(T10) |
| `Hold(Reducing)` | 감소 전용 상태다 | 7.8 |

이 절의 동작은 코어 시험과 가상 거래소에서 확인했다. 실제 거래소에서 이 사유들을 본 적은 없다.

### 7.11 알림(`[OMS P1]`)

노드는 거래 안전 조건을 내구화된 상태에서 계산해 텔레그램으로 직접 보낸다(C-ALERT, [ADR-029](../adr/ADR-029-oms-direct-alerts.md)). md-collector와 같은 봇을 쓰고 접두어로 구분한다(`[MD P1]`, `[OMS P1]`). mainnet은 `[alert]` 없이 시작하지 않는다.

토큰 파일(md-collector와 같은 형식):

```sh
install -m 0600 /dev/null /etc/oms/telegram.json     # 저장소 밖, 소유자만
# 내용: {"token": "<숫자>:<문자>", "chat_id": "<대화 ID>"}
```

```toml
[alert]
telegram = "/etc/oms/telegram.json"   # 절대 경로
# grace_ms = 30000, eval_ms = 1000, repeat_ms = 300000, exit_flush_ms = 15000, second_channel = "none"
```

파일이 0600이 아니거나, 심볼릭 링크이거나, git 저장소 안에 있거나, 형식이 틀리면 설정 오류로 시작하지 않는다(종료 코드 2, 문장에 토큰은 없다).
시험 메시지: 노드를 띄우면 설정이 활성화된 뒤 `OMS 시작 (boot n, config v). 활성 조건 k개(유예 중 m개)`가 온다. k는 이 메시지와 함께 시작 알림이 나간 조건의 수이고, m은 아직 유예(30초) 안이라 보내지 않은 조건의 수다. 거래소에 연결되기 전에 평가되면 연결 끊김이 `유예 중`으로 세어지고, 30초 안에 연결되면 그 조건은 아무것도 보내지 않는다. 오지 않으면 `status`의 `alerts`(`failures`, `last_errors`, `failing_since_ms`)를 본다. 정상 종료하면 `OMS 정상 종료. 걸린 주문 n개는 거래소에 남음`이 온다.

같은 조건은 시작 1번, 5분마다 1번(그때의 문장), 풀리면 `해소: …` 1번 온다. 연결 끊김처럼 다시 붙을 때마다 지나가는 상태는 30초 넘게 계속될 때만 온다.

| 키(문장) | 뜻 | 푸는 것 |
|---|---|---|
| `hold:reducing`(감소 전용) | 증거금 규칙이 걸었다(규칙, 내용, 시각) | 7.8의 `clear-reduce` |
| `mismatch:<종류>`(확인하지 않은 불일치) | 그 종류의 불일치 개수와 최근 3개 | 원인을 확인하고 `ack --id N`(7.2) |
| `hold:operator_account`, `halt_strategy:<s>`(운영자가 중지함) | 누군가 `halt`를 했다(메모) | `resume [--target-strategy S]` |
| `hold:storage_low`(저장 공간 부족, 30초 넘게) | 저널 디스크의 여유가 설정 아래다. 문턱 근처에서 들락거리면 30초 넘게 계속될 때만 온다 | 공간을 늘린다. 다음 측정(5초)에 풀린다 |
| `hold:link_down`, `hold:resync`(30초 넘게) | 거래소 연결이 끊겼거나 다시 확인하지 못했다 | 7.5. 다시 붙으면 저절로 풀린다 |
| `gateway_block`(30초 넘게) | Bybit 게이트웨이가 막혔다(`auth_rejected`, `ip_banned`, `account`, `unchecked`, `clock`) | 7.5의 표 |
| `hold:in_doubt`(30초 넘게) | 결과를 모르는 주문이 있다 | 7.1 |
| `margin_stale`(30초 넘게) | 신선한 증거금 보고가 없다. 60초가 지나면 걸린 일반 주문이 취소된다 | 7.10의 `MarginUnavailable` |
| `hold:venue_setting:<i>`, `hold:account_mode`(30초 넘게) | 거래소 설정이나 계좌 모드를 모르거나 정책과 다르다 | 7.10 |
| `hold:no_config`(30초 넘게) | 설정이 활성화되지 않았다 | 시작 로그를 본다 |
| `hold:kill_latched`(킬 래치) | 킬 스위치가 걸렸다(출처 `stream`·`watcher`·`marker`, 시각, 메모). 모든 주문을 거절한다(감소 전용 포함). `확인:` 줄이 킬 루프가 0건을 확인했는지, 또는 게이트웨이에 일괄 취소가 없어 운영자가 취소해야 하는지 말한다 | 7.12 |
| `hold:watcher_mismatch`(워처가 불일치를 확정함) | 워처의 검사(W1~W4)가 불일치를 확정했다. 일반 주문 거절. 감소 전용은 증거금 정책이 있을 때만 받는다(문장이 어느 쪽인지 쓴다) | 7.13 |
| `hold:venue_change`(거래소가 포지션을 바꿈) | ADL·인수·인도·정산 또는 분류 못 한 실행. 일반 주문 거절. 감소 전용은 증거금 정책이 있을 때만 받는다. 분류 못 한 실행 하나는 이 알림과 `mismatch:unclassified_execution` 두 개를 낸다. 둘은 따로 풀린다(`clear-hold`와 `ack`) | 7.14 |
| `strategy_link:<s>`(전략 연결 상실) | 적힌 전략의 마지막 거래 연결이 닫혔거나 `liveness_ms` 동안 요청이 없었다. 그 전략의 걸린 일반 주문 취소를 요청했고, 그 전략의 일반 주문을 거절한다(감소 전용은 증거금 정책이 있을 때만 받는다). 다른 전략은 그대로다 | 7.15 |
| `hold:unmonitored`(모니터링 상실) | 게이트가 있는데 워처의 하트비트가 30초 넘게 없거나 W1·W2가 10초 넘게 성공하지 못했다. 일반 주문을 거절하고(감소 전용은 증거금 정책이 있을 때만 받는다), 60초 더 가면 걸린 일반 주문 취소를 요청한다. 모든 시작이 워처의 첫 하트비트까지 이 상태를 지난다(유예 30초) | 7.16 |
| `치명 종료: …` | 노드가 멈췄다(종료 코드 2). 시작 중 첫 기록의 flush가 실패해도 이것이다. 걸린 주문은 거래소에 남아 있다 | 7.3 |
| `OMS 시작 실패(exit 3): <종류>` | 설정이 활성화된 뒤 시작을 끝내지 못했다(`hyperliquid_gateway`, `bybit_gateway`, `trading_socket`, `admin_socket`, `monitor_socket`, `signal_handler`). 노드는 멈췄다 | 노드 로그의 `cannot …` 줄(오류 문장과 경로는 로그에만 있다). 설정이 활성화되기 전의 종료 코드 3은 알림이 없다 |
| `전송 실패로 알림 N개를 버림` | 텔레그램이 오래 실패해 보관함(50개)이 넘쳤다 | 그동안의 상태는 `status`와 대시보드로 본다 |

알림을 받지 못하는 경우: SIGKILL·panic으로 끝났을 때, 종료 전 15초 안에 텔레그램이 받지 못했을 때, 보낼 것이 없을 때의 텔레그램 장애. 이때는 관측 경로(`oms_up`)와 감시자가 알린다.

### 7.12 킬 래치가 걸렸다 (`holds`에 `kill_latched`)

C-HOLD. `status`의 `core.kill`이 출처(`source`: `watcher` = 워처의 `watcher_kill`, `marker` = 시작할 때 킬 마커가 있었음, `stream` = Bybit 개인 스트림을 `kill_after_ms`(기본 10초) 잃음, C-KILL), 걸린 시각(`since`), 메모(`note`), 확인(`confirmed_at`, `outcome`)을 보여 준다. `node.kill_marker`가 마커 경로와 시작할 때 있었는지를 보여 준다.
이 상태에서 OMS는 **모든 주문을 거절한다(감소 전용 포함)**. 포지션은 닫지 않는다. 재시작은 이 상태를 풀지 않는다(U-06): 저널의 래치나 마커가 있으면 다시 시작한 노드도 걸린 채로 시작하고, 게이트웨이가 킬 루프를 다시 돌린다.

1. 걸린 이유를 본다(`core.kill.note`, 워처 로그). 마커 출처면 마커 파일의 내용이 메모에 있다(`marker: …`). `marker unreadable: <종류>`면 노드가 마커 경로를 볼 수 없었다는 뜻이다(권한 등). 경로를 고친다.
2. 게이트웨이의 답을 기다린다.
   - Bybit: 킬 루프가 돈다(`status.gateway.kill_loop`). 루프가 도는 동안 새 주문은 거래소로 나가지 않는다. `킬 루프` P1의 뜻은 아래 표. `킬 루프 정리 완료`가 오면(`core.kill.outcome: "confirmed"`) 거래소에서 미체결 0건을 확인하고 30초 지켜본 것이다. 그 전에는 해제가 `kill_unconfirmed`로 거절된다. 확인 뒤 스트림이 다시 끊기거나 워처가 다시 킬하면 확인이 지워지고(`kill_rearmed`) 새 루프가 돈다: 새 `정리 완료`를 기다린다.
   - 가상 거래소, Hyperliquid: 일괄 취소가 없어 곧바로 `unsupported`로 답한다(`outcome: "unsupported"`, 로그의 `kill latched, but this gateway has no cancel-all`). 거래소에서 0건을 확인한 것이 아니다. 걸린 주문은 운영자가 취소한다: `omsctl … cancel-all --note "kill"`(래치 아래에서도 받는다. 취소는 새 주문이 아니다), 그리고 OMS가 모르는 주문은 거래소 화면에서.
   - 어느 쪽이든 `status`의 `core.open_orders`가 0이 될 때까지 해제는 `kill_orders_open`으로 거절된다.

   | P1 | 뜻 | 할 일 |
   |---|---|---|
   | `킬 루프 시작(<출처>: <메모>)` | 래치가 디스크에 있고 루프가 시작했다. 포지션은 닫지 않는다 | 출처를 본다(1번). 포지션 묶음으로 위험을 본다 |
   | `킬 루프 N회차: 남은 미체결 n건` | 일괄 취소 뒤에도 주문이 남았거나 수가 바뀌었다. 1초마다 되풀이한다 | 기다린다. 수가 줄지 않으면 거래소 화면을 본다 |
   | `… 남은 미체결 모름: <이유>` (60초마다) | 일괄 취소나 셈이 실패한다(5xx, `auth_rejected`, `ip_banned`, `unchecked` 등). 확인하지 못한다 | 이유를 본다. 키 거부·IP 차단이 계속되면 강제로 푸는 길은 없다: 키를 폐기(X8)하고 새 키로 다시 시작하면 새 루프가 돈다. 그동안 거래소 화면에서 주문을 직접 취소한다 |
   | `킬 루프 정리 완료` | 0건 확인 뒤 30초 동안 늦은 접수가 없었다 | 3번으로 |

   - 출처가 `watcher`이거나 워처가 쓴 마커(`marker: watcher <이름> W5 …`)면 워처 자신의 루프도 같은 계정에서 돌고 있다(7.18). 워처의 P1은 `[WATCH P1] … 워처 킬 …`이고 OMS의 `킬 루프`와 따로 온다:

   | 워처의 P1 | 뜻 | 할 일 |
   |---|---|---|
   | `워처 킬 시작(W5: OMS /api/status 무응답 …. 킬 마커: …. OMS 정지: …)` | 워처가 OMS의 침묵을 보고 마커를 쓰고 OMS를 래치하거나 멈춘 뒤 자기 키로 일괄 취소를 시작했다. 포지션은 닫지 않는다 | 7.18 |
   | `워처 킬 N회차: 남은 미체결 n건(또는 모름: …)` | 60초마다(건수가 바뀌어도 바로 오지 않음). OMS의 `킬 루프`가 건수 변화를 최대 60초 먼저 보일 수 있다 | 기다린다 |
   | `워처 킬 정리 완료: …` | 워처의 키로 0건을 확인하고 30초 지켜봤다. 마지막 줄이 OMS의 상태다(래치됨, 멈춤과 마커, 확인 못 함, 정지 미확인) | 7.18의 4번 |
   | `늦은 정지: OMS가 다시 답해 watcher_kill로 래치함(…)` / `늦은 정지 실패: …` | 래치 없이 돌아온 OMS에 워처가 정지를 보냈다 / 보내지 못했다 | 7.18 |
   | `{워처 종료 \| 워처 시작 거부: …}로 워처 킬 중단(…)` | 워처가 멈추거나 시작을 거부해 자기 루프를 그만뒀다. OMS의 래치와 마커는 그대로다 | 7.18의 5번 |

   모든 킬 루프 P1 끝에 거래소 `/v5/position/list`의 포지션 묶음(심볼, 수량, 미실현손익, 마크, 마크에서 강제청산가까지 %, 교차 증거금이면 거래소 추정)이 붙는다. 일괄 취소가 나갈 수 없는 동안(IP 차단 정지, 키 거부, 시작 확인 전 `unchecked`, 계정 불일치)은 포지션을 읽지 않고 `포지션 조회 안 함(<막힘>). 수량·손익 모름`이라고 쓴다: 포지션은 거래소 화면에서 본다. 포지션 조회가 느려 알림이 늦어도 일괄 취소는 기다리지 않는다. 그동안 생긴 알림은 가장 새 것 하나만 나간다(중간 회차의 수는 건너뛸 수 있다): 회차 수와 남은 수는 `omsctl status`의 `gateway.kill_loop`에서 본다. `[alert]`가 없으면 P1 대신 노드 로그에 `kill loop notice (no [alert]: not sent)` 줄이 남는다.
3. 거래소 화면에서 열린 주문이 없는지, 포지션이 어떤지 직접 확인한다. 워처의 `watcher_kill_active`가 1이거나 워처의 마지막 킬 알림이 `워처 킬 정리 완료`도 `… 워처 킬 중단`도 아니면 아직 풀지 않는다(7.18의 4번). 포지션을 줄일지는 사람이 정한다(OMS는 래치 동안 감소 전용 주문도 받지 않는다. 거래소 화면에서 닫는다).
4. 푼다. 사유는 저널에 남는다.
   ```
   omsctl --socket <data_dir>/admin.sock clear-hold --reason kill --note "원인과 조치"
   ```
5. 응답이 `cleared; kill marker removed`인지 본다. `cleared; kill marker NOT removed (<종류>): the next start latches again`이면 해제는 됐지만 마커 파일이 남았다. 다음 시작이 다시 래치를 건다. 파일을 직접 지운다(워처가 다시 쓰지 않았는지 먼저 확인: 워처는 W5가 킬할 때만 마커를 쓰고, 그때 로그에 `error: W5: /api/status gave no good answer for <ms> ms: the watcher kills (the marker <경로>, …)` 줄이 남고 시작 알림이 `킬 마커: 씀`이라고 말한다. 그 줄이 해제 뒤에 있으면 그 마커는 새 킬의 것이다: 7.18).

| 응답 | 뜻 | 할 일 |
|---|---|---|
| `not_held` | 걸려 있지 않다 | 없음 |
| `note_required` | 사유가 비었다 | `--note`에 사유를 쓴다 |
| `kill_unconfirmed` | 이 프로세스의 게이트웨이가 아직 킬에 답하지 않았다 | 기다린다. 루프가 계속 실패하면(키 거부, IP 차단, 거래소 장애) 강제로 푸는 길은 없다: 키를 폐기(X8)하고 새 키로 다시 시작하면 새 루프가 돈다 |
| `kill_orders_open: N open orders in the book; run omsctl cancel-all first` | OMS 장부에 미체결 주문이 N개 남아 있다(게이트웨이의 답과 상관없다) | `cancel-all`을 하고, 거래소가 취소를 확인해 `core.open_orders`가 0이 되면 다시 실행한다. 취소가 거래소에 닿지 않으면(연결 끊김) 닿을 때까지 남는다 |

### 7.13 워처가 불일치를 확정했다 (`holds`에 `watcher_mismatch`)

워처(C-WATCH)의 검사 가운데 하나가 OMS와 거래소의 차이를 두 라운드 연속으로 보았고, 그 저널 항목이 `min_age_ms`(기본 2초)보다 오래됐다. `core.watcher_mismatch`가 검사 이름(`source`), 시각, 내용(`note` = 워처의 `detail`)을 보여 준다. 일반 주문을 거절하고 감소 전용 주문은 받는다(증거금 정책이 있을 때). 저절로 풀리지 않는다.

`detail`의 모양: `<검사> <종류> <대상>: venue <거래소 쪽>, journal <저널 쪽>; seen 2 rounds, <나이>`(여럿이면 끝에 `; and N more`). `<나이>`는 `journal entry <a> s old`(저널 항목, W2에서는 저널과 거래소의 변경 시각 중 새 것; 거래소 쪽은 `updatedTime`, 처음 본 시각, U19a9부터 W4가 맞춘 `execTime`), `first seen <a> s ago`(저널이 모르는 주문), `change time unknown`(어느 시각도 모름). W4(U19a8)는 `seen 2 sweeps, execTime <a> s old`(거래소 체결 시각; 없으면 `execTime unknown`)이고, `unknown_exec_type`은 `seen 1 sweep`이다(바로 확정). W4의 대상은 체결 키 `<symbol>:<execId>`다.

| 검사·종류 | 뜻 | 볼 것 |
|---|---|---|
| W1 `foreign` | 거래소에 열린 주문이 OMS의 것이 아니다(대상 = 거래소 `orderId`): 다른 형식이나 다른 인스턴스의 `orderLinkId`, 또는 저널이 모르는(지운) 주문 번호 | 거래소 화면에서 그 주문. 사람이 낸 주문이면 취소한다. OMS의 것으로 보이면 `status`의 열린 주문과 저널 |
| W1 `closed_in_journal` | 저널에서 닫힌 주문이 거래소에 아직 열려 있다 | 거래소에서 그 주문을 취소한다(`omsctl cancel`은 닫힌 주문에 듣지 않는다). 체결이 오면 W2가 본다 |
| W1 `open_after_terminal` | 저널은 거래소가 끝냈다고 적었는데(`venue_terminal`) 거래소가 아직 연다 | 거래소 화면, 게이트웨이 로그 |
| W1 `details` | 둘 다 열렸지만 종목이나 방향이 다르다 | 거래소 화면과 `get_order` |
| W1 `missing_at_venue` | 저널에 열려 있고 확인된 주문이 거래소에 없다 | 체결됐거나 거래소가 취소했는지 거래소 화면에서, 그다음 `resolve`(7.1, §6) |
| W2 `position` | 기호의 거래소 포지션 ≠ 저널의 합(체결 + 거래소 변경 + 조정) | 거래소 포지션과 `status`의 장부. 수동 거래나 OMS가 못 본 체결이면 `adjust` |
| W4 `missing_fill` | 거래소의 `Trade` 체결이 저널의 체결(OMS 주문의 것이든 외부 체결이든)에 없다 | 거래소 체결 내역에서 그 `execId`. 수동 거래면 W2도 포지션을 본다(`adjust`). OMS 주문의 것이면 게이트웨이 로그(체결 동기화)와 `mismatches_unacked` |
| W4 `fill_details` | 같은 키의 체결이 있지만 수량이나 방향이 다르다 | 거래소 체결 내역과 저널의 그 체결. OMS의 체결 해석 결함일 수 있다: 거래를 멈추고 조사한다 |
| W4 `missing_venue_change` | 거래소의 `AdlTrade`·`BustTrade`·`Delivery`·`Settle`이 저널의 거래소 변경에 없다 | 거래소 화면의 ADL·청산·인도·정산 기록, 7.14가 함께 걸렸는지, W2의 포지션 |
| W4 `venue_change_details` | 같은 키의 거래소 변경이 있지만 종류나 부호 있는 수량이 다르다 | 거래소 화면과 저널의 그 변경 |
| W4 `unknown_exec_type` | 워처가 모르는 `execType`(바로 확정) | 거래소 체결 내역에서 그 행. OMS도 그것을 분류하지 못했으면 7.14가 함께 걸려 있다 |
| W3 `account_mm_rate` | 통합 계좌의 유지증거금 ÷ 증거금 잔고가 `[w3] account_mm_rate_max_bps`(기본 5000 bp = 50 %) 이상이다(두 라운드 연속, OMS와 같은 공식). `detail`에 `totalMaintenanceMargin`, `totalMarginBalance`가 있다 | 거래소 화면의 계좌 위험, OMS의 `core.margin`. 보통 OMS도 같은 규칙(`account_mm`)으로 감소 전용 상태다: 아래 W3 순서 |
| W3 `daily_loss` | 저널의 오늘 UTC 일 기준값보다 `totalEquity`가 `[w3] daily_loss_bps`(기본 500 bp) 넘게 낮다(두 라운드 연속) | OMS의 `core.margin.day_base`와 감소 전용 상태(`daily_loss`). 입금·출금도 손실로 읽힌다(C-MARGIN) |
| W4 `unreadable_row` | 체결 행에 그 형이 필요로 하는 필드가 없다(값 = 없는 필드 이름; 키가 없으면 대상은 행의 JSON 앞 200바이트). `execTime`이 훑기의 끝보다 뒤인 행도 이것이다(값 `execTime`, `what`에 `execTime <t> after the sweep's end <end>`: 거래소가 `endTime`을 어겼거나 시계가 크게 다르다) | 거래소 체결 내역. 거래소의 응답 모양이 바뀌었을 수 있다: 워처 로그와 원본 응답 |

1. 워처 로그의 `error: confirmed difference: …` 줄을 **모두** 읽는다. OMS는 첫 메모만 남긴다(C-HOLD): 보류가 이미 걸린 동안 확정된 차이는 그 메모에 없다. 워처는 그것을 빚으로 들고 있다가 `clear-hold` 뒤 한 번 보내고, 그러면 보류가 그 메모와 함께 한 번 다시 걸린다. 로그를 미리 읽어 두면 돌아오는 보류가 예상된다.
2. 위 표대로 거래소 화면과 `status`(포지션, 열린 주문, 불일치)를 비교하고 원인을 고친다(포지션이면 `adjust`, 주문이면 `resolve`·`cancel-all`).
3. `omsctl … clear-hold --reason watcher_mismatch --note "원인과 조치"`. W1~W3의 차이가 남아 있으면 워처가 `mismatch_resend_ms`(기본 10초) 안에 다시 보내 보류가 돌아온다. 돌아오면 1로.

W3(지갑과 증거금, C-WATCH 9부)은 OMS의 한도와 같은 공식이라, OMS가 일 손실이나 계좌 유지증거금률로 감소 전용 상태에 들어가면 W3도 보통 이 보류를 건다. 위반이 남아 있는 동안 `clear-hold`하면 워처가 10초 안에 다시 보내 보류가 돌아온다. 순서: (a) 먼저 OMS 자신의 감소 상태를 정리한다(7.8: 포지션을 줄이고 `clear-reduce`로, 입출금이 원인이면 7.9의 `rebase-day`로 오늘의 새 기준을 기록한다). (b) 워처 로그에서 W3 확정 줄이 더 오지 않고 `watcher_mismatch_active{check="W3"}`가 0인지 본다(일 손실은 저널에 오늘의 새 `day_base_set`이 오거나 UTC 날짜가 바뀌면 끝난다; 계좌 유지증거금률은 거래소의 비율이 한도 아래로 내려와야 끝난다). (c) 그다음 `clear-hold --reason watcher_mismatch`. 순서를 바꾸면 보류가 곧 돌아온다.

W4(체결)는 다르다(C-WATCH 8부). W4 차이는 지난 체결 하나에 대한 사실이라, 보류 메모(`"held"` 답)가 이름한 W4 차이는 `clear-hold` 뒤 다시 보내지 않는다(거래소 창에 그 행이 남아 있어도). 이미 걸린 보류 아래에서 확정된 W4 차이는 그 메모에 없고, 해제 뒤 한 번 보내져 보류가 그 메모와 함께 한 번 돌아온다. 그것을 푼 뒤에는 다시 오지 않는다. 그래서 해제 전에 워처 로그의 확정 줄을 모두 읽는 것(1)이 W4의 전체 목록이다. 워처를 재시작하면 W4가 거둔 것을 잊으므로, 600초(`w4_initial_lookback_ms`) 안의 같은 행이 두 훑기 뒤 보류를 한 번 더 건다. 포지션이 계속 다르면 W2가 다시 보낸다.

### 7.14 거래소가 포지션을 바꿨다 (`holds`에 `venue_change`)

ADL, 청산 포지션 인수, 인도, 정산, 또는 게이트웨이가 분류하지 못한 실행이 왔다. `core.venue_change`가 처음의 내용을, `mismatches_unacked`가 `venue_position_change`·`unclassified_execution` 항목을 보여 준다. 일반 주문을 거절한다. U18b 전에 쓴 저널에 이런 기록이 있으면 시작하자마자 이 상태다. 그런 저널에 U18b 전의 스냅샷이 있어도 같다: 이 빌드는 상태 스키마 1의 스냅샷을 건너뛰고 저널을 처음부터 재생한다(§9). 그런 저널의 `verify --re-decide`는 변경 뒤, `ack` 다음에 옛 빌드가 받은 첫 주문(증거금 정책이 있으면 첫 일반 주문)에서 "this build decides the input differently"를 보고한다. 이 빌드는 그 주문을 `Hold(VenueChange)`로 거절하기 때문이다. 판단이 바뀐 것이지 저널이 상한 것이 아니다.

1. 거래소 화면에서 그 실행과 지금의 포지션을 확인한다.
2. 불일치 항목마다 `ack --id N`(7.2).
3. `omsctl … clear-hold --reason venue_change --note "…"`. `venue_change_pending`이면 변경 뒤의 거래소 포지션을 아직 읽지 않았다는 뜻이다(`core.venue_change_wait`). 다음 대조(몇 초)를 기다려 다시 실행한다. 두 단계가 모두 필요하다(`ack`만으로는 풀리지 않는다).

### 7.15 전략의 연결을 잃었다 (`strategies.<s>.holds`에 `strategy_link`)

C-LIVE. `status`의 `strategies.<s>.link`가 이유(`why`: `last trading connection closed` 또는 `no request for {ms} ms`), 시각(`since`), 지금 열린 연결 수(`connections`), 마지막 요청 뒤의 시간(`last_request_age_ms`)을 보여 준다. OMS는 그 전략의 걸린 일반 주문 취소를 이미 요청했고(취소는 요청이지 확인이 아니다: `open_orders`로 본다), 그 전략의 감소 전용 주문과 다른 전략의 주문은 그대로다. 계좌 전체의 `core.holds`에는 나오지 않는다.

1. 전략 프로세스를 본다(죽었는지, 멈췄는지, 소켓에 닿는지).
2. 전략을 다시 띄우거나 다시 연결한다. 전략의 `hello`가 보류를 풀고, 그 답이 온 뒤에는 일반 주문이 받아진다. 운영자 명령은 없다(`clear-hold`로는 풀지 않는다).
3. 살아 있는데 잃음이 되풀이되면(`no request for … ms`) 전략이 `liveness_ms`보다 짧은 간격으로 `ping`하는지 본다.

재시작해도 보류는 남는다(저널에서 복원, U-06): 다시 띄운 노드에서도 그 전략의 `hello`가 푼다. 재시작 때 걸린 일반 주문이 있는 적힌 전략은 시작부터 재므로, `liveness_ms` 안에 다시 붙지 않으면 그 주문이 취소된다.

```toml
[liveness]
strategies = { a = 5000, b = 3000 }   # 전략마다 ms, 1000..=60000, [core]의 전략
```

mainnet은 사용 중인 전략이 모두 적혀야 시작한다. testnet과 가상 거래소에서는 선택이고, 없으면 시작할 때 `warn` 한 줄이 남는다. 적히지 않은 전략은 감시하지 않는다(요청마다 연결하는 `omsctl … new`가 그렇다).

- 전략을 사용 중지(`enabled = false`)하기 전에 그 전략의 걸린 일반 주문을 취소한다: 사용 중지한 전략은 적히지 않아도 되므로 그 주문은 감시 없이 남을 수 있다.
- 운영 소켓으로 보낸 전략의 주문은 감시를 무장하지 않는다. 그 전략이 거래 소켓으로 `hello`하기 전에는 그 일반 주문이 전략의 죽음과 상관없이 남는다.

### 7.16 모니터링이 없다 (`holds`에 `unmonitored`)

C-LIVE 2부. `[core.policy.monitor]`가 있으면 노드는 `monitor.sock`에서 워처의 하트비트를 받고, 코어가 기록 시각으로 판단한다. `status`의 `core.monitor`가 이유(`reason`), 시각(`since`), 마지막 하트비트 뒤의 시간(`last_heartbeat_age_ms`), 검사마다 마지막 성공 뒤의 시간(`checks`), 단계(`cancel_stage`: `waiting`이면 `cancel_at`에 걸린 일반 주문 취소가 시작된다, `cancelling`이면 이미 요청 중)를 보여 준다.

1. 워처를 본다(떠 있는지, 노드와 같은 uid인지, `monitor.sock`에 닿는지). 시작 직후라면 워처의 첫 하트비트를 기다린다: 모든 시작은 감시 안 됨으로 시작한다.
2. 이유가 `W1 …` 또는 `W2 …`이면 워처는 살아 있지만 그 검사가 성공하지 못한다(거래소 조회 실패 등). 워처의 로그를 본다. `reason`은 감시 안 됨이 **시작된** 이유다(감시 안 됨인 채 이유가 바뀌어도 그대로다). 지금의 사정은 `checks`(검사마다 마지막 성공 뒤의 ms)와 `last_heartbeat_age_ms`로 본다.
3. 워처가 W1·W2가 신선한 하트비트를 보내면 게이트는 스스로 열린다. 운영자 명령은 없다(`clear-hold`로는 풀지 않는다). 2단계에서 이미 요청한 취소는 되돌리지 않는다.
4. 워처 킬 중(`watcher_kill_active` = 1)에는 워처의 검사가 멈춰 하트비트가 없다. 워처가 살아 있는 OMS를 래치했으면 약 30초 뒤 `hold:unmonitored`와 그 P1이 따른다: 모니터링 상실이 워처 킬 중에 따라오고, 루프가 끝나고 W1·W2가 다시 성공하면 스스로 풀린다(7.18). 그 보류는 킬 래치가 이미 막는 일반 주문만 막는다.
5. 호스트 시계를 뒤로 돌렸거나(수동 `date`, VM 재개, 시각 동기화의 한 걸음) 그런 의심이 있으면: 벽시계가 마지막 기록 시각을 따라잡을 때까지 게이트의 시계가 멈춘다. 그동안 죽은 워처도 알아채지 못하고, 건강한 워처의 하트비트는 검사가 오래돼 보여 게이트를 닫을 수 있다. 워처를 직접 확인한다. 시계가 따라잡으면 스스로 정상으로 돌아온다.

```toml
[core.policy.monitor]            # 값은 기본값(설계 §7.5, R2)
heartbeat_max_age_us = 30000000
check_max_age_us = 10000000
unmonitored_cancel_us = 60000000
```

mainnet은 이 표 없이 시작하지 않는다. testnet과 가상 거래소에서는 선택이고, 없으면 시작할 때 `warn` 한 줄이 남는다(그러면 워처가 없어도 아무것도 거래를 멈추지 않는다). 표가 있으면 `[runtime] tick_ms`는 1,000 이하여야 한다(게이트는 틱에서만 하트비트가 없다는 것을 본다). 재시작은 게이트를 열지 않는다(U-06): 다시 띄운 노드도 감시 안 됨으로 시작하고, 워처가 60초 안에 하트비트를 보내지 않으면 걸린 일반 주문이 취소된다.

### 7.17 워처 자신의 상태

워처는 지표와 로그로, U19b1c부터는 자기 텔레그램 알림 `[WATCH P1]`(`[telegram]`이 있을 때, C-WATCH 10부)으로도 자기 상태를 알린다(지표의 알림 규칙은 U20b). 지표에는 모두 `venue="bybit"`, `host`, `watcher` 이름표가 붙는다. `[WATCH P1]` 메시지의 첫 줄은 `[WATCH P1] bybit <워처 이름> <KST>`다.

| 보이는 것 | 뜻 | 할 일 |
|---|---|---|
| `watcher_auth_rejected = 1` | Bybit가 키를 거절했거나(401, 10003, 10004, 10005, 10007, 10010, 33004) 재확인에서 키가 시작 규칙에 더는 맞지 않는다. 재시작까지 Bybit에 아무것도 묻지 않는다. 검사가 성공하지 않으니 OMS의 게이트가 닫힌다(7.16) | 오류 줄로 원인을 본다. `watcher_public_ip_ok = 0`도 함께면 이 호스트의 공인 IP가 바뀐 것일 수 있다. 키나 IP 목록을 고친 뒤 워처를 다시 띄운다 |
| `watcher_public_ip_ok = 0` | 반향이 본 공인 IP가 키의 IP 목록에 없거나 확인이 실패했다(`warn: public IP …`) | IP가 바뀌었으면 키의 IP 목록을 고친다. 반향 주소(`ip_echo_url`)가 죽었는지도 본다 |
| `watcher_api_key_days_left` 있음 | 키에 만료가 있다(IP에 묶이지 않았거나 비밀번호가 바뀐 키, Bybit 문서). 없으면 해당 없음 | 0이 되기 전에 키를 바꾼다 |
| `watcher_journal_error = 1` | 저널 읽기가 실패했다. `the journal is read again at the next round`면 다음 라운드가 다시 읽고, `the journal reader stops until the watcher restarts`면 재시작까지 멈춘다(모르는 이벤트, 다른 스키마나 인스턴스, 내려간 상한) | 다시 읽는 쪽이 계속되면 파일과 권한을 본다. 멈춘 쪽은 워처가 OMS보다 오래된 빌드인지(새 이벤트) 본 뒤 워처를 고쳐 다시 띄운다 |
| `watcher_check_age_s{check}`가 계속 자람 | 그 검사가 성공하지 못한다: 거래소 오류, `/api/status` 무응답(`warn: … round failed: /api/status: …`), 저널 오류, 또는 W2의 미룸(체결이 이어지는 동안). 시작 중(시작 확인, OMS 기다림, 재생)이면 설정을 읽은 때부터 센다 | 워처 로그의 `round failed` 줄. W1·W2가 10초를 넘으면 OMS의 게이트가 닫힌다(7.16) |
| `watcher_heartbeat_ok_ts`가 멈춤 | `monitor.sock`에 하트비트가 기록되지 않는다(`warn: monitor.sock … not connected`, `error: monitor.sock heartbeat refused …`) | 워처가 노드와 같은 uid인지, `[core.policy.monitor]`가 있는지(없으면 소켓이 없다), 두 호스트 시계 |
| `watcher_up`이 없음 | 워처나 그 지표 밀어 넣기가 없다 | 프로세스와 `[telemetry] vm_url` |
| `watcher_mismatch_active{check}` > 0 | 확정된 차이 가운데 라운드가 아직 보거나 OMS에 아직 빚진 것(보류 아래에서 확정돼 해제를 기다림) | 7.13 |
| `[WATCH P1]` `워처 W<n> 불일치 확정 k건. OMS에 일반 주문 차단(watcher_mismatch)을 보내기 시작함…` | 워처가 차이를 확정했다(`watcher_mismatches_total`이 오름). 보류가 걸리면 OMS의 `hold:watcher_mismatch` 알림이 따른다. 검사마다 60초에 하나이고 그 사이의 확정은 다음 메시지에 모인다(다섯까지 보이고 나머지는 `그리고 m건 더`). 워처가 꺼질 때 모여 있던 것은 바로 간다. OMS가 이미 `watcher_mismatch`를 쥐고 있으면 "보내기 시작함"과 달리 워처는 해제까지 보내지 않는다. 운영자의 해제 뒤 다시 보내기는 새 메시지를 보내지 않는다 | 7.13(OMS의 보류 알림과 짝을 맞춘다). 모든 내용은 워처 로그의 `error: confirmed difference:` 줄에 있다 |
| `[WATCH P1]` `워처 키가 거부됨(retCode … / HTTP 401: …)` 또는 `워처 키가 시작 규칙을 더는 지키지 않음(…)` | `watcher_auth_rejected = 1`과 같은 일(위 행). 재시작까지 해소가 없고 5분마다 같은 문구가 온다 | 위 `watcher_auth_rejected` 행. 고친 뒤 워처를 다시 띄우면 멈춘다 |
| `[WATCH P1]` `워처의 공인 IP가 키의 IP 목록에 없음(…)`, 뒤에 `해소: …` | `watcher_public_ip_ok = 0`(위 행). 괄호 안이 이유다: 목록 밖 주소, `unknown: the IP echo failed …`, `unknown: the IP recheck's query-api failed …`. 반향이 흔들리면 재확인마다(`ip_check_ms`, 기본 60초) 시작·해소 한 쌍이 올 수 있다: 해소 뒤의 시작은 새 조건이라 5분 반복 규칙이 묶지 않는다 | 위 `watcher_public_ip_ok` 행. 쌍이 이어지면 반향 주소(`ip_echo_url`)와 이 호스트의 네트워크를 본다 |
| `[WATCH P1]` `워처 시작 거부: …. 워처가 꺼짐: W1~W4와 W5가 돌지 않음` | 송신기가 뜬 뒤 시작 확인이 시작을 거부했거나(마스터 키, 다른 하위 계정의 키, IP에 묶이지 않은 키, USDT로 정산되지 않는 기호) `[kill]`이 있는데 OMS가 다른 킬 마커 경로를 읽는다(`the OMS reads its kill marker at …`). 킬 중이었으면 `워처 시작 거부: …로 워처 킬 중단(…)`이 먼저 오고, 정지를 빚졌으면 거부 전에 `watcher_kill`을 하나 보내 끝에 `. 늦은 정지: latched` 또는 `. 늦은 정지: 실패(…)`가 붙는다(실패면 OMS가 래치 없이 거래할 수 있다: 7.18). 워처는 종료 코드 2로 꺼졌다. 하트비트가 없으니 OMS는 30초 뒤 일반 주문을 거절한다(7.16) | 이유를 고치고 워처를 다시 띄운다. 종료 코드 2에 다시 띄우는 단위라면 재시작마다 하나가 온다(워처 단위는 `RestartPreventExitStatus=2`) |
| `[WATCH P1]` `OMS가 읽는 킬 마커 경로가 워처의 경로와 다름(OMS: …, 워처: …). 킬 마커가 OMS를 래치하지 못함…`, 뒤에 `해소: …` | 시작 뒤 W5의 탐침 답이 다른 킬 마커 경로(또는 `없음`: OMS에 `[kill]`이 없음)를 보였다: OMS가 다른 설정으로 다시 떴다. 5분마다 되풀이한다. 이 동안 쏜 워처 킬은 마커를 쓰지만 OMS를 래치한다고 세지 않아 정지를 빚는다(`씀(OMS가 읽는 경로인지 확인 못 함)`, 7.18) | OMS 설정의 `[kill] marker`를 워처의 것과 같은 파일로 맞추고 OMS를 다시 띄운다(워처는 그대로; 맞는 답이 오면 `해소`) |

### 7.18 워처가 OMS를 멈췄다 (W5)

C-WATCH 12부([설명 §6](../explanation.md)). `[kill]`이 있는 워처는 OMS의 `/api/status`가 `silence_ms`(기본 10초, 설계 값) 동안 정상으로 답하지 않으면 킬 마커를 쓰고(K1), `admin.sock`에 `watcher_kill`을 보내고(K2; 래치하지 못하면 `systemctl stop <oms_unit>`, K3), 그 뒤에야 자기 키로 1초마다 일괄 취소해 미체결 0건을 확인하고 30초 지켜본다(linear·USDT만, 포지션은 닫지 않음). 알림은 `[WATCH P1] … 워처 킬 …`이다(표는 7.12).

1. 왜 OMS가 조용했는지 본다: OMS의 로그와 저널(마지막 기록), 프로세스가 살아 있는지. 워처 로그의 `error: W5: /api/status gave no good answer for <ms> ms …` 줄이 쏜 때다. 워처가 OMS보다 먼저 떴거나 `status_url`이 틀려도 쏜다(아래 시작 순서).
2. 마커를 본다: 시작 알림의 `킬 마커: 씀 | 이미 있음 | 쓰지 못함(…)`, 그리고 `(OMS가 읽는 경로인지 확인 못 함)`이 붙었는지. 마커가 OMS가 읽는 경로에 있으면 OMS의 다음 시작은 래치된 채 시작하고(`core.kill.source: marker`) 자기 킬 루프를 다시 돌린다(7.12). 다시 띄운 OMS의 `holds`는 `kill_latched`(워처의 하트비트가 다시 오기 전에는 `unmonitored`도)뿐이고 `watcher_mismatch`는 없다: 저널에 아직 열린 주문이 있어도 래치 동안 워처는 그 주문이 거래소에 없다고 확정하지 않는다(ADR-040). `watcher_mismatch`가 보이면 다른 차이다(7.13).
3. 기다린다: OMS의 `킬 루프 정리 완료`(OMS가 래치된 채 돌고 있으면)와 워처의 `워처 킬 정리 완료`를 둘 다. 래치된 살아 있는 OMS는 그동안 약 30초 뒤 `hold:unmonitored`와 `모니터링 상실` P1을 낸다: 모니터링 상실이 워처 킬 중에 따라오고, 루프가 끝나고 W1·W2가 다시 성공하면 스스로 풀린다(7.16).
4. 그 뒤에만 `clear-hold --reason kill`로 푼다(7.12). **`watcher_kill_active` = 1이거나 워처의 마지막 킬 알림이 `정리 완료`도 `중단`도 아니면 풀지 않는다**: OMS의 루프가 먼저 끝나도 워처의 루프는 30초 동안 0건을 볼 때까지 1초마다 일괄 취소를 보내므로, 거래를 다시 시작한 OMS의 주문을 취소한다.
5. 끝나지 않는 워처 루프(진행 알림의 `워처 키가 멈춰 일괄 취소를 보낼 수 없음: 워처를 다시 시작해야 루프가 끝남`, 또는 오래 이어지는 `워처 요청 일시 정지(HTTP 403)…`)는 워처를 다시 시작해야만 끝난다(SIGTERM: `워처 종료로 워처 킬 중단(…)` 알림이 따른다). 다시 뜬 워처는 킬을 이어 가지 않고, 그때도 OMS가 답하지 않으면 다시 킬한다(옛 마커는 `이미 있음`).
6. 마지막 알림의 마지막 줄을 본다.

   | 마지막 줄 | 뜻 | 할 일 |
   |---|---|---|
   | `OMS는 래치됨(watcher_kill). 해제는 운영자만: …` | 래치가 디스크에 있다 | 2~4번 |
   | `OMS는 멈춤(systemctl stop). 다음 시작은 킬 마커로 래치됨. …` | OMS 단위가 멈췄고 OMS가 읽는 경로에 마커가 있다 | OMS를 다시 띄우면 래치된 채 시작한다. 그다음 4번 |
   | `OMS는 멈춤(systemctl stop). 킬 마커는 OMS가 읽는 경로인지 확인 못 함: …` / `… 킬 마커가 없어 다음 시작은 래치 없이 시작함: …` | 멈췄지만 다음 시작의 래치를 확인하지 못했다. 정지를 빚졌다 | OMS를 다시 띄우기 전에 마커와 OMS 설정의 `[kill] marker`를 본다. 띄우면 워처가 첫 정상 답에 `watcher_kill`을 보낸다 |
   | `OMS 정지 미확인: OMS가 살아 있으면 래치 없이 주문을 낼 수 있음. …` | `watcher_kill`도 `systemctl stop`도 확인되지 않았다(K3에는 polkit 규칙이 설치돼 있어야 한다: [observability.md](observability.md) §13.1, A-22). 정지를 빚졌다 | OMS가 살아 있는지 본다: 60초 기다린 뒤 `systemctl is-active oms-node`(systemd의 정지 작업은 `TimeoutStopSec=60`에 끝난다). `active`·`deactivating`이면 손으로 멈춘다(`systemctl kill -s SIGKILL oms-node`, 적어도 `omsctl … halt`). observability.md §13.6 |

7. 빚진 정지: OMS 단위는 스스로 돌아오지 않는다(`Restart=no`, 4장). 그러나 사람이 다시 띄운 OMS나 멈췄을 뿐인 OMS는 마커를 확인하지 못했으면 래치 없이 돌아온다. 워처는 루프 동안 5초마다, 루프 뒤에는 OMS의 첫 정상 답에 `watcher_kill`을 보낸다(`watcher_kill_halt_owed` = 1인 동안). `늦은 정지: OMS가 다시 답해 watcher_kill로 래치함(…)`이면 래치됐다(2~4번). `늦은 정지 실패: OMS가 답하지만 watcher_kill이 안 됨(…)`이면 **OMS가 래치 없이 거래한다: 손으로 멈춘다**(`systemctl stop <oms_unit>`). 빚은 워처의 메모리에만 있다: 워처를 다시 시작하면 잊는다.

**시작 순서**: OMS를 먼저 띄우고 `/api/status`가 답할 때까지 기다린 뒤 워처를 띄운다. 워처가 먼저 뜨거나, 사람이 띄운 OMS가 아직 부팅 중일 때 워처를 다시 띄우거나, `status_url`이 틀리면 워처는 `silence_ms` 뒤 건강하거나 부팅 중인 OMS를 킬한다(C-WATCH 12부, 소유자 질문 1). **정비**: OMS를 일부러 멈출 때는 워처를 먼저 멈춘다.

### 킬 마커 (`[kill] marker`)

워처(U19b)가 킬할 때 쓰는 파일이다. 노드는 시작할 때 한 번만 본다: 무엇이든 있으면(빈 파일, 디렉터리, 심볼릭 링크 포함) 킬 래치를 걸고, 볼 수 없으면(권한 등) 역시 건다. 실행 중에 생긴 마커는 다음 시작까지 효과가 없다(워처는 `watcher_kill`도 보낸다). 지우는 것은 `clear-hold --reason kill`뿐이다. 내구화된 해제 뒤에 노드가 지운다. 손으로 지우지 않는다. 저널의 래치는 마커가 없어도 다음 시작을 건다. 그러나 마커만이 킬을 넘겨주는 경우가 있다: 새 데이터 디렉터리로 시작할 때, 그리고 워처가 마커를 썼지만 노드가 `watcher_kill`을 받지 못한 때. 그때 마커를 손으로 지우면 킬이 사라진다.

```toml
[kill]
marker = "/var/lib/hl-oms/kill.marker"   # 절대 경로, 데이터 디렉터리 밖, 디렉터리가 있어야 한다
```

mainnet은 `[kill]` 없이 시작하지 않는다. testnet과 가상 거래소에서는 선택이고, 없으면 시작할 때 `warn` 한 줄이 남는다.

## 8. 보관과 사본

- 저널은 자동으로 지우지 않는다(U-04). 하루 사용량은 요청량에 비례한다. 측정: 접수→체결→종결 한 주문에 기록 약 3건, 약 1.4 KiB.
- 사본: 닫힌 세그먼트(가장 큰 번호가 아닌 것)는 바뀌지 않으므로 그대로 복사한다. 쓰고 있는 세그먼트를 복사하면 끝이 잘릴 수 있고, 복구가 그것을 불완전한 꼬리로 처리한다.
- 사본에서의 복구는 §7.6을 따른다. 사본은 "거래를 이어 가기 위한 것"이 아니라 "무슨 일이 있었는지 알기 위한 것"으로 본다.
- 오래된 세그먼트를 지우려면: 그보다 새로운 스냅샷이 있고 `verify`가 통과하는지 확인한 뒤, 스냅샷 순번보다 앞선 세그먼트만 다른 곳으로 옮긴다. 옮긴 뒤에는 `trace`와 `verify --re-decide`가 그 구간을 볼 수 없다. 이 절차는 연습하지 않았다.

## 9. 버전 올리기와 되돌리기

저널에는 세 가지 버전이 있다: 기록 스키마(`RECORD_SCHEMA`), 상태 스키마(스냅샷), 그리고 각 부팅을 결정한 바이너리의 소스 다이제스트(`booted` 이벤트).

지금 빌드는 기록 스키마 2를 쓰고 1과 2를 읽는다([ADR-023](../adr/ADR-023-venue-made-executions.md)). 스키마 1 저널 뒤에 이어 쓰면 새 세그먼트가 열린다. 스키마 2 세그먼트가 생긴 저널은 스키마 1만 읽는 옛 바이너리로 되돌릴 수 없다.

| 바꾼 것 | 옛 저널을 새 바이너리로 | 새 저널을 옛 바이너리로(되돌리기) |
|---|---|---|
| 판단 로직만(이벤트 형식 그대로) | 된다. 복구는 이벤트만 적용하므로 옛 판단을 다시 하지 않는다 | 된다 |
| 이벤트에 선택 필드 추가 | 된다 | 옛 바이너리가 그 필드를 모르면 시작을 거부할 수 있다. 미리 시험한다 |
| 기록 스키마 번호 변경 | 새 바이너리가 옛 번호를 읽도록 만들어야 한다 | **안 된다.** 시작 거부 |
| 상태 스키마 번호 변경 | 옛 스냅샷은 건너뛰고 저널을 재생한다(저널이 처음부터 있어야 한다) | 새 스냅샷을 건너뛰고 저널을 재생한다 |

상태 스키마는 U18b1에서 1에서 2로 올랐다([ADR-030](../adr/ADR-030-kill-switch-holds.md) §8): 옛 빌드의 스냅샷에는 거래소 변경 보류가 없다. 첫 시작은 저널 전체를 재생한다. §8의 절차로 앞쪽 세그먼트를 옮긴 데이터 디렉터리는 그 세그먼트를 되돌려 놓아야 시작한다.

절차:

1. 새 바이너리로 현재 저널의 사본을 검사한다: `oms-node verify --data-dir <사본>`. 통과해야 한다.
   `--re-decide`는 판단 로직을 바꿨다면 차이를 보고할 수 있다. 그 차이가 의도한 변경인지 읽어 본다.
2. `halt` → `cancel-all` → 열린 주문 0 확인 → `snapshot` → `SIGTERM`. 열린 주문 없이 바꾸는 것이 가장 단순하다.
   열린 주문을 둔 채 바꾸면 그 주문들은 새 바이너리가 재확인한다. 이 경우를 따로 시험하지 않았다.
3. 옛 바이너리와 데이터 디렉터리의 사본을 보관한다.
4. 새 바이너리로 띄우고 화면에서 차단 사유, 불일치, 소스 다이제스트를 확인한다.
5. 되돌릴 때: 새 바이너리가 쓴 기록을 옛 바이너리가 읽을 수 있는 경우에만 같은 디렉터리로 되돌린다. 아니면 §7.6의 "새 인스턴스로 시작"을 따른다. 3의 사본으로 되돌아가 거래를 재개하지 않는다.

설정 변경: `[core]`의 내용을 바꾸면 `version`을 올린다. 재시작할 때 적용되고 저널에 남는다. 실행 중 변경은 없다.
한도를 줄이는 변경은 받는다. 새 한도는 다음 주문부터 적용되고 이미 열린 주문과 포지션은 그대로다.
열린 주문이나 포지션이 있는 전략·종목을 설정에서 빼면 거부한다. 먼저 주문을 정리한다.

## 10. testnet 첫 실행 (U-01의 허가가 있을 때)

주문 경로는 실제 거래소에서 실행된 적이 없다. 처음에는 다음 순서로 한다.

1. testnet 계정에 agent 지갑을 만들고 그 키만 환경 변수로 준다.
2. `config/hyperliquid-testnet.example.toml`을 복사해 계좌 주소를 넣는다. 한도는 예제의 작은 값 그대로 둔다.
   예제에는 증거금 정책(`[core.policy.margin]`)이 있다. OMS는 거래소의 레버리지와 증거금 방식을 바꾸지 않으므로, 거래소에서 종목을 격리·레버리지 2로 직접 맞춘다. 다르면 그 종목의 일반 주문이 `VenueSetting`으로 막힌다. `account_modes_allowed`는 비어 있어서 처음에는 일반 주문이 모두 `AccountMode`로 막힌다. `status`의 `core.margin.account_mode`에 보이는 값을 확인하고(가정 T10) 그 문자열을 적은 뒤 `version`을 올려 다시 띄운다.
3. 띄우고 화면에서 확인한다: 실행 종류가 "실제 시스템 · hyperliquid:testnet", 링크 연결, 차단 사유 없음, 참조 가격 수신. 증거금 정책이 있으면 `status`의 `core.margin`에서 계좌 가치와 그 나이, 종목별 거래소 설정, 계좌 모드도 확인한다. 보고가 거부되면 사유가 거기에 보인다.
4. 시장에서 먼 가격의 최소 금액 지정가(ALO) 한 건 → 화면의 주문 추적에서 `order_accepted → order_acked` 확인 → 취소 → `order_closed`.
5. 체결되는 IOC 한 건 → `fill_booked`, 포지션 대조 통과 확인.
6. [조사 문서 §4](../research/hyperliquid-protocol.md)의 실험 목록을 차례로 한다. 설계가 기대는 것부터: 만료 시각이 지난 요청의 거부, 체결 시각이 스트림과 조회에서 같은지, 체결 조회의 쪽 크기, IOC 부분 체결의 응답, 열린 주문·체결에 cloid가 있는지.
7. 결과를 `docs/verification/`에 기록하고 가정 A-01, A-02, A-03의 "확인한 범위"를 고친다.

증거금 규칙의 예행(위 1~7 다음에, 같은 허가 아래에서). 증거금 규칙은 가상 거래소와 시뮬레이션으로만 시험했고 실제 거래소에서는 한 번도 실행하지 않았다. 아래는 해 본 적 없는 절차다.

8. 가정 T1~T11을 하나씩 확인한다([ADR-018 §8](../adr/ADR-018-margin-rules.md), [검증 문서 §11.2](../verification/README.md)). 먼저 볼 것: `status`의 `core.margin`이 보고를 받는가(거부되면 `last_rejection`에 읽지 못한 필드가 나온다. 응답의 형식이 문서의 예와 다르면 모든 보고가 거부된다), 계좌 모드의 문자열(T10), 포지션이 없는 종목의 `venue_settings`가 채워지는가(T11), 격리 포지션의 `margin_used`와 계좌 가치가 거래소 화면의 어느 숫자와 같은가(T1, T2).
9. **T11을 먼저 본다.** 포지션이 없는 종목의 거래소 설정이 채워지지 않으면 그 종목의 일반 주문은 `VenueSetting` 차단으로 계속 거절된다. 대조기는 그 조회를 `venue_setting_poll_ms`(기본 60초)에 한 번만 되묻으므로 조회 예산은 그대로다([검증 문서 §11.5](../verification/README.md)). 응답 하나를 잃어도 같은 간격 동안 막혀 있으니, 링크가 올라온 뒤 한 간격을 기다렸다가 `venue_settings`를 다시 본다.
10. 감소 전용 상태 훈련: 작은 계좌에서 일 손실 한도를 일부러 넘기거나 출금해서(7.9) 상태를 걸고, 걸린 일반 주문이 거래소에서 취소되는지, 감소 전용 주문이 남고 새로 받아지는지, `clear-reduce`가 기준을 다시 잡는지 본다(7.8).
11. 보고 끊김 취소 훈련: 링크는 둔 채 증거금 조회만 60초 넘게 실패하게 만들 방법이 있어야 한다(지금은 그런 스위치가 없다. 방법을 정해야 한다). 걸린 일반 주문이 한 번씩 취소되는지 본다.
12. 열린 주문과 감소 전용 상태가 있는 채로 노드를 강제 종료하고 다시 띄워, 상태와 기준 금액이 그대로이고 보내지 못한 취소가 나가는지 본다.
13. 결과를 `docs/verification/`에 기록하고 ADR-018 §8의 T1~T11에 "확인한 범위"를 적는다.

mainnet은 `mainnet_confirmed = true`가 있어야 열린다(U-09). 증거금 정책이나 `[alert]`(7.11)가 없으면 mainnet 설정으로는 시작을 거부한다. 6의 실험과 8~13의 예행이 끝나기 전에는 권하지 않는다. 예행과 mainnet 모두 사용자의 승인이 필요하다.

## 11. 점검 목록

매일(자동화되어 있지 않다):

- 화면: 차단 사유, 결과 불명, 불일치가 비어 있는가. 소스 다이제스트가 배포한 것과 같은가.
- `df`: 데이터 디렉터리의 여유.
- 거래소 화면의 포지션과 화면의 계좌 장부가 같은가.
- 증거금 정책이 있으면 `status`의 `core.margin`: 감소 전용 상태가 걸려 있지 않은가(`reducing`), 기준 금액(`day_base`), 계좌 가치와 그 나이(`age_us`), 계좌 노출, 종목별 거래소 설정과 계좌 모드가 정책과 맞는가, 보고가 거부된 사유(`last_rejection`)가 없는가.

주기적으로:

- `oms-node verify --data-dir <dir>`.
- 사본에서 `verify`가 통과하는가.
- 호스트의 시계 동기화 상태(`timedatectl`). 시계가 거래소와 2초 이상 어긋나면 체결 동기화 선언이 틀릴 수 있다(A-02).
