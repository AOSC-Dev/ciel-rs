//! This module contains systemd machined related APIs

use crate::common::{is_legacy_workspace, CIEL_INST_DIR};
use crate::dbus_machine1::OrgFreedesktopMachine1Manager;
use crate::dbus_machine1_machine::OrgFreedesktopMachine1Machine;
use crate::overlayfs::is_mounted;
use crate::{color_bool, warn, overlayfs::LayerManager};
use adler32::adler32;
use anyhow::{anyhow, Result};
use console::style;
use dbus::blocking::{Connection, Proxy};
use libc::ftok;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::{ffi::OsStr, process::Command};
use std::{fs, time::Duration};

const MACHINE1_PATH: &str = "/org/freedesktop/machine1";
const MACHINE1_DEST: &str = "org.freedesktop.machine1";
const DEFAULT_NSPAWN_OPTIONS: &[&str] = &[""];

#[derive(Debug)]
pub struct CielInstance {
    name: String,
    // namespace name (in the form of `$name-$id`)
    ns_name: String,
    mounted: bool,
    running: bool,
    booted: Option<bool>,
}

fn legacy_container_name(path: &Path) -> Result<String> {
    let key_id;
    let current_dir = std::env::current_dir()?;
    let name = path
        .file_name()
        .ok_or(anyhow!("Invalid container path: {:?}", path))?;
    let mut path = current_dir.as_os_str().as_bytes().to_owned();
    path.push(0); // add trailing null terminator
    unsafe {
        // unsafe because of the `ftok` invokation
        key_id = ftok(path.as_ptr() as *const i8, 0);
    }
    if key_id < 0 {
        return Err(anyhow!("ftok() failed."));
    }

    Ok(format!(
        "{}-{:x}",
        name.to_str()
            .ok_or(anyhow!("Container name is not valid unicode."))?,
        key_id
    ))
}

fn new_container_name(path: &Path) -> Result<String> {
    let hash = adler32(path.as_os_str().as_bytes())?;
    let name = path
        .file_name()
        .ok_or(anyhow!("Invalid container path: {:?}", path))?;

    Ok(format!(
        "{}-{:x}",
        name.to_str()
            .ok_or(anyhow!("Container name is not valid unicode."))?,
        hash
    ))
}

pub fn get_container_ns_name<P: AsRef<Path>>(path: P, legacy: bool) -> Result<String> {
    let current_dir = std::env::current_dir()?;
    let path = current_dir.join(path);
    if legacy {
        return legacy_container_name(&path);
    }

    new_container_name(&path)
}

pub fn spawn_container<P: AsRef<Path>>(
    ns_name: &str,
    path: P,
    extra_options: &[String],
) -> Result<()> {
    Command::new("systemd-nspawn")
        .args(DEFAULT_NSPAWN_OPTIONS)
        .args(extra_options)
        .spawn()?;

    Ok(())
}

fn poweroff_container(proxy: &Proxy<&Connection>) -> Result<()> {
    let poweroff = nc::types::SIGRTMIN + 4; // only works with systemd
    proxy.kill("leader", poweroff)?;

    Ok(())
}

fn kill_container(proxy: &Proxy<&Connection>) -> Result<()> {
    proxy.kill("all", nc::types::SIGKILL)?;
    proxy.terminate()?;

    Ok(())
}

fn is_booted(proxy: &Proxy<&Connection>) -> Result<bool> {
    let leader_pid = proxy.leader()?;
    let f = std::fs::read(&format!("/proc/{}/cmdline", leader_pid))?;
    let pos: usize = f
        .iter()
        .position(|c| *c == 0u8)
        .ok_or(anyhow!("Unable to parse cmdline"))?;
    let path = Path::new(OsStr::from_bytes(&f[..pos]));
    let exe_name = path.file_name();
    if let Some(exe_name) = exe_name {
        return Ok(exe_name == "systemd" || exe_name == "init");
    }

    Ok(false)
}

pub fn terminate_container(proxy: &Proxy<&Connection>) -> Result<()> {
    if !is_booted(proxy)? {
        proxy.terminate()?;
        return Ok(());
    }

    // with booted container, we want to power it off gracefully ...
    poweroff_container(proxy)?;
    todo!()
}

/// Mount the filesystem layers using the specified layer manager and the instance name
pub fn mount_layers(manager: &mut dyn LayerManager, name: &str) -> Result<()> {
    let target = std::env::current_dir()?.join(name);
    if !manager.is_mounted(&target)? {
        fs::create_dir_all(&target)?;
        manager.mount(&target)?;
    }

    Ok(())
}

pub fn inspect_instance(name: &str, ns_name: &str) -> Result<CielInstance> {
    let full_path = std::env::current_dir()?.join(name);
    let mounted = is_mounted(&full_path, &OsStr::new("overlay"))?;
    let conn = Connection::new_system()?;
    let proxy = conn.with_proxy(MACHINE1_DEST, MACHINE1_PATH, Duration::from_secs(10));
    let path = proxy.get_machine(ns_name);
    if let Err(e) = path {
        let err_name = e.name().ok_or_else(|| anyhow!("{}", e))?;
        if err_name == "org.freedesktop.machine1.NoSuchMachine" {
            return Ok(CielInstance {
                name: name.to_owned(),
                ns_name: ns_name.to_owned(),
                running: false,
                mounted,
                booted: None,
            });
        }
        return Err(anyhow!("{}", e));
    }
    let path = path?;
    let proxy = conn.with_proxy(MACHINE1_DEST, path, Duration::from_secs(10));
    let state = proxy.state()?;
    let running = state == "running" || state == "degraded";
    let booted = is_booted(&proxy)?;

    Ok(CielInstance {
        name: name.to_owned(),
        ns_name: ns_name.to_owned(),
        running,
        mounted,
        booted: Some(booted),
    })
}

pub fn list_instances() -> Result<Vec<CielInstance>> {
    let legacy = is_legacy_workspace()?;
    let mut instances: Vec<CielInstance> = Vec::new();
    for entry in fs::read_dir(CIEL_INST_DIR)? {
        if let Ok(entry) = entry {
            if entry.file_type().map(|e| e.is_dir())? {
                instances.push(inspect_instance(
                    &entry.file_name().to_string_lossy(),
                    &get_container_ns_name(&entry.file_name(), legacy)?,
                )?);
            }
        }
    }

    Ok(instances)
}

pub fn print_instances() -> Result<()> {
    let instances = list_instances()?;
    eprintln!("NAME\t\tMOUNTED\t\tRUNNING\t\tBOOTED");
    for instance in instances {
        let mounted = color_bool!(instance.mounted);
        let running = color_bool!(instance.running);
        let booted = {
            if let Some(booted) = instance.booted {
                color_bool!(booted)
            } else {
                style("-").dim()
            }
        };
        eprintln!(
            "{}\t\t{}\t\t{}\t\t{}",
            instance.name, mounted, running, booted
        );
    }

    Ok(())
}

#[test]
fn test_inspect_instance() {
    println!("{:#?}", inspect_instance("alpine", "alpine"));
}

#[test]
fn test_container_name() {
    assert_eq!(
        get_container_ns_name(Path::new("/tmp/"), false).unwrap(),
        "tmp-51601b0".to_string()
    );
    println!(
        "{:#?}",
        get_container_ns_name(Path::new("/tmp/"), true).unwrap()
    );
}
