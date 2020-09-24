use crate::common;
use failure::{format_err, Error};
use libmount::{mountinfo::Parser, Overlay};
use nix::mount::{umount2, MntFlags};
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};

pub trait LayerManager {
    /// Return the name of the layer manager, e.g. "overlay".
    /// This name should be the same as the fs_type listed in the /proc/<>/mountinfo file
    fn name() -> String
    where
        Self: Sized;
    /// Create a new layer manager from the given distribution directory
    /// dist: distribution directory, inst: instance name (not directory)
    fn from_inst_dir<P: AsRef<Path>>(
        dist_path: P,
        inst_path: P,
        inst_name: P,
    ) -> Result<Box<dyn LayerManager>, Error>
    where
        Self: Sized;
    /// Mount the filesystem to the given path
    fn mount(&mut self, to: &Path) -> Result<(), Error>;
    /// Return if the filesystem is mounted
    fn is_mounted(&self, target: &Path) -> Result<bool, Error>;
    /// Rollback the filesystem to the distribution state
    fn rollback(&mut self) -> Result<(), Error>;
    /// Commit the current state of the instance filesystem to the distribution state
    fn commit(&mut self) -> Result<(), Error>;
    /// Un-mount the filesystem
    fn unmount(&mut self, target: &Path) -> Result<(), Error>;
}

struct OverlayFS {
    base: PathBuf,
    lower: PathBuf,
    upper: PathBuf,
    work: PathBuf,
}

impl LayerManager for OverlayFS {
    fn name() -> String
    where
        Self: Sized,
    {
        "overlay".to_owned()
    }
    // The overlayfs structure inherited from older CIEL looks like this:
    // |- work: .ciel/container/instances/<inst_name>/diff.tmp/
    // |- upper: .ciel/container/instances/<inst_name>/diff/
    // |- lower: .ciel/container/instances/<inst_name>/local/
    // ||- lower (base): .ciel/container/dist/
    fn from_inst_dir<P: AsRef<Path>>(
        dist_path: P,
        inst_path: P,
        inst_name: P,
    ) -> Result<Box<dyn LayerManager>, Error>
    where
        Self: Sized,
    {
        let dist = dist_path.as_ref();
        let inst = inst_path.as_ref().join(inst_name.as_ref());
        Ok(Box::new(OverlayFS {
            base: dist.to_owned(),
            lower: inst.join("layers/local"),
            upper: inst.join("layers/diff"),
            work: inst.join("layers/diff.tmp"),
        }))
    }
    fn mount(&mut self, to: &Path) -> Result<(), Error> {
        let base_dirs = [self.lower.clone(), self.base.clone()];
        let overlay = Overlay::writable(
            // base_dirs variable contains the base and lower directories
            base_dirs.iter().map(|x| x.as_ref()),
            self.upper.clone(),
            self.work.clone(),
            to,
        );
        // create the directories if they don't exist (work directory may be missing)
        fs::create_dir_all(&self.work)?;
        fs::create_dir_all(&self.upper)?;
        // let's mount them
        overlay
            .mount()
            .or_else(|e| Err(format_err!("{}", e.to_string())))?;

        Ok(())
    }
    /// is_mounted: check if a path is a mountpoint with corresponding fs_type
    fn is_mounted(&self, target: &Path) -> Result<bool, Error> {
        return is_mounted(target, &OsStr::new("overlay"));
    }
    fn rollback(&mut self) -> Result<(), Error> {
        fs::remove_dir_all(&self.upper)?;
        fs::remove_dir_all(&self.work)?;
        fs::create_dir(&self.upper)?;

        Ok(())
    }
    fn commit(&mut self) -> Result<(), Error> {
        todo!()
    }
    fn unmount(&mut self, target: &Path) -> Result<(), Error> {
        umount2(target, MntFlags::MNT_DETACH)?;

        Ok(())
    }
}

/// is_mounted: check if a path is a mountpoint with corresponding fs_type
pub(crate) fn is_mounted(mountpoint: &Path, fs_type: &OsStr) -> Result<bool, Error> {
    let mountinfo_content: Vec<u8> = fs::read("/proc/self/mountinfo")?;
    let parser = Parser::new(&mountinfo_content);

    for mount in parser {
        let mount = mount?;
        if &mount.mount_point == mountpoint && mount.fstype == fs_type {
            return Ok(true);
        }
    }

    Ok(false)
}

/// A convenience function for getting a overlayfs type LayerManager
pub(crate) fn get_overlayfs_manager(
    inst_name: &str,
) -> Result<Box<dyn LayerManager>, Error> {
    OverlayFS::from_inst_dir(common::CIEL_DIST_DIR, common::CIEL_INST_DIR, inst_name)
}
