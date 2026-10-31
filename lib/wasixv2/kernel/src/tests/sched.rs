//! The scheduler interface: per-thread attributes, thread ids as targets, inheritance, and the
//! size-versioned `sched_attr`.

use wasixv2_abi::{
    CloneFlags, GuestLayout, Idtype, KernelTimespec, SchedAttr, SchedParam, SiginfoPtr, WaitOptions,
};

use super::{
    ipc::{errno, get, machine, put},
    *,
};

const THREAD: u32 = (CloneFlags::CLONE_VM.0
    | CloneFlags::CLONE_FS.0
    | CloneFlags::CLONE_FILES.0
    | CloneFlags::CLONE_SIGHAND.0
    | CloneFlags::CLONE_THREAD.0) as u32;

/// Starts a thread in the caller's group and returns its id.
fn spawn_thread(kernel: &Kernel, caller: &Caller<'_>) -> TaskId {
    let null = GuestPtrMut::new(0);
    let tid = block_on(kernel.thread_spawn(caller, THREAD, 0, null, 0, null)).unwrap();
    kernel.take_pending(caller.task());
    TaskId(tid as u32)
}

fn set_param(kernel: &Kernel, caller: &Caller<'_>, pid: i32, priority: i32) -> SysResult<i32> {
    block_on(caller.memory().write(0x100, &priority.to_le_bytes())).unwrap();
    block_on(kernel.sched_setparam(caller, pid, GuestPtr::new(0x100)))
}

fn set_scheduler(
    kernel: &Kernel,
    caller: &Caller<'_>,
    pid: i32,
    policy: SchedPolicy,
    priority: i32,
) -> SysResult<i32> {
    block_on(caller.memory().write(0x100, &priority.to_le_bytes())).unwrap();
    block_on(kernel.sched_setscheduler(caller, pid, policy, GuestPtr::new(0x100)))
}

fn get_attr(kernel: &Kernel, caller: &Caller<'_>, pid: i32) -> SchedAttr {
    let size = SchedAttr::SIZE as u32;
    let ptr = GuestPtrMut::new(0x400);
    assert_eq!(
        block_on(kernel.sched_getattr(caller, pid, ptr, size, 0)),
        Ok(0)
    );
    let mut bytes = vec![0; SchedAttr::SIZE];
    block_on(caller.memory().read(0x400, &mut bytes)).unwrap();
    SchedAttr::read_from(&bytes).unwrap()
}

fn put_attr(memory: &FlatMemory, attr: &SchedAttr) {
    let mut bytes = vec![0; SchedAttr::SIZE];
    attr.write_to(&mut bytes);
    put(memory, 0x400, &bytes);
}

#[test]
fn policy_and_nice_are_per_thread_and_inherited() {
    let (kernel, task, memory) = machine();
    let caller = Caller::new(task, &memory);
    assert_eq!(
        set_scheduler(&kernel, &caller, 0, SchedPolicy::SCHED_BATCH, 0),
        Ok(0)
    );
    assert_eq!(
        block_on(kernel.setpriority(&caller, PrioWhich::PRIO_PROCESS, 0, 4)),
        Ok(0)
    );
    let thread = spawn_thread(&kernel, &caller);
    let tid = thread.0 as i32;
    // The thread inherited both; changing it leaves the leader alone.
    assert_eq!(
        block_on(kernel.sched_getscheduler(&caller, tid)),
        Ok(SchedPolicy::SCHED_BATCH.0)
    );
    assert_eq!(
        block_on(kernel.getpriority(&caller, PrioWhich::PRIO_PROCESS, tid)),
        Ok(20 - 4)
    );
    assert_eq!(
        set_scheduler(&kernel, &caller, tid, SchedPolicy::SCHED_IDLE, 0),
        Ok(0)
    );
    assert_eq!(
        block_on(kernel.setpriority(&caller, PrioWhich::PRIO_PROCESS, tid, 9)),
        Ok(0)
    );
    assert_eq!(
        block_on(kernel.sched_getscheduler(&caller, 0)),
        Ok(SchedPolicy::SCHED_BATCH.0)
    );
    assert_eq!(
        block_on(kernel.getpriority(&caller, PrioWhich::PRIO_PROCESS, 0)),
        Ok(16)
    );
    // The process group covers both threads and reports the highest priority.
    assert_eq!(
        block_on(kernel.getpriority(&caller, PrioWhich::PRIO_PGRP, 0)),
        Ok(16)
    );
    assert_eq!(
        block_on(kernel.setpriority(&caller, PrioWhich::PRIO_PGRP, 0, 12)),
        Ok(0)
    );
    assert_eq!(
        block_on(kernel.getpriority(&caller, PrioWhich::PRIO_PROCESS, tid)),
        Ok(8)
    );
    // Lowering it again is refused for every target.
    assert_eq!(
        errno(block_on(kernel.setpriority(
            &caller,
            PrioWhich::PRIO_USER,
            0,
            0
        ))),
        Errno::EACCES
    );
}

#[test]
fn unknown_targets_and_bad_arguments() {
    let (kernel, task, memory) = machine();
    let caller = Caller::new(task, &memory);
    assert_eq!(
        errno(block_on(kernel.sched_getscheduler(&caller, 999))),
        Errno::ESRCH
    );
    assert_eq!(
        errno(block_on(kernel.sched_getscheduler(&caller, -1))),
        Errno::EINVAL
    );
    assert_eq!(
        errno(block_on(kernel.sched_getparam(
            &caller,
            0,
            GuestPtrMut::new(0)
        ))),
        Errno::EINVAL
    );
    assert_eq!(errno(set_param(&kernel, &caller, 999, 0)), Errno::ESRCH);
    assert_eq!(
        errno(set_scheduler(&kernel, &caller, 0, SchedPolicy(-1), 0)),
        Errno::EINVAL
    );
    assert_eq!(
        errno(set_scheduler(
            &kernel,
            &caller,
            0,
            SchedPolicy::SCHED_FIFO,
            1
        )),
        Errno::EPERM
    );
    assert_eq!(
        errno(set_scheduler(
            &kernel,
            &caller,
            0,
            SchedPolicy::SCHED_FIFO,
            -1
        )),
        Errno::EINVAL
    );
    assert_eq!(
        errno(block_on(kernel.sched_rr_get_interval_time64(
            &caller,
            0,
            GuestPtrMut::new(0)
        ))),
        Errno::EFAULT
    );
    assert_eq!(
        errno(block_on(kernel.getpriority(&caller, PrioWhich(3), 0))),
        Errno::EINVAL
    );
    assert_eq!(
        errno(block_on(kernel.getpriority(
            &caller,
            PrioWhich::PRIO_USER,
            4242
        ))),
        Errno::ESRCH
    );
    assert_eq!(
        errno(block_on(kernel.getpriority(
            &caller,
            PrioWhich::PRIO_PGRP,
            4242
        ))),
        Errno::ESRCH
    );
}

#[test]
fn an_exited_thread_is_gone() {
    let (kernel, task, memory) = machine();
    let caller = Caller::new(task, &memory);
    let thread = spawn_thread(&kernel, &caller);
    let other = Caller::new(thread, &memory);
    assert_eq!(block_on(kernel.exit(&other, 0)), Ok(()));
    assert_eq!(
        errno(block_on(
            kernel.sched_getscheduler(&caller, thread.0 as i32)
        )),
        Errno::ESRCH
    );
}

/// kernel/sched/syscalls.c:find_process_by_pid resolves a zombie until
/// kernel/exit.c:release_task/__unhash_process removes its pid.
fn check_zombie_scheduler(
    kernel: &Kernel,
    caller: &Caller<'_>,
    pid: i32,
    policy: SchedPolicy,
    priority: i32,
) {
    assert_eq!(
        block_on(kernel.sched_getscheduler(caller, pid)),
        Ok(policy.0)
    );
    assert_eq!(
        block_on(kernel.sched_getparam(caller, pid, GuestPtrMut::new(0x100))),
        Ok(0)
    );
    let mut param = [0; SchedParam::SIZE];
    block_on(caller.memory().read(0x100, &mut param)).unwrap();
    assert_eq!(
        SchedParam::read_from(&param).unwrap().sched_priority,
        priority
    );
    let attr = get_attr(kernel, caller, pid);
    assert_eq!(
        (attr.sched_policy, attr.sched_priority),
        (policy.0 as u32, priority as u32)
    );
    assert_eq!(
        block_on(kernel.sched_rr_get_interval_time64(caller, pid, GuestPtrMut::new(0x200))),
        Ok(0)
    );
    let interval: KernelTimespec = block_on(wasixv2_syscalls::read_value(
        caller.memory(),
        GuestPtr::new(0x200),
    ))
    .unwrap();
    if policy == SchedPolicy::SCHED_RR {
        assert_eq!((interval.tv_sec, interval.tv_nsec), (0, 100_000_000));
    }
}

#[test]
fn a_zombies_scheduler_remains_visible_until_reaped() {
    for release_first in [true, false] {
        let (kernel, task, _) = super::cred::machine_as(0, 0);
        let memory = super::proc::forkable();
        block_on(memory.grow(1)).unwrap();
        let caller = Caller::new(task, &memory);
        let reset = SchedPolicy(SchedPolicy::SCHED_RR.0 | SchedPolicy::SCHED_RESET_ON_FORK.0);
        assert_eq!(set_scheduler(&kernel, &caller, 0, reset, 10), Ok(0));
        let child = super::proc::fork_child(&kernel, &caller);
        let pid = child.0 as i32;
        check_zombie_scheduler(&kernel, &caller, pid, SchedPolicy::SCHED_NORMAL, 0);
        // Preserve nondefault per-task metadata, not the parent's reset-on-fork values.
        assert_eq!(
            set_scheduler(&kernel, &caller, pid, SchedPolicy::SCHED_RR, 7),
            Ok(0)
        );
        let child_memory = super::proc::forkable();
        let child_caller = Caller::new(child, &child_memory);
        let mm = Arc::downgrade(&kernel.state.read_or_recover().tasks[&child].mm);
        assert_eq!(block_on(kernel.exit_group(&child_caller, 0)), Ok(()));
        check_zombie_scheduler(&kernel, &caller, pid, SchedPolicy::SCHED_RR, 7);
        if release_first {
            kernel.release(child);
            assert!(
                mm.upgrade().is_none(),
                "scheduler metadata must not retain mm"
            );
            assert!(!kernel.state.read_or_recover().tasks.contains_key(&child));
            check_zombie_scheduler(&kernel, &caller, pid, SchedPolicy::SCHED_RR, 7);
        }
        // Linux allows scheduler changes to an unreaped task too.
        assert_eq!(set_param(&kernel, &caller, pid, 8), Ok(0));
        check_zombie_scheduler(&kernel, &caller, pid, SchedPolicy::SCHED_RR, 8);
        assert_eq!(
            set_scheduler(&kernel, &caller, pid, SchedPolicy::SCHED_BATCH, 0),
            Ok(0)
        );
        let attr = SchedAttr {
            size: SchedAttr::SIZE as u32,
            sched_policy: SchedPolicy::SCHED_IDLE.0 as u32,
            ..SchedAttr::default()
        };
        let mut bytes = vec![0; SchedAttr::SIZE];
        attr.write_to(&mut bytes);
        block_on(memory.write(0x400, &bytes)).unwrap();
        assert_eq!(
            block_on(kernel.sched_setattr(&caller, pid, GuestPtrMut::new(0x400), 0)),
            Ok(0)
        );
        check_zombie_scheduler(&kernel, &caller, pid, SchedPolicy::SCHED_IDLE, 0);
        assert_eq!(
            block_on(kernel.waitid(
                &caller,
                Idtype::P_PID,
                pid,
                SiginfoPtr::new(0x600),
                WaitOptions::WEXITED,
                RusagePtr::new(0)
            )),
            Ok(0)
        );
        if !release_first {
            kernel.release(child);
        }
        assert_eq!(
            errno(block_on(kernel.sched_getscheduler(&caller, pid))),
            Errno::ESRCH
        );
        assert_eq!(
            errno(block_on(kernel.sched_getparam(
                &caller,
                pid,
                GuestPtrMut::new(0x100)
            ))),
            Errno::ESRCH
        );
        assert_eq!(
            errno(block_on(kernel.sched_getattr(
                &caller,
                pid,
                GuestPtrMut::new(0x400),
                SchedAttr::SIZE as u32,
                0
            ))),
            Errno::ESRCH
        );
        assert_eq!(
            errno(block_on(kernel.sched_rr_get_interval_time64(
                &caller,
                pid,
                GuestPtrMut::new(0x200)
            ))),
            Errno::ESRCH
        );
        assert_eq!(errno(set_param(&kernel, &caller, pid, 0)), Errno::ESRCH);
        assert!(mm.upgrade().is_none());
    }
}

#[test]
fn zombie_scheduler_setters_check_retained_credentials() {
    let (kernel, task, _) = super::cred::machine_as(0, 0);
    let memory = super::proc::forkable();
    block_on(memory.grow(1)).unwrap();
    let caller = Caller::new(task, &memory);
    let child = super::proc::fork_child(&kernel, &caller);
    let pid = child.0 as i32;
    let child_memory = super::proc::forkable();
    let other = Caller::new(child, &child_memory);
    assert_eq!(block_on(kernel.setuid(&other, 123)), Ok(0));
    assert_eq!(block_on(kernel.setuid(&caller, 456)), Ok(0));
    assert_eq!(block_on(kernel.exit_group(&other, 0)), Ok(()));
    for released in [false, true] {
        if released {
            kernel.release(child);
        }
        check_zombie_scheduler(&kernel, &caller, pid, SchedPolicy::SCHED_NORMAL, 0);
        assert_eq!(errno(set_param(&kernel, &caller, pid, 0)), Errno::EPERM);
        assert_eq!(
            errno(set_scheduler(
                &kernel,
                &caller,
                pid,
                SchedPolicy::SCHED_BATCH,
                0
            )),
            Errno::EPERM
        );
    }
}

#[test]
fn sched_attr_is_size_versioned() {
    let (kernel, task, memory) = machine();
    let caller = Caller::new(task, &memory);
    let attr = get_attr(&kernel, &caller, 0);
    assert_eq!((attr.size, attr.sched_policy), (56, 0));
    assert_eq!((attr.sched_util_min, attr.sched_util_max), (0, 1024));
    assert!(attr.sched_runtime > 0, "a fair task reports its slice");

    // A short buffer gets a short struct; a long one is zero-filled.
    put(&memory, 0x400, &[0xff; 80]);
    let ptr = GuestPtrMut::new(0x400);
    assert_eq!(
        block_on(kernel.sched_getattr(&caller, 0, ptr, 48, 0)),
        Ok(0)
    );
    assert_eq!(get(&memory, 0x400, 4), 48u32.to_le_bytes());
    assert_eq!(get(&memory, 0x400 + 48, 4), [0xff; 4]);
    assert_eq!(
        block_on(kernel.sched_getattr(&caller, 0, ptr, 80, 0)),
        Ok(0)
    );
    assert_eq!(get(&memory, 0x400, 4), 56u32.to_le_bytes());
    assert_eq!(get(&memory, 0x400 + 56, 24), [0; 24]);
    assert_eq!(
        errno(block_on(kernel.sched_getattr(&caller, 0, ptr, 47, 0))),
        Errno::EINVAL
    );
    assert_eq!(
        errno(block_on(kernel.sched_getattr(&caller, 0, ptr, 56, 1))),
        Errno::EINVAL
    );

    // setattr: nice through the attributes, KEEP_POLICY, and E2BIG with the size written back.
    let nicer = SchedAttr {
        size: 56,
        sched_policy: SchedPolicy::SCHED_NORMAL.0 as u32,
        sched_nice: 30,
        ..SchedAttr::default()
    };
    put_attr(&memory, &nicer);
    assert_eq!(block_on(kernel.sched_setattr(&caller, 0, ptr, 0)), Ok(0));
    assert_eq!(
        get_attr(&kernel, &caller, 0).sched_nice,
        19,
        "nice is clamped"
    );
    assert_eq!(
        errno(block_on(kernel.sched_setattr(&caller, 0, ptr, 1))),
        Errno::EINVAL
    );
    put_attr(&memory, &SchedAttr { size: 40, ..nicer });
    assert_eq!(
        errno(block_on(kernel.sched_setattr(&caller, 0, ptr, 0))),
        Errno::E2BIG
    );
    assert_eq!(get(&memory, 0x400, 4), 56u32.to_le_bytes());
    put_attr(&memory, &SchedAttr { size: 64, ..nicer });
    put(&memory, 0x400 + 60, &[1]);
    assert_eq!(
        errno(block_on(kernel.sched_setattr(&caller, 0, ptr, 0))),
        Errno::E2BIG
    );
    put(&memory, 0x400 + 60, &[0]);
    put_attr(&memory, &SchedAttr { size: 64, ..nicer });
    assert_eq!(block_on(kernel.sched_setattr(&caller, 0, ptr, 0)), Ok(0));
    // A deadline task needs privilege.
    put_attr(
        &memory,
        &SchedAttr {
            size: 56,
            sched_policy: SchedPolicy::SCHED_DEADLINE.0 as u32,
            sched_runtime: 10_000_000,
            sched_deadline: 30_000_000,
            sched_period: 100_000_000,
            ..SchedAttr::default()
        },
    );
    assert_eq!(
        errno(block_on(kernel.sched_setattr(&caller, 0, ptr, 0))),
        Errno::EPERM
    );
}

#[test]
fn affinity_is_per_thread_and_limited_to_online_cpus() {
    let (kernel, task, memory) = machine();
    let caller = Caller::new(task, &memory);
    let mask = GuestPtrMut::new(0x200);
    let len = block_on(kernel.sched_getaffinity(&caller, 0, 128, mask)).unwrap();
    assert!(len >= 4 && len % 4 == 0);
    let thread = spawn_thread(&kernel, &caller);
    // Only CPU 0 for the thread; a mask of offline CPUs only is refused.
    put(&memory, 0x200, &[1, 0, 0, 0]);
    let tid = thread.0 as i32;
    assert_eq!(
        block_on(kernel.sched_setaffinity(&caller, tid, 4, mask)),
        Ok(0)
    );
    put(&memory, 0x200, &[0; 4]);
    assert_eq!(
        errno(block_on(kernel.sched_setaffinity(&caller, tid, 4, mask))),
        Errno::EINVAL
    );
    assert_eq!(
        block_on(kernel.sched_getaffinity(&caller, tid, 128, mask)),
        Ok(len)
    );
    assert_eq!(get(&memory, 0x200, 4), [1, 0, 0, 0]);
    assert_eq!(
        errno(block_on(kernel.sched_getaffinity(&caller, 0, 3, mask))),
        Errno::EINVAL
    );
    assert_eq!(
        errno(block_on(kernel.sched_getaffinity(&caller, 999, 128, mask))),
        Errno::ESRCH
    );
}

#[test]
fn names_and_timer_slack_are_per_thread() {
    let (kernel, task, memory) = machine();
    let caller = Caller::new(task, &memory);
    put(&memory, 0x300, b"leader\0");
    let prctl =
        |caller: &Caller<'_>, option, arg| block_on(kernel.prctl(caller, option, arg, 0, 0, 0));
    assert_eq!(prctl(&caller, PrctlOption::PR_SET_NAME, 0x300), Ok(0));
    assert_eq!(prctl(&caller, PrctlOption::PR_SET_TIMERSLACK, 777), Ok(0));
    let thread = spawn_thread(&kernel, &caller);
    let other = Caller::new(thread, &memory);
    put(&memory, 0x300, b"worker\0");
    assert_eq!(prctl(&other, PrctlOption::PR_SET_NAME, 0x300), Ok(0));
    assert_eq!(prctl(&other, PrctlOption::PR_GET_TIMERSLACK, 0), Ok(777));
    assert_eq!(prctl(&other, PrctlOption::PR_SET_TIMERSLACK, 5), Ok(0));
    assert_eq!(prctl(&caller, PrctlOption::PR_GET_TIMERSLACK, 0), Ok(777));
    assert_eq!(prctl(&caller, PrctlOption::PR_GET_NAME, 0x300), Ok(0));
    assert_eq!(get(&memory, 0x300, 7), b"leader\0");
    assert_eq!(prctl(&other, PrctlOption::PR_GET_NAME, 0x300), Ok(0));
    assert_eq!(get(&memory, 0x300, 7), b"worker\0");
}

#[test]
fn getparam_reports_zero_for_fair_tasks() {
    let (kernel, task, memory) = machine();
    let caller = Caller::new(task, &memory);
    put(&memory, 0x100, &7i32.to_le_bytes());
    assert_eq!(
        block_on(kernel.sched_getparam(&caller, 0, GuestPtrMut::new(0x100))),
        Ok(0)
    );
    let param = SchedParam::read_from(&get(&memory, 0x100, 4)).unwrap();
    assert_eq!(param.sched_priority, 0);
    assert_eq!(set_param(&kernel, &caller, 0, 0), Ok(0));
    assert_eq!(errno(set_param(&kernel, &caller, 0, 1)), Errno::EINVAL);
}
