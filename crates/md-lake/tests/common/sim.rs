//! `SimFs` (review F10): an in-memory file system with durable and volatile
//! state. File bytes become durable at the file's fsync, directory entries
//! at the directory's fsync. `fail_after(n)` makes the n-th mutating call
//! fail and every later call fail too (the process died); `reboot(seed)`
//! keeps the durable state plus, per file, a random prefix of the bytes
//! appended since its last fsync (a torn write), and frees every lock.

use std::collections::BTreeMap;
use std::io;
use std::sync::{Arc, Mutex};

use md_lake::fs::{Fs, LakeFile};

#[derive(Debug, Default, Clone)]
struct Inode {
    durable: Vec<u8>,
    volatile: Vec<u8>,
    lock: Option<u64>,
}

#[derive(Debug, Default)]
struct State {
    inodes: BTreeMap<u64, Inode>,
    /// path → inode, as the program sees it.
    names: BTreeMap<String, u64>,
    /// path → inode, as a reboot would find it.
    durable_names: BTreeMap<String, u64>,
    next: u64,
    calls: usize,
    fail_at: Option<usize>,
    dead: bool,
}

#[derive(Debug, Clone, Default)]
pub struct SimFs(Arc<Mutex<State>>);

fn crash() -> io::Error {
    io::Error::other("simulated crash")
}

impl SimFs {
    pub fn new() -> (SimFs, Arc<dyn Fs>) {
        let s = SimFs::default();
        let fs: Arc<dyn Fs> = Arc::new(s.clone());
        (s, fs)
    }

    /// The `n`-th mutating call from now fails, and the process is dead after it.
    pub fn fail_after(&self, n: usize) {
        let mut s = self.0.lock().expect("sim");
        s.calls = 0;
        s.fail_at = Some(n);
    }

    /// Mutating calls made since `fail_after` (to know how many steps exist).
    pub fn calls(&self) -> usize {
        self.0.lock().expect("sim").calls
    }

    pub fn reboot(&self, seed: u64) {
        let mut s = self.0.lock().expect("sim");
        let mut rng = seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        let mut next = || {
            rng ^= rng << 13;
            rng ^= rng >> 7;
            rng ^= rng << 17;
            rng
        };
        for ino in s.inodes.values_mut() {
            let keep = if ino.volatile.len() > ino.durable.len()
                && ino.volatile.starts_with(&ino.durable)
            {
                let extra = ino.volatile.len() - ino.durable.len();
                ino.durable.len() + (next() as usize) % (extra + 1)
            } else {
                ino.durable.len()
            };
            if keep > ino.durable.len() {
                ino.durable = ino.volatile[..keep].to_vec();
            }
            ino.volatile = ino.durable.clone();
            ino.lock = None;
        }
        s.names = s.durable_names.clone();
        s.fail_at = None;
        s.dead = false;
    }

    fn step(s: &mut State) -> io::Result<()> {
        if s.dead {
            return Err(crash());
        }
        s.calls += 1;
        if s.fail_at == Some(s.calls) {
            s.dead = true;
            return Err(crash());
        }
        Ok(())
    }

    fn check_path(path: &str) -> io::Result<()> {
        if path.starts_with('/') || path.split('/').any(|c| c == ".." || c.is_empty()) {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "outside the lake",
            ));
        }
        Ok(())
    }
}

#[derive(Debug)]
struct SimFile {
    fs: SimFs,
    ino: u64,
    handle: u64,
}

impl Drop for SimFile {
    fn drop(&mut self) {
        let mut s = self.fs.0.lock().expect("sim");
        if let Some(i) = s.inodes.get_mut(&self.ino) {
            if i.lock == Some(self.handle) {
                i.lock = None;
            }
        }
    }
}

impl LakeFile for SimFile {
    fn write_all(&mut self, data: &[u8]) -> io::Result<()> {
        let mut s = self.fs.0.lock().expect("sim");
        SimFs::step(&mut s)?;
        let i = s.inodes.get_mut(&self.ino).ok_or_else(crash)?;
        i.volatile.extend_from_slice(data);
        Ok(())
    }
    fn sync(&mut self) -> io::Result<()> {
        let mut s = self.fs.0.lock().expect("sim");
        SimFs::step(&mut s)?;
        let i = s.inodes.get_mut(&self.ino).ok_or_else(crash)?;
        i.durable = i.volatile.clone();
        Ok(())
    }
    fn try_lock(&mut self) -> io::Result<bool> {
        let mut s = self.fs.0.lock().expect("sim");
        if s.dead {
            return Err(crash());
        }
        let i = s.inodes.get_mut(&self.ino).ok_or_else(crash)?;
        match i.lock {
            Some(h) if h != self.handle => Ok(false),
            _ => {
                i.lock = Some(self.handle);
                Ok(true)
            }
        }
    }
    fn unlock(&mut self) -> io::Result<()> {
        let mut s = self.fs.0.lock().expect("sim");
        if let Some(i) = s.inodes.get_mut(&self.ino) {
            if i.lock == Some(self.handle) {
                i.lock = None;
            }
        }
        Ok(())
    }
    fn len(&self) -> io::Result<u64> {
        let s = self.fs.0.lock().expect("sim");
        Ok(s.inodes
            .get(&self.ino)
            .map_or(0, |i| i.volatile.len() as u64))
    }
    fn set_len(&mut self, n: u64) -> io::Result<()> {
        let mut s = self.fs.0.lock().expect("sim");
        SimFs::step(&mut s)?;
        let i = s.inodes.get_mut(&self.ino).ok_or_else(crash)?;
        i.volatile.resize(n as usize, 0);
        Ok(())
    }
    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> io::Result<usize> {
        let s = self.fs.0.lock().expect("sim");
        if s.dead {
            return Err(crash());
        }
        let i = s.inodes.get(&self.ino).ok_or_else(crash)?;
        let at = (offset as usize).min(i.volatile.len());
        let n = buf.len().min(i.volatile.len() - at);
        buf[..n].copy_from_slice(&i.volatile[at..at + n]);
        Ok(n)
    }
}

impl SimFs {
    fn handle(&self, ino: u64) -> Box<dyn LakeFile> {
        let mut s = self.0.lock().expect("sim");
        s.next += 1;
        let handle = s.next;
        Box::new(SimFile {
            fs: self.clone(),
            ino,
            handle,
        })
    }

    /// Makes everything durable, as if every writer had called fsync.
    pub fn persist_all(&self) {
        let mut s = self.0.lock().expect("sim");
        for i in s.inodes.values_mut() {
            i.durable = i.volatile.clone();
        }
        s.durable_names = s.names.clone();
    }

    /// Every path the program sees (for asserts).
    pub fn files(&self) -> Vec<String> {
        self.0.lock().expect("sim").names.keys().cloned().collect()
    }
}

impl Fs for SimFs {
    fn create_new(&self, path: &str) -> io::Result<Box<dyn LakeFile>> {
        SimFs::check_path(path)?;
        let ino = {
            let mut s = self.0.lock().expect("sim");
            SimFs::step(&mut s)?;
            if s.names.contains_key(path) {
                return Err(io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    path.to_string(),
                ));
            }
            s.next += 1;
            let ino = s.next;
            s.inodes.insert(ino, Inode::default());
            s.names.insert(path.to_string(), ino);
            ino
        };
        Ok(self.handle(ino))
    }
    fn open(&self, path: &str) -> io::Result<Box<dyn LakeFile>> {
        let ino = {
            let s = self.0.lock().expect("sim");
            if s.dead {
                return Err(crash());
            }
            *s.names
                .get(path)
                .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, path.to_string()))?
        };
        Ok(self.handle(ino))
    }
    fn link(&self, from: &str, to: &str) -> io::Result<()> {
        let mut s = self.0.lock().expect("sim");
        SimFs::step(&mut s)?;
        if s.names.contains_key(to) {
            return Err(io::Error::new(io::ErrorKind::AlreadyExists, to.to_string()));
        }
        let ino = *s
            .names
            .get(from)
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, from.to_string()))?;
        s.names.insert(to.to_string(), ino);
        Ok(())
    }
    fn rename(&self, from: &str, to: &str) -> io::Result<()> {
        let mut s = self.0.lock().expect("sim");
        SimFs::step(&mut s)?;
        let ino = s
            .names
            .remove(from)
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, from.to_string()))?;
        s.names.insert(to.to_string(), ino);
        Ok(())
    }
    fn remove(&self, path: &str) -> io::Result<()> {
        let mut s = self.0.lock().expect("sim");
        SimFs::step(&mut s)?;
        s.names
            .remove(path)
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, path.to_string()))?;
        Ok(())
    }
    fn exists(&self, path: &str) -> bool {
        self.0.lock().expect("sim").names.contains_key(path)
    }
    fn sync_dir(&self, dir: &str) -> io::Result<()> {
        let mut s = self.0.lock().expect("sim");
        SimFs::step(&mut s)?;
        let in_dir = |p: &str| p.rsplit_once('/').map_or("", |(d, _)| d) == dir;
        let now: Vec<(String, u64)> = s
            .names
            .iter()
            .filter(|(p, _)| in_dir(p))
            .map(|(p, i)| (p.clone(), *i))
            .collect();
        s.durable_names.retain(|p, _| !in_dir(p));
        s.durable_names.extend(now);
        Ok(())
    }
    fn list(&self, dir: &str) -> io::Result<Vec<String>> {
        let s = self.0.lock().expect("sim");
        if s.dead {
            return Err(crash());
        }
        let prefix = format!("{dir}/");
        let mut out: Vec<String> = s
            .names
            .keys()
            .filter_map(|p| p.strip_prefix(&prefix))
            .filter(|rest| !rest.contains('/') && !rest.contains('\u{0}'))
            .map(str::to_owned)
            .collect();
        out.sort();
        Ok(out)
    }
    fn open_dir(&self, dir: &str) -> io::Result<Box<dyn LakeFile>> {
        // A directory lock is an inode of its own, keyed by the name.
        let key = format!("{dir}/\u{0}dirlock");
        let ino = {
            let mut s = self.0.lock().expect("sim");
            if s.dead {
                return Err(crash());
            }
            match s.names.get(&key) {
                Some(i) => *i,
                None => {
                    s.next += 1;
                    let ino = s.next;
                    s.inodes.insert(ino, Inode::default());
                    s.names.insert(key.clone(), ino);
                    s.durable_names.insert(key, ino);
                    ino
                }
            }
        };
        Ok(self.handle(ino))
    }
}
