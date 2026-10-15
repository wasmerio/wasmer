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

use std::{collections::VecDeque, sync::Mutex};

use wasixv2_abi::Errno;
use wasixv2_wait::{PollMask, WaitQueue};

use crate::{NetResult, Shutdown};

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
}

#[derive(Debug)]
struct Inner {
    /// False while the connection waits in a full listener's SYN queue.
    established: bool,
    /// `rx[i]`: bytes queued for side `i` to read.
    rx: [VecDeque<u8>; 2],
    side: [Side; 2],
}

/// Buffer sizes and receive low-water mark of one side.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Buffers {
    pub sndbuf: usize,
    pub rcvbuf: usize,
    pub rcvlowat: usize,
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
            ..Side::default()
        });
        Self {
            inner: Mutex::new(Inner {
                established,
                rx: [VecDeque::new(), VecDeque::new()],
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
        g.rx[peer].len().saturating_sub(g.side[peer].rcvbuf)
    }

    fn writable(g: &Inner, side: usize) -> bool {
        let queued = Self::send_queued(g, side);
        let free = g.side[side].sndbuf.saturating_sub(queued);
        free >= queued / 2 && g.rx[1 - side].len() < Self::capacity(g, 1 - side)
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
            if g.rx[side].len() >= s.rcvlowat.max(1) {
                mask |= PollMask::IN | PollMask::RDNORM;
            }
            if snd_shut || Self::writable(&g, side) {
                mask |= PollMask::OUT | PollMask::WRNORM;
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
        if !g.rx[side].is_empty() {
            let n = buf.len().min(g.rx[side].len());
            let (a, b) = g.rx[side].as_slices();
            let first = n.min(a.len());
            buf[..first].copy_from_slice(&a[..first]);
            buf[first..n].copy_from_slice(&b[..n - first]);
            if !peek {
                g.rx[side].drain(..n);
                drop(g);
                self.wq[1 - side].wake(PollMask::OUT | PollMask::WRNORM);
            }
            return Ok(n);
        }
        if buf.is_empty() {
            return Ok(0);
        }
        // SOCK_DONE (FIN received) comes before sk_err, then RCV_SHUTDOWN.
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

    /// `tcp_sendmsg` without blocking.
    pub fn write(&self, side: usize, data: &[u8]) -> NetResult<usize> {
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
            let s = &mut g.side[side];
            s.err = Some(err);
            s.done = true;
            drop(g);
            self.wq[side].wake(ALL);
            return Ok(data.len());
        }
        if data.is_empty() {
            return Ok(0);
        }
        let space = Self::capacity(&g, peer).saturating_sub(g.rx[peer].len());
        if space == 0 {
            return Err(Errno::EAGAIN);
        }
        let n = space.min(data.len());
        g.rx[peer].extend(&data[..n]);
        drop(g);
        self.wq[peer].wake(PollMask::IN | PollMask::RDNORM);
        Ok(n)
    }

    /// `tcp_shutdown` / the `RCV_SHUTDOWN` half of `inet_shutdown`.
    pub fn shutdown(&self, side: usize, how: Shutdown) {
        let mut g = self.lock();
        let mut wake_peer = false;
        if matches!(how, Shutdown::Read | Shutdown::Both) {
            g.side[side].rd_shut = true;
        }
        if matches!(how, Shutdown::Write | Shutdown::Both) && !g.side[side].wr_shut {
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

    pub fn input_len(&self, side: usize) -> usize {
        self.lock().rx[side].len()
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
    };

    fn conn() -> Conn {
        Conn::new([WaitQueue::new(), WaitQueue::new()], [B, B], true)
    }

    #[test]
    fn partial_writes_then_eagain() {
        let c = conn();
        assert_eq!(c.write(0, &[1; 20]), Ok(16));
        assert_eq!(c.write(0, &[1]), Err(Errno::EAGAIN));
        assert!(!c.poll(0).contains(PollMask::OUT));
        let mut buf = [0; 12];
        assert_eq!(c.read(1, &mut buf, false), Ok(12));
        assert!(c.poll(0).contains(PollMask::OUT));
    }

    #[test]
    fn fin_gives_eof_after_data() {
        let c = conn();
        c.write(0, b"hi").unwrap();
        c.shutdown(0, Shutdown::Write);
        assert_eq!(c.write(0, b"x"), Err(Errno::EPIPE));
        let mask = c.poll(1);
        assert!(mask.contains(PollMask::IN | PollMask::RDHUP));
        assert!(!mask.contains(PollMask::HUP));
        let mut buf = [0; 8];
        assert_eq!(c.read(1, &mut buf, false), Ok(2));
        assert_eq!(c.read(1, &mut buf, false), Ok(0));
        // The other direction still works (half close).
        assert_eq!(c.write(1, b"back"), Ok(4));
        assert_eq!(c.read(0, &mut buf, false), Ok(4));
    }

    #[test]
    fn close_with_unread_data_resets_peer() {
        let c = conn();
        c.write(1, b"unread").unwrap();
        c.close(0, false);
        assert!(c.poll(1).contains(PollMask::ERR | PollMask::HUP));
        let mut buf = [0; 8];
        assert_eq!(c.read(1, &mut buf, false), Err(Errno::ECONNRESET));
        assert_eq!(c.read(1, &mut buf, false), Ok(0));
        assert_eq!(c.write(1, b"x"), Err(Errno::EPIPE));
    }

    #[test]
    fn writing_to_closed_peer_succeeds_once_then_epipe() {
        let c = conn();
        c.close(1, false);
        let mut buf = [0; 8];
        assert_eq!(c.read(0, &mut buf, false), Ok(0));
        assert_eq!(c.write(0, b"x"), Ok(1));
        assert_eq!(c.write(0, b"x"), Err(Errno::EPIPE));
        assert_eq!(c.write(0, b"x"), Err(Errno::EPIPE));
    }

    #[test]
    fn peek_and_lowat() {
        let c = Conn::new(
            [WaitQueue::new(), WaitQueue::new()],
            [B, Buffers { rcvlowat: 3, ..B }],
            true,
        );
        c.write(0, b"ab").unwrap();
        assert!(!c.poll(1).contains(PollMask::IN));
        c.write(0, b"c").unwrap();
        assert!(c.poll(1).contains(PollMask::IN));
        let mut buf = [0; 2];
        assert_eq!(c.read(1, &mut buf, true), Ok(2));
        assert_eq!(c.input_len(1), 3);
    }

    #[test]
    fn syn_queue_connection_is_not_writable_until_established() {
        let c = Conn::new([WaitQueue::new(), WaitQueue::new()], [B, B], false);
        assert_eq!(c.poll(0), PollMask::empty());
        assert_eq!(c.write(0, b"x"), Err(Errno::EAGAIN));
        c.establish();
        assert!(c.poll(0).contains(PollMask::OUT));
        let c = Conn::new([WaitQueue::new(), WaitQueue::new()], [B, B], false);
        c.refuse();
        assert!(c.poll(0).contains(PollMask::ERR | PollMask::HUP));
        assert_eq!(c.take_error(0), Some(Errno::ECONNREFUSED));
    }
}
