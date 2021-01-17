use anyhow::{anyhow, Result};
use console::style;
use dbus::blocking::stdintf::org_freedesktop_dbus::Properties;
use dbus::blocking::Connection;
use std::sync::mpsc::channel;
use std::{fs::File, io::BufRead, time::Duration};
use std::{
    io::{BufReader, Write},
    thread,
};
use tempfile::tempfile_in;
use which::which;

use crate::error;

const SYSTEMD1_PATH: &str = "/org/freedesktop/systemd1";
const SYSTEMD1_DEST: &str = "org.freedesktop.systemd1";
const SYSTEMD1_OBJ: &str = "org.freedesktop.systemd1.Manager";
const TEST_TEXT: &[u8] = b"An-An was born a rabbit, but found herself a girl with bunny ears and tails when she woke up one day. She couldn't seem to remember why.";
const TEST_PROGRAMS: &[&str] = &["systemd-nspawn", "systemd-run", "dpkg-scanpackages"];
const TEST_CASES: &[&dyn Fn() -> Result<String>] = &[
    &test_sd_bus,
    &test_io_simple,
    &test_required_binaries,
    &test_fs_support,
    &test_vm_container,
    &test_disk_io,
];

fn test_sd_bus() -> Result<String> {
    let conn = Connection::new_system()?;
    let proxy = conn.with_proxy(SYSTEMD1_DEST, SYSTEMD1_PATH, Duration::from_secs(10));
    let version: String = proxy.get(SYSTEMD1_OBJ, "Version")?;
    Ok(format!(
        "Systemd D-Bus (systemd {}) seems to be working",
        version
    ))
}

fn test_io_simple() -> Result<String> {
    File::open("/proc/1/cmdline")?;
    Ok("Basic I/O operations seem to be working".to_string())
}

fn test_required_binaries() -> Result<String> {
    for binary in TEST_PROGRAMS {
        if which(binary).is_err() {
            return Err(anyhow!("Required program `{}` is not found", binary));
        }
    }
    Ok("Required binaries are correctly installed".to_string())
}

fn test_fs_support() -> Result<String> {
    let f = File::open("/proc/filesystems")?;
    let reader = BufReader::new(f);
    for line in reader.lines() {
        let line = line?;
        let mut fs_type = line.splitn(2, '\t');
        if let Some(fs_type) = fs_type.nth(1) {
            if fs_type == "overlay" {
                return Ok("Filesystem support seems to be sufficient".to_string());
            }
        }
    }

    Err(anyhow!(
        "Kernel does not support overlayfs, try `modprobe overlay`"
    ))
}

fn test_vm_container() -> Result<String> {
    let conn = Connection::new_system()?;
    let proxy = conn.with_proxy(SYSTEMD1_DEST, SYSTEMD1_PATH, Duration::from_secs(10));
    let virt: String = proxy.get(SYSTEMD1_OBJ, "Virtualization")?;
    if virt == "wsl" {
        return Ok("!WSL is not supported".to_string());
    }
    let virt_msg;
    if virt.is_empty() {
        virt_msg = String::new();
    } else {
        virt_msg = format!("(running in {})", virt);
    }
    Ok(format!("Environment seems sane {}", virt_msg))
}

fn test_disk_io() -> Result<String> {
    let (tx, rx) = channel();
    thread::spawn(move || {
        let f = tempfile_in("./");
        if let Ok(mut f) = f {
            if let Ok(()) = f.write_all(TEST_TEXT) {
                tx.send(()).unwrap();
            }
        }
    });

    if rx.recv_timeout(Duration::from_secs(10)).is_ok() {
        return Ok("Disk I/O seems ok".to_string());
    }

    error!("The test file is taking too long to write, suspecting I/O stuck.");

    Err(anyhow!("Disk I/O is not working correctly"))
}

/// Carry out the diagnostic tests
pub fn run_diagnose() -> Result<()> {
    let mut lines = vec![];
    let mut has_error = false;
    for test in TEST_CASES {
        match test() {
            Ok(msg) => {
                if msg.starts_with('!') {
                    lines.push(format!(
                        "{} {}",
                        style("!").yellow(),
                        style(msg.strip_prefix("!").unwrap()).yellow().bold()
                    ));
                    continue;
                }
                lines.push(format!(
                    "{} {}",
                    style("✓").green(),
                    style(msg).green().bold()
                ))
            }
            Err(err) => {
                has_error = true;
                lines.push(format!("{} {}", style("x").red(), style(err).red().bold()));
                break;
            }
        }
    }

    for line in lines {
        println!("{}", line);
    }
    if has_error {
        return Err(anyhow!("Test error detected"));
    }

    Ok(())
}
