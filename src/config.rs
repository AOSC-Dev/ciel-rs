use crate::common::CURRENT_CIEL_VERSION;
use dialoguer::{Confirm, Input};
use failure::Error;
use serde_derive::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct CielConfig {
    version: usize,
    maintainer: String,
    dnssec: bool,
    apt_sources: String,
    local_repo: bool,
}

impl CielConfig {
    pub fn new(
        version: usize,
        maintainer: String,
        dnssec: bool,
        apt_sources: String,
        local_repo: bool,
    ) -> Self {
        CielConfig {
            version,
            maintainer,
            dnssec,
            apt_sources,
            local_repo,
        }
    }

    pub fn save_config(&self) -> Result<String, Error> {
        Ok(toml::to_string(self)?)
    }

    pub fn load_config(data: &[u8]) -> Result<CielConfig, Error> {
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
            local_repo: false,
        }
    }
}

fn validate_maintainer(maintainer: &str) -> Result<(), String> {
    let mut lt = false; // "<"
    let mut gt = false; // ">"
    let mut at = false; // "@"
    let mut name = false;
    let mut nbsp = false; // space
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

pub fn ask_for_config(config: Option<CielConfig>) -> Result<CielConfig, Error> {
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
    config.local_repo = Confirm::new()
        .with_prompt("Enable local packages repository")
        .default(config.local_repo)
        .interact()?;

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
