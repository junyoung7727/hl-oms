//! `xtask layers`: the crate layering table (`tools/ci/layers.toml`,
//! ADR-033 part 2) checked against the workspace.
//!
//! Four kinds of rule:
//! - `only`: a crate in `from` may depend (any kind) on no workspace crate
//!   outside `to`;
//! - `deny`: a crate in `from` (minus `except`) may not depend (any kind) on
//!   a workspace crate in `to`;
//! - `external`: a crate in `crates` may have no normal or build dependency
//!   from outside the workspace except those in `allow`;
//! - `text`: a tracked `.rs` file outside `paths` may not contain any string
//!   in `strings` (a match in a comment counts).
//!
//! A pattern is a crate name or a prefix ending in `*`. A name in the table
//! that matches no workspace crate is a finding, so a renamed crate cannot
//! leave a rule behind that checks nothing.
//!
//! The dependencies are the declared ones of every target and platform
//! (`cargo metadata --no-deps`): a `cfg(windows)` dependency counts on
//! Linux too. Exit 0: `layers: ok`. Exit 1: one line per finding. Exit 2:
//! it could not check.

use serde::Deserialize;
use serde_json::Value;
use std::path::Path;
use std::process::Command;

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct Table {
    #[serde(default)]
    only: Vec<Only>,
    #[serde(default)]
    deny: Vec<Deny>,
    #[serde(default)]
    external: Vec<External>,
    #[serde(default)]
    text: Vec<Text>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Only {
    why: String,
    from: Vec<String>,
    to: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Deny {
    why: String,
    from: Vec<String>,
    #[serde(default)]
    except: Vec<String>,
    to: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct External {
    why: String,
    crates: Vec<String>,
    allow: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Text {
    why: String,
    strings: Vec<String>,
    paths: Vec<String>,
}

/// One declared dependency of a workspace crate.
pub struct Dep {
    pub from: String,
    pub to: String,
    /// `normal`, `dev` or `build`.
    pub kind: String,
    /// A path dependency on a workspace member.
    pub workspace: bool,
}

fn matches(pattern: &str, name: &str) -> bool {
    match pattern.strip_suffix('*') {
        Some(prefix) => name.starts_with(prefix),
        None => pattern == name,
    }
}

fn any(patterns: &[String], name: &str) -> bool {
    patterns.iter().any(|p| matches(p, name))
}

/// The workspace crates and their declared dependencies, from the output of
/// `cargo metadata --format-version 1 --no-deps`.
pub fn parse_metadata(meta: &Value) -> Result<(Vec<String>, Vec<Dep>), String> {
    let packages = meta["packages"].as_array().ok_or("no packages array")?;
    let mut crates = Vec::new();
    for p in packages {
        crates.push(
            p["name"]
                .as_str()
                .ok_or("a package without a name")?
                .to_string(),
        );
    }
    let mut deps = Vec::new();
    for p in packages {
        let from = p["name"].as_str().unwrap_or_default().to_string();
        for d in p["dependencies"]
            .as_array()
            .ok_or("a package without dependencies")?
        {
            let to = d["name"]
                .as_str()
                .ok_or("a dependency without a name")?
                .to_string();
            let workspace = d["path"].is_string() && crates.contains(&to);
            let kind = d["kind"].as_str().unwrap_or("normal").to_string();
            deps.push(Dep {
                from: from.clone(),
                to,
                kind,
                workspace,
            });
        }
    }
    Ok((crates, deps))
}

/// The findings of the dependency rules.
pub fn check_deps(table: &Table, crates: &[String], deps: &[Dep]) -> Vec<String> {
    let mut out = Vec::new();
    let mut names: Vec<(&str, &String)> = Vec::new();
    for r in &table.only {
        names.extend(r.from.iter().chain(&r.to).map(|n| (r.why.as_str(), n)));
        for d in deps
            .iter()
            .filter(|d| d.workspace && any(&r.from, &d.from) && !any(&r.to, &d.to))
        {
            out.push(format!(
                "only: {} -> {} ({}): {}",
                d.from, d.to, d.kind, r.why
            ));
        }
    }
    for r in &table.deny {
        names.extend(
            r.from
                .iter()
                .chain(&r.except)
                .chain(&r.to)
                .map(|n| (r.why.as_str(), n)),
        );
        for d in deps.iter().filter(|d| {
            d.workspace && any(&r.from, &d.from) && !any(&r.except, &d.from) && any(&r.to, &d.to)
        }) {
            out.push(format!(
                "deny: {} -> {} ({}): {}",
                d.from, d.to, d.kind, r.why
            ));
        }
    }
    for r in &table.external {
        names.extend(r.crates.iter().map(|n| (r.why.as_str(), n)));
        for d in deps.iter().filter(|d| {
            !d.workspace && d.kind != "dev" && any(&r.crates, &d.from) && !r.allow.contains(&d.to)
        }) {
            out.push(format!(
                "external: {} -> {} ({}): {}",
                d.from, d.to, d.kind, r.why
            ));
        }
    }
    for (why, n) in names {
        if !crates.iter().any(|c| matches(n, c)) {
            out.push(format!(
                "table: {n} matches no workspace crate (rule: {why})"
            ));
        }
    }
    out
}

/// The findings of the text rules over `(path, content)` pairs.
pub fn check_text(table: &Table, files: &[(String, String)]) -> Vec<String> {
    let mut out = Vec::new();
    for r in &table.text {
        for (path, content) in files {
            if r.paths.iter().any(|p| path.starts_with(p.as_str())) {
                continue;
            }
            for (n, line) in content.lines().enumerate() {
                if let Some(s) = r.strings.iter().find(|s| line.contains(s.as_str())) {
                    out.push(format!("text: {path}:{}: {s}: {}", n + 1, r.why));
                }
            }
        }
    }
    out
}

/// The table, the workspace crates, their dependencies and the tracked
/// `.rs` files as `(path, content)`.
type Gathered = (Table, Vec<String>, Vec<Dep>, Vec<(String, String)>);

fn gather(root: &Path) -> Result<Gathered, String> {
    let table_path = root.join("tools/ci/layers.toml");
    let text = std::fs::read_to_string(&table_path)
        .map_err(|e| format!("{}: {e}", table_path.display()))?;
    let table: Table = toml::from_str(&text).map_err(|e| format!("tools/ci/layers.toml: {e}"))?;
    let meta = Command::new(env!("CARGO"))
        .args([
            "metadata",
            "--format-version",
            "1",
            "--no-deps",
            "--offline",
            "--locked",
        ])
        .current_dir(root)
        .output()
        .map_err(|e| format!("cargo metadata: {e}"))?;
    if !meta.status.success() {
        return Err(format!(
            "cargo metadata failed: {}",
            String::from_utf8_lossy(&meta.stderr).trim()
        ));
    }
    let meta: Value =
        serde_json::from_slice(&meta.stdout).map_err(|e| format!("cargo metadata JSON: {e}"))?;
    let (crates, deps) = parse_metadata(&meta)?;
    let ls = Command::new("git")
        .args(["ls-files", "-z", "--", "*.rs"])
        .current_dir(root)
        .output()
        .map_err(|e| format!("git ls-files: {e}"))?;
    if !ls.status.success() {
        return Err("git ls-files failed".into());
    }
    let mut files = Vec::new();
    for path in ls.stdout.split(|b| *b == 0).filter(|p| !p.is_empty()) {
        let path = String::from_utf8_lossy(path).into_owned();
        let content = std::fs::read(root.join(&path)).map_err(|e| format!("{path}: {e}"))?;
        files.push((path, String::from_utf8_lossy(&content).into_owned()));
    }
    Ok((table, crates, deps, files))
}

pub fn run(root: &Path) -> i32 {
    let (table, crates, deps, files) = match gather(root) {
        Ok(g) => g,
        Err(e) => {
            println!("layers: cannot check: {e}");
            return 2;
        }
    };
    let mut found = check_deps(&table, &crates, &deps);
    found.extend(check_text(&table, &files));
    if found.is_empty() {
        println!(
            "layers: ok ({} crates, {} dependencies, {} files)",
            crates.len(),
            deps.len(),
            files.len()
        );
        return 0;
    }
    for f in &found {
        println!("layers: {f}");
    }
    1
}

#[cfg(test)]
mod tests {
    use super::*;

    const TABLE: &str = r#"
        [[only]]
        why = "pure"
        from = ["oms-core"]
        to = []

        [[only]]
        why = "shell"
        from = ["oms-node"]
        to = ["oms-core", "http-fake"]

        [[deny]]
        why = "market data never reaches order code"
        from = ["md-*"]
        to = ["oms-*"]

        [[deny]]
        why = "nothing links the node"
        from = ["*"]
        except = ["watcher-conformance"]
        to = ["oms-node"]

        [[external]]
        why = "no I/O in the core"
        crates = ["oms-core"]
        allow = ["serde"]

        [[text]]
        why = "only the gateway creates orders"
        strings = ["/v5/order/create"]
        paths = ["crates/oms-node/src/venue/", "crates/oms-node/tests/"]
    "#;

    fn table() -> Table {
        toml::from_str(TABLE).expect("the test table parses")
    }

    fn crates() -> Vec<String> {
        [
            "oms-core",
            "oms-node",
            "md-core",
            "md-feed",
            "http-fake",
            "oms-projector",
            "watcher-conformance",
        ]
        .map(String::from)
        .to_vec()
    }

    fn dep(from: &str, to: &str, kind: &str) -> Dep {
        let workspace = crates().iter().any(|c| c == to);
        Dep {
            from: from.into(),
            to: to.into(),
            kind: kind.into(),
            workspace,
        }
    }

    /// The shape that passes: the planted cases below add one edge each.
    fn base() -> Vec<Dep> {
        vec![
            dep("oms-core", "serde", "normal"),
            dep("oms-core", "proptest", "dev"),
            dep("oms-node", "oms-core", "normal"),
            dep("oms-node", "http-fake", "dev"),
            dep("oms-node", "tokio", "normal"),
            dep("md-feed", "md-core", "normal"),
            dep("oms-projector", "oms-core", "normal"),
            dep("watcher-conformance", "oms-node", "dev"),
        ]
    }

    fn with(extra: Dep) -> Vec<String> {
        let mut deps = base();
        deps.push(extra);
        check_deps(&table(), &crates(), &deps)
    }

    #[test]
    fn the_base_shape_passes() {
        assert_eq!(
            check_deps(&table(), &crates(), &base()),
            Vec::<String>::new()
        );
    }

    #[test]
    fn market_data_on_order_code_is_a_finding_of_every_kind() {
        for kind in ["normal", "dev", "build"] {
            assert_eq!(
                with(dep("md-feed", "oms-core", kind)),
                vec![format!(
                    "deny: md-feed -> oms-core ({kind}): market data never reaches order code"
                )]
            );
        }
    }

    #[test]
    fn the_shell_may_not_link_a_reader() {
        assert_eq!(
            with(dep("oms-node", "oms-projector", "normal")),
            vec!["only: oms-node -> oms-projector (normal): shell".to_string()]
        );
    }

    #[test]
    fn the_pure_core_links_no_workspace_crate_and_no_io_crate() {
        assert_eq!(
            with(dep("oms-core", "md-core", "normal")),
            vec!["only: oms-core -> md-core (normal): pure".to_string()]
        );
        assert_eq!(
            with(dep("oms-core", "tokio", "normal")),
            vec!["external: oms-core -> tokio (normal): no I/O in the core".to_string()]
        );
        assert_eq!(
            with(dep("oms-core", "tokio", "build")),
            vec!["external: oms-core -> tokio (build): no I/O in the core".to_string()]
        );
        assert_eq!(with(dep("oms-core", "tokio", "dev")), Vec::<String>::new());
    }

    #[test]
    fn an_exception_holds_only_for_its_crate() {
        assert_eq!(
            with(dep("oms-projector", "oms-node", "dev")),
            vec!["deny: oms-projector -> oms-node (dev): nothing links the node".to_string()]
        );
    }

    #[test]
    fn a_table_name_that_matches_no_crate_is_a_finding() {
        let t: Table =
            toml::from_str("[[deny]]\nwhy = \"w\"\nfrom = [\"md-*\"]\nto = [\"oms-nod\"]\n")
                .expect("parses");
        assert_eq!(
            check_deps(&t, &crates(), &base()),
            vec!["table: oms-nod matches no workspace crate (rule: w)".to_string()]
        );
    }

    #[test]
    fn an_unknown_field_is_refused() {
        assert!(
            toml::from_str::<Table>("[[deny]]\nwhy = \"w\"\nfrom = []\nto = []\nexcept_for = []\n")
                .is_err()
        );
        assert!(toml::from_str::<Table>("[[allow]]\nwhy = \"w\"\n").is_err());
    }

    #[test]
    fn an_order_path_outside_the_gateway_is_a_finding() {
        let files = vec![
            (
                "crates/oms-node/src/venue/bybit/orders.rs".to_string(),
                "const P: &str = \"/v5/order/create\";".to_string(),
            ),
            (
                "crates/oms-node/tests/bybit_node.rs".to_string(),
                "\"/v5/order/create\"".to_string(),
            ),
            (
                "crates/md-feed/src/lib.rs".to_string(),
                "fn f() {}\n// \"/v5/order/create\"\n".to_string(),
            ),
            (
                "crates/oms-nodex/src/lib.rs".to_string(),
                "\"/v5/order/create-batch\"".to_string(),
            ),
        ];
        assert_eq!(
            check_text(&table(), &files),
            vec![
                "text: crates/md-feed/src/lib.rs:2: /v5/order/create: only the gateway creates orders".to_string(),
                "text: crates/oms-nodex/src/lib.rs:1: /v5/order/create: only the gateway creates orders".to_string(),
            ]
        );
    }

    #[test]
    fn metadata_marks_only_path_members_as_workspace() {
        let meta = serde_json::json!({"packages": [
            {"name": "oms-core", "dependencies": [
                {"name": "serde", "kind": null},
                {"name": "proptest", "kind": "dev"}
            ]},
            {"name": "oms-node", "dependencies": [
                {"name": "oms-core", "kind": null, "path": "/w/crates/oms-core"},
                {"name": "vendored", "kind": "build", "path": "/elsewhere/vendored"}
            ]}
        ]});
        let (crates, deps) = parse_metadata(&meta).expect("parses");
        assert_eq!(crates, ["oms-core", "oms-node"]);
        let got: Vec<(String, String, String, bool)> = deps
            .into_iter()
            .map(|d| (d.from, d.to, d.kind, d.workspace))
            .collect();
        assert_eq!(
            got,
            [
                ("oms-core".into(), "serde".into(), "normal".into(), false),
                ("oms-core".into(), "proptest".into(), "dev".into(), false),
                ("oms-node".into(), "oms-core".into(), "normal".into(), true),
                ("oms-node".into(), "vendored".into(), "build".into(), false),
            ]
        );
    }

    /// The real tree against the real table: the same run as the CI step.
    #[test]
    fn the_repository_passes_its_own_table() {
        let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        assert_eq!(run(&root), 0);
    }
}
