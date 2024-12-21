use std::{
    path::Path,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, LazyLock,
    },
    thread,
    time::Duration,
};

use anyhow::{anyhow, bail, Result};
use log::info;
use reqwest::header::CONTENT_LENGTH;
use serde::{Deserialize, Serialize};

use crate::make_progress_bar;

const MANIFEST_URL: &str = "https://releases.aosc.io/manifest/recipe.json";

pub const CIEL_MAINLINE_ARCHS: &[&str] = &[
    "amd64",
    "arm64",
    "ppc64el",
    "mips64r6el",
    "riscv64",
    "loongarch64",
    "loongson3",
];
pub const CIEL_RETRO_ARCHS: &[&str] = &["armv4", "armv6hf", "armv7hf", "i486", "m68k", "powerpc"];

/// AOSC OS release manifest.
///
/// This should be kept in sync with the structure of release manifest
/// (`https://releases.aosc.io/manifest/recipe.json`)
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Recipe {
    pub version: usize,
    pub variants: Vec<Variant>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Variant {
    pub name: String,
    pub squashfs: Vec<RootFsInfo>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct RootFsInfo {
    pub arch: String,
    pub date: String,
    pub path: String,
    pub sha256sum: String,
}

pub fn http_client() -> Result<reqwest::blocking::Client> {
    Ok(reqwest::blocking::Client::builder()
        .user_agent(concat!(
            env!("CARGO_PKG_NAME"),
            "/",
            env!("CARGO_PKG_VERSION"),
        ))
        .build()?)
}

/// Download a file with progress indicator
pub fn download_file(url: &str, file: &Path) -> Result<()> {
    let mut output = std::fs::File::create(file)?;
    let resp = http_client()?.get(url).send()?;

    let mut total: u64 = 0;
    if let Some(length) = resp.headers().get(CONTENT_LENGTH) {
        total = length
            .to_str()
            .ok()
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(0);
    }
    if total > 0 {
        // pre-allocate all the required disk space,
        // fails early when there is insufficient disk space available
        fs3::FileExt::allocate(&output, total)?;
    }

    let progress_bar = indicatif::ProgressBar::new(total);
    progress_bar.set_style(
        indicatif::ProgressStyle::default_bar()
            .template(make_progress_bar!("{bytes}/{total_bytes}"))
            .unwrap(),
    );
    progress_bar.set_draw_target(indicatif::ProgressDrawTarget::stderr_with_hz(5));
    let mut reader = progress_bar.wrap_read(resp);
    std::io::copy(&mut reader, &mut output)?;
    progress_bar.finish_and_clear();

    Ok(())
}

/// Pick the latest BuildKit rootfs according to the recipe
pub fn pick_latest_rootfs(arch: &str) -> Result<RootFsInfo> {
    info!("Picking latest BuildKit for {}", arch);
    let resp = http_client()?
        .get(MANIFEST_URL)
        .send()?
        .error_for_status()?
        .json::<Recipe>()?;

    let buildkit = resp
        .variants
        .into_iter()
        .find(|v| v.name == "BuildKit")
        .ok_or_else(|| anyhow!("Unable to find BuildKit variant"))?;
    let mut rootfs: Vec<RootFsInfo> = buildkit
        .squashfs
        .into_iter()
        .filter(|rootfs| rootfs.arch == arch)
        .collect();

    if rootfs.is_empty() {
        bail!("No suitable squashfs was found")
    }
    rootfs.sort_unstable_by_key(|x| x.date.clone());
    Ok(rootfs.last().unwrap().to_owned())
}

static GIT_PROGRESS: LazyLock<indicatif::ProgressStyle> = LazyLock::new(|| {
    indicatif::ProgressStyle::default_bar()
        .template("[{bar:25.cyan/blue}] {pos}/{len} {msg} ({eta})")
        .unwrap()
});

/// Clone the Git repository to `root`
pub fn download_git(uri: &str, root: &Path) -> Result<()> {
    let mut callbacks = git2::RemoteCallbacks::new();
    let mut co_callback = git2::build::CheckoutBuilder::new();
    let current: Arc<AtomicUsize> = Arc::new(AtomicUsize::new(0usize));
    let total: Arc<AtomicUsize> = Arc::new(AtomicUsize::new(0usize));
    let stage: Arc<AtomicUsize> = Arc::new(AtomicUsize::new(0usize));
    let cur_bytes: Arc<AtomicUsize> = Arc::new(AtomicUsize::new(0usize));

    let current_tx = current.clone();
    let total_tx = total.clone();
    let stage_tx = stage.clone();
    let cur_bytes_tx = cur_bytes.clone();

    callbacks.transfer_progress(move |p: git2::Progress| {
        if p.received_objects() == p.total_objects() {
            current_tx.store(p.indexed_deltas(), Ordering::SeqCst);
            total_tx.store(p.total_deltas(), Ordering::SeqCst);
            stage_tx.store(1, Ordering::SeqCst);
        } else {
            current_tx.store(p.received_objects(), Ordering::SeqCst);
            total_tx.store(p.total_objects(), Ordering::SeqCst);
            cur_bytes_tx.store(p.received_bytes(), Ordering::SeqCst);
        }

        true
    });

    let current_co = current.clone();
    let total_co = total.clone();
    let stage_co = stage.clone();
    let stage_bar = stage.clone();

    co_callback.progress(move |_, cur, ttl| {
        current_co.store(cur, Ordering::SeqCst);
        total_co.store(ttl, Ordering::SeqCst);
        stage_co.store(2, Ordering::SeqCst);
    });
    let mut options = git2::FetchOptions::new();
    options.remote_callbacks(callbacks);
    // drawing progress bar in a separate thread
    let bar = thread::spawn(move || {
        let progress = indicatif::ProgressBar::new(1);
        progress.set_style(GIT_PROGRESS.clone());
        loop {
            let current = current.load(Ordering::SeqCst);
            let total = total.load(Ordering::SeqCst);
            progress.set_length(total as u64);
            progress.set_position(current as u64);

            match stage_bar.load(Ordering::SeqCst) {
                0 => {
                    let human_bytes =
                        indicatif::HumanBytes(cur_bytes.load(Ordering::SeqCst) as u64);
                    progress.set_message(human_bytes.to_string());
                }
                1 => progress.set_message("Resolving deltas..."),
                2 => progress.set_message("Checking out files..."),
                _ => break,
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        progress.finish_and_clear();
    });

    git2::build::RepoBuilder::new()
        .fetch_options(options)
        .with_checkout(co_callback)
        .clone(uri, root)?;
    stage.store(4, Ordering::SeqCst);
    bar.join().unwrap();

    Ok(())
}
