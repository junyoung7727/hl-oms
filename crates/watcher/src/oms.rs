//! The watcher's links to the OMS (C-WATCH part 7, U19a7), written from
//! C-API and C-LIVE part 2, not from `oms-api`: `/api/status` on the
//! loopback dashboard, `watcher_mismatch` on `admin.sock` (one connection
//! per send) and the `heartbeat` on `monitor.sock` (one persistent
//! connection). Each line is one JSON object; a reply is
//! `{"id","ok","result"}` or `{"id","ok":false,"error":{"code","message"}}`.

use std::path::Path;
use std::time::Duration;

use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;
use tokio::net::unix::{OwnedReadHalf, OwnedWriteHalf};

use crate::engine::Check;

/// A line of the OMS's sockets is at most this many bytes (C-API).
pub const LINE_MAX: u64 = 64 * 1024;

fn quote(s: &str) -> String {
    Value::String(s.to_string()).to_string()
}

fn hello_line(id: u64, name: &str) -> String {
    format!(
        r#"{{"id":{id},"op":"hello","client":{}}}"#,
        quote(&format!("watcher/{name}"))
    )
}

/// The `result` of the reply with `id` as text (a JSON string as is, any
/// other value as JSON), or `<code>: <message>` for an error reply.
/// Lines with another id are skipped.
async fn reply(rd: &mut BufReader<OwnedReadHalf>, id: u64) -> Result<String, String> {
    loop {
        let mut line = String::new();
        let n = (&mut *rd)
            .take(LINE_MAX + 1)
            .read_line(&mut line)
            .await
            .map_err(|e| format!("read: {e}"))?;
        if n == 0 {
            return Err("the OMS closed the connection".into());
        }
        if n as u64 > LINE_MAX {
            return Err("a reply line longer than 64 KiB".into());
        }
        let v: Value = serde_json::from_str(line.trim_end())
            .map_err(|e| format!("a reply that is not JSON: {e}"))?;
        if v.get("id").and_then(Value::as_u64) != Some(id) {
            continue;
        }
        if v.get("ok").and_then(Value::as_bool) == Some(true) {
            return Ok(match v.get("result") {
                Some(Value::String(s)) => s.clone(),
                Some(other) => other.to_string(),
                None => String::new(),
            });
        }
        let code = v
            .pointer("/error/code")
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        let msg = v
            .pointer("/error/message")
            .and_then(Value::as_str)
            .unwrap_or("");
        return Err(format!("{code}: {msg}"));
    }
}

async fn write_line(wr: &mut OwnedWriteHalf, line: &str) -> Result<(), String> {
    wr.write_all(format!("{line}\n").as_bytes())
        .await
        .map_err(|e| format!("write: {e}"))
}

/// One request and its reply within `timeout_ms`.
async fn ask(
    rd: &mut BufReader<OwnedReadHalf>,
    wr: &mut OwnedWriteHalf,
    id: u64,
    line: &str,
    timeout_ms: u64,
) -> Result<String, String> {
    let exchange = async {
        write_line(wr, line).await?;
        reply(rd, id).await
    };
    match tokio::time::timeout(Duration::from_millis(timeout_ms), exchange).await {
        Ok(r) => r,
        Err(_) => Err(format!("no reply to request {id} within {timeout_ms} ms")),
    }
}

async fn connect(path: &Path) -> Result<(BufReader<OwnedReadHalf>, OwnedWriteHalf), String> {
    let s = UnixStream::connect(path)
        .await
        .map_err(|e| format!("not sent: {}: {e}", path.display()))?;
    let (rd, wr) = s.into_split();
    Ok((BufReader::new(rd), wr))
}

/// One `watcher_mismatch` send to `admin.sock` at `path`: connect,
/// `hello` (client `watcher/<name>`), then the request; each reply within
/// `timeout_ms` and the whole send within 2 × `timeout_ms`; the connection
/// closes after it. The `result` (`"held"`, `"already held"`), or why
/// there is none (`<code>: <message>` for an error reply).
pub async fn admin_send(
    path: &Path,
    name: &str,
    check: Check,
    detail: &str,
    timeout_ms: u64,
) -> Result<String, String> {
    let send = async {
        let (mut rd, mut wr) = connect(path).await?;
        ask(&mut rd, &mut wr, 1, &hello_line(1, name), timeout_ms)
            .await
            .map_err(|e| format!("hello: {e}"))?;
        let line = format!(
            r#"{{"id":2,"op":"watcher_mismatch","check":"{}","detail":{}}}"#,
            check.name(),
            quote(detail)
        );
        ask(&mut rd, &mut wr, 2, &line, timeout_ms).await
    };
    let whole = 2 * timeout_ms;
    match tokio::time::timeout(Duration::from_millis(whole), send).await {
        Ok(r) => r,
        Err(_) => Err(format!("no reply within {whole} ms")),
    }
}
