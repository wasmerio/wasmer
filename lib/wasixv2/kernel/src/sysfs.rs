//! The sysfs filesystem (`fs/sysfs`), as far as the network goes: the interfaces of one network
//! namespace under `/sys/class/net` (`net/core/net-sysfs.c`). As Linux tags a sysfs mount with
//! the network namespace of whoever mounted it (`sysfs_get_tree`), each mount shows that
//! namespace's interfaces, which is what `ip netns exec` remounts `/sys` for.
//!
//! Every interface is a directory under `devices/virtual/net`, and `class/net/<name>` links to
//! it, as for Linux's virtual devices; the uplink, a physical device on Linux, is shown the same
//! way. The attributes are read-only, and nodes are computed when looked up, never stored.
//!
//! Besides the network: `fs/cgroup`, where the cgroup hierarchy is mounted, `kernel/mm`, empty
//! without huge pages, KSM or swap, and `kernel/cgroup`'s `delegate` and `features`.

use std::{any::Any, sync::Arc};

use async_trait::async_trait;
use wasixv2_syscalls::Errno;
use wasixv2_vfs::{
    Attr, Clock, DirEntry, DirSink, FileType, Filesystem, FsInfo, FsResult, Handle, Link, NAME_MAX,
    Node, OpenOptions, SetAttr, StatFs, Timestamp,
};
use wasixv2_vnet::{
    Interface,
    iface::flags::{IFF_LOOPBACK, IFF_LOWER_UP, IFF_RUNNING, IFF_UP},
};

use crate::{Kernel, fs::HostClock, ns::NetNs};

/// `SYSFS_MAGIC`.
pub(crate) const SYSFS_MAGIC: u32 = 0x6265_6572;
const ARPHRD_ETHER: u16 = 1;
const ARPHRD_LOOPBACK: u16 = 772;
/// `DEFAULT_TX_QUEUE_LEN`.
const TXQLEN: u32 = 1000;

/// The attributes of an interface's directory (`net_class_attrs`).
const ATTRS: &[&str] = &[
    "addr_assign_type",
    "addr_len",
    "address",
    "broadcast",
    "carrier",
    "dev_id",
    "dormant",
    "flags",
    "ifindex",
    "iflink",
    "mtu",
    "operstate",
    "tx_queue_len",
    "type",
    "uevent",
];

/// The fixed directories: parent, name. Index 0's parent is the root.
const ROOT: usize = usize::MAX;
const DIRS: &[(usize, &str)] = &[
    (ROOT, "class"),
    (ROOT, "devices"),
    (ROOT, "kernel"),
    (ROOT, "fs"),
    // 4
    (0, "net"),
    (1, "virtual"),
    // 6
    (5, "net"),
    // 7: where the cgroup hierarchy is mounted (`sysfs_create_mount_point(fs_kobj, "cgroup")`).
    (3, "cgroup"),
    // 8: `mm_kobj`, empty without huge pages, KSM or swap.
    (2, "mm"),
    // 9: `cgroup_sysfs_init`.
    (2, "cgroup"),
];

/// The fixed files: parent directory, name, contents.
const FILES: &[(usize, &str, &str)] = &[
    // `delegate_show`: the files a delegated cgroup's owner gets (`CFTYPE_NS_DELEGATABLE`).
    (
        9,
        "delegate",
        "cgroup.procs\ncgroup.threads\ncgroup.subtree_control\n",
    ),
    // `features_show`: the mount options cgroup2 takes.
    (
        9,
        "features",
        "nsdelegate\nfavordynmods\nmemory_localevents\nmemory_recursiveprot\n\
         memory_hugetlb_accounting\npids_localevents\n",
    ),
];
const CLASS_NET: usize = 4;
const VIRTUAL_NET: usize = 6;

impl Kernel {
    /// A sysfs instance showing the network namespace `net`, as a mount by a task of that
    /// namespace makes.
    pub(crate) fn sysfs_of(&self, net: Arc<NetNs>) -> Arc<dyn Filesystem> {
        Arc::new(SysFs(Arc::new(Source {
            net,
            mounted: HostClock(self.host.clone()).now(),
        })))
    }

    /// The sysfs of the initial namespaces, to mount at `/sys`.
    pub fn sysfs(&self) -> Arc<dyn Filesystem> {
        self.sysfs_of(self.init_ns.net.clone())
    }
}

struct Source {
    net: Arc<NetNs>,
    mounted: Timestamp,
}

impl Source {
    fn interfaces(&self) -> Vec<Interface> {
        self.net
            .network
            .as_ref()
            .map(|n| n.interfaces())
            .unwrap_or_default()
    }

    fn interface(&self, index: u32) -> Option<Interface> {
        self.interfaces().into_iter().find(|i| i.index == index)
    }
}

struct SysFs(Arc<Source>);

#[async_trait(?Send)]
impl Filesystem for SysFs {
    fn info(&self) -> FsInfo {
        FsInfo {
            name: "sysfs",
            magic: SYSFS_MAGIC,
            name_max: NAME_MAX,
            read_only: false,
            dev: None,
            has_btime: false,
            // Interfaces come, go and change names with the namespace.
            stable: false,
        }
    }

    fn root(&self) -> Arc<dyn Node> {
        Arc::new(SysNode {
            source: self.0.clone(),
            entry: Entry::Root,
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

/// A node of the tree; interfaces by index, which stays as their names change.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Entry {
    Root,
    /// One of [`DIRS`].
    Dir(usize),
    /// One of [`FILES`].
    File(usize),
    /// `class/net/<name>`, a link to the interface's directory.
    ClassLink(u32),
    /// `devices/virtual/net/<name>`.
    Device(u32),
    /// An attribute of an interface, one of [`ATTRS`].
    Attr(u32, usize),
}

struct SysNode {
    source: Arc<Source>,
    entry: Entry,
}

fn ino(entry: Entry) -> u64 {
    match entry {
        Entry::Root => 1,
        Entry::Dir(i) => 2 + i as u64,
        Entry::File(i) => 1024 + i as u64,
        Entry::ClassLink(index) => (1 << 40) | u64::from(index),
        Entry::Device(index) => (2 << 40) | (u64::from(index) << 8),
        Entry::Attr(index, attr) => (2 << 40) | (u64::from(index) << 8) | (1 + attr as u64),
    }
}

fn file_type(entry: Entry) -> FileType {
    match entry {
        Entry::Root | Entry::Dir(_) | Entry::Device(_) => FileType::Directory,
        Entry::ClassLink(_) => FileType::Symlink,
        Entry::Attr(..) | Entry::File(_) => FileType::Regular,
    }
}

/// `IF_OPER_*` names, as `operstate` shows them (`rfc2863_policy`).
fn operstate(iface: &Interface) -> &'static str {
    if iface.flags & IFF_UP == 0 {
        return "down";
    }
    match (iface.flags & IFF_LOOPBACK != 0, iface.kind) {
        (true, _) | (_, Some("dummy")) => "unknown",
        _ if iface.flags & IFF_LOWER_UP != 0 => "up",
        _ => "lowerlayerdown",
    }
}

fn mac(bytes: &[u8; 6]) -> String {
    let hex: Vec<String> = bytes.iter().map(|b| format!("{b:02x}")).collect();
    hex.join(":")
}

/// An attribute's contents (`*_show`); `carrier` of an interface that is down is `EINVAL`, as
/// `carrier_show` answers.
fn attr(iface: &Interface, name: &str) -> FsResult<String> {
    let loopback = iface.flags & IFF_LOOPBACK != 0;
    let kind = match loopback {
        true => ARPHRD_LOOPBACK,
        false => ARPHRD_ETHER,
    };
    Ok(match name {
        // `NET_ADDR_PERM` for loopback, `NET_ADDR_RANDOM` for what a request made.
        "addr_assign_type" => match iface.kind {
            Some(_) => "1".into(),
            None => "0".into(),
        },
        "addr_len" => "6".into(),
        "address" => mac(&iface.mac),
        "broadcast" => match loopback {
            true => mac(&[0; 6]),
            false => mac(&[0xff; 6]),
        },
        "carrier" if iface.flags & IFF_UP == 0 => return Err(Errno::EINVAL),
        "carrier" => u8::from(iface.flags & IFF_LOWER_UP != 0).to_string(),
        "dev_id" => "0x0".into(),
        "dormant" => "0".into(),
        // `dev->flags`: what `dev_get_flags` adds from the carrier is not in it.
        "flags" => format!("{:#x}", iface.flags & 0xffff & !IFF_RUNNING),
        "ifindex" => iface.index.to_string(),
        // `dev_get_iflink`: a veth's peer, else the interface itself.
        "iflink" => iface.peer.map_or(iface.index, |(peer, _)| peer).to_string(),
        "mtu" => iface.mtu.to_string(),
        "operstate" => operstate(iface).into(),
        "tx_queue_len" => TXQLEN.to_string(),
        "type" => kind.to_string(),
        _ => format!("INTERFACE={}\nIFINDEX={}", iface.name, iface.index),
    } + "\n")
}

impl SysNode {
    fn node(&self, entry: Entry) -> Arc<dyn Node> {
        Arc::new(Self {
            source: self.source.clone(),
            entry,
        })
    }

    /// The interface an entry is of; `ENOENT` once it is gone.
    fn interface(&self) -> FsResult<Option<Interface>> {
        match self.entry {
            Entry::ClassLink(index) | Entry::Device(index) | Entry::Attr(index, _) => {
                self.source.interface(index).map(Some).ok_or(Errno::ENOENT)
            }
            _ => Ok(None),
        }
    }

    /// A directory's entries: position, name, entry.
    fn children(&self) -> FsResult<Vec<(u64, Vec<u8>, Entry)>> {
        let mut out = Vec::new();
        let me = match self.entry {
            Entry::Root => ROOT,
            Entry::Dir(i) => i,
            Entry::Device(index) => {
                self.source.interface(index).ok_or(Errno::ENOENT)?;
                for (i, name) in ATTRS.iter().enumerate() {
                    out.push((i as u64, name.as_bytes().to_vec(), Entry::Attr(index, i)));
                }
                return Ok(out);
            }
            _ => return Err(Errno::ENOTDIR),
        };
        for (i, (parent, name)) in DIRS.iter().enumerate() {
            if *parent == me {
                out.push((i as u64, name.as_bytes().to_vec(), Entry::Dir(i)));
            }
        }
        for (i, (parent, name, _)) in FILES.iter().enumerate() {
            if *parent == me {
                out.push((512 + i as u64, name.as_bytes().to_vec(), Entry::File(i)));
            }
        }
        if me == CLASS_NET || me == VIRTUAL_NET {
            for iface in self.source.interfaces() {
                let entry = match me {
                    CLASS_NET => Entry::ClassLink(iface.index),
                    _ => Entry::Device(iface.index),
                };
                out.push((
                    1024 + u64::from(iface.index),
                    iface.name.into_bytes(),
                    entry,
                ));
            }
        }
        Ok(out)
    }
}

#[async_trait(?Send)]
impl Node for SysNode {
    fn ino(&self) -> u64 {
        ino(self.entry)
    }

    fn file_type(&self) -> FileType {
        file_type(self.entry)
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    async fn getattr(&self) -> FsResult<Attr> {
        self.interface()?;
        let file_type = self.file_type();
        let time = self.source.mounted;
        Ok(Attr {
            ino: self.ino(),
            file_type,
            mode: match file_type {
                FileType::Directory => 0o755,
                FileType::Symlink => 0o777,
                _ => 0o444,
            },
            nlink: match file_type {
                FileType::Directory => 2,
                _ => 1,
            },
            uid: 0,
            gid: 0,
            rdev: Default::default(),
            // `sysfs_init_inode`: attributes claim a page.
            size: match file_type {
                FileType::Regular => 4096,
                _ => 0,
            },
            blocks: 0,
            blksize: 4096,
            atime: time,
            mtime: time,
            ctime: time,
            btime: None,
            dev: None,
        })
    }

    async fn setattr(&self, _: &SetAttr) -> FsResult<Attr> {
        Err(Errno::EPERM)
    }

    async fn lookup(&self, name: &[u8]) -> FsResult<Arc<dyn Node>> {
        if self.file_type() != FileType::Directory {
            return Err(Errno::ENOTDIR);
        }
        let found = self
            .children()?
            .into_iter()
            .find(|(_, child, _)| child == name)
            .ok_or(Errno::ENOENT)?;
        Ok(self.node(found.2))
    }

    async fn readlink(&self) -> FsResult<Vec<u8>> {
        match self.interface()? {
            Some(iface) if self.file_type() == FileType::Symlink => {
                Ok(format!("../../devices/virtual/net/{}", iface.name).into_bytes())
            }
            _ => Err(Errno::EINVAL),
        }
    }

    async fn get_link(&self, _: &wasixv2_vfs::Ctx<'_>) -> FsResult<Link> {
        self.readlink().await.map(Link::Body)
    }

    async fn open(&self, _: OpenOptions) -> FsResult<Box<dyn Handle>> {
        match self.entry {
            Entry::Attr(_, i) => {
                let iface = self.interface()?.ok_or(Errno::ENOENT)?;
                Ok(Box::new(AttrHandle(attr(&iface, ATTRS[i]))))
            }
            Entry::File(i) => Ok(Box::new(AttrHandle(Ok(FILES[i].2.to_string())))),
            Entry::ClassLink(_) => Err(Errno::ENXIO),
            _ => {
                self.children()?;
                Ok(Box::new(DirHandle(self.node(self.entry))))
            }
        }
    }
}

/// An open directory, listed afresh on every read.
struct DirHandle(Arc<dyn Node>);

#[async_trait(?Send)]
impl Handle for DirHandle {
    async fn readdir(&self, cookie: u64, sink: &mut dyn DirSink) -> FsResult<()> {
        let node: &SysNode = self.0.as_any().downcast_ref().expect("a sysfs directory");
        for (pos, name, entry) in node.children()?.iter().filter(|(pos, ..)| *pos >= cookie) {
            let entry = DirEntry {
                name,
                ino: ino(*entry),
                file_type: file_type(*entry),
                next: pos + 1,
            };
            if !sink.push(entry) {
                break;
            }
        }
        Ok(())
    }
}

/// An open attribute: its contents at open, or the error its read gives.
struct AttrHandle(FsResult<String>);

#[async_trait(?Send)]
impl Handle for AttrHandle {
    async fn read_at(&self, buf: &mut [u8], offset: u64) -> FsResult<usize> {
        let data = self.0.as_ref().map_err(|e| *e)?.as_bytes();
        let start = usize::try_from(offset)
            .unwrap_or(usize::MAX)
            .min(data.len());
        let n = buf.len().min(data.len() - start);
        buf[..n].copy_from_slice(&data[start..start + n]);
        Ok(n)
    }

    async fn write_at(&self, _: &[u8], _: u64) -> FsResult<usize> {
        Err(Errno::EACCES)
    }
}
