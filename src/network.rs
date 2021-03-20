use crate::make_progress_bar;
use anyhow::{anyhow, Result};
use lazy_static::lazy_static;
use progress_streams::ProgressReader;
use reqwest::blocking::{Client, Response};
use serde::Deserialize;
use std::{env::consts::ARCH, path::Path};
use std::{
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    thread::{self, sleep},
    time::Duration,
};
use fs3::FileExt;

pub const GIT_TREE_URL: &str = "https://github.com/AOSC-Dev/aosc-os-abbs.git";
const MANIFEST_URL: &str = "https://releases.aosc.io/manifest/recipe.json";

#[derive(Deserialize, Debug, Clone)]
pub struct Tarball {
    pub arch: String,
    pub date: String,
    pub path: String,
    pub sha256sum: String,
}

#[derive(Deserialize)]
pub struct Variant {
    name: String,
    tarballs: Vec<Tarball>,
}

/// AOSC OS Tarball Recipe structure
#[derive(Deserialize)]
pub struct Recipe {
    pub version: usize,
    variants: Vec<Variant>,
}

lazy_static! {
    static ref GIT_PROGRESS: indicatif::ProgressStyle = indicatif::ProgressStyle::default_bar()
        .template("[{bar:25.cyan/blue}] {pos}/{len} {msg} ({eta})");
}

/// Download a file from the web
pub fn download_file(url: &str) -> Result<Response> {
    let client = Client::new().get(url).send()?;

    Ok(client)
}

/// Download a file with progress indicator
pub fn download_file_progress(url: &str, file: &str) -> Result<u64> {
    let mut output = std::fs::File::create(file)?;
    let mut resp = download_file(url)?;
    let mut total: u64 = 0;
    if let Some(length) = resp.headers().get("content-length") {
        total = length.to_str().unwrap_or("0").parse::<u64>().unwrap_or(0);
    }
    if total > 0 {
        // pre-allocate all the required disk space, 
        // fails early when there is insufficient disk space available
        output.allocate(total)?;
    }
    let progress_bar = indicatif::ProgressBar::new(total);
    progress_bar.set_style(
        indicatif::ProgressStyle::default_bar()
            .template(make_progress_bar!("{bytes}/{total_bytes}")),
    );
    progress_bar.enable_steady_tick(500);
    let mut reader = ProgressReader::new(&mut resp, |progress: usize| {
        progress_bar.inc(progress as u64);
    });
    std::io::copy(&mut reader, &mut output)?;
    progress_bar.finish_and_clear();

    Ok(total)
}

/// AOSC OS specific architecture mapping for ppc64
#[cfg(target_arch = "powerpc64")]
#[inline]
fn get_arch_name() -> Option<&'static str> {
    let mut endian: libc::c_int = -1;
    let result;
    unsafe {
        result = libc::prctl(libc::PR_GET_ENDIAN, &mut endian as *mut libc::c_int);
    }
    if result < 0 {
        return None;
    }
    match endian {
        libc::PR_ENDIAN_LITTLE | libc::PR_ENDIAN_PPC_LITTLE => Some("ppc64el"),
        libc::PR_ENDIAN_BIG => Some("ppc64"),
        _ => None,
    }
}

/// AOSC OS specific architecture mapping table
#[cfg(not(target_arch = "powerpc64"))]
#[inline]
fn get_arch_name() -> Option<&'static str> {
    match ARCH {
        "x86_64" => Some("amd64"),
        "x86" => Some("i486"),
        "powerpc" => Some("powerpc"),
        "aarch64" => Some("arm64"),
        "mips64" => Some("loongson3"),
        _ => None,
    }
}

/// Pick the latest buildkit tarball according to the recipe
pub fn pick_latest_tarball() -> Result<Tarball> {
    let arch = get_arch_name().ok_or_else(|| anyhow!("Unsupported architecture"))?;
    let resp = Client::new().get(MANIFEST_URL).send()?;
    let recipe: Recipe = resp.json()?;
    let buildkit = recipe
        .variants
        .into_iter()
        .find(|v| v.name == "BuildKit")
        .ok_or_else(|| anyhow!("Unable to find buildkit variant"))?;
    let mut tarballs: Vec<Tarball> = buildkit
        .tarballs
        .into_iter()
        .filter(|tarball| tarball.arch == arch)
        .collect();
    if tarballs.is_empty() {
        return Err(anyhow!("No suitable tarball was found"));
    }
    tarballs.sort_unstable_by_key(|x| x.date.clone());

    Ok(tarballs.last().unwrap().to_owned())
}

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
                    progress.set_message(&format!("{}", human_bytes));
                }
                1 => {
                    progress.set_message("Resolving deltas...");
                }
                2 => {
                    progress.set_message("Checking out files...");
                }
                _ => break,
            }
            sleep(Duration::from_millis(100));
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
