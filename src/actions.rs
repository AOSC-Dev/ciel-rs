use anyhow::{anyhow, Result};
use console::style;
use dialoguer::{Confirm, Input};
use git2::Repository;
use nix::unistd::sync;
use rand::random;
use std::{
    fs,
    path::{Path, PathBuf},
};

use crate::{
    common::is_instance_exists,
    common::{
        self, extract_system_tarball, is_legacy_workspace, CIEL_DATA_DIR, CIEL_DIST_DIR,
        CIEL_INST_DIR,
    },
    machine::spawn_container,
    machine::{get_container_ns_name, inspect_instance},
    network, overlayfs, repo,
};
use crate::{config, machine};
use crate::{error, info};
use crate::{network::download_file_progress, warn};
use common::create_spinner;

const DEFAULT_MOUNTS: &[(&str, &str)] = &[
    ("OUTPUT/debs/", "/debs/"),
    ("TREE", "/tree"),
    ("SRCS", "/var/cache/acbs/tarballs"),
];
const UPDATE_SCRIPT: &str = r#"export DEBIAN_FRONTEND=noninteractive;apt-get -y update && apt-get -y -o Dpkg::Options::="--force-confdef" -o Dpkg::Options::="--force-confnew" full-upgrade"#;

/// Ensure that the directories exist and mounted
macro_rules! ensure_host_sanity {
    () => {{
        let mut extra_options = Vec::new();
        let mut mounts: Vec<(String, &str)> = DEFAULT_MOUNTS
            .into_iter()
            .map(|x| (x.0.to_string(), x.1))
            .collect();
        if let Ok(c) = config::read_config() {
            extra_options = c.extra_options;
            if !c.local_sources {
                // remove SRCS
                mounts.swap_remove(2);
            }
            if c.sep_mount {
                let branch_name = get_branch_name().unwrap_or("HEAD".to_string());
                mounts.push((format!("OUTPUT-{}/debs", branch_name), "/debs/"));
            }
        } else {
            warn!("This workspace is not yet configured, default settings are used.");
        }

        for mount in &mounts {
            fs::create_dir_all(&mount.0)?;
        }

        (extra_options, mounts)
    }};
}

/// A convenience function for iterating over all the instances while executing the actions
#[inline]
pub fn for_each_instance<F: Fn(&str) -> Result<()>>(func: &F) -> Result<()> {
    let instances = machine::list_instances_simple()?;
    for instance in instances {
        eprintln!("{} {}", style(">>>").bold(), instance);
        func(&instance)?;
    }

    Ok(())
}

/// Get the branch name of the workspace TREE repository
#[inline]
fn get_branch_name() -> Result<String> {
    let repo = Repository::open("TREE")?;
    let head = repo.head()?;

    Ok(head
        .shorthand()
        .ok_or_else(|| anyhow!("Unable to resolve Git ref"))?
        .to_owned())
}

fn commit(instance: &str) -> Result<()> {
    get_instance_ns_name(instance)?;
    info!("Un-mounting all the instances...");
    // Un-mount all the instances
    for_each_instance(&unmount_fs)?;
    info!("Committing instance `{}`...", instance);
    let spinner = create_spinner("Committing upper layer...", 200);
    let man = &mut *overlayfs::get_overlayfs_manager(instance)?;
    man.commit()?;
    sync();
    spinner.finish_and_clear();

    Ok(())
}

/// Rollback the container (by removing the upper layer)
fn rollback(instance: &str) -> Result<()> {
    get_instance_ns_name(instance)?;
    info!("Rolling back instance `{}`...", instance);
    let spinner = create_spinner("Removing upper layer...", 200);
    let man = &mut *overlayfs::get_overlayfs_manager(instance)?;
    man.rollback()?;
    sync();
    spinner.finish_and_clear();

    Ok(())
}

/// Remove everything in the current workspace
pub fn farewell(path: &Path) -> Result<()> {
    let delete = Confirm::new()
        .with_prompt("DELETE THIS CIEL WORKSPACE?")
        .interact()?;
    if !delete {
        info!("Not confirmed.");
        return Ok(());
    }
    info!("If you are absolutely sure, please type the following:\nDo as I say!");
    if Input::<String>::new().with_prompt("Your turn").interact()? != "Do as I say!" {
        info!("Prompt answered incorrectly. Not confirmed.");
        return Ok(());
    }

    info!("... as you wish. Commencing destruction ...");
    info!("Un-mounting all the instances...");
    // Un-mount all the instances
    for_each_instance(&unmount_fs)?;
    fs::remove_dir_all(path.join(".ciel"))?;

    Ok(())
}

/// Download the OS tarball and then extract it for use as the base layer
pub fn load_os(url: &str) -> Result<()> {
    info!("Downloading base OS tarball...");
    let path = Path::new(url)
        .file_name()
        .ok_or_else(|| anyhow!("Unable to convert path to string"))?
        .to_str()
        .ok_or_else(|| anyhow!("Unable to decode path string"))?;
    let total = download_file_progress(url, path)?;
    extract_system_tarball(&PathBuf::from(path), total)?;

    Ok(())
}

/// Ask user for the configuration and then apply it
pub fn config_os(instance: &str) -> Result<()> {
    let config;
    if let Ok(c) = config::read_config() {
        config = config::ask_for_config(Some(c));
    } else {
        config = config::ask_for_config(None);
    }
    let man = &mut *overlayfs::get_overlayfs_manager(instance)?;
    if let Ok(c) = config {
        config::apply_config(man.get_config_layer()?, &c)?;
        fs::write(
            Path::new(CIEL_DATA_DIR).join("config.toml"),
            c.save_config()?,
        )?;
        info!("Configuration applied.");
    } else {
        return Err(anyhow!("Could not recognize the configuration."));
    }

    Ok(())
}

/// Mount the filesystem of the instance
pub fn mount_fs(instance: &str) -> Result<()> {
    let man = &mut *overlayfs::get_overlayfs_manager(instance)?;
    machine::mount_layers(man, instance)?;
    info!("{}: filesystem mounted.", instance);

    Ok(())
}

/// Un-mount the filesystem of the container
pub fn unmount_fs(instance: &str) -> Result<()> {
    let man = &mut *overlayfs::get_overlayfs_manager(instance)?;
    let target = std::env::current_dir()?.join(instance);
    let mut retry = 0usize;
    while man.is_mounted(&target)? {
        retry += 1;
        if retry > 10 {
            return Err(anyhow!("Unable to unmount filesystem after 10 attempts."));
        }
        man.unmount(&target)?;
    }
    info!("{}: filesystem un-mounted.", instance);

    Ok(())
}

/// Remove the mount point (usually a directory) of the container overlay filesystem
pub fn remove_mount(instance: &str) -> Result<()> {
    let target = std::env::current_dir()?.join(instance);
    if !target.is_dir() {
        return Err(anyhow!("{} is not a directory.", instance));
    }
    match fs::read_dir(&target) {
        Ok(mut entry) => {
            if entry.any(|_| true) {
                warn!(
                    "Mount point {:?} still contains files, so it will not be removed.",
                    target
                );
                return Ok(());
            }
        }
        Err(e) => {
            error!("Error when querying {:?}: {}", target, e);
        }
    }
    fs::remove_dir(target)?;
    info!("{}: mount point removed.", instance);

    Ok(())
}

/// Show interactive onboarding guide, triggered by issuing `ciel new`
pub fn onboarding() -> Result<()> {
    info!("Welcome to ciel!");
    if Path::new(".ciel").exists() {
        error!("Seems like you've already created a ciel workspace here.");
        info!("Please run `ciel farewell` to nuke it before running this command.");
        return Err(anyhow!("Unable to create a ciel workspace."));
    }
    info!("Before continuing, I need to ask you a few questions:");
    let config = config::ask_for_config(None)?;
    let mut init_instance: Option<String> = None;
    if Confirm::new()
        .with_prompt("Do you want to add a new instance now?")
        .interact()?
    {
        let name: String = Input::new()
            .with_prompt("Name of the instance")
            .interact()?;
        init_instance = Some(name.clone());
        info!(
            "Understood. `{}` will be created after initialization is finished.",
            name
        );
    } else {
        info!("Okay. You can still add a new instance later.");
    }

    info!("Initializing workspace...");
    common::ciel_init()?;
    info!("Initializing container OS...");
    let tarball_url;
    info!("Searching for latest AOSC OS buildkit release...");
    if let Ok(tarball) = network::pick_latest_tarball() {
        info!(
            "Ciel has picked buildkit for {}, released on {}",
            tarball.arch, tarball.date
        );
        tarball_url = format!("https://releases.aosc.io/{}", tarball.path);
    } else {
        warn!(
            "Ciel was unable to find a suitable buildkit release. Please specify the URL manually."
        );
        tarball_url = Input::<String>::new()
            .with_prompt("Tarball URL")
            .interact()?;
    }
    load_os(&tarball_url)?;
    info!("Initializing ABBS tree...");
    network::download_git(network::GIT_TREE_URL, Path::new("TREE"))?;
    config::apply_config(CIEL_DIST_DIR, &config)?;
    info!("Applying configurations...");
    fs::write(
        Path::new(CIEL_DATA_DIR).join("config.toml"),
        config.save_config()?,
    )?;
    info!("Configuration applied.");
    let cwd = std::env::current_dir()?;
    if config.local_repo {
        info!("Setting up local repository ...");
        repo::refresh_repo(&cwd)?;
        info!("Local repository ready.");
    }
    if let Some(init_instance) = init_instance {
        overlayfs::create_new_instance_fs(CIEL_INST_DIR, &init_instance)?;
        info!("Instance `{}` initialized.", init_instance);
        if config.local_repo {
            repo::init_repo(&cwd.join("OUTPUT"), &cwd.join(&init_instance))?;
            info!("Local repository initialized in `{}`.", init_instance);
        }
    }

    Ok(())
}

fn get_instance_ns_name(instance: &str) -> Result<String> {
    if !is_instance_exists(instance) {
        error!("Instance `{}` does not exist.", instance);
        info!(
            "You can add a new instance like this: `ciel add {}`",
            instance
        );
        return Err(anyhow!("Unable to acquire container information."));
    }
    let legacy = is_legacy_workspace()?;

    Ok(get_container_ns_name(instance, legacy)?)
}

/// Start the container/instance, also mounting the container filesystem prior to the action
pub fn start_container(instance: &str) -> Result<String> {
    let ns_name = get_instance_ns_name(instance)?;
    let inst = inspect_instance(instance, &ns_name)?;
    let (extra_options, mounts) = ensure_host_sanity!();
    if !inst.mounted {
        mount_fs(instance)?;
    }
    if !inst.started {
        spawn_container(&ns_name, instance, &extra_options, &mounts)?;
    }

    Ok(ns_name)
}

/// Execute the specified command in the container
pub fn run_in_container(instance: &str, args: &[&str]) -> Result<i32> {
    let ns_name = start_container(instance)?;
    let status = machine::execute_container_command(&ns_name, args)?;

    Ok(status)
}

/// Stop the container/instance (without un-mounting the filesystem)
pub fn stop_container(instance: &str) -> Result<()> {
    let ns_name = get_instance_ns_name(instance)?;
    let inst = inspect_instance(instance, &ns_name)?;
    if !inst.started {
        info!("Instance `{}` is not running!", instance);
        return Ok(());
    }
    info!("Stopping instance `{}`...", instance);
    machine::terminate_container_by_name(&ns_name)?;
    info!("Instance `{}` is stopped.", instance);

    Ok(())
}

/// Stop and un-mount the container and its filesystem
pub fn container_down(instance: &str) -> Result<()> {
    stop_container(instance)?;
    unmount_fs(instance)?;
    remove_mount(instance)?;

    Ok(())
}

/// Commit the container/instance upper layer changes to the base layer of the filesystem
pub fn commit_container(instance: &str) -> Result<()> {
    container_down(instance)?;
    commit(instance)?;
    info!("Instance `{}` has been committed.", instance);

    Ok(())
}

/// Clear the upper layer of the container/instance filesystem
pub fn rollback_container(instance: &str) -> Result<()> {
    container_down(instance)?;
    rollback(instance)?;
    info!("Instance `{}` has been rolled back.", instance);

    Ok(())
}

/// Create a new instance
#[inline]
pub fn add_instance(instance: &str) -> Result<()> {
    overlayfs::create_new_instance_fs(CIEL_INST_DIR, instance)?;
    info!("Instance `{}` created.", instance);

    Ok(())
}

/// Remove the container/instance and its filesystem from the host filesystem
pub fn remove_instance(instance: &str) -> Result<()> {
    container_down(instance)?;
    info!("Removing instance `{}`...", instance);
    let spinner = create_spinner("Removing the instance...", 200);
    let man = &mut *overlayfs::get_overlayfs_manager(instance)?;
    man.destroy()?;
    spinner.finish_and_clear();
    info!("Instance `{}` removed.", instance);

    Ok(())
}

/// Build packages in the container
pub fn package_build<'a, K: IntoIterator<Item = &'a str>>(
    instance: &str,
    packages: K,
) -> Result<i32> {
    let conf = config::read_config();
    if conf.is_err() {
        return Err(anyhow!("Please configure this workspace first!"));
    }
    let conf = conf.unwrap();

    if !conf.local_repo {
        let mut cmd = vec!["/bin/acbs-build", "--"];
        cmd.extend(packages.into_iter());
        let status = run_in_container(instance, &cmd)?;
        return Ok(status);
    }

    let root = std::env::current_dir()?.join("OUTPUT");
    mount_fs(&instance)?;
    repo::init_repo(&root, Path::new(instance))?;
    for package in packages {
        let status = run_in_container(instance, &["/bin/acbs-build", "--", package])?;
        if status != 0 {
            return Ok(status);
        }
        repo::refresh_repo(&root)?;
    }

    Ok(0)
}

/// Update AOSC OS in the container/instance
pub fn update_os() -> Result<()> {
    info!("Updating base OS...");
    let instance = format!("update-{:x}", random::<u32>());
    add_instance(&instance)?;
    run_in_container(&instance, &["/bin/bash", "-ec", UPDATE_SCRIPT])?;
    commit_container(&instance)?;
    remove_instance(&instance)?;

    Ok(())
}
