//! Sockets: the WebSocket connect with the loopback guard, and REST requests.

use std::net::SocketAddr;
use std::time::Duration;

use md_core::adapter::RestRequest;
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::http::Uri;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

pub(crate) type Ws = WebSocketStream<MaybeTlsStream<TcpStream>>;

/// The host and port of a `ws`, `wss`, `http` or `https` URL.
fn host_port(url: &str) -> Result<(String, u16), String> {
    let uri: Uri = url.parse().map_err(|e| format!("bad URL {url}: {e}"))?;
    let host = uri
        .host()
        .ok_or_else(|| format!("URL {url} has no host"))?
        .trim_start_matches('[')
        .trim_end_matches(']')
        .to_string();
    let port = match (uri.port_u16(), uri.scheme_str()) {
        (Some(p), _) => p,
        (None, Some("ws" | "http")) => 80,
        (None, Some("wss" | "https")) => 443,
        (None, s) => return Err(format!("URL {url} has an unknown scheme {s:?}")),
    };
    Ok((host, port))
}

/// Resolves the URL's host. With `loopback_only`, any address that is not
/// loopback is refused before a socket is opened.
pub(crate) async fn resolve(url: &str, loopback_only: bool) -> Result<Vec<SocketAddr>, String> {
    let (host, port) = host_port(url)?;
    let addrs: Vec<SocketAddr> = tokio::net::lookup_host((host.as_str(), port))
        .await
        .map_err(|e| format!("cannot resolve {host}: {e}"))?
        .collect();
    if addrs.is_empty() {
        return Err(format!("{host} has no address"));
    }
    if loopback_only {
        if let Some(a) = addrs.iter().find(|a| !a.ip().is_loopback()) {
            return Err(format!("address {a} is not loopback (loopback_only)"));
        }
    }
    Ok(addrs)
}

/// Opens the WebSocket on one of the checked addresses.
pub(crate) async fn connect_ws(url: &str, loopback_only: bool, timeout: Duration) -> Result<Ws, String> {
    let addrs = resolve(url, loopback_only).await?;
    let attempt = async {
        let tcp = TcpStream::connect(addrs.as_slice())
            .await
            .map_err(|e| format!("connect failed: {e}"))?;
        let _ = tcp.set_nodelay(true);
        let (ws, _) = tokio_tungstenite::client_async_tls(url, tcp)
            .await
            .map_err(|e| format!("handshake failed: {e}"))?;
        Ok(ws)
    };
    tokio::time::timeout(timeout, attempt)
        .await
        .map_err(|_| format!("connect timed out after {timeout:?}"))?
}

/// CLOCK_REALTIME in ns (`recv_ns`).
pub(crate) fn now_ns() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_nanos()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

/// REST GET requests with the same loopback guard as the WebSocket.
#[derive(Debug, Clone)]
pub struct RestClient {
    base: String,
    timeout: Duration,
    loopback_only: bool,
}

impl RestClient {
    pub fn new(base: impl Into<String>, timeout: Duration, loopback_only: bool) -> Result<Self, String> {
        Ok(Self {
            base: base.into(),
            timeout,
            loopback_only,
        })
    }

    /// A client that ignores proxy settings and redirects, so that a request
    /// goes only to the address it names; with `loopback_only` the host is
    /// pinned to the checked address (spec R4).
    fn client(&self, url: &str, addrs: &[SocketAddr]) -> Result<reqwest::Client, String> {
        let mut b = reqwest::Client::builder()
            .timeout(self.timeout)
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none());
        if self.loopback_only {
            let (host, _) = host_port(url)?;
            b = b.resolve_to_addrs(&host, addrs);
        }
        b.build().map_err(|e| e.to_string())
    }

    /// Sends the request; returns `(recv_ns, body)` of a 2xx answer. `recv_ns`
    /// is read when the whole body has arrived.
    pub async fn get(&self, req: &RestRequest) -> Result<(i64, Vec<u8>), String> {
        if req.method != "GET" {
            return Err(format!("method {} is not supported", req.method));
        }
        let url = format!("{}{}", self.base.trim_end_matches('/'), req.path);
        let addrs = resolve(&url, self.loopback_only).await?;
        let resp = self
            .client(&url, &addrs)?
            .get(&url)
            .query(&req.query)
            .send()
            .await
            .map_err(|e| format!("GET {}: {e}", req.path))?;
        let status = resp.status();
        let body = resp
            .bytes()
            .await
            .map_err(|e| format!("GET {}: {e}", req.path))?;
        let recv_ns = now_ns();
        if !status.is_success() {
            return Err(format!("GET {}: status {status}", req.path));
        }
        Ok((recv_ns, body.to_vec()))
    }
}
