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
    /// Restores in the queue whose answer has not come back. While one is
    /// in flight, a further `hello` restores too: its answer then waits
    /// for the first restore to be durable (the commit stage answers in
    /// order).
    restoring: u32,
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

/// What a `hello` did, for its answer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HelloOutcome {
    /// Nothing to restore: answer at once.
    Answer,
    /// The restore is in the queue: answer once it is answered (durable),
    /// then call `restore_answered`.
    AfterRestore,
    /// The queue refused the waiting loss or the restore: the hello is
    /// refused, and the connection does not count yet.
    Refused,
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
                        restoring: 0,
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

    /// A `hello` for `strategy` on the trading socket, on a connection that
    /// counted for `previous` (if any). A loss that waits for room goes
    /// into `push` first, then the restore, if one is needed, into
    /// `push_restore`; both go on the control lane, so the core applies the
    /// loss first. If either is refused, the table stays as it was (except
    /// that a loss that went in is no longer waiting) and the hello is
    /// refused. Otherwise a connection that counted for another strategy
    /// stops counting for it (its close rule applies) and the strategy is
    /// live from `now`.
    pub fn hello(
        &mut self,
        strategy: &StrategyId,
        held_in_core: bool,
        now: Instant,
        previous: Option<&StrategyId>,
        push: &mut dyn FnMut(Input) -> bool,
        push_restore: &mut dyn FnMut(Input) -> bool,
    ) -> HelloOutcome {
        let plan = self.hello_plan(strategy, held_in_core);
        let lost_first = plan.lost_first.is_some();
        if let Some(lost) = plan.lost_first {
            if !push(lost) {
                return HelloOutcome::Refused;
            }
        }
        if plan.restore {
            let restore = Input::Session(SessionInput::Hello {
                strategy: strategy.clone(),
            });
            if !push_restore(restore) {
                if lost_first {
                    self.lost_sent(strategy);
                }
                return HelloOutcome::Refused;
            }
        }
        let new_connection = previous != Some(strategy);
        if new_connection {
            if let Some(old) = previous {
                self.closed(old, push);
            }
        }
        self.hello_done(strategy, now, new_connection, plan.restore);
        if plan.restore {
            HelloOutcome::AfterRestore
        } else {
            HelloOutcome::Answer
        }
    }

    /// The answer to a restore that `hello` put in the queue came back
    /// (durable, or the process is stopping).
    pub fn restore_answered(&mut self, strategy: &StrategyId) {
        if let Some(e) = self.entries.get_mut(strategy) {
            e.restoring = e.restoring.saturating_sub(1);
        }
    }

    /// What a `hello` for `strategy` on the trading socket must do. Also
    /// for a strategy that is not listed: a hold of the core is released
    /// all the same.
    fn hello_plan(&self, strategy: &StrategyId, held_in_core: bool) -> HelloPlan {
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
            restore: held_in_core || e.lost != Lost::No || e.restoring > 0,
        }
    }

    /// The plan's inputs are in the queue (or there were none): the
    /// strategy is live from `now`. `new_connection` counts the connection;
    /// a second `hello` on the same connection does not. `restoring`: a
    /// restore went into the queue.
    fn hello_done(
        &mut self,
        strategy: &StrategyId,
        now: Instant,
        new_connection: bool,
        restoring: bool,
    ) {
        if let Some(e) = self.entries.get_mut(strategy) {
            e.lost = Lost::No;
            e.last_request = Some(now);
            if new_connection {
                e.connections += 1;
            }
            if restoring {
                e.restoring += 1;
            }
        }
    }

    /// The plan's waiting loss went into the queue but the restore did
    /// not: the hello is refused, and the next one restores.
    fn lost_sent(&mut self, strategy: &StrategyId) {
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
    use std::cell::{Cell, RefCell};

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

    /// The control lane: the inputs it took, in order, and whether it
    /// takes the next ones.
    struct Queue {
        took: RefCell<Vec<Input>>,
        full: Cell<bool>,
    }

    impl Queue {
        fn new() -> Queue {
            Queue {
                took: RefCell::new(Vec::new()),
                full: Cell::new(false),
            }
        }

        fn push(&self) -> impl FnMut(Input) -> bool + '_ {
            |i| {
                if self.full.get() {
                    false
                } else {
                    self.took.borrow_mut().push(i);
                    true
                }
            }
        }

        /// The losses it took, as (strategy, why).
        fn lost(&self) -> Vec<(String, String)> {
            self.took
                .borrow()
                .iter()
                .filter_map(|i| match i {
                    Input::Session(SessionInput::Lost { strategy, why }) => {
                        Some((strategy.to_string(), why.clone()))
                    }
                    _ => None,
                })
                .collect()
        }

        /// Everything it took, as `lost <s>` / `hello <s>`, in order.
        fn order(&self) -> Vec<String> {
            self.took
                .borrow()
                .iter()
                .map(|i| match i {
                    Input::Session(SessionInput::Lost { strategy, .. }) => {
                        format!("lost {strategy}")
                    }
                    Input::Session(SessionInput::Hello { strategy }) => format!("hello {strategy}"),
                    other => panic!("{other:?}"),
                })
                .collect()
        }
    }

    fn ms(t0: Instant, ms: u64) -> Instant {
        t0 + Duration::from_millis(ms)
    }

    /// A `hello` for `s` on a new connection.
    fn hello(t: &mut Liveness, q: &Queue, s: &str, at: Instant) -> HelloOutcome {
        t.hello(&sid(s), false, at, None, &mut q.push(), &mut q.push())
    }

    /// The last connection's close reports one loss; a later close, tick
    /// or silence reports nothing more until the next `hello`.
    #[test]
    fn the_last_close_reports_one_loss_until_the_next_hello() {
        let t0 = Instant::now();
        let mut t = table();
        let q = Queue::new();
        assert_eq!(hello(&mut t, &q, "a", t0), HelloOutcome::Answer);
        t.closed(&sid("a"), &mut q.push());
        assert_eq!(q.lost(), [("a".to_string(), WHY_CLOSED.to_string())]);
        t.tick(ms(t0, 60_000), &mut q.push());
        t.closed(&sid("a"), &mut q.push());
        assert_eq!(q.lost().len(), 1);

        assert_eq!(
            hello(&mut t, &q, "a", ms(t0, 61_000)),
            HelloOutcome::AfterRestore,
            "the hello restores"
        );
        t.restore_answered(&sid("a"));
        assert!(!t.hello_plan(&sid("a"), false).restore);
        // The hello moved the clock on: silence is measured from it, not
        // from the first hello.
        t.tick(ms(t0, 66_000), &mut q.push());
        assert_eq!(q.lost().len(), 1, "silence is measured from the hello");
        t.closed(&sid("a"), &mut q.push());
        assert_eq!(q.lost().len(), 2, "a new loss after the hello");
    }

    /// Silence longer than the liveness time reports a loss; exactly the
    /// liveness time does not. Any request, `ping` included, moves the
    /// clock on (the server calls `request` for every parsed line).
    #[test]
    fn no_request_for_liveness_ms_reports_a_loss_and_any_request_keeps_it_alive() {
        let t0 = Instant::now();
        let mut t = table();
        let q = Queue::new();
        hello(&mut t, &q, "a", t0);
        hello(&mut t, &q, "b", t0);
        t.tick(ms(t0, 3_000), &mut q.push());
        assert!(q.lost().is_empty(), "b at exactly 3,000 ms is not lost");
        t.tick(ms(t0, 3_001), &mut q.push());
        assert_eq!(
            q.lost(),
            [("b".to_string(), "no request for 3000 ms".to_string())]
        );
        t.request(&sid("a"), ms(t0, 4_000));
        t.tick(ms(t0, 9_000), &mut q.push());
        assert_eq!(q.lost().len(), 1, "a's request at 4 s keeps it until 9 s");
        t.tick(ms(t0, 9_001), &mut q.push());
        assert_eq!(
            q.lost()[1],
            ("a".to_string(), "no request for 5000 ms".to_string())
        );
        t.tick(ms(t0, 60_000), &mut q.push());
        assert_eq!(q.lost().len(), 2, "once until the next hello");
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
    fn two_connections_count_and_only_the_last_close_reports() {
        let t0 = Instant::now();
        let mut t = table();
        let q = Queue::new();
        hello(&mut t, &q, "a", t0);
        hello(&mut t, &q, "a", t0);
        // A second hello on the same connection does not count it twice.
        let a = sid("a");
        t.hello(&a, false, t0, Some(&a), &mut q.push(), &mut q.push());
        assert_eq!(t.view(&a, t0).unwrap().connections, 2);
        t.closed(&a, &mut q.push());
        assert!(q.lost().is_empty());
        t.closed(&a, &mut q.push());
        assert_eq!(q.lost(), [("a".to_string(), WHY_CLOSED.to_string())]);
    }

    /// A connection that said hello for `a` and then says hello for `b`
    /// stops counting for `a` (review 1 S3): it was `a`'s last, so `a` is
    /// lost, before the hello for `b` is done.
    #[test]
    fn a_hello_for_another_strategy_closes_the_connection_for_the_first() {
        let t0 = Instant::now();
        let mut t = table();
        let q = Queue::new();
        hello(&mut t, &q, "a", t0);
        let (a, b) = (sid("a"), sid("b"));
        assert_eq!(
            t.hello(&b, false, t0, Some(&a), &mut q.push(), &mut q.push()),
            HelloOutcome::Answer
        );
        assert_eq!(q.lost(), [("a".to_string(), WHY_CLOSED.to_string())]);
        assert_eq!(t.view(&a, t0).unwrap().connections, 0);
        assert_eq!(t.view(&b, t0).unwrap().connections, 1);
    }

    /// A loss the queue refused waits and is offered at every tick until
    /// the queue takes it; it is never dropped.
    #[test]
    fn a_full_queue_is_retried_on_the_next_tick() {
        let t0 = Instant::now();
        let mut t = table();
        let q = Queue::new();
        hello(&mut t, &q, "a", t0);
        q.full.set(true);
        t.closed(&sid("a"), &mut q.push());
        t.tick(ms(t0, 250), &mut q.push());
        assert!(q.lost().is_empty());
        q.full.set(false);
        t.tick(ms(t0, 500), &mut q.push());
        assert_eq!(q.lost(), [("a".to_string(), WHY_CLOSED.to_string())]);
        t.tick(ms(t0, 750), &mut q.push());
        assert_eq!(q.lost().len(), 1);
    }

    /// A `hello` after a loss is answered after the restore; a loss still
    /// waiting for the queue goes in first, before the restore (review 1
    /// S3). A full queue refuses the hello: the connection does not count,
    /// and the next hello restores. The core's own hold asks for the
    /// restore too, also for a strategy that is not listed.
    #[test]
    fn hello_after_a_loss_asks_for_the_restore_and_a_waiting_loss_goes_first() {
        let t0 = Instant::now();
        let mut t = table();
        let q = Queue::new();
        let a = sid("a");
        hello(&mut t, &q, "a", t0);
        q.full.set(true);
        t.closed(&a, &mut q.push());
        let plan = t.hello_plan(&a, false);
        assert_eq!(plan.lost_first, Some(lost_input(&a, WHY_CLOSED)));
        assert!(plan.restore);
        assert_eq!(hello(&mut t, &q, "a", t0), HelloOutcome::Refused);
        assert_eq!(t.view(&a, t0).unwrap().connections, 0);
        // The waiting loss goes in, the restore does not: refused, and the
        // next hello still restores, with nothing waiting.
        q.full.set(false);
        let mut refuse = |_| false;
        assert_eq!(
            t.hello(&a, false, t0, None, &mut q.push(), &mut refuse),
            HelloOutcome::Refused
        );
        assert_eq!(q.order(), ["lost a"]);
        let plan = t.hello_plan(&a, false);
        assert_eq!((plan.lost_first, plan.restore), (None, true));
        assert_eq!(t.view(&a, t0).unwrap().connections, 0);

        // Through `hello`: the waiting loss first, then the restore.
        let mut t = table();
        let q = Queue::new();
        hello(&mut t, &q, "a", t0);
        q.full.set(true);
        t.closed(&a, &mut q.push());
        q.full.set(false);
        assert_eq!(hello(&mut t, &q, "a", t0), HelloOutcome::AfterRestore);
        assert_eq!(q.order(), ["lost a", "hello a"]);
        t.restore_answered(&a);
        assert!(!t.hello_plan(&a, false).restore);
        assert!(t.hello_plan(&a, true).restore, "held in the core");

        let unlisted = sid("zz");
        assert!(!t.hello_plan(&unlisted, false).restore);
        assert!(t.hello_plan(&unlisted, true).restore);
    }

    /// A second `hello` while the first one's restore is not yet answered
    /// (review 1 S1): it restores too, so that its answer also waits for
    /// the restore to be durable. Once every restore is answered, a hello
    /// is answered at once again.
    #[test]
    fn a_hello_while_a_restore_is_in_flight_waits_for_it_too() {
        let t0 = Instant::now();
        let mut t = table();
        let q = Queue::new();
        let a = sid("a");
        hello(&mut t, &q, "a", t0);
        t.closed(&a, &mut q.push());
        assert_eq!(hello(&mut t, &q, "a", t0), HelloOutcome::AfterRestore);
        assert_eq!(
            hello(&mut t, &q, "a", t0),
            HelloOutcome::AfterRestore,
            "the first restore is not answered yet"
        );
        // The same connection again, too.
        assert_eq!(
            t.hello(&a, false, t0, Some(&a), &mut q.push(), &mut q.push()),
            HelloOutcome::AfterRestore
        );
        assert_eq!(q.order(), ["lost a", "hello a", "hello a", "hello a"]);
        t.restore_answered(&a);
        t.restore_answered(&a);
        assert_eq!(
            hello(&mut t, &q, "a", t0),
            HelloOutcome::AfterRestore,
            "one restore is still in flight"
        );
        t.restore_answered(&a);
        t.restore_answered(&a);
        assert_eq!(hello(&mut t, &q, "a", t0), HelloOutcome::Answer);
        // A loss while a restore is in flight is reported all the same.
        let mut t = table();
        let q = Queue::new();
        hello(&mut t, &q, "a", t0);
        t.closed(&a, &mut q.push());
        hello(&mut t, &q, "a", t0);
        t.closed(&a, &mut q.push());
        assert_eq!(q.order(), ["lost a", "hello a", "lost a"]);
    }

    /// A strategy that is not listed has no liveness: its closes and its
    /// silence report nothing, and it has no view.
    #[test]
    fn a_strategy_that_is_not_listed_has_no_liveness() {
        let t0 = Instant::now();
        let mut t = table();
        let q = Queue::new();
        let c = sid("c");
        assert_eq!(hello(&mut t, &q, "c", t0), HelloOutcome::Answer);
        t.request(&c, t0);
        t.closed(&c, &mut q.push());
        t.tick(ms(t0, 120_000), &mut q.push());
        assert!(q.order().is_empty());
        assert_eq!(t.view(&c, t0), None);
    }

    /// At the start only a listed strategy with open plain orders is
    /// armed, from the start; one that the core holds already is not
    /// reported again, and its hello restores.
    #[test]
    fn at_start_only_strategies_with_plain_orders_are_armed() {
        let t0 = Instant::now();
        let q = Queue::new();
        let mut t = table();
        t.start(t0, |s| *s == sid("a"), |_| false);
        assert_eq!(t.view(&sid("b"), t0).unwrap().last_request_age_ms, None);
        t.tick(ms(t0, 60_000), &mut q.push());
        assert_eq!(
            q.lost(),
            [("a".to_string(), "no request for 5000 ms".to_string())]
        );

        let mut t = table();
        let q = Queue::new();
        t.start(t0, |_| true, |s| *s == sid("a"));
        t.tick(ms(t0, 60_000), &mut q.push());
        assert_eq!(
            q.lost(),
            [("b".to_string(), "no request for 3000 ms".to_string())]
        );
        assert!(t.hello_plan(&sid("a"), false).restore);
    }
}
