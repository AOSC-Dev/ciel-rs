use std::{
    fs,
    path::{Path, PathBuf},
};

use libmount::Tmpfs;
use log::info;
use nix::mount::{MntFlags, umount2};

use crate::{Result, instance::TmpfsConfig};

use super::Layer;

/// A `tmpfs`-backed filesystem layer.
pub struct TmpfsLayer {
    target: PathBuf,
    size: usize,
}

impl TmpfsLayer {
    pub fn new<P: AsRef<Path>>(target: P, config: &TmpfsConfig) -> Self {
        Self {
            target: target.as_ref().into(),
            size: config.size_bytes(),
        }
    }
}

impl Layer for TmpfsLayer {
    fn fs_type(&self) -> Option<&'static str> {
        Some("tmpfs")
    }

    fn target(&self) -> &Path {
        &self.target
    }

    fn mount(&self) -> Result<()> {
        info!("tmpfs: mounting at {:?}", self.target);
        if !self.target.exists() {
            fs::create_dir_all(&self.target)?;
        }
        Tmpfs::new(&self.target).size_bytes(self.size).mount()?;
        Ok(())
    }

    fn unmount(&self) -> Result<()> {
        // tmpfs ignores unmount to avoid data loss
        Ok(())
    }

    fn reset(&self) -> Result<()> {
        if !self.is_mounted()? {
            return Ok(());
        }
        info!("tmpfs: un-mounting at {:?}", self.target);
        umount2(&self.target, MntFlags::MNT_DETACH)?;
        fs::remove_dir_all(&self.target)?;
        Ok(())
    }
}

#[cfg(test)]
mod test {
    use crate::{
        fs::Layer,
        instance::TmpfsConfig,
        test::{TestDir, is_root},
    };

    use super::TmpfsLayer;

    #[test]
    fn test_tmpfs() {
        let testdir = TestDir::new();
        let layer = TmpfsLayer::new(testdir.path(), &TmpfsConfig::default());
        assert!(!layer.is_mounted().unwrap());
        if !is_root() {
            return;
        }
        layer.mount().unwrap();
        assert!(layer.is_mounted().unwrap());
        layer.unmount().unwrap();
        assert!(layer.is_mounted().unwrap());
        layer.reset().unwrap();
        assert!(!layer.is_mounted().unwrap());
    }
}
