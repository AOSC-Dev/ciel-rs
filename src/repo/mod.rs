//! Local repository

use crate::info;
use anyhow::Result;
use sha2::{Digest, Sha256};
use std::io::Write;
use std::{fs, io, path::Path};
use time::{format_description::FormatItem, macros::format_description, OffsetDateTime};

mod monitor;
mod scan;

pub use monitor::start_monitor;

/// Debian 822 date: "%a, %d %b %Y %H:%M:%S %z"
const DEB822_DATE: &[FormatItem] = format_description!("[weekday repr:short], [day] [month repr:short] [year] [hour repr:24]:[minute]:[second] [offset_hour sign:mandatory][offset_minute]");

fn generate_release(path: &Path) -> Result<String> {
    let mut f = fs::File::open(path.join("Packages"))?;
    let mut hasher = Sha256::new();
    io::copy(&mut f, &mut hasher)?;
    let result = hasher.finalize();
    let meta = f.metadata()?;
    let timestamp = OffsetDateTime::now_utc().format(&DEB822_DATE)?;

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
    info!("Scanning {} packages...", entries.len());
    output.write_all(&scan::scan_packages_simple(&entries, &path))?;
    println!();

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
