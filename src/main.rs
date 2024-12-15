mod actions;
mod cli;
mod common;
mod config;
mod dbus_machine1;
mod dbus_machine1_machine;
mod diagnose;
mod logging;
mod machine;
mod network;
mod overlayfs;
mod repo;

use actions::{inspect_container, patch_instance_config, rollback_container};
use anyhow::{anyhow, bail, Context, Result};
use clap::ArgMatches;
use config::{InstanceConfig, WorkspaceConfig};
use console::{style, user_attended};
use dotenvy::dotenv;
use libc::exit;
use std::process;
use std::{path::Path, process::Command};

use crate::actions::BuildSettings;
use crate::common::*;

macro_rules! print_error {
    ($input:block) => {
        if let Err(e) = $input {
            error!("{:?}", e);
            process::exit(1);
        }
    };
}

macro_rules! one_or_all_instance {
    ($args:ident, $func:expr) => {{
        if let Ok(instance) = get_instance_option($args) {
            $func(&instance)
        } else {
            actions::for_each_instance($func)
        }
    }};
}

fn unsupported_target_architecture(arch: &str) -> ! {
    error!("Unknown target architecture {}", arch);
    info!("Supported target architectures:");
    eprintln!(
        "{}\n{}",
        CIEL_MAINLINE_ARCHS.join("\n\t"),
        CIEL_RETRO_ARCHS.join("\n\t")
    );
    info!("If you do want to load an OS unsupported by Ciel, specify a tarball to initialize this workspace.");
    process::exit(1);
}

fn get_output_dir() -> String {
    if let Ok(c) = WorkspaceConfig::load() {
        return actions::get_output_directory(c.sep_mount);
    }
    "OUTPUT".to_string()
}

#[inline]
fn get_instance_option(args: &ArgMatches) -> Result<String> {
    let option_instance = args.get_one::<String>("INSTANCE");
    if option_instance.is_none() {
        return Err(anyhow!("No instance specified!"));
    }

    Ok(option_instance.expect("Internal error").to_string())
}

#[inline]
fn is_root() -> bool {
    nix::unistd::geteuid().is_root()
}

fn update_tree(path: &Path, branch: Option<&String>, rebase_from: Option<&String>) -> Result<()> {
    let mut repo = network::fetch_repo(path)?;
    if let Some(branch) = branch {
        if repo.state() != git2::RepositoryState::Clean {
            bail!(
                "Cannot switch branches, because your tree seems to have an operation in progress."
            );
        }
        let result = network::git_switch_branch(&mut repo, branch, rebase_from.map(|x| x.as_str()));
        if let Err(e) = result {
            bail!("Failed to switch branches: {}\nNote that you can still use `git stash pop` to retrieve your previous changes.`", e);
        }
        info!("Successfully updated the tree and switched to {}.", branch);
    } else {
        if rebase_from.is_some() {
            bail!("You need to specify a branch to switch to when requesting a rebase.");
        }
        info!("Successfully fetched new changes from remote.");
    }

    Ok(())
}

fn main() -> Result<()> {
    // set umask to 022 to ensure correct permissions on rootfs
    unsafe {
        libc::umask(libc::S_IWGRP | libc::S_IWOTH);
    }

    // source .env file, ignore errors
    dotenv().ok();

    let build_cli = cli::build_cli();
    let version_string = build_cli.render_version();
    let args = build_cli.get_matches();
    if !is_root() {
        println!("Please run me as root!");
        process::exit(1);
    }
    let mut directory = Path::new(args.get_one::<String>("C").unwrap()).to_path_buf();
    let host_arch = get_host_arch_name();
    // Switch to the target directory
    std::env::set_current_dir(&directory).unwrap();
    // get subcommands from command line parser
    let subcmd = args.subcommand();
    // check if the workspace exists, except when the command is `init` or `new`
    match subcmd {
        Some(("init", _)) | Some(("new", _)) | Some(("version", _)) => (),
        _ if !Path::new("./.ciel").is_dir() => {
            if directory == Path::new(".") {
                directory =
                    common::find_ciel_dir(".").context("Error finding ciel workspace directory")?;
                info!(
                    "Selected Ciel directory: {}",
                    style(directory.canonicalize()?.display()).cyan()
                );
                std::env::set_current_dir(&directory).unwrap();
            } else {
                error!("This directory does not look like a Ciel workspace");
                process::exit(1);
            }
        }
        _ => (),
    }
    // list instances if no command is specified
    if subcmd.is_none() {
        machine::print_instances()?;
        return Ok(());
    }
    let subcmd = subcmd.unwrap();
    // Switch table
    match subcmd {
        ("farewell", _) => {
            actions::farewell(&directory, args.get_flag("force")).unwrap();
        }
        ("init", args) => {
            if args.get_flag("upgrade") {
                info!("Upgrading workspace...");
                info!("First, shutting down all the instances...");
                print_error!({ actions::for_each_instance(&actions::container_down) });
            } else {
                warn!("Please do not use this command manually ...");
                warn!("... try `ciel new` instead.");
            }
            print_error!({ common::ciel_init() });
            info!("Initialized working directory at {}", directory.display());
        }
        ("load-tree", args) => {
            info!("Cloning abbs tree...");
            network::download_git(args.get_one::<String>("url").unwrap(), Path::new("TREE"))?;
        }
        ("update-tree", args) => {
            let tree = Path::new("TREE");
            info!("Updating tree...");
            print_error!({ update_tree(tree, args.get_one("branch"), args.get_one("rebase")) });
        }
        ("load-os", args) => {
            let url = args.get_one::<String>("url");
            if let Some(url) = url {
                let use_tarball = !url.ends_with(".squashfs");
                // load from network using specified url
                if url.starts_with("https://") || url.starts_with("http://") {
                    print_error!({ actions::load_os(url, None, use_tarball) });
                    return Ok(());
                }
                // load from file
                let tarball = Path::new(url);
                if !tarball.is_file() {
                    error!("{:?} is not a file", url);
                    process::exit(1);
                }
                print_error!({
                    common::extract_system_rootfs(tarball, tarball.metadata()?.len(), use_tarball)
                });

                return Ok(());
            }
            // load from network using auto picked url
            let specified_arch = args.get_one::<String>("arch");
            let arch = if let Some(specified_arch) = specified_arch {
                if !check_arch_name(specified_arch.as_str()) {
                    unsupported_target_architecture(specified_arch.as_str());
                }
                specified_arch
            } else if !user_attended() {
                host_arch
                    .ok_or_else(|| anyhow!("Ciel does not support this CPU architecture."))
                    .unwrap()
            } else {
                ask_for_target_arch().unwrap()
            };
            info!("Picking OS tarball for architecture {}", arch);
            let rootfs = network::pick_latest_rootfs(arch);

            if let Err(e) = rootfs {
                error!("Unable to determine the latest tarball: {}", e);
                process::exit(1);
            }

            let rootfs = rootfs.unwrap();
            print_error!({
                actions::load_os(
                    &format!("https://releases.aosc.io/{}", rootfs.path),
                    Some(rootfs.sha256sum),
                    false,
                )
            });
        }
        ("update-os", args) => {
            let force_use_apt = if get_host_arch_name().is_some_and(|x| x == "riscv64") {
                true
            } else {
                args.get_flag("force-use-apt")
                    || WorkspaceConfig::load().is_ok_and(|x| x.force_use_apt)
            };

            print_error!({ actions::update_os(force_use_apt, Some(args)) });
        }
        ("config", args) => {
            if args.get_flag("global") {
                print_error!({ actions::config_workspace(args) });
            } else {
                let instance = get_instance_option(args)?;
                print_error!({ actions::config_instance(&instance, args) });
            }
        }
        ("mount", args) => {
            print_error!({ one_or_all_instance!(args, &actions::mount_fs) });
        }
        ("new", args) => {
            let arch = args.get_one::<String>("arch").map(|val| {
                if !check_arch_name(val) {
                    unsupported_target_architecture(val.as_str());
                }
                val.as_str()
            });
            let tarball = args.get_one::<String>("tarball");
            if let Err(e) = actions::onboarding(tarball, arch) {
                error!("{}", e);
                process::exit(1);
            }
        }
        ("run", args) => {
            let instance = get_instance_option(args)?;
            let args = args.get_many::<String>("COMMANDS").unwrap();
            let status =
                actions::run_in_container(&instance, &args.into_iter().collect::<Vec<_>>())?;
            process::exit(status);
        }
        ("shell", args) => {
            let instance = get_instance_option(args)?;
            let config_ref = InstanceConfig::get(&instance)?;
            let mut config = config_ref.read().unwrap().clone();
            patch_instance_config(&instance, args, &mut config)?;

            let container = inspect_container(&instance)?;
            let ephermal_config =
                *config_ref.read().unwrap() != InstanceConfig::load_mounted(&instance)?;
            let need_rollback = container.mounted && ephermal_config;
            if need_rollback {
                rollback_container(&instance)?;
            }
            if ephermal_config {
                *config_ref.write().unwrap() = config;
            }

            if let Some(cmd) = args.get_many::<String>("COMMANDS") {
                let command = cmd
                    .into_iter()
                    .fold(String::with_capacity(1024), |acc, x| acc + " " + x);
                let status = actions::run_in_container(&instance, &["/bin/bash", "-ec", &command])?;
                process::exit(status);
            }
            let status = actions::run_in_container(&instance, &["/bin/bash"])?;
            process::exit(status);
        }
        ("stop", args) => {
            let instance = get_instance_option(args)?;
            print_error!({ actions::stop_container(&instance) });
        }
        ("down", args) => {
            print_error!({ one_or_all_instance!(args, &actions::container_down) });
        }
        ("commit", args) => {
            let instance = get_instance_option(args)?;
            print_error!({ actions::commit_container(&instance) });
        }
        ("rollback", args) => {
            print_error!({ one_or_all_instance!(args, &actions::rollback_container) });
        }
        ("del", args) => {
            let instance = args.get_one::<String>("INSTANCE").unwrap();
            print_error!({ actions::remove_instance(instance) });
        }
        ("add", args) => {
            let instance = args.get_one::<String>("INSTANCE").unwrap();
            let tmpfs = args.get_flag("tmpfs");
            print_error!({ actions::add_instance(instance, tmpfs) });
        }
        ("build", args) => {
            let instance = get_instance_option(args)?;
            let config_ref = InstanceConfig::get(&instance)?;
            let mut config = config_ref.read().unwrap().clone();
            patch_instance_config(&instance, args, &mut config)?;

            let container = inspect_container(&instance)?;
            let ephermal_config =
                *config_ref.read().unwrap() != InstanceConfig::load_mounted(&instance)?;
            let need_rollback = container.mounted && ephermal_config;
            if need_rollback {
                rollback_container(&instance)?;
            }
            if ephermal_config {
                *config_ref.write().unwrap() = config;
            }

            let settings = BuildSettings {
                offline: args.get_flag("OFFLINE"),
                stage2: args.get_flag("STAGE2"),
            };
            let mut state = None;
            if let Some(cont) = args.get_one::<String>("CONTINUE") {
                if container.started {
                    error!("The current instance has not been started. Cannot continue.");
                    process::exit(1);
                }
                state = Some(actions::load_build_checkpoint(cont)?);
                let empty: Vec<&str> = Vec::new();
                let status = actions::package_build(&instance, empty.into_iter(), state, settings)?;
                println!("\x07"); // bell character
                process::exit(status);
            }
            let packages = args.get_many::<String>("PACKAGES");
            if packages.is_none() {
                error!("Please specify a list of packages to build!");
                process::exit(1);
            }
            let packages = packages.unwrap();

            if need_rollback {
                warn!("The current instance configuration differs from the mounted one. Rolling back.");
                actions::rollback_container(&instance)?;
            }

            if args.contains_id("SELECT") {
                let start_package = args.get_one::<String>("SELECT");
                let status =
                    actions::packages_stage_select(&instance, packages, settings, start_package)?;
                process::exit(status);
            }
            if args.get_flag("FETCH") {
                let packages = packages.into_iter().collect::<Vec<_>>();
                let status = actions::package_fetch(&instance, &packages)?;
                process::exit(status);
            }
            let status = actions::package_build(&instance, packages, state, settings)?;
            println!("\x07"); // bell character
            process::exit(status);
        }
        ("", _) => {
            machine::print_instances()?;
        }
        ("list", _) => {
            machine::print_instances()?;
        }
        ("doctor", _) => {
            print_error!({ diagnose::run_diagnose() });
        }
        ("repo", args) => match args.subcommand() {
            Some(("refresh", _)) => {
                info!("Refreshing repository...");
                print_error!({
                    repo::refresh_repo(&std::env::current_dir().unwrap().join(get_output_dir()))
                });
                info!("Repository has been refreshed.");
            }
            Some(("init", args)) => {
                info!("Initializing repository...");
                let instance = get_instance_option(args)?;
                let cwd = std::env::current_dir().unwrap();
                print_error!({ actions::mount_fs(&instance) });
                print_error!({ repo::init_repo(&cwd.join(get_output_dir()), &cwd.join(instance)) });
                info!("Repository has been initialized and refreshed.");
            }
            Some(("deinit", args)) => {
                info!("Disabling local repository...");
                let instance = get_instance_option(args)?;
                let cwd = std::env::current_dir().unwrap();
                print_error!({ actions::mount_fs(&instance) });
                print_error!({ repo::deinit_repo(&cwd.join(instance)) });
                info!("Repository has been disabled.");
            }
            _ => unreachable!(),
        },
        ("clean", _) => {
            print_error!({ actions::cleanup_outputs() });
        }
        ("version", _) => {
            println!("{}", version_string);
        }
        // catch all other conditions
        (_, options) => {
            let exe_dir = std::env::current_exe()?;
            let exe_dir = exe_dir.parent().expect("Where am I?");
            let cmd = args.subcommand().unwrap().0;
            let plugin = exe_dir
                .join("../libexec/ciel-plugin/")
                .join(format!("ciel-{}", cmd));
            if !plugin.is_file() {
                error!("Unknown command: `{}`.", cmd);
                process::exit(1);
            }
            info!("Executing applet ciel-{}", cmd);
            let mut process = &mut Command::new(plugin);
            if let Some(args) = options.get_many::<String>("COMMANDS") {
                process = process.args(args);
            }
            let status = process.status().unwrap().code().unwrap();
            if status != 0 {
                error!("Applet exited with error {}", status);
            }
            process::exit(status);
        }
    }

    Ok(())
}
