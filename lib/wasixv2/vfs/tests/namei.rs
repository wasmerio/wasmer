//! Path-walk and namespace rules, checked against Linux's behaviour on a memory filesystem.

use std::sync::Arc;

use futures::executor::block_on;
use wasixv2_abi::OpenFlags;
use wasixv2_vfs::{
    Clock, Cred, Ctx, Errno, File, LookupFlags, MountFlags, OpenHow, Path, RenameMode, Timestamp,
    Vfs,
};
use wasixv2_vfs_mem::MemFs;

struct Epoch;

impl Clock for Epoch {
    fn now(&self) -> Timestamp {
        Timestamp::default()
    }
}

struct World {
    vfs: Arc<Vfs>,
    root: Path,
    cred: Cred,
}

impl World {
    /// A root owned by uid 1, which the caller runs as.
    fn new() -> Self {
        let vfs = Vfs::new(Arc::new(MemFs::new(Arc::new(Epoch), 0o755, 1, 1)));
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
            umask: 0o022,
            task: 0,
        }
    }

    fn open(&self, path: &[u8], flags: OpenFlags, follow: bool) -> Result<File, Errno> {
        let how = OpenHow {
            flags,
            mode: 0o644,
            lookup: LookupFlags {
                follow,
                directory: flags.contains(OpenFlags::O_DIRECTORY),
                ..LookupFlags::default()
            },
        };
        block_on(self.vfs.open(&self.cx(), &self.root, path, &how))
    }

    fn create(&self, path: &[u8]) -> File {
        self.open(path, OpenFlags::O_CREAT | OpenFlags::O_RDWR, true)
            .unwrap()
    }

    fn mkdir(&self, path: &[u8]) -> Result<(), Errno> {
        block_on(self.vfs.mkdir(&self.cx(), &self.root, path, 0o755))
    }

    fn symlink(&self, target: &[u8], path: &[u8]) {
        block_on(self.vfs.symlink(&self.cx(), target, &self.root, path)).unwrap();
    }

    fn lookup(&self, path: &[u8], flags: LookupFlags) -> Result<Path, Errno> {
        block_on(self.vfs.lookup(&self.cx(), &self.root, path, flags))
    }
}

#[test]
fn trailing_slashes_demand_directories() {
    let w = World::new();
    w.create(b"/f");
    w.mkdir(b"/d").unwrap();

    assert_eq!(
        w.lookup(b"/f/", LookupFlags::follow()).err(),
        Some(Errno::ENOTDIR)
    );
    assert!(w.lookup(b"/d//", LookupFlags::follow()).is_ok());
    assert_eq!(w.mkdir(b"/d/"), Err(Errno::EEXIST));
    assert_eq!(w.mkdir(b"/new/"), Ok(()));
    assert_eq!(
        w.open(b"/g/", OpenFlags::O_CREAT | OpenFlags::O_RDWR, true)
            .err(),
        Some(Errno::EISDIR)
    );
    assert_eq!(
        block_on(w.vfs.unlink(&w.cx(), &w.root, b"/f/")),
        Err(Errno::ENOTDIR)
    );
    assert_eq!(
        block_on(w.vfs.rmdir(&w.cx(), &w.root, b"/d/.")),
        Err(Errno::EINVAL)
    );
}

#[test]
fn symlinks_resolve_in_the_middle_and_loop_out_at_forty() {
    let w = World::new();
    w.mkdir(b"/d").unwrap();
    w.create(b"/d/f");
    w.symlink(b"d", b"/l");
    w.symlink(b"/l/f", b"/abs");
    w.symlink(b"loop", b"/loop");
    w.symlink(b"missing", b"/dangling");

    assert!(w.lookup(b"/l/f", LookupFlags::nofollow()).is_ok());
    assert!(w.lookup(b"/abs", LookupFlags::follow()).is_ok());
    assert_eq!(
        w.lookup(b"/loop", LookupFlags::follow()).err(),
        Some(Errno::ELOOP)
    );
    // Not following the last component finds the link itself.
    assert!(w.lookup(b"/loop", LookupFlags::nofollow()).is_ok());
    // A trailing slash follows even without LOOKUP_FOLLOW.
    assert!(w.lookup(b"/l/", LookupFlags::nofollow()).is_ok());
    // O_CREAT through a dangling symlink creates its target; O_EXCL refuses.
    assert_eq!(
        w.open(
            b"/dangling",
            OpenFlags::O_CREAT | OpenFlags::O_EXCL | OpenFlags::O_RDWR,
            false
        )
        .err(),
        Some(Errno::EEXIST)
    );
    w.open(b"/dangling", OpenFlags::O_CREAT | OpenFlags::O_RDWR, true)
        .unwrap();
    assert!(w.lookup(b"/missing", LookupFlags::nofollow()).is_ok());
    assert_eq!(
        w.open(b"/l", OpenFlags::O_RDONLY, false).err(),
        Some(Errno::ELOOP)
    );
}

#[test]
fn resolve_beneath_and_in_root_scope_the_walk() {
    let w = World::new();
    w.mkdir(b"/jail").unwrap();
    w.mkdir(b"/jail/sub").unwrap();
    w.create(b"/secret");
    w.symlink(b"/secret", b"/jail/abs");
    let jail = w.lookup(b"/jail", LookupFlags::follow()).unwrap();
    let scoped = |path: &[u8], beneath: bool| {
        let flags = LookupFlags {
            follow: true,
            beneath,
            in_root: !beneath,
            ..LookupFlags::default()
        };
        block_on(w.vfs.lookup(&w.cx(), &jail, path, flags))
    };

    assert!(scoped(b"sub/..", true).is_ok());
    assert_eq!(scoped(b"..", true).err(), Some(Errno::EXDEV));
    assert_eq!(scoped(b"abs", true).err(), Some(Errno::EXDEV));
    // In-root: `..` and absolute targets stop at the jail.
    assert_eq!(scoped(b"../../secret", false).err(), Some(Errno::ENOENT));
    assert_eq!(scoped(b"abs", false).err(), Some(Errno::ENOENT));
}

#[test]
fn permissions_apply_to_an_unprivileged_user() {
    let w = World::new();
    w.mkdir(b"/locked").unwrap();
    w.create(b"/locked/f");
    let locked = w.lookup(b"/locked", LookupFlags::follow()).unwrap();
    block_on(w.vfs.chmod(&w.cx(), &locked, 0o600)).unwrap();

    assert_eq!(
        w.lookup(b"/locked/f", LookupFlags::follow()).err(),
        Some(Errno::EACCES)
    );
    let other = Cred::user(2, 2);
    let cx = Ctx {
        cred: &other,
        root: &w.root,
        umask: 0,
        task: 0,
    };
    assert_eq!(
        block_on(w.vfs.chmod(&cx, &locked, 0o777)),
        Err(Errno::EPERM)
    );
    assert_eq!(
        block_on(w.vfs.mkdir(&cx, &w.root, b"/x", 0o777)),
        Err(Errno::EACCES)
    );
}

#[test]
fn rename_rules_follow_linux() {
    let w = World::new();
    w.mkdir(b"/a").unwrap();
    w.mkdir(b"/a/b").unwrap();
    w.mkdir(b"/full").unwrap();
    w.create(b"/full/x");
    w.create(b"/f");
    let rename = |from: &[u8], to: &[u8], mode| {
        block_on(w.vfs.rename(&w.cx(), &w.root, from, &w.root, to, mode))
    };

    assert_eq!(
        rename(b"/a", b"/a/b/c", RenameMode::Replace),
        Err(Errno::EINVAL)
    );
    assert_eq!(
        rename(b"/a/b", b"/a", RenameMode::Replace),
        Err(Errno::ENOTEMPTY)
    );
    assert_eq!(
        rename(b"/a", b"/full", RenameMode::Replace),
        Err(Errno::ENOTEMPTY)
    );
    assert_eq!(
        rename(b"/f", b"/a", RenameMode::Replace),
        Err(Errno::EISDIR)
    );
    assert_eq!(
        rename(b"/a", b"/f", RenameMode::Replace),
        Err(Errno::ENOTDIR)
    );
    assert_eq!(
        rename(b"/f", b"/full/x", RenameMode::NoReplace),
        Err(Errno::EEXIST)
    );
    assert_eq!(rename(b"/f", b"/a", RenameMode::Exchange), Ok(()));
    assert!(w.lookup(b"/f/b", LookupFlags::follow()).is_ok());
    assert_eq!(
        rename(b"/nope", b"/x", RenameMode::Replace),
        Err(Errno::ENOENT)
    );
}

#[test]
fn unlinked_files_stay_readable_through_open_descriptors() {
    let w = World::new();
    let file = w.create(b"/f");
    block_on(file.write_at(b"still here", 0)).unwrap();

    block_on(w.vfs.unlink(&w.cx(), &w.root, b"/f")).unwrap();
    let mut buf = [0; 10];
    let n = block_on(file.read_at(&mut buf, 0)).unwrap();

    assert_eq!(&buf[..n], b"still here");
    assert_eq!(block_on(w.vfs.stat(&file.path)).unwrap().nlink, 0);
    assert!(w.vfs.d_path(&file.path, &w.root).ends_with(b" (deleted)"));
}

#[test]
fn mounts_cover_their_mountpoint_and_dotdot_climbs_out() {
    let w = World::new();
    w.mkdir(b"/m").unwrap();
    w.create(b"/m/hidden");
    let fs = Arc::new(MemFs::new(Arc::new(Epoch), 0o755, 1, 1));
    block_on(w.vfs.mount(b"/m", fs, MountFlags::default(), false)).unwrap();
    w.create(b"/m/visible");

    assert_eq!(
        w.lookup(b"/m/hidden", LookupFlags::follow()).err(),
        Some(Errno::ENOENT)
    );
    let up = w.lookup(b"/m/..", LookupFlags::follow()).unwrap();
    assert!(up == w.root);
    let m = w.lookup(b"/m", LookupFlags::follow()).unwrap();
    assert_eq!(w.vfs.d_path(&m, &w.root), b"/m");
    assert_eq!(
        block_on(w.vfs.rmdir(&w.cx(), &w.root, b"/m")),
        Err(Errno::EBUSY)
    );
    // Hard links cannot cross mounts.
    assert_eq!(
        block_on(w.vfs.link(
            &w.cx(),
            &w.root,
            b"/m/visible",
            LookupFlags::default(),
            &w.root,
            b"/v"
        )),
        Err(Errno::EXDEV)
    );
}

#[test]
fn hard_links_share_one_inode() {
    let w = World::new();
    w.create(b"/a");
    block_on(w.vfs.link(
        &w.cx(),
        &w.root,
        b"/a",
        LookupFlags::default(),
        &w.root,
        b"/b",
    ))
    .unwrap();

    let a = w.lookup(b"/a", LookupFlags::default()).unwrap();
    let b = w.lookup(b"/b", LookupFlags::default()).unwrap();
    assert!(!Arc::ptr_eq(&a.dentry, &b.dentry));
    assert!(Arc::ptr_eq(a.dentry.inode(), b.dentry.inode()));
}

#[test]
fn only_tmpfiles_can_be_linked_back_from_nothing() {
    let w = World::new();
    let link_fd = |file: &File, to: &[u8]| {
        let flags = LookupFlags {
            empty: true,
            ..LookupFlags::default()
        };
        block_on(w.vfs.link(&w.cx(), &file.path, b"", flags, &w.root, to))
    };

    // A deleted file stays deleted (`I_LINKABLE` is not set).
    let deleted = w.create(b"/f");
    block_on(w.vfs.unlink(&w.cx(), &w.root, b"/f")).unwrap();
    assert_eq!(link_fd(&deleted, b"/back"), Err(Errno::ENOENT));

    // An O_TMPFILE file gets a name once; O_EXCL makes it unlinkable.
    let tmp = OpenFlags::__O_TMPFILE | OpenFlags::O_DIRECTORY | OpenFlags::O_RDWR;
    let file = w.open(b"/", tmp, true).unwrap();
    assert_eq!(link_fd(&file, b"/named"), Ok(()));
    block_on(w.vfs.unlink(&w.cx(), &w.root, b"/named")).unwrap();
    assert_eq!(link_fd(&file, b"/again"), Err(Errno::ENOENT));
    let excl = w.open(b"/", tmp | OpenFlags::O_EXCL, true).unwrap();
    assert_eq!(link_fd(&excl, b"/excl"), Err(Errno::ENOENT));
}

#[test]
fn nothing_is_created_in_a_removed_or_replaced_directory() {
    let w = World::new();
    w.mkdir(b"/gone").unwrap();
    w.mkdir(b"/src").unwrap();
    w.mkdir(b"/dst").unwrap();
    let gone = w.lookup(b"/gone", LookupFlags::default()).unwrap();
    let replaced = w.lookup(b"/dst", LookupFlags::default()).unwrap();

    block_on(w.vfs.rmdir(&w.cx(), &w.root, b"/gone")).unwrap();
    block_on(w.vfs.rename(
        &w.cx(),
        &w.root,
        b"/src",
        &w.root,
        b"/dst",
        RenameMode::Replace,
    ))
    .unwrap();

    for dir in [&gone, &replaced] {
        assert_eq!(
            block_on(w.vfs.mkdir(&w.cx(), dir, b"x", 0o755)),
            Err(Errno::ENOENT)
        );
        assert!(w.vfs.d_path(dir, &w.root).ends_with(b" (deleted)"));
    }
    // The new /dst is the old /src and is usable.
    assert_eq!(w.mkdir(b"/dst/x"), Ok(()));
}

#[test]
fn every_filesystem_has_its_own_device() {
    let w = World::new();
    w.mkdir(b"/m").unwrap();
    let fs = Arc::new(MemFs::new(Arc::new(Epoch), 0o755, 1, 1));
    block_on(w.vfs.mount(b"/m", fs, MountFlags::default(), false)).unwrap();

    let stat = |path: &[u8]| {
        let path = w.lookup(path, LookupFlags::default()).unwrap();
        block_on(w.vfs.stat(&path)).unwrap()
    };
    let (root, m) = (stat(b"/"), stat(b"/m"));
    assert_eq!(root.dev.unwrap().major, 0);
    assert_ne!(root.dev, m.dev);
    // Both roots have the same inode number; the device tells them apart.
    assert_eq!(root.ino, m.ino);
}
