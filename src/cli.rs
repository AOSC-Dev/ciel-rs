use anyhow::{anyhow, Result};
use clap::{Arg, Command};
use std::ffi::OsStr;

pub const GIT_TREE_URL: &str = "https://github.com/AOSC-Dev/aosc-os-abbs.git";

/// List all the available plugins/helper scripts
fn list_helpers() -> Result<Vec<String>> {
    let exe_dir = std::env::current_exe().and_then(std::fs::canonicalize)?;
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
pub fn build_cli() -> Command {
    let instance_arg = Arg::new("INSTANCE")
        .short('i')
        .num_args(1)
        .env("CIEL_INST")
        .action(clap::ArgAction::Set);
    Command::new("ciel")
        .version(env!("CARGO_PKG_VERSION"))
        .about("CIEL! is a nspawn container manager")
        .allow_external_subcommands(true)
        .subcommand(Command::new("version").about("Display the version of CIEL!"))
        .subcommand(Command::new("init")
            .arg(Arg::new("upgrade").long("upgrade").action(clap::ArgAction::SetTrue).help("Upgrade Ciel workspace from an older version"))
            .about("Initialize the work directory"))
        .subcommand(
            Command::new("load-os")
                .arg(Arg::new("url").help("URL or path to the tarball"))
                .arg(Arg::new("arch").short('a').long("arch").help("Specify the target architecture for fetching OS tarball"))
                .about("Unpack OS tarball or fetch the latest BuildKit from the repository"),
        )
        .subcommand(
            Command::new("update-os")
                .arg(Arg::new("force_use_apt").long("force-use-apt").help("Use apt to update-os").action(clap::ArgAction::SetTrue))
                .about("Update the OS in the container")
        )
        .subcommand(
            Command::new("load-tree")
                .arg(Arg::new("url").default_value(GIT_TREE_URL).help("URL to the git repository"))
                .about("Clone package tree from the link provided or AOSC OS ABBS main repository"),
        )
        .subcommand(
            Command::new("update-tree")
                .arg(Arg::new("rebase").num_args(1).short('r').long("rebase").help("Rebase the specified branch from the updated upstream"))
                .arg(Arg::new("branch").num_args(1).help("Branch to switch to"))
                .about("Update the existing ABBS tree (fetch only) and optionally switch to a different branch")
        )
        .subcommand(
            Command::new("new")
            .arg(Arg::new("tarball").num_args(1).long("from-tarball").help("Create a new workspace from the specified tarball"))
            .arg(Arg::new("arch").num_args(1).short('a').long("arch").help("Create a new workspace for specified architecture"))
            .about("Create a new CIEL workspace")
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
                .arg(instance_arg.clone().help("Instance to be used"))
                .arg(Arg::new("COMMANDS").required(false).num_args(1..))
                .about("Start an interactive shell"),
        )
        .subcommand(
            Command::new("run")
                .alias("exec")
                .arg(instance_arg.clone().help("Instance to run command in"))
                .arg(Arg::new("COMMANDS").required(true).num_args(1..))
                .about("Lower-level version of 'shell', without login environment, without sourcing ~/.bash_profile"),
        )
        .subcommand(
            Command::new("config")
                .arg(instance_arg.clone().help("Instance to be configured"))
                .arg(Arg::new("g").short('g').action(clap::ArgAction::SetTrue).help("Configure base system instead of an instance"))
                .about("Configure system and toolchain for building interactively"),
        )
        .subcommand(
            Command::new("commit")
                .arg(instance_arg.clone().help("Instance to be committed"))
                .about("Commit changes onto the shared underlying OS"),
        )
        .subcommand(
            Command::new("doctor")
                .about("Diagnose problems (hopefully)"),
        )
        .subcommand(
            Command::new("build")
                .arg(Arg::new("FETCH").short('g').action(clap::ArgAction::SetTrue).help("Fetch source packages only"))
                .arg(Arg::new("OFFLINE").short('x').long("offline").action(clap::ArgAction::SetTrue).env("CIEL_OFFLINE").help("Disable network in the container during the build"))
                .arg(instance_arg.clone().help("Instance to build in"))
                .arg(Arg::new("STAGE2").long("stage2").short('2').action(clap::ArgAction::SetTrue).env("CIEL_STAGE2").help("Use stage 2 mode instead of the regular build mode"))
                .arg(Arg::new("force_use_apt").long("force-use-apt").action(clap::ArgAction::SetTrue).env("FORCE_USE_APT").help("Force use apt to run acbs"))
                .arg(Arg::new("TOPICS").long("with-topics").action(clap::ArgAction::Append).num_args(1..).help("Try to add topics before building, delimited by space"))
                .arg(Arg::new("CONTINUE").conflicts_with("SELECT").short('c').long("resume").alias("continue").num_args(1).help("Continue from a Ciel checkpoint"))
                .arg(Arg::new("SELECT").num_args(0..=1).long("stage-select").help("Select the starting point for a build"))
                .arg(Arg::new("PACKAGES").conflicts_with("CONTINUE").num_args(1..))
                .about("Build the packages using the specified instance"),
        )
        .subcommand(
            Command::new("rollback")
                .arg(instance_arg.clone().help("Instance to be rolled back"))
                .about("Rollback all or specified instance"),
        )
        .subcommand(
            Command::new("down")
                .alias("umount")
                .arg(instance_arg.clone().help("Instance to be un-mounted"))
                .about("Shutdown and unmount all or one instance"),
        )
        .subcommand(
            Command::new("stop")
                .arg(instance_arg.clone().help("Instance to be stopped"))
                .about("Shuts down an instance"),
        )
        .subcommand(
            Command::new("mount")
                .arg(instance_arg.help("Instance to be mounted"))
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
                    let name = plugin.strip_prefix("ciel-").unwrap_or("???");
                    Command::new(name.to_string())
                    .arg(Arg::new("COMMANDS").required(false).num_args(1..).help("Applet specific commands"))
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
                    .default_value(".")
                    .num_args(1..)
                    .help("Set the CIEL! working directory"),
                Arg::new("batch")
                    .short('b')
                    .long("batch")
                    .action(clap::ArgAction::SetTrue)
                    .help("Batch mode, no input required"),
            ]
        )
}
