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

use anyhow::{anyhow, Result};
use clap::{crate_version, ArgMatches};
use console::style;
use dotenv::dotenv;
use std::process;
use std::{path::Path, process::Command};

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

#[inline]
fn get_instance_option(args: &ArgMatches) -> Result<String> {
    let default_instance = std::env::var("CIEL_INST");
    let option_instance = args.value_of("INSTANCE");
    if default_instance.is_err() && option_instance.is_none() {
        return Err(anyhow!("No instance specified!"));
    }

    Ok(option_instance.map_or_else(|| default_instance.expect("Internal error"), String::from))
}

#[inline]
fn is_root() -> bool {
    nix::unistd::geteuid().is_root()
}

fn main() -> Result<()> {
    let args = cli::build_cli().get_matches();
    if !is_root() {
        println!("Please run me as root!");
        process::exit(1);
    }
    let directory = args.value_of("C").unwrap_or(".");
    // Switch to the target directory
    std::env::set_current_dir(directory).unwrap();
    // source .env file, ignore errors
    dotenv().ok();
    // get subcommands from command line parser
    let subcmd = args.subcommand();
    // check if the workspace exists, except when the command is `init` or `new`
    if !["init", "new", "version"].contains(&subcmd.0) && !Path::new("./.ciel").is_dir() {
        error!("This directory does not look like a Ciel workspace");
        process::exit(1);
    }
    // Switch table
    match subcmd {
        ("farewell", _) => {
            actions::farewell(Path::new(directory)).unwrap();
        }
        ("init", _) => {
            warn!("Please do not use this command manually ...");
            warn!("... try `ciel new` instead.");
            print_error!({ common::ciel_init() });
            info!("Initialized working directory at {}", directory);
        }
        ("load-tree", Some(args)) => {
            info!("Cloning abbs tree...");
            network::download_git(
                args.value_of("url").unwrap_or(network::GIT_TREE_URL),
                Path::new("TREE"),
            )?;
        }
        ("load-os", Some(args)) => {
            let url = args.value_of("url");
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
                    common::extract_system_tarball(
                        &tarball.to_path_buf(),
                        tarball.metadata()?.len(),
                    )
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
                actions::load_os(&format!(
                    "https://releases.aosc.io/{}",
                    tarball.path
                ), Some(tarball.sha256sum))
            });
        }
        ("update-os", _) => {
            print_error!({ actions::update_os() });
        }
        ("config", Some(args)) => {
            if args.is_present("g") {
                print_error!({ actions::config_os(None) });
                return Ok(());
            }
            let instance = get_instance_option(args)?;
            print_error!({ actions::config_os(Some(&instance)) });
        }
        ("mount", Some(args)) => {
            print_error!({ one_or_all_instance!(args, &actions::mount_fs) });
        }
        ("new", _) => {
            if let Err(e) = actions::onboarding() {
                error!("{}", e);
                process::exit(1);
            }
        }
        ("run", Some(args)) => {
            let instance = get_instance_option(args)?;
            let cmd = args.values_of("COMMANDS").unwrap();
            let args: Vec<&str> = cmd.into_iter().collect();
            let status = actions::run_in_container(&instance, &args)?;
            process::exit(status);
        }
        ("shell", Some(args)) => {
            let instance = get_instance_option(args)?;
            if let Some(cmd) = args.values_of("COMMANDS") {
                let command = cmd.into_iter().collect::<Vec<&str>>().join(" ");
                let status = actions::run_in_container(&instance, &["/bin/bash", "-ec", &command])?;
                process::exit(status);
            }
            let status = actions::run_in_container(&instance, &["/bin/bash"])?;
            process::exit(status);
        }
        ("stop", Some(args)) => {
            let instance = get_instance_option(args)?;
            print_error!({ actions::stop_container(&instance) });
        }
        ("down", Some(args)) => {
            print_error!({ one_or_all_instance!(args, &actions::container_down) });
        }
        ("commit", Some(args)) => {
            let instance = get_instance_option(args)?;
            print_error!({ actions::commit_container(&instance) });
        }
        ("rollback", Some(args)) => {
            print_error!({ one_or_all_instance!(args, &actions::rollback_container) });
        }
        ("del", Some(args)) => {
            let instance = args.value_of("INSTANCE").unwrap();
            print_error!({ actions::remove_instance(instance) });
        }
        ("add", Some(args)) => {
            let instance = args.value_of("INSTANCE").unwrap();
            print_error!({ actions::add_instance(instance) });
        }
        ("build", Some(args)) => {
            let instance = get_instance_option(args)?;
            let packages = args.values_of("PACKAGES").unwrap();
            let status = actions::package_build(&instance, packages)?;
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
        ("repo", Some(args)) => match args.subcommand() {
            ("refresh", _) => {
                info!("Refreshing repository...");
                print_error!({
                    repo::refresh_repo(&std::env::current_dir().unwrap().join("OUTPUT"))
                });
                info!("Repository has been refreshed.");
            }
            ("init", Some(args)) => {
                info!("Initializing repository...");
                let instance = get_instance_option(args)?;
                let cwd = std::env::current_dir().unwrap();
                print_error!({ actions::mount_fs(&instance) });
                print_error!({ repo::init_repo(&cwd.join("OUTPUT"), &cwd.join(instance)) });
                info!("Repository has been initialized and refreshed.");
            }
            ("deinit", Some(args)) => {
                info!("Disabling local repository...");
                let instance = get_instance_option(args)?;
                let cwd = std::env::current_dir().unwrap();
                print_error!({ actions::mount_fs(&instance) });
                print_error!({ repo::deinit_repo(&cwd.join(instance)) });
                info!("Repository has been disabled.");
            }
            _ => {
                println!("{}", args.usage());
            }
        },
        ("version", _) => {
            println!("{}", crate_version!());
        }
        // catch all other conditions
        (_, options) => {
            let exe_dir = std::env::current_exe()?;
            let exe_dir = exe_dir.parent().expect("Where am I?");
            let cmd = args.subcommand().0;
            let plugin = exe_dir
                .join("../libexec/ciel-plugin/")
                .join(format!("ciel-{}", cmd));
            if !plugin.is_file() {
                error!("Unknown command: `{}`.", cmd);
                process::exit(1);
            }
            info!("Executing plugin ciel-{}", cmd);
            let mut process = &mut Command::new(plugin);
            if let Some(args) = options {
                if let Some(args) = args.values_of("COMMANDS") {
                    process = process.args(args.collect::<Vec<&str>>());
                }
            }
            process::exit(process.status().unwrap().code().unwrap());
        }
    }

    Ok(())
}
