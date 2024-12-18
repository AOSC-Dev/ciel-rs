//! This module contains configuration files related APIs

use crate::common::CURRENT_CIEL_VERSION;
use crate::{get_host_arch_name, info, CIEL_INST_DIR};
use anyhow::{Context, Result};
use console::user_attended;
use dialoguer::{theme::ColorfulTheme, Confirm, Editor, Input};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock, RwLock};
use std::{ffi::OsString, path::Path};

const DEFAULT_CONFIG_LOCATION: &str = ".ciel/data/config.toml";
const DEFAULT_APT_SOURCE: &str = "deb https://repo.aosc.io/debs/ stable main\n";
const DEFAULT_AB4_CONFIG_LOCATION: &str = "etc/autobuild/ab4cfg.sh";
const DEFAULT_APT_LIST_LOCATION: &str = "etc/apt/sources.list";
const DEFAULT_RESOLV_LOCATION: &str = "etc/systemd/resolved.conf";
const DEFAULT_ACBS_CONFIG: &str = "etc/acbs/forest.conf";
const DEFAULT_GITCONFIG: &str = "root/.gitconfig";
const DEFAULT_CIEL_CONFIG_PATH: &str = ".ciel.toml";

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub struct WorkspaceConfig {
    version: usize,
    pub maintainer: String,
    pub dnssec: bool,
    #[serde(alias = "apt_sources")]
    pub apt_sources: String,
    #[serde(alias = "local_repo")]
    pub local_repo: bool,
    #[serde(alias = "local_sources")]
    pub local_sources: bool,
    #[serde(rename = "nspawn-extra-options")]
    pub nspawn_options: Vec<String>,
    #[serde(rename = "branch-exclusive-output")]
    pub sep_mount: bool,
    #[serde(rename = "volatile-mount", default)]
    pub volatile_mount: bool,
    #[serde(
        alias = "force_use_apt",
        default = "WorkspaceConfig::default_force_use_apt"
    )]
    pub force_use_apt: bool,
}

impl WorkspaceConfig {
    const fn default_force_use_apt() -> bool {
        cfg!(target_arch = "riscv64")
    }

    pub fn to_toml(&self) -> Result<String> {
        Ok(toml::to_string(self)?)
    }

    pub fn from_toml<S: AsRef<str>>(data: S) -> Result<Self> {
        Ok(toml::from_str(data.as_ref())?)
    }

    /// Reads the configuration file from the current workspace
    pub fn load() -> Result<Self> {
        Self::from_toml(fs::read_to_string(DEFAULT_CONFIG_LOCATION)?)
    }

    pub fn save(&self) -> Result<()> {
        fs::write(DEFAULT_CONFIG_LOCATION, self.to_toml()?)?;
        Ok(())
    }
}

impl Default for WorkspaceConfig {
    fn default() -> Self {
        WorkspaceConfig {
            version: CURRENT_CIEL_VERSION,
            maintainer: "Bot <null@aosc.io>".to_string(),
            dnssec: false,
            apt_sources: DEFAULT_APT_SOURCE.to_string(),
            local_repo: true,
            local_sources: true,
            nspawn_options: Vec::new(),
            sep_mount: true,
            volatile_mount: false,
            force_use_apt: Self::default_force_use_apt(),
        }
    }
}

pub fn validate_maintainer(maintainer: &str) -> Result<(), String> {
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
                    return Err("Please enter a name.".to_owned());
                }
                lt = true;
            }
            b'>' => {
                if !lt {
                    return Err("Invalid format.".to_owned());
                }
                gt = true;
            }
            b'@' => {
                if !lt || gt {
                    return Err("Invalid format.".to_owned());
                }
                at = true;
            }
            b' ' | b'\t' => {
                if !name {
                    return Err("Please enter a name.".to_owned());
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

    Err("Invalid format.".to_owned())
}

#[inline]
fn get_default_editor() -> OsString {
    if let Some(prog) = std::env::var_os("VISUAL") {
        return prog;
    }
    if let Some(prog) = std::env::var_os("EDITOR") {
        return prog;
    }
    if let Ok(editor) = which::which("editor") {
        return editor.as_os_str().to_os_string();
    }

    "nano".into()
}

/// Shows a series of prompts to let the user select the configurations
pub fn ask_for_config() -> Result<WorkspaceConfig> {
    let mut config = WorkspaceConfig::default();
    if !user_attended() {
        info!("Not controlled by an user. Default values are used.");
        return Ok(config);
    }
    let theme = ColorfulTheme::default();
    config.maintainer = Input::<String>::with_theme(&theme)
        .with_prompt("Maintainer")
        .default(config.maintainer)
        .validate_with(|s: &String| validate_maintainer(s.as_str()))
        .interact_text()?;
    let edit_source = Confirm::with_theme(&theme)
        .with_prompt("Edit sources.list")
        .default(false)
        .interact()?;
    if edit_source {
        config.apt_sources = Editor::new()
            .executable(get_default_editor())
            .extension(".list")
            .edit(if config.apt_sources.is_empty() {
                DEFAULT_APT_SOURCE
            } else {
                &config.apt_sources
            })?
            .unwrap_or_else(|| DEFAULT_APT_SOURCE.to_owned());
    }
    config.local_sources = Confirm::with_theme(&theme)
        .with_prompt("Enable local sources caching")
        .default(config.local_sources)
        .interact()?;
    config.local_repo = Confirm::with_theme(&theme)
        .with_prompt("Enable local packages repository")
        .default(config.local_repo)
        .interact()?;
    config.sep_mount = Confirm::with_theme(&theme)
        .with_prompt("Use different OUTPUT directories for different branches")
        .default(config.sep_mount)
        .interact()?;

    // FIXME: RISC-V build hosts is unreliable when using oma: random lock-ups
    // during `oma refresh'. Disabling oma to workaround potential lock-ups.
    if get_host_arch_name().map(|x| x != "riscv64").unwrap_or(true) {
        info!("Ciel now uses oma as the default package manager for base system updating tasks.");
        info!("You can choose whether to use oma instead of apt while configuring.");
        config.force_use_apt = Confirm::with_theme(&theme)
            .with_prompt("Use apt as package manager")
            .default(config.force_use_apt)
            .interact()?;
    }

    Ok(config)
}

#[test]
fn test_validate_maintainer() {
    assert_eq!(validate_maintainer("test <aosc@aosc.io>"), Ok(()));
    assert_eq!(
        validate_maintainer("test <aosc@aosc.io;"),
        Err("Invalid format.".to_owned())
    );
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub struct InstanceConfig {
    version: usize,
    #[serde(default)]
    pub extra_repos: Vec<String>,
    #[serde(default)]
    pub nspawn_options: Vec<String>,
    #[serde(default)]
    pub tmpfs: Option<TmpfsConfig>,
}

impl InstanceConfig {
    pub fn to_toml(&self) -> Result<String> {
        Ok(toml::to_string(self)?)
    }
    pub fn from_toml<S: AsRef<str>>(data: S) -> Result<Self> {
        Ok(toml::from_str(data.as_ref())?)
    }
}

impl InstanceConfig {
    pub const FILE_NAME: &str = "config.toml";

    pub fn path<S: AsRef<str>>(instance: S) -> PathBuf {
        PathBuf::from(CIEL_INST_DIR)
            .join(instance.as_ref())
            .join(Self::FILE_NAME)
    }

    pub fn load<S: AsRef<str>>(instance: S) -> Result<Self> {
        let path = Self::path(instance);
        if path.exists() {
            let content = fs::read_to_string(&path)
                .with_context(|| format!("load instance config from {}", path.display()))?;
            Self::from_toml(content)
        } else {
            Ok(Self::default())
        }
    }

    pub fn save<S: AsRef<str>>(&self, instance: S) -> Result<()> {
        let path = Self::path(instance);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, self.to_toml()?)?;
        Ok(())
    }

    pub fn load_mounted<S: AsRef<str>>(instance: S) -> Result<Self> {
        let path = Path::new(instance.as_ref()).join(DEFAULT_CIEL_CONFIG_PATH);
        if path.exists() {
            let content = fs::read_to_string(&path)
                .with_context(|| format!("load instance config from {}", path.display()))?;
            Self::from_toml(content)
        } else {
            Self::load(instance)
        }
    }
}

static INSTANCE_CONFIGS: OnceLock<Mutex<HashMap<String, Arc<RwLock<InstanceConfig>>>>> =
    OnceLock::new();

impl InstanceConfig {
    pub fn get<S: AsRef<str>>(instance: S) -> Result<Arc<RwLock<Self>>> {
        let mut configs = INSTANCE_CONFIGS
            .get_or_init(|| Mutex::new(HashMap::new()))
            .lock()
            .unwrap();
        if !configs.contains_key(instance.as_ref()) {
            configs.insert(
                instance.as_ref().to_string(),
                Arc::new(RwLock::new(Self::load(instance.as_ref())?)),
            );
        }
        Ok(configs[instance.as_ref()].clone())
    }
}

impl Default for InstanceConfig {
    fn default() -> Self {
        Self {
            version: CURRENT_CIEL_VERSION,
            extra_repos: Default::default(),
            nspawn_options: Default::default(),
            tmpfs: None,
        }
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
    pub const DEFAULT_SIZE: usize = 4096;

    pub fn size_bytes(&self) -> usize {
        self.size.unwrap_or(Self::DEFAULT_SIZE) * 1024 * 1024
    }
}

/// Applies the given configuration to a rootfs
pub fn apply_config<P: AsRef<Path>>(
    root: P,
    workspace: &WorkspaceConfig,
    instance: &InstanceConfig,
) -> Result<()> {
    let rootfs = root.as_ref();

    fn create_parent_dirs<P: AsRef<Path>>(path: P) -> Result<()> {
        if let Some(parent) = path.as_ref().parent() {
            fs::create_dir_all(parent)?;
        }
        Ok(())
    }

    // ciel config
    fs::write(rootfs.join(DEFAULT_CIEL_CONFIG_PATH), instance.to_toml()?)?;

    // maintainer
    let config_path = rootfs.join(DEFAULT_AB4_CONFIG_LOCATION);
    create_parent_dirs(&config_path)?;
    fs::write(
        config_path,
        format!(
            "#!/bin/bash
ABMPM=dpkg
ABAPMS=
ABINSTALL=dpkg
MTER=\"{}\"",
            workspace.maintainer
        ),
    )?;

    // sources.list
    let mut apt_sources = workspace.apt_sources.to_owned();
    if apt_sources.is_empty() {
        apt_sources.push_str(DEFAULT_APT_SOURCE);
    }
    for source in &instance.extra_repos {
        apt_sources.push_str(source);
        apt_sources.push('\n');
    }
    let apt_list_path = rootfs.join(DEFAULT_APT_LIST_LOCATION);
    create_parent_dirs(&apt_list_path)?;
    fs::write(apt_list_path, apt_sources)?;

    // write DNSSEC configuration
    if !workspace.dnssec {
        let resolv_path = rootfs.join(DEFAULT_RESOLV_LOCATION);
        create_parent_dirs(&resolv_path)?;
        fs::write(resolv_path, "[Resolve]\nDNSSEC=no\n")?;
    }

    // write acbs configuration
    let acbs_path = rootfs.join(DEFAULT_ACBS_CONFIG);
    create_parent_dirs(&acbs_path)?;
    fs::write(acbs_path, "[default]\nlocation = /tree/\n")?;

    // write git config
    let gitconfig_path = rootfs.join(DEFAULT_GITCONFIG);
    create_parent_dirs(&gitconfig_path)?;
    fs::write(gitconfig_path, "[safe]\n\tdirectory = /tree\n")?;

    Ok(())
}
