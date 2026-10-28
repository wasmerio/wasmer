//! Mount trees, namespaces and propagation, checked against Linux's behaviour (`fs/namespace.c`,
//! `fs/pnode.c`, `Documentation/filesystems/sharedsubtree.rst`).

use std::sync::Arc;

use futures::executor::block_on;
use wasixv2_abi::OpenFlags;
use wasixv2_vfs::{
    Clock, Cred, Ctx, Errno, File, LookupFlags, MntNs, MountAttr, MountFlags, MountOp, OpenHow,
    Path, Propagation, Timestamp, UmountFlags, Vfs,
};
use wasixv2_vfs_mem::MemFs;

struct Epoch;

impl Clock for Epoch {
    fn now(&self) -> Timestamp {
        Timestamp::default()
    }
}

fn tmpfs() -> Arc<MemFs> {
    Arc::new(MemFs::new(Arc::new(Epoch), 0o755, 0, 0))
}

/// A caller in a namespace, as root.
struct View {
    vfs: Arc<Vfs>,
    ns: Arc<MntNs>,
    root: Path,
    cred: Cred,
}

impl View {
    fn initial(vfs: &Arc<Vfs>) -> Self {
        Self {
            vfs: vfs.clone(),
            ns: vfs.namespace().clone(),
            root: vfs.root(),
            cred: Cred::root(),
        }
    }

    /// `unshare(CLONE_NEWNS)`: a copy of this view's namespace, its root moved along.
    fn unshare(&self) -> Self {
        let mut root = self.root.clone();
        let ns = self.vfs.copy_mnt_ns(&self.ns, &mut [&mut root]);
        Self {
            vfs: self.vfs.clone(),
            ns,
            root,
            cred: Cred::root(),
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

    fn path(&self, path: &str) -> Result<Path, Errno> {
        block_on(self.vfs.lookup(
            &self.cx(),
            &self.root,
            path.as_bytes(),
            LookupFlags::follow(),
        ))
    }

    fn mkdir(&self, path: &str) {
        block_on(
            self.vfs
                .mkdir(&self.cx(), &self.root, path.as_bytes(), 0o755),
        )
        .unwrap();
    }

    fn touch(&self, path: &str) -> File {
        let how = OpenHow {
            flags: OpenFlags::O_CREAT | OpenFlags::O_RDWR,
            mode: 0o644,
            lookup: LookupFlags::follow(),
        };
        block_on(self.vfs.open(&self.cx(), &self.root, path.as_bytes(), &how)).unwrap()
    }

    fn exists(&self, path: &str) -> bool {
        self.path(path).is_ok()
    }

    fn op(&self, target: &str, op: MountOp) -> Result<(), Errno> {
        let target = self.path(target)?;
        self.vfs.do_mount(&self.ns, &target, op)
    }

    fn mount_tmpfs(&self, at: &str) {
        let op = MountOp::New {
            fs: tmpfs(),
            source: b"none".to_vec(),
            flags: MountFlags::default(),
        };
        self.op(at, op).unwrap();
    }

    fn bind(&self, from: &str, at: &str, recursive: bool) -> Result<(), Errno> {
        let source = self.path(from)?;
        self.op(at, MountOp::Bind { source, recursive })
    }

    fn propagation(&self, at: &str, kind: Propagation) {
        let op = MountOp::Propagation {
            kind,
            recursive: false,
        };
        self.op(at, op).unwrap();
    }

    fn umount(&self, at: &str, flags: UmountFlags) -> Result<(), Errno> {
        let path = self.path(at)?;
        self.vfs.do_umount(&self.ns, &self.root, &path, flags)
    }

    /// The mount whose root `at` is, as `mountinfo` shows it: (shared, master).
    fn info(&self, at: &str) -> (Option<u32>, Option<u32>) {
        let path = self.path(at).unwrap();
        let info = self.vfs.mount_info(&self.ns, &self.root);
        let found = info
            .iter()
            .find(|info| Arc::ptr_eq(&info.mount, &path.mnt))
            .expect("mounted in the namespace");
        (found.shared, found.master)
    }

    fn mount_count(&self) -> usize {
        self.vfs.mount_info(&self.ns, &self.root).len()
    }
}

fn world() -> (Arc<Vfs>, View) {
    let vfs = Vfs::new(tmpfs());
    let view = View::initial(&vfs);
    (vfs, view)
}

#[test]
fn a_new_mount_covers_the_directory_and_goes_away_with_umount() {
    let (_vfs, v) = world();
    v.mkdir("/mnt");
    v.touch("/mnt/under");
    v.mount_tmpfs("/mnt");
    assert!(!v.exists("/mnt/under"));
    v.touch("/mnt/over");
    assert_eq!(v.mount_count(), 2);

    assert_eq!(v.umount("/mnt", UmountFlags::default()), Ok(()));
    assert!(v.exists("/mnt/under"));
    assert!(!v.exists("/mnt/over"));
    // Not a mount root any more.
    assert_eq!(v.umount("/mnt", UmountFlags::default()), Err(Errno::EINVAL));
}

#[test]
fn mounting_on_a_covered_place_goes_on_top() {
    let (_vfs, v) = world();
    v.mkdir("/mnt");
    v.mount_tmpfs("/mnt");
    v.touch("/mnt/first");
    v.mount_tmpfs("/mnt");
    assert!(!v.exists("/mnt/first"));
    v.umount("/mnt", UmountFlags::default()).unwrap();
    assert!(v.exists("/mnt/first"));
}

#[test]
fn umount_is_busy_while_used_or_covered_unless_detached() {
    let (_vfs, v) = world();
    v.mkdir("/mnt");
    v.mount_tmpfs("/mnt");
    let open = v.touch("/mnt/f");
    assert_eq!(v.umount("/mnt", UmountFlags::default()), Err(Errno::EBUSY));
    drop(open);

    v.mkdir("/mnt/sub");
    v.mount_tmpfs("/mnt/sub");
    assert_eq!(v.umount("/mnt", UmountFlags::default()), Err(Errno::EBUSY));

    let held = v.path("/mnt/sub").unwrap();
    let detach = UmountFlags {
        detach: true,
        ..UmountFlags::default()
    };
    assert_eq!(v.umount("/mnt", detach), Ok(()));
    assert_eq!(v.mount_count(), 1);
    // What was held still works, detached: `..` stays at its root.
    let cx = v.cx();
    let up = block_on(v.vfs.lookup(&cx, &held, b"..", LookupFlags::follow())).unwrap();
    assert_eq!(up, held);
}

#[test]
fn umount_expire_marks_first_then_unmounts() {
    let (_vfs, v) = world();
    v.mkdir("/mnt");
    v.mount_tmpfs("/mnt");
    let expire = UmountFlags {
        expire: true,
        ..UmountFlags::default()
    };
    assert_eq!(v.umount("/mnt", expire), Err(Errno::EAGAIN));
    assert_eq!(v.umount("/mnt", expire), Ok(()));
    let both = UmountFlags {
        expire: true,
        detach: true,
        ..UmountFlags::default()
    };
    v.mount_tmpfs("/mnt");
    assert_eq!(v.umount("/mnt", both), Err(Errno::EINVAL));
}

#[test]
fn bind_shows_a_subtree_and_rbind_brings_its_mounts() {
    let (_vfs, v) = world();
    for dir in ["/src", "/src/d", "/src/d/m", "/bind", "/rbind"] {
        v.mkdir(dir);
    }
    v.mount_tmpfs("/src/d/m");
    v.touch("/src/d/m/inner");
    v.touch("/src/d/file");

    v.bind("/src/d", "/bind", false).unwrap();
    assert!(v.exists("/bind/file"));
    assert!(!v.exists("/bind/m/inner"), "a plain bind leaves mounts out");

    v.bind("/src/d", "/rbind", true).unwrap();
    assert!(v.exists("/rbind/m/inner"));
    // A file is bound on a file, and not on a directory.
    v.touch("/f");
    v.touch("/g");
    v.bind("/f", "/g", false).unwrap();
    assert_eq!(v.bind("/f", "/bind", false), Err(Errno::ENOTDIR));
}

#[test]
fn an_unbindable_mount_is_not_bound() {
    let (_vfs, v) = world();
    v.mkdir("/u");
    v.mkdir("/b");
    v.mount_tmpfs("/u");
    v.propagation("/u", Propagation::Unbindable);
    assert_eq!(v.bind("/u", "/b", false), Err(Errno::EINVAL));
}

#[test]
fn move_takes_a_mount_with_its_tree_but_not_under_itself() {
    let (_vfs, v) = world();
    for dir in ["/a", "/b"] {
        v.mkdir(dir);
    }
    v.mount_tmpfs("/a");
    v.mkdir("/a/m");
    v.mount_tmpfs("/a/m");
    v.touch("/a/m/x");

    let source = v.path("/a").unwrap();
    v.op("/b", MountOp::Move { source }).unwrap();
    assert!(v.exists("/b/m/x"));
    assert!(!v.exists("/a/m"));

    let source = v.path("/b").unwrap();
    assert_eq!(v.op("/b/m", MountOp::Move { source }), Err(Errno::ELOOP));
    // Only a mount root moves.
    let source = v.path("/b/m/x").unwrap();
    assert_eq!(v.op("/a", MountOp::Move { source }), Err(Errno::EINVAL));
}

#[test]
fn remount_read_only_and_bind_remount_change_what_they_say() {
    let (_vfs, v) = world();
    v.mkdir("/a");
    v.mkdir("/b");
    v.mount_tmpfs("/a");
    v.bind("/a", "/b", false).unwrap();

    let ro = MountFlags {
        read_only: true,
        ..MountFlags::default()
    };
    v.op("/b", MountOp::Reconfigure { flags: ro }).unwrap();
    assert!(v.path("/b").unwrap().mnt.is_read_only());
    assert!(!v.path("/a").unwrap().mnt.is_read_only(), "per mount only");

    v.op(
        "/a",
        MountOp::Remount {
            read_only: true,
            flags: ro,
        },
    )
    .unwrap();
    let a = v.path("/a").unwrap();
    assert!(a.mnt.sb().is_read_only(), "the filesystem itself");
    let rw = MountOp::Remount {
        read_only: false,
        flags: MountFlags::default(),
    };
    v.op("/a", rw).unwrap();
    assert!(!v.path("/a").unwrap().mnt.is_read_only());
    // Remounting needs a mount root.
    v.mkdir("/a/d");
    let again = MountOp::Reconfigure {
        flags: MountFlags::default(),
    };
    assert_eq!(v.op("/a/d", again), Err(Errno::EINVAL));
}

#[test]
fn a_copied_namespace_does_not_see_new_mounts_of_the_other() {
    let (_vfs, v) = world();
    v.mkdir("/mnt");
    v.mkdir("/old");
    v.mount_tmpfs("/old");
    v.touch("/old/kept");

    let w = v.unshare();
    assert!(w.exists("/old/kept"), "the copy has the mounts there were");
    w.mount_tmpfs("/mnt");
    w.touch("/mnt/private");
    assert!(!v.exists("/mnt/private"));

    w.umount("/old", UmountFlags::default()).unwrap();
    assert!(
        v.exists("/old/kept"),
        "unmounting a copy leaves the original"
    );
    assert!(!w.exists("/old/kept"));

    // The original's mounts are not the copy's to change.
    let theirs = v.path("/old").unwrap();
    assert_eq!(
        w.vfs
            .do_umount(&w.ns, &w.root, &theirs, UmountFlags::default()),
        Err(Errno::EINVAL)
    );
}

#[test]
fn a_mount_on_a_shared_mount_propagates_to_its_peers_and_slaves() {
    let (_vfs, v) = world();
    for dir in ["/a", "/peer", "/slave"] {
        v.mkdir(dir);
    }
    v.mount_tmpfs("/a");
    v.propagation("/a", Propagation::Shared);
    v.mkdir("/a/m");
    v.bind("/a", "/peer", false).unwrap();
    v.bind("/a", "/slave", false).unwrap();
    v.propagation("/slave", Propagation::Slave);
    let (group, _) = v.info("/a");
    assert_eq!(v.info("/peer"), (group, None));
    assert_eq!(v.info("/slave"), (None, group));

    v.mount_tmpfs("/a/m");
    v.touch("/a/m/new");
    assert!(
        v.exists("/peer/m/new"),
        "a peer gets a copy of the same filesystem"
    );
    assert!(v.exists("/slave/m/new"));

    // A slave's own mounts go nowhere.
    v.mkdir("/slave/s");
    v.mount_tmpfs("/slave/s");
    v.touch("/slave/s/own");
    assert!(!v.exists("/a/s/own"));

    // Unmounting the original takes the copies along.
    v.umount("/a/m", UmountFlags::default()).unwrap();
    assert!(!v.exists("/peer/m/new"));
    assert!(!v.exists("/slave/m/new"));
}

#[test]
fn a_private_mount_propagates_nothing() {
    let (_vfs, v) = world();
    v.mkdir("/a");
    v.mkdir("/b");
    v.mount_tmpfs("/a");
    v.mkdir("/a/m");
    v.bind("/a", "/b", false).unwrap();
    assert_eq!(v.info("/b"), (None, None));
    v.mount_tmpfs("/a/m");
    v.touch("/a/m/x");
    assert!(!v.exists("/b/m/x"));
}

#[test]
fn shared_mounts_propagate_across_copied_namespaces() {
    let (_vfs, v) = world();
    v.mkdir("/s");
    v.mount_tmpfs("/s");
    v.propagation("/s", Propagation::Shared);
    v.mkdir("/s/m");
    let w = v.unshare();
    assert_eq!(w.info("/s").0, v.info("/s").0, "the copy is a peer");

    v.mount_tmpfs("/s/m");
    v.touch("/s/m/from-v");
    assert!(w.exists("/s/m/from-v"));

    // Made a slave, the copy still receives but no longer sends.
    w.propagation("/s", Propagation::Slave);
    assert_eq!(w.info("/s"), (None, v.info("/s").0));
    w.mkdir("/s/w");
    w.mount_tmpfs("/s/w");
    w.touch("/s/w/from-w");
    assert!(!v.exists("/s/w/from-w"));
}

#[test]
fn making_the_last_peer_private_hands_its_slaves_to_its_master() {
    let (_vfs, v) = world();
    for dir in ["/a", "/b", "/c"] {
        v.mkdir(dir);
    }
    v.mount_tmpfs("/a");
    v.propagation("/a", Propagation::Shared);
    v.bind("/a", "/b", false).unwrap();
    v.propagation("/b", Propagation::Slave);
    v.propagation("/b", Propagation::Shared);
    v.bind("/b", "/c", false).unwrap();
    v.propagation("/c", Propagation::Slave);
    let (a, _) = v.info("/a");
    let (b, b_master) = v.info("/b");
    assert_eq!(b_master, a, "shared and slave at once");
    assert_eq!(v.info("/c"), (None, b));

    v.propagation("/b", Propagation::Private);
    assert_eq!(v.info("/c"), (None, a), "/c now receives from /a");
}

#[test]
fn a_mount_where_a_receiver_has_one_slides_under_it() {
    let (_vfs, v) = world();
    for dir in ["/a", "/b"] {
        v.mkdir(dir);
    }
    v.mount_tmpfs("/a");
    v.propagation("/a", Propagation::Shared);
    v.mkdir("/a/m");
    v.bind("/a", "/b", false).unwrap();
    v.propagation("/b", Propagation::Slave);
    v.mount_tmpfs("/b/m");
    v.touch("/b/m/slave-own");

    v.mount_tmpfs("/a/m");
    assert!(
        v.exists("/b/m/slave-own"),
        "the slave's own mount stays on top"
    );
}

#[test]
fn removing_a_mountpoint_in_one_namespace_is_busy_there_and_detaches_elsewhere() {
    let (_vfs, v) = world();
    v.mkdir("/d");
    let w = v.unshare();
    w.mount_tmpfs("/d");
    let cx = v.cx();
    // Not a mountpoint in v's namespace: the directory goes, and w's mount with it.
    block_on(v.vfs.rmdir(&cx, &v.root, b"/d")).unwrap();
    assert_eq!(w.mount_count(), 1);

    v.mkdir("/e");
    v.mount_tmpfs("/e");
    let cx = v.cx();
    assert_eq!(
        block_on(v.vfs.rmdir(&cx, &v.root, b"/e")),
        Err(Errno::EBUSY)
    );
}

#[test]
fn pivot_root_swaps_the_root_mount_and_checks_its_arguments() {
    let (_vfs, v) = world();
    let w = v.unshare();
    w.mkdir("/jail");
    w.mount_tmpfs("/jail");
    // Inside the jail, as pivot_root01 does after chroot.
    let jail = w.path("/jail").unwrap();
    let mut j = View {
        vfs: w.vfs.clone(),
        ns: w.ns.clone(),
        root: jail,
        cred: Cred::root(),
    };
    j.mkdir("/new");
    j.mkdir("/new/old");
    let pivot = |view: &View, new: &str, old: &str| {
        let new = view.path(new)?;
        let old = view.path(old)?;
        view.vfs.pivot_root(&view.ns, &view.root, &new, &old)
    };
    // `new` on the root's filesystem.
    assert_eq!(pivot(&j, "/new", "/new/old"), Err(Errno::EBUSY));
    j.mount_tmpfs("/new");
    j.mkdir("/new/old");
    j.mkdir("/elsewhere");
    j.mount_tmpfs("/elsewhere");
    assert_eq!(pivot(&j, "/new", "/elsewhere"), Err(Errno::EINVAL));
    j.touch("/new/marker");

    assert_eq!(pivot(&j, "/new", "/new/old"), Ok(()));
    // The caller's root is moved by the kernel; here, look from the new root.
    j.root = w.path("/jail").unwrap();
    assert!(j.exists("/marker"));
    assert!(j.exists("/old/new"), "the old root under put_old");
}

#[test]
fn mount_setattr_sets_and_clears_flags_and_propagation() {
    let (_vfs, v) = world();
    v.mkdir("/a");
    v.mount_tmpfs("/a");
    v.mkdir("/a/m");
    v.mount_tmpfs("/a/m");
    let a = v.path("/a").unwrap();
    let attr = MountAttr {
        set: MountFlags {
            read_only: true,
            noexec: true,
            ..MountFlags::default()
        },
        propagation: Some(Propagation::Shared),
        recursive: true,
        ..MountAttr::default()
    };
    v.vfs.mount_setattr(&v.ns, &a, &attr).unwrap();
    let m = v.path("/a/m").unwrap();
    assert!(m.mnt.flags().read_only && m.mnt.flags().noexec);
    assert!(v.info("/a/m").0.is_some());

    let clear = MountAttr {
        clear: MountFlags {
            read_only: true,
            ..MountFlags::default()
        },
        ..MountAttr::default()
    };
    v.vfs.mount_setattr(&v.ns, &a, &clear).unwrap();
    assert!(!v.path("/a").unwrap().mnt.flags().read_only);
    assert!(
        v.path("/a/m").unwrap().mnt.flags().read_only,
        "not recursive"
    );

    v.mkdir("/a/d");
    let d = v.path("/a/d").unwrap();
    assert_eq!(v.vfs.mount_setattr(&v.ns, &d, &clear), Err(Errno::EINVAL));
}

#[test]
fn a_dropped_namespace_unmounts_its_tree() {
    let (_vfs, v) = world();
    v.mkdir("/a");
    let w = v.unshare();
    w.mount_tmpfs("/a");
    let held = w.path("/a").unwrap();
    drop(w);
    assert!(held.mnt.parent().is_none(), "detached");
    assert_eq!(v.mount_count(), 1);
}
