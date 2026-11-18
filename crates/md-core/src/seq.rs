//! Sequence rules and the per-stream gap detector.
//!
//! A snapshot starts a run. A delta that breaks the rule ends it and gives one
//! [`GapEvent`]; later deltas give no further event until the next snapshot,
//! because the book is already broken (lake contract 3.2, "Book state").

/// How a venue chains its order book messages (design §6.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SeqRule {
    /// `update_id` = previous `update_id` + 1 (Bybit `u`).
    PlusOne,
    /// `prev_update_id` = previous `update_id` (Binance `pu`, OKX `prevSeqId`).
    PrevEquals,
    /// No sequence numbers (Hyperliquid): never a gap; staleness is watched elsewhere.
    None,
}

/// A delta that did not follow the rule. The caller knows the stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GapEvent {
    /// `update_id` of the last message that kept the run; `None` when there was no run.
    pub last_good: Option<i64>,
    /// `update_id` of the delta that broke it.
    pub next: Option<i64>,
    pub recv_ns: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    /// No run: before the first snapshot, or after a gap (`reported`).
    NoRun { reported: bool },
    Run { last: i64 },
}

#[derive(Debug, Clone)]
pub struct GapDetector {
    rule: SeqRule,
    state: State,
}

impl GapDetector {
    pub fn new(rule: SeqRule) -> Self {
        Self { rule, state: State::NoRun { reported: false } }
    }

    pub fn rule(&self) -> SeqRule {
        self.rule
    }

    /// A snapshot starts a new run. It never gives a gap event.
    pub fn snapshot(&mut self, update_id: Option<i64>, _recv_ns: i64) -> Option<GapEvent> {
        self.state = match update_id {
            Some(last) => State::Run { last },
            None => State::NoRun { reported: false },
        };
        None
    }

    pub fn delta(&mut self, update_id: Option<i64>, prev_update_id: Option<i64>, recv_ns: i64) -> Option<GapEvent> {
        if self.rule == SeqRule::None {
            return None;
        }
        match self.state {
            State::NoRun { reported: true } => None,
            State::NoRun { reported: false } => {
                self.state = State::NoRun { reported: true };
                Some(GapEvent { last_good: None, next: update_id, recv_ns })
            }
            State::Run { last } => {
                let follows = match self.rule {
                    SeqRule::PlusOne => update_id == last.checked_add(1),
                    SeqRule::PrevEquals => update_id.is_some() && prev_update_id == Some(last),
                    SeqRule::None => true,
                };
                match (follows, update_id) {
                    (true, Some(u)) => {
                        self.state = State::Run { last: u };
                        None
                    }
                    _ => {
                        self.state = State::NoRun { reported: true };
                        Some(GapEvent { last_good: Some(last), next: update_id, recv_ns })
                    }
                }
            }
        }
    }
}
