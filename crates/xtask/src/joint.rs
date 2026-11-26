//! `xtask joint [--runs N] [--only <test name prefix>]`: the OMS and
//! watcher joint suite (`crates/oms-node/tests/joint.rs`, ADR-039).
//!
//! 1. `OMS_TEST_DIR` must be set, absolute, at most [`DIR_MAX`] bytes and
//!    outside any git work tree; `--runs` is 1 to [`RUNS_MAX`]. A missing
//!    directory is created (mode 0700). Else exit 2.
//! 2. Builds the watcher's test build, `omsctl` and the joint build of
//!    `oms-node`, takes each executable from cargo's `compiler-artifact`
//!    message, copies the watcher and `omsctl` into `r<pid>-bin/` (so a
//!    later build cannot replace them under a run) and checks the
//!    watcher's `--version`. Exit 2 on any failure.
//! 3. Runs the joint tests N times, one test at a time, each run in its own
//!    directory `r<pid>-<k>/`. A run passes only with exit 0, `test result:
//!    ok`, at least one test run, none ignored, and no process left that
//!    names its directory ([`orphan_scan`]). Stops at the first failed run
//!    (exit 1).

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

/// `OMS_TEST_DIR` may have at most this many bytes: the rig's longest
/// socket path is the base + 12 bytes of run directory + 29, below 100.
pub const DIR_MAX: usize = 58;
/// `--runs` at most (the run directory `r<pid>-<k>` has room for two
/// digits).
pub const RUNS_MAX: u64 = 99;

/// The git work tree `p` lies in: the first of `p` and its ancestors that
/// holds `.git`, whether `p` exists or not.
fn git_work_tree(p: &Path) -> Option<PathBuf> {
    p.ancestors()
        .find(|a| a.join(".git").exists())
        .map(Path::to_path_buf)
}

/// The rules of step 1 on the value of `OMS_TEST_DIR`.
pub fn check_test_dir(value: Option<&str>) -> Result<PathBuf, String> {
    let v = value.filter(|v| !v.is_empty()).ok_or(
        "OMS_TEST_DIR is not set: name a directory outside any git work tree, at most 58 bytes",
    )?;
    let p = PathBuf::from(v);
    if !p.is_absolute() {
        return Err(format!("OMS_TEST_DIR {v} is not an absolute path"));
    }
    if let Some(tree) = git_work_tree(&p) {
        return Err(format!(
            "OMS_TEST_DIR {v} is inside the git work tree {}: the watcher refuses a token file there",
            tree.display()
        ));
    }
    if p.exists() && !p.is_dir() {
        return Err(format!("OMS_TEST_DIR {v} exists and is not a directory"));
    }
    if v.len() > DIR_MAX {
        return Err(format!(
            "OMS_TEST_DIR {v} has {} bytes, at most {DIR_MAX} (the socket paths under it must stay below 100 bytes)",
            v.len()
        ));
    }
    Ok(p)
}

/// Creates a missing `dir` with mode 0700 on its last component; `true`
/// if it was created.
pub fn make_test_dir(dir: &Path) -> Result<bool, String> {
    if dir.is_dir() {
        return Ok(false);
    }
    std::fs::create_dir_all(dir).map_err(|e| format!("create {}: {e}", dir.display()))?;
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))
        .map_err(|e| format!("chmod {}: {e}", dir.display()))?;
    Ok(true)
}

/// Whether one run of the joint tests passed: exit 0, every `test result`
/// line ok, at least one test passed, none ignored.
pub fn run_verdict(code: Option<i32>, output: &str) -> Result<usize, String> {
    let mut passed = 0;
    let mut results = 0;
    for line in output.lines() {
        let Some(rest) = line.strip_prefix("test result: ") else {
            continue;
        };
        results += 1;
        if !rest.starts_with("ok.") {
            return Err(format!("not ok: {line}"));
        }
        let count = |what: &str| -> usize {
            rest.split(';')
                .filter_map(|part| part.trim().strip_suffix(what))
                .filter_map(|n| n.trim().rsplit(' ').next().and_then(|n| n.parse().ok()))
                .next()
                .unwrap_or(0)
        };
        passed += count("passed");
        let ignored = count("ignored");
        if ignored > 0 {
            return Err(format!("{ignored} ignored: {line}"));
        }
    }
    if code != Some(0) {
        return Err(format!("exit {code:?}"));
    }
    if results == 0 || passed == 0 {
        return Err("no test ran".into());
    }
    Ok(passed)
}

/// What the orphan scan does with a process whose command line names the
/// run directory: one of the test binaries is signalled, anything else is
/// only reported.
#[derive(Debug, PartialEq)]
pub enum Orphan {
    TestBinary { pid: u32, exe: PathBuf },
    Other { pid: u32, cmdline: String },
}

/// Classifies one process: `None` unless its command line (NUL-separated
/// as in `/proc/<pid>/cmdline`) names `run_dir`.
pub fn classify(
    pid: u32,
    cmdline: &[u8],
    exe: Option<&Path>,
    run_dir: &Path,
    bins: &[PathBuf],
) -> Option<Orphan> {
    let text = String::from_utf8_lossy(cmdline);
    let dir = run_dir.to_string_lossy();
    let below = format!("{dir}/");
    if !text
        .split('\0')
        .any(|arg| arg == dir || arg.contains(&below))
    {
        return None;
    }
    match exe {
        Some(e) if bins.iter().any(|b| b == e) => Some(Orphan::TestBinary {
            pid,
            exe: e.to_path_buf(),
        }),
        _ => Some(Orphan::Other {
            pid,
            cmdline: text.trim_end_matches('\0').replace('\0', " "),
        }),
    }
}

/// Every process but this one whose command line names `run_dir`; one of
/// `bins` gets SIGCONT then SIGKILL. Each match is one line of the
/// result (empty: no orphan).
pub fn orphan_scan(run_dir: &Path, bins: &[PathBuf]) -> Vec<String> {
    let me = std::process::id();
    let mut found = Vec::new();
    let Ok(rd) = std::fs::read_dir("/proc") else {
        return vec!["cannot read /proc".into()];
    };
    for e in rd.flatten() {
        let Some(pid) = e.file_name().to_str().and_then(|n| n.parse::<u32>().ok()) else {
            continue;
        };
        if pid == me {
            continue;
        }
        let Ok(cmdline) = std::fs::read(e.path().join("cmdline")) else {
            continue;
        };
        let exe = std::fs::read_link(e.path().join("exe")).ok().map(|p| {
            let s = p.to_string_lossy();
            PathBuf::from(s.strip_suffix(" (deleted)").unwrap_or(&s))
        });
        match classify(pid, &cmdline, exe.as_deref(), run_dir, bins) {
            Some(Orphan::TestBinary { pid, exe }) => {
                for sig in ["-CONT", "-KILL"] {
                    let _ = Command::new("kill").args([sig, &pid.to_string()]).status();
                }
                found.push(format!("orphan process {pid} {}", exe.display()));
            }
            Some(Orphan::Other { pid, cmdline }) => {
                found.push(format!("process {pid} names the run directory: {cmdline}"));
            }
            None => {}
        }
    }
    found
}

/// Builds one binary and returns the executable of its `compiler-artifact`
/// message.
fn build(root: &Path, args: &[&str], bin: &str) -> Result<PathBuf, String> {
    let out = Command::new("cargo")
        .arg("build")
        .args(["--locked", "--message-format=json-render-diagnostics"])
        .args(args)
        .args(["--bin", bin])
        .current_dir(root)
        .stderr(std::process::Stdio::inherit())
        .output()
        .map_err(|e| format!("cargo build {bin}: {e}"))?;
    if !out.status.success() {
        return Err(format!("cargo build {} --bin {bin} failed", args.join(" ")));
    }
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .filter(|v| v["reason"] == "compiler-artifact" && v["target"]["name"] == bin)
        .filter_map(|v| v["executable"].as_str().map(PathBuf::from))
        .next_back()
        .ok_or_else(|| format!("cargo named no executable for {bin}"))
}

/// Step 2: the three binaries; the watcher and `omsctl` copied into
/// `bin_dir`. Returns (watcher, omsctl, oms-node).
fn prepare_bins(root: &Path, bin_dir: &Path) -> Result<(PathBuf, PathBuf, PathBuf), String> {
    let watcher = build(
        root,
        &["-p", "watcher", "--features", "test-endpoints"],
        "watcher",
    )?;
    let omsctl = build(root, &["-p", "oms-api"], "omsctl")?;
    let node = build(root, &["-p", "oms-node", "--features", "joint"], "oms-node")?;
    let _ = std::fs::remove_dir_all(bin_dir);
    std::fs::create_dir_all(bin_dir).map_err(|e| format!("create {}: {e}", bin_dir.display()))?;
    let copy = |from: &Path| -> Result<PathBuf, String> {
        let to = bin_dir.join(from.file_name().unwrap_or_default());
        std::fs::copy(from, &to)
            .map_err(|e| format!("copy {} to {}: {e}", from.display(), to.display()))?;
        Ok(to)
    };
    let (watcher, omsctl) = (copy(&watcher)?, copy(&omsctl)?);
    let v = Command::new(&watcher)
        .arg("--version")
        .output()
        .map_err(|e| format!("{} --version: {e}", watcher.display()))?;
    let line = String::from_utf8_lossy(&v.stdout).trim_end().to_string();
    if !line.ends_with(" test-endpoints") {
        return Err(format!(
            "{} --version printed {line:?}, not the test build",
            watcher.display()
        ));
    }
    Ok((watcher, omsctl, node))
}

pub fn run(root: &Path, runs: Option<&str>, only: Option<&str>) -> i32 {
    let dir = match check_test_dir(std::env::var("OMS_TEST_DIR").ok().as_deref()) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("joint: {e}");
            return 2;
        }
    };
    let runs = match runs.map_or(Ok(1), str::parse::<u64>) {
        Ok(n) if (1..=RUNS_MAX).contains(&n) => n,
        _ => {
            eprintln!(
                "joint: --runs {}: a number from 1 to {RUNS_MAX}",
                runs.unwrap_or("")
            );
            return 2;
        }
    };
    match make_test_dir(&dir) {
        Ok(true) => println!("joint: created {}", dir.display()),
        Ok(false) => {}
        Err(e) => {
            eprintln!("joint: {e}");
            return 2;
        }
    }
    let me = std::process::id();
    let bin_dir = dir.join(format!("r{me}-bin"));
    let (watcher, omsctl, node) = match prepare_bins(root, &bin_dir) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("joint: {e}");
            return 2;
        }
    };
    let bins = [watcher.clone(), omsctl.clone(), node];
    for k in 1..=runs {
        let run_dir = dir.join(format!("r{me}-{k}"));
        let _ = std::fs::remove_dir_all(&run_dir);
        if let Err(e) = std::fs::create_dir_all(&run_dir) {
            eprintln!("joint: create {}: {e}", run_dir.display());
            return 2;
        }
        let t = Instant::now();
        let mut cmd = Command::new("cargo");
        cmd.args([
            "test",
            "--locked",
            "-p",
            "oms-node",
            "--features",
            "joint",
            "--test",
            "joint",
            "--",
            "--test-threads=1",
        ])
        .args(only)
        .env("JOINT_WATCHER_BIN", &watcher)
        .env("JOINT_OMSCTL_BIN", &omsctl)
        .env("OMS_TEST_DIR", &run_dir)
        .current_dir(root);
        let out = match cmd.output() {
            Ok(o) => o,
            Err(e) => {
                eprintln!("joint: cannot run cargo: {e}");
                return 2;
            }
        };
        let (stdout, stderr) = (
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr),
        );
        print!("{stdout}");
        eprint!("{stderr}");
        let orphans = orphan_scan(&run_dir, &bins);
        // An orphan fails the run whatever the tests said.
        let verdict = match (run_verdict(out.status.code(), &stdout), orphans.is_empty()) {
            (r, true) => r,
            (Ok(_), false) => Err(orphans.join("; ")),
            (Err(e), false) => Err(format!("{e}; {}", orphans.join("; "))),
        };
        match verdict {
            Ok(n) => {
                println!(
                    "joint run {k}/{runs}: ok in {:.1} s ({n} tests)",
                    t.elapsed().as_secs_f64()
                );
                let _ = std::fs::remove_dir(&run_dir);
            }
            Err(why) => {
                println!(
                    "joint run {k}/{runs}: FAILED in {:.1} s: {why}",
                    t.elapsed().as_secs_f64()
                );
                return 1;
            }
        }
    }
    // Kept after a failure, with the failed run's directory.
    let _ = std::fs::remove_dir_all(&bin_dir);
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("xtask-joint-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn a_test_dir_must_be_set_absolute_short_and_not_a_file() {
        for v in [None, Some("")] {
            assert!(
                check_test_dir(v)
                    .expect_err("refused")
                    .contains("OMS_TEST_DIR is not set")
            );
        }
        assert!(
            check_test_dir(Some("rel/dir"))
                .expect_err("refused")
                .contains("not an absolute path")
        );
        let long = format!("/nonexistent/{}", "x".repeat(DIR_MAX - 12));
        assert_eq!(long.len(), DIR_MAX + 1);
        assert!(
            check_test_dir(Some(&long))
                .expect_err("refused")
                .contains("at most 58")
        );
        assert!(check_test_dir(Some(&long[..DIR_MAX])).is_ok());
        // A file outside any work tree (a scratch directory may lie in one).
        assert!(
            check_test_dir(Some("/proc/version"))
                .expect_err("refused")
                .contains("is not a directory")
        );
    }

    /// The walk starts at the path itself, so a missing directory below a
    /// work tree is refused too.
    #[test]
    fn a_test_dir_inside_a_git_work_tree_is_refused_whether_it_exists_or_not() {
        let d = scratch("git");
        std::fs::create_dir_all(d.join("repo/.git")).expect("test setup");
        for p in [d.join("repo"), d.join("repo/a"), d.join("repo/missing/b")] {
            let e = check_test_dir(p.to_str()).expect_err("refused");
            assert!(e.contains("inside the git work tree"), "{e}");
        }
        // A sibling is not refused for this tree (the scratch directory
        // itself may lie in another one).
        let repo = d.join("repo").display().to_string();
        let e = check_test_dir(d.join("other").to_str())
            .err()
            .unwrap_or_default();
        assert!(!e.contains(&format!("work tree {repo}")), "{e}");
        std::fs::remove_dir_all(&d).expect("test setup");
    }

    #[test]
    fn a_missing_test_dir_is_created_with_mode_0700() {
        let d = scratch("make").join("a/b");
        assert_eq!(make_test_dir(&d), Ok(true));
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&d)
                .expect("test setup")
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
        assert_eq!(make_test_dir(&d), Ok(false));
        std::fs::remove_dir_all(
            d.parent()
                .expect("test setup")
                .parent()
                .expect("test setup"),
        )
        .expect("test setup");
    }

    #[test]
    fn a_run_passes_only_with_exit_0_ok_tests_run_and_none_ignored() {
        let ok = "test j0 ... ok\ntest result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 23.29s\n";
        assert_eq!(run_verdict(Some(0), ok), Ok(2));
        assert!(
            run_verdict(Some(101), ok)
                .expect_err("refused")
                .contains("exit")
        );
        assert!(run_verdict(None, ok).is_err());
        let failed = "test result: FAILED. 1 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1s\n";
        assert!(
            run_verdict(Some(0), failed)
                .expect_err("refused")
                .contains("not ok")
        );
        let ignored = "test result: ok. 1 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out; finished in 1s\n";
        assert!(
            run_verdict(Some(0), ignored)
                .expect_err("refused")
                .contains("1 ignored")
        );
        let none = "test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 2 filtered out; finished in 0s\n";
        assert_eq!(run_verdict(Some(0), none), Err("no test ran".into()));
        assert_eq!(run_verdict(Some(0), ""), Err("no test ran".into()));
    }

    #[test]
    fn only_a_process_naming_the_run_dir_is_an_orphan_and_only_a_test_binary_is_signalled() {
        let run = Path::new("/t/r42-1");
        let bins = [PathBuf::from("/t/r42-bin/watcher")];
        let node = b"/x/oms-node\0run\0--config\0/t/r42-1/j1-7/o/oms.toml\0";
        assert_eq!(
            classify(1, node, Some(&bins[0]), run, &bins),
            Some(Orphan::TestBinary {
                pid: 1,
                exe: bins[0].clone()
            })
        );
        assert_eq!(
            classify(
                2,
                b"tail\0-f\0/t/r42-1/j1-7/o/node.log\0",
                Some(Path::new("/usr/bin/tail")),
                run,
                &bins
            ),
            Some(Orphan::Other {
                pid: 2,
                cmdline: "tail -f /t/r42-1/j1-7/o/node.log".into()
            })
        );
        // Another invocation's run directory, a longer name with the same
        // start, and no mention at all are not matched.
        for other in [
            &b"x\0/t/r43-1/j1-7\0"[..],
            b"x\0/t/r42-10/j1-7\0",
            b"sleep\x0010\0",
        ] {
            assert_eq!(classify(3, other, Some(&bins[0]), run, &bins), None);
        }
    }

    /// The scan on the real `/proc`: a copy of `sh` that stands for a test
    /// binary is killed, the system `sh` naming the directory is only
    /// reported. The directory goes in argv[0]; the loop keeps `sh` from
    /// handing its process to a child.
    #[test]
    fn the_orphan_scan_kills_a_test_binary_and_reports_another_process() {
        let d = scratch("scan");
        let run = d.join("r1-1");
        std::fs::create_dir_all(&run).expect("test setup");
        let sh = PathBuf::from("/bin/sh");
        let copy = d.join("sh-bin");
        std::fs::copy(&sh, &copy).expect("test setup");
        let arg = run.join("x").display().to_string();
        use std::os::unix::process::CommandExt;
        let script = "while :; do sleep 0.1; done";
        let mut ours = Command::new(&copy)
            .arg0(&arg)
            .args(["-c", script])
            .spawn()
            .expect("test setup");
        let mut other = Command::new(&sh)
            .arg0(&arg)
            .args(["-c", script])
            .spawn()
            .expect("test setup");
        std::thread::sleep(std::time::Duration::from_millis(100));
        let found = orphan_scan(&run, std::slice::from_ref(&copy));
        assert!(
            found.contains(&format!("orphan process {} {}", ours.id(), copy.display())),
            "{found:?}"
        );
        assert!(
            found
                .iter()
                .any(|l| l.starts_with(&format!("process {} names the run directory", other.id()))),
            "{found:?}"
        );
        let st = ours.wait().expect("test setup");
        use std::os::unix::process::ExitStatusExt;
        assert_eq!(st.signal(), Some(9), "the test binary was not killed");
        assert!(
            other.try_wait().expect("test setup").is_none(),
            "the other process was signalled"
        );
        let _ = other.kill();
        let _ = other.wait();
        std::fs::remove_dir_all(&d).expect("test setup");
    }
}
