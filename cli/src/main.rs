use std::{
    fs,
    path::PathBuf,
    process::{exit, Command},
};

use anyhow::{anyhow, Context, Result};
use config::{config_instance, config_workspace};
use console::style;
use log::{error, info};
use nix::{
    sys::stat::{umask, Mode},
    unistd::geteuid,
};

mod actions;
mod cli;
mod config;
mod download;
mod logger;
mod utils;

use actions::*;

fn main() -> Result<()> {
    // set umask to 022 to ensure correct permissions on rootfs
    umask(Mode::S_IWGRP | Mode::S_IWOTH);

    // source .env file, ignore errors
    if fs::exists(".env")? {
        dotenvy::dotenv()?;
    }

    let cli = cli::build_cli();
    let version_string = cli.render_version();
    let args = cli.get_matches();

    if !args.get_flag("quiet") {
        logger::init()?;
    }

    let subcommand = args.subcommand();
    if let Some(("version", _)) = subcommand {
        println!("{}", version_string);
    }

    if !geteuid().is_root() {
        println!("Please run me as root!");
        std::process::exit(1);
    }

    let workspace_dir = args.get_one::<String>("ciel-dir").unwrap();
    let workspace_dir = match subcommand {
        Some(("new", _)) => PathBuf::from(workspace_dir),
        _ => {
            let dir = utils::find_ciel_dir(workspace_dir)
                .context("Error finding Ciel workspace directory")?;
            info!(
                "Selected workspace: {}",
                style(dir.canonicalize()?.display()).cyan()
            );
            dir
        }
    };
    std::env::set_current_dir(&workspace_dir)?;

    if let Some(subcommand) = subcommand {
        let result = match subcommand {
            ("list", _) => list_instances(),
            ("new", args) => new_workspace(args),
            ("farewell", args) => farewell(args.get_flag("force")),
            ("load-os", args) => load_os(
                args.get_one::<String>("URL").cloned(),
                args.get_one::<String>("sha256").cloned(),
                args.get_one::<String>("arch").cloned(),
                args.get_flag("force"),
            ),
            ("update-os", args) => update_os(args),
            ("load-tree", args) => load_tree(args.get_one::<String>("URL").unwrap().to_string()),
            ("config", args) => config_workspace(args),
            ("instconf", args) => {
                config_instance(&args.get_one::<String>("INSTANCE").unwrap(), args)
            }
            ("add", args) => add_instance(args),
            ("del", args) => del_instance(args),
            ("mount", args) => mount_instance(args),
            ("boot", args) => boot_instance(args),
            ("stop", args) => stop_instance(args),
            ("down", args) => down_instance(args),
            ("rollback", args) => rollback_instance(args),
            ("commit", args) => commit_instance(args),
            ("diagnose", _) => run_diagnose(),
            ("clean", _) => clean_outputs(),
            ("run", args) => run_in_container(args),
            ("shell", args) => shell_run_in_container(args),
            ("build", args) => build_packages(args),
            ("repo", args) => match args.subcommand().unwrap() {
                ("refresh", _) => refresh_repo(),
                (cmd, _) => Err(anyhow!("unknown command: `{}`.", cmd)),
            },
            (cmd, args) => {
                let exe_dir = std::env::current_exe()?;
                let exe_dir = exe_dir.parent().expect("Where am I?");
                let plugin = exe_dir
                    .join("../libexec/ciel-plugin/")
                    .join(format!("ciel-{}", cmd));
                if !plugin.is_file() {
                    error!("unknown command: `{}`.", cmd);
                    exit(1);
                }
                let mut process = &mut Command::new(plugin);
                if let Some(args) = args.get_many::<String>("COMMANDS") {
                    process = process.args(args);
                }
                let status = process.status()?.code().unwrap();
                exit(status);
            }
        };
        if let Err(err) = result {
            error!("{:?}", err);
            exit(1);
        }
        Ok(())
    } else {
        list_instances()
    }
}
