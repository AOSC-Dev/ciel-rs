use anyhow::{anyhow, Result};
use clap::{Command, Arg};
use std::ffi::OsStr;

/// List all the available plugins/helper scripts
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
pub fn build_cli() -> Command<'static> {
    Command::new("CIEL!")
        .version(env!("CARGO_PKG_VERSION"))
        .about("CIEL! is a nspawn container manager")
        .allow_external_subcommands(true)
        .subcommand(Command::new("version").about("Display the version of CIEL!"))
        .subcommand(Command::new("init")
            .arg(Arg::new("upgrade").long("upgrade").help("Upgrade Ciel workspace from an older version"))
            .about("Initialize the work directory"))
        .subcommand(
            Command::new("load-os")
                .arg(Arg::new("url").help("URL or path to the tarball"))
                .about("Unpack OS tarball or fetch the latest BuildKit from the repository"),
        )
        .subcommand(Command::new("update-os").about("Update the OS in the container"))
        .subcommand(
            Command::new("load-tree")
                .arg(Arg::new("url").help("URL to the git repository"))
                .about("Clone package tree from the link provided or AOSC OS ABBS main repository"),
        )
        .subcommand(
            Command::new("new").about("Create a new CIEL workspace")
        )
        .subcommand(
            Command::new("list")
                .alias("ls")
                .about("List all the instances under the specified working directory"),
        )
        .subcommand(
            Command::new("add")
                .arg(Arg::new("INSTANCE").required(true))
                .about("Add a new instance"),
        )
        .subcommand(
            Command::new("del")
                .alias("rm")
                .arg(Arg::new("INSTANCE").required(true))
                .about("Remove an instance"),
        )
        .subcommand(
            Command::new("shell")
                .alias("sh")
                .arg(Arg::new("INSTANCE").short('i').takes_value(true).help("Instance to be used"))
                .arg(Arg::new("COMMANDS").required(false).min_values(1))
                .about("Start an interactive shell"),
        )
        .subcommand(
            Command::new("run")
                .alias("exec")
                .arg(Arg::new("INSTANCE").short('i').takes_value(true).help("Instance to run command in"))
                .arg(Arg::new("COMMANDS").required(true).min_values(1))
                .about("Lower-level version of 'shell', without login environment, without sourcing ~/.bash_profile"),
        )
        .subcommand(
            Command::new("config")
                .arg(Arg::new("INSTANCE").short('i').takes_value(true).help("Instance to be configured"))
                .arg(Arg::new("g").short('g').required(false).conflicts_with("INSTANCE").help("Configure base system instead of an instance"))
                .about("Configure system and toolchain for building interactively"),
        )
        .subcommand(
            Command::new("commit")
                .arg(Arg::new("INSTANCE").short('i').takes_value(true).help("Instance to be committed"))
                .about("Commit changes onto the shared underlying OS"),
        )
        .subcommand(
            Command::new("doctor")
                .about("Diagnose problems (hopefully)"),
        )
        .subcommand(
            Command::new("build")
                .arg(Arg::new("FETCH").short('g').takes_value(false).help("Fetch source packages only"))
                .arg(Arg::new("OFFLINE").short('x').long("offline").takes_value(false).help("Disable network in the container during the build"))
                .arg(Arg::new("INSTANCE").short('i').takes_value(true).help("Instance to build in"))
                .arg(Arg::new("CONTINUE").conflicts_with("SELECT").short('c').long("resume").alias("continue").takes_value(true).help("Continue from a Ciel checkpoint"))
                .arg(Arg::new("SELECT").max_values(1).min_values(0).long("stage-select").help("Select the starting point for a build"))
                .arg(Arg::new("PACKAGES").conflicts_with("CONTINUE").min_values(1))
                .about("Build the packages using the specified instance"),
        )
        .subcommand(
            Command::new("rollback")
                .arg(Arg::new("INSTANCE").short('i').takes_value(true).help("Instance to be rolled back"))
                .about("Rollback all or specified instance"),
        )
        .subcommand(
            Command::new("down")
                .alias("umount")
                .arg(Arg::new("INSTANCE").short('i').takes_value(true).help("Instance to be un-mounted"))
                .about("Shutdown and unmount all or one instance"),
        )
        .subcommand(
            Command::new("stop")
                .arg(Arg::new("INSTANCE").short('i').takes_value(true).help("Instance to be stopped"))
                .about("Shuts down an instance"),
        )
        .subcommand(
            Command::new("mount")
                .arg(Arg::new("INSTANCE").short('i').takes_value(true).help("Instance to be mounted"))
                .about("Mount all or specified instance"),
        )
        .subcommand(
            Command::new("farewell")
                .alias("harakiri")
                .about("Remove everything related to CIEL!"),
        )
        .subcommand(
            Command::new("repo")
                .arg_required_else_help(true)
                .subcommands(vec![Command::new("refresh").about("Refresh the repository"), Command::new("init").arg(Arg::new("INSTANCE").required(true)).about("Initialize the repository"), Command::new("deinit").about("Uninitialize the repository")])
                .alias("localrepo")
                .about("Local repository operations")
        )
        .subcommand(
            Command::new("clean")
                .about("Clean all the output directories and source cache directories")
        )
        .subcommands({
            let plugins = list_helpers();
            if let Ok(plugins) = plugins {
                plugins.iter().map(|plugin| {
                    Command::new(plugin.strip_prefix("ciel-").unwrap_or("???"))
                    .arg(Arg::new("COMMANDS").required(false).min_values(1).help("Applet specific commands"))
                    .about("")
                }).collect()
            } else {
                vec![]
            }
        })
        .args(
            &[
                Arg::new("C")
                    .short('C')
                    .value_name("DIR")
                    .help("set the CIEL! working directory"),
                Arg::new("batch")
                    .short('b')
                    .long("batch")
                    .help("Batch mode, no input required"),
            ]
        )
}
