//! DualFs is a filesystem that can use both Memory and HostFs for reading,
//! but only Memory for writing to files (so that files can be read if the
//! directory mappings are set up correctly, but files can't accidentally be
//! written to the host machine).

use crate::{
    FileSystem, FsError, Metadata, OpenOptions, OpenOptionsConfig, Path, ReadDir, VirtualFile,
};
use std::fmt::Debug;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

pub trait ReadOnly: FileSystem + Debug + Default {}

impl<T: FileSystem + Debug + Default> ReadOnly for T {}
pub trait ReadWrite: FileSystem + Debug + Default {}

impl<T: FileSystem + Debug + Default> ReadWrite for T {}

#[derive(Debug, Default, Clone)]
#[cfg_attr(feature = "enable-serde", derive(Serialize, Deserialize))]
pub struct DualFilesystem<T: ReadOnly, U: ReadWrite> {
    readonly: Arc<T>,
    readwrite: Arc<Mutex<U>>,
}

impl<T: ReadOnly, U: ReadWrite> crate::FileSystem for DualFilesystem<T, U> {
    fn read_dir(&self, path: &Path) -> Result<ReadDir, FsError> {
        let r1 = self.readonly.read_dir(path);
        if r1.is_err() {
            self.readwrite.lock()?.read_dir(path)
        } else {
            r1
        }
    }

    fn create_dir(&self, path: &Path) -> Result<(), FsError> {
        self.readwrite.lock()?.create_dir(path)
    }

    fn remove_dir(&self, path: &Path) -> Result<(), FsError> {
        self.readwrite.lock()?.remove_dir(path)
    }

    fn rename(&self, from: &Path, to: &Path) -> Result<(), FsError> {
        self.readwrite.lock()?.rename(from, to)
    }

    fn remove_file(&self, path: &Path) -> Result<(), FsError> {
        self.readwrite.lock()?.remove_file(path)
    }

    fn metadata(&self, path: &Path) -> Result<Metadata, FsError> {
        let r1 = self.readonly.metadata(path);
        if r1.is_err() {
            self.readwrite.lock()?.metadata(path)
        } else {
            r1
        }
    }

    fn new_open_options(&self) -> OpenOptions {
        OpenOptions::new(Box::new(DualFsFileOpener {
            readonly: self.readonly.new_open_options(),
            readwrite: self.readwrite.lock().unwrap().new_open_options(),
        }))
    }
}

pub struct DualFsFileOpener {
    readonly: OpenOptions,
    readwrite: OpenOptions,
}

impl crate::FileOpener for DualFsFileOpener {
    fn open(
        &mut self,
        path: &Path,
        conf: &OpenOptionsConfig,
    ) -> Result<Box<dyn VirtualFile + Send + Sync + 'static>, FsError> {
        let use_write_fs =
            conf.write || conf.create_new || conf.create || conf.append || conf.truncate;

        if use_write_fs {
            self.readwrite
                .read(conf.read)
                .write(conf.write)
                .create_new(conf.create_new)
                .create(conf.create)
                .append(conf.append)
                .truncate(conf.truncate)
                .open(path)
        } else {
            let r1 = self
                .readonly
                .read(conf.read)
                .write(conf.write)
                .create_new(conf.create_new)
                .create(conf.create)
                .append(conf.append)
                .truncate(conf.truncate)
                .open(path);

            if r1.is_err() {
                self.readwrite
                    .read(conf.read)
                    .write(conf.write)
                    .create_new(conf.create_new)
                    .create(conf.create)
                    .append(conf.append)
                    .truncate(conf.truncate)
                    .open(path)
            } else {
                r1
            }
        }
    }
}

impl<T> From<PoisonError<MutexGuard<'_, T>>> for FsError {
    fn from(_: PoisonError<MutexGuard<T>>) -> Self {
        FsError::Lock
    }
}
