//! A wrong path is a file the auditor and the archiver never find, or two
//! writers on one file.

mod common;

use md_lake::paths::{Kind, L0Path};

#[test]
fn paths_build_and_parse_every_kind() {
    let cases = [
        "crypto/l0/_open/venue=bybit/date=2026-01-02/hour=14/md1-1767308398000000000-c3.qraw.zst.partial",
        "crypto/l0/_seal/venue=bybit/date=2026-01-02/hour=14.json",
        "crypto/l0/venue=bybit/market=linear/channel=orderbook.50/date=2026-01-02/BTCUSDT.qraw.zst",
        "crypto/l0/venue=bybit/market=linear/channel=orderbook.50/date=2026-01-02/BTCUSDT.qraw.zst.partial",
        "crypto/l0/venue=bybit/_conn/date=2026-01-02/md1-1767308398000000000-c3.qraw.zst",
        "crypto/l0/venue=bybit/market=linear/channel=instruments/date=2026-01-02/_all.qraw.zst.partial",
        "crypto/l0/venue=bybit/_session/date=2026-01-02/md_sealer-1767312300000000000.qraw.zst",
        "crypto/l0/_manifest/venue=bybit/date=2026-01-02.jsonl",
    ];
    let kinds = [
        Kind::Open,
        Kind::SealLog,
        Kind::DayBySymbol,
        Kind::DayBySymbol,
        Kind::DayByConnection,
        Kind::Instruments,
        Kind::Session,
        Kind::Manifest,
    ];
    for (path, kind) in cases.iter().zip(kinds) {
        let p = L0Path::parse(path).unwrap_or_else(|e| panic!("{path}: {e}"));
        assert_eq!(p.kind(), kind, "{path}");
        assert_eq!(p.to_path(), *path);
    }
    let day = L0Path::parse(cases[3]).expect("partial day");
    assert!(day.is_partial());
    assert_eq!(day.sealed().to_path(), cases[2]);
    assert_eq!(
        L0Path::parse(cases[2]).expect("day").l0_file_id(),
        Some("venue=bybit/market=linear/channel=orderbook.50/date=2026-01-02/BTCUSDT".to_string())
    );
    assert_eq!(
        md_lake::paths::sidecar(cases[2]),
        format!("{}.sha256", cases[2])
    );

    for bad in [
        "crypto/l0/_open/venue=bybit/date=2026-01-02/hour=14/md1-1-c4294967296.qraw.zst.partial",
        "crypto/l0/_open/venue=bybit/date=2026-01-02/hour=14/md1-9223372036854775808-c1.qraw.zst.partial",
        "crypto/l0/_open/venue=bybit/date=2026-01-02/hour=14/md-1-1-c1.qraw.zst.partial",
        "crypto/l0/_open/venue=bybit/date=2026-01-02/hour=24/md1-1-c1.qraw.zst.partial",
        "crypto/l0/_open/venue=bybit/date=2026-01-02/hour=14/md1-01-c1.qraw.zst.partial",
        "crypto/l0/_open/venue=bybit/date=2026-01-02/hour=14/md1-1-c1.qraw.zst",
        "crypto/l0/venue=bybit/market=linear/channel=orderbook.50/date=2026-02-30/BTCUSDT.qraw.zst",
        "crypto/l0/venue=bybit/market=linear/channel=orderbook.50/date=2026-01-02/_BTC.qraw.zst",
        "crypto/l0/venue=bybit/market=linear/channel=orderbook.50/date=2026-01-02/_all.qraw.zst",
        "crypto/l0/venue=bybit/market=linear/channel=a..b/date=2026-01-02/BTCUSDT.qraw.zst",
        "crypto/l0/venue=Bybit/market=linear/channel=orderbook.50/date=2026-01-02/BTCUSDT.qraw.zst",
        "crypto/l0/_seal/venue=bybit/date=2026-01-02/hour=14.json.partial",
        "../crypto/l0/_manifest/venue=bybit/date=2026-01-02.jsonl",
    ] {
        assert!(L0Path::parse(bad).is_err(), "{bad}");
    }
}

#[test]
fn golden_paths_parse() {
    for id in common::golden_file_ids() {
        // A golden l0_file_id is the path under crypto/l0 without the extension.
        let path = format!("crypto/l0/{id}.qraw.zst");
        let p = L0Path::parse(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
        assert_eq!(p.l0_file_id().as_deref(), Some(id.as_str()));
    }
}
