//! Names: the tree of directory entries the VFS has seen (Linux's dcache).
//!
//! A dentry ties a driver object to the name and parent it was reached through, which is what
//! `..`, `getcwd`, mount points and "is this directory inside that one" need. Only dentries
//! something holds (a working directory, an open file, a mount, a walk in progress) stay alive;
//! parents hold their children weakly. Lookups always ask the driver and then reuse the cached
//! dentry when the driver returns the same object, so a dentry never answers for a name that
//! changed behind the VFS's back (the host side of a host directory, say). A directory has one
//! dentry at most: finding it under a new name moves that dentry (`d_splice_alias`), so the
//! working directory and the rename ancestry check follow moves made on the host.
//!
//! A directory's children change only under its inode's lock, so a lookup's driver call and
//! its cache update happen as one step relative to `create`, `unlink` and `rename`.

use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex, Weak,
        atomic::{AtomicU32, Ordering},
    },
};

use crate::{driver::Node, inode::Inode, mount::Superblock, types::FileType};

pub struct Dentry {
    inode: Arc<Inode>,
    inner: Mutex<Inner>,
    /// How many mounts sit on this dentry.
    mounts: AtomicU32,
}

struct Inner {
    name: Vec<u8>,
    /// `None` for the root of a filesystem.
    parent: Option<Arc<Dentry>>,
    children: HashMap<Vec<u8>, Weak<Dentry>>,
    /// The name was removed (`unlink`, `rmdir`, replaced by `rename`).
    unlinked: bool,
}

impl Dentry {
    /// The root dentry of a filesystem.
    pub(crate) fn root(sb: &Arc<Superblock>, node: Arc<dyn Node>) -> Arc<Self> {
        Self::new(sb.iget(node), b"/".to_vec(), None)
    }

    /// A dentry with no name, for an `O_TMPFILE` file.
    pub(crate) fn anonymous(sb: &Arc<Superblock>, node: Arc<dyn Node>) -> Arc<Self> {
        let dentry = Self::new(sb.iget(node), b"#".to_vec(), None);
        dentry.inner.lock().unwrap().unlinked = true;
        dentry
    }

    fn new(inode: Arc<Inode>, name: Vec<u8>, parent: Option<Arc<Dentry>>) -> Arc<Self> {
        let is_dir = inode.file_type() == FileType::Directory;
        let dentry = Arc::new(Self {
            inode,
            inner: Mutex::new(Inner {
                name,
                parent,
                children: HashMap::new(),
                unlinked: false,
            }),
            mounts: AtomicU32::new(0),
        });
        if is_dir {
            dentry.inode.set_alias(&dentry);
        }
        dentry
    }

    pub fn inode(&self) -> &Arc<Inode> {
        &self.inode
    }

    pub fn node(&self) -> &Arc<dyn Node> {
        self.inode.node()
    }

    pub fn sb(&self) -> &Arc<Superblock> {
        self.inode.sb()
    }

    pub fn file_type(&self) -> FileType {
        self.inode.file_type()
    }

    pub fn is_dir(&self) -> bool {
        self.file_type() == FileType::Directory
    }

    pub fn name(&self) -> Vec<u8> {
        self.inner.lock().unwrap().name.clone()
    }

    pub fn parent(&self) -> Option<Arc<Dentry>> {
        self.inner.lock().unwrap().parent.clone()
    }

    pub fn is_unlinked(&self) -> bool {
        self.inner.lock().unwrap().unlinked
    }

    pub(crate) fn is_mountpoint(&self) -> bool {
        self.mounts.load(Ordering::Relaxed) != 0
    }

    pub(crate) fn add_mount(&self, delta: i32) {
        match delta {
            1 => self.mounts.fetch_add(1, Ordering::Relaxed),
            _ => self.mounts.fetch_sub(1, Ordering::Relaxed),
        };
    }

    /// The child dentry for `name`, which the driver resolved to `node` (`d_splice_alias`): the
    /// cached one if it still names the same object, the directory's existing dentry moved
    /// here, or a new one. Called with this directory locked.
    pub(crate) fn child(self: &Arc<Self>, name: &[u8], node: Arc<dyn Node>) -> Arc<Dentry> {
        let inode = self.sb().iget(node);
        if let Some(cached) = self.cached_child(name)
            && Arc::ptr_eq(&cached.inode, &inode)
        {
            return cached;
        }
        if let Some(alias) = inode.alias()
            && alias.parent().is_some()
            && !alias.is_ancestor_of(self)
        {
            alias.move_to(self, name);
            return alias;
        }
        let child = Self::new(inode, name.to_vec(), Some(self.clone()));
        let stale = {
            let mut inner = self.inner.lock().unwrap();
            inner.children.insert(name.to_vec(), Arc::downgrade(&child))
        };
        if let Some(stale) = stale.and_then(|weak| weak.upgrade()) {
            stale.inner.lock().unwrap().unlinked = true;
        }
        child
    }

    /// The driver no longer has `name`: a cached dentry for it is stale (`d_invalidate`).
    pub(crate) fn forget_name(&self, name: &[u8]) {
        if let Some(stale) = self.cached_child(name) {
            stale.detach();
        }
    }

    pub(crate) fn cached_child(&self, name: &[u8]) -> Option<Arc<Dentry>> {
        let inner = self.inner.lock().unwrap();
        inner.children.get(name).and_then(Weak::upgrade)
    }

    /// The name is gone (`d_delete`).
    pub(crate) fn detach(self: &Arc<Self>) {
        let parent = {
            let mut inner = self.inner.lock().unwrap();
            inner.unlinked = true;
            inner.parent.clone()
        };
        if let Some(parent) = parent {
            parent.forget_child(self);
        }
    }

    /// Removes the cache entry pointing at `child`, if it still does.
    fn forget_child(&self, child: &Dentry) {
        let mut inner = self.inner.lock().unwrap();
        let name = child.inner.lock().unwrap().name.clone();
        if inner
            .children
            .get(&name)
            .is_some_and(|weak| std::ptr::eq(weak.as_ptr(), child))
        {
            inner.children.remove(&name);
        }
    }

    /// The object moved to `new_name` under `new_parent` (`d_move`). Whatever the cache had
    /// under the new name is replaced, and is unlinked.
    pub(crate) fn move_to(self: &Arc<Self>, new_parent: &Arc<Dentry>, new_name: &[u8]) {
        let old_parent = self.inner.lock().unwrap().parent.clone();
        if let Some(old_parent) = &old_parent {
            old_parent.forget_child(self);
        }
        {
            let mut inner = self.inner.lock().unwrap();
            inner.name = new_name.to_vec();
            inner.parent = Some(new_parent.clone());
        }
        let displaced = {
            let mut inner = new_parent.inner.lock().unwrap();
            inner
                .children
                .insert(new_name.to_vec(), Arc::downgrade(self))
        };
        if let Some(displaced) = displaced.and_then(|weak| weak.upgrade())
            && !Arc::ptr_eq(&displaced, self)
        {
            displaced.inner.lock().unwrap().unlinked = true;
        }
        drop(old_parent);
    }

    /// Whether `self` is `other` or one of its ancestors in the same filesystem.
    pub fn is_ancestor_of(self: &Arc<Self>, other: &Arc<Dentry>) -> bool {
        let mut current = Some(other.clone());
        while let Some(dentry) = current {
            if Arc::ptr_eq(&dentry, self) {
                return true;
            }
            current = dentry.parent();
        }
        false
    }
}

impl Drop for Dentry {
    fn drop(&mut self) {
        let inner = self.inner.get_mut().unwrap();
        if let Some(parent) = inner.parent.take() {
            let mut parent_inner = parent.inner.lock().unwrap();
            if parent_inner
                .children
                .get(&inner.name)
                .is_some_and(|weak| weak.strong_count() == 0)
            {
                parent_inner.children.remove(&inner.name);
            }
        }
    }
}
