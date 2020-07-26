//! This module contains systemd machined related APIs

use crate::common::CIEL_INST_DIR;
use crate::dbus_machine1::OrgFreedesktopMachine1Manager;
use crate::dbus_machine1_machine::OrgFreedesktopMachine1Machine;
use adler32::adler32;
use dbus::blocking::{Connection, Proxy};
use failure::{format_err, Error};
use libc::ftok;
use std::ffi::OsStr;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::time::Duration;
use crate::overlayfs::is_mounted;

const MACHINE1_PATH: &str = "/org/freedesktop/machine1";
const MACHINE1_DEST: &str = "org.freedesktop.machine1";

#[derive(Debug)]
pub struct CielInstance {
    name: String,
    // namespace name (in the form of `$name-$id`)
    ns_name: String,
    mounted: bool,
    running: bool,
    booted: Option<bool>,
}

fn legacy_container_name(path: &Path) -> Result<String, Error> {
    let key_id;
    let name = path
        .file_name()
        .ok_or(format_err!("Invalid container path: {:?}", path))?;
    let mut path = path.as_os_str().as_bytes().to_owned();
    path.push(0); // add trailing null terminator
                  // unsafe because of the `ftok` invokation
    unsafe {
        key_id = ftok(path.as_ptr() as *const i8, 0);
    }
    if key_id < 0 {
        return Err(format_err!("ftok() failed."));
    }

    Ok(format!(
        "{}-{:x}",
        name.to_str()
            .ok_or(format_err!("Container name is not valid unicode."))?,
        key_id
    ))
}

fn new_container_name(path: &Path) -> Result<String, Error> {
    let hash = adler32(path.as_os_str().as_bytes())?;
    let name = path
        .file_name()
        .ok_or(format_err!("Invalid container path: {:?}", path))?;

    Ok(format!(
        "{}-{:x}",
        name.to_str()
            .ok_or(format_err!("Container name is not valid unicode."))?,
        hash
    ))
}

pub fn get_container_ns_name(path: &Path, legacy: bool) -> Result<String, Error> {
    if legacy {
        return legacy_container_name(path);
    }

    new_container_name(path)
}

fn is_booted(proxy: &Proxy<&Connection>) -> Result<bool, Error> {
    let leader_pid = proxy.leader()?;
    let f = std::fs::read(&format!("/proc/{}/cmdline", leader_pid))?;
    let pos: usize = f
        .iter()
        .position(|c| *c == 0u8)
        .ok_or(format_err!("Unable to parse cmdline"))?;
    let path = Path::new(OsStr::from_bytes(&f[..pos]));
    let exe_name = path.file_name();
    if let Some(exe_name) = exe_name {
        if exe_name == "systemd" || exe_name == "init" {
            return Ok(true);
        }
    }

    Ok(false)
}

pub fn inspect_instance(name: &str, ns_name: &str) -> Result<CielInstance, Error> {
    let mounted = is_mounted(PathBuf::from(name), &OsStr::new("overlay"))?;
    let conn = Connection::new_system()?;
    let proxy = conn.with_proxy(MACHINE1_DEST, MACHINE1_PATH, Duration::from_secs(10));
    let path = proxy.get_machine(ns_name);
    if let Err(e) = path {
        let err_name = e.name().ok_or_else(|| format_err!("{}", e))?;
        if err_name == "org.freedesktop.machine1.NoSuchMachine" {
            return Ok(CielInstance {
                name: name.to_owned(),
                ns_name: ns_name.to_owned(),
                running: false,
                mounted,
                booted: None,
            });
        }
        return Err(format_err!("{}", e));
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

// pub fn list_instances() -> Result<Vec<CielInstance>, Error> {

// }

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
