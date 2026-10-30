//! Byte-backed directory snapshots for embedders. All path, permission and mode
//! operations still go through VFS (`fs/namei.c:link_path_walk`, `fs/attr.c:notify_change`).
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, sync::Arc};
use wasixv2_abi::{Errno, OpenFlags};
use wasixv2_vfs::{Cred, Ctx, Filesystem, FsResult, LookupFlags, OpenHow, Vfs};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "lowercase", deny_unknown_fields)]
pub enum DirectoryEntry {
    Directory {
        path: String,
        mode: u32,
    },
    File {
        path: String,
        mode: u32,
        data: Vec<u8>,
    },
    Symlink {
        path: String,
        mode: u32,
        target: String,
    },
}
impl DirectoryEntry {
    fn path(&self) -> &str {
        match self {
            Self::Directory { path, .. } | Self::File { path, .. } | Self::Symlink { path, .. } => {
                path
            }
        }
    }
    fn mode(&self) -> u32 {
        match self {
            Self::Directory { mode, .. } | Self::File { mode, .. } | Self::Symlink { mode, .. } => {
                *mode
            }
        }
    }
}

/// Populate a fresh private mount, before sharing it with a Machine. Parents must
/// be declared directories; symlinks are preserved without following their targets.
/// Invalid paths/ancestry are rejected before mutation. Failure during filesystem
/// I/O leaves the private mount partially populated, so discard it on error.
pub async fn populate_directory(
    fs: Arc<dyn Filesystem>,
    entries: &[DirectoryEntry],
    uid: u32,
    gid: u32,
) -> FsResult<()> {
    let mut names = BTreeMap::new();
    for entry in entries {
        let path = entry.path();
        if entry.mode() & !0o7777 != 0
            || path.as_bytes().contains(&0)
            || (!path.is_empty() && path.split('/').any(|c| matches!(c, "" | "." | "..")))
            || names.insert(path, entry).is_some()
        {
            return Err(Errno::EINVAL);
        }
    }
    if !matches!(names.get(""), Some(DirectoryEntry::Directory { .. })) {
        return Err(Errno::EINVAL);
    }
    for path in names.keys().filter(|p| !p.is_empty()) {
        let parent = path.rsplit_once('/').map_or("", |(p, _)| p);
        if !matches!(names.get(parent), Some(DirectoryEntry::Directory { .. })) {
            return Err(Errno::EINVAL);
        }
    }
    let vfs = Vfs::new(fs);
    let root = vfs.root();
    // Trusted materialization acts as the requested owner with DAC capabilities;
    // modes are applied after the bytes, including readonly parent directories.
    let cred = Cred {
        fsuid: uid,
        fsgid: gid,
        ..Cred::root()
    };
    let cx = Ctx {
        cred: &cred,
        root: &root,
        umask: 0,
        task: 0,
        leases: None,
    };
    let how = OpenHow {
        flags: OpenFlags::O_CREAT | OpenFlags::O_EXCL | OpenFlags::O_WRONLY | OpenFlags::O_NOFOLLOW,
        mode: 0o600,
        lookup: LookupFlags {
            no_symlinks: true,
            beneath: true,
            ..LookupFlags::default()
        },
    };
    // Lexical order visits every declared parent before its children.
    for (path, entry) in &names {
        if path.is_empty() {
            continue;
        }
        match entry {
            DirectoryEntry::Directory { .. } => {
                vfs.mkdir(&cx, &root, path.as_bytes(), 0o700).await?
            }
            DirectoryEntry::File { data, mode, .. } => {
                let file = vfs.open(&cx, &root, path.as_bytes(), &how).await?;
                let mut offset = 0;
                while offset < data.len() {
                    let n = file.write_at(&data[offset..], offset as u64).await?;
                    if n == 0 {
                        return Err(Errno::EIO);
                    }
                    offset += n;
                }
                vfs.chmod(&cx, &file.path, *mode).await?;
            }
            DirectoryEntry::Symlink { target, .. } => {
                vfs.symlink(&cx, target.as_bytes(), &root, path.as_bytes())
                    .await?
            }
        }
    }
    for (path, entry) in names.iter().rev() {
        if let DirectoryEntry::Directory { mode, .. } = entry {
            let directory = if path.is_empty() {
                root.clone()
            } else {
                vfs.lookup(
                    &cx,
                    &root,
                    path.as_bytes(),
                    LookupFlags {
                        no_symlinks: true,
                        beneath: true,
                        ..LookupFlags::follow()
                    },
                )
                .await?
            };
            vfs.chmod(&cx, &directory, *mode).await?;
        }
    }
    Ok(())
}

#[cfg(all(test, feature = "tokio", not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use wasixv2_kernel::HostClock;
    use wasixv2_vfs_mem::MemFs;
    fn memory() -> Arc<dyn Filesystem> {
        Arc::new(MemFs::new(
            Arc::new(HostClock(Arc::new(crate::TokioHost::default()))),
            0o700,
            42,
            43,
        ))
    }
    #[tokio::test]
    async fn readonly_directories_bytes_and_symlinks_keep_their_identity() {
        let fs = memory();
        let entries: Vec<DirectoryEntry> = serde_json::from_value(serde_json::json!([
            {"path":"","kind":"directory","mode":320},
            {"path":"d","kind":"directory","mode":320},
            {"path":"d/file","kind":"file","mode":256,"data":[97,98,99]},
            {"path":"link","kind":"symlink","mode":511,"target":"d/file"}
        ]))
        .unwrap();
        populate_directory(fs.clone(), &entries, 42, 43)
            .await
            .unwrap();
        let vfs = Vfs::new(fs);
        let root = vfs.root();
        let cred = Cred::user(42, 43);
        let cx = Ctx {
            cred: &cred,
            root: &root,
            umask: 0,
            task: 0,
            leases: None,
        };
        let file = vfs
            .open(
                &cx,
                &root,
                b"link",
                &OpenHow {
                    flags: OpenFlags::O_RDONLY,
                    mode: 0,
                    lookup: LookupFlags::follow(),
                },
            )
            .await
            .unwrap();
        let mut bytes = [0; 3];
        assert_eq!(file.read_at(&mut bytes, 0).await.unwrap(), 3);
        assert_eq!(&bytes, b"abc");
        assert_eq!(
            root.dentry.node().getattr().await.unwrap().mode & 0o777,
            0o500
        );
        let attr = file.path.dentry.node().getattr().await.unwrap();
        assert_eq!((attr.uid, attr.gid, attr.mode & 0o777), (42, 43, 0o400));
    }
    #[tokio::test]
    async fn escaping_paths_and_symlink_parents_fail_before_mutation() {
        for invalid in ["../escape", "/absolute", "d//f", "d/./f", "d/../f"] {
            let fs = memory();
            let entries = [
                DirectoryEntry::Directory {
                    path: "".into(),
                    mode: 0o700,
                },
                DirectoryEntry::File {
                    path: invalid.into(),
                    mode: 0o600,
                    data: vec![],
                },
            ];
            assert_eq!(
                populate_directory(fs.clone(), &entries, 42, 43).await,
                Err(Errno::EINVAL)
            );
            assert_eq!(fs.root().lookup(b"d").await.err(), Some(Errno::ENOENT));
        }
        let entries = [
            DirectoryEntry::Directory {
                path: "".into(),
                mode: 0o700,
            },
            DirectoryEntry::Symlink {
                path: "d".into(),
                mode: 0o777,
                target: "/".into(),
            },
            DirectoryEntry::File {
                path: "d/escape".into(),
                mode: 0o600,
                data: vec![],
            },
        ];
        assert_eq!(
            populate_directory(memory(), &entries, 42, 43).await,
            Err(Errno::EINVAL)
        );
    }
}
