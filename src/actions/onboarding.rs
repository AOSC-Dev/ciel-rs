use anyhow::{anyhow, Result};
use console::{user_attended, Term};
use dialoguer::{theme::ColorfulTheme, Confirm, Input};
use std::{fs, path::Path, process::exit};

use crate::{
    actions::get_branch_name,
    cli::GIT_TREE_URL,
    common::*,
    config, error, info,
    network::{download_git, pick_latest_rootfs},
    overlayfs::create_new_instance_fs,
    repo::{init_repo, refresh_repo},
    warn,
};

use super::{load_os, mount_fs};

/// Show interactive onboarding guide, triggered by issuing `ciel new`
pub fn onboarding(custom_tarball: Option<&String>, arch: Option<&str>) -> Result<()> {
    ctrlc::set_handler(move || {
        let _ = Term::stderr().show_cursor();
        exit(1);
    })
    .expect("Error setting Ctrl-C handler");

    let theme = ColorfulTheme::default();
    info!("Welcome to ciel!");
    if Path::new(".ciel").exists() {
        error!("Seems like you've already created a ciel workspace here.");
        info!("Please run `ciel farewell` to nuke it before running this command.");
        return Err(anyhow!("Unable to create a ciel workspace."));
    }
    info!("Before continuing, I need to ask you a few questions:");
    let real_arch = if let Some(arch) = arch {
        arch
    } else if custom_tarball.is_some() {
        "custom"
    } else {
        ask_for_target_arch()?
    };
    let config = config::ask_for_config(None)?;
    let mut init_instance: Option<String> = None;
    if user_attended()
        && Confirm::with_theme(&theme)
            .with_prompt("Do you want to add a new instance now?")
            .interact()?
    {
        let name: String = Input::with_theme(&theme)
            .with_prompt("Name of the instance")
            .interact_text()?;
        init_instance = Some(name.clone());
        info!(
            "Understood. `{}` will be created after initialization is finished.",
            name
        );
    } else {
        info!("Okay. You can always add a new instance later.");
    }

    info!("Initializing workspace...");
    ciel_init()?;
    info!("Initializing container OS...");
    let (rootfs_url, rootfs_sha256, use_tarball) = match custom_tarball {
        Some(rootfs) => {
            let use_tarball = !rootfs.ends_with(".squashfs");
            info!(
                "Using custom {} from {}",
                if use_tarball { "tarball" } else { "squashfs" },
                rootfs
            );
            (rootfs.clone(), None, use_tarball)
        }
        None => {
            info!("Searching for latest AOSC OS buildkit release...");
            auto_pick_rootfs(&theme, real_arch)?
        }
    };
    load_os(&rootfs_url, rootfs_sha256, use_tarball)?;
    info!("Initializing ABBS tree...");
    if Path::new("TREE").is_dir() {
        warn!("TREE already exists, skipping this step...");
    } else {
        // if TREE is a file, then remove it
        fs::remove_file("TREE").ok();
        download_git(GIT_TREE_URL, Path::new("TREE"))?;
    }
    info!("Saving configurations...");
    fs::write(
        Path::new(CIEL_DATA_DIR).join("config.toml"),
        config.to_toml()?,
    )?;
    info!("Configurations saved.");
    let cwd = std::env::current_dir()?;
    let mut output_dir_name = "OUTPUT".to_string();

    if config.sep_mount {
        output_dir_name.push('-');
        output_dir_name.push_str(&get_branch_name()?);
    }

    if config.local_repo {
        info!("Setting up local repository ...");
        refresh_repo(&cwd.join(&output_dir_name))?;
        info!("Local repository ready.");
    }

    if let Some(init_instance) = init_instance {
        create_new_instance_fs(CIEL_INST_DIR, &init_instance, false)?;
        info!("{}: instance initialized.", init_instance);
        if config.local_repo {
            mount_fs(&init_instance)?;
            init_repo(&cwd.join(output_dir_name), &cwd.join(&init_instance))?;
            info!("{}: local repository initialized.", init_instance);
        }
    }

    Ok(())
}

#[inline]
fn auto_pick_rootfs(
    theme: &dyn dialoguer::theme::Theme,
    arch: &str,
) -> Result<(String, Option<String>, bool)> {
    let root = pick_latest_rootfs(arch);

    if let Ok(rootfs) = root {
        info!(
            "Ciel has picked buildkit for {}, released on {}",
            rootfs.arch, rootfs.date
        );
        Ok((
            format!("https://releases.aosc.io/{}", rootfs.path),
            Some(rootfs.sha256sum),
            false,
        ))
    } else {
        warn!(
            "Ciel was unable to find a suitable buildkit release. Please specify the URL manually."
        );
        let rootfs_url = Input::<String>::with_theme(theme)
            .with_prompt("Rootfs URL")
            .interact_text()?;

        let use_tarball = !rootfs_url.ends_with(".squashfs");

        Ok((rootfs_url, None, use_tarball))
    }
}
