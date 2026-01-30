use std::sync::Arc;

use vfs_core::flags::{OpenFlags, OpenOptions, ResolveFlags};
use vfs_core::node::{CreateFile, FsHandle, FsNode, MkdirOptions, SetMetadata};
use vfs_core::path_types::{VfsName, VfsPathBuf};
use vfs_core::VfsResult;

use crate::config::OverlayOptions;

fn read_all(handle: &dyn FsHandle) -> VfsResult<Vec<u8>> {
    let mut offset = 0u64;
    let mut data = Vec::new();
    let mut buf = vec![0u8; 8192];
    loop {
        let read = handle.read_at(offset, &mut buf)?;
        if read == 0 {
            break;
        }
        data.extend_from_slice(&buf[..read]);
        offset += read as u64;
    }
    Ok(data)
}

pub fn copy_up_file(
    parent_upper: &dyn FsNode,
    name: &VfsName,
    lower_node: &dyn FsNode,
    opts: &OverlayOptions,
) -> VfsResult<Arc<dyn FsNode>> {
    let meta = lower_node.metadata()?;
    let create = CreateFile {
        mode: Some(meta.mode.bits()),
        truncate: true,
        exclusive: false,
    };
    let upper_node = parent_upper.create_file(name, create)?;

    let lower_handle = lower_node.open(OpenOptions {
        flags: OpenFlags::READ,
        mode: None,
        resolve: ResolveFlags::empty(),
    })?;
    let upper_handle = upper_node.open(OpenOptions {
        flags: OpenFlags::WRITE | OpenFlags::TRUNC,
        mode: None,
        resolve: ResolveFlags::empty(),
    })?;

    let mut offset = 0u64;
    let mut buf = vec![0u8; opts.max_copy_chunk_size.max(1)];
    loop {
        let read = lower_handle.read_at(offset, &mut buf)?;
        if read == 0 {
            break;
        }
        upper_handle.write_at(offset, &buf[..read])?;
        offset += read as u64;
    }

    let set = SetMetadata {
        mode: Some(meta.mode),
        uid: Some(meta.uid),
        gid: Some(meta.gid),
        size: Some(meta.size),
        atime: Some(meta.atime),
        mtime: Some(meta.mtime),
        ctime: Some(meta.ctime),
    };
    let _ = upper_node.set_metadata(set);
    Ok(upper_node)
}

pub fn copy_up_symlink(
    parent_upper: &dyn FsNode,
    name: &VfsName,
    lower_node: &dyn FsNode,
) -> VfsResult<Arc<dyn FsNode>> {
    let target = lower_node.readlink()?;
    parent_upper.symlink(name, target.as_path())?;
    parent_upper.lookup(name)
}

pub fn copy_up_dir(
    parent_upper: &dyn FsNode,
    name: &VfsName,
    lower_node: &dyn FsNode,
) -> VfsResult<Arc<dyn FsNode>> {
    let meta = lower_node.metadata()?;
    let upper_node = parent_upper.mkdir(
        name,
        MkdirOptions {
            mode: Some(meta.mode.bits()),
        },
    )?;
    let set = SetMetadata {
        mode: Some(meta.mode),
        uid: Some(meta.uid),
        gid: Some(meta.gid),
        size: None,
        atime: Some(meta.atime),
        mtime: Some(meta.mtime),
        ctime: Some(meta.ctime),
    };
    let _ = upper_node.set_metadata(set);
    Ok(upper_node)
}

pub fn copy_bytes(handle: &dyn FsHandle) -> VfsResult<Vec<u8>> {
    read_all(handle)
}

pub fn copy_up_symlink_target(lower_node: &dyn FsNode) -> VfsResult<VfsPathBuf> {
    lower_node.readlink()
}
