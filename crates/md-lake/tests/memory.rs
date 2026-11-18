//! Memory of the seal (U4 done criteria): it must not grow with the hour's
//! size, so that a 2 GB hour seals next to the OMS on this PC. Peak RSS is
//! measured in a child process (VmHWM of /proc/self/status, the same value
//! as getrusage's ru_maxrss; getrusage would need unsafe code, which the
//! workspace forbids).

mod common;

use std::path::Path;
use std::process::Command;
use std::sync::Arc;

use common::kit::{FixedClock, S, T14, classify};
use md_lake::fs::{Fs, RealFs};
use md_lake::qraw::{Record, Src};
use md_lake::seal::{SealSettings, Sealer};
use md_lake::time;
use md_lake::writer::{OpenWriter, WriterSettings};
use md_lake::zst::{self, Contexts};

const ENV_DIR: &str = "MD_LAKE_RSS_DIR";
const ENV_BUDGET: &str = "MD_LAKE_RSS_BUDGET";

/// Writes an hour of `topics` topics spread over connections of
/// `per_conn` topics each, about `total` bytes of records.
fn synthetic_hour(fs: &Arc<dyn Fs>, topics: usize, per_conn: usize, total: usize) {
    let payload_len = 480;
    let n = total / (payload_len + 25);
    let conns = topics.div_ceil(per_conn);
    let mut writers: Vec<OpenWriter> = (0..conns)
        .map(|c| {
            OpenWriter::new(
                fs.clone(),
                WriterSettings::new("bybit", "md1", "0.1.0", "t", T14 - S),
                c as u32 + 1,
                &Contexts::default(),
            )
        })
        .collect();
    let mut x: u64 = 0x9E37_79B9_7F4A_7C15;
    for i in 0..n {
        let t = i % topics;
        // Digits from a xorshift: compressible like prices, not constant.
        let mut digits = String::with_capacity(payload_len);
        while digits.len() < payload_len - 60 {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            digits.push_str(&format!("{},", x % 100_000));
        }
        let payload = format!("{{\"topic\":\"orderbook.50.S{t:04}\",\"d\":\"{digits}\"}}");
        let recv = T14 + (i as i64) * (3_000 * S / n as i64);
        let conn = t / per_conn;
        writers[conn]
            .append(&Record {
                recv_ns: recv,
                src: Src::WebSocket,
                conn_id: conn as u32 + 1,
                frame_no: i as u64,
                payload: payload.into_bytes(),
            })
            .expect("append");
    }
    for w in writers {
        w.close().expect("close");
    }
}

/// Seals the lake at `dir` in a child process; returns (peak RSS, max live
/// context bytes), both in bytes.
fn seal_in_child(dir: &Path, budget: usize) -> (u64, u64) {
    let out = Command::new(std::env::current_exe().expect("exe"))
        .args([
            "--exact",
            "seal_child",
            "--ignored",
            "--nocapture",
            "--test-threads=1",
        ])
        .env(ENV_DIR, dir)
        .env(ENV_BUDGET, budget.to_string())
        .output()
        .expect("child");
    let text = String::from_utf8_lossy(&out.stdout);
    let line = text
        .lines()
        .find_map(|l| l.split("MD_LAKE_RSS ").nth(1))
        .unwrap_or_else(|| panic!("child: {text}\n{}", String::from_utf8_lossy(&out.stderr)));
    let mut it = line
        .split_whitespace()
        .map(|v| v.parse::<u64>().expect("number"));
    (it.next().expect("rss"), it.next().expect("live"))
}

#[test]
#[ignore]
fn seal_child() {
    let (Ok(dir), Ok(budget)) = (std::env::var(ENV_DIR), std::env::var(ENV_BUDGET)) else {
        return;
    };
    let fs = RealFs::new(dir);
    let mut s = SealSettings::new("bybit", "md_sealer", "0.1.0", "t", T14);
    s.context_budget = budget.parse().expect("budget");
    let clock = FixedClock::at(T14 + time::HOUR_NS + 60 * S);
    let report = Sealer::new(fs, s)
        .seal(clock.as_ref(), &classify)
        .expect("seal");
    assert_eq!(report.sealed.len(), 1);
    let status = std::fs::read_to_string("/proc/self/status").expect("status");
    let hwm_kb: u64 = status
        .lines()
        .find_map(|l| l.strip_prefix("VmHWM:"))
        .and_then(|v| v.split_whitespace().next())
        .and_then(|v| v.parse().ok())
        .expect("VmHWM");
    println!(
        "\nMD_LAKE_RSS {} {}",
        hwm_kb * 1024,
        report.max_live_context_bytes
    );
}

const MB: usize = 1024 * 1024;

#[test]
fn seal_rss_does_not_grow_with_input() {
    let mut peaks = Vec::new();
    for total in [50 * MB, 200 * MB] {
        let dir = common::test_dir(&format!("rss-{}", total / MB));
        let fs = RealFs::new(&dir);
        synthetic_hour(&fs, 200, 4, total);
        let topic = (total / 200) as u64;
        let budget = 4 * zst::context_bytes(9, Some(topic + 1024));
        let (rss, live) = seal_in_child(&dir, budget);
        assert!(live as usize <= budget, "live {live} > budget {budget}");
        eprintln!(
            "rss {} MB: peak {} MB, live contexts {} MB",
            total / MB,
            rss / MB as u64,
            live / MB as u64
        );
        peaks.push(rss);
        let _ = std::fs::remove_dir_all(&dir);
    }
    let diff = peaks[1].abs_diff(peaks[0]);
    assert!(
        diff <= 64 * MB as u64,
        "peak RSS {} MB vs {} MB",
        peaks[0] / MB as u64,
        peaks[1] / MB as u64
    );
}

#[test]
fn seal_rss_minus_contexts_is_bounded() {
    let dir = common::test_dir("rss-big-topics");
    let fs = RealFs::new(&dir);
    synthetic_hour(&fs, 4, 4, 400 * MB);
    let (rss, live) = seal_in_child(&dir, 512 * MB);
    let _ = std::fs::remove_dir_all(&dir);
    assert!(
        rss.saturating_sub(live) <= 96 * MB as u64,
        "RSS {} MB, contexts {} MB",
        rss / MB as u64,
        live / MB as u64
    );
}

#[test]
#[ignore]
fn seal_rss_2gb() {
    let dir = common::test_dir("rss-2gb");
    let fs = RealFs::new(&dir);
    let started = std::time::Instant::now();
    synthetic_hour(&fs, 200, 4, 2048 * MB);
    let written = started.elapsed();
    let sealed_at = std::time::Instant::now();
    let (rss, live) = seal_in_child(&dir, 512 * MB);
    let secs = sealed_at.elapsed().as_secs_f64();
    let _ = std::fs::remove_dir_all(&dir);
    println!(
        "seal_rss_2gb: peak RSS {} MB, live contexts {} MB, seal {secs:.1} s (writing input {:.1} s)",
        rss / MB as u64,
        live / MB as u64,
        written.as_secs_f64()
    );
    assert!(rss <= 1024 * MB as u64, "peak RSS {} MB", rss / MB as u64);
}
