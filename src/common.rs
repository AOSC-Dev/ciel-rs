use failure::Error;
use lazy_static::lazy_static;
use std::fs::{self, File};
use std::{
    io::Write,
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

pub fn ciel_init() -> Result<(), Error> {
    for dir in SKELETON_DIRS {
        fs::create_dir_all(dir)?;
    }
    let mut f = File::create(".ciel/version")?;
    f.write_all(CURRENT_CIEL_VERSION_STR.as_bytes())?;

    Ok(())
}
