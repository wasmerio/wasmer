//! Shared backend UDP error state, following Linux net/ipv4/udp.c:__udp4_lib_err
//! (udp_err in the current tree), net/ipv6/udp.c:__udp6_lib_err and
//! net/core/skbuff.c:sock_dequeue_err_skb. Ordinary host send failures are separate.
use crate::{NetResult, QueuedError};
use std::{collections::VecDeque, net::SocketAddr};
use wasixv2_abi::{Errno, IpOption, Ipv6Option, SoEeOrigin, SocketLevel};

/// Bounded ICMP port-unreachable reporting for socket backends. Connection matching is
/// performed by the provider before reporting an error; queued errors retain their quote.
#[derive(Default)]
pub struct DatagramErrors {
    receive: [bool; 2],
    pending: Option<Errno>,
    queue: VecDeque<QueuedError>,
    bytes: usize,
}
impl DatagramErrors {
    pub fn set_option(&mut self, level: i32, name: i32, value: &[u8]) -> NetResult<()> {
        let index = if level == SocketLevel::SOL_IP.0 && name == IpOption::IP_RECVERR.0 {
            0
        } else if level == SocketLevel::SOL_IPV6.0 && name == Ipv6Option::IPV6_RECVERR.0 {
            1
        } else {
            return Err(Errno::EOPNOTSUPP);
        };
        self.receive[index] = crate::opts::read_int(value)? != 0;
        if !self.receive[index] {
            // inet{,6}_setsockopt purges the error queue when RECVERR is disabled,
            // without clearing sk_err, which may still be consumed by the next I/O.
            self.queue.clear();
            self.bytes = 0;
        }
        Ok(())
    }
    /// A hard port-unreachable error. Without RECVERR only connected sockets get sk_err;
    /// with RECVERR it is also queued, including on unconnected endpoints.
    pub fn refused(
        &mut self,
        dst: SocketAddr,
        offender: Option<SocketAddr>,
        payload: &[u8],
        connected: bool,
        limit: usize,
    ) -> bool {
        let receive = self.receive[usize::from(dst.is_ipv6())];
        if !connected && !receive {
            return false;
        }
        if receive {
            let charge = payload.len().saturating_add(768);
            if self.bytes.saturating_add(charge) <= limit {
                self.queue.push_back(QueuedError {
                    errno: Errno::ECONNREFUSED,
                    origin: if dst.is_ipv4() {
                        SoEeOrigin::SO_EE_ORIGIN_ICMP
                    } else {
                        SoEeOrigin::SO_EE_ORIGIN_ICMP6
                    },
                    type_: if dst.is_ipv4() { 3 } else { 1 },
                    code: if dst.is_ipv4() { 3 } else { 4 },
                    info: 0,
                    data: 0,
                    offender,
                    dst: Some(dst),
                    payload: payload.to_vec(),
                });
                self.bytes += charge;
            }
        }
        self.pending = Some(Errno::ECONNREFUSED);
        true
    }
    pub fn has_error(&self) -> bool {
        self.pending.is_some() || !self.queue.is_empty()
    }
    pub fn take_error(&mut self) -> Option<Errno> {
        self.pending.take()
    }
    pub fn take_queued_error(&mut self) -> Option<QueuedError> {
        let error = self.queue.pop_front()?;
        self.bytes -= error.payload.len() + 768;
        self.pending = self.queue.front().map(|e| e.errno);
        Some(error)
    }
}
