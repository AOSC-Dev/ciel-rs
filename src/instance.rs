use std::{
    fmt::Debug,
    fs,
    path::{Path, PathBuf},
    sync::{Arc, RwLock},
};

use log::info;
use serde::{Deserialize, Serialize};

use crate::{workspace::Workspace, Container, Error, Result};

/// A Ciel instance.
///
/// Each instance maps to a build container. To begin interaction with
/// the container, use [Instance::open], which returns a [Container] and
/// locks the container to avoid asynchronized operations.
#[derive(Clone)]
pub struct Instance {
    workspace: Workspace,
    name: Arc<String>,
    path: Arc<PathBuf>,
    config: Arc<RwLock<InstanceConfig>>,
}

impl Instance {
    pub(crate) fn new(workspace: Workspace, name: String) -> Result<Self> {
        let path = workspace
            .directory()
            .join(Workspace::INSTANCES_DIR)
            .join(&name);

        if !path.is_dir() {
            return Err(Error::InstanceNotFound(name));
        }

        // Instance-level config.toml is not created by Ciel <= 3.6.0.
        // So fallback to default configuration for these.
        let config_path = path.join(InstanceConfig::PATH);
        let config = if !config_path.exists() {
            fs::write(
                path.join(InstanceConfig::PATH),
                InstanceConfig::default().serialize()?,
            )?;
            InstanceConfig::default()
        } else {
            InstanceConfig::load(config_path)?
        };

        Ok(Self {
            workspace,
            name: name.into(),
            path: path.into(),
            config: Arc::new(config.into()),
        })
    }

    /// Returns the workspace including this instance.
    pub fn workspace(&self) -> &Workspace {
        &self.workspace
    }

    /// Returns the name of this instance.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns the instance directory.
    pub fn directory(&self) -> &Path {
        &self.path
    }

    /// Gets the instance configuration.
    pub fn config(&self) -> InstanceConfig {
        self.config.read().unwrap().to_owned()
    }

    /// Modifies the instance configuration.
    pub fn set_config(&self, config: InstanceConfig) -> Result<()> {
        fs::write(
            self.directory().join(InstanceConfig::PATH),
            config.serialize()?,
        )?;
        *self.config.write()? = config;
        Ok(())
    }

    /// Opens the build container for further operations.
    ///
    /// This is equivalent to calling [Container::open].
    pub fn open(&self) -> Result<Container> {
        Container::open(self.to_owned())
    }

    /// Destories the container, removing all related files.
    pub fn destroy(self) -> Result<()> {
        let container = self.open()?;
        // some layers, such as tmpfs, requires rollback to fully un-mount
        container.rollback()?;
        info!("{}: destroying", self.name);
        fs::remove_dir_all(self.directory())?;
        Ok(())
    }
}

impl Debug for Instance {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_fmt(format_args!(
            "CIEL instance `{}` @ {:?}",
            self.name(),
            self.workspace.directory(),
        ))
    }
}

impl From<Instance> for PathBuf {
    fn from(value: Instance) -> Self {
        value.directory().to_owned()
    }
}

impl PartialEq for Instance {
    fn eq(&self, other: &Self) -> bool {
        self.path == other.path
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub struct InstanceConfig {
    version: usize,
    /// Extra APT repositories
    #[serde(default, alias = "extra-repos")]
    pub extra_apt_repos: Vec<String>,
    /// Extra systemd-nspawn options
    #[serde(default, alias = "nspawn-options")]
    pub extra_nspawn_options: Vec<String>,
    /// Whether local repository (the output directory) should be enabled in the container.
    #[serde(default)]
    pub use_local_repo: bool,
    /// tmpfs settings.
    ///
    /// Set to `None` to disable tmpfs for filesystem.
    #[serde(default)]
    pub tmpfs: Option<TmpfsConfig>,
    /// Whether TREE should be mounted as read-only.
    #[serde(default)]
    pub readonly_tree: bool,
    /// Path to OUTPUT directory.
    #[serde(default)]
    pub output: Option<PathBuf>,
}

impl Default for InstanceConfig {
    fn default() -> Self {
        Self {
            version: Self::CURRENT_VERSION,
            extra_apt_repos: vec![],
            extra_nspawn_options: vec![],
            use_local_repo: true,
            tmpfs: None,
            readonly_tree: false,
            output: None,
        }
    }
}

impl InstanceConfig {
    /// The default path for instance configuration.
    pub const PATH: &str = "config.toml";

    /// The current version of instance configuration format.
    pub const CURRENT_VERSION: usize = 3;

    /// Loads a instance configuration from a given file path.
    pub fn load<P: AsRef<Path>>(path: P) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        if path.exists() {
            fs::read_to_string(&path)?.as_str().try_into()
        } else {
            Err(Error::ConfigNotFound(path))
        }
    }

    /// Deserializes a instance configuration TOML.
    pub fn parse(config: &str) -> Result<Self> {
        let config = toml::from_str::<Self>(config)?;
        Ok(config)
    }

    /// Serializes a instance configuration into TOML.
    pub fn serialize(&self) -> Result<String> {
        Ok(toml::to_string_pretty(&self)?)
    }
}

impl TryFrom<&str> for InstanceConfig {
    type Error = crate::Error;

    fn try_from(value: &str) -> std::result::Result<Self, Self::Error> {
        Self::parse(value)
    }
}

impl TryFrom<&InstanceConfig> for String {
    type Error = crate::Error;

    fn try_from(value: &InstanceConfig) -> std::result::Result<Self, Self::Error> {
        value.serialize()
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
#[derive(Default)]
pub struct TmpfsConfig {
    #[serde(default)]
    pub size: Option<usize>,
}

impl TmpfsConfig {
    /// Returns the size of tmpfs or the default value (4 GiB), in MiB.
    pub fn size_or_default(&self) -> usize {
        self.size.unwrap_or(4096)
    }

    /// Returns the size of tmpfs or the default value, in bytes
    pub fn size_bytes(&self) -> usize {
        self.size_or_default() * 1024 * 1024
    }
}

#[cfg(test)]
mod test {
    use crate::{test::TestDir, Error};
    use test_log::test;

    use super::InstanceConfig;

    #[test]
    fn test_instance_config() {
        let config = InstanceConfig::default();
        let serialized = config.serialize().unwrap();
        assert_eq!(
            serialized,
            r##"version = 3
extra-apt-repos = []
extra-nspawn-options = []
use-local-repo = true
readonly-tree = false
"##
        );
        assert_eq!(InstanceConfig::parse(&serialized).unwrap(), config);
    }

    #[test]
    fn test_instance() {
        let testdir = TestDir::from("testdata/simple-workspace");
        let workspace = testdir.workspace().unwrap();
        dbg!(&workspace);

        let instance = workspace.instance("test").unwrap();
        dbg!(&instance);
        assert_eq!(instance.workspace(), &workspace);
        assert_eq!(instance.name(), "test");
        assert_eq!(
            instance.directory(),
            testdir.path().join(".ciel/container/instances/test")
        );

        assert!(matches!(
            workspace.instance("a"),
            Err(Error::InstanceNotFound(_))
        ));
    }

    #[test]
    fn test_instance_migration() {
        let testdir = TestDir::from("testdata/old-workspace");
        let ws = testdir.workspace().unwrap();
        dbg!(&ws);
        assert!(ws.is_system_loaded());
        let inst = ws.instance("test").unwrap();
        dbg!(&inst);
    }

    #[test]
    fn test_instance_destroy() {
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
        instance.destroy().unwrap();
        assert!(matches!(
            workspace.instance("test"),
            Err(Error::InstanceNotFound(_))
        ));
    }
}
