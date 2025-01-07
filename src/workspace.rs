use std::{
    fmt::Debug,
    fs,
    path::{Path, PathBuf},
    sync::Arc,
    sync::RwLock,
};

use log::info;
use rand::Rng;
use serde::{Deserialize, Serialize};

use crate::{
    container::OwnedContainer, instance::Instance, Container, Error, InstanceConfig, Result,
};

/// A Ciel workspace.
///
/// A workspace is a directory containing the following things:
/// - A workspace configuration (`.ciel/data/config.toml`)
/// - A base system for all build containers (`.ciel/container/dist`)
/// - Some instances ([Instance])
/// - (optional) Some OUTPUT directories for output deb files.
/// - (optional) A CACHE directory for caching source tarballs.
/// - (optional) A TREE directory for the default abbs tree.
///
/// Workspaces may have their base system loaded or unloaded
/// (i.e. there is no base system)
///
/// ```rust,no_run
/// use ciel::Workspace;
///
/// let workspace = Workspace::current_dir().unwrap();
/// dbg!(workspace.instances().unwrap().is_empty());
/// ```
#[derive(Clone)]
pub struct Workspace {
    path: Arc<PathBuf>,
    config: Arc<RwLock<WorkspaceConfig>>,
}

impl Workspace {
    /// The current version of workspace format.
    pub const CURRENT_VERSION: usize = 3;

    pub(crate) const CIEL_DIR: &str = ".ciel";
    pub(crate) const DATA_DIR: &str = ".ciel/data";
    pub(crate) const VERSION_PATH: &str = ".ciel/version";
    pub(crate) const DIST_DIR: &str = ".ciel/container/dist";
    pub(crate) const INSTANCES_DIR: &str = ".ciel/container/instances";

    /// Begins an existing workspace at the given path.
    ///
    /// This does not initialize a new workspace if not.
    /// To start a fully new workspace, see [Self::init].
    ///
    /// If the workspace is a legacy workspace (version 2), a default
    /// workspace configuration will be saved and the workspace will be
    /// upgraded to the current version.
    pub fn new<P: AsRef<Path>>(path: P) -> Result<Self> {
        let path = path.as_ref();

        if !path.join(Self::CIEL_DIR).is_dir() {
            return Err(Error::BrokenWorkspace);
        }
        if !path.join(Self::VERSION_PATH).is_file() {
            return Err(Error::BrokenWorkspace);
        }

        let version = fs::read_to_string(path.join(".ciel/version"))?
            .trim()
            .parse::<usize>()
            .map_err(|_| Error::NotAWorkspace)?;
        match version {
            Self::CURRENT_VERSION => {}
            2 => {
                fs::create_dir_all(path.join(Self::DATA_DIR))?;
                fs::write(
                    path.join(WorkspaceConfig::PATH),
                    WorkspaceConfig::default().serialize()?,
                )?;
                fs::write(
                    path.join(Self::VERSION_PATH),
                    Self::CURRENT_VERSION.to_string(),
                )?;
            }
            _ => return Err(Error::UnsupportedWorkspaceVersion(version)),
        }

        for dir in [Self::DATA_DIR, Self::DIST_DIR, Self::INSTANCES_DIR] {
            if !path.join(dir).is_dir() {
                return Err(Error::BrokenWorkspace);
            }
        }
        for dir in [WorkspaceConfig::PATH] {
            if !path.join(dir).is_file() {
                return Err(Error::BrokenWorkspace);
            }
        }

        let config = WorkspaceConfig::load(path.join(WorkspaceConfig::PATH))?;

        Ok(Self {
            path: Arc::new(path.into()),
            config: Arc::new(config.into()),
        })
    }

    /// Begins an existing workspace at the current directory.
    ///
    /// This is equivalent to `Workspace::new(std::env::current_dir()?)`.
    pub fn current_dir() -> Result<Self> {
        Self::new(std::env::current_dir()?)
    }

    /// Initializes a fully new workspace at the given directory,
    /// with the given configuration.
    ///
    /// The newly initialized workspace has its base system unloaded.
    /// To load a base system, extract files into [Self::system_rootfs].
    pub fn init<P: AsRef<Path>>(path: P, config: WorkspaceConfig) -> Result<Self> {
        let path = path.as_ref();

        if path.join(".ciel").exists() {
            return Err(Error::WorkspaceAlreadyExists);
        }

        info!("Initializing new CIEL! workspace at {:?}", path);

        fs::create_dir_all(path.join(Self::CIEL_DIR))?;
        fs::create_dir_all(path.join(Self::DATA_DIR))?;
        fs::create_dir_all(path.join(Self::DIST_DIR))?;
        fs::create_dir_all(path.join(Self::INSTANCES_DIR))?;
        fs::write(
            path.join(Self::VERSION_PATH),
            Self::CURRENT_VERSION.to_string(),
        )?;
        fs::write(path.join(WorkspaceConfig::PATH), config.serialize()?)?;

        Ok(Self {
            path: Arc::new(path.into()),
            config: Arc::new(config.into()),
        })
    }

    /// Gets the directory, at which this workspace is placed, as [Path].
    pub fn directory(&self) -> &Path {
        &self.path
    }

    /// Gets the workspace configuration.
    pub fn config(&self) -> WorkspaceConfig {
        self.config.read().unwrap().to_owned()
    }

    /// Modifies the workspace configuration after validation.
    pub fn set_config(&self, config: WorkspaceConfig) -> Result<()> {
        config.validate()?;
        fs::write(
            self.directory().join(WorkspaceConfig::PATH),
            config.serialize()?,
        )?;
        *self.config.write()? = config;
        Ok(())
    }

    /// Lists all existing instances.
    pub fn instances(&self) -> Result<Vec<Instance>> {
        let mut instances = vec![];
        for entry in self.directory().join(Self::INSTANCES_DIR).read_dir()? {
            let entry = entry?;
            if !entry.file_type()?.is_dir() {
                continue;
            }
            if let Some(name) = entry.file_name().to_str() {
                instances.push(Instance::new(self.clone(), name.to_string())?);
            } else {
                return Err(Error::InvalidInstanceName(entry.file_name()));
            }
        }
        Ok(instances)
    }

    /// Gets an existing instance.
    pub fn instance<S: AsRef<str>>(&self, name: S) -> Result<Instance> {
        Instance::new(self.clone(), name.as_ref().to_string())
    }

    /// Creates a new instance.
    pub fn add_instance<S: AsRef<str>>(&self, name: S, config: InstanceConfig) -> Result<Instance> {
        let name = name.as_ref();

        let instance_dir = self.directory().join(Workspace::INSTANCES_DIR).join(name);
        fs::create_dir_all(&instance_dir)?;
        fs::write(instance_dir.join(InstanceConfig::PATH), config.serialize()?)?;
        info!("{}: instance created", name);

        self.instance(name)
    }

    /// Returns the rootfs path of the base system.
    pub fn system_rootfs(&self) -> PathBuf {
        self.directory().join(Self::DIST_DIR)
    }

    /// Returns if the base system has been loaded.
    pub fn is_system_loaded(&self) -> bool {
        self.system_rootfs()
            .read_dir()
            .map(|mut r| r.next().is_some())
            .unwrap_or_default()
    }

    /// Commits changes in a container into the base system.
    ///
    /// Caller must ensure that only the container to commit is opened.
    /// Other containers will be locked and rollbacked during the commit.
    pub fn commit<C: AsRef<Container>>(&self, container: C) -> Result<()> {
        let container = container.as_ref();
        container.stop(true)?;
        let mut locks = vec![];
        for inst in self.instances()? {
            if &inst != container.instance() {
                let inst = inst.open()?;
                inst.rollback()?;
                locks.push(inst);
            }
        }
        container.overlay_manager().commit()?;
        container.rollback()?;
        Ok(())
    }

    /// Destroies the workspace, removing all Ciel files, except for
    /// the abbs tree, caches and outputs.
    pub fn destroy(self) -> Result<()> {
        for inst in self.instances()? {
            let inst = inst.open()?;
            inst.stop(true)?;
            inst.overlay_manager().rollback()?;
        }
        fs::remove_dir_all(self.directory().join(".ciel"))?;
        Ok(())
    }

    /// Creates a ephemeral owned container with the given prefix.
    ///
    /// The name of ephemeral containers are formatted as: `$prefix-$rand`.
    ///
    /// These ephemeral containers are useful for one-time tasks, such as updating
    /// the base system.
    pub fn ephemeral_container(
        &self,
        prefix: &str,
        config: InstanceConfig,
    ) -> Result<OwnedContainer> {
        let name = format!("{}-{:08x}", prefix, rand::thread_rng().r#gen::<u32>());
        Ok(self.add_instance(name, config)?.open()?.into())
    }

    /// Returns the output directory of the workspace.
    /// 
    /// See [Container::output_directory].
    pub fn output_directory(&self) -> PathBuf {
        let name = if self.config().branch_exclusive_output {
            let head = if let Ok(repo) = git2::Repository::open(self.directory().join("TREE")) {
                repo.head()
                    .ok()
                    .and_then(|head| head.shorthand().map(|s| s.to_string()))
                    .unwrap_or_else(|| "HEAD".to_string())
            } else {
                "HEAD".to_string()
            };
            format!("OUTPUT-{}", head)
        } else {
            "OUTPUT".to_string()
        };
        self.directory().join(name).join("debs")
    }
}

impl Debug for Workspace {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_fmt(format_args!("CIEL workspace `{:?}`", self.directory()))
    }
}

impl TryFrom<&Path> for Workspace {
    type Error = crate::Error;

    fn try_from(value: &Path) -> std::result::Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<Workspace> for PathBuf {
    fn from(value: Workspace) -> Self {
        value.directory().to_owned()
    }
}

impl PartialEq for Workspace {
    fn eq(&self, other: &Self) -> bool {
        self.path == other.path
    }
}

/// A Ciel workspace configuration.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub struct WorkspaceConfig {
    version: usize,
    /// The maintainer information, for example, `Bot <null@aosc.io>`
    pub maintainer: String,
    /// Whether DNSSEC should be allowed in containers.
    #[serde(default)]
    pub dnssec: bool,

    // The old version of ciel-rs uses `apt_sources`, which is kept for compatibility.
    // This is converted into [extra_apt_repos] when loaded.
    #[serde(alias = "apt_sources", default)]
    apt_sources: Option<String>,
    /// Extra APT repositories to use.
    #[serde(default)]
    pub extra_apt_repos: Vec<String>,
    /// Whether local repository (the output directory) should be enabled in containers.
    #[serde(alias = "local_repo", default)]
    pub use_local_repo: bool,
    /// Whether output directories should be branch-exclusive .
    ///
    /// This means using `OUTPUT-(branch)` instead of `OUTPUT` for outputs.
    #[serde(default)]
    pub branch_exclusive_output: bool,

    /// Whether to cache APT packages.
    #[serde(default)]
    pub no_cache_packages: bool,
    /// Whether to cache sources.
    #[serde(alias = "local_sources", default)]
    pub cache_sources: bool,

    /// Extra options for systemd-nspawn
    #[serde(alias = "nspawn-extra-options", default)]
    pub extra_nspawn_options: Vec<String>,

    /// Whether to mount the container filesystem as volatile
    #[serde(default)]
    pub volatile_mount: bool,

    /// Whether to use APT instead of oma.
    ///
    /// This is enabled by default on RISC-V hosts, because oma may run into
    /// random lock-ups on RISC-V.
    #[serde(alias = "force_use_apt", default = "WorkspaceConfig::default_use_apt")]
    pub use_apt: bool,
}

impl WorkspaceConfig {
    const fn default_use_apt() -> bool {
        cfg!(target_arch = "riscv64")
    }
}

impl Default for WorkspaceConfig {
    fn default() -> Self {
        Self {
            version: Self::CURRENT_VERSION,
            maintainer: "Bot <null@aosc.io>".to_string(),
            dnssec: false,
            apt_sources: None,
            extra_apt_repos: vec![],
            use_local_repo: true,
            branch_exclusive_output: true,
            no_cache_packages: false,
            cache_sources: true,
            extra_nspawn_options: vec![],
            volatile_mount: false,
            use_apt: Self::default_use_apt(),
        }
    }
}

impl WorkspaceConfig {
    /// The default path for workspace configuration.
    pub const PATH: &str = ".ciel/data/config.toml";

    /// The current version of workspace configuration format.
    pub const CURRENT_VERSION: usize = 3;

    /// Loads a workspace configuration from a given file path.
    pub fn load<P: AsRef<Path>>(path: P) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        if path.exists() {
            fs::read_to_string(&path)?.as_str().try_into()
        } else {
            Err(Error::ConfigNotFound(path))
        }
    }

    /// Validate the configuration.
    ///
    /// This checks:
    /// - Invalid maintainer string
    pub fn validate(&self) -> Result<()> {
        Self::validate_maintainer(&self.maintainer)?;
        Ok(())
    }

    /// Validates a maintainer information string.
    ///
    /// This ensures the string has a valid maintainer name and email address.
    pub fn validate_maintainer(maintainer: &str) -> Result<()> {
        let mut lt = false; // "<"
        let mut gt = false; // ">"
        let mut at = false; // "@"
        let mut name = false;
        let mut nbsp = false; // space
                              // A simple FSM to match the states
        for c in maintainer.as_bytes() {
            match *c {
                b'<' => {
                    if !nbsp {
                        return Err(Error::MaintainerNameNeeded);
                    }
                    lt = true;
                }
                b'>' => {
                    if !lt {
                        return Err(Error::InvalidMaintainerInfo);
                    }
                    gt = true;
                }
                b'@' => {
                    if !lt || gt {
                        return Err(Error::InvalidMaintainerInfo);
                    }
                    at = true;
                }
                b' ' | b'\t' => {
                    if !name {
                        return Err(Error::MaintainerNameNeeded);
                    }
                    nbsp = true;
                }
                _ => {
                    if !nbsp {
                        name = true;
                        continue;
                    }
                }
            }
        }

        if name && gt && lt && at {
            return Ok(());
        }

        Err(Error::InvalidMaintainerInfo)
    }

    /// Deserializes a workspace configuration TOML.
    pub fn parse(config: &str) -> Result<Self> {
        let mut config = toml::from_str::<Self>(config)?;

        // Convert old `apt_sources` into `extra_apt_repos`
        if let Some(sources) = config.apt_sources.take() {
            config.extra_apt_repos.extend(
                sources
                    .lines()
                    .map(|line| line.trim())
                    .filter(|line| !line.is_empty())
                    .filter(|line| {
                        !line.eq_ignore_ascii_case("deb https://repo.aosc.io/debs/ stable main")
                    })
                    .map(|line| line.to_string()),
            );
        }

        Ok(config)
    }

    /// Serializes a workspace configuration into TOML.
    pub fn serialize(&self) -> Result<String> {
        Ok(toml::to_string_pretty(&self)?)
    }
}

impl TryFrom<&str> for WorkspaceConfig {
    type Error = crate::Error;

    fn try_from(value: &str) -> std::result::Result<Self, Self::Error> {
        Self::parse(value)
    }
}

impl TryFrom<&WorkspaceConfig> for String {
    type Error = crate::Error;

    fn try_from(value: &WorkspaceConfig) -> std::result::Result<Self, Self::Error> {
        value.serialize()
    }
}

#[cfg(test)]
mod test {
    use std::fs;
    use test_log::test;

    use crate::{
        test::{is_root, TestDir},
        ContainerState, Error, InstanceConfig,
    };

    use super::WorkspaceConfig;

    #[test]
    fn test_config() {
        let config = WorkspaceConfig::default();
        let serialized = config.serialize().unwrap();
        assert_eq!(
            serialized,
            r##"version = 3
maintainer = "Bot <null@aosc.io>"
dnssec = false
extra-apt-repos = []
use-local-repo = true
branch-exclusive-output = true
no-cache-packages = false
cache-sources = true
extra-nspawn-options = []
volatile-mount = false
use-apt = false
"##
        );
        assert_eq!(
            WorkspaceConfig::try_from(serialized.as_str()).unwrap(),
            config
        );
    }

    #[test]
    fn test_config_migration() {
        assert_eq!(
            WorkspaceConfig::parse(
                r##"
version = 3
maintainer = "AOSC OS Maintainers <maintainers@aosc.io>"
dnssec = false
apt_sources = "deb https://repo.aosc.io/debs/ stable main"
local_repo = true
local_sources = true
branch-exclusive-output = true
volatile-mount = false
nspawn-extra-options = ["-E", "NO_COLOR=1"]
"##,
            )
            .unwrap(),
            WorkspaceConfig {
                version: 3,
                maintainer: "AOSC OS Maintainers <maintainers@aosc.io>".to_string(),
                dnssec: false,
                apt_sources: None,
                extra_apt_repos: vec![],
                use_local_repo: true,
                branch_exclusive_output: true,
                cache_sources: true,
                extra_nspawn_options: vec!["-E".to_string(), "NO_COLOR=1".to_string()],
                volatile_mount: false,
                use_apt: false,
                ..Default::default()
            }
        );

        assert_eq!(
            WorkspaceConfig::parse(
                r##"
version = 3
maintainer = "AOSC OS Maintainers <maintainers@aosc.io>"
dnssec = false
apt_sources = "deb https://repo.aosc.io/debs/ stable main\ndeb file:///test/ test test"
local_repo = true
local_sources = true
nspawn-extra-options = []
branch-exclusive-output = true
volatile-mount = false
"##,
            )
            .unwrap(),
            WorkspaceConfig {
                version: 3,
                maintainer: "AOSC OS Maintainers <maintainers@aosc.io>".to_string(),
                dnssec: false,
                apt_sources: None,
                extra_apt_repos: vec!["deb file:///test/ test test".to_string()],
                use_local_repo: true,
                branch_exclusive_output: true,
                cache_sources: true,
                extra_nspawn_options: vec![],
                volatile_mount: false,
                use_apt: false,
                ..Default::default()
            }
        );
    }

    #[test]
    fn test_validate_maintainer() {
        assert!(matches!(
            WorkspaceConfig::validate_maintainer("test <aosc@aosc.io>"),
            Ok(())
        ));
        assert!(matches!(
            WorkspaceConfig::validate_maintainer("test <aosc@aosc.io;"),
            Err(Error::InvalidMaintainerInfo)
        ));
        assert!(matches!(
            WorkspaceConfig::validate_maintainer("<aosc@aosc.io>"),
            Err(Error::MaintainerNameNeeded)
        ));
        assert!(matches!(
            WorkspaceConfig::validate_maintainer(" <aosc@aosc.io>"),
            Err(Error::MaintainerNameNeeded)
        ));
    }

    #[test]
    fn test_workspace_init() {
        let testdir = TestDir::new();
        let ws = testdir.init_workspace(WorkspaceConfig::default()).unwrap();
        dbg!(&ws);
        assert!(!ws.is_system_loaded());
        assert!(ws.config().extra_apt_repos.is_empty());
        fs::write(ws.directory().join(".ciel/container/dist/init"), "").unwrap();
        let ws = testdir.workspace().unwrap();
        dbg!(&ws);
        assert!(ws.is_system_loaded());
        assert!(ws.instances().unwrap().is_empty());
    }

    #[test]
    fn test_workspace_migration_v3() {
        // migration from Ciel <= 3.6.0
        let testdir = TestDir::from("testdata/old-workspace");
        let ws = testdir.workspace().unwrap();
        dbg!(&ws);
        assert!(ws.is_system_loaded());
        assert_eq!(
            ws.config().extra_apt_repos,
            vec!["deb file:///test/ test test".to_string(),]
        );
        assert!(ws.config().branch_exclusive_output);
    }

    #[test]
    fn test_workspace_migration_v2() {
        // migration from Ciel 2.x.x
        let testdir = TestDir::from("testdata/v2-workspace");
        let ws = testdir.workspace().unwrap();
        dbg!(&ws);
        assert!(ws.is_system_loaded());
        assert!(ws.config().extra_apt_repos.is_empty());
        assert!(ws.config().branch_exclusive_output);
    }

    #[test]
    fn test_incompatible_workspace() {
        let testdir = TestDir::from("testdata/incompat-ws-version");
        assert!(matches!(
            testdir.workspace(),
            Err(Error::UnsupportedWorkspaceVersion(0))
        ));
    }

    #[test]
    fn test_broken_workspace() {
        let testdir = TestDir::from("testdata/broken-workspace");
        assert!(matches!(testdir.workspace(), Err(Error::BrokenWorkspace)));
    }

    #[test]
    fn test_workspace_instances() {
        let testdir = TestDir::from("testdata/simple-workspace");
        let workspace = testdir.workspace().unwrap();
        dbg!(&workspace);
        assert_eq!(
            workspace
                .instances()
                .unwrap()
                .iter()
                .map(|i| i.name().to_owned())
                .collect::<Vec<_>>(),
            vec!["test".to_string(), "tmpfs".to_string()]
        );
        let instance = workspace.instance("test").unwrap();
        dbg!(&instance);
        assert_eq!(instance.name(), "test");
    }

    #[test]
    fn test_workspace_add_instance() {
        let testdir = TestDir::from("testdata/simple-workspace");
        let workspace = testdir.workspace().unwrap();
        dbg!(&workspace);
        assert_eq!(
            workspace
                .instances()
                .unwrap()
                .iter()
                .map(|i| i.name().to_owned())
                .collect::<Vec<_>>(),
            vec!["test".to_string(), "tmpfs".to_string()]
        );
        let instance = workspace
            .add_instance("a", InstanceConfig::default())
            .unwrap();
        dbg!(&instance);
        assert_eq!(instance.name(), "a");
        assert_eq!(
            workspace
                .instances()
                .unwrap()
                .iter()
                .map(|i| i.name().to_owned())
                .collect::<Vec<_>>(),
            vec!["test".to_string(), "tmpfs".to_string(), "a".to_string()]
        );
        let instance = workspace.instance("a").unwrap();
        dbg!(&instance);
        let container = instance.open().unwrap();
        dbg!(&container);
        assert_eq!(container.state().unwrap(), ContainerState::Down);
    }

    #[test]
    fn test_workspace_commit() {
        let testdir = TestDir::from("testdata/simple-workspace");
        let workspace = testdir.workspace().unwrap();
        dbg!(&workspace);
        let instance = workspace.instance("test").unwrap();
        dbg!(&instance);
        let container = instance.open().unwrap();
        dbg!(&container);
        assert_eq!(container.state().unwrap(), ContainerState::Down);
        assert!(!testdir.path().join(".ciel/container/dist/a").exists());
        if !is_root() {
            return;
        }
        container.overlay_manager().mount().unwrap();
        assert!(container.overlay_manager().is_mounted().unwrap());
        fs::write(testdir.path().join("test/a"), "test").unwrap();
        workspace.commit(&container).unwrap();
        assert!(!container.overlay_manager().is_mounted().unwrap());
        assert_eq!(
            fs::read_to_string(testdir.path().join(".ciel/container/dist/a")).unwrap(),
            "test"
        );
    }

    #[test]
    fn test_workspace_commit_tmpfs() {
        let testdir = TestDir::from("testdata/simple-workspace");
        let workspace = testdir.workspace().unwrap();
        dbg!(&workspace);
        let instance = workspace.instance("tmpfs").unwrap();
        dbg!(&instance);
        let container = instance.open().unwrap();
        dbg!(&container);
        assert_eq!(container.state().unwrap(), ContainerState::Down);
        assert!(!testdir.path().join(".ciel/container/dist/a").exists());
        if !is_root() {
            return;
        }
        container.overlay_manager().mount().unwrap();
        assert!(container.overlay_manager().is_mounted().unwrap());
        fs::write(testdir.path().join("tmpfs/a"), "test").unwrap();
        workspace.commit(&container).unwrap();
        assert!(!container.overlay_manager().is_mounted().unwrap());
        assert_eq!(
            fs::read_to_string(testdir.path().join(".ciel/container/dist/a")).unwrap(),
            "test"
        );
    }

    #[test]
    fn test_workspace_destroy() {
        let testdir = TestDir::from("testdata/simple-workspace");
        let workspace = testdir.workspace().unwrap();
        dbg!(&workspace);
        workspace.destroy().unwrap();
        assert!(!testdir.path().join(".ciel").exists());
        assert!(testdir.path().join("TREE").exists());
    }

    #[test]
    fn test_workspace_ephemeral_container() {
        let testdir = TestDir::from("testdata/simple-workspace");
        let workspace = testdir.workspace().unwrap();
        dbg!(&workspace);
        let cont = workspace
            .ephemeral_container("test", InstanceConfig::default())
            .unwrap();
        dbg!(&cont);
        assert!(cont.as_ns_name().starts_with("test-"));
        assert_eq!(workspace.instances().unwrap().len(), 3);
        drop(cont);
        assert_eq!(workspace.instances().unwrap().len(), 2);
    }
}
