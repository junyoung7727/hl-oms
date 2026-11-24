//! The watcher's kill notices (C-WATCH part 11, U19b2a): the texts of the
//! start, progress, final and abort notices, the position block every
//! notice carries, and the position reader against a loopback fake
//! (`http-fake`) on a manual clock.

#![allow(clippy::unwrap_used)]

use std::sync::{Arc, Mutex};

use http_fake::{Answer, FakeHttp};
use serde_json::{Value, json};
use watcher::alert::{MSG_LIMIT, message, split};
use watcher::clock::{Clock, ManualClock};
use watcher::kill::{
    AbortWhy, LATCHED_LINE, LastError, NOT_READ_ON_ABORT, NotSent, Notice, PAUSED_LINE, POSITIONS,
    Positions, StopOutcome, TIMED_OUT, notice_text, position_block, read_positions,
};
use watcher::metrics::Metrics;
use watcher::rest::{Client, ClientConfig, Credentials};

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
