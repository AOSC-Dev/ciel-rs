use anyhow::Result;
use dialoguer::Confirm;
use std::{fs, path::Path};

fn farewell(path: &Path) -> Result<()> {
    let delete = Confirm::new()
        .with_prompt("DELETE ALL CIEL THINGS?")
        .interact()?;
    if delete {
        fs::remove_dir_all(path.join(".ciel"))?;
    }

    Ok(())
}
