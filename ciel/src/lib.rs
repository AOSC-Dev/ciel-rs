//! Ciel (/sjɛl/) 3 is an integrated packaging environment for AOSC OS.
//!
//! Ciel uses `systemd-nspawn` as container backend and `overlay` file system
//! for layered filesystem.

pub mod build;
pub mod container;
mod dbus_machine1_machine;
mod dbus_machine1_manager;
pub mod fs;
pub mod instance;
pub mod machine;
pub mod repo;
pub mod workspace;

pub use container::{Container, ContainerConfig, ContainerState};
pub use instance::{Instance, InstanceConfig};
pub use machine::{Machine, MachineState};
pub use repo::SimpleAptRepository;
pub use workspace::{Workspace, WorkspaceConfig};

use std::ffi::OsString;

pub type Result<T> = std::result::Result<T, Error>;

/// An error produced by Ciel.
#[non_exhaustive]
#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("I/O error: {0}")]
    IoError(#[from] std::io::Error),
    #[error("Some Mutex/RwLock are poisoned")]
    PoisonError,
    #[error("Unable to parse mountinfo file: {0}")]
    MountInfoParseError(#[from] libmount::mountinfo::ParseError),
    #[error("Mount error: {0}")]
    MountError(String),
    #[error(transparent)]
    SyscallError(#[from] nix::Error),
    #[error(transparent)]
    FSTraverseError(#[from] walkdir::Error),
    #[error(transparent)]
    StripPrefixError(#[from] std::path::StripPrefixError),
    #[error("D-Bus error: {0}")]
    DBusError(#[from] zbus::Error),
    #[error("libgit2 error: {0}")]
    GitError(#[from] git2::Error),
    #[error("Time formatting error: {0}")]
    TimeFormatError(#[from] time::error::Format),

    #[error("Configuration file is not found at {0}")]
    ConfigNotFound(std::path::PathBuf),
    #[error("Invalid TOML: {0}")]
    InvalidToml(#[from] toml::de::Error),
    #[error("Unable to serialize into TOML: {0}")]
    TomlSerializerError(#[from] toml::ser::Error),
    #[error("Invalid maintainer information")]
    InvalidMaintainerInfo,
    #[error("Maintainer name is required")]
    MaintainerNameNeeded,

    #[error("Not a Ciel workspace (.ciel directory does not exist)")]
    NotAWorkspace,
    #[error("A Ciel workspace is already initialized")]
    WorkspaceAlreadyExists,
    #[error("Ciel workspace is broken")]
    BrokenWorkspace,
    #[error("Unsupported workspace version: got {0}")]
    UnsupportedWorkspaceVersion(usize),

    #[error("Invalid instance name: {0:?}")]
    InvalidInstanceName(OsString),
    #[error("Instance not found: {0}")]
    InstanceNotFound(String),
    #[error("Invalid instance path: {0}")]
    InvalidInstancePath(std::path::PathBuf),
    #[error("Improper state")]
    ImproperState,
    #[error("Subcommand error: {0}")]
    SubcommandError(std::process::ExitStatus),
    #[error("Timeout booting machine")]
    BootTimeout,
    #[error("Timeout poweroff machine")]
    PoweroffTimeout,

    #[error("Your kernel does not support overlayfs")]
    OverlayFSUnavailable,
    #[error("OverlayFS at {0} cannot be mounted due to incompat features")]
    OverlayFSIncompat(std::path::PathBuf),
    #[error("Ciel does not support overlayfs metacopy")]
    MetaCopyUnsupported,

    #[error("Unable to scan deb file '{0}': {1}")]
    DebScanError(std::path::PathBuf, repo::scan::ScanError),

    #[error("Invalid bincode: {0}")]
    InvalidBincode(#[from] bincode::Error),
    #[error("Nested package group exceeded 32 levels")]
    NestedPackageGroup,
}

impl<T> From<std::sync::PoisonError<T>> for Error {
    fn from(_: std::sync::PoisonError<T>) -> Self {
        Self::PoisonError
    }
}

impl From<libmount::Error> for Error {
    fn from(err: libmount::Error) -> Self {
        // discard details so that Error can be converted into anyhow::Error simply
        Self::MountError(format!("{:?}", err))
    }
}

#[cfg(test)]
pub(crate) mod test {
    use std::{fs, path::Path};

    use tempfile::TempDir;

    use crate::{
        repo::SimpleAptRepository,
        workspace::{Workspace, WorkspaceConfig},
        Result,
    };

    pub fn is_root() -> bool {
        nix::unistd::geteuid().is_root()
    }

    #[derive(Debug)]
    pub struct TestDir(TempDir);

    impl AsRef<Path> for TestDir {
        fn as_ref(&self) -> &Path {
            self.0.path()
        }
    }

    impl From<TempDir> for TestDir {
        fn from(value: TempDir) -> Self {
            Self(value)
        }
    }

    fn copy_file(from: &Path, to: &Path) {
        assert!(from.exists());
        if from.is_symlink() {
            std::os::unix::fs::symlink(fs::read_link(from).unwrap(), to).unwrap();
        } else if from.is_file() {
            fs::copy(from, to).unwrap();
        } else if from.is_dir() {
            fs::create_dir_all(to).unwrap();
            fs::set_permissions(to, from.metadata().unwrap().permissions()).unwrap();
            for entry in fs::read_dir(from).unwrap() {
                let entry = entry.unwrap();
                copy_file(&from.join(entry.file_name()), &to.join(entry.file_name()));
            }
        } else {
            panic!("unsupported file type");
        }
    }

    impl TestDir {
        pub fn new() -> Self {
            let dir = TempDir::with_prefix("ciel-").unwrap();
            println!("test data: {:?}", dir.path());
            dir.into()
        }

        pub fn from(template: &str) -> Self {
            let dir = Self::new();
            println!("copying test data: {} -> {:?}", template, dir.path());
            copy_file(&Path::new("testdata").join(template), dir.path());
            dir
        }

        pub fn path(&self) -> &Path {
            self.0.path()
        }

        pub fn workspace(&self) -> Result<Workspace> {
            Workspace::new(self.path())
        }

        pub fn init_workspace(&self, config: WorkspaceConfig) -> Result<Workspace> {
            Workspace::init(self.path(), config)
        }

        pub fn apt_repo(&self) -> SimpleAptRepository {
            SimpleAptRepository::new(self.path().join("debs"))
        }
    }
}
