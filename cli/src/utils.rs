use std::{
    io::Read,
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
    sync::LazyLock,
    time::Duration,
};

use anyhow::{bail, Result};
use indicatif::ProgressBar;
use nix::libc::{prctl, PR_GET_ENDIAN};
use sha2::{Digest, Sha256};
use unsquashfs_wrapper::Unsquashfs;

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

/// Finds the Ciel workspace.
pub fn find_ciel_dir<P: AsRef<Path>>(start: P) -> Result<PathBuf> {
    let start_path = std::fs::metadata(start.as_ref())?;
    let start_dev = start_path.dev();
    let mut current_dir = start.as_ref().to_path_buf();
    loop {
        if !current_dir.exists() {
            bail!("Not a Ciel workspace: jit filesystem ceiling!")
        }
        let current_dev = current_dir.metadata()?.dev();
        if current_dev != start_dev {
            bail!("Not a Ciel workspace: hit filesystem boundary!")
        }
        if current_dir.join(".ciel").is_dir() {
            return Ok(current_dir);
        }
        current_dir = current_dir.join("..");
    }
}

/// Gets host-machine architecture in AOSC specific style.
pub fn get_host_arch_name() -> Result<&'static str> {
    #[cfg(not(target_arch = "powerpc64"))]
    match std::env::consts::ARCH {
        "x86_64" => Ok("amd64"),
        "x86" => Ok("i486"),
        "powerpc" => Ok("powerpc"),
        "aarch64" => Ok("arm64"),
        "mips64" => Ok("loongson3"),
        "riscv64" => Ok("riscv64"),
        "loongarch64" => Ok("loongarch64"),
        _ => bail!("Unrecognized host architecture"),
    }

    #[cfg(target_arch = "powerpc64")]
    {
        let mut endian: nix::libc::c_int = -1;
        let result = unsafe { prctl(PR_GET_ENDIAN, &mut endian as *mut nix::libc::c_int) };
        if result < 0 {
            bail!("Failed to get host endian");
        }
        match endian {
            nix::libc::PR_ENDIAN_LITTLE | nix::libc::PR_ENDIAN_PPC_LITTLE => Ok("ppc64el"),
            nix::libc::PR_ENDIAN_BIG => Ok("ppc64"),
            _ => bail!("Unrecognized host architecture"),
        }
    }
}

/// Calculate the SHA-256 checksum of the given stream
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

/// Extract the given .squashfs
pub fn extract_squashfs(path: &Path, dist_dir: &Path, pb: &ProgressBar, total: u64) -> Result<()> {
    let unsquashfs = Unsquashfs::default();

    unsquashfs.extract(path, dist_dir, None, move |c| {
        pb.set_position(total * c as u64 / 100);
    })?;

    Ok(())
}

static SPINNER_STYLE: LazyLock<indicatif::ProgressStyle> = LazyLock::new(|| {
    indicatif::ProgressStyle::default_spinner()
        .tick_chars("⠋⠙⠸⠴⠦⠇ ")
        .template("{spinner:.green} {wide_msg}")
        .unwrap()
});

pub fn create_spinner(msg: &'static str, tick_rate: u64) -> indicatif::ProgressBar {
    let spinner = indicatif::ProgressBar::new_spinner().with_style(SPINNER_STYLE.clone());
    spinner.set_message(msg);
    spinner.enable_steady_tick(Duration::from_millis(tick_rate));
    spinner
}
