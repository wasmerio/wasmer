//! IPv4's configuration of a network namespace's interfaces (`net/ipv4/devinet.c`), as
//! `/proc/sys/net/ipv4/conf/{all,default,<interface>}` shows it: the values `devinet_sysctl`
//! names, with Linux's defaults. None of them changes how the network behaves here, so only
//! `tag`, which Linux keeps without acting on it either, can be written; the rest are read-only.

use std::collections::HashMap;

use wasixv2_syscalls::Errno;

/// `IFF_LOOPBACK`.
const IFF_LOOPBACK: u32 = 0x8;
/// The loopback interface's index (`LOOPBACK_IFINDEX`) and name, which every network
/// namespace has (`loopback_net_init`).
pub(crate) const LOOPBACK: (u32, &str) = (1, "lo");

/// One value: its name, its default in `all` (`ipv4_devconf`) and in `default`
/// (`ipv4_devconf_dflt`), and whether it can be written.
pub(crate) struct Var {
    pub(crate) name: &'static str,
    all: i32,
    dflt: i32,
    pub(crate) writable: bool,
}

const fn var(name: &'static str, all: i32, dflt: i32) -> Var {
    Var {
        name,
        all,
        dflt,
        writable: false,
    }
}

/// `devinet_sysctl.devinet_vars`, in its order.
pub(crate) const VARS: &[Var] = &[
    var("forwarding", 0, 0),
    var("mc_forwarding", 0, 0),
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
    Var {
        writable: true,
        ..var("tag", 0, 0)
    },
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

/// Whose values: `all`, `default`, or the interface with this index.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Scope {
    All,
    Default,
    Dev(u32),
}

/// An interface of a network namespace, as `conf/` lists it.
pub(crate) struct Dev {
    pub(crate) index: u32,
    pub(crate) name: String,
    loopback: bool,
}

/// The interfaces of a network: the loopback interface first, which every namespace has,
/// then the network's others.
pub(crate) fn devices(network: Option<&dyn wasixv2_vnet::Network>) -> Vec<Dev> {
    let (index, name) = LOOPBACK;
    let mut devs = vec![Dev {
        index,
        name: name.to_string(),
        loopback: true,
    }];
    let interfaces = network.map(|net| net.interfaces()).unwrap_or_default();
    for interface in interfaces {
        if interface.index == index || interface.name == name {
            continue;
        }
        devs.push(Dev {
            index: interface.index,
            name: interface.name,
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
    /// Per interface, by index, the values it holds of its own (`cnf.state`); for the others
    /// it follows `default`, as `inetdev_init` copies it and `devinet_copy_dflt_conf` passes
    /// on later writes.
    devs: HashMap<u32, HashMap<usize, i32>>,
}

impl Default for Devconf {
    /// The initial namespace's values, Linux's compiled-in ones.
    fn default() -> Self {
        Self {
            all: VARS.iter().map(|var| var.all).collect(),
            dflt: VARS.iter().map(|var| var.dflt).collect(),
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
                    _ => self.dflt[var],
                }
            }
        }
    }

    /// `devinet_conf_proc`'s write of `value` to `var` in `scope`: an interface holds it as
    /// its own from now on, and one written to `default` reaches every interface that holds
    /// none of its own. `EACCES` for a read-only value.
    pub(crate) fn set(&mut self, scope: Scope, var: usize, value: i32) -> Result<(), Errno> {
        if !VARS[var].writable {
            return Err(Errno::EACCES);
        }
        match scope {
            Scope::All => self.all[var] = value,
            Scope::Default => self.dflt[var] = value,
            Scope::Dev(index) => {
                self.devs.entry(index).or_default().insert(var, value);
            }
        }
        Ok(())
    }
}
