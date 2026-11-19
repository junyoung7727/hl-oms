//! An `Fs` over the real one whose file writes block while a gate is closed
//! (a slow disk), and a free-space value the test sets.
#![allow(dead_code)]

use std::io;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};

use md_lake::fs::{Fs, LakeFile};

#[derive(Debug, Default)]
pub struct Gate {
    closed: Mutex<bool>,
    cv: Condvar,
    /// Writes that waited at the gate.
    pub waited: AtomicU64,
}

impl Gate {
    pub fn close(&self) {
        *self.closed.lock().expect("gate") = true;
    }
    pub fn open(&self) {
        *self.closed.lock().expect("gate") = false;
        self.cv.notify_all();
    }
    fn pass(&self) {
        let mut c = self.closed.lock().expect("gate");
        if *c {
            self.waited.fetch_add(1, Ordering::SeqCst);
        }
        while *c {
            c = self.cv.wait(c).expect("gate");
        }
    }
}

#[derive(Debug)]
pub struct SlowFs {
    pub inner: Arc<dyn Fs>,
    pub gate: Arc<Gate>,
}

#[derive(Debug)]
struct SlowFile {
    inner: Box<dyn LakeFile>,
    gate: Arc<Gate>,
}

impl LakeFile for SlowFile {
    fn write_all(&mut self, data: &[u8]) -> io::Result<()> {
        self.gate.pass();
        self.inner.write_all(data)
    }
    fn sync(&mut self) -> io::Result<()> {
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
        self.inner.read_at(offset, buf)
    }
}

impl Fs for SlowFs {
    fn create_new(&self, path: &str) -> io::Result<Box<dyn LakeFile>> {
        Ok(Box::new(SlowFile {
            inner: self.inner.create_new(path)?,
            gate: self.gate.clone(),
        }))
    }
    fn open(&self, path: &str) -> io::Result<Box<dyn LakeFile>> {
        Ok(Box::new(SlowFile {
            inner: self.inner.open(path)?,
            gate: self.gate.clone(),
        }))
    }
    fn link(&self, from: &str, to: &str) -> io::Result<()> {
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

/// Free space the test sets.
#[derive(Debug)]
pub struct SetFree(pub AtomicU64, pub AtomicBool);

impl SetFree {
    pub fn new(bytes: u64) -> Arc<SetFree> {
        Arc::new(SetFree(AtomicU64::new(bytes), AtomicBool::new(false)))
    }
    pub fn set(&self, bytes: u64) {
        self.0.store(bytes, Ordering::SeqCst);
    }
}

impl md_collector::disk::FreeSpace for SetFree {
    fn free_bytes(&self) -> io::Result<u64> {
        self.1.store(true, Ordering::SeqCst);
        Ok(self.0.load(Ordering::SeqCst))
    }
}
