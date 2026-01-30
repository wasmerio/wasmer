use std::sync::Arc;

use smallvec::SmallVec;

use vfs_core::{BackendInodeId, Fs, VfsCapabilities, VfsError, VfsErrorKind, VfsResult};

use crate::config::OverlayOptions;
use crate::inodes::{BackingKey, OverlayInodeTable};
use crate::node::{LowerNode, OverlayNode, OverlayNodeKind};

pub struct OverlayFs {
    pub(crate) upper: Arc<dyn Fs>,
    pub(crate) lowers: Vec<Arc<dyn Fs>>,
    pub(crate) opts: OverlayOptions,
    pub(crate) inodes: Arc<OverlayInodeTable>,
}

impl OverlayFs {
    pub fn new(upper: Arc<dyn Fs>, lowers: Vec<Arc<dyn Fs>>, opts: OverlayOptions) -> VfsResult<Self> {
        if lowers.is_empty() {
            return Err(VfsError::new(
                VfsErrorKind::InvalidInput,
                "overlay.no_lowers",
            ));
        }
        Ok(Self {
            upper,
            lowers,
            opts,
            inodes: Arc::new(OverlayInodeTable::new()),
        })
    }

    pub(crate) fn make_node(
        self: &Arc<Self>,
        name: Option<vfs_core::VfsNameBuf>,
        parent: Option<std::sync::Weak<OverlayNode>>,
        kind: OverlayNodeKind,
        upper: Option<Arc<dyn vfs_core::node::FsNode>>,
        lowers: SmallVec<[LowerNode; 2]>,
    ) -> Arc<OverlayNode> {
        let overlay_inode = match &upper {
            Some(node) => {
                let key = BackingKey::Upper {
                    inode: node.inode(),
                };
                self.inodes.overlay_id_for(key)
            }
            None => {
                let primary = lowers
                    .first()
                    .expect("lower node required")
                    .to_key();
                self.inodes.overlay_id_for(primary)
            }
        };
        OverlayNode::new(
            self.clone(),
            kind,
            overlay_inode,
            upper,
            lowers,
            parent,
            name,
        )
    }

    pub(crate) fn root_node(self: &Arc<Self>) -> Arc<OverlayNode> {
        let upper_root = self.upper.root();
        let mut lowers = SmallVec::new();
        for (idx, lower) in self.lowers.iter().enumerate() {
            let node = lower.root();
            if node.file_type() == vfs_core::VfsFileType::Directory {
                lowers.push(LowerNode {
                    layer: idx as u16,
                    node,
                });
            }
        }
        self.make_node(
            None,
            None,
            OverlayNodeKind::Dir,
            Some(upper_root),
            lowers,
        )
    }
}

impl Fs for OverlayFs {
    fn provider_name(&self) -> &'static str {
        "overlay"
    }

    fn capabilities(&self) -> VfsCapabilities {
        self.upper.capabilities()
    }

    fn root(&self) -> Arc<dyn vfs_core::node::FsNode> {
        let fs = Arc::new(self.clone());
        fs.root_node()
    }

    fn node_by_inode(&self, _inode: BackendInodeId) -> Option<Arc<dyn vfs_core::node::FsNode>> {
        None
    }
}

impl Clone for OverlayFs {
    fn clone(&self) -> Self {
        Self {
            upper: self.upper.clone(),
            lowers: self.lowers.clone(),
            opts: self.opts.clone(),
            inodes: self.inodes.clone(),
        }
    }
}
