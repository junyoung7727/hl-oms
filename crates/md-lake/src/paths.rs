//! L0 paths (lake contract 2.1, 1.4; machine-readable form `l0/paths.json`).
//! Paths are relative to the lake root and use `/`.

pub const L0_ROOT: &str = "crypto/l0";
pub const PARTIAL: &str = ".partial";
pub const SEALED_EXT: &str = ".qraw.zst";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Open,
    SealLog,
    DayBySymbol,
    DayByConnection,
    Instruments,
    Session,
    Manifest,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum L0Path {
    Open {
        venue: String,
        date: String,
        hour: u8,
        collector: String,
        session_ns: i64,
        conn_id: u32,
    },
    SealLog {
        venue: String,
        date: String,
        hour: u8,
    },
    DayBySymbol {
        venue: String,
        market: String,
        channel: String,
        date: String,
        symbol: String,
        partial: bool,
    },
    DayByConnection {
        venue: String,
        date: String,
        collector: String,
        session_ns: i64,
        conn_id: u32,
        partial: bool,
    },
    Instruments {
        venue: String,
        market: String,
        date: String,
        partial: bool,
    },
    Session {
        venue: String,
        date: String,
        writer: String,
        start_ns: i64,
        partial: bool,
    },
    Manifest {
        venue: String,
        date: String,
    },
}

fn no_dotdot(s: &str) -> bool {
    !s.contains("..")
}

fn first_rest(s: &str, first: impl Fn(char) -> bool, rest: impl Fn(char) -> bool) -> bool {
    let mut it = s.chars();
    it.next().is_some_and(first) && it.all(rest) && no_dotdot(s)
}

fn lower_alnum(c: char) -> bool {
    c.is_ascii_lowercase() || c.is_ascii_digit()
}

/// `venue`, `market`: `^[a-z0-9][a-z0-9.-]*$`.
pub fn valid_venue(s: &str) -> bool {
    first_rest(s, lower_alnum, |c| lower_alnum(c) || c == '.' || c == '-')
}

/// `channel`, `symbol`: `^[A-Za-z0-9][A-Za-z0-9.-]*$`.
pub fn valid_topic_name(s: &str) -> bool {
    first_rest(
        s,
        |c| c.is_ascii_alphanumeric(),
        |c| c.is_ascii_alphanumeric() || c == '.' || c == '-',
    )
}

/// `collector`, `writer`: `^[a-z0-9][a-z0-9_]*$`.
pub fn valid_writer(s: &str) -> bool {
    first_rest(s, lower_alnum, |c| lower_alnum(c) || c == '_')
}

/// A real calendar date `YYYY-MM-DD`.
pub fn valid_date(s: &str) -> bool {
    let b = s.as_bytes();
    if b.len() != 10 || b[4] != b'-' || b[7] != b'-' {
        return false;
    }
    let num = |r: std::ops::Range<usize>| {
        s.get(r)
            .filter(|x| x.bytes().all(|c| c.is_ascii_digit()))
            .and_then(|x| x.parse::<u32>().ok())
    };
    let (Some(y), Some(m), Some(d)) = (num(0..4), num(5..7), num(8..10)) else {
        return false;
    };
    let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
    let days = match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => return false,
    };
    (1..=days).contains(&d)
}

/// `^(0|[1-9][0-9]*)$` with an upper limit.
fn parse_uint(s: &str, max: u64) -> Option<u64> {
    if s.is_empty() || !s.bytes().all(|c| c.is_ascii_digit()) || (s.len() > 1 && s.starts_with('0'))
    {
        return None;
    }
    s.parse::<u64>().ok().filter(|v| *v <= max)
}

fn parse_ns(s: &str) -> Option<i64> {
    parse_uint(s, i64::MAX as u64).and_then(|v| i64::try_from(v).ok())
}

fn parse_hour(s: &str) -> Option<u8> {
    let b = s.as_bytes();
    if b.len() != 2 || !b.iter().all(u8::is_ascii_digit) {
        return None;
    }
    let h = (b[0] - b'0') * 10 + (b[1] - b'0');
    (h <= 23).then_some(h)
}

fn kv<'a>(part: &'a str, key: &str) -> Result<&'a str, String> {
    part.strip_prefix(key)
        .and_then(|r| r.strip_prefix('='))
        .ok_or_else(|| format!("expected {key}=, got {part}"))
}

/// `{collector}-{session_ns}-c{conn_id}`.
fn conn_name(s: &str) -> Result<(String, i64, u32), String> {
    let mut it = s.split('-');
    let (Some(c), Some(ns), Some(conn), None) = (it.next(), it.next(), it.next(), it.next()) else {
        return Err(format!("not collector-session-cN: {s}"));
    };
    let conn = conn
        .strip_prefix('c')
        .ok_or_else(|| format!("no c before conn_id: {s}"))?;
    if !valid_writer(c) {
        return Err(format!("collector {c}"));
    }
    let ns = parse_ns(ns).ok_or_else(|| format!("session_ns {ns}"))?;
    let conn =
        parse_uint(conn, u64::from(u32::MAX)).ok_or_else(|| format!("conn_id {conn}"))? as u32;
    Ok((c.to_string(), ns, conn))
}

/// `name.qraw.zst` or `name.qraw.zst.partial` → (name, partial).
fn qraw_name(file: &str) -> Result<(&str, bool), String> {
    if let Some(n) = file.strip_suffix(".qraw.zst.partial") {
        Ok((n, true))
    } else if let Some(n) = file.strip_suffix(SEALED_EXT) {
        Ok((n, false))
    } else {
        Err(format!("not a .qraw.zst file: {file}"))
    }
}

fn check(ok: bool, what: &str, v: &str) -> Result<(), String> {
    if ok {
        Ok(())
    } else {
        Err(format!("{what} {v:?}"))
    }
}

fn hour_str(h: u8) -> String {
    format!("{h:02}")
}

fn suffix(partial: bool) -> &'static str {
    if partial {
        ".qraw.zst.partial"
    } else {
        SEALED_EXT
    }
}

impl L0Path {
    pub fn parse(path: &str) -> Result<L0Path, String> {
        let rest = path
            .strip_prefix("crypto/l0/")
            .ok_or_else(|| format!("not under {L0_ROOT}: {path}"))?;
        let parts: Vec<&str> = rest.split('/').collect();
        let date_ok = |d: &str| check(valid_date(d), "date", d);
        let venue_ok = |v: &str| check(valid_venue(v), "venue", v);
        match parts.as_slice() {
            ["_open", v, d, h, file] => {
                let (venue, date) = (kv(v, "venue")?, kv(d, "date")?);
                venue_ok(venue)?;
                date_ok(date)?;
                let h = kv(h, "hour")?;
                let hour = parse_hour(h).ok_or_else(|| format!("hour {h}"))?;
                let name = file
                    .strip_suffix(".qraw.zst.partial")
                    .ok_or_else(|| format!("an open file is .qraw.zst.partial: {file}"))?;
                let (collector, session_ns, conn_id) = conn_name(name)?;
                Ok(L0Path::Open {
                    venue: venue.into(),
                    date: date.into(),
                    hour,
                    collector,
                    session_ns,
                    conn_id,
                })
            }
            ["_seal", v, d, file] => {
                let (venue, date) = (kv(v, "venue")?, kv(d, "date")?);
                venue_ok(venue)?;
                date_ok(date)?;
                let h = kv(file, "hour")?
                    .strip_suffix(".json")
                    .ok_or_else(|| format!("seal log {file}"))?;
                let hour = parse_hour(h).ok_or_else(|| format!("hour {h}"))?;
                Ok(L0Path::SealLog {
                    venue: venue.into(),
                    date: date.into(),
                    hour,
                })
            }
            ["_manifest", v, file] => {
                let venue = kv(v, "venue")?;
                venue_ok(venue)?;
                let date = kv(file, "date")?
                    .strip_suffix(".jsonl")
                    .ok_or_else(|| format!("manifest {file}"))?;
                date_ok(date)?;
                Ok(L0Path::Manifest {
                    venue: venue.into(),
                    date: date.into(),
                })
            }
            [v, "_conn", d, file] => {
                let (venue, date) = (kv(v, "venue")?, kv(d, "date")?);
                venue_ok(venue)?;
                date_ok(date)?;
                let (name, partial) = qraw_name(file)?;
                let (collector, session_ns, conn_id) = conn_name(name)?;
                Ok(L0Path::DayByConnection {
                    venue: venue.into(),
                    date: date.into(),
                    collector,
                    session_ns,
                    conn_id,
                    partial,
                })
            }
            [v, "_session", d, file] => {
                let (venue, date) = (kv(v, "venue")?, kv(d, "date")?);
                venue_ok(venue)?;
                date_ok(date)?;
                let (name, partial) = qraw_name(file)?;
                let (writer, start) = name
                    .split_once('-')
                    .ok_or_else(|| format!("session file {name}"))?;
                check(valid_writer(writer), "writer", writer)?;
                let start_ns = parse_ns(start).ok_or_else(|| format!("start_ns {start}"))?;
                Ok(L0Path::Session {
                    venue: venue.into(),
                    date: date.into(),
                    writer: writer.into(),
                    start_ns,
                    partial,
                })
            }
            [v, m, c, d, file] => {
                let (venue, market, channel, date) = (
                    kv(v, "venue")?,
                    kv(m, "market")?,
                    kv(c, "channel")?,
                    kv(d, "date")?,
                );
                venue_ok(venue)?;
                check(valid_venue(market), "market", market)?;
                check(valid_topic_name(channel), "channel", channel)?;
                date_ok(date)?;
                let (symbol, partial) = qraw_name(file)?;
                if channel == "instruments" {
                    check(symbol == "_all", "instruments symbol", symbol)?;
                    return Ok(L0Path::Instruments {
                        venue: venue.into(),
                        market: market.into(),
                        date: date.into(),
                        partial,
                    });
                }
                check(valid_topic_name(symbol), "symbol", symbol)?;
                Ok(L0Path::DayBySymbol {
                    venue: venue.into(),
                    market: market.into(),
                    channel: channel.into(),
                    date: date.into(),
                    symbol: symbol.into(),
                    partial,
                })
            }
            _ => Err(format!("not an L0 path: {path}")),
        }
    }

    pub fn to_path(&self) -> String {
        match self {
            L0Path::Open {
                venue,
                date,
                hour,
                collector,
                session_ns,
                conn_id,
            } => format!(
                "{L0_ROOT}/_open/venue={venue}/date={date}/hour={}/{collector}-{session_ns}-c{conn_id}.qraw.zst.partial",
                hour_str(*hour)
            ),
            L0Path::SealLog { venue, date, hour } => {
                format!(
                    "{L0_ROOT}/_seal/venue={venue}/date={date}/hour={}.json",
                    hour_str(*hour)
                )
            }
            L0Path::Manifest { venue, date } => {
                format!("{L0_ROOT}/_manifest/venue={venue}/date={date}.jsonl")
            }
            L0Path::DayBySymbol {
                venue,
                market,
                channel,
                date,
                symbol,
                partial,
            } => format!(
                "{L0_ROOT}/venue={venue}/market={market}/channel={channel}/date={date}/{symbol}{}",
                suffix(*partial)
            ),
            L0Path::DayByConnection {
                venue,
                date,
                collector,
                session_ns,
                conn_id,
                partial,
            } => format!(
                "{L0_ROOT}/venue={venue}/_conn/date={date}/{collector}-{session_ns}-c{conn_id}{}",
                suffix(*partial)
            ),
            L0Path::Instruments {
                venue,
                market,
                date,
                partial,
            } => format!(
                "{L0_ROOT}/venue={venue}/market={market}/channel=instruments/date={date}/_all{}",
                suffix(*partial)
            ),
            L0Path::Session {
                venue,
                date,
                writer,
                start_ns,
                partial,
            } => format!(
                "{L0_ROOT}/venue={venue}/_session/date={date}/{writer}-{start_ns}{}",
                suffix(*partial)
            ),
        }
    }

    pub fn kind(&self) -> Kind {
        match self {
            L0Path::Open { .. } => Kind::Open,
            L0Path::SealLog { .. } => Kind::SealLog,
            L0Path::DayBySymbol { .. } => Kind::DayBySymbol,
            L0Path::DayByConnection { .. } => Kind::DayByConnection,
            L0Path::Instruments { .. } => Kind::Instruments,
            L0Path::Session { .. } => Kind::Session,
            L0Path::Manifest { .. } => Kind::Manifest,
        }
    }

    pub fn is_partial(&self) -> bool {
        match self {
            L0Path::Open { .. } => true,
            L0Path::DayBySymbol { partial, .. }
            | L0Path::DayByConnection { partial, .. }
            | L0Path::Instruments { partial, .. }
            | L0Path::Session { partial, .. } => *partial,
            L0Path::SealLog { .. } | L0Path::Manifest { .. } => false,
        }
    }

    /// The same file without `.partial` (the day seal's rename).
    pub fn sealed(&self) -> L0Path {
        let mut p = self.clone();
        match &mut p {
            L0Path::DayBySymbol { partial, .. }
            | L0Path::DayByConnection { partial, .. }
            | L0Path::Instruments { partial, .. }
            | L0Path::Session { partial, .. } => *partial = false,
            _ => {}
        }
        p
    }

    /// The same file with `.partial`.
    pub fn partial(&self) -> L0Path {
        let mut p = self.clone();
        match &mut p {
            L0Path::DayBySymbol { partial, .. }
            | L0Path::DayByConnection { partial, .. }
            | L0Path::Instruments { partial, .. }
            | L0Path::Session { partial, .. } => *partial = true,
            _ => {}
        }
        p
    }

    /// The path under `crypto/l0/` without `.qraw.zst` and `.partial`, for a
    /// file of the lake (not for open files, seal logs or manifests).
    pub fn l0_file_id(&self) -> Option<String> {
        match self {
            L0Path::Open { .. } | L0Path::SealLog { .. } | L0Path::Manifest { .. } => None,
            other => {
                let p = other.sealed().to_path();
                let id = p.strip_prefix("crypto/l0/")?.strip_suffix(SEALED_EXT)?;
                Some(id.to_string())
            }
        }
    }
}

/// The sidecar of a sealed file: `<file>.sha256`.
pub fn sidecar(file: &str) -> String {
    format!("{file}.sha256")
}
