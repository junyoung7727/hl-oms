//! The files the check reads, and parse rule P1.
//!
//! `Tree::real` is the only place that touches the disk: every file under
//! `deploy/`, the pinned qdp metric list and the source files whose literals
//! the check looks for. Everything else works on the map, so a test can plant
//! a defect by changing one file in memory.

use std::collections::BTreeMap;
use std::fmt;
use std::path::Path;

use crate::ini::{self, Dialect, Ini};

/// Files read besides `deploy/`: the registries that name the collector's
/// and the archiver's venues (rule U13), the pinned qdp metric list, and the
/// files that push the metrics of the catalogue (rule R2).
pub const ANCHORS: &[&str] = &[
    "contracts/lake/xrepo/metrics.json",
    "crates/md-collector/src/registry.rs",
    "crates/md-archiver/src/registry.rs",
    "crates/md-collector/src/obs.rs",
    "crates/md-collector/src/telemetry.rs",
    "crates/obs-push/src/lib.rs",
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Finding {
    pub file: String,
    pub rule: &'static str,
    pub text: String,
}

impl fmt::Display for Finding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "deploy: {}: {}: {}", self.file, self.rule, self.text)
    }
}

pub fn finding(out: &mut Vec<Finding>, file: &str, rule: &'static str, text: impl Into<String>) {
    out.push(Finding {
        file: file.to_string(),
        rule,
        text: text.into(),
    });
}

/// Repository path → text.
#[derive(Clone, Debug, Default)]
pub struct Tree {
    pub files: BTreeMap<String, String>,
}

impl Tree {
    pub fn real() -> Tree {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let mut t = Tree::default();
        let mut dirs = vec![root.join("deploy")];
        while let Some(dir) = dirs.pop() {
            let mut entries: Vec<_> = std::fs::read_dir(&dir)
                .unwrap()
                .map(|e| e.unwrap().path())
                .collect();
            entries.sort();
            for p in entries {
                if p.is_dir() {
                    dirs.push(p);
                } else {
                    let rel = p
                        .strip_prefix(&root)
                        .unwrap()
                        .to_string_lossy()
                        .replace('\\', "/");
                    t.files.insert(
                        rel,
                        String::from_utf8_lossy(&std::fs::read(&p).unwrap()).into_owned(),
                    );
                }
            }
        }
        for a in ANCHORS {
            t.files.insert(
                a.to_string(),
                std::fs::read_to_string(root.join(a)).unwrap(),
            );
        }
        t
    }

    /// Replaces the one occurrence of `from` in `path` (a test that names
    /// text which is not there, or is there twice, is itself wrong).
    pub fn replace(&mut self, path: &str, from: &str, to: &str) {
        let text = self
            .files
            .get_mut(path)
            .unwrap_or_else(|| panic!("no file {path}"));
        assert_eq!(
            text.matches(from).count(),
            1,
            "{path}: {from:?} must occur once"
        );
        *text = text.replacen(from, to, 1);
    }

    pub fn set(&mut self, path: &str, text: &str) {
        self.files.insert(path.to_string(), text.to_string());
    }

    pub fn remove(&mut self, path: &str) {
        assert!(self.files.remove(path).is_some(), "no file {path}");
    }
}

/// A systemd unit file under `deploy/`.
pub struct Unit {
    pub path: String,
    /// The file name, e.g. `md-collector@.service`.
    pub name: String,
    pub ini: Ini,
}

/// The files of `deploy/` read by their kind. A file that does not parse is
/// a P1 finding and is left out, so the other rules see only parsed files.
pub struct Parsed {
    pub units: Vec<Unit>,
    pub toml: BTreeMap<String, toml::Value>,
    pub json: BTreeMap<String, serde_json::Value>,
    pub ini: BTreeMap<String, Ini>,
}

pub fn file_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

pub fn parse(t: &Tree, out: &mut Vec<Finding>) -> Parsed {
    let mut p = Parsed {
        units: Vec::new(),
        toml: BTreeMap::new(),
        json: BTreeMap::new(),
        ini: BTreeMap::new(),
    };
    for (path, text) in t.files.iter().filter(|(k, _)| k.starts_with("deploy/")) {
        let name = file_name(path);
        let ext = name.rsplit_once('.').map(|(_, e)| e).unwrap_or("");
        match ext {
            "service" | "timer" | "slice" => match ini::parse(text, Dialect::Systemd) {
                Ok(ini) => p.units.push(Unit {
                    path: path.clone(),
                    name: name.to_string(),
                    ini,
                }),
                Err(e) => finding(out, path, "P1", e),
            },
            "ini" => match ini::parse(text, Dialect::Grafana) {
                Ok(ini) => {
                    p.ini.insert(path.clone(), ini);
                }
                Err(e) => finding(out, path, "P1", e),
            },
            "json" | "yaml" => match serde_json::from_str(text) {
                Ok(v) => {
                    p.json.insert(path.clone(), v);
                }
                Err(e) => finding(out, path, "P1", format!("not JSON: {e}")),
            },
            "toml" => match toml::from_str::<toml::Table>(text) {
                Ok(v) => {
                    p.toml.insert(path.clone(), toml::Value::Table(v));
                }
                Err(e) => finding(out, path, "P1", format!("not TOML: {}", e.message())),
            },
            "md" => {}
            "sh" => {
                if text.lines().next() != Some("#!/bin/sh") {
                    finding(
                        out,
                        path,
                        "P1",
                        "a shell script's first line must be #!/bin/sh",
                    );
                }
            }
            _ => finding(out, path, "P1", format!("unknown file kind {ext:?}")),
        }
    }
    p
}

#[test]
fn every_file_parses_by_its_kind() {
    use crate::{assert_rules, check};
    let unit = "deploy/observability/systemd/md-lake-seal@.timer";
    for (path, text) in [
        ("deploy/observability/x.json", "{\"a\": 1,}"),
        ("deploy/observability/x.yaml", "a: 1\n"),
        ("deploy/observability/x.toml", "a = \n"),
        ("deploy/observability/x.ini", "[server]\nhttp_addr\n"),
        ("deploy/observability/notes.txt", "text\n"),
        ("deploy/observability/noext", "text\n"),
        ("deploy/observability/bin/x.sh", "#!/bin/bash\necho\n"),
    ] {
        let mut t = Tree::real();
        t.set(path, text);
        let f = check(&t);
        let mine: Vec<_> = f.iter().filter(|x| x.file == path).collect();
        assert!(mine.len() == 1 && mine[0].rule == "P1", "{path}: {f:?}");
    }
    let mut t = Tree::real();
    t.replace(
        unit,
        "Persistent=true\n",
        "Persistent=true\nthis is not an entry\n",
    );
    let f = check(&t);
    assert!(
        f.iter()
            .any(|x| x.file == unit && x.rule == "P1" && x.text.contains("line ")),
        "{f:?}"
    );
    // The timer drops out of the other rules once it does not parse.
    assert_rules(&f, &["P1", "U2", "U5"]);
    // A JSON document with a .yaml name and a script with the right first
    // line parse.
    let mut t = Tree::real();
    t.set("deploy/observability/x.yaml", "{\"a\": 1}");
    t.set("deploy/observability/x.md", "# anything\n");
    assert!(check(&t).iter().all(|x| x.rule != "P1"));
}
