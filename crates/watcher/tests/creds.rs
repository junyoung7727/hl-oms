//! The watcher's credentials file (C-WATCH part 4): a regular file (not a
//! symlink), owned by this process's uid, mode 0600 or 0400, outside the
//! OMS data directory, with a non-empty `api_key` and `api_secret`; no
//! error quotes the file and `Debug` never prints the secret.

#![allow(clippy::unwrap_used)]

mod common;

use std::os::unix::fs::PermissionsExt;
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
