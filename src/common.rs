use anyhow::{anyhow, Result};
use console::user_attended;
use dialoguer::{theme::ColorfulTheme, FuzzySelect};
use digest_io::IoWrapper;
use faster_hex::hex_string;
use indicatif::ProgressBar;
use sha2::{Digest, Sha256};
use std::env::consts::ARCH;
use std::fs::{self, File};
use std::os::unix::prelude::MetadataExt;
use std::sync::LazyLock;
use std::{
    io::{Read, Write},
    path::{Path, PathBuf},
    time::Duration,
};
use unsquashfs_wrapper::Unsquashfs;

pub const CIEL_MAINLINE_ARCHS: &[&str] = &[
    "amd64",
    "arm64",
    "ppc64el",
    "riscv64",
    "loongarch64",
    "loongarch64_nosimd",
    "loongson3",
];
pub const CIEL_RETRO_ARCHS: &[&str] = &[
    "alpha",
    "armv4",
    "armv5te",
    "armv6hf",
    "armv7hf",
    "ia64",
    "i486",
    "loongson2f",
    "m68k",
    "powerpc",
    "ppc64",
    "sh3",
    "sparc64",
];
pub const CURRENT_CIEL_VERSION: usize = 3;
const CURRENT_CIEL_VERSION_STR: &str = "3";
pub const CIEL_DIST_DIR: &str = ".ciel/container/dist";
pub const CIEL_INST_DIR: &str = ".ciel/container/instances";
pub const CIEL_DATA_DIR: &str = ".ciel/data";
const SKELETON_DIRS: &[&str] = &[CIEL_DIST_DIR, CIEL_INST_DIR, CIEL_DATA_DIR];

static SPINNER_STYLE: LazyLock<indicatif::ProgressStyle> = LazyLock::new(|| {
    indicatif::ProgressStyle::default_spinner()
        .tick_chars("⠋⠙⠸⠴⠦⠇ ")
        .template("{spinner:.green} {wide_msg}")
        .unwrap()
});

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

#[inline]
pub fn check_arch_name(arch: &str) -> bool {
    CIEL_MAINLINE_ARCHS.contains(&arch) || CIEL_RETRO_ARCHS.contains(&arch)
}

/// AOSC OS specific architecture mapping table
#[inline]
pub fn get_host_arch_name() -> Option<&'static str> {
    #[cfg(not(any(target_arch = "powerpc64", target_arch = "loongarch64")))]
    match ARCH {
        "x86_64" => Some("amd64"),
        "x86" => Some("i486"),
        "powerpc" => Some("powerpc"),
        "aarch64" => Some("arm64"),
        "mips64" => Some("loongson3"),
        "riscv64" => Some("riscv64"),
        _ => None,
    }

    #[cfg(target_arch = "powerpc64")]
    {
        let mut endian: libc::c_int = -1;
        let result = unsafe { libc::prctl(libc::PR_GET_ENDIAN, &mut endian as *mut libc::c_int) };
        if result < 0 {
            return None;
        }
        match endian {
            libc::PR_ENDIAN_LITTLE | libc::PR_ENDIAN_PPC_LITTLE => Some("ppc64el"),
            libc::PR_ENDIAN_BIG => Some("ppc64"),
            _ => None,
        }
    }

    #[cfg(target_arch = "loongarch64")]
    {
        use core::arch::asm;

        fn loongarch_has_simd() -> bool {
            let mask: u64;
            const CONFIG_PAGE: u64 = 2;
            unsafe {
                asm! {
                    "cpucfg {mask}, {page}",
                    options(pure, nomem, nostack, preserves_flags),
                    page = in(reg) CONFIG_PAGE,
                    mask = lateout(reg) mask,
                }
                (mask >> 6) & 0x1 > 0
            }
        }

        if loongarch_has_simd() {
            Some("loongarch64")
        } else {
            Some("loongarch64_nosimd")
        }
    }
}

/// Calculate the Sha256 checksum of the given stream
pub fn sha256sum<R: Read>(mut reader: R) -> Result<String> {
    let mut hasher = IoWrapper(Sha256::new());
    std::io::copy(&mut reader, &mut hasher)?;

    Ok(hex_string(&hasher.0.finalize()))
}

/// Check if the current process is running in a container
pub fn is_in_ci() -> bool {
    match std::env::var("CI") {
        Ok(v) => match v.as_str() {
            "y" | "yes" | "1" | "t" | "true" | "on" => true,
            _ => false,
        },
        Err(_) => false,
    }
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

pub fn extract_system_rootfs(path: &Path, total: u64, use_tarball: bool) -> Result<()> {
    let f = File::open(path)?;
    let progress_bar = indicatif::ProgressBar::new(total);

    progress_bar.set_style(
        indicatif::ProgressStyle::default_bar()
            .template(make_progress_bar!("Extracting rootfs ..."))
            .unwrap(),
    );

    progress_bar.set_draw_target(indicatif::ProgressDrawTarget::stderr_with_hz(5));

    let dist_dir = PathBuf::from(CIEL_DIST_DIR);
    if dist_dir.exists() {
        fs::remove_dir_all(&dist_dir).ok();
        fs::create_dir_all(&dist_dir)?;
    }

    // detect if we are running in systemd-nspawn
    // where /dev/console character device file cannot be created
    // thus ignoring the error in extracting
    let mut in_systemd_nspawn = false;
    if let Ok(output) = std::process::Command::new("systemd-detect-virt").output() {
        if let Ok("systemd-nspawn") = std::str::from_utf8(&output.stdout) {
            in_systemd_nspawn = true;
        }
    }

    let res = if use_tarball {
        extract_tar_xz(progress_bar.wrap_read(f), &dist_dir)
    } else {
        extract_squashfs(path, &dist_dir, &progress_bar, total)
    };

    if !in_systemd_nspawn {
        res?
    }

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

/// Check whether `name` is usable as a `systemd-nspawn` container/machine
/// name.
///
/// `systemd-nspawn` silently strips characters it doesn't accept (e.g. `_`)
/// from the machine name it actually registers, while ciel keeps tracking
/// the instance by the original, unstripped name -- so ciel then can no
/// longer find/control the container it just created. Reject such names
/// up-front instead. See #47.
#[allow(clippy::ptr_arg)]
pub fn validate_instance_name(name: &String) -> Result<(), String> {
    if name.is_empty() {
        return Err("Instance name must not be empty.".to_owned());
    }
    if name.len() > 64 {
        return Err("Instance name must be 64 characters or fewer.".to_owned());
    }
    if !name.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-') {
        return Err(
            "Instance name may only contain ASCII letters, digits, and hyphens ('-').".to_owned(),
        );
    }
    if name.starts_with('-') || name.ends_with('-') {
        return Err("Instance name must not start or end with a hyphen ('-').".to_owned());
    }

    Ok(())
}

pub fn is_legacy_workspace() -> Result<bool> {
    let mut f = fs::File::open(".ciel/version")?;
    // TODO: use a more robust check
    let mut buf = [0u8; 1];
    f.read_exact(&mut buf)?;

    Ok(buf[0] < CURRENT_CIEL_VERSION_STR.as_bytes()[0])
}

pub fn ask_for_target_arch() -> Result<&'static str> {
    // Collect all supported architectures
    let host_arch = get_host_arch_name();
    if !user_attended() {
        return match host_arch {
            Some(v) => Ok(v),
            None => Err(anyhow!("Could not determine host architecture")),
        };
    }
    let mut all_archs: Vec<&'static str> = CIEL_MAINLINE_ARCHS.into();
    all_archs.append(&mut CIEL_RETRO_ARCHS.into());
    let default_arch_index = match host_arch {
        Some(host_arch) => all_archs.iter().position(|a| *a == host_arch).unwrap(),
        None => 0,
    };
    // Setup Dialoguer
    let theme = ColorfulTheme::default();
    let prefixed_archs = CIEL_MAINLINE_ARCHS
        .iter()
        .map(|x| format!("mainline: {x}"))
        .chain(CIEL_RETRO_ARCHS.iter().map(|x| format!("retro: {x}")))
        .collect::<Vec<_>>();
    let chosen_index = FuzzySelect::with_theme(&theme)
        .with_prompt("Target Architecture")
        .default(default_arch_index)
        .items(prefixed_archs.as_slice())
        .interact()?;

    Ok(all_archs[chosen_index])
}

#[test]
fn test_validate_instance_name_accepts_valid_names() {
    assert!(validate_instance_name(&"main".to_owned()).is_ok());
    assert!(validate_instance_name(&"BuildCat".to_owned()).is_ok());
    assert!(validate_instance_name(&"amd64-stable".to_owned()).is_ok());
    assert!(validate_instance_name(&"update-662d42f2".to_owned()).is_ok());
}

#[test]
fn test_validate_instance_name_rejects_characters_nspawn_would_strip() {
    // https://github.com/AOSC-Dev/ciel-rs/issues/47: systemd-nspawn silently
    // strips characters like '_' from the machine name, so ciel must refuse
    // them up-front rather than tracking a name nspawn will never register.
    assert!(validate_instance_name(&"BuildCat_la64".to_owned()).is_err());
    assert!(validate_instance_name(&"has space".to_owned()).is_err());
    assert!(validate_instance_name(&"has.dot".to_owned()).is_err());
    assert!(validate_instance_name(&"has/slash".to_owned()).is_err());
}

#[test]
fn test_validate_instance_name_rejects_edge_cases() {
    assert!(validate_instance_name(&"".to_owned()).is_err());
    assert!(validate_instance_name(&"-leading-hyphen".to_owned()).is_err());
    assert!(validate_instance_name(&"trailing-hyphen-".to_owned()).is_err());
    assert!(validate_instance_name(&"a".repeat(65)).is_err());
    assert!(validate_instance_name(&"a".repeat(64)).is_ok());
}
