//! Core dumps (`fs/coredump.c`, `fs/binfmt_elf.c`'s `elf_core_dump`): a process killed by a
//! signal whose default action dumps core writes its memory to the file `kernel.core_pattern`
//! names, or to the standard input of the helper it names after a `|`, and its wait status
//! tells (`WCOREDUMP`).
//!
//! The task that took the signal dumps on its way out, in its `exit_mm`, while its memory is
//! still there (Linux dumps in `get_signal`, before the group exits). The core is an ELF core
//! file of the wasm linear memory: see DESIGN.md, "Core dumps".

use std::{
    cell::Cell,
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicBool, AtomicU32, Ordering},
    },
};

use wasixv2_abi::{OpenFlags, RlimitResource, Siginfo, Signal};
use wasixv2_syscalls::{Caller, Errno, Image, TaskId};
use wasixv2_vfs::{FileType, LookupFlags, OpenHow};
use wasixv2_wait::NoInterrupt;

use crate::{
    Kernel, Process,
    file::{Backing, OpenFile},
    ident::Dumpable,
    printk::Level,
    sync::MutexExt,
};

/// `CORENAME_MAX_SIZE`: the longest `core_pattern`.
pub(crate) const CORENAME_MAX_SIZE: usize = 128;
/// `binfmt->min_coredump` for ELF, and the alignment of the memory in the core file:
/// `ELF_EXEC_PAGESIZE`, the page of the 32-bit Linux ABI, not wasm's.
const MIN_COREDUMP: u64 = 4096;
/// `RLIM_INFINITY` as `%c` prints it on a 32-bit kernel.
const RLIM_INFINITY_32: u64 = u32::MAX as u64;
/// `MMF_DUMP_FILTER_DEFAULT` and the bits `coredump_filter` keeps (`MMF_DUMP_FILTER_BITS`).
pub(crate) const COREDUMP_FILTER_DEFAULT: u32 = 0x33;
pub(crate) const COREDUMP_FILTER_MASK: u32 = (1 << 9) - 1;
/// `MMF_DUMP_ANON_PRIVATE`: the linear memory is private anonymous memory.
const MMF_DUMP_ANON_PRIVATE: u32 = 1 << 0;

/// `pidfd_info`'s `coredump_mask` bits.
pub(crate) const PIDFD_COREDUMPED: u32 = 1 << 0;
pub(crate) const PIDFD_COREDUMP_SKIP: u32 = 1 << 1;
pub(crate) const PIDFD_COREDUMP_USER: u32 = 1 << 2;
pub(crate) const PIDFD_COREDUMP_ROOT: u32 = 1 << 3;

/// `pidfs_coredump_mask`: how a process `dumpable` so would dump, none for one that is gone.
pub(crate) fn pidfd_mask(dumpable: Option<Dumpable>) -> u32 {
    match dumpable {
        Some(Dumpable::User) => PIDFD_COREDUMP_USER,
        Some(Dumpable::Root) => PIDFD_COREDUMP_ROOT,
        Some(Dumpable::Disable) | None => PIDFD_COREDUMP_SKIP,
    }
}

/// Starts the processes the kernel makes to run a program for it (`call_usermodehelper`): a
/// core dump's pipe helper. The embedder runs `image` as `task` on an instance of its own and
/// waits for nothing.
pub trait UsermodeHelper: Send + Sync {
    fn start(&self, task: TaskId, image: Image);
}

/// The core dump tunables and the helper starter.
pub(crate) struct Coredump {
    /// `kernel.core_pattern`.
    pattern: Mutex<Vec<u8>>,
    /// `kernel.core_uses_pid`.
    uses_pid: AtomicBool,
    /// `kernel.core_pipe_limit`, and how many pipe dumps are under way.
    pipe_limit: AtomicU32,
    pipe_count: AtomicU32,
    helper: OnceLock<Arc<dyn UsermodeHelper>>,
}

impl Default for Coredump {
    fn default() -> Self {
        Self {
            pattern: Mutex::new(b"core".to_vec()),
            uses_pid: AtomicBool::new(false),
            pipe_limit: AtomicU32::new(0),
            pipe_count: AtomicU32::new(0),
            helper: OnceLock::new(),
        }
    }
}

impl Coredump {
    pub(crate) fn pattern(&self) -> Vec<u8> {
        self.pattern.lock_or_recover().clone()
    }

    /// `proc_dostring_coredump`: a pattern of fewer than `CORENAME_MAX_SIZE` bytes.
    pub(crate) fn set_pattern(&self, pattern: &[u8]) -> Result<(), Errno> {
        let pattern = pattern.strip_suffix(b"\n").unwrap_or(pattern);
        let pattern = pattern.split(|&b| b == 0).next().unwrap_or_default();
        if pattern.len() >= CORENAME_MAX_SIZE {
            return Err(Errno::EINVAL);
        }
        *self.pattern.lock_or_recover() = pattern.to_vec();
        Ok(())
    }

    pub(crate) fn uses_pid(&self) -> bool {
        self.uses_pid.load(Ordering::Relaxed)
    }

    pub(crate) fn set_uses_pid(&self, uses: bool) {
        self.uses_pid.store(uses, Ordering::Relaxed);
    }

    pub(crate) fn pipe_limit(&self) -> u32 {
        self.pipe_limit.load(Ordering::Relaxed)
    }

    pub(crate) fn set_pipe_limit(&self, limit: u32) {
        self.pipe_limit.store(limit, Ordering::Relaxed);
    }
}

/// `sig_kernel_coredump`: the signals whose default action dumps core.
pub(crate) fn dumps_core(sig: i32) -> bool {
    matches!(
        Signal(sig),
        Signal::SIGQUIT
            | Signal::SIGILL
            | Signal::SIGTRAP
            | Signal::SIGABRT
            | Signal::SIGBUS
            | Signal::SIGFPE
            | Signal::SIGSEGV
            | Signal::SIGXCPU
            | Signal::SIGXFSZ
            | Signal::SIGSYS
    )
}

/// Where a core goes.
enum Target {
    File(Vec<u8>),
    /// The helper's arguments, its path first.
    Pipe(Vec<Vec<u8>>),
}

/// What the pattern's specifiers stand for.
struct Names {
    tgid: i32,
    global_tgid: u32,
    tid: i32,
    global_tid: u32,
    uid: u32,
    gid: u32,
    signal: i32,
    time: u64,
    hostname: Vec<u8>,
    comm: Vec<u8>,
    exe: Option<Vec<u8>>,
    core_limit: u64,
}

/// `cn_esc_printf`: `/` becomes `!`, and so does a name that is `.` or `..`.
fn escaped(text: &[u8]) -> Vec<u8> {
    if text == b"." || text == b".." {
        let mut out = text.to_vec();
        out[0] = b'!';
        return out;
    }
    text.iter()
        .map(|&b| if b == b'/' { b'!' } else { b })
        .collect()
}

/// `coredump_parse`: the pattern with its specifiers replaced; for a pipe, split into
/// arguments at blanks. `.%p` is appended to a file name with `uses_pid` and no `%p`.
fn parse(pattern: &[u8], names: &Names, uses_pid: bool) -> Option<Target> {
    let (pipe, mut rest) = match pattern.strip_prefix(b"|") {
        Some([]) => return None,
        Some(rest) => (true, rest),
        None => (false, pattern),
    };
    let mut args: Vec<Vec<u8>> = vec![Vec::new()];
    let mut pid_in_pattern = false;
    let mut was_space = false;
    while let Some((&c, after)) = rest.split_first() {
        rest = after;
        if pipe && c.is_ascii_whitespace() {
            if !args.last().expect("one argument at least").is_empty() {
                was_space = true;
            }
            continue;
        }
        if was_space {
            was_space = false;
            args.push(Vec::new());
        }
        let out = args.last_mut().expect("one argument at least");
        if c != b'%' {
            out.push(c);
            continue;
        }
        let Some((&spec, after)) = rest.split_first() else {
            break;
        };
        rest = after;
        let text = match spec {
            b'%' => b"%".to_vec(),
            b'p' => {
                pid_in_pattern = true;
                names.tgid.to_string().into_bytes()
            }
            b'P' => names.global_tgid.to_string().into_bytes(),
            b'i' => names.tid.to_string().into_bytes(),
            b'I' => names.global_tid.to_string().into_bytes(),
            b'u' => names.uid.to_string().into_bytes(),
            b'g' => names.gid.to_string().into_bytes(),
            b'd' => b"1".to_vec(),
            b's' => names.signal.to_string().into_bytes(),
            b't' => names.time.to_string().into_bytes(),
            b'h' => escaped(&names.hostname),
            b'e' => escaped(&names.comm),
            b'f' | b'E' => match &names.exe {
                Some(path) if spec == b'f' => {
                    escaped(path.rsplit(|&b| b == b'/').next().unwrap_or_default())
                }
                Some(path) => escaped(path),
                None => escaped(&[&names.comm[..], b" (path unknown)"].concat()),
            },
            b'c' => names.core_limit.to_string().into_bytes(),
            b'C' => b"0".to_vec(),
            _ => Vec::new(),
        };
        out.extend(text);
    }
    if pipe {
        return Some(Target::Pipe(args));
    }
    let mut name = args.pop().expect("one argument");
    if !pid_in_pattern && uses_pid {
        name.extend(format!(".{}", names.tgid).into_bytes());
    }
    Some(Target::File(name))
}

/// The ELF core file: its header, the program headers and the notes, padded to the page the
/// linear memory follows from.
pub(crate) struct ElfCore {
    pub(crate) head: Vec<u8>,
    /// How much of the memory is dumped: all of it, or none when `coredump_filter` leaves out
    /// private anonymous memory.
    pub(crate) memory_len: u64,
}

/// What the notes say of the process.
pub(crate) struct CoreInfo {
    pub(crate) info: Siginfo,
    pub(crate) pid: i32,
    pub(crate) ppid: i32,
    pub(crate) pgrp: i32,
    pub(crate) sid: i32,
    pub(crate) uid: u32,
    pub(crate) gid: u32,
    pub(crate) utime: std::time::Duration,
    pub(crate) stime: std::time::Duration,
    pub(crate) cutime: std::time::Duration,
    pub(crate) cstime: std::time::Duration,
    pub(crate) nice: i8,
    pub(crate) comm: [u8; 16],
    pub(crate) args: Vec<u8>,
}

const ET_CORE: u16 = 4;
const EM_NONE: u16 = 0;
const PT_LOAD: u32 = 1;
const PT_NOTE: u32 = 4;
const PF_W: u32 = 2;
const PF_R: u32 = 4;
const NT_PRSTATUS: u32 = 1;
const NT_PRPSINFO: u32 = 3;
const NT_SIGINFO: u32 = 0x5349_4749;
const EHDR_SIZE: usize = 52;
const PHDR_SIZE: usize = 32;

/// One note: its header, the name `CORE` and the description, each padded to 4 bytes.
fn note(kind: u32, desc: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend(5u32.to_le_bytes());
    out.extend((desc.len() as u32).to_le_bytes());
    out.extend(kind.to_le_bytes());
    out.extend(b"CORE\0\0\0\0");
    out.extend(desc);
    out.resize(out.len().next_multiple_of(4), 0);
    out
}

fn timeval(d: std::time::Duration) -> [u8; 8] {
    let mut out = [0; 8];
    out[..4].copy_from_slice(&(d.as_secs() as u32).to_le_bytes());
    out[4..].copy_from_slice(&d.subsec_micros().to_le_bytes());
    out
}

impl ElfCore {
    /// `elf_core_dump` for an address space of `memory_size` bytes: an `ET_CORE` ELF32
    /// little-endian file with no machine (`EM_NONE`), a `PT_NOTE` of `NT_PRSTATUS` (no
    /// registers), `NT_PRPSINFO` and `NT_SIGINFO`, then one `PT_LOAD` of the whole linear
    /// memory at address 0, page-aligned in the file.
    pub(crate) fn new(core: &CoreInfo, memory_size: u64, filter: u32) -> Self {
        let mut prstatus = Vec::new();
        let info = &core.info;
        for word in [info.si_signo, info.si_code, info.si_errno] {
            prstatus.extend(word.to_le_bytes());
        }
        prstatus.extend((info.si_signo as i16).to_le_bytes());
        prstatus.extend([0, 0]);
        prstatus.extend([0; 8]);
        for word in [core.pid, core.ppid, core.pgrp, core.sid] {
            prstatus.extend(word.to_le_bytes());
        }
        for time in [core.utime, core.stime, core.cutime, core.cstime] {
            prstatus.extend(timeval(time));
        }
        // `pr_fpvalid`; wasm has no registers to put in `pr_reg`.
        prstatus.extend(0i32.to_le_bytes());

        let mut prpsinfo = vec![0u8, b'R', 0, core.nice as u8];
        prpsinfo.extend(0u32.to_le_bytes());
        prpsinfo.extend(core.uid.to_le_bytes());
        prpsinfo.extend(core.gid.to_le_bytes());
        for word in [core.pid, core.ppid, core.pgrp, core.sid] {
            prpsinfo.extend(word.to_le_bytes());
        }
        prpsinfo.extend(core.comm);
        let mut args = core.args.clone();
        args.truncate(79);
        args.resize(80, 0);
        prpsinfo.extend(args);

        let mut siginfo = Vec::new();
        for word in [info.si_signo, info.si_errno, info.si_code] {
            siginfo.extend(word.to_le_bytes());
        }
        siginfo.extend(info.sifields);

        let notes = [
            note(NT_PRSTATUS, &prstatus),
            note(NT_PRPSINFO, &prpsinfo),
            note(NT_SIGINFO, &siginfo),
        ]
        .concat();
        let notes_offset = (EHDR_SIZE + 2 * PHDR_SIZE) as u64;
        let memory_offset = (notes_offset + notes.len() as u64).next_multiple_of(MIN_COREDUMP);
        let memory_len = match filter & MMF_DUMP_ANON_PRIVATE {
            0 => 0,
            _ => memory_size,
        };

        let mut head = Vec::new();
        head.extend(b"\x7fELF");
        // ELFCLASS32, ELFDATA2LSB, EV_CURRENT, ELFOSABI_NONE.
        head.extend([1, 1, 1, 0]);
        head.extend([0; 8]);
        head.extend(ET_CORE.to_le_bytes());
        head.extend(EM_NONE.to_le_bytes());
        head.extend(1u32.to_le_bytes());
        // Entry, program headers right after this header, no section headers, flags.
        head.extend(0u32.to_le_bytes());
        head.extend((EHDR_SIZE as u32).to_le_bytes());
        head.extend(0u32.to_le_bytes());
        head.extend(0u32.to_le_bytes());
        for half in [EHDR_SIZE as u16, PHDR_SIZE as u16, 2, 40, 0, 0] {
            head.extend(half.to_le_bytes());
        }
        let phdr = |kind: u32, offset: u64, vaddr: u32, filesz: u64, memsz: u64, flags, align| {
            let mut out = Vec::new();
            for word in [
                kind,
                offset as u32,
                vaddr,
                0,
                filesz as u32,
                memsz as u32,
                flags,
                align,
            ] {
                out.extend(word.to_le_bytes());
            }
            out
        };
        let notes_len = notes.len() as u64;
        head.extend(phdr(PT_NOTE, notes_offset, 0, notes_len, 0, 0, 0));
        head.extend(phdr(
            PT_LOAD,
            memory_offset,
            0,
            memory_len,
            memory_size,
            PF_R | PF_W,
            MIN_COREDUMP as u32,
        ));
        head.extend(notes);
        head.resize(memory_offset as usize, 0);
        Self { head, memory_len }
    }
}

/// Writes a core within `limit` bytes; false once something is not written.
struct Sink<'a> {
    file: &'a OpenFile,
    task: TaskId,
    written: u64,
    limit: u64,
}

impl Sink<'_> {
    /// `dump_emit`.
    async fn emit(&mut self, mut data: &[u8]) -> bool {
        if self.written + data.len() as u64 > self.limit {
            return false;
        }
        let broken = Cell::new(false);
        while !data.is_empty() {
            match self
                .file
                .write(self.task, &NoInterrupt, &broken, data)
                .await
            {
                Ok(0) | Err(_) => return false,
                Ok(n) => {
                    data = &data[n..];
                    self.written += n as u64;
                }
            }
        }
        true
    }
}

impl Kernel {
    /// Lets the kernel start the programs it runs itself, as a core dump's pipe helper; without
    /// one, a `core_pattern` that names a helper dumps nothing, as Linux's
    /// `CONFIG_STATIC_USERMODEHELPER_PATH=""` does. Once.
    pub fn set_usermode_helper(&self, helper: Arc<dyn UsermodeHelper>) {
        let _ = self.coredump.helper.set(helper);
    }

    /// The signal `info`, whose default action dumps core, is killing the process of `task`:
    /// the first such task dumps on its way out.
    pub(crate) fn coredump_request(&self, task: TaskId, info: Siginfo) {
        let process = self.process(task);
        let mut ident = process.ident.lock_or_recover();
        if ident.core_dumper.is_none() && !process.group.exiting() {
            ident.core_dumper = Some((task, info));
        }
    }

    /// The dump `task` is to make on its way out, if it is the dumping task.
    fn take_core_request(&self, task: TaskId) -> Option<Siginfo> {
        let process = {
            let state = self.state.read_or_recover();
            state.tasks.get(&task)?.process.clone()
        };
        let mut ident = process.ident.lock_or_recover();
        match ident.core_dumper {
            Some((dumper, info)) if dumper == task => {
                ident.core_dumper = None;
                Some(info)
            }
            _ => None,
        }
    }

    /// `coredump_report_failure`.
    fn coredump_failure(&self, process: &Process, comm: &[u8], why: &str) {
        let text = format!(
            "coredump: {}({}): {why}",
            process.pid.0,
            String::from_utf8_lossy(comm)
        );
        self.printk(Level::Info, &text);
    }

    /// `vfs_coredump` for `caller`, the dumping task, whose signal is `info`: the core goes to
    /// the file or the helper `core_pattern` names, within `RLIMIT_CORE` for a file, unless
    /// the process is not dumpable. A core that started marks the process as having dumped
    /// (`WCOREDUMP`).
    pub(crate) async fn do_coredump(&self, caller: &Caller<'_>, info: Siginfo) {
        let task = caller.task();
        let process = self.process(task);
        let (dumpable, filter) = {
            let ident = process.ident.lock_or_recover();
            (ident.dumpable, ident.coredump_filter)
        };
        let comm = self.task_ref(task, |t| t.comm.name().to_vec());
        if dumpable == Dumpable::Disable {
            self.note_coredump(&process, PIDFD_COREDUMP_SKIP, &info);
            return;
        }
        // `SUID_DUMP_ROOT`: the core is root's, and only goes to a full path or a helper.
        let as_root = dumpable == Dumpable::Root;
        let limit = process
            .resources
            .lock_or_recover()
            .soft(RlimitResource::RLIMIT_CORE);
        let names = self.core_names(task, &process, &info, &comm, limit);
        let pattern = self.coredump.pattern();
        let Some(target) = parse(&pattern, &names, self.coredump.uses_pid()) else {
            return self.coredump_failure(&process, &comm, "format_corename failed, aborting core");
        };
        let (file, limit, piped) = match target {
            Target::File(name) if as_root && !name.starts_with(b"/") => {
                let why =
                    "this process can only dump core to a fully qualified path, skipping core dump";
                return self.coredump_failure(&process, &comm, why);
            }
            Target::File(name) => match self.core_file(caller, &name, limit, as_root).await {
                Some(file) => (file, limit, false),
                None => return,
            },
            Target::Pipe(args) => match self.core_pipe(&process, &comm, args, limit).await {
                Some(file) => (file, u64::MAX, true),
                None => return,
            },
        };
        process.ident.lock_or_recover().dumped_core = true;
        let how = pidfd_mask(Some(dumpable));
        self.note_coredump(&process, PIDFD_COREDUMPED | how, &info);
        let core = self.core_info(task, &process, info, &comm);
        let size = caller.memory().size().await;
        let elf = ElfCore::new(&core, size, filter);
        let mut sink = Sink {
            file: &file,
            task,
            written: 0,
            limit,
        };
        if !sink.emit(&elf.head).await {
            return;
        }
        let mut chunk = vec![0; crate::IO_CHUNK];
        let mut at = 0;
        while at < elf.memory_len {
            let n = (elf.memory_len - at).min(chunk.len() as u64) as usize;
            if caller
                .memory()
                .read(at as u32, &mut chunk[..n])
                .await
                .is_err()
            {
                chunk[..n].fill(0);
            }
            if !sink.emit(&chunk[..n]).await {
                return;
            }
            at += n as u64;
        }
        if piped {
            self.coredump.pipe_count.fetch_sub(1, Ordering::Relaxed);
        }
    }

    /// `coredump_file`: the core file, made afresh with mode `0600` in the dumping task's
    /// working directory; none if it is too small to hold a core under `limit`, or the file
    /// made is not a regular file of the dumper's with that mode and one link.
    async fn core_file(
        &self,
        caller: &Caller<'_>,
        name: &[u8],
        limit: u64,
        as_root: bool,
    ) -> Option<OpenFile> {
        if limit < MIN_COREDUMP {
            return None;
        }
        let mut env = self.env(caller);
        // `cred->fsuid = GLOBAL_ROOT_UID`.
        if as_root {
            env.cred.fsuid = 0;
        }
        let _ = self.vfs.unlink(&env.cx(), &env.fs.cwd, name).await;
        let how = OpenHow {
            flags: OpenFlags::O_CREAT
                | OpenFlags::O_WRONLY
                | OpenFlags::O_NOFOLLOW
                | OpenFlags::O_LARGEFILE
                | OpenFlags::O_EXCL,
            mode: 0o600,
            lookup: LookupFlags::default(),
        };
        let file = self
            .vfs
            .open(&env.cx(), &env.fs.cwd, name, &how)
            .await
            .ok()?;
        let attr = file.node().getattr().await.ok()?;
        if attr.nlink > 1 || attr.file_type != FileType::Regular {
            return None;
        }
        if attr.uid != env.cred.fsuid || attr.mode & 0o677 != 0o600 {
            return None;
        }
        Some(OpenFile::new(Backing::Inode(file), OpenFlags::O_WRONLY))
    }

    /// `coredump_pipe`: the write end of a pipe whose read end is the standard input of the
    /// helper `args` names, started as root in the initial namespaces (`umh_coredump_setup`).
    /// None with `RLIMIT_CORE` of 1 (a helper's own crash), past `core_pipe_limit`, or when
    /// the helper does not start.
    async fn core_pipe(
        &self,
        process: &Process,
        comm: &[u8],
        args: Vec<Vec<u8>>,
        limit: u64,
    ) -> Option<OpenFile> {
        if limit == 1 {
            self.coredump_failure(process, comm, "RLIMIT_CORE is set to 1, aborting core");
            return None;
        }
        let count = self.coredump.pipe_count.fetch_add(1, Ordering::Relaxed) + 1;
        let pipe_limit = self.coredump.pipe_limit();
        let started = match self.coredump.helper.get() {
            _ if pipe_limit != 0 && pipe_limit < count => {
                self.coredump_failure(process, comm, "over core_pipe_limit, skipping core dump");
                None
            }
            None => {
                let name = String::from_utf8_lossy(&args[0]).into_owned();
                let why = format!("Core dump to |{name} disabled");
                self.coredump_failure(process, comm, &why);
                None
            }
            Some(helper) => self.start_core_helper(helper.clone(), args).await,
        };
        if started.is_none() {
            self.coredump.pipe_count.fetch_sub(1, Ordering::Relaxed);
        }
        started
    }

    /// `call_usermodehelper_exec` of the helper `args` names, with the read end of a new pipe
    /// as its standard input; returns the write end.
    async fn start_core_helper(
        &self,
        helper: Arc<dyn UsermodeHelper>,
        args: Vec<Vec<u8>>,
    ) -> Option<OpenFile> {
        let size = crate::pipe::DEFAULT_SIZE;
        let [read, write] = self.pipe_files(size, (0, 0), OpenFlags(0));
        let mut files = self.fd_table();
        files.install(Arc::new(read), false)?;
        let root = crate::Credentials::new(0, 0);
        let name = String::from_utf8_lossy(&args[0]).into_owned();
        let task = self
            .new_process_as(
                &root,
                vec![name],
                Vec::new(),
                files,
                self.init.map_or(0, |i| i.0),
            )
            .ok()?;
        // `call_usermodehelper_setup`'s environment.
        let env = [
            b"HOME=/".to_vec(),
            b"TERM=linux".to_vec(),
            b"PATH=/sbin:/bin:/usr/sbin:/usr/bin".to_vec(),
        ];
        match self.exec(task, &args[0].clone(), args, env.to_vec()).await {
            Ok(image) => {
                helper.start(task, image);
                Some(write)
            }
            Err(_) => {
                use wasixv2_syscalls::Control;
                self.fault(task, Signal::SIGKILL.0);
                self.release(task);
                None
            }
        }
    }

    /// The values of the pattern's specifiers for the dumping `task` of `process`.
    fn core_names(
        &self,
        task: TaskId,
        process: &Process,
        info: &Siginfo,
        comm: &[u8],
        limit: u64,
    ) -> Names {
        let cred = self.task_cred(task);
        let ns = self.task_ns(task);
        let hostname = {
            let names = ns.uts.lock_or_recover();
            let n = names.nodename.iter().position(|&b| b == 0).unwrap_or(65);
            names.nodename[..n].to_vec()
        };
        // `file_path`: from the dumping task's root.
        let root = self.task_ref(task, |t| t.fs.lock_or_recover().root.clone());
        let exe = process
            .exe
            .lock_or_recover()
            .as_ref()
            .map(|path| self.vfs.d_path(path, &root));
        Names {
            tgid: self.pids.nr(&process.pid_ns, process.pid.0),
            global_tgid: process.pid.0,
            tid: self.own_tid(task),
            global_tid: task.0,
            uid: cred.user.real.0,
            gid: cred.group.real.0,
            signal: info.si_signo,
            time: self.host.realtime().as_secs(),
            hostname,
            comm: comm.to_vec(),
            exe,
            core_limit: limit.min(RLIM_INFINITY_32),
        }
    }

    /// What the notes tell of `process` and its dumping `task`.
    fn core_info(&self, task: TaskId, process: &Process, info: Siginfo, comm: &[u8]) -> CoreInfo {
        let cred = self.task_cred(task);
        let (pgid, sid) = {
            let ident = process.ident.lock_or_recover();
            (ident.pgid, ident.sid)
        };
        let nr = |id: u32| self.pids.nr(&process.pid_ns, id);
        let parent = self
            .procs
            .tree
            .lock_or_recover()
            .nodes
            .get(&process.pid.0)
            .map_or(0, |node| node.parent);
        let times = process.cpu.times();
        let children = process.children.lock_or_recover().times;
        let nice = self.task_ref(task, |t| t.sched.nice()) as i8;
        let mut name = [0; 16];
        name[..comm.len().min(15)].copy_from_slice(&comm[..comm.len().min(15)]);
        let args = process.args.lock_or_recover();
        let mut psargs: Vec<u8> = args.join(&b' ');
        psargs.truncate(80);
        CoreInfo {
            info,
            pid: nr(process.pid.0),
            ppid: if parent == 0 { 0 } else { nr(parent) },
            pgrp: nr(pgid),
            sid: nr(sid),
            uid: self.task_cred(task).user_ns.munged_uid(cred.user.real),
            gid: self.task_cred(task).user_ns.munged_gid(cred.group.real),
            utime: times.user,
            stime: times.system,
            cutime: children.user,
            cstime: children.system,
            nice,
            comm: name,
            args: psargs,
        }
    }

    /// `pidfs_coredump`: a pidfd of the process tells how it dumped.
    fn note_coredump(&self, process: &Process, mask: u32, info: &Siginfo) {
        self.procs.note_coredump(
            process.pid.0,
            (mask, info.si_signo as u32, info.si_code as u32),
        );
    }

    /// The dump `caller` is to make, if it is the task that took a signal that dumps core.
    pub(crate) async fn coredump_if_dumper(&self, caller: &Caller<'_>) {
        if let Some(info) = self.take_core_request(caller.task()) {
            self.do_coredump(caller, info).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names() -> Names {
        Names {
            tgid: 42,
            global_tgid: 1042,
            tid: 43,
            global_tid: 1043,
            uid: 1000,
            gid: 100,
            signal: 11,
            time: 1_700_000_000,
            hostname: b"box".to_vec(),
            comm: b"a/b".to_vec(),
            exe: Some(b"/usr/bin/prog".to_vec()),
            core_limit: RLIM_INFINITY_32,
        }
    }

    fn file(target: Option<Target>) -> Vec<u8> {
        match target {
            Some(Target::File(name)) => name,
            _ => panic!("a file"),
        }
    }

    #[test]
    fn patterns_expand_as_format_corename_does() {
        let n = names();
        assert_eq!(file(parse(b"core", &n, false)), b"core");
        assert_eq!(file(parse(b"core", &n, true)), b"core.42");
        assert_eq!(
            file(parse(b"/c/%e.%p.%P.%i.%I.%u.%g.%s.%t.%h.%%.%c%", &n, true)),
            b"/c/a!b.42.1042.43.1043.1000.100.11.1700000000.box.%.4294967295"
        );
        assert_eq!(file(parse(b"%f-%E", &n, false)), b"prog-!usr!bin!prog");
        let Some(Target::Pipe(args)) = parse(b"|/bin/catch  %p   %e", &n, false) else {
            panic!("a pipe");
        };
        assert_eq!(
            args,
            [b"/bin/catch".to_vec(), b"42".to_vec(), b"a!b".to_vec()]
        );
        assert!(parse(b"|", &n, false).is_none());
    }

    #[test]
    fn the_core_is_an_elf_core_file_of_the_linear_memory() {
        let core = CoreInfo {
            info: Siginfo {
                si_signo: 6,
                si_errno: 0,
                si_code: 0,
                sifields: [0; 116],
            },
            pid: 7,
            ppid: 1,
            pgrp: 7,
            sid: 7,
            uid: 0,
            gid: 0,
            utime: Default::default(),
            stime: Default::default(),
            cutime: Default::default(),
            cstime: Default::default(),
            nice: 0,
            comm: *b"prog\0\0\0\0\0\0\0\0\0\0\0\0",
            args: b"prog -x".to_vec(),
        };
        let elf = ElfCore::new(&core, 1 << 20, COREDUMP_FILTER_DEFAULT);
        assert_eq!(&elf.head[..4], b"\x7fELF");
        assert_eq!(u16::from_le_bytes([elf.head[16], elf.head[17]]), ET_CORE);
        assert_eq!(elf.head.len() as u64 % MIN_COREDUMP, 0);
        assert_eq!(elf.memory_len, 1 << 20);
        let none = ElfCore::new(&core, 1 << 20, 0);
        assert_eq!(none.memory_len, 0);
    }
}
