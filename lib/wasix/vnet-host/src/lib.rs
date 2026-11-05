//! Network backend that carries outbound guest connections over host sockets and hands
//! connections accepted on published host ports to guest listeners.
//!
//! One reactor thread per [`HostBackend`] runs a `mio::Poll` over every host socket of the
//! backend. Guest-side objects ([`BackendStream`], [`BackendDatagram`]) are buffers between
//! the guest and that thread: the guest reads and writes the buffers without blocking, the
//! reactor moves bytes between the buffers and the host sockets and wakes the objects'
//! `WaitQueue`s. Unix hosts only for now (TODO: Windows); nothing here uses tokio.
//!
//! The namespace's DNS server asks the backend's resolver ([`ResolverConfig`]), which runs the
//! host's blocking lookups (hosts file, `getaddrinfo`, `getnameinfo`, the host's name servers)
//! on worker threads of its own and caches what they found briefly.
//!
//! With a [`VsockConfig`] the backend is also the machine's vsock g2h transport: streams to
//! the host's CIDs go to host Unix sockets by firecracker's `<path>_<port>` convention, or to
//! the host's own `AF_VSOCK`.
//!
//! [`BackendStream`]: wasixv2_vnet::BackendStream
//! [`BackendDatagram`]: wasixv2_vnet::BackendDatagram

#[cfg(unix)]
mod errno;
#[cfg(unix)]
mod reactor;
#[cfg(unix)]
mod resolver;

#[cfg(unix)]
pub use errno::errno_from_io;
#[cfg(unix)]
pub use reactor::{HostBackend, VsockConfig, VsockRoute};
#[cfg(unix)]
pub use resolver::ResolverConfig;

/// Where systemd-resolved lists the upstream servers its stub at `127.0.0.53` asks.
pub const SYSTEMD_RESOLVED_UPSTREAM: &str = "/run/systemd/resolve/resolv.conf";

/// DNS policy helper: the name servers of the host's `/etc/resolv.conf` (`resolv_conf`) as the
/// guest can reach them. The gateway alias `alias` is the host's `127.0.0.1` (or `::1`), so a
/// resolver listening there becomes the alias; one on another loopback address, such as
/// systemd-resolved's stub at `127.0.0.53` or dnsmasq's at `127.0.1.1`, cannot be reached and
/// is dropped. When that leaves none, the servers of `upstream` count instead, the file
/// systemd-resolved writes its upstream servers to ([`SYSTEMD_RESOLVED_UPSTREAM`]), as
/// container runtimes do for a host whose resolver is a local stub.
pub fn host_nameservers(
    resolv_conf: &str,
    upstream: Option<&str>,
    alias: Option<std::net::IpAddr>,
) -> Vec<std::net::IpAddr> {
    let found = reachable_nameservers(resolv_conf, alias);
    match (found.is_empty(), upstream) {
        (true, Some(upstream)) => reachable_nameservers(upstream, alias),
        _ => found,
    }
}

fn reachable_nameservers(
    resolv_conf: &str,
    alias: Option<std::net::IpAddr>,
) -> Vec<std::net::IpAddr> {
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

    let mut out = Vec::new();
    for line in resolv_conf.lines() {
        let mut words = line.split_whitespace();
        if words.next() != Some("nameserver") {
            continue;
        }
        let Some(Ok(ip)) = words
            .next()
            .map(|w| w.split('%').next().unwrap_or(w).parse::<IpAddr>())
        else {
            continue;
        };
        let ip = if ip.is_loopback() {
            let host_loopback = match ip {
                IpAddr::V4(_) => IpAddr::V4(Ipv4Addr::LOCALHOST),
                IpAddr::V6(_) => IpAddr::V6(Ipv6Addr::LOCALHOST),
            };
            match alias {
                Some(a) if ip == host_loopback && a.is_ipv4() == ip.is_ipv4() => a,
                _ => continue,
            }
        } else {
            ip
        };
        if !out.contains(&ip) {
            out.push(ip);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ips(list: &[&str]) -> Vec<std::net::IpAddr> {
        list.iter().map(|ip| ip.parse().unwrap()).collect()
    }

    #[test]
    fn host_loopback_nameservers_go_through_the_alias() {
        let conf = "# c\nnameserver 127.0.0.1\nnameserver 1.1.1.1\nnameserver ::1\noptions edns0\n";
        let alias = Some("10.0.2.2".parse().unwrap());
        assert_eq!(
            host_nameservers(conf, None, alias),
            ips(&["10.0.2.2", "1.1.1.1"])
        );
        assert_eq!(host_nameservers(conf, None, None), ips(&["1.1.1.1"]));
    }

    /// systemd-resolved's stub listens on `127.0.0.53`, which the alias (the host's
    /// `127.0.0.1`) does not reach: its upstream servers serve the guest instead.
    #[test]
    fn a_local_stub_resolver_gives_way_to_its_upstream_servers() {
        let stub = "nameserver 127.0.0.53\noptions edns0 trust-ad\nsearch .\n";
        let upstream = "nameserver 1.1.1.1\nnameserver 2606:4700:4700::1111\nnameserver 1.1.1.1\n";
        let alias = Some("10.0.2.2".parse().unwrap());
        assert_eq!(
            host_nameservers(stub, Some(upstream), alias),
            ips(&["1.1.1.1", "2606:4700:4700::1111"])
        );
        assert_eq!(host_nameservers(stub, None, alias), ips(&[]));
        // A reachable server in the host's own file wins.
        let mixed = "nameserver 127.0.0.53\nnameserver 9.9.9.9\n";
        assert_eq!(
            host_nameservers(mixed, Some(upstream), alias),
            ips(&["9.9.9.9"])
        );
        // dnsmasq's 127.0.1.1 is not the host's 127.0.0.1 either.
        assert_eq!(
            host_nameservers("nameserver 127.0.1.1\n", None, alias),
            ips(&[])
        );
    }
}
