//! The stateful fake Bybit as servers: one REST server (the IP echo on
//! it too) and the three WebSockets, all answering from one
//! [`Book`] on the wall clock, loopback only. Both programs compare venue
//! times with their own wall clocks, so the book gets `SystemTime`
//! milliseconds; a backward step of the wall clock gives the book an older
//! `now_ms` (the fake keeps no order on it). The ticker interval runs on
//! the steady clock, so such a step does not delay a ticker.
//!
//! Every input's frames are pushed while the book is still locked, so they
//! reach the private stream in the order the book made them, whichever
//! thread made them (the WS Trade task, the REST server's thread, the
//! test).

use super::book::{Book, two_keys};
use super::ws::{FakeWs, Kind, OnOrder};
use http_fake::{Answer, FakeHttp};
use serde_json::{Value, json};
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

/// Runs once, from the fake's thread, when the fake reads the next
/// create (J5a sends SIGKILL there).
pub type Hook = Box<dyn FnOnce() + Send>;

/// How often the public stream gets a ticker snapshot.
pub const TICKER_EVERY_MS: u64 = 1_000;

pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

pub struct FakeBybit {
    pub book: Arc<Mutex<Book>>,
    pub api: FakeHttp,
    pub private: Arc<FakeWs>,
    pub trade: Arc<FakeWs>,
    pub public: Arc<FakeWs>,
    silent_next: Arc<Mutex<Option<Hook>>>,
    _rt: tokio::runtime::Runtime,
}

/// Sends `frames` to the newest private connection, unless the stream is
/// silent (a half-open network carries nothing) or not connected.
fn push(private: &FakeWs, frames: Vec<String>) {
    if private.silent.load(Ordering::SeqCst) {
        return;
    }
    for f in frames {
        private.try_send(f);
    }
}

/// Runs `f` on the book with the wall clock and pushes the frames it
/// returns before the book is unlocked (`try_send` does not block).
fn apply<R>(
    book: &Mutex<Book>,
    private: &FakeWs,
    f: impl FnOnce(&mut Book, u64) -> (R, Vec<String>),
) -> R {
    let mut b = book.lock().unwrap();
    let (r, frames) = f(&mut b, now_ms());
    push(private, frames);
    r
}

impl FakeBybit {
    /// The account with the OMS's and the watcher's keys
    /// ([`super::book::two_keys`]).
    pub fn start() -> FakeBybit {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .unwrap();
        let book = Arc::new(Mutex::new(Book::new(two_keys())));
        let private = Arc::new(FakeWs::start(&rt, Kind::Private, None));
        let silent_next: Arc<Mutex<Option<Hook>>> = Arc::default();
        // The WS Trade key is the one of the connection's last `auth`.
        let trade_cell: Arc<OnceLock<Arc<FakeWs>>> = Arc::default();
        let on_order: OnOrder = {
            let (book, private, hook, cell) = (
                book.clone(),
                private.clone(),
                silent_next.clone(),
                trade_cell.clone(),
            );
            Arc::new(move |m: &Value| {
                let key = cell
                    .get()
                    .and_then(|t| t.received("auth").last().map(|a| a["args"][0].clone()))
                    .and_then(|k| k.as_str().map(str::to_string))
                    .unwrap_or_default();
                let silent = if m["op"] == "order.create" {
                    hook.lock().unwrap().take()
                } else {
                    None
                };
                let reply = apply(&book, &private, |b, now| {
                    let (reply, frames) = b.ws_trade(now, &key, m);
                    match silent {
                        Some(_) => (None, Vec::new()),
                        None => (reply, frames),
                    }
                });
                // The hook runs outside the lock: it may read the book.
                if let Some(h) = silent {
                    h();
                    return None;
                }
                reply
            })
        };
        let trade = Arc::new(FakeWs::start(&rt, Kind::Trade, Some(on_order)));
        let _ = trade_cell.set(trade.clone());
        let public = Arc::new(FakeWs::start(&rt, Kind::Public, None));
        let api = FakeHttp::start(Answer::Reply(404, "{}".into()));
        {
            let (book, private) = (book.clone(), private.clone());
            api.respond(move |req| Some(apply(&book, &private, |b, now| b.http(now, req))));
        }
        rt.spawn(ticker_pump(book.clone(), public.clone()));
        FakeBybit {
            book,
            api,
            private,
            trade,
            public,
            silent_next,
            _rt: rt,
        }
    }

    /// `OMS_TEST_BYBIT_WIRING` for this fake.
    pub fn wiring(&self) -> String {
        json!({"rest": self.api.url(), "private_ws": self.private.url, "trade_ws": self.trade.url,
            "public_ws": self.public.url, "ip_echo_url": format!("{}/ip", self.api.url())})
        .to_string()
    }

    /// Runs `f` on the book under its lock, with the wall clock.
    pub fn with<R>(&self, f: impl FnOnce(&mut Book, u64) -> R) -> R {
        f(&mut self.book.lock().unwrap(), now_ms())
    }

    /// [`Book::fill`], its frames pushed.
    pub fn fill(&self, order_id: &str, qty: &str, price: &str) {
        apply(&self.book, &self.private, |b, now| {
            ((), b.fill(now, order_id, qty, price))
        });
    }

    /// [`Book::cancel_at_venue`]; the frame pushed only if `pushed`.
    pub fn cancel_at_venue(&self, order_id: &str, pushed: bool) {
        apply(&self.book, &self.private, |b, now| {
            let frames = b.cancel_at_venue(now, order_id);
            ((), if pushed { frames } else { Vec::new() })
        });
    }

    /// The next `order.create` is booked, `hook` runs, and neither a reply
    /// nor an `order` frame is sent for it.
    pub fn silent_for_next_create(&self, hook: Hook) {
        *self.silent_next.lock().unwrap() = Some(hook);
    }
}

/// A ticker snapshot after each new subscribe on the public stream, then
/// one every [`TICKER_EVERY_MS`] of the steady clock; its `ts` is the
/// wall clock. Ends with the runtime.
async fn ticker_pump(book: Arc<Mutex<Book>>, public: Arc<FakeWs>) {
    let (mut subscribes, mut last) = (0, tokio::time::Instant::now());
    loop {
        tokio::time::sleep(Duration::from_millis(50)).await;
        let n = public.received("subscribe").len();
        if n > subscribes || (n > 0 && last.elapsed() >= Duration::from_millis(TICKER_EVERY_MS)) {
            subscribes = n;
            last = tokio::time::Instant::now();
            let t = book.lock().unwrap().ticker(now_ms());
            public.try_send(t);
        }
    }
}
