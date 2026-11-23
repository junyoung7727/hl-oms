//! A fake OMS socket (`admin.sock`, `monitor.sock`) for tests (U19a7): a
//! `UnixListener` on its own thread, one thread per connection, every
//! line recorded per connection, each answered from the test's function.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixListener;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::Value;

/// What the fake does with one request line.
pub enum Reply {
    /// One reply line.
    Line(String),
    /// No reply; the connection stays open.
    Silent,
    /// No reply; the connection is closed.
    Close,
}

/// `{"id":<id>,"ok":true,"result":<result>}`.
pub fn ok(v: &Value, result: Value) -> Reply {
    Reply::Line(serde_json::json!({"id": v["id"], "ok": true, "result": result}).to_string())
}

/// `{"id":<id>,"ok":false,"error":{"code","message"}}`.
pub fn err(v: &Value, code: &str, message: &str) -> Reply {
    Reply::Line(
        serde_json::json!({"id": v["id"], "ok": false,
            "error": {"code": code, "message": message}})
        .to_string(),
    )
}

/// An OMS's answer: `hello` ok, then `result` for every other op.
pub fn oms(v: &Value, result: &str) -> Reply {
    if v["op"] == "hello" {
        ok(v, serde_json::json!({"instance_id": "0000000000000007"}))
    } else {
        ok(v, Value::String(result.into()))
    }
}

#[derive(Default)]
pub struct Seen {
    /// The lines of each connection, in order of connection.
    pub conns: Vec<Vec<Value>>,
    /// Per connection: the client closed it.
    pub closed: Vec<bool>,
}

pub struct FakeSock {
    pub seen: Arc<Mutex<Seen>>,
}

type Answer = dyn Fn(usize, usize, &Value) -> Reply + Send + Sync;

impl FakeSock {
    /// Serves `path`; `answer(connection, line index, line)`.
    pub fn serve(
        path: &Path,
        answer: impl Fn(usize, usize, &Value) -> Reply + Send + Sync + 'static,
    ) -> FakeSock {
        let _ = std::fs::remove_file(path);
        let l = UnixListener::bind(path).unwrap();
        let seen = Arc::new(Mutex::new(Seen::default()));
        let answer: Arc<Answer> = Arc::new(answer);
        let s = seen.clone();
        std::thread::spawn(move || {
            for c in l.incoming() {
                let Ok(c) = c else { return };
                let conn = {
                    let mut g = s.lock().unwrap();
                    g.conns.push(Vec::new());
                    g.closed.push(false);
                    g.conns.len() - 1
                };
                let (s, answer) = (s.clone(), answer.clone());
                std::thread::spawn(move || {
                    let mut rd = BufReader::new(c.try_clone().unwrap());
                    let mut w = c;
                    let mut i = 0;
                    loop {
                        let mut line = String::new();
                        if rd.read_line(&mut line).unwrap_or(0) == 0 {
                            s.lock().unwrap().closed[conn] = true;
                            return;
                        }
                        let v: Value = serde_json::from_str(line.trim_end()).unwrap();
                        s.lock().unwrap().conns[conn].push(v.clone());
                        match answer(conn, i, &v) {
                            Reply::Line(r) => {
                                let _ = w.write_all(format!("{r}\n").as_bytes());
                            }
                            Reply::Silent => {}
                            Reply::Close => {
                                let _ = w.shutdown(std::net::Shutdown::Both);
                                return;
                            }
                        }
                        i += 1;
                    }
                });
            }
        });
        FakeSock { seen }
    }

    pub fn conns(&self) -> Vec<Vec<Value>> {
        self.seen.lock().unwrap().conns.clone()
    }

    /// Waits (real time, at most `timeout`) until `f` holds of what was
    /// seen.
    pub fn wait(&self, timeout: Duration, f: impl Fn(&Seen) -> bool) -> bool {
        let end = Instant::now() + timeout;
        loop {
            if f(&self.seen.lock().unwrap()) {
                return true;
            }
            if Instant::now() >= end {
                return false;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}
