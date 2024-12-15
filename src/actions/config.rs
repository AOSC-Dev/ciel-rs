use std::{fs, path::Path};

use anyhow::{anyhow, Result};
use clap::ArgMatches;

use crate::{
    actions::rollback_container,
    config::{self, workspace_config, InstanceConfig},
    info, warn, CIEL_DATA_DIR,
};

use super::{container_down, for_each_instance};

/// Ask user for the configuration and then apply it
pub fn config_workspace(args: &ArgMatches) -> Result<()> {
    let config;

    let mut prev_volatile = None;
    if let Ok(c) = config::workspace_config() {
        prev_volatile = Some(c.volatile_mount);
        config = config::ask_for_config(Some(c));
    } else {
        config = config::ask_for_config(None);
    }
    if let Ok(c) = config {
        info!("Shutting down instance(s) before saving config...");
        for_each_instance(&container_down)?;
        fs::create_dir_all(CIEL_DATA_DIR)?;
        fs::write(
            Path::new(CIEL_DATA_DIR).join("config.toml"),
            c.save_config()?,
        )?;
        info!("Workspace configurations saved.");
        let volatile_changed = if let Some(prev_voltile) = prev_volatile {
            prev_voltile != c.volatile_mount
        } else {
            false
        };
        if volatile_changed {
            warn!("You have changed the volatile mount option, please save your work and\x1b[1m\x1b[93m rollback \x1b[4mall the instances\x1b[0m.");
            return Ok(());
        }
        warn!("Please rollback all your instances for the new config to take effect!",);
    } else {
        return Err(anyhow!("Could not recognize the configuration."));
    }

    Ok(())
}

pub fn config_instance(instance: &str, args: &ArgMatches) -> Result<()> {
    let mut config = InstanceConfig::load(instance)?;
    let old_config = config.clone();

    if let Some(tmpfs) = args.get_one::<bool>("tmpfs") {
        if *tmpfs && config.tmpfs.is_none() {
            config.tmpfs = Some(Default::default());
            info!("{}: enabled tmpfs.", instance);
        }
        if !*tmpfs && config.tmpfs.is_some() {
            config.tmpfs = None;
            info!("{}: disabled tmpfs.", instance);
        }
    }

    if let Some(ref mut tmpfs) = &mut config.tmpfs {
        if let Some(tmpfs_size) = args.get_one::<u64>("tmpfs-size") {
            tmpfs.size = Some(*tmpfs_size as usize);
            info!("{}: set tmpfs size to {} MiB.", instance, tmpfs_size);
        } else if args.get_flag("unset-tmpfs-size") {
            tmpfs.size = None;
            info!("{}: set tmpfs size to default value.", instance);
        }
    }

    if args.get_flag("clear-repo") {
        info!("{}: removed all extra APT repositories.", instance);
        config.nspawn_options = vec![];
    }

    if let Some(repo) = args.get_one::<String>("add-repo") {
        if !config.extra_repos.contains(&repo) {
            config.extra_repos.push(repo.to_owned());
            info!("{}: added new extra APT repository '{}'.", instance, repo);
        } else {
            info!(
                "{}: skipped existing extra APT repository '{}'.",
                instance, repo
            );
        }
    }

    if let Some(repo) = args.get_one::<String>("remove-repo") {
        if config.extra_repos.contains(&repo) {
            config.extra_repos = config
                .extra_repos
                .into_iter()
                .filter(|o| o != repo)
                .collect();
            info!("{}: removed new extra APT repository '{}'.", instance, repo);
        } else {
            info!(
                "{}: skipped non-existing extra APT repository '{}'.",
                instance, repo
            );
        }
    }

    if args.get_flag("clear-nspawn-opt") {
        info!("{}: removed all extra nspawn options.", instance);
        config.nspawn_options = vec![];
    }

    if let Some(opt) = args.get_one::<String>("add-nspawn-opt") {
        if !config.nspawn_options.contains(&opt) {
            config.nspawn_options.push(opt.to_owned());
            info!("{}: added new extra nspawn option '{}'.", instance, opt);
        } else {
            info!(
                "{}: skipped existing extra nspawn option '{}'.",
                instance, opt
            );
        }
    }

    if let Some(opt) = args.get_one::<String>("remove-nspawn-opt") {
        if config.nspawn_options.contains(&opt) {
            config.nspawn_options = config
                .nspawn_options
                .into_iter()
                .filter(|o| o != opt)
                .collect();
            info!("{}: removed new extra nspawn option '{}'.", instance, opt);
        } else {
            info!(
                "{}: skipped non-existing extra nspawn option '{}'.",
                instance, opt
            );
        }
    }

    if config != old_config {
        info!("{}: applying configuration ...", instance);
        if !args.get_flag("force-no-rollback") {
            rollback_container(instance)?;
        }
        config.save(instance)?;
    }
    Ok(())
}
