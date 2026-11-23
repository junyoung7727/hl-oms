//! The watcher's credentials file (C-WATCH part 4): a regular file (not a
//! symlink), owned by this process's uid, mode 0600 or 0400, outside the
//! OMS data directory, with a non-empty `api_key` and `api_secret`; no
//! error quotes the file and `Debug` never prints the secret.

#![allow(clippy::unwrap_used)]

mod common;

use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::Path;
use std::sync::mpsc;
use std::time::Duration;

use watcher::creds;

const SECRET: &str = "watcher-made-up-secret-42";
const GOOD: &str = "api_key = \"WKEY\"\napi_secret = \"watcher-made-up-secret-42\"\n";

fn chmod(p: &Path, mode: u32) {
    std::fs::set_permissions(p, std::fs::Permissions::from_mode(mode)).unwrap();
}

/// Refused with `needle`, the path named, the secret not quoted.
fn refused(p: &Path, data: &Path, needle: &str) {
    match creds::load(p, data) {
        Ok(c) => panic!("{} accepted ({c:?}), expected {needle:?}", p.display()),
        Err(e) => {
            assert!(e.contains(needle), "{e:?} lacks {needle:?}");
            assert!(!e.contains(SECRET), "the error quotes the secret: {e:?}");
            assert!(!e.contains('\n'), "not one line: {e:?}");
        }
    }
}

#[test]
fn the_credentials_file_must_be_owner_only_and_outside_the_data_dir() {
    // Root reads any file and owns what it makes: the owner and mode
    // rules cannot be seen from there (as the OMS's permission tests).
    if rustix::process::geteuid().is_root() {
        return;
    }
    let dir = common::test_dir("creds");
    let data = dir.join("data");
    let etc = dir.join("etc");
    std::fs::create_dir_all(&data).unwrap();
    std::fs::create_dir_all(&etc).unwrap();
    let p = etc.join("creds.toml");
    std::fs::write(&p, GOOD).unwrap();

    // Mode 0600 or 0400 only.
    for mode in [0o600, 0o400] {
        chmod(&p, mode);
        let c = creds::load(&p, &data).unwrap();
        assert_eq!(c.api_key(), "WKEY");
        let shown = format!("{c:?}");
        assert!(shown.contains("WKEY") && !shown.contains(SECRET), "{shown}");
    }
    for mode in [
        0o644, 0o640, 0o604, 0o660, 0o610, 0o700, 0o500, 0o4600, 0o2600, 0o1600,
    ] {
        chmod(&p, mode);
        refused(&p, &data, "must have mode 0600 or 0400");
    }
    chmod(&p, 0o600);

    // Another owner (only root could make such a file, so the rule on the
    // opened file's metadata is called with another uid).
    let me = rustix::process::geteuid().as_raw();
    assert!(creds::file_rules(&p, true, me, 0o100600, me).is_ok());
    let e = creds::file_rules(&p, true, me + 1, 0o100600, me).unwrap_err();
    assert!(
        e.contains("must be owned by the user the watcher runs as"),
        "{e}"
    );
    let e = creds::file_rules(&p, false, me, 0o100600, me).unwrap_err();
    assert!(e.contains("is not a regular file"), "{e}");
    // A real file of another owner: `/etc/hostname` is root's on a normal
    // host, and the owner rule runs before the mode rule.
    let root_file = Path::new("/etc/hostname");
    if std::fs::symlink_metadata(root_file).is_ok_and(|m| m.is_file() && m.uid() != me) {
        refused(
            root_file,
            &data,
            "must be owned by the user the watcher runs as",
        );
    }

    // Not a regular file: a symlink to the good file, a directory, a FIFO
    // (refused at once, not waited on for a writer).
    let link = etc.join("link.toml");
    std::os::unix::fs::symlink(&p, &link).unwrap();
    refused(&link, &data, "is a symbolic link");
    refused(&etc, &data, "is not a regular file");
    let fifo = etc.join("fifo.toml");
    rustix::fs::mknodat(
        rustix::fs::CWD,
        &fifo,
        rustix::fs::FileType::Fifo,
        rustix::fs::Mode::from_raw_mode(0o600),
        0,
    )
    .unwrap();
    let (tx, rx) = mpsc::channel();
    let (f, d) = (fifo.clone(), data.clone());
    std::thread::spawn(move || {
        let _ = tx.send(creds::load(&f, &d));
    });
    let r = rx
        .recv_timeout(Duration::from_secs(5))
        .expect("loading a FIFO waits for a writer");
    assert!(r.unwrap_err().contains("is not a regular file"));
    refused(&etc.join("missing.toml"), &data, "missing.toml");

    // The contents: both keys, non-empty, nothing else, TOML, UTF-8; no
    // error quotes the file.
    for (text, needle) in [
        (
            "api_key = \"WKEY\"\n",
            "is not TOML with api_key and api_secret",
        ),
        (
            "api_secret = \"watcher-made-up-secret-42\"\n",
            "is not TOML with api_key and api_secret",
        ),
        (
            "api_key = \"\"\napi_secret = \"watcher-made-up-secret-42\"\n",
            "empty key or secret",
        ),
        (
            "api_key = \"WKEY\"\napi_secret = \"\"\n",
            "empty key or secret",
        ),
        (
            "api_key = \"WKEY\"\napi_secret = \"watcher-made-up-secret-42\"\nsub = 1\n",
            "is not TOML with api_key and api_secret",
        ),
        (
            "api_key = \"WKEY\"\napi_secret = watcher-made-up-secret-42\n",
            "is not TOML with api_key and api_secret",
        ),
        (
            "api_key = \"WKEY\"\napi_secret = \"watcher-made-up-secret-42\n",
            "is not TOML with api_key and api_secret",
        ),
    ] {
        std::fs::write(&p, text).unwrap();
        refused(&p, &data, needle);
    }
    std::fs::write(
        &p,
        b"api_key = \"WKEY\"\napi_secret = \"watcher-made-up-secret-42\xff\"\n",
    )
    .unwrap();
    refused(&p, &data, "creds.toml");
    std::fs::write(&p, GOOD).unwrap();
    assert!(creds::load(&p, &data).is_ok());

    // Outside the data directory: inside it, through `..`, relative, and
    // through a symlinked directory on either side.
    let inside = data.join("creds.toml");
    std::fs::write(&inside, GOOD).unwrap();
    chmod(&inside, 0o600);
    refused(
        &inside,
        &data,
        "credentials_file must be outside the data directory",
    );
    refused(
        &etc.join("..").join("data").join("creds.toml"),
        &data,
        "must not hold `..`",
    );
    refused(
        Path::new("creds.toml"),
        &data,
        "credentials_file must be an absolute path",
    );
    let alias = dir.join("alias");
    std::os::unix::fs::symlink(&data, &alias).unwrap();
    refused(
        &alias.join("creds.toml"),
        &data,
        "credentials_file must be outside the data directory",
    );
    refused(
        &inside,
        &alias,
        "credentials_file must be outside the data directory",
    );
    let etc_alias = dir.join("etc-alias");
    std::os::unix::fs::symlink(&etc, &etc_alias).unwrap();
    assert_eq!(
        creds::load(&etc_alias.join("creds.toml"), &data)
            .unwrap()
            .api_key(),
        "WKEY"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// A made-up bot token of the PR #3 shape (no real bot).
const BOT: &str = "123456:made-up_TOKEN-u19b1a";

/// The first ancestor directory of `p` that holds `.git` (the test's own
/// copy of the rule, to know where a test directory sits).
fn repo_of(p: &Path) -> Option<std::path::PathBuf> {
    p.ancestors()
        .find(|d| std::fs::symlink_metadata(d.join(".git")).is_ok())
        .map(Path::to_path_buf)
}

/// The token file refused with `needle`, the path named, the token not
/// quoted, one line.
fn token_refused(p: &Path, data: &Path, needle: &str) {
    match creds::load_token(p, data) {
        Ok(t) => panic!("{} accepted ({t:?}), expected {needle:?}", p.display()),
        Err(e) => {
            assert!(e.contains(needle), "{e:?} lacks {needle:?}");
            assert!(!e.contains(BOT), "the error quotes the token: {e:?}");
            assert!(!e.contains("made-up"), "the error quotes the token: {e:?}");
            assert!(!e.contains('\n'), "not one line: {e:?}");
        }
    }
}

#[test]
fn the_token_file_must_be_owner_only_outside_any_repository() {
    // Root reads any file and owns what it makes (as the test above).
    if rustix::process::geteuid().is_root() {
        return;
    }
    let dir = common::test_dir("token");
    let data = dir.join("data");
    let etc = dir.join("etc");
    std::fs::create_dir_all(&data).unwrap();
    std::fs::create_dir_all(&etc).unwrap();
    let p = etc.join("telegram.json");
    let good = format!("{{\"token\": \"{BOT}\", \"chat_id\": -1001234}}");
    std::fs::write(&p, &good).unwrap();
    chmod(&p, 0o600);
    if let Some(repo) = repo_of(&dir) {
        // The test directory itself sits in a repository (OMS_TEST_DIR
        // unset and the temp dir inside a checkout): only that rule can be
        // seen from here.
        eprintln!("test dir inside the repository {}", repo.display());
        token_refused(&p, &data, "is inside the git repository");
        return;
    }

    // The good file, mode 0600 or 0400; the chat id a number or a string;
    // other keys are left alone (the OMS's file may be shared).
    let want = |chat: &str| watcher::alert::Token::new(BOT, chat).unwrap();
    for mode in [0o600, 0o400] {
        chmod(&p, mode);
        assert_eq!(creds::load_token(&p, &data).unwrap(), want("-1001234"));
    }
    chmod(&p, 0o600);
    for (text, chat) in [
        (format!("{{\"token\": \"{BOT}\", \"chat_id\": \"@ops\"}}"), "@ops"),
        (format!("{{\"token\": \"{BOT}\", \"chat_id\": 42, \"note\": 1}}"), "42"),
    ] {
        std::fs::write(&p, text).unwrap();
        assert_eq!(creds::load_token(&p, &data).unwrap(), want(chat));
    }
    std::fs::write(&p, &good).unwrap();

    // No group or other bit, no execute, set-id or sticky bit.
    for mode in [
        0o644, 0o640, 0o604, 0o660, 0o620, 0o602, 0o700, 0o500, 0o4600, 0o2600, 0o1600,
    ] {
        chmod(&p, mode);
        token_refused(&p, &data, "must have mode 0600 or 0400");
    }
    chmod(&p, 0o600);

    // Another owner, a file that is not regular (the rule function, as
    // only root could make such a file).
    let me = rustix::process::geteuid().as_raw();
    assert!(creds::token_file_rules(&p, true, me, 0o100600, me).is_ok());
    let e = creds::token_file_rules(&p, true, me + 1, 0o100600, me).unwrap_err();
    assert!(
        e.contains("token file") && e.contains("must be owned by the user the watcher runs as"),
        "{e}"
    );
    let e = creds::token_file_rules(&p, false, me, 0o100600, me).unwrap_err();
    assert!(e.contains("is not a regular file"), "{e}");
    let e = creds::token_file_rules(&p, true, me, 0o100640, me).unwrap_err();
    assert!(e.contains("must have mode 0600 or 0400, not 0640"), "{e}");

    // A symlink to the good file, a directory, a FIFO (at once), missing.
    let link = etc.join("link.json");
    std::os::unix::fs::symlink(&p, &link).unwrap();
    token_refused(&link, &data, "is a symbolic link");
    token_refused(&etc, &data, "is not a regular file");
    let fifo = etc.join("fifo.json");
    rustix::fs::mknodat(
        rustix::fs::CWD,
        &fifo,
        rustix::fs::FileType::Fifo,
        rustix::fs::Mode::from_raw_mode(0o600),
        0,
    )
    .unwrap();
    let (tx, rx) = mpsc::channel();
    let (f, d) = (fifo.clone(), data.clone());
    std::thread::spawn(move || {
        let _ = tx.send(creds::load_token(&f, &d));
    });
    let r = rx
        .recv_timeout(Duration::from_secs(5))
        .expect("loading a FIFO waits for a writer");
    assert!(r.unwrap_err().contains("is not a regular file"));
    token_refused(&etc.join("missing.json"), &data, "missing.json");

    // Inside a git repository: `.git` as a directory or as a file (a
    // worktree), an ancestor several levels up, and reached through a
    // symlinked directory. The file is not read.
    let repo = dir.join("repo");
    std::fs::create_dir_all(repo.join(".git")).unwrap();
    std::fs::create_dir_all(repo.join("a/b")).unwrap();
    for q in [repo.join("t.json"), repo.join("a/b/t.json")] {
        std::fs::write(&q, &good).unwrap();
        chmod(&q, 0o600);
        token_refused(&q, &data, "is inside the git repository");
    }
    let wt = dir.join("wt");
    std::fs::create_dir_all(&wt).unwrap();
    std::fs::write(wt.join(".git"), "gitdir: /nowhere\n").unwrap();
    std::fs::write(wt.join("t.json"), &good).unwrap();
    chmod(&wt.join("t.json"), 0o600);
    token_refused(&wt.join("t.json"), &data, "is inside the git repository");
    let into_repo = dir.join("into-repo");
    std::os::unix::fs::symlink(repo.join("a"), &into_repo).unwrap();
    token_refused(
        &into_repo.join("b/t.json"),
        &data,
        "is inside the git repository",
    );

    // The contents: JSON with a token of the form <digits>:<[A-Za-z0-9_-]+>
    // and a non-empty string or integer chat id; no error quotes the file.
    for (text, needle) in [
        ("".to_string(), "is not JSON with token and chat_id"),
        (format!("token = \"{BOT}\""), "is not JSON with token and chat_id"),
        (format!("[\"{BOT}\"]"), "is not JSON with token and chat_id"),
        (format!("{{\"token\": \"{BOT}\", \"chat_id\": 1"), "is not JSON with token and chat_id"),
        ("{\"chat_id\": 1}".to_string(), "the token is not of the form"),
        (format!("{{\"token\": \"{BOT}\"}}"), "chat_id must be"),
        (format!("{{\"token\": \"{BOT}\", \"chat_id\": \"\"}}"), "chat_id must be"),
        (format!("{{\"token\": \"{BOT}\", \"chat_id\": 1.5}}"), "chat_id must be"),
        (format!("{{\"token\": \"{BOT}\", \"chat_id\": true}}"), "chat_id must be"),
        (format!("{{\"token\": \"{BOT}\", \"chat_id\": null}}"), "chat_id must be"),
    ] {
        std::fs::write(&p, text).unwrap();
        token_refused(&p, &data, needle);
    }
    for bad in [
        "123456",
        ":made-up",
        "made:up",
        "123456:",
        "123456:made up",
        "123456:made/up",
        "123456:made:up",
        "12a456:made-up",
        "123456:made-upé",
    ] {
        std::fs::write(&p, format!("{{\"token\": \"{bad}\", \"chat_id\": 1}}")).unwrap();
        match creds::load_token(&p, &data) {
            Ok(t) => panic!("token {bad:?} accepted: {t:?}"),
            Err(e) => {
                assert!(e.contains("the token is not of the form"), "{e}");
                assert!(!e.contains(bad), "the error quotes the token: {e}");
            }
        }
    }
    std::fs::write(&p, &good).unwrap();
    assert!(creds::load_token(&p, &data).is_ok());

    // Outside the data directory, absolute, no `..` (again at the read).
    let inside = data.join("telegram.json");
    std::fs::write(&inside, &good).unwrap();
    chmod(&inside, 0o600);
    token_refused(
        &inside,
        &data,
        "[telegram] token_file must be outside the data directory",
    );
    token_refused(
        &etc.join("..").join("etc").join("telegram.json"),
        &data,
        "[telegram] token_file must not hold `..`",
    );
    token_refused(
        Path::new("telegram.json"),
        &data,
        "[telegram] token_file must be an absolute path",
    );
    let _ = std::fs::remove_dir_all(&dir);
}
