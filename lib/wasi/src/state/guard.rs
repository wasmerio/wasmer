use super::*;
use std::{
    io::{Read, Seek},
    sync::{RwLockReadGuard, RwLockWriteGuard},
};

#[derive(Debug)]
pub(crate) struct InodeValFileReadGuard<'a> {
    pub(crate) guard: RwLockReadGuard<'a, Kind>,
}

impl<'a> Deref for InodeValFileReadGuard<'a> {
    type Target = Option<Box<dyn VirtualFile + Send + Sync + 'static>>;
    fn deref(&self) -> &Self::Target {
        if let Kind::File { handle, .. } = self.guard.deref() {
            return handle;
        }
        unreachable!()
    }
}

#[derive(Debug)]
pub struct InodeValFileWriteGuard<'a> {
    pub(crate) guard: RwLockWriteGuard<'a, Kind>,
}

impl<'a> Deref for InodeValFileWriteGuard<'a> {
    type Target = Option<Box<dyn VirtualFile + Send + Sync + 'static>>;
    fn deref(&self) -> &Self::Target {
        if let Kind::File { handle, .. } = self.guard.deref() {
            return handle;
        }
        unreachable!()
    }
}

impl<'a> DerefMut for InodeValFileWriteGuard<'a> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        if let Kind::File { handle, .. } = self.guard.deref_mut() {
            return handle;
        }
        unreachable!()
    }
}

#[derive(Debug)]
pub(crate) struct WasiStateFileGuard {
    inodes: Arc<RwLock<WasiInodes>>,
    inode: generational_arena::Index,
}

impl WasiStateFileGuard {
    pub fn new(state: &WasiState, fd: wasi::Fd) -> Result<Option<Self>, FsError> {
        let inodes = state.inodes.read().unwrap();
        let fd_map = state.fs.fd_map.read().unwrap();
        if let Some(fd) = fd_map.get(&fd) {
            let guard = inodes.arena[fd.inode].read();
            if let Kind::File { .. } = guard.deref() {
                Ok(Some(Self {
                    inodes: state.inodes.clone(),
                    inode: fd.inode,
                }))
            } else {
                // Our public API should ensure that this is not possible
                Err(FsError::NotAFile)
            }
        } else {
            Ok(None)
        }
    }

    pub fn lock_read<'a>(
        &self,
        inodes: &'a RwLockReadGuard<WasiInodes>,
    ) -> InodeValFileReadGuard<'a> {
        let guard = inodes.arena[self.inode].read();
        if let Kind::File { .. } = guard.deref() {
            InodeValFileReadGuard { guard }
        } else {
            // Our public API should ensure that this is not possible
            unreachable!("Non-file found in standard device location")
        }
    }

    pub fn lock_write<'a>(
        &self,
        inodes: &'a RwLockReadGuard<WasiInodes>,
    ) -> InodeValFileWriteGuard<'a> {
        let guard = inodes.arena[self.inode].write();
        if let Kind::File { .. } = guard.deref() {
            InodeValFileWriteGuard { guard }
        } else {
            // Our public API should ensure that this is not possible
            unreachable!("Non-file found in standard device location")
        }
    }
}

impl VirtualFile for WasiStateFileGuard {
    fn last_accessed(&self) -> u64 {
        let inodes = self.inodes.read().unwrap();
        let guard = self.lock_read(&inodes);
        if let Some(file) = guard.deref() {
            file.last_accessed()
        } else {
            0
        }
    }

    fn last_modified(&self) -> u64 {
        let inodes = self.inodes.read().unwrap();
        let guard = self.lock_read(&inodes);
        if let Some(file) = guard.deref() {
            file.last_modified()
        } else {
            0
        }
    }

    fn created_time(&self) -> u64 {
        let inodes = self.inodes.read().unwrap();
        let guard = self.lock_read(&inodes);
        if let Some(file) = guard.deref() {
            file.created_time()
        } else {
            0
        }
    }

    fn size(&self) -> u64 {
        let inodes = self.inodes.read().unwrap();
        let guard = self.lock_read(&inodes);
        if let Some(file) = guard.deref() {
            file.size()
        } else {
            0
        }
    }

    fn set_len(&mut self, new_size: u64) -> Result<(), FsError> {
        let inodes = self.inodes.read().unwrap();
        let mut guard = self.lock_write(&inodes);
        if let Some(file) = guard.deref_mut() {
            file.set_len(new_size)
        } else {
            Err(FsError::IOError)
        }
    }

    fn unlink(&mut self) -> Result<(), FsError> {
        let inodes = self.inodes.read().unwrap();
        let mut guard = self.lock_write(&inodes);
        if let Some(file) = guard.deref_mut() {
            file.unlink()
        } else {
            Err(FsError::IOError)
        }
    }

    fn sync_to_disk(&self) -> Result<(), FsError> {
        let inodes = self.inodes.read().unwrap();
        let guard = self.lock_read(&inodes);
        if let Some(file) = guard.deref() {
            file.sync_to_disk()
        } else {
            Err(FsError::IOError)
        }
    }

    fn bytes_available(&self) -> Result<usize, FsError> {
        let inodes = self.inodes.read().unwrap();
        let guard = self.lock_read(&inodes);
        if let Some(file) = guard.deref() {
            file.bytes_available()
        } else {
            Err(FsError::IOError)
        }
    }

    fn bytes_available_read(&self) -> Result<Option<usize>, FsError> {
        let inodes = self.inodes.read().unwrap();
        let guard = self.lock_read(&inodes);
        if let Some(file) = guard.deref() {
            file.bytes_available_read()
        } else {
            Err(FsError::IOError)
        }
    }

    fn bytes_available_write(&self) -> Result<Option<usize>, FsError> {
        let inodes = self.inodes.read().unwrap();
        let guard = self.lock_read(&inodes);
        if let Some(file) = guard.deref() {
            file.bytes_available_write()
        } else {
            Err(FsError::IOError)
        }
    }

    fn is_open(&self) -> bool {
        let inodes = self.inodes.read().unwrap();
        let guard = self.lock_read(&inodes);
        if let Some(file) = guard.deref() {
            file.is_open()
        } else {
            false
        }
    }

    fn get_fd(&self) -> Option<wasmer_vfs::FileDescriptor> {
        let inodes = self.inodes.read().unwrap();
        let guard = self.lock_read(&inodes);
        if let Some(file) = guard.deref() {
            file.get_fd()
        } else {
            None
        }
    }
}

impl Write for WasiStateFileGuard {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let inodes = self.inodes.read().unwrap();
        let mut guard = self.lock_write(&inodes);
        if let Some(file) = guard.deref_mut() {
            file.write(buf)
        } else {
            Err(std::io::ErrorKind::Unsupported.into())
        }
    }

    fn write_vectored(&mut self, bufs: &[std::io::IoSlice<'_>]) -> std::io::Result<usize> {
        let inodes = self.inodes.read().unwrap();
        let mut guard = self.lock_write(&inodes);
        if let Some(file) = guard.deref_mut() {
            file.write_vectored(bufs)
        } else {
            Err(std::io::ErrorKind::Unsupported.into())
        }
    }

    fn flush(&mut self) -> std::io::Result<()> {
        let inodes = self.inodes.read().unwrap();
        let mut guard = self.lock_write(&inodes);
        if let Some(file) = guard.deref_mut() {
            file.flush()
        } else {
            Err(std::io::ErrorKind::Unsupported.into())
        }
    }
}

impl Read for WasiStateFileGuard {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let inodes = self.inodes.read().unwrap();
        let mut guard = self.lock_write(&inodes);
        if let Some(file) = guard.deref_mut() {
            file.read(buf)
        } else {
            Err(std::io::ErrorKind::Unsupported.into())
        }
    }

    fn read_vectored(&mut self, bufs: &mut [std::io::IoSliceMut<'_>]) -> std::io::Result<usize> {
        let inodes = self.inodes.read().unwrap();
        let mut guard = self.lock_write(&inodes);
        if let Some(file) = guard.deref_mut() {
            file.read_vectored(bufs)
        } else {
            Err(std::io::ErrorKind::Unsupported.into())
        }
    }
}

impl Seek for WasiStateFileGuard {
    fn seek(&mut self, pos: std::io::SeekFrom) -> std::io::Result<u64> {
        let inodes = self.inodes.read().unwrap();
        let mut guard = self.lock_write(&inodes);
        if let Some(file) = guard.deref_mut() {
            file.seek(pos)
        } else {
            Err(std::io::ErrorKind::Unsupported.into())
        }
    }
}
