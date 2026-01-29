use std::{
    mem::transmute,
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
    time::Duration,
};

use virtual_net::{IpCidr, IpRoute, NetworkError};
use wasmer::{MemoryView, WasmPtr};
use wasmer_types::MemorySize;
use wasmer_wasix_types::{
    types::{
        __wasi_addr_ip4_t, __wasi_addr_ip6_t, __wasi_addr_port_t, __wasi_addr_port_u,
        __wasi_addr_t, __wasi_addr_u, __wasi_cidr_t, OptionTag,
        Route,
    },
    wasi::{Addressfamily, Errno},
};

pub mod socket;

#[allow(dead_code)]
pub(crate) fn read_ip<M: MemorySize>(
    memory: &MemoryView,
    ptr: WasmPtr<__wasi_addr_t, M>,
) -> Result<IpAddr, Errno> {
    let addr_ptr = ptr.deref(memory);
    let addr = addr_ptr.read().map_err(crate::mem_error_to_wasi)?;

    let o = addr.u.octs;
    Ok(match addr.tag {
        Addressfamily::Inet4 => IpAddr::V4(Ipv4Addr::new(o[0], o[1], o[2], o[3])),
        Addressfamily::Inet6 => {
            let [a, b, c, d, e, f, g, h] = unsafe { transmute::<[u8; 16], [u16; 8]>(o) };
            IpAddr::V6(Ipv6Addr::new(a, b, c, d, e, f, g, h))
        }
        _ => return Err(Errno::Inval),
    })
}

pub(crate) fn read_ip_v4<M: MemorySize>(
    memory: &MemoryView,
    ptr: WasmPtr<__wasi_addr_ip4_t, M>,
) -> Result<Ipv4Addr, Errno> {
    let addr_ptr = ptr.deref(memory);
    let addr = addr_ptr.read().map_err(crate::mem_error_to_wasi)?;

    let o = addr.octs;
    Ok(Ipv4Addr::new(o[0], o[1], o[2], o[3]))
}

pub(crate) fn read_ip_v6<M: MemorySize>(
    memory: &MemoryView,
    ptr: WasmPtr<__wasi_addr_ip6_t, M>,
) -> Result<Ipv6Addr, Errno> {
    let addr_ptr = ptr.deref(memory);
    let addr = addr_ptr.read().map_err(crate::mem_error_to_wasi)?;

    let [a, b, c, d, e, f, g, h] = unsafe { transmute::<[u8; 16], [u16; 8]>(addr.segs) };
    Ok(Ipv6Addr::new(a, b, c, d, e, f, g, h))
}

pub fn write_ip<M: MemorySize>(
    memory: &MemoryView,
    ptr: WasmPtr<__wasi_addr_t, M>,
    ip: IpAddr,
) -> Result<(), Errno> {
    let ip = match ip {
        IpAddr::V4(ip) => {
            let o = ip.octets();
            __wasi_addr_t {
                tag: Addressfamily::Inet4,
                _padding: 0,
                u: __wasi_addr_u {
                    octs: [o[0], o[1], o[2], o[3], 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
                },
            }
        }
        IpAddr::V6(ip) => {
            let o = ip.octets();
            __wasi_addr_t {
                tag: Addressfamily::Inet6,
                _padding: 0,
                u: __wasi_addr_u { octs: o },
            }
        }
    };

    let addr_ptr = ptr.deref(memory);
    addr_ptr.write(ip).map_err(crate::mem_error_to_wasi)?;
    Ok(())
}

#[allow(dead_code)]
pub(crate) fn read_cidr<M: MemorySize>(
    memory: &MemoryView,
    ptr: WasmPtr<__wasi_cidr_t, M>,
) -> Result<IpCidr, Errno> {
    let addr_ptr = ptr.deref(memory);
    let addr = addr_ptr.read().map_err(crate::mem_error_to_wasi)?;

    let o = addr.u.octs;
    Ok(match addr.tag {
        Addressfamily::Inet4 => IpCidr {
            ip: IpAddr::V4(Ipv4Addr::new(o[0], o[1], o[2], o[3])),
            prefix: o[4],
        },
        Addressfamily::Inet6 => {
            let [a, b, c, d, e, f, g, h] = {
                let o = [
                    o[0], o[1], o[2], o[3], o[4], o[5], o[6], o[7], o[8], o[9], o[10], o[11],
                    o[12], o[13], o[14], o[15],
                ];
                unsafe { transmute::<[u8; 16], [u16; 8]>(o) }
            };
            IpCidr {
                ip: IpAddr::V6(Ipv6Addr::new(a, b, c, d, e, f, g, h)),
                prefix: o[16],
            }
        }
        _ => return Err(Errno::Inval),
    })
}

#[allow(dead_code)]
pub(crate) fn write_cidr<M: MemorySize>(
    memory: &MemoryView,
    ptr: WasmPtr<__wasi_cidr_t, M>,
    cidr: IpCidr,
) -> Result<(), Errno> {
    const CIDR_SIZE: usize = 28;
    const CIDR_UNION_OFFSET: usize = 2;
    const CIDR_IPV4_PREFIX_OFFSET: usize = CIDR_UNION_OFFSET + 4;
    const CIDR_IPV6_PREFIX_OFFSET: usize = CIDR_UNION_OFFSET + 24;

    let mut buf = [0u8; CIDR_SIZE];
    match cidr.ip {
        IpAddr::V4(ip) => {
            buf[0] = Addressfamily::Inet4 as u8;
            let o = ip.octets();
            buf[CIDR_UNION_OFFSET..CIDR_UNION_OFFSET + 4].copy_from_slice(&o);
            buf[CIDR_IPV4_PREFIX_OFFSET] = cidr.prefix;
        }
        IpAddr::V6(ip) => {
            buf[0] = Addressfamily::Inet6 as u8;
            let o = ip.octets();
            buf[CIDR_UNION_OFFSET..CIDR_UNION_OFFSET + 16].copy_from_slice(&o);
            buf[CIDR_IPV6_PREFIX_OFFSET] = cidr.prefix;
        }
    }

    memory
        .write(ptr.offset().into(), &buf)
        .map_err(crate::mem_error_to_wasi)?;
    Ok(())
}

pub(crate) fn read_ip_port<M: MemorySize>(
    memory: &MemoryView,
    ptr: WasmPtr<__wasi_addr_port_t, M>,
) -> Result<(IpAddr, u16), Errno> {
    let addr_ptr = ptr.deref(memory);
    let addr = addr_ptr.read().map_err(crate::mem_error_to_wasi)?;
    let o = addr.u.octs;
    Ok(match addr.tag {
        Addressfamily::Inet4 => {
            let port = u16::from_ne_bytes([o[0], o[1]]);
            (IpAddr::V4(Ipv4Addr::new(o[2], o[3], o[4], o[5])), port)
        }
        Addressfamily::Inet6 => {
            let octets: [u8; 16] = o[2..18].try_into().unwrap();
            (
                IpAddr::V6(Ipv6Addr::from(octets)),
                u16::from_ne_bytes([o[0], o[1]]),
            )
        }
        _ => {
            tracing::debug!("invalid address family ({})", addr.tag as u8);
            return Err(Errno::Inval);
        }
    })
}

#[allow(dead_code)]
pub(crate) fn write_ip_port<M: MemorySize>(
    memory: &MemoryView,
    ptr: WasmPtr<__wasi_addr_port_t, M>,
    ip: IpAddr,
    port: u16,
) -> Result<(), Errno> {
    let p = port.to_be_bytes();
    let ipport = match ip {
        IpAddr::V4(ip) => {
            let o = ip.octets();
            __wasi_addr_port_t {
                tag: Addressfamily::Inet4,
                _padding: 0,
                u: __wasi_addr_port_u {
                    octs: [
                        p[0], p[1], o[0], o[1], o[2], o[3], 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                    ],
                },
            }
        }
        IpAddr::V6(ip) => {
            let o = ip.octets();
            __wasi_addr_port_t {
                tag: Addressfamily::Inet6,
                _padding: 0,
                u: __wasi_addr_port_u {
                    octs: [
                        p[0], p[1], o[0], o[1], o[2], o[3], o[4], o[5], o[6], o[7], o[8], o[9],
                        o[10], o[11], o[12], o[13], o[14], o[15],
                    ],
                },
            }
        }
    };

    let addr_ptr = ptr.deref(memory);
    addr_ptr.write(ipport).map_err(crate::mem_error_to_wasi)?;
    Ok(())
}

#[allow(dead_code)]
pub(crate) fn read_route<M: MemorySize>(
    memory: &MemoryView,
    ptr: WasmPtr<Route, M>,
) -> Result<IpRoute, Errno> {
    let route_ptr = ptr.deref(memory);
    let route = route_ptr.read().map_err(crate::mem_error_to_wasi)?;

    Ok(IpRoute {
        cidr: {
            let o = route.cidr.u.octs;
            match route.cidr.tag {
                Addressfamily::Inet4 => IpCidr {
                    ip: IpAddr::V4(Ipv4Addr::new(o[0], o[1], o[2], o[3])),
                    prefix: o[4],
                },
                Addressfamily::Inet6 => {
                    let [a, b, c, d, e, f, g, h] = {
                        let o = [
                            o[0], o[1], o[2], o[3], o[4], o[5], o[6], o[7], o[8], o[9], o[10],
                            o[11], o[12], o[13], o[14], o[15],
                        ];
                        unsafe { transmute::<[u8; 16], [u16; 8]>(o) }
                    };
                    IpCidr {
                        ip: IpAddr::V6(Ipv6Addr::new(a, b, c, d, e, f, g, h)),
                        prefix: o[16],
                    }
                }
                _ => return Err(Errno::Inval),
            }
        },
        via_router: {
            let o = route.via_router.u.octs;
            match route.via_router.tag {
                Addressfamily::Inet4 => IpAddr::V4(Ipv4Addr::new(o[0], o[1], o[2], o[3])),
                Addressfamily::Inet6 => {
                    let [a, b, c, d, e, f, g, h] = unsafe { transmute::<[u8; 16], [u16; 8]>(o) };
                    IpAddr::V6(Ipv6Addr::new(a, b, c, d, e, f, g, h))
                }
                _ => return Err(Errno::Inval),
            }
        },
        preferred_until: match route.preferred_until.tag {
            OptionTag::None => None,
            OptionTag::Some => Some(Duration::from_nanos(route.preferred_until.u)),
            _ => return Err(Errno::Inval),
        },
        expires_at: match route.expires_at.tag {
            OptionTag::None => None,
            OptionTag::Some => Some(Duration::from_nanos(route.expires_at.u)),
            _ => return Err(Errno::Inval),
        },
    })
}

pub(crate) fn write_route<M: MemorySize>(
    memory: &MemoryView,
    ptr: WasmPtr<Route, M>,
    route: IpRoute,
) -> Result<(), Errno> {
    const ROUTE_SIZE: usize = 176;
    const ROUTE_CIDR_OFFSET: usize = 0;
    const ROUTE_VIA_OFFSET: usize = 28;
    const ROUTE_PREFERRED_OFFSET: usize = 144;
    const ROUTE_EXPIRES_OFFSET: usize = 160;
    const ADDR_SIZE: usize = 110;
    const ADDR_UNION_OFFSET: usize = 2;

    let mut buf = [0u8; ROUTE_SIZE];

    // cidr
    let mut cidr_buf = [0u8; 28];
    match route.cidr.ip {
        IpAddr::V4(ip) => {
            cidr_buf[0] = Addressfamily::Inet4 as u8;
            let o = ip.octets();
            cidr_buf[2..6].copy_from_slice(&o);
            cidr_buf[6] = route.cidr.prefix;
        }
        IpAddr::V6(ip) => {
            cidr_buf[0] = Addressfamily::Inet6 as u8;
            let o = ip.octets();
            cidr_buf[2..18].copy_from_slice(&o);
            cidr_buf[26] = route.cidr.prefix;
        }
    }
    buf[ROUTE_CIDR_OFFSET..ROUTE_CIDR_OFFSET + cidr_buf.len()].copy_from_slice(&cidr_buf);

    // via_router
    let mut addr_buf = [0u8; ADDR_SIZE];
    match route.via_router {
        IpAddr::V4(ip) => {
            addr_buf[0] = Addressfamily::Inet4 as u8;
            let o = ip.octets();
            addr_buf[ADDR_UNION_OFFSET..ADDR_UNION_OFFSET + 4].copy_from_slice(&o);
        }
        IpAddr::V6(ip) => {
            addr_buf[0] = Addressfamily::Inet6 as u8;
            let o = ip.octets();
            addr_buf[ADDR_UNION_OFFSET..ADDR_UNION_OFFSET + 16].copy_from_slice(&o);
        }
    }
    buf[ROUTE_VIA_OFFSET..ROUTE_VIA_OFFSET + addr_buf.len()].copy_from_slice(&addr_buf);

    // preferred_until
    let mut pref_buf = [0u8; 16];
    match route.preferred_until {
        None => {
            pref_buf[0] = OptionTag::None as u8;
        }
        Some(u) => {
            pref_buf[0] = OptionTag::Some as u8;
            pref_buf[8..16].copy_from_slice(&(u.as_nanos() as u64).to_le_bytes());
        }
    }
    buf[ROUTE_PREFERRED_OFFSET..ROUTE_PREFERRED_OFFSET + pref_buf.len()]
        .copy_from_slice(&pref_buf);

    // expires_at
    let mut exp_buf = [0u8; 16];
    match route.expires_at {
        None => {
            exp_buf[0] = OptionTag::None as u8;
        }
        Some(u) => {
            exp_buf[0] = OptionTag::Some as u8;
            exp_buf[8..16].copy_from_slice(&(u.as_nanos() as u64).to_le_bytes());
        }
    }
    buf[ROUTE_EXPIRES_OFFSET..ROUTE_EXPIRES_OFFSET + exp_buf.len()]
        .copy_from_slice(&exp_buf);

    memory
        .write(ptr.offset().into(), &buf)
        .map_err(crate::mem_error_to_wasi)?;
    Ok(())
}

pub fn net_error_into_wasi_err(net_error: NetworkError) -> Errno {
    match net_error {
        NetworkError::InvalidFd => Errno::Badf,
        NetworkError::AlreadyExists => Errno::Exist,
        NetworkError::Lock => Errno::Io,
        NetworkError::IOError => Errno::Io,
        NetworkError::AddressInUse => Errno::Addrinuse,
        NetworkError::AddressNotAvailable => Errno::Addrnotavail,
        NetworkError::BrokenPipe => Errno::Pipe,
        NetworkError::ConnectionAborted => Errno::Connaborted,
        NetworkError::ConnectionRefused => Errno::Connrefused,
        NetworkError::ConnectionReset => Errno::Connreset,
        NetworkError::Interrupted => Errno::Intr,
        NetworkError::InvalidData => Errno::Io,
        NetworkError::InvalidInput => Errno::Inval,
        NetworkError::NotConnected => Errno::Notconn,
        NetworkError::NoDevice => Errno::Nodev,
        NetworkError::PermissionDenied => Errno::Perm,
        NetworkError::TimedOut => Errno::Timedout,
        NetworkError::UnexpectedEof => Errno::Proto,
        NetworkError::WouldBlock => Errno::Again,
        NetworkError::WriteZero => Errno::Nospc,
        NetworkError::TooManyOpenFiles => Errno::Mfile,
        NetworkError::InsufficientMemory => Errno::Nomem,
        NetworkError::Unsupported => Errno::Notsup,
        NetworkError::UnknownError => Errno::Io,
    }
}
