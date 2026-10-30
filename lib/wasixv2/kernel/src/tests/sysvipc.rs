//! SysV semaphores, message queues and shared memory, and POSIX message queues, between the
//! processes of one machine.

use wasixv2_abi::{IpcCmd, IpcFlags, IpcKey, MqAttr, Msqid64Ds, Semid64Ds, Shmid64Ds};
use wasixv2_syscalls::{read_value, write_value};

use super::{
    ipc::{errno, get, put},
    mapping::another_process,
    *,
};

const CREATE: i32 = IpcFlags::IPC_CREAT.0 as i32 | 0o600;
const NOWAIT: i16 = IpcFlags::IPC_NOWAIT.0 as i16;
const UNDO: i16 = 0x1000;

fn memory() -> FlatMemory {
    FlatMemory(RefCell::new(vec![0; 2 * WASM_PAGE_SIZE as usize]))
}

/// Runs one `semop` with the given operations, laid out at 0x100.
fn semop(kernel: &Kernel, caller: &Caller<'_>, id: i32, ops: &[(u16, i16, i16)]) -> SysResult<i32> {
    let bytes: Vec<u8> = ops
        .iter()
        .flat_map(|&(num, op, flg)| {
            let mut sop = num.to_le_bytes().to_vec();
            sop.extend(op.to_le_bytes());
            sop.extend(flg.to_le_bytes());
            sop
        })
        .collect();
    block_on(caller.memory().write(0x100, &bytes)).unwrap();
    let sops = GuestPtr::new(0x100);
    block_on(kernel.semop(caller, id, sops, ops.len() as u32))
}

fn semctl(
    kernel: &Kernel,
    caller: &Caller<'_>,
    id: i32,
    num: i32,
    cmd: IpcCmd,
    arg: u32,
) -> SysResult<i32> {
    block_on(kernel.semctl(caller, id, num, cmd.0, arg))
}

/// Waits until a task sleeps on semaphore 0 of `id`.
fn await_sleeper(kernel: &Kernel, caller: &Caller<'_>, id: i32) {
    while semctl(kernel, caller, id, 0, IpcCmd::GETNCNT, 0) != Ok(1) {
        std::thread::sleep(Duration::from_millis(1));
    }
}

#[test]
fn semaphores_count_block_and_report() {
    let (kernel, task, _) = setup(b"");
    let memory = memory();
    let caller = Caller::new(task, &memory);
    let key = 0x1234;
    let id = block_on(kernel.semget(&caller, key, 2, CREATE)).unwrap();

    assert_eq!(block_on(kernel.semget(&caller, key, 2, 0)), Ok(id));
    assert_eq!(
        errno(block_on(kernel.semget(&caller, key, 3, 0))),
        Errno::EINVAL
    );
    let excl = CREATE | IpcFlags::IPC_EXCL.0 as i32;
    assert_eq!(
        errno(block_on(kernel.semget(&caller, key, 2, excl))),
        Errno::EEXIST
    );
    assert_eq!(
        errno(block_on(kernel.semget(&caller, 0x99, 1, 0))),
        Errno::ENOENT
    );

    assert_eq!(semctl(&kernel, &caller, id, 1, IpcCmd::SETVAL, 2), Ok(0));
    assert_eq!(
        semop(&kernel, &caller, id, &[(1, -1, NOWAIT), (0, 1, 0)]),
        Ok(0)
    );
    assert_eq!(
        errno(semop(&kernel, &caller, id, &[(1, -2, NOWAIT)])),
        Errno::EAGAIN
    );
    assert_eq!(
        errno(semop(&kernel, &caller, id, &[(2, 1, 0)])),
        Errno::EFBIG
    );
    assert_eq!(
        errno(semctl(&kernel, &caller, id, 0, IpcCmd::SETVAL, 40000)),
        Errno::ERANGE
    );
    assert_eq!(semctl(&kernel, &caller, id, 0, IpcCmd::GETVAL, 0), Ok(1));
    assert_eq!(semctl(&kernel, &caller, id, 1, IpcCmd::GETVAL, 0), Ok(1));
    assert_eq!(
        semctl(&kernel, &caller, id, 1, IpcCmd::GETPID, 0),
        Ok(task.0 as i32)
    );
    assert_eq!(
        semctl(&kernel, &caller, id, 0, IpcCmd::GETALL, 0x200),
        Ok(0)
    );
    assert_eq!(get(&memory, 0x200, 4), [1, 0, 1, 0]);

    assert_eq!(
        semctl(&kernel, &caller, id, 0, IpcCmd::IPC_STAT, 0x300),
        Ok(0)
    );
    let ds: Semid64Ds = block_on(read_value(&memory, GuestPtr::new(0x300))).unwrap();
    assert_eq!(
        (ds.sem_nsems, ds.sem_perm.key, ds.sem_perm.mode),
        (2, key, 0o600)
    );
    assert!(ds.sem_otime > 0);

    assert_eq!(semctl(&kernel, &caller, id, 0, IpcCmd::IPC_RMID, 0), Ok(0));
    assert_eq!(
        errno(semop(&kernel, &caller, id, &[(0, 1, 0)])),
        Errno::EINVAL
    );
    let private = block_on(kernel.semget(&caller, IpcKey::IPC_PRIVATE.0, 1, CREATE)).unwrap();
    assert_ne!(private, id, "a new set takes a new id");
}

#[test]
fn a_semaphore_wakes_a_process_waiting_on_it_and_undo_applies_at_exit() {
    let (kernel, a, _) = setup(b"");
    let b = another_process(&kernel);
    let memory_a = memory();
    let caller_a = Caller::new(a, &memory_a);
    let id = block_on(kernel.semget(&caller_a, IpcKey::IPC_PRIVATE.0, 1, CREATE)).unwrap();

    std::thread::scope(|scope| {
        scope.spawn(|| {
            let memory_b = memory();
            let caller_b = Caller::new(b, &memory_b);
            // Blocks until A posts.
            assert_eq!(semop(&kernel, &caller_b, id, &[(0, -1, 0)]), Ok(0));
            // Taken back when B exits.
            assert_eq!(semop(&kernel, &caller_b, id, &[(0, 5, UNDO)]), Ok(0));
            assert_eq!(block_on(kernel.exit_group(&caller_b, 0)), Ok(()));
        });
        await_sleeper(&kernel, &caller_a, id);
        assert_eq!(semop(&kernel, &caller_a, id, &[(0, 1, 0)]), Ok(0));
    });

    assert_eq!(semctl(&kernel, &caller_a, id, 0, IpcCmd::GETVAL, 0), Ok(0));
    assert_eq!(
        semctl(&kernel, &caller_a, id, 0, IpcCmd::GETPID, 0),
        Ok(b.0 as i32)
    );
}

#[test]
fn removing_a_set_fails_its_sleepers_with_eidrm() {
    let (kernel, a, _) = setup(b"");
    let b = another_process(&kernel);
    let memory_a = memory();
    let caller_a = Caller::new(a, &memory_a);
    let id = block_on(kernel.semget(&caller_a, IpcKey::IPC_PRIVATE.0, 1, CREATE)).unwrap();

    std::thread::scope(|scope| {
        scope.spawn(|| {
            let memory_b = memory();
            let caller_b = Caller::new(b, &memory_b);
            assert_eq!(
                errno(semop(&kernel, &caller_b, id, &[(0, -1, 0)])),
                Errno::EIDRM
            );
        });
        await_sleeper(&kernel, &caller_a, id);
        assert_eq!(
            semctl(&kernel, &caller_a, id, 0, IpcCmd::IPC_RMID, 0),
            Ok(0)
        );
    });
}

fn msgsnd(
    kernel: &Kernel,
    caller: &Caller<'_>,
    id: i32,
    mtype: i32,
    text: &[u8],
    flags: i32,
) -> SysResult<i32> {
    let mut buf = mtype.to_le_bytes().to_vec();
    buf.extend(text);
    block_on(caller.memory().write(0x400, &buf)).unwrap();
    block_on(kernel.msgsnd(caller, id, GuestPtr::new(0x400), text.len() as u32, flags))
}

fn msgrcv(
    kernel: &Kernel,
    caller: &Caller<'_>,
    id: i32,
    size: u32,
    mtype: i32,
    flags: i32,
) -> SysResult<(i32, Vec<u8>)> {
    let n = block_on(kernel.msgrcv(caller, id, GuestPtrMut::new(0x500), size, mtype, flags))?;
    let mut out = vec![0; 4 + n as usize];
    block_on(caller.memory().read(0x500, &mut out)).unwrap();
    Ok((
        i32::from_le_bytes(out[..4].try_into().unwrap()),
        out[4..].to_vec(),
    ))
}

#[test]
fn message_queues_select_by_type_between_processes() {
    let (kernel, a, _) = setup(b"");
    let b = another_process(&kernel);
    let (memory_a, memory_b) = (memory(), memory());
    let (caller_a, caller_b) = (Caller::new(a, &memory_a), Caller::new(b, &memory_b));
    let id = block_on(kernel.msgget(&caller_a, 77, CREATE)).unwrap();
    assert_eq!(block_on(kernel.msgget(&caller_b, 77, 0)), Ok(id));
    let nowait = IpcFlags::IPC_NOWAIT.0 as i32;

    assert_eq!(msgsnd(&kernel, &caller_a, id, 3, b"three", 0), Ok(0));
    assert_eq!(msgsnd(&kernel, &caller_a, id, 1, b"one", 0), Ok(0));
    assert_eq!(msgsnd(&kernel, &caller_a, id, 2, b"two", 0), Ok(0));
    assert_eq!(
        errno(msgsnd(&kernel, &caller_a, id, 0, b"bad", 0)),
        Errno::EINVAL
    );

    assert_eq!(errno(msgrcv(&kernel, &caller_b, id, 2, 3, 0)), Errno::E2BIG);
    let noerror = 0x1000;
    assert_eq!(
        msgrcv(&kernel, &caller_b, id, 2, 3, noerror),
        Ok((3, b"th".to_vec()))
    );
    // The lowest type up to 2.
    assert_eq!(
        msgrcv(&kernel, &caller_b, id, 16, -2, 0),
        Ok((1, b"one".to_vec()))
    );
    assert_eq!(
        errno(msgrcv(&kernel, &caller_b, id, 16, 5, nowait)),
        Errno::ENOMSG
    );

    assert_eq!(
        block_on(kernel.msgctl(&caller_a, id, IpcCmd::IPC_STAT.0, GuestPtrMut::new(0x600))),
        Ok(0)
    );
    let ds: Msqid64Ds = block_on(read_value(&memory_a, GuestPtr::new(0x600))).unwrap();
    assert_eq!(
        (ds.msg_qnum, ds.msg_cbytes, ds.msg_lspid, ds.msg_lrpid),
        (1, 3, a.0 as i32, b.0 as i32)
    );
    assert_eq!(
        msgrcv(&kernel, &caller_b, id, 16, 0, 0),
        Ok((2, b"two".to_vec()))
    );
}

#[test]
fn ipc_objects_keep_the_pid_of_a_reaped_process() {
    let (kernel, a, _) = setup(b"");
    let b = another_process(&kernel);
    let (memory_a, memory_b) = (memory(), memory());
    let (caller_a, caller_b) = (Caller::new(a, &memory_a), Caller::new(b, &memory_b));
    let msg = block_on(kernel.msgget(&caller_a, 78, CREATE)).unwrap();
    let shm = block_on(kernel.shmget(&caller_a, 6, 100, CREATE)).unwrap();
    assert_eq!(msgsnd(&kernel, &caller_b, msg, 1, b"hi", 0), Ok(0));
    let at = block_on(kernel.shmat(&caller_b, shm, 0, 0)).unwrap() as u32;
    assert_eq!(block_on(kernel.shmdt(&caller_b, at)), Ok(0));
    // B, without a parent, is reaped as it exits; its pid is free, and the execution layer
    // then lets its task go too.
    assert_eq!(block_on(kernel.exit_group(&caller_b, 0)), Ok(()));
    kernel.release(b);
    assert_eq!(
        errno(block_on(kernel.sys_kill(&caller_a, b.0 as i32, Signal(0)))),
        Errno::ESRCH
    );
    // The queue and the segment still show it, as Linux's hold its `struct pid`.
    assert_eq!(
        block_on(kernel.msgctl(&caller_a, msg, IpcCmd::IPC_STAT.0, GuestPtrMut::new(0x600))),
        Ok(0)
    );
    let ds: Msqid64Ds = block_on(read_value(&memory_a, GuestPtr::new(0x600))).unwrap();
    assert_eq!(ds.msg_lspid, b.0 as i32);
    assert_eq!(
        block_on(kernel.shmctl(&caller_a, shm, IpcCmd::IPC_STAT.0, GuestPtrMut::new(0x700))),
        Ok(0)
    );
    let ds: Shmid64Ds = block_on(read_value(&memory_a, GuestPtr::new(0x700))).unwrap();
    assert_eq!((ds.shm_cpid, ds.shm_lpid), (a.0 as i32, b.0 as i32));
}

#[test]
fn shared_memory_hands_data_over_at_detach_and_attach() {
    let (kernel, a, _) = setup(b"");
    let b = another_process(&kernel);
    let (memory_a, memory_b) = (memory(), memory());
    let (caller_a, caller_b) = (Caller::new(a, &memory_a), Caller::new(b, &memory_b));
    let id = block_on(kernel.shmget(&caller_a, 5, 100, CREATE)).unwrap();
    let stat = |caller: &Caller<'_>, memory: &FlatMemory| {
        block_on(kernel.shmctl(caller, id, IpcCmd::IPC_STAT.0, GuestPtrMut::new(0x700)))?;
        Ok::<Shmid64Ds, SysErr>(block_on(read_value(memory, GuestPtr::new(0x700))).unwrap())
    };

    let at_a = block_on(kernel.shmat(&caller_a, id, 0, 0)).unwrap() as u32;
    put(&memory_a, at_a, b"from a");
    let at_b = block_on(kernel.shmat(&caller_b, id, 0, 0)).unwrap() as u32;
    // A has not reached a synchronisation point yet.
    assert_eq!(get(&memory_b, at_b, 6), [0; 6]);
    assert_eq!(stat(&caller_a, &memory_a).unwrap().shm_nattch, 2);
    // A semaphore operation publishes A's stores and picks up B's.
    let sem = block_on(kernel.semget(&caller_a, IpcKey::IPC_PRIVATE.0, 1, CREATE)).unwrap();
    assert_eq!(semop(&kernel, &caller_a, sem, &[(0, 1, 0)]), Ok(0));
    assert_eq!(semop(&kernel, &caller_b, sem, &[(0, -1, 0)]), Ok(0));
    assert_eq!(get(&memory_b, at_b, 6), b"from a");

    put(&memory_b, at_b, b"B");
    assert_eq!(block_on(kernel.shmdt(&caller_b, at_b)), Ok(0));
    assert_eq!(
        errno(block_on(kernel.shmdt(&caller_b, at_b))),
        Errno::EINVAL
    );
    let rmid = block_on(kernel.shmctl(&caller_a, id, IpcCmd::IPC_RMID.0, GuestPtrMut::NULL));
    assert_eq!(rmid, Ok(0));
    // Still attached by A: marked for removal and no longer found by its key.
    let ds = stat(&caller_a, &memory_a).unwrap();
    assert_eq!(
        (ds.shm_nattch, ds.shm_perm.key, ds.shm_perm.mode & 0o1000),
        (1, 0, 0o1000)
    );
    assert_eq!(
        errno(block_on(kernel.shmget(&caller_a, 5, 100, 0))),
        Errno::ENOENT
    );
    assert_eq!(block_on(kernel.shmdt(&caller_a, at_a)), Ok(0));
    assert_eq!(errno(stat(&caller_a, &memory_a)), Errno::EINVAL);
}

#[test]
fn segment_attachments_merge_both_sides_changes_at_every_sync() {
    let (kernel, a, _) = setup(b"");
    let b = another_process(&kernel);
    let (memory_a, memory_b) = (memory(), memory());
    let (caller_a, caller_b) = (Caller::new(a, &memory_a), Caller::new(b, &memory_b));
    // Two pages, so the copies cross a page of the segment.
    let size = 2 * WASM_PAGE_SIZE as u32;
    let id = block_on(kernel.shmget(&caller_a, 6, size, CREATE)).unwrap();
    let at_a = block_on(kernel.shmat(&caller_a, id, 0, 0)).unwrap() as u32;
    let at_b = block_on(kernel.shmat(&caller_b, id, 0, 0)).unwrap() as u32;
    let sem = block_on(kernel.semget(&caller_a, IpcKey::IPC_PRIVATE.0, 1, CREATE)).unwrap();
    let sync = |caller| assert_eq!(semop(&kernel, caller, sem, &[(0, 1, 0)]), Ok(0));
    let edge = WASM_PAGE_SIZE as u32 - 2;

    put(&memory_a, at_a + edge, b"a:1");
    sync(&caller_a);
    sync(&caller_b);
    assert_eq!(get(&memory_b, at_b + edge, 3), b"a:1");
    // Nothing changed on either side: syncing again keeps what both hold.
    sync(&caller_a);
    sync(&caller_b);
    assert_eq!(get(&memory_a, at_a + edge, 3), b"a:1");
    assert_eq!(get(&memory_b, at_b + edge, 3), b"a:1");
    // Changes on both sides meet; A sees B's after its own sync published nothing new.
    put(&memory_b, at_b, b"b:2");
    put(&memory_a, at_a + size - 3, b"a:3");
    sync(&caller_b);
    sync(&caller_a);
    sync(&caller_b);
    assert_eq!(get(&memory_a, at_a, 3), b"b:2");
    assert_eq!(get(&memory_b, at_b + size - 3, 3), b"a:3");
    assert_eq!(get(&memory_a, at_a + edge, 3), b"a:1");
}

fn mq_open(
    kernel: &Kernel,
    caller: &Caller<'_>,
    name: &[u8],
    flags: OpenFlags,
    attr: Option<(i32, i32)>,
) -> SysResult<i32> {
    let mut cname = name.to_vec();
    cname.push(0);
    block_on(caller.memory().write(0x800, &cname)).unwrap();
    let attr = match attr {
        None => GuestPtr::NULL,
        Some((maxmsg, msgsize)) => {
            let attr = MqAttr {
                mq_maxmsg: maxmsg,
                mq_msgsize: msgsize,
                ..MqAttr::default()
            };
            block_on(write_value(caller.memory(), GuestPtrMut::new(0x900), &attr)).unwrap();
            GuestPtr::new(0x900)
        }
    };
    block_on(kernel.mq_open(caller, GuestPtr::new(0x800), flags, 0o600, attr))
}

#[test]
fn posix_queues_order_by_priority_and_notify_once() {
    let (kernel, a, _) = setup(b"");
    let b = another_process(&kernel);
    let (memory_a, memory_b) = (memory(), memory());
    let (caller_a, caller_b) = (Caller::new(a, &memory_a), Caller::new(b, &memory_b));
    let create = OpenFlags::O_CREAT | OpenFlags::O_RDWR | OpenFlags::O_NONBLOCK;
    let qa = mq_open(&kernel, &caller_a, b"q", create, Some((2, 16))).unwrap();
    let qb = mq_open(&kernel, &caller_b, b"q", OpenFlags::O_RDWR, None).unwrap();
    let excl = create | OpenFlags::O_EXCL;
    assert_eq!(
        errno(mq_open(&kernel, &caller_a, b"q", excl, None)),
        Errno::EEXIST
    );
    assert_eq!(
        errno(mq_open(&kernel, &caller_a, b"a/b", create, None)),
        Errno::EACCES
    );
    assert_eq!(
        errno(mq_open(&kernel, &caller_a, b"big", create, Some((11, 16)))),
        Errno::EINVAL
    );
    let send = |caller: &Caller<'_>, q, text: &[u8], prio| {
        block_on(caller.memory().write(0xa00, text)).unwrap();
        block_on(kernel.mq_timedsend_time64(
            caller,
            q,
            GuestPtr::new(0xa00),
            text.len() as u32,
            prio,
            GuestPtr::NULL,
        ))
    };
    let receive = |caller: &Caller<'_>, q, size| {
        let n = block_on(kernel.mq_timedreceive_time64(
            caller,
            q,
            GuestPtrMut::new(0xb00),
            size,
            GuestPtrMut::new(0xc00),
            GuestPtr::NULL,
        ))?;
        let mut text = vec![0; n as usize];
        block_on(caller.memory().read(0xb00, &mut text)).unwrap();
        let prio: u32 = block_on(read_value(caller.memory(), GuestPtr::new(0xc00))).unwrap();
        Ok::<_, SysErr>((prio, text))
    };

    // A asks for SIGUSR1 (10) when a message arrives; B may not register as well.
    let mut raw: Vec<u8> = vec![0; 4];
    raw.extend(10i32.to_le_bytes());
    raw.extend(0i32.to_le_bytes());
    raw.resize(64, 0);
    block_on(memory_a.write(0xd00, &raw)).unwrap();
    // Blocked, so it stays pending where rt_sigpending sees it.
    put(&memory_a, 0xf00, &(1u64 << 9).to_le_bytes());
    let block = SigprocmaskHow::SIG_BLOCK;
    let masked = block_on(kernel.rt_sigprocmask(
        &caller_a,
        block,
        GuestPtr::new(0xf00),
        GuestPtrMut::NULL,
        8,
    ));
    assert_eq!(masked, Ok(0));
    block_on(memory_b.write(0xd00, &raw)).unwrap();
    assert_eq!(
        block_on(kernel.mq_notify(&caller_a, qa, GuestPtr::new(0xd00))),
        Ok(0)
    );
    assert_eq!(
        errno(block_on(kernel.mq_notify(
            &caller_b,
            qb,
            GuestPtr::new(0xd00)
        ))),
        Errno::EBUSY
    );

    assert_eq!(send(&caller_b, qb, b"low", 1), Ok(0));
    assert_eq!(send(&caller_b, qb, b"high", 7), Ok(0));
    assert_eq!(errno(send(&caller_a, qa, b"full", 0)), Errno::EAGAIN);
    assert_eq!(errno(send(&caller_a, qa, &[0; 17], 0)), Errno::EMSGSIZE);
    assert_eq!(errno(receive(&caller_a, qa, 8)), Errno::EMSGSIZE);
    assert_eq!(receive(&caller_a, qa, 16), Ok((7, b"high".to_vec())));
    assert_eq!(receive(&caller_a, qa, 16), Ok((1, b"low".to_vec())));
    assert_eq!(errno(receive(&caller_a, qa, 16)), Errno::EAGAIN);

    // The notification went to A as SIGUSR1 and is gone now: B may register.
    let pending = block_on(kernel.rt_sigpending(&caller_a, GuestPtrMut::new(0xe00), 8));
    assert_eq!(pending, Ok(0));
    assert_eq!(get(&memory_a, 0xe00, 2), [0, 2]);
    assert_eq!(
        block_on(kernel.mq_notify(&caller_b, qb, GuestPtr::new(0xd00))),
        Ok(0)
    );

    put(&memory_a, 0x800, b"q ");
    assert_eq!(
        block_on(kernel.mq_unlink(&caller_a, GuestPtr::new(0x800))),
        Ok(0)
    );
    assert_eq!(
        errno(mq_open(&kernel, &caller_a, b"q", OpenFlags::O_RDWR, None)),
        Errno::ENOENT
    );
    // The queue lives on while descriptors refer to it.
    assert_eq!(send(&caller_a, qa, b"still", 0), Ok(0));
}

/// ipc/mqueue.c do_mq_open/mq_unlink use fs/namei.c getname: an empty pathname is
/// ENOENT before the single-component name/permission checks run.
#[test]
fn posix_queue_empty_names_return_enoent() {
    let (kernel, task, _) = setup(b"");
    let memory = memory();
    let caller = Caller::new(task, &memory);
    for flags in [OpenFlags::O_RDONLY, OpenFlags::O_CREAT | OpenFlags::O_RDWR] {
        assert_eq!(
            errno(mq_open(&kernel, &caller, b"", flags, None)),
            Errno::ENOENT
        );
    }
    put(&memory, 0x800, b"\0");
    assert_eq!(
        errno(block_on(kernel.mq_unlink(&caller, GuestPtr::new(0x800)))),
        Errno::ENOENT
    );
}

#[test]
fn only_the_owner_root_or_cap_fowner_unlink_a_posix_queue() {
    let (kernel, root, memory_root) = super::cred::machine_as(0, 0);
    let user = another_process(&kernel);
    let memory_user = memory();
    let (root_caller, user_caller) = (
        Caller::new(root, &memory_root),
        Caller::new(user, &memory_user),
    );
    assert_eq!(block_on(kernel.setuid(&user_caller, 1000)), Ok(0));
    let create = OpenFlags::O_CREAT | OpenFlags::O_RDWR;
    assert_eq!(
        mq_open(&kernel, &root_caller, b"root", create, None).map(|_| ()),
        Ok(())
    );
    assert!(mq_open(&kernel, &user_caller, b"user", create, None).is_ok());
    let unlink = |caller: &Caller<'_>, name: &[u8]| {
        let mut cname = name.to_vec();
        cname.push(0);
        block_on(caller.memory().write(0x800, &cname)).unwrap();
        block_on(kernel.mq_unlink(caller, GuestPtr::new(0x800)))
    };

    // The mqueue root is sticky: another user's queue stays.
    assert_eq!(errno(unlink(&user_caller, b"root")), Errno::EPERM);
    assert_eq!(unlink(&user_caller, b"user"), Ok(0));
    assert_eq!(errno(unlink(&user_caller, b"user")), Errno::ENOENT);
    assert_eq!(unlink(&root_caller, b"root"), Ok(0));
}

#[test]
fn ipcget_checks_the_size_before_the_permissions() {
    let (kernel, root, memory_root) = super::cred::machine_as(0, 0);
    let user = another_process(&kernel);
    let memory_user = memory();
    let (root_caller, user_caller) = (
        Caller::new(root, &memory_root),
        Caller::new(user, &memory_user),
    );
    assert_eq!(block_on(kernel.setuid(&user_caller, 1000)), Ok(0));
    assert!(block_on(kernel.shmget(&root_caller, 7, 100, CREATE)).is_ok());

    // `shm_more_checks` comes first: too large is EINVAL even for whom may not use it.
    let read = 0o400;
    let too_large = block_on(kernel.shmget(&user_caller, 7, 200, read));
    assert_eq!(errno(too_large), Errno::EINVAL);
    let fits = block_on(kernel.shmget(&user_caller, 7, 50, read));
    assert_eq!(errno(fits), Errno::EACCES);
}
