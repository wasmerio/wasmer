//! When no file system is used by a WebC then this is used as a placeholder -
//! as the name suggests it always returns file not found.

use std::path::Path;
#[allow(unused_imports, dead_code)]
use tracing::{debug, error, info, trace, warn};

use crate::*;

#[derive(Debug, Default)]
pub struct EmptyFileSystem {}

#[allow(unused_variables)]
impl FileSystem for EmptyFileSystem {
    fn read_dir(&self, path: &Path) -> Result<ReadDir> {
        Err(FsError::EntryNotFound)
    }

    fn create_dir(&self, path: &Path) -> Result<()> {
        Err(FsError::EntryNotFound)
    }

    fn remove_dir(&self, path: &Path) -> Result<()> {
        Err(FsError::EntryNotFound)
    }

    fn rename(&self, from: &Path, to: &Path) -> Result<()> {
        Err(FsError::EntryNotFound)
    }

    fn metadata(&self, path: &Path) -> Result<Metadata> {
        Err(FsError::EntryNotFound)
    }

    fn symlink_metadata(&self, path: &Path) -> Result<Metadata> {
        Err(FsError::EntryNotFound)
    }

    fn remove_file(&self, path: &Path) -> Result<()> {
        Err(FsError::EntryNotFound)
    }

    fn new_open_options(&self) -> OpenOptions {
        OpenOptions::new(self)
    }
}

impl FileOpener for EmptyFileSystem {
    #[allow(unused_variables)]
    fn open(
        &self,
        path: &Path,
        conf: &OpenOptionsConfig,
    ) -> Result<Box<dyn VirtualFile + Send + Sync + 'static>> {
        Err(FsError::EntryNotFound)
    }
}
