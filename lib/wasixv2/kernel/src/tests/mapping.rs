//! File mappings: private copies, shared mappings kept coherent with the file, and `mmap`'s
//! checks.

use super::{
    ipc::{errno, get, put},
    *,
};

const PAGE: u32 = WASM_PAGE_SIZE as u32;
const RW: ProtFlags = ProtFlags(3);

/// A kernel with one task and two pages of memory; `/f` holds `contents`, open read-write at
/// the returned descriptor.
fn with_file(contents: &[u8]) -> (Kernel, TaskId, FlatMemory, Fd) {
    let (kernel, task, _) = setup(b"");
    let memory = FlatMemory(RefCell::new(vec![0; 2 * PAGE as usize]));
    let fd = {
        let caller = Caller::new(task, &memory);
        let fd = open(
            &kernel,
            &caller,
            b"/f",
            OpenFlags::O_CREAT | OpenFlags::O_RDWR,
        );
        put(&memory, 0x1000, contents);
        let len = contents.len() as u32;
        let written = block_on(kernel.pwrite64(&caller, fd, UserSlice::new(0x1000, len), 0));
        assert_eq!(written, Ok(len));
        fd
    };
    (kernel, task, memory, fd)
}

/// A second process on `kernel`, with empty stdio.
pub(super) fn another_process(kernel: &Kernel) -> TaskId {
    kernel.spawn(ProcessConfig {
        args: vec!["other".into()],
        env: Vec::new(),
        stdio: Stdio {
            stdin: Arc::new(Input(Mutex::new(Vec::new()))),
            stdout: Arc::new(Capture::default()),
            stderr: Arc::new(Capture::default()),
        },
    })
}

pub(super) fn open(kernel: &Kernel, caller: &Caller<'_>, path: &[u8], flags: OpenFlags) -> Fd {
    let memory = caller.memory();
    let mut name = path.to_vec();
    name.push(0);
    block_on(memory.write(0x800, &name)).unwrap();
    block_on(kernel.openat(caller, Fd(-100), GuestPtr::new(0x800), flags, 0o644)).unwrap()
}

fn mmap(kernel: &Kernel, caller: &Caller<'_>, len: u32, flags: MapFlags, fd: Fd) -> SysResult<u32> {
    block_on(kernel.mmap(caller, 0, len, RW, flags, fd, 0))
}

fn pread(kernel: &Kernel, caller: &Caller<'_>, fd: Fd, len: u32, pos: i64) -> Vec<u8> {
    let n = block_on(kernel.pread64(caller, fd, UserSliceMut::new(0x1000, len), pos)).unwrap();
    let memory = caller.memory();
    let mut out = vec![0; n as usize];
    block_on(memory.read(0x1000, &mut out)).unwrap();
    out
}

#[test]
fn private_mapping_copies_the_file_and_keeps_its_changes() {
    let (kernel, task, memory, fd) = with_file(b"hello file");
    let caller = Caller::new(task, &memory);

    let at = mmap(&kernel, &caller, 100, MapFlags::MAP_PRIVATE, fd).unwrap();
    assert_eq!(get(&memory, at, 10), b"hello file");
    // Past the end of the file the page reads as zeroes.
    assert_eq!(get(&memory, at + 10, 6), [0; 6]);
    put(&memory, at, b"HELLO");
    assert_eq!(block_on(kernel.munmap(&caller, at, 100)), Ok(0));

    assert_eq!(pread(&kernel, &caller, fd, 10, 0), b"hello file");
}

#[test]
fn shared_mapping_writes_back_on_msync_and_munmap_but_never_past_the_end() {
    let (kernel, task, memory, fd) = with_file(b"0123456789");
    let caller = Caller::new(task, &memory);
    let at = mmap(&kernel, &caller, PAGE, MapFlags::MAP_SHARED, fd).unwrap();

    put(&memory, at + 2, b"ab");
    put(&memory, at + 20, b"beyond");
    let flags = MsyncFlags::MS_SYNC;
    assert_eq!(block_on(kernel.msync(&caller, at, PAGE, flags)), Ok(0));
    assert_eq!(pread(&kernel, &caller, fd, 64, 0), b"01ab456789");
    // Bytes past the end stay in the mapping, as in the last page cache page.
    assert_eq!(get(&memory, at + 20, 6), b"beyond");

    put(&memory, at + 9, b"Z");
    assert_eq!(block_on(kernel.munmap(&caller, at, PAGE)), Ok(0));
    assert_eq!(pread(&kernel, &caller, fd, 64, 0), b"01ab45678Z");
}

#[test]
fn shared_mapping_and_file_io_of_one_process_agree() {
    let (kernel, task, memory, fd) = with_file(b"aaaaaaaa");
    let caller = Caller::new(task, &memory);
    let at = mmap(&kernel, &caller, 8, MapFlags::MAP_SHARED, fd).unwrap();

    // read() sees the mapping's stores without an msync.
    put(&memory, at, b"bb");
    assert_eq!(pread(&kernel, &caller, fd, 8, 0), b"bbaaaaaa");
    // The mapping sees write() at once.
    put(&memory, 0x1000, b"cc");
    let wrote = block_on(kernel.pwrite64(&caller, fd, UserSlice::new(0x1000, 2), 4));
    assert_eq!(wrote, Ok(2));
    assert_eq!(get(&memory, at, 8), b"bbaaccaa");
}

#[test]
fn shared_mappings_of_two_processes_meet_at_msync() {
    let (kernel, a, memory_a, fd_a) = with_file(b"........");
    let b = another_process(&kernel);
    let memory_b = FlatMemory(RefCell::new(vec![0; 2 * PAGE as usize]));
    let caller_a = Caller::new(a, &memory_a);
    let caller_b = Caller::new(b, &memory_b);
    let fd_b = open(&kernel, &caller_b, b"/f", OpenFlags::O_RDWR);
    let at_a = mmap(&kernel, &caller_a, 8, MapFlags::MAP_SHARED, fd_a).unwrap();
    let at_b = mmap(&kernel, &caller_b, 8, MapFlags::MAP_SHARED, fd_b).unwrap();
    let msync = |caller, at| block_on(kernel.msync(caller, at, 8, MsyncFlags::MS_ASYNC));

    put(&memory_a, at_a, b"A");
    put(&memory_b, at_b + 7, b"B");
    assert_eq!(msync(&caller_a, at_a), Ok(0));
    assert_eq!(msync(&caller_b, at_b), Ok(0));
    assert_eq!(get(&memory_b, at_b, 8), b"A......B");
    assert_eq!(msync(&caller_a, at_a), Ok(0));
    assert_eq!(get(&memory_a, at_a, 8), b"A......B");
}

#[test]
fn mmap_checks_the_file_as_linux_does() {
    let (kernel, task, memory, _) = with_file(b"x");
    let caller = Caller::new(task, &memory);
    let readonly = open(&kernel, &caller, b"/f", OpenFlags::O_RDONLY);
    let writeonly = open(&kernel, &caller, b"/f", OpenFlags::O_WRONLY);
    let dir = open(&kernel, &caller, b"/", OpenFlags::O_RDONLY);
    let shared = MapFlags::MAP_SHARED;

    assert_eq!(
        errno(mmap(&kernel, &caller, 1, shared, readonly)),
        Errno::EACCES
    );
    let read_only_prot = block_on(kernel.mmap(&caller, 0, 1, ProtFlags(1), shared, readonly, 0));
    assert!(read_only_prot.is_ok());
    let private = MapFlags::MAP_PRIVATE;
    assert_eq!(
        errno(mmap(&kernel, &caller, 1, private, writeonly)),
        Errno::EACCES
    );
    assert_eq!(
        errno(mmap(&kernel, &caller, 1, private, dir)),
        Errno::ENODEV
    );
    let validate = MapFlags::MAP_SHARED_VALIDATE | MapFlags::MAP_SYNC;
    assert_eq!(
        errno(mmap(&kernel, &caller, 1, validate, readonly)),
        Errno::EOPNOTSUPP
    );
    let anon_validate = MapFlags::MAP_SHARED_VALIDATE | MapFlags::MAP_ANONYMOUS;
    assert_eq!(
        errno(mmap(&kernel, &caller, 1, anon_validate, Fd(-1))),
        Errno::EINVAL
    );
    let negative = block_on(kernel.mmap(&caller, 0, 1, RW, private, readonly, -(PAGE as i64)));
    assert_eq!(errno(negative), Errno::EOVERFLOW);
}

#[test]
fn madvise_dontneed_reloads_a_private_mapping_from_its_file() {
    let (kernel, task, memory, fd) = with_file(b"original");
    let caller = Caller::new(task, &memory);
    let at = mmap(&kernel, &caller, 8, MapFlags::MAP_PRIVATE, fd).unwrap();

    put(&memory, at, b"changed!");
    let dontneed = MadviseAdvice::MADV_DONTNEED;
    assert_eq!(block_on(kernel.madvise(&caller, at, 8, dontneed)), Ok(0));

    assert_eq!(get(&memory, at, 8), b"original");
}
