use std::fmt::Display;

use anyhow::Result;
use ciel::{InstanceConfig, Workspace, WorkspaceConfig};
use clap::ArgMatches;
use dialoguer::{theme::ColorfulTheme, Confirm, Input};
use log::info;

use crate::utils::get_host_arch_name;

#[inline]
fn config_list<V>(args: &ArgMatches, id: &str, list: &mut Vec<V>)
where
    V: ToOwned<Owned = V> + Display + PartialEq + Clone + Send + Sync + 'static,
{
    if args.get_flag(&format!("unset-{}", id)) {
        list.clear();
    }

    if let Some(val) = args.get_one::<V>(&format!("add-{}", id)) {
        if !list.contains(val) {
            list.push(val.to_owned());
        }
    }

    if let Some(val) = args.get_one::<V>(&format!("remove-{}", id)) {
        if list.contains(val) {
            let mut new_list = list.drain(0..).filter(|o| o != val).collect();
            list.append(&mut new_list);
        }
    }
}

#[inline]
fn config_bool(args: &ArgMatches, id: &str, val: &mut bool) {
    if let Some(new_val) = args.get_one::<bool>(id) {
        *val = *new_val;
    }
}

pub fn config_workspace(args: &ArgMatches) -> Result<()> {
    let ws = Workspace::current_dir()?;
    let mut config = ws.config();
    let old_config = config.clone();

    patch_workspace_config(args, &mut config)?;

    if config != old_config {
        info!("Applying new workspace configuration ...");
        if !args.get_flag("force-no-rollback") {
            for inst in ws.instances()? {
                inst.open()?.rollback()?;
            }
        }
    } else {
        info!("Nothing has been changed");
    }
    ws.set_config(config)?;
    Ok(())
}

pub fn config_instance(instance: &str, args: &ArgMatches) -> Result<()> {
    let ws = Workspace::current_dir()?;
    let inst = ws.instance(instance)?;
    let mut config = inst.config();
    let old_config = config.clone();

    patch_instance_config(args, &mut config)?;

    if config != old_config {
        info!("{}: applying new configurations ...", instance);
        if !args.get_flag("force-no-rollback") {
            inst.open()?.rollback()?;
        }
    } else {
        info!("Nothing has been changed");
    }
    inst.set_config(config)?;
    Ok(())
}

/// Applies workspace configuration patches from [ArgMatches].
pub fn patch_workspace_config(args: &ArgMatches, config: &mut WorkspaceConfig) -> Result<()> {
    if let Some(maintainer) = args.get_one::<String>("maintainer") {
        if maintainer != &config.maintainer {
            WorkspaceConfig::validate_maintainer(maintainer)?;
            config.maintainer = maintainer.to_owned();
        }
    }

    config_bool(args, "dnssec", &mut config.dnssec);
    config_list(args, "repo", &mut config.extra_apt_repos);
    config_bool(args, "local-repo", &mut config.use_local_repo);
    config_bool(args, "source-cache", &mut config.cache_sources);
    config_list(args, "nspawn-opt", &mut config.extra_nspawn_options);
    config_bool(
        args,
        "branch-exclusive-output",
        &mut config.branch_exclusive_output,
    );
    config_bool(args, "volatile-mount", &mut config.volatile_mount);
    config_bool(args, "use-apt", &mut config.use_apt);

    Ok(())
}

/// Applies instance configuration patches from [ArgMatches].
pub fn patch_instance_config(args: &ArgMatches, config: &mut InstanceConfig) -> Result<()> {
    if let Some(tmpfs) = args.get_one::<bool>("tmpfs") {
        if *tmpfs && config.tmpfs.is_none() {
            config.tmpfs = Some(Default::default());
        }
        if !*tmpfs && config.tmpfs.is_some() {
            config.tmpfs = None;
        }
    }

    if let Some(ref mut tmpfs) = &mut config.tmpfs {
        if let Some(tmpfs_size) = args.get_one::<u64>("tmpfs-size") {
            tmpfs.size = Some(*tmpfs_size as usize);
        } else if args.get_flag("unset-tmpfs-size") {
            tmpfs.size = None;
        }
    }

    config_list(args, "repo", &mut config.extra_apt_repos);
    config_list(args, "nspawn-opt", &mut config.extra_nspawn_options);
    config_bool(args, "local-repo", &mut config.use_local_repo);
    config_bool(args, "ro-tree", &mut config.readonly_tree);

    Ok(())
}

/// Shows a series of prompts to let the user select the configurations
pub fn ask_for_init_config(config: &mut WorkspaceConfig) -> Result<()> {
    let theme = ColorfulTheme::default();
    config.maintainer = Input::<String>::with_theme(&theme)
        .with_prompt("Maintainer")
        .default(config.maintainer.to_owned())
        .validate_with(|s: &String| WorkspaceConfig::validate_maintainer(s.as_str()))
        .interact_text()?;
    config.cache_sources = Confirm::with_theme(&theme)
        .with_prompt("Enable local sources caching")
        .default(config.cache_sources)
        .interact()?;
    config.use_local_repo = Confirm::with_theme(&theme)
        .with_prompt("Enable local packages repository")
        .default(config.use_local_repo)
        .interact()?;
    config.branch_exclusive_output = Confirm::with_theme(&theme)
        .with_prompt("Use different OUTPUT directories for different branches")
        .default(config.branch_exclusive_output)
        .interact()?;

    // FIXME: RISC-V build hosts is unreliable when using oma: random lock-ups
    // during `oma refresh'. Disabling oma to workaround potential lock-ups.
    if get_host_arch_name().map(|x| x != "riscv64").unwrap_or(true) {
        info!("Ciel now uses oma as the default package manager for base system updating tasks.");
        info!("You can choose whether to use oma instead of apt while configuring.");
        config.use_apt = Confirm::with_theme(&theme)
            .with_prompt("Use apt as package manager")
            .default(config.use_apt)
            .interact()?;
    }

    Ok(())
}
