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

use anyhow::{anyhow, bail, Context, Result};
use clap::ArgMatches;
use console::style;
use dotenv::dotenv;
use std::process;
use std::{path::Path, process::Command};

use crate::actions::BuildSettings;

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

fn get_output_dir() -> String {
    if let Ok(c) = config::read_config() {
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
            actions::farewell(&directory).unwrap();
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
                // load from network using specified url
                if url.starts_with("https://") || url.starts_with("http://") {
                    print_error!({ actions::load_os(url, None) });
                    return Ok(());
                }
                // load from file
                let tarball = Path::new(url);
                if !tarball.is_file() {
                    error!("{:?} is not a file", url);
                    process::exit(1);
                }
                print_error!({
                    common::extract_system_tarball(tarball, tarball.metadata()?.len())
                });

                return Ok(());
            }
            // load from network using auto picked url
            info!("No URL specified. Ciel will automatically pick one.");
            let tarball = network::pick_latest_tarball();
            if let Err(e) = tarball {
                error!("Unable to determine the latest tarball: {}", e);
                process::exit(1);
            }
            let tarball = tarball.unwrap();
            print_error!({
                actions::load_os(
                    &format!("https://releases.aosc.io/{}", tarball.path),
                    Some(tarball.sha256sum),
                )
            });
        }
        ("update-os", _) => {
            print_error!({ actions::update_os() });
        }
        ("config", args) => {
            if args.get_flag("g") {
                print_error!({ actions::config_os(None) });
                return Ok(());
            }
            let instance = get_instance_option(args)?;
            print_error!({ actions::config_os(Some(&instance)) });
        }
        ("mount", args) => {
            print_error!({ one_or_all_instance!(args, &actions::mount_fs) });
        }
        ("new", args) => {
            let tarball = args.get_one::<String>("tarball");
            if let Err(e) = actions::onboarding(tarball) {
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
            print_error!({ actions::add_instance(instance) });
        }
        ("build", args) => {
            let instance = get_instance_option(args)?;
            let settings = BuildSettings {
                offline: args.get_flag("OFFLINE"),
                stage2: args.get_flag("STAGE2"),
            };
            let mut state = None;
            if let Some(cont) = args.get_one::<String>("CONTINUE") {
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
