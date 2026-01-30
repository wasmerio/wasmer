use crate::{node::FsNode, BackendInodeId, VfsCapabilities};
use std::sync::Arc;

pub trait Fs: Send + Sync + 'static {
    fn provider_name(&self) -> &'static str;
    fn capabilities(&self) -> VfsCapabilities;

    fn root(&self) -> Arc<dyn FsNode>;

    fn node_by_inode(&self, _inode: BackendInodeId) -> Option<Arc<dyn FsNode>> {
        None
    }
}
