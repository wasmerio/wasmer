use std::{collections::VecDeque, sync::Barrier, thread};

use futures::executor::block_on;
use wasixv2_abi::OpenFlags;
use wasixv2_vfs::{
    Cred, Ctx, File, LookupFlags, MountFlags, NewKind, OpenHow, Path, Timestamp, Vfs,
};
use wasixv2_vfs_overlay::OverlayFs;

use super::*;
use protocol::Entry;

struct FixedClock;

impl Clock for FixedClock {
    fn now(&self) -> Timestamp {
        Timestamp::new(1_700_000_000, 0)
    }
}

fn backing() -> Arc<MemFs> {
    Arc::new(MemFs::new(Arc::new(FixedClock), 0o777, 1, 1))
}

async fn mount() -> Arc<WebFs> {
    memory(Arc::new(FixedClock), 0o777, 1, 1).await.unwrap()
}

fn new(kind: NewKind<'_>) -> NewNode<'_> {
    NewNode {
        kind,
        mode: 0o644,
        uid: 1,
        gid: 1,
        task: 0,
        quota_exempt: false,
    }
}

struct Client {
    vfs: Arc<Vfs>,
    root: Path,
    cred: Cred,
}

impl Client {
    fn new(fs: Arc<dyn Filesystem>) -> Self {
        let vfs = Vfs::new(fs);
        let root = vfs.root();
        Self {
            vfs,
            root,
            cred: Cred::user(1, 1),
        }
    }

    fn cx(&self) -> Ctx<'_> {
        Ctx {
            cred: &self.cred,
            root: &self.root,
            umask: 0,
            task: 0,
            leases: None,
        }
    }

    async fn open(&self, path: &[u8], flags: OpenFlags) -> FsResult<File> {
        self.vfs
            .open(
                &self.cx(),
                &self.root,
                path,
                &OpenHow {
                    flags,
                    mode: 0o644,
                    ..OpenHow::default()
                },
            )
            .await
    }

    async fn lookup(&self, path: &[u8], lookup: LookupFlags) -> FsResult<Path> {
        self.vfs.lookup(&self.cx(), &self.root, path, lookup).await
    }
}

const CREATE: OpenFlags = OpenFlags(OpenFlags::O_CREAT.0 | OpenFlags::O_RDWR.0);

#[test]
fn shared_mount_revalidates_embedder_rename_unlink_and_replacement() {
    block_on(async {
        let fs = mount().await;
        assert!(!fs.info().stable);
        let guest = Client::new(fs.clone());
        let embedder = Client::new(fs.clone());
        let created = embedder.open(b"original", CREATE).await.unwrap();
        assert_eq!(created.write_at(b"shared", 0).await, Ok(6));
        let held = guest.open(b"original", OpenFlags::O_RDWR).await.unwrap();
        let ino = held.node().ino();
        embedder
            .vfs
            .rename(
                &embedder.cx(),
                &embedder.root,
                b"original",
                &embedder.root,
                b"moved",
                RenameMode::Replace,
            )
            .await
            .unwrap();
        assert_eq!(
            guest.lookup(b"original", LookupFlags::follow()).await.err(),
            Some(Errno::ENOENT)
        );
        assert_eq!(
            guest
                .lookup(b"moved", LookupFlags::follow())
                .await
                .unwrap()
                .dentry
                .node()
                .ino(),
            ino
        );
        embedder
            .vfs
            .unlink(&embedder.cx(), &embedder.root, b"moved")
            .await
            .unwrap();
        let replacement = embedder.open(b"moved", CREATE).await.unwrap();
        assert_ne!(replacement.node().ino(), ino);
        assert_eq!(
            guest
                .lookup(b"moved", LookupFlags::follow())
                .await
                .unwrap()
                .dentry
                .node()
                .ino(),
            replacement.node().ino()
        );
        assert_eq!(held.node().getattr().await.unwrap().nlink, 0);
        assert_eq!(held.write_at(b"!", 6).await, Ok(1));
        let mut bytes = [0; 7];
        assert_eq!(created.read_at(&mut bytes, 0).await, Ok(7));
        assert_eq!(&bytes, b"shared!");
        drop(guest);
        drop(held);
        drop(created);
        drop(embedder);
        // A fresh process/client still sees data held by the shared directory itself.
        let later = Client::new(fs);
        assert!(later.open(b"moved", OpenFlags::O_RDONLY).await.is_ok());
    });
}

#[test]
fn existing_mount_composition_and_openat2_scope_use_vfs() {
    block_on(async {
        let guest = Client::new(backing());
        let fs = mount().await;
        guest
            .vfs
            .mount(b"/mount", fs.clone(), MountFlags::default(), true)
            .await
            .unwrap();
        let embedder = Client::new(fs);
        let file = embedder.open(b"inside", CREATE).await.unwrap();
        file.write_at(b"ok", 0).await.unwrap();
        embedder
            .vfs
            .symlink(&embedder.cx(), b"../outside", &embedder.root, b"escape")
            .await
            .unwrap();
        guest.open(b"outside", CREATE).await.unwrap();
        let start = guest
            .lookup(b"/mount", LookupFlags::follow())
            .await
            .unwrap();
        let cx = guest.cx();
        let scoped = |flags| guest.vfs.lookup(&cx, &start, b"escape", flags);
        // Linux fs/namei.c:handle_dots and pick_link: no provider gets a host path.
        assert_eq!(
            scoped(LookupFlags {
                follow: true,
                beneath: true,
                ..LookupFlags::default()
            })
            .await
            .err(),
            Some(Errno::EXDEV)
        );
        assert_eq!(
            scoped(LookupFlags {
                follow: true,
                in_root: true,
                ..LookupFlags::default()
            })
            .await
            .err(),
            Some(Errno::ENOENT)
        );
        assert_eq!(
            scoped(LookupFlags {
                follow: true,
                no_symlinks: true,
                ..LookupFlags::default()
            })
            .await
            .err(),
            Some(Errno::ELOOP)
        );
        let mut bytes = [0; 2];
        let file = guest
            .open(b"/mount/inside", OpenFlags::O_RDONLY)
            .await
            .unwrap();
        assert_eq!(file.read_at(&mut bytes, 0).await, Ok(2));
        assert_eq!(&bytes, b"ok");
    });
}

#[test]
fn vfs_permissions_and_readonly_errors_are_preserved() {
    block_on(async {
        let source = backing();
        let fs = LocalProvider::mount(source.clone(), MEMORY_CAPABILITIES)
            .await
            .unwrap();
        let owner = Client::new(fs.clone());
        let file = owner.open(b"private", CREATE).await.unwrap();
        owner
            .vfs
            .chmod(&owner.cx(), &file.path, 0o600)
            .await
            .unwrap();
        let mut other = Client::new(fs);
        other.cred = Cred::user(2, 2);
        assert_eq!(
            other.open(b"private", OpenFlags::O_RDONLY).await.err(),
            Some(Errno::EACCES)
        );
        let mut info = source.info();
        info.read_only = true;
        let ro = WebFs::new(LocalProvider::new(source), info, MEMORY_CAPABILITIES)
            .await
            .unwrap();
        let reader = Client::new(ro.clone());
        assert_eq!(reader.open(b"new", CREATE).await.err(), Some(Errno::EROFS));
        assert_eq!(
            reader.open(b"private", OpenFlags::O_WRONLY).await.err(),
            Some(Errno::EROFS)
        );
        assert_eq!(
            ro.root()
                .create(b"direct", &new(NewKind::Regular))
                .await
                .err(),
            Some(Errno::EROFS)
        );
        let node = ro.root().lookup(b"private").await.unwrap();
        let handle = node.open(OpenOptions::default()).await.unwrap();
        assert_eq!(handle.write_at(b"x", 0).await, Err(Errno::EROFS));
        assert_eq!(handle.set_size(1).await, Err(Errno::EROFS));
        assert_eq!(
            node.setattr(&SetAttr {
                mode: Some(0o777),
                ..SetAttr::default()
            })
            .await
            .err(),
            Some(Errno::EROFS)
        );
    });
}

#[test]
fn embedder_acl_changes_invalidate_a_guests_existing_inode_cache() {
    use wasixv2_vfs::{AclEntry, PosixAcl, XATTR_NAME_POSIX_ACL_ACCESS};

    block_on(async {
        let fs = mount().await;
        let owner = Client::new(fs.clone());
        let file = owner.open(b"acl", CREATE).await.unwrap();
        owner
            .vfs
            .chmod(&owner.cx(), &file.path, 0o640)
            .await
            .unwrap();
        let mut guest = Client::new(fs);
        guest.cred = Cred::user(2, 2);
        // Retain the guest inode across failed opens, which cache the absent ACL.
        let held = guest.lookup(b"acl", LookupFlags::follow()).await.unwrap();
        assert_eq!(
            guest.open(b"acl", OpenFlags::O_RDONLY).await.err(),
            Some(Errno::EACCES)
        );
        let acl = |perm| {
            PosixAcl(vec![
                AclEntry {
                    tag: 1,
                    perm: 6,
                    id: u32::MAX,
                }, // ACL_USER_OBJ
                AclEntry {
                    tag: 2,
                    perm,
                    id: 2,
                }, // ACL_USER
                AclEntry {
                    tag: 4,
                    perm: 4,
                    id: u32::MAX,
                }, // ACL_GROUP_OBJ
                AclEntry {
                    tag: 16,
                    perm: 4,
                    id: u32::MAX,
                }, // ACL_MASK
                AclEntry {
                    tag: 32,
                    perm: 0,
                    id: u32::MAX,
                }, // ACL_OTHER
            ])
            .to_stored()
        };
        file.node()
            .setxattr(XATTR_NAME_POSIX_ACL_ACCESS, &acl(4), XattrSet::Any)
            .await
            .unwrap();
        assert!(guest.open(b"acl", OpenFlags::O_RDONLY).await.is_ok());
        file.node()
            .setxattr(XATTR_NAME_POSIX_ACL_ACCESS, &acl(0), XattrSet::Any)
            .await
            .unwrap();
        assert_eq!(
            guest.open(b"acl", OpenFlags::O_RDONLY).await.err(),
            Some(Errno::EACCES)
        );
        assert_eq!(held.dentry.node().ino(), file.node().ino());
    });
}

struct Names(Vec<(Vec<u8>, u64)>, usize);

impl DirSink for Names {
    fn push(&mut self, entry: DirEntry<'_>) -> bool {
        if self.0.len() == self.1 {
            return false;
        }
        self.0.push((entry.name.to_vec(), entry.next));
        true
    }
}

#[test]
fn readdir_batches_resume_after_embedder_changes_and_keep_byte_names() {
    block_on(async {
        let fs = mount().await;
        let root = fs.root();
        for i in 0..130 {
            root.create(format!("f{i:03}").as_bytes(), &new(NewKind::Regular))
                .await
                .unwrap();
        }
        root.create(b"\xff", &new(NewKind::Regular)).await.unwrap();
        let dir = root.open(OpenOptions::default()).await.unwrap();
        let mut first = Names(Vec::new(), 1);
        dir.readdir(0, &mut first).await.unwrap();
        assert_eq!(first.0[0].0, b"f000");
        root.unlink(b"f001").await.unwrap();
        root.rename(b"f002", &root, b"renamed", RenameMode::Replace)
            .await
            .unwrap();
        let mut rest = Names(Vec::new(), usize::MAX);
        dir.readdir(first.0[0].1, &mut rest).await.unwrap();
        assert_eq!(rest.0.len(), 129);
        assert!(
            !rest
                .0
                .iter()
                .any(|(name, _)| name == b"f001" || name == b"f002")
        );
        assert!(rest.0.iter().any(|(name, _)| name == b"renamed"));
        assert!(rest.0.iter().any(|(name, _)| name == b"\xff"));
        assert!(rest.0.windows(2).all(|entries| entries[0].1 < entries[1].1));
    });
}

#[test]
fn independent_clients_append_atomically() {
    let fs = block_on(mount());
    let node = block_on(fs.root().create(b"log", &new(NewKind::Regular))).unwrap();
    let barrier = Arc::new(Barrier::new(4));
    let threads: Vec<_> = (0..4u8)
        .map(|writer| {
            let fs = fs.clone();
            let barrier = barrier.clone();
            thread::spawn(move || {
                let client = Client::new(fs);
                let file = block_on(client.open(b"log", OpenFlags::O_WRONLY)).unwrap();
                barrier.wait();
                (0..32)
                    .map(|_| block_on(file.append(&[writer; 32])).unwrap().0)
                    .collect::<Vec<_>>()
            })
        })
        .collect();
    let mut ends: Vec<_> = threads
        .into_iter()
        .flat_map(|thread| thread.join().unwrap())
        .collect();
    ends.sort_unstable();
    // VFS File::append returns the end, whereas the provider returns the start offset.
    assert_eq!(ends, (1..=128).map(|i| i * 32).collect::<Vec<_>>());
    let handle = block_on(node.open(OpenOptions::default())).unwrap();
    let mut bytes = vec![0; 128 * 32];
    assert_eq!(block_on(handle.read_at(&mut bytes, 0)), Ok(bytes.len()));
    assert!(
        bytes
            .chunks_exact(32)
            .all(|record| record.iter().all(|byte| *byte == record[0]))
    );
    for writer in 0..4 {
        assert_eq!(
            bytes.iter().filter(|&&byte| byte == writer).count(),
            32 * 32
        );
    }
}

#[test]
fn last_close_releases_leases_and_stale_ids_do_not_alias_new_objects() {
    block_on(async {
        let source = backing();
        let provider = LocalProvider::new(source.clone());
        let fs = WebFs::new(provider.clone(), source.info(), MEMORY_CAPABILITIES)
            .await
            .unwrap();
        let root = fs.root();
        let (node, handle) = root
            .create_open(
                b"f",
                &new(NewKind::Regular),
                OpenOptions {
                    write: true,
                    ..OpenOptions::default()
                },
            )
            .await
            .unwrap();
        assert_eq!(provider.retained(), (2, 1));
        let old = node.as_any().downcast_ref::<WebNode>().unwrap().id();
        let same = root.lookup(b"f").await.unwrap();
        assert!(Arc::ptr_eq(&node, &same));
        assert_eq!(provider.retained(), (2, 1));
        root.unlink(b"f").await.unwrap();
        drop(node);
        drop(same);
        assert_eq!(provider.retained(), (2, 1));
        assert_eq!(handle.write_at(b"held", 0).await, Ok(4));
        drop(handle);
        assert_eq!(provider.retained(), (1, 0));
        assert_eq!(
            provider.call(Request::GetAttr(old)).await.err(),
            Some(Errno::ESTALE)
        );
        let replacement = root.create(b"f", &new(NewKind::Regular)).await.unwrap();
        assert_ne!(
            replacement.as_any().downcast_ref::<WebNode>().unwrap().id(),
            old
        );
        drop(replacement);
        drop(root);
        drop(fs);
        assert_eq!(provider.retained(), (0, 0));
    });
}

#[test]
fn directory_mutations_recheck_removed_handles_types_and_ancestry() {
    block_on(async {
        let root = mount().await.root();
        let a = root
            .create(
                b"a",
                &NewNode {
                    mode: 0o777,
                    ..new(NewKind::Directory)
                },
            )
            .await
            .unwrap();
        let sub = a.create(b"sub", &new(NewKind::Directory)).await.unwrap();
        assert_eq!(
            root.rename(b"a", &sub, b"cycle", RenameMode::Replace).await,
            Err(Errno::EINVAL)
        );
        let file = root.create(b"f", &new(NewKind::Regular)).await.unwrap();
        assert_eq!(
            root.rename(b"f", &root, b"a", RenameMode::Replace).await,
            Err(Errno::EISDIR)
        );
        assert_eq!(
            root.rename(b"a", &root, b"f", RenameMode::Replace).await,
            Err(Errno::ENOTDIR)
        );
        a.rmdir(b"sub").await.unwrap();
        assert_eq!(
            sub.create(b"orphan", &new(NewKind::Regular)).await.err(),
            Some(Errno::ENOENT)
        );
        root.rmdir(b"a").await.unwrap();
        assert_eq!(a.link(b"orphan", &file).await, Err(Errno::ENOENT));
        assert_eq!(root.link(b"directory", &a).await, Err(Errno::EPERM));
    });
}

#[test]
fn sparse_io_truncation_unnamed_files_hardlinks_and_xattrs_delegate() {
    block_on(async {
        let fs = mount().await;
        let root = fs.root();
        let (node, handle) = root
            .create_unnamed(
                &new(NewKind::Regular),
                OpenOptions {
                    write: true,
                    ..OpenOptions::default()
                },
            )
            .await
            .unwrap();
        assert_eq!(node.getattr().await.unwrap().nlink, 0);
        root.link(b"linked", &node).await.unwrap();
        assert_eq!(root.lookup(b"linked").await.unwrap().ino(), node.ino());
        assert_eq!(handle.write_at(b"data", 1 << 33).await, Ok(4));
        let mut hole = [1; 4];
        assert_eq!(handle.read_at(&mut hole, 1 << 32).await, Ok(4));
        assert_eq!(hole, [0; 4]);
        assert_eq!(
            handle.seek_hole(0, (1 << 33) + 4, SeekHole::Data).await,
            Ok(1 << 33)
        );
        handle.set_size(2).await.unwrap();
        assert_eq!(node.getattr().await.unwrap().size, 2);
        assert_eq!(handle.write_at(&vec![1; MAX_IO + 1], 0).await, Ok(MAX_IO));
        let mut bytes = vec![0; MAX_IO + 1];
        assert_eq!(handle.read_at(&mut bytes, 0).await, Ok(MAX_IO));
        node.setxattr(b"user.a", b"v", XattrSet::Create)
            .await
            .unwrap();
        assert_eq!(node.getxattr(b"user.a").await, Ok(b"v".to_vec()));
        assert_eq!(node.listxattr().await, Ok(vec![b"user.a".to_vec()]));
        node.removexattr(b"user.a").await.unwrap();
        assert_eq!(node.getxattr(b"user.a").await, Err(Errno::ENODATA));
    });
}

#[test]
fn existing_overlay_supplies_copy_on_write_without_new_storage() {
    block_on(async {
        let lower = backing();
        let original = lower
            .root()
            .create(b"package", &new(NewKind::Regular))
            .await
            .unwrap();
        let handle = original.open(OpenOptions::default()).await.unwrap();
        handle.write_at(b"lower", 0).await.unwrap();
        let overlay = OverlayFs::new(Some(backing()), vec![lower]);
        let fs = LocalProvider::mount(overlay, MEMORY_CAPABILITIES)
            .await
            .unwrap();
        let client = Client::new(fs);
        let file = client.open(b"package", OpenFlags::O_RDWR).await.unwrap();
        file.write_at(b"upper", 0).await.unwrap();
        let mut bytes = [0; 5];
        file.read_at(&mut bytes, 0).await.unwrap();
        assert_eq!(&bytes, b"upper");
        handle.read_at(&mut bytes, 0).await.unwrap();
        assert_eq!(&bytes, b"lower");
    });
}

#[test]
fn removed_lower_overlay_directory_cannot_be_repopulated_through_a_held_node() {
    block_on(async {
        for replace in [false, true] {
            let lower = backing();
            lower
                .root()
                .create(b"gone", &new(NewKind::Directory))
                .await
                .unwrap();
            let overlay = OverlayFs::new(Some(backing()), vec![lower]);
            let info = overlay.info();
            let provider = LocalProvider::new(overlay);
            let fs = WebFs::new(provider.clone(), info, MEMORY_CAPABILITIES)
                .await
                .unwrap();
            let root = fs.root();
            let held = root.lookup(b"gone").await.unwrap();
            if replace {
                root.create(b"replacement", &new(NewKind::Directory))
                    .await
                    .unwrap();
                root.rename(b"replacement", &root, b"gone", RenameMode::Replace)
                    .await
                    .unwrap();
            } else {
                root.rmdir(b"gone").await.unwrap();
            }
            assert_eq!(provider.removed_directories(), 1);
            assert_eq!(
                held.create(b"resurrected", &new(NewKind::Regular))
                    .await
                    .err(),
                Some(Errno::ENOENT)
            );
            if replace {
                assert_ne!(root.lookup(b"gone").await.unwrap().ino(), held.ino());
            } else {
                assert_eq!(root.lookup(b"gone").await.err(), Some(Errno::ENOENT));
            }
            drop(held);
            assert_eq!(provider.removed_directories(), 0);
        }
    });
}

struct Scripted {
    replies: Mutex<VecDeque<FsResult<Reply>>>,
    releases: Mutex<Vec<Resource>>,
}

#[async_trait(?Send)]
impl Provider for Scripted {
    async fn call(&self, _request: Request) -> FsResult<Reply> {
        self.replies
            .lock()
            .unwrap()
            .pop_front()
            .expect("unexpected provider request")
    }
    fn release(&self, resource: Resource) {
        self.releases.lock().unwrap().push(resource);
    }
    fn acl_version(&self) -> u64 {
        0
    }
}

fn reference(id: u64, ino: u64, file_type: FileType) -> NodeRef {
    NodeRef {
        id: ObjectId(id),
        ino,
        generation: 0,
        file_type,
    }
}

fn scripted(replies: Vec<FsResult<Reply>>, caps: Capabilities) -> (Arc<Scripted>, Arc<WebFs>) {
    let provider = Arc::new(Scripted {
        replies: Mutex::new(replies.into()),
        releases: Mutex::new(Vec::new()),
    });
    let fs = block_on(WebFs::new(provider.clone(), backing().info(), caps)).unwrap();
    (provider, fs)
}

#[test]
fn provider_capability_limits_and_components_fail_before_dispatch() {
    let (_provider, fs) = scripted(
        vec![Ok(Reply::Node(reference(1, 1, FileType::Directory)))],
        Capabilities::default(),
    );
    let root = fs.root();
    for name in [b"".as_slice(), b".", b"..", b"a/b", b"a\0b"] {
        assert_eq!(block_on(root.lookup(name)).err(), Some(Errno::EINVAL));
    }
    assert_eq!(
        block_on(root.lookup(&[b'a'; NAME_MAX + 1])).err(),
        Some(Errno::ENAMETOOLONG)
    );
    assert_eq!(
        block_on(root.create(b"symlink", &new(NewKind::Symlink(b"target")))).err(),
        Some(Errno::EOPNOTSUPP)
    );
    assert_eq!(
        block_on(root.rename(b"a", &root, b"b", RenameMode::Exchange)),
        Err(Errno::EINVAL)
    );
    assert_eq!(
        block_on(root.create_unnamed(&new(NewKind::Regular), OpenOptions::default())).err(),
        Some(Errno::EOPNOTSUPP)
    );
    assert_eq!(block_on(root.getxattr(b"user.a")), Err(Errno::EOPNOTSUPP));
}

#[test]
fn malformed_replies_release_leases_and_reject_reused_live_inode_identity() {
    let (provider, fs) = scripted(
        vec![
            Ok(Reply::Node(reference(1, 1, FileType::Directory))),
            Ok(Reply::Node(reference(2, 2, FileType::Regular))),
            Ok(Reply::Node(NodeRef {
                generation: 1,
                ..reference(3, 2, FileType::Regular)
            })),
            Ok(Reply::Open(ObjectId(4))), // Wrong response tag for lookup.
            Err(Errno::EIO),
        ],
        Capabilities::default(),
    );
    let held = block_on(fs.root().lookup(b"f")).unwrap();
    assert_eq!(
        block_on(fs.root().lookup(b"recycled")).err(),
        Some(Errno::EIO)
    );
    assert_eq!(
        block_on(fs.root().lookup(b"wrong-tag")).err(),
        Some(Errno::EIO)
    );
    assert_eq!(
        block_on(fs.root().lookup(b"disconnected")).err(),
        Some(Errno::EIO)
    );
    assert_eq!(
        *provider.releases.lock().unwrap(),
        vec![Resource::Node(ObjectId(3)), Resource::Handle(ObjectId(4))]
    );
    drop(held);
    drop(fs);
    assert_eq!(provider.releases.lock().unwrap().len(), 4);
}

#[test]
fn malformed_io_counts_and_directory_records_are_not_exposed() {
    let (_provider, fs) = scripted(
        vec![
            Ok(Reply::Node(reference(1, 1, FileType::Directory))),
            Ok(Reply::Open(ObjectId(2))),
            Ok(Reply::Bytes(vec![1, 2])),
            Ok(Reply::Count(2)),
            Ok(Reply::Appended(u64::MAX, 1)),
            Ok(Reply::Entries(vec![Entry {
                name: b"..".to_vec(),
                ino: 1,
                file_type: FileType::Directory,
                next: 1,
            }])),
        ],
        MEMORY_CAPABILITIES,
    );
    let handle = block_on(fs.root().open(OpenOptions::default())).unwrap();
    let mut buf = [0];
    assert_eq!(block_on(handle.read_at(&mut buf, 0)), Err(Errno::EIO));
    assert_eq!(buf, [0]);
    assert_eq!(block_on(handle.write_at(b"x", 0)), Err(Errno::EIO));
    assert_eq!(block_on(handle.append(b"x")), Err(Errno::EIO));
    let mut names = Names(Vec::new(), 10);
    assert_eq!(block_on(handle.readdir(0, &mut names)), Err(Errno::EIO));
    assert!(names.0.is_empty());
}

#[test]
fn provider_directory_cookies_are_opaque_and_need_not_be_ordered() {
    let (_provider, fs) = scripted(
        vec![
            Ok(Reply::Node(reference(1, 1, FileType::Directory))),
            Ok(Reply::Open(ObjectId(2))),
            Ok(Reply::Entries(vec![
                Entry {
                    name: b"a".to_vec(),
                    ino: 2,
                    file_type: FileType::Regular,
                    next: 50,
                },
                Entry {
                    name: b"b".to_vec(),
                    ino: 3,
                    file_type: FileType::Regular,
                    next: 7,
                },
            ])),
            Ok(Reply::Entries(Vec::new())),
        ],
        Capabilities::default(),
    );
    let handle = block_on(fs.root().open(OpenOptions::default())).unwrap();
    let mut names = Names(Vec::new(), 10);
    block_on(handle.readdir(0, &mut names)).unwrap();
    assert_eq!(names.0, vec![(b"a".to_vec(), 50), (b"b".to_vec(), 7)]);
}
