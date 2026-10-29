//! In-memory TCP connection between two sockets of one namespace.
//!
//! A `Conn` replaces everything below the socket layer of a loopback connection: the two
//! receive queues, the FIN and RST exchange and the error each side sees. Side 0 is the
//! connecting socket, side 1 the accepted one. The state each side observes follows Linux's
//! `tcp_poll`, `tcp_recvmsg`, `tcp_sendmsg`, `tcp_shutdown`, `tcp_close` and `tcp_reset`.
//!
//! Buffering: bytes written by one side wait in the other side's receive queue. The queue
//! takes up to the receiver's `SO_RCVBUF` plus the sender's `SO_SNDBUF`, standing in for the
//! receive queue plus the unacknowledged send queue of a real loopback connection; the sender
//! is writable while the free part of its own buffer is at least half of what it has queued
//! beyond the receiver's buffer (`sk_stream_wspace >= sk_stream_min_wspace`). Sizes count payload bytes only;
//! Linux also charges skb overhead.
//!
//! Urgent data goes with the bytes it ends: a write with `MSG_OOB` marks its last byte in the
//! peer's queue ([`RecvQueue`]) and wakes the peer with `PRI`, the new mark (see [`crate::api`]).

use std::sync::Mutex;

use wasixv2_abi::Errno;
use wasixv2_wait::{PollMask, WaitQueue};

use crate::{NetResult, RecvQueue, Shutdown};

#[derive(Debug, Default)]
struct Side {
    /// `RCV_SHUTDOWN` set by `shutdown(SHUT_RD)`.
    rd_shut: bool,
    /// FIN sent: `shutdown(SHUT_WR)` or `close`.
    wr_shut: bool,
    /// The socket was released (`close`); it is an orphan that answers data with RST.
    closed: bool,
    /// `tcp_done`: the connection is gone for this side (state `TCP_CLOSE`, both directions
    /// shut).
    done: bool,
    /// `sk_err`.
    err: Option<Errno>,
    rcvbuf: usize,
    sndbuf: usize,
    rcvlowat: usize,
    /// `SO_OOBINLINE`: urgent bytes stay in the stream.
    urginline: bool,
}

#[derive(Debug)]
struct Inner {
    /// False while the connection waits in a full listener's SYN queue.
    established: bool,
    /// `rx[i]`: bytes queued for side `i` to read.
    rx: [RecvQueue; 2],
    /// Partial TCP segments withheld by MSG_MORE/TCP_CORK, charged to their sender.
    tx: [std::collections::VecDeque<u8>; 2],
    side: [Side; 2],
}

/// What one side's options make of the connection: buffer sizes, the receive low-water mark,
/// and whether urgent bytes are read inline.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Buffers {
    pub sndbuf: usize,
    pub rcvbuf: usize,
    pub rcvlowat: usize,
    pub urginline: bool,
}

#[derive(Debug)]
pub(crate) struct Conn {
    inner: Mutex<Inner>,
    wq: [WaitQueue; 2],
}

const ALL: PollMask = PollMask::empty();

impl Conn {
    pub fn new(wq: [WaitQueue; 2], bufs: [Buffers; 2], established: bool) -> Self {
        let side = bufs.map(|b| Side {
            rcvbuf: b.rcvbuf,
            sndbuf: b.sndbuf,
            rcvlowat: b.rcvlowat,
            urginline: b.urginline,
            ..Side::default()
        });
        Self {
            inner: Mutex::new(Inner {
                established,
                rx: Default::default(),
                tx: Default::default(),
                side,
            }),
            wq,
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap()
    }

    /// Moves a connection out of the SYN queue: the connecting side becomes writable.
    pub fn establish(&self) {
        self.lock().established = true;
        self.wq[0].wake(PollMask::OUT | PollMask::WRNORM);
    }

    pub fn is_established(&self) -> bool {
        self.lock().established
    }

    /// Whether `side` is finished with the connection (reset, refused, or aborted).
    pub fn is_done(&self, side: usize) -> bool {
        self.lock().side[side].done
    }

    pub fn is_closed(&self, side: usize) -> bool {
        self.lock().side[side].closed
    }

    /// A connection that never got out of the SYN queue is refused: the client's next SYN
    /// meets a closed port and `tcp_reset` in `SYN_SENT` reports `ECONNREFUSED`.
    pub fn refuse(&self) {
        let mut g = self.lock();
        let s = &mut g.side[0];
        if !s.done {
            s.done = true;
            s.err = Some(Errno::ECONNREFUSED);
        }
        drop(g);
        self.wq[0].wake(ALL);
    }

    pub fn set_buffers(&self, side: usize, b: Buffers) {
        let mut g = self.lock();
        let s = &mut g.side[side];
        s.sndbuf = b.sndbuf;
        s.rcvbuf = b.rcvbuf;
        s.rcvlowat = b.rcvlowat;
        s.urginline = b.urginline;
        drop(g);
        self.wq[0].wake(ALL);
        self.wq[1].wake(ALL);
    }

    fn capacity(g: &Inner, to: usize) -> usize {
        g.side[to].rcvbuf + g.side[1 - to].sndbuf
    }

    /// Bytes `side` sent that are still beyond the peer's receive buffer (`SIOCOUTQ`).
    fn send_queued(g: &Inner, side: usize) -> usize {
        let peer = 1 - side;
        g.tx[side].len() + g.rx[peer].len().saturating_sub(g.side[peer].rcvbuf)
    }

    fn writable(g: &Inner, side: usize) -> bool {
        let queued = Self::send_queued(g, side);
        let free = g.side[side].sndbuf.saturating_sub(queued);
        free >= queued / 2 && g.rx[1 - side].len() + g.tx[side].len() < Self::capacity(g, 1 - side)
    }

    /// `tcp_poll` for a socket whose connection this is.
    pub fn poll(&self, side: usize) -> PollMask {
        let g = self.lock();
        let s = &g.side[side];
        let peer = &g.side[1 - side];
        let rcv_shut = s.rd_shut || s.done || (g.established && peer.wr_shut);
        let snd_shut = s.wr_shut || s.done;
        let mut mask = PollMask::empty();
        if (rcv_shut && snd_shut) || s.done {
            mask |= PollMask::HUP;
        }
        if rcv_shut {
            mask |= PollMask::IN | PollMask::RDNORM | PollMask::RDHUP;
        }
        if g.established || s.done {
            if g.rx[side].readable(s.rcvlowat, s.urginline) {
                mask |= PollMask::IN | PollMask::RDNORM;
            }
            if snd_shut || Self::writable(&g, side) {
                mask |= PollMask::OUT | PollMask::WRNORM;
            }
            if g.rx[side].urgent() {
                mask |= PollMask::PRI;
            }
        }
        if s.err.is_some() {
            mask |= PollMask::ERR;
        }
        mask
    }

    /// `tcp_recvmsg` without blocking.
    pub fn read(&self, side: usize, buf: &mut [u8], peek: bool) -> NetResult<usize> {
        let mut g = self.lock();
        let inline = g.side[side].urginline;
        let queued = g.rx[side].len();
        let n = g.rx[side].read(buf, peek, inline);
        let took = g.rx[side].len() < queued;
        let result = match n > 0 || buf.is_empty() {
            true => Ok(n),
            false => Self::nothing_read(&mut g, side),
        };
        drop(g);
        if took {
            self.wq[1 - side].wake(PollMask::OUT | PollMask::WRNORM);
        }
        result
    }

    /// What a read that found no data returns: SOCK_DONE (FIN received) comes before sk_err,
    /// then RCV_SHUTDOWN.
    fn nothing_read(g: &mut Inner, side: usize) -> NetResult<usize> {
        let established = g.established;
        let (s, peer) = split(&mut g.side, side);
        if established && peer.wr_shut {
            return Ok(0);
        }
        if let Some(err) = s.err.take() {
            return Err(err);
        }
        if s.rd_shut || s.done {
            return Ok(0);
        }
        Err(Errno::EAGAIN)
    }

    /// `tcp_sendmsg` without blocking; with `urgent` (`MSG_OOB`) the last byte taken is urgent.
    #[cfg(test)]
    pub fn write(&self, side: usize, data: &[u8], urgent: bool) -> NetResult<usize> {
        self.write_more(side, data, urgent, false, 1)
    }

    fn push_locked(g: &mut Inner, side: usize, count: usize) -> bool {
        if count == 0 {
            return false;
        }
        let data: Vec<_> = g.tx[side].drain(..count).collect();
        g.rx[1 - side].push(&data);
        true
    }

    pub fn push(&self, side: usize) {
        let mut g = self.lock();
        let count = g.tx[side].len();
        let peer = 1 - side;
        if count != 0 && (g.side[peer].closed || g.side[peer].done) {
            // tcp_rcv_state_process: the orphan resets data sent after its FIN.
            g.tx[side].clear();
            let err = if g.side[peer].wr_shut {
                Errno::EPIPE
            } else {
                Errno::ECONNRESET
            };
            if !g.side[side].done && g.side[side].err.is_none() {
                g.side[side].err = Some(err);
            }
            g.side[side].done = true;
            drop(g);
            self.wq[side].wake(ALL);
            return;
        }
        let pushed = Self::push_locked(&mut g, side, count);
        drop(g);
        if pushed {
            self.wq[1 - side].wake(PollMask::IN | PollMask::RDNORM);
        }
    }

    pub fn corked_len(&self, side: usize) -> usize {
        self.lock().tx[side].len()
    }

    /// net/ipv4/tcp.c tcp_push: MSG_MORE corks only the final partial segment.
    pub fn write_more(
        &self,
        side: usize,
        data: &[u8],
        urgent: bool,
        more: bool,
        mss: usize,
    ) -> NetResult<usize> {
        let mut g = self.lock();
        let established = g.established;
        {
            let s = &mut g.side[side];
            if s.err.is_some() || s.wr_shut || s.done {
                // `sk_stream_error`: the pending error, else EPIPE.
                return Err(s.err.take().unwrap_or(Errno::EPIPE));
            }
        }
        if !established {
            return Err(Errno::EAGAIN);
        }
        let peer = 1 - side;
        if g.side[peer].closed || g.side[peer].done {
            // The orphaned peer answers data with RST (`tcp_rcv_state_process` in FIN_WAIT
            // with RCV_SHUTDOWN). We had its FIN, so we are in CLOSE_WAIT: EPIPE.
            let err = if g.side[peer].wr_shut {
                Errno::EPIPE
            } else {
                Errno::ECONNRESET
            };
            g.tx[side].clear();
            let s = &mut g.side[side];
            s.err = Some(err);
            s.done = true;
            drop(g);
            self.wq[side].wake(ALL);
            return Ok(data.len());
        }
        if data.is_empty() {
            let count = if more { 0 } else { g.tx[side].len() };
            let pushed = Self::push_locked(&mut g, side, count);
            drop(g);
            if pushed {
                self.wq[peer].wake(PollMask::IN | PollMask::RDNORM);
            }
            return Ok(0);
        }
        let space = Self::capacity(&g, peer).saturating_sub(g.rx[peer].len() + g.tx[side].len());
        if space == 0 {
            return Err(Errno::EAGAIN);
        }
        let n = space.min(data.len());
        let mut wake = PollMask::empty();
        match urgent {
            true => {
                let count = g.tx[side].len();
                Self::push_locked(&mut g, side, count);
                let inline = g.side[peer].urginline;
                wake |= PollMask::IN | PollMask::RDNORM;
                g.rx[peer].push_urgent(&data[..n], inline);
                wake |= PollMask::PRI | PollMask::RDBAND;
            }
            false => {
                g.tx[side].extend(&data[..n]);
                let len = g.tx[side].len();
                let count = if more {
                    len / mss.max(1) * mss.max(1)
                } else {
                    len
                };
                if Self::push_locked(&mut g, side, count) {
                    wake |= PollMask::IN | PollMask::RDNORM;
                }
            }
        }
        drop(g);
        if !wake.is_empty() {
            self.wq[peer].wake(wake);
        }
        Ok(n)
    }

    /// `tcp_recv_urg` without blocking: the urgent byte for `side`.
    pub fn read_urgent(&self, side: usize, peek: bool) -> NetResult<u8> {
        let mut g = self.lock();
        // `TCP_CLOSE` without `SOCK_DONE`: the connection ended before the peer's FIN.
        let fin = g.established && g.side[1 - side].wr_shut;
        let closed = g.side[side].done && !fin;
        let inline = g.side[side].urginline;
        g.rx[side].recv_urgent(peek, inline, closed)
    }

    /// `SIOCATMARK`.
    pub fn at_mark(&self, side: usize) -> bool {
        self.lock().rx[side].at_mark()
    }

    /// `tcp_shutdown` / the `RCV_SHUTDOWN` half of `inet_shutdown`.
    pub fn shutdown(&self, side: usize, how: Shutdown) {
        let mut g = self.lock();
        let mut wake_peer = false;
        if matches!(how, Shutdown::Read | Shutdown::Both) {
            g.side[side].rd_shut = true;
        }
        if matches!(how, Shutdown::Write | Shutdown::Both) && !g.side[side].wr_shut {
            let count = g.tx[side].len();
            Self::push_locked(&mut g, side, count);
            g.side[side].wr_shut = true;
            wake_peer = true;
        }
        drop(g);
        self.wq[side].wake(ALL);
        if wake_peer {
            self.wq[1 - side].wake(PollMask::IN | PollMask::RDNORM | PollMask::RDHUP);
        }
    }

    /// `tcp_close`. Unread data or `abort` (`SO_LINGER` with zero timeout, `tcp_disconnect`)
    /// resets the peer instead of sending FIN.
    pub fn close(&self, side: usize, abort: bool) {
        let mut g = self.lock();
        let unread = !g.rx[side].is_empty();
        if abort || unread {
            g.tx[side].clear();
        } else {
            let count = g.tx[side].len();
            Self::push_locked(&mut g, side, count);
        }
        g.rx[side].clear();
        let peer = 1 - side;
        let established = g.established;
        let (s, p) = split(&mut g.side, side);
        s.closed = true;
        s.rd_shut = true;
        if !s.done && !p.done {
            if (abort || unread) && established {
                // `tcp_reset` on the peer: EPIPE in CLOSE_WAIT (it had our FIN), else
                // ECONNRESET.
                p.err = Some(if s.wr_shut {
                    Errno::EPIPE
                } else {
                    Errno::ECONNRESET
                });
                p.done = true;
            } else {
                s.wr_shut = true;
            }
        }
        s.done = true;
        drop(g);
        self.wq[peer].wake(ALL);
    }

    pub fn take_error(&self, side: usize) -> Option<Errno> {
        self.lock().side[side].err.take()
    }

    /// `SIOCINQ`.
    pub fn input_len(&self, side: usize) -> usize {
        let g = self.lock();
        g.rx[side].inq(g.side[side].urginline)
    }

    pub fn output_len(&self, side: usize) -> usize {
        Self::send_queued(&self.lock(), side)
    }
}

fn split(sides: &mut [Side; 2], side: usize) -> (&mut Side, &mut Side) {
    let (a, b) = sides.split_at_mut(1);
    if side == 0 {
        (&mut a[0], &mut b[0])
    } else {
        (&mut b[0], &mut a[0])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const B: Buffers = Buffers {
        sndbuf: 8,
        rcvbuf: 8,
        rcvlowat: 1,
        urginline: false,
    };

    impl Conn {
        fn write0(&self, data: &[u8]) -> NetResult<usize> {
            self.write(0, data, false)
        }

        fn write1(&self, data: &[u8]) -> NetResult<usize> {
            self.write(1, data, false)
        }
    }

    fn conn() -> Conn {
        Conn::new([WaitQueue::new(), WaitQueue::new()], [B, B], true)
    }

    #[test]
    fn partial_writes_then_eagain() {
        let c = conn();
        assert_eq!(c.write0(&[1; 20]), Ok(16));
        assert_eq!(c.write0(&[1]), Err(Errno::EAGAIN));
        assert!(!c.poll(0).contains(PollMask::OUT));
        let mut buf = [0; 12];
        assert_eq!(c.read(1, &mut buf, false), Ok(12));
        assert!(c.poll(0).contains(PollMask::OUT));
    }

    #[test]
    fn fin_gives_eof_after_data() {
        let c = conn();
        c.write0(b"hi").unwrap();
        c.shutdown(0, Shutdown::Write);
        assert_eq!(c.write0(b"x"), Err(Errno::EPIPE));
        let mask = c.poll(1);
        assert!(mask.contains(PollMask::IN | PollMask::RDHUP));
        assert!(!mask.contains(PollMask::HUP));
        let mut buf = [0; 8];
        assert_eq!(c.read(1, &mut buf, false), Ok(2));
        assert_eq!(c.read(1, &mut buf, false), Ok(0));
        // The other direction still works (half close).
        assert_eq!(c.write1(b"back"), Ok(4));
        assert_eq!(c.read(0, &mut buf, false), Ok(4));
    }

    #[test]
    fn close_with_unread_data_resets_peer() {
        let c = conn();
        c.write1(b"unread").unwrap();
        c.close(0, false);
        assert!(c.poll(1).contains(PollMask::ERR | PollMask::HUP));
        let mut buf = [0; 8];
        assert_eq!(c.read(1, &mut buf, false), Err(Errno::ECONNRESET));
        assert_eq!(c.read(1, &mut buf, false), Ok(0));
        assert_eq!(c.write1(b"x"), Err(Errno::EPIPE));
    }

    #[test]
    fn writing_to_closed_peer_succeeds_once_then_epipe() {
        let c = conn();
        c.close(1, false);
        let mut buf = [0; 8];
        assert_eq!(c.read(0, &mut buf, false), Ok(0));
        assert_eq!(c.write0(b"x"), Ok(1));
        assert_eq!(c.write0(b"x"), Err(Errno::EPIPE));
        assert_eq!(c.write0(b"x"), Err(Errno::EPIPE));
    }

    #[test]
    fn peek_and_lowat() {
        let c = Conn::new(
            [WaitQueue::new(), WaitQueue::new()],
            [B, Buffers { rcvlowat: 3, ..B }],
            true,
        );
        c.write0(b"ab").unwrap();
        assert!(!c.poll(1).contains(PollMask::IN));
        c.write0(b"c").unwrap();
        assert!(c.poll(1).contains(PollMask::IN));
        let mut buf = [0; 2];
        assert_eq!(c.read(1, &mut buf, true), Ok(2));
        assert_eq!(c.input_len(1), 3);
    }

    #[test]
    fn syn_queue_connection_is_not_writable_until_established() {
        let c = Conn::new([WaitQueue::new(), WaitQueue::new()], [B, B], false);
        assert_eq!(c.poll(0), PollMask::empty());
        assert_eq!(c.write0(b"x"), Err(Errno::EAGAIN));
        c.establish();
        assert!(c.poll(0).contains(PollMask::OUT));
        let c = Conn::new([WaitQueue::new(), WaitQueue::new()], [B, B], false);
        c.refuse();
        assert!(c.poll(0).contains(PollMask::ERR | PollMask::HUP));
        assert_eq!(c.take_error(0), Some(Errno::ECONNREFUSED));
    }
}
