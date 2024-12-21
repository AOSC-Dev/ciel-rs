use anyhow::Result;
use ciel::{Instance, InstanceConfig, Workspace};
use clap::ArgMatches;

use crate::{config::patch_instance_config, utils::create_spinner};

pub fn add_instance(args: &ArgMatches) -> Result<()> {
    let ws = Workspace::current_dir()?;

    let name = args.get_one::<String>("INSTANCE").unwrap();
    let mut config = InstanceConfig::default();
    patch_instance_config(args, &mut config)?;
    _ = ws.add_instance(name, config)?;

    Ok(())
}

#[inline]
fn one_or_more_instances<F>(args: &ArgMatches, op: F) -> Result<()>
where
    F: Fn(Instance) -> Result<()>,
{
    let ws = Workspace::current_dir()?;

    if args.get_flag("all") {
        for inst in ws.instances()? {
            op(inst)?;
        }
    } else {
        let name = args.get_many::<String>("INSTANCE").unwrap();
        for inst in name {
            op(ws.instance(inst)?)?;
        }
    }
    Ok(())
}

pub fn del_instance(args: &ArgMatches) -> Result<()> {
    one_or_more_instances(args, |inst| Ok(inst.destroy()?))
}

pub fn mount_instance(args: &ArgMatches) -> Result<()> {
    one_or_more_instances(args, |inst| Ok(inst.open()?.overlay_manager().mount()?))
}

pub fn boot_instance(args: &ArgMatches) -> Result<()> {
    let spinner = create_spinner("Booting instance ...", 200);
    one_or_more_instances(args, |inst| Ok(inst.open()?.boot()?))?;
    spinner.finish_with_message("Done.");
    Ok(())
}

pub fn stop_instance(args: &ArgMatches) -> Result<()> {
    let spinner = create_spinner("Stopping instance ...", 200);
    one_or_more_instances(args, |inst| Ok(inst.open()?.stop(false)?))?;
    spinner.finish_with_message("Done.");
    Ok(())
}

pub fn down_instance(args: &ArgMatches) -> Result<()> {
    let spinner = create_spinner("Stopping instance ...", 200);
    one_or_more_instances(args, |inst| Ok(inst.open()?.stop(true)?))?;
    spinner.finish_with_message("Done.");
    Ok(())
}

pub fn rollback_instance(args: &ArgMatches) -> Result<()> {
    let spinner = create_spinner("Rolling back instance ...", 200);
    one_or_more_instances(args, |inst| Ok(inst.open()?.rollback()?))?;
    spinner.finish_with_message("Done.");
    Ok(())
}

pub fn commit_instance(args: &ArgMatches) -> Result<()> {
    let spinner = create_spinner("Commiting instance ...", 200);
    let name = args.get_one::<String>("INSTANCE").unwrap();
    let ws = Workspace::current_dir()?;
    ws.commit(ws.instance(name)?.open()?)?;
    spinner.finish_with_message("Done.");
    Ok(())
}

pub fn run_in_container(args: &ArgMatches) -> Result<()> {
    let name = args.get_one::<String>("INSTANCE").unwrap();
    let commands = args.get_many::<String>("COMMANDS").unwrap();

    let ws = Workspace::current_dir()?;
    let inst = ws.instance(name)?.open()?;
    inst.boot()?;
    inst.machine()?.exec(commands)?;
    Ok(())
}

pub fn shell_run_in_container(args: &ArgMatches) -> Result<()> {
    let name = args.get_one::<String>("INSTANCE").unwrap();
    let commands = args
        .get_many::<String>("COMMANDS")
        .unwrap()
        .collect::<Vec<_>>();
    let mut cmd = vec!["/usr/bin/bash".to_string()];
    if !commands.is_empty() {
        cmd.push("-ec".to_string());
        cmd.push("exec \"$@\"".to_string());
        cmd.push("--".to_string());
        cmd.extend(commands.into_iter().map(|s| s.to_owned()));
    }

    let ws = Workspace::current_dir()?;
    let inst = ws.instance(name)?.open()?;
    inst.boot()?;
    inst.machine()?.exec(cmd)?;
    Ok(())
}
