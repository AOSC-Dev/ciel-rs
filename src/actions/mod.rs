use anyhow::Result;
use console::style;

use crate::machine;

mod container;
mod onboarding;
mod packaging;
mod config;

// re-export all the functions from the sub
pub use self::container::*;
pub use self::onboarding::onboarding;
pub use self::packaging::*;
pub use self::config::*;

const APT_UPDATE_SCRIPT: &str = r#"export DEBIAN_FRONTEND=noninteractive;apt-get update -y --allow-releaseinfo-change && apt-get -y -o Dpkg::Options::="--force-confnew" full-upgrade --autoremove --purge && apt autoclean"#;
const OMA_UPDATE_SCRIPT: &str = r#"oma upgrade -y --force-confnew --no-progress --force-unsafe-io && oma autoremove -y --remove-config && oma clean"#;

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
