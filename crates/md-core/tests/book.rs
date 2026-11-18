//! The order book is what the strategy trades on and what the archiver's
//! checkpoint rows replay: a wrong level is a wrong price.

use md_core::{BookSide, BookUpdate, Level, OrderBook, StreamKey};

fn lv(px: i64, qty: i64) -> Level {
    Level { px, qty }
}

fn update(bids: Vec<Level>, asks: Vec<Level>) -> BookUpdate {
    BookUpdate {
        key: StreamKey::new("orderbook.50", "BTCUSDT"),
        venue_ts_ns: 1,
        recv_ns: 2,
        update_id: Some(1),
        prev_update_id: None,
        first_update_id: None,
        venue_seq: None,
        bids,
        asks,
        px_exp: -1,
        qty_exp: -3,
    }
}

fn snapshot_book() -> OrderBook {
    let mut book = OrderBook::new(None);
    book.apply_snapshot(&update(
        vec![lv(420000, 1000), lv(419999, 2000), lv(419998, 4000)],
        vec![lv(420002, 3000), lv(420001, 1500), lv(420003, 500)],
    ));
    book
}

#[test]
fn snapshot_then_deltas_give_the_hand_computed_book() {
    let mut book = snapshot_book();
    book.apply_delta(&update(vec![lv(420000, 1200)], vec![lv(420001, 0)]));
    book.apply_delta(&update(vec![lv(419997, 700)], vec![lv(420004, 100)]));
    assert_eq!(
        book.levels(BookSide::Bid),
        vec![
            lv(420000, 1200),
            lv(419999, 2000),
            lv(419998, 4000),
            lv(419997, 700)
        ]
    );
    assert_eq!(
        book.levels(BookSide::Ask),
        vec![lv(420002, 3000), lv(420003, 500), lv(420004, 100)]
    );
}

#[test]
fn zero_quantity_deletes_a_level_and_an_unknown_zero_is_ignored() {
    let mut book = snapshot_book();
    book.apply_delta(&update(vec![lv(419999, 0), lv(1, 0)], vec![]));
    assert_eq!(
        book.levels(BookSide::Bid),
        vec![lv(420000, 1000), lv(419998, 4000)]
    );
    let mut from_snapshot = OrderBook::new(None);
    from_snapshot.apply_snapshot(&update(vec![lv(5, 0), lv(4, 1)], vec![]));
    assert_eq!(
        from_snapshot.levels(BookSide::Bid),
        vec![lv(4, 1)],
        "a zero level in a snapshot is not a level"
    );
}

#[test]
fn max_depth_keeps_the_best_levels_only() {
    let mut book = OrderBook::new(Some(2));
    book.apply_snapshot(&update(
        vec![lv(10, 1), lv(9, 1), lv(8, 1), lv(7, 1)],
        vec![lv(11, 1), lv(12, 1), lv(13, 1), lv(14, 1)],
    ));
    assert_eq!(book.levels(BookSide::Bid), vec![lv(10, 1), lv(9, 1)]);
    assert_eq!(book.levels(BookSide::Ask), vec![lv(11, 1), lv(12, 1)]);
    book.apply_delta(&update(vec![lv(10, 5), lv(11, 2)], vec![lv(10, 3)]));
    assert_eq!(book.levels(BookSide::Bid), vec![lv(11, 2), lv(10, 5)]);
    assert_eq!(book.levels(BookSide::Ask), vec![lv(10, 3), lv(11, 1)]);
    assert!(book.is_crossed());
}

#[test]
fn best_spread_mid_and_depth_sums_match_literals() {
    let mut book = OrderBook::new(None);
    assert_eq!(
        (
            book.best_bid(),
            book.best_ask(),
            book.mid_x2(),
            book.spread_bp()
        ),
        (None, None, None, None)
    );
    book.apply_snapshot(&update(
        vec![lv(420000, 1000), lv(419999, 2000)],
        vec![lv(420002, 3000)],
    ));
    assert_eq!(book.best_bid(), Some(lv(420000, 1000)));
    assert_eq!(book.best_ask(), Some(lv(420002, 3000)));
    assert_eq!(book.mid_x2(), Some(840002));
    assert_eq!(book.mid(), Some(420001.0));
    let bp = book.spread_bp().expect("both sides have a level");
    assert!((bp - 2.0 / 420001.0 * 10_000.0).abs() < 1e-12, "{bp}");
    assert_eq!(book.depth_sum(BookSide::Bid, 2), 3000);
    assert_eq!(book.depth_sum(BookSide::Bid, 10), 3000);
    assert_eq!(book.depth_sum(BookSide::Ask, 1), 3000);
    assert_eq!(book.depth_sum(BookSide::Ask, 0), 0);
    assert!(!book.is_crossed());
}

#[test]
fn a_new_snapshot_replaces_the_whole_book() {
    let mut book = snapshot_book();
    book.apply_snapshot(&update(vec![lv(1, 1)], vec![]));
    assert_eq!(book.levels(BookSide::Bid), vec![lv(1, 1)]);
    assert_eq!(book.levels(BookSide::Ask), vec![]);
    assert_eq!(book.best_ask(), None);
}
