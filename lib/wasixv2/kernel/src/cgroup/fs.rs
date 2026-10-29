//! The `cgroup2` filesystem: a kernfs view of the hierarchy (`kernel/cgroup/cgroup.c`'s
//! `cgroup_kf_syscall_ops` and `cgroup_base_files`, `pids_files`, `memory_files`).
//!
//! Every directory is a cgroup; `mkdir` makes one and `rmdir` removes it. Each holds the core
//! files and those of the controllers its parent enabled for it, computed from the hierarchy
//! when looked up. Ownership and modes are kept per node, so a cgroup is delegated by
//! `chown`ing its directory and `cgroup.procs`, `cgroup.threads` and `cgroup.subtree_control`.
//! A mount shows the subtree of the mounter's cgroup namespace root (`cgroup_do_get_tree`).
//!
//! A file's contents are made when a read starts at offset 0, as `seq_file` does. A write acts
//! with what the opener had (`of->file->f_cred`, the cgroup namespace of `cgroup_file_open`),
//! and names processes in the opener's pid namespace.

use std::{
    any::Any,
    sync::{Arc, Mutex, Weak},
};

use async_trait::async_trait;
use wasixv2_syscalls::Errno;
use wasixv2_vfs::{
    Attr, DirEntry, DirSink, FileType, Filesystem, FsInfo, FsResult, Handle, NAME_MAX, NewKind,
    NewNode, Node, OpenOptions, RenameMode, SetAttr, StatFs, TimeSet,
};

use super::{Cgroup, Hierarchy, Kind, MEMORY, NodeAttr, PIDS, PIDS_MAX, UNLIMITED, tasks};
use crate::{TaskTable, sync::MutexExt};

/// `CGROUP2_SUPER_MAGIC`.
pub(crate) const CGROUP2_SUPER_MAGIC: u32 = 0x6367_7270;

/// Where a file may be (`CFTYPE_NOT_ON_ROOT`, `CFTYPE_ONLY_ON_ROOT`).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Place {
    Everywhere,
    NotOnRoot,
}

/// A control file (`struct cftype`).
pub(crate) struct Cftype {
    pub(crate) name: &'static str,
    /// The controller it belongs to; 0 for a core file.
    ss: u16,
    place: Place,
    /// Writable inside a delegated cgroup namespace at its root (`CFTYPE_NS_DELEGATABLE`).
    delegatable: bool,
    read: bool,
    write: bool,
}

impl Cftype {
    /// `cgroup_file_mode`: readable by all if it reads, writable by its owner if it writes.
    pub(crate) fn mode(&self) -> u32 {
        (if self.read { 0o444 } else { 0 }) | (if self.write { 0o200 } else { 0 })
    }
}

const fn cft(name: &'static str, ss: u16, place: Place, read: bool, write: bool) -> Cftype {
    Cftype {
        name,
        ss,
        place,
        delegatable: false,
        read,
        write,
    }
}

const fn delegatable(name: &'static str) -> Cftype {
    Cftype {
        name,
        ss: 0,
        place: Place::Everywhere,
        delegatable: true,
        read: true,
        write: true,
    }
}

use Place::{Everywhere, NotOnRoot};

/// Every file a cgroup may have, sorted by name.
pub(crate) const FILES: &[Cftype] = &[
    cft("cgroup.controllers", 0, Everywhere, true, false),
    cft("cgroup.events", 0, NotOnRoot, true, false),
    cft("cgroup.freeze", 0, NotOnRoot, true, true),
    cft("cgroup.kill", 0, NotOnRoot, false, true),
    cft("cgroup.max.depth", 0, Everywhere, true, true),
    cft("cgroup.max.descendants", 0, Everywhere, true, true),
    delegatable("cgroup.procs"),
    cft("cgroup.stat", 0, Everywhere, true, false),
    delegatable("cgroup.subtree_control"),
    delegatable("cgroup.threads"),
    cft("cgroup.type", 0, NotOnRoot, true, true),
    cft("cpu.stat", 0, Everywhere, true, false),
    cft("memory.current", MEMORY, NotOnRoot, true, false),
    cft("memory.events", MEMORY, NotOnRoot, true, false),
    cft("memory.events.local", MEMORY, NotOnRoot, true, false),
    cft("memory.high", MEMORY, NotOnRoot, true, true),
    cft("memory.low", MEMORY, NotOnRoot, true, true),
    cft("memory.max", MEMORY, NotOnRoot, true, true),
    cft("memory.min", MEMORY, NotOnRoot, true, true),
    cft("memory.oom.group", MEMORY, NotOnRoot, true, true),
    cft("memory.peak", MEMORY, NotOnRoot, true, false),
    cft("memory.stat", MEMORY, Everywhere, true, false),
    cft("pids.current", PIDS, NotOnRoot, true, false),
    cft("pids.events", PIDS, NotOnRoot, true, false),
    cft("pids.events.local", PIDS, NotOnRoot, true, false),
    cft("pids.max", PIDS, NotOnRoot, true, true),
    cft("pids.peak", PIDS, NotOnRoot, true, false),
];

/// The index of the file `name` in [`FILES`].
pub(crate) fn file_index_of(name: &[u8]) -> Option<usize> {
    FILES.iter().position(|f| f.name.as_bytes() == name)
}

pub(crate) fn file_index(name: &str) -> usize {
    file_index_of(name.as_bytes()).expect("a known cgroup file")
}

/// A mount of the hierarchy, rooted at `root`.
pub(crate) struct CgroupFs {
    source: Arc<Source>,
}

pub(crate) struct Source {
    pub(crate) hierarchy: Arc<Hierarchy>,
    root: Arc<Cgroup>,
    pub(crate) state: Weak<TaskTable>,
}

impl CgroupFs {
    pub(crate) fn new(
        hierarchy: Arc<Hierarchy>,
        root: Arc<Cgroup>,
        state: Weak<TaskTable>,
    ) -> Self {
        hierarchy.mark_visible();
        Self {
            source: Arc::new(Source {
                hierarchy,
                root,
                state,
            }),
        }
    }
}

/// The cgroup a `cgroup2` directory stands for, if `node` is one (`cgroup_get_from_file`).
pub(crate) fn cgroup_of(node: &dyn Node) -> Option<Arc<Cgroup>> {
    let dir: &DirNode = node.as_any().downcast_ref()?;
    Some(dir.cgroup.clone())
}

#[async_trait(?Send)]
impl Filesystem for CgroupFs {
    fn info(&self) -> FsInfo {
        FsInfo {
            name: "cgroup2",
            magic: CGROUP2_SUPER_MAGIC,
            name_max: NAME_MAX,
            read_only: false,
            dev: None,
            has_btime: false,
            // Controllers come and go with `cgroup.subtree_control`.
            stable: false,
        }
    }

    fn root(&self) -> Arc<dyn Node> {
        Arc::new(DirNode {
            source: self.source.clone(),
            cgroup: self.source.root.clone(),
        })
    }

    /// `simple_statfs`, as kernfs answers.
    async fn statfs(&self) -> FsResult<StatFs> {
        Ok(StatFs {
            block_size: 4096,
            name_max: NAME_MAX as u32,
            fragment_size: 4096,
            ..StatFs::default()
        })
    }

    fn show_options(&self, out: &mut Vec<u8>) {
        self.source.hierarchy.lock().flags.show(out);
    }
}

fn attr(ino: u64, file_type: FileType, node: NodeAttr, nlink: u32) -> Attr {
    Attr {
        ino,
        file_type,
        mode: node.mode,
        nlink,
        uid: node.uid,
        gid: node.gid,
        rdev: Default::default(),
        size: 0,
        blocks: 0,
        blksize: 4096,
        atime: node.atime,
        mtime: node.mtime,
        ctime: node.ctime,
        btime: None,
        dev: None,
    }
}

/// `kernfs_iop_setattr`: mode, owner and times are kept.
fn set_node_attr(node: &mut NodeAttr, set: &SetAttr, now: wasixv2_vfs::Timestamp) {
    let time = |t: TimeSet| match t {
        TimeSet::At(t) => t,
        TimeSet::Now => now,
    };
    if let Some(mode) = set.mode {
        node.mode = mode & 0o7777;
    }
    if let Some(uid) = set.uid {
        node.uid = uid;
    }
    if let Some(gid) = set.gid {
        node.gid = gid;
    }
    if let Some(t) = set.atime {
        node.atime = time(t);
    }
    if let Some(t) = set.mtime {
        node.mtime = time(t);
    }
    if let Some(t) = set.ctime {
        node.ctime = time(t);
    }
}

/// A cgroup's directory.
struct DirNode {
    source: Arc<Source>,
    cgroup: Arc<Cgroup>,
}

impl DirNode {
    /// The files `cgroup` has now (`css_populate_dir`).
    fn files(&self) -> Vec<usize> {
        let tree = self.source.hierarchy.lock();
        let root = self.cgroup.parent.is_none();
        FILES
            .iter()
            .enumerate()
            .filter(|(_, f)| !(root && f.place == NotOnRoot))
            .filter(|(_, f)| f.ss == 0 || tree.has_css(&self.cgroup, f.ss))
            .map(|(i, _)| i)
            .collect()
    }

    fn live(&self) -> FsResult<()> {
        match self.source.hierarchy.lock().is_dead(&self.cgroup) {
            true => Err(Errno::ENOENT),
            false => Ok(()),
        }
    }
}

#[async_trait(?Send)]
impl Node for DirNode {
    fn ino(&self) -> u64 {
        self.cgroup.id
    }

    fn file_type(&self) -> FileType {
        FileType::Directory
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    async fn getattr(&self) -> FsResult<Attr> {
        let tree = self.source.hierarchy.lock();
        let nlink = 2 + tree.nr_children(&self.cgroup) as u32;
        Ok(attr(
            self.cgroup.id,
            FileType::Directory,
            tree.dir_attr(&self.cgroup),
            nlink,
        ))
    }

    async fn setattr(&self, set: &SetAttr) -> FsResult<Attr> {
        {
            let mut tree = self.source.hierarchy.lock();
            let now = tree.now;
            set_node_attr(tree.dir_attr_mut(&self.cgroup), set, now);
        }
        self.getattr().await
    }

    async fn lookup(&self, name: &[u8]) -> FsResult<Arc<dyn Node>> {
        if let Some(file) = file_index_of(name).filter(|f| self.files().contains(f)) {
            return Ok(Arc::new(FileNode {
                source: self.source.clone(),
                cgroup: self.cgroup.clone(),
                file,
            }));
        }
        let child = self.source.hierarchy.lock().child(&self.cgroup, name);
        let cgroup = child.ok_or(Errno::ENOENT)?;
        Ok(Arc::new(DirNode {
            source: self.source.clone(),
            cgroup,
        }))
    }

    /// `kernfs_iop_mkdir` → `cgroup_mkdir`; nothing else is made here (`EPERM`, as kernfs has
    /// no `create`, `symlink` or `mknod`).
    async fn create(&self, name: &[u8], new: &NewNode<'_>) -> FsResult<Arc<dyn Node>> {
        if !matches!(new.kind, NewKind::Directory) {
            return Err(Errno::EPERM);
        }
        let hierarchy = &self.source.hierarchy;
        let cgroup = hierarchy.mkdir(&self.cgroup, name, new.mode, (new.uid, new.gid))?;
        Ok(Arc::new(DirNode {
            source: self.source.clone(),
            cgroup,
        }))
    }

    async fn link(&self, _: &[u8], _: &Arc<dyn Node>) -> FsResult<()> {
        Err(Errno::EPERM)
    }

    async fn unlink(&self, _: &[u8]) -> FsResult<()> {
        Err(Errno::EPERM)
    }

    /// `cgroup_rmdir`.
    async fn rmdir(&self, name: &[u8]) -> FsResult<()> {
        if file_index_of(name).is_some() {
            return Err(Errno::ENOTDIR);
        }
        self.source.hierarchy.rmdir(&self.cgroup, name)
    }

    /// `cgroup_kf_syscall_ops` has no `rename`.
    async fn rename(&self, _: &[u8], _: &Arc<dyn Node>, _: &[u8], _: RenameMode) -> FsResult<()> {
        Err(Errno::EPERM)
    }

    async fn open(&self, _: OpenOptions) -> FsResult<Box<dyn Handle>> {
        self.live()?;
        Ok(Box::new(DirHandle(DirNode {
            source: self.source.clone(),
            cgroup: self.cgroup.clone(),
        })))
    }
}

/// An open directory, listed afresh on every read: its files, then its children.
struct DirHandle(DirNode);

#[async_trait(?Send)]
impl Handle for DirHandle {
    async fn readdir(&self, cookie: u64, sink: &mut dyn DirSink) -> FsResult<()> {
        let dir = &self.0;
        let mut entries: Vec<(Vec<u8>, u64, FileType)> = dir
            .files()
            .into_iter()
            .map(|f| {
                let ino = Hierarchy::file_ino(&dir.cgroup, f);
                (FILES[f].name.as_bytes().to_vec(), ino, FileType::Regular)
            })
            .collect();
        let children = dir.source.hierarchy.lock().children(&dir.cgroup);
        entries.extend(
            children
                .into_iter()
                .map(|(name, child)| (name, child.id, FileType::Directory)),
        );
        for (index, (name, ino, file_type)) in entries.iter().enumerate().skip(cookie as usize) {
            let entry = DirEntry {
                name,
                ino: *ino,
                file_type: *file_type,
                next: index as u64 + 1,
            };
            if !sink.push(entry) {
                break;
            }
        }
        Ok(())
    }
}

/// A control file of a cgroup.
struct FileNode {
    source: Arc<Source>,
    cgroup: Arc<Cgroup>,
    file: usize,
}

#[async_trait(?Send)]
impl Node for FileNode {
    fn ino(&self) -> u64 {
        Hierarchy::file_ino(&self.cgroup, self.file)
    }

    fn file_type(&self) -> FileType {
        FileType::Regular
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    async fn getattr(&self) -> FsResult<Attr> {
        let tree = self.source.hierarchy.lock();
        let node = tree.file_attr(&self.cgroup, self.file);
        Ok(attr(self.ino(), FileType::Regular, node, 1))
    }

    async fn setattr(&self, set: &SetAttr) -> FsResult<Attr> {
        if set.size.is_some_and(|size| size != 0) {
            return Err(Errno::EINVAL);
        }
        {
            let mut tree = self.source.hierarchy.lock();
            let now = tree.now;
            set_node_attr(tree.file_attr_mut(&self.cgroup, self.file), set, now);
        }
        self.getattr().await
    }

    /// `cgroup_file_open`: the opener's credentials and namespaces go with the file.
    async fn open(&self, options: OpenOptions) -> FsResult<Box<dyn Handle>> {
        if self.source.hierarchy.lock().is_dead(&self.cgroup) {
            return Err(Errno::ENODEV);
        }
        let opener = tasks::Opener::of(&self.source, options.task);
        Ok(Box::new(FileHandle {
            source: self.source.clone(),
            cgroup: self.cgroup.clone(),
            file: self.file,
            opener,
            contents: Mutex::default(),
        }))
    }
}

/// An open control file.
struct FileHandle {
    source: Arc<Source>,
    cgroup: Arc<Cgroup>,
    file: usize,
    opener: tasks::Opener,
    /// What the last read from offset 0 made (`seq_file`'s buffer).
    contents: Mutex<Vec<u8>>,
}

#[async_trait(?Send)]
impl Handle for FileHandle {
    async fn read_at(&self, buf: &mut [u8], offset: u64) -> FsResult<usize> {
        if !FILES[self.file].read {
            return Err(Errno::EINVAL);
        }
        let mut contents = self.contents.lock_or_recover();
        if offset == 0 {
            *contents = self.show()?.into_bytes();
        }
        let start = usize::try_from(offset)
            .unwrap_or(usize::MAX)
            .min(contents.len());
        let n = buf.len().min(contents.len() - start);
        buf[..n].copy_from_slice(&contents[start..start + n]);
        Ok(n)
    }

    /// `cgroup_file_write`: the whole buffer is one request, wherever it is written.
    async fn write_at(&self, data: &[u8], _offset: u64) -> FsResult<usize> {
        let cft = &FILES[self.file];
        if !cft.write {
            return Err(Errno::EINVAL);
        }
        // With `nsdelegate`, a namespace's root cgroup is configured from outside it.
        let nsdelegate = self.source.hierarchy.lock().flags.nsdelegate;
        if nsdelegate
            && !cft.delegatable
            && !self.opener.ns_is_init
            && self.opener.ns_root.id == self.cgroup.id
        {
            return Err(Errno::EPERM);
        }
        let text = std::str::from_utf8(data).map_err(|_| Errno::EINVAL)?;
        self.store(text.trim())?;
        Ok(data.len())
    }

    async fn set_size(&self, _size: u64) -> FsResult<()> {
        Ok(())
    }
}

/// `kstrtoint`/`kstrtoull` with base 0: decimal, `0x` hexadecimal or `0` octal.
fn parse_int(text: &str) -> Result<u64, Errno> {
    let text = text.strip_prefix('+').unwrap_or(text);
    let (digits, radix) =
        if let Some(hex) = text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")) {
            (hex, 16)
        } else if text.len() > 1 && text.starts_with('0') {
            (&text[1..], 8)
        } else {
            (text, 10)
        };
    if digits.is_empty() {
        return Err(Errno::EINVAL);
    }
    u64::from_str_radix(digits, radix).map_err(|e| match e.kind() {
        std::num::IntErrorKind::PosOverflow => Errno::ERANGE,
        _ => Errno::EINVAL,
    })
}

/// A signed `kstrtoint`: `ERANGE` past `i32`.
fn parse_i32(text: &str) -> Result<i64, Errno> {
    let (negative, digits) = match text.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, text),
    };
    let value = parse_int(digits)?;
    let value = i64::try_from(value).map_err(|_| Errno::ERANGE)?;
    let value = if negative { -value } else { value };
    if value > i64::from(i32::MAX) || value < i64::from(i32::MIN) {
        return Err(Errno::ERANGE);
    }
    Ok(value)
}

/// `"max"` or a `cgroup.max.*` count.
fn parse_limit(text: &str) -> Result<u32, Errno> {
    if text == "max" {
        return Ok(UNLIMITED);
    }
    let value = parse_i32(text)?;
    u32::try_from(value).map_err(|_| Errno::ERANGE)
}

/// `memparse` with `page_counter_memparse`'s `max`: bytes with an optional K, M, G, T, P or
/// E suffix, rounded down to pages; `None` for `max`.
fn parse_memory(text: &str) -> Result<Option<u64>, Errno> {
    if text == "max" {
        return Ok(None);
    }
    let (number, shift) = match text.chars().last() {
        Some(c @ ('k' | 'K' | 'm' | 'M' | 'g' | 'G' | 't' | 'T' | 'p' | 'P' | 'e' | 'E')) => {
            let shift = match c.to_ascii_uppercase() {
                'K' => 10,
                'M' => 20,
                'G' => 30,
                'T' => 40,
                'P' => 50,
                _ => 60,
            };
            (&text[..text.len() - 1], shift)
        }
        _ => (text, 0),
    };
    let bytes = parse_int(number)?.checked_shl(shift).ok_or(Errno::ERANGE)?;
    let page = crate::mm::PAGE_SIZE;
    Ok(Some(bytes / page * page))
}

fn show_limit(limit: Option<u64>) -> String {
    limit.map_or_else(|| "max\n".into(), |limit| format!("{limit}\n"))
}

impl FileHandle {
    fn state(&self) -> FsResult<Arc<TaskTable>> {
        self.source.state.upgrade().ok_or(Errno::ENODEV)
    }

    /// The file's contents (its `seq_show`).
    fn show(&self) -> FsResult<String> {
        let cgroup = &self.cgroup;
        let name = FILES[self.file].name;
        match name {
            "cgroup.procs" | "cgroup.threads" => {
                let state = self.state()?;
                let threads = name == "cgroup.threads";
                let tree = self.source.hierarchy.lock();
                // `cgroup_procs_start`: a threaded cgroup has no processes of its own.
                if !threads && tree.is_threaded(cgroup) {
                    return Err(Errno::EOPNOTSUPP);
                }
                drop(tree);
                let ids = tasks::members(&state, &self.source.hierarchy, cgroup, threads);
                Ok(ids
                    .into_iter()
                    .map(|id| format!("{}\n", self.opener.pid_nr(&state, id)))
                    .collect())
            }
            "cpu.stat" => {
                let cpu = match self.source.state.upgrade() {
                    Some(state) => tasks::cpu(&state, &self.source.hierarchy, cgroup),
                    None => Default::default(),
                };
                let usec = |d: std::time::Duration| d.as_micros();
                Ok(format!(
                    "usage_usec {}\nuser_usec {}\nsystem_usec {}\nnice_usec {}\n",
                    usec(cpu.times.total()),
                    usec(cpu.times.user),
                    usec(cpu.times.system),
                    usec(cpu.nice),
                ))
            }
            "memory.current" | "memory.stat" | "memory.peak" => {
                let current = match self.source.state.upgrade() {
                    Some(state) => tasks::memory_current(&state, &self.source.hierarchy, cgroup),
                    None => 0,
                };
                let peak = self.source.hierarchy.lock().memory_peak(cgroup, current);
                Ok(match name {
                    "memory.current" => format!("{current}\n"),
                    "memory.peak" => format!("{peak}\n"),
                    _ => memory_stat(current),
                })
            }
            _ => Ok(self.show_tree(name)),
        }
    }

    /// The files whose contents are the hierarchy's alone.
    fn show_tree(&self, name: &str) -> String {
        let cgroup = &self.cgroup;
        let tree = self.source.hierarchy.lock();
        match name {
            "cgroup.controllers" => line(super::mask_names(tree.control(cgroup))),
            "cgroup.subtree_control" => line(super::mask_names(tree.subtree_control(cgroup))),
            "cgroup.events" => format!(
                "populated {}\nfrozen {}\n",
                u8::from(tree.populated(cgroup)),
                u8::from(tree.frozen(cgroup)),
            ),
            "cgroup.freeze" => format!("{}\n", u8::from(tree.frozen(cgroup))),
            "cgroup.max.depth" => show_count(tree.max_depth(cgroup)),
            "cgroup.max.descendants" => show_count(tree.max_descendants(cgroup)),
            "cgroup.stat" => {
                let stat = tree.stat(cgroup);
                let mut out = format!("nr_descendants {}\n", stat.nr_descendants);
                for (name, n) in super::controller_names().zip(stat.subsys) {
                    out += &format!("nr_subsys_{name} {n}\n");
                }
                out += &format!("nr_dying_descendants {}\n", stat.nr_dying_descendants);
                // Nothing lingers once removed: no controller state is dying.
                for name in super::controller_names() {
                    out += &format!("nr_dying_subsys_{name} 0\n");
                }
                out
            }
            "cgroup.type" => match tree.kind(cgroup) {
                Kind::Domain => "domain\n",
                Kind::DomainThreaded => "domain threaded\n",
                Kind::DomainInvalid => "domain invalid\n",
                Kind::Threaded => "threaded\n",
            }
            .into(),
            "memory.events" | "memory.events.local" => {
                // No limit is enforced, so none was ever hit.
                "low 0\nhigh 0\nmax 0\noom 0\noom_kill 0\noom_group_kill 0\nsock_throttled 0\n"
                    .into()
            }
            "memory.high" => show_limit(tree.memory(cgroup).high),
            "memory.max" => show_limit(tree.memory(cgroup).max),
            "memory.low" => format!("{}\n", tree.memory(cgroup).low),
            "memory.min" => format!("{}\n", tree.memory(cgroup).min),
            "memory.oom.group" => format!("{}\n", u8::from(tree.memory(cgroup).oom_group)),
            "pids.current" => format!("{}\n", tree.pids_current(cgroup)),
            "pids.peak" => format!("{}\n", tree.pids_peak(cgroup)),
            "pids.max" => show_limit(tree.pids_max(cgroup)),
            "pids.events" => format!("max {}\n", tree.pids_events(cgroup, false)),
            "pids.events.local" => format!("max {}\n", tree.pids_events(cgroup, true)),
            _ => String::new(),
        }
    }

    /// The file's `write` handler.
    fn store(&self, text: &str) -> FsResult<()> {
        let cgroup = &self.cgroup;
        let name = FILES[self.file].name;
        let hierarchy = &self.source.hierarchy;
        match name {
            "cgroup.procs" | "cgroup.threads" => {
                let state = self.state()?;
                let threadgroup = name == "cgroup.procs";
                tasks::attach(
                    &state,
                    &self.source,
                    &self.opener,
                    cgroup,
                    text,
                    threadgroup,
                )
            }
            "cgroup.kill" => {
                if parse_i32(text)? != 1 {
                    return Err(Errno::ERANGE);
                }
                let state = self.state()?;
                if hierarchy.lock().is_threaded(cgroup) {
                    return Err(Errno::EOPNOTSUPP);
                }
                tasks::kill(&state, hierarchy, cgroup);
                Ok(())
            }
            "cgroup.freeze" => {
                let freeze = match parse_i32(text)? {
                    0 => false,
                    1 => true,
                    _ => return Err(Errno::ERANGE),
                };
                // Tasks cannot be held where they are yet: only thawing is possible.
                if freeze {
                    return Err(Errno::EOPNOTSUPP);
                }
                hierarchy.lock().set_freeze(cgroup, false);
                Ok(())
            }
            "cgroup.subtree_control" => {
                let (mut enable, mut disable) = (0, 0);
                for token in text.split(' ').filter(|t| !t.is_empty()) {
                    let (sign, ss) = token.split_at(1);
                    let ss = super::controller(ss.as_bytes()).ok_or(Errno::EINVAL)?;
                    match sign {
                        "+" => (enable, disable) = (enable | ss, disable & !ss),
                        "-" => (enable, disable) = (enable & !ss, disable | ss),
                        _ => return Err(Errno::EINVAL),
                    }
                }
                let mut tree = hierarchy.lock();
                if tree.is_dead(cgroup) {
                    return Err(Errno::ENODEV);
                }
                tree.set_subtree_control(cgroup, enable, disable)
            }
            "cgroup.type" => {
                if text != "threaded" {
                    return Err(Errno::EINVAL);
                }
                hierarchy.lock().enable_threaded(cgroup)
            }
            "cgroup.max.depth" => {
                let depth = parse_limit(text)?;
                hierarchy.lock().set_max_depth(cgroup, depth);
                Ok(())
            }
            "cgroup.max.descendants" => {
                let descendants = parse_limit(text)?;
                hierarchy.lock().set_max_descendants(cgroup, descendants);
                Ok(())
            }
            "pids.max" => {
                let limit = match text {
                    "max" => None,
                    text => {
                        let limit = parse_i32(text).or_else(|_| {
                            parse_int(text).map(|v| i64::try_from(v).unwrap_or(i64::MAX))
                        })?;
                        if limit < 0 || limit as u64 >= PIDS_MAX {
                            return Err(Errno::EINVAL);
                        }
                        Some(limit as u64)
                    }
                };
                hierarchy.lock().set_pids_max(cgroup, limit);
                Ok(())
            }
            "memory.high" | "memory.max" => {
                let limit = parse_memory(text)?;
                hierarchy.lock().set_memory(cgroup, |memory| match name {
                    "memory.high" => memory.high = limit,
                    _ => memory.max = limit,
                });
                Ok(())
            }
            "memory.low" | "memory.min" => {
                let value = parse_memory(text)?.unwrap_or(u64::MAX);
                hierarchy.lock().set_memory(cgroup, |memory| match name {
                    "memory.low" => memory.low = value,
                    _ => memory.min = value,
                });
                Ok(())
            }
            "memory.oom.group" => {
                let group = match parse_i32(text)? {
                    0 => false,
                    1 => true,
                    _ => return Err(Errno::EINVAL),
                };
                hierarchy
                    .lock()
                    .set_memory(cgroup, |memory| memory.oom_group = group);
                Ok(())
            }
            _ => Err(Errno::EINVAL),
        }
    }
}

/// A line of space-separated names; nothing at all for none (`cgroup_print_ss_mask`).
fn line(names: String) -> String {
    match names.is_empty() {
        true => names,
        false => names + "\n",
    }
}

fn show_count(count: u32) -> String {
    match count {
        UNLIMITED => "max\n".into(),
        count => format!("{count}\n"),
    }
}

/// `memory.stat`: the subtree's address spaces are anonymous memory, all of it active; there
/// is no page cache, kernel memory or reclaim to report.
fn memory_stat(current: u64) -> String {
    let mut out = String::new();
    for (name, value) in [
        ("anon", current),
        ("file", 0),
        ("kernel", 0),
        ("kernel_stack", 0),
        ("pagetables", 0),
        ("sec_pagetables", 0),
        ("percpu", 0),
        ("sock", 0),
        ("vmalloc", 0),
        ("shmem", 0),
        ("file_mapped", 0),
        ("file_dirty", 0),
        ("file_writeback", 0),
        ("swapcached", 0),
        ("anon_thp", 0),
        ("file_thp", 0),
        ("shmem_thp", 0),
        ("inactive_anon", 0),
        ("active_anon", current),
        ("inactive_file", 0),
        ("active_file", 0),
        ("unevictable", 0),
        ("slab_reclaimable", 0),
        ("slab_unreclaimable", 0),
        ("slab", 0),
        ("pgfault", 0),
        ("pgmajfault", 0),
    ] {
        out += &format!("{name} {value}\n");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integers_parse_as_kstrtoint_does() {
        assert_eq!(parse_int("10"), Ok(10));
        assert_eq!(parse_int("0x10"), Ok(16));
        assert_eq!(parse_int("010"), Ok(8));
        assert_eq!(parse_int(""), Err(Errno::EINVAL));
        assert_eq!(parse_i32("-1"), Ok(-1));
        assert_eq!(parse_i32("4294967296"), Err(Errno::ERANGE));
        assert_eq!(parse_limit("max"), Ok(UNLIMITED));
        assert_eq!(parse_limit("-1"), Err(Errno::ERANGE));
    }

    #[test]
    fn memory_sizes_take_suffixes_and_round_to_pages() {
        assert_eq!(parse_memory("max"), Ok(None));
        assert_eq!(parse_memory("1M"), Ok(Some(1 << 20)));
        let page = crate::mm::PAGE_SIZE;
        assert_eq!(parse_memory(&(page + 1).to_string()), Ok(Some(page)));
        assert_eq!(parse_memory("x"), Err(Errno::EINVAL));
    }

    #[test]
    fn the_files_are_sorted_and_unique() {
        assert!(FILES.windows(2).all(|w| w[0].name < w[1].name));
    }
}
