use anyhow::{anyhow, Result};
use console::style;
use dialoguer::{theme::ColorfulTheme, Select};
use nix::unistd::gethostname;
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File},
    io::{BufRead, BufReader, Write},
    path::Path,
    thread::sleep,
    time::{Duration, Instant},
};
use walkdir::WalkDir;

use crate::{common::create_spinner, config, error, info, repo, warn};

use super::{
    container::{get_output_directory, mount_fs, rollback_container, run_in_container},
    UPDATE_SCRIPT,
};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct BuildCheckPoint {
    packages: Vec<String>,
    progress: usize,
    time_elapsed: usize,
    attempts: usize,
}

#[derive(Debug, Copy, Clone)]
pub struct BuildSettings {
    pub offline: bool,
    pub stage2: bool,
}

pub fn load_build_checkpoint<P: AsRef<Path>>(path: P) -> Result<BuildCheckPoint> {
    let f = File::open(path)?;

    Ok(bincode::deserialize_from(f)?)
}

fn dump_build_checkpoint(checkpoint: &BuildCheckPoint) -> Result<()> {
    let save_state = bincode::serialize(checkpoint)?;
    let last_package = checkpoint
        .packages
        .get(checkpoint.progress)
        .map_or("unknown".to_string(), |x| x.to_owned());
    let last_package = last_package.replace('/', "_");
    let current = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs();
    fs::create_dir_all("./STATES")?;
    let path = Path::new("./STATES").join(format!("{}-{}.ciel-ckpt", last_package, current));
    let mut f = File::create(&path)?;
    f.write_all(&save_state)?;
    info!("Ciel created a check-point: {}", path.display());

    Ok(())
}

#[inline]
fn format_duration(seconds: u64) -> String {
    format!(
        "{:02}:{:02}:{:02}",
        seconds / 3600,
        (seconds / 60) % 60,
        seconds % 60
    )
}

fn read_package_list<P: AsRef<Path>>(filename: P, depth: usize) -> Result<Vec<String>> {
    if depth > 32 {
        return Err(anyhow!(
            "Nested group exceeded 32 levels! Potential infinite loop."
        ));
    }
    let f = fs::File::open(filename)?;
    let reader = BufReader::new(f);
    let mut results = Vec::new();
    for line in reader.lines() {
        let line = line?;
        // skip comment
        if line.starts_with('#') {
            continue;
        }
        // trim whitespace
        let trimmed = line.trim();
        // skip empty line
        if trimmed.is_empty() {
            continue;
        }
        // process nested groups
        if trimmed.starts_with("groups/") {
            let path = Path::new("./TREE").join(trimmed);
            let nested = read_package_list(&path, depth + 1)?;
            results.extend(nested);
            continue;
        }
        results.push(trimmed.to_owned());
    }

    Ok(results)
}

/// Expand the packages list to an array of packages
fn expand_package_list<S: AsRef<str>, I: IntoIterator<Item = S>>(packages: I) -> Vec<String> {
    let mut expanded = Vec::new();
    for package in packages {
        let package = package.as_ref();
        if !package.starts_with("groups/") {
            expanded.push(package.to_string());
            continue;
        }
        let list_file = Path::new("./TREE").join(&package);
        match read_package_list(list_file, 0) {
            Ok(list) => {
                info!("Read {} packages from {}", list.len(), package);
                expanded.extend(list);
            }
            Err(e) => {
                warn!("Unable to read package group `{}`: {}", package, e);
            }
        }
    }

    expanded
}

#[inline]
fn package_build_inner<P: AsRef<Path>>(
    packages: &[String],
    instance: &str,
    root: P,
) -> Result<(i32, usize)> {
    let total = packages.len();
    let hostname = gethostname().map_or_else(
        |_| "unknown".to_string(),
        |s| s.into_string().unwrap_or_else(|_| "unknown".to_string()),
    );
    for (index, package) in packages.iter().enumerate() {
        // set terminal title, \r is for hiding the message if the terminal does not support the sequence
        eprint!(
            "\x1b]0;ciel: [{}/{}] {} ({}@{})\x07\r",
            index + 1,
            total,
            package,
            instance,
            hostname
        );
        // hopefully the sequence gets flushed together with the `info!` below
        info!("[{}/{}] Building {}...", index + 1, total, package);
        mount_fs(instance)?;
        info!("Refreshing local repository...");
        repo::init_repo(root.as_ref(), Path::new(instance))?;
        let mut status = -1;
        for i in 1..=5 {
            status = run_in_container(instance, &["/bin/bash", "-ec", UPDATE_SCRIPT]).unwrap_or(-1);
            if status == 0 {
                break;
            } else {
                let interval = 3u64.pow(i);
                warn!(
                    "Failed to update the OS, will retry in {} seconds ...",
                    interval
                );
                sleep(Duration::from_secs(interval));
            }
        }
        if status != 0 {
            error!("Failed to update the OS before building packages");
            return Ok((status, index));
        }
        let status = run_in_container(instance, &["/bin/acbs-build", "--", package])?;
        if status != 0 {
            error!("Build failed with status: {}", status);
            return Ok((status, index));
        }
        rollback_container(instance)?;
    }

    Ok((0, 0))
}

pub fn packages_stage_select<S: AsRef<str>, K: Clone + ExactSizeIterator<Item = S>>(
    instance: &str,
    packages: K,
    settings: BuildSettings,
    start_package: Option<&String>,
) -> Result<i32> {
    let packages = expand_package_list(packages);

    let selection = if let Some(start_package) = start_package {
        packages
            .iter()
            .position(|x| {
                x == start_package || x.split_once('/').map(|x| x.1) == Some(start_package)
            })
            .ok_or_else(|| anyhow!("Can not find the specified package in the list!"))?
    } else {
        eprintln!("-*-* S T A G E\t\tS E L E C T *-*-");

        Select::with_theme(&ColorfulTheme::default())
            .default(0)
            .with_prompt(
                "Choose a package to start building from (left/right arrow keys to change pages)",
            )
            .items(&packages)
            .interact()?
    };
    let empty: Vec<&str> = Vec::new();

    package_build(
        instance,
        empty.into_iter(),
        Some(BuildCheckPoint {
            packages,
            progress: selection,
            time_elapsed: 0,
            attempts: 1,
        }),
        settings,
    )
}

/// Fetch all the source packages in one go
pub fn package_fetch<S: AsRef<str>>(instance: &str, packages: &[S]) -> Result<i32> {
    let conf = config::read_config();
    if conf.is_err() {
        return Err(anyhow!("Please configure this workspace first!"));
    }
    let conf = conf.unwrap();
    if !conf.local_sources {
        warn!("Using this function without local sources caching is probably meaningless.");
    }

    mount_fs(instance)?;
    rollback_container(instance)?;

    let mut cmd = vec!["/bin/acbs-build", "-g", "--"];
    cmd.extend(packages.iter().map(|p| p.as_ref()));
    let status = run_in_container(instance, &cmd)?;

    Ok(status)
}

/// Build packages in the container
pub fn package_build<S: AsRef<str>, K: Clone + ExactSizeIterator<Item = S>>(
    instance: &str,
    packages: K,
    state: Option<BuildCheckPoint>,
    settings: BuildSettings,
) -> Result<i32> {
    let conf = config::read_config();
    if conf.is_err() {
        return Err(anyhow!("Please configure this workspace first!"));
    }
    let conf = conf.unwrap();
    let mut attempts = 1usize;

    let packages = if let Some(p) = state {
        attempts = p.attempts + 1;
        info!(
            "Successfully restored from a checkpoint. Attempt #{} started.",
            attempts
        );
        p.packages[p.progress..].to_owned()
    } else {
        expand_package_list(packages)
    };

    if settings.offline || std::env::var("CIEL_OFFLINE").is_ok() {
        info!("Preparing offline mode. Fetching source packages first ...");
        package_fetch(instance, &packages)?;
        std::env::set_var("CIEL_OFFLINE", "ON");
        // FIXME: does not work with current version of systemd
        info!("Running in offline mode. Network access disabled.");
    }

    if settings.stage2 {
        std::env::set_var("CIEL_STAGE2", "ON");
        info!("Running in stage 2 mode. ACBS and autobuild3 may behave differently.");
    }

    mount_fs(instance)?;
    rollback_container(instance)?;

    if !conf.local_repo {
        let mut cmd = vec!["/bin/acbs-build".to_string(), "--".to_string()];
        cmd.extend(packages.into_iter());
        let status = run_in_container(instance, &cmd)?;
        return Ok(status);
    }

    let output_dir = get_output_directory(conf.sep_mount);
    let root = std::env::current_dir()?.join(output_dir);
    let total = packages.len();
    let start = Instant::now();
    let (exit_status, progress) = package_build_inner(&packages, instance, root)?;
    if exit_status != 0 {
        let checkpoint = BuildCheckPoint {
            packages,
            progress,
            attempts,
            time_elapsed: 0,
        };
        dump_build_checkpoint(&checkpoint)?;
        return Ok(exit_status);
    }
    let duration = start.elapsed().as_secs();
    eprintln!(
        "{} - {} packages in {}",
        style("BUILD SUCCESSFUL").bold().green(),
        total,
        format_duration(duration)
    );

    Ok(0)
}

/// Clean up output directories
pub fn cleanup_outputs() -> Result<()> {
    let spinner = create_spinner("Removing output directories ...", 200);
    for entry in WalkDir::new(".").max_depth(1) {
        let entry = entry?;
        if entry.file_type().is_dir() && entry.file_name().to_string_lossy().starts_with("OUTPUT-")
        {
            fs::remove_dir_all(entry.path())?;
        }
    }
    if Path::new("./SRCS").is_dir() {
        fs::remove_dir_all("./SRCS")?;
    }
    if Path::new("./STATES").is_dir() {
        fs::remove_dir_all("./STATES")?;
    }
    spinner.finish_with_message("Done.");

    Ok(())
}

#[test]
fn test_time_format() {
    let test_dur = 3661;
    assert_eq!(format_duration(test_dur), "01:01:01");
}
