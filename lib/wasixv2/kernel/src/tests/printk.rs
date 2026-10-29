//! The kernel log (`kernel/printk/printk.c`): `syslog(2)`, `/dev/kmsg`, `/proc/kmsg` and their
//! tunables.

use wasixv2_abi::SeekWhence;
use wasixv2_vfs::MountFlags;

use super::{
    ipc::{errno, get, put},
    *,
};

const CWD: Fd = Fd(-100);

/// A kernel with `/proc` and `/dev`, and one process running as `uid`.
fn machine_as(uid: u32) -> (Kernel, TaskId, FlatMemory) {
    let (kernel, task, memory) =
        super::cred::machine_with(|kernel| kernel.with_credentials(Credentials::new(uid, uid)));
    let vfs = kernel.vfs().clone();
    block_on(vfs.mount(b"/proc", kernel.procfs(), MountFlags::default(), true)).unwrap();
    let dev = MemFs::new(Arc::new(HostClock(kernel.host.clone())), 0o755, 0, 0);
    block_on(vfs.mount(b"/dev", Arc::new(dev), MountFlags::default(), true)).unwrap();
    block_on(kernel.create_device_nodes(b"/dev")).unwrap();
    (kernel, task, memory)
}

/// `syslog(action, buf, len)` with the buffer at 0x4000 of `memory`, and what it copied there.
fn syslog_in(
    kernel: &Kernel,
    memory: &FlatMemory,
    task: TaskId,
    action: i32,
    len: i32,
) -> SysResult<String> {
    let caller = Caller::new(task, memory);
    let n = block_on(kernel.syslog(&caller, action, GuestPtrMut::new(0x4000), len))?;
    Ok(String::from_utf8(get(memory, 0x4000, n.max(0) as usize)).unwrap())
}

fn open(
    kernel: &Kernel,
    caller: &Caller<'_>,
    memory: &FlatMemory,
    at: &str,
    flags: OpenFlags,
) -> SysResult<Fd> {
    put(memory, 0x1000, format!("{at}\0").as_bytes());
    block_on(kernel.openat(caller, CWD, GuestPtr::new(0x1000), flags, 0))
}

fn read(
    kernel: &Kernel,
    caller: &Caller<'_>,
    memory: &FlatMemory,
    fd: Fd,
    len: u32,
) -> SysResult<String> {
    let n = block_on(kernel.read(caller, fd, UserSliceMut::new(0x6000, len)))?;
    Ok(String::from_utf8(get(memory, 0x6000, n as usize)).unwrap())
}

fn write(
    kernel: &Kernel,
    caller: &Caller<'_>,
    memory: &FlatMemory,
    fd: Fd,
    text: &str,
) -> SysResult<u32> {
    put(memory, 0x5000, text.as_bytes());
    block_on(kernel.write(caller, fd, UserSlice::new(0x5000, text.len() as u32)))
}

/// `syslog(2)`: anyone reads everything and the size while `dmesg_restrict` is off; reading
/// on, clearing and the console need `CAP_SYSLOG`. The log starts with the banner.
#[test]
fn syslog_reads_the_log_as_its_actions_say() {
    let (kernel, task, memory) = machine_as(0);
    let caller = Caller::new(task, &memory);

    assert_eq!(
        block_on(kernel.syslog(&caller, 10, GuestPtrMut::NULL, 0)),
        Ok(1 << 17)
    );
    let all = syslog_in(&kernel, &memory, task, 3, 4096).unwrap();
    assert!(
        all.starts_with("<5>[    1.500000] Linux version 7.2.0 "),
        "{all}"
    );
    assert!(all.ends_with('\n'), "{all}");
    kernel.printk(LogLevel::Err, "two\nlines");
    let unread = block_on(kernel.syslog(&caller, 9, GuestPtrMut::NULL, 0)).unwrap() as usize;
    let all = syslog_in(&kernel, &memory, task, 3, 4096).unwrap();
    assert_eq!(unread, all.len());
    assert!(
        all.ends_with("<3>[    1.500000] two\n<3>[    1.500000] lines\n"),
        "{all}"
    );
    // Only what fits, newest first.
    let last = syslog_in(&kernel, &memory, task, 3, 50).unwrap();
    assert_eq!(last, "<3>[    1.500000] two\n<3>[    1.500000] lines\n");

    // `SYSLOG_ACTION_READ` consumes, a record at a time when the buffer is short.
    let first = syslog_in(&kernel, &memory, task, 2, 10).unwrap();
    assert_eq!(first, "<5>[    1.");
    let rest = syslog_in(&kernel, &memory, task, 2, 4096).unwrap();
    assert!(rest.starts_with("500000] Linux"), "{rest}");
    assert_eq!(
        block_on(kernel.syslog(&caller, 9, GuestPtrMut::NULL, 0)),
        Ok(0)
    );

    assert_eq!(syslog_in(&kernel, &memory, task, 5, 0), Ok(String::new()));
    assert_eq!(
        syslog_in(&kernel, &memory, task, 3, 4096),
        Ok(String::new())
    );
    assert_eq!(
        errno(syslog_in(&kernel, &memory, task, 8, 9)),
        Errno::EINVAL
    );
    assert_eq!(
        errno(syslog_in(&kernel, &memory, task, 11, 0)),
        Errno::EINVAL
    );
    assert_eq!(
        errno(syslog_in(&kernel, &memory, task, 3, -1)),
        Errno::EINVAL
    );
}

#[test]
fn syslog_restricts_what_needs_cap_syslog() {
    let (kernel, task, memory) = machine_as(1000);
    let caller = Caller::new(task, &memory);
    assert!(syslog_in(&kernel, &memory, task, 3, 4096).is_ok());
    let size = block_on(kernel.syslog(&caller, 10, GuestPtrMut::NULL, 0));
    assert_eq!(size, Ok(1 << 17));
    for action in [0, 1, 2, 4, 5, 6, 7, 9] {
        assert_eq!(
            errno(syslog_in(&kernel, &memory, task, action, 10)),
            Errno::EPERM,
            "{action}"
        );
    }
    let fd = open(
        &kernel,
        &caller,
        &memory,
        "/proc/sys/kernel/dmesg_restrict",
        OpenFlags::O_RDONLY,
    )
    .unwrap();
    assert_eq!(read(&kernel, &caller, &memory, fd, 64), Ok("0\n".into()));
    kernel.printk.set_dmesg_restrict(true);
    assert_eq!(
        errno(syslog_in(&kernel, &memory, task, 3, 4096)),
        Errno::EPERM
    );
    let fd = open(&kernel, &caller, &memory, "/dev/kmsg", OpenFlags::O_RDONLY);
    assert_eq!(errno(fd), Errno::EPERM);
    // Writing is for root, by the node's mode.
    let wronly = open(&kernel, &caller, &memory, "/dev/kmsg", OpenFlags::O_WRONLY);
    assert_eq!(errno(wronly), Errno::EACCES);
}

/// `/dev/kmsg`: one record per read with its priority, sequence number and time; writes are
/// the user's records at the default level unless a priority says otherwise; seeks only to
/// the start, past the cleared records, or the end.
#[test]
fn dev_kmsg_reads_records_and_takes_messages() {
    let (kernel, task, memory) = machine_as(0);
    let caller = Caller::new(task, &memory);
    let fd = open(
        &kernel,
        &caller,
        &memory,
        "/dev/kmsg",
        OpenFlags::O_RDWR | OpenFlags::O_NONBLOCK,
    )
    .unwrap();

    let banner = read(&kernel, &caller, &memory, fd, 4096).unwrap();
    assert!(
        banner.starts_with("5,0,1500000,-;Linux version "),
        "{banner}"
    );
    assert_eq!(
        errno(read(&kernel, &caller, &memory, fd, 4096)),
        Errno::EAGAIN
    );
    assert_eq!(write(&kernel, &caller, &memory, fd, "<3>hello\n"), Ok(9));
    assert_eq!(write(&kernel, &caller, &memory, fd, "a\tb"), Ok(3));
    assert_eq!(write(&kernel, &caller, &memory, fd, "<30>daemon"), Ok(10));
    assert_eq!(
        read(&kernel, &caller, &memory, fd, 4096),
        Ok("11,1,1500000,-;hello\n".into())
    );
    assert_eq!(errno(read(&kernel, &caller, &memory, fd, 4)), Errno::EINVAL);
    assert_eq!(
        read(&kernel, &caller, &memory, fd, 4096),
        Ok("30,3,1500000,-;daemon\n".into())
    );

    let seek = |off, whence| block_on(kernel.lseek(&caller, fd, off, whence));
    assert_eq!(seek(0, SeekWhence::SEEK_SET), Ok(0));
    assert!(
        read(&kernel, &caller, &memory, fd, 4096)
            .unwrap()
            .starts_with("5,0,")
    );
    assert_eq!(errno(seek(1, SeekWhence::SEEK_SET)), Errno::ESPIPE);
    assert_eq!(seek(0, SeekWhence::SEEK_END), Ok(0));
    assert_eq!(
        errno(read(&kernel, &caller, &memory, fd, 4096)),
        Errno::EAGAIN
    );
    block_on(kernel.syslog(&caller, 5, GuestPtrMut::NULL, 0)).unwrap();
    kernel.printk(LogLevel::Info, "after clear");
    assert_eq!(seek(0, SeekWhence::SEEK_DATA), Ok(0));
    assert_eq!(
        read(&kernel, &caller, &memory, fd, 4096),
        Ok("6,4,1500000,-;after clear\n".into())
    );
}

/// A reader the ring overtook hears of it once (`EPIPE`), then goes on from the oldest record.
#[test]
fn dev_kmsg_tells_a_reader_it_lost_records() {
    let (kernel, task, memory) = machine_as(0);
    let caller = Caller::new(task, &memory);
    let fd = open(&kernel, &caller, &memory, "/dev/kmsg", OpenFlags::O_RDONLY).unwrap();
    let text = "x".repeat(100);
    for _ in 0..2000 {
        kernel.printk(LogLevel::Info, &text);
    }
    assert_eq!(
        errno(read(&kernel, &caller, &memory, fd, 4096)),
        Errno::EPIPE
    );
    let next = read(&kernel, &caller, &memory, fd, 4096).unwrap();
    assert!(
        !next.starts_with("6,1,") && next.starts_with("6,"),
        "{next}"
    );
}

/// `/proc/kmsg` consumes the log as `SYSLOG_ACTION_READ` does, for root only.
#[test]
fn proc_kmsg_consumes_the_log() {
    let (kernel, task, memory) = machine_as(0);
    let caller = Caller::new(task, &memory);
    let fd = open(
        &kernel,
        &caller,
        &memory,
        "/proc/kmsg",
        OpenFlags::O_RDONLY | OpenFlags::O_NONBLOCK,
    )
    .unwrap();
    let text = read(&kernel, &caller, &memory, fd, 4096).unwrap();
    assert!(
        text.starts_with("<5>[    1.500000] Linux version"),
        "{text}"
    );
    assert_eq!(
        errno(read(&kernel, &caller, &memory, fd, 4096)),
        Errno::EAGAIN
    );
    kernel.printk(LogLevel::Warning, "more");
    assert_eq!(
        read(&kernel, &caller, &memory, fd, 4096),
        Ok("<4>[    1.500000] more\n".into())
    );

    let (kernel, task, memory) = machine_as(1000);
    let caller = Caller::new(task, &memory);
    let fd = open(&kernel, &caller, &memory, "/proc/kmsg", OpenFlags::O_RDONLY);
    assert_eq!(errno(fd), Errno::EACCES);
}

/// `kernel.printk` holds the four levels; `printk_devkmsg` turns `/dev/kmsg` off.
#[test]
fn the_log_tunables_are_sysctls() {
    let (kernel, task, memory) = machine_as(0);
    let caller = Caller::new(task, &memory);
    let fd = open(
        &kernel,
        &caller,
        &memory,
        "/proc/sys/kernel/printk",
        OpenFlags::O_RDWR,
    )
    .unwrap();
    assert_eq!(
        read(&kernel, &caller, &memory, fd, 64),
        Ok("7\t4\t1\t7\n".into())
    );
    assert_eq!(write(&kernel, &caller, &memory, fd, "3 4"), Ok(3));
    assert_eq!(kernel.printk.levels(), [3, 4, 1, 7]);
    let fd = open(
        &kernel,
        &caller,
        &memory,
        "/proc/sys/kernel/printk_devkmsg",
        OpenFlags::O_RDWR,
    )
    .unwrap();
    assert_eq!(
        read(&kernel, &caller, &memory, fd, 64),
        Ok("ratelimit\n".into())
    );
    assert_eq!(
        errno(write(&kernel, &caller, &memory, fd, "never")),
        Errno::EINVAL
    );
    assert_eq!(write(&kernel, &caller, &memory, fd, "off\n"), Ok(4));
    assert_eq!(
        errno(open(
            &kernel,
            &caller,
            &memory,
            "/dev/kmsg",
            OpenFlags::O_RDONLY
        )),
        Errno::EPERM
    );
}
