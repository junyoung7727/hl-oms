//! The lake as the archiver sees it: read only (final review F4). Every
//! file is opened without write access, and every operation that would
//! change the lake fails, so L0 and the audit results can be made read-only
//! for this process (systemd `ReadOnlyPaths=`).

use std::fs::{self, File};
use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use md_lake::fs::{Fs, LakeFile};

#[derive(Debug, Clone)]
pub struct ReadOnlyFs {
    root: PathBuf,
}

impl ReadOnlyFs {
    #[allow(clippy::new_ret_no_self)] // held as `Arc<dyn Fs>`, as md-lake's RealFs
    pub fn new(root: impl Into<PathBuf>) -> Arc<dyn Fs> {
        Arc::new(Self { root: root.into() })
    }

    fn at(&self, path: &str) -> io::Result<PathBuf> {
        if path.is_empty() {
            return Ok(self.root.clone());
        }
        let p = Path::new(path);
        if !p.components().all(|c| matches!(c, Component::Normal(_))) || path.contains('\\') {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                format!("{path:?} leaves the lake root"),
            ));
        }
        Ok(self.root.join(p))
    }
}

fn read_only<T>() -> io::Result<T> {
    Err(io::Error::new(
        io::ErrorKind::PermissionDenied,
        "md-archiver reads the lake only",
    ))
}

#[derive(Debug)]
struct ReadOnlyFile(File);

impl LakeFile for ReadOnlyFile {
    fn write_all(&mut self, _: &[u8]) -> io::Result<()> {
        read_only()
    }
    fn sync(&mut self) -> io::Result<()> {
        read_only()
    }
    fn try_lock(&mut self) -> io::Result<bool> {
        read_only()
    }
    fn unlock(&mut self) -> io::Result<()> {
        read_only()
    }
    fn len(&self) -> io::Result<u64> {
        Ok(self.0.metadata()?.len())
    }
    fn set_len(&mut self, _: u64) -> io::Result<()> {
        read_only()
    }
    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> io::Result<usize> {
        self.0.seek(SeekFrom::Start(offset))?;
        self.0.read(buf)
    }
}

impl Fs for ReadOnlyFs {
    fn create_new(&self, _: &str) -> io::Result<Box<dyn LakeFile>> {
        read_only()
    }
    fn open(&self, path: &str) -> io::Result<Box<dyn LakeFile>> {
        Ok(Box::new(ReadOnlyFile(File::open(self.at(path)?)?)))
    }
    fn link(&self, _: &str, _: &str) -> io::Result<()> {
        read_only()
    }
    fn rename(&self, _: &str, _: &str) -> io::Result<()> {
        read_only()
    }
    fn remove(&self, _: &str) -> io::Result<()> {
        read_only()
    }
    fn exists(&self, path: &str) -> bool {
        self.at(path).is_ok_and(|p| p.exists())
    }
    fn sync_dir(&self, _: &str) -> io::Result<()> {
        read_only()
    }
    fn list(&self, dir: &str) -> io::Result<Vec<String>> {
        let p = self.at(dir)?;
        if !p.exists() {
            return Ok(Vec::new());
        }
        let mut names: Vec<String> = fs::read_dir(p)?
            .map(|e| e.map(|e| e.file_name().to_string_lossy().into_owned()))
            .collect::<io::Result<_>>()?;
        names.sort();
        Ok(names)
    }
    fn open_dir(&self, _: &str) -> io::Result<Box<dyn LakeFile>> {
        read_only()
    }
}

/// `create_dir_all`, then an fsync of the parent of every directory made,
/// so a new folder survives a power loss like the files in it (final review
/// F5; as md-lake's writer).
pub fn create_dirs_synced(d: &Path) -> io::Result<()> {
    let missing = md_lake::fs::missing_dirs(d);
    fs::create_dir_all(d)?;
    for m in &missing {
        if let Some(parent) = m.parent() {
            File::open(parent)?.sync_all()?;
        }
    }
    Ok(())
}
