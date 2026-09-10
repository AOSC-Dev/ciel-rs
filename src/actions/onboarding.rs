use anyhow::{anyhow, Result};
use console::{style, user_attended, Term};
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
    // A custom tarball carries no architecture information, so let the user
    // declare the architecture of the tarball being imported; this ends up as
    // the `ARCH` line in the imported container's AB4 configuration file.
    let real_arch = if let Some(arch) = arch {
        Some(arch.to_string())
    } else if custom_tarball.is_some() {
        ask_for_target_arch_optional()?.map(str::to_string)
    } else {
        Some(ask_for_target_arch()?.to_string())
    };
    let mut config = config::ask_for_config(None)?;
    // check if this is a "foreign architecture"
    if let Some(real_arch) = real_arch.as_deref().filter(|arch| arch.contains("_")) {
        config.foreign_arch = Some(real_arch.to_string());
    }
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
            let arch = real_arch
                .as_deref()
                .ok_or_else(|| anyhow!("Unable to determine the target architecture"))?;
            auto_pick_rootfs(&theme, arch)?
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
    config::apply_config(CIEL_DIST_DIR, &config)?;
    info!("Applying configurations...");
    fs::write(
        Path::new(CIEL_DATA_DIR).join("config.toml"),
        config.save_config()?,
    )?;
    info!("Configurations applied.");
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
        create_new_instance_fs(CIEL_INST_DIR, &init_instance)?;
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
    match pick_latest_rootfs(arch) {
        Ok(rootfs) => {
            info!(
                "Ciel has picked buildkit for {}, released on {}",
                rootfs.arch, rootfs.date
            );
            Ok((
                format!("https://releases.aosc.io/{}", rootfs.path),
                Some(rootfs.sha256sum),
                false,
            ))
        }
        Err(e) => {
            if let Some(e) = e.downcast_ref::<ureq::Error>() {
                error!("Failed to fetch manifest: {}", e);
                std::process::exit(1);
            }

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
}
