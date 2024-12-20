use anyhow::{anyhow, Result};
use clap::ArgMatches;
use console::{style, user_attended};
use dialoguer::{theme::ColorfulTheme, Confirm, Input};
use git2::Repository;
use nix::unistd::sync;
use rand::random;
use std::{collections::HashMap, ffi::OsStr, fs, path::Path};

use crate::{
    actions::{patch_instance_config, OMA_UPDATE_SCRIPT},
    common::*,
    config::{self, InstanceConfig, WorkspaceConfig},
    error, info,
    machine::{self, get_container_ns_name, inspect_instance, spawn_container},
    network::download_file_progress,
    overlayfs, warn,
};

use super::{for_each_instance, APT_UPDATE_SCRIPT};

/// Get the branch name of the workspace TREE repository
#[inline]
pub fn get_branch_name() -> Result<String> {
    let repo = Repository::open("TREE")?;
    let head = repo.head()?;

    Ok(head
        .shorthand()
        .ok_or_else(|| anyhow!("Unable to resolve Git ref"))?
        .to_owned())
}

/// Determine the output directory name
#[inline]
pub fn get_output_directory(sep_mount: bool) -> String {
    if sep_mount {
        format!(
            "OUTPUT-{}",
            get_branch_name().unwrap_or_else(|_| "HEAD".to_string())
        )
    } else {
        "OUTPUT".to_string()
    }
}

fn commit(instance: &str) -> Result<()> {
    get_instance_ns_name(instance)?;
    info!("Un-mounting all the instances...");
    // Un-mount all the instances
    for_each_instance(&container_down)?;
    info!("{}: committing instance...", instance);
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
    info!("{}: rolling back instance...", instance);
    let spinner = create_spinner("Removing upper layer...", 200);
    let man = &mut *overlayfs::get_overlayfs_manager(instance)?;
    man.rollback()?;
    sync();
    spinner.finish_and_clear();

    Ok(())
}

/// Remove everything in the current workspace
pub fn farewell(path: &Path, force: bool) -> Result<()> {
    if !user_attended() {
        eprintln!("DELETE THIS CIEL WORKSPACE?");
        info!("Not controlled by an user. Automatically confirmed.");
    }
    if !user_attended() || force {
        // Un-mount all the instances
        info!("Un-mounting all the instances ...");
        for_each_instance(&container_down)?;
        info!("Removing workspace directory ...");
        fs::remove_dir_all(path.join(".ciel"))?;
        return Ok(());
    }
    let theme = ColorfulTheme::default();
    let delete = Confirm::with_theme(&theme)
        .with_prompt("DELETE THIS CIEL WORKSPACE?")
        .default(false)
        .interact()?;
    if !delete {
        info!("Not confirmed.");
        return Ok(());
    }
    info!(
        "If you are absolutely sure, please type the following:\n{}",
        style("Do as I say!").bold()
    );
    if Input::<String>::with_theme(&theme)
        .with_prompt("Your turn")
        .interact()?
        != "Do as I say!"
    {
        info!("Prompt answered incorrectly. Not confirmed.");
        return Ok(());
    }

    info!("... as you wish. Commencing destruction ...");
    // Un-mount all the instances
    info!("Un-mounting all the instances ...");
    for_each_instance(&container_down)?;
    info!("Removing workspace directory ...");
    fs::remove_dir_all(path.join(".ciel"))?;

    Ok(())
}

/// Download the OS tarball and then extract it for use as the base layer
pub fn load_os(url: &str, sha256: Option<String>, tarball: bool) -> Result<()> {
    let path = Path::new(url);
    let filename = path
        .file_name()
        .ok_or_else(|| anyhow!("Unable to convert path to string"))?
        .to_str()
        .ok_or_else(|| anyhow!("Unable to decode path string"))?;
    let is_local_file = path.is_file();
    let total = if !is_local_file {
        info!("Downloading base OS rootfs...");
        download_file_progress(url, filename)?
    } else {
        let tarball = fs::File::open(path)?;
        tarball.metadata()?.len()
    };
    if let Some(sha256) = sha256 {
        info!("Verifying tarball checksum...");
        let tarball = fs::File::open(Path::new(filename))?;
        let checksum = sha256sum(tarball)?;
        if sha256 == checksum {
            info!("Checksum verified.");
        } else {
            return Err(anyhow!(
                "Checksum mismatch: expected {} but got {}",
                sha256,
                checksum
            ));
        }
    }

    if is_local_file {
        extract_system_rootfs(path, total, tarball)?;
    } else {
        extract_system_rootfs(Path::new(filename), total, tarball)?;
    }

    Ok(())
}

/// Mount the filesystem of the instance
pub fn mount_fs(instance: &str) -> Result<()> {
    let workspace_config = WorkspaceConfig::load()?;
    let instance_config_ref = InstanceConfig::get(instance)?;
    let instance_config = instance_config_ref.read().unwrap();

    let man = &mut *overlayfs::get_overlayfs_manager(instance)?;
    man.set_volatile(workspace_config.volatile_mount)?;

    machine::mount_layers(man, instance)?;
    info!("{}: filesystem mounted.", instance);

    config::apply_config(man.get_config_layer()?, &workspace_config, &instance_config)?;
    info!("{}: configuration applied.", instance);

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
    if !target.exists() {
        return Ok(());
    } else if !target.is_dir() {
        warn!("{}: mount point is not a directory.", instance);
        return Ok(());
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

    get_container_ns_name(instance, legacy)
}

/// Start the container/instance, also mounting the container filesystem prior to the action
pub fn start_container(instance: &str) -> Result<String> {
    let ns_name = get_instance_ns_name(instance)?;
    let inst = inspect_instance(instance, &ns_name)?;

    let workspace_config = WorkspaceConfig::load().unwrap_or_default();

    let mut extra_options = InstanceConfig::get(instance)?
        .read()
        .unwrap()
        .nspawn_options
        .clone();
    extra_options.extend_from_slice(&workspace_config.nspawn_options);

    let mut mounts = HashMap::new();
    mounts.insert("/tree".to_string(), "TREE".to_string());
    mounts.insert("/var/cache/apt/archives".to_string(), "CACHE".to_string());
    if workspace_config.local_sources {
        mounts.insert("/var/cache/acbs/tarballs".to_string(), "SRCS".to_string());
    }
    mounts.insert(
        "/debs".to_string(),
        format!("{}/debs", get_output_directory(workspace_config.sep_mount)),
    );

    if std::env::var("CIEL_OFFLINE").is_ok() {
        // FIXME: does not work with current version of systemd
        // add the offline option (private-network means don't share the host network)
        extra_options.push("--private-network".to_string());
        info!("{}: network isolated.", instance);
    }

    if !inst.mounted {
        mount_fs(instance)?;
    }
    if !inst.started {
        spawn_container(&ns_name, instance, &extra_options, &mounts)?;
    }

    Ok(ns_name)
}

/// Execute the specified command in the container
pub fn run_in_container<S: AsRef<OsStr>>(instance: &str, args: &[S]) -> Result<i32> {
    let ns_name = start_container(instance)?;
    let status = machine::execute_container_command(&ns_name, args)?;

    Ok(status)
}

/// Stop the container/instance (without un-mounting the filesystem)
pub fn stop_container(instance: &str) -> Result<()> {
    let ns_name = get_instance_ns_name(instance)?;
    let inst = inspect_instance(instance, &ns_name)?;
    if !inst.started {
        info!("{}: instance is not running!", instance);
        return Ok(());
    }
    info!("{}: stopping...", instance);
    machine::terminate_container_by_name(&ns_name)?;
    machine::clean_child_process();
    info!("{}: instance stopped.", instance);

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
    info!("{}: instance has been committed.", instance);

    Ok(())
}

/// Clear the upper layer of the container/instance filesystem
pub fn rollback_container(instance: &str) -> Result<()> {
    container_down(instance)?;
    rollback(instance)?;
    info!("{}: instance has been rolled back.", instance);

    Ok(())
}

/// Create a new instance
#[inline]
pub fn add_instance(instance: &str, tmpfs: bool) -> Result<()> {
    overlayfs::create_new_instance_fs(CIEL_INST_DIR, instance, tmpfs)?;

    let mut config = InstanceConfig::default();
    if tmpfs {
        warn!(
            "{}: tmpfs is an experimental feature, use at your own risk!",
            instance
        );
        config.tmpfs = Some(Default::default());
    }
    config.save(instance)?;

    info!("{}: instance created.", instance);
    Ok(())
}

/// Remove the container/instance and its filesystem from the host filesystem
pub fn remove_instance(instance: &str) -> Result<()> {
    container_down(instance)?;
    info!("{}: removing instance...", instance);
    let spinner = create_spinner("Removing the instance...", 200);
    let man = &mut *overlayfs::get_overlayfs_manager(instance)?;
    man.destroy()?;
    spinner.finish_and_clear();
    info!("{}: instance removed.", instance);

    Ok(())
}

/// Update AOSC OS in the container/instance
pub fn update_os(force_use_apt: bool, args: Option<&ArgMatches>) -> Result<()> {
    info!("Updating base OS ...");
    let instance = format!("update-{:x}", random::<u32>());
    add_instance(&instance, false)?;

    if let Some(args) = args {
        let config_ref = InstanceConfig::get(&instance)?;
        let mut config = config_ref.write().unwrap();
        patch_instance_config(&instance, args, &mut config)?;
        config.save(&instance)?
    }

    if force_use_apt {
        return apt_update_os(&instance);
    }

    let status = run_in_container(&instance, &["/bin/bash", "-ec", OMA_UPDATE_SCRIPT])?;
    if status != 0 {
        return apt_update_os(&instance);
    }

    commit_container(&instance)?;
    remove_instance(&instance)?;

    Ok(())
}

fn apt_update_os(instance: &str) -> Result<()> {
    let status = run_in_container(instance, &["/bin/bash", "-ec", APT_UPDATE_SCRIPT])?;

    if status != 0 {
        return Err(anyhow!("Failed to update OS: {}", status));
    }

    commit_container(instance)?;
    remove_instance(instance)?;

    Ok(())
}
