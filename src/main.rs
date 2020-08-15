mod common;
mod config;
mod dbus_machine1;
mod dbus_machine1_machine;
mod logging;
mod machine;
mod network;
mod overlayfs;

use clap::{App, Arg, SubCommand};
use common::create_spinner;
use dialoguer::Confirm;
use failure::Error;
use nix;
use std::fs;
use std::path::{Path, PathBuf};
use std::process;
use console::style;

const VERSION: &str = "3.0.0-alpha1";

fn farewell<P: AsRef<Path>>(path: P) -> Result<(), Error> {
    let delete = Confirm::new()
        .with_prompt("DELETE ALL CIEL THINGS?")
        .interact()?;
    if delete {
        fs::remove_dir_all(path)?;
    }

    Ok(())
}

#[inline]
fn is_root() -> bool {
    nix::unistd::geteuid().is_root()
}

fn main() -> Result<(), Error> {
    let args = App::new("CIEL!")
        .version(VERSION)
        .about("CIEL! is a nspawn container manager")
        .subcommand(SubCommand::with_name("version").about("display the version of CIEL!"))
        .subcommand(SubCommand::with_name("init").about("initialize the work directory"))
        .subcommand(
            SubCommand::with_name("load-os")
                .arg(Arg::with_name("url").help("URL to the tarball"))
                .about("unpack OS tarball or fetch the latest BuildKit from the repository"),
        )
        .subcommand(
            SubCommand::with_name("load-tree")
                .arg(Arg::with_name("url").help("URL to the git repository"))
                .about("clone package tree from the link provided or AOSC OS ABBS main repository"),
        )
        .subcommand(
            SubCommand::with_name("list")
                .alias("ls")
                .about("list all the instances under the specified working directory"),
        )
        .subcommand(
            SubCommand::with_name("add")
                .arg(Arg::with_name("INSTANCE").required(true))
                .about("add a new instance"),
        )
        .subcommand(
            SubCommand::with_name("del")
                .alias("rm")
                .arg(Arg::with_name("INSTANCE").required(true))
                .about("remove an instance"),
        )
        .subcommand(
            SubCommand::with_name("shell")
                .alias("sh")
                .arg(Arg::with_name("INSTANCE").required(true))
                .arg(Arg::with_name("COMMANDS").required(false))
                .about("start an interactive shell"),
        )
        .subcommand(
            SubCommand::with_name("run")
                .alias("exec")
                .arg(Arg::with_name("INSTANCE").required(true))
                .arg(Arg::with_name("COMMANDS").required(true))
                .about("lower-level version of 'shell', without login environment, without sourcing ~/.bash_profile"),
        )
        .subcommand(
            SubCommand::with_name("config")
                .arg(Arg::with_name("INSTANCE").required(true))
                .arg(Arg::with_name("g").short("g").required(false))
                .about("configure system and toolchain for building interactively"),
        )
        .subcommand(
            SubCommand::with_name("commit")
                .arg(Arg::with_name("INSTANCE").required(true))
                .about("commit changes onto the shared underlying OS"),
        )
        .subcommand(
            SubCommand::with_name("doctor")
                .about("diagnose problems (hopefully)"),
        )
        .subcommand(
            SubCommand::with_name("build")
                .arg(Arg::with_name("INSTANCE").required(true))
                .arg(Arg::with_name("PACKAGES").required(true))
                .about("build the packages using the specified instance"),
        )
        .subcommand(
            SubCommand::with_name("rollback")
                .arg(Arg::with_name("INSTANCE").required(true))
                .about("rollback the specified instance"),
        )
        .subcommand(
            SubCommand::with_name("down")
                .arg(Arg::with_name("INSTANCE"))
                .about("shutdown and unmount all or one instance"),
        )
        .subcommand(
            SubCommand::with_name("stop")
                .arg(Arg::with_name("INSTANCE"))
                .about("shuts down an instance"),
        )
        .subcommand(
            SubCommand::with_name("mount")
                .arg(Arg::with_name("INSTANCE"))
                .about("mount all or specified instance"),
        )
        .subcommand(
            SubCommand::with_name("farewell")
                .alias("harakiri")
                .about("remove everything related to CIEL!"),
        )
        .args(
            &[
                Arg::with_name("C")
                    .short("C")
                    .value_name("DIR")
                    .help("set the CIEL! working directory"),
                Arg::with_name("n")
                    .short("n")
                    .long("no-init")
                    .help("do not boot the container (no init)"),
                Arg::with_name("batch")
                    .short("b")
                    .long("batch")
                    .help("batch mode, no input required"),
            ]
        )
        .get_matches();
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
            farewell(directory).unwrap();
        }
        ("init", _) => {
            if let Err(e) = common::ciel_init() {
                error!("{}", e);
                process::exit(1);
            }
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
            info!("Downloading base OS tarball...");
            let path = Path::new(url).file_name().unwrap().to_str().unwrap();
            let total = network::download_file_progress(url, path).unwrap();
            common::extract_system_tarball(&PathBuf::from(path), total).unwrap();
        }
        ("config", Some(args)) => {
            let config;
            if let Ok(c) = config::read_config() {
                config = config::ask_for_config(Some(c));
            } else {
                config = config::ask_for_config(None);
            }
            if let Ok(c) = config {
                config::apply_config(".", &c)?;
            } else {
                error!("Could not recognize the configuration.");
                process::exit(1);
            }
        }
        ("", _) => {
            machine::print_instances().unwrap();
        }
        // catch all other conditions
        _ => {
            error!("Unknown command.");
        }
    }

    Ok(())
}
