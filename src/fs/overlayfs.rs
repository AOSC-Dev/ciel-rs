use std::{
    ffi::OsStr,
    fs::{self, File},
    io::{BufRead, BufReader},
    os::unix::{
        ffi::OsStrExt,
        fs::{FileTypeExt, MetadataExt, PermissionsExt},
    },
    path::{Path, PathBuf},
    process::Command,
    sync::Arc,
};

use libmount::Overlay;
use log::info;
use nix::mount::{umount2, MntFlags};

use crate::{Error, Result};

use super::{BoxedLayer, OverlayManager, SimpleLayer};

/// A `overlay` filesystem-backed overlay manager.
///
/// In non-compat mode, The structure of the upper layer is as follows:
/// - `diff` (upper directory)
/// - `diff.tmp` (work directory)
///
/// To keep compatibility with old containers created by Ciel <= 3.6.0,
/// a compatibile mode is supported, which can be enabled with [OverlayFS::new_compat].
///
/// In compatibile mode, the upper layer must be a simple layer, pointing to
/// the container directory, rather than `upper` subdirectory.
/// When `rollback` is called, OverlayFS in compat mode will not
/// really call the [super::Layer::reset], instead it removes the old directories.
pub struct OverlayFS {
    target: PathBuf,
    upper: BoxedLayer,
    compat: bool,
    lower: Vec<BoxedLayer>,
    volatile: bool,
}

impl OverlayFS {
    /// Creates a new OverlayFS manager.
    pub fn new<P: AsRef<Path>>(
        target: P,
        upper: BoxedLayer,
        lower: Vec<BoxedLayer>,
        volatile: bool,
    ) -> Self {
        Self {
            target: target.as_ref().to_owned(),
            upper,
            compat: false,
            lower,
            volatile,
        }
    }

    /// Creates a new OverlayFS manager which is compatible with old containers.
    pub fn new_compat<P: AsRef<Path>>(
        target: P,
        upper: P,
        lower: Vec<BoxedLayer>,
        volatile: bool,
    ) -> Self {
        Self {
            target: target.as_ref().to_owned(),
            upper: Arc::new(Box::new(SimpleLayer::new(upper.as_ref()))),
            compat: true,
            lower,
            volatile,
        }
    }
}

impl OverlayManager for OverlayFS {
    fn fs_type(&self) -> &'static str {
        "overlay"
    }

    fn target(&self) -> &Path {
        &self.target
    }

    fn upper_layer(&self) -> &BoxedLayer {
        &self.upper
    }

    fn lower_layers(&self) -> Vec<&BoxedLayer> {
        self.lower.iter().collect()
    }

    fn mount(&self) -> Result<()> {
        if self.is_mounted()? {
            return Ok(());
        }
        if !self.upper.is_mounted()? {
            self.upper.mount()?;
        }
        let mut lowerdirs = Vec::new();
        for lower in &self.lower {
            if !lower.is_mounted()? {
                lower.mount()?;
            }
            lowerdirs.push(lower.target());
        }

        let upperdir = self.upper.target().join("diff");
        let workdir = self.upper.target().join("diff.tmp");
        // these two directories may have been created by older versions of Ciel
        if !upperdir.exists() {
            fs::create_dir(&upperdir)?;
        }
        if !workdir.exists() {
            fs::create_dir(&workdir)?;
        }

        ensure_overlayfs_support()?;
        if !self.target.exists() {
            fs::create_dir(&self.target)?;
        }
        let mut overlay = Overlay::writable(
            lowerdirs.iter().map(|x| x.as_ref()),
            upperdir.clone(),
            workdir.clone(),
            &self.target,
        );
        if self.volatile {
            overlay.set_options(b"volatile".to_vec());
        }

        if workdir.join("work/incompat").exists() {
            return Err(Error::OverlayFSIncompat(workdir));
        }

        info!("overlayfs: mounting at {:?}", self.target);
        overlay.mount()?;
        Ok(())
    }

    fn unmount(&self) -> Result<()> {
        if !self.is_mounted()? {
            return Ok(());
        }
        info!("overlayfs: un-mounting at {:?}", self.target);
        umount2(&self.target, MntFlags::MNT_DETACH)?;
        fs::remove_dir(&self.target)?;
        self.upper.unmount()?;
        for lower in &self.lower {
            lower.unmount()?;
        }
        Ok(())
    }

    fn rollback(&self) -> Result<()> {
        self.unmount()?;
        if self.compat {
            fs::remove_dir_all(self.upper.target().join("diff"))?;
            fs::remove_dir_all(self.upper.target().join("diff.tmp"))?;
        } else {
            self.upper.reset()?;
        }
        // avoid resetting the base system layer
        if let Some((_, lowers)) = &self.lower.split_last() {
            for lower in lowers.iter() {
                lower.reset()?;
            }
        }
        Ok(())
    }

    fn commit(&self) -> Result<()> {
        info!("overlayfs: commiting changes in {:?}", self.target);
        if self.volatile {
            // for safety reasons
            nix::unistd::sync();
        }

        let upper = self.upper.target().join("diff");
        let lower = self.lower.last().unwrap().target();
        let diffs = self.diff()?;

        // FIXME: use extract_if in the future
        // first, perform all the deletion actions
        for i in diffs.iter() {
            match i {
                Diff::WhiteoutFile(_) => patch_lower(i, &upper, lower)?,
                _ => continue,
            }
        }
        // second, apply other things
        for i in diffs.iter() {
            match i {
                Diff::WhiteoutFile(_) => continue,
                _ => patch_lower(i, &upper, lower)?,
            }
        }

        // clear all the remaining items in the upper layer
        self.rollback()?;

        Ok(())
    }
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
    fn diff(&self) -> Result<Vec<Diff>> {
        let mut diffs: Vec<Diff> = Vec::new();
        let mut processed_dirs: Vec<PathBuf> = Vec::new();

        let upper = self.upper.target().join("diff");
        let lower = self.lower.last().unwrap().target();

        // skip the root entry
        for entry in walkdir::WalkDir::new(&upper).into_iter().skip(1) {
            let path: PathBuf = entry?.path().to_path_buf();
            let rel_path = path.strip_prefix(&upper)?.to_path_buf();
            let lower_path = lower.join(&rel_path).to_path_buf();

            if processed_dirs
                .iter()
                .any(|prefix| rel_path.strip_prefix(prefix).is_ok())
            {
                continue; // We already dealt with it
            }

            let meta = fs::symlink_metadata(&path)?;
            let file_type = meta.file_type();
            if file_type.is_symlink() {
                // Just move the symlink
                diffs.push(Diff::Symlink(rel_path.clone()));
            } else if meta.is_dir() {
                // Deal with dirs
                let metacopy = xattr::get(&path, "trusted.overlay.metacopy")?;
                if let Some(_data) = metacopy {
                    return Err(Error::MetaCopyUnsupported);
                }

                let opaque = xattr::get(&path, "trusted.overlay.opaque")?;
                if let Some(text) = opaque {
                    // the new dir (completely) replace the old one
                    if text == b"y" {
                        // Delete corresponding dir
                        diffs.push(Diff::OverrideDir(rel_path.clone()));
                        processed_dirs.push(rel_path.clone());
                        continue;
                    }
                }

                let redirect = xattr::get(&path, "trusted.overlay.redirect")?;
                if let Some(from_utf8) = redirect {
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
                        from_rel_path = from_path.strip_prefix(&upper)?.to_path_buf();
                    }
                    diffs.push(Diff::RenamedDir(from_rel_path, rel_path));
                    continue;
                }
                if !lower_path.is_dir() {
                    // New dir
                    diffs.push(Diff::NewDir(rel_path.clone()));
                } else {
                    // Modified
                    diffs.push(Diff::ModifiedDir(rel_path.clone()));
                }
            } else {
                // Deal with files
                if file_type.is_char_device() && meta.rdev() == 0 {
                    // Whiteout file!
                    diffs.push(Diff::WhiteoutFile(rel_path.clone()));
                } else if lower_path.is_dir() {
                    // A new file overrides an old directory
                    diffs.push(Diff::OverrideDir(rel_path.clone()));
                } else {
                    diffs.push(Diff::File(rel_path.clone()));
                }
            }
        }

        Ok(diffs)
    }
}

fn ensure_overlayfs_support() -> Result<()> {
    let f = File::open("/proc/filesystems")?;
    let reader = BufReader::new(f);
    for line in reader.lines() {
        let line = line?;
        let mut fs_type = line.splitn(2, '\t');
        if fs_type.nth(1) == Some("overlay") {
            return Ok(());
        }
    }

    Command::new("modprobe")
        .arg("overlay")
        .status()
        .map_err(|_| Error::OverlayFSUnavailable)?;

    Ok(())
}

fn rename_file(from: &Path, to: &Path) -> Result<()> {
    if to.symlink_metadata().is_ok() {
        if to.is_dir() {
            fs::remove_dir_all(to)?;
        } else {
            fs::remove_file(to)?;
        }
    }

    match fs::rename(from, to) {
        Ok(_) => return Ok(()),
        Err(err) => {
            // FIXME: use CrossesDevices when stablized
            // now we just fallthrough
            _ = err;
            // if err.kind() != std::io::ErrorKind::CrossesDevices {
            //     return Err(err.into());
            // }
        }
    }

    let from_meta = from.symlink_metadata()?;
    if from_meta.is_symlink() {
        std::os::unix::fs::symlink(fs::read_link(from)?, to)?;
        fs::remove_file(from)?;
    } else if from_meta.is_file() {
        fs::copy(from, to)?;
        fs::remove_file(from)?;
    } else if from_meta.is_dir() {
        fs::create_dir_all(to)?;
        fs::set_permissions(to, from.metadata()?.permissions())?;
        for entry in fs::read_dir(from)? {
            let entry = entry?;
            rename_file(&from.join(entry.file_name()), &to.join(entry.file_name()))?;
        }
        fs::remove_dir_all(from)?;
    } else {
        unreachable!();
    }
    Ok(())
}

fn patch_lower(action: &Diff, upper: &Path, lower: &Path) -> Result<()> {
    match action {
        Diff::Symlink(path) => {
            let upper_path = upper.join(path);
            let lower_path = lower.join(path);
            // Replace lower dir with upper
            rename_file(&upper_path, &lower_path)?;
        }
        Diff::OverrideDir(path) => {
            let upper_path = upper.join(path);
            let lower_path = lower.join(path);
            // Replace lower dir with upper
            if lower_path.is_dir() {
                // If exists and was not removed already, then remove it
                fs::remove_dir_all(&lower_path)?;
            } else if lower_path.is_file() {
                // If it's a file, then remove it as well
                fs::remove_file(&lower_path)?;
            }
            rename_file(&upper_path, &lower_path)?;
        }
        Diff::RenamedDir(from, to) => {
            // TODO: Implement copy down
            // Such dir will include diff files, so this
            // section need more testing
            let from_path = lower.join(from);
            let to_path = lower.join(to);
            // TODO: Merge files from upper to lower
            // Replace lower dir with upper
            rename_file(&from_path, &to_path)?;
        }
        Diff::NewDir(path) => {
            let lower_path = lower.join(path);
            // Construct lower path
            fs::create_dir_all(lower_path)?;
        }
        Diff::ModifiedDir(path) => {
            // Do nothing, just sync permission
            let upper_path = upper.join(path);
            let lower_path = lower.join(path);
            let upper_meta = fs::metadata(upper_path)?;
            let lower_meta = fs::metadata(lower_path)?;

            if upper_meta.mode() != lower_meta.mode() {
                lower_meta.permissions().set_mode(lower_meta.mode());
            }
        }
        Diff::WhiteoutFile(path) => {
            let lower_path = lower.join(path);
            if lower_path.is_dir() {
                fs::remove_dir_all(&lower_path)?;
            } else if lower_path.is_file() {
                fs::remove_file(&lower_path)?;
            }
            // remove the whiteout in the upper layer
            fs::remove_file(upper.join(path))?;
        }
        Diff::File(path) => {
            let upper_path = upper.join(path);
            let lower_path = lower.join(path);
            // Move upper file to overwrite the lower
            rename_file(&upper_path, &lower_path)?;
        }
    }

    Ok(())
}
