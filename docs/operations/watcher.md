# 워처 운영

주인: 준영
검토일: 2026-11-24

설계: [ADR-031](../adr/ADR-031-watcher.md). 계약: C-WATCH. 이 문서는 워처의 설정과 자격 파일(U19a4), 시작 확인과 그 거부, 시작 확인이 내보내는 지표(U19a4b), 실행과 상황별 의미(U19a7), 직접 알림의 토큰 파일(U19b1a, [ADR-032](../adr/ADR-032-watcher-kill-and-alerts.md))을 적는다. U19a7부터 `watcher --config`가 돈다: W1(열린 주문)과 W2(포지션), U19a8부터 W4(체결), U19a9부터 W3(지갑과 증거금)을 검사하고, 확정된 차이를 `admin.sock`에 보내고, `monitor.sock`에 하트비트를 보낸다. loopback 가짜로만 시험했고 실제 거래소에서 돌려 본 적이 없다. 서비스 설치와 알림 규칙은 U20b다.

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

[kill]                                            # mainnet은 필수(U19b3a), testnet은 없으면 warn 한 줄(W5 꺼짐)
marker = "/var/lib/hl-oms/kill.marker"            # OMS 설정의 [kill] marker와 같은 파일(data_dir 밖, 디렉터리가 있어야 함)
oms_unit = "hl-oms.service"                       # 워처가 멈출 수 있는 단위 하나
# systemd = "system"                              # system | user(systemctl --user)
# stop_wait_ms = 10000
# round_ms = 1000  watch_ms = 30000  watch_every_ms = 5000  notice_ms = 60000

# [w5]         probe_ms = 1000  silence_ms = 10000  # [kill]이 있을 때만 돈다
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
| `[kill]` | 표 없음 | 워처 자신의 일괄 취소 확인 루프(C-WATCH 11부). 표가 있으면 키가 취소할 수 있어야 한다(아래 "워처 키"). U19b2a의 필드: `round_ms` 1,000(1,000~10,000), `watch_ms` 30,000(10,000~300,000), `watch_every_ms` 5,000(1,000~`watch_ms`), `notice_ms` 60,000(60,000~600,000); 기본값은 표 7-8의 설계 값, 범위는 판단(모델 아님). 모르는 키는 거부. U19b3a의 필드(C-WATCH 12부): `marker`(필수; OMS의 `[kill] marker` 규칙: 절대 경로, `..` 없음, `data_dir` 밖, 디렉터리가 있음), `oms_unit`(필수; `[A-Za-z0-9:_.@-]` 1~255자, `.service`로 끝남, `-`로 시작하지 않음), `systemd` `"system"`(`"system"`, `"user"`), `stop_wait_ms` 10,000(1,000~120,000, 판단(모델 아님)). **mainnet은 `[kill]` 없이 시작하지 않는다**(`mainnet requires [kill]: without it the watcher cannot stop the OMS or cancel its orders`). testnet은 없으면 `warn: no [kill]: W5 is off; …` 한 줄: 그 워처는 OMS가 조용해도 멈추지도 알리지도 않는다(OMS 자신의 모니터링 게이트와 관측 스택만이 신호). `marker`와 `oms_unit` 없는 U19b2a의 `[kill]`은 `missing field`로 거부된다. U19b3a의 워처는 아직 W5를 돌리지 않고 일괄 취소를 보내지 않는다. 킬 때 워처는 `marker` 경로에 파일 하나(모드 0600, 이미 있는 것은 건드리지 않음)와 같은 디렉터리의 임시 파일 `.<이름>.watcher-<pid>.tmp`를 쓴다(U19b3b, C-WATCH 12부 "킬 마커"): 그 디렉터리는 워처의 uid가 쓸 수 있어야 한다. U19b3b의 킬 순서도 아직 실행이 돌리지 않는다 |
| `[w5] probe_ms` | 1000(설계 §7.5) | 500~5000. `[kill]`이 있을 때만 쓴다 |
| `[w5] silence_ms` | 10,000(설계 §7.5) | 3000~60,000, 그리고 `[kill]`이 있으면 `3 × probe_ms + 3 × request_timeout_ms + 1000` 이상(판단(모델 아님): 탐침 하나를 잃거나 워처가 틈 한계보다 짧게 멈춰도 쏘지 않게; 기본값이 한계에 맞으므로 `probe_ms`나 `request_timeout_ms`를 올리면 `silence_ms`도 올려야 한다; 거부 줄 `[w5] silence_ms must be at least …`) |

기본값의 근거(설계 §7.5, 스코프, OMS의 값)와 거부의 이유는 C-WATCH 4부에 있다.

`[w3]`의 두 한도는 OMS 설정의 `account_mm_rate_max_bps`·`daily_loss_bps`(C-MARGIN)와 **같은 값으로 둔다**. 워처는 OMS 설정을 읽지 않는다(독립). 값이 다르면 W3과 OMS의 감소 전용 판단이 어긋난다(U19a9, C-WATCH 9부).

## 자격 파일

```sh
install -m 600 /dev/null /etc/hl-oms/watcher-bybit.toml   # 워처를 돌릴 사용자로
$EDITOR /etc/hl-oms/watcher-bybit.toml
```

두 형 가운데 정확히 하나를 쓴다([ADR-034](../adr/ADR-034-bybit-rsa-keys.md)). OMS의 Bybit 자격 파일과 모양이 같다([운영 절차 §2](runbook.md)).

```toml
# HMAC 키: Bybit가 만든 키(System-generated)
api_key = "..."
api_secret = "..."
```

```toml
# RSA 키: 직접 만든 키(Self-generated). openssl이 쓴 PEM 글을 머리 줄과 꼬리 줄까지 그대로
api_key = "..."
rsa_private_key = """
(BEGIN PRIVATE KEY 줄부터 END PRIVATE KEY 줄까지)
"""
```

RSA 키 만들기와 등록(Bybit testnet의 AI 하위 계정은 이 형만 준다):

```sh
umask 077
openssl genpkey -algorithm RSA -pkeyopt rsa_keygen_bits:2048 -out bybit-rsa.pem   # 개인 키, PKCS#8
openssl pkey -in bybit-rsa.pem -pubout -out bybit-rsa.pub.pem                       # 공개 키
```

1. Bybit: API management → Create New Key → Self-generated API Keys에서 `bybit-rsa.pub.pem`의 공개 키를 넣고, 권한(`[kill]`이면 `ContractTrade`의 `Order`, 출금 없음)과 이 호스트의 공인 IP를 묶는다. Bybit가 보여 주는 API 키(`api_key`)를 적어 둔다. 화면의 이름은 스펙(2026-11-25)에서 왔고 이 문서가 확인하지 않았다.
2. 자격 파일을 만든다(API 키는 비밀이 아니다. 개인 키는 이 파일에만 둔다):

```sh
install -m 600 /dev/null /etc/hl-oms/watcher-bybit.toml
{ printf 'api_key = "%s"\nrsa_private_key = """\n' '<API 키>'; cat bybit-rsa.pem; printf '"""\n'; } > /etc/hl-oms/watcher-bybit.toml
```

3. 워처가 이 파일로 시작하는 것을 본 뒤 개인 키의 원본을 지운다. 남기면 소유자·위치 검사를 받지 않는 두 번째 비밀 파일이 된다(검토 S3). 키를 잃으면 새로 만들어 등록하면 되므로 사본을 두지 않는다. 공개 키 `bybit-rsa.pub.pem`은 비밀이 아니라 남겨도 된다.

```sh
shred -u bybit-rsa.pem   # 또는 rm bybit-rsa.pem (SSD와 저널링 파일 시스템에서는 shred도 덮어쓰기를 보장하지 않는다)
```

- `api_key`와, `api_secret` 또는 `rsa_private_key` 중 하나만 둔다. 다른 키가 있거나, 둘 다 있거나, 둘 다 없거나, 값이 비면 시작하지 않는다.
- RSA 키는 `BEGIN PRIVATE KEY` 줄의 PKCS#8이어야 한다(위 `genpkey`가 쓰는 형). 시작할 때 한 번 읽어 파싱하고, 거부는 종료 코드 2와 아래 한 줄이다.

| 오류 줄(경로 뒤) | 뜻 | 할 일 |
|---|---|---|
| `has both api_secret and rsa_private_key; keep exactly one` | 두 비밀이 다 있다 | 쓰지 않는 쪽을 지운다 |
| `is not TOML with api_key and api_secret or rsa_private_key: it has neither` | 비밀이 없다 | 하나를 넣는다 |
| `api_secret holds a PEM private key; an RSA key goes in rsa_private_key` | RSA 키를 `api_secret` 자리에 넣었다 | 키 이름을 `rsa_private_key`로 바꾼다 |
| `rsa_private_key is a PKCS#1 key (BEGIN RSA PRIVATE KEY); convert it to PKCS#8 with: openssl pkcs8 -topk8 -nocrypt -in old.pem -out new.pem` | 옛 형(PKCS#1) | 적힌 명령으로 바꿔 다시 넣는다 |
| `rsa_private_key is encrypted (BEGIN ENCRYPTED PRIVATE KEY); write it unencrypted with: openssl pkcs8 -topk8 -nocrypt -in old.pem -out new.pem` | 암호가 걸린 키(무인 실행은 암호를 물을 수 없다) | 적힌 명령(암호를 묻는다)으로 풀어 다시 넣는다 |
| `rsa_private_key is not a PEM private key (BEGIN PRIVATE KEY, PKCS#8)` | 머리·꼬리 줄이 없거나 본문이 base64가 아니다 | 파일을 `genpkey`의 출력 그대로 다시 넣는다 |
| `rsa_private_key is not an RSA key of 2048 bits or more that ring accepts (<이유>)` | 2048비트 미만, RSA가 아닌 키, 깨진 본문 | 2048비트 RSA 키를 새로 만들어 등록한다 |
- 워처를 돌리는 사용자가 소유하고 권한은 `0600` 또는 `0400`이어야 한다. 심볼릭 링크는 받지 않는다(링크가 아닌 파일 경로를 적는다). OMS 데이터 디렉터리 안에 두지 않는다.
- 오류 줄은 파일 내용을 인용하지 않는다(RSA 키의 줄도 키의 바이트를 담지 않는다). 그래도 저장소, 로그, 메시지에 이 파일을 붙이지 않는다.

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
- 워처는 U19b1c부터 `[WATCH P1]`을 보낸다: 확정된 차이, 거절된 키, 키의 목록 밖 공인 IP, 시작 거부(뜻과 할 일은 런북 7.17). 시작 확인 전에 송신기가 뜨므로 시작 2단계의 키 거부도 온다. 워처는 `api.telegram.org`에 DNS로, 안 되면 `149.154.167.220:443`으로 바로 간다(U19b1b): 방화벽은 둘 다 열어 둔다.
- 토큰이 폐기되었거나(401·404) 봇이 채팅에서 빠지면(403) 워처는 같은 첫 부분을 5초마다 다시 보내고 뒤의 P1은 쌓였다가 50개를 넘으면 버려진다. `watcher_alert_failures_total`이 오르는데 `watcher_alerts_delivered_total`이 그대로면 이 경우다: 토큰 파일과 채팅의 봇을 확인하고 워처를 다시 시작한다(C-WATCH 10부 알려진 한계).

## 워처 키

- **OMS의 키와 다른 워처 전용 키**를 같은 하위 계정(`sub_account_uid`)에 만든다(설계 §7.4).
- **읽기·쓰기 키**(`readOnly` 0)로 만들고 **`ContractTrade`의 `Order` 권한**을 준다: 워처는 자기 키로 일괄 취소(`POST /v5/order/cancel-all`)를 보낸다(C-WATCH 11부). 워처가 보내는 서명된 POST는 이것 하나이고 주문을 만들거나 고치는 길은 없다. `Order`가 일괄 취소를 허락한다는 것은 권한 이름에서 한 추론이다(testnet에서 확인할 것).
- **출금 권한을 주지 않는다**(설계 §7.4). `Withdraw`는 마스터 전용이라 하위 계정 키에는 없어야 한다.
- `[kill]`이 있으면 시작 2단계와 60초마다의 재확인이 세 가지를 본다(U19b2a): `readOnly`가 0이 아니면, `ContractTrade`에 `Order`가 없으면, 어느 권한 목록에든 `Withdraw`가 있으면 시작하지 않는다(답의 다른 목록을 읽을 수 없어도; 종료 2, 줄은 `query-api: the API key is read-only …`, `… has no ContractTrade Order permission …`, `… has the Withdraw permission (<목록>) …`). 재확인에서 맞지 않으면 재시작까지 모든 Bybit 요청과 일괄 취소가 멈춘다(`watcher_auth_rejected` 1, `[WATCH P1] 워처 키가 시작 규칙을 더는 지키지 않음`). `[kill]`이 없으면(testnet) 이것을 보지 않는다: 그 워처는 조회만 하므로 읽기 전용 키도 되고, 출금 권한은 키를 만들 때 사람이 확인한다.
- 키를 IP에 묶는다(설계 §7.4). 묶지 않은 키는 시작 확인이 거부한다(U19a4b).

## 시작 확인과 거부(U19a4b)

설정과 키 파일을 읽은 뒤 워처는 차례로 묻는다. 거부는 종료 코드 2와 이유 한 줄이다(U19a7부터 stderr). 거부 뒤에는 아무 요청도 보내지 않는다.

| 단계 | 묻는 것 | 거부 줄(예) | 고칠 것 |
|---|---|---|---|
| 키 | `GET /v5/user/query-api` | `query-api: the API key is the master account's; the watcher needs a sub-account key` | 하위 계정의 워처 전용 키를 쓴다 |
| 키 | 〃 | `query-api: the API key belongs to user 4243; the config says sub_account_uid 4242` | `sub_account_uid`를 OMS와 같은 하위 계정으로, 또는 그 계정의 키로 |
| 키 | 〃 | `query-api: the API key is bound to no IP address (ips [] or "*"); bind it to this host's address` | 키를 이 호스트의 공인 IP에 묶는다 |
| 키 | 〃 | `query-api: the API key's ips hold no IP address` | 〃 |
| 키(`[kill]`이 있을 때, U19b2a) | 〃 | `query-api: the API key is read-only (readOnly 1); with [kill] the watcher cancels orders and needs a read-write key` | 읽기·쓰기 키로 만든다 |
| 키(〃) | 〃 | `query-api: the API key has no ContractTrade Order permission; with [kill] the watcher needs it to cancel orders` | 키에 `ContractTrade`의 `Order`를 준다 |
| 키(〃) | 〃 | `query-api: the API key has the Withdraw permission (Wallet); the watcher's key must not be able to withdraw` | 출금 권한이 없는 새 키를 만든다 |
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
- **시작 순서**: OMS를 먼저 띄우고 `/api/status`가 답할 때까지 기다린 뒤 워처를 띄운다. 워처가 먼저 뜨거나, 사람이 띄운 OMS가 아직 부팅 중일 때 워처를 다시 띄우거나, `status_url`이 틀리면 워처는 `[kill]`이 있을 때 `silence_ms` 뒤 건강하거나 부팅 중인 OMS를 킬한다(런북 7.18). OMS를 일부러 멈출 때는 워처를 먼저 멈춘다.
- **systemd(U20b)**: 워처가 할 수 있어야 하는 것은 `[kill] oms_unit` 하나의 `stop`뿐이다(polkit이나 사용자 단위로 U20b가 준다; 그 전에는 K3가 실패하고 알림이 `OMS 정지 실패`·`OMS 정지 미확인`이라고 말한다). 워처 단위는 종료 코드 2에 다시 띄우지 않는다(`RestartPreventExitStatus=2`): 시작 거부의 P1이 재시작마다 오지 않게.
- 킬 중의 종료(U19b3d): 도는 킬을 먼저 중단 알림 하나로 끝낸다(K1 중이면 그 결과나 기한까지, 최대 2초). `systemctl` 클라이언트는 죽이지 않고 마커는 남긴다. 포지션을 읽던 알림과 기다리던 알림은 `포지션 조회 안 함(워처 종료 중)`과 함께 바로 간다(킬이 막 끝나 마지막 알림이 읽기를 기다릴 때도). 그 뒤 아래와 같고, 송신기를 5초까지 비우고, 실행기를 `shutdown_timeout(1초)`로 끝낸다(디스크에 걸린 마커 쓰기가 종료를 잡지 않게).
- 종료: SIGTERM이나 SIGINT. 새 라운드 없이 진행 중 요청을 `request_timeout_ms`까지 기다리고, 지표를 한 번 더 밀어 넣고, 모니터 연결을 닫고 코드 0으로 끝난다. 아무것도 저장하지 않는다(다음 시작이 처음부터 다시 읽는다).
- 종료 코드 2: 사용법, 설정 파일, 키 파일, 시작 규칙(위의 표). 이유는 stderr 한 줄(`watcher: …` 또는 `watcher: error: …`).
- 로그는 stderr의 한 줄씩이다(`watcher: ` 뒤). `error:`와 `warn:`으로 시작하는 줄이 운영자가 볼 것이다.

## 상황별 의미(U19a7)

| 로그 줄(앞부분) | 뜻 | 할 일 |
|---|---|---|
| `warn: W5: the OMS answers again and the last kill owes the halt: watcher_kill` | 래치도 확인된 멈춤도 없이 끝난 킬 뒤 OMS가 다시 답해 빚진 정지를 보낸다(U19b3d) | 다음 줄 |
| `W5: the owed watcher_kill latched the OMS: …` / `error: W5: the owed watcher_kill did not latch the OMS: …` | 빚진 정지의 결과(`늦은 정지` / `늦은 정지 실패` P1이 함께) | 실패면 런북 7.18의 7번 |
| `the start is refused while the halt is owed: watcher_kill first` | 다른 킬 마커 경로를 읽는 OMS로 시작을 거부하기 전에 정지를 보낸다(U19b3d) | 거부 P1의 `늦은 정지: …`, 런북 7.17 |
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

## W5와 킬의 지표(U19b3c)

실행이 틱마다 W5와 킬의 접근자에서 적는다(C-WATCH 12부). 라벨은 다른 지표처럼 `venue="bybit"`, `host`, `watcher`.

| 이름 | 뜻 | 볼 것 |
|---|---|---|
| `watcher_w5_armed` | 1이면 W5가 OMS를 지킨다. `[kill]`이 없거나, 시작 확인이 끝나기 전, 킬 동안, 킬 뒤 첫 정상 응답까지는 0 | 시작 뒤 0에 머물면 시작 확인이 다시 묻는 중이거나(C-WATCH 12부 알려진 한계) 킬 뒤 OMS가 아직 답하지 않는다 |
| `watcher_oms_silence_s` | 지키는 동안 마지막 정상 `/api/status` 답 뒤의 초. 지키지 않으면 없다 | `silence_ms`(기본 10초)에 다가가면 킬이 가깝다 |
| `watcher_kill_active` | 1이면 킬 순서나 그 루프가 돈다 | 1인 동안 `clear-hold --reason kill` 하지 않는다(런북 7.18) |
| `watcher_kills_total` | 시작한 킬의 수 | 오르면 OMS가 10초 넘게 답하지 않았다 |
| `watcher_kill_step{step}` | 마지막 킬의 `marker`, `halt`, `stop` 단계의 결과(1 됨, 0 실패). `halt`는 K2 자신의 결과라 루프 중 재시도가 래치해도 0으로 남는다. 정지가 래치했으면 `stop`은 없다. 첫 킬 전과 단계가 끝나기 전에는 없다 | `halt` 0이고 `stop` 0이면 OMS가 멈추지 않았을 수 있다(시작 알림의 `OMS 정지 실패`) |
| `watcher_kill_round` | 지금이나 마지막 루프의 회차 | 오래 오르면 미체결이 0이 되지 않는다 |
| `watcher_kill_remaining` | 지금 루프가 마지막으로 센 미체결 수. 세지 못한 동안은 없다 | 줄지 않으면 누군가 주문을 내고 있다 |
| `watcher_kill_cancel_all_total` | 루프가 낸 일괄 취소 요청 수(보내지 못한 것도 셈, 알림의 `일괄 취소 {k}회`와 같음) | 1초에 1을 넘게 오르지 않는다 |
| `watcher_kill_halt_owed` | 1이면 정지를 빚졌다: 도는 킬이 래치도 마커 있는 멈춤도 확인하지 못했거나(루프 중 5초마다 `watcher_kill`을 다시 보냄), 끝난 킬 뒤 W5가 `Rearming {halt_owed: true}`에 있다 | 1이고 OMS가 답하면 워처가 `watcher_kill`을 보낸다(루프 동안 5초마다, 루프 뒤 첫 정상 답에, U19b3d). `늦은 정지 실패` P1이 오면 OMS가 래치 없이 거래한다: 손으로 멈춘다(런북 7.18) |
