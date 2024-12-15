use std::fmt::Display;

use anyhow::{anyhow, Result};
use clap::ArgMatches;

use crate::{
    actions::rollback_container,
    config::{InstanceConfig, WorkspaceConfig},
    info,
};

use super::for_each_instance;

fn config_list<V>(instance: &str, args: &ArgMatches, id: &str, name: &str, list: &mut Vec<V>)
where
    V: ToOwned<Owned = V> + Display + PartialEq + Clone + Send + Sync + 'static,
{
    if args.get_flag(&format!("clear-{}", id)) {
        info!("{}: removed all {}.", instance, name);
        list.clear();
    }

    if let Some(val) = args.get_one::<V>(&format!("add-{}", id)) {
        if !list.contains(val) {
            list.push(val.to_owned());
            info!("{}: added new {} '{}'.", instance, name, val);
        } else {
            info!("{}: skipped existing {} '{}'.", instance, name, val);
        }
    }

    if let Some(val) = args.get_one::<V>("remove-repo") {
        if list.contains(val) {
            let mut new_list = list.drain(0..).filter(|o| o != val).collect();
            list.append(&mut new_list);
            info!("{}: removed new {} '{}'.", instance, name, val);
        } else {
            info!("{}: skipped non-existing {} '{}'.", instance, name, val);
        }
    }
}

fn config_bool(instance: &str, args: &ArgMatches, id: &str, name: &str, val: &mut bool) {
    if let Some(new_val) = args.get_one::<bool>(id) {
        if *new_val && !*val {
            *val = true;
            info!("{}: enabled {}.", instance, name);
        }
        if !*new_val && *val {
            *val = false;
            info!("{}: disabled {}.", instance, name);
        }
    }
}

pub fn config_workspace(args: &ArgMatches) -> Result<()> {
    let mut config = WorkspaceConfig::load()?;
    let old_config = config.clone();

    if let Some(maintainer) = args.get_one::<String>("maintainer") {
        if maintainer != &config.maintainer {
            crate::config::validate_maintainer(maintainer)
                .map_err(|err| anyhow!("Invalid maintainer information: {}", err))?;
            config.maintainer = maintainer.to_owned();
            info!("workspace: updated maintainer to '{}'.", maintainer);
        }
    }

    config_bool("workspace", args, "dnssec", "DNSSEC", &mut config.dnssec);

    if let Some(repo) = args.get_one::<String>("repo") {
        if repo != &config.apt_sources {
            config.apt_sources = repo.to_owned();
            info!("workspace: updated APT sources to '{}'.", repo);
        }
    }

    config_bool(
        "workspace",
        args,
        "local-repo",
        "local package repository",
        &mut config.local_repo,
    );

    config_bool(
        "workspace",
        args,
        "source-cache",
        "local source caches",
        &mut config.local_sources,
    );

    config_list(
        "workspace",
        args,
        "nspawn-opt",
        "extra nspawn options",
        &mut config.nspawn_options,
    );

    config_bool(
        "workspace",
        args,
        "branch-output",
        "branch exclusive output",
        &mut config.sep_mount,
    );

    config_bool(
        "workspace",
        args,
        "volatile-mount",
        "volatile mounts",
        &mut config.volatile_mount,
    );

    config_bool(
        "workspace",
        args,
        "use-apt",
        "force use APT",
        &mut config.force_use_apt,
    );

    if config != old_config {
        info!("Applying workspace configuration ...");
        if !args.get_flag("force-no-rollback") {
            for_each_instance(&rollback_container)?;
        }
        config.save()?;
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

    config_list(
        instance,
        args,
        "repo",
        "extra APT repository",
        &mut config.extra_repos,
    );
    config_list(
        instance,
        args,
        "nspawn-opt",
        "extra nspawn options",
        &mut config.nspawn_options,
    );

    if config != old_config {
        info!("{}: applying configuration ...", instance);
        if !args.get_flag("force-no-rollback") {
            rollback_container(instance)?;
        }
        config.save(instance)?;
    }
    Ok(())
}
