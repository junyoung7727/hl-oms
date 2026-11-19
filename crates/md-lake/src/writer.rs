//! Stage 1 (lake contract 2.4): the collector's open files and session file.
//!
//! Lock rule: a file is created under `<name>.tmp`, locked (`LOCK_EX`), then
//! linked to its name (never replacing a file) and the `.tmp` name removed,
//! so no file with an open-file name is ever unlocked while its writer may
//! still write. The lock is released only after the frame end and the last
//! fsync. Block flush and fsync run on the clock given to `tick`.

use std::io;
use std::sync::Arc;

use crate::fs::{Fs, LakeFile, parent};
use crate::paths::L0Path;
use crate::qraw::{self, Header, Record};
use crate::time;
use crate::zst::{Contexts, FrameEncoder};

/// Level of the collector's streams (2.1).
pub const OPEN_LEVEL: i32 = 3;

#[derive(Debug, Clone)]
pub struct WriterSettings {
    pub venue: String,
    pub collector: String,
    pub collector_ver: String,
    pub host: String,
    /// Start of the collector process, ns (its session number).
    pub session_ns: i64,
    /// Block flush interval (default 1 s).
    pub flush_ns: i64,
    /// fsync interval (default 5 s).
    pub fsync_ns: i64,
}

impl WriterSettings {
    pub fn new(
        venue: &str,
        collector: &str,
        collector_ver: &str,
        host: &str,
        session_ns: i64,
    ) -> Self {
        Self {
            venue: venue.into(),
            collector: collector.into(),
            collector_ver: collector_ver.into(),
            host: host.into(),
            session_ns,
            flush_ns: 1_000_000_000,
            fsync_ns: 5_000_000_000,
        }
    }
}

/// One zstd stream into one locked file.
#[derive(Debug)]
struct Stream {
    path: String,
    file: Box<dyn LakeFile>,
    enc: FrameEncoder,
    buf: Vec<u8>,
    last_flush: i64,
    last_fsync: i64,
}

impl std::fmt::Debug for FrameEncoder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("FrameEncoder")
    }
}

impl Stream {
    /// Creates `path` under the lock rule and starts its stream with `header`.
    fn create(
        fs: &dyn Fs,
        path: String,
        header: &Header,
        now: i64,
        contexts: &Contexts,
    ) -> io::Result<Stream> {
        let tmp = format!("{path}.tmp");
        let mut file = fs.create_new(&tmp)?;
        if !file.try_lock()? {
            return Err(io::Error::other(format!(
                "{tmp} is locked by another writer"
            )));
        }
        if let Err(e) = fs.link(&tmp, &path) {
            let _ = fs.remove(&tmp);
            return Err(e);
        }
        fs.remove(&tmp)?;
        fs.sync_dir(parent(&path))?;
        let mut enc = FrameEncoder::new(OPEN_LEVEL, None, contexts)?;
        let mut buf = Vec::new();
        enc.write(&qraw::encode_header(header), &mut buf)?;
        let mut s = Stream {
            path,
            file,
            enc,
            buf,
            last_flush: now,
            last_fsync: now,
        };
        s.drain()?;
        Ok(s)
    }

    fn drain(&mut self) -> io::Result<()> {
        if !self.buf.is_empty() {
            self.file.write_all(&self.buf)?;
            self.buf.clear();
        }
        Ok(())
    }

    fn append(&mut self, r: &Record) -> io::Result<()> {
        let mut bytes = Vec::with_capacity(r.encoded_len());
        qraw::encode_record(r, &mut bytes).map_err(|e| io::Error::other(format!("{e:?}")))?;
        self.enc.write(&bytes, &mut self.buf)?;
        self.drain()
    }

    fn tick(&mut self, now: i64, flush_ns: i64, fsync_ns: i64) -> io::Result<()> {
        if now - self.last_flush >= flush_ns {
            self.enc.flush(&mut self.buf)?;
            self.drain()?;
            self.last_flush = now;
        }
        if now - self.last_fsync >= fsync_ns {
            self.file.sync()?;
            self.last_fsync = now;
        }
        Ok(())
    }

    /// Frame end, fsync, then unlock (in this order: 2.4).
    fn close(mut self) -> io::Result<()> {
        self.enc.finish(&mut self.buf)?;
        self.drain()?;
        self.file.sync()?;
        self.file.unlock()
    }
}

/// The open files of one collector connection: one per UTC hour.
#[derive(Debug)]
pub struct OpenWriter {
    fs: Arc<dyn Fs>,
    settings: WriterSettings,
    conn_id: u32,
    contexts: Contexts,
    /// Hour start and stream of the current open file.
    cur: Option<(i64, Stream)>,
    /// The earliest hour a new file may have: the hour after the last
    /// closed one (an hour is never reopened, it may be sealed already).
    next_hour: i64,
}

impl OpenWriter {
    pub fn new(
        fs: Arc<dyn Fs>,
        settings: WriterSettings,
        conn_id: u32,
        contexts: &Contexts,
    ) -> Self {
        Self {
            fs,
            settings,
            conn_id,
            contexts: contexts.clone(),
            cur: None,
            next_hour: i64::MIN,
        }
    }

    fn open(&mut self, hour_start: i64, now: i64) -> io::Result<()> {
        let s = &self.settings;
        let date = time::date(hour_start);
        let path = L0Path::Open {
            venue: s.venue.clone(),
            date: date.clone(),
            hour: time::hour(hour_start),
            collector: s.collector.clone(),
            session_ns: s.session_ns,
            conn_id: self.conn_id,
        }
        .to_path();
        let header = Header::connection(
            &s.venue,
            &date,
            &s.collector,
            &s.collector_ver,
            &s.host,
            s.session_ns,
            self.conn_id,
        );
        let stream = Stream::create(self.fs.as_ref(), path, &header, now, &self.contexts)?;
        self.cur = Some((hour_start, stream));
        Ok(())
    }

    /// Writes a record to the open file of its hour. An hour is never
    /// reopened: a record whose hour is earlier than the current file's
    /// (the clock stepped back) goes to the current file, and one whose
    /// hour was closed already goes to the next hour's file.
    pub fn append(&mut self, r: &Record) -> io::Result<()> {
        let hour = time::hour_start(r.recv_ns).max(self.next_hour);
        match &self.cur {
            Some((h, _)) if hour > *h => {
                self.close_current()?;
                self.open(hour, r.recv_ns)?;
            }
            Some(_) => {}
            None => self.open(hour, r.recv_ns)?,
        }
        match &mut self.cur {
            Some((_, s)) => s.append(r),
            None => Err(io::Error::other("no open file")),
        }
    }

    /// No file of an hour before `now_ns`'s hour is opened from now on (the
    /// floor only moves forward). The collector passes its wall clock, so
    /// that a record that waited in its queue past its hour goes to the
    /// current hour's file, never into an hour the sealer may have sealed.
    pub fn set_min_hour(&mut self, now_ns: i64) {
        self.next_hour = self.next_hour.max(time::hour_start(now_ns));
    }

    /// Flush and fsync on time; closes the file when its hour has passed.
    pub fn tick(&mut self, now: i64) -> io::Result<()> {
        let (flush, fsync) = (self.settings.flush_ns, self.settings.fsync_ns);
        if let Some((h, s)) = &mut self.cur {
            if now >= *h + time::HOUR_NS {
                return self.close_current();
            }
            s.tick(now, flush, fsync)?;
        }
        Ok(())
    }

    /// The path of the current open file.
    pub fn current_path(&self) -> Option<&str> {
        self.cur.as_ref().map(|(_, s)| s.path.as_str())
    }

    fn close_current(&mut self) -> io::Result<()> {
        match self.cur.take() {
            Some((h, s)) => {
                self.next_hour = h + time::HOUR_NS;
                s.close()
            }
            None => Ok(()),
        }
    }

    /// Ends the stream (frame end, fsync, unlock) at shutdown.
    pub fn close(mut self) -> io::Result<()> {
        self.close_current()
    }
}

/// The collector's session file: one zstd stream per UTC date (2.4).
#[derive(Debug)]
pub struct SessionWriter {
    fs: Arc<dyn Fs>,
    settings: WriterSettings,
    contexts: Contexts,
    cur: Option<(i64, Stream)>,
}

impl SessionWriter {
    pub fn new(fs: Arc<dyn Fs>, settings: WriterSettings, contexts: &Contexts) -> Self {
        Self {
            fs,
            settings,
            contexts: contexts.clone(),
            cur: None,
        }
    }

    fn open(&mut self, day: i64, now: i64) -> io::Result<()> {
        let s = &self.settings;
        let date = time::date(day);
        let path = L0Path::Session {
            venue: s.venue.clone(),
            date: date.clone(),
            writer: s.collector.clone(),
            start_ns: s.session_ns,
            partial: true,
        }
        .to_path();
        let header = Header::session(
            &s.venue,
            &date,
            &s.collector,
            &s.collector_ver,
            &s.host,
            s.session_ns,
        );
        let stream = Stream::create(self.fs.as_ref(), path, &header, now, &self.contexts)?;
        self.cur = Some((day, stream));
        Ok(())
    }

    fn roll_to(&mut self, day: i64, now: i64) -> io::Result<()> {
        if let Some((_, s)) = self.cur.take() {
            s.close()?;
        }
        self.open(day, now)
    }

    /// A record goes to the file of its date; when that date is already
    /// closed (the clock went back across midnight), to the newest date.
    pub fn append(&mut self, r: &Record) -> io::Result<()> {
        let day = time::day_start(r.recv_ns);
        match &self.cur {
            Some((d, _)) if day > *d => self.roll_to(day, r.recv_ns)?,
            Some(_) => {}
            None => self.open(day, r.recv_ns)?,
        }
        match &mut self.cur {
            Some((_, s)) => s.append(r),
            None => Err(io::Error::other("no session file")),
        }
    }

    /// Flush and fsync on time; at UTC midnight starts the next date's file
    /// and closes the old one, also when no record arrives.
    pub fn tick(&mut self, now: i64) -> io::Result<()> {
        let (flush, fsync) = (self.settings.flush_ns, self.settings.fsync_ns);
        let day = time::day_start(now);
        match &mut self.cur {
            Some((d, _)) if day > *d => self.roll_to(day, now),
            Some((_, s)) => s.tick(now, flush, fsync),
            None => Ok(()),
        }
    }

    pub fn current_path(&self) -> Option<&str> {
        self.cur.as_ref().map(|(_, s)| s.path.as_str())
    }

    pub fn close(mut self) -> io::Result<()> {
        match self.cur.take() {
            Some((_, s)) => s.close(),
            None => Ok(()),
        }
    }
}
