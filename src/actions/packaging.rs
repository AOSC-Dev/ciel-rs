
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
    let conf = WorkspaceConfig::load();
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

    Ok(0)
}

