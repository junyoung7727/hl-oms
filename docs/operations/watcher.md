# 워처 운영

설계: [ADR-031](../adr/ADR-031-watcher.md). 계약: [C-WATCH](../contracts/watcher.md). 이 문서는 워처의 설정과 자격 파일을 적는다(U19a4). **워처는 아직 실행되지 않는다**: 바이너리는 `--version`에만 답하고, 시작 확인(U19a4b), 검사(U19a5~), 실행과 상황별 의미(U19a7)는 아직 없다. 서비스 설치와 알림 규칙은 U20b다.

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
