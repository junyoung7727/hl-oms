//! A loopback fake HTTP/1.1 server for tests (unit U6). It runs on its own
//! thread, records every request and answers from a script: a status and a
//! body, or no answer at all (`Answer::Hang`), or from a handler that
//! computes it from the request (`FakeHttp::respond`). Requests need a
//! `Content-Length` body (reqwest sends one); keep-alive is supported.

use std::collections::VecDeque;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Answer {
    /// Status and body (sent as `application/json`).
    Reply(u16, String),
    /// Read the request, never answer, keep the connection open.
    Hang,
    /// Read the request, then close the connection without an answer.
    Close,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    pub method: String,
    /// The request target: path and query.
    pub target: String,
    /// Header names in lower case.
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Request {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }

    pub fn path(&self) -> &str {
        self.target.split('?').next().unwrap_or("")
    }

    pub fn query(&self) -> &str {
        self.target.split_once('?').map(|(_, q)| q).unwrap_or("")
    }

    pub fn body_str(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }
}

#[derive(Default)]
struct State {
    script: VecDeque<Answer>,
    default: Option<Answer>,
    /// Per path: answers in order, then the last one again.
    routes: Vec<(String, VecDeque<Answer>)>,
    requests: Vec<Request>,
    /// Called with each request when it arrives, before it is answered.
    hook: Option<Hook>,
    /// Asked first for the answer to each request (`FakeHttp::respond`).
    handler: Option<Handler>,
}

type Hook = Arc<dyn Fn(&Request) + Send + Sync>;
type Handler = Arc<dyn Fn(&Request) -> Option<Answer> + Send + Sync>;

/// A running fake. Dropping it stops the server thread.
pub struct FakeHttp {
    addr: SocketAddr,
    state: Arc<Mutex<State>>,
    stop: Option<tokio::sync::oneshot::Sender<()>>,
    join: Option<std::thread::JoinHandle<()>>,
}

impl FakeHttp {
    /// Starts a fake that answers `default` after the scripted answers.
    pub fn start(default: Answer) -> FakeHttp {
        let state = Arc::new(Mutex::new(State {
            default: Some(default),
            ..State::default()
        }));
        let (addr_tx, addr_rx) = std::sync::mpsc::channel();
        let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();
        let st = state.clone();
        let join = std::thread::Builder::new()
            .name("http-fake".into())
            .spawn(move || {
                let Ok(rt) = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                else {
                    return;
                };
                rt.block_on(async move {
                    let Ok(l) = TcpListener::bind("127.0.0.1:0").await else {
                        return;
                    };
                    let _ = addr_tx.send(l.local_addr().ok());
                    tokio::select! {
                        _ = accept_loop(l, st) => {}
                        _ = stop_rx => {}
                    }
                });
            })
            .ok();
        let addr = addr_rx
            .recv_timeout(Duration::from_secs(5))
            .ok()
            .flatten()
            .unwrap_or_else(|| SocketAddr::from(([127, 0, 0, 1], 9)));
        FakeHttp {
            addr,
            state,
            stop: Some(stop_tx),
            join,
        }
    }

    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    /// `http://127.0.0.1:<port>`.
    pub fn url(&self) -> String {
        format!("http://{}", self.addr)
    }

    /// Answers used before the default, in order.
    pub fn script(&self, answers: impl IntoIterator<Item = Answer>) {
        if let Ok(mut s) = self.state.lock() {
            s.script.extend(answers);
        }
    }

    /// Answers for requests whose path is exactly `path`, in order; the last
    /// one is repeated. A routed path never uses the script or the default,
    /// so concurrent requests to several paths get their own answers.
    pub fn route(&self, path: &str, answers: impl IntoIterator<Item = Answer>) {
        if let Ok(mut s) = self.state.lock() {
            let answers: VecDeque<Answer> = answers.into_iter().collect();
            s.routes.retain(|(p, _)| p != path);
            s.routes.push((path.to_string(), answers));
        }
    }

    /// Calls `f` with each request when it arrives, before the answer is
    /// chosen and sent (a test sees what was true at that moment).
    pub fn on_request(&self, f: impl Fn(&Request) + Send + Sync + 'static) {
        if let Ok(mut s) = self.state.lock() {
            s.hook = Some(Arc::new(f));
        }
    }

    /// Gives each request first to `f`: `Some(answer)` is the answer,
    /// `None` falls through to the routes, the script and the default. The
    /// request is in `requests()` before `f` runs, and `f` runs without the
    /// fake's lock, so it may read `requests()`.
    pub fn respond(&self, f: impl Fn(&Request) -> Option<Answer> + Send + Sync + 'static) {
        if let Ok(mut s) = self.state.lock() {
            s.handler = Some(Arc::new(f));
        }
    }

    pub fn set_default(&self, a: Answer) {
        if let Ok(mut s) = self.state.lock() {
            s.default = Some(a);
        }
    }

    pub fn requests(&self) -> Vec<Request> {
        self.state
            .lock()
            .map(|s| s.requests.clone())
            .unwrap_or_default()
    }

    /// Waits until at least `n` requests arrived; returns them all.
    pub fn wait_requests(&self, n: usize, timeout: Duration) -> Vec<Request> {
        let end = Instant::now() + timeout;
        loop {
            let r = self.requests();
            if r.len() >= n || Instant::now() >= end {
                return r;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}

impl Drop for FakeHttp {
    fn drop(&mut self) {
        if let Some(s) = self.stop.take() {
            let _ = s.send(());
        }
        if let Some(j) = self.join.take() {
            let _ = j.join();
        }
    }
}

/// A loopback address where nothing listens (bound, then released).
pub fn dead_addr() -> SocketAddr {
    let l = std::net::TcpListener::bind("127.0.0.1:0");
    match l.and_then(|l| l.local_addr()) {
        Ok(a) => a,
        Err(_) => SocketAddr::from(([127, 0, 0, 1], 9)),
    }
}

async fn accept_loop(l: TcpListener, st: Arc<Mutex<State>>) {
    while let Ok((sock, _)) = l.accept().await {
        tokio::spawn(serve(sock, st.clone()));
    }
}

fn next_answer(st: &Mutex<State>, req: Request) -> Answer {
    let handler = {
        let Ok(mut s) = st.lock() else {
            return Answer::Close;
        };
        s.requests.push(req.clone());
        s.handler.clone()
    };
    if let Some(a) = handler.and_then(|f| f(&req)) {
        return a;
    }
    let Ok(mut s) = st.lock() else {
        return Answer::Close;
    };
    let path = req.path().to_string();
    if let Some((_, answers)) = s.routes.iter_mut().find(|(p, _)| *p == path) {
        return match answers.len() {
            0 => Answer::Close,
            1 => answers[0].clone(),
            _ => answers.pop_front().unwrap_or(Answer::Close),
        };
    }
    match s.script.pop_front() {
        Some(a) => a,
        None => s.default.clone().unwrap_or(Answer::Close),
    }
}

async fn serve(mut sock: TcpStream, st: Arc<Mutex<State>>) {
    let mut buf: Vec<u8> = Vec::new();
    loop {
        let Some(req) = read_request(&mut sock, &mut buf).await else {
            return;
        };
        let hook = st.lock().ok().and_then(|s| s.hook.clone());
        if let Some(f) = hook {
            f(&req);
        }
        match next_answer(&st, req) {
            Answer::Reply(status, body) => {
                let head = format!(
                    "HTTP/1.1 {status} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\n\r\n",
                    body.len()
                );
                if sock.write_all(head.as_bytes()).await.is_err()
                    || sock.write_all(body.as_bytes()).await.is_err()
                {
                    return;
                }
            }
            Answer::Hang => {
                std::future::pending::<()>().await;
            }
            Answer::Close => return,
        }
    }
}

async fn read_request(sock: &mut TcpStream, buf: &mut Vec<u8>) -> Option<Request> {
    let mut chunk = [0u8; 8192];
    let head_end = loop {
        if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break i;
        }
        let n = sock.read(&mut chunk).await.ok()?;
        if n == 0 {
            return None;
        }
        buf.extend_from_slice(&chunk[..n]);
    };
    let head = String::from_utf8_lossy(&buf[..head_end]).into_owned();
    let mut lines = head.split("\r\n");
    let first = lines.next()?;
    let mut parts = first.split(' ');
    let method = parts.next()?.to_string();
    let target = parts.next()?.to_string();
    let headers: Vec<(String, String)> = lines
        .filter_map(|l| l.split_once(':'))
        .map(|(k, v)| (k.trim().to_ascii_lowercase(), v.trim().to_string()))
        .collect();
    let len: usize = headers
        .iter()
        .find(|(k, _)| k == "content-length")
        .and_then(|(_, v)| v.parse().ok())
        .unwrap_or(0);
    let start = head_end + 4;
    while buf.len() < start + len {
        let n = sock.read(&mut chunk).await.ok()?;
        if n == 0 {
            return None;
        }
        buf.extend_from_slice(&chunk[..n]);
    }
    let body = buf[start..start + len].to_vec();
    buf.drain(..start + len);
    Some(Request {
        method,
        target,
        headers,
        body,
    })
}
