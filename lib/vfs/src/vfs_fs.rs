use crate::*;
use std::io::{Read, Seek, Write};
use std::path::Path;
use std::sync::Arc;

pub use crate::host_fs::{Stderr, Stdin, Stdout};

#[derive(Clone)]
pub struct VfsFileSystem {
    inner: Arc<dyn vfs::FileSystem>,
}

impl FileSystem for VfsFileSystem {
    fn read_dir(&self, _path: &Path) -> Result<std::fs::ReadDir, FsError> {
        todo!()
    }
    fn create_dir(&self, path: &Path) -> Result<(), FsError> {
        self.inner
            .create_dir(path.to_str().unwrap())
            .map_err(Into::into)
    }
    fn remove_dir(&self, path: &Path) -> Result<(), FsError> {
        self.inner
            .remove_dir(path.to_str().unwrap())
            .map_err(Into::into)
    }
    fn rename(&self, from: &Path, to: &Path) -> Result<(), FsError> {
        self.inner
            .move_file(from.to_str().unwrap(), to.to_str().unwrap())
            .map_err(Into::into)
    }

    fn remove_file(&self, path: &Path) -> Result<(), FsError> {
        self.inner
            .remove_file(path.to_str().unwrap())
            .map_err(Into::into)
    }
    fn new_open_options(&self) -> OpenOptions {
        OpenOptions::new(Box::new(VfsFileOpener(self.clone())))
    }
}


#[derive(Clone)]
pub struct VfsFileOpener(VfsFileSystem);

impl FileOpener for VfsFileOpener {
    fn open(
        &mut self,
        path: &Path,
        conf: &OpenOptionsConfig,
    ) -> Result<Box<dyn VirtualFile>, FsError> {
        // TODO: handle create implying write, etc.
        if conf.create() || conf.create_new() {
            let result = self.0.create_file(path.to_str().unwrap())?;
            return Ok(Box::new(VfsOpenFile::Write(result)));
        } else if conf.write() || conf.append() {
            let result = self.0.append_file(path.to_str().unwrap())?;
            return Ok(Box::new(VfsOpenFile::Write(result)));
        } else {
            let result = self.0.open_file(path.to_str().unwrap())?;
            return Ok(Box::new(VfsOpenFile::SeekAndRead(result)));
        }
    }
}

pub enum VfsOpenFile {
    Write(Box<dyn Write>),
    SeekAndRead(Box<dyn vfs::SeekAndRead>),
}

impl std::fmt::Debug for VfsOpenFile {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self  {
            Self::Write(_) => f.debug_struct("Write").finish(),
            Self::SeekAndRead(_) => f.debug_struct("SeekAndRead").finish(),
        }
    }
}

impl Read for VfsOpenFile {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self {
            Self::Write(_) => todo!(),
            Self::SeekAndRead(sar) => sar.read(buf),
        }
    }
    fn read_to_end(&mut self, buf: &mut Vec<u8>) -> io::Result<usize> {
        match self {
            Self::Write(_) => todo!(),
            Self::SeekAndRead(sar) => sar.read_to_end(buf),
        }
    }
    fn read_to_string(&mut self, buf: &mut String) -> io::Result<usize> {
        match self {
            Self::Write(_) => todo!(),
            Self::SeekAndRead(sar) => sar.read_to_string(buf),
        }
    }
    fn read_exact(&mut self, buf: &mut [u8]) -> io::Result<()> {
        match self {
            Self::Write(_) => todo!(),
            Self::SeekAndRead(sar) => sar.read_exact(buf),
        }
    }
}
impl Seek for VfsOpenFile {
    fn seek(&mut self, pos: io::SeekFrom) -> io::Result<u64> {
        match self {
            Self::Write(_) => todo!(),
            Self::SeekAndRead(sar) => sar.seek(pos),
        }
    }
}
impl Write for VfsOpenFile {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match self {
            Self::Write(w) => w.write(buf),
            Self::SeekAndRead(_) => todo!(),
        }
    }
    fn flush(&mut self) -> io::Result<()> {
        match self {
            Self::Write(w) => w.flush(),
            Self::SeekAndRead(_) => todo!(),
        }
    }
    fn write_all(&mut self, buf: &[u8]) -> io::Result<()> {
        match self {
            Self::Write(w) => w.write_all(buf),
            Self::SeekAndRead(_) => todo!(),
        }
    }
    fn write_fmt(&mut self, fmt: ::std::fmt::Arguments) -> io::Result<()> {
        match self {
            Self::Write(w) => w.write_fmt(fmt),
            Self::SeekAndRead(_) => todo!(),
        }
    }
}

#[cfg_attr(feature = "enable-serde", typetag::serde)]
impl VirtualFile for VfsOpenFile {
    fn last_accessed(&self) -> u64 {
        // this data does not exist in vfs
        0
    }

    fn last_modified(&self) -> u64 {
        // this data does not exist in vfs
        0
    }

    fn created_time(&self) -> u64 {
        // this data does not exist in vfs
        0
    }

    fn size(&self) -> u64 {
        todo!("vfs can do this, but it's difficult and requires the overall FS abstraction")
    }

    fn set_len(&mut self, new_size: u64) -> Result<(), FsError> {
        todo!("vfs can't do this!")
    }

    fn unlink(&mut self) -> Result<(), FsError> {
        // no-op, in vfs this isn't done  here
        Ok(())
    }
    fn sync_to_disk(&self) -> Result<(), FsError> {
        // no-op, in vfs this isn't done  here
        Ok(())
    }

    fn rename_file(&self, _new_name: &std::path::Path) -> Result<(), FsError> {
        // no-op, in vfs this isn't done  here
        Ok(())
    }

    fn bytes_available(&self) -> Result<usize, FsError> {
        todo!("unclear if vfs can do this")
    }

    fn get_raw_fd(&self) -> Option<i32> {
        None
    }
}
