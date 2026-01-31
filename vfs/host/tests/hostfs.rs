use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use vfs_core::VfsBaseDir;
use vfs_core::flags::{OpenFlags, OpenOptions};
use vfs_core::mount::MountTable;
use vfs_core::node::{CreateFile, MkdirOptions, RenameOptions};
use vfs_core::path_types::{VfsName, VfsPath};
use vfs_core::path_walker::{PathWalker, ResolutionRequest};
use vfs_core::provider::AsyncFsFromSync;
use vfs_core::provider::{MountFlags, MountRequest};
use vfs_core::{
    AllowAllPolicy, FsNodeSync, FsProviderRegistry, MountId, VfsConfig, VfsContext, VfsDirHandle,
    VfsFileType, WalkFlags, make_vfs_inode,
};
use vfs_rt::InlineTestRuntime;

use vfs_host::{HostFsConfig, HostFsProvider};

struct TempDir {
    path: std::path::PathBuf,
}

impl TempDir {
    fn new(prefix: &str) -> Self {
        let mut base = std::env::temp_dir();
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        base.push(format!(
            "wasmer-hostfs-{prefix}-{stamp}-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&base).expect("create temp dir");
        Self { path: base }
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

fn mount_host(temp: &TempDir) -> Arc<dyn vfs_core::traits_sync::FsSync> {
    let provider = Arc::new(HostFsProvider);
    let registry = FsProviderRegistry::new();
    registry.register(provider).expect("register host provider");
    let cfg = HostFsConfig {
        root: temp.path.clone(),
        strict: true,
    };
    registry
        .create_fs_sync(
            "host",
            MountRequest {
                target_path: VfsPath::new(b"/"),
                flags: MountFlags::empty(),
                config: &cfg,
            },
        )
        .expect("mount hostfs")
}

fn make_dir_handle(
    mount_table: &MountTable,
    mount: MountId,
    node: Arc<dyn FsNodeSync>,
    parent: Option<vfs_core::NodeRef>,
    id: u64,
) -> VfsDirHandle {
    let guard = mount_table.guard(mount).expect("mount guard");
    VfsDirHandle::new(
        vfs_core::VfsHandleId(id),
        guard,
        make_vfs_inode(mount, node.inode()),
        node,
        parent,
    )
}

#[test]
fn provider_mount_smoke() {
    let temp = TempDir::new("smoke");
    let fs = mount_host(&temp);
    let root = fs.root();
    assert_eq!(root.file_type(), VfsFileType::Directory);
}

#[test]
fn file_crud_roundtrip() {
    let temp = TempDir::new("crud");
    let fs = mount_host(&temp);
    let root = fs.root();
    let name = VfsName::new(b"file").expect("name");
    let node = root
        .create_file(
            &name,
            CreateFile {
                truncate: true,
                ..Default::default()
            },
        )
        .expect("create file");
    let handle = node
        .open(OpenOptions {
            flags: OpenFlags::READ | OpenFlags::WRITE,
            mode: None,
            resolve: vfs_core::flags::ResolveFlags::empty(),
        })
        .expect("open file");
    handle.write_at(0, b"hello").expect("write");
    let mut buf = [0u8; 5];
    let read = handle.read_at(0, &mut buf).expect("read");
    assert_eq!(read, 5);
    assert_eq!(&buf, b"hello");
    let meta = node.metadata().expect("metadata");
    assert_eq!(meta.size, 5);
}

#[test]
fn mkdir_and_readdir() {
    let temp = TempDir::new("readdir");
    let fs = mount_host(&temp);
    let root = fs.root();
    let name = VfsName::new(b"dir").expect("name");
    root.mkdir(&name, MkdirOptions::default()).expect("mkdir");
    let batch = root.read_dir(None, 128).expect("read_dir");
    let names: Vec<_> = batch.entries.iter().map(|e| e.name.as_bytes()).collect();
    assert!(names.iter().any(|n| n == &b"dir".as_slice()));
}

#[test]
fn rename_moves_entry() {
    let temp = TempDir::new("rename");
    let fs = mount_host(&temp);
    let root = fs.root();
    let old = VfsName::new(b"a").expect("name");
    let new = VfsName::new(b"b").expect("name");
    root.create_file(&old, CreateFile::default())
        .expect("create file");
    root.rename(&old, root.as_ref(), &new, RenameOptions::default())
        .expect("rename");
    let err = match root.lookup(&old) {
        Ok(_) => panic!("old should be gone"),
        Err(err) => err,
    };
    assert_eq!(err.kind(), vfs_core::VfsErrorKind::NotFound);
    let node = root.lookup(&new).expect("new should exist");
    assert_eq!(node.file_type(), VfsFileType::RegularFile);
}

#[cfg(unix)]
#[test]
fn symlink_and_readlink() {
    let temp = TempDir::new("symlink");
    let fs = mount_host(&temp);
    let root = fs.root();
    let link = VfsName::new(b"link").expect("name");
    root.symlink(&link, VfsPath::new(b"target"))
        .expect("symlink");
    let node = root.lookup(&link).expect("lookup link");
    assert_eq!(node.file_type(), VfsFileType::Symlink);
    let target = node.readlink().expect("readlink");
    assert_eq!(target.as_bytes(), b"target");
}

#[cfg(unix)]
#[test]
fn resolve_beneath_blocks_escape() {
    let temp = TempDir::new("beneath");
    let fs = mount_host(&temp);
    let root = fs.root();
    let inside = VfsName::new(b"inside").expect("name");
    let inside_node = root.mkdir(&inside, MkdirOptions::default()).expect("mkdir");
    inside_node
        .symlink(
            &VfsName::new(b"escape").expect("name"),
            VfsPath::new(b"../.."),
        )
        .expect("symlink escape");

    let runtime: Arc<dyn vfs_core::provider::VfsRuntime> = Arc::new(InlineTestRuntime);
    let fs_async = Arc::new(AsyncFsFromSync::new(fs.clone(), runtime));
    let mount_table = Arc::new(MountTable::new(fs.clone(), fs_async).expect("mount table"));

    let root_node = fs.root();
    let root_handle = make_dir_handle(
        &mount_table,
        MountId::from_index(0),
        root_node.clone(),
        None,
        1,
    );
    let ctx = VfsContext::new(
        vfs_core::VfsCred::root(),
        root_handle,
        Arc::new(VfsConfig::default()),
        Arc::new(AllowAllPolicy),
    );
    let walker = PathWalker::new(mount_table.clone());

    let inside_node = fs.root().lookup(&inside).expect("inside lookup");
    let parent_ref = vfs_core::NodeRef::new(MountId::from_index(0), root_node);
    let base_handle = make_dir_handle(
        &mount_table,
        MountId::from_index(0),
        inside_node.clone(),
        Some(parent_ref),
        2,
    );

    let mut flags = WalkFlags::new(&ctx);
    flags.resolve_beneath = true;
    let err = match walker.resolve(ResolutionRequest {
        ctx: &ctx,
        base: VfsBaseDir::Handle(&base_handle),
        path: VfsPath::new(b"escape"),
        flags,
    }) {
        Ok(_) => panic!("escape should be blocked"),
        Err(err) => err,
    };
    assert_eq!(err.kind(), vfs_core::VfsErrorKind::CrossDevice);
}
