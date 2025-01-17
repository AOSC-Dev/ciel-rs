use std::{fs, path::PathBuf};

use anyhow::{anyhow, bail, Result};
use ciel::{ContainerState, InstanceConfig, Workspace, WorkspaceConfig};
use clap::ArgMatches;
use console::{style, user_attended};
use dialoguer::{theme::ColorfulTheme, Confirm, FuzzySelect, Input};
use log::info;

use crate::{
    config::{ask_for_init_config, patch_instance_config, patch_workspace_config},
    download::{download_file, pick_latest_rootfs, CIEL_MAINLINE_ARCHS, CIEL_RETRO_ARCHS},
    logger::style_bool,
    make_progress_bar,
    utils::{self, get_host_arch_name},
};

use super::load_tree;

pub fn list_instances() -> Result<()> {
    use std::io::Write;
    use tabwriter::TabWriter;

    let ws = Workspace::current_dir()?;

    let mut formatter = TabWriter::new(std::io::stderr());
    writeln!(&mut formatter, "NAME\tMOUNTED\tSTARTED\tBOOTED")?;

    for inst in ws.instances()? {
        let container = inst.open_unlocked()?;
        let state = container.state()?;
        let (mounted, started, running) = match state {
            ContainerState::Down => (false, false, false),
            ContainerState::Mounted => (true, false, false),
            ContainerState::Starting => (true, true, false),
            ContainerState::Running => (true, true, true),
        };
        let booted = {
            if started {
                style_bool(running)
            } else {
                // dim
                "\x1b[2m-\x1b[0m"
            }
        };
        let mounted = style_bool(mounted);
        let started = style_bool(started);
        writeln!(
            &mut formatter,
            "{}\t{}\t{}\t{}",
            inst.name(),
            mounted,
            started,
            booted
        )?;
    }
    formatter.flush()?;

    Ok(())
}

pub fn new_workspace(args: &ArgMatches) -> Result<()> {
    let mut config = WorkspaceConfig::default();
    let mut arch = args.get_one::<String>("arch").cloned();

    patch_workspace_config(args, &mut config)?;
    if user_attended() {
        if arch.is_none() {
            arch = Some(ask_for_target_arch()?.to_owned())
        }
        ask_for_init_config(&mut config)?;
    } else {
        info!("Running in unattended mode, using default configuration ...");
    }
    Workspace::init(std::env::current_dir()?, config)?;

    if !args.get_flag("no-load-os") {
        load_os(
            args.get_one::<String>("rootfs").cloned(),
            args.get_one::<String>("sha256").cloned(),
            arch,
            false,
        )?;
    }

    if !args.get_flag("no-load-tree") {
        load_tree(args.get_one::<String>("tree").unwrap().to_string())?;
    }

    Ok(())
}

pub fn farewell(force: bool) -> Result<()> {
    let ws = Workspace::current_dir()?;
    if !user_attended() {
        info!("Skipped user confirmation due to unattended mode");
    } else if !force {
        let theme = ColorfulTheme::default();
        let delete = Confirm::with_theme(&theme)
            .with_prompt("DELETE THIS CIEL WORKSPACE?")
            .default(false)
            .interact()?;
        if !delete {
            bail!("User cancelled")
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
            bail!("User cancelled")
        }
    }

    info!("... as you wish. Commencing destruction ...");
    ws.destroy()?;
    Ok(())
}

pub fn load_os(
    url: Option<String>,
    sha256: Option<String>,
    arch: Option<String>,
    force: bool,
) -> Result<()> {
    let ws = Workspace::current_dir()?;

    if ws.is_system_loaded() && !force {
        if user_attended() {
            let theme = ColorfulTheme::default();
            let confirm = Confirm::with_theme(&theme)
                .with_prompt("Do you want to override the existing system?")
                .default(false)
                .interact()?;
            if !confirm {
                bail!("User cancelled")
            }
        } else {
            bail!("A system is already loaded")
        }
    }

    let (url, mut sha256) = if let Some(url) = url {
        (url, sha256)
    } else {
        let arch = if let Some(arch) = arch {
            arch
        } else {
            get_host_arch_name()?.to_string()
        };
        let rootfs = pick_latest_rootfs(&arch)?;
        (
            format!("https://releases.aosc.io/{}", rootfs.path),
            Some(rootfs.sha256sum),
        )
    };

    let path = PathBuf::from(&url);
    let filename = path
        .file_name()
        .ok_or_else(|| anyhow!("Unable to convert path to string"))?
        .to_str()
        .ok_or_else(|| anyhow!("Unable to decode path string"))?
        .to_owned();

    let file = 'file: {
        if url.starts_with("http://") || url.starts_with("https://") {
            let dest = PathBuf::from(&filename);
            if dest.exists() {
                if let Some(expected_sha256) = &sha256 {
                    info!("Found local file with the same name, verifying checksum ...");
                    let tarball = fs::File::open(&dest)?;
                    let checksum = utils::sha256sum(tarball)?;
                    if expected_sha256 == &checksum {
                        info!("Checksum verified, reusing local rootfs.");
                        sha256 = None;
                        break 'file dest;
                    } else {
                        info!(
                            "Checksum mismatch: expected {} but got {}",
                            expected_sha256, checksum
                        );
                    }
                }
            }
            info!("Downloading rootfs from {} ...", url);
            download_file(&url, &dest)?;
            dest
        } else {
            info!("Using rootfs from {}", url);
            path
        }
    };

    let total = file.metadata()?.len();

    if let Some(sha256) = sha256 {
        info!("Verifying tarball checksum ...");
        let tarball = fs::File::open(&file)?;
        let checksum = utils::sha256sum(tarball)?;
        if sha256 == checksum {
            info!("Checksum verified.");
        } else {
            bail!(
                "Checksum mismatch: expected {} but got {}",
                sha256,
                checksum
            );
        }
    }

    let progress_bar = indicatif::ProgressBar::new(total);
    progress_bar.set_style(
        indicatif::ProgressStyle::default_bar()
            .template(make_progress_bar!("Extracting rootfs ..."))
            .unwrap(),
    );
    progress_bar.set_draw_target(indicatif::ProgressDrawTarget::stderr_with_hz(5));

    let rootfs_dir = ws.system_rootfs();
    if rootfs_dir.exists() {
        fs::remove_dir_all(&rootfs_dir).ok();
        fs::create_dir_all(&rootfs_dir)?;
    }

    // detect if we are running in systemd-nspawn
    // where /dev/console character device file cannot be created
    // thus ignoring the error in extracting
    let mut in_systemd_nspawn = false;
    if let Ok(output) = std::process::Command::new("systemd-detect-virt").output() {
        if let Ok("systemd-nspawn") = std::str::from_utf8(&output.stdout) {
            in_systemd_nspawn = true;
        }
    }

    let res = if filename.ends_with(".tar.xz") {
        let f = fs::File::open(&file)?;
        utils::extract_tar_xz(progress_bar.wrap_read(f), &rootfs_dir)
    } else if filename.ends_with(".sqfs") || filename.ends_with(".squashfs") {
        utils::extract_squashfs(&file, &rootfs_dir, &progress_bar, total)
    } else {
        bail!("Unsupported rootfs format")
    };

    if !in_systemd_nspawn {
        res?
    }
    progress_bar.finish_and_clear();
    Ok(())
}

pub fn update_os(args: &ArgMatches) -> Result<()> {
    let ws = Workspace::current_dir()?;

    let mut config = InstanceConfig::default();
    config.use_local_repo = false;
    patch_instance_config(args, &mut config)?;

    let inst = ws.ephemeral_container("update", config)?;
    inst.boot()?;
    if args.get_flag("force-use-apt") {
        inst.machine()?.update_system(Some(true))?;
    } else {
        inst.machine()?.update_system(None)?;
    }
    ws.commit(&inst)?;
    inst.discard()?;

    Ok(())
}

fn ask_for_target_arch() -> Result<&'static str> {
    let mut all_archs: Vec<&'static str> = CIEL_MAINLINE_ARCHS.into();
    all_archs.append(&mut CIEL_RETRO_ARCHS.into());
    let host_arch = get_host_arch_name()?;
    let default_arch_index = all_archs.iter().position(|a| *a == host_arch).unwrap();

    let theme = ColorfulTheme::default();
    let prefixed_archs = CIEL_MAINLINE_ARCHS
        .iter()
        .map(|x| format!("mainline: {x}"))
        .chain(CIEL_RETRO_ARCHS.iter().map(|x| format!("retro: {x}")))
        .collect::<Vec<_>>();
    let chosen_index = FuzzySelect::with_theme(&theme)
        .with_prompt("Target Architecture")
        .default(default_arch_index)
        .items(prefixed_archs.as_slice())
        .interact()?;
    Ok(all_archs[chosen_index])
}
