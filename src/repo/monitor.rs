use inotify::{Inotify, WatchMask};
use log::info;
use std::{
    fs::{self, File},
    io::{Read, Seek, Write},
    ops::{Deref, DerefMut},
    path::Path,
    sync::mpsc::{self, Receiver, Sender},
    thread::sleep,
    time::Duration,
};

use crate::Result;

use super::SimpleAptRepository;

struct FreshLockGuard(File);

impl FreshLockGuard {
    fn new(file: File) -> Result<Self> {
        fs3::FileExt::lock_exclusive(&file)?;
        Ok(Self(file))
    }
}

impl Deref for FreshLockGuard {
    type Target = File;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for FreshLockGuard {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl Drop for FreshLockGuard {
    fn drop(&mut self) {
        fs3::FileExt::unlock(&self.0).unwrap();
    }
}

/// A monitor thread to refresh repository automatically.
pub struct RepositoryRefreshMonitor {
    thread: std::thread::JoinHandle<Result<()>>,
    stop_handle: Sender<()>,
}

impl RepositoryRefreshMonitor {
    /// Starts a new repository refresh monitor.
    pub fn new(repo: SimpleAptRepository) -> Self {
        let (tx, rx) = mpsc::channel();
        let thread = std::thread::spawn(move || run_monitor(repo, rx));
        Self {
            thread,
            stop_handle: tx,
        }
    }

    /// Stops the monitor.
    pub fn stop(self) -> Result<()> {
        _ = self.stop_handle.send(());
        self.thread.join().unwrap()
    }
}

fn run_monitor(repo: SimpleAptRepository, stop_handle: Receiver<()>) -> Result<()> {
    // ensure lock exists
    let lock_path = repo.refresh_lock_file();
    if !Path::exists(&lock_path) {
        info!("Creating fresh lock file at {:?} ...", lock_path);
        if let Some(parent) = lock_path.parent() {
            if !parent.exists() {
                fs::create_dir_all(parent)?;
            }
        }
        File::create(&lock_path)?;
    }

    let mut inotify = Inotify::init()?;
    let mut buffer = [0u8; 1024];
    let mut ignore_next = false;
    inotify.watches().add(
        &lock_path,
        WatchMask::DELETE_SELF | WatchMask::CLOSE_WRITE | WatchMask::CREATE,
    )?;

    loop {
        match stop_handle.try_recv() {
            Ok(()) => return Ok(()),
            Err(mpsc::TryRecvError::Empty) => {}
            Err(mpsc::TryRecvError::Disconnected) => return Ok(()),
        }

        sleep(Duration::from_secs(1));
        match inotify.read_events(&mut buffer) {
            Ok(_) => {
                if ignore_next {
                    ignore_next = false;
                    continue;
                }
                refresh_once(&repo)?;
                ignore_next = true;
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => continue,
            Err(e) => return Err(e.into()),
        }
    }
}

fn refresh_once(repo: &SimpleAptRepository) -> Result<()> {
    let lock_file = repo.refresh_lock_file();
    let f = match File::options()
        .read(true)
        .write(true)
        .create(true)
        .open(&lock_file)
    {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => File::create(&lock_file)?,
        Err(e) => return Err(e.into()),
    };
    let mut f = FreshLockGuard::new(f)?;
    let mut buf = [0u8; 1];
    f.read_exact(&mut buf)?;
    if buf[0] != b'1' {
        repo.refresh()?;
        f.rewind()?;
        f.write_all("1".as_bytes())?;
    }

    Ok(())
}
