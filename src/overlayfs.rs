use failure::{format_err, Error};
use libmount::Overlay;
use nix::mount::{umount2, MntFlags};
use std::path::{Path, PathBuf};

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
