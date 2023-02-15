//! DualFs is a filesystem that can use both Memory and HostFs for reading,
//! but only Memory for writing to files (so that files can be read if the
//! directory mappings are set up correctly, but files can't accidentally be
//! written to the host machine).

use crate::{
    FileSystem, FsError, Metadata, OpenOptions, OpenOptionsConfig, Path, ReadDir, VirtualFile,
};
use std::fmt::Debug;
use std::sync::{Arc, Mutex};

pub trait ReadOnly: FileSystem + Debug {}

impl<T: FileSystem + Debug> ReadOnly for T {}
pub trait ReadWrite: FileSystem + Debug {}

impl<T: FileSystem + Debug> ReadWrite for T {}

#[derive(Debug, Default, Clone)]
#[cfg_attr(feature = "enable-serde", derive(Serialize, Deserialize))]
pub struct DualFilesystem {
    pub readonly: Vec<Arc<dyn ReadOnly>>,
    pub readwrite: Vec<Arc<Mutex<Box<dyn ReadWrite>>>>,
}

impl crate::FileSystem for DualFilesystem {
    fn read_dir(&self, path: &Path) -> Result<ReadDir, FsError> {
        for r in self.readonly.iter() {
            if let Ok(r) = r.read_dir(path) {
                return Ok(r);
            }
        }
        for r in self.readwrite.iter() {
            if let Ok(Ok(r)) = r.lock().map(|r| r.read_dir(path)) {
                return Ok(r);
            }
        }
        Err(FsError::EntityNotFound)
    }

    fn create_dir(&self, path: &Path) -> Result<(), FsError> {
        for r in self.readwrite.iter() {
            if let Ok(Ok(())) = r.lock().map(|r| r.create_dir(path)) {
                return Ok(());
            }
        }
        Err(FsError::UnknownError)
    }

    fn remove_dir(&self, path: &Path) -> Result<(), FsError> {
        for r in self.readwrite.iter() {
            if let Ok(Ok(())) = r.lock().map(|r| r.remove_dir(path)) {
                return Ok(());
            }
        }
        Err(FsError::EntityNotFound)
    }

    fn rename(&self, from: &Path, to: &Path) -> Result<(), FsError> {
        for r in self.readwrite.iter() {
            if let Ok(Ok(())) = r.lock().map(|r| r.rename(from, to)) {
                return Ok(());
            }
        }
        Err(FsError::EntityNotFound)
    }

    fn remove_file(&self, path: &Path) -> Result<(), FsError> {
        for r in self.readwrite.iter() {
            if let Ok(Ok(())) = r.lock().map(|r| r.remove_file(path)) {
                return Ok(());
            }
        }
        Err(FsError::EntityNotFound)
    }

    fn metadata(&self, path: &Path) -> Result<Metadata, FsError> {
        for r in self.readonly.iter() {
            if let Ok(r) = r.metadata(path) {
                return Ok(r);
            }
        }
        for r in self.readwrite.iter() {
            if let Ok(Ok(r)) = r.lock().map(|r| r.metadata(path)) {
                return Ok(r);
            }
        }
        Err(FsError::EntityNotFound)
    }

    fn new_open_options(&self) -> OpenOptions {
        OpenOptions::new(Box::new(DualFsFileOpener {
            readonly: self.readonly.to_vec(),
            readwrite: self.readwrite.to_vec(),
        }))
    }
}

pub struct DualFsFileOpener {
    readonly: Vec<Arc<dyn ReadOnly>>,
    readwrite: Vec<Arc<Mutex<Box<dyn ReadWrite>>>>,
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
            for r in self.readwrite.iter() {
                if let Ok(mut r) = r.lock().map(|r| r.new_open_options()) {
                    let ok = r
                        .read(conf.read)
                        .write(conf.write)
                        .create_new(conf.create_new)
                        .create(conf.create)
                        .append(conf.append)
                        .truncate(conf.truncate)
                        .open(path);

                    if let Ok(r) = ok {
                        return Ok(r);
                    }
                }
            }
        } else {
            for r in self.readonly.iter() {
                let ok = r
                    .new_open_options()
                    .read(conf.read)
                    .write(conf.write)
                    .create_new(conf.create_new)
                    .create(conf.create)
                    .append(conf.append)
                    .truncate(conf.truncate)
                    .open(path);
                if let Ok(r) = ok {
                    return Ok(r);
                }
            }
            for r in self.readwrite.iter() {
                let lock = match r.lock() {
                    Ok(o) => o,
                    Err(_) => continue,
                };
                let ok = lock
                    .new_open_options()
                    .read(conf.read)
                    .write(conf.write)
                    .create_new(conf.create_new)
                    .create(conf.create)
                    .append(conf.append)
                    .truncate(conf.truncate)
                    .open(path);
                if let Ok(r) = ok {
                    return Ok(r);
                }
            }
        }
        Err(FsError::EntityNotFound)
    }
}
