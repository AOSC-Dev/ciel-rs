mod actions;
mod cli;
mod common;
mod config;
mod dbus_machine1;
mod dbus_machine1_machine;
mod logging;
mod machine;
mod network;
mod overlayfs;

use anyhow::Result;
use console::style;
use std::path::Path;
use std::process;

macro_rules! print_error {
    ($input:block) => {
        if let Err(e) = $input {
            error!("{:?}", e);
            process::exit(1);
        }
    };
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
    // Switch table
    match args.subcommand() {
        ("farewell", _) => {
            actions::farewell(Path::new(directory)).unwrap();
        }
        ("init", _) => {
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
            let url = args.value_of("url").unwrap();
            print_error!({ actions::load_os(url) });
        }
        ("config", Some(args)) => {
            let instance = args.value_of("INSTANCE").unwrap();
            print_error!({ actions::config_os(instance) });
        }
        ("mount", Some(args)) => {
            let instance = args.value_of("INSTANCE").unwrap();
            print_error!({ actions::mount_fs(instance) });
        }
        ("new", _) => {
            if let Err(e) = actions::onboarding() {
                error!("{}", e);
                process::exit(1);
            }
        }
        ("run", Some(args)) => {
            let instance = args.value_of("INSTANCE").unwrap();
            let cmd = args.values_of("COMMANDS").unwrap();
            let args: Vec<&str> = cmd.into_iter().collect();
            let status = actions::run_in_container(instance, &args)?;
            process::exit(status);
        }
        ("shell", Some(args)) => {
            let instance = args.value_of("INSTANCE").unwrap();
            if let Some(cmd) = args.values_of("COMMANDS") {
                let command = cmd.into_iter().collect::<Vec<&str>>().join(" ");
                let status = actions::run_in_container(instance, &["/bin/bash", "-c", &command])?;
                process::exit(status);
            }
            let status = actions::run_in_container(instance, &["/bin/bash"])?;
            process::exit(status);
        }
        ("stop", Some(args)) => {
            let instance = args.value_of("INSTANCE").unwrap();
            print_error!({ actions::stop_container(instance) });
        }
        ("down", Some(args)) => {
            let instance = args.value_of("INSTANCE").unwrap();
            print_error!({ actions::container_down(instance) });
        }
        ("commit", Some(args)) => {
            let instance = args.value_of("INSTANCE").unwrap();
            print_error!({ actions::commit_container(instance) });
        }
        ("rollback", Some(args)) => {
            let instance = args.value_of("INSTANCE").unwrap();
            print_error!({ actions::rollback_container(instance) });
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
            let instance = args.value_of("INSTANCE").unwrap();
            let packages = args.values_of("PACKAGES").unwrap();
            let mut cmd = vec!["/bin/acbs-build", "--"];
            cmd.extend(packages.into_iter());
            let status = actions::run_in_container(instance, &cmd)?;
            process::exit(status);
        }
        ("", _) => {
            machine::print_instances()?;
        }
        ("list", _) => {
            machine::print_instances()?;
        }
        ("doctor", _) => {
            todo!()
        }
        // catch all other conditions
        _ => {
            error!("Unknown command.");
        }
    }

    Ok(())
}
