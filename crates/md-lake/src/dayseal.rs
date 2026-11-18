//! The day seal (lake contract 2.7): close crashed collector session files
//! (step 0), rename every `.partial` file of a complete date, write its
//! sidecar and its manifest line. Idempotent: each run does only what is
//! missing and never changes a sealed file.

use std::collections::BTreeSet;
use std::io;

use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::fs::{Held, parent};
use crate::paths::{L0Path, sidecar};
use crate::qraw::{self, Header, Record, Src};
use crate::read::{RecordIter, scan_file};
use crate::seal::{Clock, SEAL_LEVEL, SealError, SealReport, Sealer, note_payload, read_whole};
use crate::time::{self, DAY_NS, HOUR_NS};
use crate::zst::{Contexts, FrameEncoder};

/// Dates further back than this are not looked at by a normal run
/// (judgement, not a model: a date normally seals within a day).
const LOOKBACK_DAYS: i64 = 7;

impl Sealer {
    /// The day seal alone for every date from `from_ns` on (2.7: it can run
    /// at any time), for a date that became sealable after the lookback of
    /// the normal run, e.g. once a person resolved a stopped hour.
    pub fn seal_days_since(
        &self,
        clock: &dyn Clock,
        from_ns: i64,
    ) -> Result<SealReport, SealError> {
        let _lock = self.seal_lock()?;
        let last_closed = time::hour_start(clock.now_ns()) - HOUR_NS;
        let mut report = SealReport::default();
        self.seal_days_from(clock, time::day_start(from_ns), last_closed, &mut report)?;
        Ok(report)
    }

    /// The day seal of a seal run: the last `LOOKBACK_DAYS` dates, and every
    /// date from the first hour this run sealed (`earliest`).
    pub(crate) fn seal_days(
        &self,
        clock: &dyn Clock,
        last_closed: i64,
        earliest: Option<i64>,
        report: &mut SealReport,
    ) -> Result<(), SealError> {
        let window = time::day_start(last_closed) - LOOKBACK_DAYS * DAY_NS;
        let from = earliest.map_or(window, |e| time::day_start(e).min(window));
        self.seal_days_from(clock, from, last_closed, report)
    }

    fn seal_days_from(
        &self,
        clock: &dyn Clock,
        from_day: i64,
        last_closed: i64,
        report: &mut SealReport,
    ) -> Result<(), SealError> {
        let mut d = from_day.max(time::day_start(self.s.first_hour_ns));
        while d + 23 * HOUR_NS <= last_closed {
            if self.seal_day(d, clock)? {
                report.days_sealed.push(time::date(d));
            }
            d += DAY_NS;
        }
        Ok(())
    }

    /// Every lake file of the date (day, instruments, connection and session
    /// files, sealed or `.partial`).
    fn files_of_date(&self, date: &str) -> io::Result<Vec<String>> {
        let venue = format!("crypto/l0/venue={}", self.s.venue);
        let mut dirs = vec![
            format!("{venue}/_conn/date={date}"),
            format!("{venue}/_session/date={date}"),
        ];
        for m in self.fs.list(&venue)? {
            if !m.starts_with("market=") {
                continue;
            }
            for c in self.fs.list(&format!("{venue}/{m}"))? {
                if c.starts_with("channel=") {
                    dirs.push(format!("{venue}/{m}/{c}/date={date}"));
                }
            }
        }
        let mut out = Vec::new();
        for dir in dirs {
            for name in self.fs.list(&dir)? {
                if name.ends_with(".qraw.zst") || name.ends_with(".qraw.zst.partial") {
                    out.push(format!("{dir}/{name}"));
                }
            }
        }
        out.sort();
        Ok(out)
    }

    /// `true` when this call changed something and the date is now day-sealed.
    fn seal_day(&self, d: i64, clock: &dyn Clock) -> Result<bool, SealError> {
        let fs = self.fs.as_ref();
        let date = time::date(d);
        // Condition 1: no open file of the date in the hours in scope (an
        // hour before first_hour is never sealed, F7).
        let open = format!("crypto/l0/_open/venue={}/date={date}", self.s.venue);
        for hour in fs.list(&open)? {
            let before = hour
                .strip_prefix("hour=")
                .and_then(|v| v.parse::<i64>().ok())
                .is_some_and(|hh| d + hh * HOUR_NS < self.s.first_hour_ns);
            if !before && !fs.list(&format!("{open}/{hour}"))?.is_empty() {
                return Ok(false);
            }
        }
        // Condition 2: a done entry for every hour in scope.
        let mut h = d.max(self.s.first_hour_ns);
        while h < d + DAY_NS {
            let log = self.read_log(h)?;
            if log.last().and_then(|e| e["entry"].as_str()) != Some("done") {
                return Ok(false);
            }
            h += HOUR_NS;
        }
        // Condition 3: every collector session file of the date can be locked.
        let files = self.files_of_date(&date)?;
        let mut held: Vec<Held> = Vec::new();
        for p in &files {
            if let Ok(L0Path::Session {
                writer,
                partial: true,
                ..
            }) = L0Path::parse(p)
            {
                if writer != self.s.sealer {
                    let mut f = fs.open(p)?;
                    if !f.try_lock()? {
                        return Ok(false);
                    }
                    held.push(Held(f));
                }
            }
        }
        let mut changed = false;
        // Step 0: replace a crashed collector's session file.
        for p in &files {
            if let Ok(L0Path::Session {
                writer,
                partial: true,
                start_ns,
                ..
            }) = L0Path::parse(p)
            {
                if writer != self.s.sealer {
                    changed |= self.close_session_file(p, &writer, start_ns, &date, clock)?;
                }
            }
        }
        // Steps 1-3 for every file, doing only what is missing.
        let manifest = L0Path::Manifest {
            venue: self.s.venue.clone(),
            date: date.clone(),
        }
        .to_path();
        let mut have = self.read_manifest(&manifest)?;
        for p in self.files_of_date(&date)? {
            let lp = L0Path::parse(&p).map_err(io::Error::other)?;
            let sealed = if lp.is_partial() {
                let to = lp.sealed().to_path();
                let mut f = fs.open(&p)?;
                f.sync()?;
                drop(f);
                fs.rename(&p, &to)?;
                fs.sync_dir(parent(&to))?;
                changed = true;
                to
            } else {
                p.clone()
            };
            let side = sidecar(&sealed);
            let id = lp
                .l0_file_id()
                .ok_or_else(|| io::Error::other("no l0_file_id"))?;
            let need_side = !fs.exists(&side);
            let need_line = !have.contains(&id);
            if !need_side && !need_line {
                continue;
            }
            let bytes = read_whole(fs, &sealed)?;
            let sha = hex::encode(Sha256::digest(&bytes));
            if need_side {
                let name = sealed.rsplit('/').next().unwrap_or_default();
                let tmp = format!("{side}.tmp");
                if fs.exists(&tmp) {
                    fs.remove(&tmp)?;
                }
                let mut f = fs.create_new(&tmp)?;
                f.write_all(format!("{sha}  {name}\n").as_bytes())?;
                f.sync()?;
                drop(f);
                fs.rename(&tmp, &side)?;
                fs.sync_dir(parent(&side))?;
                changed = true;
            }
            if need_line {
                let line =
                    self.manifest_line(&lp, &sealed, &id, &sha, bytes.len() as u64, d, clock)?;
                self.append_line(&manifest, &line)?;
                have.insert(id);
                changed = true;
            }
        }
        drop(held);
        Ok(changed)
    }

    /// Step 0 (2.7) for one collector session file; `true` when it was replaced.
    fn close_session_file(
        &self,
        p: &str,
        writer: &str,
        start_ns: i64,
        date: &str,
        clock: &dyn Clock,
    ) -> io::Result<bool> {
        let fs = self.fs.as_ref();
        let tmp = format!("{p}.tmp");
        if fs.exists(&tmp) {
            fs.remove(&tmp)?;
            fs.sync_dir(parent(p))?;
        }
        let mut f = fs.open(p)?;
        let scan = scan_file(f.as_mut())?;
        if scan.unfinished.is_none() && !scan.frames.is_empty() {
            return Ok(false);
        }
        let end = scan.unfinished.map_or(0, |u| u.complete_blocks_end as u64);
        let mut records: Vec<Record> = Vec::new();
        let mut header: Option<Header> = None;
        {
            let mut it = RecordIter::new(f.as_mut(), 0, end, true);
            for r in it.by_ref() {
                records.push(r?);
            }
            header = header.or_else(|| it.header().cloned());
        }
        let header = header.unwrap_or_else(|| {
            Header::session(&self.s.venue, date, writer, "unknown", "unknown", start_ns)
        });
        let id = L0Path::parse(p)
            .ok()
            .and_then(|x| x.l0_file_id())
            .unwrap_or_default();
        let start = records.last().map_or(start_ns, |r| r.recv_ns);
        let note = Record {
            recv_ns: clock.now_ns(),
            src: Src::Note,
            conn_id: 0,
            frame_no: 0,
            payload: note_payload(
                "crash_truncate",
                &self.s.sealer,
                None,
                start,
                None,
                &json!({"file": id}),
            ),
        };
        records.push(note);
        let mut content = qraw::encode_header(&header);
        for r in &records {
            qraw::encode_record(r, &mut content).map_err(|e| io::Error::other(format!("{e:?}")))?;
        }
        let contexts = Contexts::default();
        let mut enc = FrameEncoder::new(SEAL_LEVEL, Some(content.len() as u64), &contexts)?;
        let mut out = Vec::new();
        enc.write(&content, &mut out)?;
        enc.finish(&mut out)?;
        let mut t = fs.create_new(&tmp)?;
        t.write_all(&out)?;
        t.sync()?;
        drop(t);
        fs.rename(&tmp, p)?;
        fs.sync_dir(parent(p))?;
        Ok(true)
    }

    /// The `l0_file_id`s of the manifest; a torn last line is cut first.
    fn read_manifest(&self, path: &str) -> io::Result<BTreeSet<String>> {
        let fs = self.fs.as_ref();
        let mut ids = BTreeSet::new();
        if !fs.exists(path) {
            return Ok(ids);
        }
        let bytes = read_whole(fs, path)?;
        let mut good = 0;
        for line in bytes.split_inclusive(|&b| b == b'\n') {
            if !line.ends_with(b"\n") {
                break;
            }
            match serde_json::from_slice::<Value>(&line[..line.len() - 1]) {
                Ok(v) => {
                    if let Some(id) = v["l0_file_id"].as_str() {
                        ids.insert(id.to_string());
                    }
                    good += line.len();
                }
                Err(_) => break,
            }
        }
        if good < bytes.len() {
            let mut f = fs.open(path)?;
            f.set_len(good as u64)?;
            f.sync()?;
        }
        Ok(ids)
    }

    fn append_line(&self, path: &str, v: &Value) -> io::Result<()> {
        let fs = self.fs.as_ref();
        let (mut f, created) = if fs.exists(path) {
            (fs.open(path)?, false)
        } else {
            (fs.create_new(path)?, true)
        };
        let mut line = v.to_string().into_bytes();
        line.push(b'\n');
        f.write_all(&line)?;
        f.sync()?;
        if created {
            fs.sync_dir(parent(path))?;
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn manifest_line(
        &self,
        lp: &L0Path,
        sealed: &str,
        id: &str,
        sha: &str,
        bytes: u64,
        d: i64,
        clock: &dyn Clock,
    ) -> io::Result<Value> {
        let fs = self.fs.as_ref();
        let mut f = fs.open(sealed)?;
        let len = f.len()?;
        let mut n = 0u64;
        let (mut first, mut last) = (None, None);
        for r in RecordIter::new(f.as_mut(), 0, len, true) {
            let r = r?;
            if first.is_none() {
                first = Some(r.recv_ns);
            }
            last = Some(r.recv_ns);
            n += 1;
        }
        let layout = match lp {
            L0Path::DayBySymbol { .. } | L0Path::Instruments { .. } => "by_symbol",
            L0Path::DayByConnection { .. } => "by_connection",
            _ => "session",
        };
        let sessions = if layout == "by_symbol" {
            self.sessions_of(&lp.partial().to_path(), d)?
        } else {
            Value::Null
        };
        Ok(json!({
            "l0_file_id": id,
            "path": sealed,
            "sha256": sha,
            "bytes": bytes,
            "records": n,
            "first_recv_ns": first,
            "last_recv_ns": last,
            "sealed_at": clock.now_ns(),
            "contract_ver": qraw::CONTRACT_VER,
            "layout": layout,
            "sessions": sessions,
        }))
    }

    /// The session runs of a by_symbol file from the done entries of the
    /// date's hours, in order, neighbouring runs of one session joined.
    fn sessions_of(&self, partial_path: &str, d: i64) -> io::Result<Value> {
        let mut runs: Vec<(String, i64, u64, u64)> = Vec::new();
        let mut rec_no = 0u64;
        for k in 0..24 {
            let log = self.read_log(d + k * HOUR_NS)?;
            let Some(done) = log.last().filter(|e| e["entry"] == "done") else {
                continue;
            };
            for t in done["targets"].as_array().into_iter().flatten() {
                if t["path"].as_str() != Some(partial_path) {
                    continue;
                }
                for r in t["sessions"].as_array().into_iter().flatten() {
                    let (Some(c), Some(ns), Some(n)) = (
                        r["collector"].as_str(),
                        r["session_ns"].as_i64(),
                        r["records"].as_u64(),
                    ) else {
                        continue;
                    };
                    match runs.last_mut() {
                        Some((lc, lns, _, ln)) if lc == c && *lns == ns => *ln += n,
                        _ => runs.push((c.to_string(), ns, rec_no, n)),
                    }
                    rec_no += n;
                }
            }
        }
        Ok(Value::Array(
            runs.into_iter()
                .map(|(c, ns, first, n)| json!({"collector": c, "session_ns": ns, "first_rec_no": first, "records": n}))
                .collect(),
        ))
    }
}
