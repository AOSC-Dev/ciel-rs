use anyhow::{anyhow, Result};
use console::style;
use dialoguer::{Confirm, Input};
use std::{
    ffi::OsStr,
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
    network, overlayfs,
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

macro_rules! ensure_host_sanity {
    () => {{
        let mut extra_options = Vec::new();
        let mut mounts = &DEFAULT_MOUNTS[..2];
        if let Ok(c) = config::read_config() {
            extra_options = c.extra_options;
            if c.local_sources {
                mounts = &DEFAULT_MOUNTS;
            }
        } else {
            warn!("This workspace is not yet configured, default settings are used.");
        }

        for mount in mounts {
            fs::create_dir_all(mount.0)?;
        }

        (extra_options, mounts)
    }};
}

pub fn list_helpers() -> Result<Vec<String>> {
    let exe_dir = std::env::current_exe()?;
    let exe_dir = exe_dir.parent().ok_or_else(|| anyhow!("Where am I?"))?;
    let plugins_dir = exe_dir.join("../libexec/ciel-plugin/").read_dir()?;
    let plugins = plugins_dir
        .filter_map(|x| {
            if let Ok(x) = x {
                let path = x.path();
                let filename = path.file_name().unwrap_or(OsStr::new("")).to_string_lossy();
                if path.is_file() && filename.starts_with("ciel-") {
                    return Some(filename.to_string());
                }
            }
            None
        })
        .collect();

    Ok(plugins)
}

fn commit(instance: &str) -> Result<()> {
    get_instance_ns_name(instance)?;
    info!("Commiting instance `{}`...", instance);
    let spinner = create_spinner("Commiting upper layer...", 200);
    let man = &mut *overlayfs::get_overlayfs_manager(instance)?;
    man.commit()?;
    spinner.finish_and_clear();

    Ok(())
}

fn rollback(instance: &str) -> Result<()> {
    get_instance_ns_name(instance)?;
    info!("Rolling back instance `{}`...", instance);
    let spinner = create_spinner("Removing upper layer...", 200);
    let man = &mut *overlayfs::get_overlayfs_manager(instance)?;
    man.rollback()?;
    spinner.finish_and_clear();

    Ok(())
}

pub fn farewell(path: &Path) -> Result<()> {
    let delete = Confirm::new()
        .with_prompt("DELETE ALL CIEL THINGS?")
        .interact()?;
    if delete {
        fs::remove_dir_all(path.join(".ciel"))?;
    }

    Ok(())
}

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

pub fn mount_fs(instance: &str) -> Result<()> {
    let man = &mut *overlayfs::get_overlayfs_manager(instance)?;
    machine::mount_layers(man, instance)?;
    info!("{}: filesystem mounted.", instance);

    Ok(())
}

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

pub fn onboarding() -> Result<()> {
    info!("Welcome to ciel!");
    if Path::new(".ciel").exists() {
        error!("Seems like you've already created a ciel workspace here.");
        info!("Please run `ciel farewell` before running this command.");
        return Err(anyhow!("Unable to create ciel workspace."));
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
    load_os("https://releases.aosc.io/os-amd64/buildkit/aosc-os_buildkit_latest_amd64.tar.xz")?;
    info!("Initializing ABBS tree...");
    network::download_git(network::GIT_TREE_URL, Path::new("TREE"))?;
    config::apply_config(CIEL_DIST_DIR, &config)?;
    info!("Applying configurations...");
    fs::write(
        Path::new(CIEL_DATA_DIR).join("config.toml"),
        config.save_config()?,
    )?;
    info!("Configuration applied.");
    if let Some(init_instance) = init_instance {
        overlayfs::create_new_instance_fs(CIEL_INST_DIR, &init_instance)?;
        info!("Instance `{}` initialized.", init_instance);
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

pub fn start_container(instance: &str) -> Result<String> {
    let ns_name = get_instance_ns_name(instance)?;
    let inst = inspect_instance(instance, &ns_name)?;
    let (extra_options, mounts) = ensure_host_sanity!();
    if !inst.mounted {
        mount_fs(instance)?;
    }
    if !inst.started {
        spawn_container(&ns_name, instance, &extra_options, mounts)?;
    }

    Ok(ns_name)
}

pub fn run_in_container(instance: &str, args: &[&str]) -> Result<i32> {
    let ns_name = start_container(instance)?;
    let status = machine::execute_container_command(&ns_name, args)?;

    Ok(status)
}

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

pub fn container_down(instance: &str) -> Result<()> {
    stop_container(instance)?;
    unmount_fs(instance)?;
    remove_mount(instance)?;

    Ok(())
}

pub fn commit_container(instance: &str) -> Result<()> {
    container_down(instance)?;
    commit(instance)?;
    info!("Instance `{}` has been commited.", instance);

    Ok(())
}

pub fn rollback_container(instance: &str) -> Result<()> {
    container_down(instance)?;
    rollback(instance)?;
    info!("Instance `{}` has been rolled back.", instance);

    Ok(())
}

#[inline]
pub fn add_instance(instance: &str) -> Result<()> {
    overlayfs::create_new_instance_fs(CIEL_INST_DIR, instance)?;
    info!("Instance `{}` created.", instance);

    Ok(())
}

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
