//! Magic links (`nd_jump_link`): a driver's link that leads to an object, not to a path.

use std::{
    any::Any,
    sync::{Arc, Mutex},
};

use async_trait::async_trait;
use futures::executor::block_on;
use wasixv2_abi::OpenFlags;
use wasixv2_vfs::{
    Attr, Clock, Cred, Ctx, Errno, FileType, Filesystem, FsInfo, FsResult, Handle, Link,
    LookupFlags, MountFlags, Node, OpenHow, OpenOptions, Path, SetAttr, StatFs, Timestamp, Vfs,
};
use wasixv2_vfs_mem::MemFs;

struct Epoch;

impl Clock for Epoch {
    fn now(&self) -> Timestamp {
        Timestamp::default()
    }
}

type Target = Arc<Mutex<Option<Path>>>;

/// A filesystem whose root holds one magic link, `jump`, to whatever `target` names.
#[derive(Default)]
struct Magic {
    target: Target,
}

struct Dir(Target);
struct Jump(Target);

fn attr(ino: u64, file_type: FileType) -> Attr {
    Attr {
        ino,
        file_type,
        mode: 0o777,
        nlink: 1,
        uid: 0,
        gid: 0,
        rdev: Default::default(),
        size: 0,
        blocks: 0,
        blksize: 4096,
        atime: Timestamp::default(),
        mtime: Timestamp::default(),
        ctime: Timestamp::default(),
        btime: None,
        dev: None,
    }
}

#[async_trait(?Send)]
impl Filesystem for Magic {
    fn info(&self) -> FsInfo {
        FsInfo {
            name: "magic",
            magic: 1,
            name_max: 255,
            read_only: true,
            dev: None,
            has_btime: false,
            stable: false,
        }
    }

    fn root(&self) -> Arc<dyn Node> {
        Arc::new(Dir(self.target.clone()))
    }

    async fn statfs(&self) -> FsResult<StatFs> {
        Ok(StatFs::default())
    }
}

#[async_trait(?Send)]
impl Node for Dir {
    fn ino(&self) -> u64 {
        1
    }

    fn file_type(&self) -> FileType {
        FileType::Directory
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    async fn getattr(&self) -> FsResult<Attr> {
        Ok(attr(1, FileType::Directory))
    }

    async fn setattr(&self, _: &SetAttr) -> FsResult<Attr> {
        Err(Errno::EPERM)
    }

    async fn lookup(&self, name: &[u8]) -> FsResult<Arc<dyn Node>> {
        match name {
            b"jump" => Ok(Arc::new(Jump(self.0.clone()))),
            _ => Err(Errno::ENOENT),
        }
    }

    async fn open(&self, _: OpenOptions) -> FsResult<Box<dyn Handle>> {
        Err(Errno::EACCES)
    }
}

#[async_trait(?Send)]
impl Node for Jump {
    fn ino(&self) -> u64 {
        2
    }

    fn file_type(&self) -> FileType {
        FileType::Symlink
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    async fn getattr(&self) -> FsResult<Attr> {
        Ok(attr(2, FileType::Symlink))
    }

    async fn setattr(&self, _: &SetAttr) -> FsResult<Attr> {
        Err(Errno::EPERM)
    }

    async fn get_link(&self, _: &Ctx<'_>) -> FsResult<Link> {
        let target = self.0.lock().unwrap().clone();
        target.map(Link::Jump).ok_or(Errno::ENOENT)
    }

    async fn open(&self, _: OpenOptions) -> FsResult<Box<dyn Handle>> {
        Err(Errno::ELOOP)
    }
}

struct World {
    vfs: Arc<Vfs>,
    root: Path,
    cred: Cred,
    target: Target,
}

impl World {
    /// A memory root with `/dir`, and the magic filesystem at `/m`.
    fn new() -> Self {
        let vfs = Vfs::new(Arc::new(MemFs::new(Arc::new(Epoch), 0o755, 1, 1)));
        let magic = Magic::default();
        let target = magic.target.clone();
        block_on(vfs.mount(b"/m", Arc::new(magic), MountFlags::default(), true)).unwrap();
        let world = Self {
            root: vfs.root(),
            vfs,
            cred: Cred::user(1, 1),
            target,
        };
        let cx = world.cx();
        block_on(world.vfs.mkdir(&cx, &world.root, b"/dir", 0o755)).unwrap();
        world
    }

    fn cx(&self) -> Ctx<'_> {
        Ctx {
            cred: &self.cred,
            root: &self.root,
            umask: 0o022,
            task: 0,
            leases: None,
        }
    }

    fn jump_to(&self, path: &Path) {
        *self.target.lock().unwrap() = Some(path.clone());
    }

    fn open(&self, path: &[u8], flags: OpenFlags) -> Result<wasixv2_vfs::File, Errno> {
        let how = OpenHow {
            flags,
            mode: 0o644,
            lookup: LookupFlags::follow(),
        };
        block_on(self.vfs.open(&self.cx(), &self.root, path, &how))
    }

    fn lookup(&self, path: &[u8], flags: LookupFlags) -> Result<Path, Errno> {
        block_on(self.vfs.lookup(&self.cx(), &self.root, path, flags))
    }
}

#[test]
fn a_trailing_jump_is_its_object_even_without_a_name() {
    let w = World::new();
    let file = w
        .open(b"/dir/f", OpenFlags::O_CREAT | OpenFlags::O_RDWR)
        .unwrap();
    w.jump_to(&file.path);

    assert!(w.lookup(b"/m/jump", LookupFlags::follow()).unwrap() == file.path);
    block_on(w.vfs.unlink(&w.cx(), &w.root, b"/dir/f")).unwrap();
    let reopened = w.open(b"/m/jump", OpenFlags::O_RDWR).unwrap();
    assert!(reopened.path == file.path);
    // O_CREAT opens what the link reaches (`LAST_BIND`); no name can be made there.
    let created = w.open(b"/m/jump", OpenFlags::O_CREAT | OpenFlags::O_RDWR);
    assert!(created.is_ok());
    let mkdir = block_on(w.vfs.mkdir(&w.cx(), &w.root, b"/m/jump", 0o755));
    assert_eq!(mkdir, Err(Errno::EEXIST));
    // readlink shows where the object is.
    let body = block_on(w.vfs.readlink(&w.cx(), &w.root, b"/m/jump", false));
    assert_eq!(body, Ok(b"/dir/f (deleted)".to_vec()));
    // Not following, the link is itself.
    let link = w.lookup(b"/m/jump", LookupFlags::nofollow()).unwrap();
    assert_eq!(link.dentry.file_type(), FileType::Symlink);
}

#[test]
fn a_jump_in_the_middle_walks_on_from_the_directory() {
    let w = World::new();
    w.jump_to(&w.lookup(b"/dir", LookupFlags::follow()).unwrap());
    w.open(b"/dir/g", OpenFlags::O_CREAT | OpenFlags::O_RDWR)
        .unwrap();

    let g = w.lookup(b"/m/jump/g", LookupFlags::follow()).unwrap();
    assert!(g == w.lookup(b"/dir/g", LookupFlags::follow()).unwrap());
    let up = w.lookup(b"/m/jump/..", LookupFlags::follow()).unwrap();
    assert!(up == w.root);
}

#[test]
fn resolve_flags_refuse_jumps() {
    let w = World::new();
    w.jump_to(&w.lookup(b"/dir", LookupFlags::follow()).unwrap());
    let refused = |start: &Path, path: &[u8], flags: LookupFlags| {
        block_on(w.vfs.lookup(&w.cx(), start, path, flags)).err()
    };

    let no_magiclinks = LookupFlags {
        no_magiclinks: true,
        ..LookupFlags::follow()
    };
    assert_eq!(
        refused(&w.root, b"/m/jump", no_magiclinks),
        Some(Errno::ELOOP)
    );
    let no_xdev = LookupFlags {
        no_xdev: true,
        ..LookupFlags::follow()
    };
    let m = w.lookup(b"/m", LookupFlags::follow()).unwrap();
    assert_eq!(refused(&m, b"jump", no_xdev), Some(Errno::EXDEV));
    let beneath = LookupFlags {
        beneath: true,
        ..LookupFlags::follow()
    };
    assert_eq!(refused(&m, b"jump", beneath), Some(Errno::EXDEV));
    let in_root = LookupFlags {
        in_root: true,
        ..LookupFlags::follow()
    };
    assert_eq!(refused(&m, b"jump", in_root), Some(Errno::EXDEV));
}
