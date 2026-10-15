//! Network backend that carries outbound guest connections over host sockets and hands
//! connections accepted on published host ports to guest listeners.
//!
//! One reactor thread per [`HostBackend`] runs a `mio::Poll` over every host socket of the
//! backend. Guest-side objects ([`BackendStream`], [`BackendDatagram`]) are buffers between
//! the guest and that thread: the guest reads and writes the buffers without blocking, the
//! reactor moves bytes between the buffers and the host sockets and wakes the objects'
//! `WaitQueue`s. Unix hosts only for now (TODO: Windows); nothing here uses tokio.
//!
//! [`BackendStream`]: wasixv2_vnet::BackendStream
//! [`BackendDatagram`]: wasixv2_vnet::BackendDatagram

#[cfg(unix)]
mod errno;
#[cfg(unix)]
mod reactor;

#[cfg(unix)]
pub use errno::errno_from_io;
#[cfg(unix)]
pub use reactor::HostBackend;

/// DNS policy helper: the name servers of the host's `/etc/resolv.conf` as the guest can
/// reach them. Loopback resolvers (systemd-resolved's 127.0.0.53, say) are only reachable
/// through the gateway alias, so they become `alias`, or are dropped without one.
pub fn host_nameservers(
    resolv_conf: &str,
    alias: Option<std::net::IpAddr>,
) -> Vec<std::net::IpAddr> {
    let mut out = Vec::new();
    for line in resolv_conf.lines() {
        let mut words = line.split_whitespace();
        if words.next() != Some("nameserver") {
            continue;
        }
        let Some(Ok(ip)) = words
            .next()
            .map(|w| w.split('%').next().unwrap_or(w).parse::<std::net::IpAddr>())
        else {
            continue;
        };
        let ip = if ip.is_loopback() {
            match alias {
                Some(a) if a.is_ipv4() == ip.is_ipv4() => a,
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

    #[test]
    fn loopback_nameservers_go_through_the_alias() {
        let conf =
            "# c\nnameserver 127.0.0.53\nnameserver 1.1.1.1\nnameserver ::1\noptions edns0\n";
        let alias = Some("10.0.2.2".parse().unwrap());
        assert_eq!(
            host_nameservers(conf, alias),
            vec![
                "10.0.2.2".parse::<std::net::IpAddr>().unwrap(),
                "1.1.1.1".parse().unwrap()
            ]
        );
        assert_eq!(
            host_nameservers(conf, None),
            vec!["1.1.1.1".parse::<std::net::IpAddr>().unwrap()]
        );
    }
}
