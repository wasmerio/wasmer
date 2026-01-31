
use std::sync::Arc;

use vfs_core::path_types::VfsPath;
use vfs_core::path_walker::{PathWalkerAsync, ResolutionRequestAsync, WalkFlags};
use vfs_core::{VfsBaseDirAsync, VfsErrorKind};
use wasmer_wasix_types::wasi::Errno;

use crate::bin_factory::{BinFactory, BinaryPackage, BinaryPackageCommand};
use crate::fs::vfs::WasiFs;

pub async fn inject_package_commands(
    fs: &WasiFs,
    bin_factory: &BinFactory,
    pkg: Arc<BinaryPackage>,
) -> Result<(), Errno> {
    ensure_dir(fs, b"/bin").await?;
    ensure_dir(fs, b"/usr/bin").await?;

    for cmd in &pkg.commands {
        write_file(fs, &format!("/bin/{}", cmd.name()), cmd).await?;
        write_file(fs, &format!("/usr/bin/{}", cmd.name()), cmd).await?;
        bin_factory.set_binary(&format!("/bin/{}", cmd.name()), pkg.clone());
        bin_factory.set_binary(&format!("/usr/bin/{}", cmd.name()), pkg.clone());
    }

    Ok(())
}

pub async fn write_command_bytes(fs: &WasiFs, command: &str, bytes: &[u8]) -> Result<(), Errno> {
    ensure_dir(fs, b"/bin").await?;
    ensure_dir(fs, b"/usr/bin").await?;
    write_bytes(fs, &format!("/bin/{command}"), bytes).await?;
    write_bytes(fs, &format!("/usr/bin/{command}"), bytes).await?;
    Ok(())
}

async fn ensure_dir(fs: &WasiFs, path: &[u8]) -> Result<(), Errno> {
    let walker = PathWalkerAsync::new(fs.mounts.clone());
    let mut flags = WalkFlags::new(&fs.ctx.read().unwrap());
    flags.allow_empty_path = true;
    let req = ResolutionRequestAsync {
        ctx: &fs.ctx.read().unwrap(),
        base: VfsBaseDirAsync::Cwd,
        path: VfsPath::new(path),
        flags,
    };
    match walker.resolve(req).await {
        Ok(resolved) => {
            if resolved.node.file_type() != vfs_core::VfsFileType::Directory {
                return Err(Errno::Notdir);
            }
            Ok(())
        }
        Err(err) if err.kind() == VfsErrorKind::NotFound => {
            let parent = walker
                .resolve_parent(ResolutionRequestAsync {
                    ctx: &fs.ctx.read().unwrap(),
                    base: VfsBaseDirAsync::Cwd,
                    path: VfsPath::new(path),
                    flags,
                })
                .await
                .map_err(|_| Errno::Io)?;
            parent
                .dir
                .node
                .mkdir(&parent.name, vfs_core::node::MkdirOptions::default())
                .await
                .map_err(|_| Errno::Io)?;
            Ok(())
        }
        Err(_) => Err(Errno::Io),
    }
}

async fn write_file(fs: &WasiFs, path: &str, cmd: &BinaryPackageCommand) -> Result<(), Errno> {
    write_bytes(fs, path, cmd.atom_ref().as_ref()).await
}

async fn write_bytes(fs: &WasiFs, path: &str, bytes: &[u8]) -> Result<(), Errno> {
    let walker = PathWalkerAsync::new(fs.mounts.clone());
    let mut flags = WalkFlags::new(&fs.ctx.read().unwrap());
    flags.allow_empty_path = true;
    let req = ResolutionRequestAsync {
        ctx: &fs.ctx.read().unwrap(),
        base: VfsBaseDirAsync::Cwd,
        path: VfsPath::new(path.as_bytes()),
        flags,
    };
    let node = match walker.resolve(req).await {
        Ok(resolved) => resolved.node,
        Err(err) if err.kind() == VfsErrorKind::NotFound => {
            let parent = walker
                .resolve_parent(ResolutionRequestAsync {
                    ctx: &fs.ctx.read().unwrap(),
                    base: VfsBaseDirAsync::Cwd,
                    path: VfsPath::new(path.as_bytes()),
                    flags,
                })
                .await
                .map_err(|_| Errno::Io)?;
            parent
                .dir
                .node
                .create_file(&parent.name, vfs_core::node::CreateFile::default())
                .await
                .map_err(|_| Errno::Io)?
        }
        Err(_) => return Err(Errno::Io),
    };

    let handle = node
        .open(vfs_core::flags::OpenOptions {
            flags: vfs_core::flags::OpenFlags::WRITE
                | vfs_core::flags::OpenFlags::CREATE
                | vfs_core::flags::OpenFlags::TRUNC,
            mode: Some(0o755),
            resolve: vfs_core::flags::ResolveFlags::empty(),
        })
        .await
        .map_err(|_| Errno::Io)?;
    let _ = handle.write_at(0, bytes).await.map_err(|_| Errno::Io)?;
    Ok(())
}
