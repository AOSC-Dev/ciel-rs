use std::{
    ffi::OsString,
    fs,
    path::{self, Path, PathBuf},
    sync::Arc,
};

use crate::Result;

pub mod overlayfs;
pub use overlayfs::OverlayFS;
pub mod tmpfs;

/// A single layer in a layered filesystem.
pub trait Layer {
    /// Returns the filesystem type of the layer, e.g. "overlay".
    ///
    /// This name should be the same as the fs_type listed in the /proc/<>/mountinfo file.
    ///
    /// For simple directory layers, this returns [None].
    fn fs_type(&self) -> Option<&'static str>;

    /// Returns the target directory to mount on.
    fn target(&self) -> &Path;

    /// Returns whether the layer filesystem is mounted.
    ///
    /// For simple directory layers, this indicates if the directory exists.
    fn is_mounted(&self) -> Result<bool> {
        if let Some(ty) = self.fs_type() {
            is_mounted(self.target(), ty)
        } else {
            unreachable!()
        }
    }

    /// Mounts the target layer filesystem.
    ///
    /// If the filesystem is already mounted, nothing is executed.
    fn mount(&self) -> Result<()>;

    /// Un-mounts the target layer filesystem.
    ///
    /// If the filesystem is not mounted, nothing is executed.
    fn unmount(&self) -> Result<()>;

    /// Reset the layer into the initial state.
    ///
    /// This can be invoked when the layer is either mounted or not.
    /// The filesystem will be in un-mounted state after resetting.
    ///
    /// Warning: resetting the base system layer of workspaces will remove the base system,
    /// leaving a base-system-unloaded workspace.
    fn reset(&self) -> Result<()>;
}

pub type BoxedLayer = Arc<Box<dyn Layer>>;

/// A overlay manager which composes a filesystem with multiple layers.
pub trait OverlayManager {
    /// Returns the name of the layer manager, e.g. "overlay".
    ///
    /// This name should be the same as the fs_type listed in the /proc/<>/mountinfo file.
    fn fs_type(&self) -> &'static str;

    /// Returns the target directory to mount on.
    fn target(&self) -> &Path;

    /// Returns the upper layer of the layered filesystem, where changes
    /// to the target directory will be reflected in.
    fn upper_layer(&self) -> &BoxedLayer;

    /// Returns the lower layers to use.
    fn lower_layers(&self) -> Vec<&BoxedLayer>;

    /// Returns whether the filesystem is mounted.
    fn is_mounted(&self) -> Result<bool> {
        is_mounted(self.target(), self.fs_type())
    }

    /// Mounts the target filesystem.
    ///
    /// If the filesystem is already mounted, nothing is executed.
    fn mount(&self) -> Result<()>;

    /// Un-mounts the target filesystem.
    ///
    /// If the filesystem is not mounted, nothing is executed.
    fn unmount(&self) -> Result<()>;

    /// Discard changes to the target filesystem.
    ///
    /// If the filesystem is mounted, it will be un-mounted.
    fn rollback(&self) -> Result<()>;

    /// Commit changes in the upper layer to the toppest lower layer.
    ///
    /// If the filesystem is mounted, it will be un-mounted.
    fn commit(&self) -> Result<()>;
}

/// Checks if a path is a mountpoint with corresponding filesystem type.
pub(crate) fn is_mounted(mountpoint: &Path, fs_type: &str) -> Result<bool> {
    let mountpoint = path::absolute(mountpoint)?;
    let fs_type = OsString::from(fs_type);
    let mountinfo_content: Vec<u8> = fs::read("/proc/self/mountinfo")?;
    let parser = libmount::mountinfo::Parser::new(&mountinfo_content);

    for mount in parser {
        let mount = mount?;
        if mount.mount_point == mountpoint && mount.fstype == fs_type {
            return Ok(true);
        }
    }
    Ok(false)
}

/// A simple layer which is backed by a directory.
#[derive(Debug, Clone, PartialEq)]
pub struct SimpleLayer(PathBuf);

impl SimpleLayer {
    /// Creates a new simple layer with the given path.
    pub fn new<P: AsRef<Path>>(path: P) -> Self {
        Self(path.as_ref().to_owned())
    }
}

impl<P: AsRef<Path>> From<P> for SimpleLayer {
    fn from(value: P) -> Self {
        Self::new(value.as_ref())
    }
}

impl Layer for SimpleLayer {
    fn fs_type(&self) -> Option<&'static str> {
        None
    }

    fn target(&self) -> &Path {
        &self.0
    }

    fn is_mounted(&self) -> Result<bool> {
        Ok(self.target().exists())
    }

    fn mount(&self) -> Result<()> {
        fs::create_dir_all(self.target())?;
        Ok(())
    }

    fn unmount(&self) -> Result<()> {
        Ok(())
    }

    fn reset(&self) -> Result<()> {
        if self.target().exists() {
            fs::remove_dir_all(self.target())?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod test {
    use std::fs;

    use libmount::Tmpfs;
    use nix::mount::{MntFlags, umount2};

    use crate::{
        fs::{Layer, is_mounted},
        test::{TestDir, is_root},
    };

    use super::SimpleLayer;

    #[test]
    fn test_is_mounted() {
        let testdir = TestDir::new();
        assert!(!is_mounted(testdir.path(), "tmpfs").unwrap());
        assert!(!is_mounted(testdir.path(), "overlay").unwrap());
        if is_root() {
            Tmpfs::new(testdir.path())
                .size_bytes(1024 * 1024 * 4)
                .mount()
                .unwrap();
            assert!(is_mounted(testdir.path(), "tmpfs").unwrap());
            assert!(!is_mounted(testdir.path(), "overlay").unwrap());
            umount2(testdir.path(), MntFlags::MNT_DETACH).unwrap();
            assert!(!is_mounted(testdir.path(), "tmpfs").unwrap());
        }
    }

    #[test]
    fn test_simple_layer() {
        let testdir = TestDir::new();
        let dir = testdir.path().join("layer");
        let layer = SimpleLayer::new(&dir);

        assert!(!dir.exists());
        assert_eq!(layer.fs_type(), None);
        assert!(!layer.is_mounted().unwrap());

        layer.mount().unwrap();
        // behaviour compatible with Ciel <= 3.6.0
        assert!(matches!(layer.mount(), Ok(())));
        assert!(layer.is_mounted().unwrap());
        assert!(dir.exists());

        fs::write(dir.join("Test"), "Test").unwrap();
        assert_eq!(fs::read_to_string(dir.join("Test")).unwrap(), "Test");

        layer.unmount().unwrap();
        assert_eq!(fs::read_to_string(dir.join("Test")).unwrap(), "Test");
        assert!(matches!(layer.unmount(), Ok(())));

        layer.reset().unwrap();
        assert!(!dir.join("Test").exists());
    }
}
