use anyhow::{anyhow, Result};
use clap::{builder::ValueParser, value_parser, Arg, ArgAction, Command};
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

fn config_list(id: &str, name: &str, parser: ValueParser) -> [Arg; 3] {
    [
        Arg::new(format!("add-{id}"))
            .long(format!("add-{id}"))
            .help(format!("Add an {name}"))
            .value_name(id.to_owned())
            .value_parser(parser.clone())
            .required(false),
        Arg::new(format!("remove-{id}"))
            .long(format!("remove-{id}"))
            .help(format!("Remove an {name}"))
            .value_name(id.to_owned())
            .value_parser(parser)
            .required(false),
        Arg::new(format!("unset-{id}"))
            .long(format!("unset-{id}"))
            .help(format!("Remove all {name}"))
            .action(ArgAction::SetTrue),
    ]
}

/// Build the CLI instance
pub fn build_cli() -> Command {
    let instance_arg = Arg::new("INSTANCE")
        .short('i')
        .num_args(1)
        .env("CIEL_INST")
        .action(clap::ArgAction::Set);
    let mut workspace_configs: Vec<Arg> = vec![
        Arg::new("maintainer")
            .long("maintainer")
            .short('m')
            .help("Maintainer information")
            .value_parser(value_parser!(String)),
        Arg::new("dnssec")
            .long("dnssec")
            .help("Enable DNSSEC")
            .value_parser(value_parser!(bool)),
        Arg::new("local-repo")
            .long("local-repo")
            .help("Enable local package repository")
            .value_parser(value_parser!(bool)),
        Arg::new("source-cache")
            .long("source-cache")
            .help("Enable local source caches")
            .value_parser(value_parser!(bool)),
        Arg::new("branch-exclusive-output")
            .long("branch-exclusive-output")
            .help("Use different OUTPUT directory for branches")
            .value_parser(value_parser!(bool)),
        Arg::new("volatile-mount")
            .long("volatile-mount")
            .help("Enable volatile mount")
            .value_parser(value_parser!(bool)),
        Arg::new("use-apt")
            .long("use-apt")
            .help("Force to use APT")
            .value_parser(value_parser!(bool)),
    ];
    workspace_configs.extend(config_list(
        "repo",
        "extra APT repository",
        value_parser!(String),
    ));
    workspace_configs.extend(config_list(
        "nspawn-opt",
        "extra nspawn option",
        value_parser!(String),
    ));
    let mut instance_configs = vec![
        Arg::new("local-repo")
            .long("local-repo")
            .help("Enable local package repository")
            .value_parser(value_parser!(bool)),
        // tmpfs
        Arg::new("tmpfs")
            .long("tmpfs")
            .help("Enable tmpfs")
            .value_parser(value_parser!(bool)),
        Arg::new("tmpfs-size")
            .long("tmpfs-size")
            .help("Size of tmpfs to use, in MiB")
            .value_parser(value_parser!(u64)),
        Arg::new("unset-tmpfs-size")
            .long("unset-tmpfs-size")
            .help("Reset tmpfs size to default")
            .action(ArgAction::SetTrue),
        // read-write tree
        Arg::new("rw-tree")
            .long("rw-tree")
            .help("Mount TREE as read-write")
            .value_parser(value_parser!(bool)),
    ];
    instance_configs.extend(config_list(
        "repo",
        "extra APT repository",
        value_parser!(String),
    ));
    instance_configs.extend(config_list(
        "nspawn-opt",
        "extra nspawn option",
        value_parser!(String),
    ));
    let one_or_more_instances = [
        Arg::new("INSTANCE")
            .required(false)
            .num_args(1..)
            .env("CIEL_INST"),
        Arg::new("all")
            .short('a')
            .long("all")
            .action(ArgAction::SetTrue)
            .required_unless_present("INSTANCE"),
    ];

    Command::new("ciel")
        .version(env!("CARGO_PKG_VERSION"))
        .about("CIEL! is a nspawn container manager")
        .allow_external_subcommands(true)
        .subcommand(Command::new("version").about("Display the version of CIEL!"))
        .subcommand(
            Command::new("list")
                .alias("ls")
                .about("List all instances in the workspace"),
        )
        .subcommand(
            Command::new("new")
                .alias("init")
                .arg(
                    Arg::new("no-load-os")
                        .long("no-load-os")
                        .action(ArgAction::SetTrue)
                        .help("Don't load OS automatically after initialization")
                        .conflicts_with_all(["rootfs", "sha256"]),
                )
                .arg(
                    Arg::new("rootfs")
                        .num_args(1)
                        .long("rootfs")
                        .alias("from-tarball")
                        .help("Specify the tarball or squashfs to load after initialization"),
                )
                .arg(
                    Arg::new("sha256")
                        .long("sha256")
                        .required(false)
                        .help("Specify the SHA-256 checksum of OS tarball"),
                )
                .arg(
                    Arg::new("arch")
                        .short('a')
                        .long("arch")
                        .help("Specify the architecture of the workspace"),
                )
                .arg(
                    Arg::new("no-load-tree")
                        .long("no-load-tree")
                        .action(ArgAction::SetTrue)
                        .help("Don't load abbs tree automatically after initialization")
                        .conflicts_with("tree"),
                )
                .arg(
                    Arg::new("tree")
                        .long("tree")
                        .default_value(GIT_TREE_URL)
                        .help("URL to the abbs tree git repository"),
                )
                .args(
                    workspace_configs
                        .iter()
                        .cloned()
                        .map(|arg| arg.required(false)),
                )
                .about("Create a new CIEL! workspace"),
        )
        .subcommand(
            Command::new("farewell")
                .alias("harakiri")
                .about("Remove everything related to CIEL!")
                .arg(
                    Arg::new("force")
                        .short('f')
                        .action(ArgAction::SetTrue)
                        .help("Force perform deletion without user confirmation"),
                ),
        )
        .subcommand(
            Command::new("load-os")
                .arg(
                    Arg::new("URL")
                        .required(false)
                        .help("URL or path to the tarball or squashfs"),
                )
                .arg(
                    Arg::new("sha256")
                        .long("sha256")
                        .required(false)
                        .help("Specify the SHA-256 checksum of OS tarball"),
                )
                .arg(
                    Arg::new("arch")
                        .short('a')
                        .long("arch")
                        .help("Specify the target architecture for fetching OS tarball"),
                )
                .arg(
                    Arg::new("force")
                        .short('f')
                        .long("force")
                        .action(ArgAction::SetTrue)
                        .help("Force override the loaded system"),
                )
                .about("Unpack OS tarball or fetch the latest BuildKit"),
        )
        .subcommand(
            Command::new("update-os")
                .arg(
                    Arg::new("force-use-apt")
                        .long("force-use-apt")
                        .help("Use apt to update-os")
                        .action(ArgAction::SetTrue),
                )
                .args(instance_configs.iter().cloned())
                .about("Update the OS in the container"),
        )
        .subcommand(
            Command::new("instconf")
                .arg(
                    instance_arg
                        .clone()
                        .help("Instance to be configured")
                        .required(true),
                )
                .arg(
                    Arg::new("force-no-rollback")
                        .long("force-no-rollback")
                        .action(ArgAction::SetTrue)
                        .help("Do not rollback instances to apply configuration"),
                )
                .args(instance_configs.iter().cloned())
                .about("Configure instances"),
        )
        .subcommand(
            Command::new("config")
                .arg(
                    Arg::new("force-no-rollback")
                        .long("force-no-rollback")
                        .action(ArgAction::SetTrue)
                        .help("Do not rollback instances to apply configuration"),
                )
                .args(workspace_configs.iter().cloned())
                .about("Configure workspace"),
        )
        .subcommand(
            Command::new("load-tree")
                .arg(
                    Arg::new("URL")
                        .default_value(GIT_TREE_URL)
                        .help("URL to the git repository"),
                )
                .about("Clone abbs tree from git"),
        )
        .subcommand(
            Command::new("add")
                .arg(Arg::new("INSTANCE").required(true))
                .args(instance_configs.iter().cloned())
                .about("Add a new instance"),
        )
        .subcommand(
            Command::new("del")
                .alias("rm")
                .args(&one_or_more_instances)
                .about("Remove one or all instance"),
        )
        .subcommand(
            Command::new("mount")
                .args(&one_or_more_instances)
                .about("Mount one or all instance"),
        )
        .subcommand(
            Command::new("boot")
                .args(&one_or_more_instances)
                .about("Start one or all instance"),
        )
        .subcommand(
            Command::new("stop")
                .args(&one_or_more_instances)
                .about("Shutdown one or all instance"),
        )
        .subcommand(
            Command::new("down")
                .alias("umount")
                .args(&one_or_more_instances)
                .about("Shutdown and unmount one or all instance"),
        )
        .subcommand(
            Command::new("rollback")
                .alias("reset")
                .args(&one_or_more_instances)
                .about("Rollback one or all instance"),
        )
        .subcommand(
            Command::new("commit")
                .arg(Arg::new("INSTANCE").env("CIEL_INST").required(true))
                .about("Commit changes onto the underlying base system"),
        )
        .subcommand(
            Command::new("shell")
                .alias("sh")
                .arg(
                    instance_arg
                        .clone()
                        .required(false)
                        .help("Instance to be used"),
                )
                .args(
                    instance_configs
                        .iter()
                        .cloned()
                        .map(|arg| arg.conflicts_with("INSTANCE")),
                )
                .arg(Arg::new("COMMANDS").required(false).num_args(1..))
                .about("Start an interactive shell or run a shell command"),
        )
        .subcommand(
            Command::new("run")
                .alias("exec")
                .arg(instance_arg.clone().help("Instance to run command in"))
                .arg(Arg::new("COMMANDS").required(true).num_args(1..))
                .about("Run a command in the container"),
        )
        .subcommand(
            Command::new("build")
                .arg(
                    instance_arg
                        .clone()
                        .required(false)
                        .help("Instance to be used"),
                )
                .args(
                    instance_configs
                        .iter()
                        .cloned()
                        .map(|arg| arg.conflicts_with("INSTANCE")),
                )
                .arg(
                    Arg::new("fetch-only")
                        .short('g')
                        .action(ArgAction::SetTrue)
                        .help("Fetch package sources only"),
                )
                .arg(
                    Arg::new("resume")
                        .short('c')
                        .long("resume")
                        .alias("continue")
                        .num_args(1)
                        .help("Resume from a Ciel checkpoint")
                        .conflicts_with("fetch-only")
                        .conflicts_with("select"),
                )
                .arg(
                    Arg::new("select")
                        .long("stage-select")
                        .action(ArgAction::SetTrue)
                        .help("Select the starting point for a build"),
                )
                .arg(
                    Arg::new("always-discard")
                        .long("always-discard")
                        .action(ArgAction::SetTrue)
                        .conflicts_with("INSTANCE")
                        .help("Destory ephemeral containers if the build fails"),
                )
                .arg(Arg::new("PACKAGES").conflicts_with("resume").num_args(1..))
                .about("Build the packages using the specified instance"),
        )
        .subcommand(
            Command::new("repo")
                .arg_required_else_help(true)
                .subcommands([Command::new("refresh")
                    .alias("init")
                    .about("Refresh the repository")])
                .alias("localrepo")
                .about("Local repository maintenance"),
        )
        .subcommand(
            Command::new("clean")
                .about("Clean all the output directories and source cache directories"),
        )
        .subcommand(
            Command::new("diagnose")
                .alias("doctor")
                .about("Diagnose problems (hopefully)"),
        )
        .subcommands({
            let plugins = list_helpers();
            if let Ok(plugins) = plugins {
                plugins
                    .iter()
                    .map(|plugin| {
                        let name = plugin.strip_prefix("ciel-").unwrap_or("???");
                        Command::new(name.to_string())
                            .arg(
                                Arg::new("COMMANDS")
                                    .required(false)
                                    .num_args(1..)
                                    .help("Applet specific commands"),
                            )
                            .about("")
                    })
                    .collect()
            } else {
                vec![]
            }
        })
        .arg(
            Arg::new("ciel-dir")
                .short('C')
                .value_name("DIR")
                .default_value(".")
                .env("CIEL_DIR")
                .help("Set the CIEL! working directory"),
        )
        .arg(
            Arg::new("quiet")
                .short('q')
                .long("quiet")
                .action(ArgAction::SetTrue)
                .help("shhhhhh!"),
        )
}
