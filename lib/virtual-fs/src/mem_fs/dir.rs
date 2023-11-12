use super::{FileSystem, Inode};
use crate::FileSystem as _;
use crate::{DirEntry, FileType, FsError, Metadata, OpenOptions, ReadDir, Result};
use futures::future::BoxFuture;
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[derive(Debug)]
pub struct Directory {
    inode: Inode,
    fs: FileSystem,
}

impl Directory {
    pub fn new(inode: Inode, fs: FileSystem) -> Self {
        Self { inode, fs }
    }
}

impl crate::Directory for Directory {
    fn parent(self) -> Option<Box<dyn crate::Directory + Send + Sync>> {
        unimplemented!();
    }

    fn get_dir(&self, path: &Path) -> Result<Box<dyn crate::Directory + Send + Sync>> {
        // self.fs.get_inode(self.inode);
        unimplemented!();
    }

    fn read_dir(&self, path: &Path) -> Result<ReadDir> {
        self.fs.read_dir(path)
    }

    fn create_dir(&self, path: &Path) -> Result<()> {
        self.fs.create_dir(path)
    }

    fn remove_dir(&self, path: &Path) -> Result<()> {
        self.fs.remove_dir(path)
    }

    fn rename<'a>(&'a self, from: &'a Path, to: &'a Path) -> BoxFuture<'a, Result<()>> {
        self.fs.rename(from, to)
    }

    fn metadata(&self, path: &Path) -> Result<Metadata> {
        self.fs.metadata(path)
    }

    fn remove_file(&self, path: &Path) -> Result<()> {
        self.fs.remove_file(path)
    }

    fn new_open_options(&self) -> OpenOptions {
        unimplemented!();
        // OpenOptions::new(self)
    }
}
