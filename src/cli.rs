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
                let filename = path
                    .file_name()
                    .unwrap_or_else(|| OsStr::new(""))
                    .to_string_lossy();
                if path.is_file() && filename.starts_with("ciel-") {
                    return Some(filename.to_string());
                }
            }
            None
        })
        .collect();

    Ok(plugins)
}

/// Build the CLI instance
pub fn build_cli() -> App<'static, 'static> {
    App::new("CIEL!")
        .version(crate_version!())
        .about("CIEL! is a nspawn container manager")
        .subcommand(SubCommand::with_name("version").about("Display the version of CIEL!"))
        .subcommand(SubCommand::with_name("init").about("Initialize the work directory"))
        .subcommand(
            SubCommand::with_name("load-os")
                .arg(Arg::with_name("url").help("URL or path to the tarball"))
                .about("Unpack OS tarball or fetch the latest BuildKit from the repository"),
        )
        .subcommand(SubCommand::with_name("update-os").about("Update the OS in the container"))
        .subcommand(
            SubCommand::with_name("load-tree")
                .arg(Arg::with_name("url").help("URL to the git repository"))
                .about("Clone package tree from the link provided or AOSC OS ABBS main repository"),
        )
        .subcommand(
            SubCommand::with_name("new").about("Create a new CIEL workspace")
        )
        .subcommand(
            SubCommand::with_name("list")
                .alias("ls")
                .about("List all the instances under the specified working directory"),
        )
        .subcommand(
            SubCommand::with_name("add")
                .arg(Arg::with_name("INSTANCE").required(true))
                .about("Add a new instance"),
        )
        .subcommand(
            SubCommand::with_name("del")
                .alias("rm")
                .arg(Arg::with_name("INSTANCE").required(true))
                .about("Remove an instance"),
        )
        .subcommand(
            SubCommand::with_name("shell")
                .alias("sh")
                .arg(Arg::with_name("INSTANCE").short("i").takes_value(true).help("Instance to be used"))
                .arg(Arg::with_name("COMMANDS").required(false).min_values(1))
                .about("Start an interactive shell"),
        )
        .subcommand(
            SubCommand::with_name("run")
                .alias("exec")
                .arg(Arg::with_name("INSTANCE").short("i").takes_value(true).help("Instance to run command in"))
                .arg(Arg::with_name("COMMANDS").required(true).min_values(1))
                .about("Lower-level version of 'shell', without login environment, without sourcing ~/.bash_profile"),
        )
        .subcommand(
            SubCommand::with_name("config")
                .arg(Arg::with_name("INSTANCE").short("i").takes_value(true).help("Instance to be configured"))
                .arg(Arg::with_name("g").short("g").required(false).conflicts_with("INSTANCE").help("Configure base system instead of an instance"))
                .about("Configure system and toolchain for building interactively"),
        )
        .subcommand(
            SubCommand::with_name("commit")
                .arg(Arg::with_name("INSTANCE").short("i").takes_value(true).help("Instance to be committed"))
                .about("Commit changes onto the shared underlying OS"),
        )
        .subcommand(
            SubCommand::with_name("doctor")
                .about("Diagnose problems (hopefully)"),
        )
        .subcommand(
            SubCommand::with_name("build")
                .arg(Arg::with_name("INSTANCE").short("i").takes_value(true).help("Instance to build in"))
                .arg(Arg::with_name("PACKAGES").required(true).min_values(1))
                .about("Build the packages using the specified instance"),
        )
        .subcommand(
            SubCommand::with_name("rollback")
                .arg(Arg::with_name("INSTANCE").short("i").takes_value(true).help("Instance to be rolled back"))
                .about("Rollback all or specified instance"),
        )
        .subcommand(
            SubCommand::with_name("down")
                .alias("umount")
                .arg(Arg::with_name("INSTANCE").short("i").takes_value(true).help("Instance to be un-mounted"))
                .about("Shutdown and unmount all or one instance"),
        )
        .subcommand(
            SubCommand::with_name("stop")
                .arg(Arg::with_name("INSTANCE").short("i").takes_value(true).help("Instance to be stopped"))
                .about("Shuts down an instance"),
        )
        .subcommand(
            SubCommand::with_name("mount")
                .arg(Arg::with_name("INSTANCE").short("i").takes_value(true).help("Instance to be mounted"))
                .about("Mount all or specified instance"),
        )
        .subcommand(
            SubCommand::with_name("farewell")
                .alias("harakiri")
                .about("Remove everything related to CIEL!"),
        )
        .subcommand(
            SubCommand::with_name("repo")
                .subcommands(vec![SubCommand::with_name("refresh").about("Refresh the repository"), SubCommand::with_name("init").arg(Arg::with_name("INSTANCE").required(true)).about("Initialize the repository"), SubCommand::with_name("deinit").about("Uninitialize the repository")])
                .alias("localrepo")
                .about("Local repository operations")
        )
        .subcommands({
            let plugins = list_helpers();
            if let Ok(plugins) = plugins {
                plugins.iter().map(|plugin| {
                    SubCommand::with_name(plugin.strip_prefix("ciel-").unwrap_or("???"))
                    .arg(Arg::with_name("COMMANDS").required(false).min_values(1).help("Plugin specific commands"))
                    .about("")
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
                    .help("Batch mode, no input required"),
            ]
        )
}
