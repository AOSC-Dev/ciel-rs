use crate::make_progress_bar;
use anyhow::{anyhow, Result};
use fs3::FileExt;
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;
use std::{
    sync::{atomic::Ordering, Arc},
};
use ureq::{http::Response, Agent};

const MANIFEST_URL: &str = "https://releases.aosc.io/manifest/recipe.json";

#[derive(Deserialize, Debug, Clone)]
pub struct RootFs {
    pub arch: String,
    pub date: String,
    pub path: String,
    pub sha256sum: String,
}

#[derive(Deserialize)]
pub struct Variant {
    name: String,
    squashfs: Vec<RootFs>,
}

/// AOSC OS Tarball Recipe structure
#[derive(Deserialize)]
pub struct Recipe {
    pub version: usize,
    variants: Vec<Variant>,
}

/// Download a file from the web
pub fn download_file(url: &str) -> Result<Response<ureq::Body>> {
    Ok(Agent::new_with_defaults().get(url).call()?)
}

/// Download a file with progress indicator
pub fn download_file_progress(url: &str, file: &str) -> Result<u64> {
    let mut output = std::fs::File::create(file)?;
    let resp = download_file(url)?;
    let (parts, body) = resp.into_parts();
    let mut total: u64 = 0;
    if let Some(length) = parts.headers.get("content-length") {
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
    let mut reader = progress_bar.wrap_read(body.into_reader());
    std::io::copy(&mut reader, &mut output)?;
    progress_bar.finish_and_clear();

    Ok(total)
}

/// Pick the latest buildkit rootfs according to the recipe
pub fn pick_latest_rootfs(arch: &str) -> Result<RootFs> {
    let mut resp = Agent::new_with_defaults().get(MANIFEST_URL).call()?;
    let recipe: Recipe = resp.body_mut().read_json()?;
    if recipe.version != 1 {
        return Err(anyhow!(
            "Unsupported recipe version {}, expected 1",
            recipe.version
        ));
    }
    let buildkit = recipe
        .variants
        .into_iter()
        .find(|v| v.name == "BuildKit")
        .ok_or_else(|| anyhow!("Unable to find buildkit variant"))?;

    let mut rootfs: Vec<RootFs> = buildkit
        .squashfs
        .into_iter()
        .filter(|rootfs| rootfs.arch == arch)
        .collect();

    if rootfs.is_empty() {
        return Err(anyhow!("No suitable squashfs was found"));
    }
    rootfs.sort_unstable_by_key(|x| x.date.clone());

    Ok(rootfs.last().unwrap().to_owned())
}

/// Clone the Git repository to `root`
pub fn download_git(uri: &str, root_path: &std::path::Path) -> Result<()> {
    let progress_root: Arc<gix::progress::tree::Root> = gix::progress::tree::root::Options {
        initial_capacity: 20,
        message_buffer_capacity: 20,
    }
    .into();

    let root_weak = Arc::downgrade(&progress_root);
    let is_interrupted = &gix::interrupt::IS_INTERRUPTED;

    let render_thread = std::thread::spawn(move || {
        let multi = MultiProgress::new();
        let mut bars: HashMap<gix::progress::Id, ProgressBar> = HashMap::new();
        let mut tasks = Vec::new();

        let read_pack_bytes_id: gix::progress::Id = gix::odb::pack::bundle::write::ProgressId::ReadPackBytes.into();
        let index_objects_id: gix::progress::Id = gix::odb::pack::index::write::ProgressId::IndexObjects.into();
        let resolve_objects_id: gix::progress::Id = gix::odb::pack::index::write::ProgressId::ResolveObjects.into();

        while let Some(root) = root_weak.upgrade() {
            root.sorted_snapshot(&mut tasks);

            for (_key, task) in &tasks {
                let task_id = task.id;
                let progress = match &task.progress {
                    Some(p) => p,
                    None => continue,
                };

                let pb = bars.entry(task_id).or_insert_with(|| {
                    let new_pb = multi.add(ProgressBar::new(0));
                    
                    if task_id == read_pack_bytes_id {
                        new_pb.set_style(ProgressStyle::with_template(
                            "{prefix:>18.yellow.bold} {binary_bytes:>10} ({binary_bytes_per_sec}) {msg}"
                        ).unwrap());
                        new_pb.set_prefix("Downloading");
                    } else if task_id == index_objects_id {
                        new_pb.set_style(ProgressStyle::with_template(
                            "{prefix:>18.green.bold} [{bar:40.green/white}] {pos}/{len} {msg}"
                        ).unwrap());
                        new_pb.set_prefix("Indexing");
                    } else if task_id == resolve_objects_id {
                        new_pb.set_style(ProgressStyle::with_template(
                            "{prefix:>18.magenta.bold} [{bar:40.magenta/white}] {pos}/{len} {msg}"
                        ).unwrap());
                        new_pb.set_prefix("Resolving");
                    } else {
                        new_pb.set_style(ProgressStyle::with_template(
                            "{prefix:>18.cyan.bold} [{bar:40.cyan/white}] {pos}/{len}"
                        ).unwrap());
                        new_pb.set_prefix(task.name.to_string());
                    }
                    new_pb
                });

                let current = progress.step.load(Ordering::Relaxed);
                if let Some(total) = progress.done_at {
                    pb.set_length(total as u64);
                } else {
                    pb.set_style(ProgressStyle::with_template(
                        "{prefix:>18.cyan.bold} {pos}"
                    ).unwrap());
                }

                pb.set_position(current as u64);

                match progress.done_at {
                    Some(t) if current >= t && t > 0 => pb.finish_and_clear(),
                    None if current == 0 => pb.finish_and_clear(),
                    _ => {}
                }

                bars.retain(|id, pb| {
                    let task_in_snapshot = tasks.iter().find(|(_, t)| t.id == *id);
                    
                    match task_in_snapshot {
                        Some((_, t)) => {
                            if t.progress.is_none() {
                                pb.finish_and_clear();
                                return false;
                            }
                            true
                        }
                        None => {
                            pb.finish_and_clear();
                            false
                        }
                    }
                });
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
    });

    let url = gix::url::parse(uri.into())?;
    let mut prepare = gix::prepare_clone(url, root_path)?;

    let mut progress_item = progress_root.add_child("clone");

    let (mut checkout, _) = prepare.fetch_then_checkout(&mut progress_item, &is_interrupted)?;
    checkout.main_worktree(&mut progress_item, &is_interrupted)?;

    drop(progress_root);
    let _ = render_thread.join();

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
