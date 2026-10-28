//! Mount namespaces, the `mount` family of syscalls and `/proc/<pid>/mountinfo`.

use wasixv2_abi::{CloneFlags, MountAttrFlags, MountFlags as MsFlags, UmountFlags as UmountBits};
use wasixv2_vfs::{Cred, Ctx, LookupFlags, MountFlags, MountOp, Propagation};
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
