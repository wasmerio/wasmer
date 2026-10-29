//! binfmt_misc (`fs/binfmt_misc.c`): formats of executables registered at run time, by a
//! magic number at an offset of the file or by its name's extension, each run by an
//! interpreter of its own. `execve` asks before it looks for a `#!` line (Linux inserts
//! binfmt_misc at the head of its formats); a match hands the file to the interpreter with the
//! file's name as its first argument, as a script hands over.
//!
//! The registry is the `binfmt_misc` filesystem, usually mounted at
//! `/proc/sys/fs/binfmt_misc`: `register` takes `:name:type:offset:magic:mask:interpreter:flags`
//! (types `M` and `E`; flags `P` to keep `argv[0]`, `F` to resolve the interpreter at
//! registration, `D` to register disabled), `status` turns the whole registry on and off,
//! and each format has a file of its own that shows it and takes `1`, `0` and `-1` (enable,
//! disable, remove). There is one instance per kernel, mounted in the initial user namespace.
//!
//! Not provided: `O` and `C`, which pass the binary as an open descriptor (`AT_EXECFD`) and run
//! with its credentials, and the newer `T`, `L` and `B` registrations (`EINVAL`).

use std::{
    any::Any,
    sync::{
        Arc, Mutex, Weak,
        atomic::{AtomicBool, AtomicU64, Ordering},
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
    sync::MutexExt,
};

/// `BINFMTFS_MAGIC`.
const BINFMTFS_MAGIC: u32 = 0x4249_4e4d;
/// `MAX_REGISTER_LENGTH`.
const MAX_REGISTER_LENGTH: usize = 1920;
const ROOT_INO: u64 = 1;
const REGISTER_INO: u64 = 2;
const STATUS_INO: u64 = 3;

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
    /// `P`: `argv[0]` stays after the file's name.
    preserve_argv0: bool,
    /// `F`: the interpreter as resolved at registration.
    interp_file: Option<Path>,
    /// The flags as given, for the entry's file.
    flags: String,
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
        out.extend_from_slice(format!("\nflags: {}\n", self.flags).as_bytes());
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

/// What `execve` does with a file a format took: the interpreter to open, and the arguments.
pub(crate) struct Handover {
    pub(crate) interpreter: Vec<u8>,
    pub(crate) interp_file: Option<Path>,
    pub(crate) preserve_argv0: bool,
}

/// The registry, and the filesystem that shows it.
pub(crate) struct BinfmtMisc {
    enabled: AtomicBool,
    /// Newest first, the order they are tried in.
    entries: Mutex<Vec<Arc<Entry>>>,
    next_ino: AtomicU64,
    mounted: Timestamp,
    /// Who writes a registration, and where its interpreter is (`F`).
    tasks: Weak<TaskTable>,
    vfs: Weak<Vfs>,
}

impl BinfmtMisc {
    pub(crate) fn new(clock: Arc<dyn Clock>, tasks: &Arc<TaskTable>, vfs: &Arc<Vfs>) -> Arc<Self> {
        Arc::new(Self {
            enabled: AtomicBool::new(true),
            entries: Mutex::default(),
            next_ino: AtomicU64::new(STATUS_INO + 1),
            mounted: clock.now(),
            tasks: Arc::downgrade(tasks),
            vfs: Arc::downgrade(vfs),
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
            preserve_argv0: entry.preserve_argv0,
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
        if entry.flags.contains('F') {
            entry.interp_file = Some(self.resolve_interpreter(&entry.interpreter, task).await?);
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
    async fn resolve_interpreter(&self, path: &[u8], task: u32) -> FsResult<Path> {
        let (tasks, vfs) = match (self.tasks.upgrade(), self.vfs.upgrade()) {
            (Some(tasks), Some(vfs)) => (tasks, vfs),
            _ => return Err(Errno::ENOENT),
        };
        let (fs, cred): (crate::fs::FsContext, Cred) = {
            let state = tasks.read_or_recover();
            let found = state.tasks.get(&TaskId(task)).ok_or(Errno::ENOENT)?;
            let fs = found.fs.lock_or_recover().clone();
            (fs, found.process.cred().fs())
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
        if target.dentry.file_type() != FileType::Regular || target.mnt.flags().noexec {
            return Err(Errno::EACCES);
        }
        vfs.permission(&cx, &target, MAY_EXEC).await?;
        Ok(target)
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
        _ => return Err(Errno::EINVAL),
    };
    if interpreter.is_empty() {
        return Err(Errno::EINVAL);
    }
    // Flags not provided here (`O`, `C`, `T`, `L`) or unknown end the registration.
    if flags.iter().any(|f| !b"PFD".contains(f)) {
        return Err(Errno::EINVAL);
    }
    let shown: String = ['P', 'F']
        .into_iter()
        .filter(|c| flags.contains(&(*c as u8)))
        .collect();
    Ok(Entry {
        name: name.to_vec(),
        ino,
        enabled: AtomicBool::new(!flags.contains(&b'D')),
        how,
        interpreter: interpreter.to_vec(),
        preserve_argv0: flags.contains(&b'P'),
        interp_file: None,
        flags: shown,
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

/// The `binfmt_misc` filesystem type: every mount is the kernel's one instance.
pub(crate) struct BinfmtMiscType(pub(crate) Arc<BinfmtMisc>);

impl FilesystemType for BinfmtMiscType {
    fn mount(
        &self,
        _source: &[u8],
        params: &[FsParam<'_>],
        _cred: &Cred,
    ) -> Result<Arc<dyn Filesystem>, Errno> {
        if !params.is_empty() {
            return Err(Errno::EINVAL);
        }
        Ok(Arc::new(MiscFs(self.0.clone())))
    }
}

struct MiscFs(Arc<BinfmtMisc>);

#[async_trait(?Send)]
impl Filesystem for MiscFs {
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
            misc: self.0.clone(),
            ino: ROOT_INO,
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
}

impl MiscNode {
    fn node(&self, ino: u64) -> Arc<dyn Node> {
        Arc::new(MiscNode {
            misc: self.misc.clone(),
            ino,
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

    /// Everything is root's: the root `0755`, `register` `0200`, the rest `0644`.
    async fn getattr(&self) -> FsResult<Attr> {
        let (mode, nlink) = match self.ino {
            ROOT_INO => (0o755, 2),
            REGISTER_INO => (0o200, 1),
            STATUS_INO => (0o644, 1),
            ino if self.misc.entry(ino).is_some() => (0o644, 1),
            _ => (0o644, 0),
        };
        let time = self.misc.mounted;
        Ok(Attr {
            ino: self.ino,
            file_type: self.file_type(),
            mode,
            nlink,
            uid: 0,
            gid: 0,
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
        assert!(e.preserve_argv0);
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
            b":ab:M::x::/i:O",
            b":ab:M::xy:\\xff:/i:",
            b":ab:M:255:xy::/i:",
        ] {
            assert!(
                create_entry(bad, 1).is_err(),
                "{:?}",
                String::from_utf8_lossy(bad)
            );
        }
    }
}
