//! Local repository

use anyhow::{anyhow, Result};
use std::{
    fs, io,
    path::Path,
    process::{Command, Stdio},
};

/// Rrefresh the local repository (Update Packages file)
pub fn refresh_repo(root: &Path) -> Result<()> {
    let path = root.join("debs");
    fs::create_dir_all(&path)?;
    let mut output = fs::File::create(path.join("Packages"))?;
    let mut child = Command::new("dpkg-scanpackages")
        .args(&["-h", "sha256", "debs/"])
        .stdout(Stdio::piped())
        .current_dir(path)
        .spawn()?;
    let mut stdout = child.stdout.take().unwrap();
    io::copy(&mut stdout, &mut output)?;

    if !child.wait()?.success() {
        return Err(anyhow!("dpkg-scanpackage failed"));
    }

    Ok(())
}

/// Initialize local repository and add entries to sources.list
pub fn init_repo(repo_root: &Path, rootfs: &Path) -> Result<()> {
    // trigger a refresh, since the metadata is probably out of date
    refresh_repo(repo_root)?;
    fs::create_dir_all(rootfs.join("etc/apt/sources.list.d/"))?;
    fs::write(
        rootfs.join("etc/apt/sources.list.d/ciel-local.list"),
        b"deb [trusted=yes] file:///debs/ /",
    )?;

    Ok(())
}

/// Uninitialize the repository
pub fn deinit_repo(rootfs: &Path) -> Result<()> {
    Ok(fs::remove_file(
        rootfs.join("etc/apt/sources.list.d/ciel-local.list"),
    )?)
}
