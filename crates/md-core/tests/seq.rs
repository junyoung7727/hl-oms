//! A missed delta makes every later book wrong until the next snapshot; the gap
//! detector is what tells the collector to resync and the archiver to stop
//! writing checkpoint rows (lake contract 3.2).

use md_core::{GapDetector, GapEvent, SeqRule};

fn feed(rule: SeqRule, msgs: &[(bool, Option<i64>, Option<i64>, i64)]) -> Vec<GapEvent> {
    let mut det = GapDetector::new(rule);
    msgs.iter()
        .filter_map(|&(snapshot, u, prev, recv)| if snapshot { det.snapshot(u, recv) } else { det.delta(u, prev, recv) })
        .collect()
}

#[test]
fn plus_one_reports_a_missing_update_id_once() {
    let gaps = feed(SeqRule::PlusOne, &[
        (true, Some(100), None, 1),
        (false, Some(101), None, 2),
        (false, Some(103), None, 3),
        (false, Some(104), None, 4),
        (true, Some(1), None, 5),
        (false, Some(2), None, 6),
    ]);
    assert_eq!(gaps, vec![GapEvent { last_good: Some(101), next: Some(103), recv_ns: 3 }]);
}

#[test]
fn prev_equals_reports_a_wrong_prev() {
    let gaps = feed(SeqRule::PrevEquals, &[
        (true, Some(500), None, 1),
        (false, Some(510), Some(500), 2),
        (false, Some(530), Some(520), 3),
        (false, Some(540), Some(530), 4),
    ]);
    assert_eq!(gaps, vec![GapEvent { last_good: Some(510), next: Some(530), recv_ns: 3 }]);
}

#[test]
fn none_rule_never_reports_a_gap() {
    let gaps = feed(SeqRule::None, &[(false, None, None, 1), (true, None, None, 2), (false, Some(9), Some(1), 3)]);
    assert_eq!(gaps, vec![]);
}

#[test]
fn delta_before_any_snapshot_is_a_gap() {
    let gaps = feed(SeqRule::PlusOne, &[(false, Some(7), None, 1), (false, Some(8), None, 2), (true, Some(8), None, 3), (false, Some(9), None, 4)]);
    assert_eq!(gaps, vec![GapEvent { last_good: None, next: Some(7), recv_ns: 1 }]);
}

#[test]
fn a_delta_without_its_sequence_field_breaks_the_run() {
    let gaps = feed(SeqRule::PlusOne, &[(true, Some(1), None, 1), (false, None, None, 2)]);
    assert_eq!(gaps, vec![GapEvent { last_good: Some(1), next: None, recv_ns: 2 }]);
}
