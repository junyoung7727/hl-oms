//! The expression scanner (ADR-035 D7): the metric names, label names,
//! label matchers and functions of a MetricsQL expression, for the subset
//! the rule files use. It is not a parser: `vmalert -dryRun` checks the
//! syntax where the binary exists.

/// Functions the rule files may use (rule R4). `label_values` is Grafana
/// templating only.
pub const FUNCTIONS: &[&str] = &[
    "time",
    "vector",
    "absent",
    "absent_over_time",
    "last_over_time",
    "tfirst_over_time",
    "max_over_time",
    "min_over_time",
    "sum_over_time",
    "count_over_time",
    "increase",
    "resets",
    "rate",
    "deriv",
    "clamp_min",
    "abs",
    "round",
    "max",
    "min",
    "sum",
    "count",
    "avg",
    "topk",
    "label_values",
];

/// Keywords followed by a parenthesised label list. Tested before the
/// function rule, so `on(venue)` is a label list, not a function.
const LIST_KEYWORDS: &[&str] = &[
    "by",
    "without",
    "on",
    "ignoring",
    "group_left",
    "group_right",
];
/// Of these, the ones whose list may be left out.
const LIST_OPTIONAL: &[&str] = &["group_left", "group_right"];
/// Keywords that are neither names nor functions.
const SKIP_KEYWORDS: &[&str] = &["and", "or", "unless", "bool", "offset"];

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Scan {
    pub names: Vec<String>,
    pub labels: Vec<String>,
    /// `(label, operator, value)` of every matcher inside `{…}`.
    pub matchers: Vec<(String, String, String)>,
    pub functions: Vec<String>,
    /// Shapes the scanner does not read (rule R4).
    pub errors: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Tok {
    Ident(String),
    Str(String),
    /// A number or a duration (`60`, `0.9`, `26h`, `1e3`).
    Num,
    /// A Grafana variable (`$venue`, `${venue}`, `$__interval`).
    Var,
    Open(char),
    Close(char),
    Op(String),
}

fn ident_start(c: char) -> bool {
    c.is_ascii_alphabetic() || c == '_' || c == ':'
}

fn ident_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_' || c == ':'
}

fn tokens(expr: &str, errors: &mut Vec<String>) -> Vec<Tok> {
    let c: Vec<char> = expr.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < c.len() {
        let ch = c[i];
        if ch.is_whitespace() {
            i += 1;
        } else if ch == '"' || ch == '\'' || ch == '`' {
            let mut s = String::new();
            let mut j = i + 1;
            let mut closed = false;
            while j < c.len() {
                if c[j] == '\\' && ch != '`' && j + 1 < c.len() {
                    s.push(c[j + 1]);
                    j += 2;
                } else if c[j] == ch {
                    closed = true;
                    j += 1;
                    break;
                } else {
                    s.push(c[j]);
                    j += 1;
                }
            }
            if !closed {
                errors.push("a string literal is not closed".to_string());
            }
            out.push(Tok::Str(s));
            i = j;
        } else if ch.is_ascii_digit()
            || (ch == '.' && c.get(i + 1).is_some_and(|d| d.is_ascii_digit()))
        {
            while i < c.len() && (c[i].is_ascii_alphanumeric() || c[i] == '.' || c[i] == '_') {
                i += 1;
            }
            out.push(Tok::Num);
        } else if ch == '$' {
            i += 1;
            if c.get(i) == Some(&'{') {
                while i < c.len() && c[i] != '}' {
                    i += 1;
                }
                i += 1;
            } else {
                while i < c.len() && ident_char(c[i]) {
                    i += 1;
                }
            }
            out.push(Tok::Var);
        } else if ident_start(ch) {
            let start = i;
            while i < c.len() && ident_char(c[i]) {
                i += 1;
            }
            out.push(Tok::Ident(c[start..i].iter().collect()));
        } else if "({[".contains(ch) {
            out.push(Tok::Open(ch));
            i += 1;
        } else if ")}]".contains(ch) {
            out.push(Tok::Close(ch));
            i += 1;
        } else {
            let two: String = c[i..(i + 2).min(c.len())].iter().collect();
            if ["=~", "!~", "!=", "==", ">=", "<="].contains(&two.as_str()) {
                out.push(Tok::Op(two));
                i += 2;
            } else {
                out.push(Tok::Op(ch.to_string()));
                i += 1;
            }
        }
    }
    out
}

/// The index of the token that closes the bracket opened at `open`.
fn closing(t: &[Tok], open: usize) -> Option<usize> {
    let Tok::Open(o) = t[open] else { return None };
    let close = match o {
        '(' => ')',
        '[' => ']',
        _ => '}',
    };
    let mut depth = 0usize;
    for (j, tok) in t.iter().enumerate().skip(open) {
        match tok {
            Tok::Open(x) if *x == o => depth += 1,
            Tok::Close(x) if *x == close => {
                depth -= 1;
                if depth == 0 {
                    return Some(j);
                }
            }
            _ => {}
        }
    }
    None
}

pub fn scan(expr: &str) -> Scan {
    let mut s = Scan::default();
    let t = tokens(expr, &mut s.errors);
    let mut i = 0;
    while i < t.len() {
        match &t[i] {
            Tok::Ident(w) if LIST_KEYWORDS.contains(&w.as_str()) => {
                if t.get(i + 1) == Some(&Tok::Open('(')) {
                    i = label_list(&t, i + 1, &mut s);
                    continue;
                }
                if !LIST_OPTIONAL.contains(&w.as_str()) {
                    s.errors.push(format!("{w} without a label list"));
                }
            }
            Tok::Ident(w) if SKIP_KEYWORDS.contains(&w.as_str()) => {}
            Tok::Ident(w) => match t.get(i + 1) {
                Some(Tok::Open('(')) => s.functions.push(w.clone()),
                Some(Tok::Ident(k)) if k == "by" || k == "without" => {
                    // The prefix form `f by (labels) (…)`.
                    let list_then_args = t.get(i + 2) == Some(&Tok::Open('('))
                        && closing(&t, i + 2)
                            .is_some_and(|c| t.get(c + 1) == Some(&Tok::Open('(')));
                    if list_then_args {
                        s.functions.push(w.clone());
                    } else {
                        s.errors.push(format!(
                            "{w} {k}: not an aggregate of the form `f {k} (labels) (…)`"
                        ));
                    }
                }
                _ => s.names.push(w.clone()),
            },
            Tok::Open('{') => {
                i = matchers(&t, i + 1, &mut s);
                continue;
            }
            Tok::Open('[') => match closing(&t, i) {
                Some(c) => {
                    i = c + 1;
                    continue;
                }
                None => s.errors.push("a [ is not closed".to_string()),
            },
            _ => {}
        }
        i += 1;
    }
    s
}

/// Reads `(a, b, …)` starting at the `(` at `open`; returns the index after
/// the `)`.
fn label_list(t: &[Tok], open: usize, s: &mut Scan) -> usize {
    let mut i = open + 1;
    while i < t.len() {
        match &t[i] {
            Tok::Close(')') => return i + 1,
            Tok::Ident(l) => s.labels.push(l.clone()),
            Tok::Op(o) if o == "," => {}
            other => s.errors.push(format!("{other:?} in a label list")),
        }
        i += 1;
    }
    s.errors.push("a label list is not closed".to_string());
    i
}

/// Reads `label op "value", …}` starting after a `{`; returns the index
/// after the `}`.
fn matchers(t: &[Tok], mut i: usize, s: &mut Scan) -> usize {
    while i < t.len() {
        match (&t[i], t.get(i + 1), t.get(i + 2)) {
            (Tok::Close('}'), _, _) => return i + 1,
            (Tok::Op(o), _, _) if o == "," => i += 1,
            (Tok::Ident(l), Some(Tok::Op(op)), Some(Tok::Str(v)))
                if ["=", "!=", "=~", "!~"].contains(&op.as_str()) =>
            {
                s.labels.push(l.clone());
                s.matchers.push((l.clone(), op.clone(), v.clone()));
                i += 3;
            }
            (other, _, _) => {
                s.errors.push(format!("{other:?} in a label matcher"));
                i += 1;
            }
        }
    }
    s.errors.push("a { is not closed".to_string());
    i
}

#[test]
fn the_scanner_finds_names_labels_and_functions() {
    fn v(x: &[&str]) -> Vec<String> {
        x.iter().map(ToString::to_string).collect()
    }
    // (expr, names, labels, functions, error count)
    let cases: &[(&str, &[&str], &[&str], &[&str], usize)] = &[
        (
            "time() - max by (venue) (md_collector_heartbeat_ts{venue=\"bybit\"}) > 60 or absent_over_time(md_collector_heartbeat_ts{venue=\"bybit\"}[2m])",
            &["md_collector_heartbeat_ts", "md_collector_heartbeat_ts"],
            &["venue", "venue", "venue"],
            &["time", "max", "absent_over_time"],
            0,
        ),
        ("max by (venue) (m)", &["m"], &["venue"], &["max"], 0),
        ("sum(m) by (a, b)", &["m"], &["a", "b"], &["sum"], 0),
        ("x and on(venue) y", &["x", "y"], &["venue"], &[], 0),
        (
            "x * on(a) group_left(b) y",
            &["x", "y"],
            &["a", "b"],
            &[],
            0,
        ),
        ("x * on(a) group_left y", &["x", "y"], &["a"], &[], 0),
        ("m by (venue)", &[], &["venue"], &[], 1),
        ("m by", &[], &[], &[], 2),
        ("x and on y", &["x", "y"], &[], &[], 1),
        ("x[30d] offset 26h", &["x"], &[], &[], 0),
        (
            "last_over_time(x[26h]) - (last_over_time(x[30d] offset 26h) or last_over_time(x[26h]) * 0) > 0",
            &["x", "x", "x"],
            &[],
            &["last_over_time", "last_over_time", "last_over_time"],
            0,
        ),
        (
            "sum_over_time(md_msgs{venue=~\"$venue\"}[$__interval]) / ${step}",
            &["md_msgs"],
            &["venue"],
            &["sum_over_time"],
            0,
        ),
        (
            "process_resident_memory_bytes{job=\"vm\", instance!~'a.*'} > 0.9 * 1073741824 + 1e3",
            &["process_resident_memory_bytes"],
            &["job", "instance"],
            &[],
            0,
        ),
        ("x{a=\"b\\\"c\"} == bool 1", &["x"], &["a"], &[], 0),
        ("x{a=\"b}", &["x"], &["a"], &[], 2),
        ("x{a}", &["x"], &[], &[], 1),
        ("nosuch_fn(x) unless y", &["x", "y"], &[], &["nosuch_fn"], 0),
        ("vector(1)", &[], &[], &["vector"], 0),
    ];
    for &(expr, names, labels, functions, errors) in cases {
        let s = scan(expr);
        assert_eq!(s.names, v(names), "names of {expr}");
        assert_eq!(s.labels, v(labels), "labels of {expr}");
        assert_eq!(s.functions, v(functions), "functions of {expr}");
        assert_eq!(s.errors.len(), errors, "errors of {expr}: {:?}", s.errors);
    }
    let s = scan("up{job=~\"a|b\", x!=\"\"} == 0");
    assert_eq!(
        s.matchers,
        vec![
            ("job".to_string(), "=~".to_string(), "a|b".to_string()),
            ("x".to_string(), "!=".to_string(), String::new()),
        ]
    );
}
