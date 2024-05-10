use anyhow::Result;
use fs3::FileExt;
use inotify::{Inotify, WatchMask};
use std::{
    fs::File,
    io::{Read, Seek, Write},
    ops::{Deref, DerefMut},
    path::Path,
    sync::mpsc::Receiver,
    thread::sleep,
    time::Duration,
};
use crate::info;
use console::style;

use super::refresh_repo;

const LOCK_FILE: &str = "debs/fresh.lock";

struct FreshLockGuard {
    inner: File,
}

impl FreshLockGuard {
    fn new(file: File) -> Result<Self> {
        file.lock_exclusive()?;

        Ok(Self { inner: file })
    }
}

impl Deref for FreshLockGuard {
    type Target = File;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl DerefMut for FreshLockGuard {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}

impl Drop for FreshLockGuard {
    fn drop(&mut self) {
        self.inner.unlock().ok();
    }
}

fn refresh_once(pool_path: &Path) -> Result<()> {
    let lock_file = pool_path.join(LOCK_FILE);
    let f = match File::options().read(true).write(true).open(&lock_file) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => File::create(&lock_file)?,
        Err(e) => return Err(e.into()),
    };
    let mut guarded = FreshLockGuard::new(f)?;
    let mut buf = [0u8; 1];
    guarded.read(&mut buf)?;
    if buf[0] != b'1' {
        refresh_repo(pool_path)?;
        guarded.rewind()?;
        guarded.write_all("1".as_bytes())?;
    }

    Ok(())
}

pub fn start_monitor(pool_path: &Path, stop_token: Receiver<()>) -> Result<()> {
    // ensure lock exists
    let lock_path  = pool_path.join(LOCK_FILE);
    if !Path::exists(&lock_path) {
        File::create(&lock_path)?;
        info!("Creating lock file at {}...", LOCK_FILE);
    }

    let mut inotify = Inotify::init()?;
    let mut buffer = [0u8; 1024];
    let mut ignore_next = false;
    inotify.watches().add(
        &lock_path,
        WatchMask::DELETE_SELF | WatchMask::CLOSE_WRITE | WatchMask::CREATE,
    )?;

    loop {
        if stop_token.try_recv().is_ok() {
            return Ok(());
        }
        sleep(Duration::from_secs(1));
        match inotify.read_events(&mut buffer) {
            Ok(_) => {
                if ignore_next {
                    ignore_next = false;
                    continue;
                }
                refresh_once(pool_path).ok();
                ignore_next = true;
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => continue,
            Err(e) => return Err(e.into()),
        }
    }
}
