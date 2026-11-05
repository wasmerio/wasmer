//! IPv4's configuration of a network namespace's interfaces (`net/ipv4/devinet.c`), as
//! `/proc/sys/net/ipv4/conf/{all,default,<interface>}` shows it (procfs's `sysnet`): the values
//! `devinet_sysctl` names, with Linux's defaults, written as Linux takes them
//! (`devinet_conf_proc`, `devinet_sysctl_forward`). None of them changes how the network
//! behaves here; they are kept as `tag` is on Linux, which nothing acts on there either.

use std::collections::HashMap;

use wasixv2_syscalls::Errno;

/// `IFF_LOOPBACK`.
const IFF_LOOPBACK: u32 = 0x8;
/// The loopback interface's index (`LOOPBACK_IFINDEX`) and name, which every network
/// namespace has (`loopback_net_init`).
pub(crate) const LOOPBACK: (u32, &str) = (1, "lo");

/// One value: its name, its default in `all` (`ipv4_devconf`) and in `default`
/// (`ipv4_devconf_dflt`), and its file's mode.
pub(crate) struct Var {
    pub(crate) name: &'static str,
    all: i32,
    dflt: i32,
    pub(crate) mode: u32,
}

/// `DEVINET_SYSCTL_RW_ENTRY`.
const fn var(name: &'static str, all: i32, dflt: i32) -> Var {
    Var {
        name,
        all,
        dflt,
        mode: 0o644,
    }
}

/// `forwarding`'s index: `devinet_sysctl_forward` handles it.
const FORWARDING: usize = 0;
/// `accept_redirects`' index, which turning forwarding on in `all` turns off.
const ACCEPT_REDIRECTS: usize = 3;

/// `devinet_sysctl.devinet_vars`, in its order.
pub(crate) const VARS: &[Var] = &[
    var("forwarding", 0, 0),
    // `DEVINET_SYSCTL_RO_ENTRY`.
    Var {
        mode: 0o444,
        ..var("mc_forwarding", 0, 0)
    },
    var("bc_forwarding", 0, 0),
    var("accept_redirects", 1, 1),
    var("secure_redirects", 1, 1),
    var("shared_media", 1, 1),
    var("rp_filter", 0, 0),
    var("send_redirects", 1, 1),
    var("accept_source_route", 0, 1),
    var("accept_local", 0, 0),
    var("src_valid_mark", 0, 0),
    var("proxy_arp", 0, 0),
    var("medium_id", 0, 0),
    var("bootp_relay", 0, 0),
    var("log_martians", 0, 0),
    var("tag", 0, 0),
    var("arp_filter", 0, 0),
    var("arp_announce", 0, 0),
    var("arp_ignore", 0, 0),
    var("arp_accept", 0, 0),
    var("arp_notify", 0, 0),
    var("arp_evict_nocarrier", 1, 1),
    var("proxy_arp_pvlan", 0, 0),
    var("force_igmp_version", 0, 0),
    var("igmpv2_unsolicited_report_interval", 10000, 10000),
    var("igmpv3_unsolicited_report_interval", 1000, 1000),
    var("ignore_routes_with_linkdown", 0, 0),
    var("drop_gratuitous_arp", 0, 0),
    var("disable_xfrm", 0, 0),
    var("disable_policy", 0, 0),
    var("promote_secondaries", 0, 0),
    var("route_localnet", 0, 0),
    var("drop_unicast_in_l2_multicast", 0, 0),
];

/// The values a loopback interface sets for itself when it registers (`inetdev_event`):
/// `disable_xfrm` and `disable_policy`.
const LOOPBACK_SETS: [(&str, i32); 2] = [("disable_xfrm", 1), ("disable_policy", 1)];

/// The values `devinet_conf_proc` writes without tracking (`cnf.state`), so a write to
/// `default` does not reach the interfaces: `disable_xfrm`, `disable_policy`,
/// `promote_secondaries` and `drop_unicast_in_l2_multicast`. Neither does `forwarding`, which
/// another handler writes. An interface keeps what `default` held when it was made.
fn untracked(var: usize) -> bool {
    var == FORWARDING
        || matches!(
            VARS[var].name,
            "disable_xfrm"
                | "disable_policy"
                | "promote_secondaries"
                | "drop_unicast_in_l2_multicast"
        )
}

/// Whose values: `all`, `default`, or the interface with this index.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Scope {
    All,
    Default,
    Dev(u32),
}

/// An interface of a network namespace, as its values know it.
pub(crate) struct Dev {
    pub(crate) index: u32,
    loopback: bool,
}

/// The interfaces of a network: the loopback interface first, which every namespace has,
/// then the network's others.
pub(crate) fn devices(network: Option<&dyn wasixv2_vnet::Network>) -> Vec<Dev> {
    let (index, name) = LOOPBACK;
    let mut devs = vec![Dev {
        index,
        loopback: true,
    }];
    let interfaces = network.map(|net| net.interfaces()).unwrap_or_default();
    for interface in interfaces {
        if interface.index == index || interface.name == name {
            continue;
        }
        devs.push(Dev {
            index: interface.index,
            loopback: interface.flags & IFF_LOOPBACK != 0,
        });
    }
    devs
}

/// A network namespace's values (`net->ipv4.devconf_all`, `devconf_dflt` and each
/// interface's `in_dev->cnf`).
#[derive(Clone)]
pub(crate) struct Devconf {
    all: Vec<i32>,
    dflt: Vec<i32>,
    /// What `default` held when the namespace, and so its interfaces, was made, which an
    /// interface keeps of the [`untracked`] values until one is written to it.
    made: Vec<i32>,
    /// Per interface, by index, the values written to it (with `cnf.state` for the tracked
    /// ones); for the others it follows `default`, as `inetdev_init` copies it and
    /// `devinet_copy_dflt_conf` passes on later writes.
    devs: HashMap<u32, HashMap<usize, i32>>,
}

impl Default for Devconf {
    /// The initial namespace's values, Linux's compiled-in ones.
    fn default() -> Self {
        let dflt: Vec<i32> = VARS.iter().map(|var| var.dflt).collect();
        Self {
            all: VARS.iter().map(|var| var.all).collect(),
            made: dflt.clone(),
            dflt,
            devs: HashMap::new(),
        }
    }
}

impl Devconf {
    /// A new namespace's values, made from those of the initial namespace, `self`
    /// (`devinet_init_net` with `net.core.devconf_inherit_init_net` 0): its `all` and
    /// `default`, but nothing of its interfaces, which the new namespace does not have.
    pub(crate) fn for_new_net(&self) -> Self {
        Self {
            all: self.all.clone(),
            dflt: self.dflt.clone(),
            made: self.dflt.clone(),
            devs: HashMap::new(),
        }
    }

    /// The value `var` (an index into [`VARS`]) has in `scope`; `dev` is that interface.
    pub(crate) fn get(&self, scope: Scope, var: usize, dev: Option<&Dev>) -> i32 {
        match scope {
            Scope::All => self.all[var],
            Scope::Default => self.dflt[var],
            Scope::Dev(index) => {
                if let Some(&value) = self.devs.get(&index).and_then(|own| own.get(&var)) {
                    return value;
                }
                let loopback = dev.is_some_and(|dev| dev.loopback);
                let set = LOOPBACK_SETS
                    .iter()
                    .find(|(name, _)| *name == VARS[var].name);
                match set {
                    Some(&(_, value)) if loopback => value,
                    _ if untracked(var) => self.made[var],
                    _ => self.dflt[var],
                }
            }
        }
    }

    /// A write of `value` to `var` in `scope` of a namespace whose interfaces are `devs`, by a
    /// writer with `CAP_NET_ADMIN` over the namespace's owner or not (`net_admin`).
    ///
    /// `devinet_conf_proc`: an interface holds it as its own from now on, and one written to
    /// `default` reaches every interface that holds none of its own (`devinet_copy_dflt_conf`),
    /// unless [`untracked`]. `devinet_sysctl_forward`: `forwarding` takes `CAP_NET_ADMIN`
    /// (`EPERM`), and a change in `all` turns it on or off in `default` and every interface,
    /// with `accept_redirects` in `all` the other way (`inet_forward_change`). `EACCES` for a
    /// read-only value.
    pub(crate) fn set(
        &mut self,
        scope: Scope,
        var: usize,
        value: i32,
        devs: &[Dev],
        net_admin: bool,
    ) -> Result<(), Errno> {
        if VARS[var].mode & 0o200 == 0 {
            return Err(Errno::EACCES);
        }
        if var == FORWARDING && !net_admin {
            return Err(Errno::EPERM);
        }
        match scope {
            Scope::All => {
                let changed = self.all[var] != value;
                self.all[var] = value;
                if var == FORWARDING && changed {
                    self.all[ACCEPT_REDIRECTS] = i32::from(value == 0);
                    self.dflt[FORWARDING] = value;
                    for dev in devs {
                        self.devs.entry(dev.index).or_default().insert(var, value);
                    }
                }
            }
            Scope::Default => self.dflt[var] = value,
            Scope::Dev(index) => {
                self.devs.entry(index).or_default().insert(var, value);
            }
        }
        Ok(())
    }
}
