//! Mount namespaces and `/proc/<pid>/mountinfo`.

use wasixv2_abi::CloneFlags;
use wasixv2_vfs::{Cred, Ctx, LookupFlags, MountFlags, MountOp, Propagation};
use wasixv2_vfs_mem::MemFs;

use super::{
    ipc::{errno, get, put},
    proc::ForkableMemory,
    *,
};

const CWD: Fd = Fd(-100);

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
