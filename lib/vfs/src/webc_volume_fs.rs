use std::path::Path;

use webc::compat::{Container, Volume};

use crate::{FileSystem, OverlayFileSystem};

#[derive(Debug, Clone)]
pub struct WebcVolumeFileSystem {
    volume: Volume,
}

impl WebcVolumeFileSystem {
    pub fn new(volume: Volume) -> Self {
        WebcVolumeFileSystem { volume }
    }

    /// Get a filesystem where all [`Volume`]s in a [`Container`] are mounted to
    /// the root directory.
    pub fn mount_all(
        container: &Container,
    ) -> OverlayFileSystem<crate::mem_fs::FileSystem, Vec<WebcVolumeFileSystem>> {
        let mut filesystems = Vec::new();

        for volume in container.volumes().into_values() {
            filesystems.push(WebcVolumeFileSystem::new(volume));
        }

        OverlayFileSystem::new(crate::mem_fs::FileSystem::default(), filesystems)
    }
}

impl FileSystem for WebcVolumeFileSystem {
    fn read_dir(&self, path: &Path) -> crate::Result<crate::ReadDir> {
        todo!()
    }

    fn create_dir(&self, path: &Path) -> crate::Result<()> {
        todo!()
    }

    fn remove_dir(&self, path: &Path) -> crate::Result<()> {
        todo!()
    }

    fn rename(&self, from: &Path, to: &Path) -> crate::Result<()> {
        todo!()
    }

    fn metadata(&self, path: &Path) -> crate::Result<crate::Metadata> {
        todo!()
    }

    fn remove_file(&self, path: &Path) -> crate::Result<()> {
        todo!()
    }

    fn new_open_options(&self) -> crate::OpenOptions {
        todo!()
    }
}
