use std::sync::Arc;

use wasixv2_syscalls::{Errno, Fd, LocalFuture};

/// Whatever an open file refers to: an inode, a socket, a pipe, a terminal.
pub trait FileObject: Send + Sync {
    fn read<'a>(&'a self, buf: &'a mut [u8]) -> LocalFuture<'a, Result<usize, Errno>>;
    fn write<'a>(&'a self, data: &'a [u8]) -> LocalFuture<'a, Result<usize, Errno>>;
}

/// An open file description: the part `dup` and `fork` share.
pub(crate) struct OpenFile {
    pub(crate) object: Arc<dyn FileObject>,
}

#[derive(Default)]
pub(crate) struct FdTable {
    slots: Vec<Option<Arc<OpenFile>>>,
}

impl FdTable {
    pub(crate) fn get(&self, fd: Fd) -> Option<Arc<OpenFile>> {
        let index = usize::try_from(fd.0).ok()?;
        self.slots.get(index)?.clone()
    }

    /// Installs `file` at the lowest free descriptor, as `open` does.
    pub(crate) fn install(&mut self, file: Arc<OpenFile>) -> Fd {
        let index = match self.slots.iter().position(Option::is_none) {
            Some(index) => index,
            None => {
                self.slots.push(None);
                self.slots.len() - 1
            }
        };
        self.slots[index] = Some(file);
        Fd(index as i32)
    }
}
