//! A real file system that counts calls and checks the lock rule at link.

use std::io;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use md_lake::fs::{Fs, LakeFile, RealFs};

#[derive(Debug, Default)]
pub struct Counts {
    pub writes: AtomicUsize,
    pub syncs: AtomicUsize,
    pub links: AtomicUsize,
    /// Each link: whether the source was locked at that moment.
    pub link_locked: Mutex<Vec<bool>>,
    pub reads: Mutex<Vec<String>>,
}

#[derive(Debug)]
pub struct CountingFs {
    inner: Arc<dyn Fs>,
    pub counts: Arc<Counts>,
}

impl CountingFs {
    pub fn open_at(root: &std::path::Path) -> (Arc<dyn Fs>, Arc<Counts>) {
        let counts = Arc::new(Counts::default());
        let fs = CountingFs {
            inner: RealFs::new(root),
            counts: counts.clone(),
        };
        (Arc::new(fs), counts)
    }
}

#[derive(Debug)]
struct CountingFile {
    inner: Box<dyn LakeFile>,
    path: String,
    counts: Arc<Counts>,
}

impl LakeFile for CountingFile {
    fn write_all(&mut self, data: &[u8]) -> io::Result<()> {
        self.counts.writes.fetch_add(1, Ordering::SeqCst);
        self.inner.write_all(data)
    }
    fn sync(&mut self) -> io::Result<()> {
        self.counts.syncs.fetch_add(1, Ordering::SeqCst);
        self.inner.sync()
    }
    fn try_lock(&mut self) -> io::Result<bool> {
        self.inner.try_lock()
    }
    fn unlock(&mut self) -> io::Result<()> {
        self.inner.unlock()
    }
    fn len(&self) -> io::Result<u64> {
        self.inner.len()
    }
    fn set_len(&mut self, n: u64) -> io::Result<()> {
        self.inner.set_len(n)
    }
    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> io::Result<usize> {
        self.counts
            .reads
            .lock()
            .expect("reads")
            .push(self.path.clone());
        self.inner.read_at(offset, buf)
    }
}

impl Fs for CountingFs {
    fn create_new(&self, path: &str) -> io::Result<Box<dyn LakeFile>> {
        let inner = self.inner.create_new(path)?;
        Ok(Box::new(CountingFile {
            inner,
            path: path.into(),
            counts: self.counts.clone(),
        }))
    }
    fn open(&self, path: &str) -> io::Result<Box<dyn LakeFile>> {
        let inner = self.inner.open(path)?;
        Ok(Box::new(CountingFile {
            inner,
            path: path.into(),
            counts: self.counts.clone(),
        }))
    }
    fn link(&self, from: &str, to: &str) -> io::Result<()> {
        // A second open file description can take the lock only if the
        // writer has not taken it yet.
        let free = self.inner.open(from)?.try_lock()?;
        self.counts
            .link_locked
            .lock()
            .expect("lock log")
            .push(!free);
        self.counts.links.fetch_add(1, Ordering::SeqCst);
        self.inner.link(from, to)
    }
    fn rename(&self, from: &str, to: &str) -> io::Result<()> {
        self.inner.rename(from, to)
    }
    fn remove(&self, path: &str) -> io::Result<()> {
        self.inner.remove(path)
    }
    fn exists(&self, path: &str) -> bool {
        self.inner.exists(path)
    }
    fn sync_dir(&self, dir: &str) -> io::Result<()> {
        self.inner.sync_dir(dir)
    }
    fn list(&self, dir: &str) -> io::Result<Vec<String>> {
        self.inner.list(dir)
    }
    fn open_dir(&self, dir: &str) -> io::Result<Box<dyn LakeFile>> {
        self.inner.open_dir(dir)
    }
}
