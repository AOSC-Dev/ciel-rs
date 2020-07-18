use failure::Error;
use lazy_static::lazy_static;
use progress_streams::ProgressReader;
use std::fs::{self, File};
use std::{
    io::{Read, Write},
    path::{Path, PathBuf},
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
            .template("{spinner:.green} {wide_msg}");
}

#[inline]
pub fn create_spinner(msg: &str, tick_rate: u64) -> indicatif::ProgressBar {
    let spinner = indicatif::ProgressBar::new_spinner().with_style(SPINNER_STYLE.clone());
    spinner.set_message(msg);
    spinner.enable_steady_tick(tick_rate);

    spinner
}

/// Extract the given .tar.xz stream and preserve all the file attributes
pub fn extract_tar_xz<R: Read>(reader: R, path: &PathBuf) -> Result<(), Error> {
    let decompress = xz2::read::XzDecoder::new(reader);
    let mut tar_processor = tar::Archive::new(decompress);
    tar_processor.set_unpack_xattrs(true);
    tar_processor.set_preserve_permissions(true);
    tar_processor.unpack(path)?;

    Ok(())
}

pub fn extract_system_tarball(path: &PathBuf, total: u64) -> Result<(), Error> {
    let mut f = File::open(path)?;
    let progress_bar = indicatif::ProgressBar::new(total);
    progress_bar.set_style(indicatif::ProgressStyle::default_bar().template(
        "{spinner} [{bar:40.cyan/blue}] Extracting tarball... ({bytes_per_sec}, eta {eta})",
    ));
    progress_bar.enable_steady_tick(500);
    let reader = ProgressReader::new(&mut f, |progress: usize| {
        progress_bar.inc(progress as u64);
    });
    extract_tar_xz(reader, &PathBuf::from(CIEL_DIST_DIR))?;
    progress_bar.finish_and_clear();

    Ok(())
}

pub fn ciel_init() -> Result<(), Error> {
    for dir in SKELETON_DIRS {
        fs::create_dir_all(dir)?;
    }
    let mut f = File::create(".ciel/version")?;
    f.write_all(CURRENT_CIEL_VERSION_STR.as_bytes())?;

    Ok(())
}
