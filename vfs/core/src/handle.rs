//! VFS handle types.
//!
//! These are placeholders for Phase 2.4 (OFD semantics and IO). For now they are opaque IDs.

use crate::VfsHandleId;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct VfsHandle {
    id: VfsHandleId,
}

impl VfsHandle {
    pub(crate) fn new(id: VfsHandleId) -> Self {
        Self { id }
    }

    pub fn id(&self) -> VfsHandleId {
        self.id
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct VfsDirHandle {
    id: VfsHandleId,
}

impl VfsDirHandle {
    pub(crate) fn new(id: VfsHandleId) -> Self {
        Self { id }
    }

    pub fn id(&self) -> VfsHandleId {
        self.id
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct DirStreamHandle {
    id: VfsHandleId,
}

impl DirStreamHandle {
    pub(crate) fn new(id: VfsHandleId) -> Self {
        Self { id }
    }

    pub fn id(&self) -> VfsHandleId {
        self.id
    }
}

