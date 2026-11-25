//! A reader for the INI files of `deploy/`: systemd units and `grafana.ini`.
//!
//! Sections are `[Name]`; entries are `Key=Value` (systemd) or
//! `key = value` (Grafana), trimmed. A line whose first non-blank character
//! is `#` or `;` is a comment. In a systemd file a trailing `\` joins the next
//! line with one space. A key given twice keeps both values in order. Any
//! other line is an error that names its line number.

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Dialect {
    Systemd,
    Grafana,
}

#[derive(Clone, Debug, Default)]
pub struct Ini {
    /// `(section, key, value)` in file order.
    pub entries: Vec<(String, String, String)>,
}

impl Ini {
    /// Every value of `key` in `section`, in file order.
    pub fn values(&self, section: &str, key: &str) -> Vec<&str> {
        self.entries
            .iter()
            .filter(|(s, k, _)| s == section && k == key)
            .map(|(_, _, v)| v.as_str())
            .collect()
    }

    /// The value of `key` in `section` when it is given exactly once.
    pub fn one(&self, section: &str, key: &str) -> Option<&str> {
        match self.values(section, key).as_slice() {
            [v] => Some(v),
            _ => None,
        }
    }

    /// Every value of `key` in any section.
    pub fn any(&self, key: &str) -> Vec<&str> {
        self.entries
            .iter()
            .filter(|(_, k, _)| k == key)
            .map(|(_, _, v)| v.as_str())
            .collect()
    }
}

pub fn parse(text: &str, dialect: Dialect) -> Result<Ini, String> {
    let mut ini = Ini::default();
    let mut section: Option<String> = None;
    let mut lines = text.lines().enumerate();
    while let Some((i, raw)) = lines.next() {
        let n = i + 1;
        let mut line = raw.trim().to_string();
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        if dialect == Dialect::Systemd {
            while let Some(head) = line.strip_suffix('\\') {
                let mut joined = head.trim_end().to_string();
                // Comment lines inside a continuation are skipped, as systemd does.
                let next = loop {
                    match lines.next() {
                        Some((_, l))
                            if l.trim_start().starts_with('#')
                                || l.trim_start().starts_with(';') =>
                        {
                            continue;
                        }
                        Some((_, l)) => break Some(l.trim().to_string()),
                        None => break None,
                    }
                };
                match next {
                    Some(l) => {
                        joined.push(' ');
                        joined.push_str(&l);
                        line = joined;
                    }
                    None => return Err(format!("line {n}: a continuation at the end of the file")),
                }
            }
        }
        if let Some(rest) = line.strip_prefix('[') {
            match rest.strip_suffix(']') {
                Some(name) if !name.trim().is_empty() => section = Some(name.trim().to_string()),
                _ => return Err(format!("line {n}: a bad section header")),
            }
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            return Err(format!(
                "line {n}: neither a section, an entry nor a comment"
            ));
        };
        let key = key.trim();
        if key.is_empty() || key.contains(char::is_whitespace) {
            return Err(format!("line {n}: a bad key"));
        }
        let Some(s) = &section else {
            return Err(format!("line {n}: an entry before the first section"));
        };
        ini.entries
            .push((s.clone(), key.to_string(), value.trim().to_string()));
    }
    Ok(ini)
}

#[test]
fn the_ini_reader_keeps_repeats_and_joins_lines() {
    let unit = "# head\n[Unit]\nDescription=x\n\n[Service]\nEnvironment=A=1\n; note\nEnvironment=B=2\nExecStart=/bin/a \\\n  # inside\n  --b \\\n  --c\n";
    let ini = parse(unit, Dialect::Systemd).unwrap();
    assert_eq!(ini.values("Service", "Environment"), ["A=1", "B=2"]);
    assert_eq!(ini.one("Service", "Environment"), None);
    assert_eq!(ini.one("Service", "ExecStart"), Some("/bin/a --b --c"));
    assert_eq!(ini.one("Unit", "Description"), Some("x"));
    assert_eq!(ini.values("Unit", "ExecStart"), Vec::<&str>::new());

    let grafana = "[server]\nhttp_addr = 192.0.2.1\n[paths]\ndata = /mnt/nvme/x\nline = a \\\n";
    let g = parse(grafana, Dialect::Grafana).unwrap();
    assert_eq!(g.one("server", "http_addr"), Some("192.0.2.1"));
    // Grafana has no continuation: the backslash stays in the value.
    assert_eq!(g.one("paths", "line"), Some("a \\"));

    for (bad, why) in [
        ("[Unit]\nno equals sign\n", "line 2: neither"),
        ("Key=before a section\n", "line 1: an entry before"),
        ("[Unit\nA=1\n", "line 1: a bad section"),
        ("[]\n", "line 1: a bad section"),
        ("[Unit]\nbad key=1\n", "line 2: a bad key"),
        ("[Unit]\n=1\n", "line 2: a bad key"),
        ("[Service]\nExecStart=/bin/a \\\n", "line 2: a continuation"),
    ] {
        let e = parse(bad, Dialect::Systemd).unwrap_err();
        assert!(e.starts_with(why), "{bad:?}: {e}");
    }
}
