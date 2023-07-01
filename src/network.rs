use crate::make_progress_bar;
use anyhow::{anyhow, Result};
use fs3::FileExt;
use lazy_static::lazy_static;
use reqwest::blocking::{Client, Response};
use serde::Deserialize;
use std::path::Path;
use std::{
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    thread::{self, sleep},
    time::Duration,
};

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
        .template("[{bar:25.cyan/blue}] {pos}/{len} {msg} ({eta})")
        .unwrap();
}

/// Download a file from the web
pub fn download_file(url: &str) -> Result<Response> {
    let client = Client::new().get(url).send()?;

    Ok(client)
}

/// Download a file with progress indicator
pub fn download_file_progress(url: &str, file: &str) -> Result<u64> {
    let mut output = std::fs::File::create(file)?;
    let resp = download_file(url)?;
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
            .template(make_progress_bar!("{bytes}/{total_bytes}"))
            .unwrap(),
    );
    progress_bar.set_draw_target(indicatif::ProgressDrawTarget::stderr_with_hz(5));
    let mut reader = progress_bar.wrap_read(resp);
    std::io::copy(&mut reader, &mut output)?;
    progress_bar.finish_and_clear();

    Ok(total)
}

/// Pick the latest buildkit tarball according to the recipe
pub fn pick_latest_tarball(arch: &str) -> Result<Tarball> {
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
                    progress.set_message(human_bytes.to_string());
                }
                1 => progress.set_message("Resolving deltas..."),
                2 => progress.set_message("Checking out files..."),
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

// other Git operations
fn find_branch<'a>(repo: &'a git2::Repository, name: &str) -> Result<git2::Branch<'a>> {
    let branch = repo.find_branch(name, git2::BranchType::Local);
    if let Ok(branch) = branch {
        return Ok(branch);
    }
    let remote_branch = repo.find_branch(&format!("origin/{}", name), git2::BranchType::Remote);
    if let Ok(branch) = remote_branch {
        let target_commit = branch.get().peel_to_commit()?;
        let branch = repo.branch(name, &target_commit, false)?;
        return Ok(branch);
    }

    Err(anyhow!("Could not find branch `{}'", name))
}

pub fn fetch_repo<P: AsRef<Path>>(path: P) -> Result<git2::Repository> {
    let repo = git2::Repository::open(path.as_ref())?;
    let mut remote = repo.find_remote("origin")?;
    let refs = remote.fetch_refspecs()?;
    let refspecs = refs.into_iter().flatten().collect::<Vec<_>>();
    let mut opts = git2::FetchOptions::new();
    opts.prune(git2::FetchPrune::On);
    remote.fetch(&refspecs, Some(&mut opts), None)?;
    drop(remote); // dis-own the variable `repo`

    Ok(repo)
}

pub fn git_switch_branch(
    repo: &mut git2::Repository,
    branch: &str,
    rebase_from: Option<&str>,
) -> Result<bool> {
    let target_branch = find_branch(repo, branch).unwrap();
    let branch_ref = target_branch.into_reference();
    let branch_refname = branch_ref.name().unwrap().to_string();
    drop(branch_ref);
    let stasher = git2::Signature::now("ciel", "bot@aosc.io")?;
    let repo_statuses = repo.statuses(None)?;
    let is_tree_dirty = !repo_statuses.is_empty();
    drop(repo_statuses);
    if is_tree_dirty {
        repo.stash_save(
            &stasher,
            "ciel auto save",
            Some(git2::StashFlags::INCLUDE_UNTRACKED),
        )?;
    }
    repo.set_head(&branch_refname)?;
    let mut opts = git2::build::CheckoutBuilder::new();
    repo.checkout_head(Some(opts.force()))?;
    repo.cleanup_state()?;
    if is_tree_dirty && rebase_from.is_none() {
        repo.stash_pop(0, None)?;
    }
    if let Some(rebase_upstream) = rebase_from {
        // attempt rebase
        let status = std::process::Command::new("git")
            .args(["rebase", rebase_upstream])
            .current_dir(repo.workdir().unwrap())
            .spawn()?
            .wait()?;
        if !status.success() {
            return Err(anyhow!("Error performing rebase"));
        }
        repo.cleanup_state()?;
        if is_tree_dirty {
            repo.stash_pop(0, None)?;
        }
    }

    // returns whether a stash was made
    Ok(is_tree_dirty)
}
