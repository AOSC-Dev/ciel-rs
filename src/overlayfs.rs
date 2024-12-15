use crate::config::{InstanceConfig, TmpfsConfig};
use crate::{common, info};
use anyhow::{anyhow, bail, Context, Result};
use libmount::{mountinfo::Parser, Overlay, Tmpfs};
use nix::mount::{umount2, MntFlags};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::{
    ffi::OsStr,
    io::{BufRead, BufReader},
};
use std::{fs, path};

pub trait LayerManager {
    /// Return the name of the layer manager, e.g. "overlay".
    /// This name should be the same as the fs_type listed in the /proc/<>/mountinfo file
    fn name() -> String
    where
        Self: Sized;
    /// Create a new layer manager from the given distribution directory
    /// dist: distribution directory, inst: instance name (not directory)
    fn from_inst_dir<P: AsRef<Path>, S: AsRef<str>>(
        dist_path: P,
        inst_path: P,
        inst_name: S,
    ) -> Result<Box<dyn LayerManager>>
    where
        Self: Sized;
    /// Mount the filesystem to the given path
    fn mount(&mut self, to: &Path) -> Result<()>;
    /// Return if the filesystem is mounted
    fn is_mounted(&self, target: &Path) -> Result<bool>;
    /// Return if the filesystem uses tmpfs for upper layer.
    fn is_tmpfs(&self) -> bool;
    /// Return if tmpfs is mounted.
    fn is_tmpfs_mounted(&self) -> Result<bool>;
    /// Rollback the filesystem to the distribution state
    fn rollback(&mut self) -> Result<()>;
    /// Commit the current state of the instance filesystem to the distribution state
    fn commit(&mut self) -> Result<()>;
    /// Un-mount the filesystem
    fn unmount(&mut self, target: &Path) -> Result<()>;
    /// Un-mount tmpfs.
    fn unmount_tmpfs(&self) -> Result<()>;
    /// Return the directory where the configuration layer is located
    /// You may temporary mount this directory if your backend does not expose this directory directly
    fn get_config_layer(&mut self) -> Result<PathBuf>;
    /// Return the directory where the base layer is located
    fn get_base_layer(&mut self) -> Result<PathBuf>;
    /// Set the volatile state of the instance filesystem
    fn set_volatile(&mut self, volatile: bool) -> Result<()>;
    /// Destroy the filesystem of the current instance
    fn destroy(&mut self) -> Result<()>;
}

struct OverlayFS {
    inst: PathBuf,
    base: PathBuf,
    lower: PathBuf,
    upper: PathBuf,
    work: PathBuf,
    volatile: bool,
    tmpfs: Option<(PathBuf, TmpfsConfig)>,
}

/// Create a new overlay filesystem on the host system
pub fn create_new_instance_fs<P: AsRef<Path>>(
    inst_path: P,
    inst_name: P,
    tmpfs: bool,
) -> Result<()> {
    let inst = inst_path.as_ref().join(inst_name.as_ref());
    fs::create_dir_all(&inst)?;
    if tmpfs {
        fs::create_dir_all(inst.join("layers/tmpfs"))?;
    }
    Ok(())
}

/// OverlayFS operations
#[derive(Debug)]
enum Diff {
    Symlink(PathBuf),
    OverrideDir(PathBuf),
    RenamedDir(PathBuf, PathBuf),
    NewDir(PathBuf),
    ModifiedDir(PathBuf),  // Modify permission only
    WhiteoutFile(PathBuf), // Dir or File
    File(PathBuf),         // Simple modified or new file
}

impl OverlayFS {
    /// Generate a list of changes made in the upper layer
    fn diff(&self) -> Result<Vec<Diff>> {
        let mut mods: Vec<Diff> = Vec::new();
        let mut processed_dirs: Vec<PathBuf> = Vec::new();

        for entry in walkdir::WalkDir::new(&self.upper).into_iter().skip(1) {
            // SKip the root
            let path: PathBuf = entry?.path().to_path_buf();
            let rel_path = path.strip_prefix(&self.upper)?.to_path_buf();
            let lower_path = self.lower.join(&rel_path).to_path_buf();

            if has_prefix(&rel_path, &processed_dirs) {
                continue; // We already dealt with it
            }
            let meta = fs::symlink_metadata(&path)?;
            let file_type = meta.file_type();

            if file_type.is_symlink() {
                // Just move the symlink
                mods.push(Diff::Symlink(rel_path.clone()));
            } else if meta.is_dir() {
                // Deal with dirs
                let opaque = xattr::get(&path, "trusted.overlay.opaque")?;
                let redirect = xattr::get(&path, "trusted.overlay.redirect")?;
                let metacopy = xattr::get(&path, "trusted.overlay.metacopy")?;

                if let Some(_data) = metacopy {
                    bail!("Unsupported filesystem feature: metacopy");
                }
                if let Some(text) = opaque {
                    // the new dir (completely) replace the old one
                    if text == b"y" {
                        // Delete corresponding dir
                        mods.push(Diff::OverrideDir(rel_path.clone()));
                        processed_dirs.push(rel_path.clone());
                    }
                } else if let Some(from_utf8) = redirect {
                    // Renamed
                    let mut from_rel_path = PathBuf::from(OsStr::from_bytes(&from_utf8));
                    if from_rel_path.is_absolute() {
                        // abs path from root of OverlayFS
                        from_rel_path = from_rel_path.strip_prefix("/")?.to_path_buf();
                    } else {
                        // rel path, same parent dir as the origin
                        let mut from_path = path.clone();
                        from_path.pop();
                        from_path.push(PathBuf::from(&from_rel_path));
                        from_rel_path = from_path.strip_prefix(&self.upper)?.to_path_buf();
                    }
                    mods.push(Diff::RenamedDir(from_rel_path, rel_path));
                } else if !lower_path.is_dir() {
                    // New dir
                    mods.push(Diff::NewDir(rel_path.clone()));
                } else {
                    // Modified
                    mods.push(Diff::ModifiedDir(rel_path.clone()));
                }
            } else {
                // Deal with files
                if file_type.is_char_device() && meta.rdev() == 0 {
                    // Whiteout file!
                    mods.push(Diff::WhiteoutFile(rel_path.clone()));
                } else if lower_path.is_dir() {
                    // A new file overrides an old directory
                    mods.push(Diff::OverrideDir(rel_path.clone()));
                } else {
                    mods.push(Diff::File(rel_path.clone()));
                }
            }
        }

        Ok(mods)
    }
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
    fn from_inst_dir<P: AsRef<Path>, S: AsRef<str>>(
        dist_path: P,
        inst_path: P,
        inst_name: S,
    ) -> Result<Box<dyn LayerManager>>
    where
        Self: Sized,
    {
        let dist = dist_path.as_ref();
        let inst = inst_path.as_ref().join(inst_name.as_ref());
        let instance_config = InstanceConfig::load(inst_name)?;

        if let Some(tmpfs) = instance_config.tmpfs {
            Ok(Box::new(OverlayFS {
                inst: inst.to_owned(),
                base: dist.to_owned(),
                lower: inst.join("layers/local"),
                upper: inst.join("layers/tmpfs/upper"),
                work: inst.join("layers/tmpfs/work"),
                volatile: false,
                tmpfs: Some((inst.join("layers/tmpfs"), tmpfs)),
            }))
        } else {
            Ok(Box::new(OverlayFS {
                inst: inst.to_owned(),
                base: dist.to_owned(),
                lower: inst.join("layers/local"),
                upper: inst.join("layers/diff"),
                work: inst.join("layers/diff.tmp"),
                volatile: false,
                tmpfs: None,
            }))
        }
    }

    fn mount(&mut self, to: &Path) -> Result<()> {
        let base_dirs = [self.lower.clone(), self.base.clone()];

        // mount tmpfs if needed
        if let Some((tmpfs, tmpfs_config)) = &self.tmpfs {
            fs::create_dir_all(&tmpfs)?;
            if !self.is_tmpfs_mounted()? {
                let tmpfs = Tmpfs::new(tmpfs).size_bytes(tmpfs_config.size_bytes());
                tmpfs
                    .mount()
                    .map_err(|e| anyhow!("failed to mount tmpfs: {}", e.to_string()))?;
            }
        }

        // create the directories if they don't exist (work directory may be missing)
        fs::create_dir_all(&self.upper)?;
        fs::create_dir_all(&self.work)?;
        fs::create_dir_all(&self.lower)?;

        let mut overlay = Overlay::writable(
            // base_dirs variable contains the base and lower directories
            base_dirs.iter().map(|x| x.as_ref()),
            self.upper.clone(),
            self.work.clone(),
            to,
        );
        // check overlay usability
        load_overlayfs_support()?;
        if self.volatile {
            overlay.set_options(b"volatile".to_vec());
        }
        let dirty_flag = self.work.join("work/incompat");
        if dirty_flag.exists() {
            return Err(anyhow!(
                "This container filesystem can't be used anymore. Please rollback."
            ));
        }
        // let's mount them
        overlay.mount().map_err(|e| anyhow!("{}", e.to_string()))?;

        Ok(())
    }

    /// is_mounted: check if a path is a mountpoint with corresponding fs_type
    fn is_mounted(&self, target: &Path) -> Result<bool> {
        is_mounted(target, OsStr::new("overlay"))
    }

    fn is_tmpfs(&self) -> bool {
        self.tmpfs.is_some()
    }

    fn is_tmpfs_mounted(&self) -> Result<bool> {
        if let Some((tmpfs, _)) = &self.tmpfs {
            is_mounted(&path::absolute(&tmpfs)?, OsStr::new("tmpfs"))
        } else {
            bail!("the container does not use tmpfs")
        }
    }

    fn rollback(&mut self) -> Result<()> {
        if self.is_tmpfs() {
            // for mounted tmpfs containers, simply un-mount the tmpfs
            self.unmount_tmpfs()?;
        } else {
            fs::remove_dir_all(&self.upper)?;
            fs::remove_dir_all(&self.work)?;
            fs::create_dir(&self.upper)?;
            fs::create_dir(&self.work)?;
        }

        Ok(())
    }

    fn commit(&mut self) -> Result<()> {
        if self.volatile {
            // for safety reasons
            nix::unistd::sync();
        }
        let mods = self.diff()?;
        // FIXME: use drain_filter in the future
        // first pass to execute all the deletion actions
        for i in mods.iter() {
            match i {
                Diff::WhiteoutFile(_) => overlay_exec_action(i, self)?,
                _ => continue,
            }
        }
        // second pass for everything else
        for i in mods.iter() {
            match i {
                Diff::WhiteoutFile(_) => continue,
                _ => overlay_exec_action(i, self)
                    .with_context(|| format!("when processing {:?}", i))?,
            }
        }
        // clear all the remnant items in the upper layer
        self.rollback()?;

        Ok(())
    }

    fn unmount(&mut self, target: &Path) -> Result<()> {
        umount2(target, MntFlags::MNT_DETACH)?;

        Ok(())
    }

    fn unmount_tmpfs(&self) -> Result<()> {
        if let Some((tmpfs, _)) = &self.tmpfs {
            if self.is_tmpfs_mounted()? {
                info!("Un-mounting tmpfs ...");
                umount2(tmpfs, MntFlags::MNT_DETACH)?;
            }
        }
        Ok(())
    }

    fn get_config_layer(&mut self) -> Result<PathBuf> {
        Ok(self.lower.clone())
    }

    fn get_base_layer(&mut self) -> Result<PathBuf> {
        Ok(self.base.clone())
    }

    fn destroy(&mut self) -> Result<()> {
        if self.is_tmpfs() {
            self.unmount_tmpfs()?;
        }
        fs::remove_dir_all(&self.inst)?;

        Ok(())
    }

    fn set_volatile(&mut self, volatile: bool) -> Result<()> {
        self.volatile = volatile;

        Ok(())
    }
}

/// is_mounted: check if a path is a mountpoint with corresponding fs_type
pub(crate) fn is_mounted(mountpoint: &Path, fs_type: &OsStr) -> Result<bool> {
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

/// A convenience function for getting a overlayfs type LayerManager
pub(crate) fn get_overlayfs_manager(inst_name: &str) -> Result<Box<dyn LayerManager>> {
    OverlayFS::from_inst_dir(common::CIEL_DIST_DIR, common::CIEL_INST_DIR, inst_name)
}

/// Check if path have all specified prefixes (with order)
#[inline]
fn has_prefix(path: &Path, prefixes: &[PathBuf]) -> bool {
    prefixes
        .iter()
        .any(|prefix| path.strip_prefix(prefix).is_ok())
}

fn load_overlayfs_support() -> Result<()> {
    if test_overlay_usability().is_err() {
        Command::new("modprobe")
            .arg("overlay")
            .status()
            .map_err(|e| anyhow!("Unable to load overlay kernel module: {}", e))?;
    }

    Ok(())
}

#[inline]
pub fn test_overlay_usability() -> Result<()> {
    let f = fs::File::open("/proc/filesystems")?;
    let reader = BufReader::new(f);
    for line in reader.lines() {
        let line = line?;
        let mut fs_type = line.splitn(2, '\t');
        if let Some(fs_type) = fs_type.nth(1) {
            if fs_type == "overlay" {
                return Ok(());
            }
        }
    }

    Err(anyhow!("No overlayfs support detected"))
}

/// Set permission of to according to from
#[inline]
fn sync_permission(from: &Path, to: &Path) -> Result<()> {
    let from_meta = fs::metadata(from)?;
    let to_meta = fs::metadata(to)?;

    if from_meta.mode() != to_meta.mode() {
        to_meta.permissions().set_mode(to_meta.mode());
    }

    Ok(())
}

fn rename_file(from: &Path, to: &Path, overlay: &OverlayFS) -> Result<()> {
    if overlay.is_tmpfs() {
        if to.symlink_metadata().is_ok() {
            if to.is_dir() {
                fs::remove_dir_all(to)?;
            } else {
                fs::remove_file(to)?;
            }
        }
        if from.is_symlink() {
            std::os::unix::fs::symlink(fs::read_link(from)?, to)?;
            fs::remove_file(from)?;
        } else if from.is_file() {
            fs::copy(from, to)?;
            fs::remove_file(from)?;
        } else if from.is_dir() {
            fs::create_dir_all(to)?;
            fs::set_permissions(to, from.metadata()?.permissions())?;
            for entry in fs::read_dir(from)? {
                let entry = entry?;
                rename_file(
                    &from.join(entry.file_name()),
                    &to.join(entry.file_name()),
                    overlay,
                )?;
            }
            fs::remove_dir_all(from)?;
        } else {
            bail!("unsupported file type");
        }
    } else {
        fs::rename(from, to)?;
    }
    Ok(())
}

#[inline]
fn overlay_exec_action(action: &Diff, overlay: &OverlayFS) -> Result<()> {
    match action {
        Diff::Symlink(path) => {
            let upper_path = overlay.upper.join(path);
            let lower_path = overlay.base.join(path);
            // Replace lower dir with upper
            rename_file(&upper_path, &lower_path, overlay)?;
        }
        Diff::OverrideDir(path) => {
            let upper_path = overlay.upper.join(path);
            let lower_path = overlay.base.join(path);
            // Replace lower dir with upper
            if lower_path.is_dir() {
                // If exists and was not removed already, then remove it
                fs::remove_dir_all(&lower_path)?;
            } else if lower_path.is_file() {
                // If it's a file, then remove it as well
                fs::remove_file(&lower_path)?;
            }
            rename_file(&upper_path, &lower_path, overlay)?;
        }
        Diff::RenamedDir(from, to) => {
            // TODO: Implement copy down
            // Such dir will include diff files, so this
            // section need more testing
            let from_path = overlay.base.join(from);
            let to_path = overlay.base.join(to);
            // TODO: Merge files from upper to lower
            // Replace lower dir with upper
            rename_file(&from_path, &to_path, overlay)?;
        }
        Diff::NewDir(path) => {
            let lower_path = overlay.base.join(path);
            // Construct lower path
            fs::create_dir_all(lower_path)?;
        }
        Diff::ModifiedDir(path) => {
            // Do nothing, just sync permission
            let upper_path = overlay.upper.join(path);
            let lower_path = overlay.base.join(path);
            sync_permission(&upper_path, &lower_path)?;
        }
        Diff::WhiteoutFile(path) => {
            let lower_path = overlay.base.join(path);
            if lower_path.is_dir() {
                fs::remove_dir_all(&lower_path)?;
            } else if lower_path.is_file() {
                fs::remove_file(&lower_path)?;
            }
            // remove the whiteout in the upper layer
            fs::remove_file(overlay.upper.join(path))?;
        }
        Diff::File(path) => {
            let upper_path = overlay.upper.join(path);
            let lower_path = overlay.base.join(path);
            // Move upper file to overwrite the lower
            rename_file(&upper_path, &lower_path, overlay)?;
        }
    }

    Ok(())
}
