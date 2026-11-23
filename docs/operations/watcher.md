# 워처 운영

설계: [ADR-031](../adr/ADR-031-watcher.md). 계약: [C-WATCH](../contracts/watcher.md). 이 문서는 워처의 설정과 자격 파일(U19a4), 시작 확인과 그 거부, 시작 확인이 내보내는 지표(U19a4b), 실행과 상황별 의미(U19a7), 직접 알림의 토큰 파일(U19b1a, [ADR-032](../adr/ADR-032-watcher-kill-and-alerts.md))을 적는다. U19a7부터 `watcher --config`가 돈다: W1(열린 주문)과 W2(포지션), U19a8부터 W4(체결), U19a9부터 W3(지갑과 증거금)을 검사하고, 확정된 차이를 `admin.sock`에 보내고, `monitor.sock`에 하트비트를 보낸다. loopback 가짜로만 시험했고 실제 거래소에서 돌려 본 적이 없다. 서비스 설치와 알림 규칙은 U20b다.

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

[telegram]                                        # mainnet은 필수(U19b1a), testnet은 없으면 warn 한 줄
token_file = "/etc/hl-oms/watcher-telegram.json"  # 데이터 디렉터리 밖, git 저장소 밖, 0600 또는 0400
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
| `[telegram] token_file` | 표 없음 | 표가 있으면 필수. 절대 경로, `..` 없음, `data_dir` 밖. **mainnet은 `[telegram]` 없이 시작하지 않는다**(`mainnet requires [telegram]: the watcher's P1 must reach a person`). 다른 키(`chat_id`, `token`)는 거부: 비밀은 토큰 파일에만 |

기본값의 근거(설계 §7.5, 스코프, OMS의 값)와 거부의 이유는 [C-WATCH 4부](../contracts/watcher.md)에 있다.

`[w3]`의 두 한도는 OMS 설정의 `account_mm_rate_max_bps`·`daily_loss_bps`(C-MARGIN)와 **같은 값으로 둔다**. 워처는 OMS 설정을 읽지 않는다(독립). 값이 다르면 W3과 OMS의 감소 전용 판단이 어긋난다(U19a9, C-WATCH 9부).

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

## 토큰 파일(U19b1a)

워처 자신의 텔레그램 송신기가 쓰는 봇 토큰과 채팅이다(C-WATCH 10부, ADR-032). OMS의 `[alert]` 파일(`alert-direct`)과 형식이 같아 같은 파일을 가리켜도 된다(같은 봇, 메시지는 `[WATCH P1]`로 구별). 단 워처는 소유자가 자기 실효 uid이고 권한이 정확히 `0600`·`0400`이어야 받으므로(`alert-direct`는 그룹·기타 비트만 본다), 한 파일은 워처와 OMS가 같은 사용자로 돌 때만 된다(`admin.sock` 때문에 어차피 같은 사용자다).

```sh
install -m 600 /dev/null /etc/hl-oms/watcher-telegram.json   # 워처를 돌릴 사용자로
$EDITOR /etc/hl-oms/watcher-telegram.json
```

```json
{"token": "<봇 id 숫자>:<토큰>", "chat_id": -1001234567890}
```

- `token`은 `<숫자>:<영문·숫자·_·->`, `chat_id`는 비지 않은 문자열이나 정수. 다른 키는 읽지 않는다. `chat_id`를 틀리게 적으면 워처는 시작하지만 텔레그램이 모든 메시지를 거부해 아무 P1도 닿지 않는다(로그에만 남는다): 처음 설치할 때 그 채팅에 메시지가 오는지 본다(U19b1c부터).
- 자격 파일과 같은 규칙: 워처를 돌리는 사용자가 소유, 권한 정확히 `0600` 또는 `0400`, 심볼릭 링크 아님, OMS 데이터 디렉터리 밖.
- **git 저장소 안에 두지 않는다**: 파일이나 링크가 가리키는 곳의 위 디렉터리 어디에 `.git`이 있으면 시작하지 않는다(`token file … is inside the git repository …`).
- 워처는 이 파일을 시작 때 한 번, 시작 확인 전에 읽는다. 규칙에 맞지 않으면 종료 코드 2와 `error: token file …` 한 줄이고 거래소에 아무것도 묻지 않는다. 파일을 바꾸면 워처를 다시 시작한다.
- 오류 줄과 로그는 토큰을 적지 않는다. 그래도 저장소, 로그, 메시지에 이 파일을 붙이지 않는다.
- 지금의 워처는 토큰을 읽기만 하고 아직 보내지 않는다: 송신 태스크(U19b1b)는 있지만 실행이 그것을 띄우는 것과 P1 조건은 U19b1c다. 워처는 `api.telegram.org`에 DNS로, 안 되면 `149.154.167.220:443`으로 바로 간다(U19b1b): 방화벽은 둘 다 열어 둔다.
- 토큰이 폐기되었거나(401·404) 봇이 채팅에서 빠지면(403) 워처는 같은 첫 부분을 5초마다 다시 보내고 뒤의 P1은 쌓였다가 50개를 넘으면 버려진다. `watcher_alert_failures_total`이 오르는데 `watcher_alerts_delivered_total`이 그대로면 이 경우다: 토큰 파일과 채팅의 봇을 확인하고 워처를 다시 시작한다(C-WATCH 10부 알려진 한계).

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

## 실행(U19a7)

```sh
watcher --config /etc/hl-oms/watcher.toml     # OMS와 같은 사용자로
watcher --version
```

- **같은 uid**: `admin.sock`과 `monitor.sock`은 노드 자신의 uid만 받는다(`oms-node/src/server.rs:188-191`). 그래서 워처는 OMS를 돌리는 사용자로 돌고, 두 프로세스는 서로의 자격 파일을 읽을 수 있다. 알려진 한계이고 U20b(서비스 배치)에 넘긴다.
- OMS에 `[core.policy.monitor]`가 없으면 `monitor.sock`이 없다. 워처는 하트비트마다 한 번 연결을 시도하고, 상태가 바뀔 때만 `warn: monitor.sock … not connected …` 한 줄을 쓴다.
- 시작 순서: 설정과 키 파일 → 지표 밀어 넣기(여기서부터) → 시작 확인(위) → `/api/status` 기다림(1초마다 다시, `warn: waiting for /api/status: …`는 10초에 하나) → 저널을 기록 1부터 재생(`the OMS answers /api/status: replaying the journal up to record N`, `boot replay done at record N; the checks start`) → 검사와 하트비트. IP 재확인은 시작 확인 뒤 내내 돈다(OMS가 내려가 있어도).
- 재생 중과 OMS를 기다리는 동안에는 하트비트가 없다. 그동안 OMS의 게이트는 감시 안 됨이다(런북 7.16). 재생은 기록 100만 건에 약 1.3초다(C-WATCH 2부).
- 종료: SIGTERM이나 SIGINT. 새 라운드 없이 진행 중 요청을 `request_timeout_ms`까지 기다리고, 지표를 한 번 더 밀어 넣고, 모니터 연결을 닫고 코드 0으로 끝난다. 아무것도 저장하지 않는다(다음 시작이 처음부터 다시 읽는다).
- 종료 코드 2: 사용법, 설정 파일, 키 파일, 시작 규칙(위의 표). 이유는 stderr 한 줄(`watcher: …` 또는 `watcher: error: …`).
- 로그는 stderr의 한 줄씩이다(`watcher: ` 뒤). `error:`와 `warn:`으로 시작하는 줄이 운영자가 볼 것이다.

## 상황별 의미(U19a7)

| 로그 줄(앞부분) | 뜻 | 할 일 |
|---|---|---|
| `error: confirmed difference: <detail>` | 검사가 차이를 확정했다. 보류가 꺼져 있으면 `admin.sock`에 보낸다 | 런북 7.13 |
| `warn: W<n> mismatch send failed: …` | 보내기가 답을 받지 못했다(소켓 없음, 시간 초과, `overloaded` 등). 차이는 빚으로 남고 `mismatch_resend_ms` 뒤 다시 간다 | OMS가 떠 있는지, 같은 uid인지 |
| `error: W<n> mismatch send refused (bad_request…` | OMS가 줄을 거절했다. 워처의 결함이다 | 줄과 함께 보고한다 |
| `warn: W<n> round failed: …` | 그 검사의 라운드가 실패했다(거래소 오류, `/api/status`, 저널). 이유가 바뀔 때만 쓰고, 다시 성공하면 `W<n> rounds succeed again` | 런북 7.17 |
| `warn: monitor.sock … not connected: …` / `… connected` | 하트비트 연결의 상태 | OMS의 `[core.policy.monitor]`, uid |
| `error: monitor.sock heartbeat refused (…)` | OMS가 하트비트를 거절했다(1분에 한 줄). `bad_request`면 검사 시각이 OMS의 시계보다 앞섰을 수 있다 | 두 프로세스의 시계(같은 호스트), 런북 7.16 |
| `error: …; the journal reader stops until the watcher restarts` | 저널을 더 읽지 않는다 | 런북 7.17 |
| `warn: W4 window from … is longer than 7 days; cut to the last 7 days` / `W4 window is 7 days or shorter again` | W4의 창이 7일을 넘어 잘렸다(상태가 바뀔 때만). 오래 체결이 없거나 맞는 행이 워터마크를 움직이지 못했다. 잘린 앞쪽의 체결은 W4가 비교하지 않는다 | `watcher_differences{W4}`, 확정 줄, 거래소 체결 내역 |
| `warn: W4 page failed 3 rounds in a row; the sweep restarts …` | 같은 커서의 쪽이 세 라운드 연달아 실패했다(또는 저널 읽기가 래치되어 라운드마다 실패한다). 훑기를 1쪽부터 새 끝으로 다시 읽는다. 실패가 이어지는 동안 한 번만 쓴다 | 그 앞의 `W4 round failed` 줄의 이유 |
| `W4 pages are read again after a restart` | 위 줄 뒤 W4가 쪽을 다시 읽었다(상태가 바뀔 때 한 줄) | — |
| `error: W4 sweep not complete after 50 rounds; …` | 훑기 하나가 50라운드에 끝나지 않았다(기본 500쪽, 50,000행). 처음부터 다시 읽는다 | 거래소의 체결량, `w4_max_pages` |

## 실행의 지표(U19a7)

| 이름 | 뜻 | 볼 것 |
|---|---|---|
| `watcher_up` | 밀어 넣을 때마다 1 | 없으면 워처나 밀어 넣기가 없다 |
| `watcher_heartbeat_ok_ts` | OMS가 마지막으로 하트비트를 기록한 하트비트 시각(epoch 초) | 멈추면 `monitor.sock` 줄 |
| `watcher_check_age_s{check}` | 검사의 마지막 성공 뒤의 초. 시작 중에는 설정을 읽은 때부터(네 검사 모두). W4는 끝난 훑기에서만 성공하므로 여러 라운드에 걸친 훑기 동안 오른다. W3은 `wallet-balance`를 읽지 못하면 오른다 | W1·W2가 10초를 넘으면 OMS 게이트가 닫힌다. W3·W4는 게이트가 판단하지 않는다(기록만) |
| `watcher_mismatch_active{check}` | 라운드가 아직 보는 확정된 차이와 아직 OMS에 빚진 것 | 런북 7.13 |
| `watcher_mismatch_active{check="W3"}` | W3의 위반(`account_mm_rate`, `daily_loss`)이 확정되어 아직 보이거나 빚진 것. 많아야 2(정체는 종류뿐) | 런북 7.13의 W3 순서. OMS의 `core.margin`(감소 전용인지)과 함께 본다 |
| `watcher_requests_total{endpoint}` | 경로마다 보낸 요청(쪽마다 하나) | 대조 예산은 초당 2.7(설계). 증가율이 그보다 크게 오래 높으면 W1의 쪽(열린 주문 수)이나 W4의 쪽(체결량)을 본다. 5초에 체결 100개면 약 3.2~3.3이 정상이다(C-WATCH 9부) |
| `watcher_journal_seq`, `watcher_journal_error` | 마지막으로 읽은 기록, 읽기 실패 | 런북 7.17 |
