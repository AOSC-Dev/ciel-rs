use anyhow::{anyhow, Result};
use console::style;
use dialoguer::{theme::ColorfulTheme, Confirm, Input};
use git2::Repository;
use nix::unistd::sync;
use rand::random;
use std::{
    fs,
    path::{Path, PathBuf},
};

use crate::{
    common::*,
    config, ensure_host_sanity, error, info,
    machine::{self, get_container_ns_name, inspect_instance, spawn_container},
    network::download_file_progress,
    overlayfs, warn,
};

use super::{for_each_instance, DEFAULT_MOUNTS, UPDATE_SCRIPT};

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
pub fn farewell(path: &Path) -> Result<()> {
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
    info!("Un-mounting all the instances...");
    // Un-mount all the instances
    for_each_instance(&container_down)?;
    fs::remove_dir_all(path.join(".ciel"))?;

    Ok(())
}

/// Download the OS tarball and then extract it for use as the base layer
pub fn load_os(url: &str, sha256: Option<String>) -> Result<()> {
    info!("Downloading base OS tarball...");
    let path = Path::new(url)
        .file_name()
        .ok_or_else(|| anyhow!("Unable to convert path to string"))?
        .to_str()
        .ok_or_else(|| anyhow!("Unable to decode path string"))?;
    let total;
    if !Path::new(path).is_file() {
        total = download_file_progress(url, path)?;
    } else {
        let tarball = fs::File::open(path)?;
        total = tarball.metadata()?.len();
    }
    if let Some(sha256) = sha256 {
        info!("Verifying tarball checksum...");
        let tarball = fs::File::open(path)?;
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
    extract_system_tarball(&PathBuf::from(path), total)?;

    Ok(())
}

/// Ask user for the configuration and then apply it
pub fn config_os(instance: Option<&str>) -> Result<()> {
    let config;
    let mut prev_voltile = None;
    if let Ok(c) = config::read_config() {
        prev_voltile = Some(c.volatile_mount);
        config = config::ask_for_config(Some(c));
    } else {
        config = config::ask_for_config(None);
    }
    let path;
    if let Some(instance) = instance {
        let man = &mut *overlayfs::get_overlayfs_manager(instance)?;
        path = man.get_config_layer()?;
    } else {
        path = PathBuf::from(CIEL_DIST_DIR);
    }
    if let Ok(c) = config {
        info!("Shutting down instance(s) before applying config...");
        if let Some(instance) = instance {
            container_down(instance)?;
        } else {
            for_each_instance(&container_down)?;
        }
        config::apply_config(path, &c)?;
        fs::create_dir_all(CIEL_DATA_DIR)?;
        fs::write(
            Path::new(CIEL_DATA_DIR).join("config.toml"),
            c.save_config()?,
        )?;
        info!("Configurations applied.");
        let volatile_changed = if let Some(prev_voltile) = prev_voltile {
            prev_voltile != c.volatile_mount
        } else {
            false
        };
        if volatile_changed {
            warn!("You have changed the volatile mount option, please save your work and\x1b[1m\x1b[93m rollback \x1b[4mall the instances\x1b[0m.");
            return Ok(());
        }
        warn!(
            "Please rollback {} for the new config to take effect!",
            if let Some(inst) = instance {
                inst
            } else {
                "all your instances"
            }
        );
    } else {
        return Err(anyhow!("Could not recognize the configuration."));
    }

    Ok(())
}

/// Mount the filesystem of the instance
pub fn mount_fs(instance: &str) -> Result<()> {
    let config = config::read_config()?;
    let man = &mut *overlayfs::get_overlayfs_manager(instance)?;
    man.set_volatile(config.volatile_mount)?;
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

    Ok(get_container_ns_name(instance, legacy)?)
}

/// Start the container/instance, also mounting the container filesystem prior to the action
pub fn start_container(instance: &str) -> Result<String> {
    let ns_name = get_instance_ns_name(instance)?;
    let inst = inspect_instance(instance, &ns_name)?;
    let (mut extra_options, mounts) = ensure_host_sanity!();
    if std::env::var("CIEL_OFFLINE").is_ok() {
        // add the offline option (private-network means don't share the host network)
        extra_options.push("--private-network".to_string());
        info!("{}: network disconnected.", instance);
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
        info!("{}: instance is not running!", instance);
        return Ok(());
    }
    info!("{}: stopping...", instance);
    machine::terminate_container_by_name(&ns_name)?;
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
pub fn add_instance(instance: &str) -> Result<()> {
    overlayfs::create_new_instance_fs(CIEL_INST_DIR, instance)?;
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
pub fn update_os() -> Result<()> {
    info!("Updating base OS...");
    let instance = format!("update-{:x}", random::<u32>());
    add_instance(&instance)?;
    let status = run_in_container(&instance, &["/bin/bash", "-ec", UPDATE_SCRIPT])?;
    if status != 0 {
        return Err(anyhow!("Failed to update OS: {}", status));
    }
    commit_container(&instance)?;
    remove_instance(&instance)?;

    Ok(())
}
