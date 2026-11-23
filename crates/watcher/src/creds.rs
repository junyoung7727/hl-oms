//! The watcher's credentials file (C-WATCH part 4): TOML `api_key` and
//! `api_secret`, both non-empty, nothing else. The file is a regular file
//! (not a symlink), owned by the user the watcher runs as, mode 0600 or
//! 0400, outside the OMS data directory. No error quotes the file, and
//! the secret is printed nowhere ([`Credentials`]'s `Debug` hides it).
//!
//! The token file of `[telegram]` (U19b1a, C-WATCH part 10) follows the
//! same file rules, is not inside a git repository, and holds the JSON of
//! the PR #3 format ([`load_token`]).

use std::io::Read;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::alert::Token;
use crate::config::{check_credentials_path, check_token_path, resolved};
use crate::rest::Credentials;

const CREDENTIALS: &str = "credentials file";
const TOKEN: &str = "token file";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    api_key: String,
    api_secret: String,
}

/// Reads the credentials file `path` of a watcher of `data_dir`. The path
/// rules of the config are applied again here. The checks and the read
/// use one open handle, so the file cannot be swapped between them; the
/// open does not follow a symlink and does not wait for a FIFO's writer.
pub fn load(path: &Path, data_dir: &Path) -> Result<Credentials, String> {
    check_credentials_path(path, data_dir)?;
    let shown = path.display();
    let mut file = open_checked(CREDENTIALS, path)?;
    let mut text = String::new();
    file.read_to_string(&mut text)
        .map_err(|e| format!("credentials file {shown}: {e}"))?;
    // The TOML error would quote the line, which may hold the secret.
    let f: File = toml::from_str(&text).map_err(|_| {
        format!("credentials file {shown} is not TOML with api_key and api_secret and nothing else")
    })?;
    if f.api_key.is_empty() || f.api_secret.is_empty() {
        return Err(format!("credentials file {shown}: empty key or secret"));
    }
    Ok(Credentials::new(&f.api_key, &f.api_secret))
}

/// Reads the token file `path` of `[telegram]` (U19b1a): the path rules
/// of the config again, the file rules of the credentials file on one
/// open handle, not inside a git repository (an ancestor directory of the
/// path, or of where it points, holds `.git`), then JSON with `token`
/// (`<digits>:<[A-Za-z0-9_-]+>`) and `chat_id` (a non-empty string or an
/// integer); other keys are left alone, so the OMS's file of the same
/// format may be shared. The repository rule comes before the read. No
/// error quotes the file.
pub fn load_token(path: &Path, data_dir: &Path) -> Result<Token, String> {
    check_token_path(path, data_dir)?;
    let shown = path.display();
    let mut file = open_checked(TOKEN, path)?;
    if let Some(repo) = repository_of(path) {
        return Err(format!(
            "token file {shown} is inside the git repository {}; keep the token outside any repository",
            repo.display()
        ));
    }
    let mut text = String::new();
    file.read_to_string(&mut text)
        .map_err(|e| format!("token file {shown}: {e}"))?;
    // The JSON error would quote the text, which may hold the token.
    let v = serde_json::from_str::<serde_json::Value>(&text)
        .ok()
        .filter(serde_json::Value::is_object)
        .ok_or_else(|| format!("token file {shown} is not JSON with token and chat_id"))?;
    let token = v["token"].as_str().unwrap_or_default();
    let chat = match &v["chat_id"] {
        serde_json::Value::String(c) => c.clone(),
        serde_json::Value::Number(n) if n.is_i64() || n.is_u64() => n.to_string(),
        _ => String::new(),
    };
    Token::new(token, &chat).map_err(|e| format!("token file {shown}: {e}"))
}

/// The first ancestor directory of `path`, as written or as resolved
/// (symlinks followed), that holds `.git` (a directory, or a file in a
/// worktree).
fn repository_of(path: &Path) -> Option<PathBuf> {
    [path.to_path_buf(), resolved(path)].into_iter().find_map(|p| {
        p.ancestors()
            .skip(1)
            .find(|d| std::fs::symlink_metadata(d.join(".git")).is_ok())
            .map(Path::to_path_buf)
    })
}

/// Opens `path` for reading without following a symlink at its last
/// component and without waiting for a FIFO's writer, and applies the
/// file rules to that handle's metadata.
fn open_checked(what: &str, path: &Path) -> Result<std::fs::File, String> {
    let shown = path.display();
    let file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags((rustix::fs::OFlags::NOFOLLOW | rustix::fs::OFlags::NONBLOCK).bits() as i32)
        .open(path)
        .map_err(|e| {
            if e.raw_os_error() == Some(rustix::io::Errno::LOOP.raw_os_error()) {
                format!("{what} {shown} is a symbolic link")
            } else {
                format!("{what} {shown}: {e}")
            }
        })?;
    let meta = file.metadata().map_err(|e| format!("{what} {shown}: {e}"))?;
    rules(
        what,
        path,
        meta.file_type().is_file(),
        meta.uid(),
        meta.mode(),
        rustix::process::geteuid().as_raw(),
    )?;
    Ok(file)
}

/// The rules on the opened file's metadata: a regular file, owned by
/// `euid` (the user the watcher runs as), mode 0600 or 0400 (no set-id or
/// sticky bit). A function of its own so that a test can give another
/// owner, which only root could make.
pub fn file_rules(
    path: &Path,
    is_file: bool,
    uid: u32,
    mode: u32,
    euid: u32,
) -> Result<(), String> {
    rules(CREDENTIALS, path, is_file, uid, mode, euid)
}

/// [`file_rules`] for the token file (U19b1a).
pub fn token_file_rules(
    path: &Path,
    is_file: bool,
    uid: u32,
    mode: u32,
    euid: u32,
) -> Result<(), String> {
    rules(TOKEN, path, is_file, uid, mode, euid)
}

fn rules(
    what: &str,
    path: &Path,
    is_file: bool,
    uid: u32,
    mode: u32,
    euid: u32,
) -> Result<(), String> {
    let shown = path.display();
    if !is_file {
        return Err(format!("{what} {shown} is not a regular file"));
    }
    if uid != euid {
        return Err(format!(
            "{what} {shown} must be owned by the user the watcher runs as"
        ));
    }
    let perm = mode & 0o7777;
    if perm != 0o600 && perm != 0o400 {
        return Err(format!(
            "{what} {shown} must have mode 0600 or 0400, not {perm:04o} (chmod 600)"
        ));
    }
    Ok(())
}
