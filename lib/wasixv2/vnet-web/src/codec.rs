//! Classic virtual-net websocket wire subset, using bincode's legacy fixed-width format.
//!
//! Tags and field order are from `lib/virtual-net/src/meta.rs`, not a guest ABI. Keeping this
//! adapter private to the driver avoids a runtime dependency on the v1 networking stack.
use crate::transport::{Command, Event};
use serde::{Serialize, de::DeserializeOwned};
use std::{
    net::{IpAddr, SocketAddr},
    time::Duration,
};
use wasixv2_abi::Errno;
use wasixv2_vnet::{Answer, Found, Lookup, LookupError, Shutdown};

/// Maximum complete websocket message, checked before decoding any variable-length field.
pub const MAX_FRAME: usize = 1024 * 1024;

fn pack(value: impl Serialize) -> Result<Vec<u8>, Errno> {
    bincode::serde::encode_to_vec(value, bincode::config::legacy()).map_err(|_| Errno::EPROTO)
}

/// Encode only records this backend needs. Unsupported DNS forms belong to an explicitly
/// configured resolver provider (e.g. DoH); the classic Resolve record only names a host.
pub fn encode(command: &Command) -> Result<Vec<u8>, Errno> {
    let out = match command {
        Command::Connect {
            request,
            socket,
            peer,
        } => {
            let local = SocketAddr::new(
                if peer.is_ipv4() {
                    IpAddr::from([0, 0, 0, 0])
                } else {
                    IpAddr::from([0u16; 8])
                },
                0,
            );
            pack((0u32, 19u32, socket, local, peer, Some(request)))?
        }
        Command::Bind {
            request,
            socket,
            addr,
        } => pack((0u32, 17u32, socket, addr, false, false, Some(request)))?,
        Command::Listen {
            request,
            socket,
            addr,
        } => pack((
            0u32,
            15u32,
            socket,
            addr,
            addr.is_ipv6(),
            false,
            false,
            Some(request),
        ))?,
        Command::Lookup {
            request,
            lookup: Lookup::Name(host),
        } => pack((
            0u32,
            20u32,
            format!("{}.", host.trim_end_matches('.')),
            None::<u16>,
            None::<IpAddr>,
            Some(request),
        ))?,
        Command::Lookup { .. } => return Err(Errno::EOPNOTSUPP),
        Command::Send {
            request,
            socket,
            data,
        } => pack((2u32, socket, data, Some(request)))?,
        // The reference server ignores SendTo entirely without Some(req_id), although it
        // sends no success acknowledgment. Zero is reserved for these unacknowledged sends.
        Command::SendTo { socket, data, addr } => pack((3u32, socket, data, addr, Some(0u64)))?,
        Command::Close { socket } => pack((1u32, socket, 21u32, None::<u64>))?,
        Command::Reset { socket } => {
            pack((1u32, socket, 30u32, Some(Duration::ZERO), None::<u64>))?
        }
        Command::Shutdown {
            request,
            socket,
            how,
        } => {
            let how = match how {
                Shutdown::Read => 0u32,
                Shutdown::Write => 1,
                Shutdown::Both => 2,
            };
            pack((1u32, socket, 44u32, how, Some(request)))?
        }
        Command::Accept { socket, child } => pack((1u32, socket, 24u32, child, None::<u64>))?,
    };
    if out.len() > MAX_FRAME {
        return Err(Errno::EMSGSIZE);
    }
    Ok(out)
}

struct Reader<'a>(&'a [u8]);
impl Reader<'_> {
    fn get<T: DeserializeOwned>(&mut self) -> Result<T, Errno> {
        let (value, n) = bincode::serde::decode_from_slice(
            self.0,
            bincode::config::legacy().with_limit::<MAX_FRAME>(),
        )
        .map_err(|_| Errno::EPROTO)?;
        self.0 = &self.0[n..];
        Ok(value)
    }
}

/// Decode a complete message; malformed, oversized and trailing data are protocol failures.
pub fn decode(frame: &[u8]) -> Result<Event, Errno> {
    if frame.len() > MAX_FRAME {
        return Err(Errno::EMSGSIZE);
    }
    let mut r = Reader(frame);
    let event = match r.get::<u32>()? {
        0 => {
            let request = r.get()?;
            match r.get::<u32>()? {
                0 => Event::Complete {
                    request,
                    result: Ok(()),
                },
                1 => Event::Complete {
                    request,
                    result: Err(network_error(r.get()?)?),
                },
                5 => Event::Lookup {
                    request,
                    result: Ok(Found {
                        answer: Answer::Addrs {
                            canonical: String::new(),
                            addrs: r.get()?,
                        },
                        ttl: 30,
                    }),
                },
                // Connect/listen/bind return their socket ID; it is assigned by the client.
                12 => {
                    let _: u64 = r.get()?;
                    Event::Complete {
                        request,
                        result: Ok(()),
                    }
                }
                _ => return Err(Errno::EPROTO),
            }
        }
        1 => Event::Data {
            socket: r.get()?,
            data: r.get()?,
        },
        2 => Event::Datagram {
            socket: r.get()?,
            data: r.get()?,
            addr: r.get()?,
        },
        3 => {
            let socket = r.get()?;
            let request = r.get()?;
            let amount: u64 = r.get()?;
            Event::Sent {
                socket,
                request,
                amount: amount.try_into().map_err(|_| Errno::EPROTO)?,
            }
        }
        4 => Event::SendError {
            socket: r.get()?,
            request: r.get()?,
            error: network_error(r.get()?)?,
        },
        5 => Event::Accepted {
            socket: r.get()?,
            child: r.get()?,
            peer: r.get()?,
        },
        6 => Event::Eof { socket: r.get()? },
        _ => return Err(Errno::EPROTO),
    };
    if !r.0.is_empty() {
        return Err(Errno::EPROTO);
    }
    Ok(event)
}

/// Explicit NetworkError variants, never guessed from a websocket close code. The codec is
/// transport translation only; Linux socket errno policy stays in vnet/kernel.
fn network_error(tag: u32) -> Result<Errno, Errno> {
    Ok(match tag {
        0 => Errno::EBADF,
        1 => Errno::EEXIST,
        2 | 3 | 12 | 19 | 21 | 26 => Errno::EIO,
        4 => Errno::EADDRINUSE,
        5 => Errno::EADDRNOTAVAIL,
        6 => Errno::EPIPE,
        7 => Errno::ENOMEM,
        8 => Errno::ECONNABORTED,
        9 => Errno::ECONNREFUSED,
        10 => Errno::ECONNRESET,
        11 => Errno::EINTR,
        13 => Errno::EINVAL,
        14 => Errno::EMSGSIZE,
        15 => Errno::ENOTCONN,
        16 => Errno::ENODEV,
        17 => Errno::EACCES,
        18 => Errno::ETIMEDOUT,
        20 => Errno::EAGAIN,
        22 => Errno::EMFILE,
        23 => Errno::EOPNOTSUPP,
        24 => Errno::ENETUNREACH,
        25 => Errno::EHOSTUNREACH,
        _ => return Err(Errno::EPROTO),
    })
}

/// Classic resolution errors carry no trustworthy NXDOMAIN/TTL information.
pub(crate) fn lookup_error(_: Errno) -> LookupError {
    LookupError::Failed
}
