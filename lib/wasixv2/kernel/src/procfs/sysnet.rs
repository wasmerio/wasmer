//! `/proc/sys/net` (`net/sysctl_net.c`, `net/ipv4/devinet.c`'s and `net/ipv6/addrconf.c`'s
//! `devconf` tables): the sysctls of the reader's network namespace. Linux looks the namespace
//! up from `current` as the walk goes; here a driver learns its caller only when a link is
//! followed, so `/proc/sys/net` is a magic link, as `/proc/net` is a link to `self/net`, to the
//! tree of the caller's namespace in a filesystem mounted nowhere (as nsfs's objects are).
//!
//! Values are kept per namespace and per interface, writable by root. `disable_ipv6` acts
//! (`addrconf_disable_ipv6`); the rest describe what packets would do and have no effect,
//! since nothing is turned into packets. `ip_local_port_range`, `ip_unprivileged_port_start`
//! and `somaxconn` show the values the namespace uses and are read-only here.

use std::{any::Any, collections::BTreeMap, sync::Arc};

use async_trait::async_trait;
use wasixv2_syscalls::Errno;
use wasixv2_vfs::{
    Attr, DirEntry, DirSink, FileType, Filesystem, FsInfo, FsResult, Handle, NAME_MAX, Node,
    OpenOptions, SetAttr, StatFs, Timestamp,
};
use wasixv2_vnet::Interface;

use crate::{ns::NetNs, sync::MutexExt};

/// `PROC_SUPER_MAGIC`: the files are procfs's.
const PROC_SUPER_MAGIC: u32 = 0x9fa0;

/// Which interfaces' values a `conf` directory holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Dev {
    All,
    Default,
    Index(u32),
}

/// The values a namespace's sysctls were set to; unset ones have their defaults.
#[derive(Default)]
pub(crate) struct NetSysctl {
    /// Keyed by (IPv6?, interfaces, name); the flat ones under [`Dev::All`] of the family.
    values: BTreeMap<(bool, Dev, &'static str), i64>,
}

/// One sysctl: name, default, whether root may write it.
struct Key {
    name: &'static str,
    default: i64,
    writable: bool,
}

const fn key(name: &'static str, default: i64) -> Key {
    Key {
        name,
        default,
        writable: true,
    }
}

const fn fixed(name: &'static str, default: i64) -> Key {
    Key {
        name,
        default,
        writable: false,
    }
}

/// `net.core.*`.
const CORE: &[Key] = &[fixed("somaxconn", 4096)];
/// `net.ipv4.*` (`ipv4_net_table`), the port range and start shown by
/// [`flat_value`].
const IPV4: &[Key] = &[
    key("icmp_echo_ignore_broadcasts", 1),
    key("ip_forward", 0),
    fixed("ip_local_port_range", 0),
    fixed("ip_unprivileged_port_start", 1024),
    key("tcp_fastopen", 1),
    key("tcp_syn_retries", 6),
];
/// `net.ipv4.conf.*.*` (`devinet_sysctl`).
const IPV4_CONF: &[Key] = &[
    key("accept_local", 0),
    key("accept_redirects", 1),
    key("arp_announce", 0),
    key("arp_ignore", 0),
    key("forwarding", 0),
    key("log_martians", 0),
    key("proxy_arp", 0),
    key("rp_filter", 0),
    key("send_redirects", 1),
];
/// `net.ipv6.conf.*.*` (`addrconf_sysctl`).
const IPV6_CONF: &[Key] = &[
    key("accept_dad", 1),
    key("accept_ra", 1),
    key("autoconf", 1),
    key("dad_transmits", 1),
    key("disable_ipv6", 0),
    key("forwarding", 0),
    key("hop_limit", 64),
    key("keep_addr_on_down", 0),
];

/// The directories of the tree.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Entry {
    Root,
    Core,
    Ipv4,
    Ipv6,
    /// `ipv4/conf` or `ipv6/conf`.
    Conf {
        v6: bool,
    },
    /// `conf/<dev>`.
    Dev {
        v6: bool,
        dev: Dev,
    },
    /// A value of `conf/<dev>`, by index in its table.
    DevKey {
        v6: bool,
        dev: Dev,
        key: usize,
    },
    /// A value of `core` or `ipv4`.
    Flat {
        core: bool,
        key: usize,
    },
}

impl Entry {
    fn file_type(self) -> FileType {
        match self {
            Entry::DevKey { .. } | Entry::Flat { .. } => FileType::Regular,
            _ => FileType::Directory,
        }
    }

    fn key(self) -> Option<&'static Key> {
        match self {
            Entry::DevKey { v6, key, .. } => Some(&conf_keys(v6)[key]),
            Entry::Flat { core: true, key } => Some(&CORE[key]),
            Entry::Flat { core: false, key } => Some(&IPV4[key]),
            _ => None,
        }
    }

    /// The inode number within a namespace's tree: below 2^32, which the namespace's own
    /// number goes above, since the VFS interns inodes by number.
    fn ino(self) -> u32 {
        let dev = |dev: Dev| match dev {
            Dev::All => 1,
            Dev::Default => 2,
            Dev::Index(i) => 3 + (i & 0x3f_ffff),
        };
        match self {
            Entry::Root => 1,
            Entry::Core => 2,
            Entry::Ipv4 => 3,
            Entry::Ipv6 => 4,
            Entry::Conf { v6 } => 5 + u32::from(v6),
            Entry::Flat { core, key } => 16 + 64 * u32::from(core) + key as u32,
            Entry::Dev { v6, dev: d } => (1 << 31) | (u32::from(v6) << 30) | (dev(d) << 8),
            Entry::DevKey { v6, dev: d, key } => {
                (1 << 31) | (u32::from(v6) << 30) | (dev(d) << 8) | (1 + key as u32)
            }
        }
    }
}

fn conf_keys(v6: bool) -> &'static [Key] {
    match v6 {
        true => IPV6_CONF,
        false => IPV4_CONF,
    }
}

fn interfaces(net: &NetNs) -> Vec<Interface> {
    net.network
        .as_ref()
        .map(|n| n.interfaces())
        .unwrap_or_default()
}

/// A value of `conf/<dev>`: set, else the interface's from `default` (Linux copies `default`
/// into a new device), else the default; `disable_ipv6` of an interface is the interface's.
fn dev_value(net: &NetNs, v6: bool, dev: Dev, key: &Key) -> i64 {
    if let (true, Dev::Index(i), "disable_ipv6") = (v6, dev, key.name) {
        let iface = interfaces(net).into_iter().find(|f| f.index == i);
        return iface.map_or(0, |f| i64::from(f.ipv6_disabled));
    }
    let values = &net.sysctl.lock_or_recover().values;
    let set = |dev| values.get(&(v6, dev, key.name)).copied();
    let inherited = match dev {
        Dev::Index(_) => set(Dev::Default),
        _ => None,
    };
    set(dev).or(inherited).unwrap_or(key.default)
}

/// A value of `core` or `ipv4`, as text.
fn flat_value(net: &NetNs, core: bool, key: &Key) -> String {
    if key.name == "ip_local_port_range" {
        // `inet_get_local_port_range`, which the namespace's port tables use.
        return "32768\t60999".into();
    }
    let values = &net.sysctl.lock_or_recover().values;
    values
        .get(&(false, Dev::All, key.name))
        .copied()
        .filter(|_| !core)
        .unwrap_or(key.default)
        .to_string()
}

/// `proc_dointvec`: one integer, with optional whitespace around it.
fn parse_int(data: &[u8]) -> Result<i64, Errno> {
    let text = std::str::from_utf8(data).map_err(|_| Errno::EINVAL)?;
    let value = text.trim().parse::<i64>().map_err(|_| Errno::EINVAL)?;
    i32::try_from(value)
        .map(i64::from)
        .map_err(|_| Errno::EINVAL)
}

/// Writes a value, with what Linux does on the change.
fn write_value(net: &NetNs, entry: Entry, value: i64) -> Result<(), Errno> {
    let key = entry.key().ok_or(Errno::EISDIR)?;
    let (v6, dev) = match entry {
        Entry::DevKey { v6, dev, .. } => (v6, dev),
        _ => (false, Dev::All),
    };
    if v6 && key.name == "disable_ipv6" {
        // `addrconf_disable_ipv6`: `all` applies to every interface and to `default`.
        let network = net.network.as_ref().ok_or(Errno::ENODEV)?;
        let targets: Vec<u32> = match dev {
            Dev::Index(i) => vec![i],
            Dev::All => interfaces(net).iter().map(|f| f.index).collect(),
            Dev::Default => Vec::new(),
        };
        for index in targets {
            network.set_ipv6_disabled(index, value != 0)?;
        }
        if dev == Dev::All {
            let mut st = net.sysctl.lock_or_recover();
            st.values.insert((true, Dev::Default, key.name), value);
        }
    }
    let live: Vec<u32> = interfaces(net).iter().map(|f| f.index).collect();
    let mut st = net.sysctl.lock_or_recover();
    // Values of interfaces that are gone go with them.
    st.values
        .retain(|(_, d, _), _| !matches!(d, Dev::Index(i) if !live.contains(i)));
    st.values.insert((v6, dev, key.name), value);
    Ok(())
}

/// The filesystem the trees are in. Its root is empty and never walked into: every walk starts
/// at a namespace's tree, through `/proc/sys/net`.
pub(crate) struct SysNetFs {
    root: Arc<dyn Node>,
}

impl SysNetFs {
    pub(crate) fn new(mounted: Timestamp) -> Self {
        Self {
            root: Arc::new(EmptyRoot(mounted)),
        }
    }
}

struct EmptyRoot(Timestamp);

#[async_trait(?Send)]
impl Node for EmptyRoot {
    fn ino(&self) -> u64 {
        1
    }

    fn file_type(&self) -> FileType {
        FileType::Directory
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    async fn getattr(&self) -> FsResult<Attr> {
        Ok(Attr {
            ino: 1,
            file_type: FileType::Directory,
            mode: 0o555,
            nlink: 2,
            uid: 0,
            gid: 0,
            rdev: Default::default(),
            size: 0,
            blocks: 0,
            blksize: 1024,
            atime: self.0,
            mtime: self.0,
            ctime: self.0,
            btime: None,
            dev: None,
        })
    }

    async fn setattr(&self, _: &SetAttr) -> FsResult<Attr> {
        Err(Errno::EPERM)
    }

    async fn open(&self, _: OpenOptions) -> FsResult<Box<dyn Handle>> {
        Err(Errno::ENOENT)
    }
}

#[async_trait(?Send)]
impl Filesystem for SysNetFs {
    fn info(&self) -> FsInfo {
        FsInfo {
            name: "proc",
            magic: PROC_SUPER_MAGIC,
            name_max: NAME_MAX,
            read_only: false,
            dev: None,
            has_btime: false,
            stable: false,
        }
    }

    fn root(&self) -> Arc<dyn Node> {
        self.root.clone()
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

/// A node of one namespace's tree.
pub(crate) struct SysNetNode {
    net: Arc<NetNs>,
    entry: Entry,
    mounted: Timestamp,
}

impl SysNetNode {
    /// The `net` directory of namespace `net`.
    pub(crate) fn root(net: Arc<NetNs>, mounted: Timestamp) -> Arc<dyn Node> {
        Arc::new(Self {
            net,
            entry: Entry::Root,
            mounted,
        })
    }

    fn node(&self, entry: Entry) -> Arc<dyn Node> {
        Arc::new(Self {
            net: self.net.clone(),
            entry,
            mounted: self.mounted,
        })
    }

    /// The entries of a directory: name and entry.
    fn children(&self) -> FsResult<Vec<(String, Entry)>> {
        let keys = |keys: &[Key], make: &dyn Fn(usize) -> Entry| {
            keys.iter()
                .enumerate()
                .map(|(i, k)| (k.name.to_string(), make(i)))
                .collect::<Vec<_>>()
        };
        Ok(match self.entry {
            Entry::Root => vec![
                ("core".into(), Entry::Core),
                ("ipv4".into(), Entry::Ipv4),
                ("ipv6".into(), Entry::Ipv6),
            ],
            Entry::Core => keys(CORE, &|key| Entry::Flat { core: true, key }),
            Entry::Ipv4 => {
                let mut out = vec![("conf".to_string(), Entry::Conf { v6: false })];
                out.extend(keys(IPV4, &|key| Entry::Flat { core: false, key }));
                out
            }
            Entry::Ipv6 => vec![("conf".into(), Entry::Conf { v6: true })],
            Entry::Conf { v6 } => {
                let mut out = vec![
                    ("all".to_string(), Entry::Dev { v6, dev: Dev::All }),
                    (
                        "default".to_string(),
                        Entry::Dev {
                            v6,
                            dev: Dev::Default,
                        },
                    ),
                ];
                for iface in interfaces(&self.net) {
                    let dev = Dev::Index(iface.index);
                    out.push((iface.name, Entry::Dev { v6, dev }));
                }
                out
            }
            Entry::Dev { v6, dev } => {
                self.alive(dev)?;
                keys(conf_keys(v6), &|key| Entry::DevKey { v6, dev, key })
            }
            Entry::DevKey { .. } | Entry::Flat { .. } => return Err(Errno::ENOTDIR),
        })
    }

    /// An interface's directory is gone with the interface.
    fn alive(&self, dev: Dev) -> FsResult<()> {
        match dev {
            Dev::Index(i) if !interfaces(&self.net).iter().any(|f| f.index == i) => {
                Err(Errno::ENOENT)
            }
            _ => Ok(()),
        }
    }

    fn text(&self) -> FsResult<String> {
        let key = self.entry.key().ok_or(Errno::EISDIR)?;
        let value = match self.entry {
            Entry::DevKey { v6, dev, .. } => {
                self.alive(dev)?;
                dev_value(&self.net, v6, dev, key).to_string()
            }
            Entry::Flat { core, .. } => flat_value(&self.net, core, key),
            _ => return Err(Errno::EISDIR),
        };
        Ok(value + "\n")
    }
}

#[async_trait(?Send)]
impl Node for SysNetNode {
    /// The namespace's own inode number above the entry's.
    fn ino(&self) -> u64 {
        let ns = crate::ns::Namespace::Net(self.net.clone()).ino();
        (u64::from(ns) << 32) | u64::from(self.entry.ino())
    }

    fn file_type(&self) -> FileType {
        self.entry.file_type()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    async fn getattr(&self) -> FsResult<Attr> {
        if let Entry::Dev { dev, .. } | Entry::DevKey { dev, .. } = self.entry {
            self.alive(dev)?;
        }
        let file_type = self.file_type();
        let mode = match self.entry.key() {
            Some(key) if key.writable => 0o644,
            Some(_) => 0o444,
            None => 0o555,
        };
        Ok(Attr {
            ino: self.ino(),
            file_type,
            mode,
            nlink: match file_type {
                FileType::Directory => 2,
                _ => 1,
            },
            uid: 0,
            gid: 0,
            rdev: Default::default(),
            size: 0,
            blocks: 0,
            blksize: 1024,
            atime: self.mounted,
            mtime: self.mounted,
            ctime: self.mounted,
            btime: None,
            dev: None,
        })
    }

    async fn setattr(&self, _: &SetAttr) -> FsResult<Attr> {
        Err(Errno::EPERM)
    }

    async fn lookup(&self, name: &[u8]) -> FsResult<Arc<dyn Node>> {
        let found = self
            .children()?
            .into_iter()
            .find(|(child, _)| child.as_bytes() == name)
            .ok_or(Errno::ENOENT)?;
        Ok(self.node(found.1))
    }

    async fn open(&self, options: OpenOptions) -> FsResult<Box<dyn Handle>> {
        if self.file_type() == FileType::Directory {
            self.children()?;
            return Ok(Box::new(DirHandle(self.node(self.entry))));
        }
        Ok(Box::new(ValueHandle {
            data: self.text()?.into_bytes(),
            net: self.net.clone(),
            entry: self.entry,
            writable: options.write,
        }))
    }
}

/// An open directory, listed afresh on every read.
struct DirHandle(Arc<dyn Node>);

#[async_trait(?Send)]
impl Handle for DirHandle {
    async fn readdir(&self, cookie: u64, sink: &mut dyn DirSink) -> FsResult<()> {
        let node: &SysNetNode = self.0.as_any().downcast_ref().expect("a sysctl directory");
        for (pos, (name, entry)) in node.children()?.iter().enumerate().skip(cookie as usize) {
            let entry = DirEntry {
                name: name.as_bytes(),
                ino: (node.ino() & !u64::from(u32::MAX)) | u64::from(entry.ino()),
                file_type: entry.file_type(),
                next: pos as u64 + 1,
            };
            if !sink.push(entry) {
                break;
            }
        }
        Ok(())
    }
}

/// An open value: its text at open; a write sets it (`proc_sys_write`).
struct ValueHandle {
    data: Vec<u8>,
    net: Arc<NetNs>,
    entry: Entry,
    writable: bool,
}

#[async_trait(?Send)]
impl Handle for ValueHandle {
    async fn read_at(&self, buf: &mut [u8], offset: u64) -> FsResult<usize> {
        let start = usize::try_from(offset)
            .unwrap_or(usize::MAX)
            .min(self.data.len());
        let n = buf.len().min(self.data.len() - start);
        buf[..n].copy_from_slice(&self.data[start..start + n]);
        Ok(n)
    }

    async fn write_at(&self, data: &[u8], _: u64) -> FsResult<usize> {
        let key = self.entry.key().ok_or(Errno::EISDIR)?;
        if !self.writable || !key.writable {
            return Err(Errno::EACCES);
        }
        write_value(&self.net, self.entry, parse_int(data)?)?;
        Ok(data.len())
    }

    async fn append(&self, data: &[u8]) -> FsResult<(u64, usize)> {
        Ok((0, self.write_at(data, 0).await?))
    }
}
