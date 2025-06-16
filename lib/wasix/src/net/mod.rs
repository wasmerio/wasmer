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
        OptionTag, OptionTimestamp, Route, __wasi_addr_ip4_t, __wasi_addr_ip6_t,
        __wasi_addr_port_t, __wasi_addr_port_u, __wasi_addr_t, __wasi_addr_u, __wasi_cidr_t,
        __wasi_cidr_u,
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
    let p = cidr.prefix;
    let cidr = match cidr.ip {
        IpAddr::V4(ip) => {
            let o = ip.octets();
            __wasi_cidr_t {
                tag: Addressfamily::Inet4,
                _padding: 0,
                u: __wasi_cidr_u {
                    octs: [
                        o[0], o[1], o[2], o[3], p, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                    ],
                },
            }
        }
        IpAddr::V6(ip) => {
            let o = ip.octets();
            __wasi_cidr_t {
                tag: Addressfamily::Inet6,
                _padding: 0,
                u: __wasi_cidr_u {
                    octs: [
                        o[0], o[1], o[2], o[3], o[4], o[5], o[6], o[7], o[8], o[9], o[10], o[11],
                        o[12], o[13], o[14], o[15], p,
                    ],
                },
            }
        }
    };

    let addr_ptr = ptr.deref(memory);
    addr_ptr.write(cidr).map_err(crate::mem_error_to_wasi)?;
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
        },
        expires_at: match route.expires_at.tag {
            OptionTag::None => None,
            OptionTag::Some => Some(Duration::from_nanos(route.expires_at.u)),
        },
    })
}

pub(crate) fn write_route<M: MemorySize>(
    memory: &MemoryView,
    ptr: WasmPtr<Route, M>,
    route: IpRoute,
) -> Result<(), Errno> {
    let cidr = {
        let p = route.cidr.prefix;
        match route.cidr.ip {
            IpAddr::V4(ip) => {
                let o = ip.octets();
                __wasi_cidr_t {
                    tag: Addressfamily::Inet4,
                    _padding: 0,
                    u: __wasi_cidr_u {
                        octs: [
                            o[0], o[1], o[2], o[3], p, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                        ],
                    },
                }
            }
            IpAddr::V6(ip) => {
                let o = ip.octets();
                __wasi_cidr_t {
                    tag: Addressfamily::Inet6,
                    _padding: 0,
                    u: __wasi_cidr_u {
                        octs: [
                            o[0], o[1], o[2], o[3], o[4], o[5], o[6], o[7], o[8], o[9], o[10],
                            o[11], o[12], o[13], o[14], o[15], p,
                        ],
                    },
                }
            }
        }
    };
    let via_router = match route.via_router {
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
    let preferred_until = match route.preferred_until {
        None => OptionTimestamp {
            tag: OptionTag::None,
            u: 0,
        },
        Some(u) => OptionTimestamp {
            tag: OptionTag::Some,
            u: u.as_nanos() as u64,
        },
    };
    let expires_at = match route.expires_at {
        None => OptionTimestamp {
            tag: OptionTag::None,
            u: 0,
        },
        Some(u) => OptionTimestamp {
            tag: OptionTag::Some,
            u: u.as_nanos() as u64,
        },
    };

    let route = Route {
        cidr,
        via_router,
        preferred_until,
        expires_at,
    };

    let route_ptr = ptr.deref(memory);
    route_ptr.write(route).map_err(crate::mem_error_to_wasi)?;
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
