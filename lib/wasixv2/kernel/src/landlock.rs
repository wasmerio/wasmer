//! Landlock (`security/landlock`): unprivileged sandboxing. A process builds a ruleset (an
//! anonymous `[landlock-ruleset]` file) of the access rights it handles and the rules that grant
//! some of them back, beneath directories or for ports, and enforces it on itself with
//! `landlock_restrict_self`, which stacks it as one more layer of its domain. The domain lives
//! in the credentials (`cred->security`): children inherit it, `execve` keeps it, and it never
//! loosens.
//!
//! An access is allowed when every layer that handles it grants it somewhere on the way from
//! the object to the root of the mount tree (`is_access_to_paths_allowed`); rules attach to
//! inodes, so they follow an object through bind mounts and renames. The VFS asks through
//! [`Security`]; the socket layer through [`Domain::check_net`]; signals and abstract Unix
//! sockets through [`Domain::scoped`].
//!
//! The credentials are the process's here, so `landlock_restrict_self` restricts every thread,
//! as `LANDLOCK_RESTRICT_SELF_TSYNC` does. There is no audit subsystem: the logging flags and
//! quiet masks are checked and kept, and change nothing.

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use wasixv2_abi::{
    Errno, Fd, GuestPtr, LandlockAccessFs as AccessFs, LandlockCreateRulesetFlags,
    LandlockRestrictSelfFlags, LandlockRuleType, LandlockRulesetAttr, OpenFlags,
};
use wasixv2_syscalls::{Caller, LocalFuture, SysResult, read_value};
use wasixv2_vfs::{
    Cap, Dentry, FileType, FsResult, Inode, MAY_EXEC, MAY_READ, MAY_WRITE, Path, Security,
};
use wasixv2_wait::{PollMask, PollTable, Pollable};

use crate::{
    Kernel,
    file::{Backing, FileObject, Io, OpenFile},
    sync::MutexExt,
};

/// The Landlock ABI this kernel implements: every right of Linux 7.2's UAPI, up to UDP ports.
pub(crate) const ABI_VERSION: i32 = 10;

/// The errata this implementation never had (`landlock_errata`): TCP sockets are told from
/// other stream sockets by protocol (1), a scoped signal to the caller's own thread group
/// passes (2), and disconnected directories do not widen rights (3).
const ERRATA: i32 = 0b111;

/// `LANDLOCK_MAX_NUM_LAYERS`.
const MAX_NUM_LAYERS: usize = 16;

/// `LANDLOCK_MASK_ACCESS_FS`, `_NET`, `LANDLOCK_MASK_SCOPE`.
const MASK_ACCESS_FS: u64 = AccessFs::LANDLOCK_ACCESS_FS_RESOLVE_UNIX.0 * 2 - 1;
const MASK_ACCESS_NET: u64 = 0xf;
const MASK_SCOPE: u64 = 0x3;

const EXECUTE: u64 = AccessFs::LANDLOCK_ACCESS_FS_EXECUTE.0;
const WRITE_FILE: u64 = AccessFs::LANDLOCK_ACCESS_FS_WRITE_FILE.0;
const READ_FILE: u64 = AccessFs::LANDLOCK_ACCESS_FS_READ_FILE.0;
const READ_DIR: u64 = AccessFs::LANDLOCK_ACCESS_FS_READ_DIR.0;
const REMOVE_DIR: u64 = AccessFs::LANDLOCK_ACCESS_FS_REMOVE_DIR.0;
const REMOVE_FILE: u64 = AccessFs::LANDLOCK_ACCESS_FS_REMOVE_FILE.0;
const REFER: u64 = AccessFs::LANDLOCK_ACCESS_FS_REFER.0;
pub(crate) const TRUNCATE: u64 = AccessFs::LANDLOCK_ACCESS_FS_TRUNCATE.0;
pub(crate) const IOCTL_DEV: u64 = AccessFs::LANDLOCK_ACCESS_FS_IOCTL_DEV.0;
pub(crate) const RESOLVE_UNIX: u64 = AccessFs::LANDLOCK_ACCESS_FS_RESOLVE_UNIX.0;

/// `ACCESS_FILE`: the rights that make sense on what is not a directory.
const ACCESS_FILE: u64 = EXECUTE | WRITE_FILE | READ_FILE | TRUNCATE | IOCTL_DEV;

/// `LANDLOCK_ACCESS_NET_*`.
pub(crate) const BIND_TCP: u64 = 0x1;
pub(crate) const CONNECT_TCP: u64 = 0x2;
pub(crate) const BIND_UDP: u64 = 0x4;
pub(crate) const CONNECT_SEND_UDP: u64 = 0x8;

/// `LANDLOCK_SCOPE_*`.
pub(crate) const SCOPE_ABSTRACT_UNIX_SOCKET: u64 = 0x1;
pub(crate) const SCOPE_SIGNAL: u64 = 0x2;

/// What one layer handles (`struct access_masks`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Handled {
    fs: u64,
    net: u64,
    scope: u64,
}

impl Handled {
    /// `landlock_upgrade_handled_layer_config`: a layer that handles any filesystem right
    /// denies `LANDLOCK_ACCESS_FS_REFER` whether it names it or not.
    fn fs(&self) -> u64 {
        match self.fs {
            0 => 0,
            fs => fs | REFER,
        }
    }
}

/// A ruleset being built (`struct landlock_ruleset`), behind its file descriptor.
pub(crate) struct Ruleset {
    handled: Handled,
    /// The rights whose denials are not logged; rules may only quiet these.
    quiet: Handled,
    rules: Mutex<Rules>,
}

/// The rules of one layer: what each inode and port grants.
#[derive(Default)]
struct Rules {
    fs: HashMap<usize, (Arc<Inode>, u64)>,
    net: HashMap<u16, u64>,
}

/// The inode a rule is attached to (`landlock_object`); the rule keeps it, so its identity
/// stays.
fn key(inode: &Arc<Inode>) -> usize {
    Arc::as_ptr(inode) as usize
}

impl Pollable for Ruleset {
    fn poll(&self, _: Option<&mut PollTable<'_>>) -> PollMask {
        PollMask::empty()
    }
}

/// `ruleset_fops`: reads and writes only make the descriptor readable and writable.
impl FileObject for Ruleset {
    fn read<'a>(&'a self, _: Io<'a>, _: &'a mut [u8]) -> LocalFuture<'a, SysResult<usize>> {
        Box::pin(async { Err(Errno::EINVAL.into()) })
    }

    fn write<'a>(&'a self, _: Io<'a>, _: &'a [u8]) -> LocalFuture<'a, SysResult<usize>> {
        Box::pin(async { Err(Errno::EINVAL.into()) })
    }

    fn can_poll(&self) -> bool {
        false
    }
}

/// What each layer of a domain grants at one inode or port, with the layer's index.
type Grants = Vec<(usize, u64)>;

/// An enforced domain (`struct landlock_domain`): its layers, oldest first, the rules of all of
/// them, and the domain it was stacked on, whose layers it starts with.
pub(crate) struct Domain {
    layers: Vec<Handled>,
    fs: HashMap<usize, (Arc<Inode>, Grants)>,
    net: HashMap<u16, Grants>,
    parent: Option<Arc<Domain>>,
}

impl std::fmt::Debug for Domain {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Domain")
            .field("layers", &self.layers)
            .finish_non_exhaustive()
    }
}

impl Domain {
    /// `landlock_merge_ruleset`: `parent`'s layers and rules, and `ruleset` as a new layer on
    /// top. `E2BIG` past `LANDLOCK_MAX_NUM_LAYERS`.
    fn merge(parent: Option<&Arc<Domain>>, ruleset: &Ruleset) -> Result<Self, Errno> {
        let mut layers = parent.map_or_else(Vec::new, |p| p.layers.clone());
        if layers.len() >= MAX_NUM_LAYERS {
            return Err(Errno::E2BIG);
        }
        let level = layers.len();
        layers.push(ruleset.handled);
        let mut fs = parent.map_or_else(HashMap::new, |p| p.fs.clone());
        let mut net = parent.map_or_else(HashMap::new, |p| p.net.clone());
        let rules = ruleset.rules.lock_or_recover();
        for (key, (inode, access)) in &rules.fs {
            let entry = fs
                .entry(*key)
                .or_insert_with(|| (inode.clone(), Vec::new()));
            entry.1.push((level, *access));
        }
        for (port, access) in &rules.net {
            net.entry(*port).or_default().push((level, *access));
        }
        Ok(Self {
            layers,
            fs,
            net,
            parent: parent.cloned(),
        })
    }

    /// Whether any layer handles a filesystem right.
    fn handles_fs(&self) -> bool {
        self.layers.iter().any(|l| l.fs != 0)
    }

    /// What of `request` each layer still denies at `path` (`landlock_init_layer_masks` and
    /// `is_access_to_paths_allowed` for one path): every layer that handles a right denies
    /// it until a rule on the object or one of its ancestors up to the root of the mount
    /// tree grants it. Mount points hidden under a mount are skipped; a pseudo object (a
    /// pipe, a socket) that no directory holds is allowed everything.
    fn denied(&self, path: &Path, request: u64) -> Vec<u64> {
        let mut denied: Vec<u64> = self.layers.iter().map(|l| request & l.fs()).collect();
        let mut path = path.clone();
        loop {
            if denied.iter().all(|d| *d == 0) {
                return denied;
            }
            if let Some((_, rules)) = self.fs.get(&key(path.dentry.inode())) {
                for (level, access) in rules {
                    denied[*level] &= !access;
                }
            }
            // `jump_up`: from the root of a mount to where it is mounted, and past that.
            while path.is_mount_root() {
                match path.mnt.parent() {
                    Some(up) => path = up,
                    // The root of the mount tree.
                    None => return denied,
                }
            }
            match path.dentry.parent() {
                Some(parent) => path.dentry = parent,
                None if path.dentry.is_pseudo() => return vec![0; denied.len()],
                None => return denied,
            }
        }
    }

    /// `request` at `path`, or `EACCES`.
    fn check(&self, path: &Path, request: u64) -> FsResult<()> {
        match self.denied(path, request).iter().all(|d| *d == 0) {
            true => Ok(()),
            false => Err(Errno::EACCES),
        }
    }

    /// The rights the open file of `path` keeps for later (`landlock_file->allowed_access`):
    /// truncation, and ioctls if it is a device.
    pub(crate) fn file_rights(&self, path: &Path) -> u64 {
        let mut optional = TRUNCATE;
        if matches!(
            path.dentry.file_type(),
            FileType::CharDevice | FileType::BlockDevice
        ) {
            optional |= IOCTL_DEV;
        }
        let denied = self.denied(path, optional);
        denied.iter().fold(optional, |allowed, d| allowed & !d)
    }

    /// `current_check_refer_path`: `old` moves (or, without `removable`, is linked) from
    /// `old_dir` into `new_dir`, over `new` or swapping with it with `exchange`. Within one
    /// directory it takes the rights to make and remove there; across directories also
    /// `LANDLOCK_ACCESS_FS_REFER` on both sides, and no layer may grant the object more where
    /// it goes than where it was (`no_more_access`), or the move fails with `EXDEV`.
    fn check_refer(
        &self,
        old_dir: &Path,
        old: &Arc<Dentry>,
        new_dir: &Path,
        new: Option<&Arc<Dentry>>,
        removable: bool,
        exchange: bool,
    ) -> FsResult<()> {
        let mut request1 = match (exchange, new) {
            (true, Some(new)) => make_access(new.file_type()),
            _ => 0,
        };
        let mut request2 = make_access(old.file_type());
        if removable {
            request1 |= remove_access(old);
            request2 |= new.map_or(0, remove_access);
        }
        if Arc::ptr_eq(&old_dir.dentry, &new_dir.dentry) {
            return self.check(new_dir, request1 | request2);
        }
        request1 |= REFER;
        request2 |= REFER;
        let denied1 = self.denied(old_dir, request1);
        let denied2 = self.denied(new_dir, request2);
        if denied1.iter().chain(&denied2).any(|d| d & !REFER != 0) {
            return Err(Errno::EACCES);
        }
        if denied1.iter().chain(&denied2).any(|d| *d != 0) {
            return Err(Errno::EXDEV);
        }
        let all = MASK_ACCESS_FS | REFER;
        let path_of = |dir: &Path, child: &Arc<Dentry>| Path {
            mnt: dir.mnt.clone(),
            dentry: child.clone(),
        };
        let may_refer = |child: &Path, to: &Path| {
            let from = self.denied(child, all);
            let to = self.denied(to, all);
            let file = !child.dentry.is_dir();
            from.iter().zip(&to).all(|(from, to)| {
                let (from, to) = match file {
                    true => (from & ACCESS_FILE, to & ACCESS_FILE),
                    false => (*from, *to),
                };
                from & !to == 0
            })
        };
        if !may_refer(&path_of(old_dir, old), new_dir) {
            return Err(Errno::EXDEV);
        }
        if exchange
            && let Some(new) = new
            && !may_refer(&path_of(new_dir, new), old_dir)
        {
            return Err(Errno::EXDEV);
        }
        Ok(())
    }

    /// `current_check_access_socket` once the port is known: the network `request`
    /// (`LANDLOCK_ACCESS_NET_*`) for `port`, or `EACCES`.
    pub(crate) fn check_net(&self, port: u16, request: u64) -> Result<(), Errno> {
        let mut denied: Vec<u64> = self.layers.iter().map(|l| request & l.net).collect();
        if let Some(rules) = self.net.get(&port) {
            for (level, access) in rules {
                denied[*level] &= !access;
            }
        }
        match denied.iter().all(|d| *d == 0) {
            true => Ok(()),
            false => Err(Errno::EACCES),
        }
    }

    /// `current_check_access_socket`: the network `request` for the address `raw` given to
    /// a socket of `ipv6`'s family. `AF_UNSPEC` disconnects freely, binds as IPv4's
    /// `INADDR_ANY` (only that, `EAFNOSUPPORT`), and sends as IPv4 does; families other than
    /// IP's are not this check's business; a family other than the socket's is `EINVAL`.
    pub(crate) fn check_socket(
        &self,
        ipv6: bool,
        raw: &[u8],
        request: u64,
        connecting: bool,
    ) -> Result<(), Errno> {
        const AF_UNSPEC: u16 = 0;
        const AF_INET: u16 = 2;
        const AF_INET6: u16 = 10;
        const SIN_LEN: usize = 16;
        const SIN6_LEN_RFC2133: usize = 24;
        if !self.handles_net(request) {
            return Ok(());
        }
        let family = match raw {
            [a, b, ..] => u16::from_le_bytes([*a, *b]),
            _ => return Err(Errno::EINVAL),
        };
        let min = match family {
            AF_UNSPEC if request == CONNECT_TCP || (request == CONNECT_SEND_UDP && connecting) => {
                return Ok(());
            }
            AF_UNSPEC if request == CONNECT_SEND_UDP && ipv6 => return Err(Errno::EACCES),
            AF_UNSPEC if request == CONNECT_SEND_UDP => SIN_LEN,
            AF_UNSPEC if ipv6 => {
                return Err(match raw.len() < SIN6_LEN_RFC2133 {
                    true => Errno::EINVAL,
                    false => Errno::EAFNOSUPPORT,
                });
            }
            AF_UNSPEC => {
                if raw.len() < SIN_LEN {
                    return Err(Errno::EINVAL);
                }
                if raw[4..8] != [0; 4] {
                    return Err(Errno::EAFNOSUPPORT);
                }
                SIN_LEN
            }
            AF_INET => SIN_LEN,
            AF_INET6 => SIN6_LEN_RFC2133,
            _ => return Ok(()),
        };
        if raw.len() < min {
            return Err(Errno::EINVAL);
        }
        let own = if ipv6 { AF_INET6 } else { AF_INET };
        if family != own && family != AF_UNSPEC {
            return Err(Errno::EINVAL);
        }
        self.check_net(u16::from_be_bytes([raw[2], raw[3]]), request)
    }

    /// Whether some layer handles the network right `request`.
    pub(crate) fn handles_net(&self, request: u64) -> bool {
        self.layers.iter().any(|l| l.net & request != 0)
    }

    /// `domain_is_scoped`: whether `scope` keeps a task of this domain from reaching what
    /// belongs to `other` (a task or a socket's creator, `None` for no domain). A layer that
    /// scopes it lets through only what is in the same domain as that layer or one nested in
    /// it.
    pub(crate) fn scoped(self: &Arc<Self>, other: Option<&Arc<Domain>>, scope: u64) -> bool {
        let mut client = Some(self);
        let mut level = self.layers.len();
        let other_depth = other.map_or(0, |o| o.layers.len());
        while let Some(c) = client {
            level -= 1;
            if c.layers[level].scope & scope != 0 {
                // `other`'s ancestor at this layer's depth must be this layer's domain.
                let mut walker = other;
                for _ in level + 1..other_depth {
                    walker = walker.and_then(|w| w.parent.as_ref());
                }
                let same = other_depth > level && walker.is_some_and(|w| Arc::ptr_eq(w, c));
                if !same {
                    return true;
                }
            }
            client = c.parent.as_ref();
        }
        false
    }

    /// `hook_unix_find`: `LANDLOCK_ACCESS_FS_RESOLVE_UNIX` to reach the socket at `path`, which
    /// a process in `server`'s domain bound: what a layer denies is let through when the server
    /// is within that layer's domain.
    pub(crate) fn check_unix_find(
        &self,
        path: &Path,
        server: Option<&Arc<Domain>>,
    ) -> Result<(), Errno> {
        let mut denied = self.denied(path, RESOLVE_UNIX);
        let mut layer = self.layers.len();
        let mut walker = Some(self);
        let server_depth = server.map_or(0, |s| s.layers.len());
        while let Some(w) = walker {
            layer -= 1;
            let mut s = server;
            for _ in layer + 1..server_depth {
                s = s.and_then(|s| s.parent.as_ref());
            }
            if server_depth > layer && s.is_some_and(|s| std::ptr::eq(s.as_ref(), w)) {
                denied[layer] = 0;
            }
            walker = w.parent.as_deref();
        }
        match denied.iter().all(|d| *d == 0) {
            true => Ok(()),
            false => Err(Errno::EACCES),
        }
    }
}

/// `is_masked_device_ioctl`: the ioctls of the file layer that `LANDLOCK_ACCESS_FS_IOCTL_DEV`
/// does not cover.
pub(crate) fn device_ioctl_masked(request: wasixv2_abi::IoctlRequest) -> bool {
    use wasixv2_abi::IoctlRequest as R;
    matches!(
        request,
        R::FIOCLEX | R::FIONCLEX | R::FIONBIO | R::FIOASYNC | R::FIOQSIZE
    ) || request == crate::ioctl::FIGETBSZ
}

/// `get_mode_access`: the right to make an object of `kind`.
fn make_access(kind: FileType) -> u64 {
    let right = match kind {
        FileType::Symlink => AccessFs::LANDLOCK_ACCESS_FS_MAKE_SYM,
        FileType::Directory => AccessFs::LANDLOCK_ACCESS_FS_MAKE_DIR,
        FileType::CharDevice => AccessFs::LANDLOCK_ACCESS_FS_MAKE_CHAR,
        FileType::BlockDevice => AccessFs::LANDLOCK_ACCESS_FS_MAKE_BLOCK,
        FileType::Fifo => AccessFs::LANDLOCK_ACCESS_FS_MAKE_FIFO,
        FileType::Socket => AccessFs::LANDLOCK_ACCESS_FS_MAKE_SOCK,
        _ => AccessFs::LANDLOCK_ACCESS_FS_MAKE_REG,
    };
    right.0
}

/// `maybe_remove`: the right to remove `dentry`.
fn remove_access(dentry: &Arc<Dentry>) -> u64 {
    match dentry.is_dir() {
        true => REMOVE_DIR,
        false => REMOVE_FILE,
    }
}

/// The filesystem hooks (`security/landlock/fs.c`).
impl Security for Domain {
    /// `hook_file_open`.
    fn file_open(&self, path: &Path, access: u32, truncate: bool) -> FsResult<()> {
        let mut request = 0;
        if access & MAY_READ != 0 {
            request |= match path.dentry.is_dir() {
                true => READ_DIR,
                false => READ_FILE,
            };
        }
        if access & MAY_WRITE != 0 {
            request |= WRITE_FILE;
        }
        if access & MAY_EXEC != 0 {
            request |= EXECUTE;
        }
        if truncate {
            request |= TRUNCATE;
        }
        self.check(path, request)
    }

    /// `hook_path_mknod`, `hook_path_mkdir`, `hook_path_symlink`.
    fn path_create(&self, dir: &Path, kind: FileType) -> FsResult<()> {
        self.check(dir, make_access(kind))
    }

    /// `hook_path_unlink`, `hook_path_rmdir`.
    fn path_remove(&self, dir: &Path, victim: &Arc<Dentry>) -> FsResult<()> {
        self.check(dir, remove_access(victim))
    }

    /// `hook_path_link`.
    fn path_link(&self, old: &Path, new_dir: &Path) -> FsResult<()> {
        let Some(parent) = old.dentry.parent() else {
            return Err(Errno::EACCES);
        };
        let old_dir = Path {
            mnt: old.mnt.clone(),
            dentry: parent,
        };
        self.check_refer(&old_dir, &old.dentry, new_dir, None, false, false)
    }

    /// `hook_path_rename`.
    fn path_rename(
        &self,
        old_dir: &Path,
        old: &Arc<Dentry>,
        new_dir: &Path,
        new: Option<&Arc<Dentry>>,
        exchange: bool,
    ) -> FsResult<()> {
        self.check_refer(old_dir, old, new_dir, new, true, exchange)
    }

    /// `hook_path_truncate`.
    fn path_truncate(&self, path: &Path) -> FsResult<()> {
        self.check(path, TRUNCATE)
    }
}

/// The domain's view of credentials for the VFS: only a domain that handles filesystem rights
/// checks anything there.
pub(crate) fn fs_security(domain: &Option<Arc<Domain>>) -> Option<Arc<dyn Security>> {
    let domain = domain.as_ref().filter(|d| d.handles_fs())?;
    Some(domain.clone())
}

impl Kernel {
    /// `landlock_create_ruleset`.
    pub(crate) async fn sys_landlock_create_ruleset(
        &self,
        caller: &Caller<'_>,
        attr: GuestPtr<LandlockRulesetAttr>,
        size: u32,
        flags: u32,
    ) -> SysResult<i32> {
        if flags != 0 {
            if !attr.is_null() || size != 0 {
                return Err(Errno::EINVAL.into());
            }
            return match LandlockCreateRulesetFlags(flags) {
                LandlockCreateRulesetFlags::LANDLOCK_CREATE_RULESET_VERSION => Ok(ABI_VERSION),
                LandlockCreateRulesetFlags::LANDLOCK_CREATE_RULESET_ERRATA => Ok(ERRATA),
                _ => Err(Errno::EINVAL.into()),
            };
        }
        let attr = read_min_struct(caller, attr, size).await?;
        let subset = |sub: u64, of: u64| sub | of == of;
        if !subset(attr.handled_access_fs, MASK_ACCESS_FS)
            || !subset(attr.handled_access_net, MASK_ACCESS_NET)
            || !subset(attr.scoped, MASK_SCOPE)
            || !subset(attr.quiet_access_fs, attr.handled_access_fs)
            || !subset(attr.quiet_access_net, attr.handled_access_net)
            || !subset(attr.quiet_scoped, attr.scoped)
        {
            return Err(Errno::EINVAL.into());
        }
        let handled = Handled {
            fs: attr.handled_access_fs,
            net: attr.handled_access_net,
            scope: attr.scoped,
        };
        if handled == Handled::default() {
            return Err(Errno::ENOMSG.into());
        }
        let ruleset = Ruleset {
            handled,
            quiet: Handled {
                fs: attr.quiet_access_fs,
                net: attr.quiet_access_net,
                scope: attr.quiet_scoped,
            },
            rules: Mutex::default(),
        };
        let file = OpenFile::new(
            Backing::Stream(Arc::new(ruleset), self.pseudo.anon("[landlock-ruleset]")),
            OpenFlags::O_RDWR,
        );
        let [fd] = self.install_files(caller, [file], true)?;
        Ok(fd.0)
    }

    /// `get_ruleset_from_fd`: `EBADF` for no descriptor, `EBADFD` for one that is no ruleset.
    fn ruleset(&self, caller: &Caller<'_>, fd: Fd) -> Result<Arc<OpenFile>, Errno> {
        let file = self.file(caller.task(), fd)?;
        if file.downcast::<Ruleset>().is_none() {
            return Err(Errno::EBADFD);
        }
        Ok(file)
    }

    /// `landlock_add_rule`.
    pub(crate) async fn sys_landlock_add_rule(
        &self,
        caller: &Caller<'_>,
        ruleset_fd: Fd,
        rule_type: i32,
        rule_attr: u32,
        flags: u32,
    ) -> SysResult<i32> {
        if flags != 0 {
            // `LANDLOCK_ADD_RULE_QUIET` marks rules for the audit log, which there is none of.
            if flags != wasixv2_abi::LandlockAddRuleFlags::LANDLOCK_ADD_RULE_QUIET.0 {
                return Err(Errno::EINVAL.into());
            }
        }
        let file = self.ruleset(caller, ruleset_fd)?;
        let ruleset = file.downcast::<Ruleset>().expect("checked");
        match LandlockRuleType(rule_type) {
            LandlockRuleType::LANDLOCK_RULE_PATH_BENEATH => {
                let mut raw = [0; 12];
                caller.memory().read(rule_attr, &mut raw).await?;
                let allowed = u64::from_le_bytes(raw[..8].try_into().expect("8 bytes"));
                let parent_fd = Fd(i32::from_le_bytes(raw[8..].try_into().expect("4 bytes")));
                if flags == 0 && allowed == 0 {
                    return Err(Errno::ENOMSG.into());
                }
                if allowed | ruleset.handled.fs != ruleset.handled.fs
                    || (flags != 0 && ruleset.quiet.fs == 0)
                {
                    return Err(Errno::EINVAL.into());
                }
                // `get_path_from_fd`: no rulesets, pipes, sockets or other internal objects.
                let parent = self.file(caller.task(), parent_fd)?;
                if parent.downcast::<Ruleset>().is_some() {
                    return Err(Errno::EBADFD.into());
                }
                let path = parent.path().clone();
                if path.dentry.is_pseudo() {
                    return Err(Errno::EBADFD.into());
                }
                // `landlock_append_fs_rule`: files only get the rights that make sense.
                if !path.dentry.is_dir() && allowed & !ACCESS_FILE != 0 {
                    return Err(Errno::EINVAL.into());
                }
                let inode = path.dentry.inode().clone();
                let mut rules = ruleset.rules.lock_or_recover();
                let entry = rules.fs.entry(key(&inode)).or_insert((inode, 0));
                entry.1 |= allowed;
                Ok(0)
            }
            LandlockRuleType::LANDLOCK_RULE_NET_PORT => {
                let attr: wasixv2_abi::LandlockNetPortAttr =
                    read_value(caller.memory(), GuestPtr::new(rule_attr)).await?;
                if flags == 0 && attr.allowed_access == 0 {
                    return Err(Errno::ENOMSG.into());
                }
                let handled = ruleset.handled.net;
                if attr.allowed_access | handled != handled
                    || (flags != 0 && ruleset.quiet.net == 0)
                {
                    return Err(Errno::EINVAL.into());
                }
                let port = u16::try_from(attr.port).map_err(|_| Errno::EINVAL)?;
                *ruleset.rules.lock_or_recover().net.entry(port).or_default() |=
                    attr.allowed_access;
                Ok(0)
            }
            _ => Err(Errno::EINVAL.into()),
        }
    }

    /// `landlock_restrict_self`: stacks the ruleset as a new layer of the caller's domain.
    /// Needs `no_new_privs` or `CAP_SYS_ADMIN` (`EPERM`); with only
    /// `LANDLOCK_RESTRICT_SELF_LOG_SUBDOMAINS_OFF`, `ruleset_fd` may be -1.
    pub(crate) async fn sys_landlock_restrict_self(
        &self,
        caller: &Caller<'_>,
        ruleset_fd: Fd,
        flags: u32,
    ) -> SysResult<i32> {
        let all = LandlockRestrictSelfFlags::LANDLOCK_RESTRICT_SELF_LOG_SAME_EXEC_OFF
            | LandlockRestrictSelfFlags::LANDLOCK_RESTRICT_SELF_LOG_NEW_EXEC_ON
            | LandlockRestrictSelfFlags::LANDLOCK_RESTRICT_SELF_LOG_SUBDOMAINS_OFF
            | LandlockRestrictSelfFlags::LANDLOCK_RESTRICT_SELF_TSYNC;
        if flags & !all.0 != 0 {
            return Err(Errno::EINVAL.into());
        }
        let cred = self.cred(caller);
        if !self.no_new_privs(caller) && !cred.ns_capable(&cred.user_ns, Cap::SysAdmin) {
            return Err(Errno::EPERM.into());
        }
        let tsync = LandlockRestrictSelfFlags::LANDLOCK_RESTRICT_SELF_TSYNC.0;
        let subdomains_off = LandlockRestrictSelfFlags::LANDLOCK_RESTRICT_SELF_LOG_SUBDOMAINS_OFF.0;
        if ruleset_fd.0 == -1 && flags & !tsync == subdomains_off {
            return Ok(0);
        }
        let file = self.ruleset(caller, ruleset_fd)?;
        let ruleset = file.downcast::<Ruleset>().expect("checked");
        let process = self.process(caller.task());
        process.update_cred(|cred| {
            let domain = Domain::merge(cred.landlock.as_ref(), ruleset)?;
            let mut new = cred.clone();
            new.landlock = Some(Arc::new(domain));
            Ok(new)
        })
    }

    /// `hook_sb_mount`, `hook_sb_umount`, `hook_move_mount`, `hook_sb_pivotroot`: a process
    /// whose domain handles filesystem rights changes no mount, so it cannot get around them
    /// (`EPERM`).
    pub(crate) fn landlock_may_change_mounts(&self, caller: &Caller<'_>) -> Result<(), Errno> {
        match self.landlock(caller).is_some_and(|d| d.handles_fs()) {
            true => Err(Errno::EPERM),
            false => Ok(()),
        }
    }

    /// The caller's Landlock domain, if it has one.
    pub(crate) fn landlock(&self, caller: &Caller<'_>) -> Option<Arc<Domain>> {
        self.process(caller.task()).cred().landlock
    }
}

/// `copy_min_struct_from_user` of a `landlock_ruleset_attr` of `size` bytes: at least its first
/// field (`EINVAL`), a pointer (`EFAULT`), at most a page (`E2BIG`), and zeroes past what the
/// kernel knows (`E2BIG`).
async fn read_min_struct(
    caller: &Caller<'_>,
    attr: GuestPtr<LandlockRulesetAttr>,
    size: u32,
) -> SysResult<LandlockRulesetAttr> {
    use wasixv2_abi::GuestLayout;
    let size = size as usize;
    if size < 8 {
        return Err(Errno::EINVAL.into());
    }
    if attr.is_null() {
        return Err(Errno::EFAULT.into());
    }
    if size > 4096 {
        return Err(Errno::E2BIG.into());
    }
    let mut raw = vec![0; size.max(LandlockRulesetAttr::SIZE)];
    caller.memory().read(attr.addr(), &mut raw[..size]).await?;
    if raw[LandlockRulesetAttr::SIZE..].iter().any(|b| *b != 0) {
        return Err(Errno::E2BIG.into());
    }
    Ok(LandlockRulesetAttr::read_from(&raw).expect("sized"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn domain(
        parent: Option<&Arc<Domain>>,
        net: u64,
        scope: u64,
        ports: &[(u16, u64)],
    ) -> Arc<Domain> {
        let ruleset = Ruleset {
            handled: Handled { fs: 0, net, scope },
            quiet: Handled::default(),
            rules: Mutex::default(),
        };
        ruleset
            .rules
            .lock_or_recover()
            .net
            .extend(ports.iter().copied());
        Arc::new(Domain::merge(parent, &ruleset).unwrap())
    }

    fn sockaddr_in(family: u16, port: u16, addr: [u8; 4]) -> Vec<u8> {
        let mut raw = family.to_le_bytes().to_vec();
        raw.extend(port.to_be_bytes());
        raw.extend(addr);
        raw.resize(16, 0);
        raw
    }

    /// `current_check_access_socket`: ports a layer handles need a rule; the address must be
    /// long enough and of the socket's family, AF_UNSPEC binds only to INADDR_ANY and connects
    /// (disconnects) freely, and other families are not looked at.
    #[test]
    fn sockets_need_their_port() {
        let d = domain(None, BIND_TCP | CONNECT_TCP, 0, &[(8080, BIND_TCP)]);
        let bind = |raw: &[u8]| d.check_socket(false, raw, BIND_TCP, false);
        assert_eq!(bind(&sockaddr_in(2, 8080, [0; 4])), Ok(()));
        assert_eq!(bind(&sockaddr_in(2, 8081, [0; 4])), Err(Errno::EACCES));
        assert_eq!(bind(&sockaddr_in(2, 8080, [0; 4])[..8]), Err(Errno::EINVAL));
        assert_eq!(bind(&[2]), Err(Errno::EINVAL));
        assert_eq!(bind(&sockaddr_in(0, 8080, [0; 4])), Ok(()));
        assert_eq!(
            bind(&sockaddr_in(0, 8080, [127, 0, 0, 1])),
            Err(Errno::EAFNOSUPPORT)
        );
        assert_eq!(bind(&sockaddr_in(1, 9, [0; 4])), Ok(()));
        let mut v6 = sockaddr_in(10, 8080, [0; 4]);
        v6.resize(28, 0);
        assert_eq!(bind(&v6), Err(Errno::EINVAL));
        assert_eq!(d.check_socket(true, &v6, BIND_TCP, false), Ok(()));
        let connect = |raw: &[u8]| d.check_socket(false, raw, CONNECT_TCP, true);
        assert_eq!(connect(&sockaddr_in(2, 80, [1; 4])), Err(Errno::EACCES));
        assert_eq!(connect(&sockaddr_in(0, 80, [1; 4])), Ok(()));
        // UDP is not handled: anything goes.
        assert_eq!(
            d.check_socket(false, &sockaddr_in(2, 53, [1; 4]), CONNECT_SEND_UDP, false),
            Ok(())
        );
    }

    /// `domain_is_scoped`: a scoping layer lets through what is in its own domain or one
    /// nested in it, not what is outside or in no domain.
    #[test]
    fn scopes_follow_the_domain_hierarchy() {
        let outer = domain(None, 0, SCOPE_SIGNAL, &[]);
        let inner = domain(Some(&outer), BIND_TCP, 0, &[]);
        let other = domain(None, 0, SCOPE_SIGNAL, &[]);
        assert!(!outer.scoped(Some(&outer), SCOPE_SIGNAL));
        assert!(!outer.scoped(Some(&inner), SCOPE_SIGNAL));
        assert!(!inner.scoped(Some(&outer), SCOPE_SIGNAL));
        assert!(outer.scoped(None, SCOPE_SIGNAL));
        assert!(outer.scoped(Some(&other), SCOPE_SIGNAL));
        assert!(!outer.scoped(None, SCOPE_ABSTRACT_UNIX_SOCKET));
        assert!(!inner.scoped(None, SCOPE_ABSTRACT_UNIX_SOCKET));
    }
}
