//! The file system behind the lake, injected so that tests can count calls,
//! inject faults and simulate crashes. Every path is relative to the lake
//! root; a path that leaves it is refused.

use std::fmt::Debug;
use std::fs::{self, File, OpenOptions, TryLockError};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

/// An open file of the lake.
pub trait LakeFile: Debug + Send {
    fn write_all(&mut self, data: &[u8]) -> io::Result<()>;
    /// fsync of the file's data.
    fn sync(&mut self) -> io::Result<()>;
    /// `LOCK_EX | LOCK_NB`: `Ok(false)` when another holder has it.
    fn try_lock(&mut self) -> io::Result<bool>;
    fn unlock(&mut self) -> io::Result<()>;
    fn len(&self) -> io::Result<u64>;
    fn is_empty(&self) -> io::Result<bool> {
        Ok(self.len()? == 0)
    }
    fn set_len(&mut self, n: u64) -> io::Result<()>;
    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> io::Result<usize>;
}

/// The operations the writer and the sealer use. Paths are lake-relative.
pub trait Fs: Debug + Send + Sync {
    /// Creates a new file (fails if it exists), with the parent directories.
    fn create_new(&self, path: &str) -> io::Result<Box<dyn LakeFile>>;
    /// Opens an existing file for reading and appending.
    fn open(&self, path: &str) -> io::Result<Box<dyn LakeFile>>;
    /// Links `from` to the new name `to`; fails if `to` exists (no replace).
    fn link(&self, from: &str, to: &str) -> io::Result<()>;
    /// Renames, replacing `to`.
    fn rename(&self, from: &str, to: &str) -> io::Result<()>;
    fn remove(&self, path: &str) -> io::Result<()>;
    fn exists(&self, path: &str) -> bool;
    /// fsync of a directory (after create, link, rename or remove in it).
    fn sync_dir(&self, dir: &str) -> io::Result<()>;
    /// Names of the entries of a directory, sorted; empty when it does not exist.
    fn list(&self, dir: &str) -> io::Result<Vec<String>>;
    /// Opens a directory to hold a lock on it (the per-venue seal lock).
    fn open_dir(&self, dir: &str) -> io::Result<Box<dyn LakeFile>>;
}

/// The parent directory of a lake-relative path.
pub fn parent(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(d, _)| d)
}

/// The real file system under `root`.
#[derive(Debug, Clone)]
pub struct RealFs {
    root: PathBuf,
}

impl RealFs {
    #[allow(clippy::new_ret_no_self)] // the lake holds it as `Arc<dyn Fs>`
    pub fn new(root: impl Into<PathBuf>) -> Arc<dyn Fs> {
        Arc::new(RealFs { root: root.into() })
    }

    fn at(&self, path: &str) -> io::Result<PathBuf> {
        let p = Path::new(path);
        if path.is_empty() {
            return Ok(self.root.clone());
        }
        let inside = p.components().all(|c| matches!(c, Component::Normal(_)));
        if !inside || path.contains('\\') {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                format!("{path:?} leaves the lake root"),
            ));
        }
        Ok(self.root.join(p))
    }
}

#[derive(Debug)]
struct RealFile(File);

impl LakeFile for RealFile {
    fn write_all(&mut self, data: &[u8]) -> io::Result<()> {
        self.0.seek(SeekFrom::End(0))?;
        Write::write_all(&mut self.0, data)
    }
    fn sync(&mut self) -> io::Result<()> {
        self.0.sync_all()
    }
    fn try_lock(&mut self) -> io::Result<bool> {
        match self.0.try_lock() {
            Ok(()) => Ok(true),
            Err(TryLockError::WouldBlock) => Ok(false),
            Err(TryLockError::Error(e)) => Err(e),
        }
    }
    fn unlock(&mut self) -> io::Result<()> {
        self.0.unlock()
    }
    fn len(&self) -> io::Result<u64> {
        Ok(self.0.metadata()?.len())
    }
    fn set_len(&mut self, n: u64) -> io::Result<()> {
        self.0.set_len(n)
    }
    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> io::Result<usize> {
        self.0.seek(SeekFrom::Start(offset))?;
        self.0.read(buf)
    }
}

impl Fs for RealFs {
    fn create_new(&self, path: &str) -> io::Result<Box<dyn LakeFile>> {
        let p = self.at(path)?;
        if let Some(d) = p.parent() {
            fs::create_dir_all(d)?;
        }
        let f = OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(p)?;
        Ok(Box::new(RealFile(f)))
    }
    fn open(&self, path: &str) -> io::Result<Box<dyn LakeFile>> {
        let f = OpenOptions::new()
            .read(true)
            .write(true)
            .open(self.at(path)?)?;
        Ok(Box::new(RealFile(f)))
    }
    fn link(&self, from: &str, to: &str) -> io::Result<()> {
        fs::hard_link(self.at(from)?, self.at(to)?)
    }
    fn rename(&self, from: &str, to: &str) -> io::Result<()> {
        let to_p = self.at(to)?;
        if let Some(d) = to_p.parent() {
            fs::create_dir_all(d)?;
        }
        fs::rename(self.at(from)?, to_p)
    }
    fn remove(&self, path: &str) -> io::Result<()> {
        fs::remove_file(self.at(path)?)
    }
    fn exists(&self, path: &str) -> bool {
        self.at(path).is_ok_and(|p| p.exists())
    }
    fn sync_dir(&self, dir: &str) -> io::Result<()> {
        File::open(self.at(dir)?)?.sync_all()
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
    fn open_dir(&self, dir: &str) -> io::Result<Box<dyn LakeFile>> {
        let p = self.at(dir)?;
        fs::create_dir_all(&p)?;
        Ok(Box::new(RealFile(File::open(p)?)))
    }
}
