//! The read-only dashboard.
//!
//! Every route is a GET. There is no route that changes anything: no halt,
//! no cancel, no configuration, no fault injection. Operator actions go
//! through the admin socket, where they are journaled.
//!
//! The listener binds to a loopback address only (enforced when the
//! configuration is loaded) and has no authentication, so anyone who can
//! open a socket on this host can read orders and positions. Requests whose
//! `Host` header is not a loopback name are refused, which keeps a web page
//! on another origin from reaching it through DNS rebinding.
//!
//! What a reader sees is the published state: built from durable records
//! only, and possibly a moment behind the authoritative state. Reading it
//! takes a shared lock that the commit stage also takes exclusively for a
//! short time per batch; that is the only cost observation puts on the
//! processing path.

use crate::server::ApiCtx;
use crate::status;
use crate::util::wall_us;
use axum::Router;
use axum::extract::{Path, Query, Request, State};
use axum::http::{HeaderValue, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Json, Response};
use axum::routing::get;
use oms_core::*;
use serde::Deserialize;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Clone)]
struct App {
    /// `None` when serving documents without a running node.
    ctx: Option<Arc<ApiCtx>>,
    static_dir: PathBuf,
    port: u16,
}

fn json_error(code: StatusCode, msg: &str) -> Response {
    (code, Json(serde_json::json!({ "error": msg }))).into_response()
}

async fn guard(State(app): State<App>, req: Request, next: Next) -> Response {
    let host = req
        .headers()
        .get(header::HOST)
        .and_then(|h| h.to_str().ok())
        .unwrap_or("");
    let allowed = [
        format!("127.0.0.1:{}", app.port),
        format!("localhost:{}", app.port),
        format!("[::1]:{}", app.port),
    ];
    if !allowed.iter().any(|a| a == host) {
        return json_error(StatusCode::FORBIDDEN, "unexpected Host header");
    }
    if req.method() != axum::http::Method::GET {
        return json_error(StatusCode::METHOD_NOT_ALLOWED, "this interface is read-only");
    }
    let mut resp = next.run(req).await;
    let h = resp.headers_mut();
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    h.insert("x-content-type-options", HeaderValue::from_static("nosniff"));
    h.insert("referrer-policy", HeaderValue::from_static("no-referrer"));
    h.insert("x-frame-options", HeaderValue::from_static("DENY"));
    h.insert(
        "content-security-policy",
        HeaderValue::from_static("default-src 'self'; style-src 'self'; script-src 'self'; frame-ancestors 'none'"),
    );
    resp
}

// The error is the HTTP response itself and is returned at once by every
// caller; boxing it would add an allocation to the error path for no gain.
#[allow(clippy::result_large_err)]
fn need(app: &App) -> Result<&Arc<ApiCtx>, Response> {
    app.ctx
        .as_ref()
        .ok_or_else(|| json_error(StatusCode::SERVICE_UNAVAILABLE, "no node is attached to this dashboard"))
}

async fn api_status(State(app): State<App>) -> Response {
    match need(&app) {
        Ok(ctx) => Json(status::status(ctx, None)).into_response(),
        Err(r) => r,
    }
}

#[derive(Deserialize)]
struct OrdersQuery {
    #[serde(default)]
    scope: Option<String>,
    #[serde(default)]
    limit: Option<usize>,
}

async fn api_orders(State(app): State<App>, Query(q): Query<OrdersQuery>) -> Response {
    let ctx = match need(&app) {
        Ok(c) => c,
        Err(r) => return r,
    };
    let limit = q.limit.unwrap_or(200).min(2000);
    let p = ctx.shared.published.read();
    let open_only = q.scope.as_deref() == Some("open");
    let list: Vec<serde_json::Value> = p
        .core
        .orders
        .values()
        .rev()
        .filter(|o| !open_only || !o.is_closed())
        .take(limit)
        .map(|o| {
            serde_json::json!({
                "id": o.id, "strategy": o.strategy, "client_order_id": o.client_order_id,
                "instrument": o.instrument, "side": o.side, "qty": o.qty, "px": o.px, "tif": o.tif,
                "phase": o.phase, "cum_qty": o.cum_qty, "reserved": o.reserved,
                "in_doubt": o.doubt.is_some(), "cancel_requested": o.cancel.is_some(),
                "accepted_at": o.accepted_at, "closed": o.close,
            })
        })
        .collect();
    Json(serde_json::json!({
        "as_of": wall_us(),
        "source": "published state (durable records only)",
        "durable_seq": p.durable_seq,
        "total_in_memory": p.core.orders.len(),
        "orders": list,
    }))
    .into_response()
}

async fn api_order(State(app): State<App>, Path(id): Path<u64>) -> Response {
    let ctx = match need(&app) {
        Ok(c) => c,
        Err(r) => return r,
    };
    let p = ctx.shared.published.read();
    let id = OrderId(id);
    let Some(order) = p.core.order(id) else {
        return json_error(
            StatusCode::NOT_FOUND,
            "not in memory; closed orders are removed after their retention. The journal still has it: oms-node trace --order N",
        );
    };
    let trace = p.traces.get(&id);
    Json(serde_json::json!({
        "as_of": wall_us(),
        "source": "published state; trace entries are journal records plus what the commit stage observed when it released them",
        "durable_seq": p.durable_seq,
        "order": order,
        "venue_client_id": ctx.info.wire_id_form.encode(VenueClientId { instance_id: p.core.instance_id, order_id: id }),
        "trace": trace.map(|t| &t.entries),
        "trace_truncated": trace.map(|t| t.truncated).unwrap_or(false),
    }))
    .into_response()
}

#[derive(Deserialize)]
struct LimitQuery {
    #[serde(default)]
    limit: Option<usize>,
}

async fn api_recent(State(app): State<App>, Query(q): Query<LimitQuery>) -> Response {
    let ctx = match need(&app) {
        Ok(c) => c,
        Err(r) => return r,
    };
    let limit = q.limit.unwrap_or(100).min(2000);
    let p = ctx.shared.published.read();
    let records: Vec<_> = p.recent.iter().rev().take(limit).collect();
    let noops: Vec<_> = p.noops.iter().rev().take(limit).collect();
    Json(serde_json::json!({
        "as_of": wall_us(),
        "durable_seq": p.durable_seq,
        "records": records,
        "inputs_without_events": noops,
        "inputs_without_events_note": "not journaled; bounded and lossy; absence here proves nothing",
    }))
    .into_response()
}

fn content_type(name: &str) -> &'static str {
    match name.rsplit('.').next() {
        Some("html") => "text/html; charset=utf-8",
        Some("js") => "text/javascript; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("json") => "application/json",
        Some("svg") => "image/svg+xml",
        _ => "application/octet-stream",
    }
}

/// Serves one file from the static directory or its `data` subdirectory.
/// Names are restricted to a plain character set, so a request cannot leave
/// the directory.
async fn serve_file(app: &App, sub: Option<&str>, name: &str) -> Response {
    let plain = |s: &str| !s.is_empty() && !s.starts_with('.') && s.chars().all(|c| c.is_ascii_alphanumeric() || "._-".contains(c));
    if !plain(name) || sub.is_some_and(|s| !plain(s)) {
        return json_error(StatusCode::NOT_FOUND, "not found");
    }
    let path = match sub {
        Some(s) => app.static_dir.join(s).join(name),
        None => app.static_dir.join(name),
    };
    match tokio::fs::read(&path).await {
        Ok(bytes) => ([(header::CONTENT_TYPE, content_type(name))], bytes).into_response(),
        Err(_) => json_error(StatusCode::NOT_FOUND, "not found"),
    }
}

/// True for a path the document viewer may read: a text file under `docs/`,
/// `crates/` or `registry/` whose every component is a plain name.
fn source_path_allowed(path: &str) -> bool {
    let plain = |s: &str| !s.is_empty() && !s.starts_with('.') && s.chars().all(|c| c.is_ascii_alphanumeric() || "._-".contains(c));
    let parts: Vec<&str> = path.split('/').collect();
    let text = matches!(path.rsplit('.').next(), Some("md" | "rs" | "toml"));
    parts.len() >= 2 && parts.len() <= 8 && matches!(parts[0], "docs" | "crates" | "registry") && parts.iter().all(|p| plain(p)) && text
}

/// Serves a document or source file as plain text, so the dashboard can show
/// the reason, the contract and the code next to what the node did. The
/// files are read from the directory above the static directory, which is
/// the repository root in a source checkout; elsewhere this answers 404.
async fn source_file(State(app): State<App>, Path(path): Path<String>) -> Response {
    if !source_path_allowed(&path) {
        return json_error(StatusCode::NOT_FOUND, "not found");
    }
    let Some(root) = app.static_dir.parent() else {
        return json_error(StatusCode::NOT_FOUND, "not found");
    };
    match tokio::fs::read(root.join(&path)).await {
        Ok(bytes) => ([(header::CONTENT_TYPE, "text/plain; charset=utf-8")], bytes).into_response(),
        Err(_) => json_error(StatusCode::NOT_FOUND, "not found in this deployment"),
    }
}

async fn index(State(app): State<App>) -> Response {
    serve_file(&app, None, "index.html").await
}

async fn root_file(State(app): State<App>, Path(name): Path<String>) -> Response {
    serve_file(&app, None, &name).await
}

async fn data_file(State(app): State<App>, Path(name): Path<String>) -> Response {
    serve_file(&app, Some("data"), &name).await
}

/// Runs the dashboard until the task is dropped.
pub async fn serve(listen: SocketAddr, static_dir: PathBuf, ctx: Option<Arc<ApiCtx>>) -> Result<(), String> {
    if !listen.ip().is_loopback() {
        return Err("the dashboard binds to loopback only".into());
    }
    let app = App {
        ctx,
        static_dir,
        port: listen.port(),
    };
    let router = Router::new()
        .route("/", get(index))
        .route("/api/status", get(api_status))
        .route("/api/orders", get(api_orders))
        .route("/api/orders/{id}", get(api_order))
        .route("/api/recent", get(api_recent))
        .route("/data/{name}", get(data_file))
        .route("/src/{*path}", get(source_file))
        .route("/{name}", get(root_file))
        .layer(middleware::from_fn_with_state(app.clone(), guard))
        .with_state(app);
    let listener = tokio::net::TcpListener::bind(listen)
        .await
        .map_err(|e| format!("bind {listen}: {e}"))?;
    axum::serve(listener, router).await.map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::source_path_allowed;

    #[test]
    fn the_document_viewer_cannot_leave_its_directories() {
        for ok in [
            "docs/adr/ADR-004-commit-gate.md",
            "crates/oms-core/src/decide.rs",
            "registry/claims.toml",
            "docs/explanation.md",
        ] {
            assert!(source_path_allowed(ok), "{ok}");
        }
        for bad in [
            "docs/../config/node.toml",
            "docs/./x.md",
            "docs//x.md",
            "/etc/passwd",
            "config/node.toml",
            "data/journal/0.seg",
            "docs/.hidden.md",
            "docs/x.json",
            "docs",
            "docs/a b.md",
            "crates/oms-node/tests/../../../x.rs",
            "docs/%2e%2e/x.md",
            "docs/a/b/c/d/e/f/g/h.md",
            "Cargo.toml",
        ] {
            assert!(!source_path_allowed(bad), "{bad}");
        }
    }
}
