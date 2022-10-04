use anyhow::{anyhow, Result};
use lazy_static::lazy_static;
use progress_streams::ProgressReader;
use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::os::unix::prelude::MetadataExt;
use std::{
    io::{Read, Write},
    path::{Path, PathBuf},
    time::Duration,
};

pub const CURRENT_CIEL_VERSION: usize = 3;
const CURRENT_CIEL_VERSION_STR: &str = "3";
pub const CIEL_DIST_DIR: &str = ".ciel/container/dist";
pub const CIEL_INST_DIR: &str = ".ciel/container/instances";
pub const CIEL_DATA_DIR: &str = ".ciel/data";
const SKELETON_DIRS: &[&str] = &[CIEL_DIST_DIR, CIEL_INST_DIR, CIEL_DATA_DIR];

lazy_static! {
    static ref SPINNER_STYLE: indicatif::ProgressStyle =
        indicatif::ProgressStyle::default_spinner()
            .tick_chars("⠋⠙⠸⠴⠦⠇ ")
            .template("{spinner:.green} {wide_msg}")
            .unwrap();
}

#[macro_export]
macro_rules! make_progress_bar {
    ($msg:expr) => {
        concat!(
            "{spinner} [{bar:25.cyan/blue}] ",
            $msg,
            " ({bytes_per_sec}, eta {eta})"
        )
    };
}

#[inline]
pub fn create_spinner(msg: &'static str, tick_rate: u64) -> indicatif::ProgressBar {
    let spinner = indicatif::ProgressBar::new_spinner().with_style(SPINNER_STYLE.clone());
    spinner.set_message(msg);
    spinner.enable_steady_tick(Duration::from_millis(tick_rate));

    spinner
}

/// Calculate the Sha256 checksum of the given stream
pub fn sha256sum<R: Read>(mut reader: R) -> Result<String> {
    let mut hasher = Sha256::new();
    std::io::copy(&mut reader, &mut hasher)?;

    Ok(format!("{:x}", hasher.finalize()))
}

/// Extract the given .tar.xz stream and preserve all the file attributes
pub fn extract_tar_xz<R: Read>(reader: R, path: &Path) -> Result<()> {
    let decompress = xz2::read::XzDecoder::new(reader);
    let mut tar_processor = tar::Archive::new(decompress);
    tar_processor.set_unpack_xattrs(true);
    tar_processor.set_preserve_permissions(true);
    tar_processor.unpack(path)?;

    Ok(())
}

pub fn extract_system_tarball(path: &Path, total: u64) -> Result<()> {
    let mut f = File::open(path)?;
    let progress_bar = indicatif::ProgressBar::new(total);
    progress_bar.set_style(
        indicatif::ProgressStyle::default_bar()
            .template(make_progress_bar!("Extracting tarball..."))
            .unwrap(),
    );
    progress_bar.enable_steady_tick(Duration::from_millis(500));
    let reader = ProgressReader::new(&mut f, |progress: usize| {
        progress_bar.inc(progress as u64);
    });
    extract_tar_xz(reader, &PathBuf::from(CIEL_DIST_DIR))?;
    progress_bar.finish_and_clear();

    Ok(())
}

pub fn ciel_init() -> Result<()> {
    for dir in SKELETON_DIRS {
        fs::create_dir_all(dir)?;
    }
    let mut f = File::create(".ciel/version")?;
    f.write_all(CURRENT_CIEL_VERSION_STR.as_bytes())?;

    Ok(())
}

/// Find the ciel directory
pub fn find_ciel_dir<P: AsRef<Path>>(start: P) -> Result<PathBuf> {
    let start_path = fs::metadata(start.as_ref())?;
    let start_dev = start_path.dev();
    let mut current_dir = start.as_ref().to_path_buf();
    loop {
        if !current_dir.exists() {
            return Err(anyhow!("Hit filesystem ceiling!"));
        }
        let current_dev = current_dir.metadata()?.dev();
        if current_dev != start_dev {
            return Err(anyhow!("Hit filesystem boundary!"));
        }
        if current_dir.join(".ciel").is_dir() {
            return Ok(current_dir);
        }
        current_dir = current_dir.join("..");
    }
}

pub fn is_instance_exists(instance: &str) -> bool {
    Path::new(CIEL_INST_DIR).join(instance).is_dir()
}

pub fn is_legacy_workspace() -> Result<bool> {
    let mut f = fs::File::open(".ciel/version")?;
    // TODO: use a more robust check
    let mut buf = [0u8; 1];
    f.read_exact(&mut buf)?;

    Ok(buf[0] < CURRENT_CIEL_VERSION_STR.as_bytes()[0])
}
