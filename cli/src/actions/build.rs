use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::Result;
use ciel::{
    build::{BuildCheckPoint, BuildRequest},
    InstanceConfig, Workspace,
};
use clap::ArgMatches;
use console::style;
use log::info;
use walkdir::WalkDir;

use crate::{config::patch_instance_config, utils::create_spinner};

pub fn clean_outputs() -> Result<()> {
    let spinner = create_spinner("Removing output directories ...", 200);
    for entry in WalkDir::new(".").max_depth(1) {
        let entry = entry?;
        if entry.file_type().is_dir() && entry.file_name().to_string_lossy().starts_with("OUTPUT-")
        {
            fs::remove_dir_all(entry.path())?;
        }
    }
    if Path::new("SRCS").is_dir() {
        fs::remove_dir_all("SRCS")?;
    }
    if Path::new("STATES").is_dir() {
        fs::remove_dir_all("STATES")?;
    }
    spinner.finish_with_message("Done.");

    Ok(())
}

pub fn build_packages(args: &ArgMatches) -> Result<()> {
    let ws = Workspace::current_dir()?;

    let ckpt = if let Some(file) = args.get_one::<String>("resume") {
        BuildCheckPoint::load(file)?
    } else {
        let mut req = BuildRequest::new(
            args.get_many::<String>("PACKAGES")
                .unwrap()
                .map(|s| s.to_owned())
                .collect(),
        );
        req.fetch_only = args.get_flag("fetch-only");
        BuildCheckPoint::from(req, &ws)?
    };

    let res = if let Some(inst) = args.get_one::<String>("INSTANCE") {
        let inst = ws.instance(inst)?.open()?;
        ckpt.execute(&inst)
    } else {
        let mut config = InstanceConfig::default();
        patch_instance_config(args, &mut config)?;
        let inst = ws.ephemeral_container("build", config)?;
        ckpt.execute(&inst)
    };
    match res {
        Ok(out) => {
            eprintln!(
                "{} - {} packages in {}",
                style("BUILD SUCCESSFUL").bold().green(),
                out.total_packages,
                format_duration(out.time_elapsed)
            );
        }
        Err((ckpt, err)) => {
            eprintln!("{} - {:?}", style("BUILD FAILED").bold().red(), err);
            if let Some(ckpt) = ckpt {
                if std::env::var("CIEL_NO_CHECKPOINT").is_err() {
                    dump_build_checkpoint(&ckpt)?;
                }
            }
        }
    }
    Ok(())
}

fn format_duration(seconds: u64) -> String {
    format!(
        "{:02}:{:02}:{:02}",
        seconds / 3600,
        (seconds / 60) % 60,
        seconds % 60
    )
}

fn dump_build_checkpoint(ckpt: &BuildCheckPoint) -> Result<()> {
    let last_package = ckpt
        .packages
        .get(ckpt.progress)
        .map_or("unknown".to_string(), |x| x.to_owned());
    let last_package = last_package.replace('/', "_");
    let current = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs();

    fs::create_dir_all("STATES")?;
    let path = PathBuf::from("STATES").join(format!("{}-{}.ciel-ckpt", last_package, current));
    ckpt.write(&path)?;
    info!("Ciel created a check-point: {:?}", path);

    Ok(())
}

#[cfg(test)]
mod test {
    use crate::actions::build::format_duration;

    #[test]
    fn test_time_format() {
        let test_dur = 3661;
        assert_eq!(format_duration(test_dur), "01:01:01");
    }
}
