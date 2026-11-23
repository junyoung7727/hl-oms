# 워처 운영

설계: [ADR-031](../adr/ADR-031-watcher.md). 계약: [C-WATCH](../contracts/watcher.md). 이 문서는 워처의 설정과 자격 파일(U19a4), 시작 확인과 그 거부, 시작 확인이 내보내는 지표(U19a4b)를 적는다. **워처는 아직 실행되지 않는다**: 바이너리는 `--version`에만 답하고, 검사(U19a5~), 실행과 상황별 의미(U19a7)는 아직 없다. 시작 확인은 만들어졌지만 그것을 돌리는 실행 코드는 U19a7이다. 서비스 설치와 알림 규칙은 U20b다.

## 설정 파일

`watcher --config /etc/hl-oms/watcher.toml`(U19a7부터). TOML이고 모르는 키는 거부한다. 거부는 종료 코드 2와 이유 한 줄이다.

```toml
name = "watcher-1"                                # 하트비트와 지표의 이름, 1~64자
network = "testnet"                               # testnet | mainnet
# mainnet_confirmed = true                        # mainnet은 이것 없이 시작하지 않음
credentials_file = "/etc/hl-oms/watcher-bybit.toml"   # 데이터 디렉터리 밖, 0600 또는 0400
sub_account_uid = 4242                            # OMS와 같은 하위 계정의 UID
symbols = ["BTCUSDT", "ETHUSDT"]                  # OMS가 거래하는 기호, linear/USDT
ip_echo_url = "https://ip.example/"               # https만, 본문 = 공인 IP
data_dir = "/var/lib/hl-oms/data"                 # OMS 데이터 디렉터리(읽기만 함)
status_url = "http://127.0.0.1:8188/api/status"   # loopback, 포트 필수, 80은 안 됨
# admin_socket = "/var/lib/hl-oms/data/admin.sock"      # 기본: <data_dir>/admin.sock
# monitor_socket = "/var/lib/hl-oms/data/monitor.sock"  # 기본: <data_dir>/monitor.sock
# host = "tokyo-1"                                # 기본: /etc/hostname

# [intervals]  w1_ms = 1000  w2_ms = 1000  w3_ms = 2000  w4_ms = 5000
#              heartbeat_ms = 5000  ip_check_ms = 60000  export_ms = 1000
# [rules]      min_age_ms = 2000  w4_overlap_ms = 10000  w4_max_pages = 10
#              w4_initial_lookback_ms = 600000  mismatch_resend_ms = 10000
# [w3]         account_mm_rate_max_bps = 5000  daily_loss_bps = 500
# [http]       recv_window_ms = 5000  request_timeout_ms = 2000

[telemetry]                                       # 없으면 아무것도 내보내지 않음
vm_url = "http://127.0.0.1:8428"                  # http, loopback 주소 숫자만
```

| 필드 | 기본 | 받는 값(범위는 판단(모델 아님)) |
|---|---|---|
| `name` | 필수 | 1~64자, 제어 문자 없음 |
| `network` | 필수 | `testnet`, `mainnet` |
| `mainnet_confirmed` | `false` | `mainnet`이면 `true`여야 함 |
| `credentials_file` | 필수 | 절대 경로, `..` 없음, `data_dir` 밖 |
| `sub_account_uid` | 필수 | 1 이상 |
| `symbols` | 필수 | 1~50개, 각각 `A-Z`·`0-9` 1~32자, 중복 없음 |
| `ip_echo_url` | 필수 | `https://…`, 사용자·비밀번호 없음, 공백·제어 문자 없음 |
| `data_dir` | 필수 | 절대 경로 |
| `status_url` | 필수 | `http://127.0.0.1:<port>/api/status`, `http://[::1]:<port>/api/status`(포트 1~65535, 80 제외) |
| `admin_socket`, `monitor_socket` | `<data_dir>/admin.sock`, `<data_dir>/monitor.sock` | 절대 경로, 서로 다름 |
| `host` | `/etc/hostname`, 없으면 `unknown` | 1~255자, 제어 문자 없음 |
| `[intervals] w1_ms`, `w2_ms` | 1000 | 500~10,000 |
| `[intervals] w3_ms` | 2000 | 1000~30,000 |
| `[intervals] w4_ms` | 5000 | 1000~60,000 |
| `[intervals] heartbeat_ms` | 5000 | 1000~10,000 |
| `[intervals] ip_check_ms` | 60,000 | 10,000~3,600,000 |
| `[intervals] export_ms` | 1000 | 1000~60,000 |
| `[rules] min_age_ms` | 2000 | 500~10,000 |
| `[rules] w4_overlap_ms` | 10,000 | 0~60,000 |
| `[rules] w4_max_pages` | 10 | 1~50 |
| `[rules] w4_initial_lookback_ms` | 600,000(판단(모델 아님)) | 0~86,400,000 |
| `[rules] mismatch_resend_ms` | 10,000(판단(모델 아님)) | 1000~600,000 |
| `[w3] account_mm_rate_max_bps` | 5000 | 1~9999 |
| `[w3] daily_loss_bps` | 500 | 1~10,000 |
| `[http] recv_window_ms` | 5000 | 1000~20,000 |
| `[http] request_timeout_ms` | 2000(판단(모델 아님)) | 200~10,000 |
| `[telemetry] vm_url` | 없음 | `http://`, loopback 주소 숫자(`localhost`는 안 됨), 공백·제어 문자 없음(끝의 공백도) |

기본값의 근거(설계 §7.5, 스코프, OMS의 값)와 거부의 이유는 [C-WATCH 4부](../contracts/watcher.md)에 있다.

## 자격 파일

```sh
install -m 600 /dev/null /etc/hl-oms/watcher-bybit.toml   # 워처를 돌릴 사용자로
$EDITOR /etc/hl-oms/watcher-bybit.toml
```

```toml
api_key = "..."
api_secret = "..."
```

- 두 키만 둔다. 다른 키가 있거나 하나가 비면 시작하지 않는다.
- 워처를 돌리는 사용자가 소유하고 권한은 `0600` 또는 `0400`이어야 한다. 심볼릭 링크는 받지 않는다(링크가 아닌 파일 경로를 적는다). OMS 데이터 디렉터리 안에 두지 않는다.
- 오류 줄은 파일 내용을 인용하지 않는다. 그래도 저장소, 로그, 메시지에 이 파일을 붙이지 않는다.

## 워처 키

- **OMS의 키와 다른 워처 전용 키**를 같은 하위 계정(`sub_account_uid`)에 만든다(설계 §7.4).
- **출금 권한을 주지 않는다**(설계 §7.4). 워처의 시작 확인은 이것을 보지 않는다(OMS도 자기 키에 대해 보지 않는다). 키를 만들 때 사람이 확인한다.
- 권한은 조회와, U19b의 일괄 취소를 쓸 때 주문 권한만 준다. U19a의 워처는 조회만 한다.
- 키를 IP에 묶는다(설계 §7.4). 묶지 않은 키는 시작 확인이 거부한다(U19a4b).

## 시작 확인과 거부(U19a4b)

설정과 키 파일을 읽은 뒤 워처는 차례로 묻는다. 거부는 종료 코드 2와 이유 한 줄이다(U19a7부터 stderr). 거부 뒤에는 아무 요청도 보내지 않는다.

| 단계 | 묻는 것 | 거부 줄(예) | 고칠 것 |
|---|---|---|---|
| 키 | `GET /v5/user/query-api` | `query-api: the API key is the master account's; the watcher needs a sub-account key` | 하위 계정의 워처 전용 키를 쓴다 |
| 키 | 〃 | `query-api: the API key belongs to user 4243; the config says sub_account_uid 4242` | `sub_account_uid`를 OMS와 같은 하위 계정으로, 또는 그 계정의 키로 |
| 키 | 〃 | `query-api: the API key is bound to no IP address (ips [] or "*"); bind it to this host's address` | 키를 이 호스트의 공인 IP에 묶는다 |
| 키 | 〃 | `query-api: the API key's ips hold no IP address` | 〃 |
| 기호 | 기호마다 `GET /v5/market/instruments-info?category=linear&symbol=<s>` | `BTCPERP settles in USDC; the watcher's scope is linear/USDT` | `symbols`에서 USDT로 정산되지 않는 기호를 뺀다 |
| 기호 | 〃 | `BTCUSDT is not in instruments-info for category linear; the watcher's scope is linear/USDT` | 기호 이름을 확인한다 |

거부가 아닌 경우:

- **키가 거절됨**(HTTP 401, 10003, 10004, 10005, 10007, 10010, 33004): 끝나지 않고 떠 있는다. `watcher_auth_rejected = 1`, 오류 한 줄 `error: Bybit rejected the key (<code>) …`. 재시작할 때까지 Bybit에 아무것도 보내지 않는다. 검사가 성공하지 않으니 OMS는 곧 감시 없음으로 일반 주문을 거절한다. 키·IP를 고친 뒤 워처를 다시 띄운다.
- **답이 없음, 요청 제한, 시각 오류, 그 밖의 거절 코드, 읽을 수 없는 답**: 한 줄(`start: … failed (<class>): …; asked again at <ms> ms`)을 쓰고 5초 뒤(판단(모델 아님)) 같은 것을 다시 묻는다. **HTTP 403**이면 600초 뒤다(Bybit의 IP 차단). 시작은 그동안 다음 단계로 가지 않는다.
- **공인 IP**(`ip_echo_url`): 키의 IP 목록에 없거나, 반향이 실패하거나, 본문이 주소가 아니면 `watcher_public_ip_ok = 0`과 경고 한 줄 `warn: public IP …`. 거부가 아니다(IP가 정말 다르면 Bybit가 10010으로 답해 위의 "키가 거절됨"이 된다).
- **만료일**: `expiredAt`을 읽을 수 없으면 경고 한 줄 `warn: query-api: expiredAt … cannot be read; watcher_api_key_days_left is not exported`.

시작 뒤 `ip_check_ms`(기본 60초)마다 키와 공인 IP를 다시 본다.

- 실패하면(답 없음 등) `watcher_public_ip_ok = 0`과 경고 한 줄 `warn: IP recheck: query-api failed …`, 남은 날은 마지막 값 그대로.
- 재확인에서 **키가 거절되면**(10010 등) Bybit에는 더 묻지 않지만 공인 IP는 60초마다 계속 본다(반향은 Bybit 요청이 아니다). 비교하는 목록은 마지막으로 통과한 키의 IP 목록이다. 그래서 `watcher_auth_rejected = 1`과 `watcher_public_ip_ok = 0`이 함께 보이면 이 호스트의 공인 IP가 바뀐 것이 원인일 수 있다. 시작 5단계에서 키가 거절된 때도 같다. 시작 2단계에서 거절되면 IP 목록을 몰라 `watcher_public_ip_ok`가 없다.
- 키가 **다른 계정의 것, 마스터 키, IP에 묶이지 않은 키**로 보이면 키 거절과 같이 재시작까지 모든 Bybit 요청을 멈추고 `watcher_public_ip_ok = 0`이 된다: `watcher_auth_rejected = 1`, 오류 한 줄 `error: the key no longer passes the start check: query-api: …; every Bybit request stops until the watcher restarts`. 키를 고친 뒤 다시 띄운다.

## 시작 확인의 지표(U19a4b)

모두 `venue="bybit"`, `host`, `watcher` 이름표가 붙는다. 알림 규칙(예: `watcher_api_key_days_left < 14`)은 U20b다.

| 이름 | 뜻 | 볼 것 |
|---|---|---|
| `watcher_api_key_days_left` | 키 만료까지 남은 날(내림, 만료 뒤 음수). IP에 묶인 키처럼 만료가 해당 없으면 없음 | 값이 있으면 키가 IP에 묶이지 않았거나 비밀번호가 바뀐 것이다(Bybit 문서). 만료 전에 키를 바꾼다 |
| `watcher_public_ip_ok` | 1: 이 호스트의 공인 IP가 키의 IP 목록(마지막으로 통과한 것)에 있음. 0: 없음, 확인 실패, 또는 키가 시작 규칙에 더는 맞지 않음. 키 거절 뒤에도 반향으로 계속 쓴다. 시작 2단계에서 키가 거절되면 없음 | 0이면 경고 줄(또는 규칙 위반의 오류 줄)로 원인을 본다. IP가 바뀌었으면 키의 IP 목록을 고친다 |
| `watcher_auth_rejected` | 1: 키 거절이나 시작 규칙 위반으로 모든 Bybit 요청이 멈췄음 | 오류 줄로 원인을 보고 키를 고친 뒤 재시작 |
