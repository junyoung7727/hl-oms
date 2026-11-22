//! Strategy liveness (C-LIVE): which strategies still have a live trading
//! link, as this process sees it.
//!
//! The table is kept per strategy listed in `[liveness]`. A strategy's link
//! is lost when its last trading connection that said `hello` closes, or
//! when no request came on any of its connections for its liveness time.
//! A loss is told to the core once, as `Input::Session(Lost)`, until the
//! strategy's next `hello`; the core then holds that strategy's plain orders
//! and asks to cancel its resting plain orders. Admin connections never
//! count.
//!
//! The table has no clock and no queue of its own: time is an argument, and
//! a loss is handed to `push`, which answers whether the core's queue took
//! it. A loss the queue refused waits and is offered again at the next
//! `tick`; it is never dropped.

use oms_core::{Input, SessionInput, StrategyId};
use std::collections::BTreeMap;
use std::time::{Duration, Instant};

/// How often the node's timer calls `tick`. 판단(모델 아님): a quarter of
/// the shortest liveness time allowed (1,000 ms).
pub const TICK_MS: u64 = 250;

/// The reason a loss names when the last connection closed.
pub const WHY_CLOSED: &str = "last trading connection closed";

#[derive(Clone, Debug, PartialEq, Eq)]
enum Lost {
    /// No loss since the last `hello` (or since the start).
    No,
    /// A loss the core's queue refused, with its reason: offered again at
    /// the next tick.
    Waiting(String),
    /// The loss is in the core's queue, or the core holds the strategy
    /// already (a hold replayed from before a restart).
    Reported,
}

#[derive(Clone, Debug)]
struct Entry {
    liveness_ms: u64,
    /// Open trading connections that said `hello` for this strategy.
    connections: u32,
    /// The last parsed request on any of them. `None`: not armed (no
    /// `hello` yet in this process, and no open plain order at the start).
    last_request: Option<Instant>,
    lost: Lost,
}

/// What a `hello` must do before it is answered.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HelloPlan {
    /// A loss that waits for room in the queue: it goes first, so that the
    /// core applies it before the restore (both on the control lane).
    pub lost_first: Option<Input>,
    /// The hello is answered only after `Input::Session(Hello)` is durable:
    /// the node reported a loss since the last restore, or the core holds
    /// the strategy.
    pub restore: bool,
}

/// What `status.strategies.<s>.link` shows of the table (process state).
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct LinkView {
    pub liveness_ms: u64,
    pub connections: u32,
    /// Null while not armed.
    pub last_request_age_ms: Option<u64>,
}

#[derive(Clone, Debug, Default)]
pub struct Liveness {
    entries: BTreeMap<StrategyId, Entry>,
}

fn lost_input(strategy: &StrategyId, why: &str) -> Input {
    Input::Session(SessionInput::Lost {
        strategy: strategy.clone(),
        why: why.to_string(),
    })
}

impl Liveness {
    /// One entry per listed strategy, with its liveness time in ms.
    pub fn new(listed: &BTreeMap<StrategyId, u64>) -> Liveness {
        let entries = listed
            .iter()
            .map(|(s, ms)| {
                (
                    s.clone(),
                    Entry {
                        liveness_ms: *ms,
                        connections: 0,
                        last_request: None,
                        lost: Lost::No,
                    },
                )
            })
            .collect();
        Liveness { entries }
    }

    /// At the start of the process, from the published core: a listed
    /// strategy with open plain orders is armed from `now` (they need
    /// protecting); one without is armed by its first `hello`. A strategy
    /// the core holds already counts as reported.
    pub fn start(
        &mut self,
        now: Instant,
        has_open_plain_orders: impl Fn(&StrategyId) -> bool,
        held: impl Fn(&StrategyId) -> bool,
    ) {
        for (s, e) in &mut self.entries {
            if has_open_plain_orders(s) {
                e.last_request = Some(now);
            }
            if held(s) {
                e.lost = Lost::Reported;
            }
        }
    }

    /// What a `hello` for `strategy` on the trading socket must do. Also
    /// for a strategy that is not listed: a hold of the core is released
    /// all the same.
    pub fn hello_plan(&self, strategy: &StrategyId, held_in_core: bool) -> HelloPlan {
        let Some(e) = self.entries.get(strategy) else {
            return HelloPlan {
                lost_first: None,
                restore: held_in_core,
            };
        };
        let lost_first = match &e.lost {
            Lost::Waiting(why) => Some(lost_input(strategy, why)),
            Lost::No | Lost::Reported => None,
        };
        HelloPlan {
            lost_first,
            restore: held_in_core || e.lost != Lost::No,
        }
    }

    /// The plan's inputs are in the queue (or there were none): the
    /// strategy is live from `now`. `new_connection` counts the connection;
    /// a second `hello` on the same connection does not.
    pub fn hello_done(&mut self, strategy: &StrategyId, now: Instant, new_connection: bool) {
        if let Some(e) = self.entries.get_mut(strategy) {
            e.lost = Lost::No;
            e.last_request = Some(now);
            if new_connection {
                e.connections += 1;
            }
        }
    }

    /// The plan's waiting loss went into the queue but the restore did
    /// not: the hello is refused, and the next one restores.
    pub fn lost_sent(&mut self, strategy: &StrategyId) {
        if let Some(e) = self.entries.get_mut(strategy) {
            e.lost = Lost::Reported;
        }
    }

    /// A parsed request on a connection of `strategy`, after its `hello`.
    pub fn request(&mut self, strategy: &StrategyId, now: Instant) {
        if let Some(e) = self.entries.get_mut(strategy) {
            e.last_request = Some(now);
        }
    }

    /// A connection of `strategy` that said `hello` closed. The last one
    /// reports a loss.
    pub fn closed(&mut self, strategy: &StrategyId, push: &mut dyn FnMut(Input) -> bool) {
        let Some(e) = self.entries.get_mut(strategy) else {
            return;
        };
        e.connections = e.connections.saturating_sub(1);
        if e.connections == 0 && e.last_request.is_some() && e.lost == Lost::No {
            report(strategy, e, WHY_CLOSED.to_string(), push);
        }
    }

    /// Offers the waiting losses again, and reports every armed strategy
    /// without a request for longer than its liveness time.
    pub fn tick(&mut self, now: Instant, push: &mut dyn FnMut(Input) -> bool) {
        for (s, e) in &mut self.entries {
            match &e.lost {
                Lost::Waiting(why) => {
                    let why = why.clone();
                    report(s, e, why, push);
                }
                Lost::Reported => {}
                Lost::No => {
                    let Some(last) = e.last_request else { continue };
                    let silent = now.saturating_duration_since(last);
                    if silent > Duration::from_millis(e.liveness_ms) {
                        let why = format!("no request for {} ms", e.liveness_ms);
                        report(s, e, why, push);
                    }
                }
            }
        }
    }

    /// The table's part of `status.strategies.<s>.link`; `None` for a
    /// strategy that is not listed.
    pub fn view(&self, strategy: &StrategyId, now: Instant) -> Option<LinkView> {
        let e = self.entries.get(strategy)?;
        Some(LinkView {
            liveness_ms: e.liveness_ms,
            connections: e.connections,
            last_request_age_ms: e.last_request.map(|t| {
                u64::try_from(now.saturating_duration_since(t).as_millis()).unwrap_or(u64::MAX)
            }),
        })
    }
}

fn report(strategy: &StrategyId, e: &mut Entry, why: String, push: &mut dyn FnMut(Input) -> bool) {
    e.lost = if push(lost_input(strategy, &why)) {
        Lost::Reported
    } else {
        Lost::Waiting(why)
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sid(s: &str) -> StrategyId {
        StrategyId::new(s).unwrap()
    }

    /// Strategies `a` (5,000 ms) and `b` (3,000 ms) are listed.
    fn table() -> Liveness {
        let mut listed = BTreeMap::new();
        listed.insert(sid("a"), 5_000);
        listed.insert(sid("b"), 3_000);
        Liveness::new(&listed)
    }

    /// The inputs a push took, and whether it takes the next ones.
    struct Queue {
        took: Vec<Input>,
        full: bool,
    }

    impl Queue {
        fn new() -> Queue {
            Queue {
                took: Vec::new(),
                full: false,
            }
        }

        fn push(&mut self) -> impl FnMut(Input) -> bool + '_ {
            |i| {
                if self.full {
                    false
                } else {
                    self.took.push(i);
                    true
                }
            }
        }

        fn lost(&self) -> Vec<(String, String)> {
            self.took
                .iter()
                .map(|i| match i {
                    Input::Session(SessionInput::Lost { strategy, why }) => {
                        (strategy.to_string(), why.clone())
                    }
                    other => panic!("{other:?}"),
                })
                .collect()
        }
    }

    fn ms(t0: Instant, ms: u64) -> Instant {
        t0 + Duration::from_millis(ms)
    }

    fn hello(t: &mut Liveness, s: &str, at: Instant) {
        let plan = t.hello_plan(&sid(s), false);
        assert_eq!(plan.lost_first, None);
        t.hello_done(&sid(s), at, true);
    }

    /// The last connection's close reports one loss; a later close, tick
    /// or silence reports nothing more until the next `hello`.
    #[test]
    fn the_last_close_reports_one_loss_until_the_next_hello() {
        let t0 = Instant::now();
        let mut t = table();
        let mut q = Queue::new();
        hello(&mut t, "a", t0);
        t.closed(&sid("a"), &mut q.push());
        assert_eq!(q.lost(), [("a".to_string(), WHY_CLOSED.to_string())]);
        t.tick(ms(t0, 60_000), &mut q.push());
        t.closed(&sid("a"), &mut q.push());
        assert_eq!(q.took.len(), 1);
        assert!(t.hello_plan(&sid("a"), false).restore, "the hello restores");

        hello(&mut t, "a", ms(t0, 61_000));
        assert!(!t.hello_plan(&sid("a"), false).restore);
        // The hello moved the clock on: silence is measured from it, not
        // from the first hello.
        t.tick(ms(t0, 66_000), &mut q.push());
        assert_eq!(q.took.len(), 1, "silence is measured from the hello");
        t.closed(&sid("a"), &mut q.push());
        assert_eq!(q.took.len(), 2, "a new loss after the hello");
    }

    /// Silence longer than the liveness time reports a loss; exactly the
    /// liveness time does not. Any request, `ping` included, moves the
    /// clock on (the server calls `request` for every parsed line).
    #[test]
    fn no_request_for_liveness_ms_reports_a_loss_and_any_request_keeps_it_alive() {
        let t0 = Instant::now();
        let mut t = table();
        let mut q = Queue::new();
        hello(&mut t, "a", t0);
        hello(&mut t, "b", t0);
        t.tick(ms(t0, 3_000), &mut q.push());
        assert!(q.took.is_empty(), "b at exactly 3,000 ms is not lost");
        t.tick(ms(t0, 3_001), &mut q.push());
        assert_eq!(
            q.lost(),
            [("b".to_string(), "no request for 3000 ms".to_string())]
        );
        t.request(&sid("a"), ms(t0, 4_000));
        t.tick(ms(t0, 9_000), &mut q.push());
        assert_eq!(q.took.len(), 1, "a's request at 4 s keeps it until 9 s");
        t.tick(ms(t0, 9_001), &mut q.push());
        assert_eq!(
            q.lost()[1],
            ("a".to_string(), "no request for 5000 ms".to_string())
        );
        t.tick(ms(t0, 60_000), &mut q.push());
        assert_eq!(q.took.len(), 2, "once until the next hello");
        assert_eq!(
            t.view(&sid("a"), ms(t0, 9_001))
                .unwrap()
                .last_request_age_ms,
            Some(5_001)
        );
    }

    /// A second connection keeps the strategy alive when the first one
    /// closes; the close of the last one reports.
    #[test]
    fn a_second_connection_keeps_the_strategy_alive() {
        let t0 = Instant::now();
        let mut t = table();
        let mut q = Queue::new();
        hello(&mut t, "a", t0);
        hello(&mut t, "a", t0);
        // A second hello on the same connection does not count it twice.
        t.hello_done(&sid("a"), t0, false);
        assert_eq!(t.view(&sid("a"), t0).unwrap().connections, 2);
        t.closed(&sid("a"), &mut q.push());
        assert!(q.took.is_empty());
        t.closed(&sid("a"), &mut q.push());
        assert_eq!(q.lost(), [("a".to_string(), WHY_CLOSED.to_string())]);
    }

    /// A loss the queue refused waits and is offered at every tick until
    /// the queue takes it; it is never dropped.
    #[test]
    fn a_full_queue_is_retried_on_the_next_tick() {
        let t0 = Instant::now();
        let mut t = table();
        let mut q = Queue::new();
        hello(&mut t, "a", t0);
        q.full = true;
        t.closed(&sid("a"), &mut q.push());
        t.tick(ms(t0, 250), &mut q.push());
        assert!(q.took.is_empty());
        q.full = false;
        t.tick(ms(t0, 500), &mut q.push());
        assert_eq!(q.lost(), [("a".to_string(), WHY_CLOSED.to_string())]);
        t.tick(ms(t0, 750), &mut q.push());
        assert_eq!(q.took.len(), 1);
    }

    /// A `hello` after a loss is answered after the restore; a loss still
    /// waiting for the queue goes first. The core's own hold asks for the
    /// restore too, also for a strategy that is not listed.
    #[test]
    fn hello_after_a_loss_asks_for_the_restore_and_a_waiting_loss_goes_first() {
        let t0 = Instant::now();
        let mut t = table();
        let mut q = Queue::new();
        hello(&mut t, "a", t0);
        q.full = true;
        t.closed(&sid("a"), &mut q.push());
        let plan = t.hello_plan(&sid("a"), false);
        assert_eq!(plan.lost_first, Some(lost_input(&sid("a"), WHY_CLOSED)));
        assert!(plan.restore);
        // The waiting loss went in, the restore did not: the next hello
        // still restores, with nothing waiting.
        t.lost_sent(&sid("a"));
        let plan = t.hello_plan(&sid("a"), false);
        assert_eq!((plan.lost_first, plan.restore), (None, true));
        t.hello_done(&sid("a"), t0, true);
        assert!(!t.hello_plan(&sid("a"), false).restore);
        assert!(t.hello_plan(&sid("a"), true).restore, "held in the core");

        let unlisted = sid("zz");
        assert!(!t.hello_plan(&unlisted, false).restore);
        assert!(t.hello_plan(&unlisted, true).restore);
    }

    /// A strategy that is not listed has no liveness: its closes and its
    /// silence report nothing, and it has no view.
    #[test]
    fn a_strategy_that_is_not_listed_has_no_liveness() {
        let t0 = Instant::now();
        let mut t = table();
        let mut q = Queue::new();
        let c = sid("c");
        t.hello_done(&c, t0, true);
        t.request(&c, t0);
        t.closed(&c, &mut q.push());
        t.tick(ms(t0, 120_000), &mut q.push());
        assert!(q.took.is_empty());
        assert_eq!(t.view(&c, t0), None);
    }

    /// At the start only a listed strategy with open plain orders is
    /// armed, from the start; one that the core holds already is not
    /// reported again, and its hello restores.
    #[test]
    fn at_start_only_strategies_with_plain_orders_are_armed() {
        let t0 = Instant::now();
        let mut q = Queue::new();
        let mut t = table();
        t.start(t0, |s| *s == sid("a"), |_| false);
        assert_eq!(t.view(&sid("b"), t0).unwrap().last_request_age_ms, None);
        t.tick(ms(t0, 60_000), &mut q.push());
        assert_eq!(
            q.lost(),
            [("a".to_string(), "no request for 5000 ms".to_string())]
        );

        let mut t = table();
        let mut q = Queue::new();
        t.start(t0, |_| true, |s| *s == sid("a"));
        t.tick(ms(t0, 60_000), &mut q.push());
        assert_eq!(
            q.lost(),
            [("b".to_string(), "no request for 3000 ms".to_string())]
        );
        assert!(t.hello_plan(&sid("a"), false).restore);
    }
}
