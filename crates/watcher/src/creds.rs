//! The watcher's credentials file (C-WATCH part 4): TOML `api_key` and
//! `api_secret`, both non-empty, nothing else. The file is a regular file
//! (not a symlink), owned by the user the watcher runs as, mode 0600 or
//! 0400, outside the OMS data directory. No error quotes the file, and
//! the secret is printed nowhere ([`Credentials`]'s `Debug` hides it).

use std::io::Read;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::Path;

use serde::Deserialize;

use crate::config::check_credentials_path;
use crate::rest::Credentials;

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
    let mut file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags((rustix::fs::OFlags::NOFOLLOW | rustix::fs::OFlags::NONBLOCK).bits() as i32)
        .open(path)
        .map_err(|e| {
            if e.raw_os_error() == Some(rustix::io::Errno::LOOP.raw_os_error()) {
                format!("credentials file {shown} is a symbolic link")
            } else {
                format!("credentials file {shown}: {e}")
            }
        })?;
    let meta = file
        .metadata()
        .map_err(|e| format!("credentials file {shown}: {e}"))?;
    file_rules(
        path,
        meta.file_type().is_file(),
        meta.uid(),
        meta.mode(),
        rustix::process::geteuid().as_raw(),
    )?;
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
    let shown = path.display();
    if !is_file {
        return Err(format!("credentials file {shown} is not a regular file"));
    }
    if uid != euid {
        return Err(format!(
            "credentials file {shown} must be owned by the user the watcher runs as"
        ));
    }
    let perm = mode & 0o7777;
    if perm != 0o600 && perm != 0o400 {
        return Err(format!(
            "credentials file {shown} must have mode 0600 or 0400, not {perm:04o} (chmod 600)"
        ));
    }
    Ok(())
}
