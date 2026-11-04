//! binfmt_misc (`fs/binfmt_misc.c`): formats of executables registered at run time, by a
//! magic number at an offset of the file or by its name's extension, each run by an
//! interpreter of its own. `execve` asks before it looks for a `#!` line (Linux inserts
//! binfmt_misc at the head of its formats); a match hands the file to the interpreter with the
//! file's name as its first argument, as a script hands over.
//!
//! The registry is the `binfmt_misc` filesystem, usually mounted at
//! `/proc/sys/fs/binfmt_misc`: `register` takes `:name:type:offset:magic:mask:interpreter:flags`
//! (types `M` and `E`), `status` turns the whole registry on and off, and each format has a
//! file of its own that shows it and takes `1`, `0` and `-1` (enable, disable, remove).
//!
//! The flags: `P` keeps `argv[0]`; `O` hands the interpreter the binary as an open descriptor
//! (`AT_EXECFD`), `C` also takes the new image's credentials from the binary rather than the
//! interpreter, and `T` (transparent) leaves the arguments alone and presents the process as
//! the binary; `F` resolves the interpreter at registration; `L` substitutes the interpreter
//! for the binary's program interpreter, which no format here has, so it is opened and
//! dropped as binfmt_elf drops it for a static binary; `D` registers disabled. The kernel has
//! no BPF, so `B` registrations are `EINVAL` (`CONFIG_BINFMT_MISC_BPF` off).
//!
//! Every user namespace that mounts the filesystem gets an instance of its own, owned by its
//! root (`FS_USERNS_MOUNT`, Linux 6.7); `execve` uses the instance of the caller's user
//! namespace or of its nearest ancestor with one (`current_binfmt_misc`). An instance's formats
//! go once nothing has it mounted (`bm_kill_sb`).

use std::{
    any::Any,
    sync::{
        Arc, Mutex, Weak,
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
    },
};

use async_trait::async_trait;
use wasixv2_syscalls::{BINPRM_BUF_SIZE, Errno, TaskId};
use wasixv2_vfs::{
    Attr, Clock, Cred, Ctx, DirEntry, DirSink, FileType, Filesystem, FsInfo, FsResult, Handle,
    LookupFlags, MAY_EXEC, NAME_MAX, NewNode, Node, OpenOptions, Path, SetAttr, StatFs, Timestamp,
    Vfs,
};

use crate::{
    TaskTable,
    fs::mount::{FilesystemType, FsParam},
    ns::UserNs,
    sync::MutexExt,
    userns::kernel_user_ns,
};

/// `BINFMTFS_MAGIC`.
const BINFMTFS_MAGIC: u32 = 0x4249_4e4d;
/// `MAX_REGISTER_LENGTH`.
const MAX_REGISTER_LENGTH: usize = 1920;
const ROOT_INO: u64 = 1;
const REGISTER_INO: u64 = 2;
const STATUS_INO: u64 = 3;

/// `MISC_FMT_PRESERVE_ARGV0` (`P`).
pub(crate) const PRESERVE_ARGV0: u32 = 1 << 31;
/// `MISC_FMT_OPEN_BINARY` (`O`).
pub(crate) const OPEN_BINARY: u32 = 1 << 30;
/// `MISC_FMT_CREDENTIALS` (`C`).
pub(crate) const CREDENTIALS: u32 = 1 << 29;
/// `MISC_FMT_OPEN_FILE` (`F`).
const OPEN_FILE: u32 = 1 << 28;
/// `MISC_FMT_TRANSPARENT` (`T`).
pub(crate) const TRANSPARENT: u32 = 1 << 27;
/// `MISC_FMT_LOADER` (`L`).
pub(crate) const LOADER: u32 = 1 << 26;
/// `MISC_FMT_DISABLED` (`D`), consumed at registration.
const DISABLED: u32 = 1 << 25;

/// `misc_flags`: each flag's letter, its bit and the bits it implies, in the order shown.
const FLAGS: [(u8, u32, u32); 7] = [
    (b'P', PRESERVE_ARGV0, 0),
    (b'O', OPEN_BINARY, 0),
    (b'C', CREDENTIALS, OPEN_BINARY),
    (b'F', OPEN_FILE, 0),
    (b'T', TRANSPARENT, OPEN_BINARY),
    (b'L', LOADER, 0),
    (b'D', DISABLED, 0),
];

/// How a format is recognised.
enum Match {
    /// `magic` at `offset` of the file's first bytes, compared under `mask`.
    Magic {
        offset: usize,
        magic: Vec<u8>,
        mask: Option<Vec<u8>>,
    },
    /// The file name's extension, after its last dot.
    Extension(Vec<u8>),
}

/// One registered format (`struct binfmt_misc_entry`).
pub(crate) struct Entry {
    name: Vec<u8>,
    ino: u64,
    enabled: AtomicBool,
    how: Match,
    interpreter: Vec<u8>,
    /// The `MISC_FMT_*` flags but `D`.
    flags: u32,
    /// `F`: the interpreter as resolved at registration.
    interp_file: Option<Path>,
    interp_access: Option<Arc<wasixv2_vfs::ExecAccess>>,
}

impl Entry {
    /// `entry_matches_magic` and `entry_matches_extension`.
    fn matches(&self, header: &[u8], name: &[u8]) -> bool {
        match &self.how {
            Match::Magic {
                offset,
                magic,
                mask,
            } => (0..magic.len()).all(|i| {
                let byte = header.get(offset + i).copied().unwrap_or(0);
                let mask = mask.as_ref().map_or(0xff, |mask| mask[i]);
                (byte ^ magic[i]) & mask == 0
            }),
            Match::Extension(ext) => {
                let dot = name.iter().rposition(|&b| b == b'.');
                dot.is_some_and(|at| &name[at + 1..] == ext.as_slice())
            }
        }
    }

    /// `bm_entry_show`.
    fn show(&self) -> Vec<u8> {
        let mut out = match self.enabled.load(Ordering::Acquire) {
            true => b"enabled\n".to_vec(),
            false => b"disabled\n".to_vec(),
        };
        out.extend_from_slice(b"interpreter ");
        out.extend_from_slice(&self.interpreter);
        out.extend_from_slice(b"\nflags: ");
        let shown = FLAGS.iter().filter(|(_, bit, _)| self.flags & bit != 0);
        out.extend(shown.map(|(c, _, _)| *c));
        out.push(b'\n');
        match &self.how {
            Match::Extension(ext) => {
                out.extend_from_slice(b"extension .");
                out.extend_from_slice(ext);
                out.push(b'\n');
            }
            Match::Magic {
                offset,
                magic,
                mask,
            } => {
                out.extend_from_slice(format!("offset {offset}\nmagic {}", hex(magic)).as_bytes());
                if let Some(mask) = mask {
                    out.extend_from_slice(format!("\nmask {}", hex(mask)).as_bytes());
                }
                out.push(b'\n');
            }
        }
        out
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// What `execve` does with a file a format took: the interpreter to open, and how
/// (`MISC_FMT_*` flags).
pub(crate) struct Handover {
    pub(crate) interpreter: Vec<u8>,
    pub(crate) interp_file: Option<Path>,
    _interp_access: Option<Arc<wasixv2_vfs::ExecAccess>>,
    pub(crate) flags: u32,
}

/// A registry of formats (`struct binfmt_misc`): the kernel's, for the initial user namespace,
/// or the one a user namespace made with its first mount.
pub(crate) struct BinfmtMisc {
    enabled: AtomicBool,
    /// Newest first, the order they are tried in.
    entries: Mutex<Vec<Arc<Entry>>>,
    next_ino: AtomicU64,
    /// The superblocks that show it: its formats go with the last (`bm_kill_sb`).
    supers: AtomicUsize,
    clock: Arc<dyn Clock>,
    /// Who writes a registration, and where its interpreter is (`F`).
    tasks: Weak<TaskTable>,
    vfs: Weak<Vfs>,
}

impl BinfmtMisc {
    pub(crate) fn new(
        clock: Arc<dyn Clock>,
        tasks: &Weak<TaskTable>,
        vfs: &Weak<Vfs>,
    ) -> Arc<Self> {
        Arc::new(Self {
            enabled: AtomicBool::new(true),
            entries: Mutex::default(),
            next_ino: AtomicU64::new(STATUS_INO + 1),
            supers: AtomicUsize::new(0),
            clock,
            tasks: tasks.clone(),
            vfs: vfs.clone(),
        })
    }

    /// `load_misc_binary`'s choice: the enabled format that takes a file starting with
    /// `header` (up to `BINPRM_BUF_SIZE` bytes, zeroes after a short file) and named `name`.
    pub(crate) fn lookup(&self, header: &[u8], name: &[u8]) -> Option<Handover> {
        if !self.enabled.load(Ordering::Acquire) {
            return None;
        }
        let entries = self.entries.lock_or_recover();
        let entry = entries
            .iter()
            .filter(|e| e.enabled.load(Ordering::Acquire))
            .find(|e| e.matches(header, name))?;
        Some(Handover {
            interpreter: entry.interpreter.clone(),
            interp_file: entry.interp_file.clone(),
            _interp_access: entry.interp_access.clone(),
            flags: entry.flags,
        })
    }

    fn entry(&self, ino: u64) -> Option<Arc<Entry>> {
        let entries = self.entries.lock_or_recover();
        entries.iter().find(|e| e.ino == ino).cloned()
    }

    /// `bm_register_write`: `create_entry`, the interpreter resolved now with `F` as the
    /// writer `task` sees it, then `add_entry` (`EEXIST` for a name in use).
    async fn register(&self, data: &[u8], task: u32) -> FsResult<usize> {
        let mut entry = create_entry(data, self.next_ino.fetch_add(1, Ordering::Relaxed))?;
        if entry.flags & OPEN_FILE != 0 {
            let (path, access) = self.resolve_interpreter(&entry.interpreter, task).await?;
            entry.interp_file = Some(path);
            entry.interp_access = access;
        }
        let mut entries = self.entries.lock_or_recover();
        if entries.iter().any(|e| e.name == entry.name) {
            return Err(Errno::EEXIST);
        }
        entries.insert(0, Arc::new(entry));
        Ok(data.len())
    }

    /// `open_exec` of the interpreter in the writer's context: an executable regular file on
    /// a mount that allows execution.
    async fn resolve_interpreter(
        &self,
        path: &[u8],
        task: u32,
    ) -> FsResult<(Path, Option<Arc<wasixv2_vfs::ExecAccess>>)> {
        let (tasks, vfs) = match (self.tasks.upgrade(), self.vfs.upgrade()) {
            (Some(tasks), Some(vfs)) => (tasks, vfs),
            _ => return Err(Errno::ENOENT),
        };
        let (fs, cred): (crate::fs::FsContext, Cred) = {
            let state = tasks.read_or_recover();
            let found = state.tasks.get(&TaskId(task)).ok_or(Errno::ENOENT)?;
            let fs = found.fs.lock_or_recover().clone();
            (fs, found.cred.clone().fs())
        };
        let cx = Ctx {
            cred: &cred,
            root: &fs.root,
            umask: fs.umask,
            task,
            leases: None,
        };
        let start = match path.starts_with(b"/") {
            true => fs.root.clone(),
            false => fs.cwd.clone(),
        };
        let target = vfs.lookup(&cx, &start, path, LookupFlags::follow()).await?;
        if target.dentry.file_type() != FileType::Regular || target.mnt.is_noexec() {
            return Err(Errno::EACCES);
        }
        vfs.permission(&cx, &target, MAY_EXEC).await?;
        // fs/binfmt_misc.c open_interp_file/close_interp_file retain open_exec's denial
        // for the registration's lifetime, including lookup handover to a new exec.
        let access = vfs.exec_access(&target)?;
        Ok((target, access))
    }

    /// `bm_status_write` and `bm_entry_write`: `1`, `0` or `-1` for `entry`, or for the whole
    /// registry without one (`-1` then removes every format).
    fn command(&self, data: &[u8], entry: Option<&Arc<Entry>>) -> FsResult<usize> {
        let mut s = data;
        if s.len() > 3 {
            return Err(Errno::EINVAL);
        }
        if s.is_empty() {
            return Ok(0);
        }
        if s.last() == Some(&b'\n') {
            s = &s[..s.len() - 1];
        }
        match (s, entry) {
            (b"0", Some(e)) => e.enabled.store(false, Ordering::Release),
            (b"1", Some(e)) => e.enabled.store(true, Ordering::Release),
            (b"-1", Some(e)) => self
                .entries
                .lock_or_recover()
                .retain(|x| !Arc::ptr_eq(x, e)),
            (b"0", None) => self.enabled.store(false, Ordering::Release),
            (b"1", None) => self.enabled.store(true, Ordering::Release),
            (b"-1", None) => self.entries.lock_or_recover().clear(),
            _ => return Err(Errno::EINVAL),
        }
        Ok(data.len())
    }
}

/// `create_entry`: parses a registration.
fn create_entry(data: &[u8], ino: u64) -> FsResult<Entry> {
    if data.len() < 11 || data.len() > MAX_REGISTER_LENGTH {
        return Err(Errno::EINVAL);
    }
    let del = data[0];
    // `misc_valid_delim`: printable punctuation, but not a backslash.
    if !del.is_ascii_punctuation() || del == b'\\' {
        return Err(Errno::EINVAL);
    }
    let mut body = &data[1..];
    if body.last() == Some(&b'\n') {
        body = &body[..body.len() - 1];
    }
    let fields: Vec<&[u8]> = body.split(|&b| b == del).collect();
    let [name, kind, offset, magic, mask, interpreter, flags] = fields[..] else {
        return Err(Errno::EINVAL);
    };
    if name.is_empty() || name == b"." || name == b".." || name.contains(&b'/') {
        return Err(Errno::EINVAL);
    }
    let how = match kind {
        b"M" => {
            let offset = match offset {
                b"" => 0,
                digits => std::str::from_utf8(digits)
                    .ok()
                    .and_then(|s| s.parse::<usize>().ok())
                    .ok_or(Errno::EINVAL)?,
            };
            let magic = unescape(magic).ok_or(Errno::EINVAL)?;
            if magic.is_empty() {
                return Err(Errno::EINVAL);
            }
            let mask = match mask {
                b"" => None,
                mask => Some(unescape(mask).ok_or(Errno::EINVAL)?),
            };
            if mask.as_ref().is_some_and(|m| m.len() != magic.len()) {
                return Err(Errno::EINVAL);
            }
            if magic.len() > BINPRM_BUF_SIZE || BINPRM_BUF_SIZE - magic.len() < offset {
                return Err(Errno::EINVAL);
            }
            Match::Magic {
                offset,
                magic,
                mask,
            }
        }
        b"E" => {
            if magic.is_empty() || magic.contains(&b'/') {
                return Err(Errno::EINVAL);
            }
            Match::Extension(magic.to_vec())
        }
        // `B`: no BPF handlers without `CONFIG_BINFMT_MISC_BPF`.
        _ => return Err(Errno::EINVAL),
    };
    if interpreter.is_empty() {
        return Err(Errno::EINVAL);
    }
    // `check_special_flags`: an unknown letter ends the flags, and what follows is `EINVAL`.
    let mut bits = 0;
    for c in flags {
        let (_, bit, implies) = FLAGS.iter().find(|(f, ..)| f == c).ok_or(Errno::EINVAL)?;
        bits |= bit | implies;
    }
    // Transparency keeps the whole argv, and a loader substitution changes nothing of it.
    if bits & TRANSPARENT != 0 && bits & PRESERVE_ARGV0 != 0 {
        return Err(Errno::EINVAL);
    }
    let invocation = TRANSPARENT | PRESERVE_ARGV0 | CREDENTIALS | OPEN_BINARY;
    if bits & LOADER != 0 && bits & invocation != 0 {
        return Err(Errno::EINVAL);
    }
    // Without `F` the interpreter is opened at exec from the caller's working directory.
    if bits & (LOADER | CREDENTIALS) != 0 && bits & OPEN_FILE == 0 && interpreter[0] != b'/' {
        return Err(Errno::EINVAL);
    }
    Ok(Entry {
        name: name.to_vec(),
        ino,
        enabled: AtomicBool::new(bits & DISABLED == 0),
        how,
        interpreter: interpreter.to_vec(),
        flags: bits & !DISABLED,
        interp_file: None,
        interp_access: None,
    })
}

/// `string_unescape_inplace(UNESCAPE_HEX)`: `\xHH` escapes decoded, up to the first NUL.
fn unescape(field: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(field.len());
    let mut i = 0;
    while i < field.len() {
        if field[i] == b'\\' && field.get(i + 1) == Some(&b'x') {
            let digits = field.get(i + 2..i + 4)?;
            let byte = u8::from_str_radix(std::str::from_utf8(digits).ok()?, 16).ok()?;
            out.push(byte);
            i += 4;
        } else {
            out.push(field[i]);
            i += 1;
        }
    }
    let end = out.iter().position(|&b| b == 0).unwrap_or(out.len());
    out.truncate(end);
    Some(out)
}

/// The instances, one per user namespace that mounted the filesystem (`user_ns->binfmt_misc`):
/// the kernel's for the initial one, and one made by the first mount in any other.
pub(crate) struct Instances {
    init: Arc<BinfmtMisc>,
    init_ns: Arc<UserNs>,
    /// An instance goes with its namespace, which nothing can then exec in.
    others: Mutex<Vec<(Weak<UserNs>, Arc<BinfmtMisc>)>>,
}

impl Instances {
    pub(crate) fn new(init: Arc<BinfmtMisc>, init_ns: &Arc<UserNs>) -> Arc<Self> {
        Arc::new(Self {
            init,
            init_ns: init_ns.clone(),
            others: Mutex::default(),
        })
    }

    fn of(&self, ns: &Arc<UserNs>) -> Option<Arc<BinfmtMisc>> {
        if Arc::ptr_eq(ns, &self.init_ns) {
            return Some(self.init.clone());
        }
        let others = self.others.lock_or_recover();
        let found = others
            .iter()
            .find(|(at, _)| std::ptr::eq(at.as_ptr(), Arc::as_ptr(ns)));
        found.map(|(_, misc)| misc.clone())
    }

    /// `current_binfmt_misc`: the instance of `ns`, or of its nearest ancestor with one.
    pub(crate) fn current(&self, ns: &Arc<UserNs>) -> Arc<BinfmtMisc> {
        let mut at = Some(ns);
        while let Some(ns) = at {
            if let Some(misc) = self.of(ns) {
                return misc;
            }
            at = ns.parent();
        }
        self.init.clone()
    }

    /// `bm_fill_super`: the instance of `ns`, made now if it has none.
    fn get_or_make(&self, ns: &Arc<UserNs>) -> Arc<BinfmtMisc> {
        if let Some(misc) = self.of(ns) {
            return misc;
        }
        let mut others = self.others.lock_or_recover();
        others.retain(|(ns, _)| ns.strong_count() > 0);
        let init = &self.init;
        let misc = BinfmtMisc::new(init.clock.clone(), &init.tasks, &init.vfs);
        others.push((Arc::downgrade(ns), misc.clone()));
        misc
    }
}

/// The `binfmt_misc` filesystem type: a mount shows the instance of the mounter's user
/// namespace.
pub(crate) struct BinfmtMiscType(pub(crate) Arc<Instances>);

impl FilesystemType for BinfmtMiscType {
    fn mount(
        &self,
        _source: &[u8],
        params: &[FsParam<'_>],
        cred: &Cred,
    ) -> Result<Arc<dyn Filesystem>, Errno> {
        if !params.is_empty() {
            return Err(Errno::EINVAL);
        }
        let ns = kernel_user_ns(&cred.user_ns, &self.0.init_ns);
        let misc = self.0.get_or_make(&ns);
        // `simple_fill_super`'s inodes belong to the namespace's root (`i_uid_write(.., 0)`).
        let owner = (
            ns.make_kuid(0).map_or(u32::MAX, |k| k.0),
            ns.make_kgid(0).map_or(u32::MAX, |k| k.0),
        );
        Ok(MiscFs::new(misc, owner))
    }

    fn userns_mount(&self) -> bool {
        true
    }
}

/// One superblock of an instance.
struct MiscFs {
    misc: Arc<BinfmtMisc>,
    owner: (u32, u32),
    mounted: Timestamp,
}

impl MiscFs {
    fn new(misc: Arc<BinfmtMisc>, owner: (u32, u32)) -> Arc<Self> {
        // A first superblock turns the instance back on (`bm_fill_super`).
        if misc.supers.fetch_add(1, Ordering::AcqRel) == 0 {
            misc.enabled.store(true, Ordering::Release);
        }
        let mounted = misc.clock.now();
        Arc::new(Self {
            misc,
            owner,
            mounted,
        })
    }
}

impl Drop for MiscFs {
    /// `bm_kill_sb`, whose eviction of the format files removes the formats.
    fn drop(&mut self) {
        if self.misc.supers.fetch_sub(1, Ordering::AcqRel) == 1 {
            self.misc.entries.lock_or_recover().clear();
        }
    }
}

#[async_trait(?Send)]
impl Filesystem for MiscFs {
    // fs/binfmt_misc.c bm_fill_super: SB_I_NOEXEC.
    fn noexec(&self) -> bool {
        true
    }

    fn info(&self) -> FsInfo {
        FsInfo {
            name: "binfmt_misc",
            magic: BINFMTFS_MAGIC,
            name_max: NAME_MAX,
            read_only: false,
            dev: None,
            has_btime: false,
            stable: false,
        }
    }

    fn root(&self) -> Arc<dyn Node> {
        Arc::new(MiscNode {
            misc: self.misc.clone(),
            ino: ROOT_INO,
            owner: self.owner,
            mounted: self.mounted,
        })
    }

    async fn statfs(&self) -> FsResult<StatFs> {
        Ok(StatFs {
            block_size: 4096,
            name_max: NAME_MAX as u32,
            fragment_size: 4096,
            ..StatFs::default()
        })
    }
}

/// The root (1), `register` (2), `status` (3), or a format's file.
struct MiscNode {
    misc: Arc<BinfmtMisc>,
    ino: u64,
    /// The superblock's: who owns everything, and when it was made.
    owner: (u32, u32),
    mounted: Timestamp,
}

impl MiscNode {
    fn node(&self, ino: u64) -> Arc<dyn Node> {
        Arc::new(MiscNode {
            misc: self.misc.clone(),
            ino,
            owner: self.owner,
            mounted: self.mounted,
        })
    }
}

#[async_trait(?Send)]
impl Node for MiscNode {
    fn ino(&self) -> u64 {
        self.ino
    }

    fn file_type(&self) -> FileType {
        match self.ino {
            ROOT_INO => FileType::Directory,
            _ => FileType::Regular,
        }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    /// Everything is the namespace root's: the root `0755`, `register` `0200`, the rest `0644`.
    async fn getattr(&self) -> FsResult<Attr> {
        let (mode, nlink) = match self.ino {
            ROOT_INO => (0o755, 2),
            REGISTER_INO => (0o200, 1),
            STATUS_INO => (0o644, 1),
            ino if self.misc.entry(ino).is_some() => (0o644, 1),
            _ => (0o644, 0),
        };
        let time = self.mounted;
        Ok(Attr {
            ino: self.ino,
            file_type: self.file_type(),
            mode,
            nlink,
            uid: self.owner.0,
            gid: self.owner.1,
            rdev: Default::default(),
            size: 0,
            blocks: 0,
            blksize: 4096,
            atime: time,
            mtime: time,
            ctime: time,
            btime: None,
            dev: None,
        })
    }

    async fn setattr(&self, _attr: &SetAttr) -> FsResult<Attr> {
        Err(Errno::EPERM)
    }

    async fn lookup(&self, name: &[u8]) -> FsResult<Arc<dyn Node>> {
        if self.ino != ROOT_INO {
            return Err(Errno::ENOTDIR);
        }
        let ino = match name {
            b"register" => REGISTER_INO,
            b"status" => STATUS_INO,
            name => {
                let entries = self.misc.entries.lock_or_recover();
                let entry = entries.iter().find(|e| e.name == name);
                entry.ok_or(Errno::ENOENT)?.ino
            }
        };
        Ok(self.node(ino))
    }

    async fn create(&self, _: &[u8], _: &NewNode<'_>) -> FsResult<Arc<dyn Node>> {
        Err(Errno::EPERM)
    }

    async fn unlink(&self, _: &[u8]) -> FsResult<()> {
        Err(Errno::EPERM)
    }

    async fn open(&self, options: OpenOptions) -> FsResult<Box<dyn Handle>> {
        if self.ino != ROOT_INO && self.ino > STATUS_INO && self.misc.entry(self.ino).is_none() {
            return Err(Errno::ENOENT);
        }
        Ok(Box::new(MiscHandle {
            misc: self.misc.clone(),
            ino: self.ino,
            task: options.task,
        }))
    }
}

struct MiscHandle {
    misc: Arc<BinfmtMisc>,
    ino: u64,
    /// The opener, as which `register` resolves an `F` interpreter.
    task: u32,
}

impl MiscHandle {
    fn text(&self) -> FsResult<Vec<u8>> {
        match self.ino {
            STATUS_INO => Ok(match self.misc.enabled.load(Ordering::Acquire) {
                true => b"enabled\n".to_vec(),
                false => b"disabled\n".to_vec(),
            }),
            REGISTER_INO => Err(Errno::EINVAL),
            ino => Ok(self.misc.entry(ino).ok_or(Errno::ENOENT)?.show()),
        }
    }
}

#[async_trait(?Send)]
impl Handle for MiscHandle {
    async fn read_at(&self, buf: &mut [u8], offset: u64) -> FsResult<usize> {
        let text = self.text()?;
        let start = (offset as usize).min(text.len());
        let n = buf.len().min(text.len() - start);
        buf[..n].copy_from_slice(&text[start..start + n]);
        Ok(n)
    }

    async fn write_at(&self, data: &[u8], _offset: u64) -> FsResult<usize> {
        match self.ino {
            REGISTER_INO => self.misc.register(data, self.task).await,
            STATUS_INO => self.misc.command(data, None),
            ino => {
                let entry = self.misc.entry(ino).ok_or(Errno::ENOENT)?;
                self.misc.command(data, Some(&entry))
            }
        }
    }

    async fn append(&self, data: &[u8]) -> FsResult<(u64, usize)> {
        self.write_at(data, 0).await.map(|n| (0, n))
    }

    async fn set_size(&self, _size: u64) -> FsResult<()> {
        Ok(())
    }

    /// `register`, `status`, then the formats, oldest first.
    async fn readdir(&self, cookie: u64, sink: &mut dyn DirSink) -> FsResult<()> {
        if self.ino != ROOT_INO {
            return Err(Errno::ENOTDIR);
        }
        let mut names: Vec<(Vec<u8>, u64)> = vec![
            (b"register".to_vec(), REGISTER_INO),
            (b"status".to_vec(), STATUS_INO),
        ];
        let entries = self.misc.entries.lock_or_recover().clone();
        names.extend(entries.iter().rev().map(|e| (e.name.clone(), e.ino)));
        for (name, ino) in names.into_iter().filter(|(_, ino)| *ino > cookie) {
            let entry = DirEntry {
                name: &name,
                ino,
                file_type: FileType::Regular,
                next: ino,
            };
            if !sink.push(entry) {
                break;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registrations_parse_as_linux_does() {
        let e = create_entry(
            b":qemu-arm:M:0:\\x7fELF\\x01:\\xff\\xff\\xff\\xff\\xfe:/bin/qemu:PF\n",
            9,
        )
        .unwrap();
        assert_eq!(e.name, b"qemu-arm");
        assert_eq!(e.flags, PRESERVE_ARGV0 | OPEN_FILE);
        assert!(e.matches(b"\x7fELF\x00rest", b"/x"));
        assert!(!e.matches(b"\x7fELF\x02rest", b"/x"));
        let shown = "enabled\ninterpreter /bin/qemu\nflags: PF\noffset 0\nmagic 7f454c4601\nmask fffffffffe\n";
        assert_eq!(String::from_utf8(e.show()).unwrap(), shown);
        let ext = create_entry(b"|py|E||py||/usr/bin/python3|D", 10).unwrap();
        assert!(!ext.enabled.load(Ordering::Relaxed));
        assert!(ext.matches(b"", b"/srv/run.py"));
        assert!(!ext.matches(b"", b"/srv.py/run"));
        // Too short, a bad delimiter, a name with a slash, an unknown type or flag, a mask of
        // another length, a magic past the header.
        for bad in [
            &b":a:M::x::/i"[..],
            b"\\a\\M\\\\x\\\\/i\\",
            b":a/b:M::x::/i:",
            b":ab:Q::x::/i:",
            b":ab:M::x::/i:Q",
            b":ab:M::xy:\\xff:/i:",
            b":ab:M:255:xy::/i:",
            // `B` needs BPF; `T` keeps argv[0] already; `L` takes no invocation flags; `C`
            // and `L` open the interpreter by an absolute name unless `F` pins it.
            b":ab:B::::h:",
            b":ab:M::x::/i:TP",
            b":ab:M::x::/i:LO",
            b":ab:M::x::i:C",
            b":ab:M::x::i:L",
        ] {
            assert!(
                create_entry(bad, 1).is_err(),
                "{:?}",
                String::from_utf8_lossy(bad)
            );
        }
    }

    #[test]
    fn flags_imply_and_show_as_linux_does() {
        // `C` and `T` imply `O`; the letters show in `misc_flags` order; `D` is consumed.
        let c = create_entry(b":c:M::x::/i:CD", 1).unwrap();
        assert_eq!(c.flags, CREDENTIALS | OPEN_BINARY);
        assert!(!c.enabled.load(Ordering::Relaxed));
        assert!(
            String::from_utf8(c.show())
                .unwrap()
                .contains("\nflags: OC\n")
        );
        let t = create_entry(b":t:E::t::/i:T", 2).unwrap();
        assert_eq!(t.flags, TRANSPARENT | OPEN_BINARY);
        let l = create_entry(b":l:E::l::rel:LF", 3).unwrap();
        assert_eq!(l.flags, LOADER | OPEN_FILE);
        assert!(
            String::from_utf8(l.show())
                .unwrap()
                .contains("\nflags: FL\n")
        );
    }
}
