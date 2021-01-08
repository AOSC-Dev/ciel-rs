use anyhow::{anyhow, Result};
use clap::{crate_version, App, Arg, SubCommand};
use std::ffi::OsStr;

fn list_helpers() -> Result<Vec<String>> {
    let exe_dir = std::env::current_exe()?;
    let exe_dir = exe_dir.parent().ok_or_else(|| anyhow!("Where am I?"))?;
    let plugins_dir = exe_dir.join("../libexec/ciel-plugin/").read_dir()?;
    let plugins = plugins_dir
        .filter_map(|x| {
            if let Ok(x) = x {
                let path = x.path();
                let filename = path.file_name().unwrap_or(OsStr::new("")).to_string_lossy();
                if path.is_file() && filename.starts_with("ciel-") {
                    return Some(filename.to_string());
                }
            }
            None
        })
        .collect();

    Ok(plugins)
}

pub fn build_cli() -> App<'static, 'static> {
    App::new("CIEL!")
        .version(crate_version!())
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
            SubCommand::with_name("new").about("Create a new CIEL workspace")
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
                .arg(Arg::with_name("COMMANDS").required(false).min_values(1))
                .about("start an interactive shell"),
        )
        .subcommand(
            SubCommand::with_name("run")
                .alias("exec")
                .arg(Arg::with_name("INSTANCE").required(true))
                .arg(Arg::with_name("COMMANDS").required(true).min_values(1))
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
                .arg(Arg::with_name("PACKAGES").required(true).min_values(1))
                .about("build the packages using the specified instance"),
        )
        .subcommand(
            SubCommand::with_name("rollback")
                .arg(Arg::with_name("INSTANCE"))
                .about("rollback all or specified instance"),
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
        .subcommands({
            let plugins = list_helpers();
            if let Ok(plugins) = plugins {
                plugins.iter().map(|plugin| {
                    SubCommand::with_name(plugin.strip_prefix("ciel-").unwrap_or("???")).about("Ciel plugin")
                }).collect()
            } else {
                vec![]
            }
        })
        .args(
            &[
                Arg::with_name("C")
                    .short("C")
                    .value_name("DIR")
                    .help("set the CIEL! working directory"),
                Arg::with_name("batch")
                    .short("b")
                    .long("batch")
                    .help("batch mode, no input required"),
            ]
        )
}
