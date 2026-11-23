//! The engine rig (U19a5): the in-process model venue, the test journal
//! and the engine on a manual clock. `at(t)` polls the engine at `t` and
//! answers every request at once (zero latency): venue requests from the
//! model venue, status requests with the journal's last record as the
//! durable cap. A test that needs to hold an answer calls the engine
//! itself. `Mismatch` is answered as the OMS answers `watcher_mismatch`
//! (U19a7): the first one while no hold is set writes `hold_set` to the
//! journal and is `"held"`, a later one `"already held"`; or from a script,
//! or not at all ([`Oms::Silent`]).

use super::{TestJournal, ev};
use serde_json::{Value, json};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use watcher::config::Config;
use watcher::engine::{Action, Check, Engine};
use watcher::journal::Tail;
use watcher::metrics::Metrics;
use watcher::rest::RestError;
use watcher::state::JournalState;
use watcher::wire_id;

/// 2023-11-14T22:13:20Z: the engine's start.
pub const T0: u64 = 1_700_000_000_000;
/// The config load, 5 s before the start.
pub const BOOT: u64 = T0 - 5_000;
/// The instance of [`TestJournal`].
pub const INSTANCE: u64 = 7;

/// A heartbeat the rig saw: when, and each check's success time.
pub type Beat = (u64, Vec<(Check, Option<u64>)>);

pub fn config() -> Config {
    Config::from_toml(
        r#"name = "w"
network = "testnet"
credentials_file = "/nonexistent-watcher-engine/creds.toml"
sub_account_uid = 1
symbols = ["BTCUSDT"]
ip_echo_url = "https://ip.example/"
data_dir = "/nonexistent-watcher-engine/data"
status_url = "http://127.0.0.1:8188/api/status"
host = "h"
"#,
    )
    .unwrap()
}

pub fn accepted(id: u64, instrument: &str, side: &str) -> Value {
    ev(
        "order_accepted",
        json!({"order_id": id, "request": {"client_order_id": format!("k{id}"),
            "instrument": instrument, "qty": "1", "side": side, "strategy": "a"}}),
    )
}

pub fn acked(id: u64) -> Value {
    ev(
        "order_acked",
        json!({"order_id": id, "venue_oid": format!("v{id}")}),
    )
}

/// `order_accepted` and `order_acked` of a BTCUSDT buy.
pub fn open(id: u64) -> [Value; 2] {
    [accepted(id, "BTCUSDT", "buy"), acked(id)]
}

/// `order_closed`, `order_in_doubt` or `venue_terminal` of an order.
pub fn of(name: &str, id: u64) -> Value {
    ev(name, json!({"order_id": id}))
}

/// An open venue row.
pub fn row(link: &str, order_id: &str, symbol: &str, side: &str) -> Value {
    json!({"orderLinkId": link, "orderId": order_id, "symbol": symbol, "side": side,
        "qty": "1", "orderStatus": "New"})
}

/// The venue row of journal order `id` (orderId `v<id>`).
pub fn ours(id: u64, symbol: &str, side: &str) -> Value {
    row(
        &wire_id::encode(INSTANCE, id),
        &format!("v{id}"),
        symbol,
        side,
    )
}

pub fn link(id: u64) -> String {
    wire_id::encode(INSTANCE, id)
}

/// A page as the venue writes it.
pub fn page(rows: Vec<Value>, cursor: &str) -> Result<Value, RestError> {
    Ok(json!({"category": "linear", "list": rows, "nextPageCursor": cursor}))
}

/// A `/v5/position/list` row (U19a6); `updated`: `updatedTime` (ms).
pub fn pos(symbol: &str, side: &str, size: &str, updated: Option<u64>) -> Value {
    let mut r = json!({"symbol": symbol, "side": side, "size": size, "positionIdx": 0});
    if let Some(t) = updated {
        r["updatedTime"] = json!(t.to_string());
    }
    r
}

/// The model venue: its open orders, served 50 a page (a full page has a
/// cursor `p<next index>`), and its positions, served 200 a page
/// (U19a6); scripted answers go first, one script per path.
#[derive(Default)]
pub struct Venue {
    pub orders: Vec<Value>,
    pub script: VecDeque<Result<Value, RestError>>,
    pub positions: Vec<Value>,
    pub pos_script: VecDeque<Result<Value, RestError>>,
    pub requests: Vec<(&'static str, Vec<(&'static str, String)>)>,
}

impl Venue {
    pub fn answer(
        &mut self,
        path: &'static str,
        params: &[(&'static str, String)],
    ) -> Result<Value, RestError> {
        self.requests.push((path, params.to_vec()));
        let positions = path == "/v5/position/list";
        let (script, all, n) = if positions {
            (&mut self.pos_script, &self.positions, 200)
        } else {
            (&mut self.script, &self.orders, 50)
        };
        if let Some(a) = script.pop_front() {
            return a;
        }
        let from = params
            .iter()
            .find(|(k, _)| *k == "cursor")
            .map(|(_, v)| v[1..].parse::<usize>().unwrap())
            .unwrap_or(0);
        let rows: Vec<Value> = all.iter().skip(from).take(n).cloned().collect();
        let next = if rows.len() == n {
            format!("p{}", from + n)
        } else {
            String::new()
        };
        page(rows, &next)
    }
}

/// How the rig answers `Mismatch` (U19a7).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Oms {
    /// As the OMS: `hold_set` and `"held"`, or `"already held"`.
    Holds,
    /// No answer: the test calls `engine.on_admin` itself.
    Silent,
}

pub struct Rig {
    pub engine: Engine,
    pub venue: Venue,
    pub journal: TestJournal,
    pub metrics: Arc<Metrics>,
    pub lines: Arc<Mutex<Vec<String>>>,
    /// The cap of the status answers; `None`: the journal's last record.
    pub cap: Option<u64>,
    pub instance: u64,
    /// Set: the OMS does not answer `/api/status` (U19a6).
    pub status_down: Option<String>,
    pub mismatches: Vec<(u64, Check, String)>,
    /// The `send_id` of each entry of `mismatches`.
    pub send_ids: Vec<u64>,
    pub heartbeats: Vec<Beat>,
    pub oms: Oms,
    /// Answers to `Mismatch` used before `oms`, in order.
    pub admin_script: VecDeque<Result<String, String>>,
    /// The OMS holds plain orders on a watcher's word.
    pub held: bool,
    /// Each confirmed difference (the engine's `error: confirmed
    /// difference` line) with the clock it came at: the two-round rule's
    /// result, whether or not a send carried it at once (U19a7).
    pub confirmed: Vec<(u64, String)>,
}

impl Rig {
    /// An engine that starts at `T0` with W1 on an empty journal.
    pub fn new(name: &str) -> Rig {
        Rig::with_config(name, &config())
    }

    /// As [`Rig::new`] with another config.
    pub fn with_config(name: &str, cfg: &Config) -> Rig {
        let mut journal = TestJournal::new(name);
        journal.segment(2);
        let metrics = Arc::new(Metrics::new("h", "w"));
        let (log, lines) = watcher::log::collect();
        let engine = Engine::new(
            cfg,
            Tail::open(&journal.dir),
            JournalState::default(),
            metrics.clone(),
            log,
            BOOT,
            T0,
        );
        Rig {
            engine,
            venue: Venue::default(),
            journal,
            metrics,
            lines,
            cap: None,
            instance: INSTANCE,
            status_down: None,
            mismatches: Vec::new(),
            send_ids: Vec::new(),
            heartbeats: Vec::new(),
            oms: Oms::Holds,
            admin_script: VecDeque::new(),
            held: false,
            confirmed: Vec::new(),
        }
    }

    /// The OMS's answer to `watcher_mismatch`: the first while no hold is
    /// set writes `hold_set` (its note is the detail) at the journal's
    /// newest time, so that a test's later records keep their order.
    pub fn hold(&mut self, check: Check, detail: &str) -> Result<String, String> {
        if self.held {
            return Ok("already held".into());
        }
        self.held = true;
        let at = self.journal.last_at;
        self.journal.append(
            at,
            &[ev(
                "hold_set",
                json!({"hold": "watcher_mismatch", "source": check.name(), "note": detail}),
            )],
        );
        Ok("held".into())
    }

    /// An operator's `clear-hold --reason watcher_mismatch` at `now_ms`.
    pub fn clear(&mut self, now_ms: u64) {
        self.held = false;
        let at = (now_ms * 1000).max(self.journal.last_at);
        self.journal.append(
            at,
            &[ev(
                "hold_cleared",
                json!({"hold": "watcher_mismatch", "note": "checked"}),
            )],
        );
    }

    /// A record at `at_ms`.
    pub fn write(&mut self, at_ms: u64, events: &[Value]) -> u64 {
        self.journal.append(at_ms * 1000, events)
    }

    pub fn status(&self) -> Value {
        let cap = self.cap.unwrap_or(self.journal.next_seq - 1);
        json!({"journal": {"durable_seq": cap},
            "core": {"instance_id": format!("{:016x}", self.instance)}})
    }

    /// Polls at `now` and answers everything at once.
    pub fn at(&mut self, now: u64) {
        let actions = self.engine.poll(now);
        self.run(now, actions);
    }

    /// Carries out `actions` and the actions their answers bring.
    pub fn run(&mut self, now: u64, actions: Vec<Action>) {
        let lines_before = self.lines.lock().unwrap().len();
        let mut queue: VecDeque<Action> = actions.into();
        let mut steps = 0;
        while let Some(a) = queue.pop_front() {
            steps += 1;
            assert!(steps < 10_000, "the rig does not settle");
            let more = match a {
                Action::Venue {
                    req_id,
                    path,
                    params,
                } => {
                    let answer = self.venue.answer(path, &params);
                    self.engine.on_venue(now, req_id, answer)
                }
                Action::Status { req_id } => {
                    let s = match &self.status_down {
                        Some(why) => Err(why.clone()),
                        None => Ok(self.status()),
                    };
                    self.engine.on_status(now, req_id, s)
                }
                Action::Mismatch {
                    check,
                    send_id,
                    detail,
                } => {
                    self.mismatches.push((now, check, detail.clone()));
                    self.send_ids.push(send_id);
                    let answer = match (self.admin_script.pop_front(), self.oms) {
                        (Some(a), _) => Some(a),
                        (None, Oms::Holds) => Some(self.hold(check, &detail)),
                        (None, Oms::Silent) => None,
                    };
                    match answer {
                        Some(a) => self.engine.on_admin(now, check, send_id, a),
                        None => vec![],
                    }
                }
                Action::Heartbeat { checks } => {
                    self.heartbeats.push((now, checks));
                    vec![]
                }
            };
            queue.extend(more);
        }
        let lines = self.lines.lock().unwrap();
        for l in &lines[lines_before..] {
            if let Some(d) = l.strip_prefix("error: confirmed difference: ") {
                self.confirmed.push((now, d.to_string()));
            }
        }
    }

    pub fn gauge(&self, name: &str, check: &str) -> Option<f64> {
        self.metrics.get(name, &[("check", check)])
    }

    /// The log lines that contain `pat`.
    pub fn lines_with(&self, pat: &str) -> usize {
        self.lines
            .lock()
            .unwrap()
            .iter()
            .filter(|l| l.contains(pat))
            .count()
    }
}
