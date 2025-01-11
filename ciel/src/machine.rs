use std::{
    ffi::{CString, OsStr},
    fs,
    mem::MaybeUninit,
    os::unix::ffi::OsStrExt,
    path::{Path, PathBuf},
    process::{Child, Command, ExitStatus, Stdio},
    sync::Arc,
    time::Duration,
};

use log::{debug, info, warn};

use crate::{
    ContainerConfig, Error, Result, dbus_machine1_machine::MachineProxyBlocking,
    dbus_machine1_manager::ManagerProxyBlocking,
};

/// A systemd-nspawn machine.
pub struct Machine {
    config: Arc<ContainerConfig>,
    rootfs_path: PathBuf,
    dbus_conn: zbus::blocking::Connection,
}

impl Machine {
    pub(crate) fn new<P: AsRef<Path>>(
        config: Arc<ContainerConfig>,
        rootfs_path: P,
    ) -> Result<Self> {
        Ok(Self {
            config,
            rootfs_path: rootfs_path.as_ref().to_owned(),
            dbus_conn: zbus::blocking::Connection::system()?,
        })
    }

    /// Returns the NS name of machine.
    pub fn name(&self) -> &str {
        &self.config.ns_name
    }

    /// Returns the state of machine.
    pub fn state(&self) -> Result<MachineState> {
        let proxy = ManagerProxyBlocking::new(&self.dbus_conn)?;
        let path = proxy.get_machine(self.name());
        if let Err(zbus::Error::MethodError(ref err_name, _, _)) = path {
            if err_name.as_ref() == "org.freedesktop.machine1.NoSuchMachine" {
                return Ok(MachineState::Down);
            }
        }
        let path = path?;
        let proxy = MachineProxyBlocking::builder(&self.dbus_conn)
            .path(&path)?
            .build()?;
        let state = proxy.state()?;
        // Sometimes the system in the container is misconfigured,
        // so we also accept "degraded" status as "running"
        if state != "running" && state != "degraded" {
            return Ok(MachineState::Starting);
        }

        // inspect the cmdline of the PID 1 in the container
        let f = std::fs::read(format!("/proc/{}/cmdline", proxy.leader()?))?;
        // take until the first null byte
        let pos = f.iter().position(|c| *c == 0u8).unwrap();
        // ... well, of course it's a path
        let path = Path::new(OsStr::from_bytes(&f[..pos]));
        let exe_name = path.file_name();
        // if PID 1 is systemd or init (System V init) then it should be a "booted" container
        if let Some(exe_name) = exe_name {
            if exe_name == "systemd" || exe_name == "init" {
                return Ok(MachineState::Running);
            }
        }
        Ok(MachineState::Starting)
    }

    /// Boots this machine up.
    ///
    /// Note that the container configuration is not yet applied after this.
    pub fn boot(&self) -> Result<()> {
        info!("{}: waiting for machine to start...", self.name());
        let mut child = Command::new("systemd-nspawn");
        child
            .args([
                "-qb",
                "--capability=CAP_IPC_LOCK",
                "--system-call-filter=swapcontext",
            ])
            .args(&self.config.workspace_config.extra_nspawn_options)
            .args(&self.config.instance_config.extra_nspawn_options)
            .args([
                "-D",
                self.rootfs_path
                    .to_str()
                    .ok_or_else(|| Error::InvalidInstancePath(self.rootfs_path.to_owned()))?,
                "-M",
                self.name(),
                "--",
            ])
            .env("SYSTEMD_NSPAWN_TMPFS_TMP", "0")
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        debug!(
            "invoking systemd-nspawn {:?}",
            child.get_args().collect::<Vec<_>>().join(OsStr::new(" "))
        );
        let child = child.spawn()?;
        wait_for_machine(child, self.name())?;
        Ok(())
    }

    /// Binds a host directory into the machine.
    pub fn bind<P: AsRef<Path>>(&self, host: P, guest: P, read_only: bool) -> Result<()> {
        let host = host.as_ref();
        let guest = guest.as_ref();

        let conn = zbus::blocking::Connection::system()?;
        let proxy = ManagerProxyBlocking::new(&conn)?;
        fs::create_dir_all(host)?;
        proxy.bind_mount_machine(
            self.name(),
            &fs::canonicalize(host)?.to_string_lossy(),
            &guest.to_string_lossy(),
            read_only,
            true,
        )?;
        Ok(())
    }

    /// Sends a poweroff signal to the machine, but does not wait.
    pub fn poweroff(&self) -> Result<()> {
        let exit_code = Command::new("systemd-run")
            .env("SYSTEMD_ADJUST_TERMINAL_TITLE", "0")
            .args(["-M", self.name(), "-q", "--no-block", "--", "poweroff"])
            .spawn()?
            .wait()?;
        if exit_code.success() {
            Ok(())
        } else {
            Err(Error::SubcommandError(exit_code))
        }
    }

    /// Stops the machine.
    ///
    /// This will first try to send a poweroff signal through [Machine::poweroff], and
    /// wait for the machine to go off. If timeout, SIGKILL will be sent to the container.
    pub fn stop(&self) -> Result<()> {
        info!("{}: stopping", self.name());
        let proxy = ManagerProxyBlocking::new(&self.dbus_conn)?;
        let path = proxy.get_machine(self.name())?;
        let machine_proxy = MachineProxyBlocking::builder(&self.dbus_conn)
            .path(&path)?
            .build()?;

        let _ = machine_proxy.receive_state_changed();
        if self.poweroff().is_ok() {
            if wait_for_poweroff(&proxy, self.name()).is_ok() {
                return Ok(());
            }
            warn!(
                "{}: container not responding to poweroff, sending SIGKILL ...",
                self.name()
            );
        }

        machine_proxy.kill("all", nix::sys::signal::SIGKILL as i32)?;
        wait_for_poweroff(&proxy, self.name())?;
        machine_proxy.terminate()?;
        proxy.terminate_machine(self.name())?;

        Ok(())
    }

    /// Executes a command in the machine.
    pub fn exec<I, S>(&self, args: I) -> Result<ExitStatus>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        if self.state()?.is_down() {
            return Err(Error::ImproperState);
        }
        // FIXME: maybe replace with systemd API cross-namespace call?
        let mut child = Command::new("systemd-run");
        child
            .env("SYSTEMD_ADJUST_TERMINAL_TITLE", "0")
            .args(["-M", self.name(), "-qt", "--setenv=HOME=/root", "--"])
            .args(args);
        debug!(
            "invoking systemd-run: {:?}",
            child.get_args().collect::<Vec<_>>().join(&OsStr::new(" "))
        );
        Ok(child.spawn()?.wait()?)
    }

    /// Executes a command in the machine, capturing stdout and stderr.
    pub fn exec_capture<I, S>(&self, args: I) -> Result<ExecResult>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        if self.state()?.is_down() {
            return Err(Error::ImproperState);
        }
        // FIXME: maybe replace with systemd API cross-namespace call?
        let mut child = Command::new("systemd-run");
        child
            .env("SYSTEMD_ADJUST_TERMINAL_TITLE", "0")
            .args(["-M", self.name(), "-qt", "--setenv=HOME=/root", "--"])
            .args(args)
            .stderr(Stdio::piped())
            .stdout(Stdio::piped());
        debug!(
            "invoking systemd-run: {:?}",
            child.get_args().collect::<Vec<_>>().join(&OsStr::new(" "))
        );
        let mut child = child.spawn()?;
        let status = child.wait()?;
        Ok(ExecResult {
            status,
            stdout: std::io::read_to_string(child.stdout.unwrap())?,
            stderr: std::io::read_to_string(child.stderr.unwrap())?,
        })
    }

    /// Updates the system of the machine with oma or APT.
    pub fn update_system(&self, apt: Option<bool>) -> Result<()> {
        let apt = apt.unwrap_or(self.config.workspace_config.use_apt);
        let script = if apt {
            APT_UPDATE_SCRIPT
        } else {
            OMA_UPDATE_SCRIPT
        };
        if apt {
            let status = self.exec(["/usr/bin/bash", "-ec", script])?;
            if !status.success() {
                Err(Error::SubcommandError(status))
            } else {
                Ok(())
            }
        } else {
            if !self.exec(["/usr/bin/bash", "-ec", script])?.success() {
                warn!(
                    "{}: failed to update OS with oma, falling back to apt",
                    self.name()
                );
                self.update_system(Some(true))
            } else {
                Ok(())
            }
        }
    }
}

const APT_UPDATE_SCRIPT: &str = r#"set -euo pipefail;export DEBIAN_FRONTEND=noninteractive;apt-get update -y --allow-releaseinfo-change && apt-get -y -o Dpkg::Options::="--force-confnew" full-upgrade --autoremove --purge && apt autoclean"#;
const OMA_UPDATE_SCRIPT: &str = r#"set -euo pipefail;oma upgrade -y --force-confnew --no-progress --force-unsafe-io && oma autoremove --no-progress -y --remove-config && oma clean --no-progress"#;

fn wait_for_machine(mut child: Child, ns_name: &str) -> Result<()> {
    for i in 0..10 {
        let exited = child.try_wait()?;
        if let Some(status) = exited {
            return Err(Error::SubcommandError(status));
        }
        // PTY spawning may happen before the systemd in the container is fully initialized.
        // To spawn a new process in the container, we need the systemd
        // in the container to be fully initialized and listening for connections.
        // One way to resolve this issue is to test the connection to the container's systemd.
        {
            // There are bunch of trickeries happening here
            // First we initialize an empty pointer
            let mut buf = MaybeUninit::uninit();
            // Convert the ns_name to C-style `const char*` (NUL-terminated)
            let ns_name = CString::new(ns_name).unwrap();
            // unsafe: these functions are from libsystemd, which involving FFI calls
            unsafe {
                use libsystemd_sys::bus::{sd_bus_flush_close_unref, sd_bus_open_system_machine};
                // Try opening a connection to the container
                if sd_bus_open_system_machine(buf.as_mut_ptr(), ns_name.as_ptr()) >= 0 {
                    // If successful, just close the connection and drop the pointer
                    sd_bus_flush_close_unref(buf.assume_init());
                    return Ok(());
                }
            }
        }
        std::thread::sleep(Duration::from_secs_f32(((i + 1) as f32).ln().ceil()));
    }
    Err(Error::BootTimeout)
}

fn wait_for_poweroff(proxy: &ManagerProxyBlocking, name: &str) -> Result<()> {
    for i in 0..10 {
        if proxy.get_machine(name).is_err() {
            return Ok(());
        }
        std::thread::sleep(Duration::from_secs_f32(((i + 1) as f32).ln().ceil()));
    }
    Err(Error::PoweroffTimeout)
}

/// The state of a machine.
#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub enum MachineState {
    /// The machine is down.
    Down,
    /// The machine is starting.
    Starting,
    /// The machine is booted.
    Running,
}

impl MachineState {
    pub fn is_down(&self) -> bool {
        matches!(self, Self::Down)
    }

    pub fn is_starting(&self) -> bool {
        matches!(self, Self::Starting)
    }

    pub fn is_running(&self) -> bool {
        matches!(self, Self::Running)
    }
}

pub struct ExecResult {
    pub status: ExitStatus,
    pub stdout: String,
    pub stderr: String,
}

#[cfg(test)]
mod test {
    use crate::test::{TestDir, is_root};
    use test_log::test;

    #[test(ignore)]
    fn test_container_boot() {
        let testdir = TestDir::from("simple-workspace");
        let ws = testdir.workspace().unwrap();
        dbg!(&ws);
        assert!(ws.is_system_loaded());
        let inst = ws.instance("test").unwrap();
        dbg!(&inst);
        let container = inst.open().unwrap();
        dbg!(&container);
        assert!(container.state().unwrap().is_down());
        if is_root() {
            container.boot().unwrap();
            assert!(container.state().unwrap().is_running());
            container.stop(true).unwrap();
        }
    }
}
