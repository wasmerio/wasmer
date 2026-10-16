//! The `port_*` queries v1 libc uses (`getifaddrs`-style address lists and the MAC), read
//! from the network namespace through a `NETLINK_ROUTE` socket, as Linux tools do.
//!
//! The configuration calls (`port_bridge`, `port_addr_add`, routes, DHCP, ...) are stubs:
//! a v2 guest's network is configured by the host. `port_route_list` is one too, until the
//! kernel answers `RTM_GETROUTE`.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use wasixv2_abi::{AddressFamily, GuestPtrMut, MsgFlags, SocketFlags, SocketType};
use wasixv2_syscalls::{Control, Syscalls, UserSliceMut};
use wasmer::RuntimeError;

use crate::{
    abi::{
        Errno, GuestLayout,
        sock::{AddrCidr, HardwareAddress},
    },
    errno::V1Result,
    shim::{self, Env, Frame},
};

const NETLINK_ROUTE: i32 = 0;
const NLMSG_HDR: usize = 16;
const NLMSG_ERROR: u16 = 2;
const NLMSG_DONE: u16 = 3;
const NLM_F_REQUEST: u16 = 1;
const NLM_F_DUMP: u16 = 0x300;
const RTM_NEWLINK: u16 = 16;
const RTM_GETLINK: u16 = 18;
const RTM_NEWADDR: u16 = 20;
const RTM_GETADDR: u16 = 22;
const IFLA_ADDRESS: u16 = 1;
const IFA_ADDRESS: u16 = 1;
const ARPHRD_LOOPBACK: u16 = 772;
/// `sizeof(struct ifinfomsg)` and `sizeof(struct ifaddrmsg)`.
const IFINFOMSG: usize = 16;
const IFADDRMSG: usize = 8;
/// Room for one dump datagram.
const DUMP_ROOM: u32 = 64 * 1024;

/// A dump request for `kind` with an all-zero family header of `body` bytes.
fn request(kind: u16, body: usize) -> Vec<u8> {
    let len = NLMSG_HDR + body;
    let mut msg = Vec::with_capacity(len);
    msg.extend_from_slice(&(len as u32).to_le_bytes());
    msg.extend_from_slice(&kind.to_le_bytes());
    msg.extend_from_slice(&(NLM_F_REQUEST | NLM_F_DUMP).to_le_bytes());
    msg.extend_from_slice(&1u32.to_le_bytes()); // seq
    msg.extend_from_slice(&0u32.to_le_bytes()); // pid: the kernel
    msg.resize(len, 0);
    msg
}

/// Appends the messages in `data` to `out` as (type, payload); whether `NLMSG_DONE` ended the
/// dump. An `NLMSG_ERROR` ends it with its errno.
fn messages(data: &[u8], out: &mut Vec<(u16, Vec<u8>)>) -> Result<bool, wasixv2_abi::Errno> {
    let mut at = 0;
    while data.len().saturating_sub(at) >= NLMSG_HDR {
        let h = &data[at..];
        let len = u32::from_le_bytes(h[0..4].try_into().unwrap()) as usize;
        if len < NLMSG_HDR || len > h.len() {
            break;
        }
        let kind = u16::from_le_bytes([h[4], h[5]]);
        let payload = &h[NLMSG_HDR..len];
        match kind {
            NLMSG_DONE => return Ok(true),
            NLMSG_ERROR => {
                let errno = payload
                    .get(..4)
                    .map_or(0, |e| i32::from_le_bytes(e.try_into().unwrap()));
                if errno != 0 {
                    return Err(wasixv2_abi::Errno(-errno));
                }
            }
            _ => out.push((kind, payload.to_vec())),
        }
        at += len.next_multiple_of(4);
    }
    Ok(false)
}

/// The attributes after a family header of `header` bytes, as (type, data).
fn attributes(payload: &[u8], header: usize) -> Vec<(u16, &[u8])> {
    let mut attrs = Vec::new();
    let mut at = header;
    while payload.len().saturating_sub(at) >= 4 {
        let len = u16::from_le_bytes([payload[at], payload[at + 1]]) as usize;
        let kind = u16::from_le_bytes([payload[at + 2], payload[at + 3]]);
        if len < 4 || at + len > payload.len() {
            break;
        }
        attrs.push((kind, &payload[at + 4..at + len]));
        at += len.next_multiple_of(4);
    }
    attrs
}

/// Dumps `kind` over a fresh netlink socket.
async fn dump<S: Syscalls + Control + 'static>(
    frame: &Frame<'_, S>,
    kind: u16,
    body: usize,
) -> V1Result<Vec<(u16, Vec<u8>)>> {
    let caller = frame.caller();
    let ty = SocketType::SOCK_DGRAM.0 | SocketFlags::SOCK_CLOEXEC.0 as i32;
    let fd = frame
        .syscalls
        .socket(&caller, AddressFamily::AF_NETLINK, ty, NETLINK_ROUTE)
        .await?;
    let result = async {
        let msg = frame.mem.alloc_bytes(&request(kind, body))?;
        frame.syscalls.write(&caller, fd, msg).await?;
        let buf = frame.mem.alloc_zeroed(DUMP_ROOM)?;
        let mut out = Vec::new();
        loop {
            let n = frame
                .syscalls
                .recvfrom(
                    &caller,
                    fd,
                    UserSliceMut::new(buf.addr, buf.len),
                    MsgFlags(0),
                    GuestPtrMut::NULL,
                    GuestPtrMut::NULL,
                )
                .await?;
            let data = frame.mem.take_bytes(buf, n as usize);
            if messages(&data, &mut out)? || n == 0 {
                return Ok(out);
            }
        }
    }
    .await;
    // Closing never blocks, so it is safe after an interrupted receive.
    let _ = frame.syscalls.close(&caller, fd).await;
    result
}

/// Every address of every interface, with its prefix length.
fn addresses(msgs: &[(u16, Vec<u8>)]) -> Vec<AddrCidr> {
    let mut out = Vec::new();
    for (kind, payload) in msgs {
        if *kind != RTM_NEWADDR || payload.len() < IFADDRMSG {
            continue;
        }
        let (family, prefix) = (i32::from(payload[0]), payload[1]);
        for (attr, data) in attributes(payload, IFADDRMSG) {
            if attr != IFA_ADDRESS {
                continue;
            }
            let ip = match (family, data.len()) {
                (f, 4) if f == AddressFamily::AF_INET.0 => {
                    IpAddr::V4(Ipv4Addr::from(<[u8; 4]>::try_from(data).unwrap()))
                }
                (f, 16) if f == AddressFamily::AF_INET6.0 => {
                    IpAddr::V6(Ipv6Addr::from(<[u8; 16]>::try_from(data).unwrap()))
                }
                _ => continue,
            };
            out.push(AddrCidr::new(ip, prefix));
        }
    }
    out
}

/// The hardware address of the first non-loopback interface (loopback's if it is the only one).
fn mac(msgs: &[(u16, Vec<u8>)]) -> Option<[u8; 6]> {
    let mut fallback = None;
    for (kind, payload) in msgs {
        if *kind != RTM_NEWLINK || payload.len() < IFINFOMSG {
            continue;
        }
        let loopback = u16::from_le_bytes([payload[2], payload[3]]) == ARPHRD_LOOPBACK;
        let Some(mac) = attributes(payload, IFINFOMSG)
            .into_iter()
            .find(|(attr, data)| *attr == IFLA_ADDRESS && data.len() == 6)
            .map(|(_, data)| <[u8; 6]>::try_from(data).unwrap())
        else {
            continue;
        };
        if !loopback {
            return Some(mac);
        }
        fallback.get_or_insert(mac);
    }
    fallback
}

/// `port_addr_list(addrs, naddrs)`: `*naddrs` holds the room on entry and the count on
/// return; `OVERFLOW` (with the count written) when they do not fit.
pub(crate) async fn port_addr_list<S: Syscalls + Control + 'static>(
    env: Env<S>,
    addrs: i32,
    naddrs: i32,
) -> Result<i32, RuntimeError> {
    shim::errno(&env, async |frame| {
        let room: u32 = frame.read(naddrs as u32).await?;
        let list = addresses(&dump(frame, RTM_GETADDR, IFADDRMSG).await?);
        frame.write(naddrs as u32, &(list.len() as u32)).await?;
        if list.len() > room as usize {
            return shim::fail(Errno::OVERFLOW);
        }
        for (i, addr) in list.iter().enumerate() {
            let at = (addrs as u32).wrapping_add((i * AddrCidr::SIZE) as u32);
            frame.write(at, addr).await?;
        }
        Ok(())
    })
    .await
}

pub(crate) async fn port_mac<S: Syscalls + Control + 'static>(
    env: Env<S>,
    ro_mac: i32,
) -> Result<i32, RuntimeError> {
    shim::errno(&env, async |frame| {
        let Some(octets) = mac(&dump(frame, RTM_GETLINK, IFINFOMSG).await?) else {
            return shim::fail(Errno::NODEV);
        };
        frame
            .write(ro_mac as u32, &HardwareAddress { octets })
            .await
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nlmsg(kind: u16, payload: &[u8]) -> Vec<u8> {
        let len = NLMSG_HDR + payload.len();
        let mut out = Vec::new();
        out.extend_from_slice(&(len as u32).to_le_bytes());
        out.extend_from_slice(&kind.to_le_bytes());
        out.extend_from_slice(&[0; 10]);
        out.extend_from_slice(payload);
        out.resize(len.next_multiple_of(4), 0);
        out
    }

    fn rta(kind: u16, data: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&((4 + data.len()) as u16).to_le_bytes());
        out.extend_from_slice(&kind.to_le_bytes());
        out.extend_from_slice(data);
        out.resize(out.len().next_multiple_of(4), 0);
        out
    }

    #[test]
    fn parses_an_address_dump() {
        let mut v4 = vec![AddressFamily::AF_INET.0 as u8, 8, 0, 254, 1, 0, 0, 0];
        v4.extend(rta(IFA_ADDRESS, &[127, 0, 0, 1]));
        let mut v6 = vec![AddressFamily::AF_INET6.0 as u8, 64, 0, 0, 2, 0, 0, 0];
        v6.extend(rta(
            IFA_ADDRESS,
            &[0xfe, 0x80, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1],
        ));
        let mut data = nlmsg(RTM_NEWADDR, &v4);
        data.extend(nlmsg(RTM_NEWADDR, &v6));
        data.extend(nlmsg(NLMSG_DONE, &0i32.to_le_bytes()));
        let mut msgs = Vec::new();
        assert_eq!(messages(&data, &mut msgs), Ok(true));
        let list = addresses(&msgs);
        assert_eq!(
            list,
            [
                AddrCidr::new("127.0.0.1".parse().unwrap(), 8),
                AddrCidr::new("fe80::1".parse().unwrap(), 64)
            ]
        );
    }

    #[test]
    fn prefers_a_real_mac() {
        let mut lo = vec![0, 0];
        lo.extend(ARPHRD_LOOPBACK.to_le_bytes());
        lo.extend([0; 12]);
        lo.extend(rta(IFLA_ADDRESS, &[0; 6]));
        let mut eth = vec![0, 0, 1, 0];
        eth.extend([0; 12]);
        eth.extend(rta(IFLA_ADDRESS, &[2, 0, 0, 0, 0, 2]));
        let msgs = vec![(RTM_NEWLINK, lo), (RTM_NEWLINK, eth)];
        assert_eq!(mac(&msgs), Some([2, 0, 0, 0, 0, 2]));
        assert_eq!(mac(&msgs[..1]), Some([0; 6]));
    }

    #[test]
    fn errors_end_a_dump() {
        let data = nlmsg(NLMSG_ERROR, &(-95i32).to_le_bytes());
        assert_eq!(
            messages(&data, &mut Vec::new()),
            Err(wasixv2_abi::Errno::EOPNOTSUPP)
        );
    }
}
