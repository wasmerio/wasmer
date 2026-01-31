
use std::path::{Path, PathBuf};
use std::sync::Arc;

use vfs_core::path_types::{VfsPath, VfsPathBuf};
use vfs_core::{BackendInodeId, VfsCapabilities, VfsError, VfsErrorKind, VfsResult};
use vfs_core::{Fs, VfsFileType};
use webc::{PathSegments, ToPathSegments, Volume};

use crate::node::WebcNode;

#[derive(Clone)]
pub struct WebcFs {
    inner: Arc<WebcFsInner>,
}

#[derive(Debug)]
pub struct WebcFsInner {
    pub volume: Volume,
    pub root: Option<PathBuf>,
}

impl WebcFs {
    pub fn new(volume: Volume, root: Option<PathBuf>) -> Self {
        Self {
            inner: Arc::new(WebcFsInner { volume, root }),
        }
    }

    pub fn root_node(&self) -> Arc<WebcNode> {
        Arc::new(WebcNode::new(
            self.inner.clone(),
            VfsPathBuf::from(b"/".as_slice()),
        ))
    }

    pub fn to_webc_path(inner: &WebcFsInner, path: &VfsPath) -> VfsResult<PathSegments> {
        let mut base = inner.root.clone().unwrap_or_else(|| PathBuf::from("/"));
        let path_str = path.to_utf8_lossy();
        let relative = path_str.trim_start_matches('/');
        base.push(relative);
        normalize(&base).map_err(|_| VfsError::new(VfsErrorKind::InvalidInput, "webc.path"))
    }
}

impl Fs for WebcFs {
    fn provider_name(&self) -> &'static str {
        "webc"
    }

    fn capabilities(&self) -> VfsCapabilities {
        VfsCapabilities::NONE
    }

    fn root(&self) -> Arc<dyn vfs_core::node::FsNode> {
        self.root_node()
    }

    fn node_by_inode(&self, inode: BackendInodeId) -> Option<Arc<dyn vfs_core::node::FsNode>> {
        // WebC volumes don't provide an inode index. We use a hash of the path
        // and can't reconstruct it without a path map.
        let _ = inode;
        None
    }
}

/// Normalize a path into webc PathSegments (handles ., .., repeated slashes).
fn normalize(path: &Path) -> Result<PathSegments, webc::PathSegmentError> {
    path.to_path_segments()
}
