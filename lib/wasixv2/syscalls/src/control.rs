use std::{
    ops::Range,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use wasixv2_abi::{Errno, Siginfo};
use wasixv2_pages::SharedPages;

use crate::{
    CpuTime, Layered, SysResult,
    caller::{Caller, TaskId},
    memory::{LocalFuture, PageAccess},
};

/// Work the execution layer must do before control returns to the guest.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GuestAction {
    /// The task ended: its thread group exited with this status, or, for a task that is not the
    /// group leader, the task itself did.
    Exit(i32),
    /// The calling task created the thread `task` (`thread-spawn`), whose tid is `tid`; start
    /// it on its own instance at the module's thread entry point with `start_arg`.
    StartThread {
        task: TaskId,
        tid: i32,
        start_arg: u32,
    },
    /// Run a signal handler on top of the interrupted guest stack, then report back through
    /// [`Control::signal_return`].
    Signal(SignalFrame),
    /// The task took part in a group stop (a stop signal's default action): it runs no guest
    /// code until [`GroupStop::stop_wait`] returns, then asks [`Control::take_pending`] again.
    Stopped,
    // ---- proc ----
    /// The task replaced its image (`execve`): run the module [`Control::take_image`] hands out
    /// as the same task, in a new instance with new memory, from `_start`. With
    /// `vfork_parent`, the task was a vfork child running in its parent's instance: the new
    /// instance runs on its own, and this one goes back to the parent as for
    /// [`GuestAction::VforkReturn`].
    Exec { vfork_parent: Option<VforkParent> },
    /// The instance runs as `task` from now on, and the current call returns `ret`: `clone`
    /// returning 0 to a vfork child.
    SwitchTask { task: TaskId, ret: i32 },
    /// The calling task created the process `child` with a copy of its address space (`clone`
    /// without `CLONE_VM`, which only an address space that
    /// [`can_duplicate`](crate::UserMemory::can_duplicate) gets): copy the memory and the guest's
    /// call stack to a new instance that runs as `child`, where the call returns 0, while here it
    /// returns the child's pid as the parent sees it, `pid`. clone's writes land after the copy,
    /// each in one address space: `pid` at `parent_tid` in this one (`CLONE_PARENT_SETTID`), the
    /// parent's new pidfd, the second value, at the first in this one too (`CLONE_PIDFD`, which
    /// the kernel checked can be written), and the child's tid as it sees it, `tid`, at
    /// `child_tid` in the child's (`CLONE_CHILD_SETTID`), whose thread pointer becomes `tls`
    /// (`CLONE_SETTLS`). The two differ for the first process of a pid namespace. With
    /// `vfork` (`CLONE_VFORK`), once the child runs, the parent's clone returns only after
    /// [`VforkWait::vfork_wait`].
    Fork {
        child: TaskId,
        pid: i32,
        tid: i32,
        parent_tid: Option<u32>,
        pidfd: Option<(u32, i32)>,
        child_tid: Option<u32>,
        tls: Option<u32>,
        vfork: bool,
    },
    /// The vfork child `child`, running in its parent's instance, ended: the instance runs as
    /// `parent` again, and the guest's `wasixv2_vfork_return(child)` export unwinds to the
    /// vfork point, unless the parent's image ended or went on elsewhere meanwhile
    /// ([`VforkParent::resumes`]).
    VforkReturn { parent: VforkParent, child: TaskId },
    /// The vfork parent `task`, which waits for the child this instance runs as, died (a fatal
    /// signal, its group's exit, another thread's `execve`): it leaves its memory now, on the
    /// memory it shares with the child, as its own `do_exit` would on Linux, while the child
    /// keeps the memory. Run [`ExitMm::exit_mm`] with a caller of `task` on this instance's
    /// memory, then go on as before. The instance goes back to `task` only once the child is
    /// done, if at all ([`VforkParent::resumes`]).
    ExitMm { task: TaskId },
    /// An `execve` of this task gave its thread group's leader `task` the new image while the
    /// leader waited for its vfork child, which runs in the leader's instance: Linux's
    /// `de_thread` kills such a leader instead of waiting for the child. Start the image
    /// [`Control::take_image`] hands out for `task` on an instance of its own, as `task`, then
    /// go on (this task's exit follows). The leader's instance goes on running the child, and
    /// ends with it ([`Resume::Gone`]); the run that instance belongs to ends with the new
    /// image's.
    StartImage { task: TaskId },
}

/// The vfork parent an instance goes back to once the child running in it ends.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VforkParent {
    pub task: TaskId,
    /// What becomes of the parent, which [`Control::take_pending`] decides when it hands out
    /// the action.
    pub resumes: Resume,
    /// The child's pid as the parent sees it, which vfork returns there: the kernel names it
    /// when it makes the child, as the child may be gone, its pid with it, by the time the
    /// parent resumes.
    pub pid: i32,
}

/// How a vfork parent goes on once its child is done with the parent's instance.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Resume {
    /// It returns from vfork: the instance runs as the parent again, whose ABI resumes it at
    /// the vfork point.
    Returns,
    /// Its image ended while it waited for the child: its thread group exited, or another
    /// thread's `execve` handed it the new image before the `execve` was done. Linux kills such
    /// a parent in `wait_for_vfork_done` (`zap_other_threads`). The instance runs as the parent
    /// without resuming it, and the parent's next work is the exit or the `execve` that ended
    /// its image.
    Dies,
    /// Its image went on in another instance: another thread's `execve` gave the leader the
    /// new image, which started on its own ([`GuestAction::StartImage`]). Nothing runs as the
    /// parent here any more: the instance ends with the child.
    Gone,
}

/// One signal handler invocation: what the execution layer writes into the guest's signal frame
/// and which guest function it calls (see `signal-callback-register` in the ABI).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SignalFrame {
    /// The registered trampoline, an index into the guest's function table.
    pub trampoline: u32,
    pub info: Siginfo,
    /// `sa_handler`, an index into the guest's function table.
    pub handler: u32,
    /// `sa_flags`.
    pub flags: u32,
    /// The mask in force before delivery, which the guest finds in `uc_sigmask`; bit `n - 1` is
    /// signal `n`.
    pub mask: u64,
    /// The task's alternate stack as delivery found it, which the guest finds in `uc_stack`
    /// (`__save_altstack`): `ss_sp`, `ss_flags` as `sigaltstack` set them, `ss_size`. The frame
    /// goes on it as [`SignalFrame::altstack_top`] says.
    pub altstack: (u32, i32, u32),
}

impl SignalFrame {
    pub fn signo(&self) -> i32 {
        self.info.si_signo
    }

    /// `SA_RESTART`: an interrupted call restarts instead of failing with `EINTR`.
    pub fn restarts(&self) -> bool {
        self.flags & wasixv2_abi::SaFlags::SA_RESTART.0 != 0
    }

    /// `on_sig_stack`: whether the stack pointer `sp` lies on the alternate stack. A stack armed
    /// with `SS_AUTODISARM` never counts: delivery disarms it, so nothing runs on it armed.
    pub fn on_altstack(&self, sp: u32) -> bool {
        let (ss_sp, flags, size) = self.altstack;
        let autodisarm = flags as u32 & wasixv2_abi::SigaltstackFlags::SS_AUTODISARM.0 != 0;
        !autodisarm && sp > ss_sp && sp - ss_sp <= size
    }

    /// `sigsp`: where the frame goes for a guest interrupted with stack pointer `sp`. The top of
    /// the alternate stack when the handler asked for it (`SA_ONSTACK`), one is set and the
    /// guest is not on it already; `None` for below `sp`.
    pub fn altstack_top(&self, sp: u32) -> Option<u32> {
        let (ss_sp, _, size) = self.altstack;
        let onstack = self.flags & wasixv2_abi::SaFlags::SA_ONSTACK.0 != 0;
        (onstack && size != 0 && !self.on_altstack(sp)).then(|| ss_sp.wrapping_add(size))
    }
}

/// Set whenever the kernel leaves work for a task: an action, a signal, or a mask change that may
/// make a pending signal deliverable (Linux's `TIF_SIGPENDING` and `TIF_NOTIFY_RESUME`). The
/// execution layer reads it without a lock on every return to the guest and asks
/// [`Control::take_pending`] only while it is set; `take_pending` clears it before it looks, so
/// work queued meanwhile raises it again.
#[derive(Debug, Default)]
pub struct WorkFlag(AtomicBool);

impl WorkFlag {
    /// A flag that is always set, for implementations that do not track work per task.
    pub fn always() -> Self {
        Self(AtomicBool::new(true))
    }

    pub fn raise(&self) {
        self.0.store(true, Ordering::SeqCst);
    }

    pub fn is_raised(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }

    pub fn clear(&self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

/// The return-to-guest interface. The execution layer asks after every syscall and at every
/// preemption point, and reports what becomes of the instances it runs tasks on.
pub trait Control:
    AtomicWait + CpuTime + ExitMm + GroupStop + PidView + SharedMappings + VforkWait
{
    /// The next thing to do for `task`: an exit, a thread to start, a signal handler to run, or
    /// nothing.
    fn take_pending(&self, task: TaskId) -> Option<GuestAction>;

    /// The flag that says whether [`Control::take_pending`] may have something for `task`.
    fn work_flag(&self, task: TaskId) -> Arc<WorkFlag> {
        let _ = task;
        Arc::new(WorkFlag::always())
    }

    /// A handler from [`GuestAction::Signal`] finished. `mask` is `uc_sigmask` as the handler
    /// left it, which becomes the task's mask (`rt_sigreturn`), or `None` when the handler did
    /// not return but unwound past its frame (`siglongjmp`).
    fn signal_return(&self, task: TaskId, mask: Option<u64>) {
        let _ = (task, mask);
    }

    /// The image `task`'s pending [`GuestAction::Exec`] runs, which `execve` loaded.
    fn take_image(&self, task: TaskId) -> Option<Image>;

    /// Before the image of a [`GuestAction::Exec`] starts, once the instance left the old one:
    /// waits until the `execve` committed and `task` is the only task left in its thread group,
    /// as `de_thread` does. `Err(status)` when the group exited meanwhile, and the image must
    /// not start.
    fn exec_wait(&self, task: TaskId) -> LocalFuture<'_, Result<(), i32>> {
        let _ = task;
        Box::pin(async { Ok(()) })
    }

    /// Before the last task of a process is released: waits until the other tasks of its thread
    /// group are gone, as a zombie leader stays until its group is empty, so whoever waits for
    /// the process never sees it end while a thread still runs.
    fn group_wait(&self, task: TaskId) -> LocalFuture<'_, ()> {
        let _ = task;
        Box::pin(async {})
    }

    /// `task`'s instance trapped and cannot go on: the hardware fault `signal` (SIGSEGV, SIGILL,
    /// SIGFPE, SIGBUS) with no way to run a handler, so its default action kills the thread
    /// group (`force_sig_fault`, then `get_signal`). Leaves the group's exit pending on `task`.
    /// False when the group was exiting already: the trap came of tearing a thread down and
    /// changes nothing.
    fn fault(&self, task: TaskId, signal: i32) -> bool {
        let _ = (task, signal);
        true
    }

    /// The execution layer is done with `task`: its instance ended, or the vfork child stopped
    /// using its parent's. Nothing is asked about the task after this. For a task of a process
    /// its parent reaped already, this is `put_task_struct`: the reap took the task out of
    /// the kernel (`release_task`), and until this call it only answers the calls its
    /// instance makes on its way out ([`Control::take_pending`] gives its exit, work flags and
    /// CPU accounts stay); every other call on it does nothing.
    fn release(&self, task: TaskId) {
        let _ = task;
    }

    /// The thread `task` from a [`GuestAction::StartThread`] could not be started: the kernel
    /// forgets it, as if `thread-spawn` had failed.
    fn abort_thread(&self, task: TaskId) {
        let _ = task;
    }
}

/// The kernel's part of a task's end that needs the task's memory.
pub trait ExitMm {
    /// The task of `caller` is done with its memory, which is still there: it unwinds through
    /// a [`GuestAction::Exit`], or its instance trapped. The kernel releases what the task
    /// leaves in the memory (its robust and PI futexes, `clear_child_tid`) and, when no other
    /// task uses the memory any more, writes back its shared mappings: Linux's `exit_mm`, which
    /// every task runs in its own `do_exit`, whatever ended it (its `exit`, another thread's
    /// `exit_group` or `execve`, a fatal signal). Calls after the first do nothing.
    fn exit_mm<'a>(&'a self, caller: &'a Caller<'a>) -> LocalFuture<'a, ()>;
}

/// How tasks name each other: a [`TaskId`] is the kernel's own name for a task, which no guest
/// sees; a guest sees a pid, which depends on the pid namespace it is in. The defaults are for
/// a kernel whose pids are the ids.
pub trait PidView {
    /// The thread group `task` belongs to, by its leader's id: the process, whatever pid any
    /// guest sees for it.
    fn tgid(&self, task: TaskId) -> TaskId {
        task
    }

    /// The pid `task` has as `viewer` sees it, in the pid namespace `viewer`'s process is in
    /// (`task_pid_nr_ns(task, task_active_pid_ns(viewer))`): 0 for a task outside it, or gone.
    fn pid_in(&self, viewer: TaskId, task: TaskId) -> i32 {
        let _ = viewer;
        task.0 as i32
    }
}

/// The kernel's part of job control that the execution layer waits in.
pub trait GroupStop {
    /// After a [`GuestAction::Stopped`]: waits while `task` stays stopped (`TASK_STOPPED`), until
    /// SIGCONT continues its thread group or SIGKILL, an exit or an `execve` of the group ends
    /// the stop.
    fn stop_wait(&self, task: TaskId) -> LocalFuture<'_, ()>;
}

/// The kernel's part of a vfork whose child has memory of its own (`clone` with
/// `CLONE_VFORK` but not `CLONE_VM`) that the execution layer waits in.
pub trait VforkWait {
    /// After a [`GuestAction::Fork`] with `vfork`, once the child runs and before the parent's
    /// clone returns: waits until the child exits or its `execve` replaced its memory
    /// (`wait_for_vfork_done`). A fatal signal ends the wait, and the parent's next return to
    /// the guest takes it.
    fn vfork_wait(&self, task: TaskId) -> LocalFuture<'_, ()>;
}

/// A range of a process's memory that shows a shared object ([`UserMemory::map_shared`]).
///
/// [`UserMemory::map_shared`]: crate::UserMemory::map_shared
#[derive(Clone)]
pub struct SharedMapping {
    pub addr: u32,
    pub len: u64,
    pub pages: Arc<dyn SharedPages>,
    /// Where in the object the range starts.
    pub offset: u64,
    /// Whether the range may be written; a read-only view otherwise
    /// ([`UserMemory::map_shared`]).
    ///
    /// [`UserMemory::map_shared`]: crate::UserMemory::map_shared
    pub writable: bool,
}

impl std::fmt::Debug for SharedMapping {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SharedMapping")
            .field("addr", &self.addr)
            .field("len", &self.len)
            .field("object", &self.pages.id())
            .field("offset", &self.offset)
            .field("writable", &self.writable)
            .finish()
    }
}

/// The kernel's part of fork that the execution layer needs besides the memory: a fork child's
/// shared mappings show the same objects as its parent's (`dup_mmap` keeps `VM_SHARED` areas on
/// their file), so the child's copy of the memory shows them again in the same ranges; and its
/// `MADV_WIPEONFORK` ranges start out zeroed (`dup_mmap` copies no pages of a `VM_WIPEONFORK`
/// area).
pub trait SharedMappings {
    /// The ranges of `task`'s memory that show shared objects, lowest first. For a fork child,
    /// asked before it runs: the execution layer maps each over the child's copy of the memory
    /// with [`UserMemory::map_shared`], and leaves them out of the copy.
    ///
    /// [`UserMemory::map_shared`]: crate::UserMemory::map_shared
    fn shared_mappings(&self, task: TaskId) -> Vec<SharedMapping>;

    /// The ranges of `task`'s memory that a fork child gets zeroed (`VM_WIPEONFORK`), lowest
    /// first and apart from the shared mappings. For a fork child, asked before it runs: the
    /// execution layer leaves them out of the child's copy of the memory.
    fn wipe_on_fork(&self, task: TaskId) -> Vec<Range<u64>>;

    /// What the kernel's copies may do with the pages of `task`'s address space, which the
    /// kernel keeps up to date: the execution layer checks every copy of an instance against it
    /// ([`UserMemory::read`]), asking when the instance starts as `task`, a fork child or a new
    /// image included. `None` leaves every page readable and writable.
    ///
    /// [`UserMemory::read`]: crate::UserMemory::read
    fn page_access(&self, task: TaskId) -> Option<Arc<PageAccess>> {
        let _ = task;
        None
    }

    /// The memory of the instance that starts as `task` (a process, a fork child, a new
    /// image), as other tasks reach it from any thread: the kernel's hold on the address space
    /// for `access_process_vm` (ptrace's `PEEKDATA`, `process_vm_readv`, `/proc/<pid>/mem`),
    /// which it lets go when the last task leaves the memory. Its copies check the address
    /// space's [`PageAccess`] unless forced. Only memory the execution layer can reach from
    /// another thread is handed over (a shared memory); without it, the address space has no
    /// pages for anyone else.
    ///
    /// [`PageAccess`]: crate::PageAccess
    fn attach_memory(&self, task: TaskId, memory: Arc<dyn crate::UserMemory + Send + Sync>) {
        let _ = (task, memory);
    }
}

/// The word a wasm `memory.atomic.wait32` or `memory.atomic.wait64` expects at its address.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AtomicWord {
    U32(u32),
    U64(u64),
}

/// Wasm's `memory.atomic.wait` and `memory.atomic.notify` instructions on the kernel's futexes.
/// The execution layer turns the instructions into calls of these, so a guest blocked in one takes
/// signals and its group's exit as one blocked in `futex` does, and the instructions and the
/// syscall wake each other's waiters on the same address.
pub trait AtomicWait {
    /// `memory.atomic.wait32` or `wait64` of the calling task at `addr`, which is aligned to the
    /// size of `expected` and in bounds: a `FUTEX_WAIT` that compares a word of that size, for
    /// `timeout` if given. Shared, as the instruction cannot say otherwise: in memory that shows
    /// a shared object it meets the waits and wakes of other processes there. 0 when woken, `EAGAIN` when the word does not hold
    /// `expected`, `ETIMEDOUT`; a signal interrupts it as it does `FUTEX_WAIT`.
    fn atomic_wait<'a>(
        &'a self,
        caller: &'a Caller<'a>,
        addr: u32,
        expected: AtomicWord,
        timeout: Option<Duration>,
    ) -> LocalFuture<'a, SysResult<i32>>;

    /// `memory.atomic.notify` of `task` at `addr`: wakes up to `count` waiters there, of either
    /// instruction or of `futex`, and returns how many it woke. Unlike `FUTEX_WAKE`, a count of
    /// 0 wakes none.
    fn atomic_notify(&self, task: TaskId, addr: u32, count: u32) -> u32;
}

/// The executable formats `execve` can start besides `#!` scripts, which the kernel handles
/// itself: Linux's binary format handlers, as `search_binary_handler` asks them. The kernel asks
/// before the point of no return, so an image nothing runs fails `execve` (with `ENOEXEC`) and
/// leaves the caller's image in place, as an unknown binary does on Linux.
pub trait Binfmt: Send + Sync {
    /// Whether `header`, the first [`BINPRM_BUF_SIZE`] bytes of an executable file (all of it
    /// when it is shorter; Linux's `bprm->buf`), starts an image a handler may start, such as
    /// by its magic. The kernel asks before it reads the rest of the file. The errno `execve`
    /// fails with when it does not, normally `ENOEXEC`.
    fn probe(&self, header: &[u8]) -> Result<(), Errno>;

    /// Whether a handler starts `image`, the whole file, once [`Binfmt::probe`] took its
    /// header: a closer look a handler takes as it loads, such as at a module's imports. Still
    /// before the point of no return. Accepts everything by default.
    fn check(&self, image: &Image) -> Result<(), Errno> {
        let _ = image;
        Ok(())
    }
}

/// `BINPRM_BUF_SIZE`: how much of an executable file the kernel reads to tell its format (and
/// a `#!` line) before it reads the rest.
pub const BINPRM_BUF_SIZE: usize = 256;

/// The contents of the executable file an `execve` loaded, for the execution layer to run.
/// Cheap to clone: the bytes are shared.
#[derive(Clone)]
pub struct Image {
    bytes: Arc<dyn AsRef<[u8]> + Send + Sync>,
    /// Whether `bytes` are the file's contents as its filesystem keeps them, unchanging.
    shared: bool,
}

impl Image {
    /// An image read into memory of its own.
    pub fn copied(bytes: Vec<u8>) -> Self {
        Self {
            bytes: Arc::new(bytes),
            shared: false,
        }
    }

    /// An image whose bytes its filesystem shares and never changes while anyone holds them,
    /// such as a file of a package volume.
    pub fn shared(bytes: Arc<dyn AsRef<[u8]> + Send + Sync>) -> Self {
        Self {
            bytes,
            shared: true,
        }
    }

    pub fn bytes(&self) -> &[u8] {
        (*self.bytes).as_ref()
    }

    /// What tells these contents apart from any others without reading them, for a cache of
    /// what was made of them: for shared bytes, their address and length, which name the same
    /// bytes for as long as someone holds [`Image::keep_alive`]. `None` for a copy, which only
    /// its bytes identify.
    pub fn identity(&self) -> Option<ImageId> {
        let bytes = self.bytes();
        self.shared.then_some(ImageId {
            addr: bytes.as_ptr() as usize,
            len: bytes.len(),
        })
    }

    /// Holds the bytes, and so keeps [`Image::identity`] theirs alone.
    pub fn keep_alive(&self) -> Arc<dyn AsRef<[u8]> + Send + Sync> {
        self.bytes.clone()
    }
}

impl std::fmt::Debug for Image {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Image")
            .field("len", &self.bytes().len())
            .field("shared", &self.shared)
            .finish()
    }
}

/// The identity of an [`Image`]'s shared bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ImageId {
    addr: usize,
    len: usize,
}

// By hand rather than generated with the rest of `Layered`: the layers see syscalls, and this
// is not one.
impl<L, S: ExitMm> ExitMm for Layered<L, S> {
    fn exit_mm<'a>(&'a self, caller: &'a Caller<'a>) -> LocalFuture<'a, ()> {
        self.inner.exit_mm(caller)
    }
}

// By hand rather than generated with the rest of `Layered`: the layers see syscalls, and this
// is not one.
impl<L, S: PidView> PidView for Layered<L, S> {
    fn tgid(&self, task: TaskId) -> TaskId {
        self.inner.tgid(task)
    }

    fn pid_in(&self, viewer: TaskId, task: TaskId) -> i32 {
        self.inner.pid_in(viewer, task)
    }
}

// By hand rather than generated with the rest of `Layered`: the layers see syscalls, and this
// is not one.
impl<L, S: VforkWait> VforkWait for Layered<L, S> {
    fn vfork_wait(&self, task: TaskId) -> LocalFuture<'_, ()> {
        self.inner.vfork_wait(task)
    }
}

// By hand rather than generated with the rest of `Layered`: the layers see syscalls, and this
// is not one.
impl<L, S: GroupStop> GroupStop for Layered<L, S> {
    fn stop_wait(&self, task: TaskId) -> LocalFuture<'_, ()> {
        self.inner.stop_wait(task)
    }
}

// By hand rather than generated with the rest of `Layered`: the layers see syscalls, and this
// is not one.
impl<L, S: SharedMappings> SharedMappings for Layered<L, S> {
    fn shared_mappings(&self, task: TaskId) -> Vec<SharedMapping> {
        self.inner.shared_mappings(task)
    }

    fn wipe_on_fork(&self, task: TaskId) -> Vec<Range<u64>> {
        self.inner.wipe_on_fork(task)
    }

    fn page_access(&self, task: TaskId) -> Option<Arc<PageAccess>> {
        self.inner.page_access(task)
    }

    fn attach_memory(&self, task: TaskId, memory: Arc<dyn crate::UserMemory + Send + Sync>) {
        self.inner.attach_memory(task, memory);
    }
}

// By hand rather than generated with the rest of `Layered`: the layers see syscalls, and these
// are instructions.
impl<L, S: AtomicWait> AtomicWait for Layered<L, S> {
    fn atomic_wait<'a>(
        &'a self,
        caller: &'a Caller<'a>,
        addr: u32,
        expected: AtomicWord,
        timeout: Option<Duration>,
    ) -> LocalFuture<'a, SysResult<i32>> {
        self.inner.atomic_wait(caller, addr, expected, timeout)
    }

    fn atomic_notify(&self, task: TaskId, addr: u32, count: u32) -> u32 {
        self.inner.atomic_notify(task, addr, count)
    }
}
