//! Processes, after Linux's `kernel/fork.c`, `kernel/exit.c`, `fs/exec.c` and the process-group
//! half of `kernel/sys.c`: the process tree, `clone` for new processes (fork and vfork),
//! `execve`, zombies and the `wait` family, reparenting, SIGCHLD, process groups, sessions and
//! pidfds.
//!
//! vfork runs the child in its parent's instance: `clone` asks the execution layer to switch
//! the instance to the child ([`GuestAction::SwitchTask`]), and the child's exit or `execve`
//! switches it back, as the kernel tracks in [`Tree::vforks`]. fork asks the execution layer to
//! copy the caller's memory and stack into a new instance for the child
//! ([`GuestAction::Fork`]).

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use wasixv2_abi::{
    AtFlags, CloneFlags, ConstSiginfoPtr, Cstr, CstrArray, GuestLayout, GuestPtr, GuestPtrMut,
    Idtype, OpenFlags, Pid, Rusage, RusagePtr, S32Ptr, SiCode, Siginfo, SiginfoPtr, Signal, Ulong,
    Uptr, WaitOptions,
};
use wasixv2_syscalls::{
    Caller, CstrError, Errno, Fd, GuestAction, LocalFuture, SysErr, SysResult, TaskId, WorkFlag,
    read_cstr, read_value, write_value,
};
use wasixv2_vfs::{FileType, LookupFlags, MAY_EXEC, OpenHow};
use wasixv2_wait::{PollMask, PollTable, Pollable, WaitQueue, poll_wait, wait_event};

use crate::{
    Kernel, Process, Task,
    file::{Backing, FdTable, FileObject, Io, OpenFile},
    ident::Comm,
    signal::{Signals, user_info},
    thread,
    work::Work,
};

/// `MAX_ARG_STRLEN`: the longest single argument or environment string.
const MAX_ARG_STRLEN: usize = 32 * 4096;
/// `ARG_MAX` as Linux computes it from the default 8 MiB stack limit: a quarter of it.
const ARG_MAX: usize = 2 * 1024 * 1024;
/// `MAX_ARG_STRINGS`.
const MAX_ARG_STRINGS: usize = 0x7FFF_FFFF;
/// `BINPRM_BUF_SIZE`: how much of a `#!` line the kernel looks at.
const BINPRM_BUF_SIZE: usize = 256;
/// How deep interpreters may nest (`#!` naming another script), as `search_binary_handler`.
const MAX_INTERP_DEPTH: usize = 4;
const WASM_MAGIC: &[u8] = b"\0asm";

/// `__WCLONE`: wait only for children that do not report with SIGCHLD.
const WCLONE: u32 = 0x8000_0000;
const WAIT_KNOWN: u32 = WaitOptions::WNOHANG.0
    | WaitOptions::WSTOPPED.0
    | WaitOptions::WEXITED.0
    | WaitOptions::WCONTINUED.0
    | WaitOptions::WNOWAIT.0
    | WaitOptions::__WNOTHREAD.0
    | WaitOptions::__WALL.0
    | WCLONE;

/// `PIDFD_NONBLOCK` (`O_NONBLOCK`).
const PIDFD_NONBLOCK: u32 = OpenFlags::O_NONBLOCK.0;

#[derive(Default)]
pub(crate) struct Procs {
    pub(crate) tree: Arc<Mutex<Tree>>,
    /// Woken whenever a process exits; waiters and pidfd pollers recheck (`wait_chldexit`).
    changed: Arc<WaitQueue>,
}

#[derive(Default)]
pub(crate) struct Tree {
    pub(crate) nodes: HashMap<u32, Node>,
    /// vfork children running in their parent's instance, to the parent task that waits.
    vforks: HashMap<TaskId, TaskId>,
    /// Images an `execve` loaded, until the execution layer takes them.
    images: HashMap<TaskId, Vec<u8>>,
}

pub(crate) struct Node {
    pub(crate) process: Arc<Process>,
    /// 0 for none: the first process, or an orphan with nobody to reap it.
    pub(crate) parent: u32,
    /// The signal the parent gets when the process exits (`CSIGNAL`).
    pub(crate) exit_signal: i32,
    /// For `setpgid`'s `EACCES`.
    did_exec: bool,
    /// The signal whose default action killed the process.
    killed_by: Option<i32>,
    /// Set once the process exited; it is a zombie until reaped.
    exit: Option<Exit>,
}

impl Node {
    /// A zombie: exited, not reaped.
    pub(crate) fn is_zombie(&self) -> bool {
        self.exit.is_some()
    }
}

/// The file `execve` runs: where it is, and what of it decides the new image's credentials.
struct Binary {
    path: wasixv2_vfs::Path,
    mode: u32,
    owner: (u32, u32),
    /// On a `nosuid` mount, whose set-user-ID and set-group-ID bits do not apply.
    nosuid: bool,
}

/// How a process ended, as `waitid` reports it.
#[derive(Clone, Copy)]
struct Exit {
    code: SiCode,
    status: i32,
    /// The process's real uid when it exited.
    uid: u32,
}

impl Procs {
    /// Enters `process` into the tree under `parent` (0 for none).
    pub(crate) fn add(&self, process: Arc<Process>, parent: u32, exit_signal: i32) {
        let pid = process.pid.0;
        self.tree.lock().unwrap().nodes.insert(
            pid,
            Node {
                process,
                parent,
                exit_signal,
                did_exec: false,
                killed_by: None,
                exit: None,
            },
        );
    }

    /// The process `pid`, zombies included.
    pub(crate) fn find(&self, pid: u32) -> Option<Arc<Process>> {
        let tree = self.tree.lock().unwrap();
        tree.nodes.get(&pid).map(|node| node.process.clone())
    }

    pub(crate) fn take_image(&self, task: TaskId) -> Option<Vec<u8>> {
        self.tree.lock().unwrap().images.remove(&task)
    }
}

impl Tree {
    fn children(&self, pid: u32) -> impl Iterator<Item = (&u32, &Node)> {
        self.nodes
            .iter()
            .filter(move |(_, node)| node.parent == pid)
    }

    /// `find_new_reaper`: the nearest living ancestor that set `PR_SET_CHILD_SUBREAPER`, else
    /// the first process, else nobody.
    fn reaper(&self, dying: u32) -> u32 {
        let mut at = self.nodes.get(&dying).map_or(0, |node| node.parent);
        while let Some(node) = self.nodes.get(&at) {
            if node.exit.is_none() && node.process.ident.lock().unwrap().child_subreaper() {
                return at;
            }
            at = node.parent;
        }
        match self.nodes.get(&1) {
            Some(init) if dying != 1 && init.exit.is_none() => 1,
            _ => 0,
        }
    }
}

/// The `siginfo` of SIGCHLD and `waitid`: `si_pid`, `si_uid` (the child's real uid),
/// `si_status`.
fn child_info(signo: i32, pid: u32, exit: Exit) -> Siginfo {
    let mut info = Siginfo {
        si_signo: signo,
        si_errno: 0,
        si_code: exit.code.0,
        sifields: [0; 116],
    };
    info.sifields[0..4].copy_from_slice(&(pid as i32).to_le_bytes());
    info.sifields[4..8].copy_from_slice(&exit.uid.to_le_bytes());
    info.sifields[8..12].copy_from_slice(&exit.status.to_le_bytes());
    info
}

/// Which children a `waitid` asks about.
#[derive(Clone, Copy)]
enum Which {
    All,
    Pid(u32),
    Pgid(u32),
}

impl Kernel {
    pub(crate) fn sys_getppid(&self, caller: &Caller<'_>) -> Pid {
        let pid = self.process(caller.task()).pid.0;
        let tree = self.procs.tree.lock().unwrap();
        tree.nodes.get(&pid).map_or(0, |node| node.parent as Pid)
    }

    /// Whether `task` waits for a vfork child that runs in its instance.
    pub(crate) fn vfork_waits(&self, task: TaskId) -> bool {
        let tree = self.procs.tree.lock().unwrap();
        tree.vforks.values().any(|&parent| parent == task)
    }

    /// The signal state of the thread group led by `pid`.
    fn group_signals(&self, pid: u32) -> Option<Arc<Signals>> {
        let state = self.state.lock().unwrap();
        let task = state.tasks.get(&TaskId(pid))?;
        Some(task.signals.group().clone())
    }

    /// Records that a signal's default action is killing the process `task` belongs to, so its
    /// parent sees `WIFSIGNALED`.
    pub(crate) fn note_killed(&self, task: TaskId, sig: i32) {
        let pid = self.process(task).pid.0;
        let mut tree = self.procs.tree.lock().unwrap();
        if let Some(node) = tree.nodes.get_mut(&pid)
            && node.exit.is_none()
        {
            node.killed_by.get_or_insert(sig);
        }
    }

    /// `exit_notify`, once per process, when its thread group's exit is decided: makes it a
    /// zombie, hands its children to a reaper, tells the parent, and releases a vfork parent.
    pub(crate) fn process_exited(&self, process: &Arc<Process>, task: TaskId, status: i32) {
        let pid = process.pid.0;
        let parent = {
            let tree = self.procs.tree.lock().unwrap();
            match tree.nodes.get(&pid) {
                Some(node) if node.exit.is_none() => node.parent,
                _ => return,
            }
        };
        self.exit_files(process);
        // `do_notify_parent`: how the parent treats SIGCHLD decides whether a zombie stays.
        let parent_signals = (parent != 0).then(|| self.group_signals(parent)).flatten();
        let (ignored, nocldwait) = parent_signals
            .as_ref()
            .map_or((true, true), |signals| signals.child_reaping());
        let (exit, exit_signal, vfork_parent, orphans) = {
            let mut tree = self.procs.tree.lock().unwrap();
            let reaper = tree.reaper(pid);
            let Some(node) = tree.nodes.get_mut(&pid) else {
                return;
            };
            if node.exit.is_some() {
                return;
            }
            let uid = process.cred().user.real;
            let exit = match node.killed_by {
                Some(sig) => Exit {
                    code: SiCode::CLD_KILLED,
                    status: sig,
                    uid,
                },
                None => Exit {
                    code: SiCode::CLD_EXITED,
                    status: status & 0xff,
                    uid,
                },
            };
            node.exit = Some(exit);
            let exit_signal = node.exit_signal;
            let autoreap = parent_signals.is_none()
                || (exit_signal == Signal::SIGCHLD.0 && (ignored || nocldwait));
            if autoreap {
                tree.nodes.remove(&pid);
            }
            // `forget_original_parent`: the children go to the reaper, and those that asked for
            // it hear of their parent's death.
            let children: Vec<u32> = tree.children(pid).map(|(&child, _)| child).collect();
            let mut orphans = Vec::new();
            for child in children {
                let node = tree.nodes.get_mut(&child).expect("listed above");
                node.parent = reaper;
                let pdeathsig = node.process.ident.lock().unwrap().pdeathsig();
                match node.exit {
                    Some(_) if reaper == 0 => drop(tree.nodes.remove(&child)),
                    None if pdeathsig != 0 => orphans.push((child, pdeathsig)),
                    _ => {}
                }
            }
            (exit, exit_signal, tree.vforks.remove(&task), orphans)
        };
        // `group_send_sig_info(pdeath_signal, SEND_SIG_NOINFO)`: from the parent, as `kill`.
        for (child, sig) in orphans {
            if let Some(signals) = self.group_signals(child) {
                let info = user_info(sig, SiCode::SI_USER, pid as Pid, exit.uid);
                let _ = signals.send_group(info);
            }
        }
        if let Some(parent_task) = vfork_parent {
            let action = GuestAction::VforkReturn {
                parent: parent_task,
                child: task,
            };
            self.queue(task, action);
        }
        if let Some(signals) = parent_signals
            && exit_signal != 0
            && !ignored
        {
            // A realtime exit signal its parent has no room for is lost.
            let _ = signals.send_group(child_info(exit_signal, pid, exit));
        }
        self.procs.changed.wake(PollMask::empty());
    }

    /// `exit_files`: the process's tasks let go of their descriptor tables, closing every file
    /// no one else holds.
    fn exit_files(&self, process: &Arc<Process>) {
        let tables: Vec<Arc<Mutex<FdTable>>> = {
            let mut state = self.state.lock().unwrap();
            state
                .tasks
                .values_mut()
                .filter(|t| Arc::ptr_eq(&t.process, process))
                .map(|t| std::mem::take(&mut t.files))
                .collect()
        };
        let memory = NoMemory;
        let caller = Caller::new(process.pid, &memory);
        for table in tables {
            let Ok(table) = Arc::try_unwrap(table) else {
                continue;
            };
            let mut table = table.into_inner().unwrap();
            for fd in (0..table.end()).map(|fd| Fd(fd as i32)) {
                if let Some(file) = table.remove(fd) {
                    self.closed(&caller, &file);
                }
            }
        }
    }

    // ---- clone ----

    /// `clone` for a new process. vfork (`CLONE_VM | CLONE_VFORK`) works in every build
    /// configuration; fork (no `CLONE_VM`) needs an address space that can be duplicated, and
    /// gets `ENOSYS` otherwise, as there is no fork without an MMU on Linux.
    pub(crate) async fn sys_clone(
        &self,
        caller: &Caller<'_>,
        flags: Ulong,
        newsp: Uptr,
        parent_tid: S32Ptr,
        tls: Uptr,
        child_tid: S32Ptr,
    ) -> SysResult<i32> {
        let flags = CloneFlags(flags.into());
        let exit_signal = (flags.0 & CloneFlags::CSIGNAL.0) as i32;
        // copy_process's checks, in its order.
        if flags.contains(CloneFlags::CLONE_THREAD) && !flags.contains(CloneFlags::CLONE_SIGHAND) {
            return Err(Errno::EINVAL.into());
        }
        if flags.contains(CloneFlags::CLONE_SIGHAND) && !flags.contains(CloneFlags::CLONE_VM) {
            return Err(Errno::EINVAL.into());
        }
        if exit_signal > 64 {
            return Err(Errno::EINVAL.into());
        }
        let namespaces = CloneFlags::CLONE_NEWNS.0
            | CloneFlags::CLONE_NEWCGROUP.0
            | CloneFlags::CLONE_NEWUTS.0
            | CloneFlags::CLONE_NEWIPC.0
            | CloneFlags::CLONE_NEWUSER.0
            | CloneFlags::CLONE_NEWPID.0
            | CloneFlags::CLONE_NEWNET.0
            | CloneFlags::CLONE_NEWTIME.0;
        if flags.0 & namespaces != 0 {
            return Err(Errno::EPERM.into());
        }
        // Threads start through `thread-spawn`; a process sharing handlers is not supported.
        if flags.contains(CloneFlags::CLONE_THREAD) || flags.contains(CloneFlags::CLONE_SIGHAND) {
            return Err(Errno::EINVAL.into());
        }
        // No pidfd comes out of clone yet (`pidfd_open` gives one): failing beats a pidfd
        // never written to `parent_tid`.
        if flags.contains(CloneFlags::CLONE_PIDFD) {
            return Err(Errno::EINVAL.into());
        }
        if !flags.contains(CloneFlags::CLONE_VM) {
            return self
                .fork(
                    caller,
                    flags,
                    exit_signal,
                    newsp,
                    parent_tid,
                    tls,
                    child_tid,
                )
                .await;
        }
        // Only vfork can run the child: it takes over the caller's instance.
        if !flags.contains(CloneFlags::CLONE_VFORK) || newsp != 0 {
            return Err(Errno::EINVAL.into());
        }
        let parent_task = caller.task();
        let child = self.copy_process(parent_task, flags, exit_signal, tls, child_tid.addr())?;
        let tid = child.0 as i32;
        // Linux ignores faults on both writes.
        if flags.contains(CloneFlags::CLONE_PARENT_SETTID) {
            let _ = write_value(caller.memory(), GuestPtrMut::new(parent_tid.addr()), &tid).await;
        }
        if flags.contains(CloneFlags::CLONE_CHILD_SETTID) {
            let _ = write_value(caller.memory(), GuestPtrMut::new(child_tid.addr()), &tid).await;
        }
        self.procs
            .tree
            .lock()
            .unwrap()
            .vforks
            .insert(child, parent_task);
        let switch = GuestAction::SwitchTask {
            task: child,
            ret: 0,
        };
        self.queue(parent_task, switch);
        Ok(0)
    }

    /// fork: `clone` without `CLONE_VM`. The child gets a copy of the address space, which the
    /// execution layer makes once the call returns ([`GuestAction::Fork`]); clone's writes to
    /// guest memory wait for that copy, so each lands only in the address space Linux writes.
    #[allow(clippy::too_many_arguments)]
    async fn fork(
        &self,
        caller: &Caller<'_>,
        flags: CloneFlags,
        exit_signal: i32,
        newsp: Uptr,
        parent_tid: S32Ptr,
        tls: Uptr,
        child_tid: S32Ptr,
    ) -> SysResult<i32> {
        // The child cannot start on another stack (`newsp`), and nothing can block the parent
        // until a child with its own memory execs (`CLONE_VFORK`).
        if newsp != 0 || flags.contains(CloneFlags::CLONE_VFORK) {
            return Err(Errno::EINVAL.into());
        }
        if !caller.memory().can_duplicate().await {
            return Err(Errno::ENOSYS.into());
        }
        let parent_task = caller.task();
        let child = self.copy_process(parent_task, flags, exit_signal, tls, child_tid.addr())?;
        let set = |flag: CloneFlags, addr: u32| flags.contains(flag).then_some(addr);
        let fork = GuestAction::Fork {
            child,
            parent_tid: set(CloneFlags::CLONE_PARENT_SETTID, parent_tid.addr()),
            child_tid: set(CloneFlags::CLONE_CHILD_SETTID, child_tid.addr()),
            tls: set(CloneFlags::CLONE_SETTLS, tls),
        };
        self.queue(parent_task, fork);
        Ok(child.0 as i32)
    }

    /// `copy_process` for a new thread group: the child gets copies of what `flags` does not
    /// share, the caller's mask, and a place in the tree. Without `CLONE_VM`, that includes the
    /// address space's mappings and break (`dup_mm`).
    fn copy_process(
        &self,
        parent_task: TaskId,
        flags: CloneFlags,
        exit_signal: i32,
        tls: u32,
        child_tid: u32,
    ) -> Result<TaskId, Errno> {
        let (id, process, parent_pid) = {
            let mut state = self.state.lock().unwrap();
            let task = &state.tasks[&parent_task];
            let parent = task.process.clone();
            if crate::resource::nproc_exceeded(&state, &parent) {
                return Err(Errno::EAGAIN);
            }
            if parent.group.exiting() {
                return Err(Errno::EINTR);
            }
            let sched = task.sched.for_child()?;
            let id = TaskId(state.last_id + 1);
            let process = Arc::new(Process {
                pid: id,
                args: Mutex::new(parent.args.lock().unwrap().clone()),
                env: Mutex::new(parent.env.lock().unwrap().clone()),
                exe: Mutex::new(parent.exe.lock().unwrap().clone()),
                group: thread::Group::new(),
                start: self.host.monotonic(),
                ident: Mutex::new(parent.ident.lock().unwrap().for_child()),
                cred: Mutex::new(parent.cred()),
                resources: Mutex::new(parent.resources.lock().unwrap().clone()),
            });
            let files = match flags.contains(CloneFlags::CLONE_FILES) {
                true => task.files.clone(),
                false => Arc::new(Mutex::new(copy_files(&task.files.lock().unwrap()))),
            };
            let fs = match flags.contains(CloneFlags::CLONE_FS) {
                true => task.fs.clone(),
                false => Arc::new(Mutex::new(task.fs.lock().unwrap().clone())),
            };
            let flag = Arc::new(WorkFlag::default());
            let account = crate::signal::Account::new(self.sigqueue_users.clone(), &process);
            let signals = task.signals.group().fork(account).attach(
                id,
                task.signals.inherit(flags),
                flag.clone(),
            );
            // A vfork child runs on its parent's memory, so it shares the address space; a fork
            // child gets a copy of its mappings and break over the memory the execution layer
            // copies.
            let mm = match flags.contains(CloneFlags::CLONE_VM) {
                true => task.mm.clone(),
                false => Arc::new(task.mm.dup(id.0)),
            };
            let new = Task {
                process: process.clone(),
                mm,
                files,
                fs,
                work: Work::new(flag),
                restart: None,
                comm: task.comm,
                sched,
                thread: task.thread.for_process(flags, tls, child_tid),
                signals,
            };
            state.last_id += 1;
            state.tasks.insert(id, new);
            (id, process, parent.pid.0)
        };
        let parent = match flags.contains(CloneFlags::CLONE_PARENT) {
            true => {
                let tree = self.procs.tree.lock().unwrap();
                tree.nodes.get(&parent_pid).map_or(0, |node| node.parent)
            }
            false => parent_pid,
        };
        self.procs.add(process, parent, exit_signal);
        Ok(id)
    }

    // ---- execve ----

    /// `do_execveat_common`: loads the file, then, past the point of no return, replaces the
    /// image. The execution layer starts the new image on the way back to the guest.
    pub(crate) async fn sys_execveat(
        &self,
        caller: &Caller<'_>,
        dfd: Fd,
        filename: Cstr,
        argv: CstrArray,
        envp: CstrArray,
        flags: AtFlags,
    ) -> SysResult<i32> {
        let known = AtFlags::AT_EMPTY_PATH | AtFlags::AT_SYMLINK_NOFOLLOW;
        if !(flags & !known).is_empty() {
            return Err(Errno::EINVAL.into());
        }
        let path = crate::fs::read_path(caller.memory(), filename).await?;
        let mut args = read_strings(caller, argv).await?;
        // Linux 5.18+: an empty argv gets an empty argv[0], so argc is never 0.
        if args.is_empty() {
            args.push(String::new());
        }
        let env = read_strings(caller, envp).await?;
        let total: usize = args.iter().chain(&env).map(|s| s.len() + 1 + 4).sum();
        if total > ARG_MAX {
            return Err(Errno::E2BIG.into());
        }
        // The name the binary is known by: what the caller passed, or `/dev/fd/N` for an fd.
        let name = match (path.is_empty(), path.starts_with(b"/") || dfd.0 < 0) {
            (true, _) => format!("/dev/fd/{}", dfd.0).into_bytes(),
            (false, true) => path.clone(),
            (false, false) => format!("/dev/fd/{}/", dfd.0)
                .into_bytes()
                .into_iter()
                .chain(path.iter().copied())
                .collect(),
        };
        let mut file = (dfd, path, flags);
        let mut depth = 0;
        let (image, binary) = loop {
            let (bytes, binary) = self.open_exec(caller, file.0, &file.1, file.2).await?;
            if bytes.starts_with(WASM_MAGIC) {
                break (bytes, binary);
            }
            let Some((interp, arg)) = parse_shebang(&bytes) else {
                return Err(Errno::ENOEXEC.into());
            };
            depth += 1;
            if depth > MAX_INTERP_DEPTH {
                return Err(Errno::ELOOP.into());
            }
            // `load_script`: argv[0] goes; the interpreter, its argument and the script's
            // name come first.
            let script = match depth {
                1 => String::from_utf8_lossy(&name).into_owned(),
                _ => args.first().cloned().unwrap_or_default(),
            };
            if !args.is_empty() {
                args.remove(0);
            }
            let mut front = vec![String::from_utf8_lossy(&interp).into_owned()];
            front.extend(arg.map(|arg| String::from_utf8_lossy(&arg).into_owned()));
            front.push(script);
            args.splice(0..0, front);
            let cwd = Fd(wasixv2_abi::AtFd::AT_FDCWD.0);
            file = (cwd, interp, AtFlags(0));
        };
        self.mm_release(caller).await;
        self.exec_commit(caller.task(), &name, args, env, image, binary);
        Ok(0)
    }

    /// `do_open_execat`: the file must be a regular file with an execute bit the caller may
    /// use, on a mount that allows execution. Returns its contents and the binary.
    async fn open_exec(
        &self,
        caller: &Caller<'_>,
        dfd: Fd,
        path: &[u8],
        flags: AtFlags,
    ) -> SysResult<(Vec<u8>, Binary)> {
        if path.is_empty() && !flags.contains(AtFlags::AT_EMPTY_PATH) {
            return Err(Errno::ENOENT.into());
        }
        let env = self.env(caller);
        let target = self
            .resolve_at(caller, &env, dfd, path, flags, true)
            .await?;
        let attr = self.vfs.stat(&target).await?;
        match attr.file_type {
            FileType::Regular => {}
            FileType::Symlink => return Err(Errno::ELOOP.into()),
            _ => return Err(Errno::EACCES.into()),
        }
        if target.mnt.flags.noexec {
            return Err(Errno::EACCES.into());
        }
        self.vfs.permission(&env.cred, &target, MAY_EXEC).await?;
        let how = OpenHow {
            flags: OpenFlags::O_RDONLY,
            mode: 0,
            lookup: LookupFlags {
                empty: true,
                ..LookupFlags::default()
            },
        };
        let file = self.vfs.open(&env.cx(), &target, b"", &how).await?;
        let mut bytes = vec![0; attr.size as usize];
        let mut at = 0;
        while at < bytes.len() {
            match file.read_at(&mut bytes[at..], at as u64).await? {
                0 => break,
                n => at += n,
            }
        }
        bytes.truncate(at);
        let binary = Binary {
            mode: attr.mode,
            owner: (attr.uid, attr.gid),
            nosuid: target.mnt.flags.nosuid,
            path: target,
        };
        Ok((bytes, binary))
    }

    /// Past the point of no return (`begin_new_exec`): the other threads die and the image runs
    /// as the group leader (`de_thread`), close-on-exec descriptors close, caught signals reset,
    /// the vfork parent resumes, the new image's arguments, environment and memory replace the
    /// old ones, and the credentials become what `binary` grants.
    fn exec_commit(
        &self,
        caller: TaskId,
        name: &[u8],
        args: Vec<String>,
        env: Vec<String>,
        image: Vec<u8>,
        binary: Binary,
    ) {
        let task = self.de_thread(caller);
        let process = self.process(task);
        let vfork_parent = {
            let mut tree = self.procs.tree.lock().unwrap();
            if let Some(node) = tree.nodes.get_mut(&process.pid.0) {
                node.did_exec = true;
            }
            tree.images.insert(task, image);
            tree.vforks.remove(&task)
        };
        // `unshare_files`, then `do_close_on_exec`. The dying threads of the group do not
        // count as sharing the table.
        let table = {
            let state = self.state.lock().unwrap();
            let files = state.tasks[&task].files.clone();
            let shared = state
                .tasks
                .values()
                .any(|t| !Arc::ptr_eq(&t.process, &process) && Arc::ptr_eq(&t.files, &files));
            (files, shared)
        };
        let table = match table {
            (files, false) => files,
            (files, true) => {
                let copy = Arc::new(Mutex::new(copy_files(&files.lock().unwrap())));
                self.with_task(task, |t| t.files = copy.clone());
                copy
            }
        };
        let closing: Vec<Arc<OpenFile>> = {
            let mut table = table.lock().unwrap();
            let cloexec: Vec<Fd> = (0..table.end())
                .map(|fd| Fd(fd as i32))
                .filter(|&fd| table.cloexec(fd) == Some(true))
                .collect();
            cloexec
                .into_iter()
                .filter_map(|fd| table.remove(fd))
                .collect()
        };
        let memory = NoMemory;
        let caller = Caller::new(task, &memory);
        for file in closing {
            self.closed(&caller, &file);
        }
        // `exit_itimers` and `flush_itimer_signals`: POSIX timers belong to the old image;
        // interval timers stay.
        let signals = self.task_signals(task);
        crate::posix_timer::exit_itimers(signals.group());
        signals.group().flush_timer_signals();
        signals.group().exec_reset();
        *process.args.lock().unwrap() = args;
        *process.env.lock().unwrap() = env;
        let no_new_privs = process.ident.lock().unwrap().no_new_privs;
        let old = process.cred();
        let (cred, secure) = old.exec(
            binary.mode,
            binary.owner,
            !binary.nosuid && !no_new_privs,
            no_new_privs,
        );
        let dumpable =
            cred.user.effective == cred.user.real && cred.group.effective == cred.group.real;
        let privileged = secure || cred.drops_dumpable(&old);
        *process.cred.lock().unwrap() = cred;
        *process.exe.lock().unwrap() = Some(binary.path);
        {
            let mut ident = process.ident.lock().unwrap();
            ident.secure_exec = secure;
            ident.dumpable = dumpable;
            if privileged {
                ident.clear_pdeathsig();
            }
        }
        self.with_task(task, |t| {
            t.comm = Comm::from_path(name);
            // `exec_mmap`: the new image gets its own address space. A vfork child leaves its
            // parent's untouched.
            t.mm = Arc::default();
            t.thread = thread::ThreadState::default();
        });
        self.hand_image(task, vfork_parent);
    }

    // ---- wait ----

    /// `kernel_waitid`.
    pub(crate) async fn sys_waitid(
        &self,
        caller: &Caller<'_>,
        which: Idtype,
        upid: Pid,
        infop: SiginfoPtr,
        options: WaitOptions,
        ru: RusagePtr,
    ) -> SysResult<i32> {
        let options = options.0;
        if options & !WAIT_KNOWN != 0 {
            return Err(Errno::EINVAL.into());
        }
        let wanted = WaitOptions::WEXITED.0 | WaitOptions::WSTOPPED.0 | WaitOptions::WCONTINUED.0;
        if options & wanted == 0 {
            return Err(Errno::EINVAL.into());
        }
        let me = self.process(caller.task());
        let which = match which {
            Idtype::P_ALL => Which::All,
            Idtype::P_PID if upid > 0 => Which::Pid(upid as u32),
            Idtype::P_PGID if upid == 0 => Which::Pgid(me.ident.lock().unwrap().pgid),
            Idtype::P_PGID if upid > 0 => Which::Pgid(upid as u32),
            Idtype::P_PIDFD => Which::Pid(self.pidfd_pid(caller, Fd(upid))?),
            _ => return Err(Errno::EINVAL.into()),
        };
        let interrupt = self.interrupt(caller);
        let found = wait_event(
            &self.procs.changed,
            || self.wait_once(me.pid.0, which, options),
            &*interrupt,
        )
        .await
        .map_err(|_| SysErr::Restart)??;
        let info = match found {
            Some((pid, exit)) => child_info(Signal::SIGCHLD.0, pid, exit),
            None => Siginfo {
                si_signo: 0,
                si_errno: 0,
                si_code: 0,
                sifields: [0; 116],
            },
        };
        if !ru.is_null() {
            let zero = vec![0; Rusage::SIZE];
            caller.memory().write(ru.addr(), &zero).await?;
        }
        if !infop.is_null() {
            write_value(caller.memory(), infop, &info).await?;
        }
        Ok(0)
    }

    /// One pass of `do_wait`: a reapable child, `Ok(None)` for `WNOHANG` when none is, `ECHILD`
    /// without matching children, and `None` to keep waiting.
    #[allow(clippy::type_complexity)]
    fn wait_once(
        &self,
        me: u32,
        which: Which,
        options: u32,
    ) -> Option<Result<Option<(u32, Exit)>, Errno>> {
        let mut tree = self.procs.tree.lock().unwrap();
        let mut any = false;
        let mut ready = None;
        for (&pid, node) in tree.children(me) {
            let matches = match which {
                Which::All => true,
                Which::Pid(want) => pid == want,
                Which::Pgid(pgid) => node.process.ident.lock().unwrap().pgid == pgid,
            };
            // `eligible_child`: `__WCLONE` picks the children that report with another
            // signal than SIGCHLD; `__WALL` takes both.
            let clone_child = node.exit_signal != Signal::SIGCHLD.0;
            let kind =
                options & WaitOptions::__WALL.0 != 0 || clone_child == (options & WCLONE != 0);
            if !matches || !kind {
                continue;
            }
            any = true;
            if let Some(exit) = node.exit
                && options & WaitOptions::WEXITED.0 != 0
            {
                ready = Some((pid, exit));
                break;
            }
        }
        if let Some((pid, _)) = ready
            && options & WaitOptions::WNOWAIT.0 == 0
        {
            tree.nodes.remove(&pid);
        }
        match (ready, any) {
            (Some(found), _) => Some(Ok(Some(found))),
            (None, false) => Some(Err(Errno::ECHILD)),
            (None, true) if options & WaitOptions::WNOHANG.0 != 0 => Some(Ok(None)),
            (None, true) => None,
        }
    }

    // ---- process groups and sessions ----

    pub(crate) fn pgid_of(&self, pid: Pid) -> Pid {
        match self.procs.find(pid as u32) {
            Some(process) => process.ident.lock().unwrap().pgid as Pid,
            None => pid,
        }
    }

    /// The thread groups in process group `pgid` that have not exited.
    pub(crate) fn pgrp_groups(&self, pgid: Pid) -> Vec<(Pid, Arc<Signals>)> {
        let members: Vec<u32> = {
            let tree = self.procs.tree.lock().unwrap();
            tree.nodes
                .iter()
                .filter(|(_, node)| {
                    node.exit.is_none() && node.process.ident.lock().unwrap().pgid as Pid == pgid
                })
                .map(|(&pid, _)| pid)
                .collect()
        };
        members
            .into_iter()
            .filter_map(|pid| Some((pid as Pid, self.group_signals(pid)?)))
            .collect()
    }

    /// `setpgid`: the caller or one of its children that has not run `execve`, in the caller's
    /// session, joins or creates a group there.
    pub(crate) fn sys_setpgid(&self, caller: &Caller<'_>, pid: Pid, pgid: Pid) -> SysResult<i32> {
        if pgid < 0 {
            return Err(Errno::EINVAL.into());
        }
        let me = self.process(caller.task());
        let my_sid = me.ident.lock().unwrap().sid;
        let pid = match pid {
            0 => me.pid.0,
            pid if pid < 0 => return Err(Errno::EINVAL.into()),
            pid => pid as u32,
        };
        let pgid = if pgid == 0 { pid } else { pgid as u32 };
        let tree = self.procs.tree.lock().unwrap();
        let node = tree.nodes.get(&pid).ok_or(Errno::ESRCH)?;
        if pid != me.pid.0 {
            if node.parent != me.pid.0 || node.exit.is_some() {
                return Err(Errno::ESRCH.into());
            }
            if node.process.ident.lock().unwrap().sid != my_sid {
                return Err(Errno::EPERM.into());
            }
            if node.did_exec {
                return Err(Errno::EACCES.into());
            }
        }
        let mut ident = node.process.ident.lock().unwrap();
        if ident.sid == pid {
            return Err(Errno::EPERM.into());
        }
        if pgid != pid {
            let exists = tree.nodes.iter().any(|(&other, n)| {
                n.exit.is_none()
                    && match other == pid {
                        true => ident.pgid == pgid,
                        false => {
                            let id = n.process.ident.lock().unwrap();
                            id.pgid == pgid && id.sid == my_sid
                        }
                    }
            });
            if !exists {
                return Err(Errno::EPERM.into());
            }
        }
        ident.pgid = pgid;
        Ok(0)
    }

    /// `setsid`: fails for a group leader, or when a group with the caller's pid exists.
    pub(crate) fn sys_setsid(&self, caller: &Caller<'_>) -> SysResult<Pid> {
        let process = self.process(caller.task());
        let me = process.pid.0;
        let tree = self.procs.tree.lock().unwrap();
        let taken = tree.nodes.iter().any(|(&pid, node)| {
            pid != me && node.exit.is_none() && node.process.ident.lock().unwrap().pgid == me
        });
        let mut ident = process.ident.lock().unwrap();
        if ident.pgid == me || taken {
            return Err(Errno::EPERM.into());
        }
        ident.pgid = me;
        ident.sid = me;
        Ok(me as Pid)
    }

    // ---- pidfd ----

    pub(crate) fn sys_pidfd_open(
        &self,
        caller: &Caller<'_>,
        pid: Pid,
        flags: u32,
    ) -> SysResult<Fd> {
        if flags & !PIDFD_NONBLOCK != 0 || pid <= 0 {
            return Err(Errno::EINVAL.into());
        }
        let pid = pid as u32;
        {
            let tree = self.procs.tree.lock().unwrap();
            if !tree.nodes.contains_key(&pid) {
                return Err(Errno::ESRCH.into());
            }
        }
        let object = PidFd {
            pid,
            tree: self.procs.tree.clone(),
            changed: self.procs.changed.clone(),
        };
        let mut status = OpenFlags::O_RDWR;
        if flags & PIDFD_NONBLOCK != 0 {
            status |= OpenFlags::O_NONBLOCK;
        }
        let file = OpenFile::new(
            Backing::Stream(Arc::new(object), self.pseudo.anon("[pidfd]")),
            status,
        );
        let [fd] = self.install_files(caller, [file], true)?;
        Ok(fd)
    }

    fn pidfd_pid(&self, caller: &Caller<'_>, fd: Fd) -> Result<u32, Errno> {
        let file = self.file(caller.task(), fd)?;
        let pidfd = file.downcast::<PidFd>().ok_or(Errno::EINVAL)?;
        Ok(pidfd.pid)
    }

    pub(crate) async fn sys_pidfd_send_signal(
        &self,
        caller: &Caller<'_>,
        pidfd: Fd,
        sig: Signal,
        info: ConstSiginfoPtr,
        flags: u32,
    ) -> SysResult<i32> {
        if flags != 0 {
            return Err(Errno::EINVAL.into());
        }
        let pid = self.pidfd_pid(caller, pidfd).map_err(|err| match err {
            Errno::EINVAL => Errno::EBADF,
            err => err,
        })?;
        let alive = {
            let tree = self.procs.tree.lock().unwrap();
            tree.nodes.get(&pid).is_some_and(|node| node.exit.is_none())
        };
        if !alive {
            return Err(Errno::ESRCH.into());
        }
        // TODO: send the caller's `info` as `rt_sigqueueinfo` does.
        let _ = info;
        self.sys_kill(caller, pid as Pid, sig).await
    }
}

/// A process descriptor (`pidfd_open`): readable once the process exited.
struct PidFd {
    pid: u32,
    tree: Arc<Mutex<Tree>>,
    changed: Arc<WaitQueue>,
}

impl Pollable for PidFd {
    /// `pidfd_poll`.
    fn poll(&self, mut table: Option<&mut PollTable<'_>>) -> PollMask {
        poll_wait(&mut table, &self.changed);
        let tree = self.tree.lock().unwrap();
        match tree.nodes.get(&self.pid) {
            Some(node) if node.exit.is_none() => PollMask::empty(),
            _ => PollMask::READABLE | PollMask::HUP,
        }
    }
}

impl FileObject for PidFd {
    fn read<'a>(&'a self, _: Io<'a>, _: &'a mut [u8]) -> LocalFuture<'a, SysResult<usize>> {
        Box::pin(async { Err(Errno::EINVAL.into()) })
    }

    fn write<'a>(&'a self, _: Io<'a>, _: &'a [u8]) -> LocalFuture<'a, SysResult<usize>> {
        Box::pin(async { Err(Errno::EINVAL.into()) })
    }
}

/// `dup_fd`: the same open files at the same numbers, close-on-exec flags included.
pub(crate) fn copy_files(table: &FdTable) -> FdTable {
    let mut copy = FdTable::default();
    copy.set_limit(table.limit());
    for fd in (0..table.end()).map(|fd| Fd(fd as i32)) {
        if let Some(file) = table.get(fd) {
            let cloexec = table.cloexec(fd).unwrap_or(false);
            copy.install_at(fd, file, cloexec);
        }
    }
    copy
}

/// Reads a NULL-terminated array of string pointers (`copy_strings`). A NULL array is empty.
async fn read_strings(caller: &Caller<'_>, array: CstrArray) -> SysResult<Vec<String>> {
    let mut out = Vec::new();
    if array.is_null() {
        return Ok(out);
    }
    let mut total = 0;
    loop {
        let at = GuestPtr::<Cstr>::new(array.addr() + 4 * out.len() as u32);
        let ptr = read_value(caller.memory(), at).await?;
        if ptr.is_null() {
            return Ok(out);
        }
        if out.len() >= MAX_ARG_STRINGS {
            return Err(Errno::E2BIG.into());
        }
        let bytes = read_cstr(caller.memory(), ptr, MAX_ARG_STRLEN)
            .await
            .map_err(|err| match err {
                CstrError::Fault => Errno::EFAULT,
                CstrError::TooLong => Errno::E2BIG,
            })?;
        total += bytes.len() + 1;
        if total > ARG_MAX {
            return Err(Errno::E2BIG.into());
        }
        out.push(String::from_utf8_lossy(&bytes).into_owned());
    }
}

/// `load_script`'s parse of `#!interpreter [argument]`: the first line, up to
/// `BINPRM_BUF_SIZE`, with the argument being the rest of the line, trimmed.
#[allow(clippy::type_complexity)]
fn parse_shebang(bytes: &[u8]) -> Option<(Vec<u8>, Option<Vec<u8>>)> {
    let line = bytes.strip_prefix(b"#!")?;
    let head = &line[..line.len().min(BINPRM_BUF_SIZE - 2)];
    let end = head.iter().position(|&b| b == b'\n').unwrap_or(head.len());
    let line = &head[..end];
    let blank = |b: &u8| *b == b' ' || *b == b'\t';
    let start = line.iter().position(|b| !blank(b))?;
    let line = &line[start..];
    let interp_end = line.iter().position(blank).unwrap_or(line.len());
    let interp = line[..interp_end].to_vec();
    let rest = &line[interp_end..];
    let rest_start = rest.iter().position(|b| !blank(b));
    let arg = rest_start.map(|s| {
        let rest = &rest[s..];
        let end = rest
            .iter()
            .rposition(|b| !blank(b) && *b != b'\r')
            .map_or(0, |e| e + 1);
        rest[..end].to_vec()
    });
    Some((interp, arg.filter(|a| !a.is_empty())))
}

/// Memory for kernel work outside any syscall, where no guest pointer is followed.
struct NoMemory;

impl wasixv2_syscalls::UserMemory for NoMemory {
    fn read<'a>(
        &'a self,
        _: u32,
        _: &'a mut [u8],
    ) -> LocalFuture<'a, Result<(), wasixv2_syscalls::Fault>> {
        Box::pin(async { Err(wasixv2_syscalls::Fault) })
    }

    fn write<'a>(
        &'a self,
        _: u32,
        _: &'a [u8],
    ) -> LocalFuture<'a, Result<(), wasixv2_syscalls::Fault>> {
        Box::pin(async { Err(wasixv2_syscalls::Fault) })
    }

    fn size(&self) -> LocalFuture<'_, u64> {
        Box::pin(async { 0 })
    }

    fn grow(&self, _: u32) -> LocalFuture<'_, Option<u64>> {
        Box::pin(async { None })
    }
}

#[cfg(test)]
mod tests {
    use super::parse_shebang;

    #[test]
    fn shebang_lines() {
        assert_eq!(
            parse_shebang(b"#!/bin/sh\necho"),
            Some((b"/bin/sh".to_vec(), None))
        );
        assert_eq!(
            parse_shebang(b"#! /usr/bin/env  python3 -u \nx"),
            Some((b"/usr/bin/env".to_vec(), Some(b"python3 -u".to_vec())))
        );
        assert_eq!(parse_shebang(b"#!\n"), None);
        assert_eq!(parse_shebang(b"\0asm"), None);
    }
}
