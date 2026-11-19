//! The collector under test: config with loopback fakes, waits, readers,
//! the md-lake seal as a child process.
#![allow(dead_code)]

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use md_collector::collector::{CollectorHandle, Deps, SystemClock};
use md_collector::config::Config;
use md_lake::fs::{Fs, RealFs};
use md_lake::qraw::Record;

use crate::common::*;
use crate::fake::{Behaviour, FakeBybit, FakeRest, Pages};
use crate::slowfs::SetFree;

pub fn pages(markets: &[(&str, &[&str])]) -> Pages {
    markets
        .iter()
        .map(|(m, syms)| {
            (
                (m.to_string(), String::new()),
                (200u16, instruments_body(m, syms, "")),
            )
        })
        .collect()
}

pub struct Setup {
    pub dir: std::path::PathBuf,
    pub fs: Arc<dyn Fs>,
    pub ws: FakeBybit,
    pub rest: FakeRest,
}

pub async fn setup(name: &str, b: Behaviour, p: Pages) -> Setup {
    let dir = test_dir(name);
    let fs = RealFs::new(&dir);
    Setup {
        dir,
        fs,
        ws: FakeBybit::start(b).await,
        rest: FakeRest::start(p).await,
    }
}

impl Setup {
    /// A config with loopback fakes; `markets` is the TOML of the
    /// `[[markets]]` tables without `ws_url`/`rest_base` (added here).
    pub fn config(&self, markets: &[(&str, &str)], rest: &str) -> Config {
        let mut t = format!(
            "venue = \"bybit\"\nlake_root = {:?}\nhost = \"test\"\nloopback_only = true\n{rest}\n",
            self.dir.to_string_lossy()
        );
        for (name, body) in markets {
            t.push_str(&format!(
                "\n[[markets]]\nname = \"{name}\"\nws_url = \"{}\"\nrest_base = \"{}\"\n{body}\n",
                self.ws.ws_url(name),
                self.rest.base
            ));
        }
        Config::from_toml(&t).unwrap_or_else(|e| panic!("{e}\n{t}"))
    }

    pub fn deps(&self) -> Deps {
        Deps {
            fs: self.fs.clone(),
            free_space: SetFree::new(u64::MAX),
            clock: Arc::new(SystemClock),
            notifier: None,
        }
    }
}

pub async fn eventually(what: &str, mut f: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(15);
    while !f() {
        assert!(Instant::now() < deadline, "not within 15 s: {what}");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

pub fn written(h: &CollectorHandle) -> u64 {
    h.state()
        .ingest
        .conns
        .values()
        .map(|c| c.frames_written)
        .sum()
}

/// Records of every file below `dir` whose path contains `part`.
pub fn records_under(fs: &dyn Fs, dir: &Path, part: &str) -> Vec<Record> {
    let mut files = files_with(dir, part);
    files.retain(|p| p.contains(".qraw.zst") && !p.ends_with(".sha256"));
    files.sort();
    files.iter().flat_map(|p| records(fs, p)).collect()
}

pub fn texts(rs: &[Record]) -> Vec<String> {
    rs.iter()
        .map(|r| String::from_utf8_lossy(&r.payload).to_string())
        .collect()
}

pub fn seal(dir: &Path, first_hour_ns: i64, now_ns: i64) -> (i32, String, String) {
    let first = format!(
        "{}T{:02}",
        md_lake::time::date(first_hour_ns),
        md_lake::time::hour(first_hour_ns)
    );
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_md-lake"))
        .args([
            "seal",
            "--venue",
            "bybit",
            "--lake",
            &dir.to_string_lossy(),
            "--first-hour",
            &first,
            "--now-ns",
            &now_ns.to_string(),
        ])
        .env("MD_LAKE_TEST_ALLOW_FUTURE_NOW", "1")
        .output()
        .expect("run md-lake");
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).to_string(),
        String::from_utf8_lossy(&out.stderr).to_string(),
    )
}
