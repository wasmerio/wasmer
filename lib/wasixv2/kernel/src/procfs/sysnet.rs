//! `/proc/sys/net` (`net/sysctl_net.c`, `net/ipv4/devinet.c`'s and `net/ipv6/addrconf.c`'s
//! `devconf` tables): the sysctls of the reader's network namespace, which Linux finds from
//! `current`. A driver learns the task only when a file or directory is opened, so the nodes
//! are the same for every namespace, interfaces' directories named by interface name, and the
//! namespace is the opener's: a directory lists, and a value reads and writes, the opener's.
//! A name no interface of the reader's namespace has can be looked up, but not opened
//! (`ENOENT`), where Linux fails the lookup already.
//!
//! Values are kept per namespace and per interface, writable by root. `disable_ipv6` acts
//! (`addrconf_disable_ipv6`), and so do the ICMP ones: `ping_group_range` decides who may
//! open ICMP datagram sockets, `icmp_echo_ignore_all` and `icmp_echo_ignore_broadcasts`
//! whether the namespace answers echo requests. The rest describe what packets would do and
//! have no effect, since nothing is turned into packets. `ip_local_port_range`,
//! `ip_unprivileged_port_start` and `somaxconn` show the values the namespace uses and are
//! read-only here.

use std::{any::Any, collections::BTreeMap, sync::Arc};

use async_trait::async_trait;
use wasixv2_syscalls::Errno;
use wasixv2_vfs::{
    Attr, DirEntry, DirSink, FileType, FsResult, Handle, Node, OpenOptions, SetAttr,
};
use wasixv2_vnet::{EchoIgnore, Interface, iface::valid_name};

use super::Source;
use crate::{ns::NetNs, sync::MutexExt};

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
    /// Keyed by (IPv6?, interfaces, name); the flat ones under [`Dev::All`] of IPv4.
    values: BTreeMap<(bool, Dev, &'static str), i64>,
    /// `net.ipv4.ping_group_range`, once set.
    ping_group_range: Option<(u32, u32)>,
}

/// `net.ipv4.ping_group_range`'s default: no group, so ICMP datagram sockets are nobody's
/// (`inet_init_net`).
const PING_GROUP_RANGE: (u32, u32) = (1, 0);
/// `GID_T_MAX`, the largest group `ping_group_range` takes.
const GID_T_MAX: u32 = u32::MAX - 1;

impl NetSysctl {
    /// `inet_get_ping_group_range_net`: the groups that may open ICMP datagram sockets.
    pub(crate) fn ping_group_range(&self) -> (u32, u32) {
        self.ping_group_range.unwrap_or(PING_GROUP_RANGE)
    }
}

/// Interface names as the tree has met them, by number: a directory's inode number is its
/// name's, which never changes meaning (the VFS interns inodes by number).
#[derive(Default)]
pub(super) struct Names(Vec<String>);

impl Names {
    fn id(&mut self, name: &str) -> u32 {
        let at = match self.0.iter().position(|n| n == name) {
            Some(at) => at,
            None => {
                self.0.push(name.into());
                self.0.len() - 1
            }
        };
        at as u32
    }

    fn name(&self, id: u32) -> Option<String> {
        self.0.get(id as usize).cloned()
    }
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
/// `net.ipv4.*` (`ipv4_net_table`); the port range is shown by [`flat_value`].
const IPV4: &[Key] = &[
    key("icmp_echo_ignore_all", 0),
    key("icmp_echo_ignore_broadcasts", 1),
    key("ip_forward", 0),
    fixed("ip_local_port_range", 0),
    fixed("ip_unprivileged_port_start", 1024),
    key("ping_group_range", 0),
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

/// A `conf` directory: `all`, `default`, or an interface's by the id of its name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Which {
    All,
    Default,
    Named(u32),
}

/// The nodes of the tree.
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
    /// `conf/<which>`.
    Dev {
        v6: bool,
        which: Which,
    },
    /// A value of `conf/<which>`, by index in its table.
    DevKey {
        v6: bool,
        which: Which,
        key: usize,
    },
    /// A value of `core` or `ipv4`.
    Flat {
        core: bool,
        key: usize,
    },
}

/// Where the tree's inode numbers start among the process filesystem's.
const INO_BASE: u64 = 1 << 62;
/// `/proc/sys/net`'s.
pub(super) const ROOT_INO: u64 = INO_BASE | 1;

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

    fn ino(self) -> u64 {
        let which = |w: Which| match w {
            Which::All => 1,
            Which::Default => 2,
            Which::Named(id) => 3 + u64::from(id),
        };
        let local = match self {
            Entry::Root => 1,
            Entry::Core => 2,
            Entry::Ipv4 => 3,
            Entry::Ipv6 => 4,
            Entry::Conf { v6 } => 5 + u64::from(v6),
            Entry::Flat { core, key } => 16 + 64 * u64::from(core) + key as u64,
            Entry::Dev { v6, which: w } => (1 << 40) | (u64::from(v6) << 39) | (which(w) << 8),
            Entry::DevKey { v6, which: w, key } => {
                (1 << 40) | (u64::from(v6) << 39) | (which(w) << 8) | (1 + key as u64)
            }
        };
        INO_BASE | local
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
        // `inet_get_local_port_range`: the range the namespace's port tables use.
        return "32768\t60999".into();
    }
    if key.name == "ping_group_range" {
        let (low, high) = net.sysctl.lock_or_recover().ping_group_range();
        return format!("{low}\t{high}");
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

/// `ipv4_ping_group_range`: two groups (`proc_douintvec_minmax` up to `GID_T_MAX`); a range
/// whose end comes before its start is the default, no group.
fn write_ping_group_range(net: &NetNs, data: &[u8]) -> Result<(), Errno> {
    let text = std::str::from_utf8(data).map_err(|_| Errno::EINVAL)?;
    let mut ids = text.split_whitespace().map(|v| match v.parse::<u32>() {
        Ok(id) if id <= GID_T_MAX => Ok(id),
        _ => Err(Errno::EINVAL),
    });
    let (low, high) = match (ids.next(), ids.next(), ids.next()) {
        (Some(low), Some(high), None) => (low?, high?),
        _ => return Err(Errno::EINVAL),
    };
    let range = match high < low {
        true => PING_GROUP_RANGE,
        false => (low, high),
    };
    net.sysctl.lock_or_recover().ping_group_range = Some(range);
    Ok(())
}

/// Writes a value of `dev` (IPv4's flat ones under `Dev::All`), with what Linux does on the
/// change.
fn write_value(net: &NetNs, v6: bool, dev: Dev, key: &Key, value: i64) -> Result<(), Errno> {
    if let name @ ("icmp_echo_ignore_all" | "icmp_echo_ignore_broadcasts") = key.name {
        // `proc_dou8vec_minmax` from 0 to 1.
        if !(0..=1).contains(&value) {
            return Err(Errno::EINVAL);
        }
        if let Some(network) = &net.network {
            let which = match name {
                "icmp_echo_ignore_broadcasts" => EchoIgnore::Broadcasts,
                _ => EchoIgnore::All,
            };
            network.set_icmp_echo_ignore(which, value != 0)?;
        }
    }
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

/// A node of the tree.
pub(super) struct SysNetNode {
    source: Arc<Source>,
    entry: Entry,
}

impl SysNetNode {
    /// `/proc/sys/net`.
    pub(super) fn root(source: Arc<Source>) -> Arc<dyn Node> {
        Arc::new(Self {
            source,
            entry: Entry::Root,
        })
    }

    fn node(&self, entry: Entry) -> Arc<dyn Node> {
        Arc::new(Self {
            source: self.source.clone(),
            entry,
        })
    }

    /// The interfaces a `conf/<which>` stands for in `net`; `ENOENT` for a name the namespace
    /// has no interface of.
    fn dev(&self, net: &NetNs, which: Which) -> FsResult<Dev> {
        Ok(match which {
            Which::All => Dev::All,
            Which::Default => Dev::Default,
            Which::Named(id) => {
                let name = self.source.sysnet_names.lock_or_recover().name(id);
                let iface = interfaces(net)
                    .into_iter()
                    .find(|f| Some(&f.name) == name.as_ref());
                Dev::Index(iface.ok_or(Errno::ENOENT)?.index)
            }
        })
    }

    /// A directory's entries in namespace `net`.
    fn children(&self, net: &NetNs) -> FsResult<Vec<(String, Entry)>> {
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
                    (
                        "all".to_string(),
                        Entry::Dev {
                            v6,
                            which: Which::All,
                        },
                    ),
                    (
                        "default".to_string(),
                        Entry::Dev {
                            v6,
                            which: Which::Default,
                        },
                    ),
                ];
                let mut names = self.source.sysnet_names.lock_or_recover();
                for iface in interfaces(net) {
                    let which = Which::Named(names.id(&iface.name));
                    out.push((iface.name, Entry::Dev { v6, which }));
                }
                out
            }
            Entry::Dev { v6, which } => {
                self.dev(net, which)?;
                keys(conf_keys(v6), &|key| Entry::DevKey { v6, which, key })
            }
            Entry::DevKey { .. } | Entry::Flat { .. } => return Err(Errno::ENOTDIR),
        })
    }

    /// A value's text in namespace `net`, and where it is kept.
    fn value(&self, net: &NetNs) -> FsResult<(String, bool, Dev)> {
        let key = self.entry.key().ok_or(Errno::EISDIR)?;
        Ok(match self.entry {
            Entry::DevKey { v6, which, .. } => {
                let dev = self.dev(net, which)?;
                (dev_value(net, v6, dev, key).to_string() + "\n", v6, dev)
            }
            Entry::Flat { core, .. } => (flat_value(net, core, key) + "\n", false, Dev::All),
            _ => return Err(Errno::EISDIR),
        })
    }
}

#[async_trait(?Send)]
impl Node for SysNetNode {
    fn ino(&self) -> u64 {
        self.entry.ino()
    }

    fn file_type(&self) -> FileType {
        self.entry.file_type()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    async fn getattr(&self) -> FsResult<Attr> {
        let file_type = self.file_type();
        let mode = match self.entry.key() {
            Some(key) if key.writable => 0o644,
            Some(_) => 0o444,
            None => 0o555,
        };
        let time = self.source.mounted;
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
            atime: time,
            mtime: time,
            ctime: time,
            btime: None,
            dev: None,
        })
    }

    /// `proc_sys_setattr`: no mode or owner changes; a truncation (an `O_TRUNC` open) is
    /// taken and changes nothing.
    async fn setattr(&self, attr: &SetAttr) -> FsResult<Attr> {
        if attr.mode.is_some() || attr.uid.is_some() || attr.gid.is_some() {
            return Err(Errno::EPERM);
        }
        self.getattr().await
    }

    /// The fixed names, and in `conf` any name an interface may have, which the opener's
    /// namespace decides.
    async fn lookup(&self, name: &[u8]) -> FsResult<Arc<dyn Node>> {
        let name = std::str::from_utf8(name).map_err(|_| Errno::ENOENT)?;
        if let Entry::Conf { v6 } = self.entry {
            let which = match name {
                "all" => Which::All,
                "default" => Which::Default,
                name if valid_name(name) => {
                    Which::Named(self.source.sysnet_names.lock_or_recover().id(name))
                }
                _ => return Err(Errno::ENOENT),
            };
            return Ok(self.node(Entry::Dev { v6, which }));
        }
        if let Entry::Dev { v6, which } = self.entry {
            let key = conf_keys(v6)
                .iter()
                .position(|k| k.name == name)
                .ok_or(Errno::ENOENT)?;
            return Ok(self.node(Entry::DevKey { v6, which, key }));
        }
        // The other entries are the same in every namespace.
        let found = self
            .children(&self.source.ns.net)?
            .into_iter()
            .find(|(child, _)| child == name)
            .ok_or(Errno::ENOENT)?;
        Ok(self.node(found.1))
    }

    async fn open(&self, options: OpenOptions) -> FsResult<Box<dyn Handle>> {
        let net = self.source.ns_of(options.task).net.clone();
        if self.file_type() == FileType::Directory {
            let children = self.children(&net)?;
            return Ok(Box::new(DirHandle(children)));
        }
        let key = self.entry.key().ok_or(Errno::EISDIR)?;
        let (text, v6, dev) = self.value(&net)?;
        Ok(Box::new(ValueHandle {
            data: text.into_bytes(),
            net,
            v6,
            dev,
            key,
            writable: options.write,
        }))
    }
}

/// An open directory: its entries in the opener's namespace when opened.
struct DirHandle(Vec<(String, Entry)>);

#[async_trait(?Send)]
impl Handle for DirHandle {
    async fn readdir(&self, cookie: u64, sink: &mut dyn DirSink) -> FsResult<()> {
        for (pos, (name, entry)) in self.0.iter().enumerate().skip(cookie as usize) {
            let entry = DirEntry {
                name: name.as_bytes(),
                ino: entry.ino(),
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

/// An open value: its text at open; a write sets it in the opener's namespace
/// (`proc_sys_write`).
struct ValueHandle {
    data: Vec<u8>,
    net: Arc<NetNs>,
    v6: bool,
    dev: Dev,
    key: &'static Key,
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
        if !self.writable || !self.key.writable {
            return Err(Errno::EACCES);
        }
        if self.key.name == "ping_group_range" {
            write_ping_group_range(&self.net, data)?;
            return Ok(data.len());
        }
        write_value(&self.net, self.v6, self.dev, self.key, parse_int(data)?)?;
        Ok(data.len())
    }

    async fn append(&self, data: &[u8]) -> FsResult<(u64, usize)> {
        Ok((0, self.write_at(data, 0).await?))
    }
}
