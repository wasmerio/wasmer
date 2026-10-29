//! Interface and route `ioctl`s on sockets, what `ifconfig` and `route` use: `dev_ioctl`
//! (`net/core/dev_ioctl.c`) on every socket; IPv4 addresses (`devinet_ioctl`) and routes
//! (`ip_rt_ioctl`) on `AF_INET` sockets; IPv6 addresses (`addrconf_add_ifaddr`,
//! `addrconf_del_ifaddr`) and routes (`ipv6_route_ioctl`) on `AF_INET6` sockets. They act on
//! the socket's namespace (`sock_net`), not the caller's. As before, the IPv4 address queries
//! (`SIOCGIFADDR`, ...) are answered on every socket, where Linux answers them on `AF_INET`
//! ones only.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

use wasixv2_abi::{AddressFamily, IoctlRequest};
use wasixv2_syscalls::{Caller, Errno, SysResult};
use wasixv2_vfs::Cap;
use wasixv2_vnet::{
    Domain, Interface, Network,
    iface::{
        AddMode, AddrMatch, IfAddr, Route, RouteMatch,
        flags::{IFF_BROADCAST, IFF_LOOPBACK, IFF_POINTOPOINT},
        rt::{
            IP6_RT_PRIO_USER, RT_SCOPE_HOST, RT_SCOPE_LINK, RT_SCOPE_NOWHERE, RT_SCOPE_UNIVERSE,
            RT_TABLE_MAIN, RTN_UNICAST, RTN_UNREACHABLE, RTPROT_BOOT,
        },
    },
};

use super::offset;
use crate::{Kernel, file::OpenFile, ns::NetNs, socket, socket::SocketFile};

/// `sizeof(struct ifreq)` and `IFNAMSIZ`.
const IFREQ: usize = 32;
const IFNAMSIZ: usize = 16;
/// `sizeof(struct in6_ifreq)`, `struct rtentry` and `struct in6_rtmsg` on wasm32.
const IN6_IFREQ: usize = 24;
const RTENTRY: usize = 84;
const IN6_RTMSG: usize = 72;
/// `sizeof(struct ifmap)` on wasm32.
const IFMAP: usize = 16;
const ARPHRD_ETHER: u16 = 1;
const ARPHRD_LOOPBACK: u16 = 772;
/// `RTF_*` (`include/uapi/linux/route.h`).
const RTF_GATEWAY: u16 = 0x2;
const RTF_HOST: u16 = 0x4;
const RTF_REJECT: u16 = 0x200;
/// `DEFAULT_TX_QUEUE_LEN`.
const TXQLEN: i32 = 1000;

/// The interface requests a socket answers.
pub(crate) fn is_dev_ioctl(request: IoctlRequest) -> bool {
    use IoctlRequest as R;
    matches!(
        request,
        R::SIOCGIFCONF
            | R::SIOCGIFINDEX
            | R::SIOCGIFNAME
            | R::SIOCGIFFLAGS
            | R::SIOCSIFFLAGS
            | R::SIOCGIFMTU
            | R::SIOCSIFMTU
            | R::SIOCSIFNAME
            | R::SIOCGIFHWADDR
            | R::SIOCSIFHWADDR
            | R::SIOCGIFTXQLEN
            | R::SIOCGIFMETRIC
            | R::SIOCGIFMAP
            | R::SIOCGIFADDR
            | R::SIOCGIFNETMASK
            | R::SIOCGIFBRDADDR
            | R::SIOCGIFDSTADDR
            | R::SIOCSIFADDR
            | R::SIOCSIFNETMASK
            | R::SIOCSIFBRDADDR
            | R::SIOCSIFDSTADDR
            | R::SIOCDIFADDR
            | R::SIOCADDRT
            | R::SIOCDELRT
    )
}

/// What `devinet_ioctl` does to an address.
fn is_inet_request(request: IoctlRequest) -> bool {
    use IoctlRequest as R;
    matches!(
        request,
        R::SIOCGIFADDR
            | R::SIOCGIFNETMASK
            | R::SIOCGIFBRDADDR
            | R::SIOCGIFDSTADDR
            | R::SIOCSIFADDR
            | R::SIOCSIFNETMASK
            | R::SIOCSIFBRDADDR
            | R::SIOCSIFDSTADDR
    )
}

/// `inet_abc_len`: the classful prefix of an address; `None` for a multicast one.
fn abc_len(a: Ipv4Addr) -> Option<u8> {
    let o = a.octets()[0];
    match a {
        a if o == 0 || a == Ipv4Addr::BROADCAST => Some(0),
        _ if o < 128 => Some(8),
        _ if o < 192 => Some(16),
        _ if o < 224 => Some(24),
        _ if o >= 240 => Some(32),
        _ => None,
    }
}

/// `bad_mask(mask, 0)`: a netmask must be contiguous ones. Returns its length.
fn mask_len(mask: Ipv4Addr) -> Option<u8> {
    let m = u32::from(mask);
    (m.leading_ones() + m.trailing_zeros() == 32 || m == 0).then(|| m.leading_ones() as u8)
}

fn mask_of(len: u8) -> Ipv4Addr {
    Ipv4Addr::from(
        u32::MAX
            .checked_shl(32 - u32::from(len.min(32)))
            .unwrap_or(0),
    )
}

/// A `struct sockaddr_in` with only the address, as interface requests return them.
fn sockaddr_in(ip: Ipv4Addr) -> Vec<u8> {
    socket::encode(SocketAddr::new(ip.into(), 0))
}

/// The family and IPv4 address of a `struct sockaddr` in a request.
fn sin(raw: &[u8]) -> (i32, Ipv4Addr) {
    let family = i32::from(u16::from_le_bytes([raw[0], raw[1]]));
    let ip = Ipv4Addr::new(raw[4], raw[5], raw[6], raw[7]);
    (family, ip)
}

/// The label an IPv4 address answers to: its alias, or the interface's name.
fn label<'a>(iface: &'a Interface, a: &'a IfAddr) -> &'a str {
    a.label.as_deref().unwrap_or(&iface.name)
}

/// `devinet_ioctl`'s lookup: the first IPv4 address labelled `name`, the one with local
/// address `local` among them if given (4.4BSD style).
fn find_ifa<'a>(iface: &'a Interface, name: &str, local: Option<Ipv4Addr>) -> Option<&'a IfAddr> {
    let labelled = || {
        iface
            .addresses
            .iter()
            .filter(|a| a.addr.is_ipv4() && label(iface, a) == name)
    };
    local
        .and_then(|l| labelled().find(|a| a.addr == IpAddr::V4(l)))
        .or_else(|| labelled().next())
}

/// What deletes exactly the address `a` (`inet_del_ifa`).
fn match_of(iface: &Interface, a: &IfAddr) -> AddrMatch {
    AddrMatch {
        v6: false,
        local: Some(a.addr),
        address: Some(a.peer.unwrap_or(a.addr)),
        prefix_len: a.prefix_len,
        label: Some(label(iface, a).to_string()),
    }
}

/// Re-adds a changed IPv4 address (`inet_insert_ifa` after `inet_del_ifa`).
fn replace_ifa(
    net: &dyn Network,
    iface: &Interface,
    old: &IfAddr,
    new: IfAddr,
) -> Result<(), Errno> {
    net.del_address(iface.index, &match_of(iface, old))?;
    net.add_address(iface.index, new, AddMode::ADD)
}

impl Kernel {
    /// The address family of the socket `file` is, for the requests only some answer.
    fn socket_domain(file: &OpenFile) -> Option<Domain> {
        file.downcast::<SocketFile>().map(|s| s.socket.domain())
    }

    /// An interface request on `file`, a socket of namespace `net`.
    pub(crate) async fn sys_dev_ioctl(
        &self,
        caller: &Caller<'_>,
        file: &OpenFile,
        net: &NetNs,
        request: IoctlRequest,
        arg: u32,
    ) -> SysResult<i32> {
        let domain = Self::socket_domain(file);
        // `dev_ioctl`, `devinet_ioctl`, `addrconf_*_ifaddr` and `ip_rt_ioctl` ask
        // `CAP_NET_ADMIN` over the owner of the socket's namespace (`ns_capable(net->user_ns)`).
        let admin = || match self.cred(caller).ns_capable(net.user_ns(), Cap::NetAdmin) {
            true => Ok(()),
            false => Err(Errno::EPERM),
        };
        match (request, domain) {
            (IoctlRequest::SIOCADDRT | IoctlRequest::SIOCDELRT, Some(domain)) => {
                admin()?;
                let network = net.network.as_ref().ok_or(Errno::ENODEV)?;
                return match domain {
                    Domain::Inet => self.rt_ioctl(caller, &**network, request, arg).await,
                    Domain::Inet6 => self.rt6_ioctl(caller, &**network, request, arg).await,
                };
            }
            (IoctlRequest::SIOCSIFADDR | IoctlRequest::SIOCDIFADDR, Some(Domain::Inet6)) => {
                admin()?;
                let network = net.network.as_ref().ok_or(Errno::ENODEV)?;
                return self.addrconf_ioctl(caller, &**network, request, arg).await;
            }
            (IoctlRequest::SIOCSIFNETMASK, Some(Domain::Inet6))
            | (IoctlRequest::SIOCSIFBRDADDR, Some(Domain::Inet6))
            | (IoctlRequest::SIOCSIFDSTADDR, Some(Domain::Inet6))
            | (IoctlRequest::SIOCADDRT | IoctlRequest::SIOCDELRT, None)
            | (IoctlRequest::SIOCDIFADDR, _) => return Err(Errno::ENOTTY.into()),
            (
                IoctlRequest::SIOCSIFADDR
                | IoctlRequest::SIOCSIFNETMASK
                | IoctlRequest::SIOCSIFBRDADDR
                | IoctlRequest::SIOCSIFDSTADDR,
                None,
            ) => return Err(Errno::ENOTTY.into()),
            _ => {}
        }
        let memory = caller.memory();
        if request == IoctlRequest::SIOCGIFCONF {
            return self.dev_ifconf(caller, net, arg).await;
        }
        let mut ifr = [0u8; IFREQ];
        memory.read(arg, &mut ifr).await?;
        let interfaces = net
            .network
            .as_ref()
            .map(|net| net.interfaces())
            .unwrap_or_default();
        use IoctlRequest as R;
        let setting = matches!(
            request,
            R::SIOCSIFFLAGS
                | R::SIOCSIFMTU
                | R::SIOCSIFNAME
                | R::SIOCSIFHWADDR
                | R::SIOCSIFADDR
                | R::SIOCSIFNETMASK
                | R::SIOCSIFBRDADDR
                | R::SIOCSIFDSTADDR
        );
        let iface = if request == R::SIOCGIFNAME {
            let index = i32::from_le_bytes(ifr[IFNAMSIZ..IFNAMSIZ + 4].try_into().unwrap());
            interfaces.iter().find(|i| i.index as i32 == index)
        } else {
            // Changes need `CAP_NET_ADMIN` before the interface is looked up; an address
            // change also needs an `AF_INET` address.
            if setting {
                admin()?;
            }
            if is_inet_request(request)
                && setting
                && sin(&ifr[IFNAMSIZ..]).0 != AddressFamily::AF_INET.0
            {
                return Err(Errno::EINVAL.into());
            }
            interfaces
                .iter()
                .find(|i| i.name.as_bytes() == dev_name(&ifr))
        };
        let iface = iface.ok_or(Errno::ENODEV)?;
        if is_inet_request(request) {
            let reply = self.devinet_ioctl(net, iface, request, &ifr)?;
            if let Some(value) = reply {
                memory.write(offset(arg, IFNAMSIZ)?, &value).await?;
            }
            return Ok(0);
        }
        let network = || net.network.as_ref().ok_or(Errno::ENODEV);
        let value = match request {
            R::SIOCSIFFLAGS => {
                // `ifr_flags` is a short, which `dev_change_flags` takes sign-extended.
                let flags = i16::from_le_bytes([ifr[IFNAMSIZ], ifr[IFNAMSIZ + 1]]) as i32 as u32;
                let name = full_name(&ifr);
                if domain == Some(Domain::Inet) && name.contains(':') {
                    // An alias's flags are its address's: down deletes it (`devinet_ioctl`).
                    let ifa = find_ifa(iface, &name, None).ok_or(Errno::EADDRNOTAVAIL)?;
                    if flags & wasixv2_vnet::iface::flags::IFF_UP == 0 {
                        network()?.del_address(iface.index, &match_of(iface, ifa))?;
                    }
                    return Ok(0);
                }
                network()?.change_flags(iface.index, flags)?;
                return Ok(0);
            }
            R::SIOCSIFMTU => {
                let mtu = i32::from_le_bytes(ifr[IFNAMSIZ..IFNAMSIZ + 4].try_into().unwrap());
                let mtu = u32::try_from(mtu).map_err(|_| Errno::EINVAL)?;
                network()?.set_mtu(iface.index, mtu)?;
                return Ok(0);
            }
            R::SIOCSIFNAME => {
                let raw = &ifr[IFNAMSIZ..IFNAMSIZ * 2 - 1];
                let end = raw.iter().position(|&b| b == 0).unwrap_or(raw.len());
                let name = std::str::from_utf8(&raw[..end]).map_err(|_| Errno::EINVAL)?;
                network()?.rename_link(iface.index, name)?;
                return Ok(0);
            }
            R::SIOCSIFHWADDR => {
                let family = u16::from_le_bytes([ifr[IFNAMSIZ], ifr[IFNAMSIZ + 1]]);
                if family != hw_type(iface) {
                    return Err(Errno::EINVAL.into());
                }
                let mac = ifr[IFNAMSIZ + 2..IFNAMSIZ + 8].try_into().unwrap();
                network()?.set_mac(iface.index, mac)?;
                return Ok(0);
            }
            R::SIOCGIFNAME => {
                let mut name = [0u8; IFNAMSIZ];
                let n = iface.name.len().min(IFNAMSIZ - 1);
                name[..n].copy_from_slice(&iface.name.as_bytes()[..n]);
                memory.write(arg, &name).await?;
                return Ok(0);
            }
            R::SIOCGIFINDEX => (iface.index as i32).to_le_bytes().to_vec(),
            R::SIOCGIFFLAGS => (iface.flags as u16).to_le_bytes().to_vec(),
            R::SIOCGIFMTU => (iface.mtu as i32).to_le_bytes().to_vec(),
            R::SIOCGIFTXQLEN => TXQLEN.to_le_bytes().to_vec(),
            R::SIOCGIFMETRIC => 0i32.to_le_bytes().to_vec(),
            R::SIOCGIFMAP => vec![0; IFMAP],
            // `dev_get_mac_address`: the hardware type as the family.
            _ => {
                let mut raw = hw_type(iface).to_le_bytes().to_vec();
                raw.extend(iface.mac);
                raw.resize(16, 0);
                raw
            }
        };
        memory.write(offset(arg, IFNAMSIZ)?, &value).await?;
        Ok(0)
    }

    /// `dev_ifconf` with `inet_gifconf`: a `struct ifreq` (label, local address) for every IPv4
    /// address into the `struct ifconf`'s buffer, as many whole ones as fit; with no buffer
    /// only the length all would take.
    async fn dev_ifconf(&self, caller: &Caller<'_>, net: &NetNs, arg: u32) -> SysResult<i32> {
        let memory = caller.memory();
        let mut ifc = [0u8; 8];
        memory.read(arg, &mut ifc).await?;
        let len = i32::from_le_bytes(ifc[..4].try_into().unwrap());
        let buf = u32::from_le_bytes(ifc[4..].try_into().unwrap());
        let interfaces = net
            .network
            .as_ref()
            .map(|n| n.interfaces())
            .unwrap_or_default();
        let mut out = Vec::new();
        for iface in &interfaces {
            for a in iface.addresses.iter().filter(|a| a.addr.is_ipv4()) {
                let mut ifr = [0u8; IFREQ];
                let name = label(iface, a).as_bytes();
                let n = name.len().min(IFNAMSIZ - 1);
                ifr[..n].copy_from_slice(&name[..n]);
                let IpAddr::V4(local) = a.addr else {
                    continue;
                };
                let sin = sockaddr_in(local);
                ifr[IFNAMSIZ..IFNAMSIZ + sin.len()].copy_from_slice(&sin);
                out.push(ifr);
            }
        }
        let total = if buf == 0 {
            out.len() * IFREQ
        } else {
            let fit = (len.max(0) as usize / IFREQ).min(out.len());
            let bytes: Vec<u8> = out[..fit].concat();
            memory.write(buf, &bytes).await?;
            bytes.len()
        };
        memory.write(arg, &(total as i32).to_le_bytes()).await?;
        Ok(0)
    }

    /// `devinet_ioctl` on interface `iface`: queries return the `sockaddr_in` to write back,
    /// changes `None`.
    fn devinet_ioctl(
        &self,
        net: &NetNs,
        iface: &Interface,
        request: IoctlRequest,
        ifr: &[u8; IFREQ],
    ) -> Result<Option<Vec<u8>>, Errno> {
        use IoctlRequest as R;
        let name = full_name(ifr);
        let (family, ip) = sin(&ifr[IFNAMSIZ..]);
        let query = matches!(
            request,
            R::SIOCGIFADDR | R::SIOCGIFNETMASK | R::SIOCGIFBRDADDR | R::SIOCGIFDSTADDR
        );
        let try_local = (query && family == AddressFamily::AF_INET.0).then_some(ip);
        let ifa = find_ifa(iface, &name, try_local).cloned();
        let Some(old) = ifa else {
            if request != R::SIOCSIFADDR {
                return Err(Errno::EADDRNOTAVAIL);
            }
            return self.set_ifaddr(net, iface, &name, None, ip).map(|()| None);
        };
        let v4 = |a: Option<IpAddr>| match a {
            Some(IpAddr::V4(v4)) => v4,
            _ => Ipv4Addr::UNSPECIFIED,
        };
        let local = v4(Some(old.addr));
        let reply = match request {
            R::SIOCGIFADDR => sockaddr_in(local),
            R::SIOCGIFBRDADDR => sockaddr_in(old.broadcast.unwrap_or(Ipv4Addr::UNSPECIFIED)),
            R::SIOCGIFDSTADDR => sockaddr_in(v4(Some(old.peer.unwrap_or(old.addr)))),
            R::SIOCGIFNETMASK => sockaddr_in(mask_of(old.prefix_len)),
            _ => {
                let network = net.network.as_ref().ok_or(Errno::ENODEV)?;
                let mut new = old.clone();
                new.flags = 0;
                match request {
                    R::SIOCSIFADDR => {
                        return self
                            .set_ifaddr(net, iface, &name, Some(&old), ip)
                            .map(|()| None);
                    }
                    R::SIOCSIFBRDADDR if old.broadcast == Some(ip) => return Ok(None),
                    R::SIOCSIFBRDADDR => new.broadcast = Some(ip),
                    R::SIOCSIFDSTADDR if old.peer.unwrap_or(old.addr) == IpAddr::V4(ip) => {
                        return Ok(None);
                    }
                    R::SIOCSIFDSTADDR => {
                        abc_len(ip).ok_or(Errno::EINVAL)?;
                        new.peer = (IpAddr::V4(ip) != old.addr).then_some(IpAddr::V4(ip));
                    }
                    _ => {
                        let len = mask_len(ip).ok_or(Errno::EINVAL)?;
                        if len == old.prefix_len {
                            return Ok(None);
                        }
                        new.prefix_len = len;
                        // A broadcast address that followed the old mask follows the new one.
                        let computed =
                            |len| Ipv4Addr::from(u32::from(local) | !u32::from(mask_of(len)));
                        if iface.flags & IFF_BROADCAST != 0
                            && len < 31
                            && old.broadcast == Some(computed(old.prefix_len))
                        {
                            new.broadcast = Some(computed(len));
                        }
                    }
                }
                replace_ifa(&**network, iface, &old, new)?;
                return Ok(None);
            }
        };
        Ok(Some(reply))
    }

    /// `SIOCSIFADDR`: the address of label `name` becomes `ip`, with its classful prefix and
    /// broadcast address (a point-to-point link: /32); 0.0.0.0 removes it (`inet_set_ifa`).
    fn set_ifaddr(
        &self,
        net: &NetNs,
        iface: &Interface,
        name: &str,
        old: Option<&IfAddr>,
        ip: Ipv4Addr,
    ) -> Result<(), Errno> {
        let len = abc_len(ip).ok_or(Errno::EINVAL)?;
        if old.is_some_and(|o| o.addr == IpAddr::V4(ip)) {
            return Ok(());
        }
        let network = net.network.as_ref().ok_or(Errno::ENODEV)?;
        if let Some(old) = old {
            network.del_address(iface.index, &match_of(iface, old))?;
        }
        if ip.is_unspecified() {
            return Ok(());
        }
        let mut a = IfAddr::new(ip.into(), len);
        a.scope = RT_SCOPE_UNIVERSE;
        if iface.flags & IFF_POINTOPOINT != 0 {
            a.prefix_len = 32;
        } else if iface.flags & IFF_BROADCAST != 0 && len < 31 {
            a.broadcast = Some(Ipv4Addr::from(u32::from(ip) | !u32::from(mask_of(len))));
        }
        a.label = (name != iface.name).then(|| name.to_string());
        network.add_address(iface.index, a, AddMode::ADD)
    }

    /// `addrconf_add_ifaddr` and `addrconf_del_ifaddr`: a `struct in6_ifreq`.
    async fn addrconf_ioctl(
        &self,
        caller: &Caller<'_>,
        net: &dyn Network,
        request: IoctlRequest,
        arg: u32,
    ) -> SysResult<i32> {
        let mut raw = [0u8; IN6_IFREQ];
        caller.memory().read(arg, &mut raw).await?;
        let addr = IpAddr::V6(Ipv6Addr::from(<[u8; 16]>::try_from(&raw[..16]).unwrap()));
        let prefix_len = u32::from_le_bytes(raw[16..20].try_into().unwrap());
        let index = i32::from_le_bytes(raw[20..24].try_into().unwrap());
        let prefix_len = u8::try_from(prefix_len)
            .ok()
            .filter(|&p| p <= 128)
            .ok_or(Errno::EINVAL)?;
        let index = u32::try_from(index).map_err(|_| Errno::ENODEV)?;
        if request == IoctlRequest::SIOCSIFADDR {
            net.add_address(index, IfAddr::new(addr, prefix_len), AddMode::ADD)?;
        } else {
            let which = AddrMatch {
                v6: true,
                local: Some(addr),
                address: None,
                prefix_len,
                label: None,
            };
            net.del_address(index, &which)?;
        }
        Ok(0)
    }

    /// `ip_rt_ioctl` with `rtentry_to_fib_config`: a `struct rtentry`.
    async fn rt_ioctl(
        &self,
        caller: &Caller<'_>,
        net: &dyn Network,
        request: IoctlRequest,
        arg: u32,
    ) -> SysResult<i32> {
        let memory = caller.memory();
        let mut rt = [0u8; RTENTRY];
        memory.read(arg, &mut rt).await?;
        let (dst_family, dst) = sin(&rt[4..20]);
        let (gw_family, gw) = sin(&rt[20..36]);
        let (mask_family, mask) = sin(&rt[36..52]);
        let flags = u16::from_le_bytes([rt[52], rt[53]]);
        let metric = i16::from_le_bytes([rt[64], rt[65]]);
        let dev = u32::from_le_bytes(rt[68..72].try_into().unwrap());
        let inet = AddressFamily::AF_INET.0;
        if dst_family != inet {
            return Err(Errno::EAFNOSUPPORT.into());
        }
        let mut prefix_len = 32;
        if flags & RTF_HOST == 0 {
            if mask_family != inet && (!mask.is_unspecified() || mask_family != 0) {
                return Err(Errno::EAFNOSUPPORT.into());
            }
            prefix_len = mask_len(mask).ok_or(Errno::EINVAL)?;
            if u32::from(dst) & !u32::from(mask) != 0 {
                return Err(Errno::EINVAL.into());
            }
        }
        let interfaces = net.interfaces();
        let mut oif = None;
        let mut prefsrc = None;
        if dev != 0 {
            let mut name = [0u8; IFNAMSIZ - 1];
            memory.read(dev, &mut name).await?;
            let end = name.iter().position(|&b| b == 0).unwrap_or(name.len());
            let name = String::from_utf8_lossy(&name[..end]).into_owned();
            let base = name.split(':').next().unwrap_or_default();
            let iface = interfaces
                .iter()
                .find(|i| i.name == base)
                .ok_or(Errno::ENODEV)?;
            oif = Some(iface.index);
            if name.contains(':') {
                let ifa = find_ifa(iface, &name, None).ok_or(Errno::ENODEV)?;
                prefsrc = Some(ifa.addr);
            }
        }
        let gateway = (gw_family == inet && !gw.is_unspecified()).then_some(IpAddr::V4(gw));
        let reject = flags & RTF_REJECT != 0;
        let mut scope = match reject {
            true => RT_SCOPE_HOST,
            false => RT_SCOPE_NOWHERE,
        };
        if !reject && flags & RTF_GATEWAY != 0 && gateway.is_some() {
            scope = RT_SCOPE_UNIVERSE;
        }
        let metric = u32::try_from(metric).map_or(0, |m| m.saturating_sub(1));
        let dst = IpAddr::V4(dst);
        if request == IoctlRequest::SIOCDELRT {
            let which = RouteMatch {
                table: RT_TABLE_MAIN,
                dst,
                prefix_len,
                gateway: gateway.filter(|_| !reject),
                oif,
                prefsrc,
                metric: Some(metric),
                protocol: None,
                scope: Some(scope),
                kind: reject.then_some(RTN_UNREACHABLE),
            };
            net.del_route(&which)?;
            return Ok(0);
        }
        if !reject && flags & RTF_GATEWAY != 0 && gateway.is_none() {
            return Err(Errno::EINVAL.into());
        }
        if scope == RT_SCOPE_NOWHERE {
            scope = RT_SCOPE_LINK;
        }
        let route = Route {
            table: RT_TABLE_MAIN,
            dst,
            prefix_len,
            gateway: gateway.filter(|_| !reject),
            oif: oif.filter(|_| !reject).unwrap_or(0),
            prefsrc,
            metric,
            protocol: RTPROT_BOOT,
            scope,
            kind: match reject {
                true => RTN_UNREACHABLE,
                false => RTN_UNICAST,
            },
        };
        let mode = AddMode {
            create: true,
            ..AddMode::default()
        };
        net.add_route(route, mode)?;
        Ok(0)
    }

    /// `ipv6_route_ioctl` with `rtmsg_to_fib6_config`: a `struct in6_rtmsg`.
    async fn rt6_ioctl(
        &self,
        caller: &Caller<'_>,
        net: &dyn Network,
        request: IoctlRequest,
        arg: u32,
    ) -> SysResult<i32> {
        let mut m = [0u8; IN6_RTMSG];
        caller.memory().read(arg, &mut m).await?;
        let addr = |at: usize| {
            IpAddr::V6(Ipv6Addr::from(
                <[u8; 16]>::try_from(&m[at..at + 16]).unwrap(),
            ))
        };
        let dst = addr(0);
        let gw = addr(32);
        let dst_len = u16::from_le_bytes([m[52], m[53]]);
        let metric = u32::from_le_bytes(m[56..60].try_into().unwrap());
        let flags = u32::from_le_bytes(m[64..68].try_into().unwrap());
        let index = i32::from_le_bytes(m[68..72].try_into().unwrap());
        let prefix_len = u8::try_from(dst_len)
            .ok()
            .filter(|&p| p <= 128)
            .ok_or(Errno::EINVAL)?;
        let gateway = (u32::from(RTF_GATEWAY) & flags != 0).then_some(gw);
        let oif = u32::try_from(index).ok().filter(|&i| i != 0);
        let reject = u32::from(RTF_REJECT) & flags != 0;
        if request == IoctlRequest::SIOCDELRT {
            let which = RouteMatch {
                table: RT_TABLE_MAIN,
                dst,
                prefix_len,
                gateway,
                oif,
                prefsrc: None,
                metric: (metric != 0).then_some(metric),
                protocol: None,
                scope: None,
                kind: None,
            };
            net.del_route(&which)?;
            return Ok(0);
        }
        let route = Route {
            table: RT_TABLE_MAIN,
            dst,
            prefix_len,
            gateway: gateway.filter(|_| !reject),
            oif: oif.filter(|_| !reject).unwrap_or(0),
            prefsrc: None,
            metric: match metric {
                0 => IP6_RT_PRIO_USER,
                m => m,
            },
            protocol: RTPROT_BOOT,
            scope: RT_SCOPE_UNIVERSE,
            kind: match reject {
                true => RTN_UNREACHABLE,
                false => RTN_UNICAST,
            },
        };
        let mode = AddMode {
            create: true,
            ..AddMode::default()
        };
        net.add_route(route, mode)?;
        Ok(0)
    }
}

/// `ARPHRD_*` of an interface.
fn hw_type(iface: &Interface) -> u16 {
    match iface.flags & IFF_LOOPBACK != 0 {
        true => ARPHRD_LOOPBACK,
        false => ARPHRD_ETHER,
    }
}

/// `ifr_name` as given, alias included (`eth0:1`); it ends at the last byte.
fn full_name(ifr: &[u8; IFREQ]) -> String {
    let raw = &ifr[..IFNAMSIZ - 1];
    let end = raw.iter().position(|&b| b == 0).unwrap_or(raw.len());
    String::from_utf8_lossy(&raw[..end]).into_owned()
}

/// The interface `ifr_name` names: an alias (`lo:1`) names its interface.
fn dev_name(ifr: &[u8; IFREQ]) -> &[u8] {
    let end = ifr[..IFNAMSIZ - 1]
        .iter()
        .position(|&b| b == 0 || b == b':')
        .unwrap_or(IFNAMSIZ - 1);
    &ifr[..end]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn masks_and_classes_follow_devinet() {
        assert_eq!(mask_len(Ipv4Addr::new(255, 255, 255, 0)), Some(24));
        assert_eq!(mask_len(Ipv4Addr::new(255, 0, 255, 0)), None);
        assert_eq!(mask_len(Ipv4Addr::UNSPECIFIED), Some(0));
        assert_eq!(mask_len(Ipv4Addr::BROADCAST), Some(32));
        assert_eq!(abc_len(Ipv4Addr::new(10, 0, 0, 1)), Some(8));
        assert_eq!(abc_len(Ipv4Addr::new(172, 16, 0, 1)), Some(16));
        assert_eq!(abc_len(Ipv4Addr::new(192, 168, 0, 1)), Some(24));
        assert_eq!(abc_len(Ipv4Addr::new(224, 0, 0, 1)), None);
        assert_eq!(abc_len(Ipv4Addr::UNSPECIFIED), Some(0));
        assert_eq!(mask_of(24), Ipv4Addr::new(255, 255, 255, 0));
        assert_eq!(mask_of(0), Ipv4Addr::UNSPECIFIED);
    }
}
