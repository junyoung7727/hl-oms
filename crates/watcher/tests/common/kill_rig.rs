//! The kill rig (U19b2c, spec "Test rig"): the watcher's kill loop
//! against `http-fake` on the manual clock, through the production
//! executor `kill::Inflight` and a real `Client`, so a rig test runs the
//! abort rule the runtime runs. Each route has scripted answers; an answer
//! may move the manual clock when its request arrives (a slow venue), and
//! the fake's `on_request` hook records the manual clock at each request,
//! so the order-rate assertions use the loop's clock, not real time.
//!
//! A step: `abort_due` (before the loop's next input, as the runtime),
//! `poll`, start the requests the loop emitted (never while a task is
//! open: two requests of the loop at once fail the test), then wait in
//! real time until every open request has delivered what it will deliver
//! (each page and answer fed to the loop at the clock when it arrives;
//! review 4 S1), except a request whose scripted answer is
//! `Answer::Hang`, and only then move the clock to
//! `min(next_wake, now + 100)`. A request the loop emits while the rig
//! waits starts at the next step, after the earlier task ended. So no
//! outcome depends on how fast the fake answers.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use http_fake::{Answer, FakeHttp, bybit};
use serde_json::Value;
use watcher::clock::{Clock, ManualClock};
use watcher::config::Kill;
use watcher::kill::{Action, Arrived, Inflight, KillLoop, Notice, Report, StopOutcome};
use watcher::metrics::Metrics;
use watcher::rest::{CANCEL_ALL, Client, ClientConfig, Credentials};

/// 2023-11-14T22:13:20Z: the loop's start.
pub const T0: u64 = 1_700_000_000_000;
/// `request_timeout_ms`: a request's deadline is its emission (a count:
/// its last page) + 13,000 ms.
pub const RT: u64 = 2000;
pub const EXTRA: u64 = RT + 11_000;
/// The step bound of every run (판단(모델 아님)).
pub const MAX_STEPS: usize = 20_000;
/// The real-time net under a wrong request count: the guard (60 per 10 s
/// of the manual clock) would hold a request forever (판단(모델 아님)).
pub const SETTLE: Duration = Duration::from_secs(5);
/// The clock's largest step.
pub const STEP_MS: u64 = 100;

/// One scripted answer, and how far the manual clock moves when its
/// request arrives.
#[derive(Debug, Clone)]
pub struct Reply {
    pub answer: Answer,
    pub advance_ms: u64,
}

pub fn at_once(answer: Answer) -> Reply {
    Reply {
        answer,
        advance_ms: 0,
    }
}

pub fn slow(answer: Answer, advance_ms: u64) -> Reply {
    Reply { answer, advance_ms }
}

/// A cancel-all that Bybit accepted with `n` orders in its list.
pub fn cancel_ok(n: usize) -> Answer {
    let list: Vec<(String, String)> = (0..n).map(|i| (format!("o{i}"), String::new())).collect();
    bybit::ok(bybit::cancel_all(&list))
}

/// One page of `/v5/order/realtime` with `n` open orders.
pub fn open_orders(n: usize, cursor: &str) -> Answer {
    let list: Vec<Value> = (0..n)
        .map(|i| bybit::order_row(&format!("l{i}"), &format!("o{i}"), "BTCUSDT", "Buy", "1", "New"))
        .collect();
    bybit::list(list, cursor)
}

pub fn http(status: u16) -> Answer {
    Answer::Reply(status, "busy".into())
}

#[derive(Default)]
struct Seen {
    /// The manual clock when each request arrived (before its answer moved
    /// it), and its path.
    requests: Vec<(u64, String)>,
    per_path: HashMap<String, usize>,
    hangs: usize,
}

pub struct KillRig {
    pub fake: FakeHttp,
    pub clock: Arc<ManualClock>,
    pub client: Arc<Client>,
    pub metrics: Arc<Metrics>,
    pub inflight: Inflight,
    pub lp: KillLoop,
    /// Every action of the loop and the clock when it came.
    pub actions: Vec<(u64, Action)>,
    /// Everything `Inflight` gave the loop and the clock when it did.
    pub arrived: Vec<(u64, Arrived)>,
    seen: Arc<Mutex<Seen>>,
    /// The requests to start, and the clock the loop emitted each at.
    pending: Vec<(u64, Action)>,
    hangs_at_start: usize,
}

impl KillRig {
    pub fn new(routes: Vec<(&str, Vec<Reply>)>) -> KillRig {
        KillRig::with(Kill::default(), routes)
    }

    pub fn with(timing: Kill, routes: Vec<(&str, Vec<Reply>)>) -> KillRig {
        let fake = FakeHttp::start(Answer::Close);
        let clock = Arc::new(ManualClock::new(T0));
        let mut scripts: HashMap<String, Vec<Reply>> = HashMap::new();
        for (path, replies) in routes {
            assert!(!replies.is_empty(), "a route without answers: {path}");
            fake.route(path, replies.iter().map(|r| r.answer.clone()));
            scripts.insert(path.to_string(), replies);
        }
        let seen = Arc::new(Mutex::new(Seen::default()));
        let (s, c) = (seen.clone(), clock.clone());
        // As http-fake: the answers in order, then the last one again.
        fake.on_request(move |r| {
            let mut s = s.lock().unwrap();
            let path = r.path().to_string();
            s.requests.push((c.now_ms(), path.clone()));
            let k = s.per_path.entry(path.clone()).or_insert(0);
            let i = *k;
            *k += 1;
            if let Some(replies) = scripts.get(&path) {
                let reply = &replies[i.min(replies.len() - 1)];
                if reply.answer == Answer::Hang {
                    s.hangs += 1;
                }
                c.advance(reply.advance_ms);
            }
        });
        let metrics = Arc::new(Metrics::new("h1", "w1"));
        let (log, _lines) = watcher::log::collect();
        let cfg = ClientConfig {
            base: fake.url(),
            recv_window_ms: 5000,
            request_timeout_ms: RT,
        };
        let creds = Credentials::new("WKEY", "watcher-made-up-secret-42");
        let client =
            Arc::new(Client::new(&cfg, creds, clock.clone(), metrics.clone(), log).unwrap());
        let inflight = Inflight::new(client.clone(), clock.clone(), RT);
        KillRig {
            fake,
            clock,
            client,
            metrics,
            inflight,
            lp: KillLoop::new(timing, RT),
            actions: Vec::new(),
            arrived: Vec::new(),
            seen,
            pending: Vec::new(),
            hangs_at_start: 0,
        }
    }

    /// Starts the loop at the clock (outcome `Latched`).
    pub fn start(&mut self) {
        let now = self.clock.now_ms();
        let acts = self.lp.start(now, "rig", StopOutcome::Latched);
        self.handle(now, acts);
    }

    /// Starts the loop and steps until it ended; returns its report.
    pub async fn run(&mut self) -> Report {
        self.start();
        self.run_until(|_| false).await;
        self.report().unwrap()
    }

    /// Steps until the loop ended or `stop` holds after a step; fails the
    /// test at the step bound.
    pub async fn run_until(&mut self, stop: impl Fn(&KillRig) -> bool) {
        for _ in 0..MAX_STEPS {
            let now = self.clock.now_ms();
            self.inflight.abort_due(now, self.lp.open_request());
            let acts = self.lp.poll(now);
            self.handle(now, acts);
            self.start_pending();
            self.settle().await;
            if self.report().is_some() || stop(self) {
                return;
            }
            if !self.pending.is_empty() {
                continue;
            }
            let now = self.clock.now_ms();
            let next = self
                .lp
                .next_wake()
                .unwrap_or(now + STEP_MS)
                .min(now + STEP_MS);
            self.clock.set(next.max(now));
        }
        panic!("the kill rig reached {MAX_STEPS} steps");
    }

    fn handle(&mut self, now: u64, acts: Vec<Action>) {
        for a in acts {
            if matches!(a, Action::CancelAll { .. } | Action::Count { .. }) {
                self.pending.push((now, a.clone()));
            }
            self.actions.push((now, a));
        }
    }

    fn start_pending(&mut self) {
        for (at, a) in std::mem::take(&mut self.pending) {
            assert_eq!(
                self.inflight.open(),
                0,
                "two requests of the loop at once: {a:?}"
            );
            self.hangs_at_start = self.seen.lock().unwrap().hangs;
            assert!(self.inflight.start(&a, at));
        }
    }

    fn hanging(&self) -> bool {
        self.seen.lock().unwrap().hangs > self.hangs_at_start
    }

    /// Feeds what arrived until no task is open, or the open one hangs.
    async fn settle(&mut self) {
        let end = Instant::now() + SETTLE;
        loop {
            for a in self.inflight.take() {
                let now = self.clock.now_ms();
                self.arrived.push((now, a.clone()));
                let acts = self.lp.arrived(now, a);
                self.handle(now, acts);
            }
            if self.inflight.open() == 0 || self.hanging() {
                return;
            }
            assert!(
                Instant::now() < end,
                "the guard held a request: the rig's clock did not move"
            );
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    }

    /// Every request at the fake: the clock when it arrived, and its path.
    pub fn requests(&self) -> Vec<(u64, String)> {
        self.seen.lock().unwrap().requests.clone()
    }

    pub fn paths(&self) -> Vec<String> {
        self.requests().into_iter().map(|(_, p)| p).collect()
    }

    /// The clock at each cancel-all that reached the fake.
    pub fn cancel_all_at(&self) -> Vec<u64> {
        self.requests()
            .into_iter()
            .filter(|(_, p)| p == CANCEL_ALL)
            .map(|(t, _)| t)
            .collect()
    }

    /// At most 2 cancel-alls reached the fake in any `[t, t + 1,000)` ms.
    pub fn assert_at_most_2_cancel_alls_per_second(&self) {
        let at = self.cancel_all_at();
        for &t in &at {
            let n = at.iter().filter(|&&u| u >= t && u < t + 1000).count();
            assert!(n <= 2, "{n} cancel-alls in [{t}, {t} + 1,000): {at:?}");
        }
    }

    pub fn report(&self) -> Option<Report> {
        self.actions.iter().find_map(|(_, a)| match a {
            Action::Ended(r) => Some(r.clone()),
            _ => None,
        })
    }

    pub fn notices(&self) -> Vec<(u64, Notice)> {
        self.actions
            .iter()
            .filter_map(|(t, a)| match a {
                Action::Notice(n) => Some((*t, n.clone())),
                _ => None,
            })
            .collect()
    }
}
