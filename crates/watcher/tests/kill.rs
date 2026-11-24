//! The watcher's kill notices (C-WATCH part 11, U19b2a): the texts of the
//! start, progress, final and abort notices, the position block every
//! notice carries, and the position reader against a loopback fake
//! (`http-fake`) on a manual clock. And the sans-IO cancel-all
//! confirmation loop `KillLoop` (U19b2b): C2–C5, the order budget, the
//! request deadlines, the notice rule and abort, driven by hand on its own
//! clock (no client, no task).

#![allow(clippy::unwrap_used)]

mod common;

use std::sync::{Arc, Mutex};

use common::kill_rig::{self as rig, KillRig, at_once, cancel_ok as ok_n, http, open_orders};
use http_fake::{Answer, FakeHttp};
use serde_json::{Value, json};
use watcher::alert::{MSG_LIMIT, message, split};
use watcher::clock::{Clock, ManualClock};
use watcher::config::Kill;
use watcher::kill::{
    AbortWhy, Action, CancelAnswer, KillLoop, LATCHED_LINE, LastError, NOT_READ_ON_ABORT, NotSent,
    Notice, PAST_DEADLINE, PAUSED_LINE, POSITIONS, Phase, Positions, Report, StopOutcome, TIMED_OUT, notice_text,
    position_block, read_positions, Arrived, Inflight, ORDERS, count_open,
};
use watcher::metrics::Metrics;
use watcher::rest::{CANCEL_ALL, CancelAll, Client, ClientConfig, Credentials};

/// 2023-11-14T22:13:20Z.
const T0: u64 = 1_700_000_000_000;
const KST: &str = "11-15 07:13:20 KST";
const HEAD: &str = "포지션(거래소 /v5/position/list, 11-15 07:13:20 KST, 강제청산가는 교차 증거금이면 거래소 추정):";

fn row(symbol: &str, side: &str, size: &str, upnl: &str, mark: &str, liq: &str) -> Value {
    json!({
        "symbol": symbol, "side": side, "size": size, "unrealisedPnl": upnl,
        "markPrice": mark, "liqPrice": liq, "positionIdx": 0,
    })
}

#[test]
fn the_position_block_lists_size_upnl_mark_and_liquidation_distance() {
    let rows = vec![
        // Long: |70000 − 63000.12| / 70000 = 9.9998… % → 9.99 (down).
        row("BTCUSDT", "Buy", "0.5", "125.5", "70000", "63000.12"),
        // Short with the liquidation price above the mark: 10 % exactly.
        row("ETHUSDT", "Sell", "2", "-10.25", "3500", "3850"),
        // Cross margin without a liquidation price.
        row("SOLUSDT", "Buy", "10", "0", "150.5", ""),
        // A liquidation price of zero is no price either.
        row("ADAUSDT", "Buy", "1", "0.1", "0.5", "0"),
        // No mark price.
        row("XRPUSDT", "Sell", "100", "1.5", "0", "0.5"),
        // Size 0 is left out; an unreadable size is shown as unknown, never
        // dropped (review 1 S3: unknown is not flat).
        row("DOGEUSDT", "Buy", "0", "0", "0.1", "0.05"),
        row("LTCUSDT", "Buy", "", "0", "80", "70"),
        // 2/3 = 66.666… % → 66.66, never 66.67.
        row("BNBUSDT", "Sell", "3", "x", "3", "1"),
    ];
    let want = [
        HEAD,
        "- BTCUSDT 매수 0.5, 미실현손익 125.5, 마크 70000, 마크에서 강제청산가까지 9.99 %",
        "- ETHUSDT 매도 2, 미실현손익 -10.25, 마크 3500, 마크에서 강제청산가까지 10 %",
        "- SOLUSDT 매수 10, 미실현손익 0, 마크 150.5, 마크에서 강제청산가까지 모름(거래소가 청산가를 주지 않음)",
        "- ADAUSDT 매수 1, 미실현손익 0.1, 마크 0.5, 마크에서 강제청산가까지 모름(거래소가 청산가를 주지 않음)",
        "- XRPUSDT 매도 100, 미실현손익 1.5, 마크 0, 마크에서 강제청산가까지 모름(마크 가격 없음)",
        "- LTCUSDT 매수 수량 모름, 미실현손익 0, 마크 80, 마크에서 강제청산가까지 12.5 %",
        "- BNBUSDT 매도 3, 미실현손익 모름, 마크 3, 마크에서 강제청산가까지 66.66 %",
    ]
    .join("\n");
    assert_eq!(position_block(&Positions::Rows(rows), KST), want);

    // No row (or only rows left out): said so under the head line.
    assert_eq!(
        position_block(&Positions::Rows(vec![]), KST),
        format!("{HEAD}\n포지션 없음")
    );
    let flat = vec![row("DOGEUSDT", "Buy", "0", "0", "0.1", "0.05")];
    assert_eq!(
        position_block(&Positions::Rows(flat), KST),
        format!("{HEAD}\n포지션 없음")
    );
    // A row whose size does not read (a JSON number, more than 8 decimal
    // places, no field) is a position of unknown size: the block never says
    // `포지션 없음` for it (review 1 S3).
    let mut number = row("ETHUSDT", "Sell", "0", "0", "3500", "3850");
    number["size"] = json!(2);
    let mut absent = row("SOLUSDT", "Buy", "0", "0", "150", "");
    absent.as_object_mut().unwrap().remove("size");
    let unknown = vec![
        number,
        row("BTCUSDT", "Buy", "0.123456789", "0", "70000", "63000"),
        absent,
    ];
    assert_eq!(
        position_block(&Positions::Rows(unknown), KST),
        [
            HEAD,
            "- ETHUSDT 매도 수량 모름, 미실현손익 0, 마크 3500, 마크에서 강제청산가까지 10 %",
            "- BTCUSDT 매수 수량 모름, 미실현손익 0, 마크 70000, 마크에서 강제청산가까지 10 %",
            "- SOLUSDT 매수 수량 모름, 미실현손익 0, 마크 150, 마크에서 강제청산가까지 모름(거래소가 청산가를 주지 않음)",
        ]
        .join("\n")
    );
    // An unknown side or symbol still shows the position; a distance too
    // large to compute is unknown, not a wrong number.
    let odd = vec![
        row("btc-usdt", "", "1", "1", "2", "1"),
        row("BIGUSDT", "Buy", "1", "1", "0.00000001", &"9".repeat(30)),
    ];
    assert_eq!(
        position_block(&Positions::Rows(odd), KST),
        [
            HEAD,
            "- ???????? 방향 모름 1, 미실현손익 1, 마크 2, 마크에서 강제청산가까지 50 %",
            "- BIGUSDT 매수 1, 미실현손익 1, 마크 0.00000001, 마크에서 강제청산가까지 모름(계산 넘침)",
        ]
        .join("\n")
    );
}

#[test]
fn a_failed_position_read_says_unknown() {
    assert_eq!(
        position_block(&Positions::Failed("no_answer".into()), KST),
        "포지션 조회 실패(no_answer). 수량·손익·마크·강제청산가 모름"
    );
    assert_eq!(
        position_block(&Positions::Failed(TIMED_OUT.into()), KST),
        "포지션 조회 실패(시간 초과). 수량·손익·마크·강제청산가 모름"
    );
    assert_eq!(
        position_block(&Positions::NotRead("워처 키가 멈춤".into()), KST),
        "포지션 조회 안 함(워처 키가 멈춤). 수량·손익 모름"
    );
}

#[test]
fn a_notice_fits_one_part_with_50_positions() {
    let rows: Vec<Value> = (0..50)
        .map(|i| {
            row(
                &format!("{i:02}{}USDT", "X".repeat(24)),
                "Sell",
                "12345.12345678",
                "-123456.12345678",
                "123456.12345678",
                "100000.5",
            )
        })
        .collect();
    let block = position_block(&Positions::Rows(rows), KST);
    let lines: Vec<&str> = block.lines().collect();
    assert_eq!(lines.len(), 1 + 20 + 1, "{block}");
    assert_eq!(lines[21], "그 밖에 30종목");
    let long = "가".repeat(400);
    let progress = Notice::Progress {
        round: 123_456,
        remaining: Err(long.clone()),
        cancel_all_calls: 123_456,
        last_error: Some(LastError::Failed(long)),
    };
    let body = format!("{}\n{block}", notice_text(&progress, ""));
    let msg = message(&"나".repeat(64), T0, &body);
    let units = msg.encode_utf16().count();
    assert!(units <= MSG_LIMIT, "{units} units");
    assert_eq!(split(&msg, MSG_LIMIT).len(), 1);
}

#[test]
fn the_final_notice_names_the_stop_outcome() {
    let head =
        "워처 킬 정리 완료: 미체결 0건 확인 뒤 30초 동안 늦은 접수 없음(7회차, 일괄 취소 8회). ";
    for (outcome, line, clear) in [
        (
            StopOutcome::Latched,
            "OMS는 래치됨(watcher_kill). 해제는 운영자만: omsctl clear-hold --reason kill --note …",
            true,
        ),
        (
            StopOutcome::StoppedMarked,
            "OMS는 멈춤(systemctl stop). 다음 시작은 킬 마커로 래치됨. 해제는 운영자만: omsctl clear-hold --reason kill --note …",
            true,
        ),
        (
            StopOutcome::StoppedUncheckedMarker,
            "OMS는 멈춤(systemctl stop). 킬 마커는 OMS가 읽는 경로인지 확인 못 함: 워처는 OMS가 다시 답하면 watcher_kill을 보냄",
            false,
        ),
        (
            StopOutcome::StoppedUnmarked,
            "OMS는 멈춤(systemctl stop). 킬 마커가 없어 다음 시작은 래치 없이 시작함: 워처는 OMS가 다시 답하면 watcher_kill을 보냄",
            false,
        ),
        (
            StopOutcome::Unconfirmed,
            "OMS 정지 미확인: OMS가 살아 있으면 래치 없이 주문을 낼 수 있음. 마커는 다음 시작에만 걸림. 워처는 OMS가 다시 답하면 watcher_kill을 보냄",
            false,
        ),
    ] {
        let n = Notice::Final {
            watch_ms: 30_000,
            round: 7,
            cancel_all_calls: 8,
            outcome,
        };
        let text = notice_text(&n, "the header is not in the final notice");
        assert_eq!(text, format!("{head}{line}"), "{outcome:?}");
        // Only a latch or a marked stop at a checked path names clear-hold.
        assert_eq!(text.contains("clear-hold"), clear, "{outcome:?}");
    }
    // The watch length is the config's, in seconds.
    let n = Notice::Final {
        watch_ms: 12_500,
        round: 1,
        cancel_all_calls: 1,
        outcome: StopOutcome::Latched,
    };
    assert!(notice_text(&n, "").contains(" 12.5초 동안 "));
    // The start notice: the header, the round in seconds, the scope.
    assert_eq!(
        notice_text(&Notice::Start { round_ms: 1000 }, "H"),
        "워처 킬 시작(H). 워처 키로 미체결 0건 확인까지 1초마다 일괄 취소(linear, USDT 정산). 포지션은 닫지 않음."
    );
    assert!(notice_text(&Notice::Start { round_ms: 2500 }, "H").contains(" 2.5초마다 "));
}

#[test]
fn a_latched_key_says_the_watcher_must_restart() {
    let progress = |remaining: Result<usize, String>, last_error: Option<LastError>| {
        notice_text(
            &Notice::Progress {
                round: 4,
                remaining,
                cancel_all_calls: 3,
                last_error,
            },
            "",
        )
    };
    assert_eq!(
        LATCHED_LINE,
        "워처 키가 멈춰 일괄 취소를 보낼 수 없음: 워처를 다시 시작해야 루프가 끝남"
    );
    assert_eq!(
        PAUSED_LINE,
        "워처 요청 일시 정지(HTTP 403): 정지가 풀리면 다시 보냄"
    );
    assert_eq!(
        progress(
            Err("no answer before the deadline".into()),
            Some(LastError::NotSent(NotSent::Latched))
        ),
        format!(
            "워처 킬 4회차: 남은 미체결 모름: no answer before the deadline, 일괄 취소 3회. {LATCHED_LINE}"
        )
    );
    assert_eq!(
        progress(Ok(2), Some(LastError::NotSent(NotSent::Paused))),
        format!("워처 킬 4회차: 남은 미체결 2건, 일괄 취소 3회. {PAUSED_LINE}")
    );
    assert_eq!(
        progress(Ok(1), None),
        "워처 킬 4회차: 남은 미체결 1건, 일괄 취소 3회."
    );
    assert_eq!(
        progress(
            Ok(1),
            Some(LastError::Failed("no answer: HTTP 503\nbusy".into()))
        ),
        "워처 킬 4회차: 남은 미체결 1건, 일괄 취소 3회. 마지막 일괄 취소 실패: no answer: HTTP 503 busy"
    );
    assert_eq!(
        progress(
            Ok(1),
            Some(LastError::NotSent(NotSent::Local("poisoned".into())))
        ),
        "워처 킬 4회차: 남은 미체결 1건, 일괄 취소 3회. 일괄 취소를 보내지 못함(로컬 오류: poisoned)"
    );
}

#[test]
fn the_abort_notice_says_positions_were_not_read() {
    assert_eq!(
        NOT_READ_ON_ABORT,
        "포지션 조회 안 함(워처 종료 중). 수량·손익·마크·강제청산가 모름"
    );
    let abort = |why: AbortWhy, remaining: Option<usize>| {
        notice_text(
            &Notice::Abort {
                why,
                round: 9,
                remaining,
                cancel_all_calls: 9,
            },
            "",
        )
    };
    assert_eq!(
        abort(AbortWhy::Shutdown, Some(2)),
        format!(
            "워처 종료로 워처 킬 중단(9회차, 남은 미체결 2건, 일괄 취소 9회). OMS의 래치·마커는 그대로.\n{NOT_READ_ON_ABORT}"
        )
    );
    // The refusal's reason is cut to 200 characters.
    let reason = format!("{}{}", "가".repeat(199), "나b".repeat(51));
    let text = abort(AbortWhy::StartRefused(reason), None);
    let kept = format!("{}나", "가".repeat(199));
    assert_eq!(
        text,
        format!(
            "워처 시작 거부: {kept}로 워처 킬 중단(9회차, 남은 미체결 모름, 일괄 취소 9회). OMS의 래치·마커는 그대로.\n{NOT_READ_ON_ABORT}"
        )
    );
    for t in [text, abort(AbortWhy::Shutdown, None)] {
        assert!(t.ends_with(NOT_READ_ON_ABORT), "{t}");
    }
}

/// A client of `fake` on a manual clock at `T0` (`request_timeout_ms`
/// 2,000: a read's deadline is `T0 + 13,000`).
struct C {
    client: Client,
    clock: Arc<ManualClock>,
    metrics: Arc<Metrics>,
}

fn client(fake: &FakeHttp) -> C {
    let clock = Arc::new(ManualClock::new(T0));
    let metrics = Arc::new(Metrics::new("h1", "w1"));
    let (log, _lines) = watcher::log::collect();
    let cfg = ClientConfig {
        base: fake.url(),
        recv_window_ms: 5000,
        request_timeout_ms: 2000,
    };
    let creds = Credentials::new("WKEY", "watcher-made-up-secret-42");
    let client = Client::new(&cfg, creds, clock.clone(), metrics.clone(), log).unwrap();
    C {
        client,
        clock,
        metrics,
    }
}

fn page(rows: Vec<Value>, cursor: &str) -> Answer {
    let result = json!({"category": "linear", "list": rows, "nextPageCursor": cursor});
    Answer::Reply(
        200,
        format!(r#"{{"retCode":0,"retMsg":"OK","result":{result},"time":1}}"#),
    )
}

fn targets(fake: &FakeHttp) -> Vec<String> {
    fake.requests().iter().map(|r| r.target.clone()).collect()
}

#[tokio::test]
async fn the_position_reader_pages_and_keeps_its_deadline() {
    let a = row("BTCUSDT", "Buy", "0.5", "1", "70000", "63000");
    let b = row("ETHUSDT", "Sell", "2", "1", "3500", "3850");
    let first = "/v5/position/list?category=linear&limit=200&settleCoin=USDT";
    let second = "/v5/position/list?category=linear&cursor=p2&limit=200&settleCoin=USDT";

    // Two pages by the cursor; every row as the venue gave it.
    let fake = FakeHttp::start(Answer::Close);
    fake.route(
        POSITIONS,
        [page(vec![a.clone()], "p2"), page(vec![b.clone()], "")],
    );
    let c = client(&fake);
    let p = read_positions(&c.client, &*c.clock, 2000).await;
    assert_eq!(p, Positions::Rows(vec![a.clone(), b.clone()]));
    assert_eq!(targets(&fake), [first, second]);

    // A page answered at or after the deadline (start + 2,000 + 11,000)
    // ends the read: no further page is sent.
    for (step, case) in [(7_000, "after"), (6_500, "at")] {
        let fake = FakeHttp::start(Answer::Close);
        fake.route(
            POSITIONS,
            [
                page(vec![a.clone()], "p2"),
                page(vec![b.clone()], "p3"),
                page(vec![], ""),
            ],
        );
        let c = client(&fake);
        let clock = c.clock.clone();
        fake.on_request(move |_| clock.advance(step));
        let p = read_positions(&c.client, &*c.clock, 2000).await;
        assert_eq!(p, Positions::Failed(TIMED_OUT.into()), "{case}");
        assert_eq!(targets(&fake), [first, second], "{case}");
        assert_eq!(c.clock.now_ms(), T0 + 2 * step, "{case}");
    }
    // One page just before the deadline is read.
    let fake = FakeHttp::start(Answer::Close);
    fake.route(POSITIONS, [page(vec![a.clone()], "")]);
    let c = client(&fake);
    let clock = c.clock.clone();
    fake.on_request(move |_| clock.advance(12_999));
    let p = read_positions(&c.client, &*c.clock, 2000).await;
    assert_eq!(p, Positions::Rows(vec![a.clone()]));

    // At most 5 pages: a sixth would be needed, so the read fails.
    let fake = FakeHttp::start(Answer::Close);
    let pages: Vec<Answer> = (2..=7)
        .map(|i| page(vec![a.clone()], &format!("p{i}")))
        .collect();
    fake.route(POSITIONS, pages);
    let c = client(&fake);
    let p = read_positions(&c.client, &*c.clock, 2000).await;
    assert_eq!(p, Positions::Failed("unreadable".into()));
    assert_eq!(fake.requests().len(), 5);

    // An answer that is not a page, and a failed request: the class.
    let not_a_list = Answer::Reply(
        200,
        r#"{"retCode":0,"retMsg":"OK","result":{"list":{}}}"#.into(),
    );
    // The requests sent tell the rule that ended the read from the 5-page
    // cap, which ends it with the same class (review 1 S1).
    for (case, answers, class, sent) in [
        ("list not a list", vec![not_a_list], "unreadable", 1),
        (
            "a full page without a cursor",
            vec![page(vec![a.clone(); 200], "")],
            "unreadable",
            1,
        ),
        (
            "the cursor just sent again",
            vec![page(vec![], "p2"), page(vec![a.clone()], "p2")],
            "unreadable",
            2,
        ),
        (
            "HTTP 503",
            vec![Answer::Reply(503, "busy".into())],
            "no_answer",
            1,
        ),
    ] {
        let fake = FakeHttp::start(Answer::Close);
        fake.route(POSITIONS, answers);
        let c = client(&fake);
        let p = read_positions(&c.client, &*c.clock, 2000).await;
        assert_eq!(p, Positions::Failed(class.into()), "{case}");
        let errors = c.metrics.get(
            "watcher_request_errors_total",
            &[("endpoint", POSITIONS), ("class", class)],
        );
        assert_eq!(errors, Some(1.0), "{case}");
        assert_eq!(fake.requests().len(), sent, "{case}");
    }

    // A latched or paused client is not asked: no request at all.
    let fake = FakeHttp::start(Answer::Close);
    fake.route(POSITIONS, [page(vec![a.clone()], "")]);
    let c = client(&fake);
    c.client.stop_key("made-up rule");
    let p = read_positions(&c.client, &*c.clock, 2000).await;
    assert_eq!(p, Positions::NotRead("워처 키가 멈춤".into()));
    assert!(fake.requests().is_empty());

    let fake = FakeHttp::start(Answer::Close);
    fake.route("/v5/order/realtime", [Answer::Reply(403, "banned".into())]);
    fake.route(POSITIONS, [page(vec![a.clone()], "")]);
    let c = client(&fake);
    let e = c.client.get("/v5/order/realtime", &[]).await.unwrap_err();
    assert_eq!(e.class(), "ip_banned");
    c.clock.set(T0 + 599_999);
    let p = read_positions(&c.client, &*c.clock, 2000).await;
    assert_eq!(p, Positions::NotRead("HTTP 403 뒤 요청 일시 정지".into()));
    assert_eq!(fake.requests().len(), 1);
    // After the pause the positions are read again.
    c.clock.set(T0 + 600_000);
    let seen = Arc::new(Mutex::new(Vec::new()));
    let s = seen.clone();
    fake.on_request(move |r| s.lock().unwrap().push(r.path().to_string()));
    let p = read_positions(&c.client, &*c.clock, 2000).await;
    assert_eq!(p, Positions::Rows(vec![a]));
    assert_eq!(*seen.lock().unwrap(), [POSITIONS]);
}

// ---- The loop (U19b2b), sans-IO ----

/// `request_timeout_ms` of the loop tests: a request's deadline is its
/// emission (or its last page) + 13,000 ms.
const RT: u64 = 2000;
const EXTRA: u64 = RT + 11_000;

fn kill_loop() -> KillLoop {
    KillLoop::new(Kill::default(), RT)
}

fn done(n: usize) -> CancelAnswer {
    CancelAnswer::Done(CancelAll { cancelled: n })
}

/// The `req_id` of the one request in `acts`, and whether it is a count.
fn req(acts: &[Action]) -> (u64, bool) {
    let reqs: Vec<(u64, bool)> = acts
        .iter()
        .filter_map(|a| match a {
            Action::CancelAll { req_id } => Some((*req_id, false)),
            Action::Count { req_id } => Some((*req_id, true)),
            _ => None,
        })
        .collect();
    assert_eq!(reqs.len(), 1, "one request in {acts:?}");
    reqs[0]
}

fn cancel_id(acts: &[Action]) -> u64 {
    let (id, count) = req(acts);
    assert!(!count, "a cancel-all in {acts:?}");
    id
}

fn count_id(acts: &[Action]) -> u64 {
    let (id, count) = req(acts);
    assert!(count, "a count in {acts:?}");
    id
}

fn notices(acts: &[Action]) -> Vec<Notice> {
    acts.iter()
        .filter_map(|a| match a {
            Action::Notice(n) => Some(n.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn the_loop_cancels_counts_and_ends_after_the_watch() {
    let mut l = kill_loop();
    assert_eq!(l.phase(), Phase::Idle);
    assert_eq!(l.next_wake(), None);
    let acts = l.start(T0, "h", StopOutcome::Latched);
    assert_eq!(acts[0], Action::Notice(Notice::Start { round_ms: 1000 }));
    assert_eq!(l.header(), "h");
    let c = cancel_id(&acts);
    assert_eq!((l.phase(), l.round()), (Phase::Cancel, 1));
    // C2 answered → C3 at once.
    let k = count_id(&l.cancel_done(T0 + 50, c, done(3)));
    assert_eq!(l.cancel_all_calls(), 1);
    // C3 zero → C5: a count every 5 s from z, the last at z + 30 s.
    let z = T0 + 100;
    assert_eq!(l.count_done(z, k, Ok(0)), []);
    assert_eq!((l.phase(), l.remaining()), (Phase::Watch, &Ok(0)));
    for step in 1..=6u64 {
        let due = z + step * 5000;
        assert_eq!(l.next_wake(), Some(due), "watch count {step}");
        assert_eq!(l.poll(due - 1), []);
        let k = count_id(&l.poll(due));
        let acts = l.count_done(due + 40, k, Ok(0));
        if step < 6 {
            assert_eq!(acts, [], "watch count {step}");
            continue;
        }
        assert_eq!(
            acts,
            [
                Action::Notice(Notice::Final {
                    watch_ms: 30_000,
                    round: 1,
                    cancel_all_calls: 1,
                    outcome: StopOutcome::Latched,
                }),
                Action::Ended(Report {
                    rounds: 1,
                    cancel_all_calls: 1,
                    zero_since: Some(z),
                    aborted: false,
                }),
            ]
        );
    }
    assert_eq!((l.phase(), l.next_wake()), (Phase::Ended, None));
    assert_eq!(l.poll(z + 90_000), []);
    assert_eq!(l.abort(z + 90_000, AbortWhy::Shutdown), []);
}

#[test]
fn a_count_above_zero_waits_for_the_round() {
    let mut l = kill_loop();
    let c = cancel_id(&l.start(T0, "h", StopOutcome::Latched));
    let k = count_id(&l.cancel_done(T0 + 10, c, done(2)));
    assert_eq!(l.count_done(T0 + 20, k, Ok(2)), []);
    assert_eq!((l.phase(), l.remaining()), (Phase::Repeat, &Ok(2)));
    // C4: the next round at t_1 + round_ms, not at once.
    assert_eq!(l.next_wake(), Some(T0 + 1000));
    assert_eq!(l.poll(T0 + 999), []);
    let c = cancel_id(&l.poll(T0 + 1000));
    assert_eq!((l.phase(), l.round()), (Phase::Cancel, 2));
    // A late poll starts the round late; the next round waits round_ms
    // from that start.
    let k = count_id(&l.cancel_done(T0 + 1010, c, done(0)));
    assert_eq!(l.count_done(T0 + 1020, k, Ok(1)), []);
    assert_eq!(l.poll(T0 + 1999), []);
    let c = cancel_id(&l.poll(T0 + 2500));
    assert_eq!(l.round(), 3);
    let k = count_id(&l.cancel_done(T0 + 2510, c, done(1)));
    assert_eq!(l.count_done(T0 + 2520, k, Ok(1)), []);
    assert_eq!(l.next_wake(), Some(T0 + 3500));
    assert_eq!(l.poll(T0 + 3499), []);
    assert_eq!(l.cancel_all_calls(), 3);
}

#[test]
fn a_failed_cancel_all_is_still_followed_by_a_count() {
    let mut l = kill_loop();
    let c = cancel_id(&l.start(T0, "h", StopOutcome::Latched));
    // A sent cancel-all that failed is counted after (unlike the OMS): the
    // count is the truth.
    let k = count_id(&l.cancel_done(T0 + 10, c, CancelAnswer::Failed("HTTP 503".into())));
    assert_eq!(l.phase(), Phase::Count);
    assert_eq!(l.cancel_all_calls(), 1);
    assert_eq!(l.count_done(T0 + 20, k, Ok(4)), []);
    assert_eq!(l.remaining(), &Ok(4));

    // Refused when nothing is open (the answer is not documented, Q11.7):
    // every cancel-all is refused, every count says zero. The loop still
    // ends on the count.
    let mut l = kill_loop();
    let mut now = T0;
    let mut acts = l.start(now, "h", StopOutcome::Latched);
    let mut ended = None;
    for _ in 0..100 {
        // Every answer at once, in the instant of its request (at most
        // 100 actions per instant: a loop that asks again and again in one
        // instant fails here instead of spinning).
        for _ in 0..100 {
            let Some(a) = acts.pop() else { break };
            match a {
                Action::CancelAll { req_id } => acts.extend(l.cancel_done(
                    now,
                    req_id,
                    CancelAnswer::Failed("retCode 170213: no order".into()),
                )),
                Action::Count { req_id } => acts.extend(l.count_done(now, req_id, Ok(0))),
                Action::Ended(r) => ended = Some(r),
                Action::Notice(_) => {}
            }
        }
        assert!(acts.is_empty(), "more than 100 actions at {now}");
        if ended.is_some() {
            break;
        }
        now = l.next_wake().unwrap();
        acts = l.poll(now);
    }
    let r = ended.expect("the loop ends");
    assert_eq!(
        (r.rounds, r.cancel_all_calls, r.zero_since),
        (1, 1, Some(T0))
    );
    assert_eq!(now, T0 + 30_000);
}

#[test]
fn a_cancel_all_that_succeeds_clears_the_last_error() {
    // Round 1's cancel-all fails, every later one is accepted: the
    // progress notice shows no stale error.
    let mut l = kill_loop();
    let mut first = true;
    let log = drive(&mut l, 100, 20_000, |now, count| {
        if now >= T0 + 70_000 {
            l_zero(count)
        } else if count {
            Some((10, Ans::Count(Ok(1))))
        } else if std::mem::take(&mut first) {
            Some((10, Ans::Cancel(CancelAnswer::Failed("HTTP 503".into()))))
        } else {
            Some((10, Ans::Cancel(done(1))))
        }
    });
    assert!(notices_at(&log).iter().any(|(t, n)| *t == T0 + 60_000
        && matches!(
            n,
            Notice::Progress {
                last_error: None,
                remaining: Ok(1),
                ..
            }
        )));
}

#[test]
fn a_cancel_all_not_sent_waits_for_the_next_round() {
    for kind in [
        NotSent::Latched,
        NotSent::Paused,
        NotSent::Local("x".into()),
    ] {
        let mut l = kill_loop();
        let mut c = cancel_id(&l.start(T0, "h", StopOutcome::Latched));
        let mut t = T0;
        let mut progress = Vec::new();
        // Nothing left the host: no count, the next cancel-all at the next
        // round, none counted as sent.
        while t < T0 + 60_000 {
            assert_eq!(l.cancel_done(t, c, CancelAnswer::NotSent(kind.clone())), []);
            assert_eq!(l.phase(), Phase::Repeat);
            assert_eq!(l.next_wake(), Some(t + 1000));
            assert_eq!(l.poll(t + 999), []);
            t += 1000;
            let acts = l.poll(t);
            progress.extend(notices(&acts));
            c = cancel_id(&acts);
        }
        assert_eq!(l.round(), 61);
        assert_eq!(l.cancel_all_calls(), 0);
        // The progress notice keeps the kind.
        assert_eq!(
            progress,
            [Notice::Progress {
                round: 61,
                remaining: Err("아직 세지 않음".into()),
                cancel_all_calls: 0,
                last_error: Some(LastError::NotSent(kind.clone())),
            }]
        );
    }
    // A cancel-all not sent keeps the last known count (review 1 N2).
    let mut l = kill_loop();
    let c = cancel_id(&l.start(T0, "h", StopOutcome::Latched));
    let k = count_id(&l.cancel_done(T0 + 10, c, done(0)));
    assert_eq!(l.count_done(T0 + 20, k, Ok(4)), []);
    let c = cancel_id(&l.poll(T0 + 1000));
    let not_sent = CancelAnswer::NotSent(NotSent::Paused);
    assert_eq!(l.cancel_done(T0 + 1010, c, not_sent), []);
    assert_eq!((l.remaining(), l.cancel_all_calls()), (&Ok(4), 1));
}

#[test]
fn a_failed_count_is_never_zero() {
    let mut l = kill_loop();
    let c = cancel_id(&l.start(T0, "h", StopOutcome::Latched));
    let k = count_id(&l.cancel_done(T0 + 10, c, done(0)));
    // A count error is no zero: no watch, the next round.
    assert_eq!(l.count_done(T0 + 20, k, Err("HTTP 503".into())), []);
    assert_eq!(l.remaining(), &Err("HTTP 503".into()));
    assert_eq!(l.phase(), Phase::Repeat);
    let c = cancel_id(&l.poll(T0 + 1000));
    let k = count_id(&l.cancel_done(T0 + 1010, c, done(0)));
    assert_eq!(l.count_done(T0 + 1020, k, Ok(0)), []);
    assert_eq!(l.phase(), Phase::Watch);
    // An error in the watch: back to C4, then C2; the next zero starts a
    // new watch.
    // (C4 waits until t_2 + round_ms, long past: C2 at once.)
    let k = count_id(&l.poll(T0 + 6020));
    let c = cancel_id(&l.count_done(T0 + 6030, k, Err("unreadable".into())));
    assert_eq!(l.round(), 3);
    let k = count_id(&l.cancel_done(T0 + 6040, c, done(0)));
    let z = T0 + 6050;
    assert_eq!(l.count_done(z, k, Ok(0)), []);
    assert_eq!(l.next_wake(), Some(z + 5000));
    let mut last = Vec::new();
    for due in (1..=6).map(|i| z + i * 5000) {
        let k = count_id(&l.poll(due));
        last = l.count_done(due, k, Ok(0));
    }
    assert!(matches!(
        last.last(),
        Some(Action::Ended(Report {
            rounds: 3,
            zero_since: Some(at),
            ..
        })) if *at == z
    ));
}

/// A loop in the watch: round 1's cancel-all and count answered at once,
/// zero at `T0 + 20` (`z`).
fn in_watch() -> (KillLoop, u64) {
    let mut l = kill_loop();
    let c = cancel_id(&l.start(T0, "h", StopOutcome::Unconfirmed));
    let k = count_id(&l.cancel_done(T0 + 10, c, done(1)));
    assert_eq!(l.count_done(T0 + 20, k, Ok(0)), []);
    assert_eq!(l.phase(), Phase::Watch);
    (l, T0 + 20)
}

#[test]
fn a_late_order_in_the_watch_goes_back_to_cancel_at_once() {
    let (mut l, z) = in_watch();
    let k = count_id(&l.poll(z + 5000));
    // An order accepted late: C2 at once, round 2, the watch is over.
    let c = cancel_id(&l.count_done(z + 5010, k, Ok(2)));
    assert_eq!(
        (l.phase(), l.round(), l.remaining()),
        (Phase::Cancel, 2, &Ok(2))
    );
    let k = count_id(&l.cancel_done(z + 5020, c, done(2)));
    // Zero again: a new watch from the new z.
    let z2 = z + 5030;
    assert_eq!(l.count_done(z2, k, Ok(0)), []);
    assert_eq!(l.next_wake(), Some(z2 + 5000));
    let mut last = Vec::new();
    for due in (1..=6).map(|i| z2 + i * 5000) {
        let k = count_id(&l.poll(due));
        last = l.count_done(due + 1, k, Ok(0));
    }
    assert!(matches!(
        last.last(),
        Some(Action::Ended(Report { rounds: 2, zero_since: Some(at), .. })) if *at == z2
    ));

    // The round the late order starts begins at its count: a count above
    // zero after its cancel-all waits for that instant + round_ms, so the
    // next cancel-all is not sent at once (review 1 S1).
    let (mut l, z) = in_watch();
    let k = count_id(&l.poll(z + 5000));
    let c = cancel_id(&l.count_done(z + 5010, k, Ok(2)));
    let k = count_id(&l.cancel_done(z + 5020, c, done(1)));
    assert_eq!(l.count_done(z + 5030, k, Ok(1)), []);
    assert_eq!(
        (l.phase(), l.round(), l.next_wake()),
        (Phase::Repeat, 2, Some(z + 5010 + 1000))
    );
    assert_eq!(l.poll(z + 5010 + 999), []);
    cancel_id(&l.poll(z + 5010 + 1000));
    assert_eq!(l.round(), 3);
}

#[test]
fn the_watch_ends_only_on_the_count_due_at_its_end() {
    let (mut l, z) = in_watch();
    for due in (1..=4).map(|i| z + i * 5000) {
        let k = count_id(&l.poll(due));
        assert_eq!(l.count_done(due + 5, k, Ok(0)), [], "the zero at {due}");
    }
    // The count due at z + 25 s answers zero only at z + 31 s (inside its
    // deadline): it is not the count due at the end, so it does not end
    // the watch.
    let k = count_id(&l.poll(z + 25_000));
    assert_eq!(l.poll(z + 30_000), [], "never two requests");
    let k_end = count_id(&l.count_done(z + 31_000, k, Ok(0)));
    assert_eq!(l.phase(), Phase::Watch);
    // The next count went at once, after z + 30 s: it ends the watch,
    // however late it answers.
    assert_eq!(l.poll(z + 40_000), []);
    let acts = l.count_done(z + 41_000, k_end, Ok(0));
    assert!(matches!(acts.last(), Some(Action::Ended(_))), "{acts:?}");
    assert_eq!(l.phase(), Phase::Ended);
}

#[test]
fn a_watch_that_is_no_multiple_of_its_step_ends_at_watch_ms() {
    // watch_ms 30,000 with watch_every_ms 7,000 (config-valid): counts at
    // z + 7, 14, 21, 28 s, and the last at z + 30 s, not z + 35 s (review
    // 1 N1).
    let timing = Kill {
        watch_every_ms: 7000,
        ..Kill::default()
    };
    let mut l = KillLoop::new(timing, RT);
    let c = cancel_id(&l.start(T0, "h", StopOutcome::Latched));
    let k = count_id(&l.cancel_done(T0 + 10, c, done(0)));
    let z = T0 + 20;
    assert_eq!(l.count_done(z, k, Ok(0)), []);
    for due in (1..=4).map(|i| z + i * 7000) {
        assert_eq!(l.next_wake(), Some(due));
        let k = count_id(&l.poll(due));
        assert_eq!(l.count_done(due + 1, k, Ok(0)), []);
    }
    assert_eq!(l.next_wake(), Some(z + 30_000));
    let k = count_id(&l.poll(z + 30_000));
    let acts = l.count_done(z + 30_001, k, Ok(0));
    assert!(matches!(acts.last(), Some(Action::Ended(_))), "{acts:?}");

    // A step longer than the watch (below the config range; `new` takes
    // the timings as they are): the first count is already the last, at
    // z + watch_ms.
    let timing = Kill {
        watch_ms: 10_000,
        watch_every_ms: 15_000,
        ..Kill::default()
    };
    let mut l = KillLoop::new(timing, RT);
    let c = cancel_id(&l.start(T0, "h", StopOutcome::Latched));
    let k = count_id(&l.cancel_done(T0 + 10, c, done(0)));
    assert_eq!(l.count_done(z, k, Ok(0)), []);
    assert_eq!(l.next_wake(), Some(z + 10_000));
    let k = count_id(&l.poll(z + 10_000));
    let acts = l.count_done(z + 10_001, k, Ok(0));
    assert!(matches!(acts.last(), Some(Action::Ended(_))), "{acts:?}");
}

#[test]
fn two_requests_of_the_loop_are_never_in_flight() {
    let (mut l, z) = in_watch();
    let k = count_id(&l.poll(z + 5000));
    // The count due at z + 10 s waits while the one of z + 5 s is open...
    assert_eq!(l.poll(z + 10_000), []);
    assert_eq!(l.poll(z + 12_000), []);
    // ...and goes at once at its answer.
    let k2 = count_id(&l.count_done(z + 12_000, k, Ok(0)));
    // Or at the deadline of the open one: that count failed, so C4, and
    // the next request is one cancel-all.
    assert_eq!(l.next_wake(), Some(z + 12_000 + EXTRA));
    assert_eq!(l.poll(z + 15_000), []);
    assert_eq!(l.poll(z + 12_000 + EXTRA - 1), []);
    let acts = l.poll(z + 12_000 + EXTRA);
    let c = cancel_id(&acts);
    assert_eq!(l.round(), 2);
    assert_eq!(l.count_done(z + 12_000 + EXTRA, k2, Ok(0)), [], "past");
    // A cancel-all that hangs: no count, no next round beside it.
    assert_eq!(l.poll(z + 12_000 + EXTRA + 5000), []);
    let k3 = count_id(&l.poll(z + 12_000 + 2 * EXTRA));
    assert_eq!(
        l.cancel_done(z + 12_000 + 2 * EXTRA, c, done(0)),
        [],
        "past"
    );
    assert_eq!(l.phase(), Phase::Count);
    let _ = k3;
}

#[test]
fn a_request_past_its_deadline_fails_and_a_late_answer_is_ignored() {
    let mut l = kill_loop();
    let c = cancel_id(&l.start(T0, "h", StopOutcome::Latched));
    // The cancel-all has its deadline at emission + request_timeout_ms +
    // 11,000 ms.
    assert_eq!(l.next_wake(), Some(T0 + EXTRA));
    assert_eq!(l.poll(T0 + EXTRA - 1), []);
    // Past it: Failed, so counted as sent and followed by a count.
    let k = count_id(&l.poll(T0 + EXTRA));
    assert_eq!(l.cancel_all_calls(), 1);
    // A later answer with that req_id is ignored.
    assert_eq!(l.cancel_done(T0 + EXTRA + 1, c, done(5)), []);
    assert_eq!((l.cancel_all_calls(), l.ignored()), (1, 1));
    assert_eq!(l.phase(), Phase::Count);
    // The count past its deadline: a count error, never zero; t_1 +
    // round_ms has long passed, so C2 at once.
    assert_eq!(l.next_wake(), Some(T0 + 2 * EXTRA));
    let c2 = cancel_id(&l.poll(T0 + 2 * EXTRA));
    assert_eq!(l.remaining(), &Err("no answer before the deadline".into()));
    assert_eq!(l.count_done(T0 + 2 * EXTRA + 1, k, Ok(0)), []);
    assert_eq!((l.phase(), l.ignored()), (Phase::Cancel, 2));
    // An answer to another req_id, or of the other kind, is ignored too.
    assert_eq!(l.count_done(T0 + 2 * EXTRA + 2, c2, Ok(0)), []);
    assert_eq!(l.count_page(T0 + 2 * EXTRA + 2, c2), []);
    assert_eq!(l.cancel_done(T0 + 2 * EXTRA + 2, c2 + 7, done(0)), []);
    assert_eq!(l.ignored(), 5);
    // c2 passed its deadline too: the first poll after it (at the first
    // progress mark) fails it and sends its count.
    let acts = l.poll(T0 + 60_000);
    let k3 = count_id(&acts);
    assert_eq!(
        notices(&acts),
        [Notice::Progress {
            round: 2,
            remaining: Err("no answer before the deadline".into()),
            cancel_all_calls: 2,
            last_error: Some(LastError::Failed("no answer before the deadline".into())),
        }]
    );
    // A page of the earlier, failed count does not move the deadline of
    // the count open now (review 1 S2); a page of the open one does.
    assert_eq!(l.next_wake(), Some(T0 + 60_000 + EXTRA));
    assert_eq!(l.count_page(T0 + 70_000, k), []);
    assert_eq!((l.next_wake(), l.ignored()), (Some(T0 + 60_000 + EXTRA), 6));
    assert_eq!(l.count_page(T0 + 70_000, k3), []);
    assert_eq!((l.next_wake(), l.ignored()), (Some(T0 + 70_000 + EXTRA), 6));
}

#[test]
fn an_answer_handled_at_its_deadline_is_past_it() {
    let past = || Err::<usize, String>("no answer before the deadline".into());
    for late in [0, 1] {
        // A cancel-all answered at its deadline (or after it, before the
        // poll that would see the deadline): Failed.
        let mut l = kill_loop();
        let c = cancel_id(&l.start(T0, "h", StopOutcome::Latched));
        let k = count_id(&l.cancel_done(T0 + EXTRA + late, c, done(0)));
        assert_eq!(
            l.poll(T0 + EXTRA + late),
            [],
            "the deadline is not seen twice"
        );
        let acts = l.poll(T0 + 60_000);
        assert!(matches!(
            notices(&acts)[..],
            [Notice::Progress { last_error: Some(LastError::Failed(ref w)), .. }]
                if w == "no answer before the deadline"
        ));
        assert_eq!(l.cancel_all_calls(), 1);

        // A count answered at its deadline: a count error, not zero.
        let mut l = kill_loop();
        let c = cancel_id(&l.start(T0, "h", StopOutcome::Latched));
        let k0 = count_id(&l.cancel_done(T0, c, done(0)));
        let acts = l.count_done(T0 + EXTRA + late, k0, Ok(0));
        assert_eq!(l.remaining(), &past());
        cancel_id(&acts);
        let _ = k;

        // A page at its deadline: a count error, the deadline not moved,
        // and the count's later result ignored.
        let mut l = kill_loop();
        let c = cancel_id(&l.start(T0, "h", StopOutcome::Latched));
        let k0 = count_id(&l.cancel_done(T0, c, done(0)));
        assert_eq!(l.count_page(T0 + EXTRA - 1, k0), []);
        let deadline = T0 + EXTRA - 1 + EXTRA;
        assert_eq!(l.next_wake(), Some(deadline));
        let acts = l.count_page(deadline + late, k0);
        assert_eq!(l.remaining(), &past());
        cancel_id(&acts);
        assert_eq!(l.count_done(deadline + late, k0, Ok(0)), []);
        assert_eq!(l.phase(), Phase::Cancel);
    }
}

#[test]
fn a_slow_paged_count_is_not_failed_while_pages_answer() {
    // 11 pages, 1,300 ms each, request_timeout_ms 2,000: the first
    // deadline (13,000 ms) falls inside the count, every page moves it.
    let mut l = kill_loop();
    let c = cancel_id(&l.start(T0, "h", StopOutcome::Latched));
    let k = count_id(&l.cancel_done(T0, c, done(0)));
    for page in 1..=10u64 {
        let at = T0 + page * 1300;
        assert_eq!(l.poll(at), [], "page {page}");
        assert_eq!(l.count_page(at, k), [], "page {page}");
        assert_eq!(l.next_wake(), Some(at + EXTRA));
        assert_eq!(l.poll(at), []);
    }
    // The result, no count error; t_1 + round_ms has passed, so the next
    // round at once.
    cancel_id(&l.count_done(T0 + 14_300, k, Ok(3)));
    assert_eq!((l.remaining(), l.round()), (&Ok(3), 2));
}

/// One answer of the scripted venue: a cancel-all's or a count's.
#[derive(Clone)]
enum Ans {
    Cancel(CancelAnswer),
    Count(Result<usize, String>),
}

/// Drives `l` from its start at `T0`: a poll every `poll_ms`, and each
/// request answered as `reply(now, is_count)` says (`Some((delay, answer))`,
/// or `None`: it hangs). Answers due at a poll's instant go first. Asserts
/// that no request is emitted while another is open (answered, or before
/// its deadline). Step-bounded. Returns every action with its instant.
fn drive(
    l: &mut KillLoop,
    poll_ms: u64,
    max_steps: usize,
    reply: impl FnMut(u64, bool) -> Option<(u64, Ans)>,
) -> Vec<(u64, Action)> {
    let (log, ended) = run(l, poll_ms, max_steps, reply);
    assert!(ended, "the step bound of {max_steps} was reached");
    log
}

/// [`drive`] without the end: returns the log and whether the loop ended
/// within `max_steps`.
fn run(
    l: &mut KillLoop,
    poll_ms: u64,
    max_steps: usize,
    mut reply: impl FnMut(u64, bool) -> Option<(u64, Ans)>,
) -> (Vec<(u64, Action)>, bool) {
    let mut log = Vec::new();
    let mut pending: Vec<(u64, u64, Ans)> = Vec::new();
    let mut closes_at: Option<u64> = None;
    let mut now = T0;
    let mut next_poll = T0 + poll_ms;
    let mut acts = l.start(now, "h", StopOutcome::Latched);
    for _ in 0..max_steps {
        let mut ended = false;
        for a in acts.drain(..) {
            let id = match a {
                Action::CancelAll { req_id } => Some((req_id, false)),
                Action::Count { req_id } => Some((req_id, true)),
                Action::Ended(_) => {
                    ended = true;
                    None
                }
                Action::Notice(_) => None,
            };
            if let Some((id, count)) = id {
                if let Some(c) = closes_at {
                    assert!(c <= now, "two requests in flight at {now}");
                }
                let deadline = now + EXTRA;
                closes_at = Some(deadline);
                if let Some((delay, ans)) = reply(now, count) {
                    pending.push((now + delay, id, ans));
                    closes_at = Some((now + delay).min(deadline));
                }
            }
            log.push((now, a));
        }
        if ended {
            return (log, true);
        }
        let first = pending
            .iter()
            .enumerate()
            .min_by_key(|(_, p)| p.0)
            .map(|(i, p)| (i, p.0));
        match first {
            Some((i, at)) if at <= next_poll => {
                let (_, id, ans) = pending.swap_remove(i);
                now = at;
                acts = match ans {
                    Ans::Cancel(a) => l.cancel_done(now, id, a),
                    Ans::Count(r) => l.count_done(now, id, r),
                };
            }
            _ => {
                now = next_poll;
                next_poll += poll_ms;
                acts = l.poll(now);
            }
        }
    }
    (log, false)
}

fn notices_at(log: &[(u64, Action)]) -> Vec<(u64, Notice)> {
    log.iter()
        .filter_map(|(t, a)| match a {
            Action::Notice(n) => Some((*t, n.clone())),
            _ => None,
        })
        .collect()
}

#[test]
fn a_progress_notice_comes_every_60_s_also_with_an_unchanged_count() {
    // One order that stays: every round counts 1.
    let mut l = kill_loop();
    let log = drive(&mut l, 100, 20_000, |now, count| {
        if now >= T0 + 200_000 {
            l_zero(count)
        } else if count {
            Some((10, Ans::Count(Ok(1))))
        } else {
            Some((10, Ans::Cancel(done(0))))
        }
    });
    let got = notices_at(&log);
    let progress: Vec<(u64, Notice)> = got
        .iter()
        .filter(|(_, n)| matches!(n, Notice::Progress { .. }))
        .cloned()
        .collect();
    assert_eq!(
        progress.iter().map(|(t, _)| t - T0).collect::<Vec<_>>(),
        [60_000, 120_000, 180_000]
    );
    for (_, n) in &progress {
        assert!(
            matches!(
                n,
                Notice::Progress {
                    remaining: Ok(1),
                    last_error: None,
                    ..
                }
            ),
            "{n:?}"
        );
    }

    // A late poll does not move the next mark: polls at +65 s and then
    // every 100 ms give notices at +65 s and +120 s.
    let mut l = kill_loop();
    let c = cancel_id(&l.start(T0, "h", StopOutcome::Latched));
    let mut marks = Vec::new();
    let _ = l.cancel_done(T0 + 1, c, CancelAnswer::NotSent(NotSent::Paused));
    let mut t = T0 + 65_000;
    while t <= T0 + 130_000 {
        for a in l.poll(t) {
            match a {
                Action::Notice(_) => marks.push(t - T0),
                Action::CancelAll { req_id } => {
                    let _ = l.cancel_done(t, req_id, CancelAnswer::NotSent(NotSent::Paused));
                }
                other => panic!("{other:?}"),
            }
        }
        t += 100;
    }
    assert_eq!(marks, [65_000, 120_000]);

    // Two marks passed at one poll: one notice, the next at the first mark
    // after it.
    let mut l = kill_loop();
    let c = cancel_id(&l.start(T0, "h", StopOutcome::Latched));
    let _ = l.cancel_done(T0 + 1, c, CancelAnswer::NotSent(NotSent::Paused));
    let acts = l.poll(T0 + 130_000);
    assert_eq!(notices(&acts).len(), 1);
    let c = cancel_id(&acts);
    let _ = l.cancel_done(T0 + 130_000, c, CancelAnswer::NotSent(NotSent::Paused));
    assert_eq!(notices(&l.poll(T0 + 179_999)).len(), 0);
    assert_eq!(notices(&l.poll(T0 + 180_000)).len(), 1);
}

/// The venue after a failure is over: cancel-all accepted, zero open.
fn l_zero(count: bool) -> Option<(u64, Ans)> {
    if count {
        Some((10, Ans::Count(Ok(0))))
    } else {
        Some((10, Ans::Cancel(done(0))))
    }
}

#[test]
fn the_notice_rule_sends_no_change_notice() {
    // The count goes down every round (5, 4, 3, 2, 1, then 1): no notice at
    // a change; the progress notice at 60 s shows the latest count.
    let mut l = kill_loop();
    let mut counts = vec![5usize, 4, 3, 2].into_iter();
    let log = drive(&mut l, 100, 20_000, |now, count| {
        if now >= T0 + 70_000 {
            l_zero(count)
        } else if count {
            Some((10, Ans::Count(Ok(counts.next().unwrap_or(1)))))
        } else {
            Some((10, Ans::Cancel(done(1))))
        }
    });
    let got = notices_at(&log);
    assert_eq!(got.len(), 3, "start, one progress, final: {got:?}");
    assert_eq!(got[0], (T0, Notice::Start { round_ms: 1000 }));
    assert_eq!(
        got[1],
        (
            T0 + 60_000,
            Notice::Progress {
                round: 61,
                remaining: Ok(1),
                cancel_all_calls: 60,
                last_error: None,
            }
        )
    );
    assert!(matches!(got[2].1, Notice::Final { .. }));
}

#[test]
fn ten_minutes_of_failing_queries_make_at_most_12_notices() {
    // Cancel-all and count fail (5xx, or no answer until the deadline) for
    // 600 s from the start, then zero. The runtime polls every 100 ms, or
    // late, every 7,000 ms.
    for poll_ms in [100, 7000] {
        let mut l = kill_loop();
        let mut n = 0u64;
        let log = drive(&mut l, poll_ms, 100_000, |now, count| {
            n += 1;
            if now >= T0 + 600_000 {
                return l_zero(count);
            }
            match (n % 3, count) {
                (0, _) => None,
                (_, true) => Some((50, Ans::Count(Err("HTTP 503".into())))),
                (_, false) => Some((50, Ans::Cancel(CancelAnswer::Failed("HTTP 503".into())))),
            }
        });
        let got = notices_at(&log);
        let kinds = |f: fn(&Notice) -> bool| got.iter().filter(|(_, n)| f(n)).count();
        assert_eq!(kinds(|n| matches!(n, Notice::Start { .. })), 1, "{poll_ms}");
        assert_eq!(
            kinds(|n| matches!(n, Notice::Progress { .. })),
            10,
            "{poll_ms}"
        );
        assert_eq!(kinds(|n| matches!(n, Notice::Final { .. })), 1, "{poll_ms}");
        assert!(matches!(got.last(), Some((_, Notice::Final { .. }))));
        for (t, _) in &got {
            let inside = got
                .iter()
                .filter(|(u, _)| *u >= *t && *u <= *t + 600_000)
                .count();
            assert!(inside <= 12, "{inside} notices from {t} ({poll_ms})");
        }
        // The final notice is not held back by notice_ms: it goes in the
        // instant of the count that ends the watch.
        let (end_at, _) = log.last().unwrap();
        assert_eq!(got.last().unwrap().0, *end_at);
    }
}

#[test]
fn abort_sends_one_notice_and_ends() {
    // Before the start: nothing (the kill sequence tells its own abort).
    let mut l = kill_loop();
    assert_eq!(l.abort(T0, AbortWhy::Shutdown), []);
    assert_eq!(l.phase(), Phase::Idle);

    // In the rounds, with a known count and a request open.
    let mut l = kill_loop();
    let c = cancel_id(&l.start(T0, "h", StopOutcome::Latched));
    let k = count_id(&l.cancel_done(T0 + 10, c, done(3)));
    let c = cancel_id(&{
        assert_eq!(l.count_done(T0 + 20, k, Ok(3)), []);
        l.poll(T0 + 1000)
    });
    assert_eq!(
        l.abort(T0 + 1500, AbortWhy::Shutdown),
        [
            Action::Notice(Notice::Abort {
                why: AbortWhy::Shutdown,
                round: 2,
                remaining: Some(3),
                cancel_all_calls: 1,
            }),
            Action::Ended(Report {
                rounds: 2,
                cancel_all_calls: 1,
                zero_since: None,
                aborted: true,
            }),
        ]
    );
    // Then nothing: no request, no notice, no second end.
    assert_eq!((l.phase(), l.next_wake()), (Phase::Aborted, None));
    assert_eq!(l.cancel_done(T0 + 1600, c, done(0)), []);
    assert_eq!(l.poll(T0 + 120_000), []);
    assert_eq!(l.abort(T0 + 120_000, AbortWhy::Shutdown), []);

    // In the watch, and with an unknown count.
    let (mut l, _) = in_watch();
    let acts = l.abort(T0 + 100, AbortWhy::StartRefused("marker path".into()));
    assert!(matches!(
        acts[..],
        [
            Action::Notice(Notice::Abort {
                remaining: Some(0),
                ..
            }),
            Action::Ended(Report { aborted: true, .. })
        ]
    ));
    let mut l = kill_loop();
    let c = cancel_id(&l.start(T0, "h", StopOutcome::Latched));
    let _ = l.cancel_done(T0, c, CancelAnswer::NotSent(NotSent::Latched));
    let acts = l.abort(T0 + 100, AbortWhy::Shutdown);
    assert!(matches!(
        acts[..],
        [
            Action::Notice(Notice::Abort {
                remaining: None,
                round: 1,
                cancel_all_calls: 0,
                ..
            }),
            _
        ]
    ));
}

#[test]
fn the_outcome_set_during_the_loop_is_in_the_final_notice() {
    // A halt retried during the loop latched (U19b3b): the final notice
    // names the latch, not the outcome the loop started with.
    let (mut l, z) = in_watch();
    l.set_outcome(StopOutcome::Latched);
    let mut last = Vec::new();
    for due in (1..=6).map(|i| z + i * 5000) {
        let k = count_id(&l.poll(due));
        last = l.count_done(due, k, Ok(0));
    }
    assert!(matches!(
        notices(&last)[..],
        [Notice::Final {
            outcome: StopOutcome::Latched,
            ..
        }]
    ));
    let mut l = kill_loop();
    let _ = l.start(T0, "h", StopOutcome::Unconfirmed);
    l.set_outcome(StopOutcome::StoppedMarked);
    assert_eq!(l.outcome(), StopOutcome::StoppedMarked);
    // Without a change the final notice names the outcome of the start.
    let mut l = KillLoop::new(Kill::default(), RT);
    let c = cancel_id(&l.start(T0, "h", StopOutcome::StoppedUncheckedMarker));
    let k = count_id(&l.cancel_done(T0, c, done(0)));
    assert_eq!(l.count_done(T0, k, Ok(0)), []);
    let mut last = Vec::new();
    for due in (1..=6).map(|i| T0 + i * 5000) {
        let k = count_id(&l.poll(due));
        last = l.count_done(due, k, Ok(0));
    }
    assert!(matches!(
        notices(&last)[..],
        [Notice::Final {
            outcome: StopOutcome::StoppedUncheckedMarker,
            ..
        }]
    ));
}

#[test]
fn a_second_start_does_nothing() {
    // One kill, one loop (review 1 S3): a start while a request is open, in
    // the watch, after the end or after an abort returns nothing and
    // changes nothing.
    let state = |l: &KillLoop| {
        (
            l.phase(),
            l.round(),
            l.next_wake(),
            l.cancel_all_calls(),
            l.header().to_string(),
            l.outcome(),
        )
    };
    let again = |l: &mut KillLoop, now: u64| {
        let before = state(l);
        assert_eq!(l.start(now, "h2", StopOutcome::Unconfirmed), []);
        assert_eq!(state(l), before);
    };
    let mut l = kill_loop();
    let c = cancel_id(&l.start(T0, "h", StopOutcome::Latched));
    again(&mut l, T0 + 1);
    let k = count_id(&l.cancel_done(T0 + 10, c, done(0)));
    again(&mut l, T0 + 11);
    assert_eq!(l.count_done(T0 + 20, k, Ok(0)), []);
    again(&mut l, T0 + 21);
    assert_eq!(l.phase(), Phase::Watch);
    let mut last = Vec::new();
    for due in (1..=6).map(|i| T0 + 20 + i * 5000) {
        let k = count_id(&l.poll(due));
        last = l.count_done(due, k, Ok(0));
    }
    assert!(matches!(last.last(), Some(Action::Ended(_))));
    again(&mut l, T0 + 40_000);
    assert_eq!(l.phase(), Phase::Ended);
    let (mut l, z) = in_watch();
    assert_eq!(l.abort(z + 1, AbortWhy::Shutdown).len(), 2);
    again(&mut l, z + 2);
    assert_eq!(l.phase(), Phase::Aborted);
}

#[test]
fn the_next_wake_is_the_earliest_due_instant_or_progress_mark() {
    // A request open across the first progress mark: the wake is the mark
    // (T0 + 60 s), not the deadline (T0 + 55 s + 13 s; review 1 S4).
    let mut l = kill_loop();
    let c = cancel_id(&l.start(T0, "h", StopOutcome::Latched));
    let k = count_id(&l.cancel_done(T0 + 10, c, done(3)));
    assert_eq!(l.count_done(T0 + 20, k, Ok(3)), []);
    assert_eq!(l.next_wake(), Some(T0 + 1000));
    cancel_id(&l.poll(T0 + 55_000));
    assert_eq!(l.next_wake(), Some(T0 + 60_000));
    assert_eq!(notices(&l.poll(T0 + 60_000)).len(), 1);
    assert_eq!(l.next_wake(), Some(T0 + 55_000 + EXTRA));

    // A cancel-all the budget holds back: the wake is the budget's
    // instant, the older of the last two emissions + 1,000 ms (round_ms
    // 100, below the config range, so the budget binds).
    let fast = Kill {
        round_ms: 100,
        ..Kill::default()
    };
    let mut l = KillLoop::new(fast, RT);
    let c = cancel_id(&l.start(T0, "h", StopOutcome::Latched));
    let k = count_id(&l.cancel_done(T0 + 1, c, done(1)));
    assert_eq!(l.count_done(T0 + 2, k, Ok(1)), []);
    assert_eq!(l.next_wake(), Some(T0 + 100));
    let c = cancel_id(&l.poll(T0 + 100));
    let k = count_id(&l.cancel_done(T0 + 101, c, done(1)));
    assert_eq!(l.count_done(T0 + 102, k, Ok(1)), []);
    assert_eq!(l.poll(T0 + 200), []);
    assert_eq!(
        (l.phase(), l.round(), l.budget_holds()),
        (Phase::Cancel, 3, 1)
    );
    assert_eq!(l.next_wake(), Some(T0 + 1000));
    assert_eq!(l.poll(T0 + 999), []);
    cancel_id(&l.poll(T0 + 1000));
}

/// xorshift64: the test's own seeded generator.
struct Rng(u64);

impl Rng {
    fn below(&mut self, n: u64) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x % n
    }
}

#[test]
fn the_order_budget_holds_under_any_answer_sequence() {
    // Once with the default timings (the budget never binds there), once
    // with round_ms and watch_every_ms 100, below the config ranges, where
    // it must hold cancel-alls back.
    let fast = Kill {
        round_ms: 100,
        watch_every_ms: 100,
        ..Kill::default()
    };
    for (timing, binds) in [(Kill::default(), false), (fast, true)] {
        let mut held = 0;
        let mut ended = 0;
        for seed in 1..=10_000u64 {
            let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
            let mut l = KillLoop::new(timing.clone(), RT);
            let poll_ms = [1, 10, 100, 1000, 7000][rng.below(5) as usize];
            let (log, done_) = run(&mut l, poll_ms, 300, |_, count| {
                let delay = match rng.below(4) {
                    0 => 0,
                    1 => rng.below(50),
                    2 => rng.below(2000),
                    _ => rng.below(20_000),
                };
                if rng.below(10) == 0 {
                    return None;
                }
                let ans = if count {
                    match rng.below(5) {
                        0 | 1 => Ans::Count(Ok(0)),
                        2 | 3 => Ans::Count(Ok(1 + rng.below(3) as usize)),
                        _ => Ans::Count(Err("HTTP 503".into())),
                    }
                } else {
                    match rng.below(8) {
                        0..=3 => Ans::Cancel(done(rng.below(3) as usize)),
                        4 | 5 => Ans::Cancel(CancelAnswer::Failed("HTTP 503".into())),
                        6 => Ans::Cancel(CancelAnswer::NotSent(NotSent::Latched)),
                        _ => Ans::Cancel(CancelAnswer::NotSent(NotSent::Paused)),
                    }
                };
                Some((delay, ans))
            });
            let at: Vec<u64> = log
                .iter()
                .filter(|(_, a)| matches!(a, Action::CancelAll { .. }))
                .map(|(t, _)| *t)
                .collect();
            // At most 2 in any half-open [t, t + 1,000): every third one
            // is at least 1,000 ms after the first.
            for w in at.windows(3) {
                assert!(w[2] - w[0] >= 1000, "seed {seed}: {w:?}");
            }
            // With valid timings no two are closer than 1,000 ms (review 1
            // S1).
            if !binds {
                for w in at.windows(2) {
                    assert!(w[1] - w[0] >= 1000, "seed {seed}: {w:?}");
                }
            }
            held += l.budget_holds();
            ended += u64::from(done_);
        }
        if binds {
            assert!(held > 0, "the budget held nothing back");
        } else {
            assert_eq!(held, 0, "valid timings never meet the budget");
        }
        assert!(ended > 0);
    }
}

// ---- The count reader, Inflight and the rig (U19b2c) ----

/// An order row of `/v5/order/realtime`.
fn order(link: &str, status: &str) -> Value {
    json!({"orderLinkId": link, "orderId": format!("o-{link}-{status}"), "symbol": "BTCUSDT",
        "side": "Buy", "qty": "1", "orderStatus": status})
}

/// One page of `/v5/order/realtime`.
fn orders(rows: Vec<Value>, cursor: &str) -> Answer {
    page(rows, cursor)
}

#[tokio::test]
async fn foreign_orders_count() {
    let ours = watcher::wire_id::encode(7, 1);
    // Open = New, PartiallyFilled, Untriggered, whoever placed the order:
    // ours, a foreign link, an empty link, a row without a link.
    let first = vec![
        order(&ours, "New"),
        order("manual-1", "PartiallyFilled"),
        order("", "Untriggered"),
        json!({"orderId": "x", "orderStatus": "New"}),
        order(&ours, "Filled"),
    ];
    let second = vec![order("web-2", "New"), order("web-3", "Cancelled")];
    let fake = FakeHttp::start(Answer::Close);
    fake.route(ORDERS, [orders(first, "c2"), orders(second, "")]);
    let c = client(&fake);
    let mut pages = 0;
    let n = count_open(&c.client, &*c.clock, RT, &mut || pages += 1).await;
    assert_eq!(n, Ok(5));
    assert_eq!(pages, 2);
    assert_eq!(
        targets(&fake),
        [
            "/v5/order/realtime?category=linear&limit=50&settleCoin=USDT",
            "/v5/order/realtime?category=linear&cursor=c2&limit=50&settleCoin=USDT",
        ]
    );

    // No open order at all: zero, the one answer that ends a watch.
    let fake = FakeHttp::start(Answer::Close);
    fake.route(ORDERS, [orders(vec![order(&ours, "Filled")], "")]);
    let c = client(&fake);
    assert_eq!(count_open(&c.client, &*c.clock, RT, &mut || {}).await, Ok(0));

    // What does not read is a count error, never a number.
    let not_a_list = Answer::Reply(
        200,
        r#"{"retCode":0,"retMsg":"OK","result":{"list":{},"nextPageCursor":""}}"#.into(),
    );
    for (case, answers, sent) in [
        ("list not a list", vec![not_a_list], 1),
        (
            "a row without orderStatus",
            vec![orders(vec![json!({"orderLinkId": "a"})], "")],
            1,
        ),
        (
            "a full page without a cursor",
            vec![orders(vec![order("a", "New"); 50], "")],
            1,
        ),
        (
            "the cursor just sent again",
            vec![orders(vec![], "c2"), orders(vec![], "c2")],
            2,
        ),
        ("HTTP 503", vec![Answer::Reply(503, "busy".into())], 1),
    ] {
        let fake = FakeHttp::start(Answer::Close);
        fake.route(ORDERS, answers);
        let c = client(&fake);
        let n = count_open(&c.client, &*c.clock, RT, &mut || {}).await;
        assert!(n.is_err(), "{case}: {n:?}");
        assert_eq!(fake.requests().len(), sent, "{case}");
    }
}

/// A client shared with the tasks of `Inflight`.
fn shared(fake: &FakeHttp) -> (Arc<Client>, Arc<ManualClock>) {
    let c = client(fake);
    (Arc::new(c.client), c.clock)
}

fn cancel_ok() -> Answer {
    Answer::Reply(
        200,
        r#"{"retCode":0,"retMsg":"OK","result":{"list":[],"success":"1"},"time":1}"#.into(),
    )
}

/// Waits in real time (at most 5 s) until `f` holds.
async fn until(mut f: impl FnMut() -> bool, what: &str) {
    let end = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while !f() {
        assert!(std::time::Instant::now() < end, "waited 5 s for {what}");
        tokio::time::sleep(std::time::Duration::from_millis(1)).await;
    }
}

/// The request action of `acts`.
fn request(acts: &[Action]) -> Action {
    acts.iter()
        .find(|a| matches!(a, Action::CancelAll { .. } | Action::Count { .. }))
        .cloned()
        .unwrap()
}

#[tokio::test]
async fn an_aborted_task_sends_no_answer_to_the_loop() {
    // A cancel-all answered in time, but its answer is still queued when
    // its deadline passes: Inflight aborts the task at the deadline and
    // drops what it sent. The loop gets nothing; poll fails the request.
    let fake = FakeHttp::start(Answer::Close);
    fake.route(CANCEL_ALL, [cancel_ok()]);
    let (client, clock) = shared(&fake);
    let mut inf = Inflight::new(client.clone(), clock.clone(), RT);
    let mut l = kill_loop();
    let acts = l.start(T0, "h", StopOutcome::Latched);
    let cancel = cancel_id(&acts);
    assert!(inf.start(&request(&acts)));
    assert_eq!(inf.open(), 1);
    until(|| inf.queued() == 1, "the cancel-all's answer").await;
    // One ms before the deadline nothing is aborted.
    assert_eq!(l.open_request(), Some((cancel, T0 + EXTRA)));
    assert_eq!(inf.abort_due(T0 + EXTRA - 1, l.open_request()), 0);
    clock.set(T0 + EXTRA);
    assert_eq!(inf.abort_due(T0 + EXTRA, l.open_request()), 1);
    assert_eq!(inf.take(), []);
    assert_eq!((inf.open(), inf.aborted(), inf.stray()), (0, 1, 1));
    let acts = l.poll(T0 + EXTRA);
    assert_eq!(l.ignored(), 0);
    assert_eq!(l.cancel_all_calls(), 1);
    let count = count_id(&acts);

    // A count whose first page answered and whose second hangs: aborted at
    // its deadline (moved by the first page), its queued page is dropped
    // and no further page goes.
    fake.route(
        ORDERS,
        [orders(vec![order("a", "New")], "c2"), Answer::Hang],
    );
    assert!(inf.start(&request(&acts)));
    until(|| fake.requests().len() == 3, "the hanging second page").await;
    until(|| inf.queued() == 1, "the first page").await;
    assert_eq!(l.open_request(), Some((count, T0 + 2 * EXTRA)));
    assert_eq!(inf.abort_due(T0 + 2 * EXTRA, l.open_request()), 1);
    assert_eq!(inf.take(), []);
    assert_eq!((inf.open(), inf.aborted(), inf.stray()), (0, 2, 2));

    // A task the loop no longer has open is aborted at once, before its
    // deadline: here the loop was aborted.
    let fake = FakeHttp::start(Answer::Close);
    fake.route(CANCEL_ALL, [Answer::Hang]);
    let (client, clock) = shared(&fake);
    let mut inf = Inflight::new(client, clock.clone(), RT);
    let mut l = kill_loop();
    let acts = l.start(T0, "h", StopOutcome::Latched);
    assert!(inf.start(&request(&acts)));
    until(|| fake.requests().len() == 1, "the hanging cancel-all").await;
    assert_eq!(inf.abort_due(T0, l.open_request()), 0);
    l.abort(T0, AbortWhy::Shutdown);
    assert_eq!(l.open_request(), None);
    assert_eq!(inf.abort_due(T0, l.open_request()), 1);
    assert_eq!(inf.open(), 0);

    // A new request of the loop aborts a task still live (the loop opens
    // one only when it closed the last), so two never run at once.
    let mut l = kill_loop();
    let acts = l.start(T0, "h", StopOutcome::Latched);
    assert!(inf.start(&request(&acts)));
    assert!(inf.start(&Action::Count { req_id: 99 }));
    assert_eq!((inf.open(), inf.aborted()), (1, 2));
    // Only requests start a task.
    assert!(!inf.start(&Action::Notice(Notice::Start { round_ms: 1000 })));
    assert_eq!(inf.open(), 1);
}

/// The notice kinds in order.
fn kinds(rig: &KillRig) -> Vec<&'static str> {
    rig.notices()
        .iter()
        .map(|(_, n)| match n {
            Notice::Start { .. } => "start",
            Notice::Progress { .. } => "progress",
            Notice::Final { .. } => "final",
            Notice::Abort { .. } => "abort",
        })
        .collect()
}

#[tokio::test]
async fn one_order_left_after_the_first_cancel_all_repeats_until_zero_is_confirmed() {
    // 2 cancel-alls and 8 counts in 31 s of the loop's clock: far under
    // the guard's 60 per 10 s.
    let mut rig = KillRig::new(vec![
        (CANCEL_ALL, vec![at_once(ok_n(3))]),
        (
            ORDERS,
            vec![at_once(open_orders(1, "")), at_once(open_orders(0, ""))],
        ),
    ]);
    let report = rig.run().await;
    assert_eq!(
        report,
        Report {
            rounds: 2,
            cancel_all_calls: 2,
            zero_since: Some(rig::T0 + 1000),
            aborted: false,
        }
    );
    // Round 2 at t_1 + round_ms; the watch counts every 5 s for 30 s.
    assert_eq!(rig.cancel_all_at(), [rig::T0, rig::T0 + 1000]);
    let counts: Vec<u64> = rig
        .requests()
        .into_iter()
        .filter(|(_, p)| p == ORDERS)
        .map(|(t, _)| t - rig::T0)
        .collect();
    assert_eq!(
        counts,
        [0, 1000, 6000, 11_000, 16_000, 21_000, 26_000, 31_000]
    );
    assert_eq!(kinds(&rig), ["start", "final"]);
    let (at, last) = rig.notices().pop().unwrap();
    assert_eq!(at, rig::T0 + 31_000);
    assert!(matches!(
        last,
        Notice::Final {
            round: 2,
            cancel_all_calls: 2,
            ..
        }
    ));
    rig.assert_at_most_2_cancel_alls_per_second();
    assert_eq!((rig.inflight.open(), rig.inflight.aborted()), (0, 0));
    assert_eq!(rig.lp.ignored(), 0);
}

#[tokio::test]
async fn a_5xx_cancel_all_or_count_repeats_until_zero_is_confirmed() {
    // A 503 cancel-all (sent, so a count follows), a 503 count (an error,
    // never zero), two orders left, then zero: 3 cancel-alls and 9 counts.
    let mut rig = KillRig::new(vec![
        (
            CANCEL_ALL,
            vec![at_once(http(503)), at_once(ok_n(2)), at_once(ok_n(2))],
        ),
        (
            ORDERS,
            vec![
                at_once(http(503)),
                at_once(open_orders(2, "")),
                at_once(open_orders(0, "")),
            ],
        ),
    ]);
    let report = rig.run().await;
    assert_eq!(
        report,
        Report {
            rounds: 3,
            cancel_all_calls: 3,
            zero_since: Some(rig::T0 + 2000),
            aborted: false,
        }
    );
    assert_eq!(
        rig.cancel_all_at(),
        [rig::T0, rig::T0 + 1000, rig::T0 + 2000]
    );
    assert_eq!(
        rig.paths()[..6],
        [CANCEL_ALL, ORDERS, CANCEL_ALL, ORDERS, CANCEL_ALL, ORDERS]
    );
    // The first cancel-all failed (counted as sent); its count's one page
    // answered 503: a count error.
    let first: Vec<&Arrived> = rig.arrived.iter().take(3).map(|(_, a)| a).collect();
    assert!(
        matches!(first[0], Arrived::Cancel { answer: CancelAnswer::Failed(why), .. } if why.contains("HTTP 503")),
        "{first:?}"
    );
    assert!(
        matches!(first[1..], [Arrived::Page { .. }, Arrived::Counted { result: Err(why), .. }] if why.contains("HTTP 503")),
        "{first:?}"
    );
    assert_eq!(kinds(&rig), ["start", "final"]);
    assert_eq!(rig.notices().last().unwrap().0, rig::T0 + 32_000);
    rig.assert_at_most_2_cancel_alls_per_second();

    // A venue that answers every request 900 ms late (the clock moves at
    // each request): the rounds still keep at most 2 per second.
    let late = |a| rig::slow(a, 900);
    let mut rig = KillRig::new(vec![
        (CANCEL_ALL, vec![late(http(503)), late(ok_n(1))]),
        (
            ORDERS,
            vec![
                late(open_orders(1, "")),
                late(http(503)),
                late(open_orders(0, "")),
            ],
        ),
    ]);
    let report = rig.run().await;
    assert!(!report.aborted);
    assert_eq!(report.rounds, 3);
    assert_eq!(kinds(&rig), ["start", "final"]);
    rig.assert_at_most_2_cancel_alls_per_second();
    assert_eq!(rig.lp.ignored(), 0);
}

#[tokio::test]
async fn a_late_count_page_ends_the_count_task() {
    // The second page answers 14,000 ms after the first (the clock moves at
    // its request), past the deadline the first page set (+13,000): the
    // task itself sends no third page and the count is a count error. No
    // abort is needed: the rig lets the task run to its end.
    let mut rig = KillRig::new(vec![
        (CANCEL_ALL, vec![at_once(ok_n(1))]),
        (
            ORDERS,
            vec![
                at_once(open_orders(1, "c2")),
                rig::slow(open_orders(1, "c3"), 14_000),
                at_once(open_orders(0, "")),
            ],
        ),
    ]);
    rig.start();
    rig.run_until(|r| r.cancel_all_at().len() == 2).await;
    assert_eq!(rig.paths(), [CANCEL_ALL, ORDERS, ORDERS, CANCEL_ALL]);
    assert_eq!(rig.cancel_all_at(), [rig::T0, rig::T0 + 14_000]);
    assert_eq!(rig.lp.remaining(), &Err(PAST_DEADLINE.to_string()));
    assert_eq!(rig.inflight.aborted(), 0);
    // One page told in time, then the result.
    let told: Vec<&Arrived> = rig.arrived.iter().map(|(_, a)| a).collect();
    assert!(
        matches!(
            told[1..],
            [Arrived::Page { .. }, Arrived::Counted { result: Err(_), .. }, ..]
        ),
        "{told:?}"
    );

    // Pages that answer 1,300 ms apart, 11 of them (+14,300 ms in all):
    // each moves the deadline, so the count is not failed (review 2 S2).
    let mut pages: Vec<rig::Reply> = (2..=11)
        .map(|i| rig::slow(open_orders(1, &format!("c{i}")), 1300))
        .collect();
    pages.push(rig::slow(open_orders(0, ""), 1300));
    let mut rig = KillRig::new(vec![(CANCEL_ALL, vec![at_once(ok_n(1))]), (ORDERS, pages)]);
    rig.start();
    rig.run_until(|r| r.cancel_all_at().len() == 2).await;
    assert_eq!(rig.lp.remaining(), &Ok(10));
    assert_eq!(rig.cancel_all_at(), [rig::T0, rig::T0 + 11 * 1300]);
    assert_eq!(rig.paths().len(), 13);
}

#[tokio::test]
async fn a_count_past_its_deadline_sends_no_further_page() {
    // The second page hangs while the rig moves the clock: Inflight aborts
    // the task at the deadline the first page set, the loop fails the
    // count, and the next request is the next round's cancel-all.
    let mut rig = KillRig::new(vec![
        (CANCEL_ALL, vec![at_once(ok_n(1))]),
        (
            ORDERS,
            vec![
                at_once(open_orders(1, "c2")),
                at_once(Answer::Hang),
                at_once(open_orders(0, "")),
            ],
        ),
    ]);
    rig.start();
    rig.run_until(|r| r.cancel_all_at().len() == 2).await;
    assert_eq!(rig.paths(), [CANCEL_ALL, ORDERS, ORDERS, CANCEL_ALL]);
    assert_eq!(rig.cancel_all_at(), [rig::T0, rig::T0 + rig::EXTRA]);
    assert_eq!(rig.inflight.aborted(), 1);
    assert_eq!(rig.lp.remaining(), &Err(PAST_DEADLINE.to_string()));
    // The aborted task is gone: after the client's own timeout (2 s of
    // real time) it sends nothing, so nothing stray comes.
    tokio::time::sleep(std::time::Duration::from_millis(rig::RT + 500)).await;
    assert_eq!(rig.inflight.take(), []);
    assert_eq!(rig.inflight.stray(), 0);
    assert_eq!(rig.paths().len(), 4);
}

#[tokio::test]
async fn more_than_1000_open_orders_is_a_count_error() {
    // 20 full pages, each with a next cursor: a 21st page would be needed.
    // Two rounds of 1 cancel-all + 20 pages = 42 requests in about 1 s of
    // the loop's clock (under the guard's 60 per 10 s), then the test
    // stops before round 3.
    // The fake repeats only its last answer, so both rounds are scripted.
    let pages: Vec<rig::Reply> = (0..40)
        .map(|i| at_once(open_orders(50, &format!("c{}", i % 20 + 2))))
        .collect();
    let mut rig = KillRig::new(vec![(CANCEL_ALL, vec![at_once(ok_n(500))]), (ORDERS, pages)]);
    rig.start();
    rig.run_until(|r| {
        r.cancel_all_at().len() == 2 && r.lp.phase() == Phase::Repeat && r.inflight.open() == 0
    })
    .await;
    let paths = rig.paths();
    assert_eq!(paths.len(), 42);
    for round in paths.chunks(21) {
        assert_eq!(round[0], CANCEL_ALL);
        assert!(round[1..].iter().all(|p| p == ORDERS));
    }
    assert_eq!(
        rig.lp.remaining(),
        &Err("more than 1000 open orders".to_string())
    );
    assert_eq!(rig.lp.round(), 2);
    assert_eq!(rig.cancel_all_at(), [rig::T0, rig::T0 + 1000]);
}

#[tokio::test]
async fn a_cancel_all_not_sent_is_told_from_the_state_before_the_call() {
    // A latched key: nothing reaches the fake, no count follows, and the
    // progress notice says the watcher must restart (C-WATCH part 11,
    // U19b2a's known limit on the error of cancel_all).
    let mut rig = KillRig::new(vec![(CANCEL_ALL, vec![at_once(ok_n(1))])]);
    rig.client.stop_key("made-up rule");
    rig.start();
    rig.run_until(|r| r.lp.round() == 3).await;
    assert!(rig.paths().is_empty());
    let told: Vec<&Arrived> = rig.arrived.iter().map(|(_, a)| a).collect();
    assert!(
        told.iter().all(|a| matches!(
            a,
            Arrived::Cancel {
                answer: CancelAnswer::NotSent(NotSent::Latched),
                ..
            }
        )),
        "{told:?}"
    );
    assert_eq!(rig.lp.cancel_all_calls(), 0);

    // A 403 the cancel-all itself got was sent (Failed, and a count
    // follows); in the pause after it the next cancel-all is not sent.
    let mut rig = KillRig::new(vec![
        (CANCEL_ALL, vec![at_once(http(403))]),
        (ORDERS, vec![at_once(open_orders(0, ""))]),
    ]);
    rig.start();
    rig.run_until(|r| r.lp.round() == 2).await;
    // The count after it is not sent either (the pause), so it fails.
    assert_eq!(rig.paths(), [CANCEL_ALL]);
    let told: Vec<&Arrived> = rig.arrived.iter().map(|(_, a)| a).collect();
    assert!(
        matches!(
            told[..],
            [
                Arrived::Cancel {
                    answer: CancelAnswer::Failed(_),
                    ..
                },
                Arrived::Page { .. },
                Arrived::Counted { result: Err(_), .. },
                Arrived::Cancel {
                    answer: CancelAnswer::NotSent(NotSent::Paused),
                    ..
                },
            ]
        ),
        "{told:?}"
    );
}
