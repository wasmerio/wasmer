//! Notification pipe UAPI and watch-list lifetimes (kernel/watch_queue.c, fs/pipe.c).

use wasixv2_abi::{
    FcntlCmd, GuestLayout, IoctlRequest, KeyNotification, Rlimit64, RlimitResource,
    WatchNotification, WatchNotificationFilter, WatchNotificationTypeFilter,
};
use wasixv2_syscalls::{read_value, write_value};
use wasixv2_wait::{PollMask, Pollable};

use super::{
    ipc::{errno, get, machine, pipe, put},
    *,
};

fn ioctl(
    kernel: &Kernel,
    caller: &Caller<'_>,
    fd: Fd,
    req: IoctlRequest,
    arg: u32,
) -> SysResult<i32> {
    block_on(kernel.ioctl(caller, fd, req, arg))
}

fn note(subtype: u32, key: u32, aux: u32) -> Vec<u8> {
    let n = KeyNotification {
        watch: WatchNotification {
            type_subtype: 1 | subtype << 24,
            info: 16,
        },
        key_id: key,
        aux,
    };
    let mut out = vec![0; KeyNotification::SIZE];
    n.write_to(&mut out);
    out
}

#[test]
fn notification_pipes_validate_configuration_and_reject_user_writes_and_splice() {
    let (kernel, task, memory) = machine();
    let caller = Caller::new(task, &memory);
    let flags = OpenFlags::O_NOTIFICATION_PIPE | OpenFlags::O_NONBLOCK;
    let (r, w) = pipe(&kernel, &caller, flags);
    assert_eq!(
        ioctl(
            &kernel,
            &caller,
            r,
            IoctlRequest::IOC_WATCH_QUEUE_SET_SIZE,
            0
        ),
        Err(SysErr::Errno(Errno::EINVAL))
    );
    assert_eq!(
        ioctl(
            &kernel,
            &caller,
            r,
            IoctlRequest::IOC_WATCH_QUEUE_SET_SIZE,
            513
        ),
        Err(SysErr::Errno(Errno::EINVAL))
    );
    assert_eq!(
        ioctl(
            &kernel,
            &caller,
            w,
            IoctlRequest::IOC_WATCH_QUEUE_SET_SIZE,
            1
        ),
        Ok(0)
    );
    assert_eq!(
        ioctl(
            &kernel,
            &caller,
            r,
            IoctlRequest::IOC_WATCH_QUEUE_SET_SIZE,
            1
        ),
        Err(SysErr::Errno(Errno::EBUSY))
    );
    assert_eq!(
        block_on(kernel.fcntl64(&caller, r, FcntlCmd::F_GETPIPE_SZ, 0)),
        Ok(4096)
    );
    assert_eq!(
        block_on(kernel.fcntl64(&caller, r, FcntlCmd::F_SETPIPE_SZ, 4096)),
        Err(SysErr::Errno(Errno::EBUSY))
    );
    put(&memory, 0x200, b"x");
    assert_eq!(
        block_on(kernel.write(&caller, w, UserSlice::new(0x200, 1))),
        Err(SysErr::Errno(Errno::EXDEV))
    );
    let (ordinary, other) = pipe(&kernel, &caller, OpenFlags::O_NONBLOCK);
    assert_eq!(
        ioctl(
            &kernel,
            &caller,
            ordinary,
            IoctlRequest::IOC_WATCH_QUEUE_SET_SIZE,
            1
        ),
        Err(SysErr::Errno(Errno::ENODEV))
    );
    assert_eq!(
        ioctl(
            &kernel,
            &caller,
            ordinary,
            IoctlRequest::IOC_WATCH_QUEUE_SET_FILTER,
            0
        ),
        Err(SysErr::Errno(Errno::ENODEV))
    );
    assert_eq!(
        block_on(kernel.splice(
            &caller,
            r,
            GuestPtrMut::NULL,
            other,
            GuestPtrMut::NULL,
            1,
            wasixv2_abi::SpliceFlags(0)
        )),
        Err(SysErr::Errno(Errno::EINVAL))
    );
    assert_eq!(
        block_on(kernel.tee(&caller, r, other, 1, wasixv2_abi::SpliceFlags(0))),
        Err(SysErr::Errno(Errno::EINVAL))
    );
}

#[test]
fn notification_reads_preserve_records_on_small_buffers_and_faults_and_report_loss_in_order() {
    let (kernel, task, memory) = machine();
    let caller = Caller::new(task, &memory);
    let (r, w) = pipe(
        &kernel,
        &caller,
        OpenFlags::O_NOTIFICATION_PIPE | OpenFlags::O_NONBLOCK,
    );
    assert_eq!(
        ioctl(
            &kernel,
            &caller,
            r,
            IoctlRequest::IOC_WATCH_QUEUE_SET_SIZE,
            1
        ),
        Ok(0)
    );
    let writer = kernel.io_file(&caller, w).unwrap();
    assert!(writer.poll(None).contains(PollMask::WRITABLE));
    let list = WatchList::default();
    assert_eq!(list.watch(&kernel, &caller, 42, r, 23), Ok(()));
    assert_eq!(list.watch(&kernel, &caller, 42, r, 24), Err(Errno::EBUSY));
    for i in 0..33 {
        list.post(42, &note(1, 42, i));
    }
    assert!(!writer.poll(None).contains(PollMask::WRITABLE));
    assert_eq!(
        ioctl(&kernel, &caller, r, IoctlRequest::FIONREAD, 0x200),
        Ok(0)
    );
    assert_eq!(
        block_on(read_value::<u32>(&memory, GuestPtr::new(0x200))).unwrap(),
        32 * 16
    );
    assert_eq!(
        block_on(kernel.read(&caller, r, UserSliceMut::new(0x300, 15))),
        Err(SysErr::Errno(Errno::ENOBUFS))
    );
    assert_eq!(
        block_on(kernel.read(&caller, r, UserSliceMut::new(65528, 16))),
        Err(SysErr::Errno(Errno::EFAULT))
    );
    assert_eq!(
        block_on(kernel.read(&caller, r, UserSliceMut::new(0x300, 32 * 16 + 8))),
        Ok(32 * 16 + 8)
    );
    assert!(writer.poll(None).contains(PollMask::WRITABLE));
    for i in 0..32 {
        let actual: KeyNotification =
            block_on(read_value(&memory, GuestPtr::new(0x300 + i * 16))).unwrap();
        assert_eq!(
            (
                actual.watch.type_subtype,
                actual.watch.info,
                actual.key_id,
                actual.aux
            ),
            (0x0100_0001, 16 | 23 << 8, 42, i)
        );
    }
    let loss: WatchNotification =
        block_on(read_value(&memory, GuestPtr::new(0x300 + 32 * 16))).unwrap();
    assert_eq!((loss.type_subtype, loss.info), (0x0100_0000, 8));
    assert_eq!(
        errno(block_on(kernel.read(
            &caller,
            r,
            UserSliceMut::new(0x300, 16)
        ))),
        Errno::EAGAIN
    );
    // Removing a watch queues an unfiltered removal notification, including object identity.
    assert_eq!(list.watch(&kernel, &caller, 42, r, -1), Ok(()));
    assert_eq!(
        block_on(kernel.read(&caller, r, UserSliceMut::new(0x300, 16))),
        Ok(16)
    );
    assert_eq!(
        get(&memory, 0x300, 16),
        [
            0u32.to_le_bytes().as_slice(),
            (16u32 | 23 << 8).to_le_bytes().as_slice(),
            42u64.to_le_bytes().as_slice()
        ]
        .concat()
    );
    assert_eq!(list.watch(&kernel, &caller, 42, r, -1), Err(Errno::EBADSLT));
}

#[test]
fn notification_filters_validate_all_entries_and_default_to_reject() {
    let (kernel, task, memory) = machine();
    let caller = Caller::new(task, &memory);
    let (r, _w) = pipe(
        &kernel,
        &caller,
        OpenFlags::O_NOTIFICATION_PIPE | OpenFlags::O_NONBLOCK,
    );
    assert_eq!(
        ioctl(
            &kernel,
            &caller,
            r,
            IoctlRequest::IOC_WATCH_QUEUE_SET_SIZE,
            1
        ),
        Ok(0)
    );
    let list = WatchList::default();
    list.watch(&kernel, &caller, 5, r, 9).unwrap();
    let set = |header: WatchNotificationFilter, filter: WatchNotificationTypeFilter| {
        block_on(write_value(&memory, GuestPtrMut::new(0x500), &header)).unwrap();
        block_on(write_value(&memory, GuestPtrMut::new(0x508), &filter)).unwrap();
        ioctl(
            &kernel,
            &caller,
            r,
            IoctlRequest::IOC_WATCH_QUEUE_SET_FILTER,
            0x500,
        )
    };
    let header = WatchNotificationFilter {
        nr_filters: 1,
        reserved: 0,
    };
    let mut f = WatchNotificationTypeFilter {
        r#type: 1,
        info_filter: 9 << 8,
        info_mask: 0xff00,
        subtype_filter: [0; 8],
    };
    f.subtype_filter[0] = 1 << 2;
    assert_eq!(
        set(
            WatchNotificationFilter {
                nr_filters: 0,
                ..header
            },
            f
        ),
        Err(SysErr::Errno(Errno::EINVAL))
    );
    assert_eq!(
        set(
            WatchNotificationFilter {
                reserved: 1,
                ..header
            },
            f
        ),
        Err(SysErr::Errno(Errno::EINVAL))
    );
    assert_eq!(
        set(
            header,
            WatchNotificationTypeFilter {
                info_mask: 0xff01,
                ..f
            }
        ),
        Err(SysErr::Errno(Errno::EINVAL))
    );
    assert_eq!(
        set(
            header,
            WatchNotificationTypeFilter {
                info_filter: 1 << 16,
                ..f
            }
        ),
        Err(SysErr::Errno(Errno::EINVAL))
    );
    assert_eq!(set(header, f), Ok(0));
    list.post(5, &note(1, 5, 0)); // wrong subtype
    list.post(5, &note(2, 5, 8));
    assert_eq!(
        block_on(kernel.read(&caller, r, UserSliceMut::new(0x300, 32))),
        Ok(16)
    );
    assert_eq!(get(&memory, 0x30c, 4), 8u32.to_le_bytes());
    assert_eq!(
        set(header, WatchNotificationTypeFilter { r#type: 100, ..f }),
        Ok(0)
    );
    list.post(5, &note(2, 5, 9));
    assert_eq!(
        errno(block_on(kernel.read(
            &caller,
            r,
            UserSliceMut::new(0x300, 16)
        ))),
        Errno::EAGAIN
    );
    assert_eq!(
        ioctl(
            &kernel,
            &caller,
            r,
            IoctlRequest::IOC_WATCH_QUEUE_SET_FILTER,
            0
        ),
        Ok(0)
    );
    list.post(5, &note(1, 5, 10));
    assert_eq!(
        block_on(kernel.read(&caller, r, UserSliceMut::new(0x300, 16))),
        Ok(16)
    );
    // Destruction sends removal even through a reject-all filter.
    assert_eq!(
        set(header, WatchNotificationTypeFilter { r#type: 100, ..f }),
        Ok(0)
    );
    drop(list);
    assert_eq!(
        block_on(kernel.read(&caller, r, UserSliceMut::new(0x300, 16))),
        Ok(16)
    );
    assert_eq!(get(&memory, 0x300, 4), [0; 4]);
}

#[test]
fn notification_watch_limits_are_per_user_and_return_on_last_pipe_close() {
    let (kernel, task, memory) = machine();
    let caller = Caller::new(task, &memory);
    let (r, w) = pipe(&kernel, &caller, OpenFlags::O_NOTIFICATION_PIPE);
    let (r2, _w2) = pipe(&kernel, &caller, OpenFlags::O_NOTIFICATION_PIPE);
    let limit = Rlimit64 {
        rlim_cur: 1,
        rlim_max: 1024,
    };
    block_on(write_value(&memory, GuestPtrMut::new(0x500), &limit)).unwrap();
    assert_eq!(
        block_on(kernel.prlimit64(
            &caller,
            0,
            RlimitResource::RLIMIT_NOFILE,
            GuestPtr::new(0x500),
            GuestPtrMut::NULL
        )),
        Ok(0)
    );
    let list = WatchList::default();
    let other = WatchList::default();
    assert_eq!(list.watch(&kernel, &caller, 1, r, 0), Ok(()));
    assert_eq!(other.watch(&kernel, &caller, 2, r2, 0), Err(Errno::EAGAIN));
    assert_eq!(
        other.watch(&kernel, &caller, 2, Fd(-1), 0),
        Err(Errno::EINVAL)
    );
    assert_eq!(
        other.watch(&kernel, &caller, 2, r2, 256),
        Err(Errno::EINVAL)
    );
    block_on(kernel.close(&caller, r)).unwrap();
    assert_eq!(other.watch(&kernel, &caller, 2, r2, 0), Err(Errno::EAGAIN));
    block_on(kernel.close(&caller, w)).unwrap();
    assert_eq!(other.watch(&kernel, &caller, 2, r2, 0), Ok(()));
}

#[test]
fn filter_copy_faults_precede_invalid_entry_errors() {
    let (kernel, task, memory) = machine();
    let caller = Caller::new(task, &memory);
    let (r, _) = pipe(&kernel, &caller, OpenFlags::O_NOTIFICATION_PIPE);
    let address = 65536 - 8 - WatchNotificationTypeFilter::SIZE as u32;
    let header = WatchNotificationFilter {
        nr_filters: 2,
        reserved: 0,
    };
    let invalid = WatchNotificationTypeFilter {
        info_mask: 1,
        ..WatchNotificationTypeFilter::default()
    };
    block_on(write_value(&memory, GuestPtrMut::new(address), &header)).unwrap();
    block_on(write_value(
        &memory,
        GuestPtrMut::new(address + 8),
        &invalid,
    ))
    .unwrap();
    assert_eq!(
        ioctl(
            &kernel,
            &caller,
            r,
            IoctlRequest::IOC_WATCH_QUEUE_SET_FILTER,
            address
        ),
        Err(SysErr::Errno(Errno::EFAULT))
    );
    block_on(write_value(
        &memory,
        GuestPtrMut::new(address),
        &WatchNotificationFilter {
            nr_filters: 1,
            ..header
        },
    ))
    .unwrap();
    assert_eq!(
        ioctl(
            &kernel,
            &caller,
            r,
            IoctlRequest::IOC_WATCH_QUEUE_SET_FILTER,
            address
        ),
        Err(SysErr::Errno(Errno::EINVAL))
    );
}
