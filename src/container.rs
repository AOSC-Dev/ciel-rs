use std::{
    fmt::Debug,
    fs::{self, File},
    mem::forget,
    ops::Deref,
    os::unix::ffi::OsStrExt,
    path::{self, Path, PathBuf},
    sync::{Arc, OnceLock},
};

use log::info;
use serde::{Deserialize, Serialize};

use crate::{
    fs::{tmpfs::TmpfsLayer, BoxedLayer, OverlayFS, OverlayManager, SimpleLayer},
    instance::{InstanceConfig, TmpfsConfig},
    machine::{Machine, MachineState},
    workspace::WorkspaceConfig,
    Error, Instance, Result, Workspace,
};

/// A Ciel container.
///
/// Each container uses a layered filesystem, with the same base system of
/// workspace as the lower layer, with a dedicated upper-layer.
/// Thus, instances can be reset into the clean state of base system quickly
/// by resetting the upper-layer. ([Container::rollback])
///
/// To make changes in containers presistent, changes made in the upper
/// layer can be committed into the base system. ([Workspace::commit])
///
/// Containers may be in one of the following [ContainerState]:
/// - Down (not started and filesystem un-mounted)
/// - Mounted (layered filesystem mounted but not started yet)
/// - Running (filesystem mounted and container started)
///
/// When a container is mounted, a snapshot its configuration ([ContainerConfig]),
/// including the workspace configruation and instance configuration,
/// will be written into the mounted filesystem, and loaded in the future.
/// This avoids misbehaviour of Ciel due to configurations' being modified
/// after mounting the container.
///
/// When [InstanceConfig::tmpfs] is [None], container upper-layer is directly
/// backed by the filesystem hosting the workspace, or else is by tmpfs.
///
/// Containers will be in [ContainerState::Down] state after the host machine rebooted.
#[derive(Clone)]
pub struct Container {
    instance: Instance,
    config: Arc<ContainerConfig>,
    #[allow(unused)]
    lock: Arc<FileLock>,
    ns_name: String,

    rootfs_path: PathBuf,
    config_path: PathBuf,
    upper_layer: BoxedLayer,
    lower_layers: Arc<Vec<BoxedLayer>>,
    overlay_mgr: Arc<OnceLock<Box<dyn OverlayManager>>>,
    machine: Arc<OnceLock<Machine>>,
}

impl PartialEq for Container {
    fn eq(&self, other: &Self) -> bool {
        self.instance == other.instance
    }
}

impl AsRef<Container> for Container {
    #[inline(always)]
    fn as_ref(&self) -> &Self {
        self
    }
}

struct FileLock(File);

impl FileLock {
    /// Unlocks the locked file forcibly.
    pub fn force_unlock(&self) {
        fs3::FileExt::unlock(&self.0).unwrap();
    }
}

impl Drop for FileLock {
    fn drop(&mut self) {
        fs3::FileExt::unlock(&self.0).unwrap();
    }
}

impl Container {
    /// Opens the build container, locking it exclusively.
    pub fn open(instance: Instance) -> Result<Self> {
        let lock = File::options()
            .read(true)
            .write(true)
            .create(true)
            .open(instance.directory().join(".lock"))?;
        fs3::FileExt::lock_exclusive(&lock)?;
        let lock = FileLock(lock);

        let ns_name = make_container_ns_name(instance.name())?;
        let rootfs_path = instance.workspace().directory().join(instance.name());

        let config_snapshot = rootfs_path.join(".ciel.toml");
        let config = if config_snapshot.exists() {
            ContainerConfig::load(config_snapshot)?
        } else {
            ContainerConfig {
                instance_name: instance.name().to_owned(),
                ns_name: ns_name.to_owned(),
                workspace_config: instance.workspace().config(),
                instance_config: instance.config(),
            }
        };

        let upper_dir = instance.directory().join("layers/upper");
        let upper_layer: BoxedLayer = if let Some(tmpfs) = &config.instance_config.tmpfs {
            Arc::new(Box::new(TmpfsLayer::new(&upper_dir, tmpfs)))
        } else {
            Arc::new(Box::new(SimpleLayer::new(&upper_dir)))
        };
        let config_path = instance.directory().join("layers/local");
        let lower_layers: Vec<BoxedLayer> = vec![
            Arc::new(Box::new(TmpfsLayer::new(
                &config_path,
                &TmpfsConfig { size: Some(16) },
            ))),
            Arc::new(Box::new(SimpleLayer::from(
                instance.workspace().system_rootfs(),
            ))),
        ];

        Ok(Self {
            instance,
            config: Arc::new(config),
            lock: Arc::new(lock),
            ns_name,
            rootfs_path,
            config_path,
            upper_layer,
            lower_layers: Arc::new(lower_layers),
            overlay_mgr: Arc::default(),
            machine: Arc::default(),
        })
    }

    /// Returns the [Instance] object.
    pub fn instance(&self) -> &Instance {
        &self.instance
    }

    /// Returns the [Workspace] object.
    pub fn workspace(&self) -> &Workspace {
        &self.instance.workspace()
    }

    /// Returns the instance directory.
    pub fn directory(&self) -> &Path {
        self.instance.directory()
    }

    /// Returns the container configuration snapshot.
    pub fn config(&self) -> &ContainerConfig {
        &self.config
    }

    /// Returns the NS name of the container.
    pub fn as_ns_name(&self) -> &str {
        &self.ns_name
    }

    /// Returns the path to the root filesystem of the container.
    pub fn rootfs_path(&self) -> &Path {
        &self.rootfs_path
    }

    /// Returns the path to the configuration layer of the container.
    pub fn config_path(&self) -> &Path {
        &self.config_path
    }

    /// Returns the upper layer of filesystem.
    ///
    /// The upper layer is for layer managers to place ephemeral contents.
    ///
    /// Note that the upper-layer structure is not guaranteed.
    /// Thus you should avoid writing files into upper layer directly.
    /// Instead, write into [Container::rootfs_path].
    pub fn upper_layer(&self) -> BoxedLayer {
        self.upper_layer.to_owned()
    }

    /// Returns the lower layers of filesystem.
    pub fn lower_layers(&self) -> impl Iterator<Item = BoxedLayer> + use<'_> {
        self.lower_layers.iter().cloned()
    }

    /// Returns the [OverlayManager] object.
    pub fn overlay_manager(&self) -> &Box<dyn OverlayManager> {
        &self.overlay_mgr.get_or_init(|| {
            Box::new(if self.instance.directory().join("diff").exists() {
                OverlayFS::new_compat(
                    self.rootfs_path.to_owned(),
                    self.instance.directory().join("layers"),
                    self.lower_layers.to_vec(),
                    self.config.workspace_config.volatile_mount,
                )
            } else {
                OverlayFS::new(
                    self.rootfs_path.as_path(),
                    self.upper_layer.to_owned(),
                    self.lower_layers.to_vec(),
                    self.config.workspace_config.volatile_mount,
                )
            })
        })
    }

    /// Returns the [Machine] object.
    pub fn machine(&self) -> Result<&Machine> {
        // FIXME: use get_or_try_init after stablization
        if let Some(machine) = self.machine.get() {
            Ok(machine)
        } else {
            let machine = Machine::new(self.config.to_owned(), self.rootfs_path.to_owned())?;
            _ = self.machine.set(machine);
            Ok(self.machine.get().unwrap())
        }
    }

    /// Returns the state of container
    pub fn state(&self) -> Result<ContainerState> {
        if self.overlay_manager().is_mounted()? {
            Ok(match self.machine()?.state()? {
                MachineState::Down => ContainerState::Mounted,
                MachineState::Starting => ContainerState::Starting,
                MachineState::Running => ContainerState::Running,
            })
        } else {
            Ok(ContainerState::Down)
        }
    }

    /// Boots this container.
    pub fn boot(&self) -> Result<()> {
        let state = self.state()?;

        if !state.is_mounted() {
            self.overlay_manager().mount()?;
            setup_container(&self)?;
        }

        if !matches!(state, ContainerState::Starting | ContainerState::Running) {
            self.machine()?.boot()?;
            setup_machine(&self)?;
        }

        Ok(())
    }

    /// Stops this container.
    pub fn stop(&self, unmount: bool) -> Result<()> {
        let state = self.state()?;

        if matches!(state, ContainerState::Starting | ContainerState::Running) {
            self.machine()?.stop()?;
        }

        if unmount {
            self.overlay_manager().unmount()?;
        }

        Ok(())
    }

    /// Rollbacks the container.
    ///
    /// The container will be in Down state after rollback.
    pub fn rollback(&self) -> Result<()> {
        self.stop(true)?;
        self.overlay_manager().rollback()?;
        nix::unistd::sync();
        Ok(())
    }
}

impl TryFrom<&Instance> for Container {
    type Error = crate::Error;

    fn try_from(value: &Instance) -> std::result::Result<Self, Self::Error> {
        value.open()
    }
}

impl Debug for Container {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        Debug::fmt(&self.instance, f)
    }
}

/// Generates the NS name for a container.
///
/// In version 3 workspaces, container names are in the following format:
/// `$name-adler32($absolute path)`
pub fn make_container_ns_name<P: AsRef<Path>>(path: P) -> Result<String> {
    let path = path.as_ref();
    let hash = adler32::adler32(path::absolute(path)?.as_os_str().as_bytes())?;
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| Error::InvalidInstancePath(path.to_owned()))?;
    Ok(format!("{}-{:x}", name, hash))
}

/// A container configuration.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct ContainerConfig {
    pub instance_name: String,
    pub ns_name: String,
    pub workspace_config: WorkspaceConfig,
    pub instance_config: InstanceConfig,
}

impl ContainerConfig {
    /// The default path for container configuration.
    pub const PATH: &str = "config.toml";

    /// The current version of container configuration format.
    pub const CURRENT_VERSION: usize = 3;

    /// Loads a container configuration from a given file path.
    pub fn load<P: AsRef<Path>>(path: P) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        if path.exists() {
            fs::read_to_string(&path)?.as_str().try_into()
        } else {
            Err(Error::ConfigNotFound(path))
        }
    }

    /// Deserializes a container configuration TOML.
    pub fn parse(config: &str) -> Result<Self> {
        let config = toml::from_str::<Self>(config)?;
        Ok(config)
    }

    /// Serializes a container configuration into TOML.
    pub fn serialize(&self) -> Result<String> {
        Ok(toml::to_string_pretty(&self)?)
    }
}

impl TryFrom<&str> for ContainerConfig {
    type Error = crate::Error;

    fn try_from(value: &str) -> std::result::Result<Self, Self::Error> {
        Self::parse(value)
    }
}

impl TryFrom<&ContainerConfig> for String {
    type Error = crate::Error;

    fn try_from(value: &ContainerConfig) -> std::result::Result<Self, Self::Error> {
        value.serialize()
    }
}

impl ContainerConfig {
    /// Returns all APT repositories that should be available in containers.
    ///
    /// This includes the stable repository (`deb https://repo.aosc.io/debs/ stable main`)
    /// and repositories from [WorkspaceConfig::extra_apt_repos] and [InstanceConfig::extra_apt_repos].
    /// If local repository is set to be included ([WorkspaceConfig::use_local_repo]
    /// and [InstanceConfig::use_local_repo]),
    /// `deb [trusted=yes] file:///debs/ /` will also be included.
    pub fn all_apt_repos(&self) -> Vec<String> {
        let mut repos = vec!["deb https://repo.aosc.io/debs/ stable main".to_string()];
        repos.extend(self.workspace_config.extra_apt_repos.iter().cloned());
        repos.extend(self.instance_config.extra_apt_repos.iter().cloned());
        if self.workspace_config.use_local_repo && self.instance_config.use_local_repo {
            repos.push("deb [trusted=yes] file:///debs/ /".to_string());
        }
        repos
    }
}

/// The state of a container.
#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub enum ContainerState {
    /// The container is down, with its filesystem un-mounted.
    Down,
    /// The container is mounted, but not started.
    Mounted,
    /// The container is starting.
    Starting,
    /// The container is booted.
    Running,
}

impl ContainerState {
    pub fn is_down(&self) -> bool {
        matches!(self, Self::Down)
    }

    pub fn is_dirty(&self) -> bool {
        !matches!(self, Self::Down)
    }

    pub fn is_mounted(&self) -> bool {
        matches!(self, Self::Mounted)
    }

    pub fn is_starting(&self) -> bool {
        matches!(self, Self::Starting)
    }

    pub fn is_running(&self) -> bool {
        matches!(self, Self::Running)
    }
}

impl From<MachineState> for ContainerState {
    fn from(value: MachineState) -> Self {
        match value {
            MachineState::Down => Self::Down,
            MachineState::Starting => Self::Starting,
            MachineState::Running => Self::Running,
        }
    }
}

fn setup_container(container: &Container) -> Result<()> {
    let config_layer = &container.config_path();
    let workspace_config = &container.config.workspace_config;
    // let instance_config = &container.config.instance_config;

    fn create_parent_dirs<P: AsRef<Path>>(path: P) -> Result<()> {
        if let Some(parent) = path.as_ref().parent() {
            fs::create_dir_all(parent)?;
        }
        Ok(())
    }

    info!(
        "{}: configuring container (post-mount) ...",
        container.ns_name
    );

    // ciel config
    fs::write(
        config_layer.join(".ciel.toml"),
        container.config.serialize()?,
    )?;

    // autobuild4 configuration
    let config_path = config_layer.join("etc/autobuild/ab4cfg.sh");
    create_parent_dirs(&config_path)?;
    fs::write(
        config_path,
        format!(
            "#!/bin/bash
ABMPM=dpkg
ABAPMS=
ABINSTALL=dpkg
MTER=\"{}\"",
            workspace_config.maintainer
        ),
    )?;

    // APT sources
    let apt_sources = container.config().all_apt_repos().join("\n");
    let apt_list_path = config_layer.join("etc/apt/sources.list");
    create_parent_dirs(&apt_list_path)?;
    fs::write(apt_list_path, apt_sources)?;

    // DNSSEC configuration
    if !workspace_config.dnssec {
        let resolv_path = config_layer.join("etc/systemd/resolved.conf");
        create_parent_dirs(&resolv_path)?;
        fs::write(resolv_path, "[Resolve]\nDNSSEC=no\n")?;
    }

    // acbs configuration
    let acbs_path = config_layer.join("etc/acbs/forest.conf");
    create_parent_dirs(&acbs_path)?;
    fs::write(acbs_path, "[default]\nlocation = /tree/\n")?;

    // git config
    let gitconfig_path = config_layer.join("root/.gitconfig");
    create_parent_dirs(&gitconfig_path)?;
    fs::write(gitconfig_path, "[safe]\n\tdirectory = /tree\n")?;

    Ok(())
}

fn setup_machine(container: &Container) -> Result<()> {
    let workspace_config = &container.config.workspace_config;
    let instance_config = &container.config.instance_config;
    let machine = container.machine()?;
    let workspace_dir = container.workspace().directory();

    info!(
        "{}: configuring container (post-boot) ...",
        container.ns_name
    );

    machine.bind(workspace_dir.join("TREE"), "/tree".into(), instance_config.readonly_tree)?;
    if !workspace_config.no_cache_packages {
        machine.bind(
            workspace_dir.join("CACHE"),
            "/var/cache/apt/archives".into(),
            false,
        )?;
    }
    if workspace_config.cache_sources {
        machine.bind(
            workspace_dir.join("SRCS"),
            "/var/cache/acbs/tarballs".into(),
            false,
        )?;
    }
    machine.bind(
        container.workspace().output_directory(),
        "/debs".into(),
        false,
    )?;

    Ok(())
}

/// A owned container which will be destroyed automatically on drop.
#[derive(Debug)]
pub struct OwnedContainer(Container);

impl OwnedContainer {
    /// Leaks the owned container.
    ///
    /// This avoids the container being destroyed on drop.
    pub fn leak(self) -> Container {
        let container = self.0.clone();
        forget(self);
        container
    }

    /// Destroies the owned container.
    pub fn discard(self) -> Result<()> {
        let instance = self.0.instance().to_owned();
        self.0.lock.force_unlock();
        forget(self);
        instance.destroy()
    }
}

impl From<Container> for OwnedContainer {
    fn from(value: Container) -> Self {
        Self(value)
    }
}

impl AsRef<Container> for OwnedContainer {
    fn as_ref(&self) -> &Container {
        &self.0
    }
}

impl Deref for OwnedContainer {
    type Target = Container;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Drop for OwnedContainer {
    fn drop(&mut self) {
        let instance = self.0.instance().to_owned();
        self.0.lock.force_unlock();
        instance.destroy().unwrap();
    }
}

#[cfg(test)]
mod test {
    use crate::{
        container::{make_container_ns_name, OwnedContainer},
        test::TestDir,
        Error,
    };
    use test_log::test;

    #[test]
    fn test_make_container_ns_name() {
        assert_eq!(
            make_container_ns_name("/home/xtex/src/aosc/ciel/a").unwrap(),
            "a-80d90979"
        );
        assert_eq!(
            make_container_ns_name("/home/xtex/src/aosc/ciel/test").unwrap(),
            "test-a0190ad8"
        );
        assert_eq!(
            make_container_ns_name("/buildroots/buildit/test").unwrap(),
            "test-75210982"
        );
        assert_eq!(
            make_container_ns_name("/buildroots/mingcongbai/amd64/amd64").unwrap(),
            "amd64-f1ac0cba"
        );
    }

    #[test]
    fn test_container_migration() {
        let testdir = TestDir::from("testdata/old-workspace");
        let ws = testdir.workspace().unwrap();
        dbg!(&ws);
        assert!(ws.is_system_loaded());
        let inst = ws.instance("test").unwrap();
        dbg!(&inst);
        let container = inst.open().unwrap();
        dbg!(&container);
        assert!(container.state().unwrap().is_down());
    }

    #[test]
    fn test_container_state() {
        let testdir = TestDir::from("testdata/simple-workspace");
        let ws = testdir.workspace().unwrap();
        dbg!(&ws);
        assert!(ws.is_system_loaded());
        let inst = ws.instance("test").unwrap();
        dbg!(&inst);
        let container = inst.open().unwrap();
        dbg!(&container);
        assert!(container.state().unwrap().is_down());
    }

    #[test]
    fn test_owned_container() {
        let testdir = TestDir::from("testdata/simple-workspace");
        let ws = testdir.workspace().unwrap();
        dbg!(&ws);
        assert!(ws.is_system_loaded());
        let inst = ws.instance("test").unwrap();
        dbg!(&inst);
        let container = OwnedContainer::from(inst.open().unwrap());
        dbg!(&container);
        assert!(container.state().unwrap().is_down());
        drop(container);
        assert!(matches!(
            ws.instance("test"),
            Err(Error::InstanceNotFound(_))
        ))
    }

    #[test]
    fn test_owned_container_leak() {
        let testdir = TestDir::from("testdata/simple-workspace");
        let ws = testdir.workspace().unwrap();
        dbg!(&ws);
        assert!(ws.is_system_loaded());
        let inst = ws.instance("test").unwrap();
        dbg!(&inst);
        let container = OwnedContainer::from(inst.open().unwrap());
        dbg!(&container);
        assert!(container.state().unwrap().is_down());
        let container = container.leak();
        drop(container);
        _ = ws.instance("test").unwrap();
    }

    #[test]
    fn test_container_config_apt_repos() {
        let testdir = TestDir::from("testdata/simple-workspace");
        let ws = testdir.workspace().unwrap();
        dbg!(&ws);
        let inst = ws.instance("test").unwrap();
        let config = inst.open().unwrap().config().to_owned();
        assert_eq!(
            config.all_apt_repos(),
            vec![
                "deb https://repo.aosc.io/debs/ stable main".to_string(),
                "deb file:///test/ test test".to_string(),
                "deb file:///test test testinst".to_string(),
                "deb [trusted=yes] file:///debs/ /".to_string(),
            ]
        );
    }
}
