//! Landlock: rulesets, path-beneath rules over the VFS, network ports, scopes and inheritance.

use wasixv2_abi::{AtFlags, ConstVoidPtr, GuestPtr, PrctlOption, RenameFlags, Signal};

use super::ipc::{errno, put};
use super::proc::ForkableMemory;
use super::*;

const CWD: Fd = Fd(-100);

const WRITE_FILE: u64 = 1 << 1;
const READ_FILE: u64 = 1 << 2;
const READ_DIR: u64 = 1 << 3;
const REMOVE_FILE: u64 = 1 << 5;
const MAKE_DIR: u64 = 1 << 7;
const MAKE_REG: u64 = 1 << 8;
const REFER: u64 = 1 << 13;
const TRUNCATE: u64 = 1 << 14;

const BIND_TCP: u64 = 1;
const CONNECT_TCP: u64 = 2;

const SCOPE_SIGNAL: u64 = 2;

/// One task's calls, with its memory at hand. Paths go at 0x1000 and 0x1400, attributes at
/// 0x3000.
struct T<'a> {
    kernel: &'a Kernel,
    caller: Caller<'a>,
    memory: &'a FlatMemory,
}

impl<'a> T<'a> {
    fn new(kernel: &'a Kernel, task: TaskId, memory: &'a FlatMemory) -> Self {
        Self {
            kernel,
            caller: Caller::new(task, memory),
            memory,
        }
    }

    fn cstr(&self, at: u32, path: &str) -> Cstr {
        let mut bytes = path.as_bytes().to_vec();
        bytes.push(0);
        put(self.memory, at, &bytes);
        GuestPtr::new(at)
    }

    fn open(&self, path: &str, flags: OpenFlags) -> SysResult<Fd> {
        let name = self.cstr(0x1000, path);
        block_on(self.kernel.openat(&self.caller, CWD, name, flags, 0o644))
    }

    fn mkdir(&self, path: &str) -> SysResult<i32> {
        let name = self.cstr(0x1000, path);
        block_on(self.kernel.mkdirat(&self.caller, CWD, name, 0o755))
    }

    fn create(&self, path: &str) -> SysResult<()> {
        let fd = self.open(path, OpenFlags::O_CREAT | OpenFlags::O_WRONLY)?;
        block_on(self.kernel.close(&self.caller, fd)).map(drop)
    }

    fn unlink(&self, path: &str) -> SysResult<i32> {
        let name = self.cstr(0x1000, path);
        block_on(self.kernel.unlinkat(&self.caller, CWD, name, AtFlags(0)))
    }

    fn link(&self, old: &str, new: &str) -> SysResult<i32> {
        let (old, new) = (self.cstr(0x1000, old), self.cstr(0x1400, new));
        block_on(
            self.kernel
                .linkat(&self.caller, CWD, old, CWD, new, AtFlags(0)),
        )
    }

    fn rename(&self, old: &str, new: &str) -> SysResult<i32> {
        let (old, new) = (self.cstr(0x1000, old), self.cstr(0x1400, new));
        let rename = self
            .kernel
            .renameat2(&self.caller, CWD, old, CWD, new, RenameFlags(0));
        block_on(rename)
    }

    fn prctl_nnp(&self) {
        let set = self
            .kernel
            .prctl(&self.caller, PrctlOption::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0);
        assert_eq!(block_on(set), Ok(0));
    }

    /// `landlock_create_ruleset` with a 48-byte attribute.
    fn ruleset(&self, fs: u64, net: u64, scoped: u64) -> SysResult<i32> {
        let attr: Vec<u8> = [fs, net, scoped, 0, 0, 0]
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect();
        put(self.memory, 0x3000, &attr);
        block_on(
            self.kernel
                .landlock_create_ruleset(&self.caller, GuestPtr::new(0x3000), 48, 0),
        )
    }

    fn create_raw(&self, attr: u32, size: u32, flags: u32) -> SysResult<i32> {
        block_on(self.kernel.landlock_create_ruleset(
            &self.caller,
            GuestPtr::new(attr),
            size,
            flags,
        ))
    }

    /// A path-beneath rule granting `access` under `dir`.
    fn allow_path(&self, ruleset: i32, dir: &str, access: u64) -> SysResult<i32> {
        let fd = self.open(dir, OpenFlags::O_PATH)?;
        let result = self.allow_fd(ruleset, fd, access);
        block_on(self.kernel.close(&self.caller, fd)).unwrap();
        result
    }

    fn allow_fd(&self, ruleset: i32, fd: Fd, access: u64) -> SysResult<i32> {
        let mut attr = access.to_le_bytes().to_vec();
        attr.extend(fd.0.to_le_bytes());
        put(self.memory, 0x3100, &attr);
        let add = self.kernel.landlock_add_rule(
            &self.caller,
            Fd(ruleset),
            1,
            ConstVoidPtr::new(0x3100),
            0,
        );
        block_on(add)
    }

    fn allow_port(&self, ruleset: i32, port: u64, access: u64) -> SysResult<i32> {
        let attr: Vec<u8> = [access, port]
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect();
        put(self.memory, 0x3200, &attr);
        let add = self.kernel.landlock_add_rule(
            &self.caller,
            Fd(ruleset),
            2,
            ConstVoidPtr::new(0x3200),
            0,
        );
        block_on(add)
    }

    fn restrict(&self, ruleset: i32) -> SysResult<i32> {
        block_on(
            self.kernel
                .landlock_restrict_self(&self.caller, Fd(ruleset), 0),
        )
    }
}

/// The ABI version and errata come back with their flag alone; anything else with them, or an
/// unknown flag, is EINVAL.
#[test]
fn version_and_errata() {
    let (kernel, task, memory) = super::ipc::machine();
    let t = T::new(&kernel, task, &memory);
    assert_eq!(t.create_raw(0, 0, 1), Ok(10));
    assert_eq!(t.create_raw(0, 0, 2), Ok(7));
    assert_eq!(errno(t.create_raw(0, 0, 3)), Errno::EINVAL);
    assert_eq!(errno(t.create_raw(0x3000, 0, 1)), Errno::EINVAL);
    assert_eq!(errno(t.create_raw(0, 8, 1)), Errno::EINVAL);
}

/// `copy_min_struct_from_user` and the masks: EINVAL for too little or unknown rights, EFAULT,
/// E2BIG for a tail it does not know, ENOMSG for nothing handled.
#[test]
fn ruleset_attributes_are_checked() {
    let (kernel, task, memory) = super::ipc::machine();
    let t = T::new(&kernel, task, &memory);
    put(&memory, 0x3000, &[0; 64]);
    put(&memory, 0x3000, &READ_FILE.to_le_bytes());
    assert_eq!(errno(t.create_raw(0x3000, 7, 0)), Errno::EINVAL);
    assert_eq!(errno(t.create_raw(0, 8, 0)), Errno::EFAULT);
    assert_eq!(errno(t.create_raw(0x3000, 4097, 0)), Errno::E2BIG);
    put(&memory, 0x3000 + 56, &[1]);
    assert_eq!(errno(t.create_raw(0x3000, 64, 0)), Errno::E2BIG);
    let fd = t.create_raw(0x3000, 8, 0).unwrap();
    assert!(fd >= 3);
    assert_eq!(errno(t.ruleset(1 << 17, 0, 0)), Errno::EINVAL);
    assert_eq!(errno(t.ruleset(0, 1 << 4, 0)), Errno::EINVAL);
    assert_eq!(errno(t.ruleset(0, 0, 1 << 2)), Errno::EINVAL);
    assert_eq!(errno(t.ruleset(0, 0, 0)), Errno::ENOMSG);
    // A quiet mask must be part of what is handled.
    let attr: Vec<u8> = [READ_FILE, 0, 0, WRITE_FILE, 0, 0]
        .iter()
        .flat_map(|v| v.to_le_bytes())
        .collect();
    put(&memory, 0x3000, &attr);
    assert_eq!(errno(t.create_raw(0x3000, 48, 0)), Errno::EINVAL);
}

/// `landlock_add_rule`: a ruleset descriptor (EBADF, EBADFD), a useful rule (ENOMSG), rights
/// the ruleset handles and that fit a file (EINVAL), a real parent (EBADFD for a pipe), and a
/// port that fits 16 bits.
#[test]
fn rules_are_checked() {
    let (kernel, task, memory) = super::ipc::machine();
    let t = T::new(&kernel, task, &memory);
    t.mkdir("/d").unwrap();
    t.create("/d/f").unwrap();
    let ruleset = t.ruleset(READ_FILE | READ_DIR, BIND_TCP, 0).unwrap();
    assert_eq!(errno(t.allow_path(42, "/d", READ_FILE)), Errno::EBADF);
    assert_eq!(errno(t.allow_path(0, "/d", READ_FILE)), Errno::EBADFD);
    assert_eq!(errno(t.allow_path(ruleset, "/d", 0)), Errno::ENOMSG);
    assert_eq!(
        errno(t.allow_path(ruleset, "/d", WRITE_FILE)),
        Errno::EINVAL
    );
    assert_eq!(
        errno(t.allow_path(ruleset, "/d/f", READ_DIR)),
        Errno::EINVAL
    );
    assert_eq!(t.allow_path(ruleset, "/d/f", READ_FILE), Ok(0));
    assert_eq!(t.allow_path(ruleset, "/d", READ_FILE | READ_DIR), Ok(0));
    let (read, _) = super::ipc::pipe(&kernel, &t.caller, OpenFlags(0));
    assert_eq!(errno(t.allow_fd(ruleset, read, READ_FILE)), Errno::EBADFD);
    assert_eq!(
        errno(t.allow_fd(ruleset, Fd(ruleset), READ_FILE)),
        Errno::EBADFD
    );
    assert_eq!(errno(t.allow_port(ruleset, 65536, BIND_TCP)), Errno::EINVAL);
    assert_eq!(errno(t.allow_port(ruleset, 80, CONNECT_TCP)), Errno::EINVAL);
    assert_eq!(t.allow_port(ruleset, 80, BIND_TCP), Ok(0));
    let add = kernel.landlock_add_rule(&t.caller, Fd(ruleset), 3, ConstVoidPtr::new(0x3100), 0);
    assert_eq!(errno(block_on(add)), Errno::EINVAL);
}

/// Enforcing takes `no_new_privs` (EPERM); then what the domain handles is allowed only beneath
/// the directories whose rules grant it, for opens, creation and removal. Mounts are refused.
#[test]
fn path_beneath_rules_confine_the_filesystem() {
    let (kernel, task, memory) = super::ipc::machine();
    let t = T::new(&kernel, task, &memory);
    for dir in ["/a", "/a/sub", "/b"] {
        t.mkdir(dir).unwrap();
    }
    for file in ["/a/f", "/a/sub/g", "/b/h"] {
        t.create(file).unwrap();
    }
    let handled = READ_FILE | WRITE_FILE | READ_DIR | MAKE_REG | MAKE_DIR | REMOVE_FILE;
    let ruleset = t.ruleset(handled, 0, 0).unwrap();
    assert_eq!(
        t.allow_path(ruleset, "/a", READ_FILE | WRITE_FILE | READ_DIR | MAKE_REG),
        Ok(0)
    );
    assert_eq!(errno(t.restrict(ruleset)), Errno::EPERM);
    t.prctl_nnp();
    assert_eq!(t.restrict(ruleset), Ok(0));

    assert!(t.open("/a/sub/g", OpenFlags::O_RDWR).is_ok());
    assert!(
        t.open("/a", OpenFlags::O_RDONLY | OpenFlags::O_DIRECTORY)
            .is_ok()
    );
    assert_eq!(errno(t.open("/b/h", OpenFlags::O_RDONLY)), Errno::EACCES);
    assert_eq!(
        errno(t.open("/", OpenFlags::O_RDONLY | OpenFlags::O_DIRECTORY)),
        Errno::EACCES
    );
    // O_PATH opens nothing.
    assert!(t.open("/b/h", OpenFlags::O_PATH).is_ok());
    assert!(t.create("/a/new").is_ok());
    assert_eq!(errno(t.create("/b/new")), Errno::EACCES);
    assert_eq!(errno(t.mkdir("/a/dir")), Errno::EACCES);
    assert_eq!(errno(t.unlink("/a/f")), Errno::EACCES);
    // What the domain does not handle is not its business.
    let rmdir = kernel.unlinkat(&t.caller, CWD, t.cstr(0x1000, "/b"), AtFlags::AT_REMOVEDIR);
    assert_eq!(errno(block_on(rmdir)), Errno::ENOTEMPTY);
    let mount = kernel.mount(
        &t.caller,
        t.cstr(0x1000, "none"),
        t.cstr(0x1400, "/a"),
        t.cstr(0x1800, "tmpfs"),
        wasixv2_abi::MountFlags(0),
        ConstVoidPtr::NULL,
    );
    assert_eq!(errno(block_on(mount)), Errno::EPERM);
}

/// Truncation needs LANDLOCK_ACCESS_FS_TRUNCATE at open time: O_TRUNC, truncate(2) and an
/// ftruncate of a file opened under the domain; a file opened before keeps its rights.
#[test]
fn truncation_is_a_right_of_its_own() {
    let (kernel, task, memory) = super::ipc::machine();
    let t = T::new(&kernel, task, &memory);
    t.mkdir("/d").unwrap();
    t.create("/d/f").unwrap();
    let before = t.open("/d/f", OpenFlags::O_RDWR).unwrap();
    let ruleset = t.ruleset(READ_FILE | WRITE_FILE | TRUNCATE, 0, 0).unwrap();
    assert_eq!(t.allow_path(ruleset, "/d", READ_FILE | WRITE_FILE), Ok(0));
    t.prctl_nnp();
    assert_eq!(t.restrict(ruleset), Ok(0));

    assert_eq!(
        errno(t.open("/d/f", OpenFlags::O_RDWR | OpenFlags::O_TRUNC)),
        Errno::EACCES
    );
    let after = t.open("/d/f", OpenFlags::O_RDWR).unwrap();
    assert_eq!(
        errno(block_on(kernel.ftruncate64(&t.caller, after, 0))),
        Errno::EACCES
    );
    assert_eq!(block_on(kernel.ftruncate64(&t.caller, before, 0)), Ok(0));
    let truncate = kernel.truncate64(&t.caller, t.cstr(0x1000, "/d/f"), 0);
    assert_eq!(errno(block_on(truncate)), Errno::EACCES);
}

/// Moving or linking across directories takes LANDLOCK_ACCESS_FS_REFER on both sides (EXDEV
/// without, so programs copy instead) and no more rights where the file goes; within one
/// directory, only the rights to make and remove there.
#[test]
fn reparenting_needs_refer_and_no_more_rights() {
    let (kernel, task, memory) = super::ipc::machine();
    let t = T::new(&kernel, task, &memory);
    for dir in ["/src", "/dst", "/wide"] {
        t.mkdir(dir).unwrap();
    }
    t.create("/src/f").unwrap();
    t.create("/src/g").unwrap();
    t.create("/wide/w").unwrap();
    let make = MAKE_REG | REMOVE_FILE;
    let ruleset = t.ruleset(READ_FILE | make | REFER, 0, 0).unwrap();
    assert_eq!(t.allow_path(ruleset, "/src", make | REFER), Ok(0));
    assert_eq!(t.allow_path(ruleset, "/dst", make), Ok(0));
    assert_eq!(
        t.allow_path(ruleset, "/wide", make | REFER | READ_FILE),
        Ok(0)
    );
    t.prctl_nnp();
    assert_eq!(t.restrict(ruleset), Ok(0));

    assert_eq!(t.rename("/src/f", "/src/f2"), Ok(0));
    assert_eq!(errno(t.rename("/src/f2", "/dst/f")), Errno::EXDEV);
    assert_eq!(errno(t.link("/src/g", "/dst/g")), Errno::EXDEV);
    // /wide would let the file be read, which it could not be where it was.
    assert_eq!(errno(t.rename("/src/f2", "/wide/f")), Errno::EXDEV);
    // From /wide to /src the file only loses rights.
    assert_eq!(t.rename("/wide/w", "/src/w"), Ok(0));
}

/// A domain is inherited by children and stacks: each restriction adds a layer, up to 16.
#[test]
fn domains_are_inherited_and_stack() {
    let (kernel, task, flat) = super::ipc::machine();
    let memory = ForkableMemory(flat);
    let t = T::new(&kernel, task, &memory.0);
    t.mkdir("/a").unwrap();
    t.create("/a/f").unwrap();
    t.create("/top").unwrap();
    t.prctl_nnp();
    let ruleset = t.ruleset(READ_FILE, 0, 0).unwrap();
    assert_eq!(t.allow_path(ruleset, "/a", READ_FILE), Ok(0));
    for _ in 0..16 {
        assert_eq!(t.restrict(ruleset), Ok(0));
    }
    assert_eq!(errno(t.restrict(ruleset)), Errno::E2BIG);
    let caller = Caller::new(task, &memory);
    let clone = kernel.clone(
        &caller,
        Signal::SIGCHLD.0 as u32,
        0,
        GuestPtrMut::NULL,
        0,
        GuestPtrMut::NULL,
    );
    let child = TaskId(block_on(clone).unwrap() as u32);
    let c = T::new(&kernel, child, &memory.0);
    assert!(c.open("/a/f", OpenFlags::O_RDONLY).is_ok());
    assert_eq!(errno(c.open("/top", OpenFlags::O_RDONLY)), Errno::EACCES);
}

/// LANDLOCK_SCOPE_SIGNAL: a scoped process signals its own domain and those nested in it, not
/// its parent outside.
#[test]
fn signal_scope_keeps_signals_inside_the_domain() {
    let (kernel, parent, flat) = super::ipc::machine();
    let memory = ForkableMemory(flat);
    let caller = Caller::new(parent, &memory);
    let clone = kernel.clone(
        &caller,
        Signal::SIGCHLD.0 as u32,
        0,
        GuestPtrMut::NULL,
        0,
        GuestPtrMut::NULL,
    );
    let child = TaskId(block_on(clone).unwrap() as u32);
    while kernel.take_pending(parent).is_some() {}
    let c = T::new(&kernel, child, &memory.0);
    c.prctl_nnp();
    let ruleset = c.ruleset(0, 0, SCOPE_SIGNAL).unwrap();
    assert_eq!(c.restrict(ruleset), Ok(0));
    let kill = |from: &Caller<'_>, to: TaskId| {
        block_on(Syscalls::kill(&kernel, from, to.0 as i32, Signal(0)))
    };
    let child_caller = Caller::new(child, &memory);
    assert_eq!(errno(kill(&child_caller, parent)), Errno::EPERM);
    assert_eq!(kill(&child_caller, child), Ok(0));
    assert_eq!(kill(&caller, child), Ok(0));
}

/// fs/open.c do_dentry_open calls security_file_open for fanotify's dentry_open_nonotify.
/// A descriptor queued before confinement must not bypass the reader's current Landlock domain.
#[test]
fn fanotify_event_descriptors_obey_the_readers_landlock_domain() {
    use wasixv2_abi::{FanotifyInitFlags as F, FanotifyMarkFlags as M, FanotifyMask as E};
    for report_error in [false, true] {
        let (kernel, task, memory) = super::cred::machine_as(0, 0);
        let t = T::new(&kernel, task, &memory);
        t.create("/allowed").unwrap();
        t.create("/denied").unwrap();
        let flags = F::FAN_NONBLOCK
            | if report_error {
                F::FAN_REPORT_FD_ERROR
            } else {
                F(0)
            };
        let group = Fd(block_on(kernel.fanotify_init(&t.caller, flags.0, 0)).unwrap());
        for path in ["/allowed", "/denied"] {
            let name = t.cstr(0x1000, path);
            let mark = kernel.fanotify_mark(
                &t.caller,
                group,
                M::FAN_MARK_ADD.0,
                E::FAN_OPEN.0,
                CWD,
                name,
            );
            assert_eq!(block_on(mark), Ok(0));
            let open = kernel.openat(&t.caller, CWD, name, OpenFlags::O_RDONLY, 0);
            let fd = block_on(wasixv2_syscalls::as_task(task, open)).unwrap();
            assert_eq!(block_on(kernel.close(&t.caller, fd)), Ok(0));
        }
        let ruleset = t.ruleset(READ_FILE, 0, 0).unwrap();
        assert_eq!(t.allow_path(ruleset, "/allowed", READ_FILE), Ok(0));
        t.prctl_nnp();
        assert_eq!(t.restrict(ruleset), Ok(0));
        let read = || block_on(kernel.read(&t.caller, group, UserSliceMut::new(0x4000, 24)));
        let event_fd = || {
            let raw = super::ipc::get(&memory, 0x4010, 4);
            i32::from_ne_bytes(raw.try_into().unwrap())
        };
        assert_eq!(read(), Ok(24));
        let fd = event_fd();
        assert!(fd >= 0);
        assert_eq!(block_on(kernel.close(&t.caller, Fd(fd))), Ok(0));
        if report_error {
            assert_eq!(read(), Ok(24));
            assert_eq!(event_fd(), -Errno::EACCES.0);
        } else {
            assert_eq!(errno(read()), Errno::EACCES);
        }
        // Event opens remain non-notifying, including the successful allowed descriptor.
        assert_eq!(errno(read()), Errno::EAGAIN);
    }
}

#[test]
fn landlock_checks_a_temporary_files_directory_when_it_is_opened() {
    let (kernel, task, memory) = super::ipc::machine();
    let t = T::new(&kernel, task, &memory);
    t.mkdir("/allowed").unwrap();
    t.mkdir("/denied").unwrap();
    let ruleset = t.ruleset(WRITE_FILE, 0, 0).unwrap();
    assert_eq!(t.allow_path(ruleset, "/allowed", WRITE_FILE), Ok(0));
    t.prctl_nnp();
    assert_eq!(t.restrict(ruleset), Ok(0));
    let flags = OpenFlags::O_TMPFILE | OpenFlags::O_WRONLY;
    let fd = t.open("/allowed", flags).unwrap();
    assert_eq!(block_on(kernel.close(&t.caller, fd)), Ok(0));
    assert_eq!(errno(t.open("/denied", flags)), Errno::EACCES);
}

/// security/landlock/syscalls.c `landlock_restrict_self` defaults to current only;
/// tsync.c `restrict_one_thread` copies the prepared domain and propagates NNP.
#[test]
fn restrictions_are_thread_local_until_tsync_and_preserve_sibling_ids() {
    use wasixv2_abi::LandlockRestrictSelfFlags as Flags;
    let (kernel, leader, memory) = super::cred::machine_as(0, 0);
    let main = T::new(&kernel, leader, &memory);
    main.create("/file").unwrap();
    let worker = TaskId(
        block_on(kernel.thread_spawn(
            &main.caller,
            0x10f00,
            0,
            GuestPtrMut::NULL,
            0,
            GuestPtrMut::NULL,
        ))
        .unwrap() as u32,
    );
    let thread = T::new(&kernel, worker, &memory);
    let rules = main.ruleset(WRITE_FILE, 0, 0).unwrap();
    assert_eq!(thread.restrict(rules), Ok(0));
    assert_eq!(
        errno(thread.open("/file", OpenFlags::O_WRONLY)),
        Errno::EACCES
    );
    assert!(main.open("/file", OpenFlags::O_WRONLY).is_ok());
    // Give the caller a different filesystem identity, with the same effective root.
    assert_eq!(block_on(kernel.setfsuid(&thread.caller, 1000)), Ok(0));
    let flags = Flags::LANDLOCK_RESTRICT_SELF_TSYNC | Flags::LANDLOCK_RESTRICT_SELF_NO_NEW_PRIVS;
    assert_eq!(
        block_on(kernel.landlock_restrict_self(&thread.caller, Fd(rules), flags.0)),
        Ok(0)
    );
    assert_eq!(
        errno(main.open("/file", OpenFlags::O_WRONLY)),
        Errno::EACCES
    );
    assert_eq!(kernel.cred(&main.caller).user.fs.0, 0);
    assert_eq!(kernel.cred(&thread.caller).user.fs.0, 1000);
    assert!(kernel.no_new_privs(&main.caller));
    assert!(kernel.no_new_privs(&thread.caller));
    assert!(Arc::ptr_eq(
        kernel.cred(&main.caller).landlock.as_ref().unwrap(),
        kernel.cred(&thread.caller).landlock.as_ref().unwrap()
    ));
}

/// NO_NEW_PRIVS satisfies the unprivileged prerequisite but is committed only after
/// validation, as in security/landlock/syscalls.c `landlock_restrict_self`.
#[test]
fn restrict_self_no_new_privs_is_committed_only_on_success() {
    use wasixv2_abi::LandlockRestrictSelfFlags as Flags;
    let (kernel, task, memory) = super::cred::machine_as(1000, 1000);
    let t = T::new(&kernel, task, &memory);
    let flags = Flags::LANDLOCK_RESTRICT_SELF_NO_NEW_PRIVS.0;
    assert_eq!(
        errno(block_on(kernel.landlock_restrict_self(
            &t.caller,
            Fd(-1),
            flags
        ))),
        Errno::EBADF
    );
    assert!(!kernel.no_new_privs(&t.caller));
    let rules = t.ruleset(READ_FILE, 0, 0).unwrap();
    assert_eq!(errno(t.restrict(rules)), Errno::EPERM);
    assert_eq!(
        block_on(kernel.landlock_restrict_self(&t.caller, Fd(rules), flags)),
        Ok(0)
    );
    assert!(kernel.no_new_privs(&t.caller));
}
