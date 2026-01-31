use std::path::PathBuf;
use vfs_core::{VfsError, VfsErrorKind, VfsResult};

#[derive(Clone, Debug)]
pub struct HostFsConfig {
    /// Host path of the directory to expose as the filesystem root.
    pub root: PathBuf,
    /// Strict validation mode (reject non-directory roots).
    pub strict: bool,
}

impl HostFsConfig {
    pub fn validate(&self) -> VfsResult<()> {
        if self.strict {
            let meta = std::fs::metadata(&self.root).map_err(|err| {
                VfsError::with_source(VfsErrorKind::NotFound, "host.config.stat_root", err)
            })?;
            if !meta.is_dir() {
                return Err(VfsError::new(
                    VfsErrorKind::NotDir,
                    "host.config.root_not_dir",
                ));
            }
        }
        Ok(())
    }
}
