//! Mount namespaces, the `mount` family of syscalls and `/proc/<pid>/mountinfo`.

use wasixv2_abi::{
    CloneFlags, FsconfigCommand, FsopenFlags, GuestLayout, Idtype, ListmountMntId, MntIdReq,
    MountAttrFlags, MountFlags as MsFlags, MoveMountFlags, OpenTreeFlags, RusagePtr, SiginfoPtr,
    Signal, Statmount, StatmountMask, Statx, StatxMask, UmountFlags as UmountBits, WaitOptions,
};
use wasixv2_syscalls::read_value;
use wasixv2_vfs::{Cred, Ctx, LookupFlags, MNT_UNIQUE_ID_OFFSET, MountFlags, MountOp, Propagation};
use wasixv2_vfs_mem::MemFs;

use super::{
    ipc::{errno, get, put},
    proc::ForkableMemory,
    *,
};

const CWD: Fd = Fd(-100);

/// A tmpfs without options.
struct TestTmpfs;

impl FilesystemType for TestTmpfs {
    fn mount(
        &self,
        _source: &[u8],
        params: &[FsParam<'_>],
        cred: &Cred,
    ) -> Result<Arc<dyn Filesystem>, Errno> {
        if !params.is_empty() {
            return Err(Errno::EINVAL);
        }
        let clock = Arc::new(HostClock(Arc::new(TestHost) as Arc<dyn Host>));
        Ok(Arc::new(MemFs::new(clock, 0o1777, cred.fsuid, cred.fsgid)))
    }
}

/// A kernel with `/proc`, and one process running as `uid`.
struct Machine {
    kernel: Kernel,
    task: TaskId,
    memory: ForkableMemory,
}

impl Machine {
    fn as_uid(uid: u32) -> Self {
        let (kernel, task, _) =
            super::cred::machine_with(|kernel| kernel.with_credentials(Credentials::new(uid, uid)));
        kernel
            .register_filesystem("tmpfs", Arc::new(TestTmpfs))
            .unwrap();
        let vfs = kernel.vfs().clone();
        block_on(vfs.mount(b"/proc", kernel.procfs(), MountFlags::default(), true)).unwrap();
        let memory = ForkableMemory(FlatMemory(RefCell::new(vec![0; 65536])));
        Self {
            kernel,
            task,
            memory,
        }
    }

    fn caller(&self) -> Caller<'_> {
        Caller::new(self.task, &self.memory)
    }

    /// Puts `text` NUL-terminated at `addr` and returns it.
    fn cstr(&self, addr: u32, text: &str) -> Cstr {
        let mut bytes = text.as_bytes().to_vec();
        bytes.push(0);
        put(&self.memory.0, addr, &bytes);
        GuestPtr::new(addr)
    }

    fn mkdir(&self, at: &str) {
        let name = self.cstr(0x1000, at);
        assert_eq!(
            block_on(self.kernel.mkdirat(&self.caller(), CWD, name, 0o755)),
            Ok(0)
        );
    }

    fn unshare(&self, flags: CloneFlags) -> SysResult<i32> {
        block_on(self.kernel.unshare(&self.caller(), flags.0 as u32))
    }

    fn readlink(&self, at: &str) -> String {
        let name = self.cstr(0x1000, at);
        let out = GuestPtrMut::new(0x2000);
        let n = block_on(self.kernel.readlinkat(&self.caller(), CWD, name, out, 256)).unwrap();
        String::from_utf8(get(&self.memory.0, 0x2000, n as usize)).unwrap()
    }

    /// The text of the file `at`.
    fn cat(&self, at: &str) -> String {
        let name = self.cstr(0x1000, at);
        let caller = self.caller();
        let fd = block_on(
            self.kernel
                .openat(&caller, CWD, name, OpenFlags::O_RDONLY, 0),
        )
        .unwrap();
        let mut out = Vec::new();
        loop {
            let buf = UserSliceMut::new(0x4000, 4096);
            let n = block_on(self.kernel.read(&caller, fd, buf)).unwrap();
            if n == 0 {
                break;
            }
            out.extend(get(&self.memory.0, 0x4000, n as usize));
        }
        block_on(self.kernel.close(&caller, fd)).unwrap();
        String::from_utf8(out).unwrap()
    }

    /// The `mountinfo` line of the mount at `point`.
    fn mountinfo(&self, point: &str) -> Option<String> {
        self.cat("/proc/self/mountinfo")
            .lines()
            .find(|line| line.split(' ').nth(4) == Some(point))
            .map(str::to_owned)
    }

    /// `mount`, with a null pointer for each `None`.
    fn mount(
        &self,
        source: Option<&str>,
        target: &str,
        fstype: Option<&str>,
        flags: MsFlags,
        data: Option<&str>,
    ) -> SysResult<i32> {
        let string = |addr, text: Option<&str>| match text {
            Some(text) => self.cstr(addr, text),
            None => GuestPtr::NULL,
        };
        let source = string(0x1100, source);
        let target = self.cstr(0x1200, target);
        let fstype = string(0x1300, fstype);
        let data = string(0x1400, data);
        block_on(self.kernel.mount(
            &self.caller(),
            source,
            target,
            fstype,
            flags,
            GuestPtr::new(data.addr()),
        ))
    }

    fn tmpfs(&self, target: &str) {
        let none = MsFlags(0);
        assert_eq!(
            self.mount(Some("none"), target, Some("tmpfs"), none, None),
            Ok(0),
            "{target}"
        );
    }

    fn umount2(&self, target: &str, flags: UmountBits) -> SysResult<i32> {
        let target = self.cstr(0x1200, target);
        block_on(self.kernel.umount2(&self.caller(), target, flags))
    }

    fn pivot_root(&self, new_root: &str, put_old: &str) -> SysResult<i32> {
        let new_root = self.cstr(0x1200, new_root);
        let put_old = self.cstr(0x1300, put_old);
        block_on(self.kernel.pivot_root(&self.caller(), new_root, put_old))
    }

    /// `mount_setattr` with a `struct mount_attr` of `size` bytes.
    fn setattr(&self, target: &str, at: AtFlags, attr: [u64; 4], size: u32) -> SysResult<i32> {
        let target = self.cstr(0x1200, target);
        let mut bytes: Vec<u8> = attr.iter().flat_map(|v| v.to_le_bytes()).collect();
        bytes.resize((size as usize).min(64), 0);
        put(&self.memory.0, 0x3000, &bytes);
        block_on(self.kernel.mount_setattr(
            &self.caller(),
            CWD,
            target,
            at,
            GuestPtr::new(0x3000),
            size,
        ))
    }

    fn create(&self, at: &str) -> SysResult<Fd> {
        let name = self.cstr(0x1000, at);
        let flags = OpenFlags::O_CREAT | OpenFlags::O_WRONLY;
        block_on(self.kernel.openat(&self.caller(), CWD, name, flags, 0o644))
    }

    fn stat(&self, at: &str) -> Statx {
        let name = self.cstr(0x1000, at);
        let out = GuestPtrMut::new(0x3000);
        let mask = StatxMask(0x7ff);
        let caller = self.caller();
        let statx = self
            .kernel
            .statx(&caller, CWD, name, AtFlags::empty(), mask, out);
        assert_eq!(block_on(statx), Ok(0), "{at}");
        block_on(read_value(&self.memory, GuestPtr::new(0x3000))).unwrap()
    }

    fn mkdir_errno(&self, at: &str) -> Errno {
        let name = self.cstr(0x1000, at);
        errno(block_on(self.kernel.mkdirat(
            &self.caller(),
            CWD,
            name,
            0o755,
        )))
    }

    fn exists(&self, at: &str) -> bool {
        let name = self.cstr(0x1000, at);
        let caller = self.caller();
        match block_on(self.kernel.openat(&caller, CWD, name, OpenFlags::O_PATH, 0)) {
            Ok(fd) => {
                block_on(self.kernel.close(&caller, fd)).unwrap();
                true
            }
            Err(_) => false,
        }
    }

    fn close(&self, fd: Fd) {
        block_on(self.kernel.close(&self.caller(), fd)).unwrap();
    }

    /// The ids `mountinfo` lists.
    fn mount_ids(&self) -> Vec<u64> {
        self.cat("/proc/self/mountinfo")
            .lines()
            .map(|line| line.split(' ').next().unwrap().parse().unwrap())
            .collect()
    }
}

const VFORK: u32 = CloneFlags::CLONE_VM.0 as u32 | CloneFlags::CLONE_VFORK.0 as u32;

/// vforks with `flags` besides `CLONE_VM | CLONE_VFORK`, and returns the child.
fn vfork(kernel: &Kernel, caller: &Caller<'_>, flags: u32) -> TaskId {
    let null = GuestPtrMut::NULL;
    let clone = kernel.clone(caller, VFORK | flags, 0, null, 0, null);
    assert_eq!(block_on(clone), Ok(0));
    match kernel.take_pending(caller.task()) {
        Some(GuestAction::SwitchTask { task, .. }) => task,
        other => panic!("vfork left {other:?}"),
    }
}

/// The inode number in an nsfs link's `kind:[ino]`.
fn ns_ino(link: &str) -> u64 {
    let digits = link.split('[').nth(1).unwrap().trim_end_matches(']');
    digits.parse().unwrap()
}

#[test]
fn unshare_newns_gives_the_caller_a_copy_of_the_tree() {
    let m = Machine::as_uid(0);
    let before = m.mount_ids();
    assert_eq!(ns_ino(&m.readlink("/proc/self/ns/mnt")), 0xEFFF_FFF8);

    assert_eq!(m.unshare(CloneFlags::CLONE_NEWNS), Ok(0));

    let ino = ns_ino(&m.readlink("/proc/self/ns/mnt"));
    assert!(ino >= 0xF000_0000, "a namespace made at run time: {ino:#x}");
    let after = m.mount_ids();
    assert_eq!(after.len(), before.len(), "the same mounts");
    assert!(
        after.iter().all(|id| !before.contains(id)),
        "copies, with ids of their own"
    );
    // The root and working directory moved to the copies.
    let root = m.kernel.task_env(m.task).fs.root;
    let ns = m.kernel.task_ns(m.task);
    assert!(ns.mnt.tree.contains(&root.mnt));
}

#[test]
fn clone_newns_gives_the_child_its_own_tree() {
    let m = Machine::as_uid(0);
    let child = vfork(&m.kernel, &m.caller(), CloneFlags::CLONE_NEWNS.0 as u32);
    let (parent_ns, child_ns) = (m.kernel.task_ns(m.task), m.kernel.task_ns(child));
    assert!(!Arc::ptr_eq(&parent_ns.mnt, &child_ns.mnt));
    assert!(
        Arc::ptr_eq(&parent_ns.uts, &child_ns.uts),
        "only the mount namespace"
    );
    let cwd = m.kernel.task_env(child).fs.cwd;
    assert!(child_ns.mnt.tree.contains(&cwd.mnt));
    assert!(!parent_ns.mnt.tree.contains(&cwd.mnt));
}

#[test]
fn mountinfo_shows_propagation_and_per_mount_flags() {
    let m = Machine::as_uid(0);
    let vfs = m.kernel.vfs().clone();
    m.mkdir("/a");
    m.mkdir("/b");
    let tmpfs = || -> Arc<dyn Filesystem> {
        Arc::new(MemFs::new(
            Arc::new(HostClock(Arc::new(TestHost) as Arc<dyn Host>)),
            0o1777,
            0,
            0,
        ))
    };
    block_on(vfs.mount(b"/a", tmpfs(), MountFlags::default(), false)).unwrap();
    let ro = MountFlags {
        read_only: true,
        nodev: true,
        ..MountFlags::default()
    };
    block_on(vfs.bind_mount(b"/a", b"/b", ro, false)).unwrap();
    let ns = vfs.namespace();
    let a = vfs.root();
    let a = block_on(vfs.lookup(
        &Ctx {
            cred: &Cred::root(),
            root: &a,
            umask: 0,
            task: 0,
            leases: None,
        },
        &a,
        b"/a",
        LookupFlags::follow(),
    ))
    .unwrap();
    let shared = MountOp::Propagation {
        kind: Propagation::Shared,
        recursive: false,
    };
    vfs.do_mount(ns, &a, shared).unwrap();

    let line = m.mountinfo("/a").unwrap();
    let fields: Vec<_> = line.split(' ').collect();
    assert_eq!(fields[5], "rw,relatime");
    assert!(fields[6].starts_with("shared:"), "{line}");
    assert!(line.ends_with(" - tmpfs tmpfs rw"), "{line}");
    // The bind was made before /a was shared, and is a private mount of its own.
    let line = m.mountinfo("/b").unwrap();
    assert!(line.contains(" ro,nodev,relatime - "), "{line}");
    assert!(!line.contains("shared:"));
}

/// Root's new mount namespace belongs to the initial user namespace, as the one it copies
/// does, so its copies keep their propagation (`copy_mnt_ns` makes shared mounts slaves only
/// for another user namespace, `CL_SHARED_TO_SLAVE`): a shared mount stays in its peer group.
#[test]
fn root_s_new_mount_namespace_keeps_shared_mounts_shared() {
    let m = Machine::as_uid(0);
    let vfs = m.kernel.vfs().clone();
    m.mkdir("/a");
    let tmpfs = Arc::new(MemFs::new(
        Arc::new(HostClock(Arc::new(TestHost) as Arc<dyn Host>)),
        0o1777,
        0,
        0,
    ));
    block_on(vfs.mount(b"/a", tmpfs, MountFlags::default(), false)).unwrap();
    let root = vfs.root();
    let cx = Ctx {
        cred: &Cred::root(),
        root: &root,
        umask: 0,
        task: 0,
        leases: None,
    };
    let a = block_on(vfs.lookup(&cx, &root, b"/a", LookupFlags::follow())).unwrap();
    let shared = MountOp::Propagation {
        kind: Propagation::Shared,
        recursive: false,
    };
    vfs.do_mount(vfs.namespace(), &a, shared).unwrap();
    let group = |line: &str| line.split(' ').nth(6).unwrap().to_string();
    let ours = group(&m.mountinfo("/a").unwrap());
    assert!(ours.starts_with("shared:"), "{ours}");

    let child = vfork(&m.kernel, &m.caller(), 0);
    let child_caller = Caller::new(child, &m.memory);
    let unshare = m
        .kernel
        .unshare(&child_caller, CloneFlags::CLONE_NEWNS.0 as u32);
    assert_eq!(block_on(unshare), Ok(0));
    let theirs = m.cat(&format!("/proc/{}/mountinfo", child.0));
    let line = theirs.lines().find(|l| l.split(' ').nth(4) == Some("/a"));
    assert_eq!(line.map(group), Some(ours), "{theirs}");
}

#[test]
fn another_namespace_shows_its_own_mounts() {
    let m = Machine::as_uid(0);
    // A vfork child unshares; its parent's view stays.
    let child = vfork(&m.kernel, &m.caller(), 0);
    let child_caller = Caller::new(child, &m.memory);
    let unshare = m
        .kernel
        .unshare(&child_caller, CloneFlags::CLONE_NEWNS.0 as u32);
    assert_eq!(block_on(unshare), Ok(0));

    let theirs = m.cat(&format!("/proc/{}/mountinfo", child.0));
    let ours = m.cat("/proc/self/mountinfo");
    assert_ne!(theirs, ours);
    assert_eq!(theirs.lines().count(), ours.lines().count());
}

#[test]
fn a_new_mount_namespace_needs_cap_sys_admin() {
    let m = Machine::as_uid(1);
    assert_eq!(errno(m.unshare(CloneFlags::CLONE_NEWNS)), Errno::EPERM);
}

#[test]
fn mount_makes_an_instance_of_a_registered_type_as_root_only() {
    let m = Machine::as_uid(0);
    m.mkdir("/mnt");
    m.tmpfs("/mnt");
    let fd = m.create("/mnt/f").unwrap();
    m.close(fd);
    let line = m.mountinfo("/mnt").unwrap();
    assert!(line.ends_with(" - tmpfs none rw"), "{line}");
    assert!(m.cat("/proc/filesystems").contains("nodev\ttmpfs\n"));

    let none = MsFlags(0);
    assert_eq!(
        errno(m.mount(None, "/mnt", Some("nosuchfs"), none, None)),
        Errno::ENODEV
    );
    assert_eq!(
        errno(m.mount(None, "/mnt", None, none, None)),
        Errno::EINVAL
    );
    assert_eq!(
        errno(m.mount(None, "/mnt", Some("tmpfs"), none, Some("size=1m"))),
        Errno::EINVAL,
        "an option the type does not take"
    );
    assert_eq!(
        errno(m.mount(None, "/nowhere", Some("tmpfs"), none, None)),
        Errno::ENOENT
    );
    // The magic value of old callers in the high half is ignored.
    let magic = MsFlags(MsFlags::MS_MGC_VAL.0 | MsFlags::MS_RDONLY.0);
    assert_eq!(
        m.mount(Some("ro"), "/mnt", Some("tmpfs"), magic, None),
        Ok(0)
    );
    assert_eq!(errno(m.create("/mnt/g")), Errno::EROFS);

    let user = Machine::as_uid(1);
    user.mkdir("/mine");
    assert_eq!(
        errno(user.mount(None, "/mine", Some("tmpfs"), none, None)),
        Errno::EPERM
    );
}

#[test]
fn umount2_checks_its_target_and_flags() {
    let m = Machine::as_uid(0);
    m.mkdir("/mnt");
    m.tmpfs("/mnt");
    m.mkdir("/mnt/d");
    let none = UmountBits(0);
    assert_eq!(errno(m.umount2("/mnt", UmountBits(0x10))), Errno::EINVAL);
    assert_eq!(
        errno(m.umount2("/mnt/d", none)),
        Errno::EINVAL,
        "not a mount root"
    );
    assert_eq!(errno(m.umount2("/nowhere", none)), Errno::ENOENT);

    let fd = m.create("/mnt/f").unwrap();
    assert_eq!(errno(m.umount2("/mnt", none)), Errno::EBUSY);
    m.close(fd);
    assert_eq!(m.umount2("/mnt", none), Ok(0));
    assert!(!m.exists("/mnt/d"));

    m.tmpfs("/mnt");
    let fd = m.create("/mnt/f").unwrap();
    assert_eq!(m.umount2("/mnt", UmountBits::MNT_DETACH), Ok(0));
    assert!(!m.exists("/mnt/f"));
    m.close(fd);

    let user = Machine::as_uid(1);
    assert_eq!(errno(user.umount2("/proc", none)), Errno::EPERM);
}

#[test]
fn bind_move_and_remount() {
    let m = Machine::as_uid(0);
    for dir in ["/a", "/b", "/c"] {
        m.mkdir(dir);
    }
    m.tmpfs("/a");
    let fd = m.create("/a/f").unwrap();
    m.close(fd);
    let bind = MsFlags::MS_BIND;
    assert_eq!(m.mount(Some("/a"), "/b", None, bind, None), Ok(0));
    assert!(m.exists("/b/f"));
    assert_eq!(errno(m.mount(None, "/c", None, bind, None)), Errno::EINVAL);
    assert_eq!(
        errno(m.mount(Some(""), "/c", None, bind, None)),
        Errno::EINVAL
    );

    let ro_bind = MsFlags(MsFlags::MS_REMOUNT.0 | MsFlags::MS_BIND.0 | MsFlags::MS_RDONLY.0);
    assert_eq!(m.mount(None, "/b", None, ro_bind, None), Ok(0));
    assert_eq!(errno(m.create("/b/g")), Errno::EROFS);
    let writer = m.create("/a/g").expect("the other mount stays writable");

    assert_eq!(
        m.mount(Some("/b"), "/c", None, MsFlags::MS_MOVE, None),
        Ok(0)
    );
    assert!(m.exists("/c/f"));
    assert!(!m.exists("/b/f"));

    let remount_ro = MsFlags(MsFlags::MS_REMOUNT.0 | MsFlags::MS_RDONLY.0);
    assert_eq!(
        errno(m.mount(None, "/a", None, remount_ro, None)),
        Errno::EBUSY,
        "while a file is open for writing"
    );
    m.close(writer);
    assert_eq!(m.mount(None, "/a", None, remount_ro, None), Ok(0));
    assert_eq!(errno(m.create("/a/h")), Errno::EROFS);
    assert!(m.mountinfo("/a").unwrap().ends_with(" - tmpfs none ro"));
    let remount_rw = MsFlags::MS_REMOUNT;
    assert_eq!(m.mount(None, "/a", None, remount_rw, Some("rw")), Ok(0));
    assert!(m.create("/a/h").is_ok());
    assert_eq!(
        errno(m.mount(None, "/a", None, remount_rw, Some("size=1"))),
        Errno::EINVAL
    );
}

/// `mount_setattr02`: propagation through `mount_setattr`, read back from `mountinfo`.
#[test]
fn mount_setattr_changes_propagation_as_mountinfo_shows() {
    let m = Machine::as_uid(0);
    assert_eq!(m.unshare(CloneFlags::CLONE_NEWNS), Ok(0));
    let private = MsFlags(MsFlags::MS_REC.0 | MsFlags::MS_PRIVATE.0);
    assert_eq!(m.mount(None, "/", None, private, None), Ok(0));
    m.mkdir("/t");
    let flags = MsFlags(MsFlags::MS_NOATIME.0 | MsFlags::MS_NODEV.0);
    assert_eq!(
        m.mount(Some("ltp"), "/t", Some("tmpfs"), flags, Some("")),
        Ok(0)
    );
    m.mkdir("/t/slave");
    let shared = |m: &Machine, at: &str| m.mountinfo(at).unwrap().contains(" shared:");
    let none = AtFlags(0);
    assert_eq!(m.setattr("/t", none, [0; 4], 32), Ok(0));
    assert!(!shared(&m, "/t"));
    assert_eq!(
        errno(m.setattr("/t", none, [0, 0, u64::MAX, 0], 32)),
        Errno::EINVAL
    );
    let ms_shared = u64::from(MsFlags::MS_SHARED.0);
    assert_eq!(m.setattr("/t", none, [0, 0, ms_shared, 0], 32), Ok(0));
    assert!(shared(&m, "/t"));

    assert_eq!(
        m.mount(Some("/t"), "/t/slave", Some("none"), MsFlags::MS_BIND, None),
        Ok(0)
    );
    assert!(
        shared(&m, "/t/slave"),
        "a bind of a shared mount is its peer"
    );
    let ms_slave = u64::from(MsFlags::MS_SLAVE.0);
    assert_eq!(m.setattr("/t/slave", none, [0, 0, ms_slave, 0], 32), Ok(0));
    let line = m.mountinfo("/t/slave").unwrap();
    assert!(
        line.contains(" master:") && !line.contains(" shared:"),
        "{line}"
    );
    assert_eq!(m.umount2("/t/slave", UmountBits(0)), Ok(0));
    let ms_private = u64::from(MsFlags::MS_PRIVATE.0);
    assert_eq!(m.setattr("/t", none, [0, 0, ms_private, 0], 32), Ok(0));
    assert!(!shared(&m, "/t"));

    // The struct's size and the flags.
    assert_eq!(errno(m.setattr("/t", none, [0; 4], 31)), Errno::EINVAL);
    assert_eq!(errno(m.setattr("/t", none, [0; 4], 0x20000)), Errno::E2BIG);
    let mut big = [0u64; 4];
    big[0] = 1;
    assert_eq!(
        m.setattr("/t", none, big, 40),
        Ok(0),
        "zeroes past the struct"
    );
    assert_eq!(
        errno(m.setattr("/t", AtFlags(1), [0; 4], 32)),
        Errno::EINVAL
    );
    let noatime = MountAttrFlags::MOUNT_ATTR_NOATIME.0;
    assert_eq!(
        errno(m.setattr("/t", none, [noatime, 0, 0, 0], 32)),
        Errno::EINVAL,
        "an atime setting needs MOUNT_ATTR__ATIME cleared"
    );
    let atime = MountAttrFlags::MOUNT_ATTR__ATIME.0;
    assert_eq!(m.setattr("/t", none, [noatime, atime, 0, 0], 32), Ok(0));
    let rdonly = MountAttrFlags::MOUNT_ATTR_RDONLY.0;
    assert_eq!(m.setattr("/t", none, [rdonly, 0, 0, 0], 32), Ok(0));
    assert!(m.mountinfo("/t").unwrap().contains(" ro,nodev,noatime "));
}

/// `pivot_root01`'s steps and failures.
#[test]
fn pivot_root_follows_pivot_root01() {
    let m = Machine::as_uid(0);
    m.mkdir("/chroot");
    assert_eq!(
        m.unshare(CloneFlags::CLONE_NEWNS | CloneFlags::CLONE_FS),
        Ok(0)
    );
    let private = MsFlags(MsFlags::MS_REC.0 | MsFlags::MS_PRIVATE.0);
    assert_eq!(m.mount(Some("none"), "/", None, private, None), Ok(0));
    m.tmpfs("/chroot");
    let chroot = m.cstr(0x1000, "/chroot");
    assert_eq!(block_on(m.kernel.chroot(&m.caller(), chroot)), Ok(0));
    m.mkdir("/new_root");

    // new_root on the current root's filesystem.
    m.mkdir("/new_root/put_old");
    assert_eq!(
        errno(m.pivot_root("/new_root", "/new_root/put_old")),
        Errno::EBUSY
    );
    m.tmpfs("/new_root");
    // put_old not under new_root.
    m.mkdir("/put_old_fs");
    m.tmpfs("/put_old_fs");
    m.mkdir("/put_old_fs/put_old");
    assert_eq!(
        errno(m.pivot_root("/new_root", "/put_old_fs/put_old")),
        Errno::EINVAL
    );
    // put_old not a directory.
    let fd = m.create("/new_root/file").unwrap();
    m.close(fd);
    assert_eq!(
        errno(m.pivot_root("/new_root", "/new_root/file")),
        Errno::ENOTDIR
    );

    m.mkdir("/new_root/put_old");
    let fd = m.create("/new_root/marker").unwrap();
    m.close(fd);
    assert_eq!(m.pivot_root("/new_root", "/new_root/put_old"), Ok(0));
    // The caller's root moved to the new root.
    assert!(m.exists("/marker"));
    assert!(m.exists("/put_old/new_root"));
    assert!(m.exists("/put_old/put_old_fs/put_old"));

    let user = Machine::as_uid(1);
    user.mkdir("/x");
    assert_eq!(errno(user.pivot_root("/x", "/x")), Errno::EPERM);
}

/// `mount08`: nothing is mounted on a descriptor's link in `/proc/<pid>/fd`.
#[test]
fn nothing_mounts_on_a_proc_fd_link() {
    let m = Machine::as_uid(0);
    m.mkdir("/mnt");
    let foo = m.create("/mnt/foo").unwrap();
    let bar = m.create("/mnt/bar").unwrap();
    m.close(bar);
    let link = format!("/proc/{}/fd/{}", m.task.0, foo.0);
    let name = m.cstr(0x1000, &link);
    let flags = OpenFlags::O_PATH | OpenFlags::O_NOFOLLOW;
    let caller = m.caller();
    let path_fd = block_on(m.kernel.openat(&caller, CWD, name, flags, 0)).unwrap();
    let through = format!("/proc/{}/fd/{}", m.task.0, path_fd.0);
    assert_eq!(
        errno(m.mount(Some("/mnt/bar"), &through, Some(""), MsFlags::MS_BIND, None)),
        Errno::ENOENT
    );
}

#[test]
fn a_mount_in_one_namespace_is_not_in_the_other() {
    let m = Machine::as_uid(0);
    m.mkdir("/mnt");
    let child = vfork(&m.kernel, &m.caller(), 0);
    let child_caller = Caller::new(child, &m.memory);
    let unshare = m
        .kernel
        .unshare(&child_caller, CloneFlags::CLONE_NEWNS.0 as u32);
    assert_eq!(block_on(unshare), Ok(0));
    let target = m.cstr(0x1200, "/mnt");
    let fstype = m.cstr(0x1300, "tmpfs");
    let mount = m.kernel.mount(
        &child_caller,
        GuestPtr::NULL,
        target,
        fstype,
        MsFlags(0),
        GuestPtr::NULL,
    );
    assert_eq!(block_on(mount), Ok(0));
    let theirs = m.cat(&format!("/proc/{}/mountinfo", child.0));
    assert!(
        theirs
            .lines()
            .any(|line| line.split(' ').nth(4) == Some("/mnt"))
    );
    assert!(m.mountinfo("/mnt").is_none());
}

impl Machine {
    fn open_path(&self, at: &str) -> Fd {
        let name = self.cstr(0x1000, at);
        block_on(
            self.kernel
                .openat(&self.caller(), CWD, name, OpenFlags::O_PATH, 0),
        )
        .unwrap()
    }

    fn open_tree(&self, at: &str, flags: u32) -> SysResult<i32> {
        let name = self.cstr(0x1000, at);
        block_on(self.kernel.open_tree(&self.caller(), CWD, name, flags))
    }

    fn move_mount(&self, from: (Fd, &str), to: (Fd, &str), flags: u32) -> SysResult<i32> {
        let from_name = self.cstr(0x1100, from.1);
        let to_name = self.cstr(0x1200, to.1);
        block_on(
            self.kernel
                .move_mount(&self.caller(), from.0, from_name, to.0, to_name, flags),
        )
    }

    /// `fsconfig` with a string value, or none.
    fn fsconfig(
        &self,
        fd: Fd,
        cmd: FsconfigCommand,
        key: Option<&str>,
        value: Option<&str>,
    ) -> SysResult<i32> {
        let key = key.map_or(GuestPtr::NULL, |key| self.cstr(0x1100, key));
        let value = value.map_or(GuestPtr::NULL, |value| self.cstr(0x1200, value));
        let value = GuestPtr::new(value.addr());
        block_on(
            self.kernel
                .fsconfig(&self.caller(), fd, cmd.0, key, value, 0),
        )
    }

    /// The `stx_mnt_id` `statx` reports for `at` when asked for `mask`.
    fn mnt_id(&self, at: &str, mask: StatxMask) -> u64 {
        let name = self.cstr(0x1000, at);
        let out = GuestPtrMut::new(0x3000);
        let flags = AtFlags(0);
        block_on(
            self.kernel
                .statx(&self.caller(), CWD, name, flags, mask, out),
        )
        .unwrap();
        let statx: Statx = block_on(read_value(&self.memory.0, GuestPtr::new(0x3000))).unwrap();
        assert!(statx.stx_mask.contains(mask));
        statx.stx_mnt_id
    }

    /// Puts a `struct mnt_id_req` (version 0) at 0x5000.
    fn mnt_id_req(&self, mnt_id: u64, param: u64) -> GuestPtr<MntIdReq> {
        let mut bytes = Vec::new();
        bytes.extend(24u32.to_le_bytes());
        bytes.extend(0u32.to_le_bytes());
        bytes.extend(mnt_id.to_le_bytes());
        bytes.extend(param.to_le_bytes());
        put(&self.memory.0, 0x5000, &bytes);
        GuestPtr::new(0x5000)
    }

    /// `statmount` into a buffer of `size` bytes at 0x6000: the record and its strings.
    fn statmount(
        &self,
        mnt_id: u64,
        mask: StatmountMask,
        size: u32,
    ) -> SysResult<(Statmount, Vec<u8>)> {
        let req = self.mnt_id_req(mnt_id, mask.0);
        let buf = GuestPtrMut::new(0x6000);
        block_on(self.kernel.statmount(&self.caller(), req, buf, size, 0))?;
        let bytes = get(&self.memory.0, 0x6000, size as usize);
        let sm = Statmount::read_from(&bytes).unwrap();
        Ok((sm, bytes[Statmount::SIZE..].to_vec()))
    }

    fn listmount(&self, mnt_id: u64, last: u64) -> SysResult<Vec<u64>> {
        let req = self.mnt_id_req(mnt_id, last);
        let out = GuestPtrMut::new(0x6000);
        let n = block_on(self.kernel.listmount(&self.caller(), req, out, 32, 0))?;
        let bytes = get(&self.memory.0, 0x6000, n as usize * 8);
        Ok(bytes
            .chunks(8)
            .map(|id| u64::from_le_bytes(id.try_into().unwrap()))
            .collect())
    }
}

/// The NUL-terminated string at `offset` of `statmount`'s strings.
fn sm_string(strings: &[u8], offset: u32) -> String {
    let rest = &strings[offset as usize..];
    let end = rest.iter().position(|&b| b == 0).unwrap();
    String::from_utf8(rest[..end].to_vec()).unwrap()
}

const OPEN_TREE_CLONE: u32 = OpenTreeFlags::OPEN_TREE_CLONE.0;

/// `move_mount03`: a copy of A goes beneath B, and shows once B is unmounted.
#[test]
fn move_mount_beneath_as_move_mount03() {
    let m = Machine::as_uid(0);
    for dir in ["/a", "/b"] {
        m.mkdir(dir);
    }
    m.tmpfs("/a");
    m.tmpfs("/b");
    m.close(m.create("/a/A").unwrap());
    m.close(m.create("/b/B").unwrap());

    let clone = OPEN_TREE_CLONE | OpenFlags::O_CLOEXEC.0;
    let a = Fd(m.open_tree("/a", clone).unwrap());
    let b = m.open_path("/b");
    let flags = MoveMountFlags::MOVE_MOUNT_BENEATH.0
        | MoveMountFlags::MOVE_MOUNT_F_EMPTY_PATH.0
        | MoveMountFlags::MOVE_MOUNT_T_EMPTY_PATH.0;
    assert_eq!(m.move_mount((a, ""), (b, ""), flags), Ok(0));
    m.close(a);
    m.close(b);
    assert!(m.exists("/b/B"));
    assert!(m.exists("/a/A"));
    assert!(!m.exists("/b/A"));

    assert_eq!(m.umount2("/b", UmountBits(0)), Ok(0));
    assert!(m.exists("/b/A"));
    assert_eq!(m.umount2("/b", UmountBits(0)), Ok(0));
    assert_eq!(m.umount2("/a", UmountBits(0)), Ok(0));
}

#[test]
fn open_tree_and_move_mount_check_their_flags() {
    let m = Machine::as_uid(0);
    m.mkdir("/a");
    m.tmpfs("/a");
    let recursive = AtFlags::AT_RECURSIVE.0;
    assert_eq!(errno(m.open_tree("/a", recursive)), Errno::EINVAL);
    assert_eq!(errno(m.open_tree("/a", 1 << 30)), Errno::EINVAL);
    // Without OPEN_TREE_CLONE, an O_PATH file of the place itself.
    let plain = Fd(m.open_tree("/a", 0).unwrap());
    let ids = m.mount_ids();
    m.close(plain);
    let copy = Fd(m.open_tree("/a", OPEN_TREE_CLONE | recursive).unwrap());
    assert_eq!(m.mount_ids(), ids, "a detached copy is in no namespace");
    let both = MoveMountFlags::MOVE_MOUNT_BENEATH.0 | MoveMountFlags::MOVE_MOUNT_SET_GROUP.0;
    assert_eq!(
        errno(m.move_mount((copy, ""), (CWD, "/a"), both)),
        Errno::EINVAL
    );
    assert_eq!(
        errno(m.move_mount((copy, ""), (CWD, "/a"), 1 << 20)),
        Errno::EINVAL
    );
    // An empty path is a path without *_EMPTY_PATH.
    assert_eq!(
        errno(m.move_mount((copy, ""), (CWD, "/a"), 0)),
        Errno::ENOENT
    );
    m.close(copy);

    let user = Machine::as_uid(1);
    user.mkdir("/tmp");
    assert_eq!(errno(user.open_tree("/tmp", OPEN_TREE_CLONE)), Errno::EPERM);
    assert_eq!(
        errno(user.move_mount((CWD, "/tmp"), (CWD, "/tmp"), 0)),
        Errno::EPERM
    );
}

/// `fsopen`, `fsconfig`, `fsmount` and `move_mount` make a mount as `mount` would; `fspick`
/// reconfigures it.
#[test]
fn a_filesystem_context_creates_mounts_and_reconfigures() {
    let m = Machine::as_uid(0);
    m.mkdir("/m");
    let name = m.cstr(0x1000, "tmpfs");
    let caller = m.caller();
    let fs = Fd(block_on(
        m.kernel
            .fsopen(&caller, name, FsopenFlags::FSOPEN_CLOEXEC.0),
    )
    .unwrap());
    let none = m.cstr(0x1000, "nosuchfs");
    assert_eq!(
        errno(block_on(m.kernel.fsopen(&caller, none, 0))),
        Errno::ENODEV
    );
    let read = block_on(m.kernel.read(&caller, fs, UserSliceMut::new(0x4000, 64)));
    assert_eq!(errno(read), Errno::ENODATA);

    use FsconfigCommand as C;
    assert_eq!(errno(m.fsconfig(fs, C(99), None, None)), Errno::EOPNOTSUPP);
    assert_eq!(
        errno(m.fsconfig(fs, C::FSCONFIG_SET_FLAG, Some("ro"), Some("x"))),
        Errno::EINVAL
    );
    assert_eq!(
        m.fsconfig(fs, C::FSCONFIG_SET_STRING, Some("source"), Some("made")),
        Ok(0)
    );
    assert_eq!(
        errno(m.fsconfig(fs, C::FSCONFIG_SET_STRING, Some("source"), Some("again"))),
        Errno::EINVAL
    );
    // Not created yet.
    assert_eq!(
        errno(block_on(m.kernel.fsmount(&caller, fs, 0, 0))),
        Errno::EINVAL
    );
    assert_eq!(m.fsconfig(fs, C::FSCONFIG_CMD_CREATE, None, None), Ok(0));
    assert_eq!(
        errno(m.fsconfig(fs, C::FSCONFIG_SET_FLAG, Some("rw"), None)),
        Errno::EBUSY
    );
    assert_eq!(
        errno(m.fsconfig(fs, C::FSCONFIG_CMD_CREATE, None, None)),
        Errno::EBUSY
    );

    let nodev = MountAttrFlags::MOUNT_ATTR_NODEV.0 as u32;
    let mnt = Fd(block_on(m.kernel.fsmount(&caller, fs, 0, nodev)).unwrap());
    assert_eq!(
        errno(block_on(m.kernel.fsmount(&caller, fs, 0, 0))),
        Errno::EBUSY
    );
    let from_fd = MoveMountFlags::MOVE_MOUNT_F_EMPTY_PATH.0;
    assert_eq!(m.move_mount((mnt, ""), (CWD, "/m"), from_fd), Ok(0));
    m.close(mnt);
    let line = m.mountinfo("/m").unwrap();
    assert!(line.contains(" rw,nodev,relatime "), "{line}");
    assert!(line.ends_with(" - tmpfs made rw"), "{line}");

    // After fsmount the context reconfigures the instance.
    assert_eq!(
        m.fsconfig(fs, C::FSCONFIG_SET_FLAG, Some("ro"), None),
        Ok(0)
    );
    assert_eq!(
        m.fsconfig(fs, C::FSCONFIG_CMD_RECONFIGURE, None, None),
        Ok(0)
    );
    assert!(m.mountinfo("/m").unwrap().ends_with(" - tmpfs made ro"));
    m.close(fs);

    let path = m.cstr(0x1000, "/m");
    let pick = Fd(block_on(m.kernel.fspick(&caller, CWD, path, 0)).unwrap());
    assert_eq!(
        errno(m.fsconfig(pick, C::FSCONFIG_CMD_CREATE, None, None)),
        Errno::EBUSY
    );
    assert_eq!(
        m.fsconfig(pick, C::FSCONFIG_SET_FLAG, Some("rw"), None),
        Ok(0)
    );
    assert_eq!(
        m.fsconfig(pick, C::FSCONFIG_CMD_RECONFIGURE, None, None),
        Ok(0)
    );
    assert!(m.mountinfo("/m").unwrap().ends_with(" - tmpfs made rw"));
    m.close(pick);
    m.mkdir("/m/d");
    let inner = m.cstr(0x1000, "/m/d");
    assert_eq!(
        errno(block_on(m.kernel.fspick(&caller, CWD, inner, 0))),
        Errno::EINVAL
    );
}

/// `statmount03`: the basic fields of a bind mount for each propagation type, with the ids
/// `statx` gives.
#[test]
fn statmount_reports_ids_and_propagation_as_statmount03() {
    let m = Machine::as_uid(0);
    for dir in ["/a", "/b"] {
        m.mkdir(dir);
    }
    assert_eq!(m.unshare(CloneFlags::CLONE_NEWNS), Ok(0));
    let rprivate = MsFlags(MsFlags::MS_REC.0 | MsFlags::MS_PRIVATE.0);
    assert_eq!(m.mount(Some("none"), "/", None, rprivate, None), Ok(0));
    assert_eq!(
        m.mount(Some("/b"), "/b", None, MsFlags::MS_BIND, None),
        Ok(0)
    );
    assert_eq!(
        m.mount(Some("none"), "/b", None, MsFlags::MS_SHARED, None),
        Ok(0)
    );
    let parent_old = m.mnt_id("/a", StatxMask::STATX_MNT_ID);
    let parent = m.mnt_id("/a", StatxMask::STATX_MNT_ID_UNIQUE);
    assert_eq!(parent, parent_old + MNT_UNIQUE_ID_OFFSET);
    let group = |line: String, tag: &str| -> u64 {
        let field = line.split(' ').find(|f| f.starts_with(tag)).unwrap();
        field[tag.len()..].parse().unwrap()
    };
    let b_group = group(m.mountinfo("/b").unwrap(), "shared:");

    for kind in [
        MsFlags::MS_PRIVATE,
        MsFlags::MS_SHARED,
        MsFlags::MS_SLAVE,
        MsFlags::MS_UNBINDABLE,
    ] {
        assert_eq!(
            m.mount(Some("/b"), "/a", None, MsFlags::MS_BIND, None),
            Ok(0)
        );
        assert_eq!(m.mount(Some("none"), "/a", None, kind, None), Ok(0));
        let id_old = m.mnt_id("/a", StatxMask::STATX_MNT_ID);
        let id = m.mnt_id("/a", StatxMask::STATX_MNT_ID_UNIQUE);
        let basic = StatmountMask::STATMOUNT_MNT_BASIC;
        let (sm, _) = m.statmount(id, basic, Statmount::SIZE as u32).unwrap();
        assert_eq!(m.umount2("/a", UmountBits(0)), Ok(0));
        assert_eq!(sm.mask, basic.0);
        assert_eq!(sm.size as usize, Statmount::SIZE);
        assert_eq!((sm.mnt_id, sm.mnt_id_old as u64), (id, id_old));
        assert_eq!(
            (sm.mnt_parent_id, sm.mnt_parent_id_old as u64),
            (parent, parent_old)
        );
        assert_eq!(sm.mnt_propagation & u64::from(kind.0), u64::from(kind.0));
        let shared = kind == MsFlags::MS_SHARED;
        let slave = kind == MsFlags::MS_SLAVE;
        assert_eq!(sm.mnt_peer_group, if shared { b_group } else { 0 });
        assert_eq!(sm.mnt_master, if slave { b_group } else { 0 });
    }
    // Ids below the unique ones are not mount ids.
    let basic = StatmountMask::STATMOUNT_MNT_BASIC;
    assert_eq!(
        errno(m.statmount(parent_old, basic, 512).map(drop)),
        Errno::EINVAL
    );
    assert_eq!(
        errno(m.statmount(parent + 1000, basic, 512).map(drop)),
        Errno::ENOENT
    );
}

#[test]
fn statmount_strings_follow_the_record() {
    let m = Machine::as_uid(0);
    m.mkdir("/t");
    m.tmpfs("/t");
    m.mkdir("/t/d");
    m.mkdir("/u");
    assert_eq!(
        m.mount(Some("/t/d"), "/u", None, MsFlags::MS_BIND, None),
        Ok(0)
    );
    let id = m.mnt_id("/u", StatxMask::STATX_MNT_ID_UNIQUE);
    let mask = StatmountMask(
        StatmountMask::STATMOUNT_FS_TYPE.0
            | StatmountMask::STATMOUNT_MNT_ROOT.0
            | StatmountMask::STATMOUNT_MNT_POINT.0
            | StatmountMask::STATMOUNT_SB_SOURCE.0
            | StatmountMask::STATMOUNT_SB_BASIC.0
            | StatmountMask::STATMOUNT_SUPPORTED_MASK.0,
    );
    assert_eq!(
        errno(m.statmount(id, mask, Statmount::SIZE as u32).map(drop)),
        Errno::EOVERFLOW
    );
    let (sm, strings) = m.statmount(id, mask, 2048).unwrap();
    assert_eq!(sm.mask, mask.0);
    assert_eq!(sm_string(&strings, sm.fs_type), "tmpfs");
    assert_eq!(sm_string(&strings, sm.mnt_root), "/d");
    assert_eq!(sm_string(&strings, sm.mnt_point), "/u");
    assert_eq!(sm_string(&strings, sm.sb_source), "none");
    assert_eq!(sm.sb_magic, 0x0102_1994);
    assert_eq!(
        sm.size as usize,
        Statmount::SIZE + strings.iter().rposition(|&b| b != 0).unwrap() + 2
    );
    assert_ne!(sm.supported_mask & StatmountMask::STATMOUNT_MNT_BASIC.0, 0);
    // No room for the strings.
    assert_eq!(
        errno(m.statmount(id, mask, Statmount::SIZE as u32 + 4).map(drop)),
        Errno::EOVERFLOW
    );
}

/// `statmount08` and `listmount03`: after `chroot`, a mount outside the new root is not shown
/// to a caller without `CAP_SYS_ADMIN`.
#[test]
fn statmount_and_listmount_hide_mounts_outside_the_root_from_users() {
    let m = Machine::as_uid(0);
    m.mkdir("/jail");
    m.mkdir("/jail/in");
    m.tmpfs("/jail/in");
    let root_id = m.mnt_id("/", StatxMask::STATX_MNT_ID_UNIQUE);
    let in_id = m.mnt_id("/jail/in", StatxMask::STATX_MNT_ID_UNIQUE);
    let all = m.listmount(ListmountMntId::LSMT_ROOT.0, 0).unwrap();
    assert!(all.contains(&root_id) && all.contains(&in_id));
    assert_eq!(
        m.listmount(root_id, in_id).unwrap(),
        Vec::<u64>::new(),
        "after the last"
    );

    let jail = m.cstr(0x1000, "/jail");
    assert_eq!(block_on(m.kernel.chroot(&m.caller(), jail)), Ok(0));
    let basic = StatmountMask::STATMOUNT_SB_BASIC;
    assert!(m.statmount(root_id, basic, 512).is_ok(), "root may look");
    assert_eq!(block_on(m.kernel.setuid(&m.caller(), 65534)), Ok(0));
    assert_eq!(
        errno(m.statmount(root_id, basic, 512).map(drop)),
        Errno::EPERM
    );
    assert_eq!(errno(m.listmount(root_id, 0)), Errno::EPERM);
    assert!(m.statmount(in_id, basic, 512).is_ok());
    assert_eq!(
        m.listmount(ListmountMntId::LSMT_ROOT.0, 0).unwrap(),
        vec![in_id]
    );
}

/// The machine's memory with a low guard of `.1` bytes, where reads go through and writes
/// fault, as the execution layer's (DESIGN.md, "Guard page at 0").
struct Guarded<'a>(&'a ForkableMemory, u32);

impl UserMemory for Guarded<'_> {
    fn read<'a>(&'a self, addr: u32, buf: &'a mut [u8]) -> LocalFuture<'a, Result<(), Fault>> {
        self.0.read(addr, buf)
    }

    fn write<'a>(&'a self, addr: u32, data: &'a [u8]) -> LocalFuture<'a, Result<(), Fault>> {
        match addr < self.1 {
            true => Box::pin(async { Err(Fault) }),
            false => self.0.write(addr, data),
        }
    }

    fn size(&self) -> LocalFuture<'_, u64> {
        self.0.size()
    }

    fn grow(&self, pages: u32) -> LocalFuture<'_, Option<u64>> {
        self.0.grow(pages)
    }

    fn low_guard(&self) -> LocalFuture<'_, u32> {
        let guard = self.1;
        Box::pin(async move { guard })
    }
}

/// `listmount04` and `statmount` alike: a request in the low guard, which reads zeros, faults
/// as in Linux's unmapped null page instead of reading as a request of size 0 (`EINVAL`).
#[test]
fn a_mount_id_request_in_the_low_guard_faults_as_listmount04() {
    let m = Machine::as_uid(0);
    let memory = Guarded(&m.memory, 0x100);
    let caller = Caller::new(m.task, &memory);
    let null = GuestPtr::<MntIdReq>::NULL;
    let out = GuestPtrMut::new(0x6000);
    assert_eq!(
        errno(block_on(m.kernel.listmount(&caller, null, out, 32, 0))),
        Errno::EFAULT
    );
    let buf = GuestPtrMut::new(0x6000);
    assert_eq!(
        errno(block_on(m.kernel.statmount(&caller, null, buf, 512, 0))),
        Errno::EFAULT
    );
    // Unknown flags come first, as in Linux.
    assert_eq!(
        errno(block_on(m.kernel.listmount(&caller, null, out, 32, !0))),
        Errno::EINVAL
    );
    // Above the guard, a request of size 0 is EINVAL.
    let req = m.mnt_id_req(ListmountMntId::LSMT_ROOT.0, 0);
    put(&m.memory.0, 0x5000, &0u32.to_le_bytes());
    assert_eq!(
        errno(block_on(m.kernel.listmount(&caller, req, out, 32, 0))),
        Errno::EINVAL
    );
    let req = m.mnt_id_req(ListmountMntId::LSMT_ROOT.0, 0);
    assert!(block_on(m.kernel.listmount(&caller, req, out, 32, 0)).is_ok());
}

#[test]
fn mountinfo_shows_a_tmpfs_s_options() {
    let m = Machine::as_uid(0);
    m.kernel
        .register_filesystem("sized", Arc::new(SizedTmpfs))
        .unwrap();
    m.mkdir("/s");
    assert_eq!(
        m.mount(Some("none"), "/s", Some("sized"), MsFlags(0), None),
        Ok(0)
    );
    let line = m.mountinfo("/s").unwrap();
    assert!(
        line.ends_with(" - tmpfs none rw,size=4k,nr_inodes=8,mode=700"),
        "{line}"
    );
    assert!(
        m.cat("/proc/self/mounts")
            .contains("none /s tmpfs rw,relatime,size=4k,nr_inodes=8,mode=700 0 0")
    );
}

/// A tmpfs of 4 KiB and 8 inodes with a private root, whatever the options.
struct SizedTmpfs;

impl FilesystemType for SizedTmpfs {
    fn mount(&self, _: &[u8], _: &[FsParam<'_>], _: &Cred) -> Result<Arc<dyn Filesystem>, Errno> {
        let clock = Arc::new(HostClock(Arc::new(TestHost) as Arc<dyn Host>));
        let limits = wasixv2_vfs_mem::Limits {
            max_bytes: Some(4096),
            max_inodes: Some(8),
        };
        Ok(Arc::new(MemFs::with_limits(clock, 0o700, 0, 0, limits)))
    }
}

/// `mq_open` of `name` (no leading slash, as libc passes it) for `caller`, read and write.
fn mq_open(m: &Machine, caller: &Caller<'_>, name: &str, flags: OpenFlags) -> SysResult<i32> {
    let mut bytes = name.as_bytes().to_vec();
    bytes.push(0);
    block_on(caller.memory().write(0x1500, &bytes)).unwrap();
    let flags = flags | OpenFlags::O_RDWR;
    block_on(
        m.kernel
            .mq_open(caller, GuestPtr::new(0x1500), flags, 0o600, GuestPtr::NULL),
    )
}

/// The status line of the queue file `at`.
fn mq_status(m: &Machine, at: &str) -> String {
    m.cat(at)
}

/// `mount -t mqueue` (LTP mqns_03, mqns_04): the mounter's IPC namespace's queues, as files of
/// one filesystem however often it is mounted. `mq_open` and the files agree: a queue made
/// either way is the other's, opening a file gives the queue (its status line, its messages),
/// and `unlink` is `mq_unlink`. The root is sticky and world-writable. A mount outlives its
/// namespace, still showing its queues, but makes none (`EACCES`).
#[test]
fn an_mqueue_mount_shows_its_ipc_namespaces_queues() {
    let m = Machine::as_uid(0);
    m.mkdir("/mnt");
    let caller = m.caller();
    let none = MsFlags(0);
    let mqueue = |data: Option<&str>| m.mount(Some("mqueue"), "/mnt", Some("mqueue"), none, data);
    assert!(m.cat("/proc/filesystems").contains("nodev\tmqueue\n"));
    assert!(mq_open(&m, &caller, "initial", OpenFlags::O_CREAT).is_ok());

    assert_eq!(m.unshare(CloneFlags::CLONE_NEWIPC), Ok(0));
    let q1 = mq_open(&m, &caller, "MQ1", OpenFlags::O_CREAT | OpenFlags::O_EXCL).unwrap();
    assert_eq!(errno(mqueue(Some("size=1"))), Errno::EINVAL);
    assert_eq!(mqueue(None), Ok(0));
    assert!(m.exists("/mnt/MQ1"));
    assert!(!m.exists("/mnt/initial"), "the initial namespace's");
    let root = m.stat("/mnt");
    assert_eq!((root.stx_mode & 0o7777, root.stx_uid), (0o1777, 0));
    let file = m.stat("/mnt/MQ1");
    assert_eq!((file.stx_mode & 0o7777, file.stx_size), (0o600, 80));

    // A message shows in the file's status line, and the file is the queue.
    put(&m.memory.0, 0x5100, b"hello");
    let send = |fd: i32| {
        let msg = GuestPtr::new(0x5100);
        block_on(
            m.kernel
                .mq_timedsend_time64(&caller, fd, msg, 5, 1, GuestPtr::NULL),
        )
    };
    assert_eq!(send(q1), Ok(0));
    assert_eq!(
        mq_status(&m, "/mnt/MQ1"),
        "QSIZE:5          NOTIFY:0     SIGNO:0     NOTIFY_PID:0     \n"
    );
    let name = m.cstr(0x1000, "/mnt/MQ1");
    let file_fd = block_on(m.kernel.openat(&caller, CWD, name, OpenFlags::O_WRONLY, 0)).unwrap();
    assert_eq!(send(file_fd.0), Ok(0));
    assert!(mq_status(&m, "/mnt/MQ1").starts_with("QSIZE:10 "));

    // creat() makes a queue (`mqueue_create`); unlink removes it as mq_unlink does.
    let fd = m.create("/mnt/MQ2").unwrap();
    m.close(fd);
    let q2 = mq_open(&m, &caller, "MQ2", OpenFlags::empty()).unwrap();
    assert_eq!(m.mkdir_errno("/mnt/dir"), Errno::EPERM);
    let name = m.cstr(0x1000, "/mnt/MQ2");
    assert_eq!(
        block_on(m.kernel.unlinkat(&caller, CWD, name, AtFlags::empty())),
        Ok(0)
    );
    assert_eq!(
        errno(mq_open(&m, &caller, "MQ2", OpenFlags::empty())),
        Errno::ENOENT
    );

    // The same filesystem at every mount and after a remount; a descriptor open through the
    // mount keeps it busy, as on Linux, one from mq_open does not.
    assert_eq!(m.umount2("/mnt", UmountBits(0)), Err(Errno::EBUSY.into()));
    m.close(file_fd);
    assert_eq!(m.umount2("/mnt", UmountBits(0)), Ok(0));
    assert!(!m.exists("/mnt/MQ1"));
    assert_eq!(mqueue(None), Ok(0));
    assert!(m.exists("/mnt/MQ1"));

    // The namespace goes once nothing uses it; its mount stays, and makes no queue.
    for fd in [Fd(q1), Fd(q2)] {
        m.close(fd);
    }
    assert_eq!(m.unshare(CloneFlags::CLONE_NEWIPC), Ok(0));
    assert!(m.exists("/mnt/MQ1"));
    assert_eq!(errno(m.create("/mnt/MQ3")), Errno::EACCES);
    assert_eq!(
        errno(mq_open(&m, &caller, "MQ1", OpenFlags::empty())),
        Errno::ENOENT,
        "not the new namespace's"
    );
}

/// mqns_04: a child in a new IPC namespace mounts its queues and exits; once it is gone, the
/// namespace is too (`exit_task_namespaces`, `exit_sem`), even while something still holds the
/// process and before the execution layer lets its task go, and the mount the parent sees
/// shows the queue but makes none (`EACCES`).
#[test]
fn an_mqueue_mount_outlives_the_process_whose_namespace_it_shows() {
    let m = Machine::as_uid(0);
    m.mkdir("/mnt");
    let null = GuestPtrMut::NULL;
    let flags = Signal::SIGCHLD.0 as u32 | CloneFlags::CLONE_NEWIPC.0 as u32;
    let pid = block_on(m.kernel.clone(&m.caller(), flags, 0, null, 0, null)).unwrap();
    let Some(GuestAction::Fork { child, .. }) = m.kernel.take_pending(m.task) else {
        panic!("no child");
    };
    let held = m.kernel.procs.find(child.0);
    let memory = ForkableMemory(FlatMemory(RefCell::new(vec![0; 65536])));
    let caller = Caller::new(child, &memory);
    let q = mq_open(&m, &caller, "MQ1", OpenFlags::O_CREAT).unwrap();
    assert_eq!(block_on(m.kernel.close(&caller, Fd(q))), Ok(0));
    put(&memory.0, 0x1100, b"mqueue\0");
    put(&memory.0, 0x1200, b"/mnt\0");
    let mount = m.kernel.mount(
        &caller,
        GuestPtr::new(0x1100),
        GuestPtr::new(0x1200),
        GuestPtr::new(0x1100),
        MsFlags(0),
        GuestPtr::NULL,
    );
    assert_eq!(block_on(mount), Ok(0));
    assert!(m.exists("/mnt/MQ1"));
    assert_eq!(block_on(m.kernel.exit_group(&caller, 0)), Ok(()));
    block_on(m.kernel.exit_mm(&caller));
    let options = WaitOptions(WaitOptions::WEXITED.0);
    let info = SiginfoPtr::new(0x300);
    let parent = m.caller();
    let wait = m
        .kernel
        .waitid(&parent, Idtype::P_PID, pid, info, options, RusagePtr::NULL);
    assert_eq!(block_on(wait), Ok(0));

    assert!(held.is_some());
    assert!(m.exists("/mnt/MQ1"));
    assert_eq!(errno(m.create("/mnt/MQ2")), Errno::EACCES);
    // The execution layer lets the task go only now.
    m.kernel.release(child);
}

#[test]
fn fanotify_mount_namespace_events_report_attach_move_and_detach() {
    use wasixv2_abi::{FanotifyInitFlags as F, FanotifyMarkFlags as M, FanotifyMask as E};
    let m = Machine::as_uid(0);
    for name in ["/a", "/b"] {
        m.mkdir(name);
    }
    let flags = F::FAN_REPORT_MNT | F::FAN_NONBLOCK;
    let group = Fd(block_on(m.kernel.fanotify_init(&m.caller(), flags.0, 0)).unwrap());
    let ns = m.cstr(0x1500, "/proc/self/ns/mnt");
    let add = M::FAN_MARK_ADD | M::FAN_MARK_MNTNS;
    let wanted = E::FAN_MNT_ATTACH | E::FAN_MNT_DETACH;
    assert_eq!(
        block_on(
            m.kernel
                .fanotify_mark(&m.caller(), group, add.0, wanted.0, CWD, ns)
        ),
        Ok(0)
    );
    let read = || {
        let n = block_on(
            m.kernel
                .read(&m.caller(), group, UserSliceMut::new(0x5000, 256)),
        )
        .unwrap();
        assert_eq!(n, 40);
        let raw = get(&m.memory.0, 0x5000, 40);
        assert_eq!(i32::from_ne_bytes(raw[16..20].try_into().unwrap()), -1);
        assert_eq!(raw[24], 7); // FAN_EVENT_INFO_TYPE_MNT
        assert_eq!(u16::from_ne_bytes(raw[26..28].try_into().unwrap()), 16);
        (
            u64::from_ne_bytes(raw[8..16].try_into().unwrap()),
            u64::from_ne_bytes(raw[32..40].try_into().unwrap()),
        )
    };
    m.tmpfs("/a");
    let (mask, id) = read();
    assert_eq!(mask, E::FAN_MNT_ATTACH.0);
    assert!(id >= MNT_UNIQUE_ID_OFFSET);
    assert_eq!(
        m.mount(Some("/a"), "/b", None, MsFlags::MS_MOVE, None),
        Ok(0)
    );
    assert_eq!(read(), (wanted.0, id));
    assert_eq!(m.umount2("/b", UmountBits::MNT_DETACH), Ok(0));
    assert_eq!(read(), (E::FAN_MNT_DETACH.0, id));
    let flush = M::FAN_MARK_FLUSH | M::FAN_MARK_MNTNS;
    assert_eq!(
        block_on(
            m.kernel
                .fanotify_mark(&m.caller(), group, flush.0, 0, CWD, GuestPtr::NULL)
        ),
        Ok(0)
    );
    m.tmpfs("/a");
    assert_eq!(
        errno(block_on(m.kernel.read(
            &m.caller(),
            group,
            UserSliceMut::new(0x5000, 256)
        ))),
        Errno::EAGAIN
    );
    // Namespace marks require a mount-reporting group and an actual mount namespace inode.
    let plain = Fd(block_on(m.kernel.fanotify_init(&m.caller(), F::FAN_REPORT_FID.0, 0)).unwrap());
    assert_eq!(
        errno(block_on(m.kernel.fanotify_mark(
            &m.caller(),
            plain,
            add.0,
            wanted.0,
            CWD,
            ns
        ))),
        Errno::EINVAL
    );
    let file = m.cstr(0x1500, "/proc/self/ns/user");
    assert_eq!(
        errno(block_on(m.kernel.fanotify_mark(
            &m.caller(),
            group,
            add.0,
            wanted.0,
            CWD,
            file
        ))),
        Errno::EINVAL
    );
    assert_eq!(
        errno(block_on(m.kernel.fanotify_init(
            &m.caller(),
            (flags | F::FAN_CLASS_CONTENT).0,
            0
        ))),
        Errno::EINVAL
    );
}

#[test]
fn fchroot_uses_fd_permissions_and_leaves_cwd() {
    let m = Machine::as_uid(0);
    m.mkdir("/jail");
    let root = m.open_path("/");
    let jail = m.open_path("/jail");
    let regular = m.create("/file").unwrap();
    assert_eq!(
        errno(block_on(m.kernel.fchroot(&m.caller(), Fd(-1), 1))),
        Errno::EINVAL
    );
    assert_eq!(
        errno(block_on(m.kernel.fchroot(&m.caller(), Fd(-1), 0))),
        Errno::EBADF
    );
    assert_eq!(
        errno(block_on(m.kernel.fchroot(&m.caller(), regular, 0))),
        Errno::ENOTDIR
    );
    assert_eq!(block_on(m.kernel.fchroot(&m.caller(), jail, 0)), Ok(0));
    let fs = m.kernel.env(&m.caller()).fs;
    assert_eq!(fs.root, m.kernel.fd_path(&m.caller(), jail).unwrap());
    assert_eq!(fs.cwd, m.kernel.fd_path(&m.caller(), root).unwrap());
    assert!(!m.exists("/file"));
    assert_eq!(block_on(m.kernel.fchroot(&m.caller(), root, 0)), Ok(0));
    assert!(m.exists("/file"));

    let user = Machine::as_uid(1);
    let fd = user.open_path("/");
    assert_eq!(
        errno(block_on(user.kernel.fchroot(&user.caller(), fd, 0))),
        Errno::EPERM
    );
    user.mkdir("/private");
    let fd = user.open_path("/private");
    let name = user.cstr(0x1000, "/private");
    block_on(user.kernel.fchmodat(&user.caller(), CWD, name, 0)).unwrap();
    assert_eq!(
        errno(block_on(user.kernel.fchroot(&user.caller(), fd, 0))),
        Errno::EACCES
    );
}

#[test]
fn open_tree_attr_changes_detached_mount_before_publication() {
    let m = Machine::as_uid(0);
    m.mkdir("/source");
    m.mkdir("/target");
    m.tmpfs("/source");
    m.close(m.create("/source/file").unwrap());
    let attr = 0x5000;
    let mut bytes = vec![0; 40];
    bytes[..8].copy_from_slice(&MountAttrFlags::MOUNT_ATTR_RDONLY.0.to_le_bytes());
    put(&m.memory.0, attr, &bytes);
    let call = |name: &str, flags, ptr, size| {
        block_on(m.kernel.open_tree_attr(
            &m.caller(),
            CWD,
            m.cstr(0x1000, name),
            flags,
            GuestPtr::new(ptr),
            size,
        ))
    };
    assert_eq!(errno(call("/source", 0, 0, 32)), Errno::EINVAL);
    assert_eq!(errno(call("/missing", 0, attr, 1)), Errno::ENOENT);
    assert_eq!(
        errno(call("/source", OPEN_TREE_CLONE, attr, 31)),
        Errno::EINVAL
    );
    let fd = Fd(call(
        "/source",
        OPEN_TREE_CLONE | OpenFlags::O_CLOEXEC.0,
        attr,
        40,
    )
    .unwrap());
    assert_eq!(
        m.move_mount(
            (fd, ""),
            (CWD, "/target"),
            MoveMountFlags::MOVE_MOUNT_F_EMPTY_PATH.0
        ),
        Ok(0)
    );
    assert_eq!(errno(m.create("/target/new")), Errno::EROFS);
    m.close(m.create("/source/new").unwrap());
    m.close(fd);

    // A failed attribute copy must not leave any new descriptor installed.
    let before = m.open_path("/");
    m.close(before);
    assert_eq!(
        errno(call("/source", OPEN_TREE_CLONE, 0xfffffff0, 32)),
        Errno::EFAULT
    );
    let after = m.open_path("/");
    assert_eq!(after, before);
    m.close(after);
    let plain = Fd(call("/source", 0, 0, 0).unwrap());
    m.close(plain);
}
