//! The LSM syscalls and securityfs: the capabilities, Landlock and Yama are listed, and none
//! keeps process attributes.

use wasixv2_abi::{GuestPtr, GuestPtrMut, LsmCtxPtr};

use super::ipc::{errno, get, machine, put};
use super::*;

/// `lsm_list_modules` lists capability (100), landlock (110) and yama (105), if `size` leaves
/// room.
#[test]
fn list_modules() {
    let (kernel, task, memory) = machine();
    let caller = Caller::new(task, &memory);
    let list = |room: u32, flags| {
        put(&memory, 0x100, &room.to_le_bytes());
        let size = GuestPtrMut::new(0x100);
        block_on(kernel.lsm_list_modules(&caller, GuestPtrMut::new(0x200), size, flags))
    };
    assert_eq!(errno(list(16, 0)), Errno::E2BIG);
    assert_eq!(get(&memory, 0x100, 4), 24u32.to_le_bytes());
    assert_eq!(list(24, 0), Ok(3));
    let ids: Vec<u8> = [100u64, 110, 105]
        .iter()
        .flat_map(|i| i.to_le_bytes())
        .collect();
    assert_eq!(get(&memory, 0x200, 24), ids);
    assert_eq!(errno(list(24, 1)), Errno::EINVAL);
}

/// No module answers for process attributes: `EOPNOTSUPP` with a size of 0, after the checks
/// of the attribute, the flags and the context.
#[test]
fn self_attributes_have_no_module() {
    let (kernel, task, memory) = machine();
    let caller = Caller::new(task, &memory);
    let get_attr = |attr, flags| {
        put(&memory, 0x100, &64u32.to_le_bytes());
        let ctx = LsmCtxPtr::new(0x200);
        block_on(kernel.lsm_get_self_attr(&caller, attr, ctx, GuestPtrMut::new(0x100), flags))
    };
    assert_eq!(errno(get_attr(0, 0)), Errno::EINVAL);
    assert_eq!(errno(get_attr(100, 2)), Errno::EINVAL);
    assert_eq!(errno(get_attr(100, 0)), Errno::EOPNOTSUPP);
    assert_eq!(get(&memory, 0x100, 4), [0; 4]);
    assert_eq!(errno(get_attr(100, 1)), Errno::EOPNOTSUPP);
    let set_attr = |len: u64, ctx_len: u64, size: u32| {
        let ctx: Vec<u8> = [110, 0, len, ctx_len]
            .iter()
            .flat_map(|v: &u64| v.to_le_bytes())
            .collect();
        put(&memory, 0x300, &ctx);
        block_on(kernel.lsm_set_self_attr(&caller, 100, GuestPtr::new(0x300), size, 0))
    };
    assert_eq!(errno(set_attr(32, 0, 16)), Errno::EINVAL);
    assert_eq!(errno(set_attr(40, 8, 8192)), Errno::E2BIG);
    assert_eq!(errno(set_attr(40, 16, 40)), Errno::EINVAL);
    assert_eq!(errno(set_attr(40, 8, 40)), Errno::EOPNOTSUPP);
}

/// securityfs has `lsm`, the module names without a newline, read-only.
#[test]
fn securityfs_lists_the_modules() {
    let (kernel, task, memory) = machine();
    let caller = Caller::new(task, &memory);
    let vfs = kernel.vfs().clone();
    put(&memory, 0x100, b"/sec\0");
    let mkdir = kernel.mkdirat(&caller, Fd(-100), GuestPtr::new(0x100), 0o755);
    assert_eq!(block_on(mkdir), Ok(0));
    let mount = vfs.mount(
        b"/sec",
        kernel.securityfs(),
        wasixv2_vfs::MountFlags::default(),
        true,
    );
    block_on(mount).unwrap();
    put(&memory, 0x100, b"/sec/lsm\0");
    let open = kernel.openat(
        &caller,
        Fd(-100),
        GuestPtr::new(0x100),
        OpenFlags::O_RDONLY,
        0,
    );
    let fd = block_on(open).unwrap();
    let n = block_on(kernel.read(&caller, fd, UserSliceMut::new(0x400, 64))).unwrap();
    assert_eq!(get(&memory, 0x400, n as usize), b"capability,landlock,yama");
    let open = kernel.openat(
        &caller,
        Fd(-100),
        GuestPtr::new(0x100),
        OpenFlags::O_WRONLY,
        0,
    );
    assert_eq!(errno(block_on(open)), Errno::EACCES);
}
