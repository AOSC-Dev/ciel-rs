use anyhow::Result;
use ciel::{SimpleAptRepository, Workspace};
use log::info;

pub fn refresh_repo() -> Result<()> {
    let ws = Workspace::current_dir()?;
    info!("Refreshing local repository ...");
    SimpleAptRepository::new(ws.output_directory()).refresh()?;
    Ok(())
}
