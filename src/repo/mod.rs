use std::{
    fmt::Debug,
    fs,
    io::Write,
    path::{Path, PathBuf},
};

use faster_hex::hex_string;
use log::info;
use sha2::{Digest, Sha256};
use time::{format_description::FormatItem, macros::format_description, OffsetDateTime};

pub mod monitor;
pub mod scan;

use crate::Result;

/// Debian 822 date: "%a, %d %b %Y %H:%M:%S %z"
const DEB822_DATE: &[FormatItem] = format_description!(
    "[weekday repr:short], [day] [month repr:short] [year] [hour repr:24]:[minute]:[second] [offset_hour sign:mandatory][offset_minute]"
);

/// A simple flat APT package repository.
#[derive(Clone)]
pub struct SimpleAptRepository {
    path: PathBuf,
}

impl Debug for SimpleAptRepository {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        Debug::fmt(&self.path, f)
    }
}

impl SimpleAptRepository {
    /// Creates a new APT repository object.
    pub fn new<P: AsRef<Path>>(path: P) -> Self {
        Self {
            path: path.as_ref().to_owned(),
        }
    }

    /// Returns the `debs` directory.
    pub fn directory(&self) -> &Path {
        &self.path
    }

    /// Returns the path of `Packages` file.
    pub fn packages_file(&self) -> PathBuf {
        self.path.join("Packages")
    }

    /// Returns the path of `Packages` file.
    pub fn release_file(&self) -> PathBuf {
        self.path.join("Release")
    }

    /// Returns the path of `fresh.lock` file.
    pub fn refresh_lock_file(&self) -> PathBuf {
        self.path.join("fresh.lock")
    }
}

impl SimpleAptRepository {
    /// Generates the `Release` file.
    pub fn generate_release(&self) -> Result<String> {
        let mut f = fs::File::open(self.packages_file())?;

        let mut hasher = Sha256::new();
        std::io::copy(&mut f, &mut hasher)?;
        let sha256sum = hex_string(&hasher.finalize());

        let meta = f.metadata()?;
        let timestamp = OffsetDateTime::now_utc().format(&DEB822_DATE)?;

        Ok(format!(
            "Date: {}\nSHA256:\n {} {} Packages\n",
            timestamp,
            sha256sum,
            meta.len()
        ))
    }

    /// Refreshes the repository index, i.e. `Packages` and `Release` file.
    pub fn refresh(&self) -> Result<()> {
        fs::create_dir_all(self.directory())?;

        let entries = scan::collect_all_packages(self.directory())?;
        info!("Scanning {} packages ...", entries.len());
        {
            let mut file = fs::File::create(self.packages_file())?;
            for chunk in scan::scan_packages_simple(&entries, self.directory())? {
                file.write(&chunk)?;
            }
        }
        fs::write(self.release_file(), self.generate_release()?)?;
        info!("Refreshed all packages");

        Ok(())
    }
}

#[cfg(test)]
mod test {
    use std::fs;

    use test_log::test;

    use crate::test::TestDir;

    #[test]
    fn test_simple_apt_repo_refresh() {
        let testdir = TestDir::from("testdata/simple-repo");
        let repo = testdir.apt_repo();
        repo.refresh().unwrap();
        assert_eq!(
            fs::read_to_string("testdata/simple-repo/debs/Packages").unwrap(),
            fs::read_to_string(testdir.path().join("debs/Packages")).unwrap(),
        )
    }
}
