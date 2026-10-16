//! memfd_create and file seals.

use wasixv2_abi::{Cstr, FallocFlags, FcntlCmd, MapFlags, MemfdFlags, ProtFlags, SealFlags};
use wasixv2_vfs::Attr;

use super::{
    ipc::{errno, get, machine, pipe, put},
    *,
};

const NAME: u32 = 0x100;
const DATA: u32 = 0x1000;

fn memfd(kernel: &Kernel, caller: &Caller<'_>, name: &[u8], flags: MemfdFlags) -> SysResult<Fd> {
    let memory = caller.memory();
    block_on(memory.write(NAME, &[name, b"\0"].concat())).unwrap();
    block_on(kernel.memfd_create(caller, Cstr::new(NAME), flags))
}

fn fcntl(kernel: &Kernel, caller: &Caller<'_>, fd: Fd, cmd: FcntlCmd, arg: u32) -> SysResult<i32> {
    block_on(kernel.fcntl64(caller, fd, cmd, arg))
}

fn add_seals(kernel: &Kernel, caller: &Caller<'_>, fd: Fd, seals: SealFlags) -> SysResult<i32> {
    fcntl(kernel, caller, fd, FcntlCmd::F_ADD_SEALS, seals.0)
}

fn seals(kernel: &Kernel, caller: &Caller<'_>, fd: Fd) -> SysResult<i32> {
    fcntl(kernel, caller, fd, FcntlCmd::F_GET_SEALS, 0)
}

fn pwrite(kernel: &Kernel, caller: &Caller<'_>, fd: Fd, len: u32, pos: i64) -> SysResult<u32> {
    block_on(kernel.pwrite64(caller, fd, UserSlice::new(DATA, len), pos))
}

fn ftruncate(kernel: &Kernel, caller: &Caller<'_>, fd: Fd, len: i64) -> SysResult<i32> {
    block_on(kernel.ftruncate64(caller, fd, len))
}

fn attr(kernel: &Kernel, task: TaskId, fd: Fd) -> Attr {
    let file = kernel.file(task, fd).unwrap();
    block_on(file.inode().unwrap().node().getattr()).unwrap()
}

fn map_shared(kernel: &Kernel, caller: &Caller<'_>, fd: Fd, prot: ProtFlags) -> SysResult<u32> {
    let len = WASM_PAGE_SIZE as u32;
    block_on(kernel.mmap(caller, 0, len, prot, MapFlags::MAP_SHARED, fd, 0))
}

#[test]
fn memfd_is_an_unnamed_read_write_file() {
    let (kernel, task, memory) = machine();
    let caller = Caller::new(task, &memory);
    let fd = memfd(&kernel, &caller, b"buf", MemfdFlags::MFD_CLOEXEC).unwrap();

    let flags = fcntl(&kernel, &caller, fd, FcntlCmd::F_GETFL, 0).unwrap();
    assert_eq!(flags as u32, (OpenFlags::O_RDWR | OpenFlags::O_LARGEFILE).0);
    assert_eq!(fcntl(&kernel, &caller, fd, FcntlCmd::F_GETFD, 0), Ok(1));
    let attr = attr(&kernel, task, fd);
    assert_eq!((attr.mode & 0o7777, attr.nlink, attr.size), (0o777, 0, 0));
    assert_eq!((attr.uid, attr.gid), (1, 1));
    let file = kernel.file(task, fd).unwrap();
    let path = &file.inode().unwrap().path;
    assert_eq!(
        kernel.vfs.d_path(path, &kernel.vfs.root()),
        b"/memfd:buf (deleted)"
    );

    put(&memory, DATA, b"hello");
    assert_eq!(pwrite(&kernel, &caller, fd, 5, 2), Ok(5));
    assert_eq!(ftruncate(&kernel, &caller, fd, 4), Ok(0));
    let n = block_on(kernel.pread64(&caller, fd, UserSliceMut::new(0x2000, 8), 0));
    assert_eq!(n, Ok(4));
    assert_eq!(get(&memory, 0x2000, 4), b"\0\0he");
}

#[test]
fn memfd_create_checks_flags_and_name() {
    let (kernel, task, memory) = machine();
    let caller = Caller::new(task, &memory);
    let exec_and_noexec = MemfdFlags::MFD_EXEC | MemfdFlags::MFD_NOEXEC_SEAL;
    let unknown = MemfdFlags(0x40);
    let huge_size = MemfdFlags(21 << 26);

    assert_eq!(errno(memfd(&kernel, &caller, b"x", unknown)), Errno::EINVAL);
    assert_eq!(
        errno(memfd(&kernel, &caller, b"x", exec_and_noexec)),
        Errno::EINVAL
    );
    assert_eq!(
        errno(memfd(&kernel, &caller, b"x", huge_size)),
        Errno::EINVAL
    );
    let huge = MemfdFlags::MFD_HUGETLB | huge_size;
    assert_eq!(errno(memfd(&kernel, &caller, b"x", huge)), Errno::ENOSYS);
    let long = vec![b'n'; 250];
    assert_eq!(
        errno(memfd(&kernel, &caller, &long, MemfdFlags::empty())),
        Errno::EINVAL
    );
    assert!(memfd(&kernel, &caller, &long[..249], MemfdFlags::empty()).is_ok());
    let fault = block_on(kernel.memfd_create(&caller, Cstr::new(1 << 30), MemfdFlags::empty()));
    assert_eq!(errno(fault), Errno::EFAULT);
    let null = block_on(kernel.memfd_create(&caller, Cstr::new(0), MemfdFlags::empty()));
    assert_eq!(errno(null), Errno::EFAULT);
}

#[test]
fn memfd_create_needs_a_shm_filesystem() {
    let host: Arc<dyn Host> = Arc::new(TestHost);
    let root = MemFs::new(Arc::new(HostClock(host.clone())), 0o755, 0, 0);
    let kernel = Kernel::new(host, Arc::new(root));
    let task = kernel.spawn(ProcessConfig {
        args: Vec::new(),
        env: Vec::new(),
        stdio: Stdio::from_fns(|_| {}, |_| {}),
    });
    let memory = FlatMemory(RefCell::new(vec![0; 65536]));
    let caller = Caller::new(task, &memory);

    let fd = memfd(&kernel, &caller, b"x", MemfdFlags::empty());
    assert_eq!(errno(fd), Errno::ENOSYS);
}

#[test]
fn seals_need_allow_sealing_and_a_tmpfs_file() {
    let (kernel, task, memory) = machine();
    let caller = Caller::new(task, &memory);
    let sealed = memfd(&kernel, &caller, b"a", MemfdFlags::empty()).unwrap();
    let open = memfd(&kernel, &caller, b"b", MemfdFlags::MFD_ALLOW_SEALING).unwrap();
    let (r, _) = pipe(&kernel, &caller, OpenFlags::empty());

    assert_eq!(seals(&kernel, &caller, sealed), Ok(1));
    let grow = SealFlags::F_SEAL_GROW;
    assert_eq!(
        errno(add_seals(&kernel, &caller, sealed, grow)),
        Errno::EPERM
    );
    assert_eq!(seals(&kernel, &caller, open), Ok(0));
    assert_eq!(
        errno(add_seals(&kernel, &caller, open, SealFlags(0x40))),
        Errno::EINVAL
    );
    assert_eq!(errno(seals(&kernel, &caller, r)), Errno::EINVAL);
    assert_eq!(errno(add_seals(&kernel, &caller, r, grow)), Errno::EINVAL);

    // A file of the tmpfs root has seals it cannot add to; opened read-only it may not try.
    put(&memory, NAME, b"/f\0");
    let create = OpenFlags::O_CREAT | OpenFlags::O_RDWR;
    let f = block_on(kernel.openat(&caller, Fd(-100), Cstr::new(NAME), create, 0o644)).unwrap();
    let ro = block_on(kernel.openat(&caller, Fd(-100), Cstr::new(NAME), OpenFlags::O_RDONLY, 0))
        .unwrap();
    assert_eq!(seals(&kernel, &caller, f), Ok(1));
    assert_eq!(errno(add_seals(&kernel, &caller, f, grow)), Errno::EPERM);
    assert_eq!(
        errno(add_seals(&kernel, &caller, ro, SealFlags(0x40))),
        Errno::EPERM
    );
}

#[test]
fn size_seals_stop_shrinking_and_growing() {
    let (kernel, task, memory) = machine();
    let caller = Caller::new(task, &memory);
    let fd = memfd(&kernel, &caller, b"s", MemfdFlags::MFD_ALLOW_SEALING).unwrap();
    assert_eq!(ftruncate(&kernel, &caller, fd, 8), Ok(0));

    assert_eq!(
        add_seals(&kernel, &caller, fd, SealFlags::F_SEAL_SHRINK),
        Ok(0)
    );
    assert_eq!(errno(ftruncate(&kernel, &caller, fd, 4)), Errno::EPERM);
    assert_eq!(ftruncate(&kernel, &caller, fd, 10), Ok(0));

    assert_eq!(
        add_seals(&kernel, &caller, fd, SealFlags::F_SEAL_GROW),
        Ok(0)
    );
    assert_eq!(seals(&kernel, &caller, fd), Ok(0x6));
    assert_eq!(errno(ftruncate(&kernel, &caller, fd, 11)), Errno::EPERM);
    assert_eq!(ftruncate(&kernel, &caller, fd, 10), Ok(0));
    // A write is cut short at the end, and one starting there fails.
    assert_eq!(pwrite(&kernel, &caller, fd, 4, 8), Ok(2));
    assert_eq!(errno(pwrite(&kernel, &caller, fd, 4, 10)), Errno::EPERM);
    assert_eq!(pwrite(&kernel, &caller, fd, 0, 10), Ok(0));
    let alloc = |mode, offset, len| block_on(kernel.fallocate(&caller, fd, mode, offset, len));
    let keep = FallocFlags::FALLOC_FL_KEEP_SIZE;
    assert_eq!(errno(alloc(FallocFlags(0), 8, 4)), Errno::EPERM);
    assert_eq!(errno(alloc(keep, 8, 4)), Errno::EPERM);
    assert_eq!(alloc(FallocFlags(0), 0, 10), Ok(0));
    assert_eq!(attr(&kernel, task, fd).size, 10);
}

#[test]
fn write_seals_stop_writes_and_writable_shared_mappings() {
    let (kernel, task, memory) = machine();
    let caller = Caller::new(task, &memory);
    let fd = memfd(&kernel, &caller, b"w", MemfdFlags::MFD_ALLOW_SEALING).unwrap();
    assert_eq!(pwrite(&kernel, &caller, fd, 16, 0), Ok(16));
    let rw = ProtFlags::PROT_READ | ProtFlags::PROT_WRITE;
    // A writable shared mapping keeps the file from being write-sealed until it goes.
    let at = map_shared(&kernel, &caller, fd, rw).unwrap();
    let write = SealFlags::F_SEAL_WRITE;
    assert_eq!(errno(add_seals(&kernel, &caller, fd, write)), Errno::EBUSY);
    let len = WASM_PAGE_SIZE as u32;
    assert_eq!(block_on(kernel.munmap(&caller, at, len)), Ok(0));
    assert_eq!(add_seals(&kernel, &caller, fd, write), Ok(0));
    assert_eq!(errno(pwrite(&kernel, &caller, fd, 4, 0)), Errno::EPERM);
    let n = block_on(kernel.write(&caller, fd, UserSlice::new(DATA, 4)));
    assert_eq!(errno(n), Errno::EPERM);
    let punch = FallocFlags::FALLOC_FL_PUNCH_HOLE | FallocFlags::FALLOC_FL_KEEP_SIZE;
    let punched = block_on(kernel.fallocate(&caller, fd, punch, 0, 4));
    assert_eq!(errno(punched), Errno::EPERM);
    // Size changes are not writes.
    assert_eq!(ftruncate(&kernel, &caller, fd, 4), Ok(0));
    assert_eq!(errno(map_shared(&kernel, &caller, fd, rw)), Errno::EPERM);
    // A read-only one still works, and never writes back.
    let read_only = map_shared(&kernel, &caller, fd, ProtFlags::PROT_READ).unwrap();
    let mprotect = kernel.mprotect(&caller, read_only, len, rw);
    assert_eq!(errno(block_on(mprotect)), Errno::EACCES);
    assert_eq!(block_on(kernel.munmap(&caller, read_only, len)), Ok(0));

    assert_eq!(
        add_seals(&kernel, &caller, fd, SealFlags::F_SEAL_SEAL),
        Ok(0)
    );
    let shrink = SealFlags::F_SEAL_SHRINK;
    assert_eq!(errno(add_seals(&kernel, &caller, fd, shrink)), Errno::EPERM);
    assert_eq!(seals(&kernel, &caller, fd), Ok(0x9));
}

#[test]
fn exec_seals_fix_the_execute_bits() {
    let (kernel, task, memory) = machine();
    let caller = Caller::new(task, &memory);
    let noexec = memfd(&kernel, &caller, b"n", MemfdFlags::MFD_NOEXEC_SEAL).unwrap();
    assert_eq!(attr(&kernel, task, noexec).mode & 0o7777, 0o666);
    assert_eq!(seals(&kernel, &caller, noexec), Ok(0x20));
    assert_eq!(
        errno(block_on(kernel.fchmod(&caller, noexec, 0o755))),
        Errno::EPERM
    );
    assert_eq!(block_on(kernel.fchmod(&caller, noexec, 0o600)), Ok(0));
    // Sealing is allowed, and writes are too.
    assert_eq!(
        add_seals(&kernel, &caller, noexec, SealFlags::F_SEAL_GROW),
        Ok(0)
    );
    assert_eq!(pwrite(&kernel, &caller, noexec, 0, 0), Ok(0));

    // On an executable file, F_SEAL_EXEC brings every write seal with it.
    let exec = memfd(&kernel, &caller, b"e", MemfdFlags::MFD_ALLOW_SEALING).unwrap();
    assert_eq!(
        add_seals(&kernel, &caller, exec, SealFlags::F_SEAL_EXEC),
        Ok(0)
    );
    assert_eq!(seals(&kernel, &caller, exec), Ok(0x3e));
}
