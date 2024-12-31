use std::{
    fs::{self},
    io::{BufRead, BufReader},
    path::Path,
    time::{Duration, Instant},
};

use log::{info, warn};
use nix::unistd::gethostname;
use serde::{Deserialize, Serialize};

use crate::{
    repo::monitor::RepositoryRefreshMonitor, Container, Error, Result, SimpleAptRepository,
    Workspace,
};

/// A build request.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BuildRequest {
    /// Packages to build.
    ///
    /// Package groups (`groups/xxx`) will be expanded on [BuildRequest::execute].
    pub packages: Vec<String>,
    /// Fetch-sources only mode.
    pub fetch_only: bool,
}

impl BuildRequest {
    /// Creates a new build request.
    pub fn new(packages: Vec<String>) -> Self {
        Self {
            packages,
            fetch_only: false,
        }
    }

    /// Expands the package list.
    ///
    /// This resolves and expands all rebuild groups.
    pub fn expand_packages(&self, workspace: &Workspace) -> Result<Vec<String>> {
        let mut out = vec![];
        let tree = workspace.directory().join("TREE");
        for pkg in &self.packages {
            if pkg.starts_with("groups/") {
                let path = tree.join(pkg);
                let nested = read_package_list(&tree, &path, 1)?;
                out.extend(nested);
            } else {
                out.push(pkg.to_owned());
            }
        }
        Ok(out)
    }

    /// Executes the build in a container.
    pub fn execute(self, container: &Container) -> BuildResult {
        BuildCheckPoint::from(self, container.workspace())
            .map_err(|err| (None, err))?
            .execute(container)
    }
}

fn read_package_list<P: AsRef<Path>>(tree: P, file: P, depth: usize) -> Result<Vec<String>> {
    if depth > 32 {
        return Err(Error::NestedPackageGroup);
    }
    let f = fs::File::open(file)?;
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
            let path = tree.as_ref().join(trimmed);
            let nested = read_package_list(tree.as_ref(), &path, depth + 1)?;
            results.extend(nested);
            continue;
        }
        results.push(trimmed.to_owned());
    }

    Ok(results)
}

/// A build checkpopint, including all packages to build and build progress.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct BuildCheckPoint {
    /// The original build request.
    pub build: BuildRequest,
    /// Expanded target packages list.
    pub packages: Vec<String>,
    /// Built packages index, starting from zero
    pub progress: usize,
    /// Elapsed time in seconds
    pub time_elapsed: u64,
    /// Retry attempts
    pub attempts: usize,
}

impl BuildCheckPoint {
    /// Loads a build checkpoint.
    pub fn load<P: AsRef<Path>>(path: P) -> Result<Self> {
        Ok(bincode::deserialize(&fs::read(path)?)?)
    }

    /// Writes a build checkpoint to file.
    pub fn write<P: AsRef<Path>>(&self, path: P) -> Result<()> {
        fs::write(path, self.serialize()?)?;
        Ok(())
    }

    /// Serializes a build checkpoint in bincode format.
    pub fn serialize(&self) -> Result<Vec<u8>> {
        Ok(bincode::serialize(self)?)
    }
}

#[derive(thiserror::Error, Debug)]
pub enum BuildError {
    #[error(transparent)]
    CielError(#[from] crate::Error),
    #[error("Failed to expand package list: {0}")]
    GroupExpansionFailure(crate::Error),
    #[error("Failed to update build container: {0}")]
    UpdateFailure(crate::Error),
    #[error("acbs-build exied with error: {0}")]
    AcbsFailure(std::process::ExitStatus),
    #[error("Failed to refresh the package repository: {0}")]
    RefreshRepoError(crate::Error),
}

/// Output of a build request.
#[derive(Debug, Clone)]
pub struct BuildOutput {
    /// Number of built packages.
    pub total_packages: usize,
    /// Total elapsed time, in seconds.
    pub time_elapsed: u64,
}

pub type BuildResult = std::result::Result<BuildOutput, (Option<BuildCheckPoint>, BuildError)>;

impl BuildCheckPoint {
    /// Creates a checkpoint from build request, marking all packages as not built yet.
    pub fn from(
        request: BuildRequest,
        workspace: &Workspace,
    ) -> std::result::Result<Self, BuildError> {
        Ok(Self {
            build: request.clone(),
            packages: request
                .expand_packages(workspace)
                .map_err(|err| BuildError::GroupExpansionFailure(err))?,
            progress: 0,
            time_elapsed: 0,
            attempts: 0,
        })
    }

    /// Resumes the build in a container.
    pub fn execute(mut self, container: &Container) -> BuildResult {
        info!("Executing build: {:?}", self.build);
        self.attempts += 1;

        let start = Instant::now();
        match execute(&mut self, container) {
            Ok(mut out) => {
                out.time_elapsed += start.elapsed().as_secs();
                Ok(out)
            }
            Err(err) => {
                self.time_elapsed += start.elapsed().as_secs();
                Err((Some(self), err))
            }
        }
    }
}

fn execute(
    ckpt: &mut BuildCheckPoint,
    container: &Container,
) -> std::result::Result<BuildOutput, BuildError> {
    let outupt_dir = container.output_directory();
    let total = ckpt.packages.len();

    let hostname = gethostname().map_or_else(
        |_| "unknown".to_string(),
        |s| s.into_string().unwrap_or_else(|_| "unknown".to_string()),
    );
    let refresh_monitor = RepositoryRefreshMonitor::new(SimpleAptRepository::new(&outupt_dir));

    for (index, package) in ckpt.packages.iter().enumerate() {
        if index < ckpt.progress {
            continue;
        }
        // set terminal title, \r is for hiding the message if the terminal does not support the sequence
        eprint!(
            "\x1b]0;ciel: [{}/{}] {} ({}@{})\x07\r",
            index + 1,
            total,
            package,
            container.instance().name(),
            hostname
        );
        info!("[{}/{}] Building {} ...", index + 1, total, package);
        container.rollback()?;
        container.boot()?;

        info!("Refreshing local repository ...");
        SimpleAptRepository::new(&outupt_dir).refresh()?;

        {
            let mut apt = None;
            for i in 1..=5 {
                match container.machine()?.update_system(apt) {
                    Ok(()) => break,
                    Err(Error::SubcommandError(status)) => {
                        if i == 5 {
                            return Err(BuildError::UpdateFailure(Error::SubcommandError(status)));
                        }
                        let interval = 3u64.pow(i);
                        warn!(
                            "Failed to update the OS, will retry in {} seconds ...",
                            interval
                        );
                        apt = Some(true);
                        std::thread::sleep(Duration::from_secs(interval));
                    }
                    Err(err) => return Err(BuildError::UpdateFailure(err)),
                }
            }
        }

        let mut args = vec!["/usr/bin/acbs-build"];
        if ckpt.build.fetch_only {
            args.push("-g");
        }
        args.push("--");
        args.push(&package);
        let status = container.machine()?.exec(args)?;
        if !status.success() {
            return Err(BuildError::AcbsFailure(status));
        }
        ckpt.progress = index;
    }

    refresh_monitor
        .stop()
        .map_err(|err| BuildError::RefreshRepoError(err))?;
    Ok(BuildOutput {
        total_packages: total,
        time_elapsed: ckpt.time_elapsed,
    })
}

pub trait BuildExt {
    fn build(&self, build: BuildRequest) -> BuildResult;
    fn resume(&self, ckpt: BuildCheckPoint) -> BuildResult;
}

impl BuildExt for Container {
    fn build(&self, build: BuildRequest) -> BuildResult {
        build.execute(&self)
    }
    fn resume(&self, ckpt: BuildCheckPoint) -> BuildResult {
        ckpt.execute(&self)
    }
}
