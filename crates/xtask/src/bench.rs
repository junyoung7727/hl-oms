//! Small measurements that ground design decisions. Each prints the
//! conditions it ran under, because a number without them means nothing.

use oms_core::*;
use std::collections::BTreeMap;
use std::io::Write;
use std::time::Instant;

fn percentile(sorted: &[u64], q: f64) -> u64 {
    if sorted.is_empty() {
        return 0;
    }
    let rank = ((sorted.len() as f64) * q).ceil() as usize;
    sorted[rank.clamp(1, sorted.len()) - 1]
}

pub fn summary(name: &str, mut v: Vec<u64>, unit: &str) -> serde_json::Value {
    v.sort_unstable();
    let mean = if v.is_empty() { 0 } else { v.iter().sum::<u64>() / v.len() as u64 };
    let j = serde_json::json!({
        "name": name,
        "unit": unit,
        "samples": v.len(),
        "min": v.first().copied().unwrap_or(0),
        "p50": percentile(&v, 0.50),
        "p90": percentile(&v, 0.90),
        "p99": percentile(&v, 0.99),
        "p999": percentile(&v, 0.999),
        "max": v.last().copied().unwrap_or(0),
        "mean": mean,
    });
    println!("{j}");
    j
}

fn describe_dir(dir: &str) {
    let out = std::process::Command::new("findmnt")
        .args(["-no", "SOURCE,FSTYPE,OPTIONS", "-T", dir])
        .output();
    if let Ok(o) = out {
        println!("# mount: {}", String::from_utf8_lossy(&o.stdout).trim());
    }
    let out = std::process::Command::new("df").args(["-h", dir]).output();
    if let Ok(o) = out {
        println!("# df: {}", String::from_utf8_lossy(&o.stdout).lines().last().unwrap_or(""));
    }
}

/// Appends `bytes` and calls `fdatasync`, `count` times, on a file in `dir`.
/// This is the floor for one journal commit on that device: the journal
/// performs exactly this pair of system calls per batch.
/// Stores a measurement next to the other evidence, with the source digest
/// and the environment it was taken in.
fn store(label: &str, dir: &std::path::Path, what: &str, results: Vec<serde_json::Value>, extra: serde_json::Value) {
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let report = serde_json::json!({
        "label": label,
        "taken_at_us": oms_api::client::now_us(),
        "build": format!("xtask src:{}", crate::digest::source_digest(&root)),
        "what": what,
        "environment": crate::perf::environment(dir),
        "results": results,
        "detail": extra,
    });
    let path = root.join(format!("evidence/perf/{label}.json"));
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    match std::fs::write(&path, serde_json::to_string_pretty(&report).unwrap_or_default()) {
        Ok(()) => println!("# written {}", path.display()),
        Err(e) => eprintln!("write {}: {e}", path.display()),
    }
}

pub fn fsync_probe(dir: &str, count: u64, bytes: u64, label: Option<&str>) -> i32 {
    if let Err(e) = std::fs::create_dir_all(dir) {
        eprintln!("create {dir}: {e}");
        return 1;
    }
    describe_dir(dir);
    let path = std::path::Path::new(dir).join("fsync-probe.tmp");
    let mut f = match std::fs::OpenOptions::new().create(true).append(true).open(&path) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("open {}: {e}", path.display());
            return 1;
        }
    };
    let block = vec![0x5au8; bytes as usize];
    let mut both = Vec::with_capacity(count as usize);
    let mut sync_only = Vec::with_capacity(count as usize);
    for _ in 0..count {
        let t0 = Instant::now();
        if f.write_all(&block).is_err() {
            return 1;
        }
        let t1 = Instant::now();
        if f.sync_data().is_err() {
            return 1;
        }
        let t2 = Instant::now();
        both.push(t2.duration_since(t0).as_micros() as u64);
        sync_only.push(t2.duration_since(t1).as_micros() as u64);
    }
    drop(f);
    let _ = std::fs::remove_file(&path);
    println!("# {count} appends of {bytes} bytes, each followed by fdatasync");
    let results = vec![summary("write+fdatasync", both, "us"), summary("fdatasync", sync_only, "us")];
    if let Some(label) = label {
        store(
            &format!("fsync-{label}"),
            std::path::Path::new(dir),
            "append then fdatasync on one file: the floor for one journal commit on this device",
            results,
            serde_json::json!({ "count": count, "bytes": bytes, "dir": dir }),
        );
    }
    0
}

/// Account value of the margin case, in quote units. With the policy's 2x
/// the limit is 25 200. The book of the case never passes 25 100 (250 orders
/// of 1 at 100 on each side, a position of at most 1, and the new order), so
/// every order is accepted, and the gate sums the whole book twice for each.
const MARGIN_CASE_ACCOUNT_VALUE: i64 = 12_600;
/// Record time of the first input of a case.
const BENCH_START: Ts = 1_700_000_000_000_000;
/// The node asks for a margin report this often (`runtime.margin_poll_ms`).
const MARGIN_CASE_POLL_US: Ts = 5_000_000;

fn bench_config(margin: bool) -> CoreConfig {
    let x = InstrumentId::new("X").expect("id");
    let mut instruments = BTreeMap::new();
    instruments.insert(x.clone(), InstrumentSpec {
        qty_step: Qty(100_000),
        px_tick: Px(10_000_000),
        px_max_sig_figs: Some(5),
        min_notional: Amount(0),
        max_order_qty: Qty(100 * 100_000_000),
        max_order_notional: Amount(i64::MAX / 4),
        price_band_bps: Some(500),
        account_pos_min: SQty(i64::MIN / 4),
        account_pos_max: SQty(i64::MAX / 4),
    });
    let mut strategies = BTreeMap::new();
    for name in ["a", "b"] {
        let mut limits = BTreeMap::new();
        limits.insert(x.clone(), PosLimit { min: SQty(i64::MIN / 4), max: SQty(i64::MAX / 4) });
        strategies.insert(StrategyId::new(name).expect("id"), StrategySpec { enabled: true, max_open_orders: 1000, limits });
    }
    let margin = margin.then(|| MarginPolicy { account_modes_allowed: vec!["standard".into()], ..MarginPolicy::default() });
    let policy = Policy { max_open_orders: 1000, max_inflight_sends: 1000, max_orders_in_memory: u32::MAX, margin, ..Policy::default() };
    CoreConfig { version: 1, instruments, strategies, policy }
}

/// What one case of the core benchmark measured, and the core it ended with.
pub struct CoreBenchCase {
    pub core: Core,
    /// Record time of the last input of the case.
    pub ended_at: Ts,
    /// Nanoseconds inside `Core::handle` for each new order.
    pub t_new: Vec<u64>,
    pub t_ack: Vec<u64>,
    pub t_fill: Vec<u64>,
    pub t_enc: Vec<u64>,
    pub sizes: Vec<u64>,
    pub elapsed_s: f64,
}

/// One order's whole life through the core, in memory: accept, acknowledge,
/// fill, close, with about 500 orders open throughout. With `margin` the
/// configuration has a margin policy, so every new order is also judged by
/// the leverage gate.
pub fn core_bench_case(orders: u64, margin: bool) -> Result<CoreBenchCase, String> {
    let mut core = Core::new(1);
    let mut at: Ts = BENCH_START;
    let go = |core: &mut Core, at: &mut Ts, input: Input| -> (Output, u64) {
        *at += 10;
        let t = Instant::now();
        let out = core.handle(*at, &input).expect("core fault");
        (out, t.elapsed().as_nanos() as u64)
    };
    go(&mut core, &mut at, Input::Boot { binary: "bench".into() });
    go(&mut core, &mut at, Input::Config(Box::new(bench_config(margin))));
    let session = 1;
    go(&mut core, &mut at, Input::Venue(VenueInput::Link { up: true, session, reason: String::new() }));
    let t = at;
    go(&mut core, &mut at, Input::Venue(VenueInput::FillsSynced { session, up_to: t }));
    let far = t + 1_000_000;
    go(&mut core, &mut at, Input::Venue(VenueInput::OpenOrders { session, queried_at: far, orders: vec![] }));
    go(&mut core, &mut at, Input::Venue(VenueInput::Positions { session, queried_at: far, positions: vec![] }));
    let x = InstrumentId::new("X").expect("id");
    go(&mut core, &mut at, Input::Venue(VenueInput::RefPrice { instrument: x.clone(), px: Px(100 * 100_000_000), venue_ts: t }));
    // What the node's reconciler feeds while a margin policy is set: the
    // venue setting and the account mode once, then a margin report and the
    // reference price at every poll.
    let mut margin_seq = 0u64;
    let mut poll = |core: &mut Core, at: &mut Ts| {
        margin_seq += 1;
        let queried_at = *at;
        go(core, at, Input::Venue(VenueInput::MarginReport(MarginReport {
            session,
            seq: margin_seq,
            queried_at,
            account_value: Amount(MARGIN_CASE_ACCOUNT_VALUE * 100_000_000),
            positions: vec![],
        })));
        let instrument = InstrumentId::new("X").expect("id");
        go(core, at, Input::Venue(VenueInput::RefPrice { instrument, px: Px(100 * 100_000_000), venue_ts: queried_at }));
    };
    let mut polled_at = at;
    if margin {
        go(&mut core, &mut at, Input::Venue(VenueInput::VenueSetting { session, instrument: x.clone(), isolated: true, leverage: 2 }));
        go(&mut core, &mut at, Input::Venue(VenueInput::AccountMode { session, mode: "standard".into() }));
        poll(&mut core, &mut at);
    }
    assert!(core.holds(None).is_empty(), "{:?}", core.holds(None));

    let (mut t_new, mut t_ack, mut t_fill, mut t_enc, mut sizes) = (Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new());
    let mut seq = 0u64;
    let mut encode = |at: Ts, input: Input, out: &Output, t_enc: &mut Vec<u64>, sizes: &mut Vec<u64>| {
        if out.events.is_empty() {
            return;
        }
        seq += 1;
        let rec = Record { seq, at, input, events: out.events.clone() };
        let t = Instant::now();
        let bytes = serde_json::to_vec(&rec).expect("encode");
        t_enc.push(t.elapsed().as_nanos() as u64);
        sizes.push(bytes.len() as u64);
    };
    let strategies = [StrategyId::new("a").expect("id"), StrategyId::new("b").expect("id")];
    let mut backlog: std::collections::VecDeque<OrderId> = std::collections::VecDeque::new();
    let started = Instant::now();
    for i in 0..orders {
        if margin && at - polled_at >= MARGIN_CASE_POLL_US {
            polled_at = at;
            poll(&mut core, &mut at);
        }
        let req = NewOrderReq {
            strategy: strategies[(i % 2) as usize].clone(),
            client_order_id: ClientOrderId::new(format!("k{i}")).expect("id"),
            instrument: x.clone(),
            side: if i % 2 == 0 { Side::Buy } else { Side::Sell },
            qty: Qty(100_000_000),
            px: Px(100 * 100_000_000),
            tif: Tif::Gtc,
            reduce_only: false,
            expires_at: at + 5_000_000,
        };
        let input = Input::NewOrder(req);
        let (out, ns) = go(&mut core, &mut at, input.clone());
        let Some(Reply::NewOrder(NewOrderReply::Accepted { order_id })) = out.reply else {
            return Err(format!("order {i}: unexpected reply {:?}", out.reply));
        };
        t_new.push(ns);
        encode(at, input, &out, &mut t_enc, &mut sizes);

        let input = Input::Venue(VenueInput::SendResult {
            order_id,
            outcome: SendOutcome::Placed {
                venue_oid: VenueOrderId::new(format!("v{i}")).expect("id"),
                status: StatusIn { status: VenueStatus::Open, cum: Some(Qty::ZERO), venue_ts: at },
            },
            timing: Default::default(),
        });
        let (out, ns) = go(&mut core, &mut at, input.clone());
        t_ack.push(ns);
        encode(at, input, &out, &mut t_enc, &mut sizes);
        backlog.push_back(order_id);

        // Keep about 500 orders open; fill the oldest beyond that.
        if backlog.len() > 500 {
            let id = backlog.pop_front().expect("order");
            let o = core.order(id).expect("order");
            let input = Input::Venue(VenueInput::Fill(FillIn {
                key: FillKey::new(format!("t{i}")).expect("id"),
                order_id: Some(id),
                venue_oid: o.venue_oid.clone(),
                instrument: x.clone(),
                side: o.side,
                qty: o.qty,
                px: o.px,
                fee: Amount(1000),
                venue_ts: at,
            }));
            let (out, ns) = go(&mut core, &mut at, input.clone());
            t_fill.push(ns);
            encode(at, input, &out, &mut t_enc, &mut sizes);
        }
    }
    let elapsed_s = started.elapsed().as_secs_f64();
    Ok(CoreBenchCase { core, ended_at: at, t_new, t_ack, t_fill, t_enc, sizes, elapsed_s })
}

/// Runs the core benchmark and stores it as `evidence/perf/core-bench.json`.
pub fn core_bench(orders: u64) -> i32 {
    let base = match core_bench_case(orders, false) {
        Ok(case) => case,
        Err(e) => {
            eprintln!("{e}");
            return 1;
        }
    };
    let CoreBenchCase { core, t_new, t_ack, t_fill, t_enc, sizes, elapsed_s, .. } = base;
    println!("# {orders} orders, ~500 open, {} in memory, release build: {}", core.orders.len(), !cfg!(debug_assertions));
    println!("# total {elapsed_s:.2} s including measurement overhead");
    let results = vec![
        summary("handle(new_order)", t_new, "ns"),
        summary("handle(send_result placed)", t_ack, "ns"),
        summary("handle(fill closing the order)", t_fill, "ns"),
        summary("encode record (serde_json)", t_enc, "ns"),
        summary("record size", sizes, "bytes"),
    ];
    // The same order life with a margin policy: the leverage gate judges
    // every new order over the whole book.
    let with_margin = match core_bench_case(orders, true) {
        Ok(case) => case,
        Err(e) => {
            eprintln!("margin case: {e}");
            return 1;
        }
    };
    println!(
        "# margin policy on: {orders} orders, {} open at the end, account exposure {:?}, total {:.2} s",
        with_margin.core.open.len(),
        with_margin.core.account_exposure().map(|e| e.to_string()),
        with_margin.elapsed_s
    );
    let mut results = results;
    results.push(summary("handle(new_order), margin policy on", with_margin.t_new, "ns"));
    results.push(summary("handle(send_result placed), margin policy on", with_margin.t_ack, "ns"));
    results.push(summary("handle(fill closing the order), margin policy on", with_margin.t_fill, "ns"));
    let t = Instant::now();
    let digest = core.digest();
    let digest_ms = t.elapsed().as_millis() as u64;
    println!("# state digest of {} orders: {digest_ms} ms ({digest:016x})", core.orders.len());
    let t = Instant::now();
    let snapshot = serde_json::to_vec(&core).expect("encode");
    let encode_ms = t.elapsed().as_millis() as u64;
    println!("# state encode: {encode_ms} ms, {} bytes", snapshot.len());
    let t = Instant::now();
    let _clone = core.clone();
    let clone_ms = t.elapsed().as_millis() as u64;
    println!("# state clone: {clone_ms} ms");
    core.check_invariants().expect("invariants");
    store(
        "core-bench",
        std::path::Path::new("."),
        "one order's life through Core::handle in memory (accept, acknowledge, fill, close), about 500 orders open; once without a margin policy and once with one (leverage gate on every new order)",
        results,
        serde_json::json!({
            "orders": orders,
            "orders_in_memory": core.orders.len(),
            "release_build": !cfg!(debug_assertions),
            "state_digest_ms": digest_ms,
            "state_encode_ms": encode_ms,
            "state_encode_bytes": snapshot.len(),
            "state_clone_ms": clone_ms,
            "margin_case": {
                "account_value": MARGIN_CASE_ACCOUNT_VALUE.to_string(),
                "max_account_leverage": "2",
                "open_orders_at_end": with_margin.core.open.len(),
                "account_exposure_at_end": with_margin.core.account_exposure().map(|e| e.to_string()),
                "margin_report_every_us": MARGIN_CASE_POLL_US,
                "record_time_span_us": with_margin.ended_at - BENCH_START,
            },
        }),
    );
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain_buy(key: &str, qty: i64, at: Ts) -> Input {
        Input::NewOrder(NewOrderReq {
            strategy: StrategyId::new("a").expect("id"),
            client_order_id: ClientOrderId::new(key).expect("id"),
            instrument: InstrumentId::new("X").expect("id"),
            side: Side::Buy,
            qty: Qty(qty * 100_000_000),
            px: Px(100 * 100_000_000),
            tif: Tif::Gtc,
            reduce_only: false,
            expires_at: at + 5_000_000,
        })
    }

    /// The case that stands behind the "margin policy on" figures: every
    /// one of its orders was accepted through the leverage gate with the
    /// book the figure names, and the gate was able to refuse.
    #[test]
    fn core_bench_margin_case_runs_the_leverage_gate_over_500_resting_orders() {
        let case = core_bench_case(1200, true).expect("every order of the case is accepted");
        assert_eq!(case.t_new.len(), 1200, "one decision time per order");
        let mut core = case.core;
        assert!(core.config.policy.margin.is_some(), "the case runs with a margin policy");
        assert!(core.holds(None).is_empty(), "{:?}", core.holds(None));
        assert_eq!(core.open.len(), 500);
        // 250 buys and 250 sells of 1 at 100 rest and the position is flat:
        // each side is worth 25 000.
        assert_eq!(core.account_exposure().map(|e| e.to_string()).as_deref(), Some("25000"));
        // The account value of the case is 12 600, so the limit is 25 200.
        // A buy of 2 more takes the exposure to exactly 25 200: accepted.
        // A buy of 3 would take it to 25 300: refused by the gate.
        let at = case.ended_at + 10;
        let out = core.handle(at, &plain_buy("probe-3", 3, at)).expect("core fault");
        assert_eq!(
            out.reply,
            Some(Reply::NewOrder(NewOrderReply::Rejected { reasons: vec![RejectReason::AccountLeverage] }))
        );
        let at = at + 10;
        let out = core.handle(at, &plain_buy("probe-2", 2, at)).expect("core fault");
        assert!(matches!(out.reply, Some(Reply::NewOrder(NewOrderReply::Accepted { .. }))), "{:?}", out.reply);
    }
}
