use std::path::PathBuf;

use anyhow::Result;
use ciel::{SimpleAptRepository, Workspace};
use log::info;

pub fn refresh_repo(path: Option<PathBuf>) -> Result<()> {
    let ws = Workspace::current_dir()?;
    info!("Refreshing local repository ...");
    SimpleAptRepository::new(path.unwrap_or_else(|| ws.output_directory())).refresh()?;
    Ok(())
}
