use anyhow::Result;
use console::style;

use crate::machine;

mod container;
mod onboarding;
mod packaging;

// re-export all the functions from the sub
pub use self::container::*;
pub use self::onboarding::onboarding;
pub use self::packaging::*;

const DEFAULT_MOUNTS: &[(&str, &str)] = &[
    ("OUTPUT/debs/", "/debs/"),
    ("TREE", "/tree"),
    ("SRCS", "/var/cache/acbs/tarballs"),
];
const UPDATE_SCRIPT: &str = r#"export DEBIAN_FRONTEND=noninteractive;apt-get update -y --allow-releaseinfo-change && apt-get -y -o Dpkg::Options::="--force-confnew" full-upgrade --autoremove --purge && apt clean"#;

type MountOptions = (Vec<String>, Vec<(String, &'static str)>);
/// Ensure that the directories exist and mounted
pub fn ensure_host_sanity() -> Result<MountOptions, std::io::Error> {
    use crate::warn;

    let mut extra_options = Vec::new();
    let mut mounts: Vec<(String, &str)> = DEFAULT_MOUNTS
        .iter()
        .map(|x| (x.0.to_string(), x.1))
        .collect();
    if let Ok(c) = crate::config::read_config() {
        extra_options = c.extra_options;
        if !c.local_sources {
            // remove SRCS
            mounts.swap_remove(2);
        }
        if c.sep_mount {
            mounts.push((format!("{}/debs", get_output_directory(true)), "/debs/"));
            mounts.swap_remove(0);
        }
    } else {
        warn!("This workspace is not yet configured, default settings are used.");
    }

    for mount in &mounts {
        std::fs::create_dir_all(&mount.0)?;
    }

    Ok((extra_options, mounts))
}

/// A convenience function for iterating over all the instances while executing the actions
#[inline]
pub fn for_each_instance<F: Fn(&str) -> Result<()>>(func: &F) -> Result<()> {
    let instances = machine::list_instances_simple()?;
    for instance in instances {
        eprintln!("{} {}", style(">>>").bold(), style(&instance).cyan().bold());
        func(&instance)?;
    }

    Ok(())
}
