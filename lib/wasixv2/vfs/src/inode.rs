//! The VFS's own inode (Linux's in-core `struct inode` and the inode cache).
//!
//! A driver may hand out several `Node` values for one object (a host directory opens a new
//! descriptor per lookup). The VFS interns them per `(superblock, ino)`, so everything it keeps
//! per object lives in one place for as long as anything (a dentry, an open file) holds it: the
//! directory lock, the removed-directory and linkable flags, and a directory's single dentry.

use std::sync::{Arc, Mutex, Weak};

use futures_util::lock::{Mutex as AsyncMutex, OwnedMutexGuard};

use crate::{dentry::Dentry, driver::Node, mount::Superblock, types::FileType};

/// Held while a directory's entries are looked up or changed (`i_rwsem`).
pub(crate) type DirLock = OwnedMutexGuard<()>;

pub struct Inode {
    node: Arc<dyn Node>,
    sb: Arc<Superblock>,
    lock: Arc<AsyncMutex<()>>,
    state: Mutex<State>,
}

#[derive(Default)]
struct State {
    /// A removed directory: nothing may be created in it (`S_DEAD`).
    dead: bool,
    /// An `O_TMPFILE` file that `linkat` may still give a name (`I_LINKABLE`).
    linkable: bool,
    /// The one dentry of a directory (Linux keeps a single alias per directory).
    alias: Weak<Dentry>,
}

impl Inode {
    pub fn node(&self) -> &Arc<dyn Node> {
        &self.node
    }

    pub fn sb(&self) -> &Arc<Superblock> {
        &self.sb
    }

    pub fn ino(&self) -> u64 {
        self.node.ino()
    }

    pub fn file_type(&self) -> FileType {
        self.node.file_type()
    }

    /// Takes the directory lock.
    pub(crate) async fn lock(&self) -> DirLock {
        self.lock.clone().lock_owned().await
    }

    pub(crate) fn lock_ptr(&self) -> usize {
        Arc::as_ptr(&self.lock) as usize
    }

    pub(crate) fn is_dead(&self) -> bool {
        self.state.lock().unwrap().dead
    }

    pub(crate) fn set_dead(&self) {
        self.state.lock().unwrap().dead = true;
    }

    pub(crate) fn is_linkable(&self) -> bool {
        self.state.lock().unwrap().linkable
    }

    pub(crate) fn set_linkable(&self, linkable: bool) {
        self.state.lock().unwrap().linkable = linkable;
    }

    pub(crate) fn alias(&self) -> Option<Arc<Dentry>> {
        self.state.lock().unwrap().alias.upgrade()
    }

    pub(crate) fn set_alias(&self, dentry: &Arc<Dentry>) {
        self.state.lock().unwrap().alias = Arc::downgrade(dentry);
    }
}

impl Drop for Inode {
    fn drop(&mut self) {
        let mut inodes = self.sb.inodes.lock().unwrap();
        let ino = self.node.ino();
        if inodes
            .get(&ino)
            .is_some_and(|weak| weak.strong_count() == 0)
        {
            inodes.remove(&ino);
        }
    }
}

impl Superblock {
    /// The VFS inode for `node` (`iget`): the live one with its inode number, else a new one.
    pub(crate) fn iget(self: &Arc<Self>, node: Arc<dyn Node>) -> Arc<Inode> {
        let ino = node.ino();
        let mut inodes = self.inodes.lock().unwrap();
        if let Some(inode) = inodes.get(&ino).and_then(Weak::upgrade) {
            return inode;
        }
        let inode = Arc::new(Inode {
            node,
            sb: self.clone(),
            lock: Arc::default(),
            state: Mutex::default(),
        });
        inodes.insert(ino, Arc::downgrade(&inode));
        inode
    }
}
