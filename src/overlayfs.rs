use failure::{format_err, Error};
use libmount::{Overlay, mountinfo::Parser};
use nix::mount::{umount2, MntFlags};
use std::path::{Path, PathBuf};
use std::ffi::OsStr;
use std::fs;

pub trait LayerManager {
    /// Return the name of the layer manager, e.g. "overlayfs"
    fn name() -> String where Self: Sized;
    /// Create a new layer manager from the given distribution directory
    fn from_inst_dir<P: AsRef<Path>>(path: P) -> Result<Box<dyn LayerManager>, Error> where Self: Sized;
    /// Mount the filesystem to the given path
    fn mount(&mut self, to: &Path) -> Result<(), Error>;
    /// Return if the filesystem is mounted
    fn is_mounted(&self) -> Result<bool, Error>;
    /// Rollback the filesystem to the distribution state
    fn rollback(&mut self) -> Result<(), Error>;
    /// Commit the current state of the instance filesystem to the distribution state
    fn commit(&mut self) -> Result<(), Error>;
    /// Un-mount the filesystem
    fn unmount(&mut self) -> Result<(), Error>;
}

/// is_mounted: check if a path is a mountpoint with corresponding fs_type
pub(crate) fn is_mounted(mountpoint: PathBuf, fs_type: &OsStr) -> Result<bool, Error> {
    let mountinfo_content: Vec<u8> = fs::read("/proc/self/mountinfo")?;
    let parser = Parser::new(&mountinfo_content);

    for mount in parser {
        let mount = mount?;
        if mount.mount_point == mountpoint && mount.fstype == fs_type {
            return Ok(true);
        }
    }

    Ok(false)
}

pub fn mount_overlay<P: AsRef<Path>>(
    base: PathBuf,
    lower: PathBuf,
    upper: P,
    work: P,
    target: P,
) -> Result<(), Error> {
    let base_dirs = [lower, base];
    let overlay = Overlay::writable(base_dirs.iter().map(|x| x.as_ref()), upper, work, target);
    overlay
        .mount()
        .or_else(|e| Err(format_err!("{}", e.to_string())))?;

    Ok(())
}

pub fn unmount(to: &Path) -> Result<(), Error> {
    umount2(to, MntFlags::MNT_DETACH)?;

    Ok(())
}
