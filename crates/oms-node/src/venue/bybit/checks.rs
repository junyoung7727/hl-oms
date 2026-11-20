//! Read-only checks before the gateway may trade (ADR-028): the key, the
//! account, the position mode, the settle coin of every traded symbol, and
//! the public IP the requests leave from. A failed check blocks the
//! gateway; no check changes a venue setting.

use std::net::IpAddr;
use std::sync::Mutex;
use std::time::Duration;

use oms_core::{Px, Qty};
use serde_json::Value;

use super::rest::{Rest, RestError};

/// Why the gateway may not trade.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Block {
    /// The venue does not accept the key, or the requests leave from an IP
    /// the key is not bound to. Never retried by the gateway.
    AuthRejected(String),
    /// The account, the key's rights or a symbol is not what the
    /// configuration says.
    Account(String),
    /// A check could not be done (no answer, rate limit). Retried.
    Unchecked(String),
    /// The venue banned this IP for too many requests (HTTP 403). No
    /// request may go out for `rest::IP_BAN_PAUSE`; then retried.
    IpBanned(String),
}

impl std::fmt::Display for Block {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Block::AuthRejected(e) => write!(f, "auth_rejected: {e}"),
            Block::Account(e) => write!(f, "account: {e}"),
            Block::Unchecked(e) => write!(f, "unchecked: {e}"),
            Block::IpBanned(e) => write!(f, "ip_banned: {e}"),
        }
    }
}

impl From<RestError> for Block {
    fn from(e: RestError) -> Block {
        match e {
            RestError::Auth { .. } => Block::AuthRejected(e.to_string()),
            RestError::Refused { .. } => Block::Account(e.to_string()),
            RestError::IpBanned(_) => Block::IpBanned(e.to_string()),
            _ => Block::Unchecked(e.to_string()),
        }
    }
}

/// The gateway's block, shared by the boot checks, the IP loop and the
/// stream (which may report the link up only while `block` is `None`).
///
/// Each source owns its slot, so a passing IP recheck cannot lift a failed
/// boot check and a passing boot rerun cannot lift an IP mismatch. An auth
/// block from any source moves to its own slot and stays until the process
/// restarts: the gateway never retries a rejected key (ADR-028).
#[derive(Default)]
pub struct Health {
    slots: Mutex<Slots>,
}

#[derive(Default)]
struct Slots {
    auth: Option<Block>,
    ban: Option<Block>,
    boot: Option<Block>,
    ip: Option<Block>,
}

impl Health {
    /// The block that keeps the link down: auth first, then an IP ban,
    /// then boot, then IP.
    pub fn block(&self) -> Option<Block> {
        let g = self.slots.lock().unwrap_or_else(|e| e.into_inner());
        g.auth
            .clone()
            .or_else(|| g.ban.clone())
            .or_else(|| g.boot.clone())
            .or_else(|| g.ip.clone())
    }

    /// The key was rejected (by any request, or by the stream's auth).
    pub fn reject_auth(&self, why: String) {
        self.set(Some(Block::AuthRejected(why)), |s| &mut s.auth);
    }

    /// The pause after HTTP 403 began (`Some`) or ended (`None`). The
    /// gateway owns the pause; this slot only keeps the link down meanwhile.
    pub fn set_ban(&self, b: Option<Block>) {
        self.slots.lock().unwrap_or_else(|e| e.into_inner()).ban = b;
    }

    /// The key was rejected: no request may go out until the restart.
    pub fn auth_rejected(&self) -> bool {
        self.slots
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .auth
            .is_some()
    }

    /// The result of a boot check run.
    pub fn set_boot(&self, b: Option<Block>) {
        self.set(b, |s| &mut s.boot);
    }

    /// The result of an IP recheck.
    pub fn set_ip(&self, b: Option<Block>) {
        self.set(b, |s| &mut s.ip);
    }

    fn set(&self, b: Option<Block>, slot: fn(&mut Slots) -> &mut Option<Block>) {
        let mut g = self.slots.lock().unwrap_or_else(|e| e.into_inner());
        match b {
            Some(Block::AuthRejected(e)) => {
                if g.auth.is_none() {
                    g.auth = Some(Block::AuthRejected(e));
                }
            }
            other => *slot(&mut g) = other,
        }
    }
}

/// What the configuration expects of the account.
#[derive(Clone, Debug)]
pub struct Expected {
    /// The sub-account the key must belong to (`userID` of query-api).
    pub sub_account_uid: u64,
    /// The traded symbols (category linear).
    pub symbols: Vec<SymbolSpec>,
}

/// A traded symbol and the steps the core rounds its orders to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SymbolSpec {
    pub name: String,
    pub px_tick: Px,
    pub qty_step: Qty,
}

/// The settle coin every traded symbol must have: the kill scope of
/// cancel-all and the open-order queries (`category=linear`,
/// `settleCoin=USDT`).
pub const SETTLE_COIN: &str = "USDT";

fn field<'a>(v: &'a Value, name: &str, what: &str) -> Result<&'a Value, Block> {
    v.get(name)
        .ok_or_else(|| Block::Unchecked(format!("{what}: no field {name}")))
}

/// Whether the key's bound IPs hold `ip`. An empty list or `*` means the
/// key is bound to no IP: refused (F20 Q4: such a key also expires in 90
/// days). IPv4-mapped IPv6 addresses compare as IPv4.
pub fn ip_bound(ips: &[String], ip: IpAddr) -> Result<(), Block> {
    let ip = ip.to_canonical();
    if ips.is_empty() || ips.iter().any(|s| s.trim() == "*") {
        return Err(Block::Account(
            "the API key is bound to no IP address; bind it to this host's address".into(),
        ));
    }
    if ips
        .iter()
        .any(|s| s.trim().parse::<IpAddr>().ok().map(|a| a.to_canonical()) == Some(ip))
    {
        Ok(())
    } else {
        Err(Block::AuthRejected(format!(
            "requests leave from {ip}, the API key is bound to {}",
            ips.join(", ")
        )))
    }
}

/// The key's bound IPs, after checking the key (`GET /v5/user/query-api`).
pub async fn check_key(rest: &Rest, want: &Expected) -> Result<Vec<String>, Block> {
    let v = rest.get("/v5/user/query-api", &[]).await?;
    let what = "query-api";
    if field(&v, "isMaster", what)?.as_bool() != Some(false) {
        return Err(Block::Account(
            "the API key is the master account's; use a sub-account key".into(),
        ));
    }
    let uid = field(&v, "userID", what)?.as_u64();
    if uid != Some(want.sub_account_uid) {
        return Err(Block::Account(format!(
            "the API key belongs to user {uid:?}, the configuration says sub-account {}",
            want.sub_account_uid
        )));
    }
    if field(&v, "readOnly", what)?.as_i64() != Some(0) {
        return Err(Block::Account("the API key is read-only".into()));
    }
    let contract = v
        .pointer("/permissions/ContractTrade")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_str).collect::<Vec<_>>())
        .unwrap_or_default();
    for need in ["Order", "Position"] {
        if !contract.contains(&need) {
            return Err(Block::Account(format!(
                "the API key lacks the ContractTrade permission {need}"
            )));
        }
    }
    let ips = field(&v, "ips", what)?
        .as_array()
        .ok_or_else(|| Block::Unchecked("query-api: ips is not a list".into()))?
        .iter()
        .map(|x| x.as_str().map(str::to_string))
        .collect::<Option<Vec<String>>>()
        .ok_or_else(|| Block::Unchecked("query-api: ips holds a non-string".into()))?;
    Ok(ips)
}

/// Cross margin on a unified account (`GET /v5/account/info`).
pub async fn check_account(rest: &Rest) -> Result<(), Block> {
    let v = rest.get("/v5/account/info", &[]).await?;
    let what = "account/info";
    let mode = field(&v, "marginMode", what)?.as_str().unwrap_or("");
    if mode != "REGULAR_MARGIN" {
        return Err(Block::Account(format!(
            "the account's margin mode is {mode:?}; the OMS trades cross margin (REGULAR_MARGIN) only"
        )));
    }
    let status = field(&v, "unifiedMarginStatus", what)?
        .as_i64()
        .unwrap_or(0);
    if !(3..=6).contains(&status) {
        return Err(Block::Account(format!(
            "unifiedMarginStatus {status}: not a unified trading account"
        )));
    }
    Ok(())
}

/// One-way position mode, the settle coin, and the steps, per symbol. The
/// configured price tick and quantity step must be multiples of the
/// venue's (`priceFilter.tickSize`, `lotSizeFilter.qtyStep`): what the venue
/// does with an order off its steps is not documented.
pub async fn check_symbol(rest: &Rest, spec: &SymbolSpec) -> Result<(), Block> {
    let symbol = spec.name.as_str();
    let params = [
        ("category", "linear".to_string()),
        ("symbol", symbol.to_string()),
    ];
    let inst = rest.get("/v5/market/instruments-info", &params).await?;
    let row = inst
        .get("list")
        .and_then(Value::as_array)
        .and_then(|l| {
            l.iter()
                .find(|r| r.get("symbol").and_then(Value::as_str) == Some(symbol))
        })
        .ok_or_else(|| Block::Account(format!("{symbol} is not a linear instrument")))?;
    let settle = row.get("settleCoin").and_then(Value::as_str).unwrap_or("");
    if settle != SETTLE_COIN {
        return Err(Block::Account(format!(
            "{symbol} settles in {settle:?}; the kill scope is {SETTLE_COIN}"
        )));
    }
    let read = |ptr: &str| row.pointer(ptr).and_then(Value::as_str).unwrap_or("");
    let (tick, step) = (read("/priceFilter/tickSize"), read("/lotSizeFilter/qtyStep"));
    let (Ok(venue_tick), Ok(venue_step)) = (Px::parse(tick), Qty::parse(step)) else {
        return Err(Block::Unchecked(format!(
            "{symbol}: tickSize {tick:?} or qtyStep {step:?} cannot be read"
        )));
    };
    let multiple = |ours: i64, venue: i64| venue > 0 && ours % venue == 0;
    if !multiple(spec.px_tick.0, venue_tick.0) || !multiple(spec.qty_step.0, venue_step.0) {
        return Err(Block::Account(format!(
            "{symbol}: px_tick {} and qty_step {} must be multiples of the venue's tickSize {venue_tick} and qtyStep {venue_step}",
            spec.px_tick, spec.qty_step
        )));
    }
    let pos = rest.get("/v5/position/list", &params).await?;
    let rows = pos
        .get("list")
        .and_then(Value::as_array)
        .ok_or_else(|| Block::Unchecked(format!("position/list {symbol}: no list")))?;
    // With a symbol the venue returns a row even without a position
    // (F20 Q7.3); positionIdx 0 is one-way mode (F20 Q7.2).
    if rows.is_empty() {
        return Err(Block::Unchecked(format!(
            "position/list {symbol}: no row to read the position mode from"
        )));
    }
    for r in rows {
        if r.get("positionIdx").and_then(Value::as_i64) != Some(0) {
            return Err(Block::Account(format!(
                "{symbol} is in hedge mode (positionIdx {:?}); the OMS trades one-way mode only",
                r.get("positionIdx")
            )));
        }
    }
    Ok(())
}

/// The address the requests leave from, as an echo service sees it (the
/// body is the address).
pub async fn public_ip(http: &reqwest::Client, url: &str) -> Result<IpAddr, Block> {
    let resp = http
        .get(url)
        .send()
        .await
        .map_err(|e| Block::Unchecked(format!("IP echo {url}: {e}")))?;
    if !resp.status().is_success() {
        return Err(Block::Unchecked(format!(
            "IP echo {url}: HTTP {}",
            resp.status()
        )));
    }
    let text = resp
        .text()
        .await
        .map_err(|e| Block::Unchecked(format!("IP echo {url}: {e}")))?;
    text.trim().parse().map_err(|_| {
        Block::Unchecked(format!(
            "IP echo {url}: not an address: {:?}",
            text.chars().take(60).collect::<String>()
        ))
    })
}

/// An HTTP client for the echo service. IPv4 only, like the REST client
/// (`rest::LOCAL_ADDRESS`), so the echo sees the address Bybit sees.
pub fn echo_client(timeout: Duration) -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .local_address(super::rest::LOCAL_ADDRESS)
        .timeout(timeout)
        .redirect(reqwest::redirect::Policy::none())
        .no_proxy()
        .build()
        .map_err(|e| e.to_string())
}

/// Every boot check, in order. `Ok` only if all pass.
pub async fn boot(
    rest: &Rest,
    echo: &reqwest::Client,
    echo_url: &str,
    want: &Expected,
) -> Result<(), Block> {
    let ips = check_key(rest, want).await?;
    ip_bound(&ips, public_ip(echo, echo_url).await?)?;
    check_account(rest).await?;
    for s in &want.symbols {
        check_symbol(rest, s).await?;
    }
    Ok(())
}

/// One IP check of the loop: re-reads the key's IPs and the public IP.
pub async fn recheck_ip(
    rest: &Rest,
    echo: &reqwest::Client,
    echo_url: &str,
    want: &Expected,
) -> Result<(), Block> {
    let ips = check_key(rest, want).await?;
    ip_bound(&ips, public_ip(echo, echo_url).await?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::venue::bybit::rest::Credentials;
    use http_fake::{Answer, FakeHttp};
    use serde_json::json;
    use std::sync::Arc;

    fn ok(result: Value) -> Answer {
        Answer::Reply(
            200,
            serde_json::json!({"retCode": 0, "retMsg": "OK", "result": result, "time": 1})
                .to_string(),
        )
    }

    fn key(over: Value) -> Value {
        let mut v = serde_json::json!({
            "isMaster": false, "userID": 4242, "parentUid": "1000", "readOnly": 0,
            "ips": ["127.0.0.1", "203.0.113.7"],
            "permissions": {"ContractTrade": ["Order", "Position"], "Spot": []}
        });
        for (k, x) in over.as_object().unwrap() {
            v[k] = x.clone();
        }
        v
    }

    fn account(mode: &str, status: i64) -> Value {
        serde_json::json!({"marginMode": mode, "unifiedMarginStatus": status, "updatedTime": "1"})
    }

    fn instrument(symbol: &str, settle: &str) -> Value {
        serde_json::json!({"category": "linear", "list": [{"symbol": symbol, "settleCoin": settle, "status": "Trading",
            "priceFilter": {"tickSize": "0.10"}, "lotSizeFilter": {"qtyStep": "0.001"}}], "nextPageCursor": ""})
    }

    fn positions(idx: &[i64]) -> Value {
        let list: Vec<Value> = idx
            .iter()
            .map(|i| serde_json::json!({"symbol": "BTCUSDT", "positionIdx": i, "size": "0"}))
            .collect();
        serde_json::json!({"category": "linear", "list": list, "nextPageCursor": ""})
    }

    struct Venue {
        api: FakeHttp,
        echo: FakeHttp,
    }

    fn venue(key_v: Value, acct: Value, inst: Value, pos: Value, ip: &str) -> Venue {
        let api = FakeHttp::start(Answer::Reply(404, "{}".into()));
        api.route("/v5/user/query-api", [ok(key_v)]);
        api.route("/v5/account/info", [ok(acct)]);
        api.route("/v5/market/instruments-info", [ok(inst)]);
        api.route("/v5/position/list", [ok(pos)]);
        let echo = FakeHttp::start(Answer::Reply(200, format!("{ip}\n")));
        Venue { api, echo }
    }

    fn good() -> Venue {
        venue(
            key(json!({})),
            account("REGULAR_MARGIN", 5),
            instrument("BTCUSDT", "USDT"),
            positions(&[0]),
            "127.0.0.1",
        )
    }

    fn spec(tick: &str, step: &str) -> SymbolSpec {
        SymbolSpec {
            name: "BTCUSDT".into(),
            px_tick: Px::parse(tick).unwrap(),
            qty_step: Qty::parse(step).unwrap(),
        }
    }

    fn want() -> Expected {
        Expected {
            sub_account_uid: 4242,
            symbols: vec![spec("0.5", "0.001")],
        }
    }

    async fn run(v: &Venue) -> Result<(), Block> {
        run_with(v, &want()).await
    }

    async fn run_with(v: &Venue, want: &Expected) -> Result<(), Block> {
        let rest = Rest::new(
            &v.api.url(),
            Arc::new(Credentials::new("K", "S")),
            5000,
            Duration::from_millis(500),
        )
        .unwrap();
        let echo = echo_client(Duration::from_millis(500)).unwrap();
        boot(&rest, &echo, &v.echo.url(), want).await
    }

    #[tokio::test]
    async fn steps_off_the_venue_steps_block_and_unreadable_steps_are_unchecked() {
        for (tick, step, ok) in [("0.1", "0.001", true), ("1", "0.01", true), ("0.05", "0.001", false), ("0.5", "0.0005", false)] {
            let v = good();
            let want = Expected { symbols: vec![spec(tick, step)], ..want() };
            let r = run_with(&v, &want).await;
            if ok {
                assert_eq!(r, Ok(()), "{tick} {step}");
            } else {
                assert!(matches!(&r, Err(Block::Account(e)) if e.contains("multiples")), "{tick} {step}: {r:?}");
            }
        }
        let bare = serde_json::json!({"category": "linear", "list": [{"symbol": "BTCUSDT", "settleCoin": "USDT", "status": "Trading"}], "nextPageCursor": ""});
        let v = venue(key(json!({})), account("REGULAR_MARGIN", 5), bare, positions(&[0]), "127.0.0.1");
        assert!(matches!(run(&v).await, Err(Block::Unchecked(e)) if e.contains("cannot be read")));
    }

    #[tokio::test]
    async fn the_boot_checks_pass_on_the_configured_account() {
        let v = good();
        run(&v).await.unwrap();
        let paths: Vec<String> = v.api.requests().iter().map(|r| r.target.clone()).collect();
        assert_eq!(
            paths,
            vec![
                "/v5/user/query-api",
                "/v5/account/info",
                "/v5/market/instruments-info?category=linear&symbol=BTCUSDT",
                "/v5/position/list?category=linear&symbol=BTCUSDT",
            ]
        );
        assert!(
            v.api.requests().iter().all(|r| r.method == "GET"),
            "read-only"
        );
    }

    #[tokio::test]
    async fn each_wrong_account_fact_blocks() {
        let cases: Vec<(Venue, &str)> = vec![
            (
                venue(
                    key(json!({"isMaster": true})),
                    account("REGULAR_MARGIN", 5),
                    instrument("BTCUSDT", "USDT"),
                    positions(&[0]),
                    "127.0.0.1",
                ),
                "master",
            ),
            (
                venue(
                    key(json!({"userID": 1})),
                    account("REGULAR_MARGIN", 5),
                    instrument("BTCUSDT", "USDT"),
                    positions(&[0]),
                    "127.0.0.1",
                ),
                "belongs to user",
            ),
            (
                venue(
                    key(json!({"readOnly": 1})),
                    account("REGULAR_MARGIN", 5),
                    instrument("BTCUSDT", "USDT"),
                    positions(&[0]),
                    "127.0.0.1",
                ),
                "read-only",
            ),
            (
                venue(
                    key(json!({"permissions": {"ContractTrade": ["Order"]}})),
                    account("REGULAR_MARGIN", 5),
                    instrument("BTCUSDT", "USDT"),
                    positions(&[0]),
                    "127.0.0.1",
                ),
                "Position",
            ),
            (
                venue(
                    key(json!({"ips": ["*"]})),
                    account("REGULAR_MARGIN", 5),
                    instrument("BTCUSDT", "USDT"),
                    positions(&[0]),
                    "127.0.0.1",
                ),
                "bound to no IP",
            ),
            (
                venue(
                    key(json!({"ips": []})),
                    account("REGULAR_MARGIN", 5),
                    instrument("BTCUSDT", "USDT"),
                    positions(&[0]),
                    "127.0.0.1",
                ),
                "bound to no IP",
            ),
            (
                venue(
                    key(json!({})),
                    account("ISOLATED_MARGIN", 5),
                    instrument("BTCUSDT", "USDT"),
                    positions(&[0]),
                    "127.0.0.1",
                ),
                "REGULAR_MARGIN",
            ),
            (
                venue(
                    key(json!({})),
                    account("PORTFOLIO_MARGIN", 5),
                    instrument("BTCUSDT", "USDT"),
                    positions(&[0]),
                    "127.0.0.1",
                ),
                "REGULAR_MARGIN",
            ),
            (
                venue(
                    key(json!({})),
                    account("REGULAR_MARGIN", 1),
                    instrument("BTCUSDT", "USDT"),
                    positions(&[0]),
                    "127.0.0.1",
                ),
                "unified",
            ),
            (
                venue(
                    key(json!({})),
                    account("REGULAR_MARGIN", 5),
                    instrument("BTCUSDT", "USDC"),
                    positions(&[0]),
                    "127.0.0.1",
                ),
                "settles",
            ),
            (
                venue(
                    key(json!({})),
                    account("REGULAR_MARGIN", 5),
                    instrument("ETHUSDT", "USDT"),
                    positions(&[0]),
                    "127.0.0.1",
                ),
                "not a linear",
            ),
            (
                venue(
                    key(json!({})),
                    account("REGULAR_MARGIN", 5),
                    instrument("BTCUSDT", "USDT"),
                    positions(&[1, 2]),
                    "127.0.0.1",
                ),
                "hedge",
            ),
        ];
        for (v, needle) in cases {
            match run(&v).await {
                Err(Block::Account(e)) => assert!(e.contains(needle), "{needle}: {e}"),
                other => panic!("{needle}: {other:?}"),
            }
        }
    }

    #[tokio::test]
    async fn requests_from_an_address_the_key_is_not_bound_to_are_an_auth_block() {
        let v = venue(
            key(json!({})),
            account("REGULAR_MARGIN", 5),
            instrument("BTCUSDT", "USDT"),
            positions(&[0]),
            "198.51.100.9",
        );
        assert!(matches!(run(&v).await, Err(Block::AuthRejected(e)) if e.contains("198.51.100.9")));
    }

    #[test]
    fn an_ipv4_mapped_address_is_its_ipv4_address() {
        let ips = vec!["203.0.113.7".to_string()];
        ip_bound(&ips, "::ffff:203.0.113.7".parse().unwrap()).unwrap();
        let mapped = vec!["::ffff:203.0.113.7".to_string()];
        ip_bound(&mapped, "203.0.113.7".parse().unwrap()).unwrap();
        assert!(matches!(
            ip_bound(&ips, "2001:db8::7".parse().unwrap()),
            Err(Block::AuthRejected(_))
        ));
    }

    #[tokio::test]
    async fn a_unified_status_outside_3_to_6_or_missing_blocks() {
        for (acct, want_account) in [
            (account("REGULAR_MARGIN", 7), true),
            (account("REGULAR_MARGIN", 2), true),
            (json!({"marginMode": "REGULAR_MARGIN"}), false),
        ] {
            let v = venue(
                key(json!({})),
                acct,
                instrument("BTCUSDT", "USDT"),
                positions(&[0]),
                "127.0.0.1",
            );
            match run(&v).await {
                Err(Block::Account(e)) if want_account => assert!(e.contains("unified"), "{e}"),
                Err(Block::Unchecked(e)) if !want_account => {
                    assert!(e.contains("unifiedMarginStatus"), "{e}")
                }
                other => panic!("{other:?}"),
            }
        }
    }

    #[tokio::test]
    async fn a_rejected_key_is_an_auth_block_and_an_unanswered_check_is_unchecked() {
        let v = good();
        v.api.route(
            "/v5/user/query-api",
            [Answer::Reply(
                200,
                r#"{"retCode":10010,"retMsg":"Unmatched IP","result":{}}"#.into(),
            )],
        );
        assert!(matches!(run(&v).await, Err(Block::AuthRejected(_))));
        assert_eq!(v.api.requests().len(), 1);
        let v = good();
        v.echo.set_default(Answer::Reply(200, "not-an-ip".into()));
        assert!(matches!(run(&v).await, Err(Block::Unchecked(_))));
        let v = good();
        v.api
            .route("/v5/account/info", [Answer::Reply(500, "".into())]);
        assert!(matches!(run(&v).await, Err(Block::Unchecked(_))));
    }

    #[tokio::test]
    async fn a_later_ip_change_blocks_and_an_auth_block_is_not_cleared() {
        let v = good();
        let rest = Rest::new(
            &v.api.url(),
            Arc::new(Credentials::new("K", "S")),
            5000,
            Duration::from_millis(500),
        )
        .unwrap();
        let echo = echo_client(Duration::from_millis(500)).unwrap();
        let h = Health::default();
        h.set_ip(recheck_ip(&rest, &echo, &v.echo.url(), &want()).await.err());
        assert_eq!(h.block(), None);
        v.echo
            .set_default(Answer::Reply(200, "198.51.100.9".into()));
        h.set_ip(recheck_ip(&rest, &echo, &v.echo.url(), &want()).await.err());
        assert!(matches!(h.block(), Some(Block::AuthRejected(_))));
        v.echo.set_default(Answer::Reply(200, "127.0.0.1".into()));
        h.set_ip(recheck_ip(&rest, &echo, &v.echo.url(), &want()).await.err());
        h.set_boot(None);
        assert!(
            matches!(h.block(), Some(Block::AuthRejected(_))),
            "never lifted by a later pass"
        );
    }

    #[test]
    fn a_passing_ip_recheck_does_not_lift_a_failed_boot_check() {
        let h = Health::default();
        h.set_boot(Some(Block::Account("BTCUSDT is in hedge mode".into())));
        h.set_ip(None);
        assert!(matches!(h.block(), Some(Block::Account(_))));
        h.set_boot(Some(Block::Unchecked("position/list: HTTP 500".into())));
        h.set_ip(None);
        assert!(matches!(h.block(), Some(Block::Unchecked(_))));
        h.set_ip(Some(Block::Unchecked("IP echo: HTTP 500".into())));
        h.set_boot(None);
        assert!(
            matches!(h.block(), Some(Block::Unchecked(e)) if e.contains("echo")),
            "a passing boot rerun does not lift the IP slot"
        );
        h.set_ip(None);
        assert_eq!(h.block(), None);
    }
}
