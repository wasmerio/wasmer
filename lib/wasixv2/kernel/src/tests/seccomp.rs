//! seccomp: strict mode, filters and their actions at syscall entry, inheritance and TSYNC.

use wasixv2_abi::{GuestPtrMut, PrctlOption, SiCode, Signal, VoidPtr};
use wasixv2_syscalls::{Layered, Syscalls};

use super::ipc::get;
use super::*;

/// The flags `pthread_create` passes.
const THREAD: u32 = 0x10f00;

const NR_GETPID: u32 = 172;
const NR_GETUID: u32 = 174;
const NR_GETGID: u32 = 176;
const NR_CLONE: u32 = 220;
const NR_WRITE: u32 = 64;

const RET_ALLOW: u32 = 0x7fff_0000;
const RET_ERRNO: u32 = 0x0005_0000;
const RET_TRAP: u32 = 0x0003_0000;
const RET_TRACE: u32 = 0x7ff0_0000;
const RET_KILL_PROCESS: u32 = 0x8000_0000;
const RET_KILL_THREAD: u32 = 0;

/// Where tests put a `sock_fprog`, and its program after it.
const FPROG: u32 = 0x1000;

fn insn(code: u16, jt: u8, jf: u8, k: u32) -> [u8; 8] {
    let mut b = [0; 8];
    b[..2].copy_from_slice(&code.to_le_bytes());
    b[2] = jt;
    b[3] = jf;
    b[4..].copy_from_slice(&k.to_le_bytes());
    b
}

/// `BPF_STMT(BPF_LD | BPF_W | BPF_ABS, k)`.
fn load(k: u32) -> [u8; 8] {
    insn(0x20, 0, 0, k)
}

/// `BPF_JUMP(BPF_JMP | BPF_JEQ | BPF_K, k, 0, 1)` then `BPF_STMT(BPF_RET | BPF_K, ret)`: returns
/// `ret` when A is `k`.
fn on(k: u32, ret: u32) -> [[u8; 8]; 2] {
    [insn(0x15, 0, 1, k), insn(0x06, 0, 0, ret)]
}

fn ret(k: u32) -> [u8; 8] {
    insn(0x06, 0, 0, k)
}

/// A filter that returns `action` for the syscall `nr` and allows everything else.
fn filter_for(nr: u32, action: u32) -> Vec<[u8; 8]> {
    let mut prog = vec![load(0)];
    prog.extend(on(nr, action));
    prog.push(ret(RET_ALLOW));
    prog
}

/// Writes `prog` and its `sock_fprog` at [`FPROG`].
fn put_filter(memory: &FlatMemory, prog: &[[u8; 8]]) -> VoidPtr {
    let fprog_at = FPROG;
    let insns_at = FPROG + 16;
    let mut fprog = [0; 8];
    fprog[..2].copy_from_slice(&(prog.len() as u16).to_le_bytes());
    fprog[4..].copy_from_slice(&insns_at.to_le_bytes());
    put(memory, fprog_at, &fprog);
    put(memory, insns_at, &prog.concat());
    VoidPtr::new(fprog_at)
}

fn set_filter(
    stack: &Layered<SyscallEntry, Kernel>,
    caller: &Caller<'_>,
    memory: &FlatMemory,
    flags: u32,
    prog: &[[u8; 8]],
) -> SysResult<i32> {
    let fprog = put_filter(memory, prog);
    block_on(stack.seccomp(caller, 1, flags, fprog))
}

fn no_new_privs(stack: &Layered<SyscallEntry, Kernel>, caller: &Caller<'_>) {
    let set = stack.prctl(caller, PrctlOption::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0);
    assert_eq!(block_on(set), Ok(0));
}

fn machine() -> (Layered<SyscallEntry, Kernel>, TaskId, FlatMemory) {
    let (kernel, task, memory) = super::ipc::machine();
    (Layered::new(SyscallEntry(Dispatch), kernel), task, memory)
}

fn spawn_thread(kernel: &Kernel, caller: &Caller<'_>) -> TaskId {
    let spawn = kernel.thread_spawn(caller, THREAD, 0, GuestPtrMut::NULL, 0, GuestPtrMut::NULL);
    let tid = block_on(spawn).unwrap();
    while kernel.take_pending(caller.task()).is_some() {}
    TaskId(tid as u32)
}

fn errno<T: std::fmt::Debug>(r: SysResult<T>) -> Errno {
    match r {
        Err(SysErr::Errno(e)) => e,
        other => panic!("expected an errno, got {other:?}"),
    }
}

/// Strict mode lets `read`, `write`, `exit` and `rt_sigreturn` through and kills the task with
/// SIGKILL on anything else; prctl reports the mode, and a second mode is refused.
#[test]
fn strict_mode_allows_four_calls_and_kills_on_the_rest() {
    let (stack, task, memory) = machine();
    let caller = Caller::new(task, &memory);
    let set = stack.prctl(&caller, PrctlOption::PR_SET_SECCOMP, 1, 0, 0, 0);
    assert_eq!(block_on(set), Ok(0));
    assert_eq!(stack.inner.prctl_get_seccomp(&caller), 1);
    put(&memory, 0x100, b"hi");
    let write = stack.write(&caller, Fd(1), wasixv2_syscalls::UserSlice::new(0x100, 2));
    assert_eq!(block_on(write), Ok(2));
    assert_eq!(stack.inner.take_pending(task), None);

    let _ = block_on(stack.getpid(&caller));
    assert_eq!(
        stack.inner.take_pending(task),
        Some(GuestAction::Exit(128 + Signal::SIGKILL.0))
    );
}

/// Installing a filter takes `no_new_privs` or CAP_SYS_ADMIN, and a program a seccomp filter
/// may run: classic BPF that only loads aligned words of `seccomp_data`.
#[test]
fn filters_need_no_new_privs_and_a_valid_program() {
    let (stack, task, memory) = machine();
    let caller = Caller::new(task, &memory);
    let allow = [ret(RET_ALLOW)];
    assert_eq!(
        errno(set_filter(&stack, &caller, &memory, 0, &allow)),
        Errno::EACCES
    );
    no_new_privs(&stack, &caller);
    // A halfword load, an unaligned word, a word past the data, BPF_MOD, an empty program.
    for bad in [
        vec![insn(0x28, 0, 0, 0), ret(RET_ALLOW)],
        vec![load(2), ret(RET_ALLOW)],
        vec![load(64), ret(RET_ALLOW)],
        vec![insn(0x94, 0, 0, 3), ret(RET_ALLOW)],
        vec![],
    ] {
        assert_eq!(
            errno(set_filter(&stack, &caller, &memory, 0, &bad)),
            Errno::EINVAL
        );
    }
    assert_eq!(
        errno(set_filter(&stack, &caller, &memory, 0x8000, &allow)),
        Errno::EINVAL
    );
    // BPF_LEN reads the size of `seccomp_data`.
    let len = [
        insn(0x80, 0, 0, 0),
        insn(0x15, 0, 1, 64),
        ret(RET_ALLOW),
        ret(RET_KILL_PROCESS),
    ];
    assert_eq!(set_filter(&stack, &caller, &memory, 0, &len), Ok(0));
    assert_eq!(stack.inner.prctl_get_seccomp(&caller), 2);
    assert!(block_on(stack.getpid(&caller)).is_ok());
    // A filter goes no further back than strict mode.
    let strict = stack.seccomp(&caller, 0, 0, VoidPtr::NULL);
    assert_eq!(errno(block_on(strict)), Errno::EINVAL);
}

/// SECCOMP_RET_ERRNO skips the call with the error; SECCOMP_RET_TRACE without a tracer with
/// ENOSYS; the filter sees the arguments as registers.
#[test]
fn errno_and_trace_skip_the_call() {
    let (stack, task, memory) = machine();
    let caller = Caller::new(task, &memory);
    no_new_privs(&stack, &caller);
    let prog = filter_for(NR_GETPID, RET_ERRNO | 13);
    assert_eq!(set_filter(&stack, &caller, &memory, 0, &prog), Ok(0));
    let prog = filter_for(NR_GETUID, RET_TRACE);
    assert_eq!(set_filter(&stack, &caller, &memory, 0, &prog), Ok(0));
    // write(2, ...) fails with EBADF's number; write(1, ...) goes through.
    let mut args = vec![
        load(0),
        insn(0x15, 0, 3, NR_WRITE),
        load(16),
        insn(0x15, 0, 1, 2),
    ];
    args.extend([ret(RET_ERRNO | 9), ret(RET_ALLOW)]);
    assert_eq!(set_filter(&stack, &caller, &memory, 0, &args), Ok(0));

    assert_eq!(errno(block_on(stack.getpid(&caller))), Errno::EACCES);
    assert_eq!(errno(block_on(stack.getuid(&caller))), Errno::ENOSYS);
    assert!(block_on(stack.getgid(&caller)).is_ok());
    put(&memory, 0x100, b"x");
    let slice = wasixv2_syscalls::UserSlice::new(0x100, 1);
    assert_eq!(
        errno(block_on(stack.write(&caller, Fd(2), slice))),
        Errno::EBADF
    );
    assert_eq!(block_on(stack.write(&caller, Fd(1), slice)), Ok(1));
    // SECCOMP_RET_ERRNO with 0 is a success that returns 0.
    let prog = filter_for(NR_GETGID, RET_ERRNO);
    assert_eq!(set_filter(&stack, &caller, &memory, 0, &prog), Ok(0));
    assert_eq!(block_on(stack.getgid(&caller)), Ok(0));
    assert_eq!(
        stack.inner.task_ref(task, |t| t.seccomp.status()),
        (true, 2, 4)
    );
}

/// SECCOMP_RET_TRAP skips the call and forces a SIGSYS on the task, with the syscall, the
/// architecture and the filter's data, even when the task blocked or ignored SIGSYS.
#[test]
fn trap_forces_sigsys_with_the_call() {
    let (stack, task, memory) = machine();
    let caller = Caller::new(task, &memory);
    no_new_privs(&stack, &caller);
    let prog = filter_for(NR_GETUID, RET_TRAP | 0x42);
    assert_eq!(set_filter(&stack, &caller, &memory, 0, &prog), Ok(0));
    let signals = stack.inner.task_signals(task);
    signals.replace_blocked(1 << (Signal::SIGSYS.0 - 1));

    let _ = block_on(stack.getuid(&caller));
    assert_eq!(
        signals.replace_blocked(0) & (1 << (Signal::SIGSYS.0 - 1)),
        0
    );
    let info = signals
        .dequeue_from(1 << (Signal::SIGSYS.0 - 1))
        .expect("SIGSYS");
    let word = |at: usize| u32::from_le_bytes(info.sifields[at..at + 4].try_into().unwrap());
    assert_eq!(info.si_code, SiCode::SYS_SECCOMP.0);
    assert_eq!(info.si_errno, 0x42);
    assert_eq!((word(0), word(4), word(8)), (0, NR_GETUID, 0x4000_00f3));
}

/// The kill actions: KILL_PROCESS ends the group with SIGSYS; KILL_THREAD only the thread
/// while others run, and the whole group for its last thread. The strongest action of all
/// filters wins.
#[test]
fn kill_actions_end_the_thread_or_the_group() {
    let (stack, task, memory) = machine();
    let caller = Caller::new(task, &memory);
    no_new_privs(&stack, &caller);
    let thread = spawn_thread(&stack.inner, &caller);
    let prog = filter_for(NR_GETUID, RET_KILL_THREAD);
    assert_eq!(set_filter(&stack, &caller, &memory, 1, &prog), Ok(0));
    let prog = filter_for(NR_GETUID, RET_ERRNO | 1);
    assert_eq!(set_filter(&stack, &caller, &memory, 1, &prog), Ok(0));

    let thread_caller = Caller::new(thread, &memory);
    let _ = block_on(stack.getuid(&thread_caller));
    assert!(matches!(
        stack.inner.take_pending(thread),
        Some(GuestAction::Exit(_))
    ));
    assert_eq!(stack.inner.take_pending(task), None);
    let _ = block_on(stack.getuid(&caller));
    assert_eq!(
        stack.inner.take_pending(task),
        Some(GuestAction::Exit(128 + Signal::SIGSYS.0))
    );

    let (stack, task, memory) = machine();
    let caller = Caller::new(task, &memory);
    no_new_privs(&stack, &caller);
    let prog = filter_for(NR_GETPID, RET_KILL_PROCESS);
    assert_eq!(set_filter(&stack, &caller, &memory, 0, &prog), Ok(0));
    let _ = block_on(stack.getpid(&caller));
    assert_eq!(
        stack.inner.take_pending(task),
        Some(GuestAction::Exit(128 + Signal::SIGSYS.0))
    );
}

/// A child and a thread start with their creator's filters and `no_new_privs`; the
/// `thread_spawn` extension is filtered as `clone`.
#[test]
fn filters_are_inherited_and_thread_spawn_is_clone() {
    let (stack, task, flat) = machine();
    // Forking copies the memory.
    let memory = super::proc::ForkableMemory(flat);
    let caller = Caller::new(task, &memory);
    no_new_privs(&stack, &caller);
    let prog = filter_for(NR_GETPID, RET_ERRNO | 1);
    assert_eq!(set_filter(&stack, &caller, &memory.0, 0, &prog), Ok(0));
    let thread = spawn_thread(&stack.inner, &caller);
    let clone = stack.clone(
        &caller,
        Signal::SIGCHLD.0 as u32,
        0,
        GuestPtrMut::NULL,
        0,
        GuestPtrMut::NULL,
    );
    let child = TaskId(block_on(clone).unwrap() as u32);
    for t in [thread, child] {
        assert_eq!(
            stack.inner.task_ref(t, |t| t.seccomp.status()),
            (true, 2, 1)
        );
        assert_eq!(
            errno(block_on(stack.getpid(&Caller::new(t, &memory)))),
            Errno::EPERM
        );
    }

    let prog = filter_for(NR_CLONE, RET_ERRNO | 11);
    assert_eq!(set_filter(&stack, &caller, &memory.0, 0, &prog), Ok(0));
    let spawn = stack.thread_spawn(&caller, THREAD, 0, GuestPtrMut::NULL, 0, GuestPtrMut::NULL);
    assert_eq!(errno(block_on(spawn)), Errno::EAGAIN);
}

/// SECCOMP_FILTER_FLAG_TSYNC gives every thread the caller's filters and `no_new_privs`,
/// unless one has filters that are not the caller's: then the call returns its tid, or fails
/// with ESRCH with SECCOMP_FILTER_FLAG_TSYNC_ESRCH.
#[test]
fn tsync_synchronises_threads_or_names_the_one_in_the_way() {
    let (stack, task, memory) = machine();
    let caller = Caller::new(task, &memory);
    let thread = spawn_thread(&stack.inner, &caller);
    let other = spawn_thread(&stack.inner, &caller);
    no_new_privs(&stack, &caller);
    let prog = filter_for(NR_GETPID, RET_ERRNO | 1);
    assert_eq!(set_filter(&stack, &caller, &memory, 1, &prog), Ok(0));
    for t in [thread, other] {
        assert_eq!(
            stack.inner.task_ref(t, |t| t.seccomp.status()),
            (true, 2, 1)
        );
    }

    let other_caller = Caller::new(other, &memory);
    let prog = filter_for(NR_GETUID, RET_ERRNO | 2);
    assert_eq!(set_filter(&stack, &other_caller, &memory, 0, &prog), Ok(0));
    let prog = filter_for(NR_GETGID, RET_ERRNO | 3);
    let tid = stack.inner.own_tid(other);
    assert_eq!(set_filter(&stack, &caller, &memory, 1, &prog), Ok(tid));
    assert_eq!(
        errno(set_filter(&stack, &caller, &memory, 0x11, &prog)),
        Errno::ESRCH
    );
    assert_eq!(
        stack.inner.task_ref(task, |t| t.seccomp.status()),
        (true, 2, 1)
    );
}

/// SECCOMP_GET_ACTION_AVAIL knows Linux's actions, and
/// SECCOMP_GET_NOTIF_SIZES gives the structures' sizes.
#[test]
fn action_avail_and_notif_sizes() {
    let (stack, task, memory) = machine();
    let caller = Caller::new(task, &memory);
    let avail = |action: u32| {
        put(&memory, 0x200, &action.to_le_bytes());
        block_on(stack.seccomp(&caller, 2, 0, VoidPtr::new(0x200)))
    };
    for action in [
        RET_KILL_PROCESS,
        RET_KILL_THREAD,
        RET_TRAP,
        RET_ERRNO,
        RET_TRACE,
        0x7ffc_0000,
        RET_ALLOW,
    ] {
        assert_eq!(avail(action), Ok(0));
    }
    assert_eq!(avail(0x7fc0_0000), Ok(0));
    assert_eq!(errno(avail(0x1234_0000)), Errno::EOPNOTSUPP);
    assert_eq!(
        block_on(stack.seccomp(&caller, 3, 0, VoidPtr::new(0x300))),
        Ok(0)
    );
    let mut sizes = [0; 6];
    block_on(memory.read(0x300, &mut sizes)).unwrap();
    assert_eq!(sizes, [80, 0, 24, 0, 64, 0]);
    assert_eq!(
        errno(block_on(stack.seccomp(&caller, 3, 1, VoidPtr::new(0x300)))),
        Errno::EINVAL
    );
    assert_eq!(
        errno(block_on(stack.seccomp(&caller, 4, 0, VoidPtr::NULL))),
        Errno::EINVAL
    );
}

const RET_USER_NOTIF: u32 = 0x7fc0_0000;
const NEW_LISTENER: u32 = 8;

/// `SECCOMP_IOCTL_NOTIF_*`.
const NOTIF_RECV: u32 = 0xc050_2100;
const NOTIF_SEND: u32 = 0xc018_2101;
const NOTIF_ID_VALID: u32 = 0x4008_2102;
const NOTIF_ADDFD: u32 = 0x4018_2103;

/// The supervisor's side, from another thread: reads the next notification at 0x5000 and
/// returns its id and syscall number, then answers it with `resp` (`val`, `error`, `flags`).
async fn supervise(
    kernel: &Kernel,
    caller: &Caller<'_>,
    memory: &FlatMemory,
    listener: Fd,
    resp: Option<(i64, i32, u32)>,
) -> (u64, i32) {
    put(memory, 0x5000, &[0; 80]);
    let recv = kernel.ioctl(
        caller,
        listener,
        wasixv2_abi::IoctlRequest(NOTIF_RECV),
        0x5000,
    );
    assert_eq!(recv.await, Ok(0));
    let notif = super::ipc::get(memory, 0x5000, 80);
    let id = u64::from_le_bytes(notif[..8].try_into().unwrap());
    let nr = i32::from_le_bytes(notif[16..20].try_into().unwrap());
    put(memory, 0x5100, &id.to_le_bytes());
    let valid = kernel.ioctl(
        caller,
        listener,
        wasixv2_abi::IoctlRequest(NOTIF_ID_VALID),
        0x5100,
    );
    assert_eq!(valid.await, Ok(0));
    if let Some((val, error, flags)) = resp {
        let mut raw = id.to_le_bytes().to_vec();
        raw.extend(val.to_le_bytes());
        raw.extend(error.to_le_bytes());
        raw.extend(flags.to_le_bytes());
        put(memory, 0x5200, &raw);
        let send = kernel.ioctl(
            caller,
            listener,
            wasixv2_abi::IoctlRequest(NOTIF_SEND),
            0x5200,
        );
        assert_eq!(send.await, Ok(0));
        let valid = kernel.ioctl(
            caller,
            listener,
            wasixv2_abi::IoctlRequest(NOTIF_ID_VALID),
            0x5100,
        );
        assert_eq!(errno(valid.await), Errno::ENOENT);
    }
    (id, nr)
}

/// SECCOMP_RET_USER_NOTIF: the call waits until the supervisor reads it from the listener
/// and answers with a value, an error, or SECCOMP_USER_NOTIF_FLAG_CONTINUE; a closed listener
/// is ENOSYS.
#[test]
fn user_notifications_go_to_the_supervisor() {
    let (stack, task, memory) = machine();
    let caller = Caller::new(task, &memory);
    no_new_privs(&stack, &caller);
    let supervisor = spawn_thread(&stack.inner, &caller);
    let sup = Caller::new(supervisor, &memory);
    let prog = filter_for(NR_GETUID, RET_USER_NOTIF);
    let listener = Fd(set_filter(&stack, &caller, &memory, NEW_LISTENER, &prog).unwrap());
    assert!(listener.0 >= 3);
    // One listener per chain; a tid and a listener cannot both be the result.
    assert_eq!(
        errno(set_filter(&stack, &caller, &memory, NEW_LISTENER, &prog)),
        Errno::EBUSY
    );
    assert_eq!(
        errno(set_filter(
            &stack,
            &caller,
            &memory,
            NEW_LISTENER | 1,
            &prog
        )),
        Errno::EINVAL
    );
    assert_eq!(
        errno(set_filter(&stack, &caller, &memory, 32, &prog)),
        Errno::EINVAL
    );

    let (ret, (_, nr)) = block_on(async {
        futures::join!(
            stack.getuid(&caller),
            supervise(&stack.inner, &sup, &memory, listener, Some((42, 0, 0)))
        )
    });
    assert_eq!((ret, nr), (Ok(42), NR_GETUID as i32));
    let (ret, _) = block_on(async {
        futures::join!(
            stack.getuid(&caller),
            supervise(&stack.inner, &sup, &memory, listener, Some((0, -1, 0)))
        )
    });
    assert_eq!(errno(ret), Errno::EPERM);
    let (ret, _) = block_on(async {
        futures::join!(
            stack.getuid(&caller),
            supervise(&stack.inner, &sup, &memory, listener, Some((0, 0, 1)))
        )
    });
    assert_eq!(ret, Ok(1));
    // An answer with CONTINUE takes no value; a struct to read into must be zeroed.
    put(&memory, 0x5000, &[1; 80]);
    let recv = stack.inner.ioctl(
        &sup,
        listener,
        wasixv2_abi::IoctlRequest(NOTIF_RECV),
        0x5000,
    );
    assert_eq!(errno(block_on(recv)), Errno::EINVAL);

    // ADDFD with SEND installs the supervisor's descriptor in the task and answers with it.
    let (read, _) = super::ipc::pipe(&stack.inner, &sup, OpenFlags(0));
    let addfd = async {
        let (id, _) = supervise(&stack.inner, &sup, &memory, listener, None).await;
        let mut raw = id.to_le_bytes().to_vec();
        raw.extend(
            [2u32, read.0 as u32, 0, 0]
                .iter()
                .flat_map(|v| v.to_le_bytes()),
        );
        put(&memory, 0x5300, &raw);
        stack
            .inner
            .ioctl(
                &sup,
                listener,
                wasixv2_abi::IoctlRequest(NOTIF_ADDFD),
                0x5300,
            )
            .await
    };
    let (ret, added) = block_on(async { futures::join!(stack.getuid(&caller), addfd) });
    assert_eq!(ret, added);

    block_on(stack.inner.close(&sup, listener)).unwrap();
    assert_eq!(errno(block_on(stack.getuid(&caller))), Errno::ENOSYS);
}

fn logging_machine() -> (Layered<SyscallEntry, Kernel>, TaskId, FlatMemory) {
    let (kernel, task, memory) = super::cred::machine_as(0, 0);
    block_on(kernel.vfs.mount(
        b"/proc",
        kernel.procfs(),
        wasixv2_vfs::MountFlags::default(),
        true,
    ))
    .unwrap();
    (Layered::new(SyscallEntry(Dispatch), kernel), task, memory)
}

fn action_file(
    kernel: &Kernel,
    caller: &Caller<'_>,
    memory: &FlatMemory,
    name: &str,
    flags: OpenFlags,
) -> Fd {
    put(
        memory,
        0x400,
        format!("/proc/sys/kernel/seccomp/{name}\0").as_bytes(),
    );
    block_on(kernel.openat(caller, Fd(-100), GuestPtr::new(0x400), flags, 0)).unwrap()
}

fn action_write(
    kernel: &Kernel,
    caller: &Caller<'_>,
    memory: &FlatMemory,
    text: &str,
) -> SysResult<u32> {
    let fd = action_file(
        kernel,
        caller,
        memory,
        "actions_logged",
        OpenFlags::O_WRONLY,
    );
    put(memory, 0x400, text.as_bytes());
    let ret = block_on(kernel.write(caller, fd, UserSlice::new(0x400, text.len() as u32)));
    block_on(kernel.close(caller, fd)).unwrap();
    ret
}

fn action_read(kernel: &Kernel, caller: &Caller<'_>, memory: &FlatMemory, fd: Fd) -> String {
    let n = block_on(kernel.pread64(caller, fd, UserSliceMut::new(0x4000, 256), 0)).unwrap();
    String::from_utf8(get(memory, 0x4000, n as usize)).unwrap()
}

fn kernel_log(kernel: &Kernel, caller: &Caller<'_>, memory: &FlatMemory) -> String {
    let n = block_on(kernel.syslog(caller, 3, GuestPtrMut::new(0x4000), 8192)).unwrap();
    String::from_utf8(get(memory, 0x4000, n as usize)).unwrap()
}

#[test]
fn seccomp_action_sysctls_validate_and_log_final_filter_verdicts() {
    let (stack, task, memory) = logging_machine();
    let kernel = &stack.inner;
    let caller = Caller::new(task, &memory);
    let avail = action_file(
        kernel,
        &caller,
        &memory,
        "actions_avail",
        OpenFlags::O_RDONLY,
    );
    assert_eq!(
        action_read(kernel, &caller, &memory, avail),
        "kill_process kill_thread trap errno user_notif trace log allow\n"
    );
    let logged = action_file(
        kernel,
        &caller,
        &memory,
        "actions_logged",
        OpenFlags::O_RDONLY,
    );
    assert_eq!(
        action_read(kernel, &caller, &memory, logged),
        "kill_process kill_thread trap errno user_notif trace log\n"
    );
    no_new_privs(&stack, &caller);
    set_filter(
        &stack,
        &caller,
        &memory,
        0,
        &filter_for(NR_GETPID, 0x7ffc0000),
    )
    .unwrap();
    assert_eq!(block_on(stack.getpid(&caller)), Ok(task.0 as i32));
    assert!(
        kernel_log(kernel, &caller, &memory)
            .contains("syscall=172 arch=0x400000f3 action=log sig=0")
    );
    block_on(kernel.syslog(&caller, 5, GuestPtrMut::NULL, 0)).unwrap();
    assert_eq!(action_write(kernel, &caller, &memory, "errno\n"), Ok(6));
    assert_eq!(
        action_read(kernel, &caller, &memory, logged),
        "errno\n",
        "an already-open reader sees changes"
    );
    for bad in ["allow\n", "unknown\n", "errno\tlog\n"] {
        assert_eq!(
            errno(action_write(kernel, &caller, &memory, bad)),
            Errno::EINVAL
        );
    }
    assert_eq!(block_on(stack.getpid(&caller)), Ok(task.0 as i32));
    assert!(!kernel_log(kernel, &caller, &memory).contains("action=log"));
    set_filter(
        &stack,
        &caller,
        &memory,
        2,
        &filter_for(NR_GETUID, RET_ERRNO | 1),
    )
    .unwrap();
    assert_eq!(errno(block_on(stack.getuid(&caller))), Errno::EPERM);
    assert!(
        kernel_log(kernel, &caller, &memory).contains("syscall=174 arch=0x400000f3 action=errno")
    );
    // A more restrictive unlogged filter wins over an older logged filter.
    block_on(kernel.syslog(&caller, 5, GuestPtrMut::NULL, 0)).unwrap();
    set_filter(
        &stack,
        &caller,
        &memory,
        0,
        &filter_for(NR_GETUID, RET_ERRNO | 2),
    )
    .unwrap();
    assert_eq!(errno(block_on(stack.getuid(&caller))), Errno::ENOENT);
    assert!(!kernel_log(kernel, &caller, &memory).contains("action=errno"));
}

#[test]
fn seccomp_kill_actions_log_without_filter_log_flag_and_respect_mask() {
    for (action, name) in [
        (RET_KILL_PROCESS, "kill_process"),
        (RET_KILL_THREAD, "kill_thread"),
        (0x12340000, "kill_process"),
    ] {
        let (stack, task, memory) = logging_machine();
        let caller = Caller::new(task, &memory);
        no_new_privs(&stack, &caller);
        set_filter(&stack, &caller, &memory, 0, &filter_for(NR_GETPID, action)).unwrap();
        assert_eq!(errno(block_on(stack.getpid(&caller))), Errno::ENOSYS);
        assert!(
            kernel_log(&stack.inner, &caller, &memory).contains(&format!("action={name} sig=31"))
        );
    }
    let (stack, task, memory) = logging_machine();
    let caller = Caller::new(task, &memory);
    action_write(&stack.inner, &caller, &memory, "\n").unwrap();
    no_new_privs(&stack, &caller);
    set_filter(
        &stack,
        &caller,
        &memory,
        0,
        &filter_for(NR_GETPID, RET_KILL_PROCESS),
    )
    .unwrap();
    assert_eq!(errno(block_on(stack.getpid(&caller))), Errno::ENOSYS);
    assert!(!kernel_log(&stack.inner, &caller, &memory).contains("seccomp:"));
}

#[test]
fn seccomp_actions_logged_requires_initial_sys_admin() {
    let (stack, task, memory) = logging_machine();
    let caller = Caller::new(task, &memory);
    let fd = action_file(
        &stack.inner,
        &caller,
        &memory,
        "actions_logged",
        OpenFlags::O_WRONLY,
    );
    super::cred::set_caps(&stack.inner, &caller, &memory, 0);
    assert_eq!(
        errno(action_write(&stack.inner, &caller, &memory, "log\n")),
        Errno::EPERM
    );
    put(&memory, 0x400, b"log\n");
    assert_eq!(
        errno(block_on(stack.inner.write(
            &caller,
            fd,
            UserSlice::new(0x400, 4)
        ))),
        Errno::EPERM,
        "an already-open fd cannot retain CAP_SYS_ADMIN after a capability drop"
    );
}
