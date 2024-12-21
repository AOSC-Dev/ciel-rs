use std::path::Path;

use anyhow::{bail, Result};
use log::info;

use crate::download::download_git;

pub fn load_tree(url: String) -> Result<()> {
    info!("Cloning abbs tree ...");
    let path = Path::new("TREE");
    if path.exists() {
        bail!("TREE already exists")
    }
    download_git(&url, path)?;
    Ok(())
}
