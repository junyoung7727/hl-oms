//! The reconciler task at its two ports: the published state it reads and
//! the query channel it writes. No venue and no process: the test holds the
//! receiving end of the channel and counts what arrives. The timers are
//! real and short.

#![allow(clippy::unwrap_used)]

use oms_core::*;
use oms_node::published::Published;
use oms_node::venue::QueryReq;
use oms_node::venue::reconciler::{self, Pace};
use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;

const SESSION: u64 = 1;

fn config() -> CoreConfig {
    let x = InstrumentId::new("X").unwrap();
    let mut instruments = BTreeMap::new();
    instruments.insert(
        x.clone(),
        InstrumentSpec {
            qty_step: Qty::parse("0.001").unwrap(),
            px_tick: Px::parse("0.1").unwrap(),
            px_max_sig_figs: None,
            min_notional: Amount::ZERO,
            max_order_qty: Qty::parse("100").unwrap(),
            max_order_notional: Amount::parse("1000000").unwrap(),
            price_band_bps: None,
            account_pos_min: SQty::parse("-100").unwrap(),
            account_pos_max: SQty::parse("100").unwrap(),
        },
    );
    let mut limits = BTreeMap::new();
    limits.insert(
        x,
        PosLimit {
            min: SQty::parse("-100").unwrap(),
            max: SQty::parse("100").unwrap(),
        },
    );
    let mut strategies = BTreeMap::new();
    strategies.insert(
        StrategyId::new("a").unwrap(),
        StrategySpec {
            enabled: true,
            max_open_orders: 10,
            limits,
        },
    );
    let policy = Policy {
        margin: Some(MarginPolicy::default()),
        ..Policy::default()
    };
    CoreConfig {
        version: 1,
        instruments,
        strategies,
        policy,
    }
}

/// A core with a margin policy whose link is up and whose three sweeps are
/// done: it needs nothing from the venue but what the margin rules ask.
fn synced_core() -> Core {
    let mut core = Core::new(7);
    let mut at: Ts = 1_000_000_000;
    let mut feed = |core: &mut Core, input: Input| {
        at += 1_000;
        core.handle(at, &input).unwrap();
    };
    feed(
        &mut core,
        Input::Boot {
            binary: "test".into(),
        },
    );
    feed(&mut core, Input::Config(Box::new(config())));
    let venue = [
        VenueInput::Link {
            up: true,
            session: SESSION,
            reason: "test".into(),
        },
        VenueInput::FillsSynced {
            session: SESSION,
            up_to: 1_000_000_000,
        },
        VenueInput::OpenOrders {
            session: SESSION,
            queried_at: 1_000_100_000,
            orders: vec![],
        },
        VenueInput::Positions {
            session: SESSION,
            queried_at: 1_000_100_000,
            positions: vec![],
        },
    ];
    for input in venue {
        feed(&mut core, Input::Venue(input));
    }
    assert_eq!(core.needs(), vec![]);
    core
}

/// The same core after the venue told the setting of X and the account
/// mode: the reconciler has no setting and no mode left to ask for.
fn core_that_knows_setting_and_mode() -> Core {
    let mut core = synced_core();
    let told = [
        VenueInput::VenueSetting {
            session: SESSION,
            instrument: InstrumentId::new("X").unwrap(),
            isolated: true,
            leverage: 2,
        },
        VenueInput::AccountMode {
            session: SESSION,
            mode: "standard".into(),
        },
    ];
    for input in told {
        core.handle(1_001_000_000, &Input::Venue(input)).unwrap();
    }
    core
}

/// A round every 10 ms, and nothing else within the time of a test.
const NEVER: Pace = Pace {
    round: Duration::from_millis(10),
    sweep: Duration::from_secs(3600),
    margin_poll: Duration::from_secs(3600),
    venue_setting_poll: Duration::from_secs(3600),
};

/// The gateway's queue was full when the margin query was due, so the
/// query was not issued. It goes out on the next round with room, not one
/// whole margin period later, and its number is the first one: a query
/// that was never issued used no number.
#[tokio::test(flavor = "current_thread")]
async fn a_margin_query_the_gateway_had_no_room_for_goes_out_on_the_next_round() {
    let published = Arc::new(Published::new(core_that_knows_setting_and_mode(), 0));
    let (queries, mut issued) = mpsc::channel::<QueryReq>(1);
    // The queue holds one query and it is taken.
    queries.try_send(QueryReq::OpenOrders).unwrap();
    let pace = Pace {
        margin_poll: Duration::from_secs(5),
        ..NEVER
    };
    tokio::spawn(reconciler::run(published, queries, pace));

    // Several rounds pass with no room.
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(matches!(issued.recv().await, Some(QueryReq::OpenOrders)));

    let next = tokio::time::timeout(Duration::from_secs(1), issued.recv()).await;
    match next {
        Ok(Some(QueryReq::Margin { seq })) => assert_eq!(seq, 1),
        other => panic!("no margin query within 1 s of room in the queue: {other:?}"),
    }
}

/// The venue never tells the setting of X nor the account mode (its
/// answers are lost or unreadable). The reconciler asks for each when the
/// link session starts and then not again before `venue_setting_poll_ms`:
/// the request budget counts one venue-setting query per instrument and
/// poll period. With a period of 10 s, 2.5 s see one query of each kind.
#[tokio::test(flavor = "current_thread")]
async fn an_unanswered_setting_or_mode_query_is_repeated_at_the_setting_period_only() {
    let published = Arc::new(Published::new(synced_core(), 0));
    let (queries, mut issued) = mpsc::channel::<QueryReq>(64);
    let pace = Pace {
        venue_setting_poll: Duration::from_secs(10),
        ..NEVER
    };
    tokio::spawn(reconciler::run(published, queries, pace));

    tokio::time::sleep(Duration::from_millis(2_500)).await;
    let (mut settings, mut modes) = (0, 0);
    while let Ok(q) = issued.try_recv() {
        match q {
            QueryReq::VenueSetting { instrument } => {
                assert_eq!(instrument.as_str(), "X");
                settings += 1;
            }
            QueryReq::AccountMode => modes += 1,
            QueryReq::Margin { .. } => {}
            other => panic!("a query nothing asked for: {other:?}"),
        }
    }
    assert_eq!((settings, modes), (1, 1));
}
