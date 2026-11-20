//! The signed Bybit v5 REST client and its error classes (ADR-028).

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use serde_json::Value;

use super::sign;

/// The API key and secret, from a file outside the repository. `Debug`
/// never prints the secret.
pub struct Credentials {
    pub api_key: String,
    api_secret: String,
}

impl std::fmt::Debug for Credentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Credentials {{ api_key: {:?}, api_secret: <hidden> }}",
            self.api_key
        )
    }
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct CredentialsFile {
    api_key: String,
    api_secret: String,
}

impl Credentials {
    pub fn new(api_key: &str, api_secret: &str) -> Credentials {
        Credentials {
            api_key: api_key.to_string(),
            api_secret: api_secret.to_string(),
        }
    }

    /// Reads `api_key` and `api_secret` (TOML). The file must be a regular
    /// file owned by this process's user and readable and writable by its
    /// owner only (mode 0600 or 0400). The checks and the read use one
    /// open handle, so the file cannot be swapped between them.
    pub fn load(path: &Path) -> Result<Credentials, String> {
        use std::io::Read;
        use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
        let err = |e: std::io::Error| format!("credentials file {}: {e}", path.display());
        let mut file = std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(rustix::fs::OFlags::NOFOLLOW.bits() as i32)
            .open(path)
            .map_err(err)?;
        let meta = file.metadata().map_err(err)?;
        if !meta.file_type().is_file() {
            return Err(format!(
                "credentials file {} is not a regular file",
                path.display()
            ));
        }
        if meta.uid() != rustix::process::geteuid().as_raw() {
            return Err(format!(
                "credentials file {} must be owned by the user the OMS runs as",
                path.display()
            ));
        }
        if meta.mode() & 0o077 != 0 {
            return Err(format!(
                "credentials file {} must not be readable by group or others (chmod 600)",
                path.display()
            ));
        }
        let mut text = String::new();
        file.read_to_string(&mut text).map_err(err)?;
        let f: CredentialsFile = toml::from_str(&text).map_err(|_| {
            format!(
                "credentials file {} is not api_key and api_secret",
                path.display()
            )
        })?;
        if f.api_key.is_empty() || f.api_secret.is_empty() {
            return Err(format!(
                "credentials file {}: empty key or secret",
                path.display()
            ));
        }
        Ok(Credentials::new(&f.api_key, &f.api_secret))
    }

    pub fn ws_auth_signature(&self, expires_ms: u64) -> String {
        sign::ws_auth_signature(&self.api_secret, expires_ms)
    }
}

/// retCodes that mean the key, its signature, its permissions or its
/// bound IPs are not accepted (F20 Q3). Retrying does not change them.
/// The scope named 10003, 10010 and 33004; 10004 (signature), 10005
/// (permission) and 10007 (authentication failed) do not heal either.
pub const AUTH_CODES: [i64; 6] = [10003, 10004, 10005, 10007, 10010, 33004];

/// The local address of every Bybit and IP-echo connection: IPv4 only, so
/// the address the echo service sees is the address Bybit sees (a
/// dual-stack host could otherwise reach one over IPv6 and the other over
/// IPv4).
pub const LOCAL_ADDRESS: std::net::IpAddr = std::net::IpAddr::V4(std::net::Ipv4Addr::UNSPECIFIED);

/// How long no request may go out after HTTP 403 (F18 L186: "wait for at
/// least 10 minutes").
pub const IP_BAN_PAUSE: Duration = Duration::from_secs(600);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RestError {
    /// The connection was not established: no byte of the request left.
    NotSent(String),
    /// Sent, but no usable answer (timeout, HTTP 5xx, unreadable body,
    /// retCode 10016).
    NoAnswer(String),
    /// The key is not accepted (`AUTH_CODES`, HTTP 401). Never retried.
    Auth { code: i64, msg: String },
    /// retCode 10006 or HTTP 429.
    RateLimited(String),
    /// HTTP 403 "access too frequent": the venue bans this IP. Every HTTP
    /// session must stop for at least `IP_BAN_PAUSE` (F18 L186).
    IpBanned(String),
    /// retCode 10002: our timestamp is outside the venue's window.
    Clock(String),
    /// Any other non-zero retCode.
    Refused { code: i64, msg: String },
}

impl std::fmt::Display for RestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RestError::NotSent(e) => write!(f, "not sent: {e}"),
            RestError::NoAnswer(e) => write!(f, "no answer: {e}"),
            RestError::Auth { code, msg } => write!(f, "key rejected ({code}): {msg}"),
            RestError::RateLimited(e) => write!(f, "rate limited: {e}"),
            RestError::IpBanned(e) => write!(f, "IP banned for too many requests: {e}"),
            RestError::Clock(e) => write!(f, "timestamp outside the venue's window: {e}"),
            RestError::Refused { code, msg } => write!(f, "refused ({code}): {msg}"),
        }
    }
}

fn error_chain(e: &dyn std::error::Error) -> String {
    let mut text = e.to_string();
    let mut cause = e.source();
    while let Some(c) = cause {
        text.push_str(": ");
        text.push_str(&c.to_string());
        cause = c.source();
    }
    text
}

/// Classifies an answer: the `result` of retCode 0, or the error.
pub fn classify(status: u16, body: &str) -> Result<Value, RestError> {
    let short: String = body.chars().take(200).collect();
    match status {
        200..=299 => {}
        401 => {
            return Err(RestError::Auth {
                code: 401,
                msg: short,
            });
        }
        403 => return Err(RestError::IpBanned(format!("HTTP 403: {short}"))),
        429 => return Err(RestError::RateLimited(format!("HTTP 429: {short}"))),
        _ => return Err(RestError::NoAnswer(format!("HTTP {status}: {short}"))),
    }
    let v: Value = serde_json::from_str(body)
        .map_err(|e| RestError::NoAnswer(format!("unreadable body: {e}")))?;
    let code = v
        .get("retCode")
        .and_then(Value::as_i64)
        .ok_or_else(|| RestError::NoAnswer(format!("no retCode: {short}")))?;
    let msg = v
        .get("retMsg")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    match code {
        0 => Ok(v.get("result").cloned().unwrap_or(Value::Null)),
        c if AUTH_CODES.contains(&c) => Err(RestError::Auth { code: c, msg }),
        10006 => Err(RestError::RateLimited(msg)),
        10002 => Err(RestError::Clock(msg)),
        10016 => Err(RestError::NoAnswer(format!("venue error 10016: {msg}"))),
        c => Err(RestError::Refused { code: c, msg }),
    }
}

/// A signed REST client.
#[derive(Clone)]
pub struct Rest {
    base: String,
    http: reqwest::Client,
    creds: Arc<Credentials>,
    recv_window_ms: u64,
    now_ms: fn() -> u64,
}

fn wall_ms() -> u64 {
    crate::util::wall_us() / 1000
}

impl Rest {
    pub fn new(
        base: &str,
        creds: Arc<Credentials>,
        recv_window_ms: u64,
        timeout: Duration,
    ) -> Result<Rest, String> {
        let http = reqwest::Client::builder()
            .local_address(LOCAL_ADDRESS)
            .connect_timeout(Duration::from_secs(3))
            .timeout(timeout)
            // A redirect would re-send a signed request. Nothing here follows one.
            .redirect(reqwest::redirect::Policy::none())
            .no_proxy()
            .build()
            .map_err(|e| e.to_string())?;
        Ok(Rest {
            base: base.trim_end_matches('/').to_string(),
            http,
            creds,
            recv_window_ms,
            now_ms: wall_ms,
        })
    }

    /// The same client with a fixed clock (tests).
    pub fn with_clock(mut self, now_ms: fn() -> u64) -> Rest {
        self.now_ms = now_ms;
        self
    }

    fn signed(&self, rb: reqwest::RequestBuilder, payload: &str) -> reqwest::RequestBuilder {
        let ts = (self.now_ms)();
        let sig = sign::rest_signature(
            &self.creds.api_secret,
            ts,
            &self.creds.api_key,
            self.recv_window_ms,
            payload,
        );
        rb.header("X-BAPI-API-KEY", &self.creds.api_key)
            .header("X-BAPI-TIMESTAMP", ts.to_string())
            .header("X-BAPI-RECV-WINDOW", self.recv_window_ms.to_string())
            .header("X-BAPI-SIGN", sig)
            .header("X-BAPI-SIGN-TYPE", "2")
    }

    async fn send(&self, rb: reqwest::RequestBuilder) -> Result<Value, RestError> {
        let resp = rb.send().await.map_err(|e| {
            let text = error_chain(&e);
            if e.is_connect() || e.is_builder() {
                RestError::NotSent(text)
            } else {
                RestError::NoAnswer(text)
            }
        })?;
        let status = resp.status().as_u16();
        let text = resp
            .text()
            .await
            .map_err(|e| RestError::NoAnswer(error_chain(&e)))?;
        classify(status, &text)
    }

    /// A signed GET; returns `result`.
    pub async fn get(&self, path: &str, params: &[(&str, String)]) -> Result<Value, RestError> {
        let q = sign::query_string(params).map_err(RestError::NotSent)?;
        let url = if q.is_empty() {
            format!("{}{path}", self.base)
        } else {
            format!("{}{path}?{q}", self.base)
        };
        self.send(self.signed(self.http.get(url), &q)).await
    }

    /// A signed POST of a JSON body; returns `result`. The signed string
    /// and the sent body are the same bytes.
    pub async fn post(&self, path: &str, body: &Value) -> Result<Value, RestError> {
        let text = serde_json::to_string(body).map_err(|e| RestError::NotSent(e.to_string()))?;
        let rb = self
            .http
            .post(format!("{}{path}", self.base))
            .header("Content-Type", "application/json")
            .body(text.clone());
        self.send(self.signed(rb, &text)).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use http_fake::{Answer, FakeHttp};

    fn ok(result: &str) -> Answer {
        Answer::Reply(
            200,
            format!(r#"{{"retCode":0,"retMsg":"OK","result":{result},"time":1}}"#),
        )
    }

    fn code(c: i64) -> Answer {
        Answer::Reply(
            200,
            format!(r#"{{"retCode":{c},"retMsg":"m{c}","result":{{}},"time":1}}"#),
        )
    }

    fn rest(fake: &FakeHttp) -> Rest {
        Rest::new(
            &fake.url(),
            Arc::new(Credentials::new("KEY", "SECRET")),
            5000,
            Duration::from_millis(500),
        )
        .unwrap()
        .with_clock(|| 1_700_000_000_000)
    }

    #[tokio::test]
    async fn a_get_is_signed_over_the_query_string_it_sends() {
        let fake = FakeHttp::start(ok(r#"{"list":[]}"#));
        let r = rest(&fake);
        let v = r
            .get(
                "/v5/position/list",
                &[("symbol", "BTCUSDT".into()), ("category", "linear".into())],
            )
            .await
            .unwrap();
        assert_eq!(v["list"], serde_json::json!([]));
        let req = &fake.requests()[0];
        assert_eq!(req.method, "GET");
        assert_eq!(
            req.target,
            "/v5/position/list?category=linear&symbol=BTCUSDT"
        );
        assert_eq!(req.header("x-bapi-api-key"), Some("KEY"));
        assert_eq!(req.header("x-bapi-timestamp"), Some("1700000000000"));
        assert_eq!(req.header("x-bapi-recv-window"), Some("5000"));
        assert_eq!(req.header("x-bapi-sign-type"), Some("2"));
        let want = sign::rest_signature(
            "SECRET",
            1_700_000_000_000,
            "KEY",
            5000,
            "category=linear&symbol=BTCUSDT",
        );
        assert_eq!(req.header("x-bapi-sign"), Some(want.as_str()));
    }

    #[tokio::test]
    async fn a_post_is_signed_over_the_exact_body_it_sends() {
        let fake = FakeHttp::start(ok(r#"{"list":[],"success":"1"}"#));
        let r = rest(&fake);
        r.post(
            "/v5/order/cancel-all",
            &serde_json::json!({"category": "linear", "settleCoin": "USDT"}),
        )
        .await
        .unwrap();
        let req = &fake.requests()[0];
        assert_eq!(req.method, "POST");
        let body = req.body_str();
        assert_eq!(body, r#"{"category":"linear","settleCoin":"USDT"}"#);
        let want = sign::rest_signature("SECRET", 1_700_000_000_000, "KEY", 5000, &body);
        assert_eq!(req.header("x-bapi-sign"), Some(want.as_str()));
    }

    #[tokio::test]
    async fn every_auth_code_is_a_rejected_key_after_one_request() {
        for c in AUTH_CODES {
            let fake = FakeHttp::start(code(c));
            let e = rest(&fake).get("/v5/account/info", &[]).await.unwrap_err();
            assert!(
                matches!(e, RestError::Auth { code, .. } if code == c),
                "{c}: {e:?}"
            );
            assert_eq!(fake.requests().len(), 1, "{c}: no retry");
        }
        let fake = FakeHttp::start(Answer::Reply(401, "{}".into()));
        assert!(matches!(
            rest(&fake).get("/v5/account/info", &[]).await,
            Err(RestError::Auth { code: 401, .. })
        ));
    }

    #[tokio::test]
    async fn other_answers_have_their_own_class() {
        let cases = [
            (code(10006), "rate"),
            (Answer::Reply(429, "".into()), "rate"),
            (Answer::Reply(403, "".into()), "banned"),
            (code(10002), "clock"),
            (code(10016), "noanswer"),
            (Answer::Reply(500, "x".into()), "noanswer"),
            (Answer::Reply(200, "not json".into()), "noanswer"),
            (Answer::Reply(200, "{}".into()), "noanswer"),
            (code(110001), "refused"),
        ];
        for (answer, want) in cases {
            let fake = FakeHttp::start(answer.clone());
            let e = rest(&fake)
                .get("/v5/order/realtime", &[])
                .await
                .unwrap_err();
            let got = match e {
                RestError::RateLimited(_) => "rate",
                RestError::IpBanned(_) => "banned",
                RestError::Clock(_) => "clock",
                RestError::NoAnswer(_) => "noanswer",
                RestError::Refused { code: 110001, .. } => "refused",
                _ => "other",
            };
            assert_eq!(got, want, "{answer:?}");
        }
        let dead = format!("http://{}", http_fake::dead_addr());
        let r = Rest::new(
            &dead,
            Arc::new(Credentials::new("K", "S")),
            5000,
            Duration::from_millis(300),
        )
        .unwrap();
        assert!(matches!(
            r.get("/v5/account/info", &[]).await,
            Err(RestError::NotSent(_))
        ));
    }

    #[test]
    fn the_credentials_file_must_be_owner_only_and_the_secret_never_prints() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("bybit-creds-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("creds.toml");
        std::fs::write(&p, "api_key = \"K1\"\napi_secret = \"S1\"\n").unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(Credentials::load(&p).unwrap_err().contains("chmod 600"));
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o600)).unwrap();
        let c = Credentials::load(&p).unwrap();
        assert_eq!(c.api_key, "K1");
        assert!(!format!("{c:?}").contains("S1"));
        let link = dir.join("link.toml");
        std::os::unix::fs::symlink(&p, &link).unwrap();
        assert!(
            Credentials::load(&link).is_err(),
            "a symlink to a good file is refused"
        );
        std::fs::write(&p, "api_key = \"K1\"\n").unwrap();
        assert!(Credentials::load(&p).is_err());
        assert!(
            Credentials::load(&dir)
                .unwrap_err()
                .contains("regular file")
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
