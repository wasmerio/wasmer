use vfs_core::node::{CreateFile, FsNode};
use vfs_core::path_types::{VfsName, VfsNameBuf};
use vfs_core::{VfsError, VfsErrorKind, VfsResult};

const RESERVED_PREFIX: &[u8] = b".wasmer_overlay.";
const OPAQUE_MARKER: &[u8] = b".wasmer_overlay.opaque";
const WHITEOUT_PREFIX: &[u8] = b".wasmer_overlay.wh.";

pub fn is_reserved_name(name: &VfsName) -> bool {
    name.as_bytes().starts_with(RESERVED_PREFIX)
}

pub fn is_reserved_name_bytes(name: &[u8]) -> bool {
    name.starts_with(RESERVED_PREFIX)
}

pub fn is_opaque_marker(name: &[u8]) -> bool {
    name == OPAQUE_MARKER
}

pub fn is_whiteout_marker(name: &[u8]) -> Option<&[u8]> {
    name.strip_prefix(WHITEOUT_PREFIX)
}

pub fn whiteout_name_for(name: &VfsName) -> VfsResult<VfsNameBuf> {
    let mut full = Vec::with_capacity(WHITEOUT_PREFIX.len() + name.as_bytes().len());
    full.extend_from_slice(WHITEOUT_PREFIX);
    full.extend_from_slice(name.as_bytes());
    VfsNameBuf::new(full)
}

pub fn opaque_name() -> VfsResult<VfsNameBuf> {
    VfsNameBuf::new(OPAQUE_MARKER.to_vec())
}

pub fn create_whiteout(parent: &dyn FsNode, name: &VfsName) -> VfsResult<()> {
    let wh = whiteout_name_for(name)?;
    let create = CreateFile {
        mode: None,
        truncate: true,
        exclusive: false,
    };
    let _ = parent.create_file(&VfsName::new(wh.as_bytes())?, create)?;
    Ok(())
}

#[allow(dead_code)]
pub fn create_opaque_marker(parent: &dyn FsNode) -> VfsResult<()> {
    let name = opaque_name()?;
    let create = CreateFile {
        mode: None,
        truncate: true,
        exclusive: false,
    };
    let _ = parent.create_file(&VfsName::new(name.as_bytes())?, create)?;
    Ok(())
}

pub fn deny_reserved_name(name: &VfsName) -> VfsResult<()> {
    if is_reserved_name(name) {
        return Err(VfsError::new(
            VfsErrorKind::PermissionDenied,
            "overlay.reserved_name",
        ));
    }
    Ok(())
}
