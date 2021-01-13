//! This module contains configuration files related APIs

use crate::common::CURRENT_CIEL_VERSION;
use anyhow::{anyhow, Result};
use dialoguer::{Confirm, Editor, Input};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::{
    fs,
    io::{Read, Write},
};

const DEFAULT_CONFIG_LOCATION: &str = ".ciel/data/config.toml";
const DEFAULT_APT_SOURCE: &str = "deb https://repo.aosc.io/debs/ stable main";
const DEFAULT_AB3_CONFIG_LOCATION: &str = "etc/autobuild/ab3cfg.sh";
const DEFAULT_APT_LIST_LOCATION: &str = "etc/apt/sources.list";
const DEFAULT_RESOLV_LOCATION: &str = "etc/systemd/resolved.conf";
const DEFAULT_ACBS_CONFIG: &str = "etc/acbs/forest.conf";

#[derive(Debug, Serialize, Deserialize)]
pub struct CielConfig {
    version: usize,
    maintainer: String,
    dnssec: bool,
    apt_sources: String,
    pub local_repo: bool,
    pub local_sources: bool,
    #[serde(rename = "nspawn-extra-options")]
    pub extra_options: Vec<String>,
    #[serde(rename = "branch-exclusive-output")]
    pub sep_mount: bool,
}

impl CielConfig {
    pub fn save_config(&self) -> Result<String> {
        Ok(toml::to_string(self)?)
    }

    pub fn load_config(data: &[u8]) -> Result<CielConfig> {
        Ok(toml::from_slice(data)?)
    }
}

impl Default for CielConfig {
    fn default() -> Self {
        CielConfig {
            version: CURRENT_CIEL_VERSION,
            maintainer: "Bot <null@aosc.io>".to_string(),
            dnssec: false,
            apt_sources: String::new(),
            local_repo: true,
            local_sources: false,
            extra_options: Vec::new(),
            sep_mount: false,
        }
    }
}

fn validate_maintainer(maintainer: &String) -> Result<(), String> {
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
fn create_parent_dir(path: &Path) -> Result<()> {
    let path = path.parent().ok_or(anyhow!("Parent directory is root."))?;
    fs::create_dir_all(path)?;

    Ok(())
}

pub fn ask_for_config(config: Option<CielConfig>) -> Result<CielConfig> {
    let mut config = config.unwrap_or_default();
    config.maintainer = Input::<String>::new()
        .with_prompt("Maintainer Information")
        .default(config.maintainer)
        .validate_with(validate_maintainer)
        .interact()?;
    config.dnssec = Confirm::new()
        .with_prompt("Enable DNSSEC")
        .default(config.dnssec)
        .interact()?;
    let edit_source = Confirm::new()
        .with_prompt("Edit sources.list")
        .default(false)
        .interact()?;
    if edit_source {
        config.apt_sources = Editor::new()
            .edit(&config.apt_sources)?
            .unwrap_or(DEFAULT_APT_SOURCE.to_owned());
    }
    config.local_sources = Confirm::new()
        .with_prompt("Enable local sources caching")
        .default(config.local_sources)
        .interact()?;
    config.local_repo = Confirm::new()
        .with_prompt("Enable local packages repository")
        .default(config.local_repo)
        .interact()?;
    config.sep_mount = Confirm::new()
        .with_prompt("Use different OUTPUT dir for different branches")
        .default(config.sep_mount)
        .interact()?;

    Ok(config)
}

pub fn read_config() -> Result<CielConfig> {
    let mut f = std::fs::File::open(DEFAULT_CONFIG_LOCATION)?;
    let mut data: Vec<u8> = Vec::new();
    f.read_to_end(&mut data)?;

    Ok(CielConfig::load_config(data.as_slice())?)
}

pub fn apply_config<P: AsRef<Path>>(root: P, config: &CielConfig) -> Result<()> {
    // write maintainer information
    let rootfs = root.as_ref();
    let mut config_path = rootfs.to_owned();
    config_path.push(DEFAULT_AB3_CONFIG_LOCATION);
    create_parent_dir(&config_path)?;
    let mut f = std::fs::File::create(config_path)?;
    f.write_all(
        format!(
            "#!/bin/bash\nABMPM=dpkg\nABAPMS=\nABINSTALL=dpkg\nMTER=\"{}\"",
            config.maintainer
        )
        .as_bytes(),
    )?;
    // write sources.list
    if !config.apt_sources.is_empty() {
        let mut apt_list_path = rootfs.to_owned();
        apt_list_path.push(DEFAULT_APT_LIST_LOCATION);
        create_parent_dir(&apt_list_path)?;
        let mut f = std::fs::File::create(apt_list_path)?;
        f.write_all(config.apt_sources.as_bytes())?;
    }
    // write DNSSEC configuration
    if !config.dnssec {
        let mut resolv_path = rootfs.to_owned();
        resolv_path.push(DEFAULT_RESOLV_LOCATION);
        create_parent_dir(&resolv_path)?;
        let mut f = std::fs::File::create(resolv_path)?;
        f.write_all(b"[Resolve]\nDNSSEC=no\n")?;
    }
    // write acbs configuration
    let mut acbs_path = rootfs.to_owned();
    acbs_path.push(DEFAULT_ACBS_CONFIG);
    create_parent_dir(&acbs_path)?;
    let mut f = std::fs::File::create(acbs_path)?;
    f.write_all(b"[default]\nlocation = /tree/\n")?;

    Ok(())
}

#[test]
fn test_validate_maintainer() {
    assert_eq!(
        validate_maintainer(&"test <aosc@aosc.io>".to_owned()),
        Ok(())
    );
    assert_eq!(
        validate_maintainer(&"test <aosc@aosc.io;".to_owned()),
        Err("Invalid format.".to_owned())
    );
}
