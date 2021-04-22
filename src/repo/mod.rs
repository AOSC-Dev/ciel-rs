//! Local repository

use anyhow::Result;
use chrono::prelude::*;
use sha2::{Digest, Sha256};
use std::io::Write;
use std::{fs, io, path::Path};

mod scan;

fn generate_release(path: &Path) -> Result<String> {
    let mut f = fs::File::open(path.join("Packages"))?;
    let mut hasher = Sha256::new();
    io::copy(&mut f, &mut hasher)?;
    let result = hasher.finalize();
    let meta = f.metadata()?;
    let timestamp = Utc::now().format("%a, %d %b %Y %X %z");

    Ok(format!(
        "Date: {}\nSHA256:\n {:x} {} Packages\n",
        timestamp,
        result,
        meta.len()
    ))
}

/// Refresh the local repository (Update Packages file)
pub fn refresh_repo(root: &Path) -> Result<()> {
    let path = root.join("debs");
    fs::create_dir_all(&path)?;
    let mut output = fs::File::create(path.join("Packages"))?;
    let entries = scan::collect_all_packages(&path)?;
    output.write_all(&scan::scan_packages_simple(&entries))?;

    let release = generate_release(&path)?;
    let mut release_file = fs::File::create(path.join("Release"))?;
    release_file.write_all(release.as_bytes())?;

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
