//! File mappings: private copies, shared mappings kept coherent with the file, and `mmap`'s
//! checks; `MADV_WIPEONFORK`, which only anonymous memory takes.

use super::{
    ipc::{errno, get, put},
    *,
};
use crate::sync::MutexExt;

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

fn mremap(
    kernel: &Kernel,
    caller: &Caller<'_>,
    addr: u32,
    old_len: u32,
    new_len: u32,
    flags: MremapFlags,
    new_addr: u32,
) -> SysResult<i32> {
    block_on(kernel.mremap(caller, addr, old_len, new_len, flags, new_addr))
}

const MOVE: MremapFlags = MremapFlags::MREMAP_MAYMOVE;
const FIXED: MremapFlags = MremapFlags(MOVE.0 | MremapFlags::MREMAP_FIXED.0);
const DONTUNMAP: MremapFlags = MremapFlags(MOVE.0 | MremapFlags::MREMAP_DONTUNMAP.0);

#[test]
fn mremap_fixed_onto_its_own_source_fails_and_keeps_the_data() {
    let (kernel, task, memory, _) = with_file(b"");
    let caller = Caller::new(task, &memory);
    let anon = MapFlags::MAP_PRIVATE | MapFlags::MAP_ANONYMOUS;
    let at = mmap(&kernel, &caller, 2 * PAGE, anon, Fd(-1)).unwrap();
    let far = mmap(&kernel, &caller, 3 * PAGE, anon, Fd(-1)).unwrap();
    put(&memory, at, b"first");
    put(&memory, at + PAGE, b"second");

    // `vrm_overlaps`: the target may not overlap the source, from either side.
    let overlaps = [at + PAGE, at, at.wrapping_sub(PAGE)];
    for target in overlaps {
        let moved = mremap(&kernel, &caller, at, 2 * PAGE, 2 * PAGE, FIXED, target);
        assert_eq!(errno(moved), Errno::EINVAL, "target {target:#x}");
    }
    assert_eq!(get(&memory, at, 5), b"first");
    assert_eq!(get(&memory, at + PAGE, 6), b"second");
    // Parameters are checked before the source is looked at.
    let unmapped = 0x7000_0000;
    let moved = mremap(&kernel, &caller, unmapped, PAGE, PAGE, FIXED, unmapped);
    assert_eq!(errno(moved), Errno::EINVAL);
    let moved = mremap(&kernel, &caller, unmapped, PAGE, PAGE, FIXED, far);
    assert_eq!(errno(moved), Errno::EFAULT);
    let fixed_alone = MremapFlags::MREMAP_FIXED;
    let moved = mremap(&kernel, &caller, at, PAGE, PAGE, fixed_alone, far);
    assert_eq!(errno(moved), Errno::EINVAL);
    let moved = mremap(&kernel, &caller, at, PAGE, PAGE, FIXED, far + 1);
    assert_eq!(errno(moved), Errno::EINVAL);
    let moved = mremap(
        &kernel,
        &caller,
        at,
        PAGE,
        2 * PAGE,
        FIXED,
        u32::MAX - PAGE + 1,
    );
    assert_eq!(errno(moved), Errno::EINVAL);

    let moved = mremap(&kernel, &caller, at, 2 * PAGE, 2 * PAGE, FIXED, far + PAGE);
    assert_eq!(moved, Ok((far + PAGE) as i32));
    assert_eq!(get(&memory, far + PAGE, 5), b"first");
    assert_eq!(get(&memory, far + 2 * PAGE, 6), b"second");
}

// `mmap_min_addr`: no mapping starts below it, whatever the flags, once the range itself was
// checked; the caller is root, and no capability lowers it.
#[test]
fn fixed_mappings_below_mmap_min_addr_fail_with_eperm() {
    let (kernel, task, memory, fd) = with_file(b"");
    let caller = Caller::new(task, &memory);
    let anon = MapFlags::MAP_PRIVATE | MapFlags::MAP_ANONYMOUS;
    let fixed = |flags: MapFlags, addr: u32, fd: Fd| {
        block_on(kernel.mmap(&caller, addr, PAGE, RW, flags, fd, 0))
    };

    let replace = fixed(anon | MapFlags::MAP_FIXED, 0, Fd(-1));
    assert_eq!(errno(replace), Errno::EPERM);
    let noreplace = fixed(anon | MapFlags::MAP_FIXED_NOREPLACE, 0, Fd(-1));
    assert_eq!(errno(noreplace), Errno::EPERM);
    let file = fixed(MapFlags::MAP_SHARED | MapFlags::MAP_FIXED, 0, fd);
    assert_eq!(errno(file), Errno::EPERM);
    // An address off a page boundary is `EINVAL` first.
    let unaligned = fixed(anon | MapFlags::MAP_FIXED, 0x1000, Fd(-1));
    assert_eq!(errno(unaligned), Errno::EINVAL);
    let at = mmap(&kernel, &caller, PAGE, anon, Fd(-1)).unwrap();
    assert!(u64::from(at) >= crate::mm::MMAP_MIN_ADDR);
    let moved = mremap(&kernel, &caller, at, PAGE, PAGE, FIXED, 0);
    assert_eq!(errno(moved), Errno::EPERM);
}

#[test]
fn mremap_dontunmap_moves_the_pages_and_leaves_the_source_mapped() {
    let (kernel, task, memory, fd) = with_file(b"from the file");
    let caller = Caller::new(task, &memory);
    let anon = MapFlags::MAP_PRIVATE | MapFlags::MAP_ANONYMOUS;
    let at = mmap(&kernel, &caller, PAGE, anon, Fd(-1)).unwrap();
    put(&memory, at, b"anonymous");

    let resized = mremap(&kernel, &caller, at, PAGE, 2 * PAGE, DONTUNMAP, 0);
    assert_eq!(errno(resized), Errno::EINVAL);
    let moved = mremap(&kernel, &caller, at, PAGE, PAGE, DONTUNMAP, 0).unwrap() as u32;
    assert_ne!(moved, at);
    assert_eq!(get(&memory, moved, 9), b"anonymous");
    // The source is still mapped, and reads as fresh anonymous memory.
    assert_eq!(get(&memory, at, 9), [0; 9]);
    let dontneed = MadviseAdvice::MADV_DONTNEED;
    assert_eq!(block_on(kernel.madvise(&caller, at, PAGE, dontneed)), Ok(0));

    // A private file mapping's source reads as the file again.
    let file = mmap(&kernel, &caller, PAGE, MapFlags::MAP_PRIVATE, fd).unwrap();
    put(&memory, file, b"FROM");
    let moved = mremap(&kernel, &caller, file, PAGE, PAGE, DONTUNMAP, 0).unwrap() as u32;
    assert_eq!(get(&memory, moved, 13), b"FROM the file");
    assert_eq!(get(&memory, file, 13), b"from the file");
}

#[test]
fn mremap_of_no_bytes_duplicates_a_shared_mapping() {
    let (kernel, task, memory, fd) = with_file(b"shared bytes");
    let caller = Caller::new(task, &memory);
    let shared = mmap(&kernel, &caller, PAGE, MapFlags::MAP_SHARED, fd).unwrap();
    let private = mmap(&kernel, &caller, PAGE, MapFlags::MAP_PRIVATE, fd).unwrap();

    let dup = mremap(&kernel, &caller, private, 0, PAGE, MOVE, 0);
    assert_eq!(errno(dup), Errno::EINVAL);
    let dup = mremap(&kernel, &caller, shared, 0, PAGE, MremapFlags(0), 0);
    assert_eq!(errno(dup), Errno::ENOMEM);
    let dup = mremap(&kernel, &caller, shared, 0, PAGE, MOVE, 0).unwrap() as u32;
    assert_ne!(dup, shared);
    assert_eq!(get(&memory, dup, 12), b"shared bytes");
    // Both are mappings of the file: a change through one reaches the other at msync.
    put(&memory, dup, b"SHARED");
    let msync = |at| block_on(kernel.msync(&caller, at, PAGE, MsyncFlags::MS_SYNC));
    assert_eq!(msync(dup), Ok(0));
    assert_eq!(msync(shared), Ok(0));
    assert_eq!(get(&memory, shared, 12), b"SHARED bytes");
}

/// Whether `pid` is seen to have exited: a zombie, or reaped already.
fn exited(kernel: &Kernel, pid: TaskId) -> bool {
    let tree = kernel.procs.tree.lock_or_recover();
    tree.nodes.get(&pid.0).is_none_or(|node| node.is_zombie())
}

/// A fatal signal ends the group as an exit does: each task leaves the memory as the execution
/// layer unwinds it ([`Kernel::exit_mm`]), the last one writes back the shared mappings, and
/// only after that is the process seen to exit (`do_exit`: `exit_mm`, then `exit_notify`).
#[test]
fn a_fatal_signal_writes_back_shared_mappings_once_the_last_task_leaves_the_memory() {
    let (kernel, task, memory, fd) = with_file(b"hello file");
    let caller = Caller::new(task, &memory);
    let at = mmap(&kernel, &caller, PAGE, MapFlags::MAP_SHARED, fd).unwrap();
    let null = GuestPtrMut::NULL;
    let spawn = kernel.thread_spawn(&caller, 0x10f00, 0, null, 0, null);
    let thread = TaskId(block_on(spawn).unwrap() as u32);
    let thread_caller = Caller::new(thread, &memory);
    while kernel.take_pending(task).is_some() {}
    put(&memory, at, b"HELLO");
    let reader = another_process(&kernel);
    let reader_memory = FlatMemory(RefCell::new(vec![0; 2 * PAGE as usize]));
    let reader_caller = Caller::new(reader, &reader_memory);
    let reader_fd = open(&kernel, &reader_caller, b"/f", OpenFlags::O_RDONLY);
    let contents = || pread(&kernel, &reader_caller, reader_fd, 10, 0);

    kernel.kill(task, Signal::SIGTERM).unwrap();
    for t in [task, thread] {
        assert_eq!(kernel.take_pending(t), Some(GuestAction::Exit(128 + 15)));
    }
    assert!(!exited(&kernel, task));

    block_on(kernel.exit_mm(&thread_caller));
    assert_eq!(
        contents(),
        b"hello file",
        "the leader still uses the memory"
    );
    kernel.release_task(thread);
    assert!(!exited(&kernel, task), "the leader is still in its memory");
    block_on(kernel.exit_mm(&caller));
    assert_eq!(contents(), b"HELLO file");
    assert!(exited(&kernel, task));
}

fn madvise(
    kernel: &Kernel,
    caller: &Caller<'_>,
    at: u32,
    len: u32,
    advice: MadviseAdvice,
) -> SysResult<i32> {
    block_on(kernel.madvise(caller, at, len, advice))
}

const WIPE: MadviseAdvice = MadviseAdvice::MADV_WIPEONFORK;
const KEEP: MadviseAdvice = MadviseAdvice::MADV_KEEPONFORK;

/// `[at, at + pages pages)`, as [`Kernel::wipe_on_fork`] lists ranges.
fn pages_at(at: u32, pages: u32) -> std::ops::Range<u64> {
    u64::from(at)..u64::from(at + pages * PAGE)
}

#[test]
fn madvise_wipeonfork_takes_only_private_anonymous_memory() {
    let (kernel, task, memory, fd) = with_file(b"file");
    let caller = Caller::new(task, &memory);
    let anon = MapFlags::MAP_PRIVATE | MapFlags::MAP_ANONYMOUS;
    let at = mmap(&kernel, &caller, 3 * PAGE, anon, Fd(-1)).unwrap();
    let private = mmap(&kernel, &caller, PAGE, MapFlags::MAP_PRIVATE, fd).unwrap();
    let shared = mmap(&kernel, &caller, PAGE, MapFlags::MAP_SHARED, fd).unwrap();
    assert_eq!(private, at + 3 * PAGE);

    // File mappings, private or shared, refuse it; marking them "keep" is harmless.
    assert_eq!(
        errno(madvise(&kernel, &caller, private, PAGE, WIPE)),
        Errno::EINVAL
    );
    assert_eq!(
        errno(madvise(&kernel, &caller, shared, PAGE, WIPE)),
        Errno::EINVAL
    );
    assert_eq!(madvise(&kernel, &caller, shared, PAGE, KEEP), Ok(0));
    assert_eq!(madvise(&kernel, &caller, at, 0, WIPE), Ok(0));
    assert!(kernel.wipe_on_fork(task).is_empty());

    // Over anonymous memory and on into a file mapping: the anonymous areas before it take the
    // flag, as `madvise_walk_vmas` changes them in order, and the file mapping fails.
    assert_eq!(
        errno(madvise(&kernel, &caller, at, 4 * PAGE, WIPE)),
        Errno::EINVAL
    );
    assert_eq!(kernel.wipe_on_fork(task), [pages_at(at, 3)]);
    // KEEPONFORK clears it again, here splitting the area; a length rounds up to a page.
    assert_eq!(madvise(&kernel, &caller, at + PAGE, 100, KEEP), Ok(0));
    assert_eq!(
        kernel.wipe_on_fork(task),
        [pages_at(at, 1), pages_at(at + 2 * PAGE, 1)]
    );
}

#[test]
fn wipeonfork_stays_with_its_pages_until_they_are_unmapped_or_the_image_changes() {
    let (kernel, task, _) = setup(b"");
    let memory = super::proc::ForkableMemory(FlatMemory(RefCell::new(vec![0; PAGE as usize])));
    let caller = Caller::new(task, &memory);
    super::task::install_prog(&kernel, &caller, &memory.0);
    let anon = MapFlags::MAP_PRIVATE | MapFlags::MAP_ANONYMOUS;
    let at = mmap(&kernel, &caller, PAGE, anon, Fd(-1)).unwrap();
    assert_eq!(madvise(&kernel, &caller, at, PAGE, WIPE), Ok(0));

    // Growing in place extends the area, and the flag with it.
    assert_eq!(
        mremap(&kernel, &caller, at, PAGE, 2 * PAGE, MremapFlags(0), 0),
        Ok(at as i32)
    );
    assert_eq!(kernel.wipe_on_fork(task), [pages_at(at, 2)]);
    // Moving it moves the flag; with MREMAP_DONTUNMAP the source keeps its own.
    let moved = mremap(&kernel, &caller, at, 2 * PAGE, 3 * PAGE, FIXED, 8 * PAGE).unwrap() as u32;
    assert_eq!(kernel.wipe_on_fork(task), [pages_at(moved, 3)]);
    let anywhere = mremap(&kernel, &caller, moved, PAGE, PAGE, DONTUNMAP, 0).unwrap() as u32;
    let mut expected = vec![pages_at(moved, 3), pages_at(anywhere, 1)];
    expected.sort_by_key(|range| range.start);
    assert_eq!(kernel.wipe_on_fork(task), expected);
    assert_eq!(block_on(kernel.munmap(&caller, anywhere, PAGE)), Ok(0));
    // Shrinking unmaps the tail, flag and all; a new mapping there starts without it.
    assert_eq!(
        mremap(&kernel, &caller, moved, 3 * PAGE, PAGE, MremapFlags(0), 0),
        Ok(moved as i32)
    );
    assert_eq!(kernel.wipe_on_fork(task), [pages_at(moved, 1)]);
    let fixed = MapFlags(anon.0 | MapFlags::MAP_FIXED.0);
    let refilled = block_on(kernel.mmap(&caller, moved + PAGE, PAGE, RW, fixed, Fd(-1), 0));
    assert_eq!(refilled, Ok(moved + PAGE));
    assert_eq!(kernel.wipe_on_fork(task), [pages_at(moved, 1)]);

    // A fork child's areas keep it (`dup_mmap` copies the flags), so its own children's do too.
    let child = super::proc::fork_child(&kernel, &caller);
    assert_eq!(kernel.wipe_on_fork(child), [pages_at(moved, 1)]);
    let child_caller = Caller::new(child, &memory);
    let grandchild = super::proc::fork_child(&kernel, &child_caller);
    assert_eq!(kernel.wipe_on_fork(grandchild), [pages_at(moved, 1)]);
    // The child's changes are its own.
    assert_eq!(madvise(&kernel, &child_caller, moved, PAGE, KEEP), Ok(0));
    assert!(kernel.wipe_on_fork(child).is_empty());
    assert_eq!(kernel.wipe_on_fork(task), [pages_at(moved, 1)]);

    // A new image starts with a new address space.
    assert_eq!(block_on(super::task::execve(&kernel, &caller)), Ok(0));
    assert!(kernel.wipe_on_fork(task).is_empty());

    // So does a munmap.
    let at = mmap(&kernel, &child_caller, PAGE, anon, Fd(-1)).unwrap();
    assert_eq!(madvise(&kernel, &child_caller, at, PAGE, WIPE), Ok(0));
    assert_eq!(block_on(kernel.munmap(&child_caller, at, PAGE)), Ok(0));
    assert!(kernel.wipe_on_fork(child).is_empty());
}
