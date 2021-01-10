use anyhow::{anyhow, Result};
use lazy_static::lazy_static;
use progress_streams::ProgressReader;
use reqwest::blocking::{Client, Response};
use serde::Deserialize;
use std::{env::consts::ARCH, path::Path};

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

#[derive(Deserialize)]
pub struct Recipe {
    pub version: usize,
    variants: Vec<Variant>,
}

lazy_static! {
    static ref GIT_PROGRESS: indicatif::ProgressStyle = indicatif::ProgressStyle::default_bar()
        .template("[{bar:40.cyan/blue}] {pos}/{len} {msg} ({eta})");
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
    let progress_bar = indicatif::ProgressBar::new(total);
    progress_bar.set_style(indicatif::ProgressStyle::default_bar().template(
        "{spinner} [{bar:40.cyan/blue}] {bytes}/{total_bytes} ({bytes_per_sec}, eta {eta})",
    ));
    progress_bar.enable_steady_tick(500);
    let mut reader = ProgressReader::new(&mut resp, |progress: usize| {
        progress_bar.inc(progress as u64);
    });
    std::io::copy(&mut reader, &mut output)?;
    progress_bar.finish_and_clear();

    Ok(total)
}

#[inline]
fn get_arch_name() -> Option<&'static str> {
    match ARCH {
        "x86_64" => Some("amd64"),
        "x86" => Some("i486"),
        "powerpc" => Some("powerpc"),
        "powerpc64" => Some("ppc64el"),
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

    Ok(tarballs.first().unwrap().to_owned())
}

/// Clone the Git repository to `root`
pub fn download_git(uri: &str, root: &Path) -> Result<()> {
    let mut callbacks = git2::RemoteCallbacks::new();
    let mut co_callback = git2::build::CheckoutBuilder::new();
    let progress_dl = indicatif::ProgressBar::new(1);
    let progress_res = indicatif::ProgressBar::new(1);
    let progress_co = indicatif::ProgressBar::new(1);

    progress_dl.set_style(GIT_PROGRESS.clone());
    progress_res.set_style(GIT_PROGRESS.clone());
    progress_co.set_style(GIT_PROGRESS.clone());

    progress_dl.set_message("Waiting for server...");
    progress_dl.set_position(0);

    callbacks.transfer_progress(move |p: git2::Progress| {
        if p.received_objects() == p.total_objects() {
            progress_res.set_message("Resolving deltas...");
            progress_res.set_length(p.total_deltas() as u64);
            progress_res.set_position(p.indexed_deltas() as u64);
        } else {
            let human_bytes = indicatif::HumanBytes(p.received_bytes() as u64);
            progress_dl.set_position(p.received_objects() as u64);
            progress_dl.set_length(p.total_objects() as u64);
            progress_dl.set_message(&format!("{}", human_bytes));
        }

        true
    });

    co_callback.progress(move |_, cur, total| {
        progress_co.set_message("Checking out files...");
        progress_co.set_length(total as u64);
        progress_co.set_position(cur as u64);
    });
    let mut options = git2::FetchOptions::new();
    options.remote_callbacks(callbacks);
    git2::build::RepoBuilder::new()
        .fetch_options(options)
        .with_checkout(co_callback)
        .clone(uri, root)?;

    Ok(())
}
